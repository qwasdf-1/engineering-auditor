//! Retrieval tools: what the reviewer reads beyond the orientation pack.
//! Each call is checked against the map, answered in Markdown that cites
//! host-issued evidence IDs, counted against the review's budget, and
//! logged.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::document::BoundedDocument;
use super::view::RepositoryView;
use crate::{NativeConversationMessage, NativeToolCall, ReviewBrief, ToolSpec};

pub const RETRIEVAL_TOOL_NAMES: [&str; 6] = [
    "read_source",
    "symbol_info",
    "search_symbols",
    "findings_in",
    "module_overview",
    "history",
];

const DEFAULT_CALLS: usize = 32;
const DEFAULT_RESULT_BYTES: usize = 16 * 1024;
const DEFAULT_TOTAL_BYTES: usize = 256 * 1024;
/// Below this, a result has no room for more than its heading.
const MINIMUM_RESULT_BYTES: usize = 512;

/// What one review may retrieve. A brief can tighten it, never widen it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalBudget {
    pub max_calls: usize,
    pub max_result_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        Self {
            max_calls: DEFAULT_CALLS,
            max_result_bytes: DEFAULT_RESULT_BYTES,
            max_total_bytes: DEFAULT_TOTAL_BYTES,
        }
    }
}

impl RetrievalBudget {
    /// The brief's lookup-request and context-byte limits, where smaller.
    pub fn tightened_by(mut self, brief: Option<&ReviewBrief>) -> Self {
        if let Some(budgets) = brief.map(|brief| brief.budgets) {
            if let Some(calls) = budgets.max_lookup_requests {
                self.max_calls = self.max_calls.min(calls);
            }
            if let Some(bytes) = budgets.max_context_bytes {
                self.max_total_bytes = self.max_total_bytes.min(bytes);
                self.max_result_bytes = self.max_result_bytes.min(bytes);
            }
        }
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "reason")]
pub enum RetrievalOutcome {
    Answered,
    Rejected(String),
}

/// One logged call: what was asked, what came back, and what it cost.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRecord {
    pub call_id: String,
    pub tool: String,
    pub arguments: Value,
    #[serde(flatten)]
    pub outcome: RetrievalOutcome,
    pub result_bytes: usize,
    pub evidence_ids: Vec<String>,
}

