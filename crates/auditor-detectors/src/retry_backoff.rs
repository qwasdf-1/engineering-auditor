//! `retry_without_backoff`: a loop that retries a failed operation at once,
//! with no wait between attempts. When the failure is an overloaded or
//! unreachable peer, immediate retries add load at the moment it can least
//! absorb it and fail again for the same reason; the attempts finish in
//! microseconds, so a bounded loop gives up before a transient fault clears.
//!
//! A retry loop repeats the same operation: a counted loop whose counter the
//! operation does not use (`for _ in range(3)`, `for i := 0; i < n; i++`,
//! `for _ in 0..3`), or a `while`/`loop` whose condition calls nothing and
//! which names its attempts (an `attempt`, `retries`, or `tries` identifier
//! in it), since an open loop may be reading, polling, or iterating. Its body handles the operation's failure and goes round
//! again: a `try` whose success leaves the loop and whose handler falls
//! through, a Go `if err == nil { break }` (or `if err != nil { continue }`
//! before the exit), or a Rust `match` on a `Result` whose `Ok` arm leaves
//! the loop and whose `Err` arm falls through. A wait anywhere in the loop is
//! a backoff: a call naming one (`sleep`, `wait`, `delay`, `backoff`,
//! `setTimeout`, `select`, `time.After`, `park`), a Go `select`, or a call to
//! a function of the same file that waits.

use std::collections::BTreeSet;

use auditor_ast::AstNode;
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, descendants, nodes_of_kind, AstScan};
use crate::{util, Language};

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
    )
}

pub(crate) fn retry_without_backoff_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(scan, supported, |file, tree, _| {
        let relative = util::path_slash(&file.relative);
        let language = &file.language;
        let waiting = waiting_functions(tree.root_node());
        findings.extend(
            nodes_of_kind(tree.root_node(), loop_kinds(language))
                .into_iter()
                .filter(|node| {
                    !(*language == Language::Rust && ast_scan::is_inside_rust_test(*node))
                })
                .filter_map(|node| retry_loop(node, language, &waiting))
                .map(|(node, operation)| finding(node, operation, &relative)),
        );
    });
    findings
}

fn loop_kinds(language: &Language) -> &'static [&'static str] {
    match language {
        Language::Python => &["for_statement", "while_statement"],
        Language::Go => &["for_statement"],
        Language::Rust => &["loop_expression", "while_expression", "for_expression"],
        _ => &["for_statement", "while_statement", "do_statement"],
    }
}

/// Loops a `break` can leave, including those iterating data.
fn is_loop(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "for_statement"
            | "while_statement"
            | "do_statement"
            | "for_in_statement"
            | "enhanced_for_statement"
            | "loop_expression"
            | "while_expression"
            | "for_expression"
    )
}

/// Statements a `break` leaves instead of the loop around them.
fn is_switch(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "switch_statement"
            | "switch_expression"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
    )
}

fn is_function(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "function_definition"
            | "lambda"
            | "class_definition"
            | "function_declaration"
            | "function_expression"
            | "function"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "class_body"
            | "method_declaration"
            | "constructor_declaration"
            | "lambda_expression"
            | "func_literal"
            | "function_item"
            | "closure_expression"
    )
}

/// The loop and its retried operation, when `node` retries an operation
/// with no wait between attempts.
fn retry_loop<'a>(
    node: AstNode<'a>,
    language: &Language,
    waiting: &BTreeSet<&str>,
) -> Option<(AstNode<'a>, AstNode<'a>)> {
    let body = node.child_by_field_name("body")?;
    let (counters, counted) = attempt_counters(node, language)?;
    // A counted loop repeats by construction; an open one must name its
    // attempts, or it may be reading, polling, or iterating.
    if (!counted && !counts_attempts(node)) || waits(node, waiting) {
        return None;
    }
    let operation = match language {
        Language::Go => go_retry(node, body),
        Language::Rust => rust_retry(node, body),
        _ => try_retry(node, body, language),
    }?;
    let varying = derived_names(body, counters);
    let uses_counter = descendants(operation)
        .any(|child| child.kind() == "identifier" && varying.contains(child.text()));
    (!uses_counter).then_some((node, operation))
}

