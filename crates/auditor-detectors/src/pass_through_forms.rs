//! The function forms `pass_through_wrapper` reads in each Tier-1
//! language: which functions are candidates, their parameters, the one call
//! their body makes, and the names a qualified target may start with.

use auditor_ast::AstNode;

use super::{has_child_kind, preceding_comments, sole_call, Forwarded, Wrapper};
use crate::ast_scan::{self, nodes_of_kind};
use crate::Language;

/// The names a qualified target may start with: imported modules and
/// packages, and JS global namespaces.
pub(super) fn namespaces(language: &Language, root: AstNode<'_>) -> Vec<String> {
    match language {
        Language::Python => python_namespaces(root),
        Language::JavaScript | Language::TypeScript => js_namespaces(root),
        Language::Go => go_namespaces(root),
        _ => Vec::new(),
    }
}

/// `import a`, `import a as b`, `from m import a`, `from m import a as b`.
fn python_namespaces(root: AstNode<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for import in nodes_of_kind(root, &["import_statement", "import_from_statement"]) {
        let from = usize::from(import.kind() == "import_from_statement");
        for name in import.named_children().skip(from) {
            let bound = match name.kind() {
                "aliased_import" => name.child_by_field_name("alias"),
                "dotted_name" => name.named_child(0),
                _ => None,
            };
            names.extend(bound.map(|bound| bound.text().to_string()));
        }
    }
    names
}

/// JavaScript globals that hold static functions rather than data.
const JS_GLOBAL_NAMESPACES: &[&str] = &[
    "Array", "Object", "JSON", "Math", "Number", "String", "Promise", "Reflect", "Date", "Symbol",
    "BigInt", "console", "Buffer", "URL",
];

/// JS global namespaces, ES import bindings, and `require` bindings.
fn js_namespaces(root: AstNode<'_>) -> Vec<String> {
    let mut names: Vec<String> = JS_GLOBAL_NAMESPACES
        .iter()
        .map(|name| name.to_string())
        .collect();
    for import in nodes_of_kind(root, &["import_clause"]) {
        names.extend(identifiers(import, &["identifier"]));
    }
    for declarator in nodes_of_kind(root, &["variable_declarator"]) {
        let required = declarator
            .child_by_field_name("value")
            .is_some_and(|value| {
                value.kind() == "call_expression" && value.text().starts_with("require(")
            });
        if let Some(name) = declarator.child_by_field_name("name").filter(|_| required) {
            names.extend(identifiers(
                name,
                &["identifier", "shorthand_property_identifier_pattern"],
            ));
        }
    }
    names
}

/// The node and its descendants of the identifier kinds, as text.
fn identifiers(node: AstNode<'_>, kinds: &[&str]) -> Vec<String> {
    std::iter::once(node)
        .chain(ast_scan::descendants(node))
        .filter(|node| kinds.contains(&node.kind()))
        .map(|node| node.text().to_string())
        .collect()
}

/// A Go import's name: its alias, or its path's last element.
fn go_namespaces(root: AstNode<'_>) -> Vec<String> {
    nodes_of_kind(root, &["import_spec"])
        .into_iter()
        .filter_map(|spec| {
            if let Some(name) = spec.child_by_field_name("name") {
                return Some(name.text().to_string());
            }
            let path = spec.child_by_field_name("path")?.text().trim_matches('"');
            Some(path.rsplit('/').next().unwrap_or(path).to_string())
        })
        .collect()
}

// Rust ---------------------------------------------------------------------

