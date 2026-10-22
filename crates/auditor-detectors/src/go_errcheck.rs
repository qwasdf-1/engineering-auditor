//! `go_errcheck`: errcheck's contract over Go syntax trees, without type
//! information. A call statement, `defer`, or `go` discards its result; a
//! result passed on, assigned (`_ =` included), returned, or tested is used.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::{AstNode, FunctionFlow, Origin};
use auditor_core::{Confidence, Finding, Severity};

use crate::api_identity::ImportScope;
use crate::ast_scan::{self, AstScan};
use crate::{util, Language};

/// Standard-library functions whose result includes an error: a bare call
/// statement discards it (errcheck reports each).
const GO_ERROR_FUNCTIONS: &[&str] = &[
    "os.Chdir",
    "os.Chmod",
    "os.Chown",
    "os.Chtimes",
    "os.Lchown",
    "os.Link",
    "os.Mkdir",
    "os.MkdirAll",
    "os.Remove",
    "os.RemoveAll",
    "os.Rename",
    "os.Setenv",
    "os.Symlink",
    "os.Truncate",
    "os.Unsetenv",
    "os.WriteFile",
    "io/ioutil.WriteFile",
    "io.Copy",
    "io.CopyBuffer",
    "io.CopyN",
    "io.ReadAtLeast",
    "io.ReadFull",
    "io.WriteString",
    "io/fs.WalkDir",
    "path/filepath.Walk",
    "path/filepath.WalkDir",
    "encoding/json.Unmarshal",
    "encoding/xml.Unmarshal",
    "net/http.ListenAndServe",
    "net/http.ListenAndServeTLS",
    "net/http.Serve",
];

/// `os/exec.Cmd` methods that report the command's failure only as an error.
const GO_COMMAND_METHODS: &[&str] = &["Run", "Start", "Wait"];

/// errcheck (golangci-lint): a call statement, deferred call, or goroutine
/// that discards an error result. Without type information a call counts
/// only when its error is known: a function in [`GO_ERROR_FUNCTIONS`],
/// `Run`/`Start`/`Wait` on an `exec.Command` value, or a `Close()`,
/// `Write(..)`, or `Remove(..)` method the file does not declare without
/// results. errcheck's default exclusions apply to writes into a
/// `bytes.Buffer`, `strings.Builder`, or hash. A result that is assigned
/// (`_ =` included), returned, or tested is used.
pub(crate) fn go_errcheck_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| *language == Language::Go,
        |file, tree, _| {
            let root = tree.root_node();
            let imports = ImportScope::go(root);
            let resultless = methods_declared_without_results(root);
            let rel = util::path_slash(&file.relative);
            let lines = tree.source().lines().collect::<Vec<_>>();
            for call in discarded_calls(root) {
                let Some(row) = ignored_error_row(&imports, &resultless, &lines, call) else {
                    continue;
                };
                let line = row + 1;
                findings.push(
                    auditor_core::finding!(
                        "go_errcheck",
                        "correctness",
                        "Ignored error return value",
                        format!("Ignored error return from call at line {line}"),
                        Severity::Medium,
                        Confidence::Low,
                        &rel,
                        Some(line),
                    )
                    .with_recommendation("Check the error return value explicitly.")
                    .with_evidence("pattern", lines.get(row).map_or("", |text| text.trim())),
                );
            }
        },
    );
    findings
}

/// Calls whose results nothing receives: expression statements, `defer`,
/// and `go`.
fn discarded_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(
        root,
        &["expression_statement", "defer_statement", "go_statement"],
    )
    .into_iter()
    .filter_map(|statement| statement.named_child(0))
    .filter(|call| call.kind() == "call_expression")
    .collect()
}

