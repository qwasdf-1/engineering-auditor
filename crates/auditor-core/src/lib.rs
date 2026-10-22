use std::collections::BTreeMap;
use std::fmt::Write;

mod parallel;
mod sha256;
mod typed_markdown;

pub use parallel::parallel_map;
pub use sha256::{sha256_digest_bytes, Sha256};
pub use typed_markdown::{
    render_typed_markdown, MarkdownDocument, MarkdownField, MarkdownSection, MarkdownValue,
    TYPED_MARKDOWN_RENDERER_VERSION,
};

pub const SEMANTIC_CONTRACT_VERSION: u32 = 1;
pub const EXTRACTOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Severity {
    Advisory,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvidenceValue {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    StringList(Vec<String>),
    IntegerList(Vec<i64>),
}

impl EvidenceValue {
    pub fn to_json(&self) -> String {
        match self {
            Self::String(value) => format!("\"{}\"", json_escape(value)),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => render_json_float(*value),
            Self::Bool(value) => value.to_string(),
            Self::StringList(values) => render_json_list(
                values
                    .iter()
                    .map(|value| format!("\"{}\"", json_escape(value))),
            ),
            Self::IntegerList(values) => render_json_list(values.iter().map(ToString::to_string)),
        }
    }
}

fn render_json_float(value: f64) -> String {
    if !value.is_finite() {
        return "0.0".to_string();
    }
    let mut rendered = format!("{value:.3}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.push('0');
    }
    rendered
}

fn render_json_list(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(", "))
}

