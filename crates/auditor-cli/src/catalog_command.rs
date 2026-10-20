use auditor_detectors::{detector_registry, ScanProfile};

pub(crate) fn run_catalog(args: &[String]) -> Result<String, String> {
    let mut profile = ScanProfile::Full;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                profile = ScanProfile::parse(
                    args.get(index)
                        .ok_or_else(|| "--profile requires gating|full".to_string())?,
                )?;
            }
            "--format" => {
                index += 1;
                if args.get(index).map(String::as_str) != Some("json") {
                    return Err("catalog output format must be json".to_string());
                }
            }
            other => return Err(format!("unknown catalog argument `{other}`")),
        }
        index += 1;
    }
    let detectors = detector_registry(profile)
        .into_iter()
        .map(|detector| {
            serde_json::json!({
                "category": detector.category,
                "context_roles": detector.context_roles.iter().map(|role| role.as_str()).collect::<Vec<_>>(),
                "default_severity": detector.default_severity.as_str(),
                "emission_roles": detector.emission_roles.iter().map(|role| role.as_str()).collect::<Vec<_>>(),
                "id": detector.id,
                "language_support": detector.language_support.iter().map(|claim| serde_json::json!({
                    "language": claim.language,
                    "limitation": claim.limitation,
                    "mode": claim.mode.as_str(),
                })).collect::<Vec<_>>(),
                "layer": detector.layer,
                "maturity": detector.maturity,
                "profile": detector.profile,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "detectors": detectors,
        "profile": profile.as_str(),
        "schema_version": 1,
    }))
    .map_err(|error| format!("failed to serialize detector catalog: {error}"))
}
