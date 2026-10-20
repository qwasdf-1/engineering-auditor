use auditor_review::{
    compare_ensemble_run, compare_probe_results, validate_comparison_recommendation_families,
    EnsembleCaseRun, EnsembleChallengeExecution, EnsembleReviewerSlot, ProbeAnswer,
    ProbeComparisonState, RecommendationFamilyState, ReviewComparisonOutcome,
};

use super::spec09_tests::ReviewFixture;
use super::spec13_tests::{pass, prompt_id, target};

#[test]
fn every_probe_comparison_state_is_structurally_distinct() {
    let fixture = ReviewFixture::new();
    let base = fixture.results[0].clone();
    let classify = |answers: [ProbeAnswer; 3], evidence: [Vec<String>; 3]| {
        let mut results = [base.clone(), base.clone(), base.clone()];
        for ((result, answer), evidence_ids) in results.iter_mut().zip(answers).zip(evidence) {
            result.answer = answer;
            result.evidence_ids = evidence_ids;
        }
        compare_probe_results(
            base.probe_id.clone(),
            &EnsembleReviewerSlot::ALL
                .into_iter()
                .zip(results.iter())
                .collect::<Vec<_>>(),
        )
        .unwrap()
        .state
    };
    let empty = || std::array::from_fn(|_| Vec::new());
    assert_eq!(
        classify([ProbeAnswer::Yes; 3], empty()),
        ProbeComparisonState::Unanimous
    );
    assert_eq!(
        classify(
            [ProbeAnswer::Yes; 3],
            [vec!["a".into()], vec!["b".into()], vec!["a".into()]]
        ),
        ProbeComparisonState::QualifiedAgreement
    );
    assert_eq!(
        classify(
            [ProbeAnswer::Yes, ProbeAnswer::Yes, ProbeAnswer::No],
            empty()
        ),
        ProbeComparisonState::Split
    );
    assert_eq!(
        classify(
            [
                ProbeAnswer::Yes,
                ProbeAnswer::No,
                ProbeAnswer::NotApplicable
            ],
            empty()
        ),
        ProbeComparisonState::AllDifferent
    );
    assert_eq!(
        classify([ProbeAnswer::Unknown; 3], empty()),
        ProbeComparisonState::SharedUnknown
    );
    assert_eq!(
        classify(
            [
                ProbeAnswer::Yes,
                ProbeAnswer::ConflictingEvidence,
                ProbeAnswer::No
            ],
            empty()
        ),
        ProbeComparisonState::EvidenceConflict
    );
}

fn run_with(
    fixture: &ReviewFixture,
    mutate: impl Fn(EnsembleReviewerSlot, &mut auditor_review::ReviewerCaseReview),
) -> EnsembleCaseRun {
    let route = target("provider", "model");
    let prompt = prompt_id("comparison");
    let make = |pass_number| {
        EnsembleReviewerSlot::ALL
            .into_iter()
            .map(|slot| {
                let mut value = pass(fixture, slot, pass_number, route.clone(), prompt.clone());
                mutate(slot, &mut value.review);
                value
            })
            .collect::<Vec<_>>()
    };
    EnsembleCaseRun::build(
        fixture.case.case_id.clone(),
        fixture.packet.packet_digest.clone(),
        fixture.packet.packet_digest.clone(),
        make(1),
        make(2),
    )
    .unwrap()
}

#[test]
fn two_to_one_recommendation_split_remains_non_authoritative() {
    let fixture = ReviewFixture::new();
    let run = run_with(&fixture, |slot, review| {
        if slot == EnsembleReviewerSlot::ReviewerC {
            review.recommendations.clear();
            review.preferred_recommendation_id = None;
        }
    });
    let comparison = compare_ensemble_run(&run, &fixture.results).unwrap();
    assert_eq!(
        comparison.outcome,
        ReviewComparisonOutcome::MajorityWithMaterialObjection
    );
    assert!(comparison
        .recommendation_families
        .iter()
        .any(|family| { family.state == RecommendationFamilyState::Contested }));
    assert!(!comparison.challenge_targets.is_empty());
}

