//! JavaScript and TypeScript API tables for L1 rules: findings-free site
//! finders over resolved API identity (`js_api_identity.rs`). Each owner
//! constructs its own findings from the sites returned here. Semantics follow
//! ESLint where a rule exists (`no-eval`, `no-new-func`, `no-implied-eval`,
//! `n/no-process-exit`) and Node's documented API behavior otherwise.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::bound_reads::reads_of_target;
use crate::java_api_rules::is_shell_invocation;
use crate::js_api_identity::{js_arguments, js_string, object_properties, JsScope};
use crate::AuditedFile;

/// Resolved calls in a JS/TS file whose identity is one of `apis`.
fn js_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(JsScope::new(root).calls(root), apis)
}

/// The `fs` API `name` under each module that exports it (`fs`,
/// `fs/promises`, and `fs.promises`).
fn fs_api(api: &str) -> Option<&str> {
    ["fs.promises.", "fs/promises.", "fs."]
        .iter()
        .find_map(|prefix| api.strip_prefix(prefix))
}

/// `child_process.exec`/`execSync` always run a shell; `spawn`/`execFile`
/// do when given `shell: true` or a shell program with `-c`.
pub(crate) fn js_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let Some(function) = api.strip_prefix("child_process.") else {
                return false;
            };
            let arguments = js_arguments(*call);
            match function {
                "exec" | "execSync" => true,
                "spawn" | "spawnSync" | "execFile" | "execFileSync" => {
                    let shell_option = arguments.iter().any(|argument| {
                        object_properties(*argument)
                            .iter()
                            .any(|(key, value)| key == "shell" && value.text() != "false")
                    });
                    let program = arguments.first().and_then(|program| js_string(*program));
                    let flag = arguments
                        .get(1)
                        .filter(|list| list.kind() == "array")
                        .and_then(|list| list.named_child(0))
                        .and_then(js_string);
                    shell_option
                        || matches!((program, flag), (Some(program), Some(flag)) if is_shell_invocation(&program, &flag))
                }
                _ => false,
            }
        })
        .map(|(_, call)| call)
        .collect()
}

/// `process.exit` and `process.abort` in library code, following ESLint
/// `n/no-process-exit`; programs (`is_js_program`) own their process.
pub(crate) fn js_exits(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    js_calls_to(root, &["process.exit", "process.abort"])
}

/// A JS/TS file that is a program rather than library code: a `#!` script, a
/// file in an unpublished (`"private": true`) package or in a package with a
/// `bin` and no root entry point (a tool), or the source of one of its
/// package's `bin` entries (`src/cli.ts` for `dist/cli.js`). Exiting
/// and printing are a program's interface, as for Go `package main`.
pub(crate) fn is_js_program(file: &AuditedFile, root: AstNode<'_>) -> bool {
    if root.text().starts_with("#!") {
        return true;
    }
    let Some(manifest) = nearest_package_manifest(file) else {
        return false;
    };
    if manifest
        .get("private")
        .and_then(|private| private.as_bool())
        == Some(true)
    {
        return true;
    }
    let stem = file
        .relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let bin_stems = match manifest.get("bin") {
        Some(serde_json::Value::String(path)) => vec![path.as_str()],
        Some(serde_json::Value::Object(entries)) => {
            entries.values().filter_map(|path| path.as_str()).collect()
        }
        _ => Vec::new(),
    };
    // A package with a `bin` and no root entry point is a tool, not a library.
    let root_entry = manifest.get("main").is_some()
        || manifest.get("module").is_some()
        || match manifest.get("exports") {
            Some(serde_json::Value::Object(entries)) => {
                entries.contains_key(".") || !entries.keys().any(|key| key.starts_with('.'))
            }
            Some(_) => true,
            None => false,
        };
    if !bin_stems.is_empty() && !root_entry {
        return true;
    }
    bin_stems.into_iter().any(|path| {
        std::path::Path::new(path)
            .file_stem()
            .and_then(|bin| bin.to_str())
            .is_some_and(|bin| bin == stem)
    })
}

