//! The overload set a function definition belongs to: definitions that share
//! a name in one scope, whatever their parameter types or template arguments.

use auditor_ast::AstNode;

use crate::Language;

/// Syntax nodes that open a named scope without being a `Module` symbol:
/// C++ `namespace`, TypeScript `namespace` / `module`.
const NAMESPACE_KINDS: &[&str] = &["namespace_definition", "internal_module", "module"];

/// The enclosing namespaces and declared name of a function (`a::b::f`), the
/// same string for every overload, specialisation, and template of one name;
/// empty when it has no name. `node` is the function's declaration and
/// `name` the name the symbol table reads from it.
pub(super) fn overload_set(language: &Language, node: Option<AstNode<'_>>, name: &str) -> String {
    let declared = match node.filter(|_| matches!(language, Language::C | Language::Cpp)) {
        Some(node) => c_declared_name(node).unwrap_or_else(|| name.to_string()),
        None => name.to_string(),
    };
    if declared.is_empty() {
        return String::new();
    }
    let mut path = node.map(enclosing_namespaces).unwrap_or_default();
    path.push(declared);
    path.join("::")
}

/// A C or C++ function's declared name as written, keeping what the symbol
/// name drops: an operator's symbol (`operator==`) and the template name of an
/// explicit specialisation (`f<int>` reads as `f`).
fn c_declared_name(node: AstNode<'_>) -> Option<String> {
    let node = match node.kind() {
        "template_declaration" => (0..node.named_child_count())
            .filter_map(|index| node.named_child(index))
            .find(|child| child.kind() == "function_definition")?,
        _ => node,
    };
    let name = declarator_name(node.child_by_field_name("declarator")?)?;
    Some(name.text().split_whitespace().collect())
}

/// The identifier a declarator names, through pointers, references,
/// qualifiers, and template arguments.
fn declarator_name(declarator: AstNode<'_>) -> Option<AstNode<'_>> {
    match declarator.kind() {
        "identifier" | "field_identifier" | "operator_name" | "destructor_name" => Some(declarator),
        "template_function" | "qualified_identifier" => {
            declarator_name(declarator.child_by_field_name("name")?)
        }
        _ => declarator_name(declarator.child_by_field_name("declarator")?),
    }
}

/// Namespace names around `node`, outermost first. An unnamed C++ namespace
/// is one scope per file.
fn enclosing_namespaces(node: AstNode<'_>) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if NAMESPACE_KINDS.contains(&parent.kind()) {
            match parent.child_by_field_name("name") {
                Some(name) => names.push(name.text().split_whitespace().collect()),
                None if parent.kind() == "namespace_definition" => {
                    names.push("{anonymous}".to_string());
                }
                None => {}
            }
        }
        current = parent.parent();
    }
    names.reverse();
    names
}