/// Functions without a receiver, outside tests, traits, FFI exports, and
/// `#[cfg]` or `#[deprecated]` attributes. Only a bare `pub` is public.
pub(super) fn rust_wrappers(root: AstNode<'_>) -> Vec<Wrapper<'_>> {
    nodes_of_kind(root, &["function_item"])
        .into_iter()
        .filter_map(|function| {
            let parameters = function.child_by_field_name("parameters")?;
            if has_child_kind(parameters, "self_parameter")
                || ast_scan::is_inside_rust_test(function)
                || in_trait(function)
                || (has_child_kind(function, "function_modifiers")
                    && function.text().contains("extern"))
            {
                return None;
            }
            let attributes = preceding_comments(function).join(" ");
            if ["cfg", "no_mangle", "export_name", "deprecated"]
                .iter()
                .any(|attribute| attributes.contains(attribute))
            {
                return None;
            }
            let call = sole_call(function.child_by_field_name("body")?, "call_expression")?;
            let callee = call
                .child_by_field_name("function")
                .filter(|callee| matches!(callee.kind(), "identifier" | "scoped_identifier"))?;
            Some(Wrapper {
                function,
                name: function.child_by_field_name("name")?.text(),
                parameters: parameters
                    .named_children()
                    .filter(|parameter| parameter.kind() == "parameter")
                    .map(|parameter| {
                        let pattern = parameter.child_by_field_name("pattern")?;
                        (pattern.kind() == "identifier")
                            .then(|| Forwarded::Plain(pattern.text().to_string()))
                    })
                    .collect::<Option<Vec<_>>>()?,
                arguments: plain_arguments(call.child_by_field_name("arguments")?)?,
                callee,
                public: function
                    .named_children()
                    .any(|child| child.kind() == "visibility_modifier" && child.text() == "pub"),
            })
        })
        .collect()
}

/// Inside a trait definition or a trait implementation.
fn in_trait(function: AstNode<'_>) -> bool {
    std::iter::successors(function.parent(), |parent| parent.parent()).any(|ancestor| {
        ancestor.kind() == "trait_item"
            || (ancestor.kind() == "impl_item" && ancestor.child_by_field_name("trait").is_some())
    })
}

/// Arguments that are each a bare identifier.
fn plain_arguments(arguments: AstNode<'_>) -> Option<Vec<Forwarded>> {
    arguments
        .named_children()
        .filter(|argument| !argument.kind().contains("comment"))
        .map(|argument| {
            (argument.kind() == "identifier").then(|| Forwarded::Plain(argument.text().to_string()))
        })
        .collect()
}

// Python -------------------------------------------------------------------

/// Module-level, undecorated functions: a definition under a condition or
/// `try` is a compatibility shim, and a decorator adds behaviour. A name
/// without a leading underscore is public.
pub(super) fn python_wrappers(root: AstNode<'_>) -> Vec<Wrapper<'_>> {
    root.named_children()
        .filter(|child| child.kind() == "function_definition")
        .filter_map(|function| read_wrapper(function, &PYTHON))
        .collect()
}

/// How a language's plain functions read as wrappers: the kind of the one
/// call, its callee kinds, the parameter and argument readers, and which
/// names are public.
struct Reader {
    call: &'static str,
    callees: &'static [&'static str],
    parameters: fn(AstNode<'_>) -> Option<Vec<Forwarded>>,
    arguments: fn(AstNode<'_>) -> Option<Vec<Forwarded>>,
    public: fn(&str) -> bool,
}

const PYTHON: Reader = Reader {
    call: "call",
    callees: &["identifier", "attribute"],
    parameters: python_parameters,
    arguments: python_arguments,
    public: |name| !name.starts_with('_'),
};

const GO: Reader = Reader {
    call: "call_expression",
    callees: &["identifier", "selector_expression"],
    parameters: go_parameters,
    arguments: go_arguments,
    public: |name| name.starts_with(|c: char| c.is_ascii_uppercase()),
};

fn read_wrapper<'t>(function: AstNode<'t>, reader: &Reader) -> Option<Wrapper<'t>> {
    let call = sole_call(function.child_by_field_name("body")?, reader.call)?;
    let callee = call
        .child_by_field_name("function")
        .filter(|callee| reader.callees.contains(&callee.kind()))?;
    let name = function.child_by_field_name("name")?.text();
    Some(Wrapper {
        function,
        name,
        parameters: (reader.parameters)(function.child_by_field_name("parameters")?)?,
        arguments: (reader.arguments)(call.child_by_field_name("arguments")?)?,
        callee,
        public: (reader.public)(name),
    })
}

fn python_parameters(parameters: AstNode<'_>) -> Option<Vec<Forwarded>> {
    let mut forwarded = Vec::new();
    for parameter in parameters.named_children() {
        let name = |node: AstNode<'_>| {
            node.child_by_field_name("name")
                .or_else(|| {
                    node.named_children()
                        .find(|child| child.kind() == "identifier")
                })
                .map(|name| name.text().to_string())
        };
        let entry = match parameter.kind() {
            "identifier" => Some(parameter.text().to_string()),
            "typed_parameter" | "default_parameter" | "typed_default_parameter" => name(parameter),
            "list_splat_pattern" => {
                forwarded.push(Forwarded::Spread(name(parameter)?));
                continue;
            }
            "dictionary_splat_pattern" => {
                forwarded.push(Forwarded::Keywords(name(parameter)?));
                continue;
            }
            // A keyword-only parameter passed on as `x=x` reads as plain.
            "keyword_separator" | "positional_separator" | "comment" => continue,
            _ => return None,
        }?;
        forwarded.push(Forwarded::Plain(entry));
    }
    Some(forwarded)
}

