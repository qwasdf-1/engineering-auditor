//! `review qualify` and `review rubric`: the offline qualification of
//! recorded engineering reviews (v1.6 plan, step 6). Both read only files:
//! the report and the repository map it names, the history digest, and the
//! review records, so they run after the checkout is gone.

use std::path::PathBuf;

use auditor_report::parse_repository_map_reference;
use auditor_review::{
    load_repository_map, qualify_reviews, usefulness_sheet, EngineeringReview, HistoryDigest,
    QualificationInputs, QuestionTemplateBundle, RecordedReview, RepositoryView, ReviewBrief,
    ReviewPromptBundleV2, ReviewRoutesV2, StabilityPolicy, UsefulnessSheet,
};
use serde_json::Value;

use crate::gate_command::POLICY_FAILURE;
use crate::review_command::{consume_arguments, parse_file, publish};
use crate::review_run::PROMPT_BUNDLE;

#[derive(Default)]
struct QualifyOptions {
    report: Option<PathBuf>,
    reviews: Vec<PathBuf>,
    routes: Option<PathBuf>,
    brief: Option<PathBuf>,
    history: Option<PathBuf>,
    scores: Vec<PathBuf>,
    top: Option<usize>,
    min_overlap: Option<f64>,
    output: Option<PathBuf>,
}

/// Exits 1 with the result on stdout when a gate fails.
pub(crate) fn run_qualify(args: &[String]) -> Result<String, String> {
    let mut options = QualifyOptions::default();
    consume_arguments(args, |argument, cursor| {
        let mut path = || cursor.required(argument).map(PathBuf::from);
        match argument {
            "--report" => options.report = Some(path()?),
            "--review" => options.reviews.push(path()?),
            "--routes" => options.routes = Some(path()?),
            "--brief" => options.brief = Some(path()?),
            "--history" => options.history = Some(path()?),
            "--scores" => options.scores.push(path()?),
            "--output" => options.output = Some(path()?),
            "--top" => {
                let value = cursor.required(argument)?;
                options.top =
                    Some(
                        value.parse().ok().filter(|top| *top > 0).ok_or_else(|| {
                            format!("--top takes a positive count, not `{value}`")
                        })?,
                    );
            }
            "--min-overlap" => {
                let value = cursor.required(argument)?;
                options.min_overlap = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|share: &f64| (0.0..=1.0).contains(share))
                        .ok_or_else(|| {
                            format!("--min-overlap takes a share from 0 to 1, not `{value}`")
                        })?,
                );
            }
            other => return Err(format!("unknown review qualify argument `{other}`")),
        }
        Ok(())
    })?;
    let report_path = options.report.ok_or("review qualify requires --report")?;
    if options.reviews.is_empty() {
        return Err("review qualify requires --review, once per run".to_string());
    }
    let routes = parse_file(
        &options.routes.ok_or("review qualify requires --routes")?,
        "provider routes",
        ReviewRoutesV2::parse,
    )?;
    let brief = options
        .brief
        .map(|path| parse_file(&path, "review brief", ReviewBrief::parse))
        .transpose()?;
    let history = options
        .history
        .map(|path| parse_file(&path, "history digest", HistoryDigest::parse))
        .transpose()?;
    let runs = options
        .reviews
        .iter()
        .map(|path| {
            Ok(RecordedReview {
                label: path.display().to_string(),
                review: parse_file(path, "engineering review", json::<EngineeringReview>)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let sheets = options
        .scores
        .iter()
        .map(|path| parse_file(path, "usefulness sheet", json::<UsefulnessSheet>))
        .collect::<Result<Vec<_>, String>>()?;
    let text = std::fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read report {}: {error}", report_path.display()))?;
    let reference = parse_repository_map_reference(&text)?;
    let map = load_repository_map(&report_path, &reference).map_err(|error| error.to_string())?;
    let report: Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid report: {error}"))?;
    let mut view = RepositoryView::new(&report, &map)?;
    if let Some(history) = &history {
        view = view.with_history(history);
    }
    let defaults = StabilityPolicy::default();
    let policy = StabilityPolicy {
        top: options.top.unwrap_or(defaults.top),
        min_overlap: options.min_overlap.unwrap_or(defaults.min_overlap),
    };
    let qualification = qualify_reviews(
        &QualificationInputs {
            view: &view,
            brief: brief.as_ref(),
            templates: &QuestionTemplateBundle::checked_in()?,
            prompts: &ReviewPromptBundleV2::parse(PROMPT_BUNDLE)?,
            routes: &routes,
        },
        &runs,
        policy,
        &sheets,
    )?;
    let passed = qualification.passed;
    let summary = publish(None, &qualification)?;
    if options.output.is_some() {
        publish(options.output, &qualification)?;
    }
    if passed {
        Ok(summary)
    } else {
        Err(format!("{POLICY_FAILURE}{summary}"))
    }
}

/// An unscored usefulness sheet for one review.
pub(crate) fn run_rubric(args: &[String]) -> Result<String, String> {
    let (mut review, mut output) = (None, None);
    consume_arguments(args, |argument, cursor| {
        match argument {
            "--review" => review = Some(PathBuf::from(cursor.required(argument)?)),
            "--output" => output = Some(PathBuf::from(cursor.required(argument)?)),
            other => return Err(format!("unknown review rubric argument `{other}`")),
        }
        Ok(())
    })?;
    let review = parse_file(
        &review.ok_or("review rubric requires --review")?,
        "engineering review",
        json::<EngineeringReview>,
    )?;
    publish(output, &usefulness_sheet(&review))
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(text)
}