/// The `package.json` nearest to `file` within the scanned repository.
fn nearest_package_manifest(file: &AuditedFile) -> Option<serde_json::Value> {
    let depth = file.relative.components().count();
    file.absolute
        .ancestors()
        .skip(1)
        .take(depth)
        .map(|directory| directory.join("package.json"))
        .find(|manifest| manifest.is_file())
        .and_then(|manifest| std::fs::read_to_string(manifest).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// A TODO comment, or a thrown `Error` whose message says the code is
/// unfinished (`throw new Error('not implemented')`).
pub(crate) fn js_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let stubs = ast_scan::nodes_of_kind(root, &["throw_statement"])
        .into_iter()
        .filter(|statement| {
            statement
                .named_child(0)
                .filter(|thrown| thrown.kind() == "new_expression")
                .and_then(|thrown| js_arguments(thrown).first().copied())
                .and_then(js_string)
                .is_some_and(|message| ast_scan::is_unimplemented_message(&message))
        });
    ast_scan::todo_nodes(root, &["comment"], stubs)
}

/// Source text executed at runtime: `eval`, the `Function` constructor, the
/// `vm` module, and `setTimeout`/`setInterval` given a string (ESLint
/// `no-eval`, `no-new-func`, `no-implied-eval`).
pub(crate) fn js_dynamic_evaluation(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| match api.as_str() {
            "eval" | "Function" | "Function.<init>" | "vm.Script.<init>" => true,
            "setTimeout" | "setInterval" => js_arguments(*call)
                .first()
                .is_some_and(|callback| js_string(*callback).is_some()),
            _ => api.strip_prefix("vm.").is_some_and(|function| {
                function.starts_with("run") || function == "compileFunction"
            }),
        })
        .map(|(_, call)| call)
        .collect()
}

/// `node-serialize` and `serialize-to-js` revive functions from their input.
pub(crate) fn js_unsafe_deserialization(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    js_calls_to(
        root,
        &["node-serialize.unserialize", "serialize-to-js.deserialize"],
    )
}

/// A server listening on a literal wildcard host (`listen(port, '0.0.0.0')`
/// or `listen({ host: '::' })`).
pub(crate) fn js_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let wildcard = |value: AstNode<'_>| {
        js_string(value).is_some_and(|host| matches!(host.as_str(), "0.0.0.0" | "::" | "[::]"))
    };
    ast_scan::nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter(|call| {
            let is_listen = call
                .child_by_field_name("function")
                .and_then(|callee| callee.child_by_field_name("property"))
                .is_some_and(|property| property.text() == "listen");
            is_listen
                && js_arguments(*call).into_iter().any(|argument| {
                    wildcard(argument)
                        || object_properties(argument)
                            .into_iter()
                            .any(|(key, value)| key == "host" && wildcard(value))
                })
        })
        .collect()
}

/// An `fs` call that sets a mode with the world-write bit (`0o002`): a
/// numeric or octal-string mode argument, or a `{ mode }` option.
pub(crate) fn js_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let index = match fs_api(api) {
                Some("chmod" | "chmodSync" | "lchmod" | "lchmodSync" | "fchmod" | "fchmodSync") => {
                    1
                }
                Some("mkdir" | "mkdirSync") => 1,
                Some("writeFile" | "writeFileSync" | "appendFile" | "appendFileSync") => 2,
                Some("open" | "openSync") => 2,
                _ => return false,
            };
            js_arguments(*call).get(index).is_some_and(|mode| {
                let mode = object_properties(*mode)
                    .into_iter()
                    .find(|(key, _)| key == "mode")
                    .map_or(*mode, |(_, value)| value);
                js_mode(mode).is_some_and(|bits| bits & 0o002 != 0)
            })
        })
        .map(|(_, call)| call)
        .collect()
}

/// A file mode literal: `0o777`, legacy `0777`, `511`, or the string `'777'`.
fn js_mode(node: AstNode<'_>) -> Option<u32> {
    let text = match node.kind() {
        "number" => node.text().replace('_', "").to_ascii_lowercase(),
        _ => return u32::from_str_radix(&js_string(node)?, 8).ok(),
    };
    if let Some(digits) = text.strip_prefix("0o") {
        u32::from_str_radix(digits, 8).ok()
    } else if let Some(digits) = text.strip_prefix("0x") {
        u32::from_str_radix(digits, 16).ok()
    } else if text.len() > 1 && text.starts_with('0') {
        u32::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse().ok()
    }
}

/// `Math.random()` is not a cryptographic generator; secrets need
/// `crypto.randomBytes`/`randomUUID`/`getRandomValues`.
pub(crate) fn js_predictable_random(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    js_calls_to(root, &["Math.random"])
}

