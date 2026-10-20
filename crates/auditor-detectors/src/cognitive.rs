use auditor_core::Finding;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::AuditedFile;

// ── Complexity thresholds ──

const BRANCH_BUDGET: usize = 10;
const BRANCH_MEDIUM_SEVERITY: usize = 30;
/// Sonar S3776's default: a function is flagged above this score.
const COGNITIVE_BUDGET: usize = 15;
const NESTING_ADVISORY: usize = 6;
const COGNITIVE_VIOLATION: usize = 30;
const MIN_PARSEABLE_TEXT_LEN: usize = 20;
const NPATH_BUDGET: u128 = 200;
const NPATH_MILLION_DISPLAY: usize = 1_000_000;
const NPATH_MEDIUM_SEVERITY: u128 = 100_000;
const HALSTEAD_BUDGET: usize = 1500;
const HALSTEAD_DIFFICULTY_BUDGET: f64 = 30.0;
const HALSTEAD_VIOLATION: usize = 5000;
/// Volume alone over budget needs at least this difficulty: large functions
/// with low difficulty are flat data tables, markup, or message maps.
const HALSTEAD_VOLUME_MIN_DIFFICULTY: f64 = 15.0;

type ComplexityFindingBuilder =
    fn(&auditor_ast::FunctionComplexity, &auditor_ast::FunctionSpan, &str) -> Option<Finding>;

pub(crate) struct ComplexityIndex {
    files: Vec<ComplexityFileFacts>,
    branch_sum_by_file: HashMap<PathBuf, usize>,
}

struct ComplexityFileFacts {
    rel: String,
    spans: std::sync::Arc<[auditor_ast::FunctionSpan]>,
    metrics: Vec<(usize, auditor_ast::FunctionComplexity)>,
}

impl ComplexityIndex {
    pub(crate) fn build(
        files: &[AuditedFile],
        cache: &crate::util::FileCache,
        repo_ast: &mut auditor_ast::RepoAst,
    ) -> Self {
        let candidates: Vec<_> = files
            .iter()
            .filter_map(|file| complexity_source(file, cache).map(|input| (file, input)))
            .collect();
        repo_ast.parse_missing_shared(
            candidates
                .iter()
                .map(|(file, (language, source))| {
                    (
                        file.absolute.clone(),
                        *language,
                        std::sync::Arc::clone(source),
                    )
                })
                .collect(),
        );
        let repo_ast = &*repo_ast;
        let per_file = auditor_core::parallel_map(&candidates, |(file, _)| {
            file_facts(file, cache, repo_ast.get(&file.absolute)?)
        });
        let mut complexity_files = Vec::new();
        let mut branch_sum_by_file = HashMap::new();
        for ((file, _), facts) in candidates.iter().zip(per_file) {
            let Some(facts) = facts else {
                continue;
            };
            let branch_sum = facts
                .metrics
                .iter()
                .map(|(_, metrics)| metrics.branch_count + 1)
                .sum::<usize>();
            if branch_sum > 0 {
                branch_sum_by_file.insert(file.relative.clone(), branch_sum);
            }
            if !facts.metrics.is_empty() {
                complexity_files.push(facts);
            }
        }
        Self {
            files: complexity_files,
            branch_sum_by_file,
        }
    }

    pub(crate) fn branch_sum(&self, path: &Path) -> Option<usize> {
        self.branch_sum_by_file.get(path).copied()
    }
}

fn findings_from_index(
    index: &ComplexityIndex,
    finding_for: ComplexityFindingBuilder,
) -> Vec<Finding> {
    index
        .files
        .iter()
        .flat_map(|file| {
            file.metrics.iter().filter_map(|(span_index, metrics)| {
                finding_for(metrics, &file.spans[*span_index], &file.rel)
            })
        })
        .collect()
}

fn function_complexity_finding(
    metrics: &auditor_ast::FunctionComplexity,
    span: &auditor_ast::FunctionSpan,
    rel: &str,
) -> Option<Finding> {
    (metrics.branch_count > BRANCH_BUDGET).then(|| {
        crate::structural::function_complexity_finding(
            "Function exceeds branch complexity budget",
            format!(
                "Function `{}` has {} branch decision points.",
                span.name, metrics.branch_count
            ),
            if metrics.branch_count >= BRANCH_MEDIUM_SEVERITY {
                auditor_core::Severity::Medium
            } else {
                auditor_core::Severity::Low
            },
            auditor_core::Confidence::High,
            rel,
            Some(span.start_line),
        )
        .with_recommendation(
            "Reduce the number of decision points by splitting or simplifying logic.",
        )
        .with_evidence("branch_count", metrics.branch_count)
        .with_evidence("function", span.name.clone())
        .with_evidence("source", "ast")
    })
}

