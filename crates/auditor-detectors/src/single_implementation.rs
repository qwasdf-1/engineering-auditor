//! `single_implementation_abstraction`: an interface, trait, or abstract
//! class with exactly one implementation anywhere in the repository, test
//! code included. The abstraction is a second declaration to read and keep
//! in step for a variation point nothing uses; the one implementation could
//! stand alone until a second one exists.
//!
//! Only abstractions that are not public are read: a public interface or
//! trait is API that code outside the repository may implement, which the
//! repository cannot see.
//!
//! Implementations are counted across every source role, so a test fake is
//! a second implementation and the abstraction a test seam. An abstraction
//! with no implementation is an extension point for code outside the
//! repository, which this rule does not judge.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{AstNode, RepoAst};
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, nodes_of_kind, AstScan};
use crate::{util, AnalysisMethod, AuditedFile, Language, SourceRole};

#[path = "single_implementation_forms.rs"]
mod forms;

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust | Language::Python | Language::TypeScript | Language::Go | Language::Java
    )
}

/// A declared abstraction.
pub(super) struct Abstraction {
    pub(super) name: String,
    pub(super) kind: &'static str,
    pub(super) line: usize,
    pub(super) public: bool,
    /// Go interface method names; empty for the nominal languages.
    pub(super) methods: Vec<String>,
}

/// What the repository implements, per language.
#[derive(Default)]
pub(super) struct Implementations {
    /// Abstraction name to its implementing type names (or an anonymous
    /// class's location).
    pub(super) by_name: BTreeMap<String, BTreeSet<String>>,
    /// Abstractions implemented generically (a Rust blanket impl), which
    /// cover any number of types.
    pub(super) unbounded: BTreeSet<String>,
    /// Types the repository defines.
    pub(super) types: BTreeSet<String>,
    /// Go: methods declared per receiver type.
    pub(super) methods: BTreeMap<String, BTreeSet<String>>,
}

pub(crate) fn single_implementation_abstraction_findings(
    emission_files: &[AuditedFile],
    context_files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut RepoAst,
) -> Vec<Finding> {
    let mut implementations: BTreeMap<Language, Implementations> = BTreeMap::new();
    let context = AstScan::context(context_files, method, cache, repo_ast);
    ast_scan::visit_ast_files(context, supported, |file, tree, _| {
        let facts = implementations.entry(file.language.clone()).or_default();
        forms::collect_implementations(&file.language, tree.root_node(), facts);
    });
    let mut findings = Vec::new();
    let emission = AstScan::new(emission_files, method, cache, repo_ast);
    ast_scan::visit_ast_files(emission, supported, |file, tree, _| {
        if file.role != SourceRole::Production {
            return;
        }
        let Some(facts) = implementations.get(&file.language) else {
            return;
        };
        let relative = util::path_slash(&file.relative);
        let abstractions = forms::abstractions(&file.language, tree.root_node());
        for abstraction in abstractions
            .into_iter()
            .filter(|abstraction| !abstraction.public)
        {
            if let Some(implementation) = sole_implementation(&abstraction, facts) {
                findings.push(finding(&abstraction, &implementation, &relative));
            }
        }
    });
    findings
}

/// The one implementation of `abstraction`, when there is exactly one.
fn sole_implementation(abstraction: &Abstraction, facts: &Implementations) -> Option<String> {
    if facts.unbounded.contains(&abstraction.name) {
        return None;
    }
    let implementers: Vec<&String> = if abstraction.methods.is_empty() {
        facts
            .by_name
            .get(&abstraction.name)
            .map(|types| types.iter().collect())
            .unwrap_or_default()
    } else {
        facts
            .methods
            .iter()
            .filter(|(_, methods)| {
                abstraction
                    .methods
                    .iter()
                    .all(|method| methods.contains(method))
            })
            .map(|(receiver, _)| receiver)
            .collect()
    };
    let [implementation] = implementers[..] else {
        return None;
    };
    // An implementer with implementations of its own (an abstract base
    // class between the interface and its subclasses) stands for several.
    if facts
        .by_name
        .get(implementation)
        .is_some_and(|below| !below.is_empty())
    {
        return None;
    }
    // A Rust trait implemented only for a type defined elsewhere is an
    // extension trait: the way to add methods to a foreign type.
    if abstraction.kind == "Trait" && !facts.types.contains(implementation) {
        return None;
    }
    Some(implementation.clone())
}

/// The first named child of one of `kinds`, as text.
pub(super) fn child_text<'t>(node: AstNode<'t>, kinds: &[&str]) -> Option<&'t str> {
    node.named_children()
        .find(|child| kinds.contains(&child.kind()))
        .map(|child| child.text())
}

/// The simple name of a possibly qualified or generic type: `Foo` of
/// `a::b::Foo<T>`, `pkg.Foo`, `Foo[int]`.
pub(super) fn simple_name(text: &str) -> String {
    let base = text.split(['<', '[', '(']).next().unwrap_or(text).trim();
    base.rsplit(['.', ':'])
        .next()
        .unwrap_or(base)
        .trim()
        .to_string()
}

pub(super) fn line_of(node: AstNode<'_>) -> usize {
    node.start_position().0 + 1
}

pub(super) fn nodes<'t>(root: AstNode<'t>, kinds: &[&str]) -> Vec<AstNode<'t>> {
    nodes_of_kind(root, kinds)
}

fn finding(abstraction: &Abstraction, implementation: &str, relative: &str) -> Finding {
    let Abstraction {
        name, kind, line, ..
    } = abstraction;
    auditor_core::finding!(
        "single_implementation_abstraction",
        "structural",
        "Single-implementation abstraction",
        format!(
            "{kind} `{name}` at line {line} has one implementation in the repository, `{implementation}`; the abstraction adds a declaration for a variation nothing uses"
        ),
        Severity::Low,
        Confidence::Medium,
        relative,
        Some(*line),
    )
    .with_recommendation(
        "Use the implementation directly and extract the abstraction when a second implementation (or a test double) needs it.",
    )
    .with_identity("abstraction", name.as_str())
    .with_evidence("implementation", implementation)
    .with_evidence(
        "visibility",
        if abstraction.public { "public" } else { "private" },
    )
}
