use super::*;

pub(super) fn invalid_detector_output(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

pub(super) fn finding_matches_changed_scope(
    finding: &Finding,
    paths: &std::collections::BTreeSet<String>,
    ranges: &[ChangedRange],
) -> bool {
    let path = finding.primary_path();
    if !paths.is_empty() && !paths.contains(path) {
        return false;
    }
    let has_path_range = ranges.iter().any(|range| range.path == path);
    if !has_path_range {
        return paths.contains(path);
    }
    finding.primary_line().is_none_or(|line| {
        ranges
            .iter()
            .any(|range| range.path == path && range.start_line <= line && line <= range.end_line)
    })
}

pub(super) fn analysis_files(
    files: &[AuditedFile],
    method: AnalysisMethod,
) -> Vec<auditor_core::AnalysisFile> {
    files
        .iter()
        .map(|file| auditor_core::AnalysisFile {
            path: file.relative.to_string_lossy().replace('\\', "/"),
            language: file.language.as_str().to_string(),
            catalog_language: file.language.catalog_key().to_string(),
            bytes: file.bytes,
            source_role: file.role.as_str().to_string(),
            analysis_method: method.as_str().to_string(),
            extractor_version: auditor_core::EXTRACTOR_VERSION,
        })
        .collect()
}

/// A finding must keep its detector's catalog contract: a declared ID of the
/// emitting owner, its catalog category, and no scan-reserved identity key
/// (a structural anchor excepted).
pub(super) fn check_catalog_contract(
    active_spec: &crate::registry::DetectorSpec,
    finding: &Finding,
) -> std::io::Result<()> {
    let Some(emitted_spec) = crate::registry::detector_spec(&finding.detector) else {
        return Err(invalid_detector_output(format!(
            "detector owner emitted undeclared ID `{}`",
            finding.detector
        )));
    };
    if emitted_spec.owner != active_spec.owner {
        return Err(invalid_detector_output(format!(
            "detector `{}` emitted by {:?}, catalog owner is {:?}",
            finding.detector, active_spec.owner, emitted_spec.owner
        )));
    }
    if let Some(key) = finding
        .identity
        .keys()
        .find(|key| auditor_core::RESERVED_IDENTITY_KEYS.contains(&key.as_str()))
    {
        if key != auditor_core::IDENTITY_ANCHOR_KEY
            || !finding
                .anchor()
                .is_some_and(|anchor| anchor.starts_with("structural:"))
        {
            return Err(invalid_detector_output(format!(
                "detector `{}` set scan-reserved identity key `{key}`",
                finding.detector
            )));
        }
    }
    if finding.category != emitted_spec.category {
        return Err(invalid_detector_output(format!(
            "detector `{}` emitted category `{}`, catalog category is `{}`",
            finding.detector, finding.category, emitted_spec.category
        )));
    }
    Ok(())
}