/// `counters` and the names the loop body assigns from them
/// (`base_url = base_urls[i]`): an operation reading one tries a different
/// target each pass, not the same one again.
fn derived_names<'a>(body: AstNode<'a>, counters: Vec<&'a str>) -> BTreeSet<&'a str> {
    let assignments: Vec<(AstNode<'a>, AstNode<'a>)> = descendants(body)
        .filter_map(|node| {
            let target = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name"))
                .or_else(|| node.child_by_field_name("pattern"))?;
            let value = node
                .child_by_field_name("right")
                .or_else(|| node.child_by_field_name("value"))?;
            Some((target, value))
        })
        .collect();
    let mut names: BTreeSet<&str> = counters.into_iter().collect();
    loop {
        let before = names.len();
        for (target, value) in &assignments {
            if descendants(*value)
                .any(|child| child.kind() == "identifier" && names.contains(child.text()))
            {
                names.extend(
                    descendants(*target)
                        .filter(|child| child.kind() == "identifier")
                        .map(|child| child.text()),
                );
            }
        }
        if names.len() == before {
            return names;
        }
    }
}

/// The names a counted loop declares and whether it is counted, when `node`
/// repeats one operation: a counted range, or a condition with no call.
fn attempt_counters<'a>(node: AstNode<'a>, language: &Language) -> Option<(Vec<&'a str>, bool)> {
    let identifiers = |part: AstNode<'a>| {
        descendants(part)
            .filter(|child| child.kind() == "identifier")
            .map(|child| child.text())
            .collect::<Vec<_>>()
    };
    let call_free = |part: Option<AstNode<'a>>| {
        part.is_none_or(|part| !descendants(part).any(|child| is_call(child)))
    };
    let names = |part: Option<AstNode<'a>>| part.map(identifiers).unwrap_or_default();
    match (language, node.kind()) {
        (Language::Python, "for_statement") => {
            let range = node.child_by_field_name("right")?;
            let counted = range.kind() == "call" && ast_scan::callee(range) == "range";
            counted.then(|| (names(node.child_by_field_name("left")), true))
        }
        (Language::Rust, "for_expression") => {
            let counted = node.child_by_field_name("value")?.kind() == "range_expression";
            counted.then(|| (names(node.child_by_field_name("pattern")), true))
        }
        (Language::Rust, "while_expression") => {
            let condition = node.child_by_field_name("condition")?;
            (condition.kind() != "let_condition" && call_free(Some(condition)))
                .then(|| (Vec::new(), false))
        }
        (Language::Go, _) => {
            if let Some(clause) = first_named_child_of_kind(node, "for_clause") {
                let condition = clause.child_by_field_name("condition");
                let counters = names(clause.child_by_field_name("initializer"));
                let counted = !counters.is_empty();
                call_free(condition).then_some((counters, counted))
            } else if first_named_child_of_kind(node, "range_clause").is_some() {
                None
            } else {
                let condition = node
                    .named_children()
                    .find(|child| child.kind() != "block" && child.kind() != "comment");
                call_free(condition).then(|| (Vec::new(), false))
            }
        }
        (_, "for_statement") => {
            let counters = names(
                node.child_by_field_name("initializer")
                    .or_else(|| node.child_by_field_name("init")),
            );
            let counted = !counters.is_empty();
            call_free(node.child_by_field_name("condition")).then_some((counters, counted))
        }
        (_, "while_statement" | "do_statement") => {
            call_free(node.child_by_field_name("condition")).then(|| (Vec::new(), false))
        }
        _ => Some((Vec::new(), false)),
    }
}