/// The row to report when `call` discards an error: the method name's row,
/// so a chained call reports where the ignored method is.
fn ignored_error_row(
    imports: &ImportScope,
    resultless: &BTreeSet<String>,
    lines: &[&str],
    call: AstNode<'_>,
) -> Option<usize> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "selector_expression" {
        return None;
    }
    if let Some(api) = imports.resolve(function) {
        return GO_ERROR_FUNCTIONS
            .contains(&api.as_str())
            .then(|| call.start_position().0);
    }
    let operand = function.child_by_field_name("operand")?;
    let field = function.child_by_field_name("field")?;
    let method = field.text();
    let arguments = call.child_by_field_name("arguments")?.named_child_count();
    let row = field.start_position().0;
    let reported = if GO_COMMAND_METHODS.contains(&method) {
        arguments == 0 && is_command(imports, operand)
    } else {
        let fallible = match method {
            "Close" => arguments == 0,
            "Write" | "Remove" => arguments > 0,
            _ => false,
        };
        fallible
            && !resultless.contains(method)
            && !(method == "Write" && infallible_writer(&lines[..row], operand.text()))
    };
    reported.then_some(row)
}

/// `operand` is an `exec.Command(..)` value: the call itself, or a local
/// whose every origin is one.
fn is_command(imports: &ImportScope, operand: AstNode<'_>) -> bool {
    let constructs = |node: AstNode<'_>| {
        node.kind() == "call_expression"
            && node
                .child_by_field_name("function")
                .and_then(|function| imports.resolve(function))
                .is_some_and(|api| {
                    matches!(api.as_str(), "os/exec.Command" | "os/exec.CommandContext")
                })
    };
    if constructs(operand) {
        return true;
    }
    if operand.kind() != "identifier" {
        return false;
    }
    FunctionFlow::enclosing(operand).only_from(
        operand,
        |origin| matches!(origin, Origin::Call(call) if constructs(*call)),
    )
}

/// errcheck's default exclusions: writes to a `bytes.Buffer`, a
/// `strings.Builder`, or a hash cannot fail. `receiver` is the call text
/// before the method; only a plain local whose declaration an earlier line
/// shows is resolved.
fn infallible_writer(earlier: &[&str], receiver: &str) -> bool {
    const INFALLIBLE: &[&str] = &[
        "bytes.Buffer",
        "strings.Builder",
        "bytes.NewBuffer",
        "hash.Hash",
        "md5.New",
        "sha1.New",
        "sha256.New",
        "sha512.New",
        "hmac.New",
        "fnv.New",
        "crc32.New",
        "crc64.New",
    ];
    let name_start = receiver
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |index| index + 1);
    let name = &receiver[name_start..];
    if name.is_empty() || receiver[..name_start].ends_with('.') {
        return false;
    }
    earlier.iter().rev().any(|line| {
        let code = util::strip_comments(line);
        let declares = code
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|word| word == name)
            && (code.contains(&format!("var {name} "))
                || code.contains(&format!("{name} :="))
                || code.contains(&format!("{name} *"))
                || code.contains(&format!("{name} hash.")));
        declares && INFALLIBLE.iter().any(|kind| code.contains(kind))
    })
}

/// Methods this file declares, and only declares, without results
/// (`Write(status int)` in an interface, as chi's `LogEntry`): a call to one
/// ignores no error.
fn methods_declared_without_results(root: AstNode<'_>) -> BTreeSet<String> {
    let mut results: BTreeMap<String, bool> = BTreeMap::new();
    for declaration in
        ast_scan::nodes_of_kind(root, &["method_declaration", "method_elem", "method_spec"])
    {
        let Some(name) = declaration.child_by_field_name("name") else {
            continue;
        };
        let without = declaration.child_by_field_name("result").is_none();
        results
            .entry(name.text().to_string())
            .and_modify(|all| *all &= without)
            .or_insert(without);
    }
    results
        .into_iter()
        .filter_map(|(name, without)| without.then_some(name))
        .collect()
}

#[cfg(test)]
#[path = "go_errcheck_tests.rs"]
mod tests;
