use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{
    CaseInstantiationError, CaseTemplateBundle, CaseTemplateRegistry, InferenceGuardKind,
    ProbeKind, TemplateStatus,
};

#[test]
fn checked_in_bundle_is_complete_and_schema_documents_are_closed() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    assert_eq!(bundle.entries.len(), 3);
    assert_eq!(bundle.entries[0].status, TemplateStatus::Production);
    assert!(bundle.entries[1..]
        .iter()
        .all(|entry| entry.status == TemplateStatus::Shadow));
    for schema in [
        include_str!("../../../../../contracts/case-template-v1.schema.json"),
        include_str!("../../../../../contracts/case-template-bundle-v1.schema.json"),
        include_str!("../../../../../contracts/engineering-case-v1.schema.json"),
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
fn lifecycle_template_carries_mandatory_negative_guards_without_prejudging_them() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    let template = bundle.entries[0].template.as_ref().unwrap();
    assert_eq!(template.mandatory_probes.len(), 8);
    assert_eq!(
        template
            .inference_guards
            .iter()
            .map(|guard| guard.kind)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            InferenceGuardKind::RelevantMutation,
            InferenceGuardKind::DistinctConfiguration,
            InferenceGuardKind::TestOnlyRepetition,
            InferenceGuardKind::FreshnessConstraint,
            InferenceGuardKind::RetentionRisk,
            InferenceGuardKind::QuantitativeBenefit,
        ])
    );
    assert!(template.allows_optional_probe(ProbeKind::Counterexample));
    assert!(!template.allows_optional_probe(ProbeKind::Mutation));
}

#[test]
fn bundle_rejects_missing_version_forbidden_conclusion_and_duplicate_owner() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    let mut missing_version = serde_json::to_value(&bundle).unwrap();
    missing_version["entries"][0]["version"] = json!(0);
    assert!(CaseTemplateBundle::from_json(&missing_version.to_string()).is_err());

    let mut forbidden = serde_json::to_value(&bundle).unwrap();
    forbidden["entries"][0]["template"]["closure_policy"]["allowed_conclusions"][0] =
        json!("invented_conclusion");
    assert!(CaseTemplateBundle::from_json(&forbidden.to_string()).is_err());

    let mut duplicate = bundle.clone();
    duplicate.entries[1] = duplicate.entries[0].clone();
    assert!(matches!(
        CaseTemplateRegistry::from_bundle_for_test(duplicate),
        Err(CaseInstantiationError::DuplicateOwner { .. })
    ));
}

#[test]
fn canonical_round_trip_and_template_edit_change_digest() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    assert_eq!(
        CaseTemplateBundle::from_json(&bundle.to_canonical_json()).unwrap(),
        bundle
    );
    let mut edited = bundle.clone();
    edited.entries[0]
        .template
        .as_mut()
        .unwrap()
        .evidence_contract
        .required[0]
        .description
        .push_str(" Updated.");
    assert_ne!(bundle.digest(), edited.digest());
}