fn cognitive_complexity_finding(
    metrics: &auditor_ast::FunctionComplexity,
    span: &auditor_ast::FunctionSpan,
    rel: &str,
) -> Option<Finding> {
    if metrics.cognitive <= COGNITIVE_BUDGET && metrics.nesting_depth < NESTING_ADVISORY {
        return None;
    }
    let tier = if metrics.cognitive >= COGNITIVE_VIOLATION {
        "violation"
    } else {
        "advisory"
    };
    let message = if metrics.cognitive > COGNITIVE_BUDGET {
        format!(
            "Function `{}` has cognitive complexity {}, nesting depth {}.",
            span.name, metrics.cognitive, metrics.nesting_depth
        )
    } else {
        format!(
            "Function `{}` has nesting depth {} (>=6).",
            span.name, metrics.nesting_depth
        )
    };
    let breakdown = &metrics.cognitive_breakdown;
    let detail = format!(
        "structural:{} hybrid:{} fundamental:{} nesting_increments:{} recursion:{}",
        breakdown.structural,
        breakdown.hybrid,
        breakdown.fundamental,
        breakdown.nesting_increments,
        breakdown.recursion,
    );
    Some(
        auditor_core::finding!(
            "cognitive_complexity",
            "structural",
            "Function cognitive complexity exceeds budget",
            format!("{message}. Breakdown: {detail}"),
            if metrics.cognitive >= COGNITIVE_VIOLATION {
                auditor_core::Severity::Medium
            } else {
                auditor_core::Severity::Low
            },
            auditor_core::Confidence::High,
            rel,
            Some(span.start_line),
        )
        .with_recommendation(
            "Reduce nesting depth and simplify control flow to lower cognitive complexity.",
        )
        .with_evidence("cognitive_complexity", metrics.cognitive)
        .with_evidence("nesting_depth", metrics.nesting_depth)
        .with_evidence("function", span.name.clone())
        .with_evidence("tier", tier)
        .with_evidence("source", "ast")
        .with_evidence("structural", breakdown.structural)
        .with_evidence("hybrid", breakdown.hybrid)
        .with_evidence("fundamental", breakdown.fundamental)
        .with_evidence("nesting_increments", breakdown.nesting_increments)
        .with_evidence("recursion", breakdown.recursion),
    )
}

fn npath_complexity_finding(
    metrics: &auditor_ast::FunctionComplexity,
    span: &auditor_ast::FunctionSpan,
    rel: &str,
) -> Option<Finding> {
    if metrics.npath <= NPATH_BUDGET {
        return None;
    }
    let npath = usize::try_from(metrics.npath).unwrap_or(usize::MAX);
    let display = if npath > NPATH_MILLION_DISPLAY {
        format!("{:.1}M", npath as f64 / NPATH_MILLION_DISPLAY as f64)
    } else {
        npath.to_string()
    };
    Some(
        auditor_core::finding!(
            "npath_complexity",
            "structural",
            "Function NPath complexity exceeds budget",
            format!("Function `{}` has NPath complexity {display}.", span.name),
            if metrics.npath >= NPATH_MEDIUM_SEVERITY {
                auditor_core::Severity::Medium
            } else {
                auditor_core::Severity::Low
            },
            auditor_core::Confidence::Medium,
            rel,
            Some(span.start_line),
        )
        .with_recommendation("Reduce combinatorial path explosion by flattening conditionals.")
        .with_evidence("npath", npath)
        .with_evidence("function", span.name.clone())
        .with_evidence("source", "ast"),
    )
}

fn halstead_information_density_finding(
    metrics: &auditor_ast::FunctionComplexity,
    span: &auditor_ast::FunctionSpan,
    rel: &str,
) -> Option<Finding> {
    let volume = metrics.halstead.volume() as usize;
    let difficulty = metrics.halstead.difficulty();
    let dense_volume = volume > HALSTEAD_BUDGET && difficulty >= HALSTEAD_VOLUME_MIN_DIFFICULTY;
    if !dense_volume && difficulty <= HALSTEAD_DIFFICULTY_BUDGET {
        return None;
    }
    let tier = if volume > HALSTEAD_VIOLATION {
        "violation"
    } else {
        "advisory"
    };
    Some(
        crate::structural::halstead_information_density_finding(
            "Function Halstead information density exceeds budget",
            format!(
                "Function `{}` has Halstead volume {volume} and difficulty {difficulty:.1}.",
                span.name
            ),
            if volume > HALSTEAD_VIOLATION {
                auditor_core::Severity::Medium
            } else {
                auditor_core::Severity::Low
            },
            auditor_core::Confidence::Medium,
            rel,
            Some(span.start_line),
        )
        .with_recommendation("Simplify the function by reducing operator/operand variety.")
        .with_evidence("halstead_volume", volume)
        .with_evidence("halstead_difficulty", difficulty)
        .with_evidence("function", span.name.clone())
        .with_evidence("tier", tier)
        .with_evidence("source", "ast"),
    )
}

