//! Resolved API identity for C#: the fully qualified name a call, object
//! creation, or member denotes. `Process.Start(..)` under `using
//! System.Diagnostics;` is `System.Diagnostics.Process.Start`, `new Random()`
//! is `System.Random..ctor`, and `md.ComputeHash(..)` after
//! `var md = MD5.Create()` is `System.Security.Cryptography.MD5.ComputeHash`.
//!
//! Type names resolve through `using` directives (namespaces, aliases, and
//! `using static`), the .NET SDK's implicit global usings, and fully
//! qualified names, against an index of the BCL and library types the rule
//! tables name. Receivers resolve through declared types of locals,
//! parameters, and fields (`var` through its initializer), static factories
//! (`MD5.Create()`), and a table of return and member types. A type the file
//! declares shadows every binding.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;

/// Namespaces the .NET SDK imports implicitly (`ImplicitUsings`).
const IMPLICIT_NAMESPACES: &[&str] = &[
    "System",
    "System.Collections.Generic",
    "System.IO",
    "System.Linq",
    "System.Net.Http",
    "System.Threading",
    "System.Threading.Tasks",
];

/// The types the rule tables name, with their namespaces.
const TYPES: &[(&str, &str)] = &[
    ("Console", "System"),
    ("Environment", "System"),
    ("NotImplementedException", "System"),
    ("Random", "System"),
    ("Regex", "System.Text.RegularExpressions"),
    ("Debug", "System.Diagnostics"),
    ("Debugger", "System.Diagnostics"),
    ("Process", "System.Diagnostics"),
    ("ProcessStartInfo", "System.Diagnostics"),
    ("File", "System.IO"),
    ("FileStream", "System.IO"),
    ("StreamReader", "System.IO"),
    ("StreamWriter", "System.IO"),
    ("UnixFileMode", "System.IO"),
    ("IPAddress", "System.Net"),
    ("IPEndPoint", "System.Net"),
    ("SecurityProtocolType", "System.Net"),
    ("ServicePointManager", "System.Net"),
    ("HttpClientHandler", "System.Net.Http"),
    ("RemoteCertificateValidationCallback", "System.Net.Security"),
    ("TcpListener", "System.Net.Sockets"),
    ("SslProtocols", "System.Security.Authentication"),
    ("Aes", "System.Security.Cryptography"),
    ("CipherMode", "System.Security.Cryptography"),
    ("DES", "System.Security.Cryptography"),
    ("DESCryptoServiceProvider", "System.Security.Cryptography"),
    ("HMACMD5", "System.Security.Cryptography"),
    ("HMACSHA1", "System.Security.Cryptography"),
    ("MD5", "System.Security.Cryptography"),
    ("MD5CryptoServiceProvider", "System.Security.Cryptography"),
    ("RC2", "System.Security.Cryptography"),
    ("RC2CryptoServiceProvider", "System.Security.Cryptography"),
    ("SHA1", "System.Security.Cryptography"),
    ("SHA1CryptoServiceProvider", "System.Security.Cryptography"),
    ("SHA1Managed", "System.Security.Cryptography"),
    ("TripleDES", "System.Security.Cryptography"),
    (
        "TripleDESCryptoServiceProvider",
        "System.Security.Cryptography",
    ),
    ("NetDataContractSerializer", "System.Runtime.Serialization"),
    (
        "BinaryFormatter",
        "System.Runtime.Serialization.Formatters.Binary",
    ),
    (
        "SoapFormatter",
        "System.Runtime.Serialization.Formatters.Soap",
    ),
    ("LosFormatter", "System.Web.UI"),
    ("ObjectStateFormatter", "System.Web.UI"),
    ("DtdProcessing", "System.Xml"),
    ("XmlDocument", "System.Xml"),
    ("XmlReader", "System.Xml"),
    ("XmlReaderSettings", "System.Xml"),
    ("XmlTextReader", "System.Xml"),
    ("XmlUrlResolver", "System.Xml"),
    ("SqlCommand", "Microsoft.Data.SqlClient"),
    ("SqlCommand", "System.Data.SqlClient"),
    ("JsonSerializerSettings", "Newtonsoft.Json"),
    ("TypeNameHandling", "Newtonsoft.Json"),
    ("CSharpScript", "Microsoft.CodeAnalysis.CSharp.Scripting"),
];

