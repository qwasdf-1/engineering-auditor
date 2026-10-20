use crate::AstNode;

mod halstead;

pub use halstead::{halstead, HalsteadMetrics};
use halstead::{halstead_body, is_halstead_function_node, is_halstead_leaf, record_halstead_leaf};

/// Branch decision nodes per language
const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "if",
    "if_expression",
    "for_statement",
    "for",
    "for_expression",
    "for_in_statement",
    "while_statement",
    "while",
    "while_expression",
    "loop_expression",
    "do_statement",
    "conditional_expression",
    "ternary_expression",
];

/// Count branch decision points within a function
pub fn branch_count(node: &AstNode) -> usize {
    let mut count = 0usize;
    count_branches(node, &mut count);
    count
}

fn count_branches(node: &AstNode, count: &mut usize) {
    let kind = node.kind();
    if node.is_named() && (BRANCH_KINDS.contains(&kind) || FLOW_COMPENSATING_KINDS.contains(&kind))
    {
        *count += 1;
    }
    for child in node.children() {
        count_branches(&child, count);
    }
}

/// Loops and exception handlers: +1 plus nesting, and they nest their bodies.
const FLOW_NESTING_KINDS: &[&str] = &[
    "for_statement",
    "for",
    "for_expression",
    "for_in_statement",
    "enhanced_for_statement",
    "foreach_statement",
    "while_statement",
    "while",
    "while_expression",
    "loop_expression",
    "do_statement",
    "do_while_statement",
    "catch",
    "except",
    "catch_clause",
    "except_clause",
];

/// A switch or match counts once, +1 plus nesting, however many arms it has.
const FLOW_CONTAINER_KINDS: &[&str] = &[
    "switch_statement",
    "switch_expression",
    "switch",
    "match_expression",
    "match_statement",
    "match",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
    "when_expression",
];

/// Case/arm nodes: decisions for cyclomatic counting, which sit at their
/// container's depth. Cognitive complexity gives them no increment.
const FLOW_COMPENSATING_KINDS: &[&str] = &[
    "match_arm",
    "case_clause",
    "case_statement",
    "switch_case",
    "switch_default",
    "switch_label",
    "switch_rule",
    "expression_case",
    "type_case",
    "communication_case",
    "when_entry",
];

/// Named else/elif clauses: +1 with no nesting penalty (Sonar's hybrid increment).
const FLOW_FLAT_KINDS: &[&str] = &["else", "elif", "else_clause", "elif_clause", "alternative"];

/// Nested functions and closures, which are scored on their own.
const LAMBDA_KINDS: &[&str] = &[
    "lambda_expression",
    "lambda",
    "arrow_function",
    "closure_expression",
    "anonymous_function",
    "anonymous_method",
    "anonymous_method_expression",
    "func_literal",
    "lambda_literal",
];

/// Short-circuit boolean operators.
const BOOL_OP_KINDS: &[&str] = &["&&", "||", "and", "or"];

/// Sonar cognitive complexity breakdown categories.
#[derive(Debug, Clone, Copy, Default)]
pub struct CognitiveBreakdown {
    /// Total cognitive complexity score
    pub score: usize,
    /// Structural increments (if/loop/switch/catch/ternary) — each adds 1 + nesting
    pub structural: usize,
    /// Hybrid increments (else/else if) — each adds 1, no nesting bonus
    pub hybrid: usize,
    /// Fundamental increments (boolean sequences, labeled jumps, recursion) — each adds 1
    pub fundamental: usize,
    /// Nesting points the structural increments added, so the four counts
    /// sum to the score
    pub nesting_increments: usize,
    /// Whether self-recursion was detected
    pub recursion: bool,
}

/// Cognitive complexity per the SonarSource specification (G. Ann Campbell,
/// "Cognitive Complexity", 2017):
///
/// - +1 plus the nesting level for `if`, a ternary, a whole `switch`/`match`,
///   each loop, and each `catch`; each also nests what it contains.
/// - +1 with no nesting penalty for `else if` and `else`; an `else if` chain
///   stays at its first `if`'s level.
/// - +1 for each run of like boolean operators: `a && b && c` adds 1,
///   `a && b || c` adds 2.
/// - +1 for a labeled jump, and +1 once if the function calls itself.
///
/// Nested functions and closures are scored as their own functions.
pub fn cognitive_complexity(node: &AstNode, function_name: &str) -> CognitiveBreakdown {
    function_complexity(node, function_name).cognitive_breakdown
}

