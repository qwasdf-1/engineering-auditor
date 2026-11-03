use auditor_review::{
    render_model_document, render_native_conversation, ModelDocument, ModelField, ModelSection,
    ModelValue, NativeConversationMessage, NativeToolCall, ProviderProtocol, SemanticRecordLedger,
    TaskCallProvenance, TaskLedgerError,
};
use serde_json::json;

fn provenance<'a>(request_id: &'a str, tool_call_id: &'a str) -> TaskCallProvenance<'a> {
    TaskCallProvenance::new(request_id, tool_call_id)
}

#[test]
fn model_markdown_is_field_oriented_and_preserves_repository_json_only_as_evidence() {
    let document = ModelDocument::new(
        "Lifecycle review",
        vec![
            ModelSection::new(
                "Finding",
                vec![
                    ModelField::new("Detector", ModelValue::text("lifecycle-loop-work")),
                    ModelField::new("Severity", ModelValue::text("high")),
                    ModelField::new(
                        "Locations",
                        ModelValue::list(["src/lib.rs:12", "src/lib.rs:19"]),
                    ),
                ],
            ),
            ModelSection::new(
                "Repository evidence",
                vec![ModelField::new(
                    "config/settings.json",
                    ModelValue::repository_source("json", "{\"enabled\":true}\n"),
                )],
            ),
        ],
    );

    let rendered = render_model_document(
        &document,
        "sha256:source",
        [
            "evidence.items[].content.identity",
            "evidence.items[].content.provenance.extractor_version",
        ],
    )
    .expect("render document");

    assert!(rendered
        .markdown
        .contains("- **Detector:** lifecycle-loop-work"));
    assert!(rendered
        .markdown
        .contains("```json\n{\"enabled\":true}\n```"));
    assert!(!rendered.markdown.contains("\"Detector\":"));
    assert_eq!(
        rendered.audit.excluded_field_paths,
        vec![
            "evidence.items[].content.identity".to_string(),
            "evidence.items[].content.provenance.extractor_version".to_string(),
        ]
    );
    assert_eq!(rendered.audit.markdown_bytes, rendered.markdown.len());
}

#[test]
fn native_conversation_preserves_assistant_calls_and_markdown_tool_results() {
    let call = NativeToolCall::new(
        "call-1",
        "answer_case_probe",
        json!({"question_id": "probe:a", "answer": "unknown"}),
        "{\"question_id\":\"probe:a\",\"answer\":\"unknown\"}",
    );
    let history = vec![
        NativeConversationMessage::Assistant {
            text: Some("I need the bounded source.".to_string()),
            tool_calls: vec![call],
        },
        NativeConversationMessage::ToolResult {
            call_id: "call-1".to_string(),
            tool_name: "answer_case_probe".to_string(),
            content_markdown: "## Admission\n\nThe answer was recorded.".to_string(),
        },
    ];

    let chat = render_native_conversation(
        ProviderProtocol::ChatCompletions,
        "system",
        "# Case",
        &history,
    );
    assert_eq!(chat[2]["tool_calls"][0]["id"], "call-1");
    assert_eq!(chat[3]["role"], "tool");
    assert_eq!(
        chat[3]["content"],
        "## Admission\n\nThe answer was recorded."
    );

    let messages =
        render_native_conversation(ProviderProtocol::Messages, "system", "# Case", &history);
    assert_eq!(messages[1]["content"][1]["type"], "tool_use");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");

    let responses =
        render_native_conversation(ProviderProtocol::Responses, "system", "# Case", &history);
    assert_eq!(responses[3]["type"], "function_call");
    assert_eq!(responses[4]["type"], "function_call_output");
    assert_eq!(
        responses[4]["output"],
        "## Admission\n\nThe answer was recorded."
    );
}

#[test]
fn semantic_ledger_preserves_creation_order_independently_of_local_ids() {
    let mut ledger = SemanticRecordLedger::default();
    ledger
        .create(
            "z-premise",
            json!({"slot": "root"}),
            provenance("request-1", "call-1"),
        )
        .expect("create premise");
    ledger
        .create(
            "a-conclusion",
            json!({"slot": "conclusion", "dependency_claim_refs": ["z-premise"]}),
            provenance("request-1", "call-2"),
        )
        .expect("create conclusion");
    assert_eq!(
        ledger
            .values_in_creation_order()
            .into_iter()
            .map(|record| record.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["z-premise", "a-conclusion"]
    );

    let round_trip: SemanticRecordLedger =
        serde_json::from_value(serde_json::to_value(&ledger).unwrap()).unwrap();
    assert_eq!(
        round_trip
            .values_in_creation_order()
            .into_iter()
            .map(|record| record.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["z-premise", "a-conclusion"]
    );
}

#[test]
fn semantic_records_are_create_only_and_revision_requires_current_revision() {
    let mut ledger = SemanticRecordLedger::default();
    ledger
        .create(
            "claim-a",
            json!({"status": "substantiated"}),
            provenance("request-1", "call-1"),
        )
        .expect("create first record");

    assert_eq!(
        ledger.create(
            "claim-a",
            json!({"status": "substantiated"}),
            provenance("request-1", "call-2"),
        ),
        Err(TaskLedgerError::DuplicateReference("claim-a".to_string()))
    );
    assert_eq!(
        ledger.revise(
            "claim-a",
            0,
            "correct the status",
            json!({"status": "partially_substantiated"}),
            provenance("request-2", "call-3"),
        ),
        Err(TaskLedgerError::StaleRevision {
            reference: "claim-a".to_string(),
            expected: 0,
            actual: 1,
        })
    );

    ledger
        .revise(
            "claim-a",
            1,
            "account for the unresolved mutation boundary",
            json!({"status": "partially_substantiated"}),
            provenance("request-2", "call-4"),
        )
        .expect("revise current record");
    assert_eq!(ledger.get("claim-a").expect("record").revision, 2);
    assert_eq!(ledger.revisions().len(), 1);
    assert_eq!(
        ledger.revisions()[0].reason,
        "account for the unresolved mutation boundary"
    );
}
