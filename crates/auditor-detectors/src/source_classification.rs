//! Source classification beyond the path's role components: .NET and
//! Kotlin test layouts, C++ headers told from C headers by content, and
//! parser-generator output checked in beside its grammar.

use std::fs;
use std::path::Path;

use super::has_component;

/// A .NET test project (`Newtonsoft.Json.Tests/`, `App.UnitTests/`) or a
/// Gradle or Kotlin Multiplatform test source set (`src/jvmTest/`,
/// `src/commonTest/`, `src/androidUnitTest/`).
pub(super) fn is_test_project(components: &[&str]) -> bool {
    let directories = &components[..components.len().saturating_sub(1)];
    directories.iter().enumerate().any(|(index, component)| {
        let lower = component.to_ascii_lowercase();
        [
            ".tests",
            ".test",
            ".unittests",
            ".integrationtests",
            ".specs",
            "testing-support",
            "test-support",
            "test-utils",
            "test-fixtures",
        ]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
            // A test module named with hyphens (`maven-tests`, `android-test-app`).
            || lower.split('-').skip(1).any(|part| matches!(part, "test" | "tests"))
            || (index > 0
                && directories[index - 1] == "src"
                && component
                    .strip_suffix("Test")
                    .is_some_and(|platform| platform.starts_with(|c: char| c.is_ascii_lowercase())))
    })
}

/// The project-layout part of a lowercased path: directories below a JVM
/// source root (`src/main/java/`, `src/commonMain/kotlin/`) spell the package
/// (`tools/jackson/core/`), so tooling keywords there do not apply.
pub(super) fn layout_components(components: &[String]) -> &[String] {
    const JVM_ROOTS: &[&str] = &["java", "kotlin", "scala", "groovy"];
    components
        .windows(3)
        .position(|window| window[0] == "src" && JVM_ROOTS.contains(&window[2].as_str()))
        .map_or(components, |start| &components[..start + 3])
}

const REFERENCE_COMPONENTS: &[&str] = &[
    "references",
    "reference",
    "vendor",
    "third_party",
    "thirdparty",
    "third-party",
    "3rdparty",
];

/// Reference material and vendored code: a `references/` or `third_party/`
/// tree, or a directory holding a released copy of another project.
pub(super) fn is_reference_path(components: &[String]) -> bool {
    has_component(components, REFERENCE_COMPONENTS)
        || is_versioned_copy(&components[..components.len().saturating_sub(1)])
}