/// Return and member types of calls and properties used as receivers.
const MEMBER_TYPES: &[(&str, &str)] = &[
    ("System.IO.File.Create", "System.IO.FileStream"),
    ("System.IO.File.Open", "System.IO.FileStream"),
    ("System.IO.File.OpenRead", "System.IO.FileStream"),
    ("System.IO.File.OpenWrite", "System.IO.FileStream"),
    ("System.IO.File.OpenText", "System.IO.StreamReader"),
    ("System.IO.File.CreateText", "System.IO.StreamWriter"),
    ("System.Net.IPAddress.Any", "System.Net.IPAddress"),
    ("System.Net.IPAddress.IPv6Any", "System.Net.IPAddress"),
    ("System.Random.Shared", "System.Random"),
];

/// Import bindings and declared types of one C# file.
pub(crate) struct CSharpScope {
    namespaces: BTreeSet<String>,
    aliases: BTreeMap<String, String>,
    statics: Vec<String>,
    declared: BTreeSet<String>,
}

impl CSharpScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            namespaces: IMPLICIT_NAMESPACES
                .iter()
                .map(|name| name.to_string())
                .collect(),
            aliases: BTreeMap::new(),
            statics: Vec::new(),
            declared: BTreeSet::new(),
        };
        for directive in ast_scan::nodes_of_kind(root, &["using_directive"]) {
            let Some(target) = directive.named_children().find(|child| {
                matches!(child.kind(), "qualified_name" | "identifier")
                    && directive
                        .child_by_field_name("name")
                        .is_none_or(|alias| alias.start_byte() != child.start_byte())
            }) else {
                continue;
            };
            let path = target.text().split_whitespace().collect::<String>();
            if let Some(alias) = directive.child_by_field_name("name") {
                scope.aliases.insert(alias.text().to_string(), path);
            } else if directive.children().any(|child| child.kind() == "static") {
                scope
                    .statics
                    .push(scope.qualified_type(&path).unwrap_or(path));
            } else {
                scope.namespaces.insert(path);
            }
        }
        scope.declared.extend(ast_scan::declared_names(
            root,
            &[
                "class_declaration",
                "struct_declaration",
                "interface_declaration",
                "enum_declaration",
                "record_declaration",
            ],
        ));
        scope
    }

    /// The qualified name a simple type name denotes in this file.
    pub(crate) fn type_named(&self, name: &str) -> Option<String> {
        if let Some(target) = self.aliases.get(name) {
            return self.qualified_type(target).or_else(|| Some(target.clone()));
        }
        if self.declared.contains(name) {
            return None;
        }
        TYPES
            .iter()
            .find(|(class, namespace)| *class == name && self.namespaces.contains(*namespace))
            .map(|(class, namespace)| format!("{namespace}.{class}"))
    }

    /// A dotted name as a type: an alias or simple type followed by nested
    /// names, or a fully qualified indexed type (`System.IO.File`).
    pub(crate) fn qualified_type(&self, dotted: &str) -> Option<String> {
        if let Some((class, namespace)) = TYPES
            .iter()
            .find(|(class, namespace)| dotted == format!("{namespace}.{class}"))
        {
            return Some(format!("{namespace}.{class}"));
        }
        let (first, rest) = dotted.split_once('.')?;
        let alias = self.aliases.get(first)?;
        let candidate = format!("{alias}.{rest}");
        TYPES
            .iter()
            .any(|(class, namespace)| candidate == format!("{namespace}.{class}"))
            .then_some(candidate)
    }

    /// The qualified name of a type node (`Random`, `List<int>`,
    /// `System.IO.File`, `Crypto.MD5`).
    pub(crate) fn resolve_type(&self, node: AstNode<'_>) -> Option<String> {
        match node.kind() {
            "identifier" => self.type_named(node.text()),
            "generic_name" => self.type_named(node.named_child(0)?.text()),
            "qualified_name" | "member_access_expression" => {
                self.qualified_type(&node.text().split_whitespace().collect::<String>())
            }
            _ => None,
        }
    }

    /// `Ns.Type.Method` for a call on a resolved receiver, `Ns.Type..ctor`
    /// for an object creation, or a `using static` member.
    pub(crate) fn resolve_call(&self, call: AstNode<'_>) -> Option<String> {
        match call.kind() {
            "invocation_expression" => {
                let function = call.child_by_field_name("function")?;
                match function.kind() {
                    "member_access_expression" => {
                        let member = function.child_by_field_name("name")?.text();
                        let owner = function.child_by_field_name("expression")?;
                        Some(format!("{}.{member}", self.expression_type(owner)?))
                    }
                    "identifier" => Some(match self.statics.first() {
                        Some(owner) => format!("{owner}.{}", function.text()),
                        None => function.text().to_string(),
                    }),
                    _ => None,
                }
            }
            "object_creation_expression" => Some(format!(
                "{}..ctor",
                self.resolve_type(call.child_by_field_name("type")?)?
            )),
            _ => None,
        }
    }

    /// The qualified member a `Type.Member` access denotes (`IPAddress.Any`,
    /// `SslProtocols.Tls`).
    pub(crate) fn resolve_member(&self, access: AstNode<'_>) -> Option<String> {
        if access.kind() != "member_access_expression" {
            return None;
        }
        let member = access.child_by_field_name("name")?.text();
        let owner = access.child_by_field_name("expression")?;
        Some(format!("{}.{member}", self.expression_type(owner)?))
    }

    /// The qualified static type of an expression, when it can be proved.
    pub(crate) fn expression_type(&self, expression: AstNode<'_>) -> Option<String> {
        match expression.kind() {
            "identifier" => match csharp_variable(expression) {
                Some((type_node, value)) if type_node.kind() == "implicit_type" => {
                    self.expression_type(value?)
                }
                Some((type_node, _)) => self.resolve_type(type_node),
                None => self.type_named(expression.text()),
            },
            "object_creation_expression" | "cast_expression" => {
                self.resolve_type(expression.child_by_field_name("type")?)
            }
            "invocation_expression" => self.returned_type(expression),
            "member_access_expression" => self.member_type(expression),
            "parenthesized_expression" => self.expression_type(expression.named_child(0)?),
            _ => None,
        }
    }

    /// The type a call returns: a known member's return type, or the owner of
    /// a static `Create` factory.
    fn returned_type(&self, call: AstNode<'_>) -> Option<String> {
        let api = self.resolve_call(call)?;
        if let Some((_, returned)) = MEMBER_TYPES.iter().find(|(name, _)| *name == api) {
            return Some(returned.to_string());
        }
        let (owner, method) = api.rsplit_once('.')?;
        (method == "Create"
            && TYPES
                .iter()
                .any(|(class, namespace)| owner == format!("{namespace}.{class}")))
        .then(|| owner.to_string())
    }

    /// A dotted name that is itself a type, or a known member's type.
    fn member_type(&self, access: AstNode<'_>) -> Option<String> {
        let dotted = access.text().split_whitespace().collect::<String>();
        if let Some(qualified) = self.qualified_type(&dotted) {
            return Some(qualified);
        }
        let member = self.resolve_member(access)?;
        MEMBER_TYPES
            .iter()
            .find(|(name, _)| *name == member)
            .map(|(_, member_type)| member_type.to_string())
    }

    /// Every call and object creation in `root` with its resolved identity,
    /// in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        crate::api_identity::resolved_calls(
            root,
            &["invocation_expression", "object_creation_expression"],
            |call| self.resolve_call(call),
        )
    }

    /// Every `Type.Member` access in `root` with its resolved identity.
    pub(crate) fn members<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        ast_scan::nodes_of_kind(root, &["member_access_expression"])
            .into_iter()
            .filter_map(|access| Some((self.resolve_member(access)?, access)))
            .collect()
    }
}

