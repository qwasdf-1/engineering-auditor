use std::path::{Path, PathBuf};

use auditor_report::parse_repository_map_reference;
use auditor_review::{
    discover_provider, parse_disposition_ledger, parse_engineering_review_v2,
    plan_engineering_review, run_case_engineering_review, validate_disposition_coverage,
    verify_case_engineering_review, DispositionLedger, DispositionStatus, FinalCaseState,
    HostedConsent, HttpTransport, RecommendationDisposition, ReviewPlanOutput, ReviewPlanRequest,
    ReviewRoutesV2, ReviewRunRequest, ReviewVerifyRequest, StableId, UncontrolledReview,
};
use serde_json::json;

use crate::arg_cursor::ArgCursor;

pub(crate) fn run_review(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("plan") => run_review_plan(&args[1..]),
        Some("inspect-case") => run_review_inspect_case(&args[1..]),
        Some("transcript") => run_review_transcript(&args[1..]),
        Some("run") => run_engineering_review(&args[1..]),
        Some("provider") => run_provider_command(&args[1..]),
        Some("disposition") => run_disposition(&args[1..]),
        Some("gate") => run_review_gate(&args[1..]),
        Some("verify") => run_review_verify(&args[1..]),
        Some(other) => Err(format!("unknown review command `{other}`")),
        None => Err(
            "review requires plan, inspect-case, transcript, run, provider, disposition, gate, or verify"
                .to_string(),
        ),
    }
}

fn run_review_transcript(args: &[String]) -> Result<String, String> {
    let mut input = None;
    let mut output = None;
    let mut model_input_only = false;
    let mut stage_prefixes = Vec::new();
    consume_arguments(args, |argument, cursor| {
        match argument {
            "--input" => input = Some(PathBuf::from(cursor.required(argument)?)),
            "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
            "--model-input-only" => model_input_only = true,
            "--stage-prefix" => stage_prefixes.push(cursor.required(argument)?.to_string()),
            other => return Err(format!("unknown review transcript argument `{other}`")),
        }
        Ok(())
    })?;
    let input = input.ok_or_else(|| "review transcript requires --input".to_string())?;
    let source = std::fs::read_to_string(&input)
        .map_err(|error| format!("failed to read transcript {}: {error}", input.display()))?;
    let rendered = auditor_review::render_transcript_markdown(
        &source,
        &auditor_review::TranscriptMarkdownOptions {
            source: input.display().to_string(),
            model_input_only,
            stage_prefixes,
        },
    )?;
    if let Some(output) = output {
        auditor_scan::atomic_write(&output, &rendered)?;
        return Ok(String::new());
    }
    Ok(rendered)
}

