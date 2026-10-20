use std::path::PathBuf;

use auditor_review::{
    run_case_engineering_review, verify_case_engineering_review, HostedConsent, HttpTransport,
    ReviewPlanRequest, ReviewRunRequest, ReviewVerifyRequest, UncontrolledReview,
};
use serde_json::json;

fn main() {
    match run() {
        Ok(value) => println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("result is serializable")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<serde_json::Value, String> {
    let mut report = None;
    let mut repo_root = None;
    let mut output = None;
    let mut case_templates = None;
    let mut routes = None;
    let mut prompt_bundle = None;
    let mut brief = None;
    let mut evidence = Vec::new();
    let mut allow_hosted = false;
    let mut allow_anonymous = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--report" => report = Some(required(&mut args, &argument)?),
            "--repo-root" => repo_root = Some(required(&mut args, &argument)?),
            "--output" => output = Some(required(&mut args, &argument)?),
            "--case-templates" => case_templates = Some(required(&mut args, &argument)?),
            "--routes" => routes = Some(required(&mut args, &argument)?),
            "--prompt-bundle" => prompt_bundle = Some(required(&mut args, &argument)?),
            "--brief" => brief = Some(required(&mut args, &argument)?),
            "--evidence" => evidence.push(required(&mut args, &argument)?),
            "--allow-hosted-review" => allow_hosted = true,
            "--allow-anonymous-provider" => allow_anonymous = true,
            other => return Err(format!("unknown review_v2_case argument `{other}`")),
        }
    }
    let repo_root = PathBuf::from(repo_root.ok_or_else(|| "--repo-root is required".to_string())?);
    let output = PathBuf::from(output.ok_or_else(|| "--output is required".to_string())?);
    let request = ReviewRunRequest {
        plan: ReviewPlanRequest {
            report: PathBuf::from(report.ok_or_else(|| "--report is required".to_string())?),
            repo_root: repo_root.clone(),
            case_templates: PathBuf::from(
                case_templates.ok_or_else(|| "--case-templates is required".to_string())?,
            ),
            routes: PathBuf::from(routes.ok_or_else(|| "--routes is required".to_string())?),
            prompt_bundle: PathBuf::from(
                prompt_bundle.ok_or_else(|| "--prompt-bundle is required".to_string())?,
            ),
            brief: brief.map(PathBuf::from),
            evidence: evidence.into_iter().map(PathBuf::from).collect(),
        },
        output: output.clone(),
        consent: HostedConsent {
            allow_hosted,
            allow_anonymous,
        },
    };
    let run = run_case_engineering_review(&request, &HttpTransport::default(), &UncontrolledReview)
        .map_err(|error| error.to_string())?;
    let verify = verify_case_engineering_review(&ReviewVerifyRequest {
        repo_root,
        report: output,
    })
    .map_err(|error| error.to_string())?;
    Ok(json!({"run": run, "verify": verify}))
}

fn required(args: &mut impl Iterator<Item = String>, argument: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{argument} requires a value"))
}
