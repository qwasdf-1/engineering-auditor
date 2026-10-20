use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

pub(crate) use crate::source_cache::FileCache;
use crate::{AuditedFile, FunctionSpan, Language};

pub(crate) fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| text.lines().map(|line| line.to_string()).collect())
        .unwrap_or_default()
}

pub(crate) fn category_for(file: &AuditedFile) -> Option<&'static str> {
    let rel = file.relative.to_string_lossy();
    if rel == "Cargo.lock"
        || rel == "package-lock.json"
        || rel == "yarn.lock"
        || rel.ends_with(".sum")
    {
        return Some("lock");
    }
    Some(match file.language {
        Language::Rust
        | Language::Python
        | Language::JavaScript
        | Language::TypeScript
        | Language::Go
        | Language::Java
        | Language::Kotlin
        | Language::CSharp
        | Language::C
        | Language::Cpp
        | Language::Ruby
        | Language::Julia
        | Language::Shell => {
            if is_test_source_path(&file.relative) {
                "tests"
            } else {
                "source"
            }
        }
        Language::Markdown => "docs",
        Language::Json | Language::Toml | Language::Yaml => "structured",
        Language::Other(_) => return None,
    })
}

pub(crate) fn relative_slash(root: &Path, path: &Path) -> String {
    path_slash(path.strip_prefix(root).unwrap_or(path))
}

pub(crate) fn path_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

const TEST_DIR_MARKERS: &[&str] = &[
    "/test/",
    "/tests/",
    "/fixtures/",
    "/fixture/",
    "/__pycache__/",
    "/spec/",
    "/mock/",
];

const TEST_NAME_SUFFIXES: &[&str] = &[
    "_test.rs",
    "_test.py",
    "_test.ts",
    "_test.js",
    "_tests.rs",
    "tests.rs",
    ".test.ts",
    ".test.tsx",
    ".spec.ts",
    ".spec.tsx",
    ".spec.js",
];

/// A file under a translation directory (`locales/de.ts`, `i18n/fr.py`):
/// each locale owns its messages, plural rules, and the code around them by
/// convention.
pub(crate) fn is_locale_path(path: &str) -> bool {
    const LOCALE_DIRECTORIES: &[&str] = &["locale", "locales", "i18n", "l10n", "translations"];
    path.split('/')
        .rev()
        .skip(1)
        .any(|directory| LOCALE_DIRECTORIES.contains(&directory))
}

pub(crate) fn is_test_source_path(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    TEST_DIR_MARKERS.iter().any(|m| lower.contains(m))
        || lower.starts_with("fixtures/")
        || name.starts_with("test_")
        || name.starts_with("lib_test")
        || name.starts_with("capability_case")
        || TEST_NAME_SUFFIXES.iter().any(|s| name.ends_with(s))
        || (lower.contains("/node_modules/") && lower.ends_with(".ts"))
}

pub(crate) fn map_language_to_ast(lang: &Language) -> Option<auditor_ast::AstLang> {
    match lang {
        Language::Rust => Some(auditor_ast::AstLang::Rust),
        Language::Python => Some(auditor_ast::AstLang::Python),
        Language::JavaScript => Some(auditor_ast::AstLang::JavaScript),
        Language::TypeScript => Some(auditor_ast::AstLang::TypeScript),
        Language::Go => Some(auditor_ast::AstLang::Go),
        Language::Java => Some(auditor_ast::AstLang::Java),
        Language::Kotlin => Some(auditor_ast::AstLang::Kotlin),
        Language::CSharp => Some(auditor_ast::AstLang::CSharp),
        Language::C => Some(auditor_ast::AstLang::C),
        Language::Cpp => Some(auditor_ast::AstLang::Cpp),
        Language::Ruby => Some(auditor_ast::AstLang::Ruby),
        Language::Julia => Some(auditor_ast::AstLang::Julia),
        Language::Shell => Some(auditor_ast::AstLang::Bash),
        _ => None,
    }
}

pub(crate) fn function_spans_for(
    language: &Language,
    lines: &[&str],
    _file_path: &str,
) -> Vec<FunctionSpan> {
    if let Some(ast_lang) = map_language_to_ast(language) {
        let source = lines.join("\n");
        if let Ok(mut parser) = auditor_ast::AstParser::new() {
            if let Ok(tree) = parser.parse(&ast_lang, &source) {
                let ast_spans = tree.function_spans();
                if !ast_spans.is_empty() {
                    return ast_spans.as_ref().to_vec();
                }
            }
        }
    }
    // Fallback: token-level function detection
    let starts: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            function_name_for_line(language, line).map(|name| (index + 1, name))
        })
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(index, (start, name))| {
            let fallback = starts
                .get(index + 1)
                .map(|(line, _)| line - 1)
                .unwrap_or(lines.len());
            FunctionSpan {
                start_line: *start,
                end_line: function_end_line(language, lines, *start, fallback),
                name: name.clone(),
                start_byte: 0,
                end_byte: 0,
            }
        })
        .collect()
}