fn consume_arguments(
    args: &[String],
    mut consume: impl FnMut(&str, &mut ArgCursor<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let mut cursor = ArgCursor::new(args);
    while let Some(argument) = cursor.next() {
        consume(argument, &mut cursor)?;
    }
    Ok(())
}

struct PlanOptions {
    request: ReviewPlanRequest,
    output: PathBuf,
}

#[derive(Default)]
struct ReviewInputOptions {
    report: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    case_templates: Option<PathBuf>,
    routes: Option<PathBuf>,
    prompt_bundle: Option<PathBuf>,
    brief: Option<PathBuf>,
    evidence: Vec<PathBuf>,
}

impl ReviewInputOptions {
    fn consume(&mut self, argument: &str, cursor: &mut ArgCursor<'_>) -> Result<bool, String> {
        let value = match argument {
            "--report" | "--repo-root" | "--case-templates" | "--routes" | "--prompt-bundle"
            | "--brief" | "--evidence" => PathBuf::from(cursor.required(argument)?),
            _ => return Ok(false),
        };
        match argument {
            "--report" => self.report = Some(value),
            "--repo-root" => self.repo_root = Some(value),
            "--case-templates" => self.case_templates = Some(value),
            "--routes" => self.routes = Some(value),
            "--prompt-bundle" => self.prompt_bundle = Some(value),
            "--brief" => self.brief = Some(value),
            "--evidence" => self.evidence.push(value),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn into_request(self, command: &str) -> Result<ReviewPlanRequest, String> {
        let required = |value: Option<PathBuf>, name: &str| {
            value.ok_or_else(|| format!("{command} requires --{name}"))
        };
        Ok(ReviewPlanRequest {
            report: required(self.report, "report")?,
            repo_root: required(self.repo_root, "repo-root")?,
            case_templates: required(self.case_templates, "case-templates")?,
            routes: required(self.routes, "routes")?,
            prompt_bundle: required(self.prompt_bundle, "prompt-bundle")?,
            brief: self.brief,
            evidence: self.evidence,
        })
    }
}

impl PlanOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let (request, output, _) = parse_review_request(args, "review plan", false)?;
        Ok(Self { request, output })
    }
}

fn parse_review_request(
    args: &[String],
    command: &str,
    allow_consent: bool,
) -> Result<(ReviewPlanRequest, PathBuf, HostedConsent), String> {
    let mut input = ReviewInputOptions::default();
    let mut output = None;
    let mut consent = HostedConsent::default();
    consume_arguments(args, |argument, cursor| {
        if allow_consent && apply_consent(argument, &mut consent) {
            return Ok(());
        }
        if input.consume(argument, cursor)? {
            return Ok(());
        }
        if argument == "--output" {
            output = Some(PathBuf::from(cursor.required(argument)?));
            return Ok(());
        }
        Err(format!("unknown {command} argument `{argument}`"))
    })?;
    let request = input.into_request(command)?;
    let output = output.ok_or_else(|| format!("{command} requires --output"))?;
    Ok((request, output, consent))
}

fn run_review_plan(args: &[String]) -> Result<String, String> {
    let options = PlanOptions::parse(args)?;
    if options.output == options.request.report {
        return Err("review plan output must differ from its input report".to_string());
    }
    let plan = plan_engineering_review(&options.request).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(&plan)
        .map_err(|error| format!("failed to serialize review plan: {error}"))?;
    auditor_scan::atomic_write(&options.output, &text)?;
    Ok(text)
}

#[derive(Clone, Copy)]
enum InspectFormat {
    Json,
    Human,
}

struct InspectOptions {
    plan: PathBuf,
    case_id: StableId,
    format: InspectFormat,
    output: Option<PathBuf>,
}

impl InspectOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut plan = None;
        let mut case_id = None;
        let mut format = InspectFormat::Human;
        let mut output = None;
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            match argument {
                "--plan" => plan = Some(PathBuf::from(cursor.required(argument)?)),
                "--case-id" => {
                    case_id = Some(parse_stable_id(&mut cursor, argument)?);
                }
                "--format" => {
                    format = match cursor.required(argument)? {
                        "json" => InspectFormat::Json,
                        "human" => InspectFormat::Human,
                        _ => return Err("inspect-case --format requires json|human".to_string()),
                    }
                }
                "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
                other => return Err(format!("unknown review inspect-case argument `{other}`")),
            }
        }
        Ok(Self {
            plan: plan.ok_or_else(|| "review inspect-case requires --plan".to_string())?,
            case_id: case_id.ok_or_else(|| "review inspect-case requires --case-id".to_string())?,
            format,
            output,
        })
    }
}

fn parse_stable_id(cursor: &mut ArgCursor<'_>, argument: &str) -> Result<StableId, String> {
    StableId::parse(cursor.required(argument)?.to_string()).map_err(|error| error.to_string())
}

