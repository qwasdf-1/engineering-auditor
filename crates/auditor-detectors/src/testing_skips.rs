//! `skipped_test_without_reason`: tests disabled with no reason written
//! down.

use auditor_ast::RepoAst;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::AstScan;
use crate::testing_scope::{self, compact};
use crate::{util, AnalysisMethod, AuditedFile};

/// Tests and test groups disabled with no reason in the mark or in a
/// comment beside it.
pub(crate) fn skipped_test_without_reason_findings(
    files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut RepoAst,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    testing_scope::visit_skip_scope(
        AstScan::test_scope(files, method, cache, repo_ast),
        |file, tree| {
            let relative = util::path_slash(&file.relative);
            let root = tree.root_node();
            let comments = testing_scope::comments(root);
            let tests = testing_scope::test_cases(root, file)
                .into_iter()
                .filter_map(|test| Some(("Test", test.name, test.skip?, Some(test.body))));
            let groups = testing_scope::skipped_groups(root, file)
                .into_iter()
                .map(|group| ("Test group", group.name, group.mark, None));
            for (kind, name, mark, body) in tests.chain(groups) {
                if mark.stated || testing_scope::explained_by_comment(&mark, &comments, body) {
                    continue;
                }
                let line = mark.node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "skipped_test_without_reason",
                        "test_quality",
                        "Skipped test without a reason",
                        format!("{kind} `{name}` is skipped at line {line} with no stated reason"),
                        Severity::Low,
                        Confidence::High,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Say why the test is skipped, in the skip itself (`#[ignore = \"..\"]`, `@Disabled(\"..\")`, `pytest.mark.skip(reason=..)`, `t.Skip(\"..\")`) or in a comment beside it, so it can be re-enabled once the cause is gone.",
                    )
                    .with_evidence("test", name.as_str())
                    .with_evidence("mark", compact(mark.node.text()))
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}