/// The tools' definitions, as provider adapters declare them; `history`
/// only when the review has a history digest.
pub fn retrieval_tool_specs(with_history: bool) -> Vec<ToolSpec> {
    let string = |description: &str| json!({"type": "string", "description": description});
    let object = |properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
            "required": required,
        })
    };
    vec![
        ToolSpec {
            name: "read_source",
            description: "Read a mapped repository file, or lines of it. Returns the text with a source-span evidence ID. Large files need a line range.",
            parameters: object(
                json!({
                    "path": string("Repository-relative path of a mapped file."),
                    "start_line": {"type": "integer", "minimum": 1, "description": "First line, 1-based."},
                    "end_line": {"type": "integer", "minimum": 1, "description": "Last line, inclusive."},
                }),
                &["path"],
            ),
            strict: false,
        },
        ToolSpec {
            name: "symbol_info",
            description: "Describe a symbol: kind, location, metrics, parent and members, base and subtypes, callers and callees from the call graph, entry points, and findings inside it.",
            parameters: object(
                json!({"symbol": string("A symbol: evidence ID, or name (`Type.method` narrows).")}),
                &["symbol"],
            ),
            strict: true,
        },
        ToolSpec {
            name: "search_symbols",
            description: "Find functions, methods, and types whose name contains the query (case-insensitive); exact and prefix matches first.",
            parameters: object(
                json!({
                    "query": string("Part of a symbol name."),
                    "kind": string("Optional symbol kind, such as function, method, class, struct, or field."),
                }),
                &["query"],
            ),
            strict: false,
        },
        ToolSpec {
            name: "findings_in",
            description: "List the findings in a file, module, or symbol, most severe first.",
            parameters: object(
                json!({"scope": string("A file path or ID, module name or ID, or symbol ID.")}),
                &["scope"],
            ),
            strict: true,
        },
        ToolSpec {
            name: "module_overview",
            description: "Expand one module of the orientation pack: its files, the modules it imports and is imported by, cycles, findings by detector, and entry points.",
            parameters: object(
                json!({"module": string("A module name or ID from the orientation pack.")}),
                &["module"],
            ),
            strict: true,
        },
        ToolSpec {
            name: "history",
            description: "Change history of a mapped file over the history digest's window: commits in all and recently, authors, lines added and removed, and the files most often changed in the same commits, with whether an import links them.",
            parameters: object(
                json!({"path": string("Repository-relative path or file ID of a mapped file.")}),
                &["path"],
            ),
            strict: true,
        },
    ]
    .into_iter()
    .filter(|spec| with_history || spec.name != "history")
    .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadSource {
    pub(super) path: String,
    pub(super) start_line: Option<usize>,
    pub(super) end_line: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SymbolQuery {
    pub(super) symbol: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchQuery {
    pub(super) query: String,
    pub(super) kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScopeQuery {
    pub(super) scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModuleQuery {
    pub(super) module: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HistoryQuery {
    pub(super) path: String,
}

/// Answers the reviewer's tool calls over one repository view.
pub struct RetrievalHost<'v, 'a> {
    pub(super) view: &'v RepositoryView<'a>,
    pub(super) root: PathBuf,
    budget: RetrievalBudget,
    calls: usize,
    bytes: usize,
    issued: BTreeSet<String>,
    log: Vec<RetrievalRecord>,
}

impl<'v, 'a> RetrievalHost<'v, 'a> {
    /// `repo_root` is the tree the map was verified against.
    pub fn new(
        view: &'v RepositoryView<'a>,
        repo_root: &Path,
        budget: RetrievalBudget,
    ) -> Result<Self, String> {
        if budget.max_calls == 0
            || budget.max_result_bytes < MINIMUM_RESULT_BYTES
            || budget.max_total_bytes < budget.max_result_bytes
        {
            return Err("the retrieval budget is empty or inconsistent".to_string());
        }
        Ok(Self {
            view,
            root: repo_root
                .canonicalize()
                .map_err(|error| error.to_string())?,
            budget,
            calls: 0,
            bytes: 0,
            issued: BTreeSet::new(),
            log: Vec::new(),
        })
    }

    /// Answers one call as a tool-result message. A rejected call is
    /// answered too, with the reason, so the reviewer can correct it.
    pub fn answer(&mut self, call: &NativeToolCall) -> NativeConversationMessage {
        let (markdown, outcome, evidence_ids) = match self.execute(call) {
            Ok((markdown, ids)) => (markdown, RetrievalOutcome::Answered, ids),
            Err(reason) => (
                format!(
                    "# Tool error\n\n- **Tool:** {}\n- **Reason:** {reason}\n",
                    call.name
                ),
                RetrievalOutcome::Rejected(reason),
                BTreeSet::new(),
            ),
        };
        self.bytes += markdown.len();
        self.issued.extend(evidence_ids.iter().cloned());
        self.log.push(RetrievalRecord {
            call_id: call.call_id.clone(),
            tool: call.name.clone(),
            arguments: call.arguments.clone(),
            outcome,
            result_bytes: markdown.len(),
            evidence_ids: evidence_ids.into_iter().collect(),
        });
        NativeConversationMessage::ToolResult {
            call_id: call.call_id.clone(),
            tool_name: call.name.clone(),
            content_markdown: markdown,
        }
    }

    /// The tools this review offers.
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        retrieval_tool_specs(self.view.history.is_some())
    }

    /// Every evidence ID an answered call returned.
    pub fn issued_evidence_ids(&self) -> &BTreeSet<String> {
        &self.issued
    }

    pub fn log(&self) -> &[RetrievalRecord] {
        &self.log
    }

    fn execute(&mut self, call: &NativeToolCall) -> Result<(String, BTreeSet<String>), String> {
        if self.calls >= self.budget.max_calls {
            return Err(format!(
                "the review's {} retrieval calls are spent; answer from the evidence already returned",
                self.budget.max_calls
            ));
        }
        self.calls += 1;
        let remaining = self.budget.max_total_bytes.saturating_sub(self.bytes);
        let limit = self.budget.max_result_bytes.min(remaining);
        if limit < MINIMUM_RESULT_BYTES {
            return Err(
                "the review's retrieval bytes are spent; answer from the evidence already returned"
                    .to_string(),
            );
        }
        if let Some(error) = &call.validation_error {
            return Err(format!("invalid arguments: {error}"));
        }
        let document = match call.name.as_str() {
            "read_source" => self.read_source(&arguments(call)?, limit)?,
            "symbol_info" => self.symbol_info(&arguments::<SymbolQuery>(call)?.symbol)?,
            "search_symbols" => self.search_symbols(&arguments(call)?)?,
            "findings_in" => self.findings_in(&arguments::<ScopeQuery>(call)?.scope)?,
            "module_overview" => self.module_overview(&arguments::<ModuleQuery>(call)?.module)?,
            "history" => self.history(&arguments::<HistoryQuery>(call)?.path)?,
            other => return Err(format!("unknown tool `{other}`")),
        };
        let rendered = render(document, limit, &self.view.source_digest)?;
        Ok((rendered.0, rendered.1))
    }
}

fn arguments<T: DeserializeOwned>(call: &NativeToolCall) -> Result<T, String> {
    serde_json::from_value(call.arguments.clone())
        .map_err(|error| format!("invalid arguments: {error}"))
}

fn render(
    document: BoundedDocument,
    limit: usize,
    source_digest: &str,
) -> Result<(String, BTreeSet<String>), String> {
    document
        .render(limit, source_digest)
        .map(|rendered| (rendered.markdown, rendered.evidence_ids))
        .map_err(|error| {
            format!("the result does not fit the {limit}-byte limit ({error}); narrow the request, such as to a line range")
        })
}
