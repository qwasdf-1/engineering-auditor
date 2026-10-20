//! Rust sites for the L1 rules resolved through API identity. Like the Go,
//! Java, JS/TS, and Python tables, these finders return nodes outside
//! inline test code; the owners construct the findings.

use std::path::Path;

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::java_api_rules::is_shell_invocation;
use crate::rust_api_identity::{method_name, rust_arguments, rust_string, RustScope};
use crate::AuditedFile;

/// Process builders whose first argument is the program.
const COMMAND_CONSTRUCTORS: &[&str] = &[
    "std::process::Command::new",
    "tokio::process::Command::new",
    "async_process::Command::new",
    "async_std::process::Command::new",
    "smol::process::Command::new",
];

fn production<'a>(nodes: impl IntoIterator<Item = AstNode<'a>>) -> Vec<AstNode<'a>> {
    let mut nodes = nodes
        .into_iter()
        .filter(|node| !ast_scan::is_inside_rust_test(*node))
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.start_byte());
    nodes
}

fn same_node(left: AstNode<'_>, right: AstNode<'_>) -> bool {
    left.start_byte() == right.start_byte()
        && left.end_byte() == right.end_byte()
        && left.kind() == right.kind()
}

/// The callee's last name: a path's final segment or a method name.
fn call_name(call: AstNode<'_>) -> Option<&str> {
    let function = call.child_by_field_name("function")?;
    match function.kind() {
        "identifier" => Some(function.text()),
        "scoped_identifier" => function.child_by_field_name("name").map(|name| name.text()),
        "field_expression" => method_name(call),
        _ => None,
    }
}

/// `std::process::exit` and `std::process::abort` calls.
pub(crate) fn rust_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    production(
        RustScope::new(root)
            .calls(root)
            .into_iter()
            .filter(|(api, _)| matches!(api.as_str(), "std::process::exit" | "std::process::abort"))
            .map(|(_, call)| call),
    )
}

/// A process builder for a shell given its command-string flag, by chained
/// `.arg`/`.args` calls or later calls on the variable that holds it.
pub(crate) fn rust_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    production(
        RustScope::new(root)
            .calls(root)
            .into_iter()
            .filter(|(api, _)| COMMAND_CONSTRUCTORS.contains(&api.as_str()))
            .filter_map(|(_, call)| {
                let program = rust_arguments(call)
                    .first()
                    .and_then(|first| rust_string(*first))?;
                let mut flags = Vec::new();
                let top = chained_arguments(call, &mut flags);
                variable_arguments(top, &mut flags);
                flags
                    .iter()
                    .any(|flag| is_shell_invocation(&program, flag))
                    .then_some(call)
            }),
    )
}

/// String arguments of the `.arg`/`.args` chain on `call`; returns the top of
/// the chain.
fn chained_arguments<'a>(call: AstNode<'a>, flags: &mut Vec<String>) -> AstNode<'a> {
    let mut node = call;
    while let Some(parent) = node.parent().filter(|parent| {
        parent.kind() == "field_expression"
            && parent
                .child_by_field_name("value")
                .is_some_and(|value| same_node(value, node))
    }) {
        let Some(chained) = parent
            .parent()
            .filter(|grand| grand.kind() == "call_expression")
        else {
            break;
        };
        collect_arguments(chained, flags);
        node = chained;
    }
    node
}

fn collect_arguments(call: AstNode<'_>, flags: &mut Vec<String>) {
    if matches!(method_name(call), Some("arg" | "args")) {
        if let Some(arguments) = call.child_by_field_name("arguments") {
            flags.extend(
                ast_scan::nodes_of_kind(arguments, &["string_literal", "raw_string_literal"])
                    .into_iter()
                    .filter_map(rust_string),
            );
        }
    }
}

/// `.arg`/`.args` calls on the variable a `let` binds the builder to.
fn variable_arguments(top: AstNode<'_>, flags: &mut Vec<String>) {
    let Some(declaration) = top
        .parent()
        .filter(|parent| parent.kind() == "let_declaration")
    else {
        return;
    };
    let Some(name) = declaration
        .child_by_field_name("pattern")
        .map(|pattern| pattern.text().trim_start_matches("mut ").trim())
    else {
        return;
    };
    let Some(function) = std::iter::successors(declaration.parent(), |parent| parent.parent())
        .find(|ancestor| ancestor.kind() == "function_item")
    else {
        return;
    };
    for call in ast_scan::nodes_of_kind(function, &["call_expression"]) {
        let receiver = call
            .child_by_field_name("function")
            .and_then(|function| function.child_by_field_name("value"));
        if receiver.is_some_and(|receiver| receiver.text() == name) {
            collect_arguments(call, flags);
        }
    }
}

