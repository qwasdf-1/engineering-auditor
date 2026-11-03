use super::is_halstead_function_node;
use crate::{AstLang, AstNode};

/// JSX nodes whose tokens are markup: tag names, attribute names and values,
/// text, and the punctuation between them.
const JSX_MARKUP_KINDS: &[&str] = &[
    "jsx_element",
    "jsx_opening_element",
    "jsx_closing_element",
    "jsx_self_closing_element",
    "jsx_attribute",
];

/// Whether a leaf is JSX markup rather than code: declarative element and
/// attribute syntax says what to render, not how to compute it. The nearest
/// JSX ancestor decides, so code in an expression container (`{cond && <X/>}`,
/// `attr={expr}`, `{...props}`) counts while the markup nested inside it does
/// not.
pub(super) fn is_jsx_markup(node: &AstNode) -> bool {
    if !matches!(node.language(), AstLang::JavaScript | AstLang::TypeScript) {
        return false;
    }
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        let kind = parent.kind();
        if kind == "jsx_expression" || is_halstead_function_node(parent.language(), kind) {
            return false;
        }
        if JSX_MARKUP_KINDS.contains(&kind) {
            return true;
        }
        ancestor = parent.parent();
    }
    false
}
