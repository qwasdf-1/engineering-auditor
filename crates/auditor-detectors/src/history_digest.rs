//! The history digest: a compact summary of a repository's recent history
//! for the reviewer. Per-file commits, growth, and co-change partners over
//! a window of `HEAD`'s first-parent line, so a review can rank hotspots by
//! change frequency and see coupling no import graph shows, without keeping
//! the clone.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::churn::{first_parent_log, git_output, head_commit_time, numstat_rows};

pub const HISTORY_DIGEST_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_HISTORY_WINDOW_DAYS: u32 = 365;

/// Days before `HEAD` whose commits count as recent.
const RECENT_DAYS: i64 = 90;
/// A commit touching more files than this is a bulk change (a reformat, a
/// vendored drop, a long branch's merge): it counts toward each file's
/// commits but not toward co-change.
const MAX_CHANGESET_FILES: usize = 30;
const MAX_PARTNERS: usize = 8;
const MIN_SHARED_COMMITS: usize = 2;
const COMMIT_MARKER: &str = "@@commit ";

struct Commit {
    sha: String,
    time: i64,
    author: String,
    /// Paths with lines added and removed.
    files: Vec<(String, i64, i64)>,
    created: Vec<String>,
}

#[derive(Default)]
struct FileHistory {
    commits: usize,
    recent: usize,
    authors: BTreeSet<String>,
    added: i64,
    removed: i64,
    created: bool,
    first: i64,
    last: i64,
    partners: HashMap<usize, usize>,
}

/// The digest of the repository at `root` over the `window_days` before its
/// `HEAD` commit, as schema-versioned JSON. Only files present at `HEAD`
/// appear. A shallow clone's boundary commits are left out (their diff is
/// the whole tree), and the digest says when the boundary cuts the window.
pub fn history_digest(root: &Path, window_days: u32) -> Result<Value, String> {
    if window_days == 0 {
        return Err("the history window must be at least one day".to_string());
    }
    let head = git_output(root, &["rev-parse", "HEAD"])?.trim().to_string();
    let head_time = head_commit_time(root).ok_or("cannot read the HEAD commit time")?;
    let since = head_time - i64::from(window_days) * 86_400;
    let boundary = shallow_boundary(root)?;
    let tracked = git_output(root, &["ls-tree", "-r", "-z", "--name-only", "HEAD"])?;
    let log = first_parent_log(
        root,
        since,
        &[
            "--relative",
            "--no-renames",
            "--numstat",
            "--summary",
            &format!("--pretty=format:{COMMIT_MARKER}%H %ct %ae"),
        ],
    )?;
    let (commits, cut): (Vec<_>, Vec<_>) = parse_log(&log)
        .into_iter()
        .partition(|commit| !boundary.contains(&commit.sha));
    let mut tally = Tally {
        tracked: tracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .collect(),
        recent_since: head_time - RECENT_DAYS * 86_400,
        ..Tally::default()
    };
    for commit in &commits {
        tally.add(commit);
    }
    let covered_from = cut.iter().map(|commit| commit.time).max();
    Ok(json!({
        "schema_version": HISTORY_DIGEST_SCHEMA_VERSION,
        "head_commit": head,
        "head_date": utc_date(head_time),
        "window_days": window_days,
        "window_start": utc_date(since),
        "recent_days": RECENT_DAYS,
        "commits": commits.len(),
        "oldest_commit_date": commits.iter().map(|commit| commit.time).min().map(utc_date),
        "shallow": !boundary.is_empty(),
        "complete": cut.is_empty(),
        "covered_from": utc_date(covered_from.unwrap_or(since).max(since)),
        "max_changeset_files": MAX_CHANGESET_FILES,
        "large_changesets": tally.large_changesets,
        "files": tally.entries(),
    }))
}

/// Per-file histories, accumulated commit by commit.
#[derive(Default)]
struct Tally<'a> {
    /// Paths present at `HEAD`; the only ones counted.
    tracked: HashSet<&'a str>,
    recent_since: i64,
    paths: Vec<&'a str>,
    index: HashMap<&'a str, usize>,
    files: Vec<FileHistory>,
    large_changesets: usize,
}

impl<'a> Tally<'a> {
    fn add(&mut self, commit: &'a Commit) {
        let mut touched = BTreeSet::new();
        for (path, added, removed) in &commit.files {
            if !self.tracked.contains(path.as_str()) {
                continue;
            }
            let file = self.file(path, commit.time);
            touched.insert(file);
            let history = &mut self.files[file];
            history.commits += 1;
            history.recent += usize::from(commit.time > self.recent_since);
            history.authors.insert(commit.author.clone());
            history.added += added;
            history.removed += removed;
            history.created |= commit.created.contains(path);
            history.first = history.first.min(commit.time);
            history.last = history.last.max(commit.time);
        }
        if touched.len() > MAX_CHANGESET_FILES {
            self.large_changesets += 1;
            return;
        }
        for &file in &touched {
            let partners = &mut self.files[file].partners;
            for &partner in touched.iter().filter(|partner| **partner != file) {
                *partners.entry(partner).or_default() += 1;
            }
        }
    }

    /// The index of `path`'s history, created at `time` when new.
    fn file(&mut self, path: &'a str, time: i64) -> usize {
        if let Some(&file) = self.index.get(path) {
            return file;
        }
        self.paths.push(path);
        self.files.push(FileHistory {
            first: time,
            last: time,
            ..FileHistory::default()
        });
        self.index.insert(path, self.files.len() - 1);
        self.files.len() - 1
    }

    /// The file entries, ordered by path.
    fn entries(&self) -> Vec<Value> {
        self.paths
            .iter()
            .zip(&self.files)
            .map(|(path, history)| (*path, file_entry(path, history, &self.paths)))
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect()
    }
}

