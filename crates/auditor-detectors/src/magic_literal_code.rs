//! The code of one line as `magic_literal_cluster` reads it: numbers that
//! carry their own name or are data rather than logic are removed first.
//!
//! - A JavaScript or TypeScript regular-expression literal is a pattern, not
//!   arithmetic (`/[0-9a-f]{12}/`).
//! - A JavaScript or TypeScript object property or JSX attribute value is
//!   named by its key (`{ width: 1200 }`, `width={1200}`); ESLint's
//!   `no-magic-numbers` skips object values by default (`detectObjects:
//!   false`).
//! - A bracketed list of three or more numbers alone is a lookup table
//!   (`[0, 2, 4, 6, 8, 1, 3, 5, 7, 9][digit]`), whose entries have no names.

use std::sync::OnceLock;

use regex::Regex;

use crate::Language;

/// `line` with regex literals, key-named values, and numeric tables blanked.
pub(crate) fn magic_candidate_code(language: &Language, line: &str) -> String {
    let scripted = matches!(language, Language::JavaScript | Language::TypeScript);
    let line = if scripted {
        mask_regex_literals(line)
    } else {
        line.to_string()
    };
    let code = crate::util::strip_comments(&line);
    let code = numeric_table().replace_all(&code, "[]");
    if scripted {
        key_named_value().replace_all(&code, "$1").into_owned()
    } else {
        code.into_owned()
    }
}

fn numeric_table() -> &'static Regex {
    static TABLE: OnceLock<Regex> = OnceLock::new();
    TABLE.get_or_init(|| {
        let number = r"-?(?:0[xXoObB][0-9a-fA-F_]+|[0-9][0-9_]*(?:\.[0-9_]+)?)";
        Regex::new(&format!(
            r"[\[{{]\s*{number}(?:\s*,\s*{number}){{2,}}\s*,?\s*[\]}}]"
        ))
        .expect("numeric table pattern")
    })
}

fn key_named_value() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        let number = r"-?(?:0[xXoObB][0-9a-fA-F_]+|[0-9][0-9_]*(?:\.[0-9_]+)?)";
        Regex::new(&format!(
            r#"((?:^|[{{,])\s*(?:[A-Za-z_$][\w$]*|"[^"]*"|'[^']*')\s*:\s*){number}|(\b[A-Za-z_$][\w$-]*=)\{{\s*{number}\s*\}}"#
        ))
        .expect("key-named value pattern")
    })
}

/// Blanks the body of each regex literal: a `/` where an operand is
/// expected (after an operator, an opening bracket, `return`, or at the
/// start of the line) opens one, and the next unescaped `/` outside a
/// character class closes it.
fn mask_regex_literals(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(line.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if matches!(ch, '"' | '\'' | '`') {
            let end = quoted_end(&chars, index);
            output.extend(&chars[index..end]);
            index = end;
        } else if ch == '/' && matches!(chars.get(index + 1), Some('/' | '*')) {
            output.extend(&chars[index..]);
            break;
        } else if let Some(end) = (ch == '/' && expects_operand(&chars[..index]))
            .then(|| regex_end(&chars, index))
            .flatten()
        {
            output.push_str("/r/");
            index = end;
        } else {
            output.push(ch);
            index += 1;
        }
    }
    output
}

/// The index after the literal quoted by `chars[start]`.
fn quoted_end(chars: &[char], start: usize) -> usize {
    let mut index = start + 1;
    while index < chars.len() && chars[index] != chars[start] {
        index += if chars[index] == '\\' { 2 } else { 1 };
    }
    (index + 1).min(chars.len())
}

/// The index after the regex literal opened at `chars[start]`, if it closes
/// on this line.
fn regex_end(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut class = false;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '[' => class = true,
            ']' => class = false,
            '/' if !class => return Some(index + 1),
            _ => {}
        }
        index += 1;
    }
    None
}

fn expects_operand(before: &[char]) -> bool {
    let text = before.iter().collect::<String>();
    let trimmed = text.trim_end();
    trimmed.is_empty()
        || trimmed.ends_with([
            '(', ',', '=', ':', '[', '!', '&', '|', '?', '{', '}', ';', '+', '-', '*', '%', '<',
            '>', '~', '^',
        ])
        || ["return", "typeof", "case", "in", "of"]
            .iter()
            .any(|keyword| trimmed.ends_with(keyword) && !trimmed.ends_with(&format!(".{keyword}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(language: &Language, line: &str) -> Vec<String> {
        crate::util::numeric_literals(&magic_candidate_code(language, line))
    }

    #[test]
    fn named_values_patterns_and_tables_carry_no_magic_numbers() {
        let ts = Language::TypeScript;
        assert!(numbers(&ts, "  return /^[0-9a-f]{8}-[0-9a-f]{12}$/.test(id);").is_empty());
        assert!(numbers(&ts, "const re = new RegExp(x), y = a / 7 / 9;").contains(&"7".to_string()));
        assert!(numbers(&ts, "  style={{ width: 1200, height: 630 }} size={48}").is_empty());
        assert_eq!(numbers(&ts, "  const t = ok ? 5 : 60;"), ["5", "60"]);
        assert!(numbers(&ts, "  sum += bit ? [0, 2, 4, 6, 8, 1, 3, 5, 7, 9][v] : v;").is_empty());
        assert_eq!(
            numbers(&Language::Java, "  int b = MASK[3] + 0x3f;"),
            ["3", "0x3f"]
        );
        assert!(numbers(&Language::Java, "  int[] t = {7, 11, 13};").is_empty());
        assert_eq!(
            numbers(&Language::Python, "  return {'width': 1200}"),
            ["1200"],
            "object values are skipped only where ESLint's convention applies"
        );
    }
}