pub(crate) fn cached_function_spans_for(
    cache: &FileCache,
    file: &AuditedFile,
    lines: &[&str],
) -> Arc<[FunctionSpan]> {
    if let Some(spans) = cache.function_spans(file) {
        return spans;
    }
    let rel = path_slash(&file.relative);
    let spans: Arc<[FunctionSpan]> = function_spans_for(&file.language, lines, &rel).into();
    cache.cache_function_spans(file, Arc::clone(&spans));
    spans
}

pub(crate) fn go_function_name(line: &str) -> Option<String> {
    let value = line.strip_prefix("func ")?.trim_start();
    // Handle receiver: func (r *Type) Name(...)
    let after_receiver = if value.starts_with('(') {
        let close = value.find(')')?;
        let rest = value[close + 1..].trim_start();
        rest
    } else {
        value
    };
    identifier_prefix(after_receiver)
}

pub(crate) fn java_function_name(line: &str) -> Option<String> {
    let mut value = line.trim_start();
    for prefix in [
        "public ",
        "private ",
        "protected ",
        "static ",
        "final ",
        "abstract ",
        "synchronized ",
        "native ",
        "transient ",
        "volatile ",
    ] {
        value = value.strip_prefix(prefix).unwrap_or(value).trim_start();
    }
    if value.starts_with('<') {
        let close = value.find('>')?;
        value = value[close + 1..].trim_start();
    }
    let paren = value.find('(')?;
    let before_paren = &value[..paren].trim_end();
    let name = before_paren.split_whitespace().last()?;
    if name.contains('.') || name.is_empty() {
        return None;
    }
    if matches!(name, "if" | "for" | "while" | "switch" | "catch") {
        return None;
    }
    Some(name.to_string())
}

pub(crate) fn function_name_for_line(language: &Language, line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    match language {
        Language::Rust => rust_function_name(trimmed),
        Language::Python => python_function_name(trimmed),
        Language::JavaScript | Language::TypeScript => js_function_name(trimmed),
        Language::Go => go_function_name(trimmed),
        Language::Java | Language::Kotlin | Language::CSharp => java_function_name(trimmed),
        Language::C | Language::Cpp => c_function_name(trimmed),
        Language::Ruby => ruby_function_name(trimmed),
        Language::Julia => julia_function_name(trimmed),
        Language::Shell => shell_function_name(trimmed),
        _ => None,
    }
}

pub(crate) fn c_function_name(line: &str) -> Option<String> {
    let mut value = line.trim_start();
    for prefix in ["static ", "extern ", "inline ", "virtual ", "const "] {
        value = value.strip_prefix(prefix).unwrap_or(value).trim_start();
    }
    let paren = value.find('(')?;
    let before_paren = value[..paren].trim_end();
    let name = before_paren.split_whitespace().last()?;
    if name.contains('*') || name.contains('.') || name.is_empty() {
        return None;
    }
    if matches!(
        name,
        "if" | "for" | "while" | "switch" | "catch" | "return" | "sizeof"
    ) {
        return None;
    }
    Some(name.to_string())
}

pub(crate) fn ruby_function_name(line: &str) -> Option<String> {
    let value = line.strip_prefix("def ")?;
    let name = value.split_whitespace().next()?;
    if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    Some(name.to_string())
}

pub(crate) fn julia_function_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("function ") {
        let name = rest.split_whitespace().next()?;
        Some(name.to_string())
    } else {
        identifier_prefix(line)
    }
}

pub(crate) fn rust_function_name(line: &str) -> Option<String> {
    let mut value = line.trim_start();
    for prefix in [
        "pub ",
        "pub(crate) ",
        "pub(super) ",
        "async ",
        "const ",
        "unsafe ",
    ] {
        value = value.strip_prefix(prefix).unwrap_or(value).trim_start();
    }
    let rest = value.strip_prefix("fn ")?;
    identifier_prefix(rest)
}

pub(crate) fn python_function_name(line: &str) -> Option<String> {
    let value = line
        .strip_prefix("async def ")
        .or_else(|| line.strip_prefix("def "))?;
    identifier_prefix(value)
}

