use serde_json::{json, Value};

use super::candidate::{
    CandidateSelection, CaseKind, DecisionKind, ReviewCandidate, SelectionReason,
};
use super::index::AgendaIndex;
use super::normalize::normalize_candidates;
use super::planning_ops::has_reviewable_findings;
use super::selector::BuiltInSelectorKind;
use super::suppress::{SuppressionReason, SuppressionRecord};

fn finding(
    id: &str,
    detector: &str,
    subject: &str,
    recommendation: &str,
) -> (Value, Vec<Value>, Vec<Value>) {
    let graph_id = format!("finding:{id}");
    let support_id = format!("measurement:{id}");
    (
        json!({
            "id": id,
            "detector": detector,
            "recommendation": recommendation,
        }),
        vec![
            json!({"id": graph_id, "kind": "finding", "label": id, "properties": {}}),
            json!({"id": support_id, "kind": "measurement", "label": id, "properties": {}}),
        ],
        vec![
            json!({"from": support_id, "to": graph_id, "kind": "supports", "properties": {}}),
            json!({"from": support_id, "to": subject, "kind": "derived_from", "properties": {}}),
            json!({"from": graph_id, "to": format!("detector:{detector}"), "kind": "produced_by", "properties": {}}),
        ],
    )
}

fn fixture_report() -> Value {
    let specifications = [
        ("EA-one", "structure_owner", "symbol:a", "Use shared owner."),
        (
            "EA-two",
            "algorithm_owner",
            "symbol:a",
            "  use SHARED owner. ",
        ),
        (
            "EA-three",
            "repeated_sort_in_loop",
            "symbol:b",
            "Move invariant sorting.",
        ),
        (
            "EA-four",
            "structure_owner",
            "symbol:c",
            "Use shared owner.",
        ),
    ];
    let mut findings = Vec::new();
    let mut nodes = vec![
        json!({"id": "symbol:a", "kind": "symbol", "label": "a", "properties": {}}),
        json!({"id": "symbol:b", "kind": "symbol", "label": "b", "properties": {}}),
        json!({"id": "symbol:c", "kind": "symbol", "label": "c", "properties": {}}),
        json!({"id": "file:shared", "kind": "file", "label": "src/shared.rs", "properties": {}}),
    ];
    let mut edges = vec![
        json!({"from": "file:shared", "to": "symbol:a", "kind": "contains", "properties": {}}),
        json!({"from": "file:shared", "to": "symbol:b", "kind": "contains", "properties": {}}),
    ];
    for (id, detector, subject, recommendation) in specifications {
        let (report_finding, graph_nodes, graph_edges) =
            finding(id, detector, subject, recommendation);
        findings.push(report_finding);
        nodes.extend(graph_nodes);
        edges.extend(graph_edges);
    }
    for detector in [
        "structure_owner",
        "algorithm_owner",
        "repeated_sort_in_loop",
        "gap_owner",
        "unsupported_owner",
    ] {
        nodes.push(
            json!({"id": format!("detector:{detector}"), "kind": "detector", "label": detector, "properties": {}}),
        );
    }
    json!({
        "detectors": [
            {"id": "structure_owner", "layer": "L2"},
            {"id": "algorithm_owner", "layer": "L3"},
            {"id": "repeated_sort_in_loop", "layer": "L3"},
            {"id": "gap_owner", "layer": "L4"},
            {"id": "unsupported_owner", "layer": "L4"}
        ],
        "findings": findings,
        "capability_executions": [
            {"detector_id": "gap_owner", "layer": "L4", "status": "missing_input", "eligible_files": 3},
            {"detector_id": "unsupported_owner", "layer": "L4", "status": "unsupported", "eligible_files": 3}
        ],
        "evidence_graph": {"nodes": nodes, "edges": edges}
    })
}

