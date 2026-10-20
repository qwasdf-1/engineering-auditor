use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use auditor_core::{Confidence, Finding, Severity};

use crate::util;
use crate::{AuditedFile, Language};

#[path = "markdown_link_parser.rs"]
mod link_parser;
use link_parser::{existing_doc_target, markdown_links, resolve_markdown_target};

const RESOLVABLE_DOC_PREFIXES: &[&str] = &[
    "docs/",
    "runtime/",
    "apps/",
    "contracts/",
    "tooling/",
    "provider-packs/",
    "charters/",
];

const EXCLUDED_DOC_PREFIXES: &[&str] =
    &["apps/operator/node_modules", "apps/operator/src-tauri/gen"];

#[derive(Clone, Copy)]
enum FileReferenceKind {
    Backtick,
    Link,
}

struct FileReference {
    line: usize,
    raw: String,
    target: String,
}

struct ReferenceSpec {
    detector: &'static str,
    title: &'static str,
    subject: &'static str,
    recommendation: &'static str,
    target_evidence: &'static str,
}

impl FileReferenceKind {
    fn spec(self) -> ReferenceSpec {
        match self {
            Self::Backtick => ReferenceSpec {
                detector: "stale_path_reference",
                title: "Backticked path reference does not resolve",
                subject: "Backticked path reference",
                recommendation:
                    "Confirm whether the path is stale, generated later, or intentionally external.",
                target_evidence: "normalized_reference",
            },
            Self::Link => ReferenceSpec {
                detector: "stale_link_reference",
                title: "Markdown link reference does not resolve",
                subject: "Markdown link",
                recommendation: "Update the link to point to an existing file or remove it.",
                target_evidence: "normalized_path",
            },
        }
    }

    fn scans(self, relative: &str) -> bool {
        matches!(self, Self::Link) || should_scan_markdown_refs(relative)
    }

    fn references(self, lines: &[&str]) -> Vec<FileReference> {
        match self {
            Self::Backtick => backtick_references(lines),
            Self::Link => markdown_links(lines)
                .into_iter()
                .map(|link| FileReference {
                    line: link.line,
                    raw: link.raw,
                    target: link.target,
                })
                .collect(),
        }
    }

    /// The normalized target when it does not resolve to an existing file.
    fn missing(self, root: &Path, file: &AuditedFile, target: &str) -> Option<String> {
        match self {
            Self::Backtick => (!root.join(target).exists()).then(|| target.to_string()),
            Self::Link => resolve_markdown_target(root, &file.relative, target)
                .filter(|(path, _)| existing_doc_target(path).is_none())
                .map(|(_, normalized)| normalized),
        }
    }

    fn finding(self, relative: String, reference: FileReference, normalized: String) -> Finding {
        let spec = self.spec();
        auditor_core::finding!(
            spec.detector,
            "staleness",
            spec.title,
            format!(
                "{} `{}` does not resolve to `{normalized}`.",
                spec.subject, reference.raw
            ),
            Severity::Low,
            Confidence::High,
            relative,
            Some(reference.line),
        )
        .with_recommendation(spec.recommendation)
        .with_identity("reference", reference.raw.clone())
        .with_evidence("reference", reference.raw)
        .with_evidence(spec.target_evidence, normalized)
    }
}

pub(crate) fn stale_path_reference_findings(
    root: &Path,
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    stale_file_reference_findings(root, files, cache, FileReferenceKind::Backtick)
}

/// The MyST `(label)=` targets a file's lines declare.
fn myst_targets(lines: &[&str]) -> Vec<String> {
    lines.iter().filter_map(|line| myst_target(line)).collect()
}

