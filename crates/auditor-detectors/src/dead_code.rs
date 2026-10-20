#[cfg(test)]
use std::collections::HashSet;
use std::collections::{BTreeSet, HashMap};

use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::AstScan;
use crate::dead_code_semantic;
use crate::util;
use crate::{AnalysisMethod, AuditedFile, Language};

pub(super) const MIN_NAME_LENGTH: usize = 4;

const COMMON_NAMES: &[&str] = &[
    "run", "main", "setup", "init", "start", "stop", "get", "set", "update", "create", "delete",
    "load", "save", "process", "execute", "handle", "build", "parse", "check", "validate",
];

const DYNAMIC_LANGUAGES: &[Language] = &[
    Language::Python,
    Language::JavaScript,
    Language::TypeScript,
    Language::Ruby,
];

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SymbolDef {
    pub(super) name: String,
    pub(super) kind: &'static str,
    pub(super) file: String,
    pub(super) line: usize,
    pub(super) language: Language,
}

#[cfg(test)]
pub(crate) fn dead_code_findings(files: &[AuditedFile], cache: &util::FileCache) -> Vec<Finding> {
    let mut repo_ast = auditor_ast::RepoAst::new();
    dead_code_findings_with_context(files, files, AnalysisMethod::Ast, cache, &mut repo_ast)
}

pub(crate) fn dead_code_findings_with_context(
    emission_files: &[AuditedFile],
    context_files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut auditor_ast::RepoAst,
) -> Vec<Finding> {
    let definitions = dead_code_semantic::collect_definitions(AstScan::new(
        emission_files,
        method,
        cache,
        repo_ast,
    ));
    let references = dead_code_semantic::reference_counts(
        &definitions,
        AstScan::context(context_files, method, cache, repo_ast),
    );
    find_unreferenced(definitions, &references, "dead_code", "correctness")
}

#[cfg(test)]
fn orphan_symbol_findings(files: &[AuditedFile], cache: &util::FileCache) -> Vec<Finding> {
    orphan_symbol_findings_with_context(files, files, cache)
}

#[cfg(test)]
fn orphan_symbol_findings_with_context(
    emission_files: &[AuditedFile],
    context_files: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let definitions: Vec<_> = collect_definitions(emission_files, cache, false, true)
        .into_iter()
        .filter(|definition| {
            matches!(
                definition.kind,
                "constant" | "static" | "type" | "union" | "variable"
            )
        })
        .collect();
    let references = source_reference_counts(&definitions, context_files, cache);
    find_unreferenced(definitions, &references, "orphan_symbol", "structural")
}

#[cfg(test)]
fn collect_definitions(
    files: &[AuditedFile],
    cache: &util::FileCache,
    only_functions_and_classes: bool,
    internal_only: bool,
) -> Vec<SymbolDef> {
    let mut definitions = Vec::new();
    for file in files {
        if !file.language.is_source() || util::is_test_source_path(&file.relative) {
            continue;
        }
        let rel = util::path_slash(&file.relative);
        let lines = cache.lines(file);
        let trait_lines = rust_trait_lines(&file.language, &lines);
        for (line_number, line) in lines.iter().enumerate() {
            let code = util::strip_comments(line);
            let trimmed = code.trim();
            if trimmed.is_empty() {
                continue;
            }
            if is_test_definition(&file.language, &lines, line_number) {
                continue;
            }
            let defs = extract_definitions(&file.language, trimmed, line_number, &rel);
            for def in defs {
                if def.kind == "function" && (trimmed.ends_with(';') || trait_lines[line_number]) {
                    continue;
                }
                if only_functions_and_classes
                    && !matches!(
                        def.kind,
                        "function" | "class" | "struct" | "enum" | "trait" | "interface"
                    )
                {
                    continue;
                }
                if def.name.len() < MIN_NAME_LENGTH {
                    continue;
                }
                if internal_only && is_externally_visible(&file.language, trimmed, &def.name) {
                    continue;
                }
                definitions.push(def);
            }
        }
    }
    definitions
}

