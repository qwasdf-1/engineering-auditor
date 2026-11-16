//! Plain-text search over the repository's tracked files, including the
//! tests, build files, and configuration the map leaves out. A line in a
//! mapped file is read through the map's hash check and cites a source
//! span; a line elsewhere is context only, since nothing in the review's
//! inputs could later confirm it.

use std::process::Command;

use super::document::{BoundedDocument, Item, Row, Section};
use super::retrieval::{RetrievalHost, TextQuery};
use crate::repository_map::checked_source_path;

/// Matching lines one search shows.
const MAX_HITS: usize = 50;
/// Unmapped files larger than this are not searched.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Characters of a matching line shown.
const EXCERPT_CHARS: usize = 200;
const MIN_QUERY_CHARS: usize = 3;

/// What one search found.
#[derive(Default)]
struct Hits {
    mapped: Vec<Row>,
    unmapped: Vec<Row>,
    lines: usize,
    files: usize,
    unread: usize,
}

impl RetrievalHost<'_, '_> {
    pub(super) fn search_text(&self, request: &TextQuery) -> Result<BoundedDocument, String> {
        let query = request.query.as_str();
        if query.trim().chars().count() < MIN_QUERY_CHARS || query.contains('\n') {
            return Err(format!(
                "the query must be one line of at least {MIN_QUERY_CHARS} characters"
            ));
        }
        let scope = request
            .path
            .as_deref()
            .map(|path| path.trim_start_matches("./").trim_end_matches('/'))
            .filter(|path| !path.is_empty());
        let in_scope = |path: &str| {
            scope.is_none_or(|scope| {
                path == scope
                    || path
                        .strip_prefix(scope)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        };
        let mut hits = Hits::default();
        for path in self.tracked_files().iter().filter(|path| in_scope(path)) {
            let mapped = self.view.file_by_path(path);
            let text = match mapped {
                Some(file) => Some(self.source_text(file)?),
                None => self.unmapped_text(path),
            };
            let Some(text) = text else {
                hits.unread += 1;
                continue;
            };
            hits.add(
                path,
                mapped.map(|file| self.view.file(file).id.as_str()),
                &text,
                query,
            );
        }
        Ok(hits.document(query, scope))
    }

    /// The tracked files, from `git ls-files`; the mapped files when the
    /// root is not a git work tree.
    fn tracked_files(&self) -> Vec<String> {
        let listed = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.fsmonitor=false", "ls-files", "-z"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok());
        match listed {
            Some(listed) => listed
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(ToString::to_string)
                .collect(),
            None => self
                .view
                .map
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect(),
        }
    }

    /// An unmapped file's text; none for one that is missing, large,
    /// binary, or not UTF-8.
    fn unmapped_text(&self, path: &str) -> Option<String> {
        let path = checked_source_path(&self.root, path).ok()?;
        if std::fs::metadata(&path).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        if bytes.contains(&0) {
            return None;
        }
        String::from_utf8(bytes).ok()
    }
}

impl Hits {
    fn add(&mut self, path: &str, file_id: Option<&str>, text: &str, query: &str) {
        let mut found = false;
        for (index, line) in text.lines().enumerate() {
            if !line.contains(query) {
                continue;
            }
            found = true;
            self.lines += 1;
            if self.mapped.len() + self.unmapped.len() >= MAX_HITS {
                continue;
            }
            let number = index + 1;
            let excerpt = line.trim().chars().take(EXCERPT_CHARS).collect::<String>();
            match file_id {
                Some(file_id) => self.mapped.push(Row::new(
                    format!("{path}:{number}: {excerpt}"),
                    [
                        format!("source-span:{path}#L{number}-L{number}"),
                        file_id.to_string(),
                    ],
                )),
                None => self.unmapped.push(Row::new(
                    format!("{path}:{number}: {excerpt}"),
                    Vec::<String>::new(),
                )),
            }
        }
        self.files += usize::from(found);
    }

    fn document(self, query: &str, scope: Option<&str>) -> BoundedDocument {
        let shown = self.mapped.len() + self.unmapped.len();
        let mut summary = format!("{} matching lines in {} files", self.lines, self.files);
        if shown < self.lines {
            summary.push_str(&format!(
                "; the first {shown} are shown, so narrow the query or the path for the rest"
            ));
        }
        if self.unread > 0 {
            summary.push_str(&format!(
                "; {} large, binary, or unreadable files not searched",
                self.unread
            ));
        }
        BoundedDocument::new(
            format!("Lines containing `{query}`"),
            vec![Section::new(
                "Matches",
                vec![
                    Item::text("Searched", scope.unwrap_or("every tracked file")),
                    Item::text("Found", summary),
                    Item::rows("In mapped files", self.mapped, MAX_HITS),
                    Item::rows(
                        "In files the map leaves out (context only: not citable, not readable)",
                        self.unmapped,
                        MAX_HITS,
                    ),
                ],
            )],
        )
    }
}
