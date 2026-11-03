use crate::{analyze_repository, ScanProfile, SourceDiscoveryOptions};

use super::test_support::TempRepo;

fn halstead_findings(path: &str, source: &str) -> Vec<String> {
    let repository = TempRepo::new("halstead-jsx");
    repository.write(path, source);
    analyze_repository(
        repository.path(),
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap()
    .findings
    .into_iter()
    .filter(|finding| finding.detector == "halstead_information_density")
    .map(|finding| finding.message)
    .collect()
}

/// A component whose body is `rows` lines of declarative markup and one
/// guarded default.
fn markup_component(rows: usize) -> String {
    let items = (0..rows)
        .map(|_| "      <li className=\"row\">entry</li>")
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "export function Page({{ title }}: {{ title?: string }}) {{\n  const heading = title ?? 'untitled';\n  return (\n    <ul id=\"rows\" title={{heading}}>\n{items}\n    </ul>\n  );\n}}\n"
    )
}

/// A component whose logic is `lines` dense arithmetic statements, with a
/// short markup return.
fn logic_component(lines: usize) -> String {
    let statements = (1..=lines)
        .map(|index| {
            let previous = index - 1;
            format!(
                "  const value_{index} = (input * {index} + value_{previous}) % ((input - {index}) | 1) ^ value_{previous};"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "export function Meter({{ input }}: {{ input: number }}) {{\n  const value_0 = input;\n{statements}\n  return <span className=\"meter\">{{value_{lines}}}</span>;\n}}\n"
    )
}

#[test]
fn markup_heavy_component_is_not_reported() {
    assert!(halstead_findings("src/Page.tsx", &markup_component(120)).is_empty());
}

#[test]
fn heavy_logic_in_a_component_is_still_reported() {
    let findings = halstead_findings("src/Meter.tsx", &logic_component(120));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("`Meter`"));
}