#[cfg(test)]
fn rust_trait_lines(language: &Language, lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    if !matches!(language, Language::Rust) {
        return mask;
    }
    let mut brace_depth = 0isize;
    let mut trait_depth = None;
    let mut pending_trait = false;
    for (index, line) in lines.iter().enumerate() {
        let code = util::strip_comments(line);
        let trimmed = code.trim();
        if trait_depth.is_some() {
            mask[index] = true;
        }
        if trait_depth.is_none()
            && (trimmed.starts_with("trait ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("pub(crate) trait ")
                || trimmed.starts_with("unsafe trait ")
                || trimmed.starts_with("pub unsafe trait "))
        {
            pending_trait = true;
        }
        let opens = trimmed
            .chars()
            .filter(|character| *character == '{')
            .count() as isize;
        let closes = trimmed
            .chars()
            .filter(|character| *character == '}')
            .count() as isize;
        if pending_trait && opens > 0 {
            trait_depth = Some(brace_depth + 1);
            mask[index] = true;
            pending_trait = false;
        }
        brace_depth += opens - closes;
        if trait_depth.is_some_and(|depth| brace_depth < depth) {
            trait_depth = None;
        }
    }
    mask
}

#[cfg(test)]
fn is_test_definition(language: &Language, lines: &[&str], line_number: usize) -> bool {
    if !matches!(language, Language::Rust) {
        return false;
    }
    lines[..line_number]
        .iter()
        .rev()
        .map(|line| line.trim())
        .take_while(|line| line.is_empty() || line.starts_with("#["))
        .any(|line| line == "#[test]" || line.ends_with("::test]"))
}

#[cfg(test)]
fn is_externally_visible(language: &Language, line: &str, name: &str) -> bool {
    match language {
        Language::Rust => line.starts_with("pub ") || line.starts_with("pub("),
        Language::JavaScript | Language::TypeScript => line.starts_with("export "),
        Language::Go => name.chars().next().is_some_and(char::is_uppercase),
        Language::Python | Language::Ruby => !name.starts_with('_'),
        Language::Java | Language::Kotlin | Language::CSharp => !line.starts_with("private "),
        Language::C | Language::Cpp => !line.starts_with("static "),
        _ => false,
    }
}

#[cfg(test)]
fn extract_definitions(
    language: &Language,
    line: &str,
    line_number: usize,
    rel: &str,
) -> Vec<SymbolDef> {
    match language {
        Language::Rust => prefixed_definitions(
            language,
            line,
            line_number,
            rel,
            &[
                ("fn ", "function"),
                ("pub fn ", "function"),
                ("pub(crate) fn ", "function"),
                ("pub(super) fn ", "function"),
                ("async fn ", "function"),
                ("pub async fn ", "function"),
                ("struct ", "struct"),
                ("pub struct ", "struct"),
                ("enum ", "enum"),
                ("pub enum ", "enum"),
                ("trait ", "trait"),
                ("pub trait ", "trait"),
                ("type ", "type"),
                ("pub type ", "type"),
                ("mod ", "mod"),
                ("pub mod ", "mod"),
                ("union ", "union"),
                ("pub union ", "union"),
                ("const ", "constant"),
                ("pub const ", "constant"),
                ("static ", "static"),
                ("pub static ", "static"),
            ],
        ),
        Language::Python => prefixed_definitions(
            language,
            line,
            line_number,
            rel,
            &[
                ("def ", "function"),
                ("async def ", "function"),
                ("class ", "class"),
            ],
        ),
        Language::Go => go_definitions(language, line, line_number, rel),
        Language::JavaScript | Language::TypeScript => {
            javascript_definitions(language, line, line_number, rel)
        }
        Language::Java => prefixed_definitions(
            language,
            line,
            line_number,
            rel,
            &[("class ", "class"), ("interface ", "interface")],
        ),
        Language::Cpp => prefixed_definitions(
            language,
            line,
            line_number,
            rel,
            &[
                ("class ", "class"),
                ("struct ", "struct"),
                ("enum ", "enum"),
            ],
        ),
        Language::Ruby => prefixed_definitions(
            language,
            line,
            line_number,
            rel,
            &[
                ("def ", "function"),
                ("class ", "class"),
                ("module ", "module"),
            ],
        ),
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn prefixed_definitions(
    language: &Language,
    line: &str,
    line_number: usize,
    rel: &str,
    prefixes: &[(&str, &'static str)],
) -> Vec<SymbolDef> {
    prefixes
        .iter()
        .filter_map(|(prefix, kind)| {
            let name = line
                .strip_prefix(prefix)
                .and_then(util::identifier_prefix)?;
            Some(symbol_definition(language, name, kind, line_number, rel))
        })
        .collect()
}

#[cfg(test)]
fn go_definitions(
    language: &Language,
    line: &str,
    line_number: usize,
    rel: &str,
) -> Vec<SymbolDef> {
    let mut definitions = prefixed_definitions(
        language,
        line,
        line_number,
        rel,
        &[
            ("type ", "type"),
            ("struct ", "struct"),
            ("interface ", "interface"),
            ("const ", "constant"),
            ("var ", "variable"),
        ],
    );
    if let Some(name) = go_function_name(line) {
        definitions.push(symbol_definition(
            language,
            name,
            "function",
            line_number,
            rel,
        ));
    }
    definitions
}

#[cfg(test)]
fn javascript_definitions(
    language: &Language,
    line: &str,
    line_number: usize,
    rel: &str,
) -> Vec<SymbolDef> {
    let mut definitions =
        prefixed_definitions(language, line, line_number, rel, &[("class ", "class")]);
    if let Some(name) = js_function_name(line) {
        definitions.push(symbol_definition(
            language,
            name,
            "function",
            line_number,
            rel,
        ));
    }
    definitions
}

#[cfg(test)]
fn symbol_definition(
    language: &Language,
    name: String,
    kind: &'static str,
    line_number: usize,
    rel: &str,
) -> SymbolDef {
    SymbolDef {
        name,
        kind,
        file: rel.to_string(),
        line: line_number + 1,
        language: language.clone(),
    }
}

#[cfg(test)]
fn go_function_name(line: &str) -> Option<String> {
    let line = line.trim_start();
    let rest = if let Some(val) = line.strip_prefix("func ") {
        val
    } else {
        line.strip_prefix("pub func ")?
    };
    let after_receiver = if rest.starts_with('(') {
        let close = rest.find(')')?;
        rest[close + 1..].trim_start()
    } else {
        rest
    };
    util::identifier_prefix(after_receiver)
}

#[cfg(test)]
fn js_function_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("export async function ")
        .or_else(|| trimmed.strip_prefix("export function "))
        .or_else(|| trimmed.strip_prefix("async function "))
        .or_else(|| trimmed.strip_prefix("function "));
    if let Some(val) = rest {
        return util::identifier_prefix(val);
    }
    for prefix in ["const ", "let ", "var "] {
        if let Some(val) = trimmed.strip_prefix(prefix) {
            let name = util::identifier_prefix(val)?;
            if val.contains("=>") {
                return Some(name);
            }
        }
    }
    None
}

fn find_unreferenced(
    definitions: Vec<SymbolDef>,
    references: &HashMap<String, usize>,
    detector_id: &str,
    category: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for def in &definitions {
        let key = format!("{}:{}", def.name, def.file);
        if !seen.insert(key) {
            continue;
        }
        let source_refs = references
            .get(&def.name)
            .copied()
            .unwrap_or(0)
            .saturating_sub(1);
        if source_refs > 0 {
            continue;
        }
        let (confidence, caveats) = compute_confidence(&def.name, &def.language);
        if confidence == Confidence::Low {
            continue;
        }
        let line = def.line;
        let message = format!(
            "Unreferenced {} `{}` defined at line {} has zero source references beyond its definition",
            def.kind, def.name, line
        );
        let mut finding = auditor_core::finding!(
            detector_id,
            category,
            "Unreferenced symbol",
            message,
            Severity::Medium,
            confidence,
            &def.file,
            Some(line),
        )
        .with_recommendation(format!(
            "Verify that `{}` is intended for external use or remove it if dead code.",
            def.name
        ))
        .with_evidence("symbol", def.name.clone())
        .with_evidence("symbol_kind", def.kind)
        .with_evidence("source_refs", "0");
        for caveat in &caveats {
            finding = finding.with_evidence("caveat", caveat.clone());
        }
        findings.push(finding);
    }
    findings
}

#[cfg(test)]
fn source_reference_counts(
    definitions: &[SymbolDef],
    files: &[AuditedFile],
    cache: &util::FileCache,
) -> HashMap<String, usize> {
    let names = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<HashSet<_>>();
    let mut occurrences = HashMap::new();
    for file in files {
        if !file.language.is_source() {
            continue;
        }
        for line in cache.lines(file) {
            for identifier in
                line.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            {
                if names.contains(identifier) {
                    *occurrences.entry(identifier.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    occurrences
}

#[cfg(test)]
fn count_identifier_occurrences(text: &str, name: &str) -> usize {
    let mut count = 0usize;
    let mut start = 0usize;
    while let Some(pos) = text[start..].find(name) {
        let abs_pos = start + pos;
        let before = abs_pos.saturating_sub(1);
        let after = abs_pos + name.len();
        let is_boundary_before = abs_pos == 0
            || !text.as_bytes()[before].is_ascii_alphanumeric() && text.as_bytes()[before] != b'_';
        let is_boundary_after = after >= text.len()
            || !text.as_bytes()[after].is_ascii_alphanumeric() && text.as_bytes()[after] != b'_';
        if is_boundary_before && is_boundary_after {
            count += 1;
        }
        start = abs_pos + name.len();
    }
    count
}

const BASE_CONFIDENCE_SCORE: i32 = 3;
const MEDIUM_NAME_LENGTH: usize = 7;
const SHORT_NAME_PENALTY: i32 = 2;
const COMMON_NAME_PENALTY: i32 = 2;
const HIGH_CONFIDENCE_THRESHOLD: i32 = 3;

fn compute_confidence(name: &str, language: &Language) -> (Confidence, Vec<String>) {
    let mut caveats = Vec::new();
    let mut score: i32 = BASE_CONFIDENCE_SCORE;
    // rustc's convention for an item kept on purpose without callers, such
    // as a compile-time `fn _assert_send_sync()` check.
    if *language == Language::Rust && name.starts_with('_') {
        return (
            Confidence::Low,
            vec!["underscore-prefixed Rust item — unused by design".to_string()],
        );
    }
    if name.starts_with("__") && name.ends_with("__") {
        return (
            Confidence::Low,
            vec!["dunder method — may be invoked by runtime, not visible in source".to_string()],
        );
    }
    if name.len() <= MIN_NAME_LENGTH {
        caveats.push(format!(
            "short name ({} chars) — high false-positive risk",
            name.len()
        ));
        score -= SHORT_NAME_PENALTY;
    } else if name.len() <= MEDIUM_NAME_LENGTH {
        caveats.push(format!(
            "short-medium name ({} chars) — may match unrelated symbols",
            name.len()
        ));
        score -= 1;
    }
    if COMMON_NAMES.contains(&name) {
        caveats.push(format!(
            "common name '{}' — likely to match unrelated identifiers",
            name
        ));
        score -= COMMON_NAME_PENALTY;
    }
    if DYNAMIC_LANGUAGES.contains(language) {
        caveats.push(format!(
            "{}: reflection and dynamic dispatch cannot be detected statically",
            language.as_str()
        ));
        score -= 1;
    }
    let confidence = if score >= HIGH_CONFIDENCE_THRESHOLD {
        Confidence::High
    } else if score >= 1 {
        Confidence::Medium
    } else {
        Confidence::Low
    };
    (confidence, caveats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-deadcode-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_file(root: &std::path::Path, rel: &str, text: &str) -> AuditedFile {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        let language = if rel.ends_with(".rs") {
            Language::Rust
        } else if rel.ends_with(".py") {
            Language::Python
        } else if rel.ends_with(".go") {
            Language::Go
        } else if rel.ends_with(".js") {
            Language::JavaScript
        } else {
            Language::Rust
        };
        AuditedFile {
            absolute: path,
            relative: std::path::PathBuf::from(rel),
            language,
            bytes: text.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    fn test_cache(files: &[AuditedFile]) -> util::FileCache {
        util::FileCache::new(files)
    }

    #[test]
    fn reference_index_preserves_identifier_boundary_counts() {
        let root = temp_root("reference_index");
        let file = make_file(
            &root,
            "src/lib.rs",
            "fn helper_name() {}\n// helper_name\nconst NOTE: &str = \"helper_name\";\nfn helper_name_extra() {}\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let definitions = collect_definitions(std::slice::from_ref(&file), &cache, true, true);
        let counts = source_reference_counts(&definitions, std::slice::from_ref(&file), &cache);
        let text = cache.text(&file);
        assert_eq!(
            counts.get("helper_name").copied().unwrap_or(0),
            count_identifier_occurrences(text, "helper_name")
        );
        assert_eq!(counts.get("helper_name"), Some(&3));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rust_unreferenced_function_detected() {
        let root = temp_root("rust_unref");
        let caller = make_file(
            &root,
            "src/main.rs",
            "fn main() {}\nfn unused_helper() {}\n",
        );
        let cache = test_cache(std::slice::from_ref(&caller));
        let findings = dead_code_findings(&[caller], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "dead_code" && f.message.contains("Unreferenced")),
            "expected dead_code finding for a private unreferenced function"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rust_function_referenced_in_same_file_is_silent() {
        let root = temp_root("rust_same_file_ref");
        let file = make_file(
            &root,
            "src/main.rs",
            "fn main() { helper(); }\nfn helper() {}\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let findings = dead_code_findings(std::slice::from_ref(&file), &cache);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.message.contains("helper")),
            "a same-file call is a real source reference"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rust_function_referenced_in_other_file_is_silent() {
        let root = temp_root("rust_ref");
        let lib = make_file(&root, "src/lib.rs", "pub fn helper() -> i32 { 42 }\n");
        let main_text = "use crate::helper;\nfn main() { let x = helper(); }\n";
        let main = make_file(&root, "src/main.rs", main_text);
        let files = [lib, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(
            !findings.iter().any(|f| f.detector == "dead_code"),
            "expected no dead_code finding when fn is referenced across files"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dead_code_does_not_claim_python_support() {
        let root = temp_root("py_unref");
        let file = make_file(
            &root,
            "src/module.py",
            "def used(): pass\ndef _unused_fn(): pass\n",
        );
        let main = make_file(&root, "src/main.py", "from module import used\nused()\n");
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dead_code_does_not_claim_go_support() {
        let root = temp_root("go_unref");
        let file = make_file(
            &root,
            "src/helper.go",
            "package main\nfunc helper() {}\nfunc unusedFn() {}\n",
        );
        let main = make_file(
            &root,
            "src/main.go",
            "package main\nfunc main() { helper() }\n",
        );
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn orphan_symbol_detects_variables() {
        let root = temp_root("orphan_var");
        let file = make_file(
            &root,
            "src/config.rs",
            "const DB_NAME: &str = \"myapp\";\nstatic MAX_RETRIES: usize = 3;\n",
        );
        let main = make_file(&root, "src/main.rs", "fn main() {}\n");
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = orphan_symbol_findings(&files, &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "orphan_symbol" && f.message.contains("DB_NAME")),
            "expected orphan_symbol finding for unreferenced const"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "orphan_symbol" && f.message.contains("MAX_RETRIES")),
            "expected orphan_symbol finding for unreferenced static"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn short_names_are_filtered() {
        let root = temp_root("short_name");
        let file = make_file(&root, "src/lib.rs", "fn x() {}\nfn abc() {}\n");
        let main = make_file(&root, "src/main.rs", "fn main() {}\n");
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(
            !findings
                .iter()
                .any(|f| f.message.contains("`x`") && f.detector == "dead_code"),
            "expected no dead_code finding for 1-char name"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn self_reference_is_a_source_reference() {
        let root = temp_root("self_ref");
        let file = make_file(
            &root,
            "src/lib.rs",
            "pub fn process_data() { process_data(); }\n",
        );
        let main = make_file(&root, "src/main.rs", "fn main() {}\n");
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(
            !findings.iter().any(|f| f.detector == "dead_code"),
            "the high-precision detector does not infer reachability from a call graph"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dead_code_does_not_claim_python_class_support() {
        let root = temp_root("py_class");
        let file = make_file(
            &root,
            "src/models.py",
            "class _UnusedModel:\n    pass\nclass UsedModel:\n    pass\n",
        );
        let main = make_file(
            &root,
            "src/app.py",
            "from models import UsedModel\nm = UsedModel()\n",
        );
        let files = [file, main];
        let cache = test_cache(&files);
        let findings = dead_code_findings(&files, &cache);
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dead_code_and_orphan_symbol_own_disjoint_symbol_kinds() {
        let root = temp_root("disjoint_owners");
        let file = make_file(
            &root,
            "src/lib.rs",
            "fn unused_helper() {}\nconst UNUSED_VALUE: usize = 3;\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let dead = dead_code_findings(std::slice::from_ref(&file), &cache);
        let orphan = orphan_symbol_findings(std::slice::from_ref(&file), &cache);
        assert!(dead
            .iter()
            .any(|finding| finding.message.contains("unused_helper")));
        assert!(!orphan
            .iter()
            .any(|finding| finding.message.contains("unused_helper")));
        assert!(orphan
            .iter()
            .any(|finding| finding.message.contains("UNUSED_VALUE")));
        assert!(!dead
            .iter()
            .any(|finding| finding.message.contains("UNUSED_VALUE")));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn trait_methods_are_contract_members_not_dead_functions() {
        let root = temp_root("trait_contract");
        let file = make_file(
            &root,
            "src/lib.rs",
            "pub trait ProviderApi {\n    fn stream_items(&self);\n    fn index_key(&self) -> usize { 1 }\n}\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let findings = dead_code_findings(std::slice::from_ref(&file), &cache);
        assert!(!findings.iter().any(|finding| {
            finding.message.contains("stream_items") || finding.message.contains("index_key")
        }));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn serde_default_attributes_are_semantic_references() {
        let root = temp_root("serde_default_reference");
        let file = make_file(
            &root,
            "src/lib.rs",
            "struct Config {\n    #[serde(default = \"default_enabled\")]\n    enabled: bool,\n}\nfn default_enabled() -> bool { true }\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let findings = dead_code_findings(std::slice::from_ref(&file), &cache);
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("default_enabled")));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn trait_impl_methods_are_contract_members_not_dead_functions() {
        let root = temp_root("trait_impl_contract");
        let file = make_file(
            &root,
            "src/lib.rs",
            "struct Value;\nimpl serde::Serialize for Value {\n    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> { todo!() }\n}\n",
        );
        let cache = test_cache(std::slice::from_ref(&file));
        let findings = dead_code_findings(std::slice::from_ref(&file), &cache);
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("serialize")));
        fs::remove_dir_all(&root).unwrap();
    }
}
