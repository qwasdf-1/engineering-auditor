//! Unified diffs as a model writes them: parsed leniently (a hunk's header
//! counts are ignored and its body decides its extent), applied exactly
//! (every context and removed line must match the file, though a hunk may
//! sit at another line than its header says).

use std::collections::BTreeSet;
use std::path::{Component, Path};

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

/// Applies every file's hunks in memory, then writes; nothing is written
/// unless every hunk applies.
pub(super) fn apply(root: &Path, files: &[FilePatch]) -> Result<(), String> {
    let mut results = Vec::new();
    for file in files {
        let target = root.join(&file.path);
        let patched = apply_hunks(&current_text(&target, file)?, &file.hunks, &file.path)?;
        let delete = file.change == Change::Delete;
        if delete && !patched.is_empty() {
            return Err(format!(
                "deleting `{}` must remove every line of it",
                file.path
            ));
        }
        results.push((target, delete, patched));
    }
    for (target, delete, patched) in results {
        let written = if delete {
            std::fs::remove_file(&target)
        } else {
            target
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&target, patched))
        };
        written.map_err(|error| format!("cannot write {}: {error}", target.display()))?;
    }
    Ok(())
}

/// The text a file patch applies to: nothing for a new file.
fn current_text(target: &Path, file: &FilePatch) -> Result<String, String> {
    if file.change == Change::Create {
        return match target.exists() {
            true => Err(format!("`{}` already exists", file.path)),
            false => Ok(String::new()),
        };
    }
    if !target.is_file() {
        return Err(format!("`{}` is not a repository file", file.path));
    }
    let bytes = std::fs::read(target).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| format!("`{}` is not UTF-8 text", file.path))
}

fn apply_hunks(content: &str, hunks: &[Hunk], path: &str) -> Result<String, String> {
    let ending = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    let mut output: Vec<String> = Vec::new();
    let mut cursor = 0;
    let mut drift = 0isize;
    for (number, hunk) in hunks.iter().enumerate() {
        let hint = (hunk.old_start.saturating_sub(1) as isize + drift).max(0) as usize;
        let old = hunk.old_lines();
        let at = locate(&lines, &old, cursor, hint).ok_or_else(|| {
            format!(
                "hunk {} of `{path}` does not match the file near line {}{}",
                number + 1,
                hunk.old_start,
                first_difference(&lines, &old, cursor, hint)
            )
        })?;
        drift = at as isize - hunk.old_start.saturating_sub(1) as isize;
        output.extend(lines[cursor..at].iter().map(ToString::to_string));
        cursor = hunk.splice(&lines, at, ending, &mut output);
    }
    // The last hunk decides the file's final newline only if it reaches
    // the end.
    let ends_without_newline = (cursor == lines.len() && !hunks.is_empty()).then(|| {
        hunks
            .last()
            .is_some_and(|hunk| hunk.new_ends_without_newline)
    });
    output.extend(lines[cursor..].iter().map(ToString::to_string));
    terminate_lines(&mut output, ending, ends_without_newline);
    Ok(output.concat())
}

impl Hunk {
    /// The context and removed lines: what the file must contain.
    fn old_lines(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                Line::Context(text) | Line::Removed(text) => Some(text.as_str()),
                Line::Added(_) => None,
            })
            .collect()
    }

    /// Writes the hunk's new lines for the old ones at `at` (context lines
    /// keep the file's text); returns the line after them.
    fn splice(&self, lines: &[&str], at: usize, ending: &str, output: &mut Vec<String>) -> usize {
        let mut source = at;
        for line in &self.lines {
            match line {
                Line::Context(_) => {
                    output.push(lines[source].to_string());
                    source += 1;
                }
                Line::Removed(_) => source += 1,
                Line::Added(text) => output.push(format!("{text}{ending}")),
            }
        }
        source
    }
}

/// Ends every line but the last with `ending`; the last as the final hunk
/// says when it reached the end of the file, unchanged otherwise.
fn terminate_lines(output: &mut [String], ending: &str, ends_without_newline: Option<bool>) {
    let Some((last, rest)) = output.split_last_mut() else {
        return;
    };
    for line in rest.iter_mut().filter(|line| !line.ends_with('\n')) {
        line.push_str(ending);
    }
    match ends_without_newline {
        Some(true) => {
            let kept = last.trim_end_matches(['\n', '\r']).len();
            last.truncate(kept);
        }
        Some(false) if !last.ends_with('\n') => last.push_str(ending),
        _ => {}
    }
}

