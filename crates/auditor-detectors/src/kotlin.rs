use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::AuditedFile;

pub(crate) fn kt_non_null_assertion_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let rel = util::path_slash(&file.relative);
        let text = cache.text(file);
        let is_test = util::is_test_source_path(&file.relative);
        // KDoc examples (` * body!!`) and raw strings are not code.
        let lines = text.lines().collect::<Vec<_>>();
        let masked = crate::block_strings::mask_block_strings(&crate::Language::Kotlin, &lines);
        for (line_number, line) in masked.iter().enumerate() {
            let code = util::strip_comments(line);
            let trimmed = code.trim();

            if trimmed.contains("!!") && !is_test {
                let clean = trimmed.replace("\"", "");
                if clean.contains("!!") {
                    findings.push(
                        auditor_core::finding!(
                            "kt_non_null_assertion",
                            "correctness",
                            "Non-null assertion (!!) in production code",
                            format!("Non-null assertion `!!` at line {}", line_number + 1),
                            Severity::Medium,
                            Confidence::High,
                            &rel,
                            Some(line_number + 1),
                        )
                        .with_recommendation(
                            "Use safe call (?.) or Elvis operator (?:) instead of !!",
                        ),
                    );
                }
            }
        }
    }
    findings
}

pub(crate) fn kt_require_check_misuse_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let rel = util::path_slash(&file.relative);
        for (line_number, line) in cache.text(file).lines().enumerate() {
            let code = util::strip_comments(line);
            let trimmed = code.trim();
            if ["require(", "check("]
                .iter()
                .filter_map(|call| trimmed.strip_prefix(call))
                .any(is_constant_first_argument)
            {
                findings.push(
                    auditor_core::finding!(
                        "kt_require_check_misuse",
                        "correctness",
                        "Misuse of require/check with constant",
                        format!(
                            "require/check called with constant false/null at line {}",
                            line_number + 1
                        ),
                        Severity::Medium,
                        Confidence::Medium,
                        &rel,
                        Some(line_number + 1),
                    )
                    .with_recommendation(
                        "Pass a proper predicate to require/check instead of a constant.",
                    ),
                );
            }
        }
    }
    findings
}

pub(crate) fn unsafe_type_cast_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        if util::is_test_source_path(&file.relative) {
            continue;
        }
        let rel = util::path_slash(&file.relative);
        // KDoc prose and string literals use the English word "as".
        let lines = cache.text(file).lines().collect::<Vec<_>>();
        let masked = crate::block_strings::mask_block_strings(&crate::Language::Kotlin, &lines);
        for (line_number, line) in masked.iter().enumerate() {
            let code = without_string_literals(&util::strip_comments(line));
            let trimmed = code.trim();
            // A hard `as` cast throws ClassCastException; `as?` is the safe
            // form the recommendation points to, and `import a.B as C` aliases.
            if !trimmed.starts_with("import ")
                && trimmed.split_whitespace().any(|token| token == "as")
            {
                findings.push(
                    auditor_core::finding!(
                        "unsafe_type_cast",
                        "correctness",
                        "Potential unsafe cast",
                        format!("Cast expression at line {}", line_number + 1),
                        Severity::Low,
                        Confidence::Medium,
                        &rel,
                        Some(line_number + 1),
                    )
                    .with_recommendation("Use safe cast with ?. or check type before casting.")
                    .with_evidence("pattern", line.trim()),
                );
            }
        }
    }
    findings
}

/// `line` with the contents of `"..."` literals removed.
fn without_string_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                out.push('"');
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        }
        out.push(character);
    }
    out
}

/// Whether the first argument of a call whose argument list starts at
/// `arguments` is the constant `false`, `true`, or `null` (`require(false)`),
/// not a predicate that mentions one (`check(exchange == null)`).
fn is_constant_first_argument(arguments: &str) -> bool {
    let mut depth = 0usize;
    let end = arguments
        .char_indices()
        .find(|(_, character)| match character {
            '(' | '[' | '{' => {
                depth += 1;
                false
            }
            ')' if depth == 0 => true,
            ',' if depth == 0 => true,
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => false,
        })
        .map_or(arguments.len(), |(index, _)| index);
    matches!(arguments[..end].trim(), "false" | "true" | "null")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-kotlin-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_kt_file(root: &std::path::Path, rel: &str, text: &str) -> AuditedFile {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        AuditedFile {
            relative: std::path::PathBuf::from(rel),
            absolute: path,
            language: crate::Language::Kotlin,
            bytes: text.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    #[test]
    fn kt_detects_non_null_assertion() {
        let root = temp_root("knn");
        let file = make_kt_file(
            &root,
            "src/Main.kt",
            "fun main() {\n    val name: String? = null\n    println(name!!.length)\n}\n",
        );
        let cache = crate::util::FileCache::new(std::slice::from_ref(&file));
        let kt_files = vec![&file];
        let findings = kt_non_null_assertion_findings(&kt_files, &cache);
        assert!(findings
            .iter()
            .any(|f| f.detector == "kt_non_null_assertion"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn kdoc_examples_are_not_assertions_and_predicates_are_not_constants() {
        let root = temp_root("kdoc");
        let file = make_kt_file(
            &root,
            "src/Reader.kt",
            "/**\n * val reader = MultipartReader(response.body!!) as a reader\n */\nclass Reader {\n  fun f(x: Any?) {\n    check(x == null) { \"set\" }\n    require(x != null || ok)\n    check(false)\n    log(\"same as before\")\n  }\n}\n",
        );
        let cache = crate::util::FileCache::new(std::slice::from_ref(&file));
        let files = vec![&file];
        assert!(kt_non_null_assertion_findings(&files, &cache).is_empty());
        assert!(unsafe_type_cast_findings(&files, &cache).is_empty());
        let misuse = kt_require_check_misuse_findings(&files, &cache);
        assert_eq!(
            misuse.iter().map(|f| f.primary_line()).collect::<Vec<_>>(),
            [Some(8)]
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
