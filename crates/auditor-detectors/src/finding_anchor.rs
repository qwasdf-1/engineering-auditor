//! Finding-ID v3 anchor derivation. Every owner's findings pass through here
//! once, so detectors do not hand-roll line-free identity: the scan binds the
//! enclosing qualified symbol (or a normalized line digest), a line-content
//! subject for statement-level findings, and an occurrence ordinal.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use auditor_ast::{SymbolFact, SymbolKind};
use auditor_core::{
    normalized_line_digest, Finding, FindingAnchor, IDENTITY_LINE_CONTENT_KEY, IDENTITY_ORDINAL_KEY,
};

use crate::{util, AnalysisMethod, AuditedFile};

pub(crate) struct AnchorResolver<'a> {
    files: HashMap<&'a Path, &'a AuditedFile>,
}

impl<'a> AnchorResolver<'a> {
    pub(crate) fn new(files: &'a [AuditedFile]) -> Self {
        Self {
            files: files
                .iter()
                .map(|file| (file.relative.as_path(), file))
                .collect(),
        }
    }

    pub(crate) fn anchor_findings(
        &self,
        findings: Vec<Finding>,
        repo_ast: &mut auditor_ast::RepoAst,
        cache: &util::FileCache,
        method: AnalysisMethod,
    ) -> Vec<Finding> {
        // Provisional IDs are line-keyed, so equal IDs here are exact repeats;
        // drop them before ordinals would make them distinct.
        let mut seen = std::collections::HashSet::new();
        let anchored = findings
            .into_iter()
            .filter(|finding| seen.insert(finding.id.clone()))
            .map(|finding| self.anchor(finding, repo_ast, cache, method))
            .collect();
        assign_ordinals(anchored)
    }

    fn anchor(
        &self,
        finding: Finding,
        repo_ast: &mut auditor_ast::RepoAst,
        cache: &util::FileCache,
        method: AnalysisMethod,
    ) -> Finding {
        if finding.anchor().is_some() {
            return finding;
        }
        let (Some(line), Some(file)) = (
            finding.primary_line(),
            self.files.get(Path::new(finding.primary_path())),
        ) else {
            return finding;
        };
        let Some(text) = cache.lines(file).get(line.wrapping_sub(1)).copied() else {
            return finding;
        };
        let digest = normalized_line_digest(text);
        match enclosing_symbol(file, line, repo_ast, cache, method) {
            Some(symbol) => {
                let has_subject = !finding.identity.is_empty();
                let finding = finding.with_anchor(FindingAnchor::Symbol(symbol.path));
                if symbol.start_line == line || has_subject {
                    finding
                } else {
                    finding.with_derived_identity(IDENTITY_LINE_CONTENT_KEY, digest)
                }
            }
            None => finding.with_anchor(FindingAnchor::Content(digest)),
        }
    }
}

struct EnclosingSymbol {
    path: String,
    start_line: usize,
}

fn enclosing_symbol(
    file: &AuditedFile,
    line: usize,
    repo_ast: &mut auditor_ast::RepoAst,
    cache: &util::FileCache,
    method: AnalysisMethod,
) -> Option<EnclosingSymbol> {
    if method == AnalysisMethod::Token {
        return None;
    }
    let language = util::map_language_to_ast(&file.language)?;
    let source = cache.shared_text(file)?;
    let tree = repo_ast
        .get_or_parse_shared(&file.absolute, &language, source)
        .ok()?;
    let facts = tree.semantic_facts();
    let symbols = &facts.symbols;
    let innermost = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| {
            is_anchor_owner(symbol)
                && symbol.span.start_line <= line
                && line <= symbol.span.end_line
        })
        .min_by_key(|(_, symbol)| symbol.span.end_byte - symbol.span.start_byte)?;
    let mut segments = Vec::new();
    let mut cursor = Some(innermost.0);
    let mut outermost = &symbols[innermost.0];
    while let Some(index) = cursor {
        let symbol = &symbols[index];
        if is_anchor_owner(symbol) {
            segments.push(symbol_segment(tree, symbol));
            outermost = symbol;
        }
        cursor = symbol.parent.map(|parent| parent as usize);
    }
    segments.extend(method_owner(tree, outermost));
    segments.reverse();
    Some(EnclosingSymbol {
        path: segments.join("::"),
        start_line: innermost.1.span.start_line,
    })
}

