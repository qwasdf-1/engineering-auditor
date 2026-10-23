//! Java API tables for L1 rules: findings-free site finders over resolved
//! API identity (`java_api_identity.rs`). Each owner constructs its own
//! findings from the sites returned here. Semantics follow PMD where a PMD
//! rule exists (`DoNotTerminateVM`, `SystemPrintln`, `CloseResource`) and the
//! FindSecBugs patterns otherwise.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::bound_reads::released;
use crate::java_api_identity::{
    declares_java_main, enclosing_java_method, is_java_main_method, java_arguments, java_string,
    java_string_arguments, JavaScope,
};
use crate::jca_algorithms::weak_jca_algorithm;

/// Calls in a Java file whose resolved identity is one of `apis`.
pub(crate) fn java_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(JavaScope::new(root).calls(root), apis)
}

/// `System.exit`, `Runtime.exit`, and `Runtime.halt` outside a `main`
/// method (PMD `DoNotTerminateVM`).
pub(crate) fn java_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const EXITS: &[&str] = &[
        "java.lang.System.exit",
        "java.lang.Runtime.exit",
        "java.lang.Runtime.halt",
    ];
    java_calls_to(root, EXITS)
        .into_iter()
        .filter(|call| !enclosing_java_method(*call).is_some_and(is_java_main_method))
        .collect()
}

/// `System.out.print*` and `System.err.print*` (PMD `SystemPrintln`). A file
/// that declares `main` is a program whose standard streams are its interface.
pub(crate) fn java_console_output(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    if declares_java_main(root) {
        return Vec::new();
    }
    let scope = JavaScope::new(root);
    scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            api.strip_prefix("java.io.PrintStream.print").is_some()
                && call
                    .child_by_field_name("object")
                    .is_some_and(|object| object.kind() == "field_access")
        })
        .map(|(_, call)| call)
        .collect()
}

/// A TODO comment, or an `UnsupportedOperationException` whose message says
/// the code is unfinished. A bare `UnsupportedOperationException` is the
/// JDK's contract for optional operations and is not reported.
pub(crate) fn java_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let stubs = java_calls_to(root, &["java.lang.UnsupportedOperationException.<init>"])
        .into_iter()
        .filter(|call| {
            java_arguments(*call)
                .first()
                .and_then(|message| java_string(*message))
                .is_some_and(|message| ast_scan::is_unimplemented_message(&message))
        });
    ast_scan::todo_nodes(root, &["line_comment", "block_comment"], stubs)
}

/// `ScriptEngine.eval` and Groovy `GroovyShell.evaluate`: source text
/// executed at runtime.
pub(crate) fn java_dynamic_evaluation(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    java_calls_to(
        root,
        &[
            "javax.script.ScriptEngine.eval",
            "groovy.lang.GroovyShell.evaluate",
        ],
    )
}

/// `new ProcessBuilder("sh", "-c", ..)`, `pb.command("bash", "-c", ..)`, or
/// `Runtime.exec(new String[] {"sh", "-c", ..})`: a command line handed to a
/// shell interpreter. `Runtime.exec(String)` tokenizes without a shell.
pub(crate) fn java_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const LAUNCHERS: &[&str] = &[
        "java.lang.ProcessBuilder.<init>",
        "java.lang.ProcessBuilder.command",
        "java.lang.Runtime.exec",
    ];
    java_calls_to(root, LAUNCHERS)
        .into_iter()
        .filter(|call| {
            let values = java_string_arguments(*call);
            match (values.first(), values.get(1)) {
                (Some(Some(program)), Some(Some(flag))) => is_shell_invocation(program, flag),
                _ => false,
            }
        })
        .collect()
}

/// A shell program followed by its command-string flag.
pub(crate) fn is_shell_invocation(program: &str, flag: &str) -> bool {
    const SHELLS: &[&str] = &[
        "sh",
        "bash",
        "zsh",
        "dash",
        "ksh",
        "cmd",
        "cmd.exe",
        "powershell",
        "pwsh",
    ];
    const COMMAND_FLAGS: &[&str] = &["-c", "/c", "/C", "-Command"];
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    SHELLS.contains(&program) && COMMAND_FLAGS.contains(&flag)
}

