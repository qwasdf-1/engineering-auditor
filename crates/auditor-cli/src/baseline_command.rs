use std::path::PathBuf;

use auditor_report::{parse_baseline_document, parse_legacy_baseline_reasons, BaselineDocument};

use crate::arg_cursor::ArgCursor;

pub(crate) fn run_baseline(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("create") => run_create(args),
        Some("rekey") => run_rekey(&args[1..]),
        Some("migrate") => run_migrate(&args[1..]),
        _ => Err("baseline command requires `create`, `rekey`, or `migrate`".to_string()),
    }
}

fn read(path: &str, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("failed to read {what}: {error}"))
}

fn run_rekey(args: &[String]) -> Result<String, String> {
    let (mut baseline, mut report, mut prune) = (None, None, false);
    let mut cursor = ArgCursor::new(args);
    while let Some(argument) = cursor.next() {
        match argument {
            "--baseline" => baseline = Some(cursor.required(argument)?.to_string()),
            "--report" => report = Some(cursor.required(argument)?.to_string()),
            "--prune-resolved" => prune = true,
            other => return Err(format!("unknown baseline rekey argument `{other}`")),
        }
    }
    let baseline = baseline.ok_or("baseline rekey requires --baseline")?;
    let report = report.ok_or("baseline rekey requires --report")?;
    let document = parse_baseline_document(&read(&baseline, "baseline")?)?;
    let (rekeyed, matched) = document.rekey(&read(&report, "report")?, prune)?;
    eprintln!(
        "auditor: baseline rekey unchanged={} carried={} resolved={}{} new_unaccepted={}",
        matched.unchanged.len(),
        matched.carried.len(),
        matched.resolved.len(),
        if prune { " (pruned)" } else { " (kept)" },
        matched.new.len()
    );
    Ok(rekeyed.to_json())
}

fn run_migrate(args: &[String]) -> Result<String, String> {
    let (mut baseline, mut previous, mut report) = (None, None, None);
    let mut cursor = ArgCursor::new(args);
    while let Some(argument) = cursor.next() {
        match argument {
            "--baseline" => baseline = Some(cursor.required(argument)?.to_string()),
            "--from-report" => previous = Some(cursor.required(argument)?.to_string()),
            "--report" => report = Some(cursor.required(argument)?.to_string()),
            other => return Err(format!("unknown baseline migrate argument `{other}`")),
        }
    }
    let baseline = baseline.ok_or("baseline migrate requires --baseline")?;
    let previous = previous.ok_or("baseline migrate requires --from-report")?;
    let report = report.ok_or("baseline migrate requires --report")?;
    let reasons = parse_legacy_baseline_reasons(&read(&baseline, "baseline")?)?;
    let (migrated, unmapped) = BaselineDocument::migrate(
        &reasons,
        &read(&previous, "previous report")?,
        &read(&report, "report")?,
    )?;
    eprintln!(
        "auditor: baseline migrate mapped={} unmapped={}",
        migrated.entries.len(),
        unmapped.len()
    );
    for id in &unmapped {
        eprintln!("auditor: unmapped baseline entry {id} (review: resolved or reshaped)");
    }
    Ok(migrated.to_json())
}

fn run_create(args: &[String]) -> Result<String, String> {
    let mut report_path: Option<PathBuf> = None;
    let mut reason: Option<String> = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--report" => {
                index += 1;
                report_path = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--report requires a path".to_string())?,
                );
            }
            "--reason" => {
                index += 1;
                reason = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--reason requires review text".to_string())?,
                );
            }
            other => return Err(format!("unknown baseline argument `{other}`")),
        }
        index += 1;
    }
    let report_path = report_path.ok_or_else(|| "baseline create requires --report".to_string())?;
    let reason = reason
        .filter(|reason| !reason.trim().is_empty())
        .ok_or_else(|| "baseline create requires nonempty --reason".to_string())?;
    let report = std::fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read report: {error}"))?;
    Ok(BaselineDocument::from_report_json(&report, &reason)?.to_json())
}