pub(crate) fn js_function_name(line: &str) -> Option<String> {
    let value = line
        .strip_prefix("export async function ")
        .or_else(|| line.strip_prefix("export function "))
        .or_else(|| line.strip_prefix("async function "))
        .or_else(|| line.strip_prefix("function "));
    if let Some(rest) = value {
        return identifier_prefix(rest);
    }
    for prefix in ["const ", "let ", "var "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            let name = identifier_prefix(rest)?;
            if rest.contains("=>") {
                return Some(name);
            }
        }
    }
    None
}

fn shell_function_name(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("function ") {
        if let Some(name) = rest.split_whitespace().next() {
            return Some(name.to_string());
        }
    }
    let (name, rest) = line.split_once("()")?;
    rest.trim_start()
        .starts_with('{')
        .then(|| name.trim().to_string())
}

pub(crate) fn identifier_prefix(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut value = String::from(first);
    for ch in chars {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            value.push(ch);
        } else {
            break;
        }
    }
    Some(value)
}

pub(crate) fn function_end_line(
    language: &Language,
    lines: &[&str],
    start_line: usize,
    fallback: usize,
) -> usize {
    if matches!(
        language,
        Language::Python | Language::Ruby | Language::Julia
    ) {
        return indent_end_line(lines, start_line, fallback);
    }
    brace_end_line(lines, start_line, fallback)
}

fn indent_end_line(lines: &[&str], start_line: usize, fallback: usize) -> usize {
    let start_index = start_line - 1;
    let base_indent = leading_spaces(lines[start_index]);
    for (index, line) in lines.iter().enumerate().skip(start_index + 1) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if leading_spaces(line) <= base_indent {
            return index;
        }
    }
    fallback
}

fn brace_end_line(lines: &[&str], start_line: usize, fallback: usize) -> usize {
    let mut depth = 0isize;
    let mut opened = false;
    for (index, line) in lines.iter().enumerate().skip(start_line - 1) {
        let code = strip_comments(line);
        if !opened && code.contains(';') && !code.contains('{') {
            return start_line;
        }
        for ch in code.chars() {
            if ch == '{' {
                depth += 1;
                opened = true;
            } else if ch == '}' && opened {
                depth -= 1;
                if depth <= 0 {
                    return index + 1;
                }
            }
        }
    }
    fallback
}

pub(crate) fn function_body_lines<'a>(
    lines: &[&'a str],
    span: &FunctionSpan,
    spans: &[FunctionSpan],
) -> Vec<(usize, &'a str)> {
    compute_body_lines(lines, span, spans)
}

pub(crate) fn function_executable_lines<'a>(
    language: &Language,
    lines: &[&'a str],
    span: &FunctionSpan,
    spans: &[FunctionSpan],
) -> Vec<(usize, &'a str)> {
    let Some((body_line, body_offset)) = signature_body_start(language, lines, span) else {
        return function_body_lines(lines, span, spans);
    };
    function_body_lines(lines, span, spans)
        .into_iter()
        .filter_map(|(line_number, line)| match line_number.cmp(&body_line) {
            Ordering::Less => None,
            Ordering::Equal => Some((line_number, line.get(body_offset..).unwrap_or_default())),
            Ordering::Greater => Some((line_number, line)),
        })
        .collect()
}

fn signature_body_start(
    language: &Language,
    lines: &[&str],
    span: &FunctionSpan,
) -> Option<(usize, usize)> {
    if matches!(language, Language::Ruby) {
        return Some((span.start_line.saturating_add(1), 0));
    }
    let python_style = matches!(language, Language::Python);
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quotes = QuoteState::default();
    for line_number in span.start_line..=span.end_line {
        let line = lines.get(line_number - 1)?;
        for (offset, ch) in line.char_indices() {
            if quotes.consumes(ch) {
                continue;
            }
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                ':' if python_style && paren_depth == 0 && bracket_depth == 0 => {
                    return Some((line_number, offset + ch.len_utf8()));
                }
                '{' if !python_style && paren_depth == 0 && bracket_depth == 0 => {
                    return Some((line_number, offset + ch.len_utf8()));
                }
                _ => {}
            }
        }
    }
    None
}

/// Quote and escape state while scanning code character by character.
#[derive(Default)]
pub(crate) struct QuoteState {
    quote: Option<char>,
    escaped: bool,
}

