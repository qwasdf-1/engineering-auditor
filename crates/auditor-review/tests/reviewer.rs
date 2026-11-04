//! The reviewer's orientation pack and retrieval tools over real scans of
//! fixture repositories.

#[path = "reviewer/fixture.rs"]
mod fixture;

use std::path::Path;

use auditor_detectors::ScanProfile;
use auditor_review::{
    load_history_digest, render_native_conversation, retrieval_tool_specs, translate_tool_spec,
    HistoryDigest, NativeConversationMessage, NativeToolCall, ProviderProtocol, RepositoryView,
    RetrievalBudget, RetrievalHost, RetrievalOutcome, ReviewBrief, ToolChoice,
    MINIMUM_ORIENTATION_BYTES, RETRIEVAL_TOOL_NAMES, TARGET_ORIENTATION_BYTES,
};
use fixture::{assert_cites_only_issued, Fixture, GO_SERVICE, PYTHON_APP};
use serde_json::{json, Value};

/// The graph-analysis build adds opt-in detectors that the pack's coverage
/// lists, so it has its own goldens.
fn golden(name: &str, actual: &str) {
    let graph_analysis = auditor_detectors::detector_registry(ScanProfile::Full)
        .iter()
        .any(|detector| detector.id == "repo_graph_import_cycle");
    let build = if graph_analysis {
        ".graph-analysis"
    } else {
        ""
    };
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/reviewer/{name}{build}.md"));
    if std::env::var_os("EA_BLESS_ORIENTATION").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        expected == actual,
        "{name} orientation pack differs from its golden; rerun with EA_BLESS_ORIENTATION=1 for an intended change:\n{actual}"
    );
}

#[test]
fn orientation_packs_match_their_goldens() {
    for (name, files) in [("python-app", PYTHON_APP), ("go-service", GO_SERVICE)] {
        let fixture = Fixture::scan(files);
        let view = fixture.view();
        let pack = view.orientation_pack(None).unwrap();
        assert!(pack.markdown.len() <= TARGET_ORIENTATION_BYTES);
        assert!(!pack.truncated);
        assert_eq!(pack.audit.markdown_bytes, pack.markdown.len());
        assert_eq!(pack.markdown, view.orientation_pack(None).unwrap().markdown);
        assert_cites_only_issued(&pack.markdown, &pack.evidence_ids);
        golden(name, &pack.markdown);
    }
}

#[test]
fn the_python_pack_names_its_modules_cycle_and_entry_points() {
    let fixture = Fixture::scan(PYTHON_APP);
    let pack = fixture.view().orientation_pack(None).unwrap();
    for expected in [
        "module:app/store",
        "module:app/core",
        "module-cycle:1",
        "main at app/main.py:10 (__main__ guard), reaching `main`",
        "app/api/routes.py: 1 route",
    ] {
        assert!(
            pack.markdown.contains(expected),
            "pack lacks {expected}:\n{}",
            pack.markdown
        );
    }
    assert!(pack
        .evidence_ids
        .contains(&fixture.symbol_id("main", "function")));
}

#[test]
fn a_brief_budget_shrinks_the_pack_and_too_small_a_budget_is_refused() {
    let fixture = Fixture::scan(PYTHON_APP);
    let view = fixture.view();
    let full = view.orientation_pack(None).unwrap();
    let mut brief = ReviewBrief::general();
    brief.budgets.max_context_bytes = Some(MINIMUM_ORIENTATION_BYTES);
    let small = view.orientation_pack(Some(&brief)).unwrap();
    assert!(small.markdown.len() <= MINIMUM_ORIENTATION_BYTES);
    if full.markdown.len() > MINIMUM_ORIENTATION_BYTES {
        assert!(small.truncated);
        assert!(small.evidence_ids.is_subset(&full.evidence_ids));
    }
    assert_cites_only_issued(&small.markdown, &small.evidence_ids);
    brief.budgets.max_context_bytes = Some(MINIMUM_ORIENTATION_BYTES - 1);
    assert!(view.orientation_pack(Some(&brief)).is_err());
}

