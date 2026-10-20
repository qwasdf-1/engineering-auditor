use auditor_core::json_escape;

use crate::AuditReport;

pub fn report_to_sarif(report: &AuditReport) -> String {
    let mut output = String::new();
    output.push_str("{\n");
    output.push_str("  \"$schema\": \"https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json\",\n");
    output.push_str("  \"version\": \"2.1.0\",\n");
    output.push_str("  \"runs\": [{\n");
    output.push_str(&format!(
        "    \"tool\": {{\"driver\": {{\"name\": \"Engineering Auditor\", \"version\": \"{}\", \"informationUri\": \"https://engineering-auditor.dev\"}}}},\n",
        env!("CARGO_PKG_VERSION")
    ));
    output.push_str(&format!(
        "    \"automationDetails\": {{\"id\": \"{}/{}\"}},\n",
        json_escape(&report.profile),
        json_escape(&report.repo_root)
    ));
    output.push_str("    \"results\": [\n");
    for (index, finding) in report.findings.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        output.push_str(&finding_to_sarif_result(finding, index));
    }
    output.push_str("\n    ],\n");
    output.push_str("    \"properties\": {\n");
    output.push_str(&format!(
        "      \"scanned_files\": {},\n",
        report.scanned_files
    ));
    output.push_str(&format!(
        "      \"profile\": \"{}\"\n",
        json_escape(&report.profile)
    ));
    output.push_str("    }\n");
    output.push_str("  }]\n");
    output.push('}');
    output
}

fn finding_to_sarif_result(finding: &auditor_core::Finding, _rule_index: usize) -> String {
    let level = match finding.severity.as_str() {
        "critical" | "high" => "error",
        "medium" => "warning",
        "low" => "note",
        "advisory" => "note",
        _ => "note",
    };

    let mut output = String::new();
    output.push_str("      {\n");
    output.push_str(&format!(
        "        \"ruleId\": \"{}\",\n",
        json_escape(&finding.detector)
    ));
    output.push_str(&format!("        \"level\": \"{level}\",\n"));
    output.push_str(&format!(
        "        \"message\": {{\"text\": \"{}\"}},\n",
        json_escape(&finding.message)
    ));
    if let Some(loc) = finding.locations.first() {
        output.push_str("        \"locations\": [{\n");
        output.push_str("          \"physicalLocation\": {\n");
        output.push_str("            \"artifactLocation\": {\n");
        output.push_str(&format!(
            "              \"uri\": \"{}\"\n",
            json_escape(&loc.path)
        ));
        output.push_str("            }");
        if let Some(line) = loc.line {
            output.push_str(",\n");
            output.push_str("            \"region\": {\n");
            output.push_str(&format!("              \"startLine\": {line}\n"));
            output.push_str("            }");
        }
        output.push_str("\n          }\n");
        output.push_str("        }],\n");
    }
    // Finding-ID v3 is line-free, so it is the stable fingerprint. Supplying
    // `primaryLocationLineHash` stops GitHub code scanning from substituting
    // its own line-content hash when tracking alerts across commits.
    output.push_str(&format!(
        "        \"partialFingerprints\": {{\"engineeringAuditor/v{version}\": \"{id}\", \"primaryLocationLineHash\": \"{id}\"}},\n",
        version = auditor_core::FINDING_ID_VERSION,
        id = json_escape(&finding.id)
    ));
    output.push_str(&format!(
        "        \"properties\": {{\"id\": \"{}\"}}\n",
        json_escape(&finding.id)
    ));
    output.push_str("      }");
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuditReport;
    use auditor_core::{Confidence, Severity};

    #[test]
    fn empty_report_sarif() {
        let mut report = AuditReport::empty("gating", ".");
        report.finalize();
        let sarif = report_to_sarif(&report);
        assert!(sarif.contains("\"version\": \"2.1.0\""));
        assert!(sarif.contains("\"results\""));
    }

    #[test]
    fn finding_in_sarif() {
        let mut report = AuditReport::empty("full", "/repo");
        report.findings.push(
            auditor_core::finding!(
                "test_detector",
                "structural",
                "Test",
                "test message",
                Severity::High,
                Confidence::High,
                "src/lib.rs",
                Some(5),
            )
            .with_recommendation("fix it"),
        );
        report.finalize();
        let sarif = report_to_sarif(&report);
        assert!(sarif.contains("\"ruleId\": \"test_detector\""));
        assert!(sarif.contains("\"level\": \"error\""));
        assert!(sarif.contains("\"uri\": \"src/lib.rs\""));
        assert!(sarif.contains("\"startLine\": 5"));
        let parsed: serde_json::Value = serde_json::from_str(&sarif).unwrap();
        let fingerprints = &parsed["runs"][0]["results"][0]["partialFingerprints"];
        let id = report.findings[0].id.as_str();
        assert_eq!(fingerprints["engineeringAuditor/v3"], id);
        assert_eq!(fingerprints["primaryLocationLineHash"], id);
    }

    #[test]
    fn severity_maps_correctly() {
        let mut report = AuditReport::empty("gating", ".");
        for sev in [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
            Severity::Advisory,
        ] {
            report.findings.push(auditor_core::finding!(
                "det",
                "cat",
                "t",
                "m",
                sev,
                Confidence::High,
                "f.rs",
                None,
            ));
        }
        report.finalize();
        let sarif = report_to_sarif(&report);
        assert!(sarif.contains("\"level\": \"error\""));
        assert!(sarif.contains("\"level\": \"warning\""));
        assert!(sarif.contains("\"level\": \"note\""));
    }
}
