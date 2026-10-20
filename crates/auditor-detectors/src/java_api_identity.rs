//! Resolved API identity for Java: the fully qualified name a call denotes.
//! `System.exit(1)` is `java.lang.System.exit`, `new Random()` is
//! `java.util.Random.<init>`, and `rt.exec(..)` after
//! `Runtime rt = Runtime.getRuntime()` is `java.lang.Runtime.exec`.
//!
//! Type names resolve through single-type imports, static imports,
//! `java.lang`, and wildcard imports of packages in a small JDK class index.
//! Receivers resolve through declared variable types (locals, parameters,
//! resources, fields; `var` through its initializer) and a table of JDK
//! return types for call chains. A class the file declares shadows any
//! implicit or wildcard binding, so a local `Random` is never `java.util`'s.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::java_syntax::compact;
pub(crate) use crate::java_syntax::{
    declares_java_main, enclosing_java_method, is_java_main_method, java_arguments, java_string,
    java_string_arguments,
};

/// `java.lang` classes the API tables name; they need no import.
pub(crate) const JAVA_LANG: &[&str] = &[
    "Boolean",
    "Character",
    "Double",
    "Exception",
    "Float",
    "IllegalArgumentException",
    "IllegalStateException",
    "Integer",
    "Iterable",
    "Long",
    "Math",
    "Object",
    "ProcessBuilder",
    "Runtime",
    "RuntimeException",
    "String",
    "StringBuilder",
    "System",
    "Thread",
    "UnsupportedOperationException",
];

/// JDK classes a wildcard import (`import java.util.*;`) may bring in.
pub(crate) const WILDCARD_CLASSES: &[(&str, &str)] = &[
    ("ArrayDeque", "java.util"),
    ("ArrayList", "java.util"),
    ("Arrays", "java.util"),
    ("Collection", "java.util"),
    ("Collections", "java.util"),
    ("HashMap", "java.util"),
    ("HashSet", "java.util"),
    ("LinkedHashMap", "java.util"),
    ("LinkedHashSet", "java.util"),
    ("LinkedList", "java.util"),
    ("List", "java.util"),
    ("Map", "java.util"),
    ("Objects", "java.util"),
    ("Properties", "java.util"),
    ("Random", "java.util"),
    ("Set", "java.util"),
    ("TreeMap", "java.util"),
    ("TreeSet", "java.util"),
    ("Optional", "java.util"),
    ("CompletableFuture", "java.util.concurrent"),
    ("Executor", "java.util.concurrent"),
    ("ExecutorService", "java.util.concurrent"),
    ("Executors", "java.util.concurrent"),
    ("ThreadLocalRandom", "java.util.concurrent"),
    ("Collectors", "java.util.stream"),
    ("Stream", "java.util.stream"),
    ("Pattern", "java.util.regex"),
    ("BufferedReader", "java.io"),
    ("File", "java.io"),
    ("FileInputStream", "java.io"),
    ("FileOutputStream", "java.io"),
    ("FileReader", "java.io"),
    ("FileWriter", "java.io"),
    ("ObjectInputStream", "java.io"),
    ("PrintStream", "java.io"),
    ("RandomAccessFile", "java.io"),
    ("Files", "java.nio.file"),
    ("Path", "java.nio.file"),
    ("Paths", "java.nio.file"),
    ("PosixFilePermission", "java.nio.file.attribute"),
    ("PosixFilePermissions", "java.nio.file.attribute"),
    ("InetAddress", "java.net"),
    ("InetSocketAddress", "java.net"),
    ("ServerSocket", "java.net"),
    ("MessageDigest", "java.security"),
    ("SecureRandom", "java.security"),
    ("Cipher", "javax.crypto"),
    ("KeyGenerator", "javax.crypto"),
    ("HostnameVerifier", "javax.net.ssl"),
    ("HttpsURLConnection", "javax.net.ssl"),
    ("SSLContext", "javax.net.ssl"),
    ("X509TrustManager", "javax.net.ssl"),
    ("DocumentBuilderFactory", "javax.xml.parsers"),
    ("SAXParserFactory", "javax.xml.parsers"),
    ("XMLInputFactory", "javax.xml.stream"),
    ("TransformerFactory", "javax.xml.transform"),
    ("ScriptEngine", "javax.script"),
    ("ScriptEngineManager", "javax.script"),
    ("XMLDecoder", "java.beans"),
    ("Connection", "java.sql"),
    ("DriverManager", "java.sql"),
    ("PreparedStatement", "java.sql"),
    ("Statement", "java.sql"),
];

/// Import bindings and declared classes of one Java file.
pub(crate) struct JavaScope {
    types: BTreeMap<String, String>,
    statics: BTreeMap<String, String>,
    wildcards: BTreeSet<String>,
    declared: BTreeSet<String>,
}

impl JavaScope {
    pub(crate) fn new(root: AstNode<'_>) -> Self {
        let mut scope = Self {
            types: BTreeMap::new(),
            statics: BTreeMap::new(),
            wildcards: BTreeSet::new(),
            declared: BTreeSet::new(),
        };
        for import in ast_scan::nodes_of_kind(root, &["import_declaration"]) {
            let Some(path) = import
                .named_children()
                .find(|child| matches!(child.kind(), "scoped_identifier" | "identifier"))
            else {
                continue;
            };
            let path = compact(path.text());
            let is_static = import.children().any(|child| child.kind() == "static");
            let is_wildcard = import
                .named_children()
                .any(|child| child.kind() == "asterisk");
            let Some((_, member)) = path.rsplit_once('.') else {
                continue;
            };
            match (is_static, is_wildcard) {
                (false, true) => {
                    scope.wildcards.insert(path);
                }
                (false, false) => {
                    scope.types.insert(member.to_string(), path.clone());
                }
                (true, false) => {
                    scope.statics.insert(member.to_string(), path.clone());
                }
                (true, true) => {}
            }
        }
        scope.declared.extend(ast_scan::declared_names(
            root,
            &[
                "class_declaration",
                "interface_declaration",
                "enum_declaration",
                "record_declaration",
            ],
        ));
        scope
    }