fn run_review_inspect_case(args: &[String]) -> Result<String, String> {
    let options = InspectOptions::parse(args)?;
    let plan = parse_file(&options.plan, "review plan", ReviewPlanOutput::parse)?;
    let inspection = auditor_review::inspect_engineering_case(&plan, &options.case_id)
        .map_err(|error| error.to_string())?;
    let rendered = match options.format {
        InspectFormat::Json => serde_json::to_string_pretty(&inspection)
            .map_err(|error| format!("failed to serialize case inspection: {error}")),
        InspectFormat::Human => {
            auditor_review::render_engineering_case_context(&plan, &options.case_id)
                .map(|projection| projection.markdown)
                .map_err(|error| error.to_string())
        }
    }?;
    if let Some(output) = &options.output {
        auditor_scan::atomic_write(output, &rendered)?;
    }
    Ok(rendered)
}

pub(crate) fn run_scan_review(args: &[String]) -> Result<String, String> {
    run_scan_review_with_transport(args, &HttpTransport::default())
}

pub(crate) fn run_scan_review_with_transport(
    args: &[String],
    transport: &dyn auditor_review::Transport,
) -> Result<String, String> {
    let options = ScanReviewOptions::parse(args)?;
    let existing_artifacts = auditor_scan::sibling_names(Some(&options.output));
    let scan_input = temporary_scan_input(&options.output);
    let scan_report = crate::scan_command::run_scan_with_overrides(
        &options.scan_args,
        Some(scan_input.clone()),
        Some(options.output.clone()),
        std::slice::from_ref(&options.output),
    )?;
    let map_path = repository_map_path(&scan_report, &scan_input);
    let review_args = options.review_args(&scan_input);
    // Stage-progress stderr: emit `auditor: <stage>` on each review checkpoint
    // unless --quiet is set. The interpreter-controlled path (review run/)
    // continues to use UncontrolledReview and is unaffected here.
    let stage_progress = StderrStageProgress::new(options.quiet);
    let reviewed = match run_engineering_review_with_transport_and_control(
        &review_args,
        transport,
        &stage_progress,
    ) {
        Ok(reviewed) => reviewed,
        Err(error) => {
            let removable_map = map_path.as_deref().filter(|path| {
                path.file_name()
                    .is_some_and(|name| !existing_artifacts.contains(name))
            });
            cleanup_scan_input(&scan_input, removable_map);
            return Err(error);
        }
    };
    std::fs::remove_file(&scan_input)
        .map_err(|error| format!("failed to remove intermediate scan report: {error}"))?;
    // Post-review human digest for `--format human`. The reviewed JSON lives
    // at `options.output`; render the digest next to it at `<output>.md`.
    if options.human_format {
        let digest_path = digest_sibling_for(&options.output);
        render_reviewed_digest(&options.output, &digest_path, &options)?;
    }
    Ok(reviewed)
}

/// Companion human digest path for a reviewed JSON report. Mirrors the
/// Task 7 sibling convention: `audit.json` -> `audit.md`.
fn digest_sibling_for(reviewed_json: &std::path::Path) -> PathBuf {
    reviewed_json.with_extension("md")
}

/// Render the digest of an already-written reviewed JSON report. Parses via
/// `auditor_report::parse_report_for_digest`, computes a footer cross-
/// referencing the reviewed JSON by basename and SHA-256, and atomically
/// writes the digest to `digest_path`. Honors `--no-color` (file destinations
/// are not ttys, so color is Plain in practice) and `--summary-only`.
fn render_reviewed_digest(
    reviewed_json: &std::path::Path,
    digest_path: &std::path::Path,
    options: &ScanReviewOptions,
) -> Result<(), String> {
    let machine_text = std::fs::read_to_string(reviewed_json).map_err(|error| {
        format!(
            "failed to read reviewed report {}: {error}",
            reviewed_json.display()
        )
    })?;
    let digest = crate::digest_command::render_machine_digest(
        &machine_text,
        reviewed_json,
        auditor_report::ColorMode::Plain,
        options.summary_only,
    )?;
    auditor_scan::atomic_write(digest_path, &digest)
}

