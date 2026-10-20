use super::{NodeKind, SymbolKind, IDENTIFIER_KINDS};
use crate::{languages, AstLang, AstNode};

const PARAMETER_KINDS: &[&str] = &[
    "parameter",
    "parameter_declaration",
    "formal_parameter",
    "required_parameter",
    "optional_parameter",
    "simple_parameter",
    "default_parameter",
    "typed_parameter",
    "typed_default_parameter",
];

const SYMBOL_KIND_GROUPS: &[(&[&str], SymbolKind)] = &[
    (
        &["class_definition", "class_declaration", "class_specifier"],
        SymbolKind::Class,
    ),
    (&["object_declaration"], SymbolKind::Object),
    (
        &["struct_item", "struct_specifier", "struct_declaration"],
        SymbolKind::Struct,
    ),
    (&["union_item", "union_specifier"], SymbolKind::Union),
    (&["interface_declaration"], SymbolKind::Interface),
    (&["trait_item", "trait_declaration"], SymbolKind::Trait),
    (&["enum_item", "enum_declaration"], SymbolKind::Enum),
    (
        &["module", "module_declaration", "mod_item"],
        SymbolKind::Module,
    ),
    (PARAMETER_KINDS, SymbolKind::Parameter),
    (
        &["field_declaration", "field_definition"],
        SymbolKind::Field,
    ),
    (
        &[
            "let_declaration",
            "variable_declaration",
            "lexical_declaration",
            "local_variable_declaration",
            "short_var_declaration",
            "var_declaration",
            "property_declaration",
        ],
        SymbolKind::Variable,
    ),
    (&["const_item", "const_declaration"], SymbolKind::Constant),
    (&["static_item"], SymbolKind::Static),
    (
        &["type_item", "type_alias_declaration"],
        SymbolKind::TypeAlias,
    ),
];

pub(super) fn symbol_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> Option<SymbolKind> {
    let kind = declared_symbol_kind(node, node_kind)?;
    // Go decides its signature parameters in `go_parameter_kind`.
    (kind != SymbolKind::Parameter
        || node.language() == AstLang::Go
        || is_signature_parameter(node))
    .then_some(kind)
}

/// Lambdas and closures that are not function symbols themselves.
const ANONYMOUS_FUNCTION_KINDS: &[&str] = &[
    "lambda",
    "lambda_expression",
    "lambda_literal",
    "anonymous_function",
    "anonymous_method_expression",
    "closure_expression",
    "func_literal",
];

/// Whether the nearest enclosing function-like node is a lambda or closure.
pub(super) fn in_anonymous_function(node: AstNode<'_>) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if languages::function_node_kinds().contains(&ancestor.kind()) {
            return false;
        }
        if ANONYMOUS_FUNCTION_KINDS.contains(&ancestor.kind()) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

/// A parameter of the nearest named function's own signature: not one of a
/// lambda inside it (`(ctx, token) => ..`), nor one inside a function-pointer
/// type, whether a local (`int (*fp)(int, int);`) or another parameter's.
fn is_signature_parameter(node: AstNode<'_>) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        let kind = ancestor.kind();
        if languages::function_node_kinds().contains(&kind) {
            return true;
        }
        if ANONYMOUS_FUNCTION_KINDS.contains(&kind)
            || PARAMETER_KINDS.contains(&kind)
            || matches!(
                kind,
                "compound_statement"
                    | "declaration"
                    | "field_declaration"
                    | "type_definition"
                    // `where F: FnMut(Match)`: a type's parameters, not the function's.
                    | "function_type"
                    | "where_clause"
                    | "type_parameters"
                    | "type_arguments"
            )
        {
            return false;
        }
        current = ancestor.parent();
    }
    true
}

/// Kotlin `limit: Int = length`: a default value sits in the parameter list
/// beside its parameter, after the `=`.
fn is_default_value(node: AstNode<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .children()
            .take_while(|child| child.start_byte() < node.start_byte())
            .last()
            .is_some_and(|previous| previous.kind() == "=")
    })
}

fn declared_symbol_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> Option<SymbolKind> {
    if let Some(kind) = go_declared_kind(node, node_kind) {
        return kind;
    }
    if is_non_declaration(node, node_kind) {
        return None;
    }
    // A JavaScript class expression (`module.exports = class Store {}`).
    if node_kind.0 == "class"
        && matches!(node.language(), AstLang::JavaScript | AstLang::TypeScript)
    {
        return Some(SymbolKind::Class);
    }
    if languages::function_node_kinds().contains(&node_kind.0) {
        return Some(function_kind(node, node_kind));
    }
    if is_identifier_parameter(node, node_kind) {
        return Some(SymbolKind::Parameter);
    }
    SYMBOL_KIND_GROUPS
        .iter()
        .find_map(|(kinds, kind)| kinds.contains(&node_kind.0).then_some(*kind))
}

/// Go decides its parameters and type declarations itself.
fn go_declared_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> Option<Option<SymbolKind>> {
    if node.language() != AstLang::Go {
        return None;
    }
    go_parameter_kind(node, node_kind).or_else(|| go_type_kind(node, node_kind).map(Some))
}