/// Where `old` matches whole lines at or after `cursor`, nearest `hint`.
fn locate(lines: &[&str], old: &[&str], cursor: usize, hint: usize) -> Option<usize> {
    let hint = hint.max(cursor);
    if old.is_empty() {
        return Some(hint.min(lines.len()));
    }
    let last = lines.len().checked_sub(old.len())?;
    (cursor..=last)
        .filter(|at| matching_lines(lines, old, *at) == old.len())
        .min_by_key(|at| (at.abs_diff(hint), *at))
}

/// How many of `old`'s lines match the file's at `at`.
fn matching_lines(lines: &[&str], old: &[&str], at: usize) -> usize {
    old.iter()
        .zip(&lines[at..])
        .filter(|(expected, line)| line_text(line) == **expected)
        .count()
}

fn line_text(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

/// For a hunk that matches nowhere, its first line that differs from the
/// file where most of it matches, so whoever wrote it can correct it.
fn first_difference(lines: &[&str], old: &[&str], cursor: usize, hint: usize) -> String {
    let Some(last) = lines.len().checked_sub(old.len()) else {
        return format!(": its {} old lines run past the end of the file", old.len());
    };
    let best = (cursor.min(last)..=last).max_by_key(|at| {
        (
            matching_lines(lines, old, *at),
            std::cmp::Reverse(at.abs_diff(hint)),
        )
    });
    let Some((at, offset)) = best.and_then(|at| {
        let offset = old
            .iter()
            .zip(&lines[at..])
            .position(|(expected, line)| line_text(line) != *expected)?;
        Some((at, offset))
    }) else {
        return String::new();
    };
    let quoted = |text: &str| text.chars().take(160).collect::<String>();
    format!(
        ": its line {} reads `{}` where line {} of the file reads `{}`",
        offset + 1,
        quoted(old[offset]),
        at + offset + 1,
        quoted(line_text(lines[at + offset]))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patched(content: &str, patch: &str) -> Result<String, String> {
        let files = parse(patch)?;
        apply_hunks(content, &files[0].hunks, &files[0].path)
    }

    #[test]
    fn a_hunk_applies_where_its_context_matches_even_off_its_header() {
        let content = "a\nb\nc\nd\ne\n";
        let patch = "--- a/f.py\n+++ b/f.py\n@@ -1,3 +1,3 @@\n c\n-d\n+D\n e\n";
        assert_eq!(patched(content, patch).unwrap(), "a\nb\nc\nD\ne\n");
    }

    #[test]
    fn a_mismatched_context_is_refused() {
        let patch = "--- a/f.py\n+++ b/f.py\n@@ -1,2 +1,2 @@\n x\n-b\n+B\n";
        let error = patched("a\nb\n", patch).unwrap_err();
        assert!(error.contains("hunk 1 of `f.py` does not match"), "{error}");
    }

    #[test]
    fn a_mismatch_names_the_line_that_differs_where_most_of_the_hunk_matches() {
        let content = "a\nb\n// see #well-known\nc\nd\n";
        let patch = "--- a/f.go\n+++ b/f.go\n@@ -9,4 +9,4 @@\n b\n // see #wellknown\n c\n-d\n+D\n";
        let error = patched(content, patch).unwrap_err();
        assert!(
            error.ends_with(
                "near line 9: its line 2 reads `// see #wellknown` where line 3 of the file reads `// see #well-known`"
            ),
            "{error}"
        );
        let long = "--- a/f.go\n+++ b/f.go\n@@ -1,6 +1,6 @@\n a\n b\n c\n d\n e\n-f\n+F\n";
        assert!(patched(content, long)
            .unwrap_err()
            .ends_with(": its 6 old lines run past the end of the file"));
    }

    #[test]
    fn blank_context_lines_and_missing_final_newlines_are_kept() {
        let content = "def f():\n\n    return 1";
        let patch = "--- f.py\n+++ f.py\n@@ -1,3 +1,3 @@\n def f():\n\n-    return 1\n\\ No newline at end of file\n+    return 2\n\\ No newline at end of file\n\n";
        assert_eq!(patched(content, patch).unwrap(), "def f():\n\n    return 2");
        let appended = "--- f.py\n+++ f.py\n@@ -3 +3,2 @@\n     return 1\n+x = 1\n";
        assert_eq!(
            patched(content, appended).unwrap(),
            "def f():\n\n    return 1\nx = 1\n"
        );
    }

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

    #[test]
    fn crlf_files_keep_their_line_endings() {
        let patch = "--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n";
        assert_eq!(patched("a\r\nb\r\n", patch).unwrap(), "a\r\nc\r\n");
    }
}