/// A directory holding a released copy of another project, named for its
/// release (`test/googletest-1.13.0/`, `deps/lua-5.4.6/`): vendored code,
/// like a `third_party/` tree.
fn is_versioned_copy(directories: &[String]) -> bool {
    directories.iter().any(|directory| {
        directory.rsplit_once('-').is_some_and(|(name, version)| {
            let groups: Vec<&str> = version.trim_start_matches('v').split('.').collect();
            name.starts_with(|c: char| c.is_ascii_alphabetic())
                && groups.len() >= 2
                && groups
                    .iter()
                    .all(|group| !group.is_empty() && group.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

/// A JVM or .NET test class file (`BufferTest.kt`, `JArrayTests.cs`,
/// `AbstractFileSystemTest.kt`): the class-name convention JUnit and xUnit
/// discovery use. `Contest.java` is not one.
pub(super) fn is_test_class_file(components: &[&str]) -> bool {
    let Some(stem) = components
        .last()
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
    else {
        return false;
    };
    ["Test", "Tests"].iter().any(|suffix| {
        stem.strip_suffix(suffix).is_some_and(|class| {
            class
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        })
    })
}

/// A `.h` header written in C++: `.h` serves both languages, so, as GitHub
/// Linguist does, the content decides. A namespace, template, class access
/// specifier, or `std::` name marks C++; `extern "C"` guards do not, since C
/// headers carry them for C++ callers.
pub(super) fn header_is_cpp(path: &Path) -> bool {
    const PREFIX: usize = 64 * 1024;
    if path.extension().and_then(|ext| ext.to_str()) != Some("h") {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(PREFIX)]);
    text.lines().map(str::trim_start).any(|line| {
        line.starts_with("namespace ")
            || line.starts_with("template <")
            || line.starts_with("template<")
            || matches!(line, "public:" | "private:" | "protected:")
            || (line.starts_with("class ") && line.trim_end().ends_with(['{', ':']))
            || line.contains("std::")
    })
}

/// Source a generator wrote and checked in: a parser or scanner generator's
/// banner (Bison, yacc, flex), a webpack bundle's runtime, or a
/// generated-file notice ("autogenerated", "Generated automatically by",
/// `@generated`, Go's "Code generated ... DO NOT EDIT"), in the first lines;
/// or minified JavaScript, whose lines average over
/// [`MINIFIED_LINE_LENGTH`] bytes; as GitHub Linguist reads them.
pub(super) fn is_generated_output(path: &Path) -> bool {
    const PREFIX: usize = 1024;
    const BANNERS: &[&str] = &[
        "A Bison parser, made by GNU Bison",
        "A lexical scanner generated by flex",
        "yacc parser generated by",
        "@generated",
        "webpackBootstrap",
        "webpackJsonp",
        "webpackChunk",
    ];
    const NOTICES: &[&str] = &["autogenerated", "auto-generated", "generated automatically"];
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(PREFIX)]);
    let lower = text.to_ascii_lowercase();
    BANNERS.iter().any(|banner| text.contains(banner))
        || NOTICES.iter().any(|notice| lower.contains(notice))
        || (lower.contains("code generated") && text.contains("DO NOT EDIT"))
        || is_minified_javascript(path, &bytes)
}

/// Linguist's bound: hand-written code keeps lines far shorter.
const MINIFIED_LINE_LENGTH: usize = 110;

/// Bundlers emit kilobytes; a short file of long lines is a hand-written
/// snippet.
const MINIFIED_MIN_BYTES: usize = 2048;

fn is_minified_javascript(path: &Path, bytes: &[u8]) -> bool {
    let javascript = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "js" | "mjs" | "cjs"));
    let lines = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .count();
    javascript
        && bytes.len() >= MINIFIED_MIN_BYTES
        && lines > 0
        && bytes.len() / lines > MINIFIED_LINE_LENGTH
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dotnet_test_projects_and_kotlin_test_source_sets_are_test_code() {
        for (path, role) in [
            (
                "Src/Newtonsoft.Json.Tests/Linq/JArrayTests.cs",
                SourceRole::Test,
            ),
            (
                "okio/src/jvmTest/kotlin/okio/BufferTest.kt",
                SourceRole::Test,
            ),
            (
                "okio/src/commonTest/kotlin/okio/Utf8Test.kt",
                SourceRole::Test,
            ),
            (
                "okio/src/commonMain/kotlin/okio/Buffer.kt",
                SourceRole::Production,
            ),
            ("Src/Newtonsoft.Json/Linq/JArray.cs", SourceRole::Production),
            (
                "samples/src/jvmMain/kotlin/okio/samples/GoldenValue.kt",
                SourceRole::Example,
            ),
            ("src/latest/contest.rs", SourceRole::Production),
            ("src/main/java/org/Contest.java", SourceRole::Production),
            (
                "okio-testing-support/src/commonMain/kotlin/okio/TestingCommon.kt",
                SourceRole::Test,
            ),
            (
                "lib/src/main/kotlin/AbstractFileSystemTest.kt",
                SourceRole::Test,
            ),
            (
                "src/main/java/tools/jackson/core/Base64Variant.java",
                SourceRole::Production,
            ),
            (
                "src/test/java/tools/jackson/core/ParserTest.java",
                SourceRole::Test,
            ),
            ("tools/src/main/java/Gen.java", SourceRole::Tooling),
            ("build/release-adapter.ts", SourceRole::Tooling),
            ("support/release.py", SourceRole::Tooling),
            ("extra/release/lib.mjs", SourceRole::Tooling),
            ("hack/update-codegen.go", SourceRole::Tooling),
            ("src/extra/parse.rs", SourceRole::Production),
            ("extra/push-examples/python/index.py", SourceRole::Example),
            ("src/mock-examples.ts", SourceRole::Production),
            ("lib/Support/Path.cpp", SourceRole::Production),
            ("integration/server.js", SourceRole::Test),
            ("src/build/plan.py", SourceRole::Production),
            ("src/integration/client.py", SourceRole::Production),
            ("single_include/nlohmann/json.hpp", SourceRole::Generated),
            (
                "include/nlohmann/thirdparty/hedley/hedley.hpp",
                SourceRole::Reference,
            ),
            (
                "test/googletest-1.13.0/googletest/test/gtest_unittest.cc",
                SourceRole::Reference,
            ),
            ("src/lua-5.4/lapi.c", SourceRole::Reference),
            ("src/utf-8/decode.c", SourceRole::Production),
            ("pkg/api-v2/client.go", SourceRole::Production),
            ("lib/googletest-1.13.0.cc", SourceRole::Production),
            (
                "maven-tests/src/main/java/SampleHttpClient.java",
                SourceRole::Test,
            ),
            (
                "android-test-app/src/main/kotlin/MainActivity.kt",
                SourceRole::Test,
            ),
            ("ossfuzz/round_trip_fuzzer.c", SourceRole::Test),
            ("src/Snippets/Docs/Retry.cs", SourceRole::Example),
            ("__performance_tests__/todo.mjs", SourceRole::Benchmark),
            (
                "perf-testing/immutability-benchmarks.mjs",
                SourceRole::Benchmark,
            ),
            (
                "okhttp-testing-support/src/main/kotlin/Rule.kt",
                SourceRole::Test,
            ),
            ("latest-news/src/main.rs", SourceRole::Production),
            (
                "src/main/java/org/jsoup/examples/ListLinks.java",
                SourceRole::Example,
            ),
        ] {
            assert_eq!(source_role_for_path(Path::new(path)), role, "{path}");
        }
    }

    #[test]
    fn parser_generator_output_is_generated_source() {
        let root = std::env::temp_dir().join(format!(
            "ea-parser-generator-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (path, text) in [
            ("src/parser.c", "/* A Bison parser, made by GNU Bison 3.8.2.  */\nint yyparse(void) { return 0; }\n"),
            ("src/lexer.c", "#line 2 \"src/lexer.c\"\n/* A lexical scanner generated by flex */\nint yylex(void) { return 0; }\n"),
            ("src/compile.c", "/* Compiles the tree the Bison parser builds. */\nint compile(void) { return 0; }\n"),
            ("crc32.h", "/* crc32.h -- tables for rapid CRC calculation\n * Generated automatically by crc32.c\n */\n"),
            ("lib/config-validator.js", "// This file is autogenerated by build/build-validation.js, do not edit\n"),
            ("api/types.go", "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage api\n"),
            ("lib/notes.js", "// Do not edit without updating the docs.\nmodule.exports = {};\n"),
        ] {
            let absolute = root.join(path);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, text).unwrap();
        }
        let minified = "function a(b){return b+1}var c=a(2);".repeat(80);
        let wide = format!("const message = '{}';\n", "x".repeat(200));
        for (path, text) in [
            ("static/main-CA4.js", minified.clone()),
            ("static/main-CA4.ts", minified),
            ("static/vendor.js", "(window[\"webpackJsonp\"] = window[\"webpackJsonp\"] || []).push([[\"vendor\"],{\n".to_string()),
            ("lib/wide.js", format!("{wide}{}", "let x = 1;\n".repeat(40))),
            ("lib/snippet.js", "function a(b){return b+1}var c=a(2);".repeat(20)),
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
        assert_eq!(roles["src/parser.c"], SourceRole::Generated);
        assert_eq!(roles["src/lexer.c"], SourceRole::Generated);
        assert_eq!(roles["src/compile.c"], SourceRole::Production);
        assert_eq!(roles["crc32.h"], SourceRole::Generated);
        assert_eq!(roles["lib/config-validator.js"], SourceRole::Generated);
        assert_eq!(roles["api/types.go"], SourceRole::Generated);
        assert_eq!(roles["lib/notes.js"], SourceRole::Production);
        assert_eq!(roles["static/main-CA4.js"], SourceRole::Generated);
        assert_eq!(roles["static/main-CA4.ts"], SourceRole::Production);
        assert_eq!(roles["static/vendor.js"], SourceRole::Generated);
        assert_eq!(roles["lib/wide.js"], SourceRole::Production);
        assert_eq!(roles["lib/snippet.js"], SourceRole::Production);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cpp_headers_are_told_from_c_headers_by_content() {
        let root = std::env::temp_dir().join(format!(
            "ea-header-language-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (path, text) in [
            (
                "include/base.h",
                "namespace fmt {\ntemplate <typename T> struct formatter;\n}\n",
            ),
            (
                "include/api.h",
                "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint uv_run(void);\n",
            ),
        ] {
            let absolute = root.join(path);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, text).unwrap();
        }
        let languages = discover_audited_files_with_filter(
            &root,
            SourceDiscoveryOptions::default(),
            &ScopeFilter::default(),
        )
        .unwrap()
        .into_iter()
        .map(|file| (util::path_slash(&file.relative), file.language))
        .collect::<BTreeMap<_, _>>();
        assert_eq!(languages["include/base.h"], Language::Cpp);
        assert_eq!(languages["include/api.h"], Language::C);
        fs::remove_dir_all(root).unwrap();
    }
}
