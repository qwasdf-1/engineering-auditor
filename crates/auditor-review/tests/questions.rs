//! Question templates over real scans of the fixture repositories: what
//! each kind's trigger asks, and the answers the check accepts and refuses.

#[path = "reviewer/fixture.rs"]
mod fixture;

use std::collections::BTreeSet;

use auditor_review::{
    ClaimConfidence, ClaimDraft, ClaimSlot, ClaimStatus, ContextualRecommendation,
    DetectorAdviceRelationship, EngineeringClaim, EngineeringClaimKind, Materiality, Question,
    QuestionAnswer, QuestionKind, QuestionTemplateBundle, RecommendationDraft,
    RecommendationFamily, VerificationStep,
};
use fixture::{assert_cites_only_issued, Fixture, GO_SERVICE, PYTHON_APP};
use serde_json::Value;

fn questions(fixture: &Fixture) -> Vec<Question> {
    fixture
        .view()
        .questions(&QuestionTemplateBundle::checked_in().unwrap())
        .unwrap()
}

fn kinds(questions: &[Question]) -> Vec<QuestionKind> {
    questions.iter().map(|question| question.kind).collect()
}

fn claim(
    question: &Question,
    slot: ClaimSlot,
    kind: EngineeringClaimKind,
    cites: &[&str],
) -> EngineeringClaim {
    EngineeringClaim::from_draft(
        question.question_id.clone(),
        ClaimDraft {
            slot,
            kind,
            proposition: format!("{slot:?} as cited"),
            status: ClaimStatus::Substantiated,
            supporting_evidence_ids: cites.iter().map(|id| id.to_string()).collect(),
            counter_evidence_ids: Vec::new(),
            dependency_claim_ids: Vec::new(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::Medium,
        },
    )
    .unwrap()
}

fn recommendation(
    question: &Question,
    family: RecommendationFamily,
    premises: &[&EngineeringClaim],
    findings: &[&str],
) -> ContextualRecommendation {
    ContextualRecommendation::from_draft(
        question.question_id.clone(),
        RecommendationDraft {
            family,
            summary: format!("{family:?} for {}", findings.join(", ")),
            scope_ids: findings.iter().map(|id| id.to_string()).collect(),
            premise_claim_ids: premises
                .iter()
                .map(|claim| claim.claim_id.clone())
                .collect(),
            affected_finding_ids: findings.iter().map(|id| id.to_string()).collect(),
            detector_advice_relationship: DetectorAdviceRelationship::Consolidates,
            preconditions: Vec::new(),
            expected_benefits: Vec::new(),
            risks: Vec::new(),
            costs: Vec::new(),
            alternatives: Vec::new(),
            no_alternative_reason: None,
            verification_steps: vec![VerificationStep {
                method: "scan again".to_string(),
                expected_observation: "the findings are gone".to_string(),
                evidence_ids: Vec::new(),
            }],
            materiality: Materiality::Medium,
            materiality_basis: "the cited findings".to_string(),
            confidence: ClaimConfidence::Medium,
            patch: None,
        },
    )
    .unwrap()
}

fn answer(
    question: &Question,
    claims: Vec<EngineeringClaim>,
    recommendations: Vec<ContextualRecommendation>,
    ranking: Vec<String>,
) -> QuestionAnswer {
    QuestionAnswer {
        question_id: question.question_id.clone(),
        claims,
        recommendations,
        ranking,
    }
}

/// Well-formed answers to the python fixture's four questions, in order.
fn python_answers(questions: &[Question]) -> Vec<QuestionAnswer> {
    let [hotspots, cluster, architecture, plan] = questions else {
        panic!("the python fixture asks four questions");
    };
    let risks = hotspots
        .subjects
        .iter()
        .map(|subject| {
            claim(
                hotspots,
                ClaimSlot::HotspotRisk,
                EngineeringClaimKind::RecommendationPremise,
                &[&subject.id],
            )
        })
        .collect::<Vec<_>>();
    let ranking = hotspots
        .subjects
        .iter()
        .rev()
        .map(|subject| subject.id.clone())
        .collect();
    let findings = cluster
        .finding_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let cause = claim(
        cluster,
        ClaimSlot::CausalOverlap,
        EngineeringClaimKind::RootCause,
        &findings,
    );
    let fix = recommendation(
        cluster,
        RecommendationFamily::SharedRootRemediation,
        &[&cause],
        &findings,
    );
    let structure = architecture
        .subjects
        .iter()
        .map(|subject| subject.id.as_str())
        .collect::<Vec<_>>();
    let intended = claim(
        architecture,
        ClaimSlot::IntendedStructure,
        EngineeringClaimKind::RecommendationPremise,
        &structure,
    );
    let step = recommendation(
        plan,
        RecommendationFamily::SharedRootRemediation,
        &[&cause],
        &findings,
    );
    let plan_ranking = vec![step.recommendation_id.as_str().to_string()];
    vec![
        answer(hotspots, risks, Vec::new(), ranking),
        answer(cluster, vec![cause], vec![fix], Vec::new()),
        answer(architecture, vec![intended], Vec::new(), Vec::new()),
        answer(plan, Vec::new(), vec![step], plan_ranking),
    ]
}

fn known(fixture: &Fixture) -> BTreeSet<String> {
    fixture.view().orientation_pack(None).unwrap().evidence_ids
}

#[test]
fn each_fixture_asks_what_its_scan_supports_with_the_plan_last() {
    let python = Fixture::scan(PYTHON_APP);
    let asked = questions(&python);
    assert_eq!(asked.len(), 4);
    assert_eq!(kinds(&asked), QuestionKind::ALL);
    for question in &asked {
        assert_cites_only_issued(&question.markdown, &question.evidence_ids);
        assert!(question.markdown.contains(question.question_id.as_str()));
        assert!(question
            .question_id
            .as_str()
            .starts_with("engineering-case:"));
    }
    let plan = &asked[3];
    assert_eq!(
        plan.depends_on,
        asked[..3]
            .iter()
            .map(|question| question.question_id.clone())
            .collect::<Vec<_>>()
    );
    assert!(plan.subjects.is_empty());
    assert_eq!(
        plan.finding_ids.len(),
        python.report["findings"].as_array().unwrap().len()
    );

    let cluster = &asked[1];
    assert_eq!(cluster.subjects.len(), 1);
    assert_eq!(
        cluster.subjects[0].id,
        python.symbol_id("record", "function")
    );
    assert_eq!(cluster.finding_ids.len(), 2);

    let architecture = &asked[2];
    assert_eq!(architecture.subjects[0].id, "module-cycle:1");
    assert!(architecture.subjects[1].id.starts_with("finding:"));
    assert_eq!(
        architecture.finding_ids,
        vec![architecture.subjects[1].id.clone()]
    );

    let ids = |questions: &[Question]| {
        questions
            .iter()
            .map(|question| question.question_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&asked), ids(&questions(&python)));

    let go = Fixture::scan(GO_SERVICE);
    let asked = questions(&go);
    assert_eq!(
        kinds(&asked),
        [
            QuestionKind::HotspotAssessment,
            QuestionKind::SharedRootCause,
            QuestionKind::RemediationPlan
        ],
        "no module cycle or structure finding, so no architecture question"
    );
    assert_eq!(asked[2].depends_on.len(), 2);
}

#[test]
fn well_formed_answers_pass_the_check() {
    let python = Fixture::scan(PYTHON_APP);
    let asked = questions(&python);
    let known = known(&python);
    let answers = python_answers(&asked);
    for (index, (question, answer)) in asked.iter().zip(&answers).enumerate() {
        question
            .check_answer(answer, &known, &answers[..index])
            .unwrap_or_else(|error| panic!("{:?}: {error}", question.kind));
    }
}

#[test]
fn answers_that_break_their_kind_are_refused() {
    let python = Fixture::scan(PYTHON_APP);
    let asked = questions(&python);
    let known = known(&python);
    let answers = python_answers(&asked);
    let refused = |index: usize, answer: &QuestionAnswer, earlier: &[QuestionAnswer]| {
        asked[index]
            .check_answer(answer, &known, earlier)
            .expect_err("the check refuses the answer")
    };

    let mut hotspots = answers[0].clone();
    hotspots.claims.pop();
    assert!(refused(0, &hotspots, &[]).contains("no claim cites subject"));
    let mut hotspots = answers[0].clone();
    hotspots.ranking.pop();
    assert!(refused(0, &hotspots, &[]).contains("ranking must list every subject"));
    let mut hotspots = answers[0].clone();
    hotspots.question_id = answers[1].question_id.clone();
    assert!(refused(0, &hotspots, &[]).contains("the answer is for"));
    let mut hotspots = answers[0].clone();
    hotspots.claims[0] = claim(
        &asked[0],
        ClaimSlot::HotspotRisk,
        EngineeringClaimKind::RecommendationPremise,
        &["file:not-issued"],
    );
    assert!(refused(0, &hotspots, &[]).contains("cites unknown evidence"));
    let mut read = known.clone();
    read.insert("source-span:app/store/db.py#L8-L12".to_string());
    for (span, inside) in [
        ("#L8-L12", true),
        ("#L9-L10", true),
        ("#L7-L10", false),
        ("#L10-L13", false),
        ("#L10-L9", false),
    ] {
        let mut narrowed = answers[0].clone();
        narrowed.claims[0] = claim(
            &asked[0],
            ClaimSlot::HotspotRisk,
            EngineeringClaimKind::RecommendationPremise,
            &[
                &asked[0].subjects[0].id,
                &format!("source-span:app/store/db.py{span}"),
            ],
        );
        let checked = asked[0].check_answer(&narrowed, &read, &[]);
        assert_eq!(checked.is_ok(), inside, "{span}: {checked:?}");
    }

    let cluster = &asked[1];
    let lone = claim(
        cluster,
        ClaimSlot::CausalOverlap,
        EngineeringClaimKind::RootCause,
        &[&cluster.finding_ids[0]],
    );
    let lone_answer = answer(cluster, vec![lone], Vec::new(), Vec::new());
    assert!(refused(1, &lone_answer, &[]).contains("a shared cause explains at least two"));
    let mut two_fixes = answers[1].clone();
    let findings = cluster
        .finding_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    two_fixes.recommendations.push(recommendation(
        cluster,
        RecommendationFamily::SharedRootRemediation,
        &[&answers[1].claims[0]],
        &findings[..1],
    ));
    assert!(refused(1, &two_fixes, &[]).contains("one shared cause gets one fix"));
    let mut ranked = answers[1].clone();
    ranked.ranking = vec![cluster.subjects[0].id.clone()];
    assert!(refused(1, &ranked, &[]).contains("takes no ranking"));
    let mut outside = answers[1].clone();
    outside.recommendations = vec![recommendation(
        cluster,
        RecommendationFamily::StructuralRefactor,
        &[&answers[1].claims[0]],
        &findings,
    )];
    assert!(refused(1, &outside, &[]).contains("family is outside"));

    let architecture = &asked[2];
    let violation = claim(
        architecture,
        ClaimSlot::StructuralViolation,
        EngineeringClaimKind::RecommendationPremise,
        &["module-cycle:1", &architecture.subjects[1].id],
    );
    let no_intent = answer(architecture, vec![violation], Vec::new(), Vec::new());
    assert!(refused(2, &no_intent, &[]).contains("no intended structure"));

    let plan = &asked[3];
    let mut unranked = answers[3].clone();
    unranked.ranking.clear();
    assert!(refused(3, &unranked, &answers[..3]).contains("every recommendation ID"));
    assert!(
        refused(3, &answers[3], &[]).contains("dangling premise"),
        "the plan's premise is a cluster claim, so it needs the earlier answers"
    );
    let mut draft = answers[3].recommendations[0].to_draft();
    draft.affected_finding_ids.clear();
    let unfounded = ContextualRecommendation::from_draft(plan.question_id.clone(), draft).unwrap();
    let mut empty_step = answers[3].clone();
    empty_step.ranking = vec![unfounded.recommendation_id.as_str().to_string()];
    empty_step.recommendations = vec![unfounded];
    assert!(refused(3, &empty_step, &answers[..3]).contains("resolves no finding"));

    let lifecycle = |reason: Option<&str>| {
        let mut draft = answers[3].recommendations[0].to_draft();
        draft.family = RecommendationFamily::LifecycleReownership;
        draft.no_alternative_reason = reason.map(str::to_string);
        let step = ContextualRecommendation::from_draft(plan.question_id.clone(), draft).unwrap();
        let mut moved = answers[3].clone();
        moved.ranking = vec![step.recommendation_id.as_str().to_string()];
        moved.recommendations = vec![step];
        plan.check_answer(&moved, &known, &answers[..3])
    };
    assert!(lifecycle(None)
        .unwrap_err()
        .contains("requires an alternative"));
    lifecycle(Some("the wider owner is the only one")).unwrap();
}

fn edited_bundle(edit: impl FnOnce(&mut Value)) -> Result<QuestionTemplateBundle, String> {
    let mut value = serde_json::to_value(QuestionTemplateBundle::checked_in().unwrap()).unwrap();
    edit(&mut value);
    QuestionTemplateBundle::from_json(&value.to_string())
}

#[test]
fn the_template_bundle_is_checked_and_its_limits_apply() {
    assert!(edited_bundle(|bundle| {
        bundle["templates"].as_array_mut().unwrap().swap(0, 3);
    })
    .unwrap_err()
    .contains("in that order"));
    assert!(edited_bundle(|bundle| bundle["templates"][3]["max_subjects"] = 2.into()).is_err());
    assert!(edited_bundle(|bundle| bundle["templates"][0]["max_subjects"] = 0.into()).is_err());
    assert!(edited_bundle(|bundle| {
        bundle["templates"][1]["claim_slots"] =
            serde_json::json!(["responsibility", "responsibility"]);
    })
    .is_err());
    assert!(edited_bundle(|bundle| bundle["templates"][0]["extra"] = 1.into()).is_err());

    let narrow = edited_bundle(|bundle| {
        bundle["templates"][0]["max_subjects"] = 2.into();
        bundle["templates"][2]["max_subjects"] = 1.into();
        bundle["templates"][1]["question"] = "Retuned wording.".into();
    })
    .unwrap();
    let python = Fixture::scan(PYTHON_APP);
    let asked = python.view().questions(&narrow).unwrap();
    assert_eq!(asked[0].subjects.len(), 2);
    assert_eq!(asked[2].subjects.len(), 1);
    assert!(asked[2]
        .markdown
        .contains("Subjects (1 more not asked about)"));
    assert!(asked[1].markdown.contains("Retuned wording."));
    let default = questions(&python);
    assert_ne!(
        asked[1].question_id, default[1].question_id,
        "the template bundle is part of a question's identity"
    );
}

#[test]
fn one_prolific_structure_detector_does_not_crowd_out_the_rest() {
    let god_module: String = (0..21)
        .map(|index| format!("def step_{index}():\n    return {index}\n\n\n"))
        .collect();
    let mut files = PYTHON_APP.to_vec();
    files.extend([
        ("app/a_one.py", god_module.as_str()),
        ("app/a_two.py", god_module.as_str()),
    ]);
    let fixture = Fixture::scan(&files);
    let bundle = edited_bundle(|bundle| bundle["templates"][2]["max_subjects"] = 3.into()).unwrap();
    let asked = fixture.view().questions(&bundle).unwrap();
    let architecture = asked
        .iter()
        .find(|question| question.kind == QuestionKind::ArchitectureReview)
        .unwrap();
    let summaries = architecture
        .subjects
        .iter()
        .map(|subject| subject.summary.as_str())
        .collect::<Vec<_>>();
    assert!(
        summaries[0].contains("modules import each other"),
        "{summaries:?}"
    );
    assert!(
        summaries[1].contains("architectural_layering"),
        "{summaries:?}"
    );
    assert!(summaries[2].contains("god_module"), "{summaries:?}");
    assert!(architecture
        .markdown
        .contains("Subjects (1 more not asked about)"));
}