impl From<&str> for EvidenceValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for EvidenceValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<usize> for EvidenceValue {
    fn from(value: usize) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<i64> for EvidenceValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for EvidenceValue {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<f64> for EvidenceValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for EvidenceValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<Vec<String>> for EvidenceValue {
    fn from(value: Vec<String>) -> Self {
        Self::StringList(value)
    }
}

impl From<Vec<&str>> for EvidenceValue {
    fn from(value: Vec<&str>) -> Self {
        Self::StringList(value.into_iter().map(str::to_string).collect())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Location {
    pub path: String,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindingProvenance {
    pub language: String,
    pub source_role: String,
    pub analysis_method: String,
    pub extractor_version: u32,
    pub origin: String,
    pub span_precision: String,
}

impl FindingProvenance {
    pub fn unknown() -> Self {
        Self {
            language: "unknown".to_string(),
            source_role: "unknown".to_string(),
            analysis_method: "unknown".to_string(),
            extractor_version: EXTRACTOR_VERSION,
            origin: "native".to_string(),
            span_precision: "unknown".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageCapability {
    pub language: String,
    pub mode: String,
    pub limitation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectorContract {
    pub id: String,
    pub category: String,
    pub default_severity: Severity,
    pub profile: String,
    pub maturity: String,
    pub layer: String,
    pub emission_roles: Vec<String>,
    pub context_roles: Vec<String>,
    pub language_support: Vec<LanguageCapability>,
    pub finding_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityExecutionStatus {
    Executed,
    NotApplicable,
    Unsupported,
    MissingInput,
    Unavailable,
}

impl CapabilityExecutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "executed",
            Self::NotApplicable => "not_applicable",
            Self::Unsupported => "unsupported",
            Self::MissingInput => "missing_input",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityExecution {
    pub detector_id: String,
    pub layer: String,
    pub status: CapabilityExecutionStatus,
    pub eligible_files: usize,
    pub analyzed_files: usize,
    pub unsupported_files: usize,
    pub not_applicable_files: usize,
    pub required_inputs: Vec<String>,
    pub missing_inputs: Vec<String>,
    pub limitations: Vec<String>,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryMapArtifact {
    pub path: String,
    pub digest: String,
    pub source_snapshot_digest: String,
    pub completeness: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisFile {
    pub path: String,
    pub language: String,
    pub catalog_language: String,
    pub bytes: u64,
    pub source_role: String,
    pub analysis_method: String,
    pub extractor_version: u32,
    /// `recovered` when the file parsed only after quarantining the
    /// declarations holding syntax errors, `unparsed` when syntax errors
    /// kept it out of structural analysis; `None` when it parsed as written
    /// or was not parsed.
    pub syntax: Option<String>,
}

/// A finding an inline `auditor-ignore[..]: reason` comment removed from a
/// scan's findings, kept with the reason the comment gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuppressedFinding {
    pub id: String,
    pub detector: String,
    pub path: String,
    pub line: usize,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub id: String,
    pub detector: String,
    pub category: String,
    pub title: String,
    pub message: String,
    pub recommendation: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub locations: Vec<Location>,
    pub evidence: BTreeMap<String, EvidenceValue>,
    pub identity: BTreeMap<String, String>,
    pub provenance: FindingProvenance,
}

impl PartialEq for Finding {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.detector == other.detector
            && self.category == other.category
            && self.title == other.title
            && self.message == other.message
            && self.recommendation == other.recommendation
            && self.severity == other.severity
            && self.confidence == other.confidence
            && self.locations == other.locations
            && self.evidence == other.evidence
            && self.identity == other.identity
            && self.provenance == other.provenance
    }
}

impl Finding {
    /// Titles participate in finding identity, so they must be static: a
    /// formatted title would re-key a finding whenever its values change.
    pub fn builder(detector: impl Into<String>, title: &'static str) -> FindingBuilder {
        FindingBuilder::new(detector, title)
    }

    pub fn at(mut self, path: impl Into<String>, line: Option<usize>) -> Self {
        let path = path.into();
        self.locations = vec![Location {
            path: path.clone(),
            line,
        }];
        self.refresh_id();
        self
    }

    pub fn with_recommendation(mut self, recommendation: impl Into<String>) -> Self {
        self.recommendation = recommendation.into();
        self
    }

    pub fn with_evidence(
        mut self,
        key: impl Into<String>,
        value: impl Into<EvidenceValue>,
    ) -> Self {
        self.evidence.insert(key.into(), value.into());
        self
    }

    pub fn with_identity(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.identity.insert(key.into(), value.into());
        self.refresh_id();
        self
    }

    pub fn with_provenance(mut self, provenance: FindingProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    /// Bind a code anchor and recompute the ID. Detectors set explicit
    /// `Structural` anchors; the scan derives `Symbol`/`Content` anchors.
    pub fn with_anchor(mut self, anchor: FindingAnchor) -> Self {
        anchor.write_identity(&mut self.identity);
        self.refresh_id();
        self
    }

    pub fn anchor(&self) -> Option<&str> {
        self.identity.get(IDENTITY_ANCHOR_KEY).map(String::as_str)
    }

    /// Record a disambiguating subject or occurrence ordinal derived by the
    /// scan, then recompute the ID. `key` must be one of `RESERVED_IDENTITY_KEYS`.
    pub fn with_derived_identity(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.identity.insert(key.to_string(), value.into());
        self.refresh_id();
        self
    }

    fn refresh_id(&mut self) {
        self.id = stable_finding_id_v3(
            &self.detector,
            &self.title,
            self.primary_path(),
            self.primary_line(),
            &self.identity,
        );
    }

    pub fn evidence_keys(&self) -> Vec<String> {
        self.evidence.keys().cloned().collect()
    }

    pub fn primary_path(&self) -> &str {
        self.locations
            .first()
            .map(|location| location.path.as_str())
            .unwrap_or("")
    }

    pub fn primary_line(&self) -> Option<usize> {
        self.locations.first().and_then(|location| location.line)
    }
}

/// Construct a finding through the typed builder while preserving the concise
/// detector call shape. The macro owns the stable positional compatibility
/// boundary; the underlying Rust API remains field-named and extensible.
#[macro_export]
macro_rules! finding {
    (
        $detector:expr,
        $category:expr,
        $title:expr,
        $message:expr,
        $severity:expr,
        $confidence:expr,
        $path:expr,
        $line:expr $(,)?
    ) => {{
        $crate::Finding::builder($detector, $title)
            .category($category)
            .message($message)
            .severity($severity)
            .confidence($confidence)
            .at($path, $line)
            .build()
    }};
}

pub struct FindingBuilder {
    detector: String,
    category: String,
    title: String,
    message: String,
    recommendation: String,
    severity: Severity,
    confidence: Confidence,
    locations: Vec<Location>,
    evidence: BTreeMap<String, EvidenceValue>,
    identity: BTreeMap<String, String>,
    provenance: FindingProvenance,
}

impl FindingBuilder {
    pub fn new(detector: impl Into<String>, title: &'static str) -> Self {
        Self {
            detector: detector.into(),
            category: String::new(),
            title: title.to_string(),
            message: String::new(),
            recommendation: String::new(),
            severity: Severity::Medium,
            confidence: Confidence::Medium,
            locations: Vec::new(),
            evidence: BTreeMap::new(),
            identity: BTreeMap::new(),
            provenance: FindingProvenance::unknown(),
        }
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn recommendation(mut self, recommendation: impl Into<String>) -> Self {
        self.recommendation = recommendation.into();
        self
    }

    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn at(mut self, path: impl Into<String>, line: Option<usize>) -> Self {
        self.locations = vec![Location {
            path: path.into(),
            line,
        }];
        self
    }

    pub fn evidence(mut self, key: impl Into<String>, value: impl Into<EvidenceValue>) -> Self {
        self.evidence.insert(key.into(), value.into());
        self
    }

    pub fn identity(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.identity.insert(key.into(), value.into());
        self
    }

    pub fn provenance(mut self, provenance: FindingProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn build(self) -> Finding {
        let location = self.locations.first();
        let id = stable_finding_id_v3(
            &self.detector,
            &self.title,
            location.map(|l| l.path.as_str()).unwrap_or(""),
            location.and_then(|l| l.line),
            &self.identity,
        );
        Finding {
            id,
            detector: self.detector,
            category: self.category,
            title: self.title,
            message: self.message,
            recommendation: self.recommendation,
            severity: self.severity,
            confidence: self.confidence,
            locations: self.locations,
            evidence: self.evidence,
            identity: self.identity,
            provenance: self.provenance,
        }
    }
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn stable_digest_bytes(bytes: &[u8]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("fnv1a64:{hash:016x}")
}

pub fn stable_digest<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for segment in segments {
        for byte in segment.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    format!("fnv1a64:{hash:016x}")
}

pub fn stable_finding_id(detector: &str, path: &str, title: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for segment in [detector, "\0", path, "\0", title] {
        for byte in segment.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    format!("EA-{hash:016x}")
}

pub const FINDING_ID_VERSION: u32 = 3;

/// Reserved `identity` keys owned by the scan rather than by detectors.
pub const IDENTITY_ANCHOR_KEY: &str = "anchor";
pub const IDENTITY_LINE_CONTENT_KEY: &str = "line_content";
pub const IDENTITY_ORDINAL_KEY: &str = "ordinal";
pub const RESERVED_IDENTITY_KEYS: [&str; 3] = [
    IDENTITY_ANCHOR_KEY,
    IDENTITY_LINE_CONTENT_KEY,
    IDENTITY_ORDINAL_KEY,
];

/// The stable code owner a finding is keyed on. No variant carries a line
/// number, so edits outside the anchor cannot change finding identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FindingAnchor {
    /// Qualified symbol path inside the finding's primary file.
    Symbol(String),
    /// Normalized-content digest of the flagged line inside the primary file.
    Content(String),
    /// Owner-declared key that is independent of the primary path, such as a
    /// clone class or dependency cycle.
    Structural(String),
}

impl FindingAnchor {
    fn write_identity(&self, identity: &mut BTreeMap<String, String>) {
        let value = match self {
            Self::Symbol(path) => format!("symbol:{path}"),
            Self::Content(digest) => format!("content:{digest}"),
            Self::Structural(key) => format!("structural:{key}"),
        };
        identity.insert(IDENTITY_ANCHOR_KEY.to_string(), value);
    }

    fn is_structural(anchor: Option<&String>) -> bool {
        anchor.is_some_and(|value| value.starts_with("structural:"))
    }
}

/// Digest of one source line with whitespace runs collapsed, so reindenting
/// or reformatting the line keeps its digest.
pub fn normalized_line_digest(line: &str) -> String {
    let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
    stable_digest([normalized.as_str()])
}

/// Finding-ID v3: detector, static title, and the identity map (detector
/// subjects plus scan-derived anchor, line content, and ordinal). The primary
/// path participates unless the anchor is structural. The line participates
/// only while no anchor is bound: that provisional ID keeps owner-local
/// deduplication exact, and anchoring removes the line from the final ID.
pub fn stable_finding_id_v3(
    detector: &str,
    title: &str,
    path: &str,
    line: Option<usize>,
    identity: &BTreeMap<String, String>,
) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    let mut hash_segment = |segment: &str| {
        for byte in segment.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    hash_segment("v3");
    hash_segment(detector);
    hash_segment(title);
    let anchor = identity.get(IDENTITY_ANCHOR_KEY);
    if !FindingAnchor::is_structural(anchor) {
        hash_segment(path);
    }
    if anchor.is_none() {
        hash_segment(&line.unwrap_or(0).to_string());
    }
    for (key, value) in identity {
        hash_segment(key);
        hash_segment(value);
    }
    format!("EA-{hash:016x}")
}

/// Logarithmic weight function: `ln(1 + (value - threshold) / scale)`.
///
/// Returns 0.0 when `value <= threshold`. Use for normalizing raw counts
/// into bounded scores that grow logarithmically beyond a noise floor.
pub fn log_weight(value: f64, threshold: f64, scale: f64) -> f64 {
    if value <= threshold {
        0.0
    } else {
        (1.0 + (value - threshold) / scale).ln()
    }
}

pub fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        push_json_char(&mut escaped, ch);
    }
    escaped
}

fn push_json_char(escaped: &mut String, ch: char) {
    match ch {
        '"' => escaped.push_str("\\\""),
        '\\' => escaped.push_str("\\\\"),
        '\n' => escaped.push_str("\\n"),
        '\r' => escaped.push_str("\\r"),
        '\t' => escaped.push_str("\\t"),
        c if c.is_control() => write!(escaped, "\\u{:04x}", c as u32).unwrap(),
        c => escaped.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_are_deterministic() {
        let first = stable_finding_id("detector", "src/lib.rs", "title");
        let second = stable_finding_id("detector", "src/lib.rs", "title");
        assert_eq!(first, second);
        assert!(first.starts_with("EA-"));
    }

    #[test]
    fn json_escape_handles_controls() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }

    #[test]
    fn finding_builder_records_evidence_keys() {
        let finding = crate::finding!(
            "detector",
            "structural",
            "Title",
            "message",
            Severity::Medium,
            Confidence::High,
            "src/lib.rs",
            Some(3),
        )
        .with_evidence("count", 7usize)
        .with_evidence("kind", "fixture");

        assert_eq!(finding.evidence_keys(), vec!["count", "kind"]);
        assert_eq!(finding.primary_path(), "src/lib.rs");
        assert_eq!(finding.primary_line(), Some(3));
        assert_eq!(finding.provenance.span_precision, "unknown");
    }

    fn located(path: &str, line: usize) -> Finding {
        crate::finding!(
            "detector",
            "structural",
            "Title",
            "message",
            Severity::Medium,
            Confidence::High,
            path,
            Some(line),
        )
    }

    #[test]
    fn finding_id_v3_ignores_line_numbers() {
        let anchor = || FindingAnchor::Symbol("Type::method".to_string());
        // Unanchored IDs are provisional and stay line-keyed for owner dedup.
        assert_ne!(located("src/a.rs", 3).id, located("src/a.rs", 90).id);
        assert_eq!(
            located("src/a.rs", 3).with_anchor(anchor()).id,
            located("src/a.rs", 90).with_anchor(anchor()).id
        );
        assert_ne!(
            located("src/a.rs", 3).with_anchor(anchor()).id,
            located("src/a.rs", 3).id
        );
    }

    #[test]
    fn finding_id_v3_structural_anchor_ignores_primary_path() {
        let anchor = || FindingAnchor::Structural("clone:0f".to_string());
        assert_eq!(
            located("src/a.rs", 3).with_anchor(anchor()).id,
            located("src/b.rs", 7).with_anchor(anchor()).id
        );
        let symbol = || FindingAnchor::Symbol("f".to_string());
        assert_ne!(
            located("src/a.rs", 3).with_anchor(symbol()).id,
            located("src/b.rs", 3).with_anchor(symbol()).id
        );
    }

    #[test]
    fn finding_id_v3_derived_identity_disambiguates_occurrences() {
        let first = located("src/a.rs", 3);
        let second = located("src/a.rs", 4).with_derived_identity(IDENTITY_ORDINAL_KEY, "1");
        assert_ne!(first.id, second.id);
        assert_eq!(
            second
                .identity
                .get(IDENTITY_ORDINAL_KEY)
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            normalized_line_digest("  let  x = 1;"),
            normalized_line_digest("let x = 1;\t")
        );
    }

    #[test]
    fn stable_contract_digests_are_order_sensitive() {
        assert_eq!(stable_digest(["a", "b"]), stable_digest(["a", "b"]));
        assert_ne!(stable_digest(["a", "b"]), stable_digest(["b", "a"]));
        assert_eq!(
            stable_digest_bytes(b"report"),
            stable_digest_bytes(b"report")
        );
    }
}
