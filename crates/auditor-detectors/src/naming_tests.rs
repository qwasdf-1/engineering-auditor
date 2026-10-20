use super::*;
use crate::ast_scan::AstScan;
use crate::config::DetectorConfig;
use crate::{AnalysisMethod, SourceRole};
use std::time::{SystemTime, UNIX_EPOCH};

fn detect(language: Language, relative: &str, source: &str, config: AuditorConfig) -> Vec<Finding> {
    detect_files(language, &[(relative, source)], config)
}

fn detect_files(
    language: Language,
    sources: &[(&str, &str)],
    config: AuditorConfig,
) -> Vec<Finding> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "engineering-auditor-naming-{}-{nonce}",
        std::process::id()
    ));
    let files = sources
        .iter()
        .map(|(relative, source)| {
            let absolute = root.join(relative);
            std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            std::fs::write(&absolute, source).unwrap();
            AuditedFile {
                absolute,
                relative: relative.into(),
                language: language.clone(),
                bytes: source.len() as u64,
                role: SourceRole::Production,
            }
        })
        .collect::<Vec<_>>();
    let cache = util::FileCache::new(&files);
    let mut repo_ast = auditor_ast::RepoAst::new();
    let structure = StructureIndex::build(AstScan::new(
        &files,
        AnalysisMethod::Ast,
        &cache,
        &mut repo_ast,
    ));
    let findings = naming_convention_findings(
        &files.iter().collect::<Vec<_>>(),
        &cache,
        &config,
        &structure,
    );
    std::fs::remove_dir_all(root).unwrap();
    findings
}

#[test]
fn rust_modules_allowing_naming_lints_cover_their_submodules() {
    let findings = detect_files(
        Language::Rust,
        &[
            ("src/lib.rs", "mod ffi;\nmod other;\n"),
            (
                "src/ffi/mod.rs",
                "#![allow(non_camel_case_types)]\nmod body;\npub struct hyper_io;\n",
            ),
            (
                "src/ffi/body.rs",
                "pub struct hyper_body;\npub fn Bad() {}\n",
            ),
            ("src/other.rs", "pub struct bad_name;\n"),
        ],
        AuditorConfig::default(),
    );
    let names = findings
        .iter()
        .map(|finding| {
            finding
                .message
                .split('`')
                .nth(1)
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["Bad", "bad_name"], "{names:?}");
}

#[test]
fn semantic_frontends_detect_java_and_go_declaration_names() {
    let java = detect(
        Language::Java,
        "src/BadNames.java",
        "public class badNames { public void DoWork() {} }\n",
        AuditorConfig::default(),
    );
    let go = detect(
        Language::Go,
        "src/service.go",
        "package service\nfunc process_item() {}\n",
        AuditorConfig::default(),
    );
    assert!(java.iter().any(|finding| finding.message.contains("Type")));
    assert!(java
        .iter()
        .any(|finding| finding.message.contains("Method")));
    assert!(go
        .iter()
        .any(|finding| finding.message.contains("Function")));
}

#[test]
fn java_constants_may_hold_digits() {
    let findings = detect(
        Language::Java,
        "src/Base64Variant.java",
        "public class Base64Variant {\n    public final static int BASE64_VALUE_INVALID = -1;\n    static final int http2Max = 2;\n}\n",
        AuditorConfig::default(),
    );
    let constants = findings
        .iter()
        .filter(|finding| finding.message.contains("Constant"))
        .map(|finding| finding.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(constants.len(), 1, "{constants:?}");
    assert!(constants[0].contains("http2Max"));
}

#[test]
fn kotlin_supplements_cover_properties_parameters_and_enum_entries() {
    let findings = detect(
        Language::Kotlin,
        "src/App.kt",
        "class User(Name: String)\nfun load_user() {}\nenum class Color { red, Green }\n",
        AuditorConfig::default(),
    );
    for expected in ["ConstructorParameter", "Function", "EnumEntry"] {
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains(expected)),
            "missing {expected} finding"
        );
    }
}

#[test]
fn kotlin_clean_supplements_are_silent() {
    let findings = detect(
        Language::Kotlin,
        "src/User.kt",
        "class User(name: String) : Base(Level.NONE)\nval done: Boolean = true\nfun User(id: Int): User = User(\"$id\")\nenum class Color { Red, GREEN }\n",
        AuditorConfig::default(),
    );
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn kotlin_length_limits_use_runtime_detector_configuration() {
    let mut config = AuditorConfig::default();
    config.detectors.insert(
        "naming_convention".to_string(),
        DetectorConfig {
            min_length: Some(2),
            max_length: Some(10),
            ..Default::default()
        },
    );
    let findings = detect(
        Language::Kotlin,
        "src/App.kt",
        "val a = 1\nval thisVariableNameIsTooLong = 2\n",
        config,
    );
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("short")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("long")));
}

#[test]
fn kotlin_hidden_file_prefix_still_matches_its_declaration() {
    let findings = detect(
        Language::Kotlin,
        "okhttp3/internal/dns/-DnsMessageReader.kt",
        "package okhttp3.internal.dns\n\nclass DnsMessageReader(val x: Int)\n",
        AuditorConfig::default(),
    );
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn kotlin_single_declaration_must_match_the_file_name() {
    let findings = detect(
        Language::Kotlin,
        "src/Wrong.kt",
        "class Right\n",
        AuditorConfig::default(),
    );
    assert!(findings
        .iter()
        .any(|finding| finding.title.contains("File name")));
}
