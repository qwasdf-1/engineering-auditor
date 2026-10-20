use std::fmt::Write;
use std::path::Path;

use auditor_core::json_escape;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LicenseTier {
    PermissiveCompatible,
    WeakCopyleftReferenceOnly,
    StrongCopyleftReferenceOnly,
    UnknownReferenceOnly,
    InternalReference,
}

impl LicenseTier {
    pub fn allows_direct_port(self) -> bool {
        matches!(self, Self::PermissiveCompatible | Self::InternalReference)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PermissiveCompatible => "permissive_compatible",
            Self::WeakCopyleftReferenceOnly => "weak_copyleft_reference_only",
            Self::StrongCopyleftReferenceOnly => "strong_copyleft_reference_only",
            Self::UnknownReferenceOnly => "unknown_reference_only",
            Self::InternalReference => "internal_reference",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "permissive_compatible" => Some(Self::PermissiveCompatible),
            "weak_copyleft_reference_only" => Some(Self::WeakCopyleftReferenceOnly),
            "strong_copyleft_reference_only" => Some(Self::StrongCopyleftReferenceOnly),
            "unknown_reference_only" => Some(Self::UnknownReferenceOnly),
            "internal_reference" => Some(Self::InternalReference),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonorReference {
    pub id: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    pub snapshot: &'static str,
    pub license: &'static str,
    pub license_tier: LicenseTier,
    pub use_mode: &'static str,
    pub runtime_dependency: bool,
    pub required: bool,
    pub expected_paths: &'static [&'static str],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonorStatus {
    pub donor: DonorReference,
    pub status: &'static str,
    pub path_exists: bool,
    pub missing_expected_paths: Vec<String>,
    pub allows_direct_port: bool,
    pub policy_violations: Vec<String>,
}

pub fn baseline_donors() -> Vec<DonorReference> {
    vec![
        DonorReference {
            id: "agent-slop-lint",
            name: "agent-slop-lint",
            source: "references/agent-slop-lint/src",
            snapshot: "v1.0.3:d6a80bda0717cb2e3de024430b63bcb203fc9fca",
            license: "Apache-2.0",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "adapter_first_parity_oracle",
            runtime_dependency: false,
            required: false,
            expected_paths: &["tests/test_rules", "cli/slop/rules"],
        },
        DonorReference {
            id: "rust-clippy",
            name: "Rust Clippy",
            source: "installed:cargo clippy",
            snapshot: "operator-installed",
            license: "MIT OR Apache-2.0",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "installed_optional_oracle",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "ruff",
            name: "Ruff (Python linter)",
            source: "references/ruff",
            snapshot: "main:e6f1513",
            license: "MIT",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "eslint",
            name: "ESLint (JavaScript/TypeScript linter)",
            source: "references/eslint",
            snapshot: "main:0493f53",
            license: "MIT",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "semgrep",
            name: "Semgrep (multi-language pattern matching)",
            source: "references/semgrep",
            snapshot: "main:816abbf",
            license: "LGPL-2.1",
            license_tier: LicenseTier::WeakCopyleftReferenceOnly,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "pmd",
            name: "PMD (Java static analysis + CPD)",
            source: "references/pmd",
            snapshot: "main:e30a701",
            license: "Apache-2.0",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "detekt",
            name: "Detekt (Kotlin static analysis)",
            source: "references/detekt",
            snapshot: "main:f60d921",
            license: "Apache-2.0",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "golangci-lint",
            name: "Golangci-Lint (Go meta-linter)",
            source: "references/golangci-lint",
            snapshot: "main:12b97aa",
            license: "GPL-3.0",
            license_tier: LicenseTier::StrongCopyleftReferenceOnly,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "pylint",
            name: "Pylint (Python static analysis)",
            source: "references/pylint",
            snapshot: "main:554b1d1",
            license: "GPL-2.0",
            license_tier: LicenseTier::StrongCopyleftReferenceOnly,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "jscpd",
            name: "JSCPD (copy-paste detector)",
            source: "references/jscpd",
            snapshot: "master:0d4af2a",
            license: "MIT",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "deptrac",
            name: "Deptrac (PHP architectural dependency checker)",
            source: "references/deptrac",
            snapshot: "main:9fbc71c",
            license: "MIT",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "bandit",
            name: "Bandit (Python security linter)",
            source: "references/bandit",
            snapshot: "main:3b5e651",
            license: "Apache-2.0",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "algorithm_donor",
            runtime_dependency: false,
            required: false,
            expected_paths: &[],
        },
        DonorReference {
            id: "import-linter",
            name: "Import Linter (Python architecture contracts)",
            source: "references/import-linter",
            snapshot: "main:f544debbb0efe10092cd387032ea76b94a0acee0",
            license: "BSD-2-Clause",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &["pyproject.toml", "LICENSE"],
        },
        DonorReference {
            id: "dependency-cruiser",
            name: "Dependency Cruiser (JavaScript/TypeScript architecture rules)",
            source: "references/dependency-cruiser",
            snapshot: "main:26dffc05710bc7663315e4d70863dd46c009c48e",
            license: "MIT",
            license_tier: LicenseTier::PermissiveCompatible,
            use_mode: "parity_reference",
            runtime_dependency: false,
            required: false,
            expected_paths: &["package.json", "LICENSE"],
        },
    ]
}

pub fn donor_check_summary_json() -> String {
    donor_check_summary_json_for(Path::new("."))
}

pub fn donor_sync_summary_json() -> String {
    donor_sync_summary_json_for(Path::new("."))
}

pub fn donor_check_summary_json_for(repo_root: &Path) -> String {
    let manifest_validation = validate_manifest_at(repo_root);
    let statuses = donor_statuses(repo_root);
    let passed = manifest_validation.is_empty()
        && statuses
            .iter()
            .all(|status| status.status == "present" || status.status == "optional_skip");
    summary_json("check", passed, &manifest_validation, &statuses)
}

pub fn donor_sync_summary_json_for(repo_root: &Path) -> String {
    let manifest_validation = validate_manifest_at(repo_root);
    let statuses = donor_statuses(repo_root);
    summary_json(
        "sync",
        manifest_validation.is_empty(),
        &manifest_validation,
        &statuses,
    )
}

pub fn validate_manifest_text(text: &str) -> Vec<String> {
    const REQUIRED: [(&str, &str); 3] = [
        ("\"schema_version\"", "missing_schema_version"),
        (
            "\"policy\": \"docs/donor-policy.md\"",
            "missing_or_wrong_policy",
        ),
        (
            "\"references_directory\": \"references\"",
            "missing_or_wrong_references_directory",
        ),
    ];
    let mut violations = REQUIRED
        .iter()
        .filter(|(needle, _)| !text.contains(needle))
        .map(|(_, violation)| violation.to_string())
        .collect::<Vec<_>>();
    for donor in baseline_donors() {
        violations.extend(donor_violations(text, &donor));
    }
    let direct_port = text.contains("\"use_mode\": \"direct_port\"");
    violations.extend(
        [
            "weak_copyleft_reference_only",
            "strong_copyleft_reference_only",
            "unknown_reference_only",
        ]
        .iter()
        .filter(|tier| direct_port && text.contains(&format!("\"license_tier\": \"{tier}\"")))
        .map(|tier| format!("restricted_tier_direct_port:{tier}")),
    );
    violations
}

fn donor_violations(text: &str, donor: &DonorReference) -> Vec<String> {
    let listed = text.contains(&format!("\"id\": \"{}\"", donor.id));
    let tier = text.contains(&format!(
        "\"license_tier\": \"{}\"",
        donor.license_tier.as_str()
    ));
    [
        (!listed, "missing_donor"),
        (!tier, "license_tier_mismatch"),
        (
            donor.runtime_dependency && listed,
            "runtime_dependency_not_allowed",
        ),
    ]
    .into_iter()
    .filter(|(violated, _)| *violated)
    .map(|(_, violation)| format!("{violation}:{}", donor.id))
    .collect()
}

fn validate_manifest_at(repo_root: &Path) -> Vec<String> {
    let manifest_path = repo_root.join("auditor-donors.lock.json");
    match std::fs::read_to_string(&manifest_path) {
        Ok(text) => validate_manifest_text(&text),
        Err(error) => vec![format!("manifest_unreadable:{error}")],
    }
}

fn donor_statuses(repo_root: &Path) -> Vec<DonorStatus> {
    baseline_donors()
        .into_iter()
        .map(|donor| {
            let path_exists = donor_source_exists(repo_root, &donor);
            let missing_expected_paths = missing_expected_paths(repo_root, &donor);
            let status = if path_exists && missing_expected_paths.is_empty() {
                "present"
            } else if donor.required {
                "missing_required"
            } else {
                "optional_skip"
            };
            let mut policy_violations = Vec::new();
            if !donor.license_tier.allows_direct_port() && donor.use_mode == "direct_port" {
                policy_violations.push("restricted_tier_direct_port".to_string());
            }
            DonorStatus {
                allows_direct_port: donor.license_tier.allows_direct_port(),
                donor,
                status,
                path_exists,
                missing_expected_paths,
                policy_violations,
            }
        })
        .collect()
}

fn donor_source_exists(repo_root: &Path, donor: &DonorReference) -> bool {
    if donor.source.starts_with("installed:") {
        return true;
    }
    repo_root.join(donor.source).exists()
}

fn missing_expected_paths(repo_root: &Path, donor: &DonorReference) -> Vec<String> {
    if donor.source.starts_with("installed:") || !repo_root.join(donor.source).exists() {
        return donor
            .expected_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect();
    }
    donor
        .expected_paths
        .iter()
        .filter(|path| !repo_root.join(donor.source).join(path).exists())
        .map(|path| (*path).to_string())
        .collect()
}

fn summary_json(
    command: &str,
    passed: bool,
    manifest_violations: &[String],
    statuses: &[DonorStatus],
) -> String {
    let mut output = String::from("{\n");
    output.push_str("  \"schema_version\": 1,\n");
    output.push_str(&format!("  \"command\": \"{}\",\n", json_escape(command)));
    output.push_str("  \"policy\": \"docs/donor-policy.md\",\n");
    output.push_str(&format!("  \"passed\": {},\n", passed));
    output.push_str("  \"manifest_violations\": [");
    for (index, violation) in manifest_violations.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        write!(output, "\"{}\"", json_escape(violation)).unwrap();
    }
    output.push_str("],\n");
    output.push_str("  \"donors\": [");
    for (index, status) in statuses.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&donor_status_json(status));
    }
    output.push_str("]\n");
    output.push('}');
    output
}