/// A `break` or `continue` that names its target: `break outer;` (Java,
/// JavaScript, Go) or `break 'outer` (Rust, where an unlabeled identifier
/// child is a break value instead).
fn is_labeled_jump(node: &AstNode) -> bool {
    let labels: &[&str] = if node.kind().ends_with("_expression") {
        &["label"]
    } else {
        &["identifier", "statement_identifier", "label_name"]
    };
    node.named_children()
        .any(|child| labels.contains(&child.kind()))
}

/// A call to the enclosing function itself: `name(..)`, `this.name(..)`,
/// `self.name(..)`, or `Self::name(..)`.
fn is_self_call(node: &AstNode, fn_name: &str) -> bool {
    if node.kind() == "method_invocation" {
        // Java: `name(..)` or `this.name(..)`.
        return node
            .child_by_field_name("object")
            .is_none_or(|object| object.text() == "this")
            && node
                .child_by_field_name("name")
                .is_some_and(|name| name.text() == fn_name);
    }
    let Some(callee) = node
        .child_by_field_name("function")
        .or_else(|| node.child(0))
    else {
        return false;
    };
    let text = callee.text();
    text == fn_name
        || ["this.", "self.", "Self::"]
            .iter()
            .any(|prefix| text.strip_prefix(prefix) == Some(fn_name))
}

/// NPath complexity: the number of acyclic execution paths (Nejmeh 1988),
/// matching PMD's `NPathComplexity`.
///
/// - Sequential statements multiply, except that paths which return, throw,
///   or raise leave the function: `if (a) return; if (b) return; return;`
///   has 3 paths, not 4. `break` and `continue` skip the rest of a loop or
///   switch body and rejoin after it; a labeled jump rejoins after its
///   labeled statement.
/// - Conditions have true and false outcomes (`a || b`: two true, one
///   false). An `if` consequence runs per true outcome, later alternatives
///   per false outcome; ternaries split the same way, and a boolean value
///   outside a condition has one path per operator plus one.
/// - Loops: the body per true outcome, plus the false outcomes that skip
///   it, unless the loop cannot be skipped (`while (true)`, `loop`, `for {}`).
/// - Switch/match: the sum of the case paths (see [`case_paths`]). Try: the
///   body's paths only; exception handlers are not normal-flow paths.
/// - Nested functions count as one path.
pub fn npath_complexity(node: &AstNode) -> u128 {
    let body = node
        .named_children()
        .find(|child| is_block_kind(BlockKind::from(child.kind())));
    match body {
        Some(body) => paths_of(&body).total(),
        // Flat-body or unusual grammars (Julia, Ruby)
        None => sequence_paths(node).total(),
    }
}
const IF_KINDS: &[&str] = &["if_statement", "if", "if_expression"];

const ELSE_KINDS: &[&str] = &["else_clause", "else", "elif_clause", "elif"];

const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "enhanced_for_statement",
    "foreach_statement",
    "do_while_statement",
    "for",
    "for_expression",
    "for_in_statement",
    "for_of_statement",
    "while_statement",
    "while",
    "while_expression",
    "do_statement",
    "loop_expression",
];

const CASE_KINDS: &[&str] = &[
    "case",
    "switch_block_statement_group",
    "switch_rule",
    "switch_case",
    "match_arm",
    "case_clause",
    "case_statement",
    "expression_case",
    "switch_section",
    "switch_label",
    "when_entry",
];

/// Case groups whose statements fall through into the next group (Java,
/// JavaScript, C).
const FALLTHROUGH_CASE_KINDS: &[&str] = &[
    "switch_block_statement_group",
    "switch_case",
    "switch_default",
    "case_statement",
];

const TRY_KINDS: &[&str] = &["try_statement", "try", "try_expression"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Block,
    StatementBlock,
    CompoundStatement,
    BodyStatement,
    Other,
}

impl From<&str> for BlockKind {
    fn from(s: &str) -> Self {
        match s {
            "block" => Self::Block,
            "statement_block" => Self::StatementBlock,
            "compound_statement" => Self::CompoundStatement,
            "body_statement" => Self::BodyStatement,
            // Kotlin: function bodies, blocks, and branch and loop bodies.
            "function_body" | "statements" | "control_structure_body" => Self::Block,
            _ => Self::Other,
        }
    }
}

