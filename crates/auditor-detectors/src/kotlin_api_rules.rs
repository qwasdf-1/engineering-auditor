//! Kotlin API tables for L1 rules: findings-free site finders over resolved
//! API identity (`kotlin_api_identity.rs`). Kotlin on the JVM shares Java's
//! security APIs, so the semantics follow the Java tables (PMD and
//! FindSecBugs) with Kotlin's own forms: `exitProcess`, `println`, `TODO()`,
//! `.use {}` blocks, trailing lambdas, and named arguments.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::java_api_rules::{is_shell_invocation, JVM_PREDICTABLE_RANDOM};
use crate::jca_algorithms::weak_jca_algorithm;
use crate::kotlin_api_identity::{
    declares_kotlin_main, enclosing_kotlin_function, kotlin_arguments, kotlin_function_name,
    kotlin_is_restricted, kotlin_string_arguments, KotlinScope,
};

/// Calls in a Kotlin file whose resolved identity is one of `apis`.
pub(crate) fn kotlin_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(KotlinScope::new(root).calls(root), apis)
}

/// `exitProcess`, `System.exit`, `Runtime.exit`, and `Runtime.halt` outside
/// `fun main` (PMD `DoNotTerminateVM`).
pub(crate) fn kotlin_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const EXITS: &[&str] = &[
        "kotlin.system.exitProcess",
        "java.lang.System.exit",
        "java.lang.Runtime.exit",
        "java.lang.Runtime.halt",
    ];
    kotlin_calls_to(root, EXITS)
        .into_iter()
        .filter(|call| {
            enclosing_kotlin_function(*call).and_then(kotlin_function_name) != Some("main")
        })
        .collect()
}

/// `println`/`print` and `System.out`/`System.err` printing. A file that
/// declares `fun main` is a program whose standard streams are its interface.
pub(crate) fn kotlin_console_output(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    if declares_kotlin_main(root) {
        return Vec::new();
    }
    KotlinScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, _)| {
            matches!(api.as_str(), "kotlin.io.println" | "kotlin.io.print")
                || api.starts_with("java.io.PrintStream.print")
        })
        .map(|(_, call)| call)
        .collect()
}

/// A TODO comment, a `TODO()` call (it throws `NotImplementedError`), or a
/// thrown `NotImplementedError`.
pub(crate) fn kotlin_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let stubs = kotlin_calls_to(root, &["kotlin.TODO", "kotlin.NotImplementedError.<init>"]);
    ast_scan::todo_nodes(root, &["line_comment", "multiline_comment"], stubs)
}

/// `ScriptEngine.eval`: source text executed at runtime.
pub(crate) fn kotlin_dynamic_evaluation(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    kotlin_calls_to(root, &["javax.script.ScriptEngine.eval"])
}

/// `ProcessBuilder("sh", "-c", ..)`, `pb.command("bash", "-c", ..)`, or
/// `Runtime.exec(arrayOf("sh", "-c", ..))`: a command line handed to a shell.
pub(crate) fn kotlin_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const LAUNCHERS: &[&str] = &[
        "java.lang.ProcessBuilder.<init>",
        "java.lang.ProcessBuilder.command",
        "java.lang.Runtime.exec",
    ];
    kotlin_calls_to(root, LAUNCHERS)
        .into_iter()
        .filter(|call| {
            let mut values = kotlin_string_arguments(*call);
            // A single `arrayOf(..)`/`listOf(..)` argument holds the command.
            if let [(None, list)] = kotlin_arguments(*call).as_slice() {
                if list.kind() == "call_expression"
                    && matches!(
                        list.named_child(0).map(|callee| callee.text()),
                        Some("arrayOf" | "listOf" | "mutableListOf")
                    )
                {
                    values = kotlin_string_arguments(*list);
                }
            }
            matches!(
                (values.first(), values.get(1)),
                (Some(Some(program)), Some(Some(flag))) if is_shell_invocation(program, flag)
            )
        })
        .collect()
}