/// Certificate verification switched off: `rejectUnauthorized: false` or
/// `strictSSL: false` in an options object, or
/// `process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0'`.
pub(crate) fn js_disabled_certificate_checks(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = option_sites(root, |key, value| {
        matches!(key, "rejectUnauthorized" | "strictSSL") && value.text() == "false"
    });
    sites.extend(
        ast_scan::nodes_of_kind(root, &["assignment_expression"])
            .into_iter()
            .filter(|assignment| {
                let target = assignment
                    .child_by_field_name("left")
                    .map(|left| left.text());
                let value = assignment.child_by_field_name("right");
                target.is_some_and(|target| target.ends_with("NODE_TLS_REJECT_UNAUTHORIZED"))
                    && value.is_some_and(|value| {
                        value.text() == "0" || js_string(value).as_deref() == Some("0")
                    })
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// `secureProtocol` naming SSL or TLS 1.0/1.1, or `minVersion`/`maxVersion`
/// (and `tls.DEFAULT_MIN_VERSION`) set to TLS 1.0 or 1.1.
pub(crate) fn js_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let weak_version = |value: AstNode<'_>| {
        js_string(value).is_some_and(|version| matches!(version.as_str(), "TLSv1" | "TLSv1.1"))
    };
    let mut sites = option_sites(root, |key, value| match key {
        "secureProtocol" => js_string(value).is_some_and(|protocol| {
            [
                "SSLv2_",
                "SSLv3_",
                "TLSv1_method",
                "TLSv1_client",
                "TLSv1_server",
                "TLSv1_1_",
            ]
            .iter()
            .any(|weak| protocol.starts_with(weak))
        }),
        "minVersion" | "maxVersion" => weak_version(value),
        _ => false,
    });
    sites.extend(
        ast_scan::nodes_of_kind(root, &["assignment_expression"])
            .into_iter()
            .filter(|assignment| {
                assignment
                    .child_by_field_name("left")
                    .is_some_and(|left| left.text().ends_with("DEFAULT_MIN_VERSION"))
                    && assignment
                        .child_by_field_name("right")
                        .is_some_and(weak_version)
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

/// An `ssh2` connection whose `hostVerifier` accepts every key.
pub(crate) fn js_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    option_sites(root, |key, value| {
        key == "hostVerifier"
            && matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "function"
            ) && value.child_by_field_name("body").is_some_and(|body| {
            body.text() == "true" || {
                let mut statements = body.named_children();
                matches!(
                    (statements.next(), statements.next()),
                    (Some(statement), None)
                        if statement.kind() == "return_statement"
                            && statement.named_child(0).is_some_and(|value| value.text() == "true")
                )
            }
        })
    })
}

/// `libxmljs` parsing with `noent: true`, which expands external entities.
pub(crate) fn js_xxe_parsers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let parses = ["libxmljs.", "libxmljs2."].iter().any(|module| {
                api.strip_prefix(module)
                    .is_some_and(|function| function.starts_with("parseXml"))
            });
            parses
                && js_arguments(*call).into_iter().any(|argument| {
                    object_properties(argument)
                        .into_iter()
                        .any(|(key, value)| key == "noent" && value.text() == "true")
                })
        })
        .map(|(_, call)| call)
        .collect()
}

/// `crypto` hashes and ciphers with a broken algorithm or ECB mode, and the
/// deprecated `createCipher`, which derives its key with MD5. An HMAC does
/// not rely on its hash's collision resistance.
pub(crate) fn js_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            let function = api.strip_prefix("crypto.")?;
            if matches!(function, "createCipher" | "createDecipher") {
                return Some((call, "MD5 key derivation"));
            }
            if !matches!(
                function,
                "createHash" | "createCipheriv" | "createDecipheriv"
            ) {
                return None;
            }
            let algorithm = js_arguments(call)
                .first()
                .and_then(|algorithm| js_string(*algorithm))?;
            weak_js_algorithm(&algorithm.to_ascii_lowercase()).map(|weak| (call, weak))
        })
        .collect()
}

