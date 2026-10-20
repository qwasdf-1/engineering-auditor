use std::path::Path;

use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::AstScan;
use crate::config::AuditorConfig;
use crate::{util, AuditedFile};

pub(crate) fn source_duplicate_finding(
    title: &'static str,
    message: impl Into<String>,
    severity: Severity,
    confidence: Confidence,
    path: impl Into<String>,
    line: Option<usize>,
) -> Finding {
    auditor_core::finding!(
        "source_duplicate_detection",
        "structural",
        title,
        message,
        severity,
        confidence,
        path,
        line,
    )
}

pub(crate) fn large_file_findings(
    root: &Path,
    files: &[AuditedFile],
    config: &AuditorConfig,
    cache: &util::FileCache,
) -> Vec<Finding> {
    let advisory = config.threshold_for("large_file", crate::LARGE_FILE_ADVISORY_LINES);
    let strict = config.threshold_for("large_file", crate::LARGE_FILE_STRICT_LINES);
    let mut findings = Vec::new();
    for file in files {
        let Some(category) = util::category_for(file) else {
            continue;
        };
        let lines = cache.line_count(file);
        if lines < advisory {
            continue;
        }
        findings.push(
            auditor_core::finding!(
                "large_file",
                "structural",
                "Large file exceeds v0 audit threshold",
                format!("{category} file has {lines} nonblank lines."),
                severity_for_threshold(lines, strict),
                Confidence::High,
                util::relative_slash(root, &file.absolute),
                None,
            )
            .with_recommendation(
                "Audit for true deletion, consolidation, or local simplification; split only when net complexity drops.",
            )
            .with_evidence("lines", lines)
            .with_evidence("advisory_threshold", advisory)
            .with_evidence("strict_threshold", strict)
            .with_evidence(
                "tier",
                if lines >= strict {
                    "violation"
                } else {
                    "advisory"
                },
            ),
        );
    }
    findings
}

pub(crate) fn source_duplicate_detection_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    crate::duplication::copy_paste_detect(scan)
}

fn severity_for_threshold(actual: usize, strict: usize) -> Severity {
    if actual >= strict {
        Severity::High
    } else {
        Severity::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuditorConfig;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-bloat-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_file(root: &Path, relative: &str, source: &str) -> AuditedFile {
        let absolute = root.join(relative);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(&absolute, source).unwrap();
        AuditedFile {
            absolute,
            relative: relative.into(),
            language: crate::Language::Rust,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    #[test]
    fn large_file_detects_above_advisory() {
        let root = temp_root("large-file");
        let mut content = String::from("fn main() {\n");
        for index in 0..1100 {
            content.push_str(&format!("    let value_{index} = {index};\n"));
        }
        content.push_str("}\n");
        let file = make_file(&root, "src/main.rs", &content);
        let cache = util::FileCache::new(std::slice::from_ref(&file));
        let findings = large_file_findings(
            &root,
            std::slice::from_ref(&file),
            &AuditorConfig::default(),
            &cache,
        );
        assert!(findings
            .iter()
            .any(|finding| finding.detector == "large_file"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn large_file_below_advisory_is_silent() {
        let root = temp_root("small-file");
        let file = make_file(&root, "src/main.rs", "fn main() {}\n");
        let cache = util::FileCache::new(std::slice::from_ref(&file));
        assert!(large_file_findings(
            &root,
            std::slice::from_ref(&file),
            &AuditorConfig::default(),
            &cache,
        )
        .is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
