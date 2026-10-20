//! `auditor digest` subcommand.
//!
//! Regenerates a human-readable digest by running a scan and rendering
//! `report_to_human`. The plan originally proposed `digest --report
//! <machine.json>` — i.e. parsing a saved JSON report back into an
//! `AuditReport` — but `AuditReport` does not derive `Deserialize` (no
//! serde derives anywhere in auditor-core/auditor-report; serialization is
//! hand-rolled `to_json`). Rather than expand the report contract or
//! maintain a stale hand-parser, this subcommand re-runs the scan in
//! `--format human` and writes the digest. The two-file atomic write from
//! Task 7 already produces the byte-identical machine sibling next to any
//! `--output <digest.md>`, so operators still get the round-trippable JSON
//! artifact without a separate `--report` parse path.

use std::path::PathBuf;

use crate::{arg_cursor::ArgCursor, scan_command};

struct DigestOptions {
    repo_root: Option<PathBuf>,
    report_path: Option<PathBuf>,
    output: Option<PathBuf>,
    no_color: bool,
    summary_only: bool,
    format: String,
    profile: String,
}

impl DigestOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            repo_root: None,
            report_path: None,
            output: None,
            no_color: false,
            summary_only: false,
            format: "human".to_string(),
            profile: "full".to_string(),
        };
        let mut cursor = ArgCursor::new(args);
        while let Some(argument) = cursor.next() {
            match argument {
                "--repo-root" => {
                    options.repo_root = Some(PathBuf::from(cursor.required(argument)?))
                }
                "--report" => options.report_path = Some(PathBuf::from(cursor.required(argument)?)),
                "--output" => options.output = Some(PathBuf::from(cursor.required(argument)?)),
                "--format" => options.format = cursor.required(argument)?.to_string(),
                "--profile" => options.profile = cursor.required(argument)?.to_string(),
                "--no-color" => options.no_color = true,
                "--summary-only" => options.summary_only = true,
                "--quiet" => {}
                other => return Err(format!("unknown digest argument `{other}`")),
            }
        }
        Ok(options)
    }
}

pub(crate) fn run_digest(args: &[String]) -> Result<String, String> {
    let options = DigestOptions::parse(args)?;
    if options.format != "human" {
        return Err(format!(
            "digest only supports --format human (got `{}`); use `scan --format {}` for the machine artifact",
            options.format, options.format
        ));
    }
    match (options.report_path.as_deref(), options.repo_root.as_deref()) {
        (Some(report_path), None) => render_digest_from_report(
            report_path,
            options.output.as_deref(),
            options.no_color,
            options.summary_only,
        ),
        (None, Some(repo_root)) => render_digest_from_repo(
            repo_root,
            options.output.as_deref(),
            options.no_color,
            options.summary_only,
            &options.profile,
        ),
        (Some(_), Some(_)) => Err("digest accepts --report or --repo-root, not both".to_string()),
        (None, None) => {
            Err("digest requires --report <machine.json> or --repo-root <path>".to_string())
        }
    }
}

/// `digest --report <machine.json>`: parse the saved report via
/// `auditor_report::parse_report_for_digest`, render the digest via
/// `report_to_human`, and write it to `--output` (or return the bytes for
/// stdout when `--output` is absent). The footer references the input file
/// by its file_name and carries the SHA-256 of the input bytes so the digested
/// artifact can be traced back to the source of truth.
fn render_digest_from_report(
    report_path: &std::path::Path,
    output: Option<&std::path::Path>,
    no_color: bool,
    summary_only: bool,
) -> Result<String, String> {
    let machine_text = std::fs::read_to_string(report_path)
        .map_err(|error| format!("failed to read --report {}: {error}", report_path.display()))?;
    let color = resolve_color(no_color, output);
    let digest = render_machine_digest(&machine_text, report_path, color, summary_only)?;
    if let Some(output) = output {
        auditor_scan::atomic_write(output, &digest)?;
    }
    Ok(digest)
}

/// `digest --repo-root <path>`: re-run the scan in `--format human` via the
/// CLI scan path. Produces the same digest + sibling pair the scan path
/// emits; provided as an operator convenience when a saved artifact is not
/// already on disk.
fn render_digest_from_repo(
    repo_root: &std::path::Path,
    output: Option<&std::path::Path>,
    no_color: bool,
    summary_only: bool,
    profile: &str,
) -> Result<String, String> {
    let mut scan_args = vec![
        "--repo-root".to_string(),
        repo_root.to_string_lossy().into_owned(),
        "--format".to_string(),
        "human".to_string(),
        "--profile".to_string(),
        profile.to_string(),
    ];
    if let Some(output) = output {
        scan_args.push("--output".to_string());
        scan_args.push(output.to_string_lossy().into_owned());
    }
    if no_color {
        scan_args.push("--no-color".to_string());
    }
    if summary_only {
        scan_args.push("--summary-only".to_string());
    }
    scan_command::run_scan(&scan_args)
}

/// Resolve the digest color. File destinations are not ttys, so `--output`
/// always suppresses ANSI; without `--output`, color is gated by stdout's
/// tty status unless `--no-color` is set.
fn resolve_color(no_color: bool, output: Option<&std::path::Path>) -> auditor_report::ColorMode {
    if no_color || output.is_some() {
        return auditor_report::ColorMode::Plain;
    }
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        auditor_report::ColorMode::Auto
    } else {
        auditor_report::ColorMode::Plain
    }
}

