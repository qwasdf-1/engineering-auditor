use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::baseline_match::{match_baseline, BaselineMatch, MatchSubject};
use crate::{
    parse_report_findings, parse_report_identity, validate_report_json, AuditReport,
    ReportIdentity, BASELINE_SCHEMA_VERSION, EVIDENCE_GRAPH_SCHEMA_VERSION, REPORT_SCHEMA_VERSION,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineEntry {
    pub id: String,
    pub detector: String,
    pub title: String,
    pub path: String,
    pub message: String,
    pub identity: BTreeMap<String, String>,
    pub reason: String,
}

impl BaselineEntry {
    pub fn match_subject(&self) -> MatchSubject<'_> {
        MatchSubject {
            id: &self.id,
            detector: &self.detector,
            title: &self.title,
            path: &self.path,
            message: &self.message,
            identity: &self.identity,
        }
    }

    /// Rebind this accepted entry to the current finding it was carried to,
    /// keeping the human review reason.
    pub fn rebind(&self, current: &crate::ParsedFinding) -> Self {
        Self {
            reason: self.reason.clone(),
            ..BaselineEntry::from_parsed(current, String::new())
        }
    }

    pub(crate) fn from_parsed(finding: &crate::ParsedFinding, reason: String) -> Self {
        Self {
            id: finding.id.clone(),
            detector: finding.detector.clone(),
            title: finding.title.clone(),
            path: finding.path.clone(),
            message: finding.message.clone(),
            identity: finding.identity.clone(),
            reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselineDocument {
    pub schema_version: u32,
    identity: ReportIdentity,
    pub entries: Vec<BaselineEntry>,
}

impl BaselineDocument {
    pub fn from_report(report: &AuditReport, reason: &str) -> Result<Self, String> {
        let reason = review_reason(reason)?;
        let entries = report
            .findings
            .iter()
            .map(|finding| BaselineEntry {
                id: finding.id.clone(),
                detector: finding.detector.clone(),
                title: finding.title.clone(),
                path: finding.primary_path().to_string(),
                message: finding.message.clone(),
                identity: finding.identity.clone(),
                reason: reason.clone(),
            })
            .collect();
        Ok(Self::new(report.identity(), entries))
    }

    pub fn from_report_json(report_json: &str, reason: &str) -> Result<Self, String> {
        let reason = review_reason(reason)?;
        validate_report_json(report_json)?;
        let entries = parse_report_findings(report_json)?
            .iter()
            .map(|finding| BaselineEntry::from_parsed(finding, reason.clone()))
            .collect();
        Ok(Self::new(parse_report_identity(report_json)?, entries))
    }

    pub(crate) fn new(identity: ReportIdentity, mut entries: Vec<BaselineEntry>) -> Self {
        entries.sort_by(|left, right| {
            (&left.path, &left.detector, &left.id).cmp(&(&right.path, &right.detector, &right.id))
        });
        Self {
            schema_version: BASELINE_SCHEMA_VERSION,
            identity,
            entries,
        }
    }

    pub fn identity(&self) -> ReportIdentity {
        self.identity.clone()
    }

    /// Re-key accepted entries against a current report under the same
    /// finding-ID contract: exact and carried matches are rebound to their
    /// current finding (keeping the review reason); unmatched entries are
    /// kept unless `prune_resolved`. New findings are never accepted here.
    pub fn rekey(
        &self,
        report_json: &str,
        prune_resolved: bool,
    ) -> Result<(Self, BaselineMatch), String> {
        validate_report_json(report_json)?;
        let identity = parse_report_identity(report_json)?;
        if !rekey_compatible(&self.identity, &identity) {
            return Err(
                "baseline and report differ in profile, scope, method, or contract versions; \
                 use `baseline migrate` across finding-ID versions"
                    .to_string(),
            );
        }
        let current = parse_report_findings(report_json)?;
        let by_id = current
            .iter()
            .map(|finding| (finding.id.as_str(), finding))
            .collect::<BTreeMap<_, _>>();
        let accepted = self
            .entries
            .iter()
            .map(BaselineEntry::match_subject)
            .collect::<Vec<_>>();
        let subjects = current
            .iter()
            .map(crate::ParsedFinding::match_subject)
            .collect::<Vec<_>>();
        let matched = match_baseline(&accepted, &subjects);
        let carried = matched
            .carried
            .iter()
            .map(|carried| (carried.baseline_id.as_str(), carried.current_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        let entries = self
            .entries
            .iter()
            .filter_map(|entry| {
                let current_id = carried.get(entry.id.as_str()).copied().unwrap_or(&entry.id);
                match by_id.get(current_id) {
                    Some(finding) => Some(entry.rebind(finding)),
                    None => (!prune_resolved).then(|| entry.clone()),
                }
            })
            .collect();
        Ok((Self::new(identity, entries), matched))
    }

    /// One-time migration across finding-ID versions. `previous_report` and
    /// `report` must be scans of the same source tree by the previous and the
    /// current auditor; findings correspond exactly by detector, location,
    /// and message. Returns the accepted IDs that had no unique counterpart.
    pub fn migrate(
        reasons: &BTreeMap<String, String>,
        previous_report_json: &str,
        report_json: &str,
    ) -> Result<(Self, Vec<String>), String> {
        validate_report_json(report_json)?;
        let correspondence = |finding: &crate::ParsedFinding| {
            (
                finding.detector.clone(),
                finding.path.clone(),
                finding.line,
                finding.message.clone(),
            )
        };
        let mut current: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let findings = parse_report_findings(report_json)?;
        for finding in &findings {
            current
                .entry(correspondence(finding))
                .or_default()
                .push(finding);
        }
        let previous = parse_report_findings(previous_report_json)?;
        let previous = previous
            .iter()
            .map(|finding| (finding.id.as_str(), finding))
            .collect::<BTreeMap<_, _>>();
        let mut entries = Vec::new();
        let mut unmapped = Vec::new();
        for (id, reason) in reasons {
            let target = previous
                .get(id.as_str())
                .and_then(|finding| current.get(&correspondence(finding)))
                .and_then(|candidates| match candidates.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                });
            match target {
                Some(finding) => entries.push(BaselineEntry::from_parsed(finding, reason.clone())),
                None => unmapped.push(id.clone()),
            }
        }
        Ok((
            Self::new(parse_report_identity(report_json)?, entries),
            unmapped,
        ))
    }

    pub fn to_json(&self) -> String {
        let identity = &self.identity;
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "detector": entry.detector,
                    "id": entry.id,
                    "identity": entry.identity,
                    "message": entry.message,
                    "path": entry.path,
                    "reason": entry.reason,
                    "title": entry.title,
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_string_pretty(&json!({
            "analysis_contract_digest": identity.analysis_contract_digest,
            "analysis_method": identity.analysis_method,
            "entries": entries,
            "evidence_graph_schema_version": identity.evidence_graph_schema_version,
            "finding_id_version": identity.finding_id_version,
            "profile": identity.profile,
            "report_schema_version": identity.schema_version,
            "schema_version": self.schema_version,
            "scope": identity.scope,
            "semantic_contract_version": identity.semantic_contract_version,
            "source_role_version": identity.source_role_version,
        }))
        .expect("baseline values are serializable")
    }
}

pub fn parse_baseline_document(text: &str) -> Result<BaselineDocument, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid baseline JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "baseline document must be an object".to_string())?;
    let identity = ReportIdentity {
        schema_version: required_u32(object.get("report_schema_version"), "report_schema_version")?,
        semantic_contract_version: required_u32(
            object.get("semantic_contract_version"),
            "semantic_contract_version",
        )?,
        evidence_graph_schema_version: required_u32(
            object.get("evidence_graph_schema_version"),
            "evidence_graph_schema_version",
        )?,
        finding_id_version: required_u32(object.get("finding_id_version"), "finding_id_version")?,
        source_role_version: required_u32(
            object.get("source_role_version"),
            "source_role_version",
        )?,
        profile: required_string(object.get("profile"), "profile")?,
        scope: required_string(object.get("scope"), "scope")?,
        analysis_method: required_string(object.get("analysis_method"), "analysis_method")?,
        analysis_contract_digest: required_string(
            object.get("analysis_contract_digest"),
            "analysis_contract_digest",
        )?,
    };
    let document = BaselineDocument::new(identity, parse_entries(object.get("entries"))?);
    if required_u32(object.get("schema_version"), "schema_version")? != document.schema_version {
        return Err("unsupported baseline schema version".to_string());
    }
    validate_identity(&document.identity)?;
    let mut ids = BTreeSet::new();
    if document.entries.iter().any(|entry| !ids.insert(&entry.id)) {
        return Err("baseline document contains duplicate finding IDs".to_string());
    }
    Ok(document)
}

/// Re-keying tolerates a changed analysis contract digest (detector code or
/// configuration changed) but not a different scan shape or ID contract.
fn rekey_compatible(baseline: &ReportIdentity, report: &ReportIdentity) -> bool {
    ReportIdentity {
        analysis_contract_digest: report.analysis_contract_digest.clone(),
        ..baseline.clone()
    } == *report
}

fn validate_identity(identity: &ReportIdentity) -> Result<(), String> {
    if identity.schema_version != REPORT_SCHEMA_VERSION
        || identity.semantic_contract_version != auditor_core::SEMANTIC_CONTRACT_VERSION
        || identity.evidence_graph_schema_version != EVIDENCE_GRAPH_SCHEMA_VERSION
        || identity.finding_id_version != auditor_core::FINDING_ID_VERSION
        || !matches!(identity.profile.as_str(), "gating" | "full")
        || !matches!(identity.scope.as_str(), "full" | "changed")
        || !matches!(identity.analysis_method.as_str(), "auto" | "ast" | "token")
        || identity.analysis_contract_digest.is_empty()
    {
        return Err("unsupported or incomplete baseline identity".to_string());
    }
    Ok(())
}

fn parse_entries(value: Option<&Value>) -> Result<Vec<BaselineEntry>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| "baseline document requires entries array".to_string())?;
    values
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .ok_or_else(|| "baseline entry must be an object".to_string())?;
            let reason = required_string(object.get("reason"), "entry.reason")?;
            if reason.trim().is_empty() {
                return Err("baseline entry reason must not be empty".to_string());
            }
            Ok(BaselineEntry {
                id: required_string(object.get("id"), "entry.id")?,
                detector: required_string(object.get("detector"), "entry.detector")?,
                title: required_string(object.get("title"), "entry.title")?,
                path: required_string(object.get("path"), "entry.path")?,
                message: required_string(object.get("message"), "entry.message")?,
                identity: crate::parser::string_map(
                    object.get("identity"),
                    "baseline entry.identity",
                )?,
                reason,
            })
        })
        .collect()
}

/// Read only the accepted IDs and review reasons of a baseline of any schema
/// or finding-ID version, for one-time migration to the current contract.
pub fn parse_legacy_baseline_reasons(text: &str) -> Result<BTreeMap<String, String>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid baseline JSON: {error}"))?;
    value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "baseline document requires entries array".to_string())?
        .iter()
        .map(|entry| {
            Ok((
                required_string(entry.get("id"), "entry.id")?,
                required_string(entry.get("reason"), "entry.reason")?,
            ))
        })
        .collect()
}

fn review_reason(reason: &str) -> Result<String, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        Err("baseline review reason must not be empty".to_string())
    } else {
        Ok(reason.to_string())
    }
}

fn required_string(value: Option<&Value>, field: &str) -> Result<String, String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("baseline requires nonempty {field}"))
}

fn required_u32(value: Option<&Value>, field: &str) -> Result<u32, String> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("baseline requires integer {field}"))
}