/// A socket address on the wildcard host: a literal `0.0.0.0` or `::`, an
/// `InetSocketAddress` from a port alone, or a Ktor server on that host.
pub(crate) fn kotlin_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let wildcard = |value: AstNode<'_>| {
        ast_scan::string_literal_content(value)
            .is_some_and(|host| matches!(host.as_str(), "0.0.0.0" | "::" | "[::]"))
    };
    KotlinScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let arguments = kotlin_arguments(*call);
            match api.as_str() {
                "java.net.InetSocketAddress.<init>" => match arguments.as_slice() {
                    [(None, port)] => ast_scan::string_literal_content(*port).is_none(),
                    [(_, host), ..] => wildcard(*host),
                    [] => false,
                },
                "java.net.InetAddress.getByName" => {
                    arguments.first().is_some_and(|(_, host)| wildcard(*host))
                }
                "io.ktor.server.engine.embeddedServer" | "embeddedServer" => arguments
                    .iter()
                    .any(|(name, value)| *name == Some("host") && wildcard(*value)),
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// `File.setWritable(true, false)`, `PosixFilePermissions.fromString` with
/// the others-write bit, or `PosixFilePermission.OTHERS_WRITE`.
pub(crate) fn kotlin_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = KotlinScope::new(root);
    let mut sites = scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let arguments = kotlin_arguments(*call);
            match api.as_str() {
                "java.nio.file.attribute.PosixFilePermissions.fromString" => arguments
                    .first()
                    .and_then(|(_, mode)| ast_scan::string_literal_content(*mode))
                    .is_some_and(|mode| mode.chars().nth(7) == Some('w')),
                "java.io.File.setWritable" => {
                    arguments.len() == 2
                        && arguments[0].1.text() == "true"
                        && arguments[1].1.text() == "false"
                }
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["navigation_expression"])
            .into_iter()
            .filter(|navigation| {
                navigation.text().ends_with(".OTHERS_WRITE")
                    && navigation.named_child(0).is_some_and(|owner| {
                        scope.expression_type(owner).as_deref()
                            == Some("java.nio.file.attribute.PosixFilePermission")
                    })
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `java.util.Random`, `Math.random`, `ThreadLocalRandom`, and
/// `kotlin.random.Random` are predictable; secrets need `SecureRandom`.
pub(crate) fn kotlin_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    KotlinScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, _)| {
            JVM_PREDICTABLE_RANDOM.contains(&api.as_str())
                || api.starts_with("kotlin.random.Random.")
        })
        .map(|(_, call)| call)
        .collect()
}

/// Certificate and hostname checks switched off: an empty
/// `checkServerTrusted` override, a `verify` that returns `true`, or a
/// hostname-verifier lambda that returns `true`.
pub(crate) fn kotlin_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = ast_scan::nodes_of_kind(root, &["function_declaration"])
        .into_iter()
        .filter(|function| {
            let body = function
                .named_children()
                .find(|child| child.kind() == "function_body");
            match kotlin_function_name(*function) {
                Some("checkServerTrusted") => {
                    body.is_some_and(|body| body.named_child_count() == 0)
                }
                Some("verify") => body.is_some_and(returns_only_true),
                _ => false,
            }
        })
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                let callee = call.named_child(0).map(|callee| match callee.kind() {
                    "navigation_expression" => callee.text().rsplit('.').next().unwrap_or(""),
                    _ => callee.text(),
                });
                matches!(
                    callee,
                    Some(
                        "hostnameVerifier"
                            | "setHostnameVerifier"
                            | "setDefaultHostnameVerifier"
                            | "HostnameVerifier"
                    )
                ) && kotlin_arguments(*call)
                    .last()
                    .and_then(|(_, argument)| {
                        ast_scan::nodes_of_kind(*argument, &["lambda_literal"])
                            .into_iter()
                            .next()
                    })
                    .is_some_and(returns_only_true)
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// A function body or lambda whose only statement is `true` (`= true`,
/// `{ return true }`, `{ _, _ -> true }`).
fn returns_only_true(body: AstNode<'_>) -> bool {
    let statements = body
        .named_children()
        .find(|child| child.kind() == "statements")
        .unwrap_or(body);
    let mut values = statements
        .named_children()
        .filter(|child| !matches!(child.kind(), "lambda_parameters" | "line_comment"));
    match (values.next(), values.next()) {
        (Some(value), None) => {
            value.text() == "true"
                || (value.kind() == "jump_expression"
                    && value.text().split_whitespace().eq(["return", "true"]))
        }
        _ => false,
    }
}