fn donor_status_json(status: &DonorStatus) -> String {
    let donor = &status.donor;
    let mut output = String::from("{");
    output.push_str(&format!("\"id\": \"{}\"", json_escape(donor.id)));
    output.push_str(&format!(", \"name\": \"{}\"", json_escape(donor.name)));
    output.push_str(&format!(", \"source\": \"{}\"", json_escape(donor.source)));
    output.push_str(&format!(
        ", \"snapshot\": \"{}\"",
        json_escape(donor.snapshot)
    ));
    output.push_str(&format!(
        ", \"license\": \"{}\"",
        json_escape(donor.license)
    ));
    output.push_str(&format!(
        ", \"license_tier\": \"{}\"",
        donor.license_tier.as_str()
    ));
    output.push_str(&format!(
        ", \"use_mode\": \"{}\"",
        json_escape(donor.use_mode)
    ));
    output.push_str(&format!(
        ", \"runtime_dependency\": {}",
        donor.runtime_dependency
    ));
    output.push_str(&format!(", \"required\": {}", donor.required));
    output.push_str(&format!(", \"status\": \"{}\"", status.status));
    output.push_str(&format!(", \"path_exists\": {}", status.path_exists));
    output.push_str(&format!(
        ", \"allows_direct_port\": {}",
        status.allows_direct_port
    ));
    output.push_str(", \"missing_expected_paths\": [");
    for (index, path) in status.missing_expected_paths.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        write!(output, "\"{}\"", json_escape(path)).unwrap();
    }
    output.push_str("], \"policy_violations\": [");
    for (index, violation) in status.policy_violations.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        write!(output, "\"{}\"", json_escape(violation)).unwrap();
    }
    output.push_str("]}");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_tiers_are_reference_only() {
        assert!(!LicenseTier::StrongCopyleftReferenceOnly.allows_direct_port());
        assert!(!LicenseTier::WeakCopyleftReferenceOnly.allows_direct_port());
        assert!(!LicenseTier::UnknownReferenceOnly.allows_direct_port());
        assert!(LicenseTier::PermissiveCompatible.allows_direct_port());
    }

    #[test]
    fn manifest_validation_rejects_restricted_direct_ports() {
        let manifest = r#"{
          "schema_version": 1,
          "policy": "docs/donor-policy.md",
          "references_directory": "references",
          "donors": [
            {"id": "agent-slop-lint", "license_tier": "permissive_compatible", "use_mode": "adapter_first_parity_oracle"},
            {"id": "rust-clippy", "license_tier": "permissive_compatible", "use_mode": "installed_optional_oracle"},
            {"id": "ruff", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "eslint", "license_tier": "permissive_compatible", "use_mode": "parity_reference"},
            {"id": "semgrep", "license_tier": "weak_copyleft_reference_only", "use_mode": "parity_reference"},
            {"id": "pmd", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "detekt", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "golangci-lint", "license_tier": "strong_copyleft_reference_only", "use_mode": "parity_reference"},
            {"id": "pylint", "license_tier": "strong_copyleft_reference_only", "use_mode": "parity_reference"},
            {"id": "jscpd", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "deptrac", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "bandit", "license_tier": "permissive_compatible", "use_mode": "algorithm_donor"},
            {"id": "import-linter", "license_tier": "permissive_compatible", "use_mode": "parity_reference"},
            {"id": "dependency-cruiser", "license_tier": "permissive_compatible", "use_mode": "parity_reference"},
            {"id": "bad-gpl", "license_tier": "strong_copyleft_reference_only", "use_mode": "direct_port"}
          ]
        }"#;

        let violations = validate_manifest_text(manifest);

        assert!(violations
            .iter()
            .any(|violation| violation
                == "restricted_tier_direct_port:strong_copyleft_reference_only"));
    }

    #[test]
    fn baseline_donor_check_names_optional_skips() {
        let summary = donor_check_summary_json_for(Path::new("."));
        assert!(summary.contains("agent-slop-lint"));
        assert!(summary.contains("optional_skip") || summary.contains("present"));
    }
}
