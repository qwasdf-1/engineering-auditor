//! The patch checker, the deterministic half of "the model proposes, a
//! checker decides". A recommendation's patch is applied to a scratch copy
//! of the repository (the operator's tree is never written) and the copy is
//! re-scanned. The patch is verified only when the findings it cites are
//! gone, it adds no finding, and the operator's test command, if any, still
//! passes. The comparison base is a scan of an unpatched copy made the same
//! way, so a difference is the patch's.

mod command;
mod diff;

use std::cell::{Cell, OnceCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use auditor_core::sha256_digest_bytes;
use auditor_detectors::{repository_files, AnalysisRequest, GraphProvider};
use auditor_report::{AuditReport, ColorMode, NoReviewArtifactLoader};
use serde::{Deserialize, Serialize};

pub use command::{TestCommand, TestRun};

use crate::{run_scan, ScanFormat, ScanRequest, TEMPORARY_ID};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchVerdict {
    /// Applies, resolves every cited finding, adds none, and keeps the
    /// tests passing.
    Verified,
    /// Fails one of those; `step` says which.
    Refuted,
    /// Could not be judged: the cited findings do not reproduce, a scan
    /// failed, or the tests fail without the patch.
    Unchecked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStep {
    Apply,
    Rescan,
    Tests,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AddedFinding {
    pub id: String,
    pub detector: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub message: String,
}

/// One patch's verdict and the facts behind it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchCheck {
    pub verdict: PatchVerdict,
    /// The step that refuted the patch or left it unchecked.
    pub step: Option<CheckStep>,
    pub reason: Option<String>,
    pub patch_sha256: String,
    pub cited_findings: Vec<String>,
    pub touched_files: Vec<String>,
    pub resolved_findings: Vec<String>,
    /// Cited findings the patched copy still reports, suppressed ones
    /// included.
    pub remaining_findings: Vec<String>,
    pub added_findings: Vec<AddedFinding>,
    pub tests: Option<TestRun>,
}

impl PatchCheck {
    fn new(patch: &str, cited: &[String]) -> Self {
        Self {
            verdict: PatchVerdict::Unchecked,
            step: None,
            reason: None,
            patch_sha256: sha256_digest_bytes(patch.as_bytes()),
            cited_findings: cited
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            touched_files: Vec::new(),
            resolved_findings: Vec::new(),
            remaining_findings: Vec::new(),
            added_findings: Vec::new(),
            tests: None,
        }
    }
}

/// What the unpatched copy reports, and whether its tests pass.
struct Base {
    findings: BTreeSet<String>,
    tests: Option<TestRun>,
}

/// Checks patches against one repository; scratch copies live under a
/// directory removed when the checker drops.
pub struct PatchChecker<'p> {
    repo_root: PathBuf,
    files: Vec<PathBuf>,
    analysis: AnalysisRequest,
    provider: &'p dyn GraphProvider,
    tests: Option<TestCommand>,
    scratch: ScratchDirectory,
    base: OnceCell<Result<Base, String>>,
    copies: Cell<usize>,
}

impl<'p> PatchChecker<'p> {
    /// `analysis` is the review scan's request. Its map output is dropped,
    /// the paths it excludes are left out of each copy, and churn reads the
    /// original repository's history.
    pub fn new(
        repo_root: &Path,
        analysis: AnalysisRequest,
        provider: &'p dyn GraphProvider,
        tests: Option<TestCommand>,
    ) -> Result<Self, String> {
        let repo_root = repo_root
            .canonicalize()
            .map_err(|error| format!("cannot open {}: {error}", repo_root.display()))?;
        let excluded = analysis
            .excluded_paths
            .iter()
            .filter_map(|path| {
                let path = path.canonicalize().ok()?;
                Some(path.strip_prefix(&repo_root).ok()?.to_path_buf())
            })
            .collect::<BTreeSet<_>>();
        let files = repository_files(&repo_root, analysis.discovery)
            .map_err(|error| format!("cannot list the repository: {error}"))?
            .into_iter()
            .filter(|file| !excluded.contains(file))
            .collect();
        let analysis = AnalysisRequest {
            repository_map_output: None,
            excluded_paths: BTreeSet::new(),
            history_root: Some(analysis.history_root.unwrap_or_else(|| repo_root.clone())),
            ..analysis
        };
        Ok(Self {
            scratch: ScratchDirectory::new()?,
            repo_root,
            files,
            analysis,
            provider,
            tests,
            base: OnceCell::new(),
            copies: Cell::new(0),
        })
    }

    /// Checks one patch that claims to resolve `cited` findings.
    pub fn check(&self, patch: &str, cited: &[String]) -> PatchCheck {
        let mut check = PatchCheck::new(patch, cited);
        match self.judge(&mut check, patch) {
            Ok(()) => check.verdict = PatchVerdict::Verified,
            Err((verdict, step, reason)) => {
                check.verdict = verdict;
                check.step = Some(step);
                check.reason = Some(reason);
            }
        }
        check
    }

    /// Runs the steps in order; the first that fails stops the check.
    fn judge(&self, check: &mut PatchCheck, patch: &str) -> Result<(), Stop> {
        if check.cited_findings.is_empty() {
            return Err(stop(
                PatchVerdict::Unchecked,
                CheckStep::Rescan,
                "the recommendation cites no finding for its patch to resolve",
            ));
        }
        let files = diff::parse(patch)
            .map_err(|reason| stop(PatchVerdict::Refuted, CheckStep::Apply, reason))?;
        check.touched_files = files.iter().map(|file| file.path.clone()).collect();
        let base = self
            .base
            .get_or_init(|| self.base_scan())
            .as_ref()
            .map_err(|reason| stop(PatchVerdict::Unchecked, CheckStep::Rescan, reason.clone()))?;
        base.can_judge(&check.cited_findings)?;
        let copy = self
            .copy()
            .map_err(|reason| stop(PatchVerdict::Unchecked, CheckStep::Apply, reason))?;
        diff::apply(copy.path(), &files)
            .map_err(|reason| stop(PatchVerdict::Refuted, CheckStep::Apply, reason))?;
        let report = self
            .scan(copy.path())
            .map_err(|reason| stop(PatchVerdict::Unchecked, CheckStep::Rescan, reason))?;
        compare(check, base, &report)?;
        if let Some(tests) = &self.tests {
            let run = check
                .tests
                .insert(command::run(tests, copy.path(), &copy.log()));
            if !run.passed() {
                let verdict = if run.error.is_none() {
                    PatchVerdict::Refuted
                } else {
                    PatchVerdict::Unchecked
                };
                let reason = format!("the test command {} with the patch", run.outcome());
                return Err(stop(verdict, CheckStep::Tests, reason));
            }
        }
        Ok(())
    }

    /// Scans an unpatched copy, then runs the tests in it.
    fn base_scan(&self) -> Result<Base, String> {
        let copy = self.copy()?;
        let report = self.scan(copy.path())?;
        let tests = self
            .tests
            .as_ref()
            .map(|tests| command::run(tests, copy.path(), &copy.log()));
        Ok(Base {
            findings: reported_ids(&report),
            tests,
        })
    }

    fn copy(&self) -> Result<Copy, String> {
        self.copies.set(self.copies.get() + 1);
        let path = self
            .scratch
            .path
            .join(format!("copy-{}", self.copies.get()));
        let copy = Copy { path };
        for file in &self.files {
            let target = copy.path.join(file);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::copy(self.repo_root.join(file), &target)
                .map_err(|error| format!("cannot copy {}: {error}", file.display()))?;
        }
        Ok(copy)
    }

    fn scan(&self, root: &Path) -> Result<AuditReport, String> {
        run_scan(
            ScanRequest {
                repo_root: root.to_path_buf(),
                analysis: self.analysis.clone(),
                external_inputs: Vec::new(),
                baseline: None,
                format: ScanFormat::Json,
                output: None,
                color: ColorMode::Plain,
                summary_only: false,
                review_loader: Box::new(NoReviewArtifactLoader),
            },
            self.provider,
        )
        .map(|result| result.report)
    }
}

/// Finding IDs a report carries, the ones inline comments suppressed
/// included: a patch must fix a finding, not silence it.
fn reported_ids(report: &AuditReport) -> BTreeSet<String> {
    report
        .findings
        .iter()
        .map(|finding| finding.id.clone())
        .chain(
            report
                .suppressed_findings
                .iter()
                .map(|finding| finding.id.clone()),
        )
        .collect()
}

/// Why a check stopped: its verdict, the step, and the reason.
type Stop = (PatchVerdict, CheckStep, String);

fn stop(verdict: PatchVerdict, step: CheckStep, reason: impl Into<String>) -> Stop {
    (verdict, step, reason.into())
}

impl Base {
    /// Whether the unpatched copy can judge a patch: it reports every cited
    /// finding, and its tests pass.
    fn can_judge(&self, cited: &[String]) -> Result<(), Stop> {
        let missing = cited
            .iter()
            .filter(|id| !self.findings.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let reason = format!(
                "a scan of the unpatched copy does not report {}",
                missing.join(", ")
            );
            return Err(stop(PatchVerdict::Unchecked, CheckStep::Rescan, reason));
        }
        match self.tests.as_ref().filter(|run| !run.passed()) {
            Some(run) => Err(stop(
                PatchVerdict::Unchecked,
                CheckStep::Tests,
                format!(
                    "the test command {} without the patch, so it cannot judge the patch",
                    run.outcome()
                ),
            )),
            None => Ok(()),
        }
    }
}

/// Records which cited findings the patched copy resolved and which
/// findings it added; any remaining or added finding refutes the patch.
fn compare(check: &mut PatchCheck, base: &Base, report: &AuditReport) -> Result<(), Stop> {
    let reported = reported_ids(report);
    let (remaining, resolved): (Vec<_>, Vec<_>) = check
        .cited_findings
        .iter()
        .cloned()
        .partition(|id| reported.contains(id));
    check.resolved_findings = resolved;
    check.remaining_findings = remaining;
    check.added_findings = report
        .findings
        .iter()
        .filter(|finding| !base.findings.contains(&finding.id))
        .map(|finding| {
            let location = finding.locations.first();
            AddedFinding {
                id: finding.id.clone(),
                detector: finding.detector.clone(),
                path: location.map(|location| location.path.clone()),
                line: location.and_then(|location| location.line),
                message: finding.message.clone(),
            }
        })
        .collect();
    if !check.remaining_findings.is_empty() {
        let reason = format!(
            "the patched copy still reports {}",
            check.remaining_findings.join(", ")
        );
        return Err(stop(PatchVerdict::Refuted, CheckStep::Rescan, reason));
    }
    if check.added_findings.is_empty() {
        return Ok(());
    }
    let added = check
        .added_findings
        .iter()
        .map(|finding| format!("{} ({})", finding.id, finding.detector))
        .collect::<Vec<_>>();
    let reason = format!("the patch adds {}", added.join(", "));
    Err(stop(PatchVerdict::Refuted, CheckStep::Rescan, reason))
}

/// A scratch copy, removed with its output log when dropped.
struct Copy {
    path: PathBuf,
}

impl Copy {
    fn path(&self) -> &Path {
        &self.path
    }

    fn log(&self) -> PathBuf {
        self.path.with_extension("log")
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        let _ = std::fs::remove_file(self.log());
    }
}

struct ScratchDirectory {
    path: PathBuf,
}

impl ScratchDirectory {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "ea-patch-check-{}-{}",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
