//! Resolved API identity for Kotlin: the fully qualified name a call
//! denotes. `exitProcess(1)` after `import kotlin.system.exitProcess` is
//! `kotlin.system.exitProcess`, `Random()` after `import java.util.Random` is
//! `java.util.Random.<init>`, and `rt.exec(..)` after
//! `val rt = Runtime.getRuntime()` is `java.lang.Runtime.exec`.
//!
//! Names resolve through explicit and aliased imports, Kotlin's default
//! imports (`kotlin.*`, `kotlin.io.*`, `java.lang.*`), and wildcard imports of
//! packages in the JDK class index Java resolution uses. Receivers resolve
//! through declared or initializer types of `val`/`var` properties and
//! parameters, and the JDK return-type table. A class, object, or function the
//! file declares shadows every implicit binding.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::java_api_identity::{JAVA_LANG, WILDCARD_CLASSES};
use crate::java_expression_types::{FIELD_TYPES, RETURN_TYPES};

/// Top-level functions of Kotlin's default imports that the rule tables name.
const KOTLIN_FUNCTIONS: &[(&str, &str)] = &[
    ("TODO", "kotlin.TODO"),
    ("assert", "kotlin.assert"),
    ("error", "kotlin.error"),
    ("print", "kotlin.io.print"),
    ("println", "kotlin.io.println"),
];

/// Classes of Kotlin's default imports that the rule tables name.
const KOTLIN_TYPES: &[(&str, &str)] = &[("NotImplementedError", "kotlin.NotImplementedError")];

/// Import bindings and declarations of one Kotlin file.
pub(crate) struct KotlinScope {
    names: BTreeMap<String, String>,
    wildcards: BTreeSet<String>,
    declared: BTreeSet<String>,
}

