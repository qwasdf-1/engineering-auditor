use serde_json::{json, Value};

use super::{InputBinding, InputBindingDigests};
use crate::{
    CanonicalIdentity, ContentDigest, ContractKind, EvidenceAttachmentAdapter,
    EvidenceAttachmentKind, InputContract, StableId,
};

#[derive(Clone)]
struct TestContract {
    kind: ContractKind,
    schema_version: u32,
    supported_schema_version: u32,
    sequence: Vec<&'static str>,
    timestamp: u64,
    output_path: &'static str,
}

impl CanonicalIdentity for TestContract {
    fn identity_fields(&self) -> Value {
        json!({"sequence": self.sequence})
    }
}

impl InputContract for TestContract {
    fn contract_kind(&self) -> ContractKind {
        self.kind
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        self.supported_schema_version
    }

    fn validate_contract(&self) -> Result<(), String> {
        if self.sequence.is_empty() {
            Err("semantic sequence must not be empty".to_string())
        } else {
            Ok(())
        }
    }
}

impl EvidenceAttachmentAdapter for TestContract {
    fn attachment_kind(&self) -> EvidenceAttachmentKind {
        EvidenceAttachmentKind::RuntimeMeasurement
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        self.supported_schema_version
    }

    fn validate_attachment(&self) -> Result<(), String> {
        self.validate_contract()
    }
}

fn digest(value: &str) -> ContentDigest {
    ContentDigest::from_bytes(value.as_bytes())
}

fn binding(attachments: Vec<ContentDigest>) -> InputBinding {
    InputBinding::new(InputBindingDigests {
        report: digest("report"),
        evidence_graph: digest("graph"),
        repository_map: digest("map"),
        source_snapshot: digest("snapshot"),
        case_template_bundle: digest("templates"),
        prompt_bundle: digest("prompts"),
        route_configuration: digest("routes"),
        operator_brief: Some(digest("brief")),
        evidence_attachments: attachments,
    })
    .unwrap()
}

#[test]
fn base_types_round_trip_through_canonical_json() {
    let original = binding(vec![digest("b"), digest("a")]);
    let json = serde_json::to_string(&original).unwrap();
    let parsed = InputBinding::from_json(&json).unwrap();
    assert_eq!(parsed, original);
    let id_json = serde_json::to_string(&original.stable_id()).unwrap();
    assert_eq!(
        serde_json::from_str::<StableId>(&id_json).unwrap(),
        original.stable_id()
    );
    assert_eq!(parsed.to_canonical_json(), original.to_canonical_json());
    assert_eq!(
        InputBinding::from_json(&original.to_canonical_json()).unwrap(),
        original
    );
}

#[test]
fn attachment_set_order_does_not_change_input_identity() {
    let left = binding(vec![digest("a"), digest("b")]);
    let right = binding(vec![digest("b"), digest("a")]);
    assert_eq!(left, right);
    assert_eq!(left.stable_id(), right.stable_id());
}

#[test]
fn semantic_sequence_order_remains_significant() {
    let left = TestContract {
        kind: ContractKind::PromptBundle,
        schema_version: 1,
        supported_schema_version: 1,
        sequence: vec!["reviewer", "challenger"],
        timestamp: 1,
        output_path: "first.json",
    };
    let right = TestContract {
        sequence: vec!["challenger", "reviewer"],
        ..left.clone()
    };
    assert_ne!(
        StableId::from_identity("prompt-bundle", 1, &left.identity_fields()).unwrap(),
        StableId::from_identity("prompt-bundle", 1, &right.identity_fields()).unwrap()
    );
}

#[test]
fn timestamps_and_output_paths_do_not_change_identity() {
    let left = TestContract {
        kind: ContractKind::RouteConfiguration,
        schema_version: 2,
        supported_schema_version: 2,
        sequence: vec!["single", "challenger"],
        timestamp: 1,
        output_path: "one.json",
    };
    let right = TestContract {
        timestamp: 999,
        output_path: "two.json",
        ..left.clone()
    };
    assert_ne!(left.timestamp, right.timestamp);
    assert_ne!(left.output_path, right.output_path);
    assert_eq!(
        StableId::from_identity("route-configuration", 2, &left.identity_fields()).unwrap(),
        StableId::from_identity("route-configuration", 2, &right.identity_fields()).unwrap()
    );
}

#[test]
fn binding_rejects_unknown_field_wrong_version_and_duplicate_digest() {
    let mut value = serde_json::to_value(binding(Vec::new())).unwrap();
    value["unknown"] = json!(true);
    assert!(InputBinding::from_json(&value.to_string()).is_err());

    let mut value = serde_json::to_value(binding(Vec::new())).unwrap();
    value["schema_version"] = json!(99);
    assert!(InputBinding::from_json(&value.to_string())
        .unwrap_err()
        .contains("unsupported"));

    assert!(InputBinding::new(InputBindingDigests {
        report: digest("report"),
        evidence_graph: digest("graph"),
        repository_map: digest("map"),
        source_snapshot: digest("snapshot"),
        case_template_bundle: digest("templates"),
        prompt_bundle: digest("prompts"),
        route_configuration: digest("routes"),
        operator_brief: None,
        evidence_attachments: vec![digest("same"), digest("same")],
    })
    .is_err());
}
