use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::{util, Language};

const HASH_BASE: u64 = 0x9e37_79b9_7f4a_7c15;
const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;
const MIN_TOKENS: usize = 50;
const MIN_LINES: usize = 8;

#[path = "duplication_filters.rs"]
mod filters;
#[path = "duplication_matcher.rs"]
mod matcher;
use filters::{
    abuts_as_rows, is_declarative_call_shape, is_declarative_field_mapping,
    is_declarative_json_macro, is_locale_repetition, is_parallel_rows, is_python_docstring,
    is_state_machine_repetition, is_sync_async_twin,
};
#[cfg(test)]
use matcher::{base_pow, roll};
use matcher::{find_clone_classes, fnv1a_hash, hash_window, token_hash};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum TokenKind {
    Identifier = 1,
    Literal = 2,
    Syntax = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DetectionToken {
    hash: u64,
    identity: u64,
    kind: TokenKind,
    semantic_anchor: bool,
    function_owner: Option<usize>,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

#[derive(Clone, Debug)]
struct TokenizedFile {
    path: String,
    /// The build package (nearest directory with a build manifest) of the
    /// file: a clone across two packages can be shared only by adding public
    /// API to one of them.
    package: Option<std::path::PathBuf>,
    tokens: Vec<DetectionToken>,
    /// Function owner (start byte) -> its signature facts.
    owners: BTreeMap<usize, Owner>,
}

#[derive(Clone, Debug)]
struct Owner {
    name: String,
    /// Declared `async`.
    is_async: bool,
    /// The enum (start byte) whose constant body declares this method: a
    /// Java enum state machine's per-state handler.
    state_of: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct WindowLocation {
    file: usize,
    start: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CloneOccurrence {
    file: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct CloneClass {
    token_hash: u64,
    token_count: usize,
    /// Most differing bindings between the first occurrence and another.
    parameters: usize,
    occurrences: BTreeSet<CloneOccurrence>,
}

pub(crate) fn copy_paste_detect(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut by_language: BTreeMap<Language, Vec<TokenizedFile>> = BTreeMap::new();
    let mut packages = BTreeMap::new();
    ast_scan::visit_ast_files(scan, Language::is_source, |file, tree, _language| {
        let mut tokens = Vec::new();
        let mut owners = BTreeMap::new();
        collect_tokens(
            tree.root_node(),
            &file.language,
            None,
            &mut tokens,
            &mut owners,
        );
        if tokens.len() >= MIN_TOKENS {
            by_language
                .entry(file.language.clone())
                .or_default()
                .push(TokenizedFile {
                    path: util::path_slash(&file.relative),
                    package: build_package(&file.absolute, &mut packages),
                    tokens,
                    owners,
                });
        }
    });
    let mut findings = Vec::new();
    for files in by_language.values_mut() {
        files.sort_by(|left, right| left.path.cmp(&right.path));
        findings.extend(find_clone_classes(files));
    }
    findings.sort_by(|left, right| {
        (left.primary_path(), &left.title, &left.id).cmp(&(
            right.primary_path(),
            &right.title,
            &right.id,
        ))
    });
    findings
}

/// The nearest ancestor directory of `path` holding a build manifest.
fn build_package(
    path: &std::path::Path,
    packages: &mut BTreeMap<std::path::PathBuf, Option<std::path::PathBuf>>,
) -> Option<std::path::PathBuf> {
    let directory = path.parent()?;
    if let Some(package) = packages.get(directory) {
        return package.clone();
    }
    const MANIFESTS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "pyproject.toml",
        "setup.py",
    ];
    let package = if MANIFESTS
        .iter()
        .any(|manifest| directory.join(manifest).is_file())
    {
        Some(directory.to_path_buf())
    } else {
        build_package(directory, packages)
    };
    packages.insert(directory.to_path_buf(), package.clone());
    package
}

fn collect_tokens(
    node: AstNode<'_>,
    language: &Language,
    function_owner: Option<usize>,
    tokens: &mut Vec<DetectionToken>,
    owners: &mut BTreeMap<usize, Owner>,
) {
    if node.kind().contains("comment")
        || is_python_docstring(node, language)
        || is_declarative_json_macro(node)
        || is_declarative_field_mapping(node, language)
        || is_declarative_call_shape(node)
        || (auditor_ast::function_node_kinds().contains(&node.kind())
            && ast_scan::is_inside_rust_test(node))
    {
        return;
    }
    let is_function = auditor_ast::function_node_kinds().contains(&node.kind());
    if is_function {
        owners.insert(node.start_byte(), function_signature(node));
    }
    let function_owner = is_function.then_some(node.start_byte()).or(function_owner);
    // A signature (generics, `where` clauses, return types) states an
    // interface; only the body can repeat behavior.
    if let Some(body) = is_function
        .then(|| node.child_by_field_name("body"))
        .flatten()
    {
        collect_tokens(body, language, function_owner, tokens, owners);
        return;
    }
    if node.child_count() != 0 {
        for child in node.children() {
            collect_tokens(child, language, function_owner, tokens, owners);
        }
        return;
    }
    if let Some(token) = detection_token(node, function_owner) {
        tokens.push(token);
    }
}

/// A function's name, whether it is declared `async` (the keyword before its
/// name: `async def`, `async function`, `pub async fn`), and the enum whose
/// constant body declares it.
fn function_signature(node: AstNode<'_>) -> Owner {
    let name = node.child_by_field_name("name");
    let head_end = name.map_or(node.end_byte(), |name| name.start_byte());
    let head = node
        .text()
        .get(..head_end - node.start_byte())
        .unwrap_or_default();
    let is_async = head
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .any(|word| word == "async");
    let state_of = node
        .parent()
        .filter(|body| body.kind() == "class_body")
        .and_then(|body| body.parent())
        .filter(|constant| constant.kind() == "enum_constant")
        .and_then(|constant| constant.parent())
        .and_then(|body| body.parent())
        .map(|declaration| declaration.start_byte());
    Owner {
        name: name.map(|name| name.text().to_string()).unwrap_or_default(),
        is_async,
        state_of,
    }
}

fn detection_token(node: AstNode<'_>, function_owner: Option<usize>) -> Option<DetectionToken> {
    let text = node.text().trim();
    if text.is_empty() {
        return None;
    }
    let kind = classify_token(node.kind(), text);
    let normalized = match kind {
        TokenKind::Identifier => "<identifier>",
        TokenKind::Literal => "<literal>",
        TokenKind::Syntax => text,
    };
    let node_kind = fnv1a_hash(node.kind());
    let parent_kind = node
        .parent()
        .map(|parent| fnv1a_hash(parent.kind()))
        .unwrap_or(0);
    let (start_line, start_column) = node.start_position();
    let (end_line, end_column) = node.end_position();
    Some(DetectionToken {
        hash: hash_window([token_hash(kind, normalized), node_kind, parent_kind]),
        identity: fnv1a_hash(text),
        kind,
        semantic_anchor: kind != TokenKind::Syntax && !matches!(text, "self" | "this" | "cls"),
        function_owner,
        start_line: start_line + 1,
        start_column: start_column + 1,
        end_line: end_line + 1,
        end_column: end_column + 1,
    })
}

fn classify_token(kind: &str, text: &str) -> TokenKind {
    if kind.contains("identifier")
        || matches!(
            kind,
            "constant" | "field" | "label" | "property" | "simple_name" | "word"
        )
    {
        return TokenKind::Identifier;
    }
    if kind.contains("string")
        || kind.contains("character")
        || kind.contains("integer")
        || kind.contains("float")
        || kind.contains("number")
        || kind.contains("literal")
        || matches!(text, "true" | "false" | "null" | "None" | "nil")
        || text.starts_with(|character: char| character.is_ascii_digit())
    {
        return TokenKind::Literal;
    }
    TokenKind::Syntax
}

fn clone_finding(files: &[TokenizedFile], class: CloneClass) -> Option<Finding> {
    // An extraction that needs more parameters than the parameter budget
    // (table rows, parallel match arms) is not a reasonable refactoring.
    let representative = canonical_occurrence(&class)?;
    let representative_tokens =
        &files[representative.file].tokens[representative.start..representative.end];
    if is_sync_async_twin(files, &class)
        || class.parameters > crate::TOO_MANY_PARAMETERS_THRESHOLD
        || is_parallel_rows(representative_tokens)
        || abuts_as_rows(files, &class)
        || is_state_machine_repetition(files, &class)
        || is_locale_repetition(files, &class)
    {
        return None;
    }
    let occurrences = class
        .occurrences
        .iter()
        .map(|occurrence| occurrence_label(files, *occurrence))
        .collect::<Vec<_>>();
    let first = &files[representative.file];
    let start = first.tokens.get(representative.start)?;
    Some(
        crate::bloat::source_duplicate_finding(
            "Normalized source clone class exceeds budget",
            format!(
                "Clone class spans {} normalized tokens across {} non-overlapping occurrence(s).",
                class.token_count,
                occurrences.len()
            ),
            Severity::Medium,
            Confidence::High,
            &first.path,
            Some(start.start_line),
        )
        .with_recommendation(
            "Consolidate the repeated behavior only when a shared owner reduces total coupling.",
        )
        .with_anchor(auditor_core::FindingAnchor::Structural(format!(
            "clone:{:016x}",
            class.token_hash
        )))
        .with_evidence("clone_class", format!("{:016x}", class.token_hash))
        .with_evidence("token_count", class.token_count)
        .with_evidence("differing_bindings", class.parameters)
        .with_evidence("occurrence_count", occurrences.len())
        .with_evidence("occurrences", occurrences),
    )
}

fn canonical_occurrence(class: &CloneClass) -> Option<CloneOccurrence> {
    class.occurrences.iter().next().copied()
}

fn occurrence_label(files: &[TokenizedFile], occurrence: CloneOccurrence) -> String {
    let tokens = &files[occurrence.file].tokens;
    let start = tokens[occurrence.start];
    let end = tokens[occurrence.end - 1];
    format!(
        "{}:{}:{}-{}:{}",
        files[occurrence.file].path,
        start.start_line,
        start.start_column,
        end.end_line,
        end.end_column
    )
}

fn occurrence_line_count(files: &[TokenizedFile], occurrence: CloneOccurrence) -> usize {
    let tokens = &files[occurrence.file].tokens;
    let start = tokens[occurrence.start].start_line;
    let end = tokens[occurrence.end - 1].end_line;
    end.saturating_sub(start) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const ROLLING_HASH_VALUES: [u64; 6] = [3, 5, 7, 11, 13, 17];

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-clone-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_file(root: &std::path::Path, relative: &str, source: &str) -> crate::AuditedFile {
        let absolute = root.join(relative);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(&absolute, source).unwrap();
        crate::AuditedFile {
            absolute,
            relative: relative.into(),
            language: Language::Rust,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    fn detect(files: &[crate::AuditedFile]) -> Vec<Finding> {
        let cache = util::FileCache::new(files);
        let mut repo_ast = auditor_ast::RepoAst::new();
        copy_paste_detect(AstScan::new(
            files,
            crate::AnalysisMethod::Ast,
            &cache,
            &mut repo_ast,
        ))
    }

    fn clone_source(name: &str, variable: &str, literal: usize) -> String {
        let literal = literal + 100;
        format!(
            "fn {name}(input: &[i32], limit: i32) -> i32 {{
    let mut {variable} = 0;
    for item in input {{
        if *item > limit {{
            {variable} += item * {literal};
        }} else if *item < 0 {{
            {variable} -= item.abs();
        }}
    }}
    let average = {variable} / input.len().max(1) as i32;
    match average.cmp(&limit) {{
        std::cmp::Ordering::Less => average + limit,
        std::cmp::Ordering::Equal => limit,
        std::cmp::Ordering::Greater => average - {literal},
    }}
}}
"
        )
    }

    #[test]
    fn identifiers_and_literals_are_normalized_into_one_clone_class() {
        let root = temp_root("normalized-clone");
        let first = make_file(&root, "src/first.rs", &clone_source("first", "value", 2));
        let second = make_file(&root, "src/second.rs", &clone_source("second", "scaled", 7));
        let findings = detect(&[first, second]);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].evidence.get("occurrence_count"),
            Some(&auditor_core::EvidenceValue::Integer(2))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn intra_file_clones_are_supported_without_pair_expansion() {
        let root = temp_root("intra-file-clone");
        let source = format!(
            "{}\n{}",
            clone_source("first", "value", 2),
            clone_source("second", "scaled", 7)
        );
        let file = make_file(&root, "src/lib.rs", &source);
        let findings = detect(std::slice::from_ref(&file));
        assert_eq!(findings.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_family_has_one_stable_representative() {
        let root = temp_root("clone-family");
        let files = (0..5)
            .map(|index| {
                make_file(
                    &root,
                    &format!("src/module_{index}.rs"),
                    &clone_source(&format!("process_{index}"), "value", index + 2),
                )
            })
            .collect::<Vec<_>>();
        let forward = detect(&files);
        let reverse = detect(&files.iter().cloned().rev().collect::<Vec<_>>());
        assert_eq!(forward.len(), 1);
        assert_eq!(
            (forward[0].id.clone(), forward[0].evidence.clone()),
            (reverse[0].id.clone(), reverse[0].evidence.clone())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn short_or_overlapping_windows_do_not_emit() {
        let root = temp_root("clone-boundaries");
        let short = make_file(
            &root,
            "src/short.rs",
            "fn compact(value: i32) -> i32 { value + 1 }\n",
        );
        assert!(detect(std::slice::from_ref(&short)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn homogeneous_unique_call_lists_are_not_clone_classes() {
        let root = temp_root("homogeneous-call-list");
        let calls = (0..80)
            .map(|index| format!("        self.path_{index}(),"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!(
            "struct Paths;\nimpl Paths {{\n    fn all(&self) -> Vec<usize> {{\n        vec![\n{calls}\n        ]\n    }}\n}}\n"
        );
        let file = make_file(&root, "src/lib.rs", &source);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn declarative_json_mappings_are_not_behavior_clones() {
        let root = temp_root("json-schema-mapping");
        let source = r#"
fn baseline_value(report: &Report) -> Value {
    serde_json::json!({
        "baseline_path": report.baseline.baseline_path,
        "compared": report.baseline.compared,
        "new_findings": report.baseline.new_findings,
        "resolved_findings": report.baseline.resolved_findings,
        "schema_version": report.baseline.schema_version,
        "unchanged_findings": report.baseline.unchanged_findings,
    })
}

fn provenance_value(finding: &Finding) -> Value {
    serde_json::json!({
        "analysis_method": finding.provenance.analysis_method,
        "extractor_version": finding.provenance.extractor_version,
        "language": finding.provenance.language,
        "origin": finding.provenance.origin,
        "source_role": finding.provenance.source_role,
        "span_precision": finding.provenance.span_precision,
    })
}
"#;
        let file = make_file(&root, "src/lib.rs", source);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn declarative_struct_field_mappings_are_not_behavior_clones() {
        let root = temp_root("struct-field-mapping");
        let fields = (0..24)
            .map(|index| {
                format!(
                    "        field_{index}: context.map_{index}.get(&id).map(|entry| entry.value.clone()).unwrap_or_default(),"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!(
            "fn project(context: &Context, id: Id) -> Output {{\n    Output {{\n{fields}\n    }}\n}}\n"
        );
        let file = make_file(&root, "src/lib.rs", &source);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_keyword_wrappers_and_data_rows_are_not_behavior_clones() {
        let root = temp_root("keyword-wrappers");
        let names = [
            "params", "headers", "cookies", "auth", "proxy", "timeout", "verify",
        ];
        let signature = names
            .map(|name| format!("    {name}: Opt = None,\n"))
            .concat();
        let forwarded = names
            .map(|name| format!("        {name}={name},\n"))
            .concat();
        let wrapper = |method: &str| {
            format!(
                "def {method}(\n    url: str,\n    *,\n{signature}) -> Response:\n    return request(\n        \"{}\",\n        url,\n{forwarded}    )\n\n",
                method.to_uppercase()
            )
        };
        let rows = (0..14)
            .map(|index| format!("    table.add_row(\"--flag-{index}\", \"Help text {index}.\")\n"))
            .collect::<String>();
        let source = format!(
            "{}{}{}def help():\n{rows}\ndef usage():\n{rows}",
            wrapper("get"),
            wrapper("head"),
            wrapper("options")
        );
        let absolute = root.join("api.py");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&absolute, &source).unwrap();
        let file = crate::AuditedFile {
            absolute,
            relative: "api.py".into(),
            language: Language::Python,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        };
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn docstrings_do_not_lengthen_short_bodies_into_clones() {
        let root = temp_root("docstrings");
        let aligned = |name: &str| {
            format!(
                "def {name}(lines, width, height, style):\n    \"\"\"Align lines.\n\n    Args:\n        lines: Lines.\n        width: Width.\n        height: Height.\n        style: Style.\n    \"\"\"\n    extra = height - len(lines)\n    if not extra:\n        return lines[:]\n    rows = [line[:width] for line in lines if line]\n    blank = [Segment(\" \" * width, style)]\n    return rows[:height] + [blank] * extra\n\n"
            )
        };
        let source = format!("{}{}", aligned("align_top"), aligned("align_bottom"));
        let absolute = root.join("segment.py");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&absolute, &source).unwrap();
        let file = crate::AuditedFile {
            absolute,
            relative: "segment.py".into(),
            language: Language::Python,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        };
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sync_and_async_twins_are_not_clone_classes() {
        let root = temp_root("async-twins");
        let body = r#"    response = transport.handle(request)
    if response.status >= 400:
        log(response, request)
        raise_error(response.status, request.url)
    headers = dict(response.headers)
    for name, value in headers.items():
        if name.startswith("x-"):
            request.meta[name] = value
    cookies = parse_cookies(headers.get("set-cookie"), request.url)
    request.session.update(cookies)
    elapsed = clock() - request.started
    metrics.record(request.url, response.status, elapsed)
"#;
        let source = format!(
            "def send(request, transport):\n{body}    return request\n\nasync def send(request, transport):\n{body}    return request\n"
        );
        let absolute = root.join("client.py");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&absolute, &source).unwrap();
        let file = crate::AuditedFile {
            absolute,
            relative: "client.py".into(),
            language: Language::Python,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        };
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        let twin = source.replacen("async def send", "def resend", 1);
        std::fs::write(&file.absolute, &twin).unwrap();
        assert_eq!(
            detect(std::slice::from_ref(&file)).len(),
            1,
            "a sync copy is a clone"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clones_needing_more_parameters_than_the_budget_are_not_reported() {
        let root = temp_root("parallel-arms");
        let parser = |name: &str, kind: &str, labels: &[&str]| {
            let arms = labels
                .iter()
                .map(|label| {
                    format!(
                        "        \"{label}\" => Ok({kind}::{}),\n",
                        label.to_uppercase()
                    )
                })
                .collect::<String>();
            format!(
                "fn {name}(text: &str) -> Result<{kind}, Error> {{\n    match &*text.to_lowercase() {{\n{arms}        _ => Err(Error::Unrecognized(text.to_string())),\n    }}\n}}\n"
            )
        };
        let source = format!(
            "{}{}",
            parser(
                "parse_out",
                "OutType",
                &[
                    "path",
                    "line",
                    "column",
                    "match",
                    "context",
                    "separator",
                    "binary"
                ]
            ),
            parser(
                "parse_spec",
                "SpecType",
                &["fg", "bg", "style", "none", "bold", "intense", "underline"]
            )
        );
        let file = make_file(&root, "src/lib.rs", &source);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn signatures_row_tables_and_cross_package_copies_are_not_clones() {
        let root = temp_root("clone-precision");
        // Identical `where` clauses around one-line bodies.
        let routed = |name: &str| {
            format!(
                "pub fn {name}<H, T, S>(self, handler: H) -> Self\nwhere\n    H: Handler<T, S> + Clone + Send + Sync + 'static,\n    T: Extract<S> + Send + 'static,\n    S: Clone + Send + Sync + 'static,\n    H::Future: Send + 'static,\n    H::Output: IntoResponse + Send + 'static,\n{{\n    self.route(handler)\n}}\n"
            )
        };
        // Match arms repeating one row shape with different names.
        let rows = |name: &str, kind: &str| {
            let arms = ["begin", "end", "matched", "context", "summary"]
                .map(|label| format!("        Message::{label}(ref msg) => {{\n            state.field(\"type\", \"{label}\")?;\n            state.field(\"data\", msg)?;\n        }}\n"))
                .concat();
            format!("fn {name}(message: &{kind}, state: &mut State) -> Result<(), Error> {{\n    match message {{\n{arms}    }}\n    Ok(())\n}}\n")
        };
        // Enum delegation: every arm forwards to the method's own name, a
        // row table as well.
        let delegate = |name: &str| {
            format!("fn {name}(&mut self, value: &Spec) -> io::Result<()> {{\n    use self::Kind::*;\n    match self.0 {{\n        LineBuffered(ref mut w) => w.{name}(value),\n        BlockBuffered(ref mut w) => w.{name}(value),\n        Unbuffered(ref mut w) => w.{name}(value),\n        Discard(ref mut w) => w.{name}(value),\n    }}\n}}\n")
        };
        let source = format!(
            "{}{}{}{}{}{}",
            routed("typed_get"),
            routed("typed_post"),
            rows("write_json", "Json"),
            rows("write_text", "Text"),
            delegate("set_color"),
            delegate("set_hyperlink")
        );
        let file = make_file(&root, "src/lib.rs", &source);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        // Halves of one row table, back to back in one function.
        let halves = (0..4)
            .map(|index| format!("        (Kind::Request, {index}) => {{\n            let via = format_ident!(\"via_{index}\");\n            let span = via.span();\n            quote_spanned! {{ span => #rejection: From<<#via<T> as Extract<#generics>>::Rejection> }}\n        }},\n"))
            .collect::<String>();
        let table = make_file(
            &root,
            "src/table.rs",
            &format!("fn bounds(kind: Kind, index: usize) -> TokenStream {{\n    match (kind, index) {{\n{halves}        _ => quote! {{}},\n    }}\n}}\n"),
        );
        assert!(detect(std::slice::from_ref(&table)).is_empty());
        // The same function in two Cargo packages, and in one.
        std::fs::create_dir_all(root.join("first")).unwrap();
        std::fs::create_dir_all(root.join("second")).unwrap();
        std::fs::write(root.join("first/Cargo.toml"), "").unwrap();
        std::fs::write(root.join("second/Cargo.toml"), "").unwrap();
        let first = make_file(&root, "first/src/a.rs", &clone_source("first", "value", 2));
        let second = make_file(
            &root,
            "second/src/b.rs",
            &clone_source("second", "scaled", 7),
        );
        assert!(detect(&[first.clone(), second]).is_empty());
        let sibling = make_file(&root, "first/src/c.rs", &clone_source("third", "scaled", 7));
        assert_eq!(detect(&[first, sibling]).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn write_file(
        root: &std::path::Path,
        relative: &str,
        language: Language,
        source: &str,
    ) -> crate::AuditedFile {
        let absolute = root.join(relative);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(&absolute, source).unwrap();
        crate::AuditedFile {
            absolute,
            relative: relative.into(),
            language,
            bytes: source.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    #[test]
    fn enum_state_handlers_and_locale_files_are_not_clones() {
        let root = temp_root("states-and-locales");
        let handler = |name: &str| {
            format!("    {name} {{\n        @Override void read(Tokeniser t, CharacterReader r) {{\n            char c = r.consume();\n            switch (c) {{\n                case '\"':\n                    t.transition(AfterIdentifier);\n                    break;\n                case '>':\n                    t.error(this);\n                    t.emitPending();\n                    t.transition(Data);\n                    break;\n                default:\n                    t.pending.append(c);\n            }}\n        }}\n    }},\n")
        };
        let states = format!("enum TokeniserState {{\n{}{};\n    abstract void read(Tokeniser t, CharacterReader r);\n}}\n", handler("PublicId"), handler("SystemId"));
        let file = write_file(&root, "src/TokeniserState.java", Language::Java, &states);
        assert!(detect(std::slice::from_ref(&file)).is_empty());
        let plain = states
            .replace("enum TokeniserState", "class TokeniserState")
            .replace("    PublicId {\n", "    class PublicId {\n")
            .replace("    SystemId {\n", "    class SystemId {\n")
            .replace("    },\n", "    }\n")
            .replace("};\n    abstract", "\n    abstract");
        let file = write_file(&root, "src/Plain.java", Language::Java, &plain);
        assert_eq!(detect(std::slice::from_ref(&file)).len(), 1, "{plain}");
        let messages = "export default function messages(issue) {\n  switch (issue.code) {\n    case \"too_big\": {\n      const adj = issue.inclusive ? \"<=\" : \"<\";\n      const sizing = getSizing(issue.origin);\n      if (sizing) {\n        return `${issue.origin} ${adj} ${issue.maximum.toString()} ${sizing.unit}`;\n      }\n      return `${issue.origin} ${adj} ${issue.maximum.toString()}`;\n    }\n    default:\n      return `invalid ${issue.origin}`;\n  }\n}\n";
        let locales = [
            write_file(&root, "src/locales/de.ts", Language::TypeScript, messages),
            write_file(&root, "src/locales/fr.ts", Language::TypeScript, messages),
        ];
        assert!(detect(&locales).is_empty());
        let modules = [
            write_file(&root, "src/errors/de.ts", Language::TypeScript, messages),
            write_file(&root, "src/errors/fr.ts", Language::TypeScript, messages),
        ];
        assert_eq!(detect(&modules).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn locals_renamed_inside_a_clone_are_not_parameters() {
        let root = temp_root("renamed-locals");
        let first = make_file(&root, "src/first.rs", &clone_source("first", "value", 2));
        let second = make_file(&root, "src/second.rs", &clone_source("second", "scaled", 7));
        let findings = detect(&[first, second]);
        assert_eq!(
            findings[0].evidence.get("differing_bindings"),
            Some(&auditor_core::EvidenceValue::Integer(1))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rolling_hash_matches_full_recomputation() {
        let hashes = ROLLING_HASH_VALUES;
        let current = hash_window(hashes[..4].iter().copied());
        let rolled = roll(current, hashes[0], hashes[4], base_pow(3));
        assert_eq!(rolled, hash_window(hashes[1..5].iter().copied()));
    }
}