/// `println!`, `print!`, `eprintln!`, and `eprint!` outside a program.
pub(crate) fn rust_console_output<'a>(file: &AuditedFile, root: AstNode<'a>) -> Vec<AstNode<'a>> {
    if is_rust_program(file) {
        return Vec::new();
    }
    macros_named(root, &["println", "print", "eprintln", "eprint"])
}

/// `dbg!`, Rust's debugging leftover (clippy `dbg_macro`).
pub(crate) fn rust_debug_macros(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    macros_named(root, &["dbg"])
}

fn macros_named<'a>(root: AstNode<'a>, names: &[&str]) -> Vec<AstNode<'a>> {
    production(
        ast_scan::nodes_of_kind(root, &["macro_invocation"])
            .into_iter()
            .filter(|invocation| {
                invocation
                    .child_by_field_name("macro")
                    .map(|name| name.text().rsplit("::").next().unwrap_or_default())
                    .is_some_and(|name| names.contains(&name))
            }),
    )
}

/// A file of a program rather than a library: `main.rs`, `build.rs`, a
/// `src/bin` target, or any file of a package without a library target.
pub(crate) fn is_rust_program(file: &AuditedFile) -> bool {
    let relative = crate::util::path_slash(&file.relative);
    let name = file.relative.file_name().and_then(|name| name.to_str());
    if matches!(name, Some("main.rs" | "build.rs"))
        || relative.starts_with("src/bin/")
        || relative.contains("/src/bin/")
    {
        return true;
    }
    let depth = file.relative.components().count();
    let Some(package) = file
        .absolute
        .ancestors()
        .skip(1)
        .take(depth)
        .find(|directory| directory.join("Cargo.toml").is_file())
    else {
        return false;
    };
    let manifest = std::fs::read_to_string(package.join("Cargo.toml")).unwrap_or_default();
    manifest.lines().any(|line| line.trim() == "[package]")
        && !manifest.lines().any(|line| line.trim() == "[lib]")
        && !Path::new(package).join("src/lib.rs").is_file()
}

/// A listener or socket address on the wildcard host: a literal `0.0.0.0`
/// or `[::]` passed to `bind`, or a `SocketAddr` built from an unspecified
/// address.
pub(crate) fn rust_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = RustScope::new(root);
    production(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                let first = rust_arguments(*call).first().copied();
                match call_name(*call) {
                    Some("bind") => first.and_then(rust_string).is_some_and(|address| {
                        address.starts_with("0.0.0.0") || address.starts_with("[::]")
                    }),
                    Some("from" | "new") => {
                        let api = call
                            .child_by_field_name("function")
                            .and_then(|function| scope.resolve(function))
                            .unwrap_or_default();
                        let owner = api.rsplit("::").nth(1).unwrap_or_default();
                        matches!(owner, "SocketAddr" | "SocketAddrV4" | "SocketAddrV6")
                            && first.is_some_and(is_unspecified_address)
                    }
                    _ => false,
                }
            }),
    )
}

fn is_unspecified_address(address: AstNode<'_>) -> bool {
    let compact = address
        .text()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    compact.contains("UNSPECIFIED")
        || compact.contains("[0,0,0,0]")
        || compact.contains("Ipv4Addr::new(0,0,0,0)")
}

/// A Unix mode with the world-write bit: `Permissions::from_mode`,
/// `set_mode`, or an `OpenOptionsExt`/`DirBuilderExt` `mode` given an octal
/// literal.
pub(crate) fn rust_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    production(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                matches!(call_name(*call), Some("from_mode" | "set_mode" | "mode"))
                    && rust_arguments(*call).first().is_some_and(|mode| {
                        mode.kind() == "integer_literal"
                            && mode
                                .text()
                                .strip_prefix("0o")
                                .and_then(|digits| {
                                    u32::from_str_radix(&digits.replace('_', ""), 8).ok()
                                })
                                .is_some_and(|mode| mode & 0o002 != 0)
                    })
            }),
    )
}

/// Non-cryptographic generators (`fastrand`, `oorandom`, `SmallRng`,
/// xorshift and PCG) and deterministic `seed_from_u64` seeding.
/// `rand::thread_rng` and `OsRng` are cryptographically secure.
pub(crate) fn rust_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = RustScope::new(root);
    production(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                if call_name(*call) == Some("seed_from_u64") {
                    return true;
                }
                let api = call
                    .child_by_field_name("function")
                    .and_then(|function| scope.resolve(function))
                    .unwrap_or_default();
                let owner = api.rsplit("::").nth(1).unwrap_or_default();
                api.starts_with("fastrand::")
                    || api.starts_with("oorandom::")
                    || matches!(
                        owner,
                        "SmallRng" | "XorShiftRng" | "Pcg32" | "Pcg64" | "Pcg64Mcg"
                    )
            }),
    )
}

