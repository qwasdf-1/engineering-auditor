//! What a declaration states about its value, read from its syntax: the
//! type it declares (for a function, its return type), its initializer, and
//! whether it binds once (`final`, `const`, `readonly`, `val`). Java, C#,
//! Go, Kotlin, and TypeScript; other languages state no types here.

use auditor_ast::AstNode;

use crate::Language;

/// A declaration's stated type, initializer, and immutability.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SymbolTyping {
    /// The declared type's simple name (`long`, `String`, `List`, `Guid`),
    /// or a function's return type.
    pub(crate) type_name: Option<String>,
    /// The initializer's source text (`"x"`, `List.of(a, b)`).
    pub(crate) initializer: Option<String>,
    /// Bound once: `final`, `const`, `readonly`, Kotlin `val`, TS `const`.
    pub(crate) immutable: bool,
    /// The one name the declaration binds, where its symbol's name is not
    /// it (a C# field, a Go `const`, a Kotlin property).
    pub(crate) binding: Option<String>,
    /// A Java or C# method declared without a body (interface, abstract,
    /// `extern`): what runs is elsewhere.
    pub(crate) bodiless: bool,
}

/// Syntax kinds that name a type.
const TYPE_KINDS: &[&str] = &[
    "integral_type",
    "floating_point_type",
    "boolean_type",
    "void_type",
    "type_identifier",
    "generic_type",
    "scoped_type_identifier",
    "array_type",
    "predefined_type",
    "implicit_type",
    "generic_name",
    "qualified_name",
    "user_type",
    "nullable_type",
    "pointer_type",
    "qualified_type",
    "slice_type",
    "map_type",
    "type_annotation",
];

/// The typing of the declaration `node` of `language`.
pub(crate) fn symbol_typing(language: &Language, node: AstNode<'_>) -> SymbolTyping {
    match language {
        Language::Java => java_typing(node),
        Language::CSharp => csharp_typing(node),
        Language::Go => go_typing(node),
        Language::Kotlin => kotlin_typing(node),
        Language::TypeScript => typescript_typing(node),
        _ => SymbolTyping::default(),
    }
}

fn java_typing(node: AstNode<'_>) -> SymbolTyping {
    match node.kind() {
        "field_declaration" | "local_variable_declaration" | "constant_declaration" => {
            let final_ = child_of_kind(node, "modifiers")
                .is_some_and(|modifiers| has_word(modifiers.text(), "final"));
            declarator_typing(node, final_ || node.kind() == "constant_declaration")
        }
        "formal_parameter" | "method_declaration" => SymbolTyping {
            type_name: first_type(node),
            bodiless: node.kind() == "method_declaration"
                && node.child_by_field_name("body").is_none(),
            ..SymbolTyping::default()
        },
        _ => SymbolTyping::default(),
    }
}

fn csharp_typing(node: AstNode<'_>) -> SymbolTyping {
    match node.kind() {
        "field_declaration" | "local_declaration_statement" => {
            let modifiers = node
                .named_children()
                .filter(|child| child.kind() == "modifier")
                .map(|modifier| modifier.text())
                .collect::<Vec<_>>();
            let immutable = modifiers.contains(&"const") || modifiers.contains(&"readonly");
            child_of_kind(node, "variable_declaration")
                .map(|declaration| declarator_typing(declaration, immutable))
                .unwrap_or_default()
        }
        "variable_declaration" => declarator_typing(node, false),
        "property_declaration" | "parameter" | "method_declaration" => SymbolTyping {
            type_name: first_type(node),
            bodiless: node.kind() == "method_declaration"
                && node.child_by_field_name("body").is_none()
                && !node
                    .named_children()
                    .any(|child| child.kind() == "arrow_expression_clause"),
            ..SymbolTyping::default()
        },
        _ => SymbolTyping::default(),
    }
}

fn go_typing(node: AstNode<'_>) -> SymbolTyping {
    match node.kind() {
        // One `const` or `var` spec per declaration; a grouped declaration
        // binds several names, and its symbol stands for none of them.
        "const_declaration" | "var_declaration" => {
            let Some(spec) = only_child(node, &["const_spec", "var_spec"]) else {
                return SymbolTyping::default();
            };
            let Some(name) = only_child(spec, &["identifier"]) else {
                return SymbolTyping::default();
            };
            SymbolTyping {
                bodiless: false,
                type_name: first_type(spec),
                initializer: child_of_kind(spec, "expression_list")
                    .and_then(|values| values.named_child(0))
                    .map(|value| value.text().to_string()),
                immutable: node.kind() == "const_declaration",
                binding: Some(name.text().to_string()),
            }
        }
        "parameter_declaration" | "field_declaration" => SymbolTyping {
            type_name: first_type(node),
            ..SymbolTyping::default()
        },
        "function_declaration" | "method_declaration" => SymbolTyping {
            type_name: go_result(node),
            ..SymbolTyping::default()
        },
        _ => SymbolTyping::default(),
    }
}

