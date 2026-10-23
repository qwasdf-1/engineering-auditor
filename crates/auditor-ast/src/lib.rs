use std::borrow::Cow;
use std::sync::{Arc, OnceLock};
use tree_sitter::{Node, Parser, Tree};

mod complexity;
mod dataflow;
mod extract;
mod languages;
mod macro_blanking;
mod parse;
mod quarantine;
mod semantic;

pub use complexity::{
    branch_count, cognitive_complexity, function_complexity, halstead, npath_complexity,
    CognitiveBreakdown, FunctionComplexity, HalsteadMetrics,
};
pub use dataflow::{
    call_arguments, call_receiver, Assigned, Binding, BindingKind, CallFlow, Destination,
    FlowCache, FunctionFlow, Origin,
};
pub use extract::{
    can_parse, extract_comments, extract_functions, function_fingerprint, structural_fingerprint,
    FunctionSpan,
};
pub use languages::function_node_kinds;
pub use languages::AstLang;
pub use languages::SUPPORTED_LANGUAGES;
pub use quarantine::{Quarantine, QuarantineReason};
pub use semantic::{
    AssignmentFact, CallFact, ControlRegionFact, ControlRegionKind, ImportFact, LiteralFact,
    LoopFact, ReferenceFact, ReferenceKind, SemanticFileFacts, SourceSpan, SymbolFact, SymbolKind,
};

pub struct AstParser {
    parser: Parser,
}

/// Repository-wide AST cache. Parsed trees are cached by file path and reused
/// across detectors, eliminating redundant parses in a single scan.
pub struct RepoAst {
    trees: std::collections::HashMap<std::path::PathBuf, AstTree>,
}

impl Default for RepoAst {
    fn default() -> Self {
        Self::new()
    }
}

impl RepoAst {
    pub fn new() -> Self {
        Self {
            trees: std::collections::HashMap::new(),
        }
    }

    /// Retrieve a cached tree by file path.
    pub fn get(&self, path: &std::path::Path) -> Option<&AstTree> {
        self.trees.get(path)
    }

    /// Parse and cache a tree, or return the cached one. This compatibility
    /// entry point copies the source into shared storage; scan owners should use
    /// `get_or_parse_shared` when they already own an `Arc<str>`.
    pub fn get_or_parse(
        &mut self,
        path: &std::path::Path,
        lang: &AstLang,
        source: &str,
    ) -> Result<&AstTree, String> {
        self.get_or_parse_shared(path, lang, Arc::<str>::from(source))
    }

    pub fn get_or_parse_shared(
        &mut self,
        path: &std::path::Path,
        lang: &AstLang,
        source: Arc<str>,
    ) -> Result<&AstTree, String> {
        use std::collections::hash_map::Entry;
        match self.trees.entry(path.to_path_buf()) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(e) => {
                let mut parser = AstParser::new()?;
                let parsed = parser.parse_shared(lang, source)?;
                Ok(e.insert(parsed))
            }
        }
    }

    /// Parse every input not yet cached on all cores and cache the trees.
    /// Inputs that fail to parse stay uncached, as with `get_or_parse_shared`.
    pub fn parse_missing_shared(&mut self, inputs: Vec<(std::path::PathBuf, AstLang, Arc<str>)>) {
        let missing: Vec<_> = inputs
            .into_iter()
            .filter(|(path, _, _)| !self.trees.contains_key(path))
            .collect();
        let parsed = auditor_core::parallel_map(&missing, |(_, lang, source)| {
            AstParser::new().and_then(|mut parser| parser.parse_shared(lang, Arc::clone(source)))
        });
        for ((path, _, _), tree) in missing.into_iter().zip(parsed) {
            if let Ok(tree) = tree {
                self.trees.insert(path, tree);
            }
        }
    }
}

pub struct AstTree {
    tree: Tree,
    source: Arc<str>,
    language: AstLang,
    /// Declarations blanked to recover this tree from a source with syntax
    /// errors; empty for a tree of the source as written.
    quarantined: Vec<Quarantine>,
    /// For a tree with syntax errors, the tree recovered without the
    /// declarations holding them, when recovery succeeds.
    recovered: Option<Box<AstTree>>,
    function_spans: OnceLock<Arc<[FunctionSpan]>>,
    semantic_facts: OnceLock<Arc<SemanticFileFacts>>,
}

impl AstTree {
    pub fn root_node(&self) -> AstNode<'_> {
        AstNode {
            node: self.tree.root_node(),
            source: &self.source,
            language: self.language,
        }
    }

    /// The source the tree was parsed from; in a recovered tree, with the
    /// quarantined declarations blanked.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The declarations removed to recover this tree from syntax errors.
    pub fn quarantined(&self) -> &[Quarantine] {
        &self.quarantined
    }

    /// This tree when it has no syntax error, else the tree recovered
    /// without the declarations holding its errors, if recovery succeeded.
    /// Owners that need a whole, error-free tree read this one.
    pub fn error_free(&self) -> Option<&AstTree> {
        if self.tree.root_node().has_error() {
            self.recovered.as_deref()
        } else {
            Some(self)
        }
    }

    pub fn shared_source(&self) -> Arc<str> {
        Arc::clone(&self.source)
    }

    pub fn function_spans(&self) -> Arc<[FunctionSpan]> {
        Arc::clone(
            self.function_spans
                .get_or_init(|| extract::extract_functions_uncached(self).into()),
        )
    }

    pub fn semantic_facts(&self) -> Arc<SemanticFileFacts> {
        Arc::clone(
            self.semantic_facts
                .get_or_init(|| Arc::new(semantic::extract_semantic_facts(self))),
        )
    }

    pub fn text_for_span(&self, span: SourceSpan) -> &str {
        self.source
            .get(span.start_byte..span.end_byte)
            .unwrap_or("")
    }
}