/// `danger_accept_invalid_certs(true)` or `danger_accept_invalid_hostnames(true)`
/// (reqwest, native-tls), OpenSSL `set_verify(SslVerifyMode::NONE)`, and a
/// rustls `verify_server_cert` that accepts every certificate.
pub(crate) fn rust_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = ast_scan::nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter(|call| {
            let first = rust_arguments(*call)
                .first()
                .map(|argument| argument.text());
            match method_name(*call) {
                Some("danger_accept_invalid_certs" | "danger_accept_invalid_hostnames") => {
                    first == Some("true")
                }
                Some("set_verify") => {
                    first.is_some_and(|mode| mode.ends_with("SslVerifyMode::NONE"))
                }
                _ => false,
            }
        })
        .collect::<Vec<_>>();
    sites.extend(functions_returning(root, "verify_server_cert", |value| {
        value.starts_with("Ok(") && value.ends_with("::assertion())")
    }));
    production(sites)
}

/// A minimum or maximum TLS version naming SSL 3 or TLS 1.0/1.1
/// (native-tls `Protocol`, OpenSSL `SslVersion`).
pub(crate) fn rust_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    production(
        ast_scan::nodes_of_kind(root, &["call_expression"])
            .into_iter()
            .filter(|call| {
                matches!(
                    method_name(*call),
                    Some(
                        "min_protocol_version"
                            | "max_protocol_version"
                            | "set_min_proto_version"
                            | "set_max_proto_version"
                    )
                ) && call
                    .child_by_field_name("arguments")
                    .is_some_and(|arguments| {
                        ast_scan::nodes_of_kind(arguments, &["scoped_identifier"])
                            .into_iter()
                            .filter_map(|path| path.child_by_field_name("name"))
                            .any(|name| {
                                matches!(
                                    name.text(),
                                    "Sslv3" | "Tlsv10" | "Tlsv11" | "SSL3" | "TLS1" | "TLS1_1"
                                )
                            })
                    })
            }),
    )
}

/// A russh client handler whose `check_server_key` accepts every key.
pub(crate) fn rust_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    production(functions_returning(root, "check_server_key", |value| {
        value == "Ok(true)"
    }))
}

/// Functions named `name` whose body is a single expression accepted by
/// `accepts` (whitespace removed).
fn functions_returning<'a>(
    root: AstNode<'a>,
    name: &str,
    accepts: impl Fn(&str) -> bool,
) -> Vec<AstNode<'a>> {
    ast_scan::nodes_of_kind(root, &["function_item"])
        .into_iter()
        .filter(|function| {
            function
                .child_by_field_name("name")
                .is_some_and(|function_name| function_name.text() == name)
        })
        .filter(|function| {
            let statements = function
                .child_by_field_name("body")
                .map(|body| {
                    body.named_children()
                        .filter(|statement| !statement.kind().ends_with("comment"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let [value] = statements.as_slice() else {
                return false;
            };
            let compact = value
                .text()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            accepts(compact.trim_end_matches(';'))
        })
        .collect()
}

/// Broken hashes and ciphers: the `md5`, `md4`, `md2`, `sha1`, `sha1_smol`,
/// `des`, `rc4`, `blowfish`, and `ecb` crates, and OpenSSL `MessageDigest`
/// and `Cipher` constructors for them.
pub(crate) fn rust_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    let sites = RustScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(_, call)| !ast_scan::is_inside_rust_test(*call))
        .filter_map(|(api, call)| Some((call, weak_rust_api(&api)?)))
        .collect();
    sites
}

/// Crates that implement only a broken algorithm.
const WEAK_CRATES: &[(&str, &str)] = &[
    ("md2", "MD5"),
    ("md4", "MD5"),
    ("md5", "MD5"),
    ("sha1", "SHA-1"),
    ("sha1_smol", "SHA-1"),
    ("des", "DES"),
    ("rc4", "RC4"),
    ("blowfish", "Blowfish"),
    ("ecb", "ECB mode"),
];

fn weak_rust_api(api: &str) -> Option<&'static str> {
    let crate_name = api.split("::").next()?;
    if let Some((_, algorithm)) = WEAK_CRATES.iter().find(|(name, _)| *name == crate_name) {
        return Some(algorithm);
    }
    let (owner, function) = api.strip_prefix("openssl::")?.rsplit_once("::")?;
    match (owner.rsplit("::").next()?, function) {
        ("MessageDigest", "md5") => Some("MD5"),
        ("MessageDigest", "sha1") => Some("SHA-1"),
        ("Cipher", cipher) => weak_openssl_cipher(cipher),
        _ => None,
    }
}

fn weak_openssl_cipher(cipher: &str) -> Option<&'static str> {
    match cipher {
        _ if cipher.starts_with("des") => Some("DES"),
        _ if cipher.starts_with("rc4") => Some("RC4"),
        _ if cipher.starts_with("bf_") => Some("Blowfish"),
        _ if cipher.ends_with("_ecb") => Some("ECB mode"),
        _ => None,
    }
}