/// Whether the loop names its attempts: an identifier in it whose words
/// include `attempt`, `retry`, or `tries` (`attempt`, `max_retries`,
/// `triesLeft`), outside nested functions.
fn counts_attempts(node: AstNode<'_>) -> bool {
    descendants(node)
        .filter(|child| {
            matches!(
                child.kind(),
                "identifier" | "field_identifier" | "property_identifier"
            )
        })
        .filter(|child| {
            !ancestors(*child)
                .take_while(|ancestor| *ancestor != node)
                .any(is_function)
        })
        .any(|child| {
            util::split_identifier(child.text()).iter().any(|word| {
                matches!(
                    word.as_str(),
                    "attempt" | "attempts" | "retry" | "retries" | "retried" | "tries"
                )
            })
        })
}

/// Whether `node` waits: a call naming a wait or a function of this file
/// that waits, or a Go `select`.
fn waits(node: AstNode<'_>, waiting: &BTreeSet<&str>) -> bool {
    descendants(node).any(|child| {
        child.kind() == "select_statement"
            || (is_call(child)
                && (names_wait(callee_name(child))
                    || waiting.contains(last_segment(callee_name(child)))))
            || (child.kind() == "macro_invocation"
                && child
                    .child_by_field_name("macro")
                    .is_some_and(|name| names_wait(name.text())))
    })
}

/// The names of this file's functions that wait, directly or through
/// another of them (`can_read` calling `select.select` with a timeout).
fn waiting_functions(root: AstNode<'_>) -> BTreeSet<&str> {
    let functions: Vec<(&str, AstNode<'_>)> = descendants(root)
        .filter(|node| is_function(*node) && !node.kind().starts_with("class"))
        .filter_map(|node| Some((node.child_by_field_name("name")?.text(), node)))
        .collect();
    let mut waiting = BTreeSet::new();
    loop {
        let before = waiting.len();
        for (name, function) in &functions {
            if !waiting.contains(name) && waits(*function, &waiting) {
                waiting.insert(*name);
            }
        }
        if waiting.len() == before {
            return waiting;
        }
    }
}

fn last_segment(callee: &str) -> &str {
    callee
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(callee)
}

fn names_wait(callee: &str) -> bool {
    let last = last_segment(callee);
    let words = util::split_identifier(last);
    words.iter().any(|word| {
        matches!(
            word.as_str(),
            "sleep"
                | "wait"
                | "delay"
                | "backoff"
                | "pause"
                | "throttle"
                | "timeout"
                | "park"
                | "jitter"
                | "select"
        )
    }) || last == "After"
}

fn is_call(node: AstNode<'_>) -> bool {
    matches!(
        node.kind(),
        "call" | "call_expression" | "method_invocation" | "new_expression"
    )
}

fn callee_name(node: AstNode<'_>) -> &str {
    match node.kind() {
        "method_invocation" => node
            .child_by_field_name("name")
            .map(|name| name.text())
            .unwrap_or_default(),
        _ => ast_scan::callee(node),
    }
}

/// A `try` owned by the loop whose success leaves the loop and whose
/// handler falls through to the next attempt; its body is the operation.
fn try_retry<'a>(node: AstNode<'a>, body: AstNode<'a>, language: &Language) -> Option<AstNode<'a>> {
    descendants(body)
        .filter(|child| {
            matches!(
                child.kind(),
                "try_statement" | "try_with_resources_statement"
            )
        })
        .filter(|statement| owner_loop(*statement) == Some(node))
        .find_map(|statement| {
            let block = statement.child_by_field_name("body")?;
            let handlers: Vec<_> = statement
                .named_children()
                .filter(|child| matches!(child.kind(), "except_clause" | "catch_clause"))
                .chain(statement.child_by_field_name("handler"))
                .collect();
            let success = statement
                .named_children()
                .filter(|child| child.start_byte() >= block.start_byte())
                .filter(|child| matches!(child.kind(), "block" | "statement_block" | "else_clause"))
                .any(|part| leaves(part, node))
                || exits_after(statement, node);
            let retries = handlers
                .iter()
                .any(|handler| !ends_in_exit(handler_body(*handler, language)));
            (success && retries).then_some(block)
        })
}

fn handler_body<'a>(handler: AstNode<'a>, language: &Language) -> Option<AstNode<'a>> {
    match language {
        Language::Python => handler
            .named_children()
            .filter(|child| child.kind() == "block")
            .last(),
        _ => handler.child_by_field_name("body"),
    }
}