/// A socket address on the wildcard host: a literal `0.0.0.0` or `::`, or an
/// `InetSocketAddress` built from a port alone.
pub(crate) fn java_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = JavaScope::new(root);
    scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let arguments = java_arguments(*call);
            let wildcard_host = arguments
                .first()
                .and_then(|host| java_string(*host))
                .is_some_and(|host| matches!(host.as_str(), "0.0.0.0" | "::" | "[::]"));
            match api.as_str() {
                "java.net.InetSocketAddress.<init>" => {
                    wildcard_host || (arguments.len() == 1 && java_string(arguments[0]).is_none())
                }
                "java.net.InetAddress.getByName" => wildcard_host,
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// `PosixFilePermissions.fromString` with the others-write bit,
/// `PosixFilePermission.OTHERS_WRITE`, or `File.setWritable(true, false)`.
pub(crate) fn java_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = JavaScope::new(root);
    let mut sites = scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let arguments = java_arguments(*call);
            match api.as_str() {
                "java.nio.file.attribute.PosixFilePermissions.fromString" => arguments
                    .first()
                    .and_then(|mode| java_string(*mode))
                    .is_some_and(|mode| mode.chars().nth(7) == Some('w')),
                "java.io.File.setWritable" => {
                    arguments.len() == 2
                        && arguments[0].text() == "true"
                        && arguments[1].text() == "false"
                }
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["field_access", "identifier"])
            .into_iter()
            .filter(|node| {
                let qualified = match node.kind() {
                    "field_access" => node.child_by_field_name("object").and_then(|object| {
                        Some(format!(
                            "{}.{}",
                            scope.expression_type(object)?,
                            node.child_by_field_name("field")?.text()
                        ))
                    }),
                    _ => scope.static_member(node.text()),
                };
                qualified.as_deref()
                    == Some("java.nio.file.attribute.PosixFilePermission.OTHERS_WRITE")
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// JVM random sources whose output is predictable, shared by Java and Kotlin.
pub(crate) const JVM_PREDICTABLE_RANDOM: &[&str] = &[
    "java.util.Random.<init>",
    "java.lang.Math.random",
    "java.util.concurrent.ThreadLocalRandom.current",
];

/// `java.util.Random`, `Math.random`, `ThreadLocalRandom`, and Commons
/// `RandomStringUtils` are predictable generators; secrets need
/// `SecureRandom`.
pub(crate) fn java_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    JavaScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, _)| {
            JVM_PREDICTABLE_RANDOM.contains(&api.as_str())
                || api.starts_with("org.apache.commons.lang.RandomStringUtils.")
                || api.starts_with("org.apache.commons.lang3.RandomStringUtils.")
        })
        .map(|(_, call)| call)
        .collect()
}

