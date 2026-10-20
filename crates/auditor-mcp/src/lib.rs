use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use auditor_core::{
    render_typed_markdown, sha256_digest_bytes, MarkdownDocument, MarkdownField, MarkdownSection,
    MarkdownValue,
};
use auditor_detectors::{
    detector_registry, AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_report::{ColorMode, ExternalInput, ExternalInputMetadata, NoReviewArtifactLoader};
use auditor_review::{
    inspect_engineering_case, parse_disposition_ledger, parse_engineering_review_v2,
    plan_engineering_review, run_case_engineering_review, validate_disposition_coverage,
    verify_case_engineering_review, HostedConsent, HttpTransport, ReviewControl, ReviewPlanOutput,
    ReviewPlanRequest, ReviewRunRequest, ReviewStage, ReviewVerifyRequest, StableId, Transport,
};
use auditor_scan::{run_scan as execute_scan, ScanFormat, ScanRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_JOB_RECORDS: usize = 32;

pub fn serve_stdio() {
    let server = McpServer::default();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let response = server.handle(&line);
        if !response.is_empty() {
            let _ = writeln!(output, "{response}");
            let _ = output.flush();
        }
    }
}

pub struct McpServer {
    jobs: JobStore,
    transport: Arc<dyn Transport>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self {
            jobs: JobStore::default(),
            transport: Arc::new(HttpTransport::default()),
        }
    }
}