fn python_arguments(arguments: AstNode<'_>) -> Option<Vec<Forwarded>> {
    let mut forwarded = Vec::new();
    for argument in arguments.named_children() {
        let inner = || argument.named_child(0).map(|name| name.text().to_string());
        forwarded.push(match argument.kind() {
            "identifier" => Forwarded::Plain(argument.text().to_string()),
            "list_splat" => Forwarded::Spread(inner()?),
            "dictionary_splat" => Forwarded::Keywords(inner()?),
            "keyword_argument" => {
                let name = argument.child_by_field_name("name")?.text();
                let value = argument.child_by_field_name("value")?;
                if value.kind() != "identifier" || value.text() != name {
                    return None;
                }
                // `x=x` passes `x` on whether it was positional or not.
                Forwarded::Plain(name.to_string())
            }
            "comment" => continue,
            _ => return None,
        });
    }
    Some(forwarded)
}

// JavaScript and TypeScript ------------------------------------------------

/// Top-level function declarations and `const f = (..) => ..` arrows,
/// exported (by `export` or CommonJS) or not.
pub(super) fn js_wrappers(root: AstNode<'_>) -> Vec<Wrapper<'_>> {
    let common_js = common_js_exports(root);
    let mut wrappers = Vec::new();
    for statement in root.named_children() {
        let (exported, declaration) = if statement.kind() == "export_statement" {
            match statement.child_by_field_name("declaration") {
                Some(declaration) => (true, declaration),
                None => continue,
            }
        } else {
            (false, statement)
        };
        for (name, function) in js_functions(declaration) {
            let exported = exported || common_js.iter().any(|export| export == name.text());
            if let Some(wrapper) = js_wrapper(name, function, exported) {
                wrappers.push(wrapper);
            }
        }
    }
    wrappers
}

/// The named functions a top-level declaration defines.
fn js_functions(declaration: AstNode<'_>) -> Vec<(AstNode<'_>, AstNode<'_>)> {
    match declaration.kind() {
        "function_declaration" => declaration
            .child_by_field_name("name")
            .map(|name| vec![(name, declaration)])
            .unwrap_or_default(),
        "lexical_declaration" => declaration
            .named_children()
            .filter(|declarator| declarator.kind() == "variable_declarator")
            .filter_map(|declarator| {
                let value = declarator.child_by_field_name("value").filter(|value| {
                    matches!(value.kind(), "arrow_function" | "function_expression")
                })?;
                Some((declarator.child_by_field_name("name")?, value))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The names a CommonJS module exports: `module.exports = { a, b: c }`,
/// `module.exports = a`, and `module.exports.x = a`/`exports.x = a`.
fn common_js_exports(root: AstNode<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for assignment in nodes_of_kind(root, &["assignment_expression"]) {
        let (Some(left), Some(right)) = (
            assignment.child_by_field_name("left"),
            assignment.child_by_field_name("right"),
        ) else {
            continue;
        };
        let target = left.text();
        if !(target == "module.exports"
            || target.starts_with("module.exports.")
            || target.starts_with("exports."))
        {
            continue;
        }
        match right.kind() {
            "identifier" => names.push(right.text().to_string()),
            "object" => names.extend(right.named_children().filter_map(|property| {
                let value = match property.kind() {
                    "shorthand_property_identifier" => Some(property),
                    "pair" => property.child_by_field_name("value"),
                    _ => None,
                }?;
                matches!(value.kind(), "identifier" | "shorthand_property_identifier")
                    .then(|| value.text().to_string())
            })),
            _ => {}
        }
    }
    names
}

fn js_wrapper<'t>(name: AstNode<'t>, function: AstNode<'t>, exported: bool) -> Option<Wrapper<'t>> {
    let body = function.child_by_field_name("body")?;
    let call = if body.kind() == "statement_block" {
        sole_call(body, "call_expression")?
    } else {
        let mut expression = body;
        while matches!(
            expression.kind(),
            "await_expression" | "parenthesized_expression"
        ) {
            expression = expression.named_child(0)?;
        }
        (expression.kind() == "call_expression").then_some(expression)?
    };
    let predicate = function
        .child_by_field_name("return_type")
        .is_some_and(|annotation| {
            ast_scan::descendants(annotation).any(|node| node.kind() == "type_predicate")
        });
    if predicate || call.child_by_field_name("type_arguments").is_some() {
        return None;
    }
    let callee = call
        .child_by_field_name("function")
        .filter(|callee| matches!(callee.kind(), "identifier" | "member_expression"))?;
    if callee.text().starts_with("this.") || callee.text().starts_with("super.") {
        return None;
    }
    let parameters = match function.child_by_field_name("parameters") {
        Some(parameters) => js_parameters(parameters)?,
        None => vec![Forwarded::Plain(
            function
                .child_by_field_name("parameter")?
                .text()
                .to_string(),
        )],
    };
    let arguments = call
        .child_by_field_name("arguments")?
        .named_children()
        .filter(|argument| !argument.kind().contains("comment"))
        .map(|argument| js_name(argument, "spread_element"))
        .collect::<Option<Vec<_>>>()?;
    Some(Wrapper {
        function,
        name: name.text(),
        parameters,
        callee,
        arguments,
        public: exported,
    })
}

/// A JS parameter or argument that is a plain name, or a name spread by
/// `...` (`spread`: a `rest_pattern` or `spread_element`).
fn js_name(node: AstNode<'_>, spread: &str) -> Option<Forwarded> {
    match node.kind() {
        "identifier" => Some(Forwarded::Plain(node.text().to_string())),
        kind if kind == spread => node
            .named_child(0)
            .filter(|inner| inner.kind() == "identifier")
            .map(|inner| Forwarded::Spread(inner.text().to_string())),
        _ => None,
    }
}

fn js_parameters(parameters: AstNode<'_>) -> Option<Vec<Forwarded>> {
    parameters
        .named_children()
        .filter(|parameter| !parameter.kind().contains("comment"))
        .map(|parameter| {
            let pattern = match parameter.kind() {
                "required_parameter" | "optional_parameter" => {
                    parameter.child_by_field_name("pattern")?
                }
                _ => parameter,
            };
            js_name(pattern, "rest_pattern")
        })
        .collect()
}

// Go -------------------------------------------------------------------------

/// A file built only for some platforms: a `//go:build` or `// +build`
/// line. Its functions are platform shims.
pub(super) fn go_build_constrained(root: AstNode<'_>) -> bool {
    root.named_children()
        .take_while(|child| child.kind() == "comment")
        .any(|comment| {
            let text = comment.text();
            text.starts_with("//go:build") || text.starts_with("// +build")
        })
}

/// Functions (not methods) without a `//go:linkname` or `//export`
/// directive. An uppercase name is exported.
pub(super) fn go_wrappers(root: AstNode<'_>) -> Vec<Wrapper<'_>> {
    nodes_of_kind(root, &["function_declaration"])
        .into_iter()
        .filter_map(|function| {
            let directives = preceding_comments(function).join(" ");
            if directives.contains("//go:linkname") || directives.contains("//export") {
                return None;
            }
            read_wrapper(function, &GO)
        })
        .collect()
}

/// Named parameters (`a, b int`, `parts ...string`); an unnamed one stops
/// the reading.
fn go_parameters(parameters: AstNode<'_>) -> Option<Vec<Forwarded>> {
    let mut forwarded = Vec::new();
    for declaration in parameters.named_children() {
        let variadic = declaration.kind() == "variadic_parameter_declaration";
        let names: Vec<AstNode<'_>> = declaration
            .named_children()
            .filter(|child| child.kind() == "identifier")
            .collect();
        if names.is_empty() {
            return None;
        }
        forwarded.extend(names.into_iter().map(|name| {
            let name = name.text().to_string();
            if variadic {
                Forwarded::Spread(name)
            } else {
                Forwarded::Plain(name)
            }
        }));
    }
    Some(forwarded)
}

/// Plain names and `name...` spreads.
fn go_arguments(arguments: AstNode<'_>) -> Option<Vec<Forwarded>> {
    let mut forwarded = Vec::new();
    for argument in arguments.named_children() {
        forwarded.push(match argument.kind() {
            "identifier" => Forwarded::Plain(argument.text().to_string()),
            "variadic_argument" => Forwarded::Spread(
                argument
                    .named_child(0)
                    .filter(|inner| inner.kind() == "identifier")?
                    .text()
                    .to_string(),
            ),
            "comment" => continue,
            _ => return None,
        });
    }
    Some(forwarded)
}

// Java -----------------------------------------------------------------------

/// Static methods without annotations, outside overload sets: an instance
/// method forwards for its object, and an annotation (`@Override`,
/// `@Deprecated`, a framework mapping) gives the method a role. `public`
/// and `protected` are public.
pub(super) fn java_wrappers(root: AstNode<'_>) -> Vec<Wrapper<'_>> {
    nodes_of_kind(root, &["method_declaration"])
        .into_iter()
        .filter_map(|method| {
            let modifiers = method
                .named_children()
                .find(|child| child.kind() == "modifiers")?;
            let words: Vec<&str> = modifiers.text().split_whitespace().collect();
            if !words.contains(&"static")
                || modifiers
                    .named_children()
                    .any(|modifier| modifier.kind().contains("annotation"))
                || overloaded(method)
            {
                return None;
            }
            let call = sole_call(method.child_by_field_name("body")?, "method_invocation")?;
            if call.child_by_field_name("type_arguments").is_some() {
                return None;
            }
            let callee = match call.child_by_field_name("object") {
                None => call.child_by_field_name("name")?,
                Some(object) if object.kind() == "identifier" => call,
                Some(_) => return None,
            };
            Some(Wrapper {
                function: method,
                name: method.child_by_field_name("name")?.text(),
                parameters: java_parameters(method.child_by_field_name("parameters")?)?,
                arguments: plain_arguments(call.child_by_field_name("arguments")?)?,
                callee,
                public: words.contains(&"public") || words.contains(&"protected"),
            })
        })
        .collect()
}

/// Formal parameters and a varargs parameter, by name.
fn java_parameters(parameters: AstNode<'_>) -> Option<Vec<Forwarded>> {
    parameters
        .named_children()
        .map(|parameter| match parameter.kind() {
            "formal_parameter" => parameter
                .child_by_field_name("name")
                .map(|name| Forwarded::Plain(name.text().to_string())),
            "spread_parameter" => parameter
                .named_children()
                .find(|child| child.kind() == "variable_declarator")
                .and_then(|declarator| declarator.child_by_field_name("name"))
                .map(|name| Forwarded::Plain(name.text().to_string())),
            _ => None,
        })
        .collect()
}

/// Another method of the same name in the class: an overload set, whose
/// forwarding members dispatch on type.
fn overloaded(method: AstNode<'_>) -> bool {
    let name = method.child_by_field_name("name").map(|name| name.text());
    method.parent().is_some_and(|body| {
        body.named_children()
            .filter(|member| {
                member.kind() == "method_declaration"
                    && member.child_by_field_name("name").map(|name| name.text()) == name
            })
            .count()
            > 1
    })
}
