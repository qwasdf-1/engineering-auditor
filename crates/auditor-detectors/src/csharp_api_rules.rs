//! C# API tables for L1 rules: findings-free site finders over resolved API
//! identity (`csharp_api_identity.rs`). Semantics follow the .NET security
//! analyzers (CA2300-series deserializers, CA5350/CA5351 weak algorithms,
//! CA5359 disabled certificate validation, CA5397/CA5398 TLS versions,
//! CA3075 DTD processing) and their PMD counterparts for termination and
//! console output.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::csharp_api_identity::{
    csharp_arguments, csharp_is_public, csharp_string, csharp_string_arguments,
    declares_csharp_entry_point, enclosing_csharp_method, CSharpScope,
};
use crate::java_api_rules::is_shell_invocation;

/// Calls and object creations whose resolved identity is one of `apis`.
pub(crate) fn csharp_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(CSharpScope::new(root).calls(root), apis)
}

/// `Type.Member` accesses whose resolved identity is one of `members`.
fn csharp_members<'a>(root: AstNode<'a>, members: &[&str]) -> Vec<AstNode<'a>> {
    CSharpScope::new(root)
        .members(root)
        .into_iter()
        .filter(|(member, _)| members.contains(&member.as_str()))
        .map(|(_, access)| access)
        .collect()
}

/// `Environment.Exit` outside `Main` (PMD `DoNotTerminateVM`).
pub(crate) fn csharp_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_calls_to(root, &["System.Environment.Exit"])
        .into_iter()
        .filter(|call| {
            enclosing_csharp_method(*call)
                .and_then(|method| method.child_by_field_name("name"))
                .is_none_or(|name| name.text() != "Main")
                && !declares_csharp_entry_point(root)
        })
        .collect()
}

/// `Console.Write`/`WriteLine`. A program entry point (a `Main` method or
/// top-level statements) talks to its user through standard streams.
pub(crate) fn csharp_console_output(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    if declares_csharp_entry_point(root) {
        return Vec::new();
    }
    csharp_calls_to(root, &["System.Console.Write", "System.Console.WriteLine"])
}

/// A TODO comment, or a `NotImplementedException`, the .NET marker for
/// unfinished code (`NotSupportedException` is the contract for optional
/// operations and is not reported).
pub(crate) fn csharp_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let stubs = csharp_calls_to(root, &["System.NotImplementedException..ctor"]);
    ast_scan::todo_nodes(root, &["comment"], stubs)
}

/// Roslyn scripting (`CSharpScript.EvaluateAsync`/`RunAsync`): source text
/// executed at runtime.
pub(crate) fn csharp_dynamic_evaluation(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_calls_to(
        root,
        &[
            "Microsoft.CodeAnalysis.CSharp.Scripting.CSharpScript.EvaluateAsync",
            "Microsoft.CodeAnalysis.CSharp.Scripting.CSharpScript.RunAsync",
        ],
    )
}

/// `Process.Start("cmd.exe", "/c ..")` or a `ProcessStartInfo` whose file
/// is a shell and whose arguments start with its command flag.
pub(crate) fn csharp_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let starts_with_flag = |program: &str, arguments: &str| {
        arguments
            .split_whitespace()
            .next()
            .is_some_and(|flag| is_shell_invocation(program, flag))
    };
    let scope = CSharpScope::new(root);
    scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| match api.as_str() {
            "System.Diagnostics.Process.Start" | "System.Diagnostics.ProcessStartInfo..ctor" => {
                let arguments = csharp_arguments(*call);
                let program = arguments
                    .first()
                    .and_then(|(_, value)| csharp_string(*value));
                let flags = arguments.get(1).and_then(|(_, value)| {
                    csharp_string(*value).or_else(|| {
                        (value.kind() == "binary_expression")
                            .then(|| value.child_by_field_name("left").and_then(csharp_string))
                            .flatten()
                    })
                });
                let initialized = initializer_strings(*call);
                let program = program.or_else(|| initialized_value(&initialized, "FileName"));
                let flags = flags.or_else(|| initialized_value(&initialized, "Arguments"));
                program
                    .zip(flags)
                    .is_some_and(|(program, flags)| starts_with_flag(&program, &flags))
            }
            _ => false,
        })
        .map(|(_, call)| call)
        .collect()
}