impl McpServer {
    pub fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self {
            jobs: JobStore::default(),
            transport,
        }
    }

    pub fn handle(&self, input: &str) -> String {
        let request = match parse_rpc_request(input) {
            Ok(request) => request,
            Err((code, message)) => return rpc_error(Value::Null, code, message),
        };
        let id = request.id.clone().unwrap_or(Value::Null);
        if request.jsonrpc != "2.0" {
            return rpc_error(id, -32600, "request requires jsonrpc 2.0".to_string());
        }
        render_rpc(id, self.dispatch_request(request))
    }

    fn dispatch_request(&self, request: RpcRequest) -> Result<Value, (i64, String)> {
        match request.method.as_str() {
            "initialize" => Ok(initialize_result()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_definitions()})),
            "tools/call" => self.call_tool(request.params),
            _ => Err((-32601, format!("unknown method: {}", request.method))),
        }
    }

    fn call_tool(&self, params: Option<Value>) -> Result<Value, (i64, String)> {
        let call: ToolCall = serde_json::from_value(params.unwrap_or(Value::Null))
            .map_err(|error| (-32602, format!("invalid tool call: {error}")))?;
        let result = self.dispatch_tool(&call.name, call.arguments)?;
        let readable = render_tool_result(&call.name, &result).map_err(tool_error)?;
        Ok(json!({
            "content": [{"type": "text", "text": readable}],
            "isError": false,
            "structuredContent": result,
        }))
    }

    fn dispatch_tool(&self, name: &str, arguments: Value) -> Result<Value, (i64, String)> {
        match name {
            "audit_scan_start" => self.start_scan_tool(arguments),
            "audit_catalog" => parse_and_run(arguments, catalog),
            "audit_review_validate" => parse_and_run(arguments, validate_review_artifacts),
            "audit_review_plan" => parse_and_run(arguments, create_review_plan),
            "audit_review_case" => parse_and_run(arguments, inspect_review_case),
            _ => self.dispatch_job_tool(name, arguments),
        }
    }

    fn dispatch_job_tool(&self, name: &str, arguments: Value) -> Result<Value, (i64, String)> {
        match name {
            "audit_job_get" => self.job_get_tool(arguments),
            "audit_review_start" => self.start_review_tool(arguments),
            "audit_review_status" => self.review_status_tool(arguments),
            "audit_review_result" => {
                let args: ReviewResultArgs = parse_arguments(arguments)?;
                self.jobs.review_result(&args).map_err(tool_error)
            }
            "audit_review_cancel" => self.review_cancel_tool(arguments),
            _ => Err((-32602, format!("unknown tool: {name}"))),
        }
    }

    fn start_scan_tool(&self, arguments: Value) -> Result<Value, (i64, String)> {
        let args: ScanJobArgs = parse_arguments(arguments)?;
        self.jobs
            .start_scan(args)
            .map(queued_job)
            .map_err(tool_error)
    }

    fn job_get_tool(&self, arguments: Value) -> Result<Value, (i64, String)> {
        let args: JobGetArgs = parse_arguments(arguments)?;
        serialized_tool_result(self.jobs.get(&args.job_id).map_err(tool_error)?)
    }

    fn start_review_tool(&self, arguments: Value) -> Result<Value, (i64, String)> {
        let args: ReviewJobArgs = parse_arguments(arguments)?;
        self.jobs
            .start_review(args, Arc::clone(&self.transport))
            .map(queued_job)
            .map_err(review_start_error)
    }

    fn review_status_tool(&self, arguments: Value) -> Result<Value, (i64, String)> {
        let args: JobGetArgs = parse_arguments(arguments)?;
        serialized_tool_result(self.jobs.review_status(&args.job_id).map_err(tool_error)?)
    }

    fn review_cancel_tool(&self, arguments: Value) -> Result<Value, (i64, String)> {
        let args: JobGetArgs = parse_arguments(arguments)?;
        serialized_tool_result(self.jobs.cancel_review(&args.job_id).map_err(tool_error)?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

fn initialize_result() -> Value {
    json!({
        "capabilities": {"tools": {"listChanged": false}},
        "protocolVersion": "2025-06-18",
        "serverInfo": {"name": "engineering-auditor", "version": env!("CARGO_PKG_VERSION")}
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_definition(
            "audit_scan_start",
            "Start a bounded asynchronous engineering audit and write its report artifact.",
            json!({
                    "include_references": {"type": "boolean"},
                    "method": {"enum": ["auto", "ast", "token"]},
                    "output": {"type": "string"},
                    "profile": {"enum": ["gating", "full"]},
                    "repo_root": {"type": "string"}
            }),
            &["repo_root", "output"],
        ),
        job_tool_definition("audit_job_get", "Read the current state of an audit job."),
        tool_definition(
            "audit_catalog",
            "List detector contracts for a scan profile.",
            json!({"profile": {"enum": ["gating", "full"]}}),
            &[],
        ),
        tool_definition(
            "audit_review_validate",
            "Validate a reviewed report, its digest-addressed review artifact, and optional disposition coverage.",
            string_properties(&["dispositions", "repo_root", "report"]),
            &["report", "repo_root"],
        ),
        tool_definition(
            "audit_review_plan",
            "Build a deterministic provider-free engineering review plan.",
            review_job_properties(),
            review_job_required(),
        ),
        tool_definition(
            "audit_review_case",
            "Inspect one bound case from a deterministic engineering review plan.",
            review_case_properties(),
            &["plan", "case_id"],
        ),
        tool_definition(
            "audit_review_start",
            "Start one bounded asynchronous engineering review using the canonical review service.",
            review_start_properties(),
            review_job_required(),
        ),
        job_tool_definition(
            "audit_review_status",
            "Read queued, running-stage, complete, failed, or cancelled review state.",
        ),
        tool_definition(
            "audit_review_result",
            "Read completed review artifact metadata and optionally the canonical reviewed report.",
            json!({
                    "include_report": {"type": "boolean"},
                    "job_id": {"type": "string"}
            }),
            &["job_id"],
        ),
        job_tool_definition(
            "audit_review_cancel",
            "Cancel a queued review or prevent a running review from entering later stages.",
        ),
    ]
}

fn review_job_properties() -> Value {
    json!({
        "brief": {"type": "string"},
        "case_templates": {"type": "string"},
        "evidence": {"items": {"type": "string"}, "type": "array"},
        "output": {"type": "string"},
        "prompt_bundle": {"type": "string"},
        "repo_root": {"type": "string"},
        "report": {"type": "string"},
        "routes": {"type": "string"}
    })
}

fn string_properties(names: &[&str]) -> Value {
    Value::Object(
        names
            .iter()
            .map(|name| ((*name).to_string(), json!({"type": "string"})))
            .collect(),
    )
}

fn review_case_properties() -> Value {
    let mut properties = string_properties(&["case_id", "plan"]);
    properties["case_id"]["pattern"] = json!("^engineering-case:[0-9a-f]{64}$");
    properties
}

fn review_start_properties() -> Value {
    let mut properties = review_job_properties();
    properties["allow_anonymous_provider"] = json!({"type": "boolean"});
    properties["allow_hosted_review"] = json!({"type": "boolean"});
    properties
}

fn review_job_required() -> &'static [&'static str] {
    &[
        "report",
        "repo_root",
        "case_templates",
        "routes",
        "prompt_bundle",
        "output",
    ]
}

fn tool_definition(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut input_schema = json!({
        "additionalProperties": false,
        "properties": properties,
        "type": "object",
    });
    if !required.is_empty() {
        input_schema["required"] = json!(required);
    }
    // Every MCP tool is experimental (docs/reference/stability.md).
    json!({
        "description": format!("Experimental: {description}"),
        "inputSchema": input_schema,
        "name": name,
    })
}

fn job_tool_definition(name: &str, description: &str) -> Value {
    tool_definition(
        name,
        description,
        json!({"job_id": {"type": "string"}}),
        &["job_id"],
    )
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanJobArgs {
    repo_root: PathBuf,
    output: PathBuf,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    include_references: bool,
}

fn default_profile() -> String {
    "gating".to_string()
}

fn default_method() -> String {
    "auto".to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobGetArgs {
    job_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogArgs {
    #[serde(default = "default_profile")]
    profile: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewValidateArgs {
    report: PathBuf,
    repo_root: PathBuf,
    dispositions: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewPlanArgs {
    report: PathBuf,
    repo_root: PathBuf,
    case_templates: PathBuf,
    routes: PathBuf,
    prompt_bundle: PathBuf,
    brief: Option<PathBuf>,
    #[serde(default)]
    evidence: Vec<PathBuf>,
    output: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewCaseArgs {
    plan: PathBuf,
    case_id: StableId,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewJobArgs {
    report: PathBuf,
    repo_root: PathBuf,
    output: PathBuf,
    case_templates: PathBuf,
    routes: PathBuf,
    prompt_bundle: PathBuf,
    brief: Option<PathBuf>,
    #[serde(default)]
    evidence: Vec<PathBuf>,
    #[serde(default)]
    allow_hosted_review: bool,
    #[serde(default)]
    allow_anonymous_provider: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewResultArgs {
    job_id: String,
    #[serde(default)]
    include_report: bool,
}

#[derive(Clone, Debug, Serialize)]
struct JobRecord {
    job_id: String,
    kind: JobKind,
    status: JobStatus,
    stage: Option<ReviewStage>,
    result: Option<Value>,
    error: Option<JobFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobStatus {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobKind {
    Scan,
    Review,
}

#[derive(Clone, Debug, Serialize)]
struct JobFailure {
    class: String,
    message: String,
}

#[derive(Default)]
struct JobState {
    next_id: u64,
    jobs: BTreeMap<String, JobRecord>,
    cancellations: BTreeMap<String, Arc<AtomicBool>>,
}

#[derive(Clone, Default)]
struct JobStore(Arc<Mutex<JobState>>);

impl JobStore {
    fn start_scan(&self, args: ScanJobArgs) -> Result<String, String> {
        validate_scan_args(&args)?;
        let job_id = {
            let mut state = self.0.lock().map_err(|_| "job store lock failed")?;
            reserve_job(&mut state, JobKind::Scan, "audit-job")?
        };
        let jobs = self.clone();
        let spawned_id = job_id.clone();
        std::thread::Builder::new()
            .name(spawned_id.clone())
            .spawn(move || {
                jobs.update(&spawned_id, JobStatus::Running, None, None, None);
                match run_scan_job(args) {
                    Ok(result) => {
                        jobs.update(&spawned_id, JobStatus::Complete, None, Some(result), None)
                    }
                    Err(error) => jobs.update(
                        &spawned_id,
                        JobStatus::Failed,
                        None,
                        None,
                        Some(job_failure("scan", error)),
                    ),
                }
            })
            .map_err(|error| format!("failed to start audit job: {error}"))?;
        Ok(job_id)
    }

    fn start_review(
        &self,
        args: ReviewJobArgs,
        transport: Arc<dyn Transport>,
    ) -> Result<String, String> {
        validate_review_args(&args)?;
        let cancellation = Arc::new(AtomicBool::new(false));
        let job_id = {
            let mut state = self.0.lock().map_err(|_| "job store lock failed")?;
            prune_jobs(&mut state.jobs);
            if state.jobs.values().any(|record| {
                record.kind == JobKind::Review
                    && matches!(record.status, JobStatus::Queued | JobStatus::Running)
            }) {
                return Err("review worker is busy with another active review".to_string());
            }
            let job_id = reserve_job(&mut state, JobKind::Review, "review-job")?;
            state
                .cancellations
                .insert(job_id.clone(), Arc::clone(&cancellation));
            job_id
        };
        let jobs = self.clone();
        let spawned_id = job_id.clone();
        std::thread::Builder::new()
            .name(spawned_id.clone())
            .spawn(move || {
                let control = McpReviewControl {
                    jobs: jobs.clone(),
                    job_id: spawned_id.clone(),
                    cancellation,
                };
                if !control.checkpoint(ReviewStage::Preparing) {
                    jobs.finish_cancelled(&spawned_id);
                    return;
                }
                let request = ReviewRunRequest {
                    plan: ReviewPlanRequest {
                        report: args.report,
                        repo_root: args.repo_root,
                        case_templates: args.case_templates,
                        routes: args.routes,
                        prompt_bundle: args.prompt_bundle,
                        brief: args.brief,
                        evidence: args.evidence,
                    },
                    output: args.output,
                    consent: HostedConsent {
                        allow_hosted: args.allow_hosted_review,
                        allow_anonymous: args.allow_anonymous_provider,
                    },
                };
                match run_case_engineering_review(&request, transport.as_ref(), &control) {
                    Ok(result) => jobs.update(
                        &spawned_id,
                        JobStatus::Complete,
                        Some(ReviewStage::Publishing),
                        serde_json::to_value(result).ok(),
                        None,
                    ),
                    Err(error) if error.is_cancelled() => jobs.finish_cancelled(&spawned_id),
                    Err(error) => jobs.update(
                        &spawned_id,
                        JobStatus::Failed,
                        None,
                        None,
                        Some(job_failure("review", error.to_string())),
                    ),
                }
                jobs.release_cancellation(&spawned_id);
            })
            .map_err(|error| format!("failed to start review job: {error}"))?;
        Ok(job_id)
    }

    fn get(&self, job_id: &str) -> Result<JobRecord, String> {
        self.0
            .lock()
            .map_err(|_| "job store lock failed".to_string())?
            .jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| format!("unknown audit job: {job_id}"))
    }

    fn review_status(&self, job_id: &str) -> Result<JobRecord, String> {
        let mut record = self.get(job_id)?;
        if record.kind != JobKind::Review {
            return Err(format!("audit job is not a review: {job_id}"));
        }
        record.result = None;
        Ok(record)
    }

    fn review_result(&self, args: &ReviewResultArgs) -> Result<Value, String> {
        let record = self.get(&args.job_id)?;
        if record.kind != JobKind::Review {
            return Err(format!("audit job is not a review: {}", args.job_id));
        }
        if record.status != JobStatus::Complete {
            return Err(format!(
                "review result is unavailable while status is {:?}",
                record.status
            ));
        }
        let mut result = record
            .result
            .ok_or_else(|| "completed review has no result metadata".to_string())?;
        if args.include_report {
            let path = result["reviewed_report_path"]
                .as_str()
                .ok_or_else(|| "review result omitted its report path".to_string())?;
            let report = std::fs::read_to_string(path)
                .map_err(|error| format!("failed to read canonical reviewed report: {error}"))?;
            result["reviewed_report"] = serde_json::from_str(&report)
                .map_err(|error| format!("canonical reviewed report is invalid: {error}"))?;
        }
        Ok(result)
    }

    fn cancel_review(&self, job_id: &str) -> Result<JobRecord, String> {
        let mut state = self.0.lock().map_err(|_| "job store lock failed")?;
        let record = state
            .jobs
            .get(job_id)
            .ok_or_else(|| format!("unknown audit job: {job_id}"))?;
        if record.kind != JobKind::Review {
            return Err(format!("audit job is not a review: {job_id}"));
        }
        if let Some(cancellation) = state.cancellations.get(job_id) {
            cancellation.store(true, Ordering::Release);
        }
        let record = state.jobs.get_mut(job_id).expect("review record exists");
        if record.status == JobStatus::Queued {
            record.status = JobStatus::Cancelled;
            record.stage = None;
        }
        Ok(record.clone())
    }

    fn update(
        &self,
        job_id: &str,
        status: JobStatus,
        stage: Option<ReviewStage>,
        result: Option<Value>,
        error: Option<JobFailure>,
    ) {
        if let Ok(mut state) = self.0.lock() {
            if let Some(record) = state.jobs.get_mut(job_id) {
                record.status = status;
                record.stage = stage;
                record.result = result;
                record.error = error;
            }
        }
    }

    fn finish_cancelled(&self, job_id: &str) {
        self.update(job_id, JobStatus::Cancelled, None, None, None);
        self.release_cancellation(job_id);
    }

    fn release_cancellation(&self, job_id: &str) {
        if let Ok(mut state) = self.0.lock() {
            state.cancellations.remove(job_id);
        }
    }
}

struct McpReviewControl {
    jobs: JobStore,
    job_id: String,
    cancellation: Arc<AtomicBool>,
}

impl ReviewControl for McpReviewControl {
    fn checkpoint(&self, stage: ReviewStage) -> bool {
        if self.cancellation.load(Ordering::Acquire) {
            return false;
        }
        self.jobs
            .update(&self.job_id, JobStatus::Running, Some(stage), None, None);
        !self.cancellation.load(Ordering::Acquire)
    }
}

fn validate_scan_args(args: &ScanJobArgs) -> Result<(), String> {
    ScanProfile::parse(&args.profile)?;
    AnalysisMethod::parse(&args.method)?;
    if args.output.as_os_str().is_empty() {
        return Err("audit scan job requires an output path".to_string());
    }
    Ok(())
}

fn validate_review_args(args: &ReviewJobArgs) -> Result<(), String> {
    if args.report == args.output {
        return Err("review output must differ from its input report".to_string());
    }
    Ok(())
}

fn run_scan_job(args: ScanJobArgs) -> Result<Value, String> {
    let profile = ScanProfile::parse(&args.profile)?;
    let method = AnalysisMethod::parse(&args.method)?;
    let map_output = (profile == ScanProfile::Full && method != AnalysisMethod::Token)
        .then(|| args.output.clone());
    let report_path = args.output.clone();
    let result = execute_scan(
        ScanRequest {
            repo_root: args.repo_root,
            analysis: AnalysisRequest {
                discovery: SourceDiscoveryOptions {
                    include_references: args.include_references,
                    method,
                },
                profile,
                scope: ScanScope::All,
                repository_map_output: map_output,
                excluded_paths: std::iter::once(args.output.clone()).collect(),
            },
            external_inputs: vec![ExternalInput::from_text(
                ExternalInputMetadata::analysis("scan_request", "mcp", 1, "complete"),
                &format!(
                    "profile={};method={};scope=full",
                    profile.as_str(),
                    method.as_str()
                ),
            )],
            baseline: None,
            format: ScanFormat::Json,
            output: Some(args.output),
            color: ColorMode::Plain,
            summary_only: false,
            review_loader: Box::new(NoReviewArtifactLoader),
        },
        &NoGraphProvider,
    )
    .map_err(|error| format!("audit job scan failed: {error}"))?;
    Ok(json!({
        "finding_count": result.report.findings.len(),
        "report_digest": sha256_digest_bytes(result.rendered.as_bytes()),
        "report_path": report_path,
        "repository_map_status": result.report.repository_map.status,
        "schema_version": result.report.schema_version,
    }))
}

fn catalog(args: CatalogArgs) -> Result<Value, String> {
    let profile = ScanProfile::parse(&args.profile)?;
    Ok(json!({
        "detectors": detector_registry(profile).into_iter().map(|detector| json!({
            "category": detector.category,
            "id": detector.id,
            "layer": detector.layer,
            "profile": detector.profile,
        })).collect::<Vec<_>>(),
        "profile": profile.as_str(),
    }))
}

fn validate_review_artifacts(args: ReviewValidateArgs) -> Result<Value, String> {
    let verified = verify_case_engineering_review(&ReviewVerifyRequest {
        repo_root: args.repo_root,
        report: args.report.clone(),
    })
    .map_err(|error| error.to_string())?;
    let report_json = std::fs::read_to_string(&args.report)
        .map_err(|error| format!("failed to read reviewed report: {error}"))?;
    let report: Value = serde_json::from_str(&report_json)
        .map_err(|error| format!("invalid reviewed report: {error}"))?;
    let review = &report["engineering_review"];
    let relative = review["path"]
        .as_str()
        .ok_or_else(|| "reviewed report has no review artifact path".to_string())?;
    let artifact_path = args
        .report
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(safe_sibling(relative)?);
    let artifact_json = std::fs::read_to_string(&artifact_path)
        .map_err(|error| format!("failed to read review artifact: {error}"))?;
    let artifact =
        parse_engineering_review_v2(&artifact_json).map_err(|error| error.to_string())?;
    let disposition_complete = if let Some(path) = args.dispositions {
        let ledger = parse_disposition_ledger(
            &std::fs::read_to_string(path)
                .map_err(|error| format!("failed to read disposition ledger: {error}"))?,
        )?;
        validate_disposition_coverage(&artifact, &ledger)?;
        true
    } else {
        false
    };
    Ok(json!({
        "accepted_recommendation_count": verified.accepted_recommendation_count,
        "blocked_case_count": verified.blocked_case_count,
        "case_count": verified.case_count,
        "challenge_response_count": verified.challenge_response_count,
        "contested_case_count": verified.contested_case_count,
        "disposition_complete": disposition_complete,
        "review_id": verified.review_id,
        "valid": verified.valid,
    }))
}

fn create_review_plan(args: ReviewPlanArgs) -> Result<Value, String> {
    if args.output == args.report {
        return Err("review plan output must differ from its input report".to_string());
    }
    let plan = plan_engineering_review(&ReviewPlanRequest {
        report: args.report,
        repo_root: args.repo_root,
        case_templates: args.case_templates,
        routes: args.routes,
        prompt_bundle: args.prompt_bundle,
        brief: args.brief,
        evidence: args.evidence,
    })
    .map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(&plan)
        .map_err(|error| format!("failed to serialize review plan: {error}"))?;
    auditor_scan::atomic_write(&args.output, &text)?;
    serde_json::to_value(plan).map_err(|error| error.to_string())
}

fn inspect_review_case(args: ReviewCaseArgs) -> Result<Value, String> {
    let text = std::fs::read_to_string(&args.plan)
        .map_err(|error| format!("failed to read review plan: {error}"))?;
    let plan = ReviewPlanOutput::parse(&text).map_err(|error| error.to_string())?;
    let inspection =
        inspect_engineering_case(&plan, &args.case_id).map_err(|error| error.to_string())?;
    serde_json::to_value(inspection).map_err(|error| error.to_string())
}

fn prune_jobs(jobs: &mut BTreeMap<String, JobRecord>) {
    while jobs.len() >= MAX_JOB_RECORDS {
        let removable = jobs
            .iter()
            .find(|(_, record)| {
                matches!(
                    record.status,
                    JobStatus::Complete | JobStatus::Failed | JobStatus::Cancelled
                )
            })
            .map(|(id, _)| id.clone());
        let Some(removable) = removable else {
            break;
        };
        jobs.remove(&removable);
    }
}

fn reserve_job(state: &mut JobState, kind: JobKind, prefix: &str) -> Result<String, String> {
    prune_jobs(&mut state.jobs);
    if state.jobs.len() >= MAX_JOB_RECORDS {
        return Err("audit job record limit reached; active jobs must finish".to_string());
    }
    state.next_id += 1;
    let job_id = format!("{prefix}-{:08}", state.next_id);
    state.jobs.insert(
        job_id.clone(),
        JobRecord {
            job_id: job_id.clone(),
            kind,
            status: JobStatus::Queued,
            stage: None,
            result: None,
            error: None,
        },
    );
    Ok(job_id)
}

fn safe_sibling(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("artifact path must be a safe report sibling".to_string());
    }
    Ok(path.to_path_buf())
}

fn render_tool_result(tool_name: &str, result: &Value) -> Result<String, String> {
    let fields = match result {
        Value::Object(values) => values
            .iter()
            .map(|(name, value)| MarkdownField::new(readable_label(name), markdown_value(value)))
            .collect(),
        value => vec![MarkdownField::new("Result", markdown_value(value))],
    };
    render_typed_markdown(&MarkdownDocument::new(
        format!("{} result", readable_label(tool_name)),
        vec![MarkdownSection::new("Output", fields)],
    ))
    .map_err(|error| format!("failed to render readable tool result: {error}"))
}

fn markdown_value(value: &Value) -> MarkdownValue {
    match value {
        Value::Null => MarkdownValue::text("Not provided"),
        Value::Bool(value) => MarkdownValue::text(if *value { "Yes" } else { "No" }),
        Value::Number(value) => MarkdownValue::text(value.to_string()),
        Value::String(value) => MarkdownValue::text(value),
        Value::Array(values) => MarkdownValue::Items(values.iter().map(markdown_value).collect()),
        Value::Object(values) => MarkdownValue::fields(
            values
                .iter()
                .map(|(name, value)| {
                    MarkdownField::new(readable_label(name), markdown_value(value))
                })
                .collect(),
        ),
    }
}

fn readable_label(value: &str) -> String {
    value.replace('_', " ")
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, (i64, String)> {
    serde_json::from_value(value).map_err(|error| (-32602, format!("invalid arguments: {error}")))
}

fn parse_and_run<T>(
    arguments: Value,
    operation: impl FnOnce(T) -> Result<Value, String>,
) -> Result<Value, (i64, String)>
where
    T: for<'de> Deserialize<'de>,
{
    operation(parse_arguments(arguments)?).map_err(tool_error)
}

fn parse_rpc_request(input: &str) -> Result<RpcRequest, (i64, String)> {
    let value =
        serde_json::from_str(input).map_err(|error| (-32700, format!("parse error: {error}")))?;
    serde_json::from_value(value)
        .map_err(|error| (-32600, format!("invalid JSON-RPC request: {error}")))
}

fn serialized_tool_result(value: impl Serialize) -> Result<Value, (i64, String)> {
    serde_json::to_value(value).map_err(|error| tool_error(error.to_string()))
}

fn queued_job(job_id: String) -> Value {
    json!({"job_id": job_id, "status": "queued"})
}

fn tool_error(message: String) -> (i64, String) {
    (-32000, message)
}

fn review_start_error(message: String) -> (i64, String) {
    if message.contains("worker is busy") {
        (-32001, message)
    } else {
        tool_error(message)
    }
}

fn job_failure(class: &str, message: String) -> JobFailure {
    let message = message
        .lines()
        .next()
        .unwrap_or("review job failed")
        .chars()
        .take(512)
        .collect();
    JobFailure {
        class: class.to_string(),
        message,
    }
}

fn rpc_result(id: Value, result: Value) -> String {
    json!({"id": id, "jsonrpc": "2.0", "result": result}).to_string()
}

fn render_rpc(id: Value, result: Result<Value, (i64, String)>) -> String {
    match result {
        Ok(result) => rpc_result(id, result),
        Err((code, message)) => rpc_error(id, code, message),
    }
}

fn rpc_error(id: Value, code: i64, message: String) -> String {
    json!({"error": {"code": code, "message": message}, "id": id, "jsonrpc": "2.0"}).to_string()
}

include!("lib_tests.rs");