/// The type node and initializer of the variable `identifier` refers to: the
/// nearest enclosing local declared before the use, a parameter, or a field.
pub(crate) fn csharp_variable(
    identifier: AstNode<'_>,
) -> Option<(AstNode<'_>, Option<AstNode<'_>>)> {
    let name = identifier.text();
    let mut node = identifier;
    while let Some(scope) = node.parent() {
        for child in scope.named_children() {
            if child.kind() == "parameter_list" {
                if let Some(parameter) = child.named_children().find(|parameter| {
                    parameter
                        .child_by_field_name("name")
                        .is_some_and(|binding| binding.text() == name)
                }) {
                    return Some((parameter.child_by_field_name("type")?, None));
                }
            }
            if let Some(binding) = visible_declaration(child, identifier)
                .and_then(|declaration| declared_variable(declaration, name))
            {
                return Some(binding);
            }
        }
        node = scope;
    }
    None
}

/// The `variable_declaration` of a field, or of a local declared before
/// `identifier`.
fn visible_declaration<'a>(child: AstNode<'a>, identifier: AstNode<'_>) -> Option<AstNode<'a>> {
    let before = child.end_byte() <= identifier.start_byte();
    match child.kind() {
        "field_declaration" => {}
        "local_declaration_statement" if before => {}
        "variable_declaration" if before => return Some(child),
        _ => return None,
    }
    child
        .named_children()
        .find(|part| part.kind() == "variable_declaration")
}

