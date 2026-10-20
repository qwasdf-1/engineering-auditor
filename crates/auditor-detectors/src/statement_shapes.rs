//! Statement-shape sites for languages whose L1 rules read the grammar
//! directly: empty handlers and branches, and awaits that serialize a loop.
//! The finders return nodes; the owners construct the findings.

use auditor_ast::AstNode;

use crate::ast_scan;

/// `except Exception: pass` (or `BaseException`, alone or in a tuple) with no
/// comment explaining it. A narrow handler such as `except KeyError: pass` is
/// Python's ask-forgiveness lookup idiom, and a bare `except:` belongs to
/// `try_except_pass` (ruff `S110` and Bandit `B110` defaults).
pub(crate) fn py_broad_empty_handlers(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["except_clause"])
        .into_iter()
        .filter(|handler| {
            let caught = handler
                .named_children()
                .find(|child| !matches!(child.kind(), "block" | "comment"));
            caught.is_some_and(is_broad_exception)
                && handler
                    .named_children()
                    .last()
                    .is_some_and(|body| py_block_is_empty(body))
                && ast_scan::nodes_of_kind(*handler, &["comment"]).is_empty()
        })
        .collect()
}

fn is_broad_exception(caught: AstNode<'_>) -> bool {
    match caught.kind() {
        "as_pattern" => caught.named_child(0).is_some_and(is_broad_exception),
        "tuple" | "parenthesized_expression" => caught.named_children().any(is_broad_exception),
        "identifier" => matches!(caught.text(), "Exception" | "BaseException"),
        _ => false,
    }
}

/// `if`, `elif`, `else`, `for`, and `while` clauses whose block is only
/// `pass` or `...`, with no comment in the clause.
pub(crate) fn py_empty_branches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut branches = ast_scan::nodes_of_kind(
        root,
        &[
            "if_statement",
            "elif_clause",
            "else_clause",
            "for_statement",
            "while_statement",
        ],
    )
    .into_iter()
    .filter(|clause| {
        let body = clause
            .child_by_field_name("consequence")
            .or_else(|| clause.child_by_field_name("body"));
        body.is_some_and(|body| {
            py_block_is_empty(body)
                && ast_scan::nodes_of_kind(body, &["comment"]).is_empty()
                && !clause
                    .named_children()
                    .any(|child| child.kind() == "comment" && child.end_byte() <= body.start_byte())
        })
    })
    .collect::<Vec<_>>();
    branches.sort_by_key(|clause| clause.start_byte());
    branches
}

fn py_block_is_empty(block: AstNode<'_>) -> bool {
    block.kind() == "block"
        && block
            .named_children()
            .filter(|statement| statement.kind() != "comment")
            .all(|statement| {
                statement.kind() == "pass_statement"
                    || statement.kind() == "expression_statement"
                        && statement
                            .named_child(0)
                            .is_some_and(|value| value.kind() == "ellipsis")
            })
}

/// `await` in the body of a `for` loop over a collection, in the same
/// function: each iteration waits for the previous one (ESLint
/// `no-await-in-loop`). `async for` consumes a stream, `while` loops are
/// event and polling loops, and a loop that can `break` or `return`, or that
/// sleeps, is sequential on purpose (a search, retry, or rate limit). So is
/// a loop that awaits each element itself (`await hook(request)`,
/// `await transport.aclose()`): dispatching to hooks or resources in order
/// is their contract, unlike applying one operation to independent inputs.
/// Awaiting a promise the element already holds (`await pair.value`) starts
/// no work, and `setTimeout` paces a loop like `sleep`.
pub(crate) fn py_awaits_in_loops(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    awaits_in_loops(root, &PYTHON_AWAITS)
}

/// The same boundary for Rust `.await` in a `for` body; `while let` stream
/// loops and `loop` event loops are excluded like Python's `while`.
pub(crate) fn rust_awaits_in_loops(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    awaits_in_loops(root, &RUST_AWAITS)
        .into_iter()
        .filter(|node| !ast_scan::is_inside_rust_test(*node))
        .collect()
}

/// The same boundary for JavaScript/TypeScript (ESLint `no-await-in-loop`'s
/// counting and `for-of` loops; `for await` consumes a stream).
pub(crate) fn js_awaits_in_loops(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    awaits_in_loops(root, &JS_AWAITS)
}

