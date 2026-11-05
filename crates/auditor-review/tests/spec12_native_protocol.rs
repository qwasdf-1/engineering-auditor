use auditor_review::{
    render_model_document, render_native_conversation, ModelDocument, ModelField, ModelSection,
    ModelValue, NativeConversationMessage, NativeToolCall, ProviderProtocol,
};
use serde_json::json;

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