#[test]
fn a_report_and_a_map_from_different_scans_are_refused() {
    let python = Fixture::scan(PYTHON_APP);
    let go = Fixture::scan(GO_SERVICE);
    assert!(RepositoryView::new(&python.report, &go.map).is_err());
}

fn call(id: &str, name: &str, arguments: Value) -> NativeToolCall {
    NativeToolCall::new(id, name, arguments.clone(), arguments.to_string())
}

/// Plays scripted assistant turns against the host, as a provider would
/// issue them, and returns the conversation.
fn play(
    host: &mut RetrievalHost<'_, '_>,
    turns: Vec<Vec<NativeToolCall>>,
) -> Vec<NativeConversationMessage> {
    let mut history = Vec::new();
    for calls in turns {
        let answers = calls
            .iter()
            .map(|call| host.answer(call))
            .collect::<Vec<_>>();
        history.push(NativeConversationMessage::Assistant {
            text: None,
            tool_calls: calls,
        });
        history.extend(answers);
    }
    history
}

fn result_text(message: &NativeConversationMessage) -> &str {
    match message {
        NativeConversationMessage::ToolResult {
            content_markdown, ..
        } => content_markdown,
        other => panic!("expected a tool result, got {other:?}"),
    }
}

#[test]
fn a_scripted_reviewer_navigates_from_the_pack_through_every_tool() {
    let fixture = Fixture::scan(PYTHON_APP);
    let view = fixture.view();
    let pack = view.orientation_pack(None).unwrap();
    let store = fixture.symbol_id("Store", "class");
    let mut host = RetrievalHost::new(&view, &fixture.repo, RetrievalBudget::default()).unwrap();
    let history = play(
        &mut host,
        vec![
            vec![
                call(
                    "c1",
                    "module_overview",
                    json!({"module": "module:app/store"}),
                ),
                call("c2", "search_symbols", json!({"query": "store"})),
            ],
            vec![
                call("c3", "symbol_info", json!({"symbol": store})),
                call("c4", "symbol_info", json!({"symbol": "Store.load"})),
                call("c5", "findings_in", json!({"scope": "app/store/db.py"})),
                call(
                    "c6",
                    "read_source",
                    json!({"path": "app/store/db.py", "start_line": 8, "end_line": 10}),
                ),
            ],
        ],
    );
    let answers = history
        .iter()
        .filter(|message| matches!(message, NativeConversationMessage::ToolResult { .. }))
        .map(result_text)
        .collect::<Vec<_>>();
    assert_eq!(answers.len(), 6);
    assert!(
        host.log()
            .iter()
            .all(|record| record.outcome == RetrievalOutcome::Answered),
        "{:#?}",
        host.log()
    );
    assert!(answers[0].contains("app/store/db.py") && answers[0].contains("module-cycle:1"));
    assert!(answers[1].contains("`Store` class") && answers[1].contains("`BaseStore` class"));
    assert!(
        answers[2].contains("`BaseStore` (app/store/base.py:1-6)")
            && answers[2].contains("`Store.load` function")
    );
    assert!(
        answers[3].contains("inside `Store`")
            && answers[3].contains("`record` (app/core/audit.py:4-8) at line 16")
    );
    assert!(answers[5].contains("source-span:app/store/db.py#L8-L10"));
    assert!(answers[5].contains("class Store(BaseStore):\n    def __init__(self, path=\"app.db\"):\n        self.connection"));
    let issued = host.issued_evidence_ids();
    for answer in &answers {
        let ids = answer
            .split('[')
            .skip(1)
            .filter_map(|rest| rest.split(']').next())
            .flat_map(|citation| citation.split(", "))
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| issued.contains(*id)), "{answer}");
    }
    assert!(issued.contains(&store) && pack.evidence_ids.contains("module:app/store"));
    for protocol in [
        ProviderProtocol::ChatCompletions,
        ProviderProtocol::Messages,
        ProviderProtocol::Responses,
    ] {
        let rendered = render_native_conversation(
            protocol,
            "Review the repository.",
            &pack.markdown,
            &history,
        )
        .to_string();
        assert!(rendered.contains("source-span:app/store/db.py#L8-L10"));
        let declared =
            translate_tool_spec(&retrieval_tool_specs(true), ToolChoice::Auto, protocol).unwrap();
        for name in RETRIEVAL_TOOL_NAMES {
            assert!(declared.tool_block.to_string().contains(name));
        }
    }
}

