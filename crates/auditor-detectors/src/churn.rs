use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Days of history, ending at the `HEAD` commit, that count as churn.
const CHURN_WINDOW_DAYS: i64 = 14;

/// Per-file commit counts and line growth from git history.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileChurn {
    pub commits: usize,
    pub loc_delta: i64,
}

/// Collect churn for the commits reachable from `HEAD` within
/// [`CHURN_WINDOW_DAYS`] of `HEAD`'s commit time. Anchoring on the scanned
/// commit instead of the wall clock and other refs keeps a scan of one commit
/// reproducible on any day and in any clone. Returns an empty map when git is
/// unavailable or the directory is not a repository with commits.
pub fn collect_file_churn(root: &Path) -> HashMap<PathBuf, FileChurn> {
    let Some(head_time) = head_commit_time(root) else {
        return HashMap::new();
    };
    let since = head_time - CHURN_WINDOW_DAYS * 86_400;
    let output = Command::new("git")
        .arg("log")
        .arg("HEAD")
        .arg(format!("--since=@{since}"))
        .arg("--numstat")
        .arg("--summary")
        .arg("--pretty=format:commit")
        .arg("--diff-filter=AMCR")
        .current_dir(root)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let mut churn: HashMap<PathBuf, FileChurn> = HashMap::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for (rows, created) in stdout.split("commit\n").map(numstat_rows) {
        for (path, delta) in discount_moves(rows) {
            // A file's initial size is not growth; only later commits grow it.
            let delta = if created.contains(&path) { 0 } else { delta };
            let entry = churn.entry(path).or_default();
            entry.commits += 1;
            entry.loc_delta += delta;
        }
    }
    churn
}

/// One commit's `--numstat` rows as (path, lines added, lines removed), and
/// the paths its `--summary` reports created.
fn numstat_rows(block: &str) -> (Vec<(PathBuf, i64, i64)>, Vec<PathBuf>) {
    let rows = block
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let added = parts.next()?.parse::<i64>().unwrap_or(0);
            let removed = parts.next()?.parse::<i64>().unwrap_or(0);
            let path = parts.next().filter(|path| !path.is_empty())?;
            Some((PathBuf::from(path), added, removed))
        })
        .collect();
    let created = block
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("create mode "))
        .filter_map(|rest| rest.split_once(' ').map(|(_, path)| PathBuf::from(path)))
        .collect();
    (rows, created)
}

/// Each file's line growth in one commit, less the lines the commit removed
/// from other files of its language (extension): code moved by a split or
/// rename is not new growth. The largest growth absorbs the moved lines first,
/// where a move lands.
fn discount_moves(mut rows: Vec<(PathBuf, i64, i64)>) -> Vec<(PathBuf, i64)> {
    let extension = |path: &Path| path.extension().map(|value| value.to_os_string());
    let mut moved = HashMap::new();
    for (path, added, removed) in &rows {
        *moved.entry(extension(path)).or_insert(0) += (removed - added).max(0);
    }
    rows.sort_by_key(|(path, added, removed)| (std::cmp::Reverse(added - removed), path.clone()));
    rows.into_iter()
        .map(|(path, added, removed)| {
            let pool = moved.entry(extension(&path)).or_insert(0);
            let offset = (added - removed).max(0).min(*pool);
            *pool -= offset;
            (path, added - removed - offset)
        })
        .collect()
}

fn head_commit_time(root: &Path) -> Option<i64> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}
