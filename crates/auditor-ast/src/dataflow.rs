//! Def-use flow within one function, read from the syntax tree. Findings-free.
//!
//! A [`FunctionFlow`] holds a named function's bindings (parameters,
//! declarations, assignments, loop variables) and the identifier reads in
//! its body, including the bodies of the anonymous functions it contains.
//! It answers three questions the same way for every language:
//!
//! - which bindings may reach a read ([`FunctionFlow::reaching`]): those
//!   before it in source order that no later unconditional assignment
//!   replaces, and in a loop, those after it that the next iteration sees;
//! - where a value may come from ([`FunctionFlow::origins`]): reads are
//!   followed to the values bound to them, through concatenation,
//!   interpolation, conditional choices, and member access, until a literal,
//!   a parameter, a call, or a name bound outside the function;
//! - where a value may go ([`FunctionFlow::destinations`]): the names it is
//!   bound to, the calls it is passed to, the properties it is stored under,
//!   and the function's return.
//!
//! How a call passes values through is not knowable from syntax, so callers
//! say it with a [`CallFlow`] policy. The analysis is a may-analysis without
//! types: no aliasing, no flow through fields, and no flow between functions;
//! callers carry that through call summaries.

use crate::{AstLang, AstNode};

#[path = "dataflow_bindings.rs"]
mod bindings;
#[path = "dataflow_shapes.rs"]
mod shapes;
#[path = "dataflow_values.rs"]
mod values;

pub use values::{call_arguments, call_receiver};

/// Functions with a name of their own, each a flow scope.
const NAMED_FUNCTION_KINDS: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "function_definition",
    "function_item",
    "method_declaration",
    "method_definition",
    "constructor_declaration",
    "local_function_statement",
];

/// Anonymous functions, which share the scope of the function around them.
const ANONYMOUS_FUNCTION_KINDS: &[&str] = &[
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
    "lambda",
    "lambda_expression",
    "lambda_literal",
    "anonymous_function",
    "anonymous_method_expression",
    "func_literal",
    "closure_expression",
    "arrow_function_expression",
];

/// Ruby's named methods and its blocks, whose kinds other grammars use for
/// statement blocks.
const RUBY_NAMED_FUNCTION_KINDS: &[&str] = &["method", "singleton_method"];
const RUBY_ANONYMOUS_FUNCTION_KINDS: &[&str] = &["block", "do_block"];

/// A function with a name of its own, which is a flow scope.
pub(super) fn is_named_function(node: AstNode<'_>) -> bool {
    NAMED_FUNCTION_KINDS.contains(&node.kind())
        || node.language() == AstLang::Ruby && RUBY_NAMED_FUNCTION_KINDS.contains(&node.kind())
}

/// An anonymous function, which shares the scope of the function around it.
pub(super) fn is_anonymous_function(node: AstNode<'_>) -> bool {
    ANONYMOUS_FUNCTION_KINDS.contains(&node.kind())
        || node.language() == AstLang::Ruby && RUBY_ANONYMOUS_FUNCTION_KINDS.contains(&node.kind())
}

/// Statements that run their body zero or more times.
const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "for_in_statement",
    "for_of_statement",
    "enhanced_for_statement",
    "foreach_statement",
    "for_range_loop",
    "while_statement",
    "do_statement",
    "do_while_statement",
    "loop_expression",
    "while_expression",
    "for_expression",
    // Ruby.
    "for",
    "while",
    "until",
];

/// Nodes whose parts may not run: branches, loops, handlers, short-circuit
/// operators, and anonymous functions.
const CONDITIONAL_KINDS: &[&str] = &[
    "if_statement",
    "if_expression",
    "if_let_expression",
    "else_clause",
    "elif_clause",
    "conditional_expression",
    "ternary_expression",
    "switch_statement",
    "switch_expression",
    "switch_case",
    "switch_section",
    "switch_block_statement_group",
    "case_clause",
    "expression_case",
    "type_case",
    "default_case",
    "communication_case",
    "select_statement",
    "match_expression",
    "match_arm",
    "match_statement",
    "when_expression",
    "when_entry",
    "try_statement",
    "try_expression",
    "catch_clause",
    "except_clause",
    "finally_clause",
    "boolean_operator",
];

/// How a binding gives its name a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    /// A function or catch parameter; its value comes from the caller.
    Parameter,
    /// `const x = v`, `x := v`, `int x = v`.
    Declaration,
    /// `x = v`.
    Assignment,
    /// `x += v`, `x++`: the old value flows on with the new one.
    Augmented,
    /// `for (x of xs)`: each element of the iterable.
    LoopVariable,
}

/// A name given a value at one place.
#[derive(Clone, Copy, Debug)]
pub struct Binding<'a> {
    pub name: &'a str,
    /// The identifier that names the binding.
    pub target: AstNode<'a>,
    /// The value bound: the assigned expression, the iterable of a loop
    /// variable, or a parameter's default.
    pub value: Option<AstNode<'a>>,
    /// The declaring node: the declarator, assignment, parameter, or loop.
    pub node: AstNode<'a>,
    pub kind: BindingKind,
    /// Where the name is visible: the scope, an anonymous function for its
    /// parameters, or a loop for its variable.
    region: AstNode<'a>,
    /// The byte from which the binding holds.
    point: usize,
}