#[derive(Clone, Copy)]
pub struct AstNode<'a> {
    node: Node<'a>,
    source: &'a str,
    language: AstLang,
}

/// Nodes are equal when they are the same node of the same tree.
impl PartialEq for AstNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Eq for AstNode<'_> {}

impl std::fmt::Debug for AstNode<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}@{}..{} {:?}",
            self.kind(),
            self.start_byte(),
            self.end_byte(),
            self.text()
        )
    }
}

impl std::hash::Hash for AstNode<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

impl<'a> AstNode<'a> {
    pub fn kind(&self) -> &'a str {
        self.node.kind()
    }

    pub fn is_named(&self) -> bool {
        self.node.is_named()
    }

    pub fn start_byte(&self) -> usize {
        self.node.start_byte()
    }

    pub fn end_byte(&self) -> usize {
        self.node.end_byte()
    }

    pub fn start_position(&self) -> (usize, usize) {
        let pos = self.node.start_position();
        (pos.row, pos.column)
    }

    pub fn end_position(&self) -> (usize, usize) {
        let pos = self.node.end_position();
        (pos.row, pos.column)
    }

    pub fn text(&self) -> &'a str {
        self.node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    pub fn language(&self) -> AstLang {
        self.language
    }

    pub fn has_error(&self) -> bool {
        self.node.has_error()
    }

    pub fn is_error(&self) -> bool {
        self.node.is_error()
    }

    pub fn child_count(&self) -> usize {
        self.node.child_count()
    }

    pub fn named_child_count(&self) -> usize {
        self.node.named_child_count()
    }

    pub fn child(&self, index: usize) -> Option<AstNode<'a>> {
        self.node.child(index as u32).map(|n| AstNode {
            node: n,
            source: self.source,
            language: self.language,
        })
    }

    pub fn named_child(&self, index: usize) -> Option<AstNode<'a>> {
        self.node.named_child(index as u32).map(|n| AstNode {
            node: n,
            source: self.source,
            language: self.language,
        })
    }

    pub fn child_by_field_name(&self, field: &str) -> Option<AstNode<'a>> {
        self.node.child_by_field_name(field).map(|node| AstNode {
            node,
            source: self.source,
            language: self.language,
        })
    }

    pub fn parent(&self) -> Option<AstNode<'a>> {
        self.node.parent().map(|n| AstNode {
            node: n,
            source: self.source,
            language: self.language,
        })
    }

    pub fn children(&self) -> AstNodeIter<'a> {
        let count = self.node.child_count();
        AstNodeIter {
            node: self.node,
            source: self.source,
            language: self.language,
            index: 0,
            count,
            named: false,
        }
    }

    pub fn named_children(&self) -> AstNodeIter<'a> {
        let count = self.node.named_child_count();
        AstNodeIter {
            node: self.node,
            source: self.source,
            language: self.language,
            index: 0,
            count,
            named: true,
        }
    }

    pub fn descendants_of_kind(&self, kind: &AstNodeKind) -> Vec<AstNode<'a>> {
        let mut cursor = self.node.walk();
        let mut result = Vec::new();
        for child_node in self.node.children(&mut cursor) {
            collect_nodes_of_kind(child_node, self.source, self.language, kind, &mut result);
        }
        result
    }

    pub fn first_descendant_of_kind(&self, kind: &AstNodeKind) -> Option<AstNode<'a>> {
        for child in self.children() {
            if child.kind() == kind.as_str() {
                return Some(child);
            }
            if let Some(found) = child.first_descendant_of_kind(kind) {
                return Some(found);
            }
        }
        None
    }
}

fn collect_nodes_of_kind<'a>(
    node: Node<'a>,
    source: &'a str,
    language: AstLang,
    kind: &AstNodeKind,
    result: &mut Vec<AstNode<'a>>,
) {
    if node.is_named() && node.kind() == kind.as_str() {
        result.push(AstNode {
            node,
            source,
            language,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes_of_kind(child, source, language, kind, result);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstNodeKind {
    ArrowFunction,
    Function,
    FunctionDefinition,
    Kind(Cow<'static, str>),
}

impl AstNodeKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ArrowFunction => "arrow_function",
            Self::Function => "function",
            Self::FunctionDefinition => "function_definition",
            Self::Kind(s) => s,
        }
    }
}

impl<'a> From<&'a str> for AstNodeKind {
    fn from(s: &'a str) -> Self {
        match s {
            "arrow_function" => Self::ArrowFunction,
            "function" => Self::Function,
            "function_definition" => Self::FunctionDefinition,
            _ => Self::Kind(Cow::Owned(s.to_string())),
        }
    }
}

impl<'a> From<&'a String> for AstNodeKind {
    fn from(s: &'a String) -> Self {
        Self::from(s.as_str())
    }
}

pub struct AstNodeIter<'a> {
    node: Node<'a>,
    source: &'a str,
    language: AstLang,
    index: usize,
    count: usize,
    named: bool,
}

impl<'a> Iterator for AstNodeIter<'a> {
    type Item = AstNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let result = if self.named {
            self.node.named_child(self.index as u32)
        } else {
            self.node.child(self.index as u32)
        }
        .map(|n| AstNode {
            node: n,
            source: self.source,
            language: self.language,
        })?;
        self.index += 1;
        Some(result)
    }
}

#[cfg(test)]
mod lib_tests;
