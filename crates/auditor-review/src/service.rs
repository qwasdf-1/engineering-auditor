use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::TranscriptRecorder;

const TRANSCRIPT_ENV_VAR: &str = "AUDITOR_REVIEW_TRANSCRIPT";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[repr(usize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStage {
    Preparing,
    Planning,
    CompilingEvidence,
    ResolvingProbes,
    Qualifying,
    Reviewing,
    Reconsidering,
    Comparing,
    Challenging,
    Responding,
    Finalizing,
    Publishing,
}

impl ReviewStage {
    pub const fn as_str(self) -> &'static str {
        const NAMES: [&str; 12] = [
            "preparing",
            "planning",
            "compiling_evidence",
            "resolving_probes",
            "qualifying",
            "reviewing",
            "reconsidering",
            "comparing",
            "challenging",
            "responding",
            "finalizing",
            "publishing",
        ];
        NAMES[self as usize]
    }
}

pub trait ReviewControl: Send + Sync {
    fn checkpoint(&self, stage: ReviewStage) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UncontrolledReview;

impl ReviewControl for UncontrolledReview {
    fn checkpoint(&self, _stage: ReviewStage) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewServiceError {
    Cancelled,
    Failed(String),
}

impl ReviewServiceError {
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl fmt::Display for ReviewServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("engineering review was cancelled"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ReviewServiceError {}

pub(crate) fn transcript_enabled_from_env() -> bool {
    std::env::var_os(TRANSCRIPT_ENV_VAR)
        .map(|value| value != "0" && !value.is_empty())
        .unwrap_or(false)
}

pub(crate) fn publish_transcript(
    transcript: &TranscriptRecorder,
    output: &Path,
    review_id: Option<&str>,
) -> Result<(), ReviewServiceError> {
    let path = transcript_path(output);
    let payload = transcript.to_json(review_id);
    let text = serde_json::to_string_pretty(&payload).map_err(|error| {
        ReviewServiceError::Failed(format!("failed to serialize transcript: {error}"))
    })?;
    atomic_write(&path, &text)
}

pub(crate) fn publish_failure_transcript(
    transcript: &TranscriptRecorder,
    output: &Path,
    review_id: Option<&str>,
) {
    let _ = publish_transcript(transcript, output, review_id);
}

fn transcript_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audit");
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("json");
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}.transcript.{extension}"))
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), ReviewServiceError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            ReviewServiceError::Failed(format!("failed to create output directory: {error}"))
        })?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, contents).map_err(|error| {
        ReviewServiceError::Failed(format!("failed to write temporary output: {error}"))
    })?;
    std::fs::rename(&temporary, path).map_err(|error| {
        ReviewServiceError::Failed(format!(
            "failed to publish output {}: {error}",
            path.display()
        ))
    })
}