/// `ReviewControl` adapter that mirrors review-pipeline stage transitions
/// to stderr as `auditor: <stage>` lines. Used by `scan --review` to give
/// an operator a visible progress signal while the (potentially slow)
/// engineering review runs. Silent under `--quiet`.
struct StderrStageProgress {
    quiet: bool,
}

impl StderrStageProgress {
    fn new(quiet: bool) -> Self {
        Self { quiet }
    }
}

impl auditor_review::ReviewControl for StderrStageProgress {
    fn checkpoint(&self, stage: auditor_review::ReviewStage) -> bool {
        if !self.quiet {
            eprintln!("auditor: {}", stage.as_str());
        }
        true
    }
}

/// Run the engineering review with a caller-supplied `ReviewControl`. The
fn run_engineering_review_with_transport_and_control(
    args: &[String],
    transport: &dyn auditor_review::Transport,
    control: &dyn auditor_review::ReviewControl,
) -> Result<String, String> {
    let options = ReviewOptions::parse(args)?;
    run_case_engineering_review(
        &ReviewRunRequest {
            plan: options.plan,
            output: options.output,
            consent: options.consent,
        },
        transport,
        control,
    )
    .map(|result| result.reviewed_report)
    .map_err(|error| error.to_string())
}

struct ScanReviewOptions {
    scan_args: Vec<String>,
    case_templates: String,
    routes: String,
    prompt_bundle: String,
    brief: Option<String>,
    evidence: Vec<String>,
    output: PathBuf,
    repo_root: PathBuf,
    consent: HostedConsent,
    /// `--format human`: after the engineering review publishes the reviewed
    /// JSON to `output`, parse it via `auditor_report::parse_report_for_digest`
    /// and write a human digest next to it at `<output>.md`. The inner scan
    /// is always JSON (the reviewer requires JSON input); `human_format`
    /// only toggles the post-review digest rendering step.
    human_format: bool,
    /// `--quiet`: suppresses stdout AND suppresses stage-progress stderr
    /// lines that the review pipeline emits for format=human by default.
    quiet: bool,
    /// `--no-color`: forces `ColorMode::Plain` for the post-review digest.
    /// `--summary-only`: restricts the post-review digest to sections 1-3.
    summary_only: bool,
}

#[derive(Default)]
struct ScanReviewParser {
    scan_args: Vec<String>,
    case_templates: Option<String>,
    routes: Option<String>,
    prompt_bundle: Option<String>,
    brief: Option<String>,
    evidence: Vec<String>,
    output: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    profile_seen: bool,
    format_seen: bool,
    review_marker: bool,
    consent: HostedConsent,
    human_format: bool,
    quiet: bool,
    summary_only: bool,
}

impl ScanReviewOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut parser = ScanReviewParser::default();
        let mut index = 0;
        while index < args.len() {
            let argument = args[index].as_str();
            if apply_consent(argument, &mut parser.consent) {
                index += 1;
                continue;
            }
            match argument {
                "--review" => parser.review_marker = true,
                "--quiet" => parser.quiet = true,
                "--no-color" => {}
                "--summary-only" => parser.summary_only = true,
                "--case-templates" | "--routes" | "--prompt-bundle" | "--brief" | "--evidence" => {
                    let value = required_indexed_value(args, index, argument)?;
                    parser.set_review_value(argument, value)?;
                    index += 1;
                }
                "--profile" | "--format" | "--method" | "--output" | "--repo-root" => {
                    let value = required_indexed_value(args, index, argument)?;
                    parser.set_scan_value(argument, value)?;
                    index += 1;
                }
                "--path" | "--changed-ranges-json" => {
                    return Err("scan --review requires a full repository scope".to_string())
                }
                _ => parser.scan_args.push(args[index].clone()),
            }
            index += 1;
        }
        parser.finish()
    }

    fn review_args(&self, scan_input: &Path) -> Vec<String> {
        let mut args = vec![
            "--report".to_string(),
            scan_input.display().to_string(),
            "--repo-root".to_string(),
            self.repo_root.display().to_string(),
            "--output".to_string(),
            self.output.display().to_string(),
            "--case-templates".to_string(),
            self.case_templates.clone(),
            "--routes".to_string(),
            self.routes.clone(),
            "--prompt-bundle".to_string(),
            self.prompt_bundle.clone(),
        ];
        if let Some(brief) = &self.brief {
            args.extend(["--brief".to_string(), brief.clone()]);
        }
        for evidence in &self.evidence {
            args.extend(["--evidence".to_string(), evidence.clone()]);
        }
        if self.consent.allow_hosted {
            args.push("--allow-hosted-review".to_string());
        }
        if self.consent.allow_anonymous {
            args.push("--allow-anonymous-provider".to_string());
        }
        args
    }
}

