use std::path::Path;

use crate::{
    CaseTemplateBundle, EvidenceAttachmentAdapter, InputVerificationRequest, InputVerifier,
    ReviewBrief, ReviewPromptBundleV2, ReviewRoutesV2,
};

use super::{
    failed, FileEvidenceAttachment, LoadedPlanInputs, ReviewPlanRequest, ReviewServiceError,
};

pub(super) fn load_inputs(
    request: &ReviewPlanRequest,
    expected: Option<&crate::InputBinding>,
) -> Result<LoadedPlanInputs, ReviewServiceError> {
    let templates = parse_file(
        &request.case_templates,
        "case template bundle",
        CaseTemplateBundle::from_json,
    )?;
    let routes = parse_file(&request.routes, "v2 routes", ReviewRoutesV2::parse)?;
    let prompts = parse_file(
        &request.prompt_bundle,
        "v2 prompt bundle",
        ReviewPromptBundleV2::parse,
    )?;
    let brief = request
        .brief
        .as_deref()
        .map(|path| parse_file(path, "review brief", ReviewBrief::parse))
        .transpose()?;
    let attachments = request
        .evidence
        .iter()
        .map(|path| parse_file(path, "evidence attachment", parse_attachment))
        .collect::<Result<Vec<_>, _>>()?;
    let attachment_refs = attachments
        .iter()
        .map(|attachment| attachment as &dyn EvidenceAttachmentAdapter)
        .collect::<Vec<_>>();
    let input = InputVerifier
        .verify(InputVerificationRequest {
            report_path: &request.report,
            repo_root: &request.repo_root,
            operator_brief: brief.as_ref(),
            case_template_bundle: Some(&templates),
            prompt_bundle: Some(&prompts),
            route_configuration: Some(&routes),
            attachment_adapters: &attachment_refs,
            required_attachment_kinds: &[],
            expected_binding: expected,
        })
        .map_err(|error| failed(error.to_string()))?;
    Ok(LoadedPlanInputs {
        input,
        routes,
        prompts,
        source_request: request.clone(),
    })
}

fn parse_attachment(text: &str) -> Result<FileEvidenceAttachment, String> {
    let attachment: FileEvidenceAttachment = serde_json::from_str(text)
        .map_err(|error| format!("invalid evidence attachment: {error}"))?;
    crate::EvidenceAttachmentAdapter::validate_attachment(&attachment)?;
    Ok(attachment)
}

fn parse_file<T, E>(
    path: &Path,
    label: &str,
    parser: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, ReviewServiceError>
where
    E: std::fmt::Display,
{
    let text = std::fs::read_to_string(path).map_err(|error| {
        failed(format!(
            "failed to read {label} {}: {error}",
            path.display()
        ))
    })?;
    parser(&text).map_err(|error| failed(error.to_string()))
}