fn stale_file_reference_findings(
    root: &Path,
    files: &[AuditedFile],
    cache: &util::FileCache,
    kind: FileReferenceKind,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let markdown = || {
        files
            .iter()
            .filter(|file| file.language == Language::Markdown)
    };
    let myst_targets = match kind {
        FileReferenceKind::Link => markdown()
            .flat_map(|file| myst_targets(&cache.lines(file)))
            .collect(),
        FileReferenceKind::Backtick => BTreeSet::new(),
    };
    for file in markdown() {
        let rel = util::path_slash(&file.relative);
        if kind.scans(&rel) {
            let lines = cache.lines(file);
            findings.extend(file_stale_references(
                root,
                file,
                &lines,
                kind,
                &myst_targets,
            ));
        }
    }
    findings
}

/// One Markdown file's references whose target is missing.
fn file_stale_references(
    root: &Path,
    file: &AuditedFile,
    lines: &[&str],
    kind: FileReferenceKind,
    myst_targets: &BTreeSet<String>,
) -> Vec<Finding> {
    let rel = util::path_slash(&file.relative);
    // A site page with a front-matter `slug:` (Docusaurus, Hugo) links pages
    // by URL route; extensionless targets are not file paths.
    let routed = declares_slug(lines);
    let mut findings = Vec::new();
    for reference in kind.references(lines) {
        let target = reference.target.as_str();
        let route = Path::new(target).extension().is_none();
        let resolved = (routed && route)
            || matches!(kind, FileReferenceKind::Link)
                && (myst_targets.contains(target)
                    || (route && resolves_as_site_route(root, &file.relative, target)));
        if resolved {
            continue;
        }
        if let Some(normalized) = kind.missing(root, file, target) {
            findings.push(kind.finding(rel.clone(), reference, normalized));
        }
    }
    findings
}

/// A MyST target line, `(labels/hug-parens)=`, which Sphinx resolves as a
/// link destination anywhere in the docs.
fn myst_target(line: &str) -> Option<String> {
    let label = line.trim().strip_prefix('(')?.strip_suffix(")=")?;
    (!label.is_empty() && !label.contains(char::is_whitespace)).then(|| label.to_string())
}

/// An extensionless link a documentation site resolves from the page's URL
/// rather than its file: MkDocs serves `docs/async.md` at `docs/async/`, and
/// Eleventy-style sites serve `docs/src/pages/index.md` from `docs/src/`.
fn resolves_as_site_route(root: &Path, source: &Path, target: &str) -> bool {
    let page_directory = source.with_extension("").join("index.md");
    let components = source.components().collect::<Vec<_>>();
    let site_root = components
        .iter()
        .position(|component| component.as_os_str() == "pages")
        .map(|pages| {
            components[..pages]
                .iter()
                .chain(&components[pages + 1..])
                .collect::<PathBuf>()
        });
    std::iter::once(page_directory)
        .chain(site_root)
        .filter_map(|base| resolve_markdown_target(root, &base, target))
        .any(|(path, _)| existing_doc_target(&path).is_some())
}

fn declares_slug(lines: &[&str]) -> bool {
    lines.first().is_some_and(|line| line.trim() == "---")
        && lines
            .iter()
            .skip(1)
            .take_while(|line| line.trim() != "---")
            .any(|line| line.trim_start().starts_with("slug:"))
}

fn should_scan_markdown_refs(rel: &str) -> bool {
    matches!(
        rel,
        "README.md"
            | "docs/README.md"
            | "docs/project-reference.md"
            | "docs/architecture.md"
            | "docs/current-state.md"
            | "docs/architecture-decisions.md"
            | "docs/open-work.md"
            | "docs/TODOS.md"
            | "docs/documentation-layout.md"
            | "docs/issue-ledger.md"
    ) || rel.starts_with("docs/v1/")
        || rel.starts_with("docs/reference/")
}

fn backtick_references(lines: &[&str]) -> Vec<FileReference> {
    let mut refs = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let mut rest = *line;
        while let Some(start) = rest.find('`') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('`') else {
                break;
            };
            let raw = after[..end].trim().to_string();
            if let Some(normalized) = normalize_doc_ref(&raw) {
                refs.push(FileReference {
                    line: index + 1,
                    raw,
                    target: normalized,
                });
            }
            rest = &after[end + 1..];
        }
    }
    refs
}