impl KotlinScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            names: BTreeMap::new(),
            wildcards: BTreeSet::new(),
            declared: BTreeSet::new(),
        };
        for import in ast_scan::nodes_of_kind(root, &["import_header"]) {
            let Some(path) = import
                .named_children()
                .find(|child| child.kind() == "identifier")
            else {
                continue;
            };
            let path = path.text().split_whitespace().collect::<String>();
            if import.text().trim_end().ends_with(".*") {
                scope.wildcards.insert(path);
                continue;
            }
            let alias = import
                .named_children()
                .find(|child| child.kind() == "import_alias")
                .and_then(|alias| alias.named_child(0))
                .map(|alias| alias.text().to_string());
            let binding = alias.or_else(|| path.rsplit('.').next().map(str::to_string));
            if let Some(binding) = binding {
                scope.names.insert(binding, path);
            }
        }
        for declaration in ast_scan::nodes_of_kind(
            root,
            &[
                "class_declaration",
                "object_declaration",
                "function_declaration",
            ],
        ) {
            if let Some(name) = declaration
                .named_children()
                .find(|child| matches!(child.kind(), "type_identifier" | "simple_identifier"))
            {
                scope.declared.insert(name.text().to_string());
            }
        }
        scope
    }

    /// The qualified name a simple type name denotes in this file.
    pub(crate) fn type_named(&self, name: &str) -> Option<String> {
        if let Some(qualified) = self.names.get(name) {
            return is_type_name(qualified).then(|| qualified.clone());
        }
        if self.declared.contains(name) {
            return None;
        }
        if JAVA_LANG.contains(&name) {
            return Some(format!("java.lang.{name}"));
        }
        if let Some((_, qualified)) = KOTLIN_TYPES.iter().find(|(simple, _)| *simple == name) {
            return Some(qualified.to_string());
        }
        WILDCARD_CLASSES
            .iter()
            .find(|(class, package)| *class == name && self.wildcards.contains(*package))
            .map(|(class, package)| format!("{package}.{class}"))
    }

    /// `pkg.Type.member` for a call on a resolved receiver, `pkg.Type.<init>`
    /// for a constructor, or a function's qualified name; a call on an
    /// unresolved receiver has no identity.
    pub(crate) fn resolve_call(&self, call: AstNode<'_>) -> Option<String> {
        if call.kind() != "call_expression" {
            return None;
        }
        let callee = call.named_child(0)?;
        match callee.kind() {
            "simple_identifier" => self.resolve_name(callee.text()),
            "navigation_expression" => {
                let receiver = callee.named_child(0)?;
                let member = navigation_member(callee)?;
                if let Some(package) = package_path(receiver) {
                    return is_type_name(member).then(|| format!("{package}.{member}.<init>"));
                }
                Some(format!("{}.{member}", self.expression_type(receiver)?))
            }
            _ => None,
        }
    }

    fn resolve_name(&self, name: &str) -> Option<String> {
        if let Some(qualified) = self.names.get(name) {
            return Some(if is_type_name(qualified) {
                format!("{qualified}.<init>")
            } else {
                qualified.clone()
            });
        }
        if self.declared.contains(name) {
            return None;
        }
        if let Some((_, qualified)) = KOTLIN_FUNCTIONS.iter().find(|(simple, _)| *simple == name) {
            return Some(qualified.to_string());
        }
        match self.type_named(name) {
            Some(qualified) => Some(format!("{qualified}.<init>")),
            None => Some(name.to_string()),
        }
    }

    /// The qualified static type of an expression, when it can be proved: a
    /// declared or initialized variable, a type used as a static receiver, a
    /// JDK static field, a constructor, or a JDK call with a known return.
    pub(crate) fn expression_type(&self, expression: AstNode<'_>) -> Option<String> {
        match expression.kind() {
            "simple_identifier" => match kotlin_variable(expression) {
                Some((declared, value)) => match declared {
                    Some(type_node) => self.resolve_user_type(type_node),
                    None => self.expression_type(value?),
                },
                None => self.type_named(expression.text()),
            },
            "call_expression" => self.returned_type(expression),
            "navigation_expression" => {
                let owner = self.expression_type(expression.named_child(0)?)?;
                let qualified = format!("{owner}.{}", navigation_member(expression)?);
                FIELD_TYPES
                    .iter()
                    .find(|(name, _)| *name == qualified)
                    .map(|(_, field_type)| field_type.to_string())
            }
            "parenthesized_expression" => self.expression_type(expression.named_child(0)?),
            _ => None,
        }
    }

    /// The type a call returns: the class a constructor call builds, a
    /// known function's return type, or the owner of a JDK factory.
    fn returned_type(&self, call: AstNode<'_>) -> Option<String> {
        let api = self.resolve_call(call)?;
        if let Some(owner) = api.strip_suffix(".<init>") {
            return Some(owner.to_string());
        }
        if let Some((_, returned)) = RETURN_TYPES.iter().find(|(name, _)| *name == api) {
            return Some(returned.to_string());
        }
        let (owner, method) = api.rsplit_once('.')?;
        (owner.starts_with("java")
            && matches!(method, "getInstance" | "newInstance" | "newFactory"))
        .then(|| owner.to_string())
    }

    /// The qualified name of a `user_type` node (`Random`, `List<String>`).
    pub(crate) fn resolve_user_type(&self, type_node: AstNode<'_>) -> Option<String> {
        let names = type_node
            .named_children()
            .filter(|child| child.kind() == "type_identifier")
            .map(|child| child.text())
            .collect::<Vec<_>>();
        match names.as_slice() {
            [single] => self.type_named(single),
            [first, ..] if first.starts_with(|c: char| c.is_ascii_lowercase()) => {
                Some(names.join("."))
            }
            [first, rest @ ..] => Some(format!("{}.{}", self.type_named(first)?, rest.join("."))),
            [] => None,
        }
    }

    /// Every call in `root` with its resolved identity, in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter_map(|call| Some((self.resolve_call(call)?, call)))
            .collect()
    }
}