/// `(type, initializer)` of `name` in a C# `variable_declaration`.
fn declared_variable<'a>(
    declaration: AstNode<'a>,
    name: &str,
) -> Option<(AstNode<'a>, Option<AstNode<'a>>)> {
    let type_node = declaration.child_by_field_name("type")?;
    let declarator = declaration
        .named_children()
        .filter(|part| part.kind() == "variable_declarator")
        .find(|declarator| {
            declarator
                .child_by_field_name("name")
                .is_some_and(|binding| binding.text() == name)
        })?;
    let value = declarator
        .named_children()
        .find(|part| part.kind() != "identifier" && part.kind() != "bracketed_argument_list");
    Some((type_node, value))
}

/// The value of a C# string literal without escapes or interpolation.
pub(crate) fn csharp_string(node: AstNode<'_>) -> Option<String> {
    match node.kind() {
        // A verbatim string is one token: `@"C:\dir"`, with `""` for a quote.
        "verbatim_string_literal" => Some(verbatim_content(node.text())?.replace("\"\"", "\"")),
        "string_literal" | "raw_string_literal" => {
            let mut parts = node.named_children();
            match (parts.next(), parts.next()) {
                (None, _) => Some(String::new()),
                (Some(content), None) if content.kind().ends_with("content") => {
                    Some(content.text().to_string())
                }
                _ => {
                    let text = node.text().trim_start_matches('@');
                    (!text.contains('\\') && text.len() >= 2)
                        .then(|| text[1..text.len() - 1].to_string())
                }
            }
        }
        _ => None,
    }
}

/// The text between a verbatim string's `@"` and closing quote.
pub(crate) fn verbatim_content(text: &str) -> Option<&str> {
    let text = text.trim_end_matches(['u', 'U', '8']);
    text.strip_prefix("@\"")?.strip_suffix('"')
}