impl PartialEq for Binding<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
    }
}

/// How a call's result relates to its receiver and arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallFlow {
    /// The result is a new value: neither receiver nor arguments flow into it.
    Opaque,
    /// The result derives from the receiver (`s.trim()`, `args.get("q")`).
    Receiver,
    /// The result derives from the receiver and every argument.
    All,
}

/// A place a value may come from.
#[derive(Clone, Copy, Debug)]
pub enum Origin<'a> {
    Literal(AstNode<'a>),
    Parameter(Binding<'a>),
    /// A call result the policy does not see through.
    Call(AstNode<'a>),
    /// A member or element read (`req.query.table`, `row[0]`); its object's
    /// origins are reported too.
    Member(AstNode<'a>),
    /// A name with no binding in the function: a module-level name, an
    /// import, a field, or a global.
    Unbound(AstNode<'a>),
    /// Any other expression (`new T()`, a lambda).
    Other(AstNode<'a>),
}

/// A place a value may go.
#[derive(Clone, Copy, Debug)]
pub enum Destination<'a> {
    /// Bound to a name; the reads it reaches are followed too.
    Bound(Binding<'a>),
    /// Passed to a call the policy does not see through.
    Argument { call: AstNode<'a>, index: usize },
    /// The receiver of a call the policy does not see through.
    Receiver(AstNode<'a>),
    /// Stored under a key (`{ token: v }`, `nonce=v`) or in a member
    /// (`req.session.token = v`); `key` is the key or the member written.
    Property { key: AstNode<'a> },
    /// Returned from `function`, a named or anonymous function.
    Returned { function: AstNode<'a> },
}

/// One of the expressions a name's value may be made of.
#[derive(Clone, Copy, Debug)]
pub enum Assigned<'a> {
    /// The whole value: `v` in `x = v`.
    Whole(AstNode<'a>),
    /// A part added to the old value: `v` in `x += v`.
    Appended(AstNode<'a>),
    /// A value from outside the function's bindings: the read of a
    /// parameter, a loop variable, or a name bound elsewhere.
    Outside(AstNode<'a>),
}

impl<'a> Assigned<'a> {
    pub fn node(&self) -> AstNode<'a> {
        match self {
            Self::Whole(node) | Self::Appended(node) | Self::Outside(node) => *node,
        }
    }
}

/// The bindings and reads of one named function, or of a file's top level.
pub struct FunctionFlow<'a> {
    scope: AstNode<'a>,
    bindings: Vec<Binding<'a>>,
}

impl<'a> FunctionFlow<'a> {
    /// The flow of the named function enclosing `node`, or of its file's top
    /// level outside any named function.
    pub fn enclosing(node: AstNode<'a>) -> Self {
        Self::of(enclosing_scope(node))
    }

    /// The flow of `scope`, a named function or a root node.
    pub fn of(scope: AstNode<'a>) -> Self {
        Self {
            scope,
            bindings: bindings::collect(scope),
        }
    }

    pub fn scope(&self) -> AstNode<'a> {
        self.scope
    }

    /// The scope's name, when it is a named function.
    pub fn function_name(&self) -> Option<&'a str> {
        is_named_function(self.scope).then(|| shapes::function_name(self.scope))?
    }

    pub fn bindings(&self) -> &[Binding<'a>] {
        &self.bindings
    }

    /// The bindings that may give the read `read` its value.
    pub fn reaching(&self, read: AstNode<'a>) -> Vec<Binding<'a>> {
        let candidates = self.visible(read);
        let before = candidates
            .iter()
            .filter(|binding| binding.point <= read.start_byte())
            .copied()
            .collect::<Vec<_>>();
        let mut reaching = before
            .iter()
            .filter(|binding| !self.replaced(binding, &before, read))
            .copied()
            .collect::<Vec<_>>();
        reaching.extend(
            candidates
                .iter()
                .filter(|binding| binding.point > read.start_byte() && in_same_loop(binding, read))
                .copied(),
        );
        reaching
    }

    /// The reads `binding` may reach.
    pub fn reads_of(&self, binding: &Binding<'a>) -> Vec<AstNode<'a>> {
        bindings::reads(self.scope, binding.name)
            .into_iter()
            .filter(|read| self.reaching(*read).contains(binding))
            .collect()
    }

    /// The expressions `expression`'s value may be made of: itself, or for a
    /// bare name, the values bound to it that reach it, following names bound
    /// to names and `x += v` to the values `x` held before. In source order.
    pub fn assigned(&self, expression: AstNode<'a>) -> Vec<Assigned<'a>> {
        let mut assigned = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![(expression, false)];
        while let Some((node, appended)) = pending.pop() {
            if !seen.insert((node, appended)) {
                continue;
            }
            if values::is_name(node) {
                pending.extend(self.bound_values(node, appended, &mut assigned));
            } else if appended {
                assigned.push(Assigned::Appended(node));
            } else {
                assigned.push(Assigned::Whole(node));
            }
        }
        assigned.sort_by_key(|part| part.node().start_byte());
        assigned.dedup_by_key(|part| part.node());
        assigned
    }

    /// The values bound to the name `read` that reach it, each marked as
    /// appended or not; a name bound outside the function goes to `assigned`.
    fn bound_values(
        &self,
        read: AstNode<'a>,
        appended: bool,
        assigned: &mut Vec<Assigned<'a>>,
    ) -> Vec<(AstNode<'a>, bool)> {
        let reaching = self.reaching(read);
        if reaching.is_empty() {
            assigned.push(Assigned::Outside(read));
        }
        let mut values = Vec::new();
        for binding in reaching {
            match (binding.kind, binding.value) {
                (BindingKind::Augmented, value) => {
                    values.push((binding.target, appended));
                    values.extend(value.map(|value| (value, true)));
                }
                (BindingKind::Declaration | BindingKind::Assignment, Some(value)) => {
                    values.push((value, appended));
                }
                _ => assigned.push(Assigned::Outside(read)),
            }
        }
        values
    }

    /// Where `expression`'s value may come from, seeing through calls as
    /// `policy` says.
    pub fn origins(
        &self,
        expression: AstNode<'a>,
        policy: &dyn Fn(AstNode<'a>) -> CallFlow,
    ) -> Vec<Origin<'a>> {
        let mut tracer = values::Tracer::new(self, policy);
        tracer.trace(expression);
        tracer.origins
    }

    /// Where `expression`'s value may go, through the calls `policy` sees
    /// through.
    pub fn destinations(
        &self,
        expression: AstNode<'a>,
        policy: &dyn Fn(AstNode<'a>) -> CallFlow,
    ) -> Vec<Destination<'a>> {
        let mut walker = values::Walker::new(self, policy);
        walker.walk(expression);
        walker.destinations
    }

    /// The bindings of `read`'s name visible at it, from the innermost
    /// region that binds the name (a lambda parameter shadows an outer
    /// variable).
    fn visible(&self, read: AstNode<'a>) -> Vec<Binding<'a>> {
        let name = read.text();
        let visible = self
            .bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && contains(binding.region, read)
                    && binding.target != read
                    && !evaluates_before_binding(binding, read)
            })
            .copied()
            .collect::<Vec<_>>();
        let innermost = visible
            .iter()
            .map(|binding| binding.region.start_byte())
            .max();
        visible
            .into_iter()
            .filter(|binding| Some(binding.region.start_byte()) == innermost)
            .collect()
    }

    /// Whether a later binding in `before` replaces `binding` on every path
    /// to `read`.
    fn replaced(&self, binding: &Binding<'a>, before: &[Binding<'a>], read: AstNode<'a>) -> bool {
        before.iter().any(|later| {
            later.point > binding.point
                && later.kind != BindingKind::Augmented
                && dominates(later.node, read)
        })
    }
}

/// The named function enclosing `node`, or its root.
fn enclosing_scope(node: AstNode<'_>) -> AstNode<'_> {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if is_named_function(parent) {
            return parent;
        }
        current = parent;
    }
    current
}

pub(super) fn contains(outer: AstNode<'_>, inner: AstNode<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// `x = x + 1`: the read in the value is evaluated before its own binding.
fn evaluates_before_binding(binding: &Binding<'_>, read: AstNode<'_>) -> bool {
    !matches!(
        binding.kind,
        BindingKind::Parameter | BindingKind::LoopVariable
    ) && binding.value.is_some_and(|value| contains(value, read))
}

/// Whether `binding` and `read` share a loop, so a binding after the read
/// reaches it on the next iteration.
fn in_same_loop(binding: &Binding<'_>, read: AstNode<'_>) -> bool {
    let Some(common) = common_ancestor(binding.node, read) else {
        return false;
    };
    std::iter::successors(Some(common), AstNode::parent)
        .take_while(|node| !is_named_function(*node))
        .any(|node| LOOP_KINDS.contains(&node.kind()))
}

/// Whether `node` runs whenever `read` does, `node` coming first: no
/// conditional part lies between them and their common ancestor.
fn dominates(node: AstNode<'_>, read: AstNode<'_>) -> bool {
    let Some(common) = common_ancestor(node, read) else {
        return false;
    };
    !is_conditional(common)
        && std::iter::successors(node.parent(), AstNode::parent)
            .take_while(|ancestor| *ancestor != common)
            .all(|ancestor| !is_conditional(ancestor))
}

fn is_conditional(node: AstNode<'_>) -> bool {
    CONDITIONAL_KINDS.contains(&node.kind())
        || LOOP_KINDS.contains(&node.kind())
        || is_anonymous_function(node)
        || node.kind() == "binary_expression"
            && node
                .child_by_field_name("operator")
                .is_some_and(|operator| matches!(operator.text(), "&&" | "||" | "??"))
}

fn common_ancestor<'a>(first: AstNode<'a>, second: AstNode<'a>) -> Option<AstNode<'a>> {
    std::iter::successors(Some(first), AstNode::parent).find(|ancestor| contains(*ancestor, second))
}

#[cfg(test)]
#[path = "dataflow_tests.rs"]
mod tests;
