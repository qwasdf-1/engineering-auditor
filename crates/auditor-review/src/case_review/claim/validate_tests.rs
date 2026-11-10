use serde_json::json;

use super::*;
use crate::{ClaimConfidence, ClaimDraft, EngineeringClaimKind};

fn case_id() -> StableId {
    StableId::from_identity("engineering-case", 2, &json!({"case": "claims"})).expect("case id")
}

fn claim(
    status: ClaimStatus,
    supporting: &[&str],
    counter: &[&str],
    dependencies: &[&EngineeringClaim],
) -> EngineeringClaim {
    EngineeringClaim::from_draft(
        case_id(),
        ClaimDraft {
            slot: ClaimSlot::LifecycleConclusion,
            kind: EngineeringClaimKind::RootCause,
            proposition: format!("claim {status:?} {supporting:?} {counter:?}"),
            status,
            supporting_evidence_ids: supporting.iter().map(|id| id.to_string()).collect(),
            counter_evidence_ids: counter.iter().map(|id| id.to_string()).collect(),
            dependency_claim_ids: dependencies
                .iter()
                .map(|dependency| dependency.claim_id.clone())
                .collect(),
            assumptions: Vec::new(),
            confidence: ClaimConfidence::High,
        },
    )
    .expect("claim")
}

fn known() -> BTreeSet<String> {
    ["finding:a", "source-span:b", "symbol:c"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn validate(claims: &[EngineeringClaim]) -> Result<(), String> {
    validate_claims(
        &case_id(),
        &[ClaimSlot::LifecycleConclusion],
        &known(),
        claims,
    )
}

#[test]
fn substantiated_claim_with_known_citations_is_accepted() {
    let claim = claim(
        ClaimStatus::Substantiated,
        &["finding:a", "source-span:b"],
        &[],
        &[],
    );
    assert_eq!(validate(&[claim]), Ok(()));
}

#[test]
fn unknown_citation_is_rejected() {
    let claim = claim(ClaimStatus::Substantiated, &["finding:invented"], &[], &[]);
    let error = validate(&[claim]).expect_err("unknown evidence");
    assert!(error.contains("cites unknown evidence"), "{error}");
}

#[test]
fn claim_without_citation_or_dependency_is_rejected() {
    let draft = ClaimDraft {
        slot: ClaimSlot::LifecycleConclusion,
        kind: EngineeringClaimKind::RootCause,
        proposition: "Unsupported.".to_string(),
        status: ClaimStatus::Substantiated,
        supporting_evidence_ids: Vec::new(),
        counter_evidence_ids: Vec::new(),
        dependency_claim_ids: Vec::new(),
        assumptions: Vec::new(),
        confidence: ClaimConfidence::Low,
    };
    assert!(EngineeringClaim::from_draft(case_id(), draft).is_err());
}

#[test]
fn counter_evidence_rules_out_substantiated_but_allows_contested() {
    let substantiated = claim(
        ClaimStatus::Substantiated,
        &["finding:a"],
        &["symbol:c"],
        &[],
    );
    let error = validate(&[substantiated]).expect_err("countered");
    assert!(
        error.ends_with("is not supported: it cites counter-evidence; a claim with evidence on both sides is partially_substantiated or contested"),
        "{error}"
    );
    let contested = claim(ClaimStatus::Contested, &["finding:a"], &["symbol:c"], &[]);
    assert_eq!(validate(&[contested]), Ok(()));
}

#[test]
fn contested_dependency_supports_contested_and_blocks_substantiated() {
    let dependency = claim(ClaimStatus::Contested, &["finding:a"], &["symbol:c"], &[]);
    let contested = claim(ClaimStatus::Contested, &[], &[], &[&dependency]);
    assert_eq!(validate(&[dependency.clone(), contested]), Ok(()));
    let substantiated = claim(
        ClaimStatus::Substantiated,
        &["finding:a"],
        &[],
        &[&dependency],
    );
    let error = validate(&[dependency, substantiated]).expect_err("weak dependency");
    assert!(
        error.contains("it depends on a claim that is not substantiated"),
        "{error}"
    );
    let uncountered = claim(ClaimStatus::Contested, &["finding:a"], &[], &[]);
    let error = validate(&[uncountered]).expect_err("nothing contests it");
    assert!(
        error.ends_with("it cites no counter-evidence and depends on no contested claim"),
        "{error}"
    );
}

#[test]
fn tampered_identity_and_disallowed_slot_are_rejected() {
    let mut tampered = claim(ClaimStatus::Substantiated, &["finding:a"], &[], &[]);
    tampered.content.proposition = "Changed after hashing.".to_string();
    assert!(validate(&[tampered]).is_err());
    let allowed = validate_claims(
        &case_id(),
        &[ClaimSlot::Repetition],
        &known(),
        &[claim(ClaimStatus::Blocked, &["finding:a"], &[], &[])],
    );
    assert!(allowed.expect_err("slot").contains("not allowed"));
}