fn is_block_kind(kind: BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::Block
            | BlockKind::StatementBlock
            | BlockKind::CompoundStatement
            | BlockKind::BodyStatement
    )
}

const JUMP_KINDS: &[&str] = &[
    "break_statement",
    "break_expression",
    "continue_statement",
    "continue_expression",
];

const EXIT_KINDS: &[&str] = &[
    "return_statement",
    "return_expression",
    "throw_statement",
    "throw_expression",
    "raise_statement",
];

/// Paths through a statement: those that fall through to the next one,
/// those that leave the function, those that jump (`break`, `continue`)
/// past the rest of the enclosing loop or switch body, and labeled jumps,
/// which pass enclosing loops until their labeled statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Paths {
    fall: u128,
    exit: u128,
    jump: u128,
    labeled: u128,
}

impl Paths {
    const STRAIGHT: Self = Self::falling(1);
    const NONE: Self = Self::falling(0);
    const JUMP: Self = Self {
        jump: 1,
        ..Self::NONE
    };
    const LABELED_JUMP: Self = Self {
        labeled: 1,
        ..Self::NONE
    };

    const fn falling(fall: u128) -> Self {
        Self {
            fall,
            exit: 0,
            jump: 0,
            labeled: 0,
        }
    }

    fn total(self) -> u128 {
        self.fall
            .saturating_add(self.exit)
            .saturating_add(self.jump)
            .saturating_add(self.labeled)
    }

    /// `self` followed by `next`: only fall-through paths reach `next`.
    fn then(self, next: Self) -> Self {
        Self {
            fall: self.fall.saturating_mul(next.fall),
            exit: self
                .exit
                .saturating_add(self.fall.saturating_mul(next.exit)),
            jump: self
                .jump
                .saturating_add(self.fall.saturating_mul(next.jump)),
            labeled: self
                .labeled
                .saturating_add(self.fall.saturating_mul(next.labeled)),
        }
    }

    /// Alternatives: their paths add.
    fn or(self, other: Self) -> Self {
        Self {
            fall: self.fall.saturating_add(other.fall),
            exit: self.exit.saturating_add(other.exit),
            jump: self.jump.saturating_add(other.jump),
            labeled: self.labeled.saturating_add(other.labeled),
        }
    }

    /// Each path repeated for `count` entries (one per case label).
    fn times(self, count: u128) -> Self {
        Self {
            fall: self.fall.saturating_mul(count),
            exit: self.exit.saturating_mul(count),
            jump: self.jump.saturating_mul(count),
            labeled: self.labeled.saturating_mul(count),
        }
    }

    fn with_fall(self, extra: u128) -> Self {
        self.or(Self::falling(extra))
    }

    /// At the end of a loop or switch, jumps rejoin the fall-through paths.
    fn close_jumps(self) -> Self {
        Self {
            fall: self.fall.saturating_add(self.jump),
            jump: 0,
            ..self
        }
    }

    /// At a labeled statement, its labeled jumps rejoin the fall-through
    /// paths. Labels are not matched by name, so a jump to an outer label
    /// also closes at an inner labeled statement (rare in practice).
    fn close_labeled(self) -> Self {
        Self {
            fall: self.fall.saturating_add(self.labeled),
            labeled: 0,
            ..self
        }
    }
}

fn sequence_paths(node: &AstNode) -> Paths {
    node.children()
        .fold(Paths::STRAIGHT, |paths, child| paths.then(paths_of(&child)))
}

fn paths_of(node: &AstNode) -> Paths {
    let kind = node.kind();
    if crate::function_node_kinds().contains(&kind) || LAMBDA_KINDS.contains(&kind) {
        return Paths::STRAIGHT;
    }
    transfer_paths(node, kind)
        .or_else(|| compound_paths(node, kind))
        .unwrap_or_else(|| sequence_paths(node))
}