/// `Name = "literal"` pairs of an object initializer; a concatenation keeps
/// its leading literal.
fn initializer_strings(creation: AstNode<'_>) -> Vec<(String, String)> {
    let Some(initializer) = creation.child_by_field_name("initializer") else {
        return Vec::new();
    };
    initializer
        .named_children()
        .filter(|child| child.kind() == "assignment_expression")
        .filter_map(|assignment| {
            let name = assignment.child_by_field_name("left")?.text().to_string();
            let value = assignment.child_by_field_name("right")?;
            let value = csharp_string(value).or_else(|| {
                (value.kind() == "binary_expression")
                    .then(|| value.child_by_field_name("left").and_then(csharp_string))
                    .flatten()
            })?;
            Some((name, value))
        })
        .collect()
}

fn initialized_value(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

/// A listener on every interface: `IPAddress.Any`/`IPv6Any` handed to a
/// listener or endpoint, Kestrel's `ListenAnyIP`, or a URL on `0.0.0.0`,
/// `*`, or `+`.
pub(crate) fn csharp_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = CSharpScope::new(root);
    let mut sites = scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let arguments = csharp_arguments(*call);
            let any_address = || {
                arguments.first().is_some_and(|(_, address)| {
                    matches!(
                        scope.resolve_member(*address).as_deref(),
                        Some("System.Net.IPAddress.Any" | "System.Net.IPAddress.IPv6Any")
                    )
                })
            };
            let wildcard_url = || {
                csharp_string_arguments(*call).iter().flatten().any(|url| {
                    [
                        "http://0.0.0.0",
                        "https://0.0.0.0",
                        "http://*",
                        "https://*",
                        "http://+",
                        "https://+",
                    ]
                    .iter()
                    .any(|prefix| url.starts_with(prefix))
                })
            };
            match api.as_str() {
                "System.Net.Sockets.TcpListener..ctor" | "System.Net.IPEndPoint..ctor" => {
                    any_address()
                }
                _ => {
                    let method = api.rsplit('.').next().unwrap_or_default();
                    method == "ListenAnyIP" || (method == "UseUrls" && wildcard_url())
                }
            }
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    // `UseUrls` and `ListenAnyIP` are extension methods on builder receivers
    // the resolver cannot type.
    sites.extend(
        ast_scan::nodes_of_kind(root, &["invocation_expression"])
            .into_iter()
            .filter(|call| {
                let method = call
                    .child_by_field_name("function")
                    .and_then(|function| function.child_by_field_name("name"))
                    .map(|name| name.text());
                scope.resolve_call(*call).is_none()
                    && (method == Some("ListenAnyIP")
                        || (method == Some("UseUrls")
                            && csharp_string_arguments(*call).iter().flatten().any(|url| {
                                url.contains("://0.0.0.0")
                                    || url.contains("://*")
                                    || url.contains("://+")
                            })))
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites.dedup_by_key(|node| node.start_byte());
    sites
}

/// `File.SetUnixFileMode` with `UnixFileMode.OtherWrite` in its mode.
pub(crate) fn csharp_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_calls_to(root, &["System.IO.File.SetUnixFileMode"])
        .into_iter()
        .filter(|call| {
            csharp_arguments(*call).get(1).is_some_and(|(_, mode)| {
                mode.text()
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word == "OtherWrite")
            })
        })
        .collect()
}

/// `System.Random` is predictable; secrets need `RandomNumberGenerator`.
pub(crate) fn csharp_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = csharp_calls_to(root, &["System.Random..ctor"]);
    sites.extend(csharp_members(root, &["System.Random.Shared"]));
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// Certificate validation switched off (CA5359): a validation callback
/// that returns `true`, or `DangerousAcceptAnyServerCertificateValidator`.
pub(crate) fn csharp_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const CALLBACKS: &[&str] = &[
        "ServerCertificateCustomValidationCallback",
        "ServerCertificateValidationCallback",
        "RemoteCertificateValidationCallback",
        "CertificateValidationCallback",
    ];
    let mut sites = ast_scan::nodes_of_kind(root, &["assignment_expression"])
        .into_iter()
        .filter(|assignment| {
            let target = assignment.child_by_field_name("left").map(|left| {
                left.text()
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            });
            let value = assignment.child_by_field_name("right");
            target.is_some_and(|target| CALLBACKS.contains(&target.as_str()))
                && value.is_some_and(|value| {
                    value
                        .text()
                        .ends_with("DangerousAcceptAnyServerCertificateValidator")
                        || accepts_everything(value)
                })
        })
        .collect::<Vec<_>>();
    sites.extend(
        csharp_calls_to(
            root,
            &["System.Net.Security.RemoteCertificateValidationCallback..ctor"],
        )
        .into_iter()
        .filter(|creation| {
            csharp_arguments(*creation)
                .first()
                .is_some_and(|(_, callback)| accepts_everything(*callback))
        }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// A lambda whose body is `true` or a block that only returns `true`.
fn accepts_everything(value: AstNode<'_>) -> bool {
    if value.kind() != "lambda_expression" {
        return false;
    }
    let Some(body) = value.child_by_field_name("body") else {
        return false;
    };
    body.text() == "true" || ast_scan::returns_only_true(body)
}

/// SSL 2/3 or TLS 1.0/1.1 named through `SslProtocols` or
/// `SecurityProtocolType` (CA5397, CA5398).
pub(crate) fn csharp_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_members(
        root,
        &[
            "System.Security.Authentication.SslProtocols.Ssl2",
            "System.Security.Authentication.SslProtocols.Ssl3",
            "System.Security.Authentication.SslProtocols.Tls",
            "System.Security.Authentication.SslProtocols.Tls11",
            "System.Net.SecurityProtocolType.Ssl3",
            "System.Net.SecurityProtocolType.Tls",
            "System.Net.SecurityProtocolType.Tls11",
        ],
    )
}

/// SSH.NET's `HostKeyReceived` handler trusting every key
/// (`e.CanTrust = true`).
pub(crate) fn csharp_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["assignment_expression"])
        .into_iter()
        .filter(|assignment| {
            assignment
                .child_by_field_name("left")
                .is_some_and(|left| left.text().ends_with(".CanTrust"))
                && assignment
                    .child_by_field_name("right")
                    .is_some_and(|right| right.text() == "true")
        })
        .collect()
}