#[test]
fn bad_calls_are_answered_with_a_reason_and_issue_nothing() {
    let fixture = Fixture::scan(PYTHON_APP);
    let view = fixture.view();
    let mut host = RetrievalHost::new(&view, &fixture.repo, RetrievalBudget::default()).unwrap();
    let cases = [
        (
            call("b1", "run_shell", json!({"command": "ls"})),
            "unknown tool",
        ),
        (
            call("b2", "read_source", json!({"path": "../etc/passwd"})),
            "not a mapped file",
        ),
        (
            call(
                "b3",
                "read_source",
                json!({"path": "app/main.py", "mode": "raw"}),
            ),
            "invalid arguments",
        ),
        (
            call(
                "b4",
                "read_source",
                json!({"path": "app/main.py", "start_line": 40}),
            ),
            "outside",
        ),
        (
            call("b5", "findings_in", json!({"scope": "nowhere"})),
            "not a mapped file, module, or symbol",
        ),
        (
            call("b6", "symbol_info", json!({"symbol": "missing_function"})),
            "search_symbols",
        ),
        (
            call("b7", "module_overview", json!({"module": "lib"})),
            "not a module",
        ),
    ];
    for (call, reason) in &cases {
        let text = result_text(&host.answer(call)).to_string();
        assert!(
            text.starts_with("# Tool error") && text.contains(reason),
            "{text}"
        );
    }
    std::fs::write(fixture.repo.join("app/main.py"), "print('changed')\n").unwrap();
    let stale =
        result_text(&host.answer(&call("b8", "read_source", json!({"path": "app/main.py"}))))
            .to_string();
    assert!(stale.contains("changed since the scan"), "{stale}");
    assert!(host.issued_evidence_ids().is_empty());
    assert!(host.log().iter().all(|record| matches!(
        record.outcome,
        RetrievalOutcome::Rejected(_)
    ) && record.evidence_ids.is_empty()));
}

#[test]
fn the_retrieval_budget_bounds_calls_and_result_size() {
    let fixture = Fixture::scan(PYTHON_APP);
    let view = fixture.view();
    let mut brief = ReviewBrief::general();
    brief.budgets.max_lookup_requests = Some(2);
    let budget = RetrievalBudget::default().tightened_by(Some(&brief));
    assert_eq!(budget.max_calls, 2);
    let mut host = RetrievalHost::new(&view, &fixture.repo, budget).unwrap();
    let search = || call("s", "search_symbols", json!({"query": "e"}));
    for _ in 0..2 {
        assert!(!result_text(&host.answer(&search())).starts_with("# Tool error"));
    }
    assert!(result_text(&host.answer(&search())).contains("retrieval calls are spent"));

    let tight = RetrievalBudget {
        max_calls: 8,
        max_result_bytes: 600,
        max_total_bytes: 4_000,
    };
    let mut host = RetrievalHost::new(&view, &fixture.repo, tight).unwrap();
    let whole = result_text(&host.answer(&call(
        "r",
        "read_source",
        json!({"path": "app/store/db.py"}),
    )))
    .to_string();
    assert!(whole.contains("line range"), "{whole}");
    let listed = result_text(&host.answer(&search())).to_string();
    assert!(listed.len() <= 600 && listed.contains("(top "), "{listed}");
    assert!(RetrievalHost::new(
        &view,
        &fixture.repo,
        RetrievalBudget {
            max_calls: 0,
            ..tight
        }
    )
    .is_err());
}

