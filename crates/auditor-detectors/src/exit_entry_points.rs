//! Process exits inside code that owns the process: CLI command handlers,
//! management commands, signal handlers, forked children, `multiprocessing`
//! entry points, and functions that say they exit. Ending the process is the
//! contract of these functions, not a library bailing out on its caller.

use std::collections::BTreeSet;
use std::ops::Range;

use auditor_ast::AstNode;

use crate::api_identity::ImportScope;
use crate::ast_scan::nodes_of_kind;
use crate::js_api_identity::{js_arguments, js_string, JsScope};
use crate::python_api_identity::{positional_arguments, PyScope};
use crate::{AuditedFile, Language};

#[cfg(test)]
#[path = "exit_entry_points_tests.rs"]
mod tests;

/// The exits that are not inside an entry point of their file.
pub(crate) fn outside_entry_points<'a>(
    file: &AuditedFile,
    root: AstNode<'a>,
    exits: Vec<AstNode<'a>>,
) -> Vec<AstNode<'a>> {
    if exits.is_empty() {
        return exits;
    }
    let spans = entry_spans(file, root);
    let scope = (file.language == Language::Python).then(|| PyScope::new(root));
    exits
        .into_iter()
        .filter(|exit| {
            let start = exit.start_byte();
            !spans.iter().any(|span| span.contains(&start))
                && !scope
                    .as_ref()
                    .is_some_and(|scope| is_fork_child_exit(*exit, scope))
        })
        .collect()
}

/// Whether a function name says that it exits (`exit`, `Exit`,
/// `printVersionAndExit`, `print_and_exit`).
pub(crate) fn names_an_exit(name: &str) -> bool {
    matches!(name, "exit" | "Exit") || name.ends_with("AndExit") || name.ends_with("_and_exit")
}

fn span(node: AstNode<'_>) -> Range<usize> {
    node.start_byte()..node.end_byte()
}

fn enclosing<'a>(node: AstNode<'a>, kinds: &[&str]) -> Option<AstNode<'a>> {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| kinds.contains(&ancestor.kind()))
}

/// Byte spans of the functions whose exits are their own business.
fn entry_spans(file: &AuditedFile, root: AstNode<'_>) -> Vec<Range<usize>> {
    let mut spans = if file.language == Language::Go {
        Vec::new()
    } else {
        exit_named_functions(root)
    };
    match file.language {
        Language::Python => spans.extend(python_entry_spans(file, root)),
        Language::JavaScript | Language::TypeScript => spans.extend(js_signal_handlers(root)),
        Language::Go => spans.extend(go_entry_functions(root)),
        _ => {}
    }
    spans
}

/// Functions and methods named for exiting ([`names_an_exit`]).
fn exit_named_functions(root: AstNode<'_>) -> Vec<Range<usize>> {
    const KINDS: &[&str] = &[
        "function_definition",
        "function_declaration",
        "method_definition",
        "method_declaration",
        "function_item",
        "local_function_statement",
    ];
    nodes_of_kind(root, KINDS)
        .into_iter()
        .filter(|function| {
            let name = function
                .child_by_field_name("name")
                .map(|name| name.text().to_string())
                .or_else(|| function.declared_name());
            name.is_some_and(|name| names_an_exit(&name))
        })
        .map(span)
        .collect()
}

// ---------------------------------------------------------------- Python

fn python_entry_spans(file: &AuditedFile, root: AstNode<'_>) -> Vec<Range<usize>> {
    let scope = PyScope::new(root);
    let mut spans = nodes_of_kind(root, &["function_definition"])
        .into_iter()
        .filter(|function| is_cli_command(*function, &scope))
        .map(span)
        .collect::<Vec<_>>();
    spans.extend(management_command_classes(file, root));
    spans.extend(process_run_methods(root, &scope));
    spans.extend(py_signal_handlers(root, &scope));
    spans
}

/// A function decorated as a click/typer command or group: `@click.command()`,
/// `@click.group()`, `@<name>.command()`, `@<name>.group()`.
fn is_cli_command(function: AstNode<'_>, scope: &PyScope) -> bool {
    let Some(definition) = function
        .parent()
        .filter(|parent| parent.kind() == "decorated_definition")
    else {
        return false;
    };
    definition
        .children()
        .filter(|child| child.kind() == "decorator")
        .filter_map(|decorator| decorator.named_child(0))
        .map(|expression| {
            expression
                .child_by_field_name("function")
                .unwrap_or(expression)
        })
        .any(|target| match target.kind() {
            "attribute" => target
                .child_by_field_name("attribute")
                .is_some_and(|name| matches!(name.text(), "command" | "group")),
            _ => matches!(
                scope.resolve(target).as_deref(),
                Some("click.command" | "click.group")
            ),
        })
}

