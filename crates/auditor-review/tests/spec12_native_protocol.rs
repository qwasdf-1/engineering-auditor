use std::collections::BTreeSet;

use auditor_review::{
    render_model_document, render_native_conversation, ChallengeTaskLedger,
    ChallengeTaskRequirements, ModelDocument, ModelField, ModelSection, ModelValue,
    NativeConversationMessage, NativeToolCall, ProviderProtocol, ResponseTaskLedger,
    ResponseTaskRequirements, ReviewerTaskLedger, ReviewerTaskRequirements, SemanticRecordLedger,
    StableId, TaskCallProvenance, TaskLedgerError,
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
fn challenge_completion_implicitly_clears_unobjected_targets() {
    let requirements = ChallengeTaskRequirements {
        targets: [
            ("claim:1".to_string(), "claim".to_string()),
            ("recommendation:1".to_string(), "recommendation".to_string()),
        ]
        .into_iter()
        .collect(),
    };
    let mut ledger = ChallengeTaskLedger::new(requirements);
    ledger
        .record_objection(
            "objection-a",
            "recommendation",
            "recommendation:1",
            json!({"reason": "unsafe_lifecycle_change"}),
            provenance("request-1", "call-1"),
        )
        .expect("record recommendation objection");
    ledger.complete().expect("complete challenge");
    assert_eq!(ledger.derived_status(), "objections_raised");
    let dispositions = ledger.disposition_records(
        &StableId::from_identity("reviewer-case-review", 1, &json!({"test": true})).unwrap(),
        &StableId::from_identity("challenger-run", 1, &json!({"test": true})).unwrap(),
    );
    assert!(dispositions.iter().any(|record| {
        record.target_id == "claim:1"
            && record.disposition == auditor_review::ChallengeTargetDisposition::Cleared
    }));
    assert!(dispositions.iter().any(|record| {
        record.target_id == "recommendation:1"
            && matches!(
                record.disposition,
                auditor_review::ChallengeTargetDisposition::Objections { .. }
            )
    }));
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
fn recommendation_creation_is_complete_create_only_and_explicitly_revisable() {
    let mut ledger = ReviewerTaskLedger::new(ReviewerTaskRequirements {
        probe_ids: Default::default(),
        claim_slots: ["wider_owner".to_string()].into_iter().collect(),
        recommendation_families: ["lifecycle_reownership".to_string()].into_iter().collect(),
    });
    ledger
        .record_claim(
            "lifecycle-mismatch",
            "wider_owner",
            json!({
                "slot": "wider_owner",
                "dependency_claim_refs": []
            }),
            provenance("request-1", "call-premise"),
        )
        .expect("create recommendation premise");
    let recommendation = json!({
        "recommendation_ref": "hoist-sort",
        "family": "lifecycle_reownership",
        "summary": "Hoist the invariant sort",
        "scope_ids": ["symbol:owner"],
        "premise_claim_refs": ["lifecycle-mismatch"],
        "affected_finding_ids": ["EA-1"],
        "detector_advice_relationship": "preserves",
        "preconditions": ["inputs remain equivalent"],
        "risks": ["future mutation"],
        "costs": ["one source edit"],
        "materiality": "low",
        "materiality_basis": "bounded source change",
        "confidence": "high",
        "expected_benefits": [{
            "description": "removes redundant sorts",
            "evidence_mode": "structural_inference",
            "evidence_probe_ids": ["case-probe:benefit"]
        }],
        "alternatives": [{
            "summary": "Leave the code unchanged",
            "tradeoffs": ["retains redundant work"]
        }],
        "no_alternative_reason": null,
        "verification_steps": [{
            "method": "rerun the detector",
            "expected_observation": "the finding clears",
            "evidence_probe_ids": ["case-probe:benefit"]
        }]
    });
    ledger
        .record_recommendation(
            "hoist-sort",
            "lifecycle_reownership",
            recommendation.clone(),
            provenance("request-1", "call-recommendation"),
        )
        .expect("record complete recommendation");
    assert!(matches!(
        ledger.record_recommendation(
            "hoist-sort",
            "lifecycle_reownership",
            recommendation.clone(),
            provenance("request-1", "call-duplicate"),
        ),
        Err(TaskLedgerError::DuplicateReference(_))
    ));
    let mut replacement = recommendation;
    replacement["summary"] = json!("Hoist the invariant sort after confirming mutation guards");
    ledger
        .revise_recommendation(
            "hoist-sort",
            1,
            "Clarify the mutation precondition.",
            replacement,
            provenance("request-2", "call-revision"),
        )
        .expect("revise recommendation explicitly");
    let value = &ledger
        .recommendations()
        .get("hoist-sort")
        .expect("recorded recommendation")
        .value;
    assert_eq!(
        ledger.recommendations().get("hoist-sort").unwrap().revision,
        2
    );
    assert!(value["summary"]
        .as_str()
        .unwrap()
        .contains("mutation guards"));
    assert_eq!(value["expected_benefits"].as_array().unwrap().len(), 1);
    assert_eq!(value["alternatives"].as_array().unwrap().len(), 1);
    assert_eq!(value["verification_steps"].as_array().unwrap().len(), 1);
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
fn reviewer_ledger_rejects_claim_dependencies_before_they_become_unrepairable() {
    let mut ledger = ReviewerTaskLedger::new(ReviewerTaskRequirements {
        probe_ids: BTreeSet::new(),
        claim_slots: BTreeSet::from(["premise".to_string(), "conclusion".to_string()]),
        recommendation_families: BTreeSet::new(),
    });
    let invalid = ledger.record_claim(
        "conclusion",
        "conclusion",
        json!({"slot": "conclusion", "dependency_claim_refs": ["premise"]}),
        provenance("request-1", "call-1"),
    );
    assert!(matches!(invalid, Err(TaskLedgerError::InvalidComponent(_))));
    assert!(ledger.claims().get("conclusion").is_none());

    ledger
        .record_claim(
            "premise",
            "premise",
            json!({"slot": "premise", "dependency_claim_refs": []}),
            provenance("request-1", "call-2"),
        )
        .expect("create premise first");
    ledger
        .record_claim(
            "conclusion",
            "conclusion",
            json!({"slot": "conclusion", "dependency_claim_refs": ["premise"]}),
            provenance("request-1", "call-3"),
        )
        .expect("create conclusion after premise");

    let invalid_revision = ledger.revise_claim(
        "premise",
        1,
        "introduce a backward dependency",
        json!({"slot": "premise", "dependency_claim_refs": ["conclusion"]}),
        provenance("request-2", "call-4"),
    );
    assert!(matches!(
        invalid_revision,
        Err(TaskLedgerError::InvalidComponent(_))
    ));
    assert_eq!(ledger.claims().get("premise").unwrap().revision, 1);
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

#[test]
fn reviewer_completion_requires_probe_answers_and_derives_other_bookkeeping() {
    let requirements = ReviewerTaskRequirements {
        probe_ids: BTreeSet::from(["probe:a".to_string()]),
        claim_slots: BTreeSet::new(),
        recommendation_families: BTreeSet::from(["lifecycle_reownership".to_string()]),
    };
    let mut ledger = ReviewerTaskLedger::new(requirements);

    assert!(matches!(
        ledger.complete(None, vec![]),
        Err(TaskLedgerError::IncompleteTasks(_))
    ));

    ledger
        .answer_probe(
            "probe:a",
            json!({"answer": "yes"}),
            provenance("request-1", "probe-call"),
        )
        .expect("answer probe");
    ledger.complete(None, vec![]).expect("complete ledger");
    assert!(ledger.is_complete());
    assert_eq!(ledger.recommendation_family_disposition_records().len(), 1);
}

#[test]
fn challenge_records_are_create_only_and_explicitly_revisable() {
    let mut ledger = ChallengeTaskLedger::new(ChallengeTaskRequirements {
        targets: [("claim-a".to_string(), "claim".to_string())]
            .into_iter()
            .collect(),
    });
    let objection = json!({
        "objection_ref": "objection-a",
        "target_type": "claim",
        "target_id": "claim-a",
        "reason": "unsupported_assumption"
    });
    ledger
        .record_objection(
            "objection-a",
            "claim",
            "claim-a",
            objection.clone(),
            provenance("request-1", "call-1"),
        )
        .expect("create objection");
    assert!(matches!(
        ledger.record_objection(
            "objection-a",
            "claim",
            "claim-a",
            objection,
            provenance("request-2", "call-2"),
        ),
        Err(TaskLedgerError::DuplicateReference(reference)) if reference == "objection-a"
    ));
    ledger.complete().expect("complete challenge");

    ledger
        .revise_objection(
            "objection-a",
            1,
            "tighten the required resolution",
            json!({
                "objection_ref": "objection-a",
                "target_type": "claim",
                "target_id": "claim-a",
                "reason": "ignored_counterevidence"
            }),
            provenance("request-3", "call-3"),
        )
        .expect("revise objection");
    assert!(!ledger.is_complete());
    ledger.complete().expect("recomplete challenge");

    ledger
        .withdraw_objection(
            "objection-a",
            2,
            "the counterevidence was superseded",
            provenance("request-4", "call-4"),
        )
        .expect("withdraw objection");
    ledger
        .complete()
        .expect("withdrawn objection leaves the target implicitly cleared");
    ledger
        .reopen_objection(
            "objection-a",
            3,
            "the counterevidence remains applicable",
            provenance("request-5", "call-5"),
        )
        .expect("reopen objection");
    ledger.complete().expect("complete reopened challenge");
    assert_eq!(ledger.objections().revisions().len(), 3);
}

#[test]
fn response_records_require_revision_tools_and_active_replacements() {
    let objection_id = "challenge-objection:a";
    let mut ledger = ResponseTaskLedger::new(ResponseTaskRequirements {
        objection_ids: [objection_id.to_string()].into_iter().collect(),
    });
    ledger
        .record_replacement_recommendation(
            "replacement-a",
            json!({"recommendation_ref": "replacement-a", "summary": "narrow"}),
            provenance("request-1", "call-1"),
        )
        .expect("create replacement");
    let response = json!({
        "objection_id": objection_id,
        "action": "narrow",
        "replacement_recommendation_ref": "replacement-a"
    });
    ledger
        .record_response(
            objection_id,
            response.clone(),
            provenance("request-1", "call-2"),
        )
        .expect("create response");
    assert!(matches!(
        ledger.record_response(
            objection_id,
            response,
            provenance("request-2", "call-3"),
        ),
        Err(TaskLedgerError::DuplicateReference(reference)) if reference == objection_id
    ));
    ledger.complete().expect("complete response");

    ledger
        .revise_response(
            objection_id,
            1,
            "make the concession explicit",
            json!({
                "objection_id": objection_id,
                "action": "concede",
                "replacement_recommendation_ref": "replacement-a"
            }),
            provenance("request-3", "call-4"),
        )
        .expect("revise response");
    assert!(!ledger.is_complete());
    ledger
        .withdraw_replacement_recommendation(
            "replacement-a",
            1,
            "replacement needs another pass",
            provenance("request-4", "call-5"),
        )
        .expect("withdraw replacement");
    assert!(matches!(
        ledger.complete(),
        Err(TaskLedgerError::UnknownReference(reference)) if reference == "replacement-a"
    ));
    ledger
        .reopen_replacement_recommendation(
            "replacement-a",
            2,
            "replacement is now complete",
            provenance("request-5", "call-6"),
        )
        .expect("reopen replacement");
    ledger.complete().expect("complete response after reopen");
}
