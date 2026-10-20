use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::AuditedFile;

pub(crate) fn java_inefficient_string_buffer_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let rel = util::path_slash(&file.relative);
        if util::is_test_source_path(&file.relative) {
            continue;
        }
        // Constructing one is the choice to make; a `StringBuffer` parameter
        // can be an API's (`java.text.Format.format`), and Javadoc or a
        // `_streamStringBuffer` name is not a use.
        let lines = cache.text(file).lines().collect::<Vec<_>>();
        let masked = crate::block_strings::mask_block_strings(&crate::Language::Java, &lines);
        let Some(line) = masked.iter().position(|line| {
            let code = util::strip_comments(line);
            code.match_indices("new StringBuffer").any(|(index, _)| {
                code[index + "new StringBuffer".len()..]
                    .chars()
                    .next()
                    .is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
            })
        }) else {
            continue;
        };
        findings.push(
            auditor_core::finding!(
                "java_inefficient_string_buffer",
                "optimization",
                "StringBuffer usage (should be StringBuilder)",
                format!(
                    "{} constructs a StringBuffer at line {} instead of a StringBuilder",
                    file.relative.display(),
                    line + 1
                ),
                Severity::Low,
                Confidence::Medium,
                &rel,
                Some(line + 1),
            )
            .with_recommendation("Use StringBuilder instead of StringBuffer for better performance (non-threaded contexts)."),
        );
    }
    findings
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
            "engineering-auditor-java-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_java_file(root: &std::path::Path, rel: &str, text: &str) -> AuditedFile {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        AuditedFile {
            relative: std::path::PathBuf::from(rel),
            absolute: path,
            language: crate::Language::Java,
            bytes: text.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    #[test]
    fn java_inefficient_stringbuffer() {
        let root = temp_root("jisb");
        let file = make_java_file(&root, "src/Main.java", "public class Main {\n    public void build(String a, String b) {\n        StringBuffer sb = new StringBuffer();\n        sb.append(a);\n        sb.append(b);\n    }\n}\n");
        let cache = crate::util::FileCache::new(std::slice::from_ref(&file));
        let java_files = vec![&file];
        let findings = java_inefficient_string_buffer_findings(&java_files, &cache);
        assert!(findings
            .iter()
            .any(|f| f.detector == "java_inefficient_string_buffer"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn api_parameters_javadoc_and_lookalike_names_are_not_constructions() {
        let root = temp_root("jisb-neg");
        let file = make_java_file(
            &root,
            "src/Fmt.java",
            "/**\n * Replaces {@link StringBuffer} use.\n */\nclass Fmt extends java.text.Format {\n    private char[] _streamStringBuffer;\n    public StringBuffer format(Object o, StringBuffer to, java.text.FieldPosition p) { return to; }\n}\n",
        );
        let cache = crate::util::FileCache::new(std::slice::from_ref(&file));
        assert!(java_inefficient_string_buffer_findings(&[&file], &cache).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