const JS_AWAITS: AwaitGrammar = AwaitGrammar {
    await_kind: "await_expression",
    for_kinds: &["for_statement", "for_in_statement"],
    stream_prefix: "for await",
    call_kind: "call_expression",
    exits: &["break_statement", "return_statement"],
    boundaries: &[
        "function_declaration",
        "function_expression",
        "function",
        "arrow_function",
        "method_definition",
        "generator_function_declaration",
    ],
};

pub(crate) fn csharp_awaits_in_loops(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    awaits_in_loops(root, &CSHARP_AWAITS)
}

const CSHARP_AWAITS: AwaitGrammar = AwaitGrammar {
    await_kind: "await_expression",
    for_kinds: &["for_statement", "foreach_statement"],
    stream_prefix: "await foreach",
    call_kind: "invocation_expression",
    exits: &["break_statement", "return_statement"],
    boundaries: &[
        "method_declaration",
        "constructor_declaration",
        "local_function_statement",
        "lambda_expression",
        "anonymous_method_expression",
    ],
};

/// A grammar's node kinds for the await-in-loop rule.
struct AwaitGrammar {
    await_kind: &'static str,
    for_kinds: &'static [&'static str],
    /// The prefix of a loop that consumes a stream (`async for`, `for await`).
    stream_prefix: &'static str,
    call_kind: &'static str,
    exits: &'static [&'static str],
    boundaries: &'static [&'static str],
}

const PYTHON_AWAITS: AwaitGrammar = AwaitGrammar {
    await_kind: "await",
    for_kinds: &["for_statement"],
    stream_prefix: "async",
    call_kind: "call",
    exits: &["break_statement", "return_statement"],
    boundaries: &["function_definition", "lambda"],
};

const RUST_AWAITS: AwaitGrammar = AwaitGrammar {
    await_kind: "await_expression",
    for_kinds: &["for_expression"],
    stream_prefix: "async",
    call_kind: "call_expression",
    exits: &["break_expression", "return_expression"],
    boundaries: &["function_item", "closure_expression", "async_block"],
};

fn awaits_in_loops<'a>(root: AstNode<'a>, grammar: &AwaitGrammar) -> Vec<AstNode<'a>> {
    ast_scan::nodes_of_kind(root, &[grammar.await_kind])
        .into_iter()
        .filter(|await_node| {
            enclosing_loop(*await_node, grammar.boundaries)
                .filter(|loop_node| {
                    grammar.for_kinds.contains(&loop_node.kind())
                        && !loop_node.text().starts_with(grammar.stream_prefix)
                })
                .is_some_and(|loop_node| {
                    !awaits_element(*await_node, loop_node, grammar.call_kind)
                        && !exits_early(loop_node, grammar.exits, grammar.boundaries)
                        && !ast_scan::nodes_of_kind(loop_node, &[grammar.call_kind])
                            .iter()
                            .any(|call| {
                                ast_scan::callee(*call)
                                    .rsplit(['.', ':'])
                                    .next()
                                    .is_some_and(|name| {
                                        matches!(name, "sleep" | "setTimeout" | "Delay")
                                    })
                            })
                })
        })
        .collect()
}

/// `if` and `else` blocks with nothing in them, not even a comment.
pub(crate) fn rust_empty_branches(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    let mut branches = ast_scan::nodes_of_kind(root, &["if_expression", "else_clause"])
        .into_iter()
        .filter(|branch| {
            let block = match branch.kind() {
                "if_expression" => branch.child_by_field_name("consequence"),
                _ => branch
                    .named_children()
                    .find(|child| child.kind() == "block"),
            };
            block.is_some_and(|block| block.named_child_count() == 0)
                && !ast_scan::is_inside_rust_test(*branch)
        })
        .collect::<Vec<_>>();
    branches.sort_by_key(|branch| branch.start_byte());
    branches
}

