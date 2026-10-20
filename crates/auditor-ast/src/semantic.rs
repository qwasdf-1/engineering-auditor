use std::sync::Arc;

use crate::{AstLang, AstNode, AstTree};

#[path = "semantic_controls.rs"]
mod controls;
use controls::{control_kind, is_control_branch, nesting_increment};
#[path = "semantic_fields.rs"]
mod fields;
use fields::{assignment_fields, first_field, loop_field, symbol_name};
#[path = "semantic_symbols.rs"]
mod symbols;
use symbols::{in_anonymous_function, symbol_kind};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl SourceSpan {
    pub fn from_node(node: AstNode<'_>) -> Self {
        let (start_line, start_column) = node.start_position();
        let (end_line, end_column) = node.end_position();
        Self {
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_line: start_line + 1,
            start_column: start_column + 1,
            end_line: end_line + 1,
            end_column: end_column + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Object,
    Struct,
    Union,
    Interface,
    Trait,
    Enum,
    Module,
    Constant,
    Static,
    TypeAlias,
    Variable,
    Parameter,
    Field,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SymbolFact {
    pub kind: SymbolKind,
    pub span: SourceSpan,
    pub name: Option<SourceSpan>,
    pub parent: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReferenceKind {
    Read,
    Call,
    Import,
    Type,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReferenceFact {
    pub kind: ReferenceKind,
    pub span: SourceSpan,
    pub owner: Option<u32>,
    pub control_region: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CallFact {
    pub span: SourceSpan,
    pub callee: SourceSpan,
    pub receiver: Option<SourceSpan>,
    pub arguments: Option<SourceSpan>,
    pub owner: Option<u32>,
    pub control_region: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssignmentFact {
    pub span: SourceSpan,
    pub target: Option<SourceSpan>,
    pub value: Option<SourceSpan>,
    pub owner: Option<u32>,
    pub control_region: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ControlRegionKind {
    Branch,
    Conditional,
    Loop,
    Match,
    Try,
    Catch,
    Async,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ControlRegionFact {
    pub kind: ControlRegionKind,
    pub span: SourceSpan,
    pub condition: Option<SourceSpan>,
    pub parent: Option<u32>,
    pub owner: Option<u32>,
    pub nesting_increment: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LoopFact {
    pub span: SourceSpan,
    pub binding: Option<SourceSpan>,
    pub iterable: Option<SourceSpan>,
    pub control_region: u32,
    pub owner: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImportFact {
    pub span: SourceSpan,
    pub path: SourceSpan,
    pub owner: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LiteralFact {
    pub span: SourceSpan,
    pub owner: Option<u32>,
    pub control_region: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReturnFact {
    pub span: SourceSpan,
    pub owner: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StatementFact {
    pub span: SourceSpan,
    pub owner: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticFileFacts {
    pub parse_has_errors: bool,
    pub symbols: Arc<[SymbolFact]>,
    pub references: Arc<[ReferenceFact]>,
    pub calls: Arc<[CallFact]>,
    pub assignments: Arc<[AssignmentFact]>,
    pub control_regions: Arc<[ControlRegionFact]>,
    pub loops: Arc<[LoopFact]>,
    pub imports: Arc<[ImportFact]>,
    pub literals: Arc<[LiteralFact]>,
    pub returns: Arc<[ReturnFact]>,
    pub statements: Arc<[StatementFact]>,
}

#[derive(Default)]
struct FactsBuilder {
    symbols: Vec<SymbolFact>,
    references: Vec<ReferenceFact>,
    calls: Vec<CallFact>,
    assignments: Vec<AssignmentFact>,
    control_regions: Vec<ControlRegionFact>,
    loops: Vec<LoopFact>,
    imports: Vec<ImportFact>,
    literals: Vec<LiteralFact>,
    returns: Vec<ReturnFact>,
    statements: Vec<StatementFact>,
}

#[derive(Clone, Copy)]
struct NodeKind<'a>(&'a str);

#[derive(Clone, Copy)]
struct TraversalContext {
    owner: Option<u32>,
    control: Option<u32>,
}

impl<'a> NodeKind<'a> {
    fn of(node: AstNode<'a>) -> Self {
        Self(node.kind())
    }

    fn is(self, kinds: &[&str]) -> bool {
        kinds.contains(&self.0)
    }
}

pub(crate) fn extract_semantic_facts(tree: &AstTree) -> SemanticFileFacts {
    let root = tree.root_node();
    let parse_has_errors = root.has_error();
    let mut builder = FactsBuilder::default();
    walk(root, None, None, &mut builder);
    SemanticFileFacts {
        parse_has_errors,
        symbols: builder.symbols.into(),
        references: builder.references.into(),
        calls: builder.calls.into(),
        assignments: builder.assignments.into(),
        control_regions: builder.control_regions.into(),
        loops: builder.loops.into(),
        imports: builder.imports.into(),
        literals: builder.literals.into(),
        returns: builder.returns.into(),
        statements: builder.statements.into(),
    }
}

fn walk(
    node: AstNode<'_>,
    parent_symbol: Option<u32>,
    parent_control: Option<u32>,
    builder: &mut FactsBuilder,
) {
    let span = SourceSpan::from_node(node);
    let kind = NodeKind::of(node);
    let owner = record_symbol(node, kind, span, parent_symbol, builder);
    let parent = TraversalContext {
        owner,
        control: parent_control,
    };
    let control = record_control(node, kind, span, parent, builder);
    let context = TraversalContext { owner, control };
    record_operations(node, kind, span, context, builder);
    record_leaf(kind, span, context, builder);
    for child in node.named_children() {
        walk(child, owner, control, builder);
    }
}

fn record_symbol(
    node: AstNode<'_>,
    node_kind: NodeKind<'_>,
    span: SourceSpan,
    parent: Option<u32>,
    builder: &mut FactsBuilder,
) -> Option<u32> {
    let Some(kind) = symbol_kind(node, node_kind) else {
        return parent;
    };
    // A Go parameter named inside a shared declaration spans that whole
    // declaration, so consumers still read its type (`a, b int`).
    let span = match node.parent() {
        Some(declaration)
            if kind == SymbolKind::Parameter
                && node_kind.is(IDENTIFIER_KINDS)
                && node.language() == AstLang::Go =>
        {
            SourceSpan::from_node(declaration)
        }
        _ => span,
    };
    let index = builder.symbols.len() as u32;
    builder.symbols.push(SymbolFact {
        kind,
        span,
        name: symbol_name(node),
        parent,
    });
    Some(index)
}

fn record_control(
    node: AstNode<'_>,
    node_kind: NodeKind<'_>,
    span: SourceSpan,
    context: TraversalContext,
    builder: &mut FactsBuilder,
) -> Option<u32> {
    let mut parent = context.control;
    if is_control_branch(node) {
        let index = builder.control_regions.len() as u32;
        builder.control_regions.push(ControlRegionFact {
            kind: ControlRegionKind::Branch,
            span,
            condition: None,
            parent,
            owner: context.owner,
            nesting_increment: false,
        });
        parent = Some(index);
    }
    let Some(kind) = control_kind(node_kind.0) else {
        return parent;
    };
    let index = builder.control_regions.len() as u32;
    builder.control_regions.push(ControlRegionFact {
        kind,
        span,
        condition: first_field(node, &["condition", "predicate"]),
        parent,
        owner: context.owner,
        nesting_increment: nesting_increment(node, kind),
    });
    if kind == ControlRegionKind::Loop {
        builder.loops.push(LoopFact {
            span,
            binding: loop_field(node, &["left", "pattern", "variable", "name"]),
            iterable: loop_field(node, &["right", "value", "iterable", "condition"]),
            control_region: index,
            owner: context.owner,
        });
    }
    Some(index)
}

fn record_operations(
    node: AstNode<'_>,
    node_kind: NodeKind<'_>,
    span: SourceSpan,
    context: TraversalContext,
    builder: &mut FactsBuilder,
) {
    if node_kind.is(CALL_KINDS) {
        let callee = first_field(
            node,
            &["function", "callee", "constructor", "name", "method"],
        )
        .or_else(|| node.named_child(0).map(SourceSpan::from_node))
        .unwrap_or(span);
        builder.calls.push(CallFact {
            span,
            callee,
            receiver: first_field(node, &["object", "receiver"]),
            arguments: first_field(node, &["arguments", "argument_list"]),
            owner: context.owner,
            control_region: context.control,
        });
    }
    if node_kind.is(ASSIGNMENT_KINDS) {
        let (target, value) = assignment_fields(node);
        builder.assignments.push(AssignmentFact {
            span,
            target,
            value,
            owner: context.owner,
            control_region: context.control,
        });
    }
    if node_kind.is(IMPORT_KINDS) {
        let path = first_field(node, &["path", "source", "module", "name"]).unwrap_or(span);
        builder.imports.push(ImportFact {
            span,
            path,
            owner: context.owner,
        });
    }
    // A lambda's or closure's `return` leaves the lambda, not its owner.
    if node_kind.is(RETURN_KINDS) && !in_anonymous_function(node) {
        builder.returns.push(ReturnFact {
            span,
            owner: context.owner,
        });
    }
    if node_kind.is(STATEMENT_KINDS) {
        builder.statements.push(StatementFact {
            span,
            owner: context.owner,
        });
    }
}

fn record_leaf(
    node_kind: NodeKind<'_>,
    span: SourceSpan,
    context: TraversalContext,
    builder: &mut FactsBuilder,
) {
    if node_kind.is(IDENTIFIER_KINDS) {
        builder.references.push(ReferenceFact {
            kind: ReferenceKind::Read,
            span,
            owner: context.owner,
            control_region: context.control,
        });
    } else if node_kind.is(LITERAL_KINDS) {
        builder.literals.push(LiteralFact {
            span,
            owner: context.owner,
            control_region: context.control,
        });
    }
}

const CALL_KINDS: &[&str] = &[
    "call_expression",
    "call",
    "method_invocation",
    "invocation_expression",
    "new_expression",
    "command",
    "macro_invocation",
];
const ASSIGNMENT_KINDS: &[&str] = &[
    "assignment",
    "assignment_expression",
    "assignment_statement",
    "augmented_assignment",
    "augmented_assignment_expression",
    "compound_assignment_expr",
    "dec_statement",
    "inc_statement",
    "let_declaration",
    "property_declaration",
    "short_var_declaration",
    "update_expression",
    "variable_declarator",
];
const IMPORT_KINDS: &[&str] = &[
    "use_declaration",
    "import_statement",
    "import_from_statement",
    "import_declaration",
    "import_header",
    "using_directive",
    "preproc_include",
    "require",
];
const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "simple_identifier",
    "constant",
    "word",
];
const LITERAL_KINDS: &[&str] = &[
    "string",
    "string_literal",
    "raw_string_literal",
    "integer",
    "integer_literal",
    "float",
    "float_literal",
    "true",
    "false",
    "null",
    "none",
];
const RETURN_KINDS: &[&str] = &["return", "return_expression", "return_statement"];
const STATEMENT_KINDS: &[&str] = &[
    "expression_statement",
    "assignment_statement",
    "let_declaration",
    "lexical_declaration",
    "local_variable_declaration",
    "variable_declaration",
    "short_var_declaration",
    "return",
    "return_expression",
    "return_statement",
    "break_expression",
    "break_statement",
    "continue_expression",
    "continue_statement",
    "throw_expression",
    "throw_statement",
    "yield",
    "yield_expression",
    "yield_statement",
    "defer_statement",
];

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod tests;
