use serde_json::{json, Value};

use super::{
    CaseInstantiationError, CaseTemplateBundle, CaseTemplateRegistry, ClaimSlot,
    RecommendationFamily, TemplateStatus,
};

#[test]
fn checked_in_bundle_is_complete_and_schema_documents_are_closed() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    assert_eq!(bundle.entries.len(), 3);
    assert_eq!(bundle.entries[0].status, TemplateStatus::Production);
    assert!(bundle.entries[1..]
        .iter()
        .all(|entry| entry.status == TemplateStatus::Shadow));
    for (schema, version) in [
        (
            include_str!("../../../../../contracts/case-template-v1.schema.json"),
            super::CASE_TEMPLATE_SCHEMA_VERSION,
        ),
        (
            include_str!("../../../../../contracts/case-template-bundle-v1.schema.json"),
            super::CASE_TEMPLATE_BUNDLE_SCHEMA_VERSION,
        ),
        (
            include_str!("../../../../../contracts/engineering-case-v1.schema.json"),
            super::ENGINEERING_CASE_SCHEMA_VERSION,
        ),
    ] {
        let value: Value = serde_json::from_str(schema).unwrap();
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["additionalProperties"], false);
        assert_eq!(value["properties"]["schema_version"]["const"], version);
    }
}

#[test]
fn lifecycle_template_names_its_claim_slots_and_recommendation_families() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    let template = bundle.entries[0].template.as_ref().unwrap();
    assert!(template
        .claim_slots
        .contains(&ClaimSlot::LifecycleConclusion));
    assert!(template
        .recommendation_families
        .contains(&RecommendationFamily::LifecycleReownership));
}

#[test]
fn bundle_rejects_missing_version_unknown_slot_and_duplicate_owner() {
    let bundle = CaseTemplateBundle::checked_in().unwrap();
    let mut missing_version = serde_json::to_value(&bundle).unwrap();
    missing_version["entries"][0]["version"] = json!(0);
    assert!(CaseTemplateBundle::from_json(&missing_version.to_string()).is_err());

    let mut forbidden = serde_json::to_value(&bundle).unwrap();
    forbidden["entries"][0]["template"]["claim_slots"][0] = json!("invented_slot");
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
