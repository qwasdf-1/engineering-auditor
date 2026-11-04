//! The history digest a review reads (`contracts/history-digest.schema.json`):
//! per-file commits, growth, and co-change partners, built by
//! `auditor review history` and checked here against the repository's
//! `HEAD`.

use std::path::Path;
use std::process::Command;

use auditor_core::sha256_digest_bytes;
use serde::{Deserialize, Serialize};

pub const HISTORY_DIGEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryDigest {
    pub schema_version: u32,
    pub head_commit: String,
    pub head_date: String,
    pub window_days: u32,
    pub window_start: String,
    pub recent_days: u32,
    pub commits: usize,
    pub oldest_commit_date: Option<String>,
    pub shallow: bool,
    /// False when a shallow clone's boundary falls inside the window.
    pub complete: bool,
    pub covered_from: String,
    pub max_changeset_files: usize,
    pub large_changesets: usize,
    pub files: Vec<FileHistory>,
    /// SHA-256 of the digest's bytes, which a review's projections bind.
    #[serde(skip)]
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileHistory {
    pub path: String,
    pub commits: usize,
    pub recent_commits: usize,
    pub authors: usize,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub created_in_window: bool,
    pub first_change: String,
    pub last_change: String,
    pub co_changes: Vec<CoChange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoChange {
    pub path: String,
    pub commits: usize,
}

impl HistoryDigest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut digest: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid history digest: {error}"))?;
        if digest.schema_version != HISTORY_DIGEST_SCHEMA_VERSION {
            return Err(format!(
                "unsupported history digest schema version {}",
                digest.schema_version
            ));
        }
        digest.sha256 = sha256_digest_bytes(text.as_bytes());
        Ok(digest)
    }
}

/// Reads a digest and checks it summarizes the history of `repo_root`'s
/// current `HEAD`.
pub fn load_history_digest(path: &Path, repo_root: &Path) -> Result<HistoryDigest, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read history digest {}: {error}", path.display()))?;
    let digest = HistoryDigest::parse(&text)?;
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|error| format!("cannot run git to check the history digest: {error}"))?;
    let head = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || head.trim() != digest.head_commit {
        return Err(format!(
            "the history digest summarizes commit {}, not the repository's HEAD",
            digest.head_commit
        ));
    }
    Ok(digest)
}
