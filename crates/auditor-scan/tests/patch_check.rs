//! The patch checker over real scans of a fixture repository.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_scan::{CheckStep, PatchChecker, PatchVerdict, TestCommand};

const STORE: &str = "import subprocess\n\n\ndef record(event, value):\n    try:\n        print(event, value)\n    except:\n        pass\n\n\ndef backup(target):\n    subprocess.call(\"cp app.db \" + target, shell=True)\n";

/// Replaces the swallowed exception; `extra` lands after the `raise`.
fn fix(extra: &str) -> String {
    format!(
        "--- a/app/store.py\n+++ b/app/store.py\n@@ -5,4 +5,4 @@\n     try:\n         print(event, value)\n-    except:\n-        pass\n+    except OSError as error:\n+        raise RuntimeError(\"record failed\") from error{extra}\n"
    )
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ea-patch-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/store.py"), STORE).unwrap();
        std::fs::write(
            root.join("app/main.py"),
            "from app.store import record\n\n\ndef main():\n    record(\"start\", 1)\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/huge.py"), "except_everything = 1\n").unwrap();
        Self { root }
    }

    fn checker(&self, tests: Option<TestCommand>) -> PatchChecker<'static> {
        PatchChecker::new(
            &self.root,
            AnalysisRequest {
                discovery: SourceDiscoveryOptions {
                    include_references: false,
                    method: AnalysisMethod::Auto,
                },
                profile: ScanProfile::Full,
                scope: ScanScope::All,
                repository_map_output: None,
                excluded_paths: BTreeSet::new(),
                history_root: None,
            },
            &NoGraphProvider,
            tests,
        )
        .unwrap()
    }

    /// The fixture's finding of `detector`, from a scan of the original.
    fn finding(&self, detector: &str) -> String {
        let analysis = auditor_detectors::analyze_repository(
            &self.root,
            SourceDiscoveryOptions::default(),
            ScanProfile::Full,
        )
        .unwrap();
        analysis
            .findings
            .iter()
            .find(|finding| finding.detector == detector)
            .unwrap_or_else(|| panic!("the fixture has a {detector} finding"))
            .id
            .clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn shell(script: &str, timeout: Duration) -> TestCommand {
    TestCommand {
        argv: vec!["sh".to_string(), "-c".to_string(), script.to_string()],
        timeout,
        pass_env: Vec::new(),
    }
}

#[test]
fn a_patch_that_fixes_its_finding_is_verified_and_the_tree_is_untouched() {
    let fixture = Fixture::new();
    let swallowed = fixture.finding("try_except_pass");
    let checker = fixture.checker(Some(shell(
        "test ! -d build && test -f app/main.py",
        Duration::from_secs(30),
    )));
    let check = checker.check(&fix(""), std::slice::from_ref(&swallowed));
    assert_eq!(check.verdict, PatchVerdict::Verified, "{check:?}");
    assert_eq!(check.resolved_findings, std::slice::from_ref(&swallowed));
    assert_eq!(check.touched_files, ["app/store.py"]);
    let tests = check.tests.expect("the test command ran");
    // Ignored files are not copied; the copy is what the scan would read.
    assert!(tests.passed(), "{tests:?}");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("app/store.py")).unwrap(),
        STORE
    );

    // The same checker judges the next patch from the same base.
    let unrelated = checker.check(
        "--- a/app/main.py\n+++ b/app/main.py\n@@ -5 +5,2 @@\n     record(\"start\", 1)\n+    record(\"stop\", 0)\n",
        std::slice::from_ref(&swallowed),
    );
    assert_eq!(unrelated.verdict, PatchVerdict::Refuted);
    assert_eq!(unrelated.step, Some(CheckStep::Rescan));
    assert_eq!(unrelated.remaining_findings, [swallowed]);
}

#[test]
fn a_patch_that_adds_a_finding_silences_one_or_does_not_apply_is_refuted() {
    let fixture = Fixture::new();
    let swallowed = fixture.finding("try_except_pass");
    let checker = fixture.checker(None);
    let cited = std::slice::from_ref(&swallowed);

    let adds = checker.check(&fix("\n+    eval(value)"), cited);
    assert_eq!(adds.verdict, PatchVerdict::Refuted, "{adds:?}");
    assert!(adds.resolved_findings.contains(&swallowed));
    assert!(!adds.added_findings.is_empty());
    assert!(adds.reason.unwrap().starts_with("the patch adds"));

    let silences = checker.check(
        "--- a/app/store.py\n+++ b/app/store.py\n@@ -7,2 +7,2 @@\n-    except:\n+    except:  # auditor-ignore[try_except_pass]: logging must never fail\n         pass\n",
        cited,
    );
    assert_eq!(silences.verdict, PatchVerdict::Refuted, "{silences:?}");
    assert_eq!(
        silences.remaining_findings,
        std::slice::from_ref(&swallowed)
    );

    let stale = checker.check(
        "--- a/app/store.py\n+++ b/app/store.py\n@@ -7,2 +7,2 @@\n-    except Exception:\n+    except OSError:\n         pass\n",
        cited,
    );
    assert_eq!(stale.verdict, PatchVerdict::Refuted);
    assert_eq!(stale.step, Some(CheckStep::Apply));

    let configuration = checker.check(
        "--- a/.gitignore\n+++ b/.gitignore\n@@ -1 +1,2 @@\n build/\n+app/\n",
        cited,
    );
    assert_eq!(configuration.step, Some(CheckStep::Apply));
    assert!(configuration.reason.unwrap().contains("scan configuration"));
}

#[test]
fn unreproduced_findings_and_failing_base_tests_leave_a_patch_unchecked() {
    let fixture = Fixture::new();
    let swallowed = fixture.finding("try_except_pass");
    let checker = fixture.checker(None);
    let unknown = checker.check(&fix(""), &["EA-0000000000000000".to_string()]);
    assert_eq!(unknown.verdict, PatchVerdict::Unchecked);
    assert!(unknown
        .reason
        .unwrap()
        .contains("does not report EA-0000000000000000"));
    let uncited = checker.check(&fix(""), &[]);
    assert_eq!(uncited.verdict, PatchVerdict::Unchecked);

    let slow = fixture.checker(Some(shell("sleep 5", Duration::from_millis(300))));
    let timed_out = slow.check(&fix(""), std::slice::from_ref(&swallowed));
    assert_eq!(timed_out.verdict, PatchVerdict::Unchecked, "{timed_out:?}");
    assert_eq!(timed_out.step, Some(CheckStep::Tests));
    assert!(timed_out.reason.unwrap().contains("timed out"));

    let failing_with_patch = fixture.checker(Some(shell(
        "! grep -q RuntimeError app/store.py",
        Duration::from_secs(30),
    )));
    let refuted = failing_with_patch.check(&fix(""), std::slice::from_ref(&swallowed));
    assert_eq!(refuted.verdict, PatchVerdict::Refuted, "{refuted:?}");
    assert_eq!(refuted.step, Some(CheckStep::Tests));
    assert_eq!(refuted.tests.unwrap().exit_code, Some(1));
}
