use std::path::Path;

use auditor_core::Finding;

use crate::AuditedFile;

pub(super) fn finding_matches_catalog(
    detector: &str,
    finding: &Finding,
    files: &[AuditedFile],
) -> bool {
    let mut matched_location = false;
    for location in &finding.locations {
        let location_path = Path::new(&location.path);
        for file in files.iter().filter(|file| {
            file.relative == location_path || file.relative.starts_with(location_path)
        }) {
            matched_location = true;
            if crate::registry::detector_supports_language(detector, &file.language) {
                return true;
            }
        }
    }
    !matched_location
}