/// Whether the awaited call's callee or receiver is a loop binding.
fn awaits_element(await_node: AstNode<'_>, loop_node: AstNode<'_>, call_kind: &str) -> bool {
    // A counting loop's element is `items[i]`; awaiting a call on it waits on
    // each element in turn, as `for x of items` does.
    if let Some(counter) = loop_counter(loop_node) {
        let awaited = await_node
            .named_child(0)
            .map_or("", |awaited| awaited.text());
        if awaited.split(['.', '(']).next().is_some_and(|head| {
            head.trim_end()
                .strip_suffix(']')
                .and_then(|indexed| indexed.rsplit_once('['))
                .is_some_and(|(_, index)| index.trim() == counter)
        }) {
            return true;
        }
    }
    let Some(binding) = loop_node
        .child_by_field_name("left")
        .or_else(|| loop_node.child_by_field_name("pattern"))
    else {
        return false;
    };
    let bindings = std::iter::once(binding)
        .chain(ast_scan::nodes_of_kind(binding, &["identifier"]))
        .filter(|node| node.kind() == "identifier")
        .map(|node| node.text())
        .collect::<Vec<_>>();
    await_node
        .named_child(0)
        // `await pair.value` waits on a promise the loop already holds.
        .map(|awaited| {
            if awaited.kind() == call_kind {
                ast_scan::callee(awaited)
            } else {
                awaited.text()
            }
        })
        .and_then(|head| head.split(['.', '(', '[']).next())
        .is_some_and(|head| bindings.contains(&head.trim()))
}

/// The variable a counting `for` loop declares in its initializer
/// (`for (let i = 0; ..)`, `for (int i = 0; ..)`).
fn loop_counter(loop_node: AstNode<'_>) -> Option<&str> {
    if loop_node.kind() != "for_statement" {
        return None;
    }
    let declared = loop_node
        .child_by_field_name("initializer")
        .or_else(|| loop_node.child_by_field_name("init"))
        .and_then(|initializer| {
            ast_scan::nodes_of_kind(initializer, &["variable_declarator"])
                .into_iter()
                .next()
        })
        .and_then(|declarator| declarator.child_by_field_name("name"))
        .map(|name| name.text());
    // `for (; i < n; i++)` counts with a variable declared before the loop.
    declared.or_else(|| {
        loop_node
            .named_children()
            .find(|child| {
                matches!(
                    child.kind(),
                    "postfix_unary_expression" | "prefix_unary_expression" | "update_expression"
                )
            })
            .and_then(|update| update.named_child(0))
            .filter(|counter| counter.kind() == "identifier")
            .map(|counter| counter.text())
    })
}

/// The innermost loop whose body contains `node`, unless a function
/// boundary comes first. A node in the loop's header is not in its body.
fn enclosing_loop<'a>(node: AstNode<'a>, boundaries: &[&str]) -> Option<AstNode<'a>> {
    let mut child = node;
    for ancestor in std::iter::successors(node.parent(), |parent| parent.parent()) {
        if boundaries.contains(&ancestor.kind()) {
            return None;
        }
        if ["for_", "foreach_", "while_", "loop_"]
            .iter()
            .any(|prefix| ancestor.kind().starts_with(prefix))
        {
            let body = ancestor.child_by_field_name("body")?;
            return (child.start_byte() >= body.start_byte()
                && child.end_byte() <= body.end_byte())
            .then_some(ancestor);
        }
        child = ancestor;
    }
    None
}

/// Whether `loop_node` holds an exit of `kinds` that belongs to it: not in a
/// nested function, and (for `break`) not in a nested loop.
fn exits_early(loop_node: AstNode<'_>, kinds: &[&str], boundaries: &[&str]) -> bool {
    ast_scan::nodes_of_kind(loop_node, kinds)
        .into_iter()
        .any(|exit| {
            std::iter::successors(exit.parent(), |parent| parent.parent())
                .take_while(|ancestor| {
                    ancestor.start_byte() != loop_node.start_byte()
                        || ancestor.kind() != loop_node.kind()
                })
                .all(|ancestor| {
                    !boundaries.contains(&ancestor.kind())
                        && !(exit.kind().starts_with("break")
                            && (ancestor.kind().starts_with("for_")
                                || ancestor.kind().starts_with("foreach_")
                                || ancestor.kind().starts_with("while_")
                                || ancestor.kind().starts_with("loop_")))
                })
        })
}