/// A qualified name whose last segment names a type (`java.util.Random`).
fn is_type_name(qualified: &str) -> bool {
    qualified
        .rsplit('.')
        .next()
        .is_some_and(|last| last.starts_with(|c: char| c.is_ascii_uppercase()))
}

/// The member a navigation expression selects (`.exec` in `rt.exec`).
fn navigation_member(navigation: AstNode<'_>) -> Option<&str> {
    navigation
        .named_children()
        .find(|child| child.kind() == "navigation_suffix")?
        .named_child(0)
        .map(|member| member.text())
}

/// A dotted package path used as a receiver (`java.io` in `java.io.File`).
fn package_path(receiver: AstNode<'_>) -> Option<String> {
    let text = receiver.text().split_whitespace().collect::<String>();
    let all_lowercase = text.split('.').all(|segment| {
        !segment.is_empty() && segment.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    });
    (all_lowercase && text.contains('.')).then_some(text)
}

/// A variable's declared `user_type` (if any) and initializer.
pub(crate) type KotlinDeclaration<'a> = (Option<AstNode<'a>>, Option<AstNode<'a>>);

/// The declaration `identifier` refers to: the nearest enclosing property
/// declared before the use, a function parameter, or a constructor property.
pub(crate) fn kotlin_variable(identifier: AstNode<'_>) -> Option<KotlinDeclaration<'_>> {
    let name = identifier.text();
    let mut node = identifier;
    while let Some(scope) = node.parent() {
        for child in scope.named_children() {
            match child.kind() {
                // A use inside its own initializer (`val sink = sink.buffer()`)
                // names the outer binding.
                "property_declaration" if child.end_byte() <= identifier.start_byte() => {
                    let Some(variable) = child
                        .named_children()
                        .find(|part| part.kind() == "variable_declaration")
                    else {
                        continue;
                    };
                    if variable.named_child(0).map(|binding| binding.text()) != Some(name) {
                        continue;
                    }
                    let declared = variable
                        .named_children()
                        .find(|part| part.kind() == "user_type");
                    let value = child
                        .named_children()
                        .skip_while(|part| part.kind() != "variable_declaration")
                        .nth(1);
                    return Some((declared, value));
                }
                "function_value_parameters" | "primary_constructor" => {
                    let parameter = child.named_children().find(|parameter| {
                        matches!(parameter.kind(), "parameter" | "class_parameter")
                            && parameter
                                .named_children()
                                .find(|part| part.kind() == "simple_identifier")
                                .is_some_and(|binding| binding.text() == name)
                    });
                    if let Some(parameter) = parameter {
                        let declared = parameter
                            .named_children()
                            .find(|part| part.kind() == "user_type");
                        return Some((declared, None));
                    }
                }
                _ => {}
            }
        }
        node = scope;
    }
    None
}

/// The argument expressions of a call with their parameter names
/// (`host = "0.0.0.0"`), in order; a trailing lambda is the last argument.
pub(crate) fn kotlin_arguments(call: AstNode<'_>) -> Vec<(Option<&str>, AstNode<'_>)> {
    let Some(suffix) = call
        .named_children()
        .find(|child| child.kind() == "call_suffix")
    else {
        return Vec::new();
    };
    let mut arguments = Vec::new();
    for part in suffix.named_children() {
        match part.kind() {
            "value_arguments" => {
                for argument in part
                    .named_children()
                    .filter(|child| child.kind() == "value_argument")
                {
                    let children = argument.named_children().collect::<Vec<_>>();
                    match children.as_slice() {
                        [name, value] if name.kind() == "simple_identifier" => {
                            arguments.push((Some(name.text()), *value));
                        }
                        [.., value] => arguments.push((None, *value)),
                        [] => {}
                    }
                }
            }
            "annotated_lambda" | "lambda_literal" => arguments.push((None, part)),
            _ => {}
        }
    }
    arguments
}

/// The string values of a call's positional arguments, `None` where an
/// argument is not a plain string literal.
pub(crate) fn kotlin_string_arguments(call: AstNode<'_>) -> Vec<Option<String>> {
    kotlin_arguments(call)
        .into_iter()
        .filter(|(name, _)| name.is_none())
        .map(|(_, value)| ast_scan::string_literal_content(value))
        .collect()
}

/// The function declaration enclosing `node`.
pub(crate) fn enclosing_kotlin_function(node: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| ancestor.kind() == "function_declaration")
}

