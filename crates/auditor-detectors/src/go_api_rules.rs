//! Go API tables for L1 rules: findings-free site finders over resolved API
//! identity (`api_identity.rs`). Each owner constructs its own findings from
//! the sites returned here.

use auditor_ast::AstNode;

use crate::api_identity::{arguments, go_integer, go_package, go_string, ImportScope};
use crate::ast_scan;
use crate::{AuditedFile, Language};

/// Calls in a Go file to one of `apis`, with the argument at each API's index.
fn go_api_arguments<'a>(
    root: AstNode<'a>,
    apis: &[(&str, usize)],
) -> Vec<(AstNode<'a>, AstNode<'a>)> {
    ImportScope::go(root)
        .calls(root)
        .into_iter()
        .filter_map(|(callee, call)| {
            let index = apis.iter().find(|(api, _)| *api == callee)?.1;
            Some((call, *arguments(call).get(index)?))
        })
        .collect()
}

/// A listener on a literal address with no host (`":8080"`) or a wildcard
/// host binds every interface (gosec G102).
pub(crate) fn go_bind_all_interfaces(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const LISTENERS: &[(&str, usize)] = &[
        ("net.Listen", 1),
        ("net.ListenPacket", 1),
        ("crypto/tls.Listen", 1),
        ("net/http.ListenAndServe", 0),
        ("net/http.ListenAndServeTLS", 0),
    ];
    go_api_arguments(root, LISTENERS)
        .into_iter()
        .filter(|(_, address)| {
            go_string(*address).is_some_and(|address| {
                let host = address
                    .rsplit_once(':')
                    .map_or(address.as_str(), |(host, _)| host);
                matches!(host, "" | "0.0.0.0" | "[::]" | "::")
            })
        })
        .map(|(call, _)| call)
        .collect()
}

/// A file or directory created or chmodded with the world-write bit
/// (`0o002`), including `os.ModePerm` (`0o777`).
pub(crate) fn go_world_writable_modes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const MODE_SETTERS: &[(&str, usize)] = &[
        ("os.Chmod", 1),
        ("os.Mkdir", 1),
        ("os.MkdirAll", 1),
        ("os.OpenFile", 2),
        ("os.WriteFile", 2),
        ("io/ioutil.WriteFile", 2),
    ];
    let scope = ImportScope::go(root);
    go_api_arguments(root, MODE_SETTERS)
        .into_iter()
        .filter(|(_, mode)| {
            let value = go_integer(*mode).or_else(|| {
                matches!(
                    scope.resolve(*mode).as_deref(),
                    Some("os.ModePerm" | "io/fs.ModePerm")
                )
                .then_some(0o777)
            });
            value.is_some_and(|mode| mode & 0o002 != 0)
        })
        .map(|(call, _)| call)
        .collect()
}

/// `math/rand` is a deterministic generator; key material and tokens need
/// `crypto/rand` (gosec G404).
pub(crate) fn go_math_rand_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ImportScope::go(root)
        .calls(root)
        .into_iter()
        .filter(|(callee, _)| {
            ["math/rand.", "math/rand/v2."].iter().any(|package| {
                callee
                    .strip_prefix(package)
                    .is_some_and(|name| name != "Seed")
            })
        })
        .map(|(_, call)| call)
        .collect()
}

/// `tls.Config{InsecureSkipVerify: true}`, or the field set to `true` later,
/// accepts any server certificate (gosec G402).
pub(crate) fn go_insecure_skip_verify(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut sites = ImportScope::go(root)
        .composite_fields(root, "crypto/tls.Config")
        .into_iter()
        .filter(|(field, value, _)| field == "InsecureSkipVerify" && value.text() == "true")
        .map(|(_, _, element)| element)
        .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["assignment_statement"])
            .into_iter()
            .filter(|assignment| {
                let target = assignment
                    .child_by_field_name("left")
                    .map(|left| left.text().trim());
                let value = assignment
                    .child_by_field_name("right")
                    .map(|right| right.text().trim());
                target.is_some_and(|target| target.ends_with(".InsecureSkipVerify"))
                    && value == Some("true")
            }),
    );
    sites
}

/// A `tls.Config` whose minimum or maximum version is SSL 3.0, TLS 1.0, or
/// TLS 1.1 (gosec G402).
pub(crate) fn go_deprecated_tls_versions(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let scope = ImportScope::go(root);
    scope
        .composite_fields(root, "crypto/tls.Config")
        .into_iter()
        .filter(|(field, value, _)| {
            matches!(field.as_str(), "MinVersion" | "MaxVersion")
                && matches!(
                    scope.resolve(*value).as_deref(),
                    Some(
                        "crypto/tls.VersionSSL30"
                            | "crypto/tls.VersionTLS10"
                            | "crypto/tls.VersionTLS11"
                    )
                )
        })
        .map(|(_, _, element)| element)
        .collect()
}

/// `ssh.InsecureIgnoreHostKey()` accepts any host key (gosec G106).
pub(crate) fn go_ssh_insecure_host_keys(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ImportScope::go(root)
        .calls(root)
        .into_iter()
        .filter(|(callee, _)| callee == "golang.org/x/crypto/ssh.InsecureIgnoreHostKey")
        .map(|(_, call)| call)
        .collect()
}