pub(crate) fn function_complexity_ast_findings(index: &ComplexityIndex) -> Vec<Finding> {
    findings_from_index(index, function_complexity_finding)
}

pub(crate) fn cognitive_complexity_ast_findings(index: &ComplexityIndex) -> Vec<Finding> {
    findings_from_index(index, cognitive_complexity_finding)
}

pub(crate) fn npath_complexity_ast_findings(index: &ComplexityIndex) -> Vec<Finding> {
    findings_from_index(index, npath_complexity_finding)
}

pub(crate) fn halstead_information_density_ast_findings(index: &ComplexityIndex) -> Vec<Finding> {
    findings_from_index(index, halstead_information_density_finding)
}

/// The grammar and source of a parseable, non-test source file.
fn complexity_source(
    file: &AuditedFile,
    cache: &crate::util::FileCache,
) -> Option<(auditor_ast::AstLang, std::sync::Arc<str>)> {
    if !file.language.is_source() || crate::util::is_test_source_path(&file.relative) {
        return None;
    }
    let ast_lang = crate::util::map_language_to_ast(&file.language)?;
    let source = cache
        .shared_text(file)
        .filter(|source| source.len() >= MIN_PARSEABLE_TEXT_LEN)?;
    Some((ast_lang, source))
}

/// The complexity of each non-test function in `tree`; caches the file's
/// function spans on the way.
fn file_facts(
    file: &AuditedFile,
    cache: &crate::util::FileCache,
    tree: &auditor_ast::AstTree,
) -> Option<ComplexityFileFacts> {
    let functions = tree.function_spans();
    cache.cache_function_spans(file, functions.clone());
    let mut function_nodes = function_nodes_by_span(tree);
    let metrics = functions
        .iter()
        .enumerate()
        .filter_map(|(span_index, span)| {
            let function_node = function_nodes
                .remove(&(span.start_line, span.end_line, span.name.clone()))
                .map(|(_, node)| node)?;
            let in_test = file.language == crate::Language::Rust
                && crate::ast_scan::is_inside_rust_test(function_node);
            (!in_test).then(|| {
                (
                    span_index,
                    auditor_ast::function_complexity(&function_node, &span.name),
                )
            })
        })
        .collect();
    Some(ComplexityFileFacts {
        rel: crate::util::path_slash(&file.relative),
        spans: functions,
        metrics,
    })
}

/// Every named function node in `tree`, keyed by 1-based start line, end line,
/// and declared name. When several nodes share a key, the one whose kind comes
/// first in `function_node_kinds`, then first in document order, wins. One
/// walk serves every span in the file.
fn function_nodes_by_span(
    tree: &auditor_ast::AstTree,
) -> HashMap<(usize, usize, String), (usize, auditor_ast::AstNode<'_>)> {
    let kinds = auditor_ast::function_node_kinds();
    let mut nodes = HashMap::new();
    let mut pending: Vec<_> = tree.root_node().children().collect();
    pending.reverse();
    while let Some(node) = pending.pop() {
        let priority = kinds.iter().position(|kind| *kind == node.kind());
        if let Some((priority, name)) = priority
            .filter(|_| node.is_named())
            .and_then(|priority| Some((priority, node.declared_name()?)))
        {
            let key = (node.start_position().0 + 1, node.end_position().0 + 1, name);
            nodes
                .entry(key)
                .and_modify(|entry: &mut (usize, auditor_ast::AstNode<'_>)| {
                    if priority < entry.0 {
                        *entry = (priority, node);
                    }
                })
                .or_insert((priority, node));
        }
        let first_child = pending.len();
        pending.extend(node.children());
        pending[first_child..].reverse();
    }
    nodes
}

#[cfg(test)]
mod tests {
    use crate::Language;

    #[test]
    fn deep_nesting_triggers_cognitive_ast() {
        let source = "\
fn main() {
    if a {
        if b {
            if c {
                if d {
                    if e {
                        if f {
                            println!(\"deep\");
                        }
                    }
                }
            }
        }
    }
}
";
        let score = compute_cognitive_for_fn(source, "main", &Language::Rust);
        assert!(score >= 21, "deep nesting should score >= 21, got {score}");
    }