fn git(repo: &Path, args: &[&str], date: &str) {
    let status = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// The Python fixture with history: `main.py` and `base.py` change
/// together in every commit, and nothing imports one from the other.
fn python_history(fixture: &Fixture) -> HistoryDigest {
    let repo = &fixture.repo;
    git(repo, &["init", "-q"], "2026-01-01T00:00:00Z");
    git(repo, &["add", "-A"], "2026-01-01T00:00:00Z");
    git(repo, &["commit", "-qm", "start"], "2026-01-01T00:00:00Z");
    for (round, date) in ["2026-02-01T00:00:00Z", "2026-03-01T00:00:00Z"]
        .iter()
        .enumerate()
    {
        for path in ["app/main.py", "app/store/base.py"] {
            let mut text = std::fs::read_to_string(repo.join(path)).unwrap();
            text.push_str(&format!("# revision {round}\n"));
            std::fs::write(repo.join(path), text).unwrap();
        }
        git(repo, &["commit", "-qam", "together"], date);
    }
    let digest = auditor_detectors::history_digest(repo, 365).unwrap();
    let path = fixture.root.join("history.json");
    std::fs::write(&path, digest.to_string()).unwrap();
    load_history_digest(&path, repo).unwrap()
}

#[test]
fn a_history_digest_ranks_change_and_answers_the_history_tool() {
    let fixture = Fixture::scan(PYTHON_APP);
    // The map hashes the scanned tree; history commits after the scan
    // change two files, so only map-level facts are read, not source.
    let digest = python_history(&fixture);
    let view = fixture.view();
    let plain = view.orientation_pack(None).unwrap();
    let view = fixture.view().with_history(&digest);
    let pack = view.orientation_pack(None).unwrap();
    assert_cites_only_issued(&pack.markdown, &pack.evidence_ids);
    for expected in [
        "Changed most (commits × lines)",
        "history:app/main.py",
        "co-change:app/main.py+app/store/base.py",
        "on the first-parent line from",
    ] {
        assert!(
            pack.markdown.contains(expected),
            "missing {expected}:\n{}",
            pack.markdown
        );
    }
    assert!(!pack.markdown.contains("no history digest was supplied"));
    assert!(plain.markdown.contains("no history digest was supplied"));
    assert_ne!(pack.audit.source_digest, plain.audit.source_digest);

    let mut host = RetrievalHost::new(&view, &fixture.repo, RetrievalBudget::default()).unwrap();
    assert!(host.tool_specs().iter().any(|spec| spec.name == "history"));
    let answers = play(
        &mut host,
        vec![vec![
            call("h1", "history", json!({"path": "app/main.py"})),
            call("h2", "history", json!({"path": "app/core/audit.py"})),
        ]],
    );
    let main = result_text(&answers[1]);
    assert!(main.contains("3 commits"), "{main}");
    assert!(
        main.contains("app/store/base.py: 3 commits together"),
        "{main}"
    );
    assert!(main.contains("no import links them"), "{main}");
    assert!(
        main.contains("[co-change:app/main.py+app/store/base.py]"),
        "{main}"
    );
    let audit = result_text(&answers[2]);
    assert!(audit.contains("1 commit"), "{audit}");
    assert_cites_only_issued(&format!("{main}{audit}"), host.issued_evidence_ids());

    let without = fixture.view();
    let mut plain_host =
        RetrievalHost::new(&without, &fixture.repo, RetrievalBudget::default()).unwrap();
    assert!(!plain_host
        .tool_specs()
        .iter()
        .any(|spec| spec.name == "history"));
    let refused = plain_host.answer(&call("h3", "history", json!({"path": "app/main.py"})));
    assert!(result_text(&refused).contains("no history digest was supplied"));
}

#[test]
fn a_history_digest_of_another_commit_is_refused() {
    let fixture = Fixture::scan(PYTHON_APP);
    let digest = python_history(&fixture);
    let mut text = serde_json::to_value(&digest).unwrap();
    text["head_commit"] = json!("0".repeat(40));
    let path = fixture.root.join("other.json");
    std::fs::write(&path, text.to_string()).unwrap();
    let error = load_history_digest(&path, &fixture.repo).unwrap_err();
    assert!(error.contains("not the repository's HEAD"), "{error}");
}