impl ScanReviewParser {
    fn set_review_value(&mut self, argument: &str, value: String) -> Result<(), String> {
        match argument {
            "--case-templates" => self.case_templates = Some(value),
            "--routes" => self.routes = Some(value),
            "--prompt-bundle" => self.prompt_bundle = Some(value),
            "--brief" => self.brief = Some(value),
            "--evidence" => self.evidence.push(value),
            _ => return Err(format!("unknown scan --review argument `{argument}`")),
        }
        Ok(())
    }

    fn set_scan_value(&mut self, argument: &str, value: String) -> Result<(), String> {
        match argument {
            "--profile" => {
                self.profile_seen = true;
                require_value(&value, "full", "scan --review requires profile full")?;
            }
            "--format" => {
                self.format_seen = true;
                match value.as_str() {
                    "json" => {
                        // Inner scan stays JSON; reviewer requires JSON input.
                    }
                    "human" => {
                        // --format human means "after the review publishes the
                        // reviewed JSON, render a human digest next to it".
                        // The inner scan itself remains JSON.
                        self.human_format = true;
                    }
                    _ => {
                        return Err(
                            "scan --review requires --format json or --format human".to_string()
                        );
                    }
                }
                // Push the inner-scan --format as json always; the human
                // rendering happens as a post-review step.
                self.scan_args.push(argument.to_string());
                self.scan_args.push("json".to_string());
                return Ok(());
            }
            "--method" if value == "token" => {
                return Err("scan --review requires an AST-capable analysis method".to_string())
            }
            "--output" => self.output = Some(PathBuf::from(&value)),
            "--repo-root" => self.repo_root = Some(PathBuf::from(&value)),
            _ => {}
        }
        self.scan_args.push(argument.to_string());
        self.scan_args.push(value);
        Ok(())
    }

    fn finish(mut self) -> Result<ScanReviewOptions, String> {
        if !self.review_marker {
            return Err("scan review orchestration requires --review".to_string());
        }
        if !self.profile_seen {
            self.scan_args
                .extend(["--profile".to_string(), "full".to_string()]);
        }
        if !self.format_seen {
            self.scan_args
                .extend(["--format".to_string(), "json".to_string()]);
        }
        if self.summary_only && !self.human_format {
            return Err("--summary-only requires --format human".to_string());
        }
        Ok(ScanReviewOptions {
            scan_args: self.scan_args,
            case_templates: self
                .case_templates
                .ok_or_else(|| "scan --review requires --case-templates".to_string())?,
            routes: self
                .routes
                .ok_or_else(|| "scan --review requires --routes".to_string())?,
            prompt_bundle: self
                .prompt_bundle
                .ok_or_else(|| "scan --review requires --prompt-bundle".to_string())?,
            brief: self.brief,
            evidence: self.evidence,
            output: self
                .output
                .ok_or_else(|| "scan --review requires --output".to_string())?,
            repo_root: self.repo_root.unwrap_or_else(|| PathBuf::from(".")),
            consent: self.consent,
            human_format: self.human_format,
            quiet: self.quiet,
            summary_only: self.summary_only,
        })
    }
}

fn required_indexed_value(args: &[String], index: usize, argument: &str) -> Result<String, String> {
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("{argument} requires a value"))
}

