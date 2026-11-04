use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions, DEFAULT_HISTORY_WINDOW_DAYS,
};
use auditor_review::{
    discover_provider, HostedConsent, HttpTransport, ReviewBrief, ReviewRoutesV2,
};
use auditor_scan::{PatchChecker, TestCommand};
use serde_json::json;

use crate::arg_cursor::ArgCursor;

/// The engineering review is being rebuilt (v1.6 plan): the subcommands of
/// the v2 case protocol are gone. Provider discovery and transcript
/// rendering remain, beside the new path's history digest, patch check, and
/// `review run --experimental-reviewer`.
const REMOVED_REVIEW_COMMANDS: [&str; 5] =
    ["plan", "inspect-case", "verify", "disposition", "gate"];

pub(crate) fn run_review(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("transcript") => run_review_transcript(&args[1..]),
        Some("provider") => run_provider_command(&args[1..]),
        Some("history") => run_review_history(&args[1..]),
        Some("check-patch") => run_check_patch(&args[1..]),
        Some("run") => crate::review_run::run(&args[1..]),
        Some(removed) if REMOVED_REVIEW_COMMANDS.contains(&removed) => Err(format!(
            "review {removed} was removed while the engineering review is rebuilt; see docs/reference/layer6-llm-review.md"
        )),
        Some(other) => Err(format!("unknown review command `{other}`")),
        None => Err(
            "review requires transcript, provider, history, check-patch, or run".to_string(),
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

fn run_review_history(args: &[String]) -> Result<String, String> {
    let mut repo_root = PathBuf::from(".");
    let mut window_days = DEFAULT_HISTORY_WINDOW_DAYS;
    let mut output = None;
    consume_arguments(args, |argument, cursor| {
        match argument {
            "--repo-root" => repo_root = PathBuf::from(cursor.required(argument)?),
            "--window-days" => {
                window_days = cursor
                    .required(argument)?
                    .parse()
                    .map_err(|_| "--window-days requires a positive integer".to_string())?;
            }
            "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
            other => return Err(format!("unknown review history argument `{other}`")),
        }
        Ok(())
    })?;
    let digest = auditor_detectors::history_digest(&repo_root, window_days)?;
    publish(output, &digest)
}

fn run_check_patch(args: &[String]) -> Result<String, String> {
    let mut repo_root = PathBuf::from(".");
    let mut patch = None;
    let mut cited = Vec::new();
    let mut profile = ScanProfile::Full;
    let mut brief = None;
    let mut include_references = false;
    let mut output = None;
    consume_arguments(args, |argument, cursor| {
        match argument {
            "--repo-root" => repo_root = PathBuf::from(cursor.required(argument)?),
            "--patch" => patch = Some(PathBuf::from(cursor.required(argument)?)),
            "--resolves" => cited.push(cursor.required(argument)?.to_string()),
            "--profile" => profile = ScanProfile::parse(cursor.required(argument)?)?,
            "--brief" => {
                let path = PathBuf::from(cursor.required(argument)?);
                brief = Some(parse_file(&path, "review brief", ReviewBrief::parse)?);
            }
            "--include-references" => include_references = true,
            "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
            other => return Err(format!("unknown review check-patch argument `{other}`")),
        }
        Ok(())
    })?;
    let patch = patch.ok_or("review check-patch requires --patch")?;
    if cited.is_empty() {
        return Err("review check-patch requires --resolves with a finding ID".to_string());
    }
    let patch = std::fs::read_to_string(&patch)
        .map_err(|error| format!("failed to read patch {}: {error}", patch.display()))?;
    let tests = brief
        .and_then(|brief| brief.test_command)
        .map(|command| TestCommand {
            argv: command.argv,
            timeout: Duration::from_secs(command.timeout_seconds),
            pass_env: command.pass_env,
        });
    let analysis = AnalysisRequest {
        discovery: SourceDiscoveryOptions {
            include_references,
            method: AnalysisMethod::Auto,
        },
        profile,
        scope: ScanScope::All,
        repository_map_output: None,
        excluded_paths: output.iter().cloned().collect::<BTreeSet<_>>(),
        history_root: None,
    };
    let checker = PatchChecker::new(&repo_root, analysis, &NoGraphProvider, tests)?;
    let check = checker.check(&patch, &cited);
    publish(output, &check)
}

/// Pretty JSON, written atomically to `output` or returned.
pub(crate) fn publish(
    output: Option<PathBuf>,
    value: &impl serde::Serialize,
) -> Result<String, String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("failed to serialize: {error}"))?;
    match output {
        Some(path) => {
            auditor_scan::atomic_write(&path, &format!("{text}\n"))?;
            Ok(String::new())
        }
        None => Ok(text),
    }
}

pub(crate) fn consume_arguments(
    args: &[String],
    mut consume: impl FnMut(&str, &mut ArgCursor<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let mut cursor = ArgCursor::new(args);
    while let Some(argument) = cursor.next() {
        consume(argument, &mut cursor)?;
    }
    Ok(())
}

pub(crate) fn apply_consent(argument: &str, consent: &mut HostedConsent) -> bool {
    match argument {
        "--allow-hosted-review" | "--allow-hosted" => consent.allow_hosted = true,
        "--allow-anonymous-provider" | "--allow-anonymous" => consent.allow_anonymous = true,
        _ => return false,
    }
    true
}

pub(crate) fn parse_file<T, E>(
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