/// `SSLContext.getInstance` naming SSL 3.0, TLS 1.0, or TLS 1.1, or OkHttp's
/// `TlsVersion.TLS_1_0`/`TLS_1_1`/`SSL_3_0`.
pub(crate) fn kotlin_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const WEAK: &[&str] = &["SSL", "SSLv2", "SSLv3", "TLSv1", "TLSv1.1"];
    let mut sites = kotlin_calls_to(root, &["javax.net.ssl.SSLContext.getInstance"])
        .into_iter()
        .filter(|call| {
            kotlin_string_arguments(*call)
                .first()
                .and_then(Option::as_deref)
                .is_some_and(|protocol| WEAK.contains(&protocol))
        })
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["navigation_expression"])
            .into_iter()
            .filter(|navigation| {
                matches!(
                    navigation.text(),
                    "TlsVersion.TLS_1_0" | "TlsVersion.TLS_1_1" | "TlsVersion.SSL_3_0"
                )
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// JSch `StrictHostKeyChecking=no` and sshj's `PromiscuousVerifier`.
pub(crate) fn kotlin_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = kotlin_calls_to(
        root,
        &["net.schmizz.sshj.transport.verification.PromiscuousVerifier.<init>"],
    );
    sites.extend(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                let method = call
                    .named_child(0)
                    .map(|callee| callee.text().rsplit('.').next().unwrap_or(""));
                matches!(method, Some("setConfig" | "put" | "setProperty"))
                    && kotlin_string_arguments(*call)
                        .iter()
                        .map(Option::as_deref)
                        .eq([Some("StrictHostKeyChecking"), Some("no")])
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `ObjectInputStream(..)` and `XMLDecoder(..)`: Java deserialization runs
/// gadget code from the stream.
pub(crate) fn kotlin_unsafe_deserialization(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    kotlin_calls_to(
        root,
        &[
            "java.io.ObjectInputStream.<init>",
            "java.beans.XMLDecoder.<init>",
        ],
    )
}

/// An XML parser factory created in a function that parses with it and
/// neither disables DOCTYPE declarations nor restricts external entities.
pub(crate) fn kotlin_xxe_parsers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const FACTORIES: &[&str] = &[
        "javax.xml.parsers.DocumentBuilderFactory.newInstance",
        "javax.xml.parsers.SAXParserFactory.newInstance",
        "javax.xml.stream.XMLInputFactory.newInstance",
        "javax.xml.stream.XMLInputFactory.newFactory",
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
        "isExpandEntityReferences",
    ];
    const PARSES: &[&str] = &[
        ".parse(",
        ".createXMLStreamReader(",
        ".createXMLEventReader(",
    ];
    kotlin_calls_to(root, FACTORIES)
        .into_iter()
        .filter(|call| {
            enclosing_kotlin_function(*call).is_some_and(|function| {
                let text = function.text();
                PARSES.iter().any(|parse| text.contains(parse))
                    && !HARDENING.iter().any(|token| text.contains(token))
            })
        })
        .collect()
}

/// `MessageDigest`, `Cipher`, `KeyGenerator`, `Mac`, and `Signature`
/// algorithms that are broken or default to ECB, with the algorithm's name.
pub(crate) fn kotlin_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    const FACTORIES: &[&str] = &[
        "java.security.MessageDigest.getInstance",
        "javax.crypto.Cipher.getInstance",
        "javax.crypto.KeyGenerator.getInstance",
        "javax.crypto.Mac.getInstance",
        "java.security.Signature.getInstance",
    ];
    kotlin_calls_to(root, FACTORIES)
        .into_iter()
        .filter_map(|call| {
            let transformation = kotlin_string_arguments(call).into_iter().next()??;
            weak_jca_algorithm(&transformation).map(|algorithm| (call, algorithm))
        })
        .collect()
}

/// `val input = FileInputStream(..)` (or another JDK file stream) that is
/// neither closed, used in a `.use {}` block, nor handed on in the same
/// function: any other use (returning, storing, passing it) transfers
/// ownership, as for Java (PMD `CloseResource`).
pub(crate) fn kotlin_unclosed_streams(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const OPENERS: &[&str] = &[
        "java.io.FileInputStream.<init>",
        "java.io.FileOutputStream.<init>",
        "java.io.FileReader.<init>",
        "java.io.FileWriter.<init>",
        "java.io.RandomAccessFile.<init>",
    ];
    let scope = KotlinScope::new(root);
    ast_scan::nodes_of_kind(root, &["property_declaration"])
        .into_iter()
        .filter(|declaration| {
            let Some(name) = declaration
                .named_children()
                .find(|child| child.kind() == "variable_declaration")
                .and_then(|variable| variable.named_child(0))
            else {
                return false;
            };
            let opens = declaration.named_children().any(|child| {
                scope
                    .resolve_call(child)
                    .is_some_and(|api| OPENERS.contains(&api.as_str()))
            });
            let Some(function) = enclosing_kotlin_function(*declaration) else {
                return false;
            };
            opens
                && !ast_scan::nodes_of_kind(function, &["simple_identifier"])
                    .into_iter()
                    .filter(|usage| {
                        usage.start_byte() > declaration.end_byte() && usage.text() == name.text()
                    })
                    .any(|usage| {
                        kotlin_method_called_on(usage)
                            .is_none_or(|method| method == "close" || method == "use")
                    })
        })
        .collect()
}

/// The method name when `usage` is the receiver of a call (`input.read()`).
fn kotlin_method_called_on(usage: AstNode<'_>) -> Option<&str> {
    let navigation = usage
        .parent()
        .filter(|parent| parent.kind() == "navigation_expression")?;
    if navigation.named_child(0)?.start_byte() != usage.start_byte() {
        return None;
    }
    let call = navigation
        .parent()
        .filter(|parent| parent.kind() == "call_expression")?;
    (call.named_child(0)?.start_byte() == navigation.start_byte())
        .then(|| navigation.text().rsplit('.').next())
        .flatten()
}

/// `assert(..)` on a parameter of a public function. Kotlin's `assert` runs
/// only under the JVM's `-ea`; argument contracts belong to `require`.
pub(crate) fn kotlin_public_argument_asserts(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    kotlin_calls_to(root, &["kotlin.assert"])
        .into_iter()
        .filter(|call| {
            let Some(function) = enclosing_kotlin_function(*call) else {
                return false;
            };
            let parameters = function
                .named_children()
                .find(|child| child.kind() == "function_value_parameters")
                .map(|parameters| {
                    parameters
                        .named_children()
                        .filter_map(|parameter| parameter.named_child(0))
                        .map(|name| name.text())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            !kotlin_is_restricted(function)
                && kotlin_arguments(*call)
                    .first()
                    .is_some_and(|(_, condition)| {
                        ast_scan::mentions_any(*condition, "simple_identifier", &parameters)
                    })
        })
        .collect()
}