fn weak_js_algorithm(algorithm: &str) -> Option<&'static str> {
    match algorithm {
        "md4" | "md5" => Some("MD5"),
        "sha1" | "sha-1" => Some("SHA-1"),
        "rc4" | "rc2" => Some("RC4"),
        _ if algorithm.starts_with("des") => Some("DES"),
        _ if algorithm.starts_with("bf") || algorithm.starts_with("blowfish") => Some("Blowfish"),
        _ if algorithm.ends_with("-ecb") => Some("ECB mode"),
        _ => None,
    }
}

/// A file descriptor from `fs.openSync`, or a `FileHandle` from
/// `fs.promises.open`, held in a local that is neither closed nor handed on
/// in the same function. As for Go and Java, any use other than passing the
/// descriptor to an `fs` data call (or calling a non-`close` method on the
/// handle) transfers ownership. TypeScript `using` declarations release it.
pub(crate) fn js_unclosed_files(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = JsScope::new(root);
    let mut unclosed = Vec::new();
    for declarator in ast_scan::nodes_of_kind(root, &["variable_declarator"]) {
        let (Some(name), Some(value)) = (
            declarator.child_by_field_name("name"),
            declarator.child_by_field_name("value"),
        ) else {
            continue;
        };
        let call = if value.kind() == "await_expression" {
            value.named_child(0)
        } else {
            Some(value)
        };
        let opener = call
            .and_then(|call| scope.resolve_call(call))
            .and_then(|api| fs_api(&api).map(str::to_string));
        let handle = match opener.as_deref() {
            Some("openSync") => false,
            Some("open") if value.kind() == "await_expression" => true,
            _ => continue,
        };
        let Some(declaration) = declarator.parent() else {
            continue;
        };
        if name.kind() != "identifier"
            || declaration.text().trim_start().starts_with("await using")
            || declaration.text().trim_start().starts_with("using")
        {
            continue;
        }
        if enclosing_function_body(declarator).is_none() {
            continue;
        }
        let handled = reads_of_target(name).into_iter().any(|usage| {
            if handle {
                method_called_on(usage).is_none_or(|method| method == "close")
            } else {
                fs_descriptor_use(&scope, usage)
                    .is_none_or(|function| function.starts_with("close"))
            }
        });
        if !handled {
            unclosed.push(declaration);
        }
    }
    unclosed
}

/// The `fs` function `usage` is the first argument of (`fs.readSync(fd, ..)`).
fn fs_descriptor_use(scope: &JsScope, usage: AstNode<'_>) -> Option<String> {
    let arguments = usage
        .parent()
        .filter(|parent| parent.kind() == "arguments")?;
    let call = arguments.parent()?;
    let first = arguments.named_child(0)?;
    if first.start_byte() != usage.start_byte() {
        return None;
    }
    let api = scope.resolve_call(call)?;
    fs_api(&api).map(str::to_string)
}

/// The method name when `usage` is the receiver of a method call.
fn method_called_on(usage: AstNode<'_>) -> Option<&str> {
    let member = usage
        .parent()
        .filter(|parent| parent.kind() == "member_expression")?;
    let call = member
        .parent()
        .filter(|parent| parent.kind() == "call_expression")?;
    let is_callee = call
        .child_by_field_name("function")
        .is_some_and(|function| function.start_byte() == member.start_byte());
    let is_object = member
        .child_by_field_name("object")
        .is_some_and(|object| object.start_byte() == usage.start_byte());
    (is_callee && is_object)
        .then(|| {
            member
                .child_by_field_name("property")
                .map(|property| property.text())
        })
        .flatten()
}

fn enclosing_function_body(mut node: AstNode<'_>) -> Option<AstNode<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration"
                | "function_expression"
                | "function"
                | "arrow_function"
                | "method_definition"
                | "generator_function_declaration"
        ) {
            return parent.child_by_field_name("body");
        }
        node = parent;
    }
    None
}

/// Option-object properties (`{ key: value }`) satisfying `matches`.
fn option_sites<'a>(
    root: AstNode<'a>,
    matches: impl Fn(&str, AstNode<'a>) -> bool,
) -> Vec<AstNode<'a>> {
    ast_scan::nodes_of_kind(root, &["pair"])
        .into_iter()
        .filter(|pair| {
            let (Some(key), Some(value)) = (
                pair.child_by_field_name("key"),
                pair.child_by_field_name("value"),
            ) else {
                return false;
            };
            let key = js_string(key).unwrap_or_else(|| key.text().to_string());
            matches(&key, value)
        })
        .collect()
}