/// The names a class lists as bases: `Base` and `module.Base` as written.
fn base_classes(class: AstNode<'_>) -> Vec<AstNode<'_>> {
    class
        .child_by_field_name("superclasses")
        .map(|bases| {
            bases
                .named_children()
                .filter(|base| matches!(base.kind(), "identifier" | "attribute"))
                .collect()
        })
        .unwrap_or_default()
}

fn class_name(class: AstNode<'_>) -> Option<&str> {
    class.child_by_field_name("name").map(|name| name.text())
}

fn base_name(base: AstNode<'_>) -> &str {
    base.child_by_field_name("attribute").unwrap_or(base).text()
}

/// Classes deriving from `BaseCommand` (or a base named `...Command`) in a
/// `management/commands/` file: Django management commands, whose methods
/// run as the command process.
fn management_command_classes(file: &AuditedFile, root: AstNode<'_>) -> Vec<Range<usize>> {
    let path = file.relative.to_string_lossy().replace('\\', "/");
    if !path.contains("management/commands/") {
        return Vec::new();
    }
    nodes_of_kind(root, &["class_definition"])
        .into_iter()
        .filter(|class| {
            base_classes(*class)
                .into_iter()
                .any(|base| base_name(base).ends_with("Command"))
        })
        .map(span)
        .collect()
}

/// `run` methods of classes whose base resolves to `multiprocessing.Process`
/// within this file, directly or through another class of the file.
fn process_run_methods(root: AstNode<'_>, scope: &PyScope) -> Vec<Range<usize>> {
    let classes = nodes_of_kind(root, &["class_definition"]);
    let is_process_base = |base: AstNode<'_>, known: &BTreeSet<&str>| {
        matches!(
            scope.resolve(base).as_deref(),
            Some("multiprocessing.Process" | "multiprocessing.context.Process")
        ) || (base.kind() == "identifier" && known.contains(base.text()))
    };
    let mut known = BTreeSet::new();
    loop {
        let before = known.len();
        for class in &classes {
            if base_classes(*class)
                .into_iter()
                .any(|base| is_process_base(base, &known))
            {
                known.extend(class_name(*class));
            }
        }
        if known.len() == before {
            break;
        }
    }
    classes
        .into_iter()
        .filter(|class| class_name(*class).is_some_and(|name| known.contains(name)))
        .flat_map(|class| methods_named(class, "run"))
        .map(span)
        .collect()
}

