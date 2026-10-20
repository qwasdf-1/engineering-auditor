use auditor_review::{
    classify_ensemble_independence, plan_evidence_union, CaseEvidenceRequest, ClaimStatus,
    EnsembleCaseFailure, EnsembleCaseRun, EnsembleFailureStage, EnsembleIndependence,
    EnsembleReviewerPass, EnsembleReviewerSlot, EvidenceLookupRequestKind, EvidenceUnionBoundary,
    ReviewerAdmissionContext, ReviewerCaseReview, ReviewerEvidenceRequest, RouteTarget, StableId,
};

use super::spec09_tests::{review_payload, ReviewFixture};
use super::*;

fn run_id(slot: EnsembleReviewerSlot) -> StableId {
    StableId::from_identity("reviewer-run", 1, &json!({"slot": slot})).unwrap()
}

pub(super) fn prompt_id(suffix: &str) -> StableId {
    StableId::from_identity("review-prompt-bundle", 2, &json!({"suffix": suffix})).unwrap()
}

pub(super) fn target(provider: &str, model: &str) -> RouteTarget {
    RouteTarget {
        provider_id: provider.to_string(),
        model: model.to_string(),
    }
}

pub(super) fn admitted_review(
    fixture: &ReviewFixture,
    slot: EnsembleReviewerSlot,
    route: &RouteTarget,
) -> ReviewerCaseReview {
    ReviewerCaseReview::admit(
        review_payload(
            &fixture.case,
            &fixture.definitions,
            ClaimStatus::Substantiated,
            false,
        ),
        ReviewerAdmissionContext {
            reviewer_run_id: &run_id(slot),
            route_target: route,
            case: &fixture.case,
            packet: &fixture.packet,
            probe_definitions: &fixture.definitions,
            deterministic_probe_results: &fixture.results,
        },
    )
    .unwrap()
}

pub(super) fn pass(
    fixture: &ReviewFixture,
    slot: EnsembleReviewerSlot,
    pass: u8,
    route: RouteTarget,
    prompt: StableId,
) -> EnsembleReviewerPass {
    EnsembleReviewerPass {
        reviewer: slot,
        pass,
        packet_digest: fixture.packet.packet_digest.clone(),
        review: admitted_review(fixture, slot, &route),
        route_target: route,
        prompt_identity: prompt,
        evidence_requests: Vec::new(),
    }
}

#[test]
fn evidence_union_deduplicates_without_losing_reviewer_provenance() {
    let fixture = ReviewFixture::new();
    let probe_id = fixture.definitions[0].probe_id.clone();
    let target_id = format!("finding:{}", fixture.case.finding_ids[0]);
    let request = CaseEvidenceRequest {
        case_id: fixture.case.case_id.clone(),
        probe_id: probe_id.clone(),
        request_kind: EvidenceLookupRequestKind::FindingSource,
        target_id: target_id.clone(),
        decision_relevance: "Confirm the production source boundary.".to_string(),
    };
    let plan = plan_evidence_union(
        [
            ReviewerEvidenceRequest {
                reviewer: EnsembleReviewerSlot::ReviewerC,
                request: request.clone(),
            },
            ReviewerEvidenceRequest {
                reviewer: EnsembleReviewerSlot::ReviewerA,
                request,
            },
        ],
        EvidenceUnionBoundary {
            case_id: &fixture.case.case_id,
            probe_ids: &BTreeSet::from([probe_id]),
            target_ids: &BTreeSet::from([target_id]),
            maximum_requests: 2,
        },
    )
    .unwrap();
    assert_eq!(plan.requests.len(), 1);
    assert_eq!(
        plan.requests[0].requested_by,
        vec![
            EnsembleReviewerSlot::ReviewerA,
            EnsembleReviewerSlot::ReviewerC
        ]
    );
}