impl QuoteState {
    /// Whether `ch` is a quote or part of a quoted string, updating the state.
    pub(crate) fn consumes(&mut self, ch: char) -> bool {
        if self.escaped {
            self.escaped = false;
            return true;
        }
        if self.quote.is_some() && ch == '\\' {
            self.escaped = true;
            return true;
        }
        if matches!(ch, '\'' | '"' | '`') {
            if self.quote == Some(ch) {
                self.quote = None;
            } else if self.quote.is_none() {
                self.quote = Some(ch);
            }
            return true;
        }
        self.quote.is_some()
    }
}

fn compute_body_lines<'a>(
    lines: &[&'a str],
    span: &FunctionSpan,
    spans: &[FunctionSpan],
) -> Vec<(usize, &'a str)> {
    let child_ranges: Vec<_> = spans
        .iter()
        .filter(|child| span.start_line < child.start_line && child.end_line <= span.end_line)
        .map(|child| (child.start_line, child.end_line))
        .collect();
    (span.start_line..=span.end_line)
        .filter(|line_number| {
            !child_ranges
                .iter()
                .any(|(start, end)| start <= line_number && line_number <= end)
        })
        .filter_map(|line_number| lines.get(line_number - 1).map(|line| (line_number, *line)))
        .collect()
}

pub(crate) fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Whether `line` carries a `//` or `#` comment naming one of `reasons`
/// (lower case).
pub(crate) fn comment_gives_reason(line: &str, reasons: &[&str]) -> bool {
    let start = [line.find("//"), line.find('#')]
        .into_iter()
        .flatten()
        .min();
    start.is_some_and(|start| {
        let comment = line[start..].to_ascii_lowercase();
        reasons.iter().any(|reason| comment.contains(reason))
    })
}

pub(crate) fn strip_comments(line: &str) -> String {
    let stripped = line.trim_start();
    if stripped.starts_with("//") || stripped.starts_with('#') {
        return String::new();
    }
    let literal_free = strip_string_literals(line);
    literal_free
        .split("//")
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("")
        .to_string()
}

pub(crate) fn strip_comments_preserve_literals(line: &str) -> String {
    let mut output = String::new();
    let mut quote = None;
    let mut escape = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '"' | '\'' | '`') {
            quote = Some(ch);
            output.push(ch);
        } else if ch == '#' || (ch == '/' && chars.peek() == Some(&'/')) {
            break;
        } else {
            output.push(ch);
        }
    }
    output
}

pub(crate) fn strip_string_literals(line: &str) -> String {
    let mut output = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, '"' | '\'' | '`') {
            output.push_str("\"\"");
            skip_quoted(&mut chars, ch);
        } else {
            output.push(ch);
        }
    }
    output
}

/// Consumes characters through the closing `quote`, honoring escapes.
fn skip_quoted(chars: &mut impl Iterator<Item = char>, quote: char) {
    let mut escape = false;
    for inner in chars {
        if escape {
            escape = false;
        } else if inner == '\\' {
            escape = true;
        } else if inner == quote {
            return;
        }
    }
}

pub(crate) fn branch_decision_count(line: &str) -> usize {
    let code = strip_comments(line);
    [
        "if", "elif", "for", "while", "loop", "match", "case", "catch", "except", "guard", "where",
        "with", "yield", "select", "until", "esac",
    ]
    .iter()
    .map(|word| count_word(&code, word))
    .sum::<usize>()
        + code.matches("&&").count()
        + code.matches("||").count()
        + if code.contains('?') && code.contains(':') {
            1
        } else {
            0
        }
}

pub(crate) fn count_word(text: &str, word: &str) -> usize {
    identifiers(text)
        .into_iter()
        .filter(|item| item == word)
        .count()
}

pub(crate) fn identifiers(text: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            push_identifier(&mut identifiers, &mut current);
        }
    }
    push_identifier(&mut identifiers, &mut current);
    identifiers
}

fn push_identifier(values: &mut Vec<String>, current: &mut String) {
    if current
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        values.push(current.clone());
    }
    current.clear();
}

pub(crate) fn identifier_occurrences(
    language: &Language,
    lines: &[&str],
    span: &FunctionSpan,
    spans: &[FunctionSpan],
) -> Vec<(usize, String)> {
    let keyword_set = keywords(language);
    let mut occurrences = Vec::new();
    for (line_number, line) in function_executable_lines(language, lines, span, spans) {
        for name in identifiers(&strip_comments(line)) {
            if !keyword_set.contains(name.as_str()) {
                occurrences.push((line_number, name));
            }
        }
    }
    occurrences
}