fn require_value(value: &str, expected: &str, message: &str) -> Result<(), String> {
    (value == expected)
        .then_some(())
        .ok_or_else(|| message.to_string())
}

fn apply_consent(argument: &str, consent: &mut HostedConsent) -> bool {
    match argument {
        "--allow-hosted-review" | "--allow-hosted" => consent.allow_hosted = true,
        "--allow-anonymous-provider" | "--allow-anonymous" => consent.allow_anonymous = true,
        _ => return false,
    }
    true
}

fn parse_file<T, E>(
    path: &Path,
    label: &str,
    parser: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, String>
where
    E: std::fmt::Display,
{
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {label} {}: {error}", path.display()))?;
    parser(&contents).map_err(|error| error.to_string())
}

fn temporary_scan_input(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("audit");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("json");
    output.with_file_name(format!(
        "{stem}.scan-input-{}.{extension}",
        std::process::id()
    ))
}

fn repository_map_path(report: &str, report_path: &Path) -> Option<PathBuf> {
    let reference = parse_repository_map_reference(report).ok()?;
    let relative = reference.path?;
    let path = Path::new(&relative);
    (path.components().count() == 1
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))))
    .then(|| {
        report_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    })
}

fn cleanup_scan_input(report: &Path, map: Option<&Path>) {
    let _ = std::fs::remove_file(report);
    if let Some(map) = map {
        let _ = std::fs::remove_file(map);
    }
}

struct ReviewOptions {
    plan: ReviewPlanRequest,
    output: PathBuf,
    consent: HostedConsent,
}

impl ReviewOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let (plan, output, consent) = parse_review_request(args, "review run", true)?;
        Ok(Self {
            plan,
            output,
            consent,
        })
    }
}

fn run_engineering_review(args: &[String]) -> Result<String, String> {
    run_engineering_review_with_transport(args, &HttpTransport::default())
}

fn run_engineering_review_with_transport(
    args: &[String],
    transport: &dyn auditor_review::Transport,
) -> Result<String, String> {
    run_engineering_review_with_transport_and_control(args, transport, &UncontrolledReview)
}

fn run_provider_command(args: &[String]) -> Result<String, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("review provider requires discover".to_string());
    };
    let options = ProviderOperationOptions::parse(&args[1..])?;
    let transport = HttpTransport::default();
    let value = match command {
        "discover" => json!({
            "manifests": options.routes.providers.iter().map(|provider| provider.manifest()).collect::<Vec<_>>(),
            "providers": options.routes.providers.iter().map(|provider| discover_provider(
                provider,
                &transport,
                options.consent,
                options.routes.review_budget.timeout_ms,
            )).collect::<Vec<_>>(),
            "schema_version": 2,
        }),
        other => return Err(format!("unknown review provider command `{other}`")),
    };
    let output = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("failed to serialize provider artifact: {error}"))?;
    if let Some(path) = options.output {
        auditor_scan::atomic_write(&path, &output)?;
    }
    Ok(output)
}

struct ProviderOperationOptions {
    routes: ReviewRoutesV2,
    output: Option<PathBuf>,
    consent: HostedConsent,
}

impl ProviderOperationOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut routes = None;
        let mut output = None;
        let mut consent = HostedConsent::default();
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            if apply_consent(argument, &mut consent) {
                continue;
            }
            match argument {
                "--routes" => {
                    let path = cursor.required(argument)?;
                    routes = Some(parse_file(
                        Path::new(path),
                        "provider routes",
                        ReviewRoutesV2::parse,
                    )?);
                }
                "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
                other => return Err(format!("unknown review provider argument `{other}`")),
            }
        }
        Ok(Self {
            routes: routes.ok_or_else(|| "review provider requires --routes".to_string())?,
            output,
            consent,
        })
    }
}

struct DispositionOptions {
    review_path: PathBuf,
    ledger_path: PathBuf,
    recommendation_id: StableId,
    status: DispositionStatus,
    operator_identifier: String,
    rationale: Option<String>,
    counterevidence: Vec<String>,
}