fn has_nonsensical_chars(value: &str) -> bool {
    value.is_empty()
        || value.contains("://")
        || value.starts_with('/')
        || value.starts_with('~')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains('*')
        || value.contains('{')
        || value.contains('}')
        || value.contains('$')
        || value.contains('<')
        || value.contains('>')
        || value.contains('|')
}

fn strip_qualified_ref(raw: &str) -> String {
    let command_or_path = raw.split_whitespace().next().unwrap_or(raw);
    let mut value = command_or_path
        .split('#')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if let Some((path, _symbol)) = value.split_once("::") {
        value = path.to_string();
    }
    if let Some((path, suffix)) = value.rsplit_once(':') {
        if suffix.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
            value = path.to_string();
        }
    }
    value
}

fn normalize_doc_ref(raw: &str) -> Option<String> {
    let value = strip_qualified_ref(raw);
    let prefixed = |prefixes: &[&str]| prefixes.iter().any(|p| value.starts_with(p));
    (!has_nonsensical_chars(&value)
        && prefixed(RESOLVABLE_DOC_PREFIXES)
        && !prefixed(EXCLUDED_DOC_PREFIXES))
    .then(|| value.trim_end_matches('/').to_string())
}

pub(crate) fn stale_link_reference_findings(
    root: &Path,
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    stale_file_reference_findings(root, files, cache, FileReferenceKind::Link)
}

fn slugify_header(header: &str) -> String {
    let mut slug = String::new();
    let mut last_was_hyphen = false;
    for ch in header.trim().to_ascii_lowercase().chars() {
        match ch {
            'a'..='z' | '0'..='9' | '_' => {
                slug.push(ch);
                last_was_hyphen = false;
            }
            ' ' | '-' | '.' | '/' if !last_was_hyphen => {
                slug.push('-');
                last_was_hyphen = true;
            }
            _ => {}
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// GitHub's heading id: lowercase, drop punctuation other than `-` and `_`,
/// and turn each space into `-` without collapsing runs.
fn github_slug(header: &str) -> String {
    header
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|ch| match ch {
            ' ' => Some('-'),
            '-' | '_' => Some(ch),
            ch if ch.is_alphanumeric() => Some(ch),
            _ => None,
        })
        .collect()
}

/// Splits a trailing explicit id (`## Title {#custom-id}`) from heading text.
fn explicit_heading_id(content: &str) -> (&str, Option<&str>) {
    content
        .trim_end()
        .strip_suffix('}')
        .and_then(|rest| rest.rsplit_once("{#"))
        .map_or((content, None), |(text, id)| {
            (text.trim_end(), Some(id.trim()))
        })
}

/// `id="..."` and `name="..."` attributes inside HTML tags on the line.
fn html_anchor_ids(line: &str) -> impl Iterator<Item = &str> {
    line.split('<').skip(1).flat_map(|tag| {
        let tag = tag.split('>').next().unwrap_or_default();
        ["id=\"", "name=\""]
            .into_iter()
            .filter_map(move |attribute| {
                let value = tag.split_once(attribute)?.1;
                value.split_once('"').map(|(id, _)| id)
            })
    })
}

/// Heading text as rendered: link text for `[text](url)` and `[text][ref]`,
/// without code-span backticks or `*`/`_` emphasis delimiters.
fn rendered_heading(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        out.push_str(&after[..close]);
        let tail = &after[close + 1..];
        rest = match tail.chars().next() {
            Some('(') => tail.find(')').map_or(tail, |end| &tail[end + 1..]),
            Some('[') => tail.find(']').map_or(tail, |end| &tail[end + 1..]),
            _ => tail,
        };
    }
    out.push_str(rest);
    out.chars()
        .filter(|character| !matches!(character, '`' | '*'))
        .collect::<String>()
}

/// A setext heading: a text line underlined with `===` or `---`.
fn setext_heading<'a>(line: &'a str, next: Option<&str>) -> Option<&'a str> {
    let underline = next?.trim();
    let text = line.trim();
    (!text.is_empty()
        && !text.starts_with(['#', '-', '*', '>', '|', '<'])
        && underline.len() >= 2
        && (underline.chars().all(|character| character == '=')
            || underline.chars().all(|character| character == '-')))
    .then_some(text)
}