fn file_entry(path: &str, history: &FileHistory, paths: &[&str]) -> Value {
    let mut partners = history
        .partners
        .iter()
        .filter(|(_, shared)| **shared >= MIN_SHARED_COMMITS)
        .map(|(partner, shared)| (paths[*partner], *shared))
        .collect::<Vec<_>>();
    partners.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    partners.truncate(MAX_PARTNERS);
    json!({
        "path": path,
        "commits": history.commits,
        "recent_commits": history.recent,
        "authors": history.authors.len(),
        "lines_added": history.added,
        "lines_removed": history.removed,
        "created_in_window": history.created,
        "first_change": utc_date(history.first),
        "last_change": utc_date(history.last),
        "co_changes": partners
            .into_iter()
            .map(|(path, commits)| json!({"path": path, "commits": commits}))
            .collect::<Vec<_>>(),
    })
}

fn parse_log(log: &str) -> Vec<Commit> {
    log.split(COMMIT_MARKER)
        .filter_map(|block| {
            let (header, body) = block.split_once('\n').unwrap_or((block, ""));
            let mut fields = header.split(' ');
            let sha = fields.next().filter(|sha| !sha.is_empty())?.to_string();
            let time = fields.next()?.parse().ok()?;
            let author = fields.next().unwrap_or_default().to_lowercase();
            let (rows, created) = numstat_rows(body);
            let text = |path: &Path| path.to_string_lossy().into_owned();
            Some(Commit {
                sha,
                time,
                author,
                files: rows
                    .iter()
                    .map(|(path, added, removed)| (text(path), *added, *removed))
                    .collect(),
                created: created.iter().map(|path| text(path)).collect(),
            })
        })
        .collect()
}

/// The commits a shallow clone's history stops at.
fn shallow_boundary(root: &Path) -> Result<HashSet<String>, String> {
    let path = git_output(root, &["rev-parse", "--git-path", "shallow"])?;
    let path = root.join(path.trim());
    Ok(std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .map(ToString::to_string)
        .collect())
}

/// `YYYY-MM-DD` of a Unix time, in UTC.
fn utc_date(seconds: i64) -> String {
    // Days to civil date (H. Hinnant's algorithm).
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str], date: &str) {
        let status = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn commit(root: &Path, files: &[(&str, &str)], date: &str) {
        for (path, content) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        git(root, &["add", "-A"], date);
        git(root, &["commit", "-qm", "change"], date);
    }

    #[test]
    fn utc_dates_follow_the_calendar() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(1_798_675_200), "2026-12-31");
        assert_eq!(utc_date(-86_400), "1969-12-31");
    }

    #[test]
    fn the_digest_counts_commits_growth_and_co_change_within_the_window() {
        let root = std::env::temp_dir().join(format!("ea-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"], "2025-01-01T00:00:00Z");
        commit(
            &root,
            &[("old.py", "x\n"), ("gone.py", "y\n")],
            "2024-01-01T00:00:00Z",
        );
        commit(
            &root,
            &[("a.py", "1\n"), ("b.py", "1\n")],
            "2025-03-01T00:00:00Z",
        );
        commit(
            &root,
            &[("a.py", "1\n2\n"), ("b.py", "1\n2\n")],
            "2025-04-01T00:00:00Z",
        );
        commit(&root, &[("a.py", "1\n2\n3\n")], "2025-05-01T00:00:00Z");
        std::fs::remove_file(root.join("gone.py")).unwrap();
        commit(&root, &[], "2025-05-02T00:00:00Z");

        let digest = history_digest(&root, 180).unwrap();
        assert_eq!(digest["schema_version"], 1);
        assert_eq!(digest["head_date"], "2025-05-02");
        assert_eq!(digest["window_start"], "2024-11-03");
        assert_eq!(digest["commits"], 4);
        assert_eq!(digest["shallow"], false);
        assert_eq!(digest["complete"], true);
        let files = digest["files"].as_array().unwrap();
        // `old.py` changed before the window; `gone.py` is not at HEAD.
        let paths = files
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["a.py", "b.py"]);
        let a = &files[0];
        assert_eq!(a["commits"], 3);
        assert_eq!(a["recent_commits"], 3);
        assert_eq!(a["authors"], 1);
        assert_eq!(a["lines_added"], 3);
        assert_eq!(a["created_in_window"], true);
        assert_eq!(a["first_change"], "2025-03-01");
        assert_eq!(a["last_change"], "2025-05-01");
        assert_eq!(a["co_changes"], json!([{"path": "b.py", "commits": 2}]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_shallow_boundary_inside_the_window_marks_the_digest_incomplete() {
        let base = std::env::temp_dir().join(format!("ea-history-shallow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let source = base.join("source");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q"], "2025-01-01T00:00:00Z");
        for (index, date) in ["2025-01-01", "2025-02-01", "2025-03-01"]
            .iter()
            .enumerate()
        {
            commit(
                &source,
                &[
                    ("a.py", &"x\n".repeat(index + 1)),
                    ("b.py", &index.to_string()),
                ],
                &format!("{date}T00:00:00Z"),
            );
        }
        let url = format!("file://{}", source.display());
        let clone = base.join("clone");
        let status = Command::new("git")
            .args(["clone", "-q", "--depth", "2", &url])
            .arg(&clone)
            .status()
            .unwrap();
        assert!(status.success());
        let digest = history_digest(&clone, 365).unwrap();
        assert_eq!(digest["shallow"], true);
        assert_eq!(digest["complete"], false);
        // The boundary commit (February) is left out; March remains.
        assert_eq!(digest["commits"], 1);
        assert_eq!(digest["covered_from"], "2025-02-01");
        assert_eq!(digest["files"][0]["commits"], 1);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
