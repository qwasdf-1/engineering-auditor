use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use auditor_core::sha256_digest_bytes;
use auditor_report::validate_report_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::brief::{DocumentIntent, ReviewBrief};
use crate::repository_map::{safe_relative_path, RepositoryMap, RepositoryMapError, SourceExcerpt};
use crate::LookupBudget;

const DEFAULT_DOCUMENT_BYTES: usize = 32 * 1024;
const DEFAULT_DOCUMENT_TOTAL_BYTES: usize = 128 * 1024;
const DEFAULT_CONTEXT_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLabel {
    ProtocolInstruction,
    OperatorIntent,
    DeterministicEvidence,
    RepositorySource,
    UntrustedRepositoryDocumentation,
    UntrustedProviderOutput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CapabilityBrief {
    pub detector_id: String,
    pub layer: String,
    pub status: String,
    pub analyzed_files: usize,
    pub unsupported_files: usize,
    pub missing_inputs: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FindingBrief {
    pub id: String,
    pub detector: String,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub recommendation: String,
    pub path: String,
    pub line: Option<usize>,
    pub evidence: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DocumentationClaim {
    pub path: String,
    pub intent: DocumentIntent,
    pub digest: String,
    pub trust: TrustLabel,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueryableSymbol {
    pub id: String,
    pub name: String,
    pub file_id: String,
    pub file_path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvidenceGraphOutline {
    pub schema_version: u32,
    pub node_count: usize,
    pub edge_count: usize,
    pub complete: bool,
}

pub const MAX_QUERYABLE_SYMBOLS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvidenceBrief {
    pub schema_version: u32,
    pub report_digest: String,
    pub report_schema_version: u32,
    pub analysis_contract_digest: String,
    pub repository_map_digest: String,
    pub profile: String,
    pub scope: String,
    pub trust: TrustLabel,
    pub capabilities: Vec<CapabilityBrief>,
    pub unavailable_domains: Vec<String>,
    pub findings: Vec<FindingBrief>,
    pub evidence_graph: EvidenceGraphOutline,
    pub map_file_count: usize,
    pub map_symbol_count: usize,
    pub map_edge_count: usize,
    pub map_symbols: Vec<QueryableSymbol>,
    pub operator_brief: ReviewBrief,
    pub operator_brief_digest: String,
    pub documentation: Vec<DocumentationClaim>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBudget {
    pub max_document_bytes: usize,
    pub max_document_total_bytes: usize,
    pub max_context_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderContextBudgets {
    pub context: ContextBudget,
    pub lookup: LookupBudget,
}

impl ProviderContextBudgets {
    pub const fn new(context: ContextBudget, lookup: LookupBudget) -> Self {
        Self { context, lookup }
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_document_bytes: DEFAULT_DOCUMENT_BYTES,
            max_document_total_bytes: DEFAULT_DOCUMENT_TOTAL_BYTES,
            max_context_bytes: DEFAULT_CONTEXT_BYTES,
        }
    }
}

impl ContextBudget {
    pub fn tightened_by(self, brief: &ReviewBrief) -> Self {
        Self {
            max_document_bytes: brief
                .budgets
                .max_document_bytes
                .map_or(self.max_document_bytes, |value| {
                    value.min(self.max_document_bytes)
                }),
            max_document_total_bytes: brief
                .budgets
                .max_document_total_bytes
                .map_or(self.max_document_total_bytes, |value| {
                    value.min(self.max_document_total_bytes)
                }),
            max_context_bytes: brief
                .budgets
                .max_context_bytes
                .map_or(self.max_context_bytes, |value| {
                    value.min(self.max_context_bytes)
                }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EgressPolicy {
    pub allow_deterministic_evidence: bool,
    pub allow_source_excerpts: bool,
    pub allow_documentation: bool,
}

impl EgressPolicy {
    pub const fn hosted_review() -> Self {
        Self {
            allow_deterministic_evidence: true,
            allow_source_excerpts: true,
            allow_documentation: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBlockKind {
    Protocol,
    CoverageLedger,
    EvidenceGraphOutline,
    FindingPortfolio,
    RepositoryMapOutline,
    CaseEvidencePacket,
    OperatorBrief,
    RepositoryDocument,
    SourceExcerpt,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContextBlock {
    pub kind: ContextBlockKind,
    pub provenance: String,
    pub digest: String,
    pub trust: TrustLabel,
    pub byte_count: usize,
    pub content: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EgressManifestEntry {
    pub kind: ContextBlockKind,
    pub provenance: String,
    pub digest: String,
    pub trust: TrustLabel,
    pub byte_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EgressManifest {
    pub schema_version: u32,
    pub block_count: usize,
    pub byte_count: usize,
    pub blocks: Vec<EgressManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderContext {
    pub schema_version: u32,
    pub stage: String,
    pub blocks: Vec<ContextBlock>,
    pub egress_manifest: EgressManifest,
    pub byte_count: usize,
}

pub fn compile_evidence_brief(
    report_json: &str,
    map: &RepositoryMap,
    repo_root: &Path,
    review_brief: &ReviewBrief,
    budget: ContextBudget,
) -> Result<EvidenceBrief, RepositoryMapError> {
    review_brief
        .validate()
        .map_err(RepositoryMapError::Contract)?;
    validate_report_json(report_json).map_err(RepositoryMapError::Contract)?;
    let report: Value = serde_json::from_str(report_json)
        .map_err(|error| RepositoryMapError::Contract(error.to_string()))?;
    let capabilities = report["capability_executions"]
        .as_array()
        .ok_or_else(|| RepositoryMapError::Contract("capability ledger is absent".to_string()))?
        .iter()
        .map(capability_brief)
        .collect::<Result<Vec<_>, _>>()?;
    let unavailable_domains = ["L1", "L2", "L3", "L4", "L5"]
        .into_iter()
        .filter(|layer| {
            let layer_capabilities = capabilities
                .iter()
                .filter(|capability| capability.layer == *layer)
                .collect::<Vec<_>>();
            !layer_capabilities.is_empty()
                && layer_capabilities
                    .iter()
                    .all(|capability| capability.status != "executed")
        })
        .map(|layer| domain_for_layer(layer).to_string())
        .chain(review_brief.excluded_domains.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let findings = report["findings"]
        .as_array()
        .ok_or_else(|| RepositoryMapError::Contract("findings are absent".to_string()))?
        .iter()
        .map(finding_brief)
        .collect::<Result<Vec<_>, _>>()?;
    let documentation = load_documentation(
        repo_root,
        &review_brief.documents,
        budget.max_document_bytes,
        budget.max_document_total_bytes,
    )?;
    let graph = &report["evidence_graph"];
    let graph_nodes = graph["nodes"].as_array().ok_or_else(|| {
        RepositoryMapError::Contract("evidence graph nodes are absent".to_string())
    })?;
    let graph_edges = graph["edges"].as_array().ok_or_else(|| {
        RepositoryMapError::Contract("evidence graph edges are absent".to_string())
    })?;
    let operator_brief_json = serde_json::to_vec(review_brief)
        .map_err(|error| RepositoryMapError::Contract(error.to_string()))?;
    let report_digest = deterministic_report_digest(&report)?;
    let map_symbols = build_queryable_symbols(map, &findings);
    Ok(EvidenceBrief {
        schema_version: 1,
        report_digest,
        report_schema_version: required_u32(&report, "schema_version")?,
        analysis_contract_digest: required_string(&report, "analysis_contract_digest")?,
        repository_map_digest: sha256_digest_bytes(
            &serde_json::to_vec(map)
                .map_err(|error| RepositoryMapError::Contract(error.to_string()))?,
        ),
        profile: required_string(&report, "profile")?,
        scope: required_string(&report, "scope")?,
        trust: TrustLabel::DeterministicEvidence,
        capabilities,
        unavailable_domains,
        findings,
        evidence_graph: EvidenceGraphOutline {
            schema_version: required_u32(graph, "schema_version")?,
            node_count: graph_nodes.len(),
            edge_count: graph_edges.len(),
            complete: graph["completeness"]["complete"].as_bool().unwrap_or(false),
        },
        map_file_count: map.files.len(),
        map_symbol_count: map.symbols.len(),
        map_edge_count: map.edges.len(),
        map_symbols,
        operator_brief: review_brief.clone(),
        operator_brief_digest: sha256_digest_bytes(&operator_brief_json),
        documentation,
    })
}

fn domain_for_layer(layer: &str) -> &'static str {
    match layer {
        "L1" => "correctness",
        "L2" => "structure",
        "L3" => "algorithmic_cost",
        "L4" => "systems_economics",
        "L5" => "optimization",
        _ => "evidence_coverage",
    }
}

fn deterministic_report_digest(report: &Value) -> Result<String, RepositoryMapError> {
    let mut value = report.clone();
    value["engineering_review"] = json!({
        "assessment_count": 0,
        "completeness": "not_requested",
        "contested_count": 0,
        "digest": null,
        "input_report_digest": null,
        "material_recommendation_count": 0,
        "path": null,
        "review_id": null,
        "revision_count": 0,
        "schema_version": 1,
        "status": "absent"
    });
    serde_json::to_vec(&value)
        .map(|bytes| sha256_digest_bytes(&bytes))
        .map_err(|error| RepositoryMapError::Contract(error.to_string()))
}

fn build_queryable_symbols(map: &RepositoryMap, findings: &[FindingBrief]) -> Vec<QueryableSymbol> {
    let findings_paths = findings
        .iter()
        .map(|finding| finding.path.as_str())
        .collect::<BTreeSet<_>>();
    let file_lookup = map
        .files
        .iter()
        .map(|file| (file.id.as_str(), file.path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let project = map.symbols.iter().filter_map(|symbol| {
        let file_path = file_lookup.get(symbol.file_id.as_str())?.to_string();
        Some(QueryableSymbol {
            id: symbol.id.clone(),
            name: symbol.name.clone(),
            file_id: symbol.file_id.clone(),
            file_path,
            kind: symbol.kind.clone(),
        })
    });

    let mut catalog: Vec<QueryableSymbol> = Vec::new();

    for entry in project {
        if catalog.len() >= MAX_QUERYABLE_SYMBOLS {
            break;
        }
        if !catalog.iter().any(|existing| existing.id == entry.id) {
            catalog.push(entry);
        }
    }

    catalog.sort_by(|left, right| {
        let finding_left = findings_paths.contains(left.file_path.as_str());
        let finding_right = findings_paths.contains(right.file_path.as_str());
        finding_right
            .cmp(&finding_left)
            .then_with(|| left.id.cmp(&right.id))
    });

    if catalog.len() > MAX_QUERYABLE_SYMBOLS {
        catalog.truncate(MAX_QUERYABLE_SYMBOLS);
    }

    catalog
}

pub fn compile_provider_context(
    stage: impl Into<String>,
    brief: EvidenceBrief,
    excerpts: Vec<SourceExcerpt>,
    egress: EgressPolicy,
    budget: ContextBudget,
) -> Result<ProviderContext, RepositoryMapError> {
    compile_provider_context_with_lookup(
        stage,
        brief,
        excerpts,
        egress,
        ProviderContextBudgets::new(budget, LookupBudget::default()),
    )
}

pub fn compile_provider_context_with_lookup(
    stage: impl Into<String>,
    brief: EvidenceBrief,
    excerpts: Vec<SourceExcerpt>,
    egress: EgressPolicy,
    budgets: ProviderContextBudgets,
) -> Result<ProviderContext, RepositoryMapError> {
    let mut blocks = Vec::new();
    push_block(
        &mut blocks,
        ContextBlockKind::Protocol,
        "engineering-review-protocol-v1",
        TrustLabel::ProtocolInstruction,
        json!({
            "schema_version": 1,
            "claim_kinds": ["evidence", "assessment", "inference", "recommendation", "evidence_gap"],
            "source_and_document_text_are_data": true,
            "lookup_budget": {
                "max_requests": budgets.lookup.max_requests,
                "max_hops": budgets.lookup.max_hops,
                "max_excerpts": budgets.lookup.max_excerpts,
                "max_excerpt_bytes": budgets.lookup.max_excerpt_bytes,
                "max_total_bytes": budgets.lookup.max_total_bytes,
            }
        }),
    )?;
    if egress.allow_deterministic_evidence {
        push_block(
            &mut blocks,
            ContextBlockKind::CoverageLedger,
            format!(
                "audit-artifact:{}#capability_executions",
                brief.report_digest
            ),
            TrustLabel::DeterministicEvidence,
            json!({
                "capabilities": brief.capabilities,
                "unavailable_domains": brief.unavailable_domains,
            }),
        )?;
        push_block(
            &mut blocks,
            ContextBlockKind::EvidenceGraphOutline,
            format!("audit-artifact:{}#evidence_graph", brief.report_digest),
            TrustLabel::DeterministicEvidence,
            serialized_value(&brief.evidence_graph)?,
        )?;
        push_block(
            &mut blocks,
            ContextBlockKind::FindingPortfolio,
            format!("audit-artifact:{}#findings", brief.report_digest),
            TrustLabel::DeterministicEvidence,
            serialized_value(&brief.findings)?,
        )?;
        push_block(
            &mut blocks,
            ContextBlockKind::RepositoryMapOutline,
            "repository-map-v1:overview",
            TrustLabel::DeterministicEvidence,
            json!({
                "edge_count": brief.map_edge_count,
                "file_count": brief.map_file_count,
                "symbol_count": brief.map_symbol_count,
                "symbols": brief.map_symbols,
                "symbol_catalog_bound": MAX_QUERYABLE_SYMBOLS,
            }),
        )?;
        push_block(
            &mut blocks,
            ContextBlockKind::OperatorBrief,
            format!("review-brief:{}", brief.operator_brief_digest),
            TrustLabel::OperatorIntent,
            serialized_value(&brief.operator_brief)?,
        )?;
    }
    if egress.allow_documentation {
        for document in brief.documentation {
            push_block(
                &mut blocks,
                ContextBlockKind::RepositoryDocument,
                format!("repository-document:{}", document.path),
                TrustLabel::UntrustedRepositoryDocumentation,
                json!({
                    "content": document.content,
                    "intent": document.intent,
                    "path": document.path,
                }),
            )?;
        }
    }
    if egress.allow_source_excerpts {
        for excerpt in excerpts {
            push_block(
                &mut blocks,
                ContextBlockKind::SourceExcerpt,
                format!("repository-source:{}:{}", excerpt.path, excerpt.id),
                TrustLabel::RepositorySource,
                serialized_value(&excerpt)?,
            )?;
        }
    }
    let manifest_blocks = blocks
        .iter()
        .map(|block| EgressManifestEntry {
            kind: block.kind,
            provenance: block.provenance.clone(),
            digest: block.digest.clone(),
            trust: block.trust,
            byte_count: block.byte_count,
        })
        .collect::<Vec<_>>();
    let egress_manifest = EgressManifest {
        schema_version: 1,
        block_count: manifest_blocks.len(),
        byte_count: manifest_blocks.iter().map(|block| block.byte_count).sum(),
        blocks: manifest_blocks,
    };
    let mut context = ProviderContext {
        schema_version: 1,
        stage: stage.into(),
        blocks,
        egress_manifest,
        byte_count: 0,
    };
    context.byte_count = serialized_len(&context)?;
    if context.byte_count > budgets.context.max_context_bytes {
        return Err(RepositoryMapError::LookupBudgetExceeded(
            "compiled provider context exceeds the configured byte budget".to_string(),
        ));
    }
    Ok(context)
}

fn push_block(
    blocks: &mut Vec<ContextBlock>,
    kind: ContextBlockKind,
    provenance: impl Into<String>,
    trust: TrustLabel,
    content: Value,
) -> Result<(), RepositoryMapError> {
    let bytes = serde_json::to_vec(&content)
        .map_err(|error| RepositoryMapError::Contract(error.to_string()))?;
    blocks.push(ContextBlock {
        kind,
        provenance: provenance.into(),
        digest: sha256_digest_bytes(&bytes),
        trust,
        byte_count: bytes.len(),
        content,
    });
    Ok(())
}

fn serialized_len(value: &impl Serialize) -> Result<usize, RepositoryMapError> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| RepositoryMapError::Contract(error.to_string()))
}

fn serialized_value(value: &impl Serialize) -> Result<Value, RepositoryMapError> {
    serde_json::to_value(value).map_err(|error| RepositoryMapError::Contract(error.to_string()))
}

fn capability_brief(value: &Value) -> Result<CapabilityBrief, RepositoryMapError> {
    Ok(CapabilityBrief {
        detector_id: required_string(value, "detector_id")?,
        layer: required_string(value, "layer")?,
        status: required_string(value, "status")?,
        analyzed_files: required_usize(value, "analyzed_files")?,
        unsupported_files: required_usize(value, "unsupported_files")?,
        missing_inputs: string_array(value, "missing_inputs")?,
        limitations: string_array(value, "limitations")?,
    })
}

fn finding_brief(value: &Value) -> Result<FindingBrief, RepositoryMapError> {
    let location = value["locations"]
        .as_array()
        .and_then(|locations| locations.first())
        .ok_or_else(|| RepositoryMapError::Contract("finding location is absent".to_string()))?;
    let evidence = value["evidence"]
        .as_object()
        .ok_or_else(|| RepositoryMapError::Contract("finding evidence is absent".to_string()))?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(FindingBrief {
        id: required_string(value, "id")?,
        detector: required_string(value, "detector")?,
        category: required_string(value, "category")?,
        severity: required_string(value, "severity")?,
        title: required_string(value, "title")?,
        message: required_string(value, "message")?,
        recommendation: required_string(value, "recommendation")?,
        path: required_string(location, "path")?,
        line: location["line"]
            .as_u64()
            .and_then(|line| usize::try_from(line).ok()),
        evidence,
    })
}

fn load_documentation(
    root: &Path,
    allowlist: &[crate::ReviewDocument],
    per_document: usize,
    total: usize,
) -> Result<Vec<DocumentationClaim>, RepositoryMapError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
    let mut claims = Vec::new();
    let mut bytes = 0;
    for document in allowlist {
        let relative = &document.path;
        let path = safe_relative_path(relative, "documentation allowlist")?;
        let canonical = canonical_root
            .join(path)
            .canonicalize()
            .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(RepositoryMapError::Contract(format!(
                "documentation path escapes repository: {relative}"
            )));
        }
        let content = fs::read_to_string(canonical)
            .map_err(|error| RepositoryMapError::Io(error.to_string()))?;
        if content.len() > per_document || bytes + content.len() > total {
            return Err(RepositoryMapError::LookupBudgetExceeded(
                "documentation context exceeds the configured byte budget".to_string(),
            ));
        }
        bytes += content.len();
        claims.push(DocumentationClaim {
            path: relative.clone(),
            intent: document.intent,
            digest: sha256_digest_bytes(content.as_bytes()),
            trust: TrustLabel::UntrustedRepositoryDocumentation,
            content,
        });
    }
    claims.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(claims)
}

fn string_array(value: &Value, field: &str) -> Result<Vec<String>, RepositoryMapError> {
    value[field]
        .as_array()
        .ok_or_else(|| RepositoryMapError::Contract(format!("{field} is absent")))?
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                RepositoryMapError::Contract(format!("{field} must contain strings"))
            })
        })
        .collect()
}

fn required_string(value: &Value, field: &str) -> Result<String, RepositoryMapError> {
    value[field]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| RepositoryMapError::Contract(format!("brief input requires {field}")))
}

fn required_u32(value: &Value, field: &str) -> Result<u32, RepositoryMapError> {
    value[field]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| RepositoryMapError::Contract(format!("brief input requires {field}")))
}

fn required_usize(value: &Value, field: &str) -> Result<usize, RepositoryMapError> {
    value[field]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| RepositoryMapError::Contract(format!("brief input requires {field}")))
}