/// The methods `name` declared in a class body, decorated or not.
fn methods_named<'a>(class: AstNode<'a>, name: &str) -> Vec<AstNode<'a>> {
    class
        .child_by_field_name("body")
        .map(|body| {
            body.named_children()
                .map(|member| member.child_by_field_name("definition").unwrap_or(member))
                .filter(|member| {
                    member.kind() == "function_definition"
                        && member
                            .child_by_field_name("name")
                            .is_some_and(|n| n.text() == name)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Handlers the file registers with `signal.signal(sig, handler)`: a lambda,
/// a function defined in the registering function, or a method of the
/// registering method's class (`self.stop`).
fn py_signal_handlers(root: AstNode<'_>, scope: &PyScope) -> Vec<Range<usize>> {
    scope
        .calls(root)
        .into_iter()
        .filter(|(api, _)| api == "signal.signal")
        .filter_map(|(_, call)| Some((call, *positional_arguments(call).get(1)?)))
        .flat_map(|(call, handler)| py_handler_functions(call, handler))
        .map(span)
        .collect()
}

fn py_handler_functions<'a>(call: AstNode<'a>, handler: AstNode<'a>) -> Vec<AstNode<'a>> {
    match handler.kind() {
        "lambda" => vec![handler],
        "identifier" => enclosing(call, &["function_definition"])
            .map(|owner| {
                nodes_of_kind(owner, &["function_definition"])
                    .into_iter()
                    .filter(|function| {
                        function
                            .child_by_field_name("name")
                            .is_some_and(|name| name.text() == handler.text())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "attribute"
            if handler
                .child_by_field_name("object")
                .is_some_and(|object| object.text() == "self") =>
        {
            let method = handler.child_by_field_name("attribute");
            enclosing(call, &["class_definition"])
                .zip(method)
                .map(|(class, method)| methods_named(class, method.text()))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// `os._exit` in the child branch of an `os.fork()` (`if os.fork() == 0:`, or
/// `if pid == 0:` after `pid = os.fork()`): a forked child must skip the
/// parent's cleanup.
fn is_fork_child_exit(exit: AstNode<'_>, scope: &PyScope) -> bool {
    let is_os_exit = exit
        .child_by_field_name("function")
        .and_then(|function| scope.resolve(function))
        .as_deref()
        == Some("os._exit");
    is_os_exit
        && std::iter::successors(exit.parent(), |parent| parent.parent())
            .filter(|ancestor| ancestor.kind() == "if_statement")
            .filter(|branch| {
                branch
                    .child_by_field_name("consequence")
                    .is_some_and(|child| span(child).contains(&exit.start_byte()))
            })
            .filter_map(|branch| branch.child_by_field_name("condition"))
            .any(|condition| is_fork_child_condition(condition, exit, scope))
}

/// `<fork> == 0` where `<fork>` is an `os.fork()` call or a variable
/// assigned from one.
fn is_fork_child_condition(condition: AstNode<'_>, site: AstNode<'_>, scope: &PyScope) -> bool {
    if condition.kind() != "comparison_operator" || !condition.children().any(|c| c.kind() == "==")
    {
        return false;
    }
    let operands = condition.named_children().collect::<Vec<_>>();
    let [first, second] = operands.as_slice() else {
        return false;
    };
    let (value, zero) = if second.text() == "0" {
        (*first, *second)
    } else {
        (*second, *first)
    };
    zero.text() == "0" && is_fork_value(value, site, scope)
}

fn is_fork_value(value: AstNode<'_>, site: AstNode<'_>, scope: &PyScope) -> bool {
    let is_fork_call = |node: AstNode<'_>| {
        node.kind() == "call"
            && node
                .child_by_field_name("function")
                .and_then(|function| scope.resolve(function))
                .as_deref()
                == Some("os.fork")
    };
    match value.kind() {
        "call" => is_fork_call(value),
        "identifier" => {
            let owner = enclosing(site, &["function_definition"]).unwrap_or_else(|| root_of(site));
            nodes_of_kind(owner, &["assignment"])
                .into_iter()
                .any(|assignment| {
                    assignment
                        .child_by_field_name("left")
                        .is_some_and(|left| left.text() == value.text())
                        && assignment
                            .child_by_field_name("right")
                            .is_some_and(is_fork_call)
                })
        }
        _ => false,
    }
}

fn root_of(node: AstNode<'_>) -> AstNode<'_> {
    std::iter::successors(Some(node), |node| node.parent())
        .last()
        .unwrap_or(node)
}

// ------------------------------------------------------------- JS and TS

const JS_FUNCTIONS: &[&str] = &[
    "function_declaration",
    "function_expression",
    "function",
    "arrow_function",
    "method_definition",
    "generator_function_declaration",
];

/// Handlers the file registers for a process signal with
/// `process.on|once('SIGINT', handler)`: an inline function, one declared in
/// the registering function, or a method of the registering method's class
/// (`this.stop`, or an arrow that only calls `this.stop()`).
fn js_signal_handlers(root: AstNode<'_>) -> Vec<Range<usize>> {
    let scope = JsScope::new(root);
    nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter(|call| is_signal_registration(*call, &scope))
        .filter_map(|call| Some((call, *js_arguments(call).get(1)?)))
        .flat_map(|(call, handler)| js_handler_functions(call, handler))
        .map(span)
        .collect()
}

/// `process.on('SIGTERM', ..)`, possibly in a chain on `process`.
fn is_signal_registration(call: AstNode<'_>, scope: &JsScope) -> bool {
    let Some(callee) = call
        .child_by_field_name("function")
        .filter(|callee| callee.kind() == "member_expression")
    else {
        return false;
    };
    let registers = callee
        .child_by_field_name("property")
        .is_some_and(|property| matches!(property.text(), "on" | "once" | "addListener"));
    let signal = js_arguments(call)
        .first()
        .and_then(|name| js_string(*name))
        .is_some_and(|name| {
            name.strip_prefix("SIG").is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            })
        });
    registers
        && signal
        && callee
            .child_by_field_name("object")
            .is_some_and(|object| is_process(object, scope))
}

/// `process`, or a call chain whose receiver is `process`.
fn is_process(mut receiver: AstNode<'_>, scope: &JsScope) -> bool {
    while receiver.kind() == "call_expression" {
        let Some(object) = receiver
            .child_by_field_name("function")
            .and_then(|callee| callee.child_by_field_name("object"))
        else {
            return false;
        };
        receiver = object;
    }
    receiver.kind() == "identifier" && scope.resolve(receiver).as_deref() == Some("process")
}

fn js_handler_functions<'a>(call: AstNode<'a>, handler: AstNode<'a>) -> Vec<AstNode<'a>> {
    match handler.kind() {
        "arrow_function" | "function_expression" | "function" => {
            let mut functions = vec![handler];
            if let Some(method) = delegated_method(handler) {
                functions.extend(class_methods_named(call, method));
            }
            functions
        }
        "member_expression" => this_member(handler)
            .map(|method| class_methods_named(call, method))
            .unwrap_or_default(),
        "identifier" => enclosing(call, JS_FUNCTIONS)
            .map(|owner| js_functions_named(owner, handler.text()))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// `m` for `this.m`.
fn this_member(member: AstNode<'_>) -> Option<&str> {
    let object = member.child_by_field_name("object")?;
    (object.kind() == "this").then(|| {
        member
            .child_by_field_name("property")
            .map(|name| name.text())
    })?
}

/// `m` for an arrow function that only calls `this.m(..)`.
fn delegated_method(function: AstNode<'_>) -> Option<&str> {
    let body = function.child_by_field_name("body")?;
    let expression = match body.kind() {
        "statement_block" if body.named_child_count() == 1 => body
            .named_child(0)
            .and_then(|statement| statement.named_child(0))?,
        _ => body,
    };
    let callee = expression.child_by_field_name("function")?;
    (expression.kind() == "call_expression")
        .then(|| this_member(callee))
        .flatten()
}

fn class_methods_named<'a>(call: AstNode<'a>, name: &str) -> Vec<AstNode<'a>> {
    enclosing(call, &["class_declaration", "class"])
        .map(|class| {
            nodes_of_kind(class, &["method_definition"])
                .into_iter()
                .filter(|method| {
                    method
                        .child_by_field_name("name")
                        .is_some_and(|n| n.text() == name)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Functions `name` declared inside `owner`, as a declaration or as a
/// `const name = () => ..` binding.
fn js_functions_named<'a>(owner: AstNode<'a>, name: &str) -> Vec<AstNode<'a>> {
    let named = |node: AstNode<'_>| {
        node.child_by_field_name("name")
            .is_some_and(|n| n.text() == name)
    };
    let mut functions = nodes_of_kind(owner, &["function_declaration"])
        .into_iter()
        .filter(|function| named(*function))
        .collect::<Vec<_>>();
    functions.extend(
        nodes_of_kind(owner, &["variable_declarator"])
            .into_iter()
            .filter(|binding| named(*binding))
            .filter_map(|binding| binding.child_by_field_name("value"))
            .filter(|value| matches!(value.kind(), "arrow_function" | "function_expression")),
    );
    functions
}

// -------------------------------------------------------------------- Go

/// Go functions whose parameters make them a program's command or test
/// entry: a urfave/cli action (`*cli.Context`, `*cli.Command`) or a
/// `TestMain` helper taking the `interface{ Run() int }` shape of
/// `*testing.M`.
fn go_entry_functions(root: AstNode<'_>) -> Vec<Range<usize>> {
    let scope = ImportScope::go(root);
    nodes_of_kind(
        root,
        &["function_declaration", "method_declaration", "func_literal"],
    )
    .into_iter()
    .filter(|function| {
        function
            .child_by_field_name("parameters")
            .is_some_and(|parameters| {
                parameters
                    .named_children()
                    .filter_map(|parameter| parameter.child_by_field_name("type"))
                    .any(|kind| is_urfave_type(kind, &scope) || is_test_runner_type(kind))
            })
    })
    .map(span)
    .collect()
}

/// `*cli.Context` or `*cli.Command` from any major version of urfave/cli.
fn is_urfave_type(kind: AstNode<'_>, scope: &ImportScope) -> bool {
    let kind = match kind.kind() {
        "pointer_type" => kind.named_child(0).unwrap_or(kind),
        _ => kind,
    };
    let Some((module, member)) = scope
        .resolve(kind)
        .filter(|_| kind.kind() == "qualified_type")
        .and_then(|resolved| {
            resolved
                .rsplit_once('.')
                .map(|(m, n)| (m.to_string(), n.to_string()))
        })
    else {
        return false;
    };
    let is_urfave = ["github.com/urfave/cli", "gopkg.in/urfave/cli"]
        .iter()
        .any(|base| {
            module
                .strip_prefix(base)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '.']))
        });
    is_urfave && matches!(member.as_str(), "Context" | "Command")
}

/// `interface{ Run() int }`: what `*testing.M` provides to a test runner.
fn is_test_runner_type(kind: AstNode<'_>) -> bool {
    kind.kind() == "interface_type"
        && kind
            .text()
            .split_whitespace()
            .collect::<String>()
            .eq("interface{Run()int}")
}