/// The argument expressions of a call or object creation, with their names
/// (`key: value`), in order.
pub(crate) fn csharp_arguments(call: AstNode<'_>) -> Vec<(Option<&str>, AstNode<'_>)> {
    call.child_by_field_name("arguments")
        .map(|list| {
            list.named_children()
                .filter(|argument| argument.kind() == "argument")
                .filter_map(|argument| {
                    let name = argument.child_by_field_name("name").map(|name| name.text());
                    let value = argument
                        .named_children()
                        .filter(|child| {
                            argument
                                .child_by_field_name("name")
                                .is_none_or(|name| name.start_byte() != child.start_byte())
                        })
                        .last()?;
                    Some((name, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The string values of a call's arguments, `None` where an argument is not
/// a plain string literal.
pub(crate) fn csharp_string_arguments(call: AstNode<'_>) -> Vec<Option<String>> {
    csharp_arguments(call)
        .into_iter()
        .map(|(_, value)| csharp_string(value))
        .collect()
}

/// The method, constructor, or local function enclosing `node`.
pub(crate) fn enclosing_csharp_method(node: AstNode<'_>) -> Option<AstNode<'_>> {
    std::iter::successors(node.parent(), |parent| parent.parent()).find(|ancestor| {
        matches!(
            ancestor.kind(),
            "method_declaration" | "constructor_declaration" | "local_function_statement"
        )
    })
}

/// Whether a declaration carries the `public` modifier.
pub(crate) fn csharp_is_public(declaration: AstNode<'_>) -> bool {
    declaration
        .named_children()
        .any(|child| child.kind() == "modifier" && child.text() == "public")
}

/// Whether the file is a program entry point: a `Main` method or top-level
/// statements, whose standard streams and exit code are its interface.
pub(crate) fn declares_csharp_entry_point(root: AstNode<'_>) -> bool {
    root.named_children()
        .any(|child| child.kind() == "global_statement")
        || ast_scan::nodes_of_kind(root, &["method_declaration"])
            .into_iter()
            .any(|method| {
                method
                    .child_by_field_name("name")
                    .is_some_and(|name| name.text() == "Main")
                    && method
                        .named_children()
                        .any(|child| child.kind() == "modifier" && child.text() == "static")
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"using System.Diagnostics;
using Crypto = System.Security.Cryptography;
using System.Net;

namespace Demo
{
    public class Service
    {
        private readonly Random random = new Random();

        public void Run(string line, ServicePointManager manager)
        {
            var md = Crypto.MD5.Create();
            md.ComputeHash(null);
            Process.Start("cmd.exe", "/c " + line);
            Console.WriteLine(line);
            Environment.Exit(1);
            random.Next();
            System.IO.File.OpenRead(line);
            var any = IPAddress.Any;
        }
    }
}
"#;

    #[test]
    fn csharp_calls_resolve_through_usings_aliases_and_factories() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::CSharp, SOURCE)
            .unwrap();
        let scope = CSharpScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "System.Random..ctor",
                "System.Security.Cryptography.MD5.Create",
                "System.Security.Cryptography.MD5.ComputeHash",
                "System.Diagnostics.Process.Start",
                "System.Console.WriteLine",
                "System.Environment.Exit",
                "System.Random.Next",
                "System.IO.File.OpenRead",
            ]
        );
        assert!(scope
            .members(tree.root_node())
            .iter()
            .any(|(name, _)| name == "System.Net.IPAddress.Any"));
        assert!(!declares_csharp_entry_point(tree.root_node()));
    }

    #[test]
    fn verbatim_strings_read_their_content() {
        let source = "class C { void F() { G(@\"C:\\tmp\\\"\"x\"\"\", \"plain\", @\"\"); } }\n";
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::CSharp, source)
            .unwrap();
        let call = ast_scan::nodes_of_kind(tree.root_node(), &["invocation_expression"])[0];
        assert_eq!(
            csharp_string_arguments(call),
            [
                Some("C:\\tmp\\\"x\"".to_string()),
                Some("plain".to_string()),
                Some(String::new()),
            ]
        );
    }

    #[test]
    fn a_declared_type_shadows_bcl_bindings() {
        let source = "using System;\nclass Random { }\nclass C { void F() { new Random(); } }\n";
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::CSharp, source)
            .unwrap();
        assert!(CSharpScope::new(tree.root_node())
            .calls(tree.root_node())
            .is_empty());
    }
}