/// A Go `if err == nil { break }`, or `if err != nil { continue }` before an
/// exit, after the call assigning `err`; that call is the operation.
fn go_retry<'a>(node: AstNode<'a>, body: AstNode<'a>) -> Option<AstNode<'a>> {
    let body_statements = statements(body);
    body_statements
        .iter()
        .enumerate()
        .find_map(|(index, statement)| {
            if statement.kind() != "if_statement" {
                return None;
            }
            let condition = statement.child_by_field_name("condition")?;
            let consequence = statement.child_by_field_name("consequence")?;
            let operator = condition.child_by_field_name("operator")?.kind();
            let left = condition.child_by_field_name("left")?;
            let right = condition.child_by_field_name("right")?;
            if right.kind() != "nil" || !left.text().to_ascii_lowercase().ends_with("err") {
                return None;
            }
            let retries = match operator {
                "==" => leaves(consequence, node),
                "!=" => {
                    let last = *statements(consequence).last()?;
                    last.kind() == "continue_statement"
                        && body_statements[index + 1..]
                            .iter()
                            .any(|later| leaves(*later, node))
                }
                _ => false,
            };
            if !retries {
                return None;
            }
            body_statements[..index].iter().rev().find_map(|earlier| {
                let assigns = matches!(
                    earlier.kind(),
                    "short_var_declaration" | "assignment_statement"
                ) && earlier
                    .child_by_field_name("left")
                    .is_some_and(|names| names.text().contains(left.text()));
                let call = earlier
                    .child_by_field_name("right")?
                    .named_children()
                    .find(|value| value.kind() == "call_expression");
                assigns.then_some(call).flatten()
            })
        })
}

/// A Rust `match` on a call whose `Ok`/`Some` arm leaves the loop and whose
/// other arm falls through, or an `if let Ok(..) = call { break }`.
fn rust_retry<'a>(node: AstNode<'a>, body: AstNode<'a>) -> Option<AstNode<'a>> {
    descendants(body)
        .filter(|child| matches!(child.kind(), "match_expression" | "if_expression"))
        .filter(|expression| owner_loop(*expression) == Some(node))
        .find_map(|expression| {
            if expression.kind() == "if_expression" {
                let condition = expression.child_by_field_name("condition")?;
                let pattern = condition.child_by_field_name("pattern")?.text();
                let value = condition.child_by_field_name("value")?;
                let consequence = expression.child_by_field_name("consequence")?;
                let success = pattern.starts_with("Ok(");
                return (condition.kind() == "let_condition"
                    && success
                    && invokes(value)
                    && leaves(consequence, node))
                .then_some(value);
            }
            let value = expression.child_by_field_name("value")?;
            let arms: Vec<_> = descendants(expression.child_by_field_name("body")?)
                .filter(|arm| {
                    arm.kind() == "match_arm"
                        && arm.parent().and_then(|block| block.parent()) == Some(expression)
                })
                .collect();
            let arm_parts = |arm: &AstNode<'a>| {
                Some((
                    arm.child_by_field_name("pattern")?.text(),
                    arm.child_by_field_name("value")?,
                ))
            };
            let success = arms
                .iter()
                .filter_map(arm_parts)
                .any(|(pattern, value)| pattern.starts_with("Ok(") && leaves(value, node));
            let retries = arms.iter().filter_map(arm_parts).any(|(pattern, value)| {
                (pattern.starts_with("Err(") || pattern == "_")
                    && !exits(value)
                    && !descendants(value).any(|child| leaves_loop(child, node))
            });
            (invokes(value) && success && retries).then_some(value)
        })
}