impl DispositionOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut review_path = None;
        let mut ledger_path = None;
        let mut recommendation_id = None;
        let mut status = None;
        let mut operator_identifier = None;
        let mut rationale = None;
        let mut counterevidence = Vec::new();
        consume_arguments(args, |argument, cursor| {
            match argument {
                "--review" => review_path = Some(PathBuf::from(cursor.required(argument)?)),
                "--ledger" => ledger_path = Some(PathBuf::from(cursor.required(argument)?)),
                "--recommendation" => {
                    recommendation_id = Some(parse_stable_id(cursor, argument)?);
                }
                "--status" => status = Some(disposition_status(cursor.required(argument)?)?),
                "--operator" => operator_identifier = Some(cursor.required(argument)?.to_string()),
                "--rationale" => rationale = Some(cursor.required(argument)?.to_string()),
                "--counterevidence" => counterevidence.push(cursor.required(argument)?.to_string()),
                other => return Err(format!("unknown review disposition argument `{other}`")),
            }
            Ok(())
        })?;
        Ok(Self {
            review_path: review_path.ok_or_else(|| "disposition requires --review".to_string())?,
            ledger_path: ledger_path.ok_or_else(|| "disposition requires --ledger".to_string())?,
            recommendation_id: recommendation_id
                .ok_or_else(|| "disposition requires --recommendation".to_string())?,
            status: status.ok_or_else(|| "disposition requires --status".to_string())?,
            operator_identifier: operator_identifier
                .ok_or_else(|| "disposition requires --operator".to_string())?,
            rationale,
            counterevidence,
        })
    }
}

fn run_disposition(args: &[String]) -> Result<String, String> {
    let options = DispositionOptions::parse(args)?;
    let review = parse_file(
        &options.review_path,
        "review artifact",
        parse_engineering_review_v2,
    )?;
    if !review
        .portfolio
        .accepted_recommendation_ids
        .iter()
        .any(|recommendation| recommendation == &options.recommendation_id)
    {
        return Err("disposition recommendation is absent from the review".to_string());
    }
    let mut ledger = if options.ledger_path.exists() {
        parse_file(
            &options.ledger_path,
            "disposition ledger",
            parse_disposition_ledger,
        )?
    } else {
        DispositionLedger::new(review.review_id.clone())
    };
    if ledger.review_id != review.review_id {
        return Err("disposition ledger targets a different review".to_string());
    }
    ledger.upsert(RecommendationDisposition {
        recommendation_id: options.recommendation_id,
        status: options.status,
        timestamp_unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "system clock is before the Unix epoch".to_string())?
            .as_secs(),
        operator_identifier: options.operator_identifier,
        rationale: options.rationale,
        counterevidence: options.counterevidence,
    })?;
    let json = ledger.to_json();
    auditor_scan::atomic_write(&options.ledger_path, &json)?;
    Ok(json)
}

fn run_review_gate(args: &[String]) -> Result<String, String> {
    let mut review_path = None;
    let mut dispositions = None;
    let mut cursor = ArgCursor::new(args);
    while let Some(argument) = cursor.next() {
        match argument {
            "--review" => review_path = Some(PathBuf::from(cursor.required(argument)?)),
            "--dispositions" => dispositions = Some(PathBuf::from(cursor.required(argument)?)),
            other => return Err(format!("unknown review gate argument `{other}`")),
        }
    }
    let review_path = review_path.ok_or_else(|| "review gate requires --review".to_string())?;
    let disposition_path =
        dispositions.ok_or_else(|| "review gate requires --dispositions".to_string())?;
    let review = parse_file(&review_path, "review artifact", parse_engineering_review_v2)?;
    let ledger = parse_file(
        &disposition_path,
        "disposition ledger",
        parse_disposition_ledger,
    )?;
    let result = validate_disposition_coverage(&review, &ledger);
    let contested = review
        .execution_truth
        .iter()
        .any(|case| case.closure.final_state == FinalCaseState::Contested);
    let summary = json!({
        "contested": contested,
        "material_recommendations": review.portfolio.accepted_recommendation_ids.len(),
        "passed": result.is_ok(),
        "review_id": review.review_id,
    })
    .to_string();
    match result {
        Ok(()) => Ok(summary),
        Err(error) => Err(format!(
            "{}{summary}\n{error}",
            crate::gate_command::POLICY_FAILURE
        )),
    }
}

