use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use auditor_report::{parse_repository_map_reference, validate_report_json};
use serde_json::{json, Value};

use super::{
    ContractKind, EvidenceAttachment, EvidenceAttachmentAdapter, EvidenceAttachmentKind,
    InputBinding, InputBindingDigests, InputContract, VerifiedContract,
};
use crate::case_review::identity::canonicalize;
use crate::{
    load_and_validate_repository_map, ContentDigest, RepositoryMap, RepositoryMapError,
    ReviewBrief, StableId,
};

pub struct InputVerificationRequest<'a> {
    pub report_path: &'a Path,
    pub repo_root: &'a Path,
    pub operator_brief: Option<&'a ReviewBrief>,
    pub case_template_bundle: Option<&'a dyn InputContract>,
    pub prompt_bundle: Option<&'a dyn InputContract>,
    pub route_configuration: Option<&'a dyn InputContract>,
    pub attachment_adapters: &'a [&'a dyn EvidenceAttachmentAdapter],
    pub required_attachment_kinds: &'a [EvidenceAttachmentKind],
    pub expected_binding: Option<&'a InputBinding>,
}

#[derive(Clone, Debug)]
pub struct ReviewInput {
    schema_version: u32,
    id: StableId,
    binding: InputBinding,
    report: Value,
    repository_map: RepositoryMap,
    operator_brief: Option<ReviewBrief>,
    case_template_bundle: VerifiedContract,
    prompt_bundle: VerifiedContract,
    route_configuration: VerifiedContract,
    evidence_attachments: Vec<EvidenceAttachment>,
}

impl ReviewInput {
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn id(&self) -> &StableId {
        &self.id
    }

    pub fn binding(&self) -> &InputBinding {
        &self.binding
    }

    pub fn report(&self) -> &Value {
        &self.report
    }

    pub fn finding(&self, finding_id: &str) -> Option<&Value> {
        self.report["findings"]
            .as_array()?
            .iter()
            .find(|finding| finding["id"].as_str() == Some(finding_id))
    }

    pub fn repository_map(&self) -> &RepositoryMap {
        &self.repository_map
    }

    pub fn operator_brief(&self) -> Option<&ReviewBrief> {
        self.operator_brief.as_ref()
    }

    pub fn case_template_bundle(&self) -> &VerifiedContract {
        &self.case_template_bundle
    }

    pub fn prompt_bundle(&self) -> &VerifiedContract {
        &self.prompt_bundle
    }

    pub fn route_configuration(&self) -> &VerifiedContract {
        &self.route_configuration
    }

