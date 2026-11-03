use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
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

#[derive(Default)]
pub struct McpServer {
    jobs: JobStore,
}

impl McpServer {
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
            "audit_job_get" => self.job_get_tool(arguments),
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

#[derive(Clone, Debug, Serialize)]
struct JobRecord {
    job_id: String,
    kind: JobKind,
    status: JobStatus,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum JobKind {
    Scan,
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
                jobs.update(&spawned_id, JobStatus::Running, None, None);
                match run_scan_job(args) {
                    Ok(result) => jobs.update(&spawned_id, JobStatus::Complete, Some(result), None),
                    Err(error) => jobs.update(
                        &spawned_id,
                        JobStatus::Failed,
                        None,
                        Some(job_failure("scan", error)),
                    ),
                }
            })
            .map_err(|error| format!("failed to start audit job: {error}"))?;
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

    fn update(
        &self,
        job_id: &str,
        status: JobStatus,
        result: Option<Value>,
        error: Option<JobFailure>,
    ) {
        if let Ok(mut state) = self.0.lock() {
            if let Some(record) = state.jobs.get_mut(job_id) {
                record.status = status;
                record.result = result;
                record.error = error;
            }
        }
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

fn prune_jobs(jobs: &mut BTreeMap<String, JobRecord>) {
    while jobs.len() >= MAX_JOB_RECORDS {
        let removable = jobs
            .iter()
            .find(|(_, record)| matches!(record.status, JobStatus::Complete | JobStatus::Failed))
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
            result: None,
            error: None,
        },
    );
    Ok(job_id)
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

fn job_failure(class: &str, message: String) -> JobFailure {
    let message = message
        .lines()
        .next()
        .unwrap_or("audit job failed")
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