struct VerifyOptions {
    report_path: PathBuf,
    repo_root: PathBuf,
    dispositions: Option<PathBuf>,
}

impl VerifyOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut report_path = None;
        let mut repo_root = None;
        let mut dispositions = None;
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            match argument {
                "--report" => report_path = Some(PathBuf::from(cursor.required(argument)?)),
                "--repo-root" => repo_root = Some(PathBuf::from(cursor.required(argument)?)),
                "--dispositions" => dispositions = Some(PathBuf::from(cursor.required(argument)?)),
                other => return Err(format!("unknown review verify argument `{other}`")),
            }
        }
        Ok(Self {
            report_path: report_path
                .ok_or_else(|| "review verify requires --report".to_string())?,
            repo_root: repo_root.ok_or_else(|| "review verify requires --repo-root".to_string())?,
            dispositions,
        })
    }
}

fn run_review_verify(args: &[String]) -> Result<String, String> {
    let options = VerifyOptions::parse(args)?;
    let verified = verify_case_engineering_review(&ReviewVerifyRequest {
        repo_root: options.repo_root,
        report: options.report_path.clone(),
    })
    .map_err(|error| error.to_string())?;
    let disposition_complete = if let Some(path) = options.dispositions {
        let review = load_review_artifact_for_report(&options.report_path)?;
        let ledger = parse_file(&path, "disposition ledger", parse_disposition_ledger)?;
        if let Err(error) = validate_disposition_coverage(&review, &ledger) {
            return Err(format!("{}{}", crate::gate_command::POLICY_FAILURE, error));
        }
        true
    } else {
        false
    };
    Ok(json!({
        "case_count": verified.case_count,
        "accepted_recommendation_count": verified.accepted_recommendation_count,
        "blocked_case_count": verified.blocked_case_count,
        "contested_case_count": verified.contested_case_count,
        "challenge_response_count": verified.challenge_response_count,
        "disposition_complete": disposition_complete,
        "review_id": verified.review_id,
        "valid": verified.valid,
    })
    .to_string())
}

fn load_review_artifact_for_report(
    report_path: &Path,
) -> Result<auditor_review::EngineeringReviewV2, String> {
    let report_text = std::fs::read_to_string(report_path)
        .map_err(|error| format!("failed to read reviewed report: {error}"))?;
    let report: serde_json::Value =
        serde_json::from_str(&report_text).map_err(|error| error.to_string())?;
    let name = report
        .get("engineering_review")
        .and_then(|value| value.get("path"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "reviewed report has no engineering review artifact path".to_string())?;
    let relative = Path::new(name);
    if relative.components().count() != 1
        || !relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err("review artifact path is not a safe sibling".to_string());
    }
    let artifact_path = report_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative);
    parse_file(
        &artifact_path,
        "review artifact",
        parse_engineering_review_v2,
    )
}

fn disposition_status(value: &str) -> Result<DispositionStatus, String> {
    let index = ["accepted", "deferred", "rejected"]
        .iter()
        .position(|candidate| *candidate == value)
        .ok_or_else(|| "--status requires accepted|deferred|rejected".to_string())?;
    [
        DispositionStatus::Accepted,
        DispositionStatus::Deferred,
        DispositionStatus::Rejected,
    ]
    .into_iter()
    .nth(index)
    .ok_or_else(|| "invalid disposition status table".to_string())
}