/// A Go function's (first) result type: the type or result list between its
/// parameters and its body.
fn go_result(node: AstNode<'_>) -> Option<String> {
    let result = node
        .named_children()
        .take_while(|child| child.kind() != "block")
        .last()?;
    match result.kind() {
        "parameter_list" if node.child_by_field_name("result") == Some(result) => {
            result.named_child(0).and_then(first_type)
        }
        kind if TYPE_KINDS.contains(&kind) => Some(simple_type(result.text())),
        _ => None,
    }
}

fn kotlin_typing(node: AstNode<'_>) -> SymbolTyping {
    match node.kind() {
        "property_declaration" => {
            let declaration = child_of_kind(node, "variable_declaration");
            SymbolTyping {
                bodiless: false,
                type_name: declaration.and_then(first_type),
                initializer: node
                    .named_children()
                    .skip_while(|child| child.kind() != "variable_declaration")
                    .nth(1)
                    .map(|value| value.text().to_string()),
                immutable: child_of_kind(node, "binding_pattern_kind")
                    .is_some_and(|binding| binding.text() == "val"),
                binding: declaration
                    .and_then(|declaration| child_of_kind(declaration, "simple_identifier"))
                    .map(|name| name.text().to_string()),
            }
        }
        "parameter" | "function_declaration" => SymbolTyping {
            type_name: node
                .named_children()
                .find(|child| child.kind() == "user_type" || child.kind() == "nullable_type")
                .map(|type_node| simple_type(type_node.text())),
            ..SymbolTyping::default()
        },
        _ => SymbolTyping::default(),
    }
}

fn typescript_typing(node: AstNode<'_>) -> SymbolTyping {
    match node.kind() {
        "lexical_declaration" => {
            let Some(declarator) = only_child(node, &["variable_declarator"]) else {
                return SymbolTyping::default();
            };
            SymbolTyping {
                bodiless: false,
                type_name: first_type(declarator),
                initializer: declarator
                    .child_by_field_name("value")
                    .map(|value| value.text().to_string()),
                immutable: node
                    .children()
                    .next()
                    .is_some_and(|token| token.kind() == "const"),
                binding: declarator
                    .child_by_field_name("name")
                    .map(|name| name.text().to_string()),
            }
        }
        "required_parameter"
        | "optional_parameter"
        | "method_definition"
        | "function_declaration" => SymbolTyping {
            type_name: child_of_kind(node, "type_annotation")
                .map(|type_node| simple_type(type_node.text())),
            ..SymbolTyping::default()
        },
        _ => SymbolTyping::default(),
    }
}

/// A Java or C# declaration with one declarator: its type and the
/// declarator's initializer.
fn declarator_typing(node: AstNode<'_>, immutable: bool) -> SymbolTyping {
    let declarator = only_child(node, &["variable_declarator"]);
    let binding = declarator
        .and_then(|declarator| child_of_kind(declarator, "identifier"))
        .map(|name| name.text().to_string());
    let initializer = declarator
        .and_then(|declarator| {
            declarator
                .named_children()
                .skip(1)
                .find(|child| !child.kind().ends_with("comment"))
        })
        .map(|value| {
            // C#'s `equals_value_clause` wraps the value.
            value
                .named_child(0)
                .filter(|_| value.kind() == "equals_value_clause")
                .unwrap_or(value)
        })
        .map(|value| value.text().to_string());
    SymbolTyping {
        bodiless: false,
        type_name: first_type(node),
        initializer,
        immutable,
        binding,
    }
}

/// The type a declaration states: its grammar's `type` or `returns` field
/// (C# names a user type with a bare `identifier`), else its first child of
/// a type kind.
fn first_type(node: AstNode<'_>) -> Option<String> {
    node.child_by_field_name("type")
        .or_else(|| node.child_by_field_name("returns"))
        .or_else(|| {
            node.named_children()
                .find(|child| TYPE_KINDS.contains(&child.kind()))
        })
        .map(|type_node| simple_type(type_node.text()))
}

/// The one named child of `node` of one of `kinds`; none when it has
/// several (a grouped declaration binds no single name).
fn only_child<'a>(node: AstNode<'a>, kinds: &[&str]) -> Option<AstNode<'a>> {
    let mut matching = node
        .named_children()
        .filter(|child| kinds.contains(&child.kind()));
    let only = matching.next()?;
    matching.next().is_none().then_some(only)
}

fn child_of_kind<'a>(node: AstNode<'a>, kind: &str) -> Option<AstNode<'a>> {
    node.named_children().find(|child| child.kind() == kind)
}

fn has_word(text: &str, word: &str) -> bool {
    crate::util::identifiers(text)
        .iter()
        .any(|identifier| identifier == word)
}

/// A type's simple name: `java.util.List<String>` -> `List`, `*T` -> `T`,
/// `int?` -> `int`, `: number` -> `number`; an array keeps its brackets
/// (`String[]`, `[]byte`).
pub(crate) fn simple_type(text: &str) -> String {
    let text = text.trim().trim_start_matches(':').trim();
    let text = text.trim_start_matches('*').trim_end_matches(['?', '!']);
    let base = text.split('<').next().unwrap_or(text).trim();
    if base.starts_with("[]") || base.ends_with("[]") {
        return base.to_string();
    }
    base.rsplit(['.', ':']).next().unwrap_or(base).to_string()
}