/// Paths through a statement that leaves its block: a return, throw, break,
/// or continue, and the label such a jump may target.
fn transfer_paths(node: &AstNode, kind: &str) -> Option<Paths> {
    if kind == "jump_expression" {
        Some(kotlin_jump_paths(node))
    } else if EXIT_KINDS.contains(&kind) {
        // Paths inside the returned expression all leave the function.
        Some(Paths {
            exit: sequence_paths(node).total(),
            ..Paths::NONE
        })
    } else if JUMP_KINDS.contains(&kind) {
        Some(jump_paths(node))
    } else if kind == "labeled_statement" {
        Some(sequence_paths(node).close_labeled())
    } else {
        None
    }
}

/// Kotlin spells `return`, `throw`, `break`, and `continue` as jumps.
fn kotlin_jump_paths(node: &AstNode) -> Paths {
    let text = node.text().trim_start();
    if text.starts_with("return") || text.starts_with("throw") {
        Paths {
            exit: sequence_paths(node).total(),
            ..Paths::NONE
        }
    } else if text.contains('@') {
        Paths::LABELED_JUMP
    } else {
        Paths::JUMP
    }
}

/// Paths through a branching construct: conditionals, loops, switches,
/// `try`, and short-circuit operators.
fn compound_paths(node: &AstNode, kind: &str) -> Option<Paths> {
    if IF_KINDS.contains(&kind) {
        Some(if_paths(node))
    } else if LOOP_KINDS.contains(&kind) {
        Some(loop_paths(node))
    } else if FLOW_CONTAINER_KINDS.contains(&kind) {
        Some(switch_paths(node))
    } else if TRY_KINDS.contains(&kind) {
        // Exception handlers are not normal-flow paths (PMD counts only the body).
        Some(block_paths(node))
    } else if matches!(kind, "conditional_expression" | "ternary_expression") {
        Some(ternary_paths(node))
    } else if logical_operator(node).is_some() {
        // A boolean value outside a condition (`x = a || b`, `return a && b`):
        // one path per short-circuit operator, plus one (PMD).
        Some(Paths::falling(logical_operators(node).saturating_add(1)))
    } else {
        None
    }
}

fn jump_paths(node: &AstNode) -> Paths {
    if is_labeled_jump(node) {
        Paths::LABELED_JUMP
    } else {
        Paths::JUMP
    }
}

/// Paths of a node's first block child (a loop or `try` body).
fn block_paths(node: &AstNode) -> Paths {
    node.named_children()
        .find(|child| is_block_kind(BlockKind::from(child.kind())))
        .map_or(Paths::STRAIGHT, |body| paths_of(&body))
}

/// The body runs once per true outcome of the condition; false ones skip it.
fn loop_paths(node: &AstNode) -> Paths {
    let (enter, skip) = if is_unconditional_loop(node) {
        (1, 0)
    } else {
        node.child_by_field_name("condition")
            .map_or((1, 1), |condition| condition_outcomes(&condition))
    };
    let paths = block_paths(node).close_jumps().times(enter).with_fall(skip);
    // A Rust loop carries its own label (`'outer: loop`).
    if node.named_children().any(|child| child.kind() == "label") {
        paths.close_labeled()
    } else {
        paths
    }
}

fn switch_paths(node: &AstNode) -> Paths {
    let cases = case_paths(node).close_jumps();
    if cases.total() == 0 {
        Paths::STRAIGHT
    } else {
        cases
    }
}

/// `c ? a : b` (Python `a if c else b`): `a` once per true outcome of the
/// condition and `b` once per false outcome.
fn ternary_paths(node: &AstNode) -> Paths {
    let part = |field: &str, python_index: usize| {
        node.child_by_field_name(field)
            .or_else(|| node.named_child(python_index))
    };
    let (when_true, when_false) =
        part("condition", 1).map_or((1, 1), |condition| condition_outcomes(&condition));
    let branch = |field: &str, python_index: usize| {
        part(field, python_index).map_or(Paths::STRAIGHT, |branch| paths_of(&branch))
    };
    branch("consequence", 0)
        .times(when_true)
        .or(branch("alternative", 2).times(when_false))
}

