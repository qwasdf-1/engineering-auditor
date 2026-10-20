//! Preprocessor constructs tree-sitter cannot see through, blanked with
//! spaces so every byte offset is kept.
//!
//! C++ export macros between `class`/`struct` and the type name
//! (`class LEVELDB_EXPORT Env {`, `class YAML_CPP_API Emitter : ..`) expand
//! to visibility attributes the grammar cannot see, so tree-sitter reads the
//! macro as the type and the class as a function definition spanning the
//! whole body. A macro alone on its line likewise derails the next
//! declaration: a namespace macro (`NLOHMANN_JSON_NAMESPACE_BEGIN`,
//! `FMT_END_NAMESPACE`), one before `class`/`namespace`/`template`
//! (`FMT_END_EXPORT`, `NLOHMANN_BASIC_JSON_TPL_DECLARATION`), and one used as
//! an access specifier (`JSON_PRIVATE_UNLESS_TESTED:`).
//!
//! In C and C++, a block comment inside a `#define` body (`/* ARGSUSED */ \`)
//! ends the macro for tree-sitter, which then parses the rest of the body as
//! code.

use std::borrow::Cow;

/// `source` with each such construct blanked, or unchanged when it has none.
/// Export and lone macros are C++ only.
pub(crate) fn blank_macros(source: &str, cpp: bool) -> Cow<'_, str> {
    let mut lines = Vec::new();
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        lines.push((line_start, line));
        line_start += line.len();
    }
    let mut ranges = directive_comment_ranges(&lines);
    if cpp {
        ranges.extend(export_macro_ranges(source.as_bytes()));
        ranges.extend(lone_macro_ranges(&lines));
    }
    if ranges.is_empty() {
        return Cow::Borrowed(source);
    }
    let mut output = source.as_bytes().to_vec();
    for range in ranges {
        output[range].fill(b' ');
    }
    // Only ASCII bytes were replaced with ASCII spaces.
    Cow::Owned(String::from_utf8(output).expect("ASCII replacement"))
}

/// Export macros between a `class`/`struct` keyword and the type name.
fn export_macro_ranges(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(offset) = find_keyword(&bytes[index..]) {
        let keyword_end = index + offset;
        index = match export_macro_after(bytes, keyword_end) {
            Some((macro_start, macro_end)) => {
                ranges.push(macro_start..macro_end);
                macro_end
            }
            None => keyword_end,
        };
    }
    ranges
}

/// Block comments inside preprocessor directives, continuation lines
/// included.
fn directive_comment_ranges(lines: &[(usize, &str)]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut in_directive = false;
    for &(start, line) in lines {
        let continued = line.trim_end().ends_with('\\');
        in_directive = (in_directive || line.trim_start().starts_with('#')) && continued;
        if in_directive {
            ranges.extend(
                block_comments(line)
                    .into_iter()
                    .map(|comment| start + comment.start..start + comment.end),
            );
        }
    }
    ranges
}

/// A macro alone on its line before a namespace or declaration, or used as
/// an access specifier (`JSON_PRIVATE_UNLESS_TESTED:`).
fn lone_macro_ranges(lines: &[(usize, &str)]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    for (position, &(start, line)) in lines.iter().enumerate() {
        let name = line.trim();
        let lone = is_macro_name(name.as_bytes())
            && (name.contains("NAMESPACE") || precedes_declaration(&lines[position + 1..]));
        let access = name
            .strip_suffix(':')
            .is_some_and(|label| is_macro_name(label.as_bytes()));
        if lone || access {
            let name_start = start + (line.len() - line.trim_start().len());
            ranges.push(name_start..name_start + name.len());
        }
    }
    ranges
}

/// Whether the next nonblank line opens a class, namespace, or template.
fn precedes_declaration(rest: &[(usize, &str)]) -> bool {
    rest.iter()
        .map(|(_, line)| line.trim_start())
        .find(|line| !line.is_empty())
        .is_some_and(|line| {
            ["class ", "struct ", "union ", "namespace ", "template"]
                .iter()
                .any(|keyword| line.starts_with(keyword))
        })
}

/// Byte ranges of the `/* .. */` comments that open and close on `line`.
fn block_comments(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut comments = Vec::new();
    let mut from = 0;
    while let Some(open) = line[from..].find("/*").map(|offset| from + offset) {
        let Some(close) = line[open + 2..]
            .find("*/")
            .map(|offset| open + 2 + offset + 2)
        else {
            break;
        };
        comments.push(open..close);
        from = close;
    }
    comments
}