/// Formatters that instantiate types named in the payload (CA2300-series):
/// `BinaryFormatter`, `SoapFormatter`, `NetDataContractSerializer`,
/// `LosFormatter`, `ObjectStateFormatter`, and Json.NET `TypeNameHandling`
/// other than `None`.
pub(crate) fn csharp_unsafe_deserialization(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = csharp_calls_to(
        root,
        &[
            "System.Runtime.Serialization.Formatters.Binary.BinaryFormatter..ctor",
            "System.Runtime.Serialization.Formatters.Soap.SoapFormatter..ctor",
            "System.Runtime.Serialization.NetDataContractSerializer..ctor",
            "System.Web.UI.LosFormatter..ctor",
            "System.Web.UI.ObjectStateFormatter..ctor",
        ],
    );
    sites.extend(csharp_members(
        root,
        &[
            "Newtonsoft.Json.TypeNameHandling.All",
            "Newtonsoft.Json.TypeNameHandling.Auto",
            "Newtonsoft.Json.TypeNameHandling.Objects",
            "Newtonsoft.Json.TypeNameHandling.Arrays",
        ],
    ));
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// DTD processing enabled (CA3075): `DtdProcessing.Parse`, or an
/// `XmlUrlResolver` assigned as a document's resolver.
pub(crate) fn csharp_xxe_parsers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = csharp_members(root, &["System.Xml.DtdProcessing.Parse"]);
    sites.extend(
        ast_scan::nodes_of_kind(root, &["assignment_expression"])
            .into_iter()
            .filter(|assignment| {
                assignment
                    .child_by_field_name("left")
                    .is_some_and(|left| left.text().ends_with("XmlResolver"))
                    && assignment
                        .child_by_field_name("right")
                        .is_some_and(|right| {
                            right.kind() == "object_creation_expression"
                                && right.text().contains("XmlUrlResolver")
                        })
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// MD5, SHA-1, DES, 3DES, and RC2 providers (CA5350, CA5351) and
/// `CipherMode.ECB`, with the algorithm's display name.
pub(crate) fn csharp_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    const CRYPTO: &str = "System.Security.Cryptography.";
    let scope = CSharpScope::new(root);
    let mut sites = scope
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            let rest = api.strip_prefix(CRYPTO)?;
            let (class, member) = rest.split_once('.')?;
            if !matches!(member, "Create" | ".ctor") {
                return None;
            }
            let algorithm = match class {
                "MD5" | "MD5CryptoServiceProvider" | "HMACMD5" => "MD5",
                "SHA1" | "SHA1CryptoServiceProvider" | "SHA1Managed" | "HMACSHA1" => "SHA-1",
                "DES"
                | "DESCryptoServiceProvider"
                | "TripleDES"
                | "TripleDESCryptoServiceProvider" => "DES",
                "RC2" | "RC2CryptoServiceProvider" => "RC2",
                _ => return None,
            };
            Some((call, algorithm))
        })
        .collect::<Vec<_>>();
    sites.extend(
        scope
            .members(root)
            .into_iter()
            .filter(|(member, _)| member == "System.Security.Cryptography.CipherMode.ECB")
            .map(|(_, access)| (access, "ECB mode")),
    );
    sites.sort_by_key(|(node, _)| node.start_byte());
    sites
}

/// `var stream = new FileStream(..)` or `File.Open*(..)` neither in a
/// `using` declaration or statement, disposed, nor handed on in the same
/// method; other uses transfer ownership (CA2000).
pub(crate) fn csharp_unclosed_streams(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const OPENERS: &[&str] = &[
        "System.IO.FileStream..ctor",
        "System.IO.StreamReader..ctor",
        "System.IO.StreamWriter..ctor",
        "System.IO.File.Create",
        "System.IO.File.Open",
        "System.IO.File.OpenRead",
        "System.IO.File.OpenWrite",
        "System.IO.File.OpenText",
        "System.IO.File.CreateText",
    ];
    let scope = CSharpScope::new(root);
    ast_scan::nodes_of_kind(root, &["local_declaration_statement"])
        .into_iter()
        .filter(|statement| !statement.children().any(|child| child.kind() == "using"))
        .filter(|statement| {
            let Some(declarator) = ast_scan::nodes_of_kind(*statement, &["variable_declarator"])
                .into_iter()
                .next()
            else {
                return false;
            };
            let Some(name) = declarator.child_by_field_name("name") else {
                return false;
            };
            let opens = declarator.named_children().any(|value| {
                scope
                    .resolve_call(value)
                    .is_some_and(|api| OPENERS.contains(&api.as_str()))
            });
            let Some(method) = enclosing_csharp_method(*statement) else {
                return false;
            };
            opens
                && !ast_scan::nodes_of_kind(method, &["identifier"])
                    .into_iter()
                    .filter(|usage| {
                        usage.start_byte() > statement.end_byte() && usage.text() == name.text()
                    })
                    .any(|usage| {
                        csharp_method_called_on(usage).is_none_or(|method| {
                            matches!(method, "Dispose" | "Close" | "DisposeAsync")
                        })
                    })
        })
        .collect()
}

/// The method name when `usage` is the receiver of a call (`stream.Read()`).
fn csharp_method_called_on(usage: AstNode<'_>) -> Option<&str> {
    let access = usage
        .parent()
        .filter(|parent| parent.kind() == "member_access_expression")?;
    if access.child_by_field_name("expression")?.start_byte() != usage.start_byte() {
        return None;
    }
    access
        .parent()
        .filter(|parent| parent.kind() == "invocation_expression")?;
    access.child_by_field_name("name").map(|name| name.text())
}

/// `Debug.Assert` on a parameter of a public method: the call is
/// `[Conditional("DEBUG")]`, so release builds drop it and the argument
/// contract goes unchecked (throw `ArgumentException` instead).
pub(crate) fn csharp_public_argument_asserts(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_calls_to(root, &["System.Diagnostics.Debug.Assert"])
        .into_iter()
        .filter(|call| {
            let Some(method) = enclosing_csharp_method(*call) else {
                return false;
            };
            let parameters = method
                .child_by_field_name("parameters")
                .map(|parameters| {
                    parameters
                        .named_children()
                        .filter_map(|parameter| parameter.child_by_field_name("name"))
                        .map(|name| name.text())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            // An assertion helper marked `[Conditional("DEBUG")]` is dropped
            // from release builds itself.
            let conditional = method
                .named_children()
                .filter(|child| child.kind() == "attribute_list")
                .any(|attributes| attributes.text().contains("Conditional"));
            csharp_is_public(method)
                && !conditional
                && csharp_arguments(*call)
                    .first()
                    .is_some_and(|(_, condition)| {
                        ast_scan::mentions_any(*condition, "identifier", &parameters)
                    })
        })
        .collect()
}

/// `Debugger.Break()` and `Debugger.Launch()`: debugger hooks left in code.
pub(crate) fn csharp_debugger_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    csharp_calls_to(
        root,
        &[
            "System.Diagnostics.Debugger.Break",
            "System.Diagnostics.Debugger.Launch",
        ],
    )
}
