use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::AuditedFile;

pub(crate) use crate::dead_stores::unused_assignment_findings;

/// Go correctness: log.Fatal (other patterns absorbed by shared detectors)
pub(crate) fn go_log_fatal_findings(
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let rel = util::path_slash(&file.relative);
        let text = cache.text(file);
        for (line_number, line) in text.lines().enumerate() {
            let code = util::strip_comments(line);
            let lower = code.to_ascii_lowercase();
            if lower.contains("log.fatal(") {
                findings.push(
                    auditor_core::finding!(
                        "go_log_fatal",
                        "correctness",
                        "log.Fatal call in Go production code",
                        format!("Go source calls log.Fatal at line {}", line_number + 1),
                        Severity::High,
                        Confidence::High,
                        &rel,
                        Some(line_number + 1),
                    )
                    .with_recommendation("Return an error instead of calling log.Fatal.")
                    .with_evidence("pattern", line.trim()),
                );
            }
        }
    }
    findings
}

/// golangci-lint inspired: detect repeated string literals that should be constants
pub(crate) fn go_repeated_string_literal_findings(
    _root: &std::path::Path,
    files: &[&AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let rel = util::path_slash(&file.relative);
        for (literal, lines) in go_string_literals(cache.text(file)) {
            if lines.len() >= 3 {
                findings.push(repeated_literal_finding(&rel, literal, &lines));
            }
        }
    }
    findings
}

fn repeated_literal_finding(rel: &str, literal: String, lines: &[usize]) -> Finding {
    auditor_core::finding!(
        "go_repeated_string_literal",
        "lexical",
        "Repeated string literal should be constant",
        format!("String literal `{literal}` appears {} times.", lines.len()),
        Severity::Low,
        Confidence::High,
        rel,
        Some(lines[0]),
    )
    .with_recommendation("Extract to a named constant.")
    .with_anchor(auditor_core::FindingAnchor::Structural(format!(
        "literal:{rel}:{literal}"
    )))
    .with_evidence("occurrences", lines.len())
    .with_evidence(
        "lines",
        lines.iter().map(|l| l.to_string()).collect::<Vec<String>>(),
    )
    .with_evidence("literal", literal)
}

/// Interpreted string literals of four or more characters in Go source,
/// with the lines each appears on.
fn go_string_literals(text: &str) -> std::collections::BTreeMap<String, Vec<usize>> {
    let mut literals: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    let mut scan = GoLiteralScan::default();
    for (line_number, line) in text.lines().enumerate() {
        if !scan.in_raw && util::strip_comments(line).trim().is_empty() {
            continue;
        }
        for literal in scan.line(line) {
            if literal.len() >= 4 {
                literals.entry(literal).or_default().push(line_number + 1);
            }
        }
    }
    literals
}

/// A line-by-line Go literal lexer. Raw strings (shell templates, struct
/// tags) span lines and hold no string literals, only text that may contain
/// quote characters; a line comment or a rune literal such as '`' or '"'
/// holds no string delimiters either.
#[derive(Default)]
struct GoLiteralScan {
    in_raw: bool,
    in_str: bool,
    in_rune: bool,
    escape: bool,
    previous: char,
    current: String,
}

impl GoLiteralScan {
    /// The interpreted string literals that close on `line`.
    fn line(&mut self, line: &str) -> Vec<String> {
        self.in_str = false;
        self.in_rune = false;
        self.escape = false;
        self.previous = ' ';
        self.current.clear();
        let mut closed = Vec::new();
        for ch in line.chars() {
            if self.in_raw {
                self.in_raw = ch != '`';
                continue;
            }
            if !self.in_str {
                if ch == '/' && self.previous == '/' && !self.in_rune {
                    break;
                }
                if self.code_char(ch) {
                    continue;
                }
            }
            closed.extend(self.string_char(ch));
        }
        closed
    }

    /// Consumes a character outside a string that is part of a rune or
    /// opens a rune or raw string.
    fn code_char(&mut self, ch: char) -> bool {
        self.previous = ch;
        if self.in_rune {
            self.in_rune = ch != '\'' || self.escape;
            self.escape = ch == '\\' && !self.escape;
            return true;
        }
        match ch {
            '\'' => self.in_rune = true,
            '`' => self.in_raw = true,
            _ => return false,
        }
        true
    }

    /// Opens, extends, or closes an interpreted string; returns the literal
    /// a closing quote ends.
    fn string_char(&mut self, ch: char) -> Option<String> {
        if self.escape {
            self.escape = false;
            return None;
        }
        match ch {
            '\\' => self.escape = true,
            '"' => {
                self.in_str = !self.in_str;
                if !self.in_str {
                    return Some(std::mem::take(&mut self.current));
                }
            }
            _ if self.in_str => self.current.push(ch),
            _ => {}
        }
        None
    }
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
            "engineering-auditor-go-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_go_file(root: &std::path::Path, rel: &str, text: &str) -> AuditedFile {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        AuditedFile {
            absolute: path,
            relative: std::path::PathBuf::from(rel),
            language: crate::Language::Go,
            bytes: text.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    /// The findings `scan` reports on one Go file holding `text`.
    fn scan_go(
        name: &str,
        text: &str,
        scan: impl FnOnce(&std::path::Path, &AuditedFile, &crate::util::FileCache) -> Vec<Finding>,
    ) -> Vec<Finding> {
        let root = temp_root(name);
        let file = make_go_file(&root, "src/main.go", text);
        let cache = crate::util::FileCache::new(std::slice::from_ref(&file));
        let findings = scan(&root, &file, &cache);
        fs::remove_dir_all(&root).unwrap();
        findings
    }

    fn unused_assignments(name: &str, text: &str) -> Vec<Finding> {
        scan_go(name, text, |_, file, cache| {
            let mut repo_ast = auditor_ast::RepoAst::new();
            unused_assignment_findings(crate::ast_scan::AstScan::new(
                std::slice::from_ref(file),
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ))
        })
    }

    #[test]
    fn go_detects_log_fatal() {
        let findings = scan_go(
            "gofatal",
            "package main\nimport \"log\"\nfunc main() {\n    log.Fatal(\"die\")\n}\n",
            |_, file, cache| go_log_fatal_findings(&[file], cache),
        );
        assert!(findings.iter().any(|f| f.detector == "go_log_fatal"));
    }

    #[test]
    fn go_unused_assignment_ineffectual() {
        let findings = unused_assignments(
            "goineff",
            "package main\nfunc value() int {\n    x := 42\n    x = 7\n    return x\n}\n",
        );
        assert!(
            findings.iter().any(|f| f.detector == "unused_assignment"),
            "expected unused_assignment finding for ineffectual assignment"
        );
    }

    #[test]
    fn go_unused_assignment_skips_used_variable() {
        let findings = unused_assignments(
            "goineff_used",
            "package main\nfunc main() {\n    x := 42\n    println(x)\n}\n",
        );
        assert!(
            !findings.iter().any(|f| f.detector == "unused_assignment"),
            "expected no unused_assignment finding when variable is used"
        );
    }

    #[test]
    fn go_goconst_repeated_literal() {
        let findings = scan_go(
            "goconst",
            "package main\nfunc main() {\n    a := \"hello\"\n    b := \"hello\"\n    c := \"hello\"\n}\n",
            |root, file, cache| go_repeated_string_literal_findings(root, &[file], cache),
        );
        assert!(findings
            .iter()
            .any(|f| f.detector == "go_repeated_string_literal"));
    }
}