/// Declarations that own code. Locals, parameters, and fields are excluded:
/// they are too fine-grained and would win "innermost" on a declaration line.
/// An unnamed module is the file itself, which the primary path already keys.
fn is_anchor_owner(symbol: &SymbolFact) -> bool {
    match symbol.kind {
        SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::Field => false,
        SymbolKind::Module => symbol.name.is_some(),
        _ => true,
    }
}

/// Rust and Go methods are not children of a symbol, so semantic facts do not
/// record their owner. Qualify them with it, so same-named methods on
/// different types stay distinct.
fn method_owner(tree: &auditor_ast::AstTree, symbol: &SymbolFact) -> Option<String> {
    match tree.root_node().language() {
        auditor_ast::AstLang::Rust
            if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) =>
        {
            rust_impl_owner(tree, symbol)
        }
        auditor_ast::AstLang::Go if symbol.kind == SymbolKind::Method => {
            go_receiver_owner(tree, symbol)
        }
        _ => None,
    }
}

/// The owner of a Go method is its receiver's base type. Pointer and type
/// parameters are dropped: Go forbids one method name on both `T` and `*T`,
/// so switching receiver form must not re-key the finding.
fn go_receiver_owner(tree: &auditor_ast::AstTree, symbol: &SymbolFact) -> Option<String> {
    let start = symbol.span.start_byte;
    let method = tree
        .root_node()
        .descendants_of_kind(&auditor_ast::AstNodeKind::from("method_declaration"))
        .into_iter()
        .find(|method| method.start_byte() == start)?;
    let receiver = method.child_by_field_name("receiver")?.named_child(0)?;
    let base = receiver
        .child_by_field_name("type")?
        .text()
        .trim_start_matches(|character: char| character == '*' || character.is_whitespace());
    let base = base.split('[').next()?.trim();
    (!base.is_empty()).then(|| base.to_string())
}

/// The owner of a Rust method is the enclosing `impl` block's self type (and
/// trait).
fn rust_impl_owner(tree: &auditor_ast::AstTree, symbol: &SymbolFact) -> Option<String> {
    let start = symbol.span.start_byte;
    let block = tree
        .root_node()
        .descendants_of_kind(&auditor_ast::AstNodeKind::from("impl_item"))
        .into_iter()
        .filter(|block| block.start_byte() <= start && start < block.end_byte())
        .min_by_key(|block| block.end_byte() - block.start_byte())?;
    let normalized = |node: auditor_ast::AstNode<'_>| {
        node.text().split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let self_type = normalized(block.child_by_field_name("type")?);
    Some(match block.child_by_field_name("trait") {
        Some(trait_name) => format!("<{self_type} as {}>", normalized(trait_name)),
        None => self_type,
    })
}

fn symbol_segment(tree: &auditor_ast::AstTree, symbol: &SymbolFact) -> String {
    symbol
        .name
        .map(|name| tree.text_for_span(name).to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("<{:?}>", symbol.kind).to_ascii_lowercase())
}

/// Number findings whose identity inputs are otherwise identical, in source
/// order. The first occurrence keeps ordinal zero and no ordinal key.
fn assign_ordinals(findings: Vec<Finding>) -> Vec<Finding> {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, finding) in findings.iter().enumerate() {
        groups.entry(finding.id.clone()).or_default().push(index);
    }
    let mut ordinals = vec![0usize; findings.len()];
    for indexes in groups.values_mut().filter(|indexes| indexes.len() > 1) {
        indexes.sort_by_key(|&index| (findings[index].primary_line(), index));
        for (ordinal, &index) in indexes.iter().enumerate() {
            ordinals[index] = ordinal;
        }
    }
    findings
        .into_iter()
        .zip(ordinals)
        .map(|(finding, ordinal)| match ordinal {
            0 => finding,
            ordinal => finding.with_derived_identity(IDENTITY_ORDINAL_KEY, ordinal.to_string()),
        })
        .collect()
}
