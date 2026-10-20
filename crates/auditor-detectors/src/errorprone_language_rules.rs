//! Language-specific error-prone rules: the Ruby, C++, Kotlin, and C#
//! lexical contracts.

use auditor_core::{Confidence, Finding, Severity};

use crate::{util, AuditedFile};

fn kotlin_when_is_empty(lines: &[&str], start: usize) -> bool {
    let mut depth = 0isize;
    let mut opened = false;
    for line in &lines[start..] {
        let code = util::strip_comments(line);
        if code.contains("->") || code.split_whitespace().any(|token| token == "else") {
            return false;
        }
        for ch in code.chars() {
            match ch {
                '{' => {
                    opened = true;
                    depth += 1;
                }
                '}' if opened => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return true;
        }
    }
    false
}

fn findings_for_language(
    files: &[AuditedFile],
    cache: &util::FileCache,
    language: crate::Language,
    detector: fn(&str, &str) -> Vec<Finding>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files
        .iter()
        .filter(|file| file.language == language && !util::is_test_source_path(&file.relative))
    {
        let rel = util::path_slash(&file.relative);
        findings.extend(detector(&rel, cache.text(file)));
    }
    findings
}

fn ruby_double_bang_per_file(rel: &str, text: &str) -> Vec<Finding> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| util::strip_comments(line).contains("!!"))
        .map(|(index, line)| {
            auditor_core::finding!(
                "ruby_double_bang",
                "correctness",
                "Double-bang (!!) used for boolean coercion",
                format!(
                    "Ruby double-bang usage at line {}; prefer an explicit nil check",
                    index + 1
                ),
                Severity::Low,
                Confidence::High,
                rel,
                Some(index + 1),
            )
            .with_recommendation(
                "Use a descriptive boolean method or explicit `!value.nil?` instead of `!!`.",
            )
            .with_evidence("pattern", line.trim())
        })
        .collect()
}

/// A `new` expression that allocates: the whole word `new`, not placement
/// `new (address) T{..}`, which constructs in storage someone else owns.
fn allocates_with_new(code: &str) -> bool {
    code.match_indices("new").any(|(index, _)| {
        let before = code[..index].chars().next_back();
        let after = &code[index + 3..];
        before.is_none_or(|character| !(character.is_alphanumeric() || character == '_'))
            && after.starts_with(char::is_whitespace)
            && !after.trim_start().starts_with('(')
    })
}

fn cpp_raw_ptr_ownership_per_file(rel: &str, text: &str) -> Vec<Finding> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let code = util::strip_comments(line);
            let raw_allocation = allocates_with_new(&code)
                && code.contains('*')
                && !["unique_ptr", "shared_ptr", "auto_ptr"]
                    .iter()
                    .any(|smart_pointer| code.contains(smart_pointer))
                && (code.contains('=') || code.contains('('));
            raw_allocation.then(|| {
                auditor_core::finding!(
                    "cpp_raw_ptr_ownership",
                    "correctness",
                    "Raw pointer ownership ambiguity (v0.6.4)",
                    format!(
                        "Raw pointer allocation with `new` at line {}; ownership is unclear",
                        index + 1
                    ),
                    Severity::Medium,
                    Confidence::Medium,
                    rel,
                    Some(index + 1),
                )
                .with_recommendation(
                    "Use std::unique_ptr or std::shared_ptr to express ownership explicitly.",
                )
                .with_evidence("pattern", line.trim())
            })
        })
        .collect()
}

fn kotlin_empty_when_per_file(rel: &str, text: &str) -> Vec<Finding> {
    let lines: Vec<_> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let code = util::strip_comments(line);
            let trimmed = code.trim();
            (trimmed.contains("when ")
                && trimmed.contains('{')
                && kotlin_when_is_empty(&lines, index))
            .then(|| {
                auditor_core::finding!(
                    "kotlin_empty_when",
                    "correctness",
                    "Empty when expression in production code",
                    format!("When expression without branches at line {}", index + 1),
                    Severity::Low,
                    Confidence::Medium,
                    rel,
                    Some(index + 1),
                )
                .with_recommendation("Add branches to the when expression or remove it.")
            })
        })
        .collect()
}

fn csharp_linq_enumeration_per_file(rel: &str, text: &str) -> Vec<Finding> {
    let calls = [".Count()", ".ToList()", ".ToArray()"];
    let enumeration_count = calls
        .iter()
        .map(|call| text.matches(call).count())
        .sum::<usize>();
    if !text.contains("IEnumerable") || enumeration_count < 3 {
        return Vec::new();
    }
    let line = text
        .lines()
        .position(|line| calls.iter().any(|call| line.contains(call)))
        .map(|index| index + 1);
    vec![auditor_core::finding!(
        "csharp_linq_enumeration",
        "correctness",
        "Potential multiple IEnumerable enumeration",
        format!(
            "{rel} has {enumeration_count} LINQ enumeration calls; consider materializing once"
        ),
        Severity::Low,
        Confidence::Low,
        rel,
        line,
    )
    .with_recommendation(
        "Materialize the IEnumerable once with .ToList() or .ToArray() at the source.",
    )]
}

pub(crate) fn ruby_double_bang_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    findings_for_language(
        files,
        cache,
        crate::Language::Ruby,
        ruby_double_bang_per_file,
    )
}

pub(crate) fn cpp_raw_ptr_ownership_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    findings_for_language(
        files,
        cache,
        crate::Language::Cpp,
        cpp_raw_ptr_ownership_per_file,
    )
}

pub(crate) fn kotlin_empty_when_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    findings_for_language(
        files,
        cache,
        crate::Language::Kotlin,
        kotlin_empty_when_per_file,
    )
}

pub(crate) fn csharp_linq_enumeration_findings(
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    findings_for_language(
        files,
        cache,
        crate::Language::CSharp,
        csharp_linq_enumeration_per_file,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_new_and_words_ending_in_new_do_not_allocate() {
        let findings = cpp_raw_ptr_ownership_per_file(
            "src/map.hpp",
            "Node* n = new Node();\nnew (&*it) value_type{std::move(*next)};\nint* renew = renew_buffer(p);\n",
        );
        assert_eq!(
            findings
                .iter()
                .map(|f| f.primary_line())
                .collect::<Vec<_>>(),
            [Some(1)]
        );
    }
}