#[test]
fn shared_unknown_without_actions_closes_as_evidence_gap() {
    let fixture = ReviewFixture::new();
    let run = run_with(&fixture, |_slot, review| {
        review.recommendations.clear();
        review.preferred_recommendation_id = None;
        review.case_position = auditor_review::CasePosition::EvidenceBlocked;
        review.case_conclusion = auditor_review::CaseConclusion::InsufficientEvidence;
    });
    let comparison = compare_ensemble_run(&run, &fixture.results).unwrap();
    assert_eq!(
        comparison.outcome,
        ReviewComparisonOutcome::SharedEvidenceGap
    );
    assert!(!comparison.shared_unknown_probe_ids.is_empty());
}

#[test]
fn scope_variants_remain_variants_and_order_is_canonical() {
    let fixture = ReviewFixture::new();
    let run = run_with(&fixture, |slot, review| {
        if slot == EnsembleReviewerSlot::ReviewerC {
            review.recommendations[0]
                .content
                .scope_ids
                .push("narrower-owner".to_string());
        }
    });
    let expected = compare_ensemble_run(&run, &fixture.results).unwrap();
    assert_eq!(
        expected.outcome,
        ReviewComparisonOutcome::ConvergedWithVariants
    );
    assert!(expected.recommendation_families.iter().any(|family| {
        family.state == RecommendationFamilyState::CompatibleVariant
            && !family.decision_dependencies.is_empty()
    }));
    let mut shuffled = run.clone();
    shuffled.pass_a.reverse();
    shuffled.pass_b.reverse();
    let rebuilt = EnsembleCaseRun::build(
        shuffled.case_id.clone(),
        shuffled.initial_packet_digest,
        shuffled.shared_packet_digest,
        shuffled.pass_a,
        shuffled.pass_b,
    )
    .unwrap();
    assert_eq!(
        expected,
        compare_ensemble_run(&rebuilt, &fixture.results).unwrap()
    );
}

#[test]
fn challenger_cannot_introduce_a_new_recommendation_family() {
    let fixture = ReviewFixture::new();
    let run = run_with(&fixture, |_slot, _review| {});
    let comparison = compare_ensemble_run(&run, &fixture.results).unwrap();
    let mut candidate = run.pass_b[0].review.recommendations.clone();
    candidate[0].content.family = auditor_review::RecommendationFamily::ExplicitRetentionAvoidance;
    assert!(validate_comparison_recommendation_families(
        &comparison.recommendation_families,
        &candidate,
    )
    .is_err());
}

#[test]
fn comparison_challenge_execution_covers_one_reviewers_exact_targets() {
    let fixture = ReviewFixture::new();
    let run = run_with(&fixture, |slot, review| {
        if slot == EnsembleReviewerSlot::ReviewerC {
            review.recommendations.clear();
            review.preferred_recommendation_id = None;
        }
    });
    let comparison = compare_ensemble_run(&run, &fixture.results).unwrap();
    let pass = &run.pass_b[0];
    let owned = pass
        .review
        .claims
        .iter()
        .map(|claim| &claim.claim_id)
        .chain(
            pass.review
                .recommendations
                .iter()
                .map(|recommendation| &recommendation.recommendation_id),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let target_ids = comparison
        .challenge_targets
        .iter()
        .filter(|target| owned.contains(&target.target_id))
        .map(|target| target.target_id.clone())
        .collect::<Vec<_>>();
    let execution = EnsembleChallengeExecution::failed(
        &comparison,
        pass.reviewer,
        pass.review.review_id.clone(),
        target_ids,
        "scripted_shadow_failure",
    );
    execution.validate(&run, &comparison).unwrap();

    let mut partial = execution.clone();
    partial.target_ids.pop();
    assert!(partial.validate(&run, &comparison).is_err());
}
