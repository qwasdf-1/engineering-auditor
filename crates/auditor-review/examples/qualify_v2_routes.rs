use std::fs;
use std::path::PathBuf;

use auditor_review::{
    case_tool_schema_digest, qualification_binding, qualify_case_target, CaseKind,
    CaseQualificationRuntime, HostedConsent, HttpTransport, ProbeKind, QualificationScope,
    ReviewPromptBundleV2, ReviewRole, ReviewRoutesV2, RouteTarget, TranscriptRecorder,
};
use serde_json::json;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut routes_path = None;
    let mut prompt_bundle_path = None;
    let mut output_path = None;
    let mut transcript_path = None;
    let mut include_challenger = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--routes" => routes_path = Some(required(&mut args, &argument)?),
            "--prompt-bundle" => prompt_bundle_path = Some(required(&mut args, &argument)?),
            "--output" => output_path = Some(required(&mut args, &argument)?),
            "--transcript" => transcript_path = Some(required(&mut args, &argument)?),
            "--include-challenger" => include_challenger = true,
            other => return Err(format!("unknown qualify_v2_routes argument `{other}`")),
        }
    }

    let routes_path = PathBuf::from(routes_path.ok_or_else(|| "--routes is required".to_string())?);
    let prompt_bundle_path =
        PathBuf::from(prompt_bundle_path.ok_or_else(|| "--prompt-bundle is required".to_string())?);
    let output_path = PathBuf::from(output_path.ok_or_else(|| "--output is required".to_string())?);
    let transcript_path =
        PathBuf::from(transcript_path.ok_or_else(|| "--transcript is required".to_string())?);

    let routes = ReviewRoutesV2::parse(
        &fs::read_to_string(&routes_path)
            .map_err(|error| format!("failed to read {}: {error}", routes_path.display()))?,
    )?;
    let prompts = ReviewPromptBundleV2::parse(
        &fs::read_to_string(&prompt_bundle_path)
            .map_err(|error| format!("failed to read {}: {error}", prompt_bundle_path.display()))?,
    )?;
    let transcript = TranscriptRecorder::enabled();
    let transport = HttpTransport::default();
    let tool_schema_digest = case_tool_schema_digest();
    let mut roles = vec![ReviewRole::SingleReviewer];
    if include_challenger {
        roles.push(ReviewRole::Challenger);
    }
    let mut qualifications = Vec::new();

    for provider in &routes.providers {
        for model in &provider.models {
            for role in &roles {
                let binding = qualification_binding(
                    RouteTarget {
                        provider_id: provider.id.clone(),
                        model: model.clone(),
                    },
                    QualificationScope::new(
                        *role,
                        CaseKind::LifecycleMismatch,
                        vec![
                            ProbeKind::SemanticEquivalence,
                            ProbeKind::Mutation,
                            ProbeKind::Cost,
                            ProbeKind::Cardinality,
                            ProbeKind::Lifecycle,
                            ProbeKind::ConsumerRequirement,
                            ProbeKind::Constraint,
                            ProbeKind::AlternativeFeasibility,
                        ],
                        prompts.bundle_id.clone(),
                        tool_schema_digest.clone(),
                    ),
                );
                qualifications.push(qualify_case_target(
                    provider,
                    binding,
                    CaseQualificationRuntime {
                        transport: &transport,
                        consent: HostedConsent {
                            allow_hosted: true,
                            allow_anonymous: true,
                        },
                        budget: routes.budget_for(CaseKind::LifecycleMismatch),
                        transcript: &transcript,
                        transcript_enabled: true,
                    },
                ));
            }
        }
    }

    let report = json!({
        "schema_version": 1,
        "routes": routes_path,
        "prompt_bundle": prompt_bundle_path,
        "prompt_bundle_id": prompts.bundle_id,
        "tool_schema_digest": tool_schema_digest,
        "qualifications": qualifications,
    });
    write_json(&output_path, &report)?;
    write_json(&transcript_path, &transcript.to_json(None))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn required(args: &mut impl Iterator<Item = String>, argument: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{argument} requires a value"))
}

fn write_json(path: &PathBuf, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("failed to write {}: {error}", path.display()))
}
