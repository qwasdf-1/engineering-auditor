use crate::AstNode;

mod operands;
mod operators;

use operands::is_halstead_operand;
use operators::is_halstead_operator;

#[derive(Debug, Clone, Default)]
pub struct HalsteadMetrics {
    pub unique_operators: usize,
    pub unique_operands: usize,
    pub total_operators: usize,
    pub total_operands: usize,
}

impl HalsteadMetrics {
    pub fn vocabulary(&self) -> usize {
        self.unique_operators + self.unique_operands
    }
    pub fn length(&self) -> usize {
        self.total_operators + self.total_operands
    }
    pub fn volume(&self) -> f64 {
        let n = self.vocabulary() as f64;
        let n_len = self.length() as f64;
        if n < 1.0 {
            return 0.0;
        }
        n_len * n.log2()
    }
    pub fn difficulty(&self) -> f64 {
        let uo2 = self.unique_operators as f64 / 2.0;
        let to2 = self.total_operands as f64;
        if self.unique_operands == 0 {
            return 0.0;
        }
        uo2 * to2 / self.unique_operands as f64
    }
    pub fn effort(&self) -> f64 {
        self.difficulty() * self.volume()
    }
}

/// Compute Halstead metrics from language-specific tree-sitter leaf tokens.
pub fn halstead(node: &AstNode) -> HalsteadMetrics {
    let mut m = HalsteadMetrics::default();
    let mut op_set = std::collections::BTreeSet::new();
    let mut od_set = std::collections::BTreeSet::new();
    halstead_walk(&halstead_body(node), &mut m, &mut op_set, &mut od_set, true);
    m.unique_operators = op_set.len();
    m.unique_operands = od_set.len();
    m
}

/// The function body Halstead counts over; Kotlin's grammar names it by kind,
/// not by field.
pub(super) fn halstead_body<'a>(node: &AstNode<'a>) -> AstNode<'a> {
    node.child_by_field_name("body")
        .or_else(|| {
            node.named_children()
                .find(|child| child.kind() == "function_body")
        })
        .unwrap_or(*node)
}

/// A token Halstead classifies. Kotlin wraps a soft keyword used as a name
/// (`value`, `data`) in a `simple_identifier` over the keyword token; the
/// name is the operand.
pub(super) fn is_halstead_leaf(node: &AstNode) -> bool {
    node.child_count() == 0
        || node.language() == crate::AstLang::Kotlin && node.kind() == "simple_identifier"
}

fn halstead_walk(
    node: &AstNode,
    m: &mut HalsteadMetrics,
    op_set: &mut std::collections::BTreeSet<String>,
    od_set: &mut std::collections::BTreeSet<String>,
    is_root: bool,
) {
    if !is_root && is_halstead_function_node(node.language(), node.kind()) {
        return;
    }
    if is_halstead_leaf(node) {
        record_halstead_leaf(
            node,
            op_set,
            od_set,
            &mut m.total_operators,
            &mut m.total_operands,
        );
        return;
    }
    for child in node.children() {
        halstead_walk(&child, m, op_set, od_set, false);
    }
}

pub(super) fn record_halstead_leaf(
    node: &AstNode,
    op_set: &mut std::collections::BTreeSet<String>,
    od_set: &mut std::collections::BTreeSet<String>,
    total_operators: &mut usize,
    total_operands: &mut usize,
) {
    let kind = node.kind();
    let text = node.text().to_string();
    if is_halstead_operator(node.language(), kind) {
        *total_operators += 1;
        op_set.insert(text);
    } else if is_halstead_operand(node.language(), kind) {
        *total_operands += 1;
        od_set.insert(text);
    }
}

/// The nodes that open a nested function, whose tokens belong to that
/// function rather than the one being measured.
const HALSTEAD_FUNCTION_KINDS: &[(crate::AstLang, &[&str])] = {
    use crate::AstLang;
    const JS: &[&str] = &[
        "function_declaration",
        "function_expression",
        "arrow_function",
        "method_definition",
        "generator_function_declaration",
    ];
    const JVM: &[&str] = &[
        "method_declaration",
        "constructor_declaration",
        "lambda_expression",
    ];
    &[
        (AstLang::Python, &["function_definition", "lambda"]),
        (AstLang::JavaScript, JS),
        (AstLang::TypeScript, JS),
        (
            AstLang::Go,
            &["function_declaration", "method_declaration", "func_literal"],
        ),
        (AstLang::Rust, &["function_item", "closure_expression"]),
        (AstLang::Java, JVM),
        (AstLang::CSharp, JVM),
        (
            AstLang::Kotlin,
            &[
                "function_declaration",
                "lambda_literal",
                "anonymous_function",
            ],
        ),
        (
            AstLang::Julia,
            &[
                "function_definition",
                "arrow_function_expression",
                "do_clause",
            ],
        ),
        (AstLang::C, &["function_definition"]),
        (AstLang::Cpp, &["function_definition", "lambda_expression"]),
        (
            AstLang::Ruby,
            &["method", "singleton_method", "lambda", "do_block", "block"],
        ),
    ]
};

pub(super) fn is_halstead_function_node(language: crate::AstLang, kind: &str) -> bool {
    HALSTEAD_FUNCTION_KINDS
        .iter()
        .find(|(candidate, _)| *candidate == language)
        .is_some_and(|(_, kinds)| kinds.contains(&kind))
}