/// Declaration-shaped nodes that declare nothing: a foreign JavaScript
/// signature's parameter; `*args: T` and `**kwargs: T` (pylint and ruff skip
/// them, like their untyped forms); a C++ `class Name;` forward declaration.
fn is_non_declaration(node: AstNode<'_>, node_kind: NodeKind<'_>) -> bool {
    (matches!(node.language(), AstLang::JavaScript | AstLang::TypeScript)
        && is_foreign_javascript_parameter(node))
        || (node_kind.0 == "typed_parameter"
            && node.named_child(0).is_some_and(|first| {
                matches!(
                    first.kind(),
                    "list_splat_pattern" | "dictionary_splat_pattern"
                )
            }))
        || (node_kind.0 == "class_specifier" && node.child_by_field_name("body").is_none())
}

fn function_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> SymbolKind {
    if node_kind.0.contains("method")
        || node_kind.0 == "singleton_method"
        || is_associated_function(node)
    {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    }
}

fn is_identifier_parameter(node: AstNode<'_>, node_kind: NodeKind<'_>) -> bool {
    node_kind.is(IDENTIFIER_KINDS)
        && node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "parameters"
                    | "formal_parameters"
                    | "function_value_parameters"
                    | "lambda_parameters"
            )
        })
        && !is_default_value(node)
}

/// A Rust `fn` in an `impl` or `trait` body, or a C++ definition of a
/// qualified name (`int Block::Compare() {}`), is a method of its type.
fn is_associated_function(node: AstNode<'_>) -> bool {
    match node.language() {
        AstLang::Rust => node
            .parent()
            .filter(|body| body.kind() == "declaration_list")
            .and_then(|body| body.parent())
            .is_some_and(|owner| matches!(owner.kind(), "impl_item" | "trait_item")),
        AstLang::Cpp => {
            let mut declarator = node.child_by_field_name("declarator");
            while let Some(current) = declarator {
                if current.kind() == "function_declarator" {
                    return current
                        .child_by_field_name("declarator")
                        .is_some_and(|name| name.kind() == "qualified_identifier");
                }
                declarator = current.child_by_field_name("declarator");
            }
            false
        }
        _ => false,
    }
}

/// Go spells parameters, results, receivers, function types, and closure
/// parameters with the same `parameter_declaration` kind, and declares several
/// names in one declaration (`a, b int`). Only the `parameters` list of a
/// function or method declaration yields parameter symbols: one per name, or
/// one for a nameless declaration. `None` defers to the shared rules.
fn go_parameter_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> Option<Option<SymbolKind>> {
    const DECLARATIONS: &[&str] = &["parameter_declaration", "variadic_parameter_declaration"];
    if DECLARATIONS.contains(&node_kind.0) {
        let unnamed = node.child_by_field_name("name").is_none();
        return Some((unnamed && is_go_signature_parameter(node)).then_some(SymbolKind::Parameter));
    }
    if node_kind.0 != "identifier" {
        return None;
    }
    let declaration = node
        .parent()
        .filter(|parent| DECLARATIONS.contains(&parent.kind()))?;
    let is_type = declaration
        .child_by_field_name("type")
        .is_some_and(|ty| ty.start_byte() == node.start_byte());
    Some((!is_type && is_go_signature_parameter(declaration)).then_some(SymbolKind::Parameter))
}

/// A Go `type` declaration, by what it declares: `type T struct{..}`,
/// `type T interface{..}`, or any other named or aliased type.
fn go_type_kind(node: AstNode<'_>, node_kind: NodeKind<'_>) -> Option<SymbolKind> {
    match node_kind.0 {
        "type_spec" => Some(match node.child_by_field_name("type").map(|ty| ty.kind()) {
            Some("struct_type") => SymbolKind::Struct,
            Some("interface_type") => SymbolKind::Interface,
            _ => SymbolKind::TypeAlias,
        }),
        "type_alias" => Some(SymbolKind::TypeAlias),
        _ => None,
    }
}

/// A JavaScript/TypeScript parameter of something that is not a function
/// symbol: a TS function type (`done: (err?: Error) => void`) or a
/// `function (..) {}` expression. Recording it would credit the parameter to
/// the enclosing function.
fn is_foreign_javascript_parameter(node: AstNode<'_>) -> bool {
    let Some(list) = node
        .parent()
        .filter(|parent| parent.kind() == "formal_parameters")
    else {
        return false;
    };
    list.parent().is_some_and(|owner| {
        !languages::function_node_kinds().contains(&owner.kind())
            && owner.kind() != "method_signature"
            && owner.kind() != "abstract_method_signature"
    })
}

fn is_go_signature_parameter(declaration: AstNode<'_>) -> bool {
    declaration.parent().is_some_and(|list| {
        list.parent()
            .filter(|owner| matches!(owner.kind(), "function_declaration" | "method_declaration"))
            .and_then(|owner| owner.child_by_field_name("parameters"))
            .is_some_and(|parameters| parameters.start_byte() == list.start_byte())
    })
}
