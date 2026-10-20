use auditor_ast::{AstLang, AstNode};
use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::{util, Language};

pub(crate) fn unreachable_code_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    visit_flow_trees(scan, |root, language, relative| {
        collect_unreachable(root, language, relative, &mut findings);
    });
    findings
}

pub(crate) fn constant_condition_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    visit_flow_trees(scan, |root, _, relative| {
        collect_constant_conditions(root, relative, &mut findings);
    });
    findings
}

fn visit_flow_trees(scan: AstScan<'_, '_>, mut visit: impl FnMut(AstNode<'_>, AstLang, &str)) {
    ast_scan::visit_ast_files(scan, flow_language, |file, tree, language| {
        let relative = util::path_slash(&file.relative);
        visit(tree.root_node(), language, &relative);
    });
}

fn flow_language(language: &Language) -> bool {
    matches!(
        language,
        Language::Rust
            | Language::Python
            | Language::JavaScript
            | Language::TypeScript
            | Language::Go
            | Language::Java
            | Language::Kotlin
            | Language::CSharp
            | Language::C
            | Language::Cpp
    )
}

fn unreachable_finding(relative: &str, line: usize, cause: &str) -> Finding {
    auditor_core::finding!(
        "unreachable_code",
        "correctness",
        "Statement is unreachable",
        format!("Code at line {line} follows an unconditional {cause}."),
        Severity::Medium,
        Confidence::High,
        relative,
        Some(line),
    )
    .with_recommendation(
        "Remove the unreachable statement or restructure the preceding control flow.",
    )
    .with_evidence("terminator", cause)
    .with_evidence("analysis", "ast")
}

fn collect_unreachable(
    node: AstNode<'_>,
    language: AstLang,
    relative: &str,
    findings: &mut Vec<Finding>,
) {
    if is_statement_container(node.kind()) {
        collect_unreachable_statements(node, language, relative, findings);
    } else {
        for child in node.named_children() {
            collect_unreachable(child, language, relative, findings);
        }
    }
}

/// Reports the first statement after each unconditional terminator in a
/// statement list, then recurses into the reachable statements.
fn collect_unreachable_statements(
    node: AstNode<'_>,
    language: AstLang,
    relative: &str,
    findings: &mut Vec<Finding>,
) {
    let mut terminator = None;
    let mut reported_segment = false;
    let mut cfg_gated = false;
    for child in node.named_children() {
        // A `case`/`default` or `goto` label is an entry point of its own.
        if is_entry_label(child.kind()) {
            terminator = None;
            reported_segment = false;
        }
        if let Some(cause) = terminator {
            if is_non_executing_after_terminator(child, language) {
                continue;
            }
            if !reported_segment {
                findings.push(unreachable_finding(
                    relative,
                    child.start_position().0 + 1,
                    cause,
                ));
                reported_segment = true;
            }
            continue;
        }

        collect_unreachable(child, language, relative, findings);
        // `#[cfg(..)] return x;` only terminates in one configuration.
        terminator = (!cfg_gated).then(|| termination_cause(child)).flatten();
        cfg_gated = child.kind() == "attribute_item" && child.text().contains("cfg(");
    }
}

fn termination_cause(node: AstNode<'_>) -> Option<&'static str> {
    direct_termination_cause(node).or_else(|| {
        if is_statement_container(node.kind()) {
            return node
                .named_children()
                .filter(|child| !is_trivia_kind(child.kind()))
                .last()
                .and_then(termination_cause);
        }
        if is_if_node(node.kind()) && if_alternatives_all_terminate(node)? {
            return Some("branch whose alternatives terminate");
        }
        None
    })
}

/// Every arm of an if terminates. Python lists each `elif`/`else` as its own
/// `alternative` field, so the chain is exhaustive only when it ends in `else`.
fn if_alternatives_all_terminate(node: AstNode<'_>) -> Option<bool> {
    let consequence = node.child_by_field_name("consequence")?;
    let alternative = node.child_by_field_name("alternative")?;
    if alternative.kind() == "elif_clause" {
        let arms: Vec<_> = node
            .named_children()
            .filter(|child| matches!(child.kind(), "elif_clause" | "else_clause"))
            .collect();
        let exhaustive = arms.last().is_some_and(|arm| arm.kind() == "else_clause");
        return Some(
            exhaustive
                && termination_cause(consequence).is_some()
                && arms
                    .iter()
                    .all(|arm| termination_cause(branch_body(*arm)).is_some()),
        );
    }
    Some(
        termination_cause(branch_body(consequence)).is_some()
            && termination_cause(branch_body(alternative)).is_some(),
    )
}

fn direct_termination_cause(node: AstNode<'_>) -> Option<&'static str> {
    match node.kind() {
        "return_statement" | "return_expression" | "return" => Some("return"),
        "throw_statement" | "throw_expression" | "raise_statement" => Some("throw"),
        "break_statement" | "break_expression" | "break" => Some("break"),
        "continue_statement" | "continue_expression" | "continue" => Some("continue"),
        "jump_expression" => jump_cause(node.text()),
        "expression_statement" if node.named_child_count() == 1 => {
            direct_termination_cause(node.named_child(0)?)
        }
        _ => None,
    }
}

fn jump_cause(text: &str) -> Option<&'static str> {
    let first = text.split_whitespace().next().unwrap_or_default();
    if first.starts_with("return") {
        Some("return")
    } else if first.starts_with("throw") {
        Some("throw")
    } else if first.starts_with("break") {
        Some("break")
    } else if first.starts_with("continue") {
        Some("continue")
    } else {
        None
    }
}

fn branch_body(node: AstNode<'_>) -> AstNode<'_> {
    if matches!(node.kind(), "else_clause" | "elif_clause") {
        node.named_children().last().unwrap_or(node)
    } else {
        node
    }
}

fn is_statement_container(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "statement_block"
            | "statement_list"
            | "statements"
            | "compound_statement"
            | "program"
            | "module"
            | "switch_case"
            | "switch_default"
            | "case_statement"
            | "case_clause"
            | "default_clause"
    )
}

fn is_entry_label(kind: &str) -> bool {
    matches!(
        kind,
        "case_statement"
            | "switch_case"
            | "switch_default"
            | "switch_section"
            | "case_clause"
            | "default_clause"
            | "labeled_statement"
    )
}

fn is_if_node(kind: &str) -> bool {
    matches!(kind, "if_statement" | "if_expression" | "if")
}

fn is_non_executing_after_terminator(node: AstNode<'_>, language: AstLang) -> bool {
    if is_trivia_kind(node.kind()) {
        return true;
    }
    match language {
        AstLang::JavaScript | AstLang::TypeScript => {
            node.kind() == "function_declaration"
                || node.kind() == "variable_declaration" && !node.text().contains('=')
        }
        AstLang::Rust => node.kind() == "function_item",
        // A local function declared after `return` is called, not run in place.
        AstLang::CSharp => node.kind() == "local_function_statement",
        // `raise ...` followed by `yield` is the idiom that makes a function a
        // generator; the yield is load-bearing even though it never runs.
        AstLang::Python => {
            node.kind() == "expression_statement" && node.text().trim_start().starts_with("yield")
        }
        _ => false,
    }
}

/// Comments, and preprocessor directives (`#pragma`, `#region`), which
/// neither run nor end a path.
fn is_trivia_kind(kind: &str) -> bool {
    matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "multiline_comment"
    ) || kind.starts_with("preproc_")
}

fn collect_constant_conditions(node: AstNode<'_>, relative: &str, findings: &mut Vec<Finding>) {
    if is_condition_owner(node.kind()) {
        if let Some(condition) = node.child_by_field_name("condition") {
            let condition = unwrap_condition(condition);
            if constant_condition(condition).is_some()
                && !is_intentional_infinite_loop(node.kind(), condition)
            {
                let line = condition.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "constant_condition",
                        "correctness",
                        "Control-flow condition is constant",
                        format!(
                            "Condition `{}` at line {line} does not depend on runtime state.",
                            condition.text().trim()
                        ),
                        Severity::Medium,
                        Confidence::High,
                        relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Remove the constant branch or replace the condition with the intended runtime predicate.",
                    )
                    .with_evidence("condition", condition.text().trim())
                    .with_evidence("analysis", "ast"),
                );
            }
        }
    }
    for child in node.named_children() {
        collect_constant_conditions(child, relative, findings);
    }
}

