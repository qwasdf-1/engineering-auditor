use std::path::PathBuf;

#[cfg(feature = "graph-analysis")]
use auditor_detectors::JsonGraphProvider;
#[cfg(not(feature = "graph-analysis"))]
use auditor_detectors::NoGraphProvider;
use auditor_report::NoReviewArtifactLoader;
use auditor_scan::{run_scan as execute_scan, ScanFormat, ScanRequest};

use crate::scan_options::ScanOptions;

pub(crate) fn run_scan(args: &[String]) -> Result<String, String> {
    run_scan_with_overrides(args, None, None, &[])
}

pub(crate) fn run_scan_with_overrides(
    args: &[String],
    output: Option<PathBuf>,
    repository_map_output: Option<PathBuf>,
    additional_excluded_paths: &[PathBuf],
) -> Result<String, String> {
    let mut options = ScanOptions::parse(args)?;
    if let Some(output) = output {
        options.output = Some(output);
    }
    let mut prepared = options.prepare_with_exclusions(additional_excluded_paths)?;
    if let Some(repository_map_output) = repository_map_output {
        prepared.request.repository_map_output = Some(repository_map_output);
    }
    #[cfg(feature = "graph-analysis")]
    let provider = JsonGraphProvider {
        repo_graph: options.repo_graph.clone(),
        co_edit_journal: options.co_edit_journal.clone(),
    };
    #[cfg(not(feature = "graph-analysis"))]
    let provider = NoGraphProvider;
    let format = {
        let parsed = ScanFormat::parse(&options.format)?;
        if parsed != ScanFormat::Human && options.summary_only {
            return Err("--summary-only requires --format human".to_string());
        }
        parsed
    };
    let color = options.resolved_color();
    let summary_only = options.summary_only;
    let quiet = options.quiet;
    let requested_format = format;
    let output_path = options.output.clone();
    execute_scan(
        ScanRequest {
            repo_root: options.repo_root,
            analysis: prepared.request,
            external_inputs: prepared.external_inputs,
            baseline: options.baseline,
            format,
            output: options.output,
            color,
            summary_only,
            review_loader: Box::new(NoReviewArtifactLoader),
        },
        &provider,
    )
    .map(|result| {
        if quiet {
            // --quiet: suppress stdout. Emit a single stderr line so an
            // operator sees the finding total and where the artifact lives;
            // without --output the report is not written anywhere.
            let destination = output_path
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".to_string());
            eprintln!(
                "auditor: {} findings; format={}; output={}",
                result.report.findings.len(),
                format_label(requested_format),
                destination
            );
            // Return empty bytes so main() prints nothing on stdout.
            String::new()
        } else {
            result.rendered
        }
    })
}

fn format_label(format: ScanFormat) -> &'static str {
    match format {
        ScanFormat::Json => "json",
        ScanFormat::Sarif => "sarif",
        ScanFormat::Human => "human",
    }
}