/// Sum of the case bodies' paths; some grammars wrap cases in a body node.
///
/// For Java, JavaScript and C this follows PMD: each case label is a path
/// into its group (`case 1, 2 ->` is two), a group's fall-through paths enter
/// the next group, and a switch without `default` adds the path that matches
/// no case. Other languages' arms (Go, Python, Rust) add one path each.
fn case_paths(node: &AstNode) -> Paths {
    let mut total = Paths::NONE;
    let mut carry = 0;
    let mut labeled_cases = false;
    let mut has_default = false;
    for child in node.named_children() {
        let kind = child.kind();
        if FALLTHROUGH_CASE_KINDS.contains(&kind) || kind == "switch_rule" {
            let (paths, default, next_carry) = labeled_case_paths(&child, carry);
            labeled_cases = true;
            has_default |= default;
            carry = next_carry;
            total = total.or(paths);
        } else if CASE_KINDS.contains(&kind) {
            total = total.or(sequence_paths(&child));
        } else {
            total = total.or(case_paths(&child));
        }
    }
    total = total.with_fall(carry);
    if labeled_cases && !has_default {
        total = total.with_fall(1);
    }
    total
}

/// A labeled case group's paths, whether it holds `default`, and the paths
/// that fall through into the next group (entering it with `carry` more).
fn labeled_case_paths(case: &AstNode, carry: u128) -> (Paths, bool, u128) {
    let (labels, default) = case_labels(case);
    let paths = sequence_paths(case).times(labels + carry);
    if FALLTHROUGH_CASE_KINDS.contains(&case.kind()) {
        (Paths { fall: 0, ..paths }, default, paths.fall)
    } else {
        (paths, default, 0)
    }
}

/// (Number of labels, whether one is `default`) for a Java, JavaScript, or C
/// case group.
fn case_labels(case: &AstNode) -> (u128, bool) {
    let is_default = |node: &AstNode| node.text().trim_start().starts_with("default");
    match case.kind() {
        "switch_case" => (1, false),
        "switch_default" => (1, true),
        "case_statement" => (1, is_default(case)),
        _ => case
            .named_children()
            .filter(|child| child.kind() == "switch_label")
            .fold((0, false), |(count, default), label| {
                let alternatives = label
                    .named_children()
                    .filter(|value| value.kind() != "guard")
                    .count()
                    .max(1);
                (count + alternatives as u128, default || is_default(&label))
            }),
    }
}

/// Branches of an `if`: the consequence, then `elif` clauses, an `else if`
/// (a nested `if`, directly or inside an `else` clause), or a final `else`
/// block. Without a final `else`, the false condition falls through. As in
/// PMD, the consequence runs once per true outcome of the condition and each
/// later alternative once per false outcome of every condition before it.
fn if_paths(node: &AstNode) -> Paths {
    let condition = node.child_by_field_name("condition");
    let (when_true, when_false) =
        condition.map_or((1, 1), |condition| condition_outcomes(&condition));
    let mut branches = Paths::NONE;
    let mut pending = when_false;
    let mut seen_consequence = false;
    let mut closed = false;
    for child in node.named_children() {
        if condition.is_some_and(|condition| condition.start_byte() == child.start_byte()) {
            continue;
        }
        let child_kind = child.kind();
        if (is_block_kind(BlockKind::from(child_kind)) || !matches_branch_kind(child_kind))
            && !seen_consequence
        {
            // The consequence, braced or not (`if (a) return;` in Java, JS, C).
            branches = branches.or(paths_of(&child).times(when_true));
            seen_consequence = true;
        } else if IF_KINDS.contains(&child_kind) {
            branches = branches.or(if_paths(&child).times(pending));
            closed = true;
        } else if matches!(child_kind, "elif_clause" | "elif") {
            let (paths, still_pending) = elif_paths(&child, pending);
            branches = branches.or(paths);
            pending = still_pending;
        } else if ELSE_KINDS.contains(&child_kind) {
            branches = branches.or(else_paths(&child, pending));
            closed = true;
        } else {
            // A second block (a bare `else` in Java and Go) or a brace-less `else`.
            branches = branches.or(paths_of(&child).times(pending));
            closed = true;
        }
    }
    if closed {
        branches
    } else {
        branches.with_fall(pending)
    }
}

/// An `elif` clause's paths, entered per `pending` false outcome, and the
/// false outcomes still pending after its own condition.
fn elif_paths(clause: &AstNode, pending: u128) -> (Paths, u128) {
    let (when_true, when_false) = clause
        .child_by_field_name("condition")
        .map_or((1, 1), |condition| condition_outcomes(&condition));
    let block = clause
        .named_children()
        .find(|child| is_block_kind(BlockKind::from(child.kind())));
    let paths = block.map_or(Paths::STRAIGHT, |block| paths_of(&block));
    (
        paths.times(pending.saturating_mul(when_true)),
        pending.saturating_mul(when_false),
    )
}