fn is_condition_owner(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "if_expression"
            | "while_statement"
            | "while_expression"
            | "do_statement"
            | "do_while_statement"
            | "for_statement"
            | "conditional_expression"
            | "ternary_expression"
    )
}

fn unwrap_condition(mut node: AstNode<'_>) -> AstNode<'_> {
    while matches!(
        node.kind(),
        "parenthesized_expression" | "parenthesized_condition" | "condition_clause"
    ) && node.named_child_count() == 1
    {
        node = node.named_child(0).unwrap_or(node);
    }
    node
}

fn constant_condition(node: AstNode<'_>) -> Option<&str> {
    let text = node.text().trim();
    if matches!(
        text,
        "true" | "false" | "True" | "False" | "None" | "null" | "nil"
    ) || matches!(
        node.kind(),
        "integer_literal" | "float_literal" | "number" | "number_literal"
    ) && text.parse::<f64>().is_ok()
    {
        return Some(text);
    }
    None
}

fn is_intentional_infinite_loop(owner_kind: &str, condition: AstNode<'_>) -> bool {
    matches!(
        owner_kind,
        "while_statement"
            | "while_expression"
            | "for_statement"
            | "do_statement"
            | "do_while_statement"
    ) && matches!(condition.text().trim(), "true" | "True" | "1")
}

#[cfg(test)]
#[path = "control_flow_tests.rs"]
mod tests;
