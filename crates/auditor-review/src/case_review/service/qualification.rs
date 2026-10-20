use std::collections::BTreeSet;

use crate::{
    case_tool_schema_digest, qualification_binding, qualify_case_target, CaseQualificationRecord,
    CaseQualificationRuntime, QualificationScope, ReviewControl, ReviewPlanOutput, ReviewRole,
    ReviewRoutesV2, ReviewStage, TranscriptRecorder, Transport, TransportRequest,
    TransportResponse,
};

use super::{checkpoint, failed, ReviewServiceError};

pub(super) struct QualificationRuntime<'a> {
    pub routes: &'a ReviewRoutesV2,
    pub prompts: &'a crate::ReviewPromptBundleV2,
    pub transport: &'a dyn Transport,
    pub consent: crate::HostedConsent,
    pub control: &'a dyn ReviewControl,
    pub transcript: &'a TranscriptRecorder,
    pub transcript_enabled: bool,
}

/// The provider chains a case's deliberation mode runs: the single reviewer
/// and challenger, plus the three ensemble slots for an ensemble case.
fn role_chains(
    routes: &ReviewRoutesV2,
    mode: crate::DeliberationMode,
) -> Result<Vec<(ReviewRole, &crate::V2ProviderChain)>, ReviewServiceError> {
    let mut chains = vec![
        (ReviewRole::SingleReviewer, &routes.single_reviewer),
        (ReviewRole::Challenger, &routes.challenger),
    ];
    if mode == crate::DeliberationMode::Ensemble {
        let ensemble = routes
            .ensemble
            .as_ref()
            .ok_or_else(|| failed("ensemble case planned without three configured route slots"))?;
        chains.extend([
            (ReviewRole::ReviewerA, &ensemble.reviewer_a),
            (ReviewRole::ReviewerB, &ensemble.reviewer_b),
            (ReviewRole::ReviewerC, &ensemble.reviewer_c),
        ]);
    }
    Ok(chains)
}

pub(super) fn qualify_plan(
    plan: &ReviewPlanOutput,
    runtime: QualificationRuntime<'_>,
) -> Result<Vec<CaseQualificationRecord>, ReviewServiceError> {
    let guarded = CheckpointTransport {
        inner: runtime.transport,
        control: runtime.control,
        stage: ReviewStage::Qualifying,
    };
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    let hosted = plan.cases.iter().filter(|planned| {
        planned.routing.closure.is_none()
            && planned.routing.selection.mode != crate::DeliberationMode::Deterministic
    });
    for planned in hosted {
        let probe_kinds = planned
            .probe_definitions
            .iter()
            .map(|probe| probe.kind)
            .collect::<Vec<_>>();
        for (role, chain) in role_chains(runtime.routes, planned.routing.selection.mode)? {
            for target in &chain.targets {
                let binding = qualification_binding(
                    target.clone(),
                    QualificationScope::new(
                        role,
                        planned.case.case_kind,
                        probe_kinds.clone(),
                        runtime.prompts.bundle_id.clone(),
                        case_tool_schema_digest(),
                    ),
                );
                if !seen.insert(binding.qualification_id()) {
                    continue;
                }
                checkpoint(runtime.control, ReviewStage::Qualifying)?;
                let provider = runtime
                    .routes
                    .provider(&target.provider_id)
                    .ok_or_else(|| {
                        failed(format!(
                            "qualification target references unknown provider {}",
                            target.provider_id
                        ))
                    })?;
                let record = qualify_case_target(
                    provider,
                    binding,
                    CaseQualificationRuntime {
                        transport: &guarded,
                        consent: runtime.consent,
                        budget: runtime.routes.budget_for(planned.case.case_kind),
                        transcript: runtime.transcript,
                        transcript_enabled: runtime.transcript_enabled,
                    },
                );
                if !runtime.control.checkpoint(ReviewStage::Qualifying) {
                    return Err(ReviewServiceError::Cancelled);
                }
                records.push(record);
            }
        }
    }
    records.sort_by(|left, right| left.qualification_id.cmp(&right.qualification_id));
    Ok(records)
}

struct CheckpointTransport<'a> {
    inner: &'a dyn Transport,
    control: &'a dyn ReviewControl,
    stage: ReviewStage,
}

impl Transport for CheckpointTransport<'_> {
    fn post(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        if !self.control.checkpoint(self.stage) {
            return Err("engineering review was cancelled".to_string());
        }
        self.inner.post(request)
    }

    fn get(&self, request: TransportRequest) -> Result<TransportResponse, String> {
        if !self.control.checkpoint(self.stage) {
            return Err("engineering review was cancelled".to_string());
        }
        self.inner.get(request)
    }
}