/// The name of a function declaration.
pub(crate) fn kotlin_function_name(function: AstNode<'_>) -> Option<&str> {
    function
        .named_children()
        .find(|child| child.kind() == "simple_identifier")
        .map(|name| name.text())
}

/// Whether the file declares a top-level `fun main`: a program, whose
/// standard streams and exit status are its interface.
pub(crate) fn declares_kotlin_main(root: AstNode<'_>) -> bool {
    root.named_children().any(|child| {
        child.kind() == "function_declaration" && kotlin_function_name(child) == Some("main")
    })
}

/// Whether a declaration carries a `private` or `internal` modifier.
pub(crate) fn kotlin_is_restricted(declaration: AstNode<'_>) -> bool {
    declaration
        .named_children()
        .find(|child| child.kind() == "modifiers")
        .is_some_and(|modifiers| {
            modifiers
                .text()
                .split_whitespace()
                .any(|word| matches!(word, "private" | "internal"))
        })
}

/// `(name, value)` of a Kotlin `val`/`var` property with an initializer.
pub(crate) fn kotlin_property_store(node: AstNode<'_>) -> Option<(AstNode<'_>, AstNode<'_>)> {
    let mut children = node
        .named_children()
        .skip_while(|child| child.kind() != "variable_declaration");
    let variable = children.next()?;
    Some((variable.named_child(0)?, children.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"package demo

import java.util.Random
import kotlin.system.exitProcess
import java.security.MessageDigest as MD
import javax.script.*

class Demo(private val engine: ScriptEngine) {
    fun run(line: String) {
        val r = Random()
        r.nextInt()
        val md = MD.getInstance("MD5")
        md.update(line.toByteArray())
        Runtime.getRuntime().exec(arrayOf("sh", "-c", line))
        System.out.println(line)
        println(line)
        exitProcess(1)
        engine.eval(line)
        java.io.File(line).setWritable(true, false)
        TODO("later")
    }
}
"#;

    #[test]
    fn kotlin_calls_resolve_through_imports_declarations_and_return_types() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Kotlin, SOURCE)
            .unwrap();
        let scope = KotlinScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.contains('.'))
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "java.util.Random.<init>",
                "java.util.Random.nextInt",
                "java.security.MessageDigest.getInstance",
                "java.security.MessageDigest.update",
                "java.lang.String.toByteArray",
                "java.lang.Runtime.exec",
                "java.lang.Runtime.getRuntime",
                "java.io.PrintStream.println",
                "kotlin.io.println",
                "kotlin.system.exitProcess",
                "javax.script.ScriptEngine.eval",
                "java.io.File.setWritable",
                "java.io.File.<init>",
                "kotlin.TODO",
            ]
        );
        assert!(!declares_kotlin_main(tree.root_node()));
    }

    #[test]
    fn a_declared_class_or_function_shadows_implicit_bindings() {
        let source = "class Runtime { fun exec() {} }\nfun println(x: Any) {}\nfun f(sink: Sink) { Runtime().exec(); println(1); val sink = sink.buffer(); sink.write() }\n";
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Kotlin, source)
            .unwrap();
        let scope = KotlinScope::new(tree.root_node());
        assert!(scope
            .calls(tree.root_node())
            .iter()
            .all(|(name, _)| !name.contains('.')));
    }
}
