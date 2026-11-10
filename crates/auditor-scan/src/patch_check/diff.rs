//! Unified diffs as a model writes them: parsed leniently (a hunk's header
//! counts are ignored and its body decides its extent), and applied exactly
//! by [`apply`].

use std::collections::BTreeSet;
use std::path::{Component, Path};

mod apply;

pub(super) use apply::apply;

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Modify,
    Create,
    Delete,
}

#[derive(Debug)]
pub(super) struct FilePatch {
    pub(super) path: String,
    pub(super) change: Change,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    /// The 1-based line the header places the hunk at: a hint.
    old_start: usize,
    lines: Vec<Line>,
    /// `\ No newline at end of file` after the hunk's last new line.
    new_ends_without_newline: bool,
}

#[derive(Debug)]
enum Line {
    Context(String),
    Removed(String),
    Added(String),
}

/// Files whose edit could hide findings instead of fixing them: ignore
/// rules, Linguist attributes, and the auditor's configuration.
const SCAN_CONFIGURATION: [&str; 6] = [
    ".gitignore",
    ".ignore",
    ".eaignore",
    ".gitattributes",
    "auditor.toml",
    ".slop.toml",
];

pub(super) fn parse(patch: &str) -> Result<Vec<FilePatch>, String> {
    let lines = patch.lines().collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if is_file_header(&lines, index) {
            let (file, next) = parse_file(&lines, index)?;
            files.push(file);
            index = next;
            continue;
        }
        if line.starts_with("GIT binary patch") || line.starts_with("Binary files ") {
            return Err("binary patches are not supported".to_string());
        }
        for (prefix, what) in [
            ("rename from", "renames"),
            ("copy from", "copies"),
            ("old mode", "mode changes"),
        ] {
            if line.starts_with(prefix) {
                return Err(format!("{what} are not supported; patch file contents"));
            }
        }
        index += 1;
    }
    if files.is_empty() {
        return Err("the patch changes no file (no `---`/`+++` header)".to_string());
    }
    let mut paths = BTreeSet::new();
    for file in &files {
        check_path(&file.path)?;
        if !paths.insert(file.path.as_str()) {
            return Err(format!("the patch changes `{}` twice", file.path));
        }
    }
    Ok(files)
}

fn is_file_header(lines: &[&str], index: usize) -> bool {
    lines[index].starts_with("--- ")
        && lines
            .get(index + 1)
            .is_some_and(|next| next.starts_with("+++ "))
}

fn parse_file(lines: &[&str], start: usize) -> Result<(FilePatch, usize), String> {
    let (old, new) = header_paths(&lines[start][4..], &lines[start + 1][4..]);
    let (path, change) = match (old, new) {
        (None, Some(new)) => (new, Change::Create),
        (Some(old), None) => (old, Change::Delete),
        (Some(old), Some(new)) if old == new => (new, Change::Modify),
        (Some(old), Some(new)) => {
            return Err(format!(
                "`{old}` to `{new}` is a rename, which is not supported; patch file contents"
            ))
        }
        (None, None) => return Err("a file header names /dev/null on both sides".to_string()),
    };
    let mut index = start + 2;
    let mut hunks = Vec::new();
    while index < lines.len() && lines[index].starts_with("@@") {
        let (hunk, next) = parse_hunk(lines, index, &path)?;
        hunks.push(hunk);
        index = next;
    }
    if hunks.is_empty() {
        return Err(format!("`{path}` has a header but no hunk"));
    }
    Ok((
        FilePatch {
            path,
            change,
            hunks,
        },
        index,
    ))
}

/// The two header paths, `/dev/null` as `None`, and git's `a/`, `b/`
/// prefixes removed when both sides carry them.
fn header_paths(old: &str, new: &str) -> (Option<String>, Option<String>) {
    let clean = |raw: &str| {
        let raw = raw.split('\t').next().unwrap_or(raw).trim();
        (raw != "/dev/null").then(|| raw.to_string())
    };
    let (old, new) = (clean(old), clean(new));
    let prefixed = old.as_deref().is_none_or(|path| path.starts_with("a/"))
        && new.as_deref().is_none_or(|path| path.starts_with("b/"));
    if prefixed {
        let strip = |path: Option<String>| path.map(|path| path[2..].to_string());
        (strip(old), strip(new))
    } else {
        (old, new)
    }
}