    pub fn evidence_attachments(&self) -> &[EvidenceAttachment] {
        &self.evidence_attachments
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputError {
    InvalidContract {
        contract: &'static str,
        detail: String,
    },
    StaleInput {
        contract: &'static str,
        typed_id: String,
    },
    UnsupportedVersion {
        contract: &'static str,
        found: u32,
        supported: u32,
    },
    BindingMismatch {
        contract: &'static str,
        expected: String,
        actual: String,
    },
    MissingAttachmentAdapter {
        kind: EvidenceAttachmentKind,
    },
    UnsafePath {
        contract: &'static str,
        path: String,
    },
    MissingInput {
        contract: &'static str,
    },
    DuplicateAttachment {
        typed_id: String,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContract { contract, detail } => {
                write!(formatter, "invalid {contract} contract: {detail}")
            }
            Self::StaleInput { contract, typed_id } => {
                write!(formatter, "stale {contract} input at {typed_id}")
            }
            Self::UnsupportedVersion {
                contract,
                found,
                supported,
            } => write!(
                formatter,
                "unsupported {contract} schema version {found}; expected {supported}"
            ),
            Self::BindingMismatch {
                contract,
                expected,
                actual,
            } => write!(
                formatter,
                "{contract} binding mismatch: expected {expected}, actual {actual}"
            ),
            Self::MissingAttachmentAdapter { kind } => {
                write!(
                    formatter,
                    "missing typed evidence attachment adapter for {}",
                    kind.as_str()
                )
            }
            Self::UnsafePath { contract, path } => {
                write!(formatter, "{contract} contains unsafe path: {path}")
            }
            Self::MissingInput { contract } => {
                write!(formatter, "missing mandatory {contract} input")
            }
            Self::DuplicateAttachment { typed_id } => {
                write!(
                    formatter,
                    "duplicate evidence attachment identity {typed_id}"
                )
            }
        }
    }
}

impl std::error::Error for InputError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputVerifier;

impl InputVerifier {
    pub fn verify(&self, request: InputVerificationRequest<'_>) -> Result<ReviewInput, InputError> {
        let report_text = std::fs::read_to_string(request.report_path).map_err(|error| {
            InputError::InvalidContract {
                contract: "report-v5",
                detail: error.to_string(),
            }
        })?;
        validate_report_json(&report_text).map_err(|detail| InputError::InvalidContract {
            contract: "report-v5",
            detail,
        })?;
        let report: Value =
            serde_json::from_str(&report_text).map_err(|error| InputError::InvalidContract {
                contract: "report-v5",
                detail: error.to_string(),
            })?;
        let evidence_graph =
            report
                .get("evidence_graph")
                .ok_or_else(|| InputError::InvalidContract {
                    contract: "evidence-graph-v2",
                    detail: "missing mandatory evidence graph".to_string(),
                })?;
        let map_reference = parse_repository_map_reference(&report_text).map_err(|detail| {
            InputError::InvalidContract {
                contract: "repository-map-v1",
                detail,
            }
        })?;
        let repository_map = load_and_validate_repository_map(
            request.report_path,
            request.repo_root,
            &map_reference,
        )
        .map_err(map_error)?;

        if let Some(brief) = request.operator_brief {
            brief
                .validate()
                .map_err(|detail| InputError::InvalidContract {
                    contract: "review-brief-v1",
                    detail,
                })?;
        }
        let case_template_bundle = verify_contract(
            ContractKind::CaseTemplateBundle,
            request.case_template_bundle,
        )?;
        let prompt_bundle = verify_contract(ContractKind::PromptBundle, request.prompt_bundle)?;
        let route_configuration = verify_contract(
            ContractKind::RouteConfiguration,
            request.route_configuration,
        )?;
        let evidence_attachments = verify_attachments(
            request.attachment_adapters,
            request.required_attachment_kinds,
        )?;
        let operator_brief_digest = request
            .operator_brief
            .map(|brief| {
                serde_json::to_value(brief)
                    .map(|value| ContentDigest::from_canonical_value(&value))
                    .map_err(|error| InputError::InvalidContract {
                        contract: "review-brief-v1",
                        detail: error.to_string(),
                    })
            })
            .transpose()?;
        let map_digest = map_reference
            .digest
            .as_deref()
            .ok_or_else(|| InputError::InvalidContract {
                contract: "repository-map-v1",
                detail: "missing mandatory map digest".to_string(),
            })
            .and_then(|value| {
                ContentDigest::from_unprefixed(value).map_err(|error| InputError::InvalidContract {
                    contract: "repository-map-v1",
                    detail: error.to_string(),
                })
            })?;
        let snapshot_digest = ContentDigest::from_unprefixed(
            &repository_map.source_snapshot_digest,
        )
        .map_err(|error| InputError::InvalidContract {
            contract: "repository-map-v1",
            detail: error.to_string(),
        })?;
        let binding = InputBinding::new(InputBindingDigests {
            report: ContentDigest::from_canonical_value(&report),
            evidence_graph: ContentDigest::from_canonical_value(evidence_graph),
            repository_map: map_digest,
            source_snapshot: snapshot_digest,
            case_template_bundle: case_template_bundle.digest.clone(),
            prompt_bundle: prompt_bundle.digest.clone(),
            route_configuration: route_configuration.digest.clone(),
            operator_brief: operator_brief_digest,
            evidence_attachments: evidence_attachments
                .iter()
                .map(|attachment| attachment.digest.clone())
                .collect(),
        })
        .map_err(|detail| InputError::InvalidContract {
            contract: "review-input-binding-v1",
            detail,
        })?;
        if let Some(expected) = request.expected_binding {
            verify_expected_binding(expected, &binding)?;
        }
        let id = binding.stable_id();
        Ok(ReviewInput {
            schema_version: super::INPUT_BINDING_SCHEMA_VERSION,
            id,
            binding,
            report,
            repository_map,
            operator_brief: request.operator_brief.cloned(),
            case_template_bundle,
            prompt_bundle,
            route_configuration,
            evidence_attachments,
        })
    }
}

fn verify_contract(
    expected_kind: ContractKind,
    contract: Option<&dyn InputContract>,
) -> Result<VerifiedContract, InputError> {
    let contract = contract.ok_or(InputError::MissingInput {
        contract: expected_kind.as_str(),
    })?;
    if contract.contract_kind() != expected_kind {
        return Err(InputError::InvalidContract {
            contract: expected_kind.as_str(),
            detail: format!(
                "received {} typed contract",
                contract.contract_kind().as_str()
            ),
        });
    }
    validate_typed_input(
        expected_kind.as_str(),
        contract.schema_version(),
        contract.supported_schema_version(),
        || contract.validate_contract(),
    )?;
    let semantic_snapshot = canonicalize(&contract.identity_fields());
    let envelope = json!({
        "contract_kind": expected_kind,
        "identity_fields": semantic_snapshot,
        "schema_version": contract.schema_version(),
    });
    let digest = ContentDigest::from_canonical_value(&envelope);
    let id = typed_stable_id(
        expected_kind.as_str(),
        contract.schema_version(),
        &semantic_snapshot,
        expected_kind.as_str(),
    )?;
    Ok(VerifiedContract {
        kind: expected_kind,
        schema_version: contract.schema_version(),
        id,
        digest,
        semantic_snapshot,
    })
}

fn verify_attachments(
    adapters: &[&dyn EvidenceAttachmentAdapter],
    required_kinds: &[EvidenceAttachmentKind],
) -> Result<Vec<EvidenceAttachment>, InputError> {
    for kind in required_kinds {
        if !adapters
            .iter()
            .any(|adapter| adapter.attachment_kind() == *kind)
        {
            return Err(InputError::MissingAttachmentAdapter { kind: *kind });
        }
    }
    let mut seen = BTreeSet::new();
    let mut attachments = Vec::with_capacity(adapters.len());
    for adapter in adapters {
        let kind = adapter.attachment_kind();
        validate_typed_input(
            "evidence-attachment",
            adapter.schema_version(),
            adapter.supported_schema_version(),
            || adapter.validate_attachment(),
        )?;
        let semantic_snapshot = canonicalize(&adapter.identity_fields());
        let id = typed_stable_id(
            kind.as_str(),
            adapter.schema_version(),
            &semantic_snapshot,
            "evidence-attachment",
        )?;
        if !seen.insert(id.clone()) {
            return Err(InputError::DuplicateAttachment {
                typed_id: id.to_string(),
            });
        }
        let digest = ContentDigest::from_canonical_value(&json!({
            "attachment_kind": kind,
            "identity_fields": semantic_snapshot,
            "schema_version": adapter.schema_version(),
        }));
        attachments.push(EvidenceAttachment {
            kind,
            schema_version: adapter.schema_version(),
            id,
            digest,
            semantic_snapshot,
        });
    }
    attachments.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(attachments)
}

fn validate_typed_input(
    contract: &'static str,
    found: u32,
    supported: u32,
    validation: impl FnOnce() -> Result<(), String>,
) -> Result<(), InputError> {
    if found != supported {
        return Err(InputError::UnsupportedVersion {
            contract,
            found,
            supported,
        });
    }
    validation().map_err(|detail| InputError::InvalidContract { contract, detail })
}

fn typed_stable_id(
    prefix: &'static str,
    schema_version: u32,
    semantic_snapshot: &Value,
    contract: &'static str,
) -> Result<StableId, InputError> {
    StableId::from_identity(prefix, schema_version, semantic_snapshot).map_err(|error| {
        InputError::InvalidContract {
            contract,
            detail: error.to_string(),
        }
    })
}

fn verify_expected_binding(
    expected: &InputBinding,
    actual: &InputBinding,
) -> Result<(), InputError> {
    expected
        .validate()
        .map_err(|detail| InputError::InvalidContract {
            contract: "review-input-binding-v1",
            detail,
        })?;
    if expected == actual {
        return Ok(());
    }
    let contract = if expected.report_digest != actual.report_digest {
        "report-v5"
    } else if expected.evidence_graph_digest != actual.evidence_graph_digest {
        "evidence-graph-v2"
    } else if expected.repository_map_digest != actual.repository_map_digest {
        "repository-map-v1"
    } else {
        "review-input-binding-v1"
    };
    Err(InputError::BindingMismatch {
        contract,
        expected: expected.stable_id().to_string(),
        actual: actual.stable_id().to_string(),
    })
}

fn map_error(error: RepositoryMapError) -> InputError {
    match error {
        RepositoryMapError::StaleSourceSnapshot(typed_id) => InputError::StaleInput {
            contract: "repository-map-v1",
            typed_id,
        },
        RepositoryMapError::ArtifactDigestMismatch => InputError::BindingMismatch {
            contract: "repository-map-v1",
            expected: "report-bound-map-digest".to_string(),
            actual: "live-map-digest".to_string(),
        },
        RepositoryMapError::Contract(detail)
            if detail.contains("unsafe path") || detail.contains("escapes root") =>
        {
            InputError::UnsafePath {
                contract: "repository-map-v1",
                path: redacted_path_detail(&detail),
            }
        }
        RepositoryMapError::Contract(detail)
        | RepositoryMapError::Io(detail)
        | RepositoryMapError::LookupBudgetExceeded(detail) => InputError::InvalidContract {
            contract: "repository-map-v1",
            detail,
        },
    }
}

fn redacted_path_detail(detail: &str) -> String {
    detail
        .rsplit_once(':')
        .map(|(_, path)| path.trim().to_string())
        .unwrap_or_else(|| "<invalid>".to_string())
}
