use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::case_review::identity::CanonicalIdentity;
use crate::{ContentDigest, StableId};

pub const INPUT_BINDING_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct InputBindingDigests {
    pub report: ContentDigest,
    pub evidence_graph: ContentDigest,
    pub repository_map: ContentDigest,
    pub source_snapshot: ContentDigest,
    pub case_template_bundle: ContentDigest,
    pub prompt_bundle: ContentDigest,
    pub route_configuration: ContentDigest,
    pub operator_brief: Option<ContentDigest>,
    pub evidence_attachments: Vec<ContentDigest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputBinding {
    pub schema_version: u32,
    pub report_digest: ContentDigest,
    pub evidence_graph_digest: ContentDigest,
    pub repository_map_digest: ContentDigest,
    pub source_snapshot_digest: ContentDigest,
    pub case_template_bundle_digest: ContentDigest,
    pub prompt_bundle_digest: ContentDigest,
    pub route_configuration_digest: ContentDigest,
    pub operator_brief_digest: Option<ContentDigest>,
    pub evidence_attachment_digests: Vec<ContentDigest>,
}

impl InputBinding {
    pub fn new(mut digests: InputBindingDigests) -> Result<Self, String> {
        digests.evidence_attachments.sort();
        if digests
            .evidence_attachments
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err("input binding contains duplicate evidence attachment digest".to_string());
        }
        Ok(Self {
            schema_version: INPUT_BINDING_SCHEMA_VERSION,
            report_digest: digests.report,
            evidence_graph_digest: digests.evidence_graph,
            repository_map_digest: digests.repository_map,
            source_snapshot_digest: digests.source_snapshot,
            case_template_bundle_digest: digests.case_template_bundle,
            prompt_bundle_digest: digests.prompt_bundle,
            route_configuration_digest: digests.route_configuration,
            operator_brief_digest: digests.operator_brief,
            evidence_attachment_digests: digests.evidence_attachments,
        })
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let binding: Self = serde_json::from_str(text)
            .map_err(|error| format!("invalid review input binding: {error}"))?;
        binding.validate()?;
        Ok(binding)
    }

    pub fn to_canonical_json(&self) -> String {
        crate::case_review::identity::canonical_json(
            &serde_json::to_value(self).expect("input binding is serializable"),
        )
    }

    pub fn stable_id(&self) -> StableId {
        StableId::from_identity(
            "review-input",
            INPUT_BINDING_SCHEMA_VERSION,
            &self.identity_fields(),
        )
        .expect("review-input is a valid typed prefix")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != INPUT_BINDING_SCHEMA_VERSION {
            return Err(format!(
                "unsupported review input binding schema version {}",
                self.schema_version
            ));
        }
        if self
            .evidence_attachment_digests
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("review input attachment digests must be sorted and unique".to_string());
        }
        Ok(())
    }
}

impl CanonicalIdentity for InputBinding {
    fn identity_fields(&self) -> Value {
        json!({
            "case_template_bundle_digest": self.case_template_bundle_digest,
            "evidence_attachment_digests": self.evidence_attachment_digests,
            "evidence_graph_digest": self.evidence_graph_digest,
            "operator_brief_digest": self.operator_brief_digest,
            "prompt_bundle_digest": self.prompt_bundle_digest,
            "report_digest": self.report_digest,
            "repository_map_digest": self.repository_map_digest,
            "route_configuration_digest": self.route_configuration_digest,
            "source_snapshot_digest": self.source_snapshot_digest,
        })
    }
}
