use std::path::{Component, Path, PathBuf};

use crate::util;

pub(super) struct MarkdownLink {
    pub(super) line: usize,
    pub(super) raw: String,
    pub(super) target: String,
    pub(super) anchor: Option<String>,
}

pub(super) fn markdown_links(lines: &[&str]) -> Vec<MarkdownLink> {
    let mut fence: Option<&str> = None;
    let mut links = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = without_list_marker(line.trim_start());
        if let Some(marker) = ["```", "~~~"]
            .into_iter()
            .find(|marker| trimmed.starts_with(marker))
        {
            fence = match fence {
                Some(open) if open == marker => None,
                Some(open) => Some(open),
                None => Some(marker),
            };
            continue;
        }
        if fence.is_none() {
            links.extend(markdown_links_on_line(index + 1, &without_code_spans(line)));
        }
    }
    links
}

/// A fence may open inside a list item: `1. ```sh` or `- ```sh`.
fn without_list_marker(line: &str) -> &str {
    let bullet = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "));
    let ordered = line
        .split_once(". ")
        .filter(|(number, _)| !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(_, rest)| rest);
    bullet.or(ordered).map_or(line, str::trim_start)
}

/// Blank out inline code spans so link syntax shown as code is not parsed.
fn without_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
        } else {
            out.push(if in_code { ' ' } else { ch });
        }
    }
    out
}

fn markdown_links_on_line(line_number: usize, line: &str) -> Vec<MarkdownLink> {
    let mut links = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let raw = after[..end].trim();
        // `\]` is a literal bracket, not the end of link text.
        let escaped = rest[..start].ends_with('\\');
        if let Some((target, anchor)) = (!escaped).then(|| local_markdown_target(raw)).flatten() {
            links.push(MarkdownLink {
                line: line_number,
                raw: raw.to_string(),
                target,
                anchor,
            });
        }
        rest = &after[end + 1..];
    }
    links
}

fn local_markdown_target(raw: &str) -> Option<(String, Option<String>)> {
    // `path "Title"` carries an optional link title after the destination; a
    // destination with other spaces is not a link (`[=](auto out)` in C++).
    let mut words = raw.splitn(2, char::is_whitespace);
    let destination = words.next().unwrap_or(raw);
    let title = words.next().map(str::trim).unwrap_or_default();
    if !(title.is_empty()
        || destination.starts_with('<')
        || [('"', '"'), ('\'', '\''), ('(', ')')]
            .iter()
            .any(|(open, close)| title.starts_with(*open) && title.ends_with(*close)))
    {
        return None;
    }
    let raw = destination
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(destination);
    // Rustdoc intra-doc paths (`crate::module`) and template placeholders
    // (`{{ url }}`) are resolved by other tools, not the filesystem.
    if raw.contains("://")
        || raw.starts_with('/')
        || raw.starts_with('#')
        || raw.starts_with("mailto:")
        || raw.contains("::")
        || raw.contains("{{")
        || raw.contains("{%")
        || has_uri_scheme(raw)
    {
        return None;
    }
    let (target, anchor) = raw
        .split_once('#')
        .map_or((raw, None), |(target, anchor)| (target, Some(anchor)));
    (!target.is_empty()).then(|| (percent_decoded(target), anchor.map(str::to_string)))
}

/// `xref:Polly.Retry`, `tel:..`, `data:..`: a URI scheme names a resolver
/// other than the filesystem. A dot keeps `file.md:12` a path.
fn has_uri_scheme(raw: &str) -> bool {
    raw.split_once(':').is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme.starts_with(|character: char| character.is_ascii_alphabetic())
            && scheme.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-')
            })
    })
}

/// `operator%5B%5D.md` names the file `operator[].md`.
fn percent_decoded(target: &str) -> String {
    let bytes = target.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = (bytes[index] == b'%')
            .then(|| target.get(index + 1..index + 3))
            .flatten()
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        if let Some(byte) = hex {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| target.to_string())
}

pub(super) fn resolve_markdown_target(
    root: &Path,
    source_relative: &Path,
    target: &str,
) -> Option<(PathBuf, String)> {
    let target = target.split('?').next().unwrap_or(target).trim();
    let candidate = source_relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(target);
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir if normalized.pop() => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let normalized_path = util::path_slash(&normalized);
    Some((root.join(normalized), normalized_path))
}

/// Documentation sites (Docusaurus, MkDocs, VitePress) link pages without
/// their `.md`/`.mdx` extension.
/// A directory link names its index page (`use/` is `use/index.md`).
pub(super) fn existing_doc_target(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        let index = ["index.md", "index.mdx", "README.md"]
            .into_iter()
            .map(|name| path.join(name))
            .find(|candidate| candidate.is_file());
        return Some(index.unwrap_or_else(|| path.to_path_buf()));
    }
    if path.exists() {
        return Some(path.to_path_buf());
    }
    // Append rather than replace: `migrating-to-1.0.0` has no `.0` extension.
    let has_doc_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "md" | "mdx" | "html"));
    if has_doc_extension {
        return None;
    }
    [".md", ".mdx"]
        .into_iter()
        .map(|extension| {
            let mut candidate = path.as_os_str().to_owned();
            candidate.push(extension);
            PathBuf::from(candidate)
        })
        .find(|candidate| candidate.is_file())
}