/// The offset just past the next `class` or `struct` keyword in `bytes`.
fn find_keyword(bytes: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index < bytes.len() {
        for keyword in [&b"class"[..], &b"struct"[..]] {
            let end = index + keyword.len();
            if bytes[index..].starts_with(keyword)
                && (index == 0 || !is_identifier_byte(bytes[index - 1]))
                && bytes.get(end).is_some_and(u8::is_ascii_whitespace)
            {
                return Some(end);
            }
        }
        index += 1;
    }
    None
}

/// An ALL_CAPS macro after `keyword_end`, followed by a type name and then
/// `{`, `:`, or `final`.
fn export_macro_after(bytes: &[u8], keyword_end: usize) -> Option<(usize, usize)> {
    let macro_start = skip_whitespace(bytes, keyword_end);
    let macro_end = identifier_end(bytes, macro_start)?;
    if !is_macro_name(&bytes[macro_start..macro_end]) {
        return None;
    }
    let type_start = skip_whitespace(bytes, macro_end);
    if type_start == macro_end {
        return None;
    }
    let type_end = identifier_end(bytes, type_start)?;
    let next = skip_whitespace(bytes, type_end);
    (matches!(bytes.get(next), Some(b'{' | b':')) || bytes[next..].starts_with(b"final"))
        .then_some((macro_start, macro_end))
}

fn is_macro_name(name: &[u8]) -> bool {
    name.len() >= 3
        && name.contains(&b'_')
        && name.first().is_some_and(u8::is_ascii_uppercase)
        && name
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn identifier_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut index = start + 1;
    while bytes.get(index).copied().is_some_and(is_identifier_byte) {
        index += 1;
    }
    Some(index)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_macros_are_blanked_in_place() {
        let source = "class YAML_CPP_API node_data {\nstruct LEVELDB_EXPORT Options : Base {};\nclass FMT_API final_t final {};\n";
        let blanked = blank_macros(source, true);
        assert_eq!(blanked.len(), source.len());
        assert_eq!(
            blanked,
            "class              node_data {\nstruct                Options : Base {};\nclass         final_t final {};\n"
        );
        for untouched in [
            "class Foo {};",
            "class Foo;",
            "struct A_B c;",
            "enum class MY_ENUM : int {};",
            "x = MY_NAMESPACE_VALUE;\n",
            "#define FMT_BEGIN_NAMESPACE namespace fmt {\n",
        ] {
            assert!(
                matches!(blank_macros(untouched, true), Cow::Borrowed(_)),
                "{untouched}"
            );
        }
    }

    #[test]
    fn namespace_macro_lines_are_blanked_in_place() {
        let source = "NLOHMANN_JSON_NAMESPACE_BEGIN\nnamespace detail {\n}\n  FMT_END_NAMESPACE\n";
        assert_eq!(
            blank_macros(source, true),
            "                             \nnamespace detail {\n}\n                   \n"
        );
    }

    #[test]
    fn lone_macros_before_declarations_and_access_macros_are_blanked() {
        let source = "FMT_END_EXPORT\n\nnamespace detail {}\nTPL_DECL\nclass basic {\n  JSON_PRIVATE_UNLESS_TESTED:\n  int x;\n};\n";
        assert_eq!(
            blank_macros(source, true),
            "              \n\nnamespace detail {}\n        \nclass basic {\n                             \n  int x;\n};\n"
        );
        // An enum's last value and a goto label are not macros to blank.
        for untouched in ["enum { A_B,\n  LAST_VALUE\n};\n", "  retry_now:\n  x();\n"] {
            assert!(
                matches!(blank_macros(untouched, true), Cow::Borrowed(_)),
                "{untouched}"
            );
        }
        assert!(matches!(blank_macros(source, false), Cow::Borrowed(_)));
    }

    #[test]
    fn comments_inside_macro_bodies_are_blanked() {
        let source = "#define GEN(name) \\\n/* ARGSUSED */ \\\nint name(void) { return 0; }\n/* kept */\nx = 1; /* kept */ \\\n";
        assert_eq!(
            blank_macros(source, false),
            "#define GEN(name) \\\n               \\\nint name(void) { return 0; }\n/* kept */\nx = 1; /* kept */ \\\n"
        );
    }
}