fn selection(
    selector_id: &'static str,
    subjects: Vec<&str>,
    findings: Vec<&str>,
    reason: SelectionReason,
) -> ReviewCandidate {
    ReviewCandidate::from_selection(CandidateSelection {
        selector_id,
        selector_version: 1,
        decision_kind: DecisionKind::EvaluateSharedRootCause,
        subject_ids: subjects.into_iter().map(str::to_string).collect(),
        finding_ids: findings.into_iter().map(str::to_string).collect(),
        selection_evidence_refs: vec!["measurement:EA-one".to_string()],
        eligible_case_kinds: vec![CaseKind::SharedRootCause],
        selection_reasons: vec![reason],
    })
    .unwrap()
}

#[test]
fn candidate_round_trip_rejects_unknown_wrong_version_and_duplicate_sets() {
    let candidate = selection(
        "test_selector",
        vec!["symbol:a"],
        vec!["EA-one"],
        SelectionReason::SameSubjectConcentration,
    );
    assert_eq!(
        ReviewCandidate::from_json(&candidate.to_canonical_json()).unwrap(),
        candidate
    );
    let mut value = serde_json::to_value(&candidate).unwrap();
    value["claim"] = json!("conclusion");
    assert!(ReviewCandidate::from_json(&value.to_string()).is_err());
    let mut value = serde_json::to_value(&candidate).unwrap();
    value["schema_version"] = json!(2);
    assert!(ReviewCandidate::from_json(&value.to_string()).is_err());
    let mut value = serde_json::to_value(&candidate).unwrap();
    value["subject_ids"] = json!(["symbol:a", "symbol:a"]);
    assert!(ReviewCandidate::from_json(&value.to_string()).is_err());
}

#[test]
fn public_schema_documents_are_closed_and_versioned() {
    for schema in [
        include_str!("../../../../../contracts/review-candidate-v1.schema.json"),
        include_str!("../../../../../contracts/review-agenda-v1.schema.json"),
    ] {
        let value: Value = serde_json::from_str(schema).unwrap();
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["additionalProperties"], false);
        assert_eq!(value["properties"]["schema_version"]["const"], 1);
    }
}

#[test]
fn every_selector_has_a_positive_and_semantic_negative() {
    let index = AgendaIndex::from_report(&fixture_report()).unwrap();
    let selected = BuiltInSelectorKind::ALL
        .iter()
        .map(|kind| (*kind, kind.select(&index).unwrap()))
        .collect::<Vec<_>>();
    let same_subject = &selected[0].1;
    assert_eq!(same_subject.len(), 1);
    assert_eq!(same_subject[0].finding_ids, ["EA-one", "EA-two"]);
    assert!(!same_subject[0].finding_ids.contains(&"EA-four".to_string()));

    let lifecycle = &selected[1].1;
    assert_eq!(lifecycle.len(), 1);
    assert_eq!(lifecycle[0].finding_ids, ["EA-three"]);

    let recommendation = &selected[2].1;
    assert_eq!(recommendation.len(), 1);
    assert_eq!(recommendation[0].finding_ids, ["EA-one", "EA-two"]);
    assert!(!recommendation[0]
        .finding_ids
        .contains(&"EA-four".to_string()));

    let cross_layer = &selected[3].1;
    assert_eq!(cross_layer.len(), 1);
    assert_eq!(cross_layer[0].subject_ids, ["symbol:a"]);

    let capability = &selected[4].1;
    assert_eq!(capability.len(), 1);
    assert_eq!(capability[0].subject_ids, ["detector:gap_owner"]);
    // No template can review a finding-free capability gap; planning must
    // suppress it rather than plan a case that instantiation rejects.
    assert!(!has_reviewable_findings(&capability[0]));
    assert!(selected[..4]
        .iter()
        .flat_map(|(_, candidates)| candidates)
        .all(has_reviewable_findings));
}