#[test]
fn evidence_union_rejects_out_of_case_and_over_budget_requests() {
    let fixture = ReviewFixture::new();
    let probe_id = fixture.definitions[0].probe_id.clone();
    let target_id = format!("finding:{}", fixture.case.finding_ids[0]);
    let request = CaseEvidenceRequest {
        case_id: fixture.case.case_id.clone(),
        probe_id: probe_id.clone(),
        request_kind: EvidenceLookupRequestKind::FindingSource,
        target_id: target_id.clone(),
        decision_relevance: "Resolve one material boundary.".to_string(),
    };
    let probe_ids = BTreeSet::from([probe_id.clone()]);
    let target_ids = BTreeSet::from([target_id.clone()]);
    let boundary = || EvidenceUnionBoundary {
        case_id: &fixture.case.case_id,
        probe_ids: &probe_ids,
        target_ids: &target_ids,
        maximum_requests: 1,
    };
    let mut outside = request.clone();
    outside.target_id = "finding:not-in-case".to_string();
    assert!(plan_evidence_union(
        [ReviewerEvidenceRequest {
            reviewer: EnsembleReviewerSlot::ReviewerB,
            request: outside,
        }],
        boundary(),
    )
    .is_err());

    let mut second = request.clone();
    second.request_kind = EvidenceLookupRequestKind::SymbolNeighborhood;
    assert!(plan_evidence_union(
        [
            ReviewerEvidenceRequest {
                reviewer: EnsembleReviewerSlot::ReviewerA,
                request,
            },
            ReviewerEvidenceRequest {
                reviewer: EnsembleReviewerSlot::ReviewerB,
                request: second,
            },
        ],
        boundary(),
    )
    .is_err());
}

#[test]
fn complete_three_slot_run_is_stable_and_records_actual_independence() {
    let fixture = ReviewFixture::new();
    let prompt = prompt_id("shared");
    let routes = [
        target("provider-one", "model-one"),
        target("provider-two", "model-two"),
        target("provider-three", "model-three"),
    ];
    let mut pass_a = EnsembleReviewerSlot::ALL
        .into_iter()
        .zip(routes.clone())
        .map(|(slot, route)| pass(&fixture, slot, 1, route, prompt.clone()))
        .collect::<Vec<_>>();
    pass_a.reverse();
    let pass_b = EnsembleReviewerSlot::ALL
        .into_iter()
        .zip(routes)
        .map(|(slot, route)| pass(&fixture, slot, 2, route, prompt.clone()))
        .collect::<Vec<_>>();
    let run = EnsembleCaseRun::build(
        fixture.case.case_id.clone(),
        fixture.packet.packet_digest.clone(),
        fixture.packet.packet_digest.clone(),
        pass_a,
        pass_b,
    )
    .unwrap();
    assert_eq!(run.independence, EnsembleIndependence::CrossProvider);
    assert_eq!(
        run.pass_a
            .iter()
            .map(|pass| pass.reviewer)
            .collect::<Vec<_>>(),
        EnsembleReviewerSlot::ALL
    );
    assert!(run.validate().is_ok());
}

#[test]
fn incomplete_ensemble_cannot_masquerade_as_success_and_failure_is_typed() {
    let fixture = ReviewFixture::new();
    let prompt = prompt_id("same-model");
    let route = target("provider", "model");
    let passes = [
        EnsembleReviewerSlot::ReviewerA,
        EnsembleReviewerSlot::ReviewerB,
    ]
    .into_iter()
    .map(|slot| pass(&fixture, slot, 1, route.clone(), prompt.clone()))
    .collect::<Vec<_>>();
    assert!(EnsembleCaseRun::build(
        fixture.case.case_id.clone(),
        fixture.packet.packet_digest.clone(),
        fixture.packet.packet_digest.clone(),
        passes.clone(),
        passes,
    )
    .is_err());
    let failure = EnsembleCaseFailure::new(
        fixture.case.case_id.clone(),
        EnsembleFailureStage::PassB,
        vec![
            EnsembleReviewerSlot::ReviewerB,
            EnsembleReviewerSlot::ReviewerA,
        ],
        "reviewer_pass_b_incomplete",
    )
    .unwrap();
    assert_eq!(
        failure.completed_slots,
        vec![
            EnsembleReviewerSlot::ReviewerA,
            EnsembleReviewerSlot::ReviewerB
        ]
    );
}

#[test]
fn same_model_repetition_is_not_overstated_as_diversity() {
    let fixture = ReviewFixture::new();
    let route = target("one-provider", "one-model");
    let prompt = prompt_id("one-prompt");
    let passes = EnsembleReviewerSlot::ALL
        .into_iter()
        .map(|slot| pass(&fixture, slot, 1, route.clone(), prompt.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        classify_ensemble_independence(&passes),
        EnsembleIndependence::SameModelRepeated
    );
}