fn parse_hunk(lines: &[&str], start: usize, path: &str) -> Result<(Hunk, usize), String> {
    let old_start = lines[start]
        .strip_prefix("@@ -")
        .and_then(|rest| {
            rest.split(|character: char| !character.is_ascii_digit())
                .next()
        })
        .and_then(|number| number.parse::<usize>().ok())
        .ok_or_else(|| format!("`{}` is not a hunk header", lines[start]))?;
    let mut hunk = Hunk {
        old_start,
        lines: Vec::new(),
        new_ends_without_newline: false,
    };
    // Blank lines a model wrote for empty context lines; trailing ones
    // separate hunks instead.
    let mut trailing_blanks = 0;
    let mut index = start + 1;
    while index < lines.len() {
        let line = lines[index];
        if line.starts_with("@@") || is_file_header(lines, index) {
            break;
        }
        // The marker is one ASCII byte, so the text after it is a slice.
        let text = line.get(1..).unwrap_or_default().to_string();
        match line.as_bytes().first() {
            None => {
                hunk.lines.push(Line::Context(String::new()));
                trailing_blanks += 1;
                index += 1;
                continue;
            }
            Some(b' ') => hunk.lines.push(Line::Context(text)),
            Some(b'-') => hunk.lines.push(Line::Removed(text)),
            Some(b'+') => hunk.lines.push(Line::Added(text)),
            Some(b'\\') => {
                if matches!(hunk.lines.last(), Some(Line::Added(_) | Line::Context(_))) {
                    hunk.new_ends_without_newline = true;
                }
            }
            _ => break,
        }
        trailing_blanks = 0;
        index += 1;
    }
    hunk.lines.truncate(hunk.lines.len() - trailing_blanks);
    if hunk.lines.is_empty() {
        return Err(format!("a hunk of `{path}` is empty"));
    }
    Ok((hunk, index))
}

/// A repository-relative path that stays inside the copy and leaves the
/// scan's configuration alone.
fn check_path(path: &str) -> Result<(), String> {
    let relative = Path::new(path);
    let normal = !path.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !relative
            .components()
            .any(|component| component.as_os_str() == ".git");
    if !normal {
        return Err(format!("`{path}` is not a repository-relative path"));
    }
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if SCAN_CONFIGURATION.contains(&name) {
        return Err(format!(
            "the patch edits `{path}`, scan configuration that could hide findings instead of fixing them"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creations_deletions_renames_and_unsafe_paths_are_classified() {
        let files = parse(
            "diff --git a/new.py b/new.py\nnew file mode 100644\n--- /dev/null\n+++ b/new.py\n@@ -0,0 +1 @@\n+x = 1\n--- a/old.py\n+++ /dev/null\n@@ -1 +0,0 @@\n-y = 2\n",
        )
        .unwrap();
        assert_eq!(files[0].path, "new.py");
        assert_eq!(files[0].change, Change::Create);
        assert_eq!(files[1].path, "old.py");
        assert_eq!(files[1].change, Change::Delete);
        for (patch, expected) in [
            ("--- a/x\n+++ b/y\n@@ -1 +1 @@\n-a\n+b\n", "rename"),
            (
                "--- a/../x\n+++ b/../x\n@@ -1 +1 @@\n-a\n+b\n",
                "not a repository-relative path",
            ),
            (
                "--- a/.git/config\n+++ b/.git/config\n@@ -1 +1 @@\n-a\n+b\n",
                "not a repository-relative path",
            ),
            (
                "--- a/src/.gitignore\n+++ b/src/.gitignore\n@@ -1 +1 @@\n-a\n+b\n",
                "scan configuration",
            ),
            ("just prose", "changes no file"),
            ("--- a/x\n+++ b/x\n", "no hunk"),
        ] {
            let error = parse(patch).unwrap_err();
            assert!(error.contains(expected), "{patch}: {error}");
        }
    }
}