pub(crate) fn keywords(language: &Language) -> BTreeSet<&'static str> {
    let words = match language {
        Language::Rust => "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
        Language::Python => "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case",
        Language::JavaScript | Language::TypeScript => "abstract any as async await boolean break case catch class const continue debugger declare default delete do else enum export extends false finally for from function get if implements import in instanceof interface let new null number of private protected public readonly return set static string super switch this throw true try type typeof undefined var void while yield",
        Language::Shell => "if then else elif fi for while do done case esac select until in function return local export readonly unset",
        _ => "",
    };
    words.split_whitespace().collect()
}

pub(crate) fn split_identifier(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let trimmed = name.trim_matches('_');
    let mut previous_lower = false;
    for ch in trimmed.chars() {
        if ch == '_' || ch.is_ascii_digit() {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            previous_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }
        previous_lower = ch.is_ascii_lowercase();
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub(crate) fn numeric_literals(code: &str) -> Vec<String> {
    let chars = code.chars().collect::<Vec<_>>();
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if !starts_numeric_literal(&chars, index) {
            index += 1;
            continue;
        }
        let start = index;
        index = numeric_literal_end(&chars, index);
        // Digits glued to an identifier tail (`1_abc`) are part of a name.
        if chars.get(index) == Some(&'_') {
            index = skip(&chars, index, |item| {
                item.is_ascii_alphanumeric() || item == '_'
            });
            continue;
        }
        let value = chars[start..index]
            .iter()
            .collect::<String>()
            .replace('_', "");
        let trimmed = value.trim_matches('.');
        if !trimmed.is_empty() && trimmed.chars().any(|item| item.is_ascii_digit()) {
            values.push(trimmed.to_string());
        }
    }
    values
}

/// The first index from `index` whose character `keep` rejects.
fn skip(chars: &[char], mut index: usize, keep: impl Fn(char) -> bool) -> usize {
    while chars.get(index).is_some_and(|item| keep(*item)) {
        index += 1;
    }
    index
}

/// A numeric literal starts at `index`: a digit, `.5`, or a sign leading the
/// text, but not inside an identifier.
fn starts_numeric_literal(chars: &[char], index: usize) -> bool {
    let ch = chars[index];
    let next = chars.get(index + 1);
    let starts_decimal =
        ch.is_ascii_digit() || (ch == '.' && next.is_some_and(char::is_ascii_digit));
    let starts_signed = index == 0
        && matches!(ch, '+' | '-')
        && next.is_some_and(|next| next.is_ascii_digit() || *next == '.');
    let in_identifier =
        index > 0 && (chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_');
    (starts_decimal || starts_signed) && !in_identifier
}

/// The end of the numeric literal starting at `index`: a radix-prefixed
/// integer (`0x1F`), or a decimal with optional fraction and exponent.
fn numeric_literal_end(chars: &[char], mut index: usize) -> usize {
    let digit = |item: char| item.is_ascii_digit() || item == '_';
    if matches!(chars[index], '+' | '-') {
        index += 1;
    }
    if chars.get(index) == Some(&'0')
        && chars
            .get(index + 1)
            .is_some_and(|item| matches!(item, 'x' | 'X' | 'o' | 'O' | 'b' | 'B'))
    {
        return skip(chars, index + 2, |item| {
            item.is_ascii_hexdigit() || item == '_'
        });
    }
    index = skip(chars, index, digit);
    if chars.get(index) == Some(&'.') {
        index = skip(chars, index + 1, digit);
    }
    exponent_end(chars, index)
}

/// The end of an exponent (`e-3`) at `index`, or `index` when none follows.
fn exponent_end(chars: &[char], index: usize) -> usize {
    if !chars
        .get(index)
        .is_some_and(|item| matches!(item, 'e' | 'E'))
    {
        return index;
    }
    let mut digits = index + 1;
    if chars
        .get(digits)
        .is_some_and(|item| matches!(item, '+' | '-'))
    {
        digits += 1;
    }
    let end = skip(chars, digits, |item| item.is_ascii_digit() || item == '_');
    if end == digits {
        index
    } else {
        end
    }
}

pub(crate) fn is_trivial_numeric_literal(value: &str) -> bool {
    matches!(numeric_value(value), Some(-1.0 | 0.0 | 1.0 | 2.0))
}

/// A numeric literal's value, so `0x10` and `16` are the same number.
pub(crate) fn numeric_value(literal: &str) -> Option<f64> {
    let (sign, digits) = match literal.strip_prefix('-') {
        Some(digits) => (-1.0, digits),
        None => (1.0, literal.trim_start_matches('+')),
    };
    let radix = match digits.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => return digits.parse::<f64>().ok().map(|value| sign * value),
    };
    u64::from_str_radix(&digits[2..], radix)
        .ok()
        .map(|value| sign * value as f64)
}
