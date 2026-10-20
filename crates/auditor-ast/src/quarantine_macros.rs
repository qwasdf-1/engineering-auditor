//! Macros in a C or C++ declaration header that the grammar reads as code:
//! calling conventions and export or inline markers (`static DWORD WINAPI
//! run(..)`, `GTEST_API_ void Init(..)`, `local int inflate_fast(..)`), and
//! attribute macros with arguments (`JSON_HEDLEY_NON_NULL(1) bool parse(..)`).
//! Each candidate is a guess the caller keeps only when blanking it lets the
//! declaration parse.

use std::ops::Range;

/// Most candidates a header yields.
const MAX_CANDIDATES: usize = 6;

/// Macros that expand to a calling convention, linkage, or inline marker.
const KNOWN_MACROS: &[&str] = &[
    "WINAPI",
    "WSAAPI",
    "APIENTRY",
    "CALLBACK",
    "NTAPI",
    "STDMETHODCALLTYPE",
    "DECLSPEC_IMPORT",
    "INLINE",
    "FORCEINLINE",
];

/// Name endings of export, visibility, and attribute macros.
const MACRO_SUFFIXES: &[&str] = &[
    "_API",
    "_API_",
    "_EXPORT",
    "_EXTERN",
    "_INLINE",
    "_NOEXCEPT",
    "_NODISCARD",
    "_DEPRECATED",
    "_HIDDEN",
    "_VISIBLE",
    "_DLL",
];

/// C and C++ keywords that start or qualify a declaration.
const KEYWORDS: &[&str] = &[
    "auto",
    "bool",
    "char",
    "class",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "double",
    "enum",
    "explicit",
    "extern",
    "float",
    "friend",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "noexcept",
    "operator",
    "register",
    "restrict",
    "short",
    "signed",
    "static",
    "struct",
    "template",
    "thread_local",
    "typedef",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
];

struct Word {
    span: Range<usize>,
    /// The span through the balanced parentheses that follow the word.
    call: Option<Range<usize>>,
}

/// The byte ranges in `source[range]`'s header (before its first `{`, `;`,
/// or `=` outside parentheses) that may be macros, most likely first.
pub(super) fn candidates(source: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let words = header_words(source, range);
    let text = |word: &Word| &source[word.span.clone()];
    // The declared name: the last word before a parameter list.
    let name = words.iter().rposition(|word| word.call.is_some());
    let mut ranked = Vec::new();
    for (index, word) in words.iter().enumerate() {
        if Some(index) == name || KEYWORDS.contains(&text(word)) {
            continue;
        }
        let shouting = is_shouting(text(word));
        let rank = if KNOWN_MACROS.contains(&text(word)) {
            0
        } else if shouting
            && MACRO_SUFFIXES
                .iter()
                .any(|suffix| text(word).ends_with(suffix))
        {
            1
        } else if shouting {
            2
        } else if index == 0 && words.len() >= 3 {
            // `local int f(..)`: a lowercase word before a type and a name.
            3
        } else {
            continue;
        };
        let span = match &word.call {
            Some(call) if shouting => call.clone(),
            Some(_) => continue,
            None => word.span.clone(),
        };
        ranked.push((rank, span));
    }
    ranked.sort_by_key(|(rank, span)| (*rank, span.start));
    ranked
        .into_iter()
        .map(|(_, span)| span)
        .take(MAX_CANDIDATES)
        .collect()
}

fn is_shouting(word: &str) -> bool {
    word.len() >= 2
        && word.chars().any(|character| character.is_ascii_uppercase())
        && word.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// The identifiers outside parentheses in the declaration header.
fn header_words(source: &str, range: Range<usize>) -> Vec<Word> {
    let bytes = source.as_bytes();
    let mut words = Vec::new();
    let mut index = range.start;
    while index < range.end {
        match bytes[index] {
            b'{' | b';' | b'=' => break,
            b'(' => index = closing(bytes, index, range.end),
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let word = word_at(bytes, index, range.end);
                index = word.span.end;
                words.push(word);
            }
            _ => index += 1,
        }
    }
    words
}

/// The identifier starting at `start`, with the call that follows it.
fn word_at(bytes: &[u8], start: usize, end: usize) -> Word {
    let is_word = |byte: &u8| byte.is_ascii_alphanumeric() || *byte == b'_';
    let stop = start
        + bytes[start..end]
            .iter()
            .take_while(|byte| is_word(byte))
            .count();
    let next = stop
        + bytes[stop..end]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
    Word {
        span: start..stop,
        call: (next < end && bytes[next] == b'(').then(|| start..closing(bytes, next, end)),
    }
}

/// The byte after the parenthesis closing the one at `open`.
fn closing(bytes: &[u8], open: usize, end: usize) -> usize {
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().take(end).skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    end
}