    #[test]
    fn boolean_operators_increase_score_ast() {
        let source = "\
fn main() {
    if a && b && c && d && e {
        if f {
            if g {
                if h {
                    if i {
                        println!(\"deep\");
                    }
                }
            }
        }
    }
}
";
        let score = compute_cognitive_for_fn(source, "main", &Language::Rust);
        // 4 booleans (&&) = +4, outer if = +1 (nest 0), inner ifs: +2+3+4+5 = +14
        // total = 4 + 1 + 2 + 3 + 4 + 5 = 19
        assert!(
            score >= 15,
            "booleans + nesting should score >= 15, got {score}"
        );
    }

    #[test]
    fn flat_function_is_silent_ast() {
        let source = "\
fn main() {
    println!(\"hello\");
}
";
        let score = compute_cognitive_for_fn(source, "main", &Language::Rust);
        assert_eq!(score, 0, "flat function should score 0, got {score}");
    }

    #[test]
    fn else_if_does_not_increment_nesting_as_much() {
        // else if should not increment nesting for the else part
        let source = "\
fn main() {
    if a {
        if b {
            println!(\"both\");
        }
    } else if c {
        println!(\"c\");
    }
}
";
        let score = compute_cognitive_for_fn(source, "main", &Language::Rust);
        // outer if: 1+0 = 1, nest=1
        // inner if: 1+1 = 2, nest=2, total=3
        // else if c: else adds +1 (flat), or since it wraps an if_expression,
        //   the else itself doesn't add, but the inner if: 1+1=2, total=5
        assert!(
            score >= 3,
            "else if should have reasonable score, got {score}"
        );
        assert!(score <= 6, "else if should not over-nest, got {score}");
    }

    #[test]
    fn npath_exceeds_budget_with_many_if_chains() {
        let source = "\
fn count(x: i32) -> i32 {
    let mut n = 0;
    if x == 1 { n += 1; }
    if x == 2 { n += 1; }
    if x == 3 { n += 1; }
    if x == 4 { n += 1; }
    if x == 5 { n += 1; }
    if x == 6 { n += 1; }
    if x == 7 { n += 1; }
    if x == 8 { n += 1; }
    if x == 9 { n += 1; }
    if x == 10 { n += 1; }
    if x == 11 { n += 1; }
    n
}
";
        let lang = Language::Rust;
        let ast_lang = crate::util::map_language_to_ast(&lang).unwrap();
        let mut parser = auditor_ast::AstParser::new().unwrap();
        let tree = parser.parse(&ast_lang, source).unwrap();
        let functions = auditor_ast::extract_functions(&tree, &ast_lang);
        let func = functions.iter().find(|f| f.name == "count").unwrap();
        let root = tree.root_node();
        for kind in auditor_ast::function_node_kinds() {
            let ak = auditor_ast::AstNodeKind::from(*kind);
            for node in root.descendants_of_kind(&ak) {
                let (sr, _) = node.start_position();
                let (er, _) = node.end_position();
                if sr + 1 == func.start_line && er + 1 == func.end_line {
                    let has_name = node.named_children().any(|c| {
                        matches!(c.kind(), "name" | "identifier" | "word") && c.text() == "count"
                    });
                    if has_name {
                        let fc = auditor_ast::function_complexity(&node, "count");
                        assert!(
                            fc.npath > 200,
                            "npath {} should exceed budget 200",
                            fc.npath
                        );
                        return;
                    }
                }
            }
        }
        panic!("function count not found in AST");
    }

    fn compute_cognitive_for_fn(source: &str, fn_name: &str, lang: &Language) -> usize {
        let ast_lang = crate::util::map_language_to_ast(lang).unwrap();
        let mut parser = auditor_ast::AstParser::new().unwrap();
        let tree = parser.parse(&ast_lang, source).unwrap();
        let functions = auditor_ast::extract_functions(&tree, &ast_lang);
        let func = functions.iter().find(|f| f.name == fn_name).unwrap();
        let root = tree.root_node();
        // Find the AST node for this function
        for kind in auditor_ast::function_node_kinds() {
            let ak = auditor_ast::AstNodeKind::from(*kind);
            for node in root.descendants_of_kind(&ak) {
                let (sr, _) = node.start_position();
                let (er, _) = node.end_position();
                if sr + 1 == func.start_line && er + 1 == func.end_line {
                    // Check name
                    let has_name = node.named_children().any(|c| {
                        matches!(c.kind(), "name" | "identifier" | "word") && c.text() == fn_name
                    });
                    if has_name {
                        return auditor_ast::cognitive_complexity(&node, fn_name).score;
                    }
                }
            }
        }
        panic!("function {fn_name} not found in AST");
    }
}
