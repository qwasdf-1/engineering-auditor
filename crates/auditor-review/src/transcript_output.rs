//! Transcript output: a review writes its provider transcript beside its
//! report when `AUDITOR_REVIEW_TRANSCRIPT` is set. The review driver being
//! rebuilt (v1.6 plan, Phase A step 5) is its caller; until then nothing
//! calls it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::TranscriptRecorder;

const TRANSCRIPT_ENV_VAR: &str = "AUDITOR_REVIEW_TRANSCRIPT";

pub(crate) fn transcript_enabled_from_env() -> bool {
    std::env::var_os(TRANSCRIPT_ENV_VAR)
        .map(|value| value != "0" && !value.is_empty())
        .unwrap_or(false)
}

pub(crate) fn publish_transcript(
    transcript: &TranscriptRecorder,
    output: &Path,
    review_id: Option<&str>,
) -> Result<(), String> {
    let path = transcript_path(output);
    let payload = transcript.to_json(review_id);
    let text = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize transcript: {error}"))?;
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

fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create output directory: {error}"))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, contents)
        .map_err(|error| format!("failed to write temporary output: {error}"))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("failed to publish output {}: {error}", path.display()))
}