/// `f, err := os.Open(..)` whose file is neither closed nor handed on in the
/// same function. Any use of `f` other than calling one of its methods
/// (returning it, storing it, passing it to `io.Copy`) transfers ownership,
/// so only a file used solely through non-`Close` methods is reported.
pub(crate) fn go_unclosed_files(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const OPENERS: &[&str] = &["os.Open", "os.Create", "os.OpenFile"];
    let scope = ImportScope::go(root);
    let mut unclosed = Vec::new();
    for declaration in
        ast_scan::nodes_of_kind(root, &["short_var_declaration", "assignment_statement"])
    {
        let (Some(left), Some(right)) = (
            declaration.child_by_field_name("left"),
            declaration.child_by_field_name("right"),
        ) else {
            continue;
        };
        let opens = right.named_child(0).is_some_and(|call| {
            call.kind() == "call_expression"
                && call
                    .child_by_field_name("function")
                    .and_then(|callee| scope.resolve(callee))
                    .is_some_and(|callee| OPENERS.contains(&callee.as_str()))
        });
        let Some(target) = left
            .named_child(0)
            .filter(|target| target.kind() == "identifier")
        else {
            continue;
        };
        let Some(body) = enclosing_go_function_body(declaration) else {
            continue;
        };
        if !opens || target.text() == "_" {
            continue;
        }
        let handled = ast_scan::nodes_of_kind(body, &["identifier"])
            .into_iter()
            .filter(|usage| {
                usage.start_byte() > declaration.end_byte() && usage.text() == target.text()
            })
            .any(|usage| go_method_called_on(usage).is_none_or(|method| method == "Close"));
        if !handled {
            unclosed.push(declaration);
        }
    }
    unclosed
}

/// The method name when `usage` is the receiver of a method call (`f.Read(..)`).
fn go_method_called_on(usage: AstNode<'_>) -> Option<&str> {
    let selector = usage
        .parent()
        .filter(|parent| parent.kind() == "selector_expression")?;
    let operand = selector.child_by_field_name("operand")?;
    let call = selector
        .parent()
        .filter(|parent| parent.kind() == "call_expression")?;
    let is_callee = call
        .child_by_field_name("function")
        .is_some_and(|function| function.start_byte() == selector.start_byte());
    (operand.start_byte() == usage.start_byte() && is_callee)
        .then(|| {
            selector
                .child_by_field_name("field")
                .map(|field| field.text())
        })
        .flatten()
}

fn enclosing_go_function_body(mut node: AstNode<'_>) -> Option<AstNode<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        ) {
            return parent.child_by_field_name("body");
        }
        node = parent;
    }
    None
}

/// A Go `package main` file is a program entry point: exiting and printing
/// are its interface, as they are for a Rust `main.rs`.
pub(crate) fn is_go_main_package(file: &AuditedFile, root: AstNode<'_>) -> bool {
    file.language == Language::Go && go_package(root) == Some("main")
}

/// Calls in a Go file whose callee resolves to one of `apis`.
pub(crate) fn go_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(ImportScope::go(root).calls(root), apis)
}

/// Go writes to standard output through `fmt.Print*` and the `print` and
/// `println` builtins; a library that does so bypasses its caller's logging.
pub(crate) const GO_STDOUT_APIS: &[&str] =
    &["fmt.Print", "fmt.Println", "fmt.Printf", "print", "println"];

/// `exec.Command("sh", "-c", ..)`: a command line handed to a shell
/// interpreter, which parses it (Go's `exec` never invokes a shell itself).
pub(crate) fn go_shell_commands(root: AstNode<'_>) -> Vec<AstNode<'_>> {
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
    let scope = ImportScope::go(root);
    scope
        .calls(root)
        .into_iter()
        .filter_map(|(callee, call)| {
            let skip = match callee.as_str() {
                "os/exec.Command" => 0,
                "os/exec.CommandContext" => 1,
                _ => return None,
            };
            let values = arguments(call)
                .into_iter()
                .skip(skip)
                .take(2)
                .map(go_string)
                .collect::<Vec<_>>();
            let program = values.first()?.as_deref()?;
            let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
            let flag = values.get(1)?.as_deref()?;
            (SHELLS.contains(&program) && COMMAND_FLAGS.contains(&flag)).then_some(call)
        })
        .collect()
}

/// Go has no `todo!` macro: a TODO comment, or a `panic` whose message says
/// the code is unfinished (`panic("not implemented")`).
pub(crate) fn go_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let stubs = ImportScope::go(root)
        .calls(root)
        .into_iter()
        .filter(|(callee, call)| {
            callee == "panic"
                && arguments(*call)
                    .first()
                    .and_then(|message| go_string(*message))
                    .is_some_and(|message| ast_scan::is_unimplemented_message(&message))
        })
        .map(|(_, call)| call);
    ast_scan::todo_nodes(root, &["comment"], stubs)
}