    /// The qualified member a statically imported name denotes.
    pub(crate) fn static_member(&self, name: &str) -> Option<String> {
        self.statics.get(name).cloned()
    }

    /// The qualified name a simple type name denotes in this file.
    pub(crate) fn type_named(&self, name: &str) -> Option<String> {
        if let Some(qualified) = self.types.get(name) {
            return Some(qualified.clone());
        }
        if self.declared.contains(name) {
            return None;
        }
        if JAVA_LANG.contains(&name) {
            return Some(format!("java.lang.{name}"));
        }
        WILDCARD_CLASSES
            .iter()
            .find(|(class, package)| *class == name && self.wildcards.contains(*package))
            .map(|(class, package)| format!("{package}.{class}"))
    }

    /// The qualified name of a type node (`Random`, `List<String>`,
    /// `java.util.Random`, `Map.Entry`).
    pub(crate) fn resolve_type(&self, node: AstNode<'_>) -> Option<String> {
        match node.kind() {
            "type_identifier" | "identifier" => self.type_named(node.text()),
            "generic_type" => self.resolve_type(node.named_child(0)?),
            "scoped_type_identifier" | "scoped_identifier" | "field_access" => {
                self.qualified_name(&compact(node.text()))
            }
            _ => None,
        }
    }

    /// A dotted name as a type: fully qualified when its first segment is a
    /// package (`java.util.Random`), or nested in a resolved outer type.
    pub(crate) fn qualified_name(&self, dotted: &str) -> Option<String> {
        let (first, rest) = dotted.split_once('.')?;
        if first.starts_with(|c: char| c.is_ascii_lowercase()) {
            return dotted
                .rsplit('.')
                .next()
                .is_some_and(|last| last.starts_with(|c: char| c.is_ascii_uppercase()))
                .then(|| dotted.to_string());
        }
        Some(format!("{}.{rest}", self.type_named(first)?))
    }

    /// `pkg.Type.method` for a method call or `pkg.Type.<init>` for a
    /// constructor; a call on an unresolved receiver has no identity.
    /// An unqualified call resolves through a static import or to its name.
    pub(crate) fn resolve_call(&self, call: AstNode<'_>) -> Option<String> {
        match call.kind() {
            "method_invocation" => {
                let name = call.child_by_field_name("name")?.text();
                match call.child_by_field_name("object") {
                    None => Some(
                        self.statics
                            .get(name)
                            .cloned()
                            .unwrap_or_else(|| name.to_string()),
                    ),
                    Some(object) => Some(format!("{}.{name}", self.expression_type(object)?)),
                }
            }
            "object_creation_expression" => Some(format!(
                "{}.<init>",
                self.resolve_type(call.child_by_field_name("type")?)?
            )),
            _ => None,
        }
    }

    /// Every method call and constructor in `root` with its resolved identity,
    /// in source order.
    pub(crate) fn calls<'a>(&self, root: AstNode<'a>) -> Vec<(String, AstNode<'a>)> {
        crate::api_identity::resolved_calls(
            root,
            &["method_invocation", "object_creation_expression"],
            |call| self.resolve_call(call),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    const SOURCE: &str = r#"package demo;

import java.util.*;
import java.security.MessageDigest;
import static java.lang.System.exit;

class Demo {
    private final Runtime runtime = Runtime.getRuntime();

    void run(String line, javax.script.ScriptEngine engine) throws Exception {
        Runtime.getRuntime().exec(line);
        var digest = MessageDigest.getInstance("MD5");
        digest.update(line.getBytes());
        Random random = new Random();
        random.nextInt();
        System.out.println(line);
        java.lang.System.exit(2);
        exit(1);
        this.runtime.halt(3);
        engine.eval(line);
        list.sort(null);
        try (java.io.FileInputStream in = new java.io.FileInputStream(line)) { in.read(); }
    }
}
"#;

    #[test]
    fn java_calls_resolve_through_imports_declarations_and_return_types() {
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Java, SOURCE)
            .unwrap();
        let scope = JavaScope::new(tree.root_node());
        let calls = scope
            .calls(tree.root_node())
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(
            calls,
            [
                "java.lang.Runtime.getRuntime",
                "java.lang.Runtime.exec",
                "java.lang.Runtime.getRuntime",
                "java.security.MessageDigest.getInstance",
                "java.security.MessageDigest.update",
                "java.lang.String.getBytes",
                "java.util.Random.<init>",
                "java.util.Random.nextInt",
                "java.io.PrintStream.println",
                "java.lang.System.exit",
                "java.lang.System.exit",
                "java.lang.Runtime.halt",
                "javax.script.ScriptEngine.eval",
                "java.io.FileInputStream.<init>",
                "java.io.FileInputStream.read",
            ]
        );
        assert!(!declares_java_main(tree.root_node()));
    }

    #[test]
    fn a_declared_class_shadows_implicit_and_wildcard_bindings() {
        let source = "import java.util.*;\nclass Random { void f() { new Random(); System.exit(0); } }\nclass System {}\n";
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Java, source)
            .unwrap();
        let scope = JavaScope::new(tree.root_node());
        assert!(scope.calls(tree.root_node()).is_empty());
    }
}