/// An `else` clause's paths: a nested `else if` or a block.
fn else_paths(clause: &AstNode, pending: u128) -> Paths {
    clause
        .named_children()
        .filter_map(|inner| {
            if IF_KINDS.contains(&inner.kind()) {
                Some(if_paths(&inner))
            } else if is_block_kind(BlockKind::from(inner.kind())) {
                Some(paths_of(&inner))
            } else {
                None
            }
        })
        .fold(Paths::NONE, |branches, paths| {
            branches.or(paths.times(pending))
        })
}

fn matches_branch_kind(kind: &str) -> bool {
    IF_KINDS.contains(&kind) || ELSE_KINDS.contains(&kind) || matches!(kind, "elif_clause" | "elif")
}

/// A loop whose condition cannot be false on entry: Rust `loop`, Go `for {}`,
/// or a literal `true` condition.
fn is_unconditional_loop(node: &AstNode) -> bool {
    if node.kind() == "loop_expression" {
        return true;
    }
    match node.child_by_field_name("condition") {
        Some(condition) => matches!(
            condition
                .text()
                .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace()),
            "true" | "True"
        ),
        // Go's `for { .. }`: a body and nothing else.
        None => node.kind() == "for_statement" && node.named_child_count() == 1,
    }
}

fn logical_operators(node: &AstNode) -> u128 {
    node.named_children().fold(
        u128::from(logical_operator(node).is_some()),
        |total, child| total.saturating_add(logical_operators(&child)),
    )
}

/// (true, false) outcome paths of a condition, as PMD counts them: each
/// `&&`/`||` short-circuits, so `a || b` is true two ways and false one way,
/// and `!` swaps the outcomes.
fn condition_outcomes(node: &AstNode) -> (u128, u128) {
    if let Some(operator) = logical_operator(node) {
        let operands: Vec<_> = node.named_children().collect();
        let side = |field: &str, fallback: Option<&AstNode>| {
            node.child_by_field_name(field)
                .or_else(|| fallback.copied())
                .map_or((1, 1), |operand| condition_outcomes(&operand))
        };
        let (left_true, left_false) = side("left", operands.first());
        let (right_true, right_false) = side("right", operands.last());
        return if matches!(operator.as_str(), "&&" | "and") {
            (
                left_true.saturating_mul(right_true),
                left_false.saturating_add(left_true.saturating_mul(right_false)),
            )
        } else {
            (
                left_true.saturating_add(left_false.saturating_mul(right_true)),
                left_false.saturating_mul(right_false),
            )
        };
    }
    let text = node.text().trim_start();
    match node.kind() {
        "parenthesized_expression" | "condition_clause" if node.named_child_count() == 1 => node
            .named_child(0)
            .map_or((1, 1), |inner| condition_outcomes(&inner)),
        "unary_expression" | "not_operator" if text.starts_with('!') || text.starts_with("not") => {
            let operand = node.named_children().last();
            let (when_true, when_false) =
                operand.map_or((1, 1), |operand| condition_outcomes(&operand));
            (when_false, when_true)
        }
        _ => (1, 1),
    }
}

/// Compute all complexity metrics for a function node using a single combined
/// traversal for the four additive metrics (branch_count, cognitive, nesting_depth,
/// halstead) and one separate traversal for npath (multiplicative semantics).
pub fn function_complexity(node: &AstNode, fn_name: &str) -> FunctionComplexity {
    let mut m = CombinedMetrics {
        branch_count: 0usize,
        cognitive: 0usize,
        nesting_depth: 0usize,
        halstead_op_set: std::collections::BTreeSet::new(),
        halstead_od_set: std::collections::BTreeSet::new(),
        halstead_total_ops: 0usize,
        halstead_total_operands: 0usize,
        breakdown: CognitiveBreakdown::default(),
    };
    let mut nesting = 0usize;
    let halstead_body = halstead_body(node);
    let halstead_range = (halstead_body.start_byte(), halstead_body.end_byte());
    walk_combined(
        node,
        fn_name,
        &mut m,
        &mut nesting,
        true,
        true,
        halstead_range,
    );
    m.breakdown.score = m.cognitive;
    FunctionComplexity {
        branch_count: m.branch_count,
        cognitive: m.cognitive,
        npath: npath_complexity(node),
        halstead: HalsteadMetrics {
            unique_operators: m.halstead_op_set.len(),
            unique_operands: m.halstead_od_set.len(),
            total_operators: m.halstead_total_ops,
            total_operands: m.halstead_total_operands,
        },
        nesting_depth: m.nesting_depth,
        cognitive_breakdown: m.breakdown,
    }
}