/// Whether `node` is or awaits a call.
fn invokes(node: AstNode<'_>) -> bool {
    is_call(node) || (node.kind() == "await_expression" && descendants(node).any(is_call))
}

/// Whether `part` holds a `break` or `return` that leaves `node`.
fn leaves(part: AstNode<'_>, node: AstNode<'_>) -> bool {
    descendants(part).any(|child| leaves_loop(child, node))
}

fn leaves_loop(child: AstNode<'_>, node: AstNode<'_>) -> bool {
    match child.kind() {
        "break_statement" | "break_expression" => owner_loop(child) == Some(node),
        "return_statement" | "return_expression" => !ancestors(child)
            .take_while(|ancestor| *ancestor != node)
            .any(is_function),
        _ => false,
    }
}

/// Whether the statement after `statement` in its block leaves `node`.
fn exits_after(statement: AstNode<'_>, node: AstNode<'_>) -> bool {
    let Some(parent) = statement.parent() else {
        return false;
    };
    parent
        .named_children()
        .skip_while(|sibling| *sibling != statement)
        .nth(1)
        .is_some_and(|next| exits(next) && leaves_loop(next, node))
}

fn ends_in_exit(block: Option<AstNode<'_>>) -> bool {
    block
        .and_then(|block| {
            block
                .named_children()
                .filter(|child| child.kind() != "comment")
                .last()
        })
        .is_some_and(exits)
}

fn exits(node: AstNode<'_>) -> bool {
    let node = if node.kind() == "expression_statement" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
    };
    matches!(
        node.kind(),
        "raise_statement"
            | "throw_statement"
            | "throw_expression"
            | "return_statement"
            | "return_expression"
            | "break_statement"
            | "break_expression"
    ) || (node.kind() == "macro_invocation" && node.text().starts_with("panic!"))
}

/// The innermost loop a `break` at `node` leaves, if no function or switch
/// lies between them.
fn owner_loop(node: AstNode<'_>) -> Option<AstNode<'_>> {
    for ancestor in ancestors(node) {
        if is_function(ancestor) || is_switch(ancestor) {
            return None;
        }
        if is_loop(ancestor) {
            return Some(ancestor);
        }
    }
    None
}

fn ancestors(node: AstNode<'_>) -> impl Iterator<Item = AstNode<'_>> {
    std::iter::successors(node.parent(), |node| node.parent())
}

/// A block's statements, through Go's `statement_list`.
fn statements(block: AstNode<'_>) -> Vec<AstNode<'_>> {
    block
        .named_children()
        .flat_map(|child| {
            if child.kind() == "statement_list" {
                child.named_children().collect()
            } else {
                vec![child]
            }
        })
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn first_named_child_of_kind<'a>(node: AstNode<'a>, kind: &str) -> Option<AstNode<'a>> {
    node.named_children().find(|child| child.kind() == kind)
}

fn finding(node: AstNode<'_>, operation: AstNode<'_>, relative: &str) -> Finding {
    let line = node.start_position().0 + 1;
    let operation_line = operation.start_position().0 + 1;
    let summary = ast_scan::source_summary(operation);
    auditor_core::finding!(
        "retry_without_backoff",
        "systems_economics",
        "Retry without backoff",
        format!(
            "The loop at line {line} retries the operation at line {operation_line} immediately after it fails, with no wait between attempts"
        ),
        Severity::Medium,
        Confidence::Medium,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Wait between attempts with exponential backoff and jitter (or use the client's retry policy), so retries do not add load to a failing peer and outlast a transient fault.",
    )
    .with_identity("operation", summary.as_str())
    .with_evidence("operation_line", operation_line)
}
