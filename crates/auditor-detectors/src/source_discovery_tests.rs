use super::*;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn rust_modules_declared_under_cfg_test_are_test_code() {
    let root = std::env::temp_dir().join(format!(
        "ea-cfg-test-modules-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (path, text) in [
        (
            "src/lib.rs",
            "pub mod engine;\n#[cfg(test)]\nmod testutil;\n#[cfg(test)] #[path = \"harness.rs\"] mod harness;\n",
        ),
        ("src/engine.rs", "pub fn run() {}\n"),
        ("src/testutil.rs", "mod fixtures;\npub fn helper() {}\n"),
        ("src/testutil/fixtures.rs", "pub fn sample() {}\n"),
        ("src/harness.rs", "pub fn drive() {}\n"),
    ] {
        let absolute = root.join(path);
        fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        fs::write(absolute, text).unwrap();
    }
    let roles = discover_audited_files_with_filter(
        &root,
        SourceDiscoveryOptions::default(),
        &ScopeFilter::default(),
    )
    .unwrap()
    .into_iter()
    .map(|file| (util::path_slash(&file.relative), file.role))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(roles["src/engine.rs"], SourceRole::Production);
    assert_eq!(roles["src/lib.rs"], SourceRole::Production);
    for test in [
        "src/testutil.rs",
        "src/testutil/fixtures.rs",
        "src/harness.rs",
    ] {
        assert_eq!(roles[test], SourceRole::Test, "{test}");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_tool_that_builds_its_own_image_is_production() {
    let root = std::env::temp_dir().join(format!(
        "ea-shipped-tool-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (path, text) in [
        ("extra/push/main.go", "package main\n"),
        ("extra/push/Dockerfile", "FROM scratch\n"),
        ("extra/release.js", "run()\n"),
        ("extra/Dockerfile", "FROM scratch\n"),
        ("hack/image/src/docker/Dockerfile", "FROM scratch\n"),
        ("hack/image/src/main.go", "package main\n"),
    ] {
        let absolute = root.join(path);
        fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        fs::write(absolute, text).unwrap();
    }
    let roles = discover_audited_files_with_filter(
        &root,
        SourceDiscoveryOptions::default(),
        &ScopeFilter::default(),
    )
    .unwrap()
    .into_iter()
    .map(|file| (util::path_slash(&file.relative), file.role))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(roles["extra/push/main.go"], SourceRole::Production);
    assert_eq!(roles["extra/release.js"], SourceRole::Tooling);
    assert_eq!(roles["hack/image/src/main.go"], SourceRole::Tooling);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_roles_follow_stable_path_precedence() {
    let cases = [
        ("src/lib.rs", SourceRole::Production),
        ("tooling/check.py", SourceRole::Tooling),
        ("crates/core/tests/api.rs", SourceRole::Test),
        ("src/security_tests.rs", SourceRole::Test),
        ("fixtures/python/sample.py", SourceRole::Fixture),
        ("src/capability_cases_rules.rs", SourceRole::Fixture),
        ("benches/scan.rs", SourceRole::Benchmark),
        (
            "metrics/src/main/java/ParseBenchmark.java",
            SourceRole::Benchmark,
        ),
        ("www/scripts/check.ts", SourceRole::Tooling),
        ("packages/zod/tools/gen.ts", SourceRole::Tooling),
        ("eslint.config.js", SourceRole::Tooling),
        ("www/docusaurus.config.ts", SourceRole::Tooling),
        ("okio-wasifilesystem/build.gradle.kts", SourceRole::Tooling),
        ("Makefile.js", SourceRole::Tooling),
        (
            "build-logic/src/main/kotlin/JavaModules.kt",
            SourceRole::Tooling,
        ),
        ("buildSrc/src/main/kotlin/Deps.kt", SourceRole::Tooling),
        ("src/app/app.config.ts", SourceRole::Production),
        ("examples/basic.rs", SourceRole::Example),
        ("src/generated/model.rs", SourceRole::Generated),
        ("docs/reference.md", SourceRole::Documentation),
        ("README.md", SourceRole::Documentation),
        ("references/donor/tests/sample.py", SourceRole::Reference),
        ("src/donor_parity_tests.rs", SourceRole::Test),
        ("packages/resolution/test-resolution.ts", SourceRole::Test),
        ("web/e2e/global-setup.ts", SourceRole::Test),
        (
            "test-framework/remote/src/main/java/SerializationUtil.java",
            SourceRole::Test,
        ),
        ("cmd/authelia-scripts/cmd/bootstrap.go", SourceRole::Tooling),
    ];
    for (path, expected) in cases {
        assert_eq!(source_role_for_path(Path::new(path)), expected, "{path}");
    }
}

#[test]
fn ordinary_names_containing_test_are_production() {
    assert_eq!(
        source_role_for_path(Path::new("src/latest_state.rs")),
        SourceRole::Production
    );
    assert_eq!(
        source_role_for_path(Path::new("src/contest.rs")),
        SourceRole::Production
    );
}

#[test]
fn discovery_honors_repository_ignore_files_without_hiding_hidden_source() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("auditor-ignore-{nonce}"));
    fs::create_dir_all(root.join("ignored/nested")).unwrap();
    fs::create_dir_all(root.join("nested/generated")).unwrap();
    fs::create_dir_all(root.join(".github")).unwrap();
    fs::write(root.join(".gitignore"), "ignored/\nignored.py\n").unwrap();
    fs::write(root.join("nested/.gitignore"), "generated/\n").unwrap();
    fs::write(root.join("ignored/nested/data.json"), "{}\n").unwrap();
    fs::write(root.join("ignored.py"), "ignored = True\n").unwrap();
    fs::write(root.join("nested/generated/data.json"), "{}\n").unwrap();
    fs::write(root.join("visible.py"), "visible = True\n").unwrap();
    fs::write(root.join(".github/check.py"), "hidden = False\n").unwrap();

    let paths = discover_sources(&root, SourceDiscoveryOptions::default())
        .unwrap()
        .into_iter()
        .map(|file| util::path_slash(&file.path))
        .collect::<Vec<_>>();

    assert_eq!(paths, vec![".github/check.py", "visible.py"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_honors_eaignore_and_underscore_role_directories() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("auditor-eaignore-{nonce}"));
    fs::create_dir_all(root.join("www/build")).unwrap();
    fs::create_dir_all(root.join("_examples/demo")).unwrap();
    fs::write(root.join(".eaignore"), "www/\n*.gen.go\n").unwrap();
    fs::write(root.join("www/build/site.ts"), "export {}\n").unwrap();
    fs::write(root.join("api.gen.go"), "package api\n").unwrap();
    fs::write(root.join("api.go"), "package api\n").unwrap();
    fs::write(root.join("_examples/demo/main.go"), "package main\n").unwrap();

    let files = discover_sources(&root, SourceDiscoveryOptions::default()).unwrap();
    let paths = files
        .iter()
        .map(|file| (util::path_slash(&file.path), file.role))
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![
            ("_examples/demo/main.go".to_string(), SourceRole::Example),
            ("api.go".to_string(), SourceRole::Production),
        ]
    );
    fs::remove_dir_all(root).unwrap();
}
