//! `review run --experimental-reviewer`: the rebuilt engineering review
//! over a scan's report and repository map, behind an explicit flag until
//! a route qualifies on it (v1.6 plan, Phase B).
//!
//! With `--output`, the record (and the transcript, with `--transcript`)
//! is written after each question, marked stopped until the review ends,
//! so `--resume` with that record continues an interrupted or stopped run.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_report::parse_repository_map_reference;
use auditor_review::{
    load_and_validate_repository_map, load_history_digest, run_engineering_review,
    EngineeringReview, HostedConsent, HttpTransport, QuestionTemplateBundle, RecommendationChecker,
    RepositoryView, ReviewBrief, ReviewPromptBundleV2, ReviewRoutesV2, ReviewSetup,
    TranscriptRecorder, DEFAULT_PATCH_CHECKS,
};
use auditor_scan::{PatchChecker, TestCommand};
use serde_json::Value;

use crate::review_command::{apply_consent, consume_arguments, parse_file, publish};

pub(crate) const PROMPT_BUNDLE: &str =
    include_str!("../../../config/auditor/review-prompt-bundle-v2.json");

#[derive(Default)]
struct Options {
    experimental: bool,
    report: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    routes: Option<PathBuf>,
    brief: Option<PathBuf>,
    history: Option<PathBuf>,
    check_patches: bool,
    max_patch_checks: Option<usize>,
    include_references: bool,
    transcript: Option<PathBuf>,
    output: Option<PathBuf>,
    resume: Option<PathBuf>,
    consent: HostedConsent,
}

pub(crate) fn run(args: &[String]) -> Result<String, String> {
    let mut options = Options::default();
    consume_arguments(args, |argument, cursor| {
        if apply_consent(argument, &mut options.consent) {
            return Ok(());
        }
        let mut path = || cursor.required(argument).map(PathBuf::from);
        match argument {
            "--experimental-reviewer" => options.experimental = true,
            "--report" => options.report = Some(path()?),
            "--repo-root" => options.repo_root = Some(path()?),
            "--routes" => options.routes = Some(path()?),
            "--brief" => options.brief = Some(path()?),
            "--history" => options.history = Some(path()?),
            "--transcript" => options.transcript = Some(path()?),
            "--output" => options.output = Some(path()?),
            "--resume" => options.resume = Some(path()?),
            "--check-patches" => options.check_patches = true,
            "--include-references" => options.include_references = true,
            "--max-patch-checks" => {
                let value = cursor.required(argument)?;
                options.max_patch_checks = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--max-patch-checks takes a count, not `{value}`"))?,
                );
            }
            other => return Err(format!("unknown review run argument `{other}`")),
        }
        Ok(())
    })?;
    if !options.experimental {
        return Err("review run was removed while the engineering review is rebuilt; the rebuilt review runs with --experimental-reviewer (see docs/reference/layer6-llm-review.md)".to_string());
    }
    review(options)
}

