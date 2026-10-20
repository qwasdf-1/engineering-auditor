//! Masking for string literals that can span lines: Python triple-quoted
//! strings, Rust raw strings (`r#"..."#`), Java and Kotlin text blocks, and
//! JavaScript, TypeScript, and Go backtick strings, and `/* .. */` comments
//! in the C-family languages. A line-wise scan would read the words and
//! numbers of a docstring, an embedded document, or a comment's prose lines
//! as code. Single-line literals are left to the line-wise strippers.

use crate::Language;

/// `lines` with the contents of every block string replaced by `""`; each
/// output line corresponds to the input line.
pub(crate) fn mask_block_strings(language: &Language, lines: &[&str]) -> Vec<String> {
    let mut open: Option<String> = None;
    lines
        .iter()
        .map(|line| {
            let mut output = String::new();
            let mut rest = *line;
            loop {
                if let Some(close) = &open {
                    let Some(end) = rest.find(close.as_str()) else {
                        break;
                    };
                    output.push_str("\"\"");
                    rest = &rest[end + close.len()..];
                    open = None;
                    continue;
                }
                let Some((start, opener_len, close)) = next_block_opener(language, rest) else {
                    output.push_str(rest);
                    break;
                };
                output.push_str(&rest[..start]);
                rest = &rest[start + opener_len..];
                open = Some(close);
            }
            output
        })
        .collect()
}

/// The byte offset, opener length, and closing delimiter of the first block
/// string that opens in `text` outside a comment or an ordinary literal.
fn next_block_opener(language: &Language, text: &str) -> Option<(usize, usize, String)> {
    let comment = if *language == Language::Python {
        "#"
    } else {
        "//"
    };
    let single_quoted = matches!(
        language,
        Language::Python | Language::JavaScript | Language::TypeScript
    );
    let mut index = 0;
    while let Some(ch) = text[index..].chars().next() {
        let rest = &text[index..];
        if let Some((opener_len, close)) = block_opener(language, text, index) {
            return Some((index, opener_len, close));
        }
        if rest.starts_with(comment) {
            return None;
        }
        if *language == Language::Rust && rest.starts_with("'\"'") {
            index += 3;
            continue;
        }
        if ch == '"' || (single_quoted && ch == '\'') {
            index += ordinary_literal_len(rest, ch);
            continue;
        }
        index += ch.len_utf8();
    }
    None
}

fn block_opener(language: &Language, text: &str, index: usize) -> Option<(usize, String)> {
    let rest = &text[index..];
    if *language != Language::Python && rest.starts_with("/*") {
        return Some((2, "*/".to_string()));
    }
    match language {
        Language::Python => ["\"\"\"", "'''"]
            .into_iter()
            .find(|quote| rest.starts_with(quote))
            .map(|quote| (3, quote.to_string())),
        Language::Java | Language::Kotlin => rest
            .starts_with("\"\"\"")
            .then(|| (3, "\"\"\"".to_string())),
        Language::JavaScript | Language::TypeScript | Language::Go => {
            rest.starts_with('`').then(|| (1, "`".to_string()))
        }
        Language::Rust => {
            let before = text[..index].chars().next_back();
            let starts_token = before.is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'))
                || (before == Some('b')
                    && text[..index - 1]
                        .chars()
                        .next_back()
                        .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_')));
            let hashes = rest.strip_prefix('r')?.len() - rest[1..].trim_start_matches('#').len();
            (starts_token && rest[1 + hashes..].starts_with('"'))
                .then(|| (hashes + 2, format!("\"{}", "#".repeat(hashes))))
        }
        _ => None,
    }
}

/// The byte length of an ordinary literal opened by `quote` at the start of
/// `rest`, through its closing quote or the end of the line.
fn ordinary_literal_len(rest: &str, quote: char) -> usize {
    let mut escaped = false;
    for (offset, ch) in rest.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return offset + ch.len_utf8();
        }
    }
    rest.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_strings_are_masked_across_lines() {
        assert_eq!(
            mask_block_strings(&Language::Python, &["x = '''a 4", "5 b''' + 7"]),
            ["x = ", "\"\" + 7"]
        );
        assert_eq!(
            mask_block_strings(
                &Language::Rust,
                &[
                    "let s = r#\"use 42",
                    "and 7\"# + 3; // r\"x",
                    "let for_ = \"for\";"
                ]
            ),
            ["let s = ", "\"\" + 3; // r\"x", "let for_ = \"for\";"]
        );
        assert_eq!(
            mask_block_strings(&Language::Go, &["s := \"`\" + `a 9", "b` // 12"]),
            ["s := \"`\" + ", "\"\" // 12"]
        );
        assert_eq!(
            mask_block_strings(&Language::Java, &["String s = \"\"\"", "  80", "\"\"\";"]),
            ["String s = ", "", "\"\";"]
        );
        assert_eq!(
            mask_block_strings(
                &Language::Java,
                &[
                    "int d = 2; /*",
                    "  length 5, then 7",
                    "*/ int e = 9; /* 3 */"
                ]
            ),
            ["int d = 2; ", "", "\"\" int e = 9; \"\""]
        );
    }
}