#[test]
fn shuffled_report_and_selector_order_are_identity_stable() {
    let report = fixture_report();
    let first_index = AgendaIndex::from_report(&report).unwrap();
    let mut shuffled = report;
    shuffled["findings"].as_array_mut().unwrap().reverse();
    shuffled["evidence_graph"]["nodes"]
        .as_array_mut()
        .unwrap()
        .reverse();
    shuffled["evidence_graph"]["edges"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let second_index = AgendaIndex::from_report(&shuffled).unwrap();
    let first = BuiltInSelectorKind::ALL
        .iter()
        .flat_map(|kind| kind.select(&first_index).unwrap())
        .collect::<Vec<_>>();
    let mut second = BuiltInSelectorKind::ALL
        .iter()
        .rev()
        .flat_map(|kind| kind.select(&second_index).unwrap())
        .collect::<Vec<_>>();
    second.reverse();
    let first = normalize_candidates(first, &first_index).unwrap();
    let second = normalize_candidates(second, &second_index).unwrap();
    assert_eq!(first.candidates, second.candidates);
}

#[test]
fn shared_file_with_disconnected_subjects_splits() {
    let index = AgendaIndex::from_report(&fixture_report()).unwrap();
    let candidate = ReviewCandidate::from_selection(CandidateSelection {
        selector_id: "split_selector",
        selector_version: 1,
        decision_kind: DecisionKind::EvaluateSharedRootCause,
        subject_ids: vec!["symbol:a".to_string(), "symbol:b".to_string()],
        finding_ids: vec!["EA-one".to_string(), "EA-three".to_string()],
        selection_evidence_refs: vec![
            "measurement:EA-one".to_string(),
            "measurement:EA-three".to_string(),
        ],
        eligible_case_kinds: vec![CaseKind::SharedRootCause],
        selection_reasons: vec![SelectionReason::SameSubjectConcentration],
    })
    .unwrap();
    let normalized = normalize_candidates(vec![candidate], &index).unwrap();
    assert_eq!(normalized.candidates.len(), 2);
    assert!(normalized
        .candidates
        .iter()
        .all(|candidate| candidate.subject_ids.len() == 1));
}

#[test]
fn exact_duplicates_preserve_typed_source_provenance() {
    let index = AgendaIndex::from_report(&fixture_report()).unwrap();
    let first = selection(
        "alpha_selector",
        vec!["symbol:a"],
        vec!["EA-one"],
        SelectionReason::SameSubjectConcentration,
    );
    let second = selection(
        "beta_selector",
        vec!["symbol:a"],
        vec!["EA-one"],
        SelectionReason::SameSubjectConcentration,
    );
    let normalized = normalize_candidates(vec![second, first], &index).unwrap();
    assert_eq!(normalized.candidates.len(), 1);
    assert_eq!(normalized.candidates[0].source_candidates.len(), 2);
    assert_eq!(normalized.duplicates.len(), 1);
}

#[test]
fn every_suppression_reason_has_a_stable_typed_record() {
    let candidate = selection(
        "test_selector",
        vec!["symbol:a"],
        vec!["EA-one"],
        SelectionReason::SameSubjectConcentration,
    );
    let reasons = [
        SuppressionReason::UnsupportedEvidenceNeighborhood,
        SuppressionReason::DuplicateOfCase,
        SuppressionReason::OutsideOperatorScope,
        SuppressionReason::PrivacyPolicyExcludesRequiredContext,
        SuppressionReason::DeterministicActionAlreadyComplete,
        SuppressionReason::NoAdditionalContextCanChangeAction,
        SuppressionReason::InsufficientMateriality,
        SuppressionReason::ReviewBudgetDeferred,
    ];
    let records = reasons
        .into_iter()
        .map(|reason| {
            SuppressionRecord::new(
                candidate.candidate_id.clone(),
                reason,
                vec!["measurement:EA-one".to_string()],
                None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .map(|record| &record.suppression_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        reasons.len()
    );
}