fn review(options: Options) -> Result<String, String> {
    let report_path = options.report.ok_or("review run requires --report")?;
    let routes = parse_file(
        &options.routes.ok_or("review run requires --routes")?,
        "provider routes",
        ReviewRoutesV2::parse,
    )?;
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let brief = options
        .brief
        .map(|path| parse_file(&path, "review brief", ReviewBrief::parse))
        .transpose()?;
    let text = std::fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read report {}: {error}", report_path.display()))?;
    let reference = parse_repository_map_reference(&text)?;
    let map = load_and_validate_repository_map(&report_path, &repo_root, &reference)
        .map_err(|error| error.to_string())?;
    let report: Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid report: {error}"))?;
    let history = options
        .history
        .map(|path| load_history_digest(&path, &repo_root))
        .transpose()?;
    let mut view = RepositoryView::new(&report, &map)?;
    if let Some(history) = &history {
        view = view.with_history(history);
    }
    let templates = QuestionTemplateBundle::checked_in()?;
    let prompts = ReviewPromptBundleV2::parse(PROMPT_BUNDLE)?;
    let artifacts = [
        Some(report_path.clone()),
        reference
            .path
            .as_ref()
            .and_then(|path| Some(report_path.parent()?.join(path))),
        options.output.clone(),
        options.transcript.clone(),
    ];
    let checker = if options.check_patches {
        let tests = brief
            .as_ref()
            .and_then(|brief| brief.test_command.clone())
            .map(|command| TestCommand {
                argv: command.argv,
                timeout: Duration::from_secs(command.timeout_seconds),
                pass_env: command.pass_env,
            });
        let analysis = AnalysisRequest {
            discovery: SourceDiscoveryOptions {
                include_references: options.include_references,
                method: AnalysisMethod::Auto,
            },
            profile: ScanProfile::parse(report["profile"].as_str().unwrap_or("full"))?,
            scope: ScanScope::All,
            repository_map_output: None,
            excluded_paths: artifacts.into_iter().flatten().collect::<BTreeSet<_>>(),
            history_root: None,
        };
        Some(ScanChecker(PatchChecker::new(
            &repo_root,
            analysis,
            &NoGraphProvider,
            tests,
        )?))
    } else {
        None
    };
    let prior = options
        .resume
        .map(|path| {
            parse_file(&path, "review to resume", |text| {
                serde_json::from_str::<EngineeringReview>(text)
                    .map_err(|error| format!("invalid review to resume: {error}"))
            })
        })
        .transpose()?;
    // A resumed run's transcript continues the stopped run's.
    let earlier_transcript = match (&prior, &options.transcript) {
        (Some(_), Some(path)) if path.exists() => {
            let recorded: Value =
                parse_file(path, "transcript", |text| serde_json::from_str(text))?;
            recorded["transcript"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let transport = HttpTransport::default();
    let transcript = if options.transcript.is_some() {
        TranscriptRecorder::enabled()
    } else {
        TranscriptRecorder::null()
    };
    let write_transcript = || -> Result<(), String> {
        let Some(path) = &options.transcript else {
            return Ok(());
        };
        let mut recorded = transcript.to_json(None);
        if let Some(events) = recorded["transcript"].as_array_mut() {
            events.splice(0..0, earlier_transcript.iter().cloned());
        }
        publish(Some(path.clone()), &recorded).map(drop)
    };
    let checkpoint = |review: &EngineeringReview| {
        let written = publish(options.output.clone(), review).and_then(|_| write_transcript());
        if let Err(error) = written {
            eprintln!("auditor: the review checkpoint was not written: {error}");
        }
    };
    let review = run_engineering_review(
        &view,
        ReviewSetup {
            repo_root: &repo_root,
            brief: brief.as_ref(),
            templates: &templates,
            prompts: &prompts,
            routes: &routes,
            consent: options.consent,
            transport: &transport,
            transcript: &transcript,
            checker: checker
                .as_ref()
                .map(|checker| checker as &dyn RecommendationChecker),
            max_patch_checks: options.max_patch_checks.unwrap_or(DEFAULT_PATCH_CHECKS),
            resume: prior.as_ref(),
            checkpoint: options
                .output
                .is_some()
                .then_some(&checkpoint as &dyn Fn(&EngineeringReview)),
        },
    )?;
    write_transcript()?;
    let published = publish(options.output, &review)?;
    match &review.stopped {
        Some(reason) if review.answered().next().is_none() => {
            Err(format!("the review answered no question: {reason}"))
        }
        _ => Ok(published),
    }
}

/// The scan-backed patch checker behind the review's `check_patch`.
struct ScanChecker<'p>(PatchChecker<'p>);

impl RecommendationChecker for ScanChecker<'_> {
    fn check(&self, patch: &str, cited_findings: &[String]) -> Value {
        serde_json::to_value(self.0.check(patch, cited_findings)).expect("a patch check serializes")
    }
}