pub(crate) fn render_machine_digest(
    machine_text: &str,
    report_path: &std::path::Path,
    color: auditor_report::ColorMode,
    summary_only: bool,
) -> Result<String, String> {
    auditor_report::validate_report_json(machine_text)?;
    let report = auditor_report::parse_report_for_digest(machine_text)?;
    let sibling_path = report_path.file_name().map_or_else(
        || report_path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let footer = auditor_report::HumanFooter {
        sibling_path,
        sibling_digest: auditor_core::stable_digest_bytes(machine_text.as_bytes()),
    };
    Ok(auditor_report::report_to_human(
        &report,
        color,
        &footer,
        summary_only,
        &auditor_report::NoReviewArtifactLoader,
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_repo(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ea-digest-{name}-{nonce}"));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn one() -> usize { 1 }\n").unwrap();
        root
    }

    #[test]
    fn digest_rejects_missing_input() {
        let error = run_digest(&[]).expect_err("must reject");
        assert!(
            error.contains("digest requires --report")
                || error.contains("digest requires --repo-root"),
            "error: {error}"
        );
    }

    #[test]
    fn digest_rejects_both_report_and_repo_root() {
        let root = temp_repo("digest-both");
        let machine_path = root.join("audit.json");
        std::fs::write(&machine_path, "{}").unwrap();
        let args = vec![
            "--report".to_string(),
            machine_path.display().to_string(),
            "--repo-root".to_string(),
            root.display().to_string(),
        ];
        let error = run_digest(&args).expect_err("must reject");
        assert!(error.contains("not both"), "error: {error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_rejects_non_human_format() {
        let root = temp_repo("digest-fmt");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--format".into(),
            "json".into(),
        ];
        let error = run_digest(&args).expect_err("must reject");
        assert!(
            error.contains("digest only supports --format human"),
            "error: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_to_stdout_renders_human_digest() {
        let root = temp_repo("digest-stdout");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--no-color".into(),
        ];
        let rendered = run_digest(&args).expect("digest succeeds");
        assert!(
            rendered.contains("# Engineering Auditor"),
            "header: {rendered}"
        );
        assert!(
            rendered.contains("## Severity Totals"),
            "totals: {rendered}"
        );
        assert!(!rendered.contains('\x1b'), "no-color strips ANSI");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_to_file_writes_digest_and_sibling() {
        let root = temp_repo("digest-file");
        let digest_path = root.join("reports/audit.md");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--output".into(),
            digest_path.display().to_string(),
        ];
        let rendered = run_digest(&args).expect("digest succeeds");
        assert!(rendered.contains("# Engineering Auditor"));
        assert!(
            digest_path.exists(),
            "digest file: {}",
            digest_path.display()
        );
        let sibling = root.join("reports/audit.audit.json");
        assert!(sibling.exists(), "sibling: {}", sibling.display());
        let machine_text = std::fs::read_to_string(&sibling).unwrap();
        auditor_report::validate_report_json(&machine_text).expect("sibling is valid");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_summary_only_emits_truncated_digest() {
        let root = temp_repo("digest-summary");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--summary-only".into(),
            "--no-color".into(),
        ];
        let rendered = run_digest(&args).expect("digest succeeds");
        assert!(rendered.contains("# Engineering Auditor"), "header kept");
        assert!(rendered.contains("## Severity Totals"), "totals kept");
        assert!(
            rendered.contains("## Per-Detector Breakdown"),
            "breakdown kept"
        );
        // Section 4 (Findings by Detector) and the review section are
        // omitted under --summary-only.
        assert!(
            !rendered.contains("## Findings by Detector"),
            "section 4 omitted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_from_report_renders_human_digest() {
        let root = temp_repo("digest-from-report");
        // First run a scan to produce a saved machine JSON report.
        let machine_path = root.join("reports/audit.json");
        let scan_args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--profile".into(),
            "full".into(),
            "--format".into(),
            "json".into(),
            "--output".into(),
            machine_path.display().to_string(),
        ];
        let _ = scan_command::run_scan(&scan_args).expect("scan produces machine JSON");
        assert!(machine_path.exists(), "machine report written");
        // Now digest from the saved machine JSON, to stdout.
        let stdout_args = vec![
            "--report".to_string(),
            machine_path.display().to_string(),
            "--no-color".into(),
        ];
        let rendered = run_digest(&stdout_args).expect("digest from report succeeds");
        assert!(
            rendered.contains("# Engineering Auditor"),
            "header: {rendered}"
        );
        assert!(
            rendered.contains("## Severity Totals"),
            "totals: {rendered}"
        );
        assert!(!rendered.contains('\x1b'), "no-color strips ANSI");
        // The digest footer cross-references the input file by basename.
        assert!(
            rendered.contains("audit.json") || rendered.contains("machine_sibling"),
            "footer references report: {rendered}"
        );
        // And digest from --report to --output writes the digest file.
        let out_path = root.join("reports/digest.md");
        let file_args = vec![
            "--report".to_string(),
            machine_path.display().to_string(),
            "--output".into(),
            out_path.display().to_string(),
        ];
        let _ = run_digest(&file_args).expect("digest to file succeeds");
        assert!(
            out_path.exists(),
            "digest file written: {}",
            out_path.display()
        );
        let digest_text = std::fs::read_to_string(&out_path).unwrap();
        assert!(
            digest_text.contains("# Engineering Auditor"),
            "digest header"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_unknown_argument_rejected() {
        let root = temp_repo("digest-unknown");
        let args = vec![
            "--repo-root".to_string(),
            root.display().to_string(),
            "--turbo".into(),
        ];
        let error = run_digest(&args).expect_err("must reject");
        assert!(error.contains("unknown digest argument"), "error: {error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