fn collect_section_anchors<T: AsRef<str>>(lines: &[T]) -> Vec<String> {
    let mut anchors = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        anchors.extend(html_anchor_ids(line).map(str::to_string));
        let trimmed = line.trim();
        let content = match setext_heading(line, lines.get(index + 1).map(AsRef::as_ref)) {
            Some(text) => text,
            None if trimmed.starts_with('#') => trimmed.trim_start_matches('#').trim(),
            None => continue,
        };
        if content.is_empty() {
            continue;
        }
        let (text, explicit) = explicit_heading_id(content);
        anchors.extend(explicit.map(str::to_string));
        let text = text
            .split('<')
            .enumerate()
            .map(|(index, part)| {
                if index == 0 {
                    part
                } else {
                    part.split_once('>').map_or(part, |p| p.1)
                }
            })
            .collect::<String>();
        let rendered = rendered_heading(&text);
        // GitHub drops `_` emphasis delimiters too (`_Black_`).
        let plain = rendered.replace('_', "");
        for slug in [
            slugify_header(&text),
            github_slug(&text),
            github_slug(&rendered),
            github_slug(&plain),
        ] {
            if !slug.is_empty() {
                anchors.push(slug);
            }
        }
        // A repeated heading's id gets `-1`, `-2`, .. on GitHub.
        let count = seen.entry(github_slug(&rendered)).or_insert(0);
        if *count > 0 {
            anchors.push(format!("{}-{count}", github_slug(&rendered)));
        }
        *count += 1;
    }
    anchors
}

pub(crate) fn stale_section_anchor_findings(
    root: &Path,
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut anchor_cache: std::collections::HashMap<PathBuf, Vec<String>> =
        std::collections::HashMap::new();
    for file in files
        .iter()
        .filter(|file| file.language == Language::Markdown)
    {
        let rel = util::path_slash(&file.relative);
        for link in markdown_links(&cache.lines(file)) {
            let Some(anchor) = link.anchor else {
                continue;
            };
            if is_line_range_anchor(&anchor) {
                continue;
            }
            let Some((target_path, normalized_target)) =
                resolve_markdown_target(root, &file.relative, &link.target)
            else {
                continue;
            };
            // A directory without an index page has no anchors to check.
            let Some(target_path) = existing_doc_target(&target_path).filter(|path| path.is_file())
            else {
                continue;
            };
            let anchors = anchor_cache
                .entry(target_path.clone())
                .or_insert_with(|| collect_section_anchors(&util::read_lines(&target_path)));
            if !anchors.contains(&anchor) {
                findings.push(
                    auditor_core::finding!(
                        "stale_section_anchor",
                        "staleness",
                        "Markdown section anchor does not resolve",
                        format!("Markdown link `{}` references anchor `#{anchor}` which does not exist in `{normalized_target}`.", link.raw),
                        Severity::Low,
                        Confidence::High,
                        rel.clone(),
                        Some(link.line),
                    )
                    .with_recommendation(
                        "Update the anchor to point to an existing section heading in the target file.",
                    )
                    .with_identity("reference", link.raw.clone())
                    .with_evidence("reference", link.raw)
                    .with_evidence("target_file", normalized_target)
                    .with_evidence("missing_anchor", anchor),
                );
            }
        }
    }
    findings
}

fn is_line_range_anchor(anchor: &str) -> bool {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    anchor
        .strip_prefix('L')
        .is_some_and(|rest| match rest.split_once("-L") {
            Some((start, end)) => digits(start) && digits(end),
            None => digits(rest),
        })
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
