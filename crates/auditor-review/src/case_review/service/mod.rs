mod execute;
mod load;
mod native_repair;
mod plan;
mod provider_call;
mod provider_record;
mod qualification;
mod review_case;
mod verify;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    CanonicalIdentity, EngineeringCase, EvidenceAttachmentAdapter, EvidenceAttachmentKind,
    EvidencePacket, HostedConsent, InputBinding, ProbeDefinition, ProbeLookupPlan, ProbeResult,
    ReviewAgenda, ReviewRoutesV2, StableId, TemplateNotEnabledRecord,
};

pub use crate::service::ReviewServiceError;

pub const REVIEW_PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct ReviewPlanRequest {
    pub report: PathBuf,
    pub repo_root: PathBuf,
    pub case_templates: PathBuf,
    pub routes: PathBuf,
    pub prompt_bundle: PathBuf,
    pub brief: Option<PathBuf>,
    pub evidence: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewInputSources {
    pub report: PathBuf,
    pub repo_root: PathBuf,
    pub case_templates: PathBuf,
    pub routes: PathBuf,
    pub prompt_bundle: PathBuf,
    pub brief: Option<PathBuf>,
    pub evidence: Vec<PathBuf>,
}

impl From<&ReviewPlanRequest> for ReviewInputSources {
    fn from(request: &ReviewPlanRequest) -> Self {
        Self {
            report: request.report.clone(),
            repo_root: request.repo_root.clone(),
            case_templates: request.case_templates.clone(),
            routes: request.routes.clone(),
            prompt_bundle: request.prompt_bundle.clone(),
            brief: request.brief.clone(),
            evidence: request.evidence.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileEvidenceAttachment {
    pub schema_version: u32,
    pub kind: EvidenceAttachmentKind,
    pub content: Value,
}

impl CanonicalIdentity for FileEvidenceAttachment {
    fn identity_fields(&self) -> Value {
        self.content.clone()
    }
}

impl EvidenceAttachmentAdapter for FileEvidenceAttachment {
    fn attachment_kind(&self) -> EvidenceAttachmentKind {
        self.kind
    }

    fn schema_version(&self) -> u32 {
        self.schema_version
    }

    fn supported_schema_version(&self) -> u32 {
        1
    }

    fn validate_attachment(&self) -> Result<(), String> {
        if self.schema_version != 1 || !self.content.is_object() {
            return Err(
                "file evidence attachment requires schema version 1 and object content".to_string(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPlanCase {
    pub case: EngineeringCase,
    pub evidence_packet: EvidencePacket,
    pub probe_definitions: Vec<ProbeDefinition>,
    pub deterministic_probe_results: Vec<ProbeResult>,
    pub lookup_plan: ProbeLookupPlan,
    pub routing: crate::ModeRoutingOutcome,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPlanOutput {
    pub schema_version: u32,
    pub plan_id: StableId,
    pub input_binding: InputBinding,
    pub sources: ReviewInputSources,
    pub agenda: ReviewAgenda,
    pub cases: Vec<ReviewPlanCase>,
    pub unavailable_cases: Vec<TemplateNotEnabledRecord>,
}

impl ReviewPlanOutput {
    pub fn parse(text: &str) -> Result<Self, ReviewServiceError> {
        let plan: Self = serde_json::from_str(text)
            .map_err(|error| ReviewServiceError::Failed(error.to_string()))?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), ReviewServiceError> {
        if self.schema_version != REVIEW_PLAN_SCHEMA_VERSION
            || self.input_binding != self.agenda.input_binding
            || self.plan_id != self.expected_id()?
        {
            return Err(ReviewServiceError::Failed(
                "review plan schema, binding, or identity is stale".to_string(),
            ));
        }
        let mut case_ids = std::collections::BTreeSet::new();
        for planned in &self.cases {
            if !case_ids.insert(&planned.case.case_id)
                || planned.evidence_packet.case_id != planned.case.case_id
                || planned
                    .probe_definitions
                    .iter()
                    .any(|probe| probe.case_id != planned.case.case_id)
                || planned
                    .deterministic_probe_results
                    .iter()
                    .any(|result| result.case_id != planned.case.case_id)
            {
                return Err(ReviewServiceError::Failed(
                    "review plan contains a dangling or duplicate case record".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn expected_id(&self) -> Result<StableId, ReviewServiceError> {
        StableId::from_identity(
            "review-plan",
            REVIEW_PLAN_SCHEMA_VERSION,
            &json!({
                "agenda": self.agenda,
                "cases": self.cases,
                "input_binding": self.input_binding,
                "unavailable_cases": self.unavailable_cases,
            }),
        )
        .map_err(|error| ReviewServiceError::Failed(error.to_string()))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseInspection {
    pub plan_id: StableId,
    pub case: EngineeringCase,
    pub finding_count: usize,
    pub evidence_item_count: usize,
    pub evidence_bytes: usize,
    pub closure_state: crate::ClosureState,
    pub probe_definitions: Vec<ProbeDefinition>,
    pub deterministic_probe_results: Vec<ProbeResult>,
    pub allowed_lookups: ProbeLookupPlan,
    pub mode: crate::ModeSelection,
    pub route_closure: Option<crate::CaseClosure>,
}

#[derive(Clone, Debug)]
pub struct ReviewRunRequest {
    pub plan: ReviewPlanRequest,
    pub output: PathBuf,
    pub consent: HostedConsent,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewRunOutput {
    pub schema_version: u32,
    pub review_id: StableId,
    pub reviewed_report_path: PathBuf,
    pub reviewed_report_digest: String,
    pub review_artifact_path: PathBuf,
    pub review_artifact_digest: String,
    pub review_plan_path: PathBuf,
    pub case_count: usize,
    pub accepted_recommendation_count: usize,
    pub blocked_case_count: usize,
    pub contested_case_count: usize,
    pub challenge_response_count: usize,
    #[serde(skip_serializing)]
    pub reviewed_report: String,
}

#[derive(Clone, Debug)]
pub struct ReviewVerifyRequest {
    pub repo_root: PathBuf,
    pub report: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewVerifyOutput {
    pub schema_version: u32,
    pub review_id: StableId,
    pub artifact_digest: String,
    pub case_count: usize,
    pub accepted_recommendation_count: usize,
    pub blocked_case_count: usize,
    pub contested_case_count: usize,
    pub challenge_response_count: usize,
    pub valid: bool,
}

pub use execute::run_case_engineering_review;
pub use plan::{
    inspect_engineering_case, plan_engineering_review, render_engineering_case_context,
};
pub use verify::verify_case_engineering_review;

pub(super) struct LoadedPlanInputs {
    input: crate::ReviewInput,
    routes: ReviewRoutesV2,
    prompts: crate::ReviewPromptBundleV2,
    source_request: ReviewPlanRequest,
}

fn failed(message: impl Into<String>) -> ReviewServiceError {
    ReviewServiceError::Failed(message.into())
}

pub(super) fn checkpoint(
    control: &dyn crate::ReviewControl,
    stage: crate::ReviewStage,
) -> Result<(), ReviewServiceError> {
    control
        .checkpoint(stage)
        .then_some(())
        .ok_or(ReviewServiceError::Cancelled)
}

pub(super) fn review_plan_path(report: &Path) -> PathBuf {
    super::publish::sidecar_path(report, "review-plan.json")
}
