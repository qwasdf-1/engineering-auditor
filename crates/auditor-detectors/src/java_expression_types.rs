//! The static type of a Java expression, when the frontend can prove it,
//! and the JDK return and field types that call chains pass through. The
//! Kotlin resolver reads the same JDK tables.

use auditor_ast::AstNode;

use crate::java_api_identity::JavaScope;
use crate::java_syntax::{compact, field_declaration, variable_declaration};

/// Return types of JDK calls that appear as receivers in call chains
/// (`Runtime.getRuntime().exec(..)`).
pub(crate) const RETURN_TYPES: &[(&str, &str)] = &[
    ("java.lang.Runtime.getRuntime", "java.lang.Runtime"),
    (
        "java.lang.ProcessBuilder.command",
        "java.lang.ProcessBuilder",
    ),
    (
        "javax.script.ScriptEngineManager.getEngineByName",
        "javax.script.ScriptEngine",
    ),
    (
        "javax.script.ScriptEngineManager.getEngineByExtension",
        "javax.script.ScriptEngine",
    ),
    (
        "javax.script.ScriptEngineManager.getEngineByMimeType",
        "javax.script.ScriptEngine",
    ),
    ("java.nio.file.Paths.get", "java.nio.file.Path"),
    ("java.nio.file.Path.of", "java.nio.file.Path"),
    ("java.nio.file.Path.toFile", "java.io.File"),
    (
        "java.sql.DriverManager.getConnection",
        "java.sql.Connection",
    ),
    ("java.sql.Connection.createStatement", "java.sql.Statement"),
    (
        "java.sql.Connection.prepareStatement",
        "java.sql.PreparedStatement",
    ),
];

/// Types of JDK static fields used as receivers (`System.out.println`).
pub(crate) const FIELD_TYPES: &[(&str, &str)] = &[
    ("java.lang.System.out", "java.io.PrintStream"),
    ("java.lang.System.err", "java.io.PrintStream"),
];

impl JavaScope {
    /// The qualified static type of an expression, when the frontend can
    /// prove it: a declared variable, a type name used as a static receiver,
    /// a JDK static field, a constructor, or a JDK call with a known return.
    pub(crate) fn expression_type(&self, expression: AstNode<'_>) -> Option<String> {
        match expression.kind() {
            "identifier" => match variable_declaration(expression) {
                Some((type_node, value)) => self.declared_type(type_node, value),
                None => self.type_named(expression.text()),
            },
            "field_access" => self.field_access_type(expression),
            "method_invocation" => self.call_return_type(expression),
            "object_creation_expression" => {
                self.resolve_type(expression.child_by_field_name("type")?)
            }
            "parenthesized_expression" => self.expression_type(expression.named_child(0)?),
            "cast_expression" => self.resolve_type(expression.child_by_field_name("type")?),
            _ => None,
        }
    }

    /// `this.field`'s declared type, a JDK static field's type
    /// (`System.out`), or a fully qualified type name used as a receiver.
    fn field_access_type(&self, access: AstNode<'_>) -> Option<String> {
        let object = access.child_by_field_name("object")?;
        let field = access.child_by_field_name("field")?.text();
        if object.kind() == "this" {
            let (type_node, value) = field_declaration(access, field)?;
            return self.declared_type(type_node, value);
        }
        if let Some(owner) = self.expression_type(object) {
            let qualified = format!("{owner}.{field}");
            return FIELD_TYPES
                .iter()
                .find(|(name, _)| *name == qualified)
                .map(|(_, field_type)| field_type.to_string());
        }
        self.qualified_name(&compact(access.text()))
    }

    /// The return type of a JDK call in `RETURN_TYPES`, or of a JDK static
    /// factory, which returns its own class (`MessageDigest.getInstance`).
    fn call_return_type(&self, call: AstNode<'_>) -> Option<String> {
        let api = self.resolve_call(call)?;
        if let Some((_, return_type)) = RETURN_TYPES.iter().find(|(name, _)| *name == api) {
            return Some(return_type.to_string());
        }
        let (owner, method) = api.rsplit_once('.')?;
        (call.child_by_field_name("object").is_some()
            && owner.starts_with("java")
            && matches!(method, "getInstance" | "newInstance" | "newFactory"))
        .then(|| owner.to_string())
    }

    /// A declared type, or the initializer's type for `var`.
    fn declared_type(&self, type_node: AstNode<'_>, value: Option<AstNode<'_>>) -> Option<String> {
        if type_node.text() == "var" {
            return self.expression_type(value?);
        }
        self.resolve_type(type_node)
    }
}
