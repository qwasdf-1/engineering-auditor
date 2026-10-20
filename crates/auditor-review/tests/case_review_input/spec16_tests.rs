use auditor_review::{
    AxisValue, DecisionAxis, DecisionAxisKind, FinalizedCase, OperatorPriorityClass,
    PortfolioAssembler, PortfolioAssemblyDirectives, PortfolioBlocker, PortfolioBlockerKind,
    PortfolioConflict, PortfolioConflictKind, PortfolioDependency, RecommendationDecisionAxes,
    RecommendationPriority, TemplateStatus,
};

use super::spec09_tests::ReviewFixture;

fn portfolio_fixture() -> (Vec<auditor_review::EngineeringCase>, Vec<FinalizedCase>) {
    let fixture = ReviewFixture::new();
    let mut case = (*fixture.case).clone();
    case.template_status = TemplateStatus::Production;
    let base = fixture
        .admit(super::spec09_tests::review_payload(
            &case,
            &fixture.definitions,
            auditor_review::ClaimStatus::Substantiated,
            false,
        ))
        .unwrap()
        .recommendations[0]
        .clone();
    let recommendations = ["prepare", "apply", "verify"]
        .into_iter()
        .map(|summary| {
            let mut draft = base.to_draft();
            draft.summary = summary.to_string();
            auditor_review::ContextualRecommendation::from_draft(case.case_id.clone(), draft)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut finalized = FinalizedCase::deterministic(
        &case,
        auditor_review::DeterministicDisposition::ActionFullyDetermined,
    )
    .unwrap();
    finalized.accepted_recommendations = recommendations;
    (vec![case], vec![finalized])
}

fn edge(
    prerequisite_id: auditor_review::StableId,
    dependent_id: auditor_review::StableId,
) -> PortfolioDependency {
    PortfolioDependency {
        prerequisite_id,
        dependent_id,
        rationale: "explicit fixture prerequisite".to_string(),
        evidence_ids: vec!["fixture:dependency".to_string()],
    }
}

#[test]
fn portfolio_order_is_stable_and_keeps_axes_separate() {
    let (cases, finalizations) = portfolio_fixture();
    let ids = finalizations[0]
        .accepted_recommendations
        .iter()
        .map(|recommendation| recommendation.recommendation_id.clone())
        .collect::<Vec<_>>();
    let directives = PortfolioAssemblyDirectives {
        dependencies: vec![
            edge(ids[0].clone(), ids[2].clone()),
            edge(ids[0].clone(), ids[1].clone()),
        ],
        decision_axes: vec![RecommendationDecisionAxes {
            recommendation_id: ids[1].clone(),
            axes: vec![DecisionAxis {
                axis: DecisionAxisKind::Urgency,
                value: AxisValue::Explicit {
                    value: "next maintenance window".to_string(),
                    provenance_refs: vec!["operator:window".to_string()],
                },
            }],
        }],
        priority_classes: vec![RecommendationPriority {
            recommendation_id: ids[2].clone(),
            class: OperatorPriorityClass::Immediate,
            provenance_refs: vec!["operator:priority".to_string()],
        }],
        ..PortfolioAssemblyDirectives::default()
    };
    let expected = PortfolioAssembler
        .assemble(&finalizations, &cases, &[], directives.clone())
        .unwrap();
    let mut shuffled = directives;
    shuffled.dependencies.reverse();
    assert_eq!(
        expected,
        PortfolioAssembler
            .assemble(&finalizations, &cases, &[], shuffled)
            .unwrap()
    );
    assert_eq!(expected.topological_order[0], ids[0]);
    let axes = &expected
        .decision_axes
        .iter()
        .find(|record| record.recommendation_id == ids[1])
        .unwrap()
        .axes;
    assert_eq!(axes.len(), DecisionAxisKind::ALL.len());
    assert_eq!(
        axes.iter()
            .filter(|axis| !matches!(axis.value, AxisValue::Unknown))
            .count(),
        1
    );
}

#[test]
fn portfolio_records_conflicts_cycles_and_blocked_dependents() {
    let (cases, finalizations) = portfolio_fixture();
    let ids = finalizations[0]
        .accepted_recommendations
        .iter()
        .map(|recommendation| recommendation.recommendation_id.clone())
        .collect::<Vec<_>>();
    let portfolio = PortfolioAssembler
        .assemble(
            &finalizations,
            &cases,
            &[],
            PortfolioAssemblyDirectives {
                dependencies: vec![
                    edge(ids[0].clone(), ids[1].clone()),
                    edge(ids[1].clone(), ids[0].clone()),
                    edge(ids[1].clone(), ids[2].clone()),
                ],
                blockers: vec![PortfolioBlocker {
                    blocked_recommendation_id: ids[2].clone(),
                    kind: PortfolioBlockerKind::OperatorDecision,
                    blocking_recommendation_id: None,
                    reason: "operator must choose the rollout window".to_string(),
                    evidence_ids: vec!["operator:missing".to_string()],
                }],
                action_conflicts: vec![PortfolioConflict {
                    kind: PortfolioConflictKind::ActionConflict,
                    recommendation_ids: vec![ids[0].clone(), ids[2].clone()],
                    reason: "fixture actions contend for one boundary".to_string(),
                    evidence_ids: vec!["fixture:conflict".to_string()],
                }],
                ..PortfolioAssemblyDirectives::default()
            },
        )
        .unwrap();
    assert_eq!(portfolio.cyclic_recommendation_ids.len(), 2);
    assert_eq!(portfolio.dependency_blocked_ids, vec![ids[2].clone()]);
    assert!(portfolio.conflicts.iter().any(|conflict| {
        conflict.kind == PortfolioConflictKind::DependencyCycle
            && conflict.recommendation_ids.len() == 2
    }));
    assert_eq!(portfolio.blockers.len(), 1);
}

#[test]
fn shadow_actions_and_unbound_comparator_content_cannot_enter_portfolio() {
    let (mut cases, finalizations) = portfolio_fixture();
    cases[0].template_status = TemplateStatus::Shadow;
    let portfolio = PortfolioAssembler
        .assemble(
            &finalizations,
            &cases,
            &[],
            PortfolioAssemblyDirectives::default(),
        )
        .unwrap();
    assert!(portfolio.accepted_recommendation_ids.is_empty());
    assert_eq!(portfolio.shadow_excluded_recommendation_ids.len(), 3);

    cases[0].template_status = TemplateStatus::Production;
    let ids = finalizations[0]
        .accepted_recommendations
        .iter()
        .map(|recommendation| recommendation.recommendation_id.clone())
        .collect::<Vec<_>>();
    let invalid = PortfolioAssemblyDirectives {
        alternative_comparison: Some(auditor_review::AlternativeComparisonRecord {
            compared_recommendation_ids: vec![ids[0].clone(), ids[1].clone()],
            tradeoff_comparison: "Compare the admitted alternatives.".to_string(),
            missing_operator_decision: "Choose the maintenance window.".to_string(),
            evidence_probe_ids: vec![auditor_review::StableId::from_identity(
                "case-probe",
                1,
                &serde_json::json!({"unbound": true}),
            )
            .unwrap()],
        }),
        ..PortfolioAssemblyDirectives::default()
    };
    assert!(PortfolioAssembler
        .assemble(&finalizations, &cases, &[], invalid)
        .is_err());
}

#[test]
fn empty_portfolio_is_valid() {
    assert_eq!(
        PortfolioAssembler
            .assemble(&[], &[], &[], PortfolioAssemblyDirectives::default())
            .unwrap(),
        auditor_review::ReviewPortfolio::empty()
    );
}