/// Certificate and hostname checks switched off: a `HostnameVerifier` that
/// returns `true` (method or lambda), an `X509TrustManager` whose
/// `checkServerTrusted` is empty, or the Apache HttpClient no-op verifier
/// and trust-all strategy.
pub(crate) fn java_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const ACCEPT_ALL: &[&str] = &[
        "org.apache.http.conn.ssl.NoopHostnameVerifier.<init>",
        "org.apache.http.conn.ssl.TrustAllStrategy.<init>",
        "org.apache.hc.client5.http.conn.ssl.NoopHostnameVerifier.<init>",
        "org.apache.hc.client5.http.conn.ssl.TrustAllStrategy.<init>",
    ];
    let scope = JavaScope::new(root);
    let mut sites = ast_scan::nodes_of_kind(root, &["method_declaration"])
        .into_iter()
        .filter(|method| {
            let name = method.child_by_field_name("name").map(|name| name.text());
            let parameters = method
                .child_by_field_name("parameters")
                .map(|parameters| parameters.named_children().count())
                .unwrap_or_default();
            let body = method.child_by_field_name("body");
            match name {
                Some("verify") => parameters == 2 && body.is_some_and(ast_scan::returns_only_true),
                Some("checkServerTrusted") => {
                    body.is_some_and(|body| body.named_children().count() == 0)
                }
                _ => false,
            }
        })
        .collect::<Vec<_>>();
    for (api, call) in scope.calls(root) {
        if ACCEPT_ALL.contains(&api.as_str()) {
            sites.push(call);
            continue;
        }
        if !matches!(
            api.rsplit_once('.').map(|(_, method)| method),
            Some("setHostnameVerifier" | "setDefaultHostnameVerifier" | "setSSLHostnameVerifier")
        ) {
            continue;
        }
        let accepts_all = java_arguments(call).first().is_some_and(|argument| {
            argument.kind() == "lambda_expression"
                && argument
                    .child_by_field_name("body")
                    .is_some_and(|body| body.text() == "true" || ast_scan::returns_only_true(body))
        });
        if accepts_all {
            sites.push(call);
        }
    }
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `SSLContext.getInstance` or `setEnabledProtocols` naming SSL 3.0, TLS
/// 1.0, or TLS 1.1 (or the bare "SSL" family name).
pub(crate) fn java_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const WEAK: &[&str] = &["SSL", "SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];
    JavaScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let names_weak = || {
                java_string_arguments(*call)
                    .iter()
                    .flatten()
                    .any(|protocol| WEAK.contains(&protocol.as_str()))
            };
            match api.as_str() {
                "javax.net.ssl.SSLContext.getInstance" => java_arguments(*call)
                    .first()
                    .and_then(|protocol| java_string(*protocol))
                    .is_some_and(|protocol| WEAK.contains(&protocol.as_str())),
                _ => api.ends_with(".setEnabledProtocols") && names_weak(),
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// JSch `StrictHostKeyChecking=no` (through `setConfig` or a `Properties`
/// `put`/`setProperty`) and sshj's `PromiscuousVerifier`.
pub(crate) fn java_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = java_calls_to(
        root,
        &["net.schmizz.sshj.transport.verification.PromiscuousVerifier.<init>"],
    );
    sites.extend(
        ast_scan::nodes_of_kind(root, &["method_invocation"])
            .into_iter()
            .filter(|call| {
                let method = call.child_by_field_name("name").map(|name| name.text());
                matches!(method, Some("setConfig" | "put" | "setProperty"))
                    && java_string_arguments(*call)
                        .iter()
                        .map(Option::as_deref)
                        .eq([Some("StrictHostKeyChecking"), Some("no")])
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `new ObjectInputStream(..)` and `new XMLDecoder(..)`: Java
/// deserialization runs gadget code from the stream. An object stream whose
/// method installs a class filter (`setObjectInputFilter`, a filter
/// builder's `setFilter(in)`, or a `resolveClass` override) reads only the
/// classes it allows.
pub(crate) fn java_unsafe_deserialization(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const FILTERS: &[&str] = &["setObjectInputFilter", ".setFilter(", "resolveClass"];
    let decoders = java_calls_to(root, &["java.beans.XMLDecoder.<init>"]);
    let mut sites = java_calls_to(root, &["java.io.ObjectInputStream.<init>"])
        .into_iter()
        .filter(|call| {
            !enclosing_java_method(*call)
                .is_some_and(|method| FILTERS.iter().any(|token| method.text().contains(token)))
        })
        .chain(decoders)
        .collect::<Vec<_>>();
    sites.sort_by_key(|call| call.start_byte());
    sites
}

/// An XML parser factory created in a method that neither disables DOCTYPE
/// declarations nor restricts external entities, and that parses with it.
pub(crate) fn java_xxe_parsers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const FACTORIES: &[&str] = &[
        "javax.xml.parsers.DocumentBuilderFactory.newInstance",
        "javax.xml.parsers.SAXParserFactory.newInstance",
        "javax.xml.stream.XMLInputFactory.newInstance",
        "javax.xml.stream.XMLInputFactory.newFactory",
        "javax.xml.transform.TransformerFactory.newInstance",
        "javax.xml.validation.SchemaFactory.newInstance",
        "org.xml.sax.helpers.XMLReaderFactory.createXMLReader",
        "org.dom4j.io.SAXReader.<init>",
        "org.jdom2.input.SAXBuilder.<init>",
    ];
    const HARDENING: &[&str] = &[
        "disallow-doctype-decl",
        "external-general-entities",
        "external-parameter-entities",
        "load-external-dtd",
        "FEATURE_SECURE_PROCESSING",
        "ACCESS_EXTERNAL_DTD",
        "SUPPORT_DTD",
        "IS_SUPPORTING_EXTERNAL_ENTITIES",
        "setExpandEntityReferences",
    ];
    java_calls_to(root, FACTORIES)
        .into_iter()
        .filter(|call| {
            let Some(method) = enclosing_java_method(*call) else {
                return false;
            };
            let text = method.text();
            let parses = ast_scan::nodes_of_kind(method, &["method_invocation"])
                .into_iter()
                .any(reads_xml_input);
            parses && !HARDENING.iter().any(|token| text.contains(token))
        })
        .collect()
}

/// A call that parses XML from outside the program. A transformer parses
/// only a stylesheet it is built from or a `StreamSource` it transforms; an
/// in-memory DOM serialized through `newTransformer()` reads no entities.
fn reads_xml_input(invocation: AstNode<'_>) -> bool {
    let arguments = java_arguments(invocation);
    match invocation
        .child_by_field_name("name")
        .map(|name| name.text())
    {
        Some(
            "parse"
            | "createXMLStreamReader"
            | "createXMLEventReader"
            | "read"
            | "build"
            | "newSchema",
        ) => true,
        Some("newTransformer" | "newTemplates") => !arguments.is_empty(),
        Some("transform") => arguments
            .first()
            .is_some_and(|source| source.text().contains("StreamSource")),
        _ => false,
    }
}

/// `MessageDigest`, `Cipher`, `KeyGenerator`, `Mac`, and `Signature`
/// algorithms that are broken or default to ECB, and Commons Codec
/// MD5/SHA-1 helpers. Returns each site with its algorithm's display name.
pub(crate) fn java_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    const FACTORIES: &[&str] = &[
        "java.security.MessageDigest.getInstance",
        "javax.crypto.Cipher.getInstance",
        "javax.crypto.KeyGenerator.getInstance",
        "javax.crypto.Mac.getInstance",
        "java.security.Signature.getInstance",
    ];
    JavaScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            if let Some(helper) = api.strip_prefix("org.apache.commons.codec.digest.DigestUtils.") {
                let lower = helper.to_ascii_lowercase();
                let algorithm = if lower.starts_with("md5") || lower.starts_with("md2") {
                    "MD5"
                } else if lower.starts_with("sha1") || lower == "sha" || lower == "shahex" {
                    "SHA-1"
                } else {
                    return None;
                };
                return Some((call, algorithm));
            }
            if !FACTORIES.contains(&api.as_str()) {
                return None;
            }
            let transformation = java_arguments(call)
                .first()
                .and_then(|algorithm| java_string(*algorithm))?;
            weak_jca_algorithm(&transformation).map(|algorithm| (call, algorithm))
        })
        .collect()
}

/// `in = new FileInputStream(..)` (or another JDK file stream) assigned to a
/// local that is neither in try-with-resources, closed, nor handed on in the
/// same method. As for Go, any use other than calling a non-`close` method on
/// it (returning, storing, passing it) transfers ownership (PMD
/// `CloseResource`).
pub(crate) fn java_unclosed_streams(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const OPENERS: &[&str] = &[
        "java.io.FileInputStream.<init>",
        "java.io.FileOutputStream.<init>",
        "java.io.FileReader.<init>",
        "java.io.FileWriter.<init>",
        "java.io.RandomAccessFile.<init>",
        "java.nio.file.Files.newInputStream",
        "java.nio.file.Files.newOutputStream",
        "java.nio.file.Files.newBufferedReader",
        "java.nio.file.Files.newBufferedWriter",
        "java.nio.file.Files.lines",
        "java.nio.file.Files.list",
        "java.nio.file.Files.walk",
    ];
    let scope = JavaScope::new(root);
    let mut unclosed = Vec::new();
    for declaration in ast_scan::nodes_of_kind(root, &["local_variable_declaration"]) {
        for declarator in declaration
            .named_children()
            .filter(|child| child.kind() == "variable_declarator")
        {
            let (Some(name), Some(value)) = (
                declarator.child_by_field_name("name"),
                declarator.child_by_field_name("value"),
            ) else {
                continue;
            };
            let opens = scope
                .resolve_call(value)
                .is_some_and(|api| OPENERS.contains(&api.as_str()));
            if !opens || enclosing_java_method(declaration).is_none() {
                continue;
            }
            if !released(name, java_method_called_on, &["close"]) {
                unclosed.push(declaration);
            }
        }
    }
    unclosed.sort_by_key(|node| node.start_byte());
    unclosed.dedup_by_key(|node| node.start_byte());
    unclosed
}

/// The method name when `usage` is the receiver of a call (`in.read()`).
fn java_method_called_on(usage: AstNode<'_>) -> Option<&str> {
    let call = usage
        .parent()
        .filter(|parent| parent.kind() == "method_invocation")?;
    let object = call.child_by_field_name("object")?;
    (object.start_byte() == usage.start_byte() && object.end_byte() == usage.end_byte())
        .then(|| call.child_by_field_name("name").map(|name| name.text()))
        .flatten()
}