struct CombinedMetrics {
    branch_count: usize,
    cognitive: usize,
    nesting_depth: usize,
    halstead_op_set: std::collections::BTreeSet<String>,
    halstead_od_set: std::collections::BTreeSet<String>,
    halstead_total_ops: usize,
    halstead_total_operands: usize,
    breakdown: CognitiveBreakdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CognitiveNodeKind {
    LogicalSequence,
    Switch,
    If { else_if: bool, bare_else: bool },
    Nesting,
    Else,
    Conditional,
    LabeledJump,
    SelfCall,
    None,
}

fn classify_cognitive_node(node: &AstNode, kind: &str, fn_name: &str) -> CognitiveNodeKind {
    if !node.is_named() {
        CognitiveNodeKind::None
    } else if FLOW_CONTAINER_KINDS.contains(&kind) {
        CognitiveNodeKind::Switch
    } else if IF_KINDS.contains(&kind) {
        CognitiveNodeKind::If {
            else_if: is_else_if(node),
            bare_else: node
                .child_by_field_name("alternative")
                .is_some_and(|alternative| {
                    !IF_KINDS.contains(&alternative.kind())
                        && !FLOW_FLAT_KINDS.contains(&alternative.kind())
                }),
        }
    } else if FLOW_NESTING_KINDS.contains(&kind) {
        CognitiveNodeKind::Nesting
    } else if FLOW_FLAT_KINDS.contains(&kind) {
        // An `else` wrapping an `if` is an `else if`, which the `if` counts.
        if node
            .children()
            .any(|child| IF_KINDS.contains(&child.kind()))
        {
            CognitiveNodeKind::None
        } else {
            CognitiveNodeKind::Else
        }
    } else if matches!(kind, "conditional_expression" | "ternary_expression") {
        CognitiveNodeKind::Conditional
    } else if starts_logical_sequence(node) {
        CognitiveNodeKind::LogicalSequence
    } else if JUMP_KINDS.contains(&kind) && is_labeled_jump(node) {
        CognitiveNodeKind::LabeledJump
    } else if matches!(kind, "call_expression" | "method_invocation" | "call")
        && is_self_call(node, fn_name)
    {
        CognitiveNodeKind::SelfCall
    } else {
        CognitiveNodeKind::None
    }
}

/// An `if` that is the alternative of another `if`: inside a named `else`
/// clause (Rust, JS, C, Python) or the `alternative` field itself (Java, Go).
fn is_else_if(node: &AstNode) -> bool {
    node.parent().is_some_and(|parent| {
        FLOW_FLAT_KINDS.contains(&parent.kind())
            || (IF_KINDS.contains(&parent.kind())
                && parent
                    .child_by_field_name("alternative")
                    .is_some_and(|alternative| alternative.start_byte() == node.start_byte()))
    })
}

fn logical_operator(node: &AstNode) -> Option<String> {
    match node.kind() {
        "conjunction_expression" => Some("&&".to_string()),
        "disjunction_expression" => Some("||".to_string()),
        "binary_expression" | "boolean_operator" | "binary" => node
            .child_by_field_name("operator")
            .map(|operator| operator.text().to_string())
            .filter(|operator| BOOL_OP_KINDS.contains(&operator.as_str())),
        _ => None,
    }
}

/// A boolean operator whose operand position is not already inside a run of
/// the same operator, looking through parentheses.
fn starts_logical_sequence(node: &AstNode) -> bool {
    let Some(operator) = logical_operator(node) else {
        return false;
    };
    let mut parent = node.parent();
    while let Some(wrapper) = parent.filter(|p| p.kind() == "parenthesized_expression") {
        parent = wrapper.parent();
    }
    parent.and_then(|p| logical_operator(&p)) != Some(operator)
}

/// Adds `1 + nesting`; returns true when the node nests its children.
fn add_structural(metrics: &mut CombinedMetrics, nesting: &mut usize) -> bool {
    metrics.cognitive += 1 + *nesting;
    metrics.breakdown.structural += 1;
    metrics.breakdown.nesting_increments += *nesting;
    *nesting += 1;
    true
}

fn add_flat(metrics: &mut CombinedMetrics, hybrid: bool) {
    metrics.cognitive += 1;
    if hybrid {
        metrics.breakdown.hybrid += 1;
    } else {
        metrics.breakdown.fundamental += 1;
    }
}

fn update_cognitive_metrics(
    node_kind: CognitiveNodeKind,
    metrics: &mut CombinedMetrics,
    nesting: &mut usize,
) -> bool {
    match node_kind {
        CognitiveNodeKind::Switch => {
            metrics.nesting_depth = metrics.nesting_depth.max(*nesting + 1);
            return add_structural(metrics, nesting);
        }
        CognitiveNodeKind::If { else_if, bare_else } => {
            if bare_else {
                add_flat(metrics, true);
            }
            if else_if {
                add_flat(metrics, true);
            } else {
                return add_structural(metrics, nesting);
            }
        }
        CognitiveNodeKind::Nesting | CognitiveNodeKind::Conditional => {
            return add_structural(metrics, nesting);
        }
        CognitiveNodeKind::Else => add_flat(metrics, true),
        CognitiveNodeKind::LogicalSequence => add_flat(metrics, false),
        CognitiveNodeKind::LabeledJump => add_flat(metrics, false),
        // The whitepaper adds 1 per method in a recursion cycle, not per call.
        CognitiveNodeKind::SelfCall if !metrics.breakdown.recursion => {
            add_flat(metrics, false);
            metrics.breakdown.recursion = true;
        }
        CognitiveNodeKind::SelfCall => {}
        CognitiveNodeKind::None => {}
    }
    false
}

/// Counts a decision point and the nesting depth it reaches: arms sit at
/// their container's depth, and an `else if` at its first `if`'s.
fn record_branch(node: &AstNode, kind: &str, m: &mut CombinedMetrics, nesting: usize) {
    let compensating = FLOW_COMPENSATING_KINDS.contains(&kind);
    if !node.is_named() || !(compensating || BRANCH_KINDS.contains(&kind)) {
        return;
    }
    m.branch_count += 1;
    let depth = if compensating || (IF_KINDS.contains(&kind) && is_else_if(node)) {
        nesting.max(1)
    } else {
        nesting + 1
    };
    m.nesting_depth = m.nesting_depth.max(depth);
}

fn walk_combined(
    node: &AstNode,
    fn_name: &str,
    m: &mut CombinedMetrics,
    nesting: &mut usize,
    is_root: bool,
    collect_halstead: bool,
    halstead_range: (usize, usize),
) {
    let kind = node.kind();

    if !is_root && (crate::function_node_kinds().contains(&kind) || LAMBDA_KINDS.contains(&kind)) {
        return;
    }
    let collect_halstead =
        collect_halstead && (is_root || !is_halstead_function_node(node.language(), kind));

    record_branch(node, kind, m, *nesting);
    let halstead_leaf = is_halstead_leaf(node);
    if collect_halstead
        && halstead_leaf
        && node.start_byte() >= halstead_range.0
        && node.end_byte() <= halstead_range.1
    {
        record_halstead_leaf(
            node,
            &mut m.halstead_op_set,
            &mut m.halstead_od_set,
            &mut m.halstead_total_ops,
            &mut m.halstead_total_operands,
        );
    }

    let cognitive_kind = classify_cognitive_node(node, kind, fn_name);
    let incr_nesting = update_cognitive_metrics(cognitive_kind, m, nesting);

    for child in node.children() {
        walk_combined(
            &child,
            fn_name,
            m,
            nesting,
            false,
            collect_halstead && !halstead_leaf,
            halstead_range,
        );
    }

    if incr_nesting {
        *nesting -= 1;
    }
}

#[derive(Debug, Clone)]
pub struct FunctionComplexity {
    pub branch_count: usize,
    pub cognitive: usize,
    pub npath: u128,
    pub halstead: HalsteadMetrics,
    pub nesting_depth: usize,
    pub cognitive_breakdown: CognitiveBreakdown,
}

#[cfg(test)]
#[path = "complexity_tests.rs"]
mod tests;
