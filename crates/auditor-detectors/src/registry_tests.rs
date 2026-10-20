use std::collections::BTreeSet;

use super::cell_dispositions::CELL_DISPOSITIONS;
use super::language_support::{
    catalog_languages, language_support, policy_mode, support_policy_count, SupportPolicy,
};
use super::*;

#[test]
fn catalog_ids_are_unique_and_well_formed() {
    let mut ids = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for spec in catalog_specs() {
        assert!(ids.insert(spec.id), "duplicate detector ID: {}", spec.id);
        assert!(
            owners.insert(format!("{:?}", spec.owner)),
            "detector owner is shared: {:?}",
            spec.owner
        );
        assert!(!spec.category.is_empty());
        assert!(spec.layer.starts_with('L'));
        assert_ne!(
            support_policy_for(spec.id),
            SupportPolicy::Unclassified,
            "detector {} has no explicit language-support policy",
            spec.id
        );
        assert_eq!(
            support_policy_count(spec.id),
            1,
            "detector {} must have exactly one language-support policy",
            spec.id
        );
        let claims = language_support(spec.id, support_policy_for(spec.id));
        assert_eq!(claims.len(), catalog_languages().len());
        assert_eq!(
            claims
                .iter()
                .map(|claim| claim.language)
                .collect::<BTreeSet<_>>(),
            catalog_languages().iter().copied().collect::<BTreeSet<_>>()
        );
    }
}

#[test]
fn gating_registry_is_a_strict_subset_of_full() {
    let gating = detector_catalog(ScanProfile::Gating)
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    let full = detector_catalog(ScanProfile::Full)
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    assert!(gating.is_subset(&full));
    assert!(gating.len() < full.len());
    assert!(detector_catalog(ScanProfile::Gating)
        .all(|spec| spec.minimum_profile == ScanProfile::Gating));
}

#[test]
fn cell_dispositions_name_only_unsupported_cataloged_cells() {
    let ids = catalog_specs().map(|spec| spec.id).collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for disposition in CELL_DISPOSITIONS {
        for id in disposition.ids {
            assert!(ids.contains(id), "disposition names unknown detector {id}");
            for language in disposition.languages {
                assert!(
                    seen.insert((*id, *language)),
                    "duplicate disposition for {id}/{language}"
                );
                assert_eq!(
                    policy_mode(id, language),
                    crate::LanguageSupportMode::Unsupported,
                    "disposition for {id}/{language} is stale: the policy already supports it",
                );
            }
        }
    }
}
