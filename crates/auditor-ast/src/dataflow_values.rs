//! Where values come from and where they go within a flow scope.

use std::collections::HashSet;

use super::bindings::is_member;
use super::{
    contains, BindingKind, CallFlow, Destination, FunctionFlow, Origin, ANONYMOUS_FUNCTION_KINDS,
    NAMED_FUNCTION_KINDS,
};
use crate::AstNode;

const CALL_KINDS: &[&str] = &[
    "call_expression",
    "call",
    "method_invocation",
    "invocation_expression",
];

const LITERAL_KINDS: &[&str] = &[
    "number",
    "integer",
    "integer_literal",
    "decimal_integer_literal",
    "int_literal",
    "float",
    "float_literal",
    "real_literal",
    "true",
    "false",
    "null",
    "null_literal",
    "none",
    "nil",
    "undefined",
    "boolean_literal",
    "character_literal",
    "rune_literal",
];

const STRING_KINDS: &[&str] = &[
    "string",
    "string_literal",
    "template_string",
    "interpolated_string_expression",
    "interpreted_string_literal",
    "raw_string_literal",
    "verbatim_string_literal",
    "concatenated_string",
];

const INTERPOLATION_KINDS: &[&str] = &[
    "template_substitution",
    "interpolation",
    "interpolated_identifier",
    "interpolated_expression",
];

/// Expressions whose value is made of their operands' values.
const COMBINING_KINDS: &[&str] = &[
    "binary_expression",
    "binary_operator",
    "additive_expression",
    "multiplicative_expression",
    "parenthesized_expression",
    "await_expression",
    "await",
    "unary_expression",
    "unary_operator",
    "as_expression",
    "cast_expression",
    "type_assertion",
    "type_assertion_expression",
    "non_null_expression",
    "satisfies_expression",
    "spread_element",
    "list_splat",
    "array",
    "list",
    "tuple",
    "array_creation_expression",
    "array_initializer",
    "composite_literal",
    "literal_value",
    "elvis_expression",
    "conditional_expression",
    "ternary_expression",
];

/// The receiver of a method call: `s` in `s.trim()`, `request.args` in
/// `request.args.get("q")`.
pub fn call_receiver(call: AstNode<'_>) -> Option<AstNode<'_>> {
    if let Some(object) = call.child_by_field_name("object") {
        return Some(object);
    }
    let callee = call
        .child_by_field_name("function")
        .or_else(|| call.named_child(0))?;
    ["object", "operand", "expression", "value", "receiver"]
        .iter()
        .find_map(|field| callee.child_by_field_name(field))
        .or_else(|| {
            (callee.kind() == "navigation_expression")
                .then(|| callee.named_child(0))
                .flatten()
        })
}

/// A call's arguments in order, without wrappers: the value of a C#
/// `argument` or Kotlin `value_argument`, and a keyword argument whole.
pub fn call_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    let list = call.child_by_field_name("arguments").or_else(|| {
        call.named_children()
            .find(|child| child.kind() == "call_suffix")
            .and_then(|suffix| {
                suffix
                    .named_children()
                    .find(|child| child.kind() == "value_arguments")
            })
    });
    let Some(list) = list else {
        return Vec::new();
    };
    list.named_children()
        .filter(|argument| !argument.kind().ends_with("comment"))
        .map(|argument| match argument.kind() {
            "argument" | "value_argument" => argument.named_children().last().unwrap_or(argument),
            _ => argument,
        })
        .collect()
}

/// Follows values back to where they come from.
pub(super) struct Tracer<'f, 'a> {
    flow: &'f FunctionFlow<'a>,
    policy: &'f dyn Fn(AstNode<'a>) -> CallFlow,
    seen: HashSet<AstNode<'a>>,
    pub(super) origins: Vec<Origin<'a>>,
}

impl<'f, 'a> Tracer<'f, 'a> {
    pub(super) fn new(
        flow: &'f FunctionFlow<'a>,
        policy: &'f dyn Fn(AstNode<'a>) -> CallFlow,
    ) -> Self {
        Self {
            flow,
            policy,
            seen: HashSet::new(),
            origins: Vec::new(),
        }
    }

    pub(super) fn trace(&mut self, node: AstNode<'a>) {
        if !self.seen.insert(node) {
            return;
        }
        let kind = node.kind();
        if LITERAL_KINDS.contains(&kind) {
            self.origins.push(Origin::Literal(node));
        } else if STRING_KINDS.contains(&kind) {
            self.string(node);
        } else if matches!(
            kind,
            "identifier" | "simple_identifier" | "shorthand_property_identifier"
        ) {
            self.identifier(node);
        } else if CALL_KINDS.contains(&kind) {
            self.call(node);
        } else if is_member(node) {
            self.origins.push(Origin::Member(node));
            member_object(node)
                .into_iter()
                .for_each(|object| self.trace(object));
        } else if COMBINING_KINDS.contains(&kind) {
            let condition = node.child_by_field_name("condition");
            node.named_children()
                .filter(|child| Some(*child) != condition && !child.kind().contains("type"))
                .for_each(|child| self.trace(child));
        } else if matches!(kind, "argument" | "value_argument" | "keyword_argument") {
            let value = node
                .child_by_field_name("value")
                .or_else(|| node.named_children().last());
            value.into_iter().for_each(|value| self.trace(value));
        } else {
            self.origins.push(Origin::Other(node));
        }
    }

    /// A literal, or the values interpolated into it.
    fn string(&mut self, node: AstNode<'a>) {
        if node.kind() == "concatenated_string" {
            node.named_children().for_each(|part| self.trace(part));
            return;
        }
        let parts = interpolations(node);
        if parts.is_empty() {
            self.origins.push(Origin::Literal(node));
        }
        parts.into_iter().for_each(|part| self.trace(part));
    }

    fn call(&mut self, node: AstNode<'a>) {
        self.origins.push(Origin::Call(node));
        let flows = (self.policy)(node);
        if flows != CallFlow::Opaque {
            call_receiver(node)
                .into_iter()
                .for_each(|receiver| self.trace(receiver));
        }
        if flows == CallFlow::All {
            call_arguments(node)
                .into_iter()
                .for_each(|argument| self.trace(argument));
        }
    }

    fn identifier(&mut self, read: AstNode<'a>) {
        let reaching = self.flow.reaching(read);
        if reaching.is_empty() {
            self.origins.push(Origin::Unbound(read));
        }
        for binding in reaching {
            match (binding.kind, binding.value) {
                (BindingKind::Parameter, default) => {
                    self.origins.push(Origin::Parameter(binding));
                    default.into_iter().for_each(|value| self.trace(value));
                }
                (BindingKind::Augmented, value) => {
                    // The old value flows on: what reached the target before.
                    self.trace(binding.target);
                    value.into_iter().for_each(|value| self.trace(value));
                }
                (_, Some(value)) => self.trace(value),
                (_, None) => self.origins.push(Origin::Other(binding.node)),
            }
        }
    }
}

/// The interpolated expressions of a string, in order.
fn interpolations(node: AstNode<'_>) -> Vec<AstNode<'_>> {
    node.named_children()
        .filter(|part| INTERPOLATION_KINDS.contains(&part.kind()))
        .map(|part| {
            part.named_children()
                .find(|child| !child.kind().contains("brace") && !child.kind().contains("format"))
                .unwrap_or(part)
        })
        .collect()
}

/// The object of a member or element access: `req.query` in
/// `req.query.table`, `row` in `row[0]`.
fn member_object(node: AstNode<'_>) -> Option<AstNode<'_>> {
    ["object", "operand", "value", "expression", "argument"]
        .iter()
        .find_map(|field| node.child_by_field_name(field))
        .or_else(|| node.named_child(0))
}

/// Follows a value forward to where it goes.
pub(super) struct Walker<'f, 'a> {
    flow: &'f FunctionFlow<'a>,
    policy: &'f dyn Fn(AstNode<'a>) -> CallFlow,
    pending: Vec<AstNode<'a>>,
    pub(super) destinations: Vec<Destination<'a>>,
}

impl<'f, 'a> Walker<'f, 'a> {
    pub(super) fn new(
        flow: &'f FunctionFlow<'a>,
        policy: &'f dyn Fn(AstNode<'a>) -> CallFlow,
    ) -> Self {
        Self {
            flow,
            policy,
            pending: Vec::new(),
            destinations: Vec::new(),
        }
    }

    pub(super) fn walk(&mut self, start: AstNode<'a>) {
        let mut seen = HashSet::new();
        self.pending.push(start);
        while let Some(node) = self.pending.pop() {
            if !seen.insert(node) || !contains(self.flow.scope, node) {
                continue;
            }
            if let Some(parent) = node.parent() {
                self.step(node, parent);
            }
        }
    }

    /// One step up from `node`, whose value flows into `parent`.
    fn step(&mut self, node: AstNode<'a>, parent: AstNode<'a>) {
        if let Some(bound) = bound_by(self.flow, node) {
            self.bind(bound);
        } else if let Some((call, index)) = argument_position(node, parent) {
            self.call(call, CallFlow::All, Destination::Argument { call, index });
        } else if let Some(call) = receiver_of(node, parent) {
            self.call(call, CallFlow::Receiver, Destination::Receiver(call));
        } else if let Some(function) = returned_from(node, parent) {
            self.destinations.push(Destination::Returned { function });
            if ANONYMOUS_FUNCTION_KINDS.contains(&function.kind()) {
                self.pending.push(function);
            }
        } else if let Some(key) = property_key(node, parent) {
            self.destinations.push(Destination::Property { key });
            self.pending.push(parent);
        } else if carries(node, parent) || is_callee(node, parent) {
            self.pending.push(parent);
        }
    }

    fn bind(&mut self, bound: Vec<super::Binding<'a>>) {
        for binding in bound {
            self.destinations.push(Destination::Bound(binding));
            self.pending.extend(self.flow.reads_of(&binding));
        }
    }

    /// Passes the value through `call` when the policy carries the value's
    /// position (`needed`), or stops at `destination`.
    fn call(&mut self, call: AstNode<'a>, needed: CallFlow, destination: Destination<'a>) {
        let flows = (self.policy)(call);
        let carried = flows == CallFlow::All || flows == needed;
        if carried {
            self.pending.push(call);
        } else {
            self.destinations.push(destination);
        }
    }
}

/// The bindings `node`'s value is bound to, when it is a bound value.
fn bound_by<'a>(flow: &FunctionFlow<'a>, node: AstNode<'a>) -> Option<Vec<super::Binding<'a>>> {
    let bound = flow
        .bindings
        .iter()
        .filter(|binding| binding.kind != BindingKind::Parameter && binding.value == Some(node))
        .copied()
        .collect::<Vec<_>>();
    (!bound.is_empty()).then_some(bound)
}

/// The call and position of an argument: `node` is an argument itself, or
/// the value inside a C# or Kotlin argument wrapper.
fn argument_position<'a>(node: AstNode<'a>, parent: AstNode<'a>) -> Option<(AstNode<'a>, usize)> {
    let (argument, list) = if matches!(parent.kind(), "argument" | "value_argument") {
        (parent, parent.parent()?)
    } else {
        (node, parent)
    };
    if !matches!(
        list.kind(),
        "arguments" | "argument_list" | "value_arguments"
    ) {
        return None;
    }
    let call = std::iter::successors(list.parent(), AstNode::parent)
        .take(2)
        .find(|ancestor| {
            CALL_KINDS.contains(&ancestor.kind())
                || ancestor.kind() == "new_expression"
                || ancestor.kind() == "object_creation_expression"
        })?;
    let index = list
        .named_children()
        .filter(|child| !child.kind().ends_with("comment"))
        .position(|child| child == argument)?;
    Some((call, index))
}

/// The call `node` is the receiver of.
fn receiver_of<'a>(node: AstNode<'a>, parent: AstNode<'a>) -> Option<AstNode<'a>> {
    if CALL_KINDS.contains(&parent.kind()) && call_receiver(parent) == Some(node) {
        return Some(parent);
    }
    let call = parent.parent()?;
    (is_member(parent) && CALL_KINDS.contains(&call.kind()) && call_receiver(call) == Some(node))
        .then_some(call)
}

/// The function `node`'s value is returned from: by a `return`, or as an
/// arrow function's expression body.
fn returned_from<'a>(node: AstNode<'a>, parent: AstNode<'a>) -> Option<AstNode<'a>> {
    let returns = matches!(
        parent.kind(),
        "return_statement" | "return" | "return_expression" | "jump_expression"
    ) && (parent.kind() != "jump_expression" || parent.text().starts_with("return"));
    let body = ANONYMOUS_FUNCTION_KINDS.contains(&parent.kind())
        && parent.child_by_field_name("body") == Some(node)
        && !node.kind().contains("block");
    if body {
        return Some(parent);
    }
    if !returns {
        return None;
    }
    std::iter::successors(parent.parent(), AstNode::parent).find(|ancestor| {
        NAMED_FUNCTION_KINDS.contains(&ancestor.kind())
            || ANONYMOUS_FUNCTION_KINDS.contains(&ancestor.kind())
    })
}

/// The key `node` is stored under: an object pair's key, a keyword
/// argument's name, or the member an assignment writes (`req.session.id`).
fn property_key<'a>(node: AstNode<'a>, parent: AstNode<'a>) -> Option<AstNode<'a>> {
    match parent.kind() {
        "assignment"
        | "assignment_expression"
        | "assignment_statement"
        | "augmented_assignment"
        | "augmented_assignment_expression" => {
            let target = parent.child_by_field_name("left")?;
            (parent.child_by_field_name("right") == Some(node) && is_member(target))
                .then_some(target)
        }
        "pair" | "keyword_argument" | "named_argument" | "assignment_pattern" => {
            let key = parent
                .child_by_field_name("key")
                .or_else(|| parent.child_by_field_name("name"))?;
            (key != node).then_some(key)
        }
        _ => None,
    }
}

/// `(() => v)()`: a called function value's result is the call's value.
fn is_callee(node: AstNode<'_>, parent: AstNode<'_>) -> bool {
    CALL_KINDS.contains(&parent.kind()) && parent.child_by_field_name("function") == Some(node)
}

/// Whether `parent`'s value is made from `node`'s.
fn carries(node: AstNode<'_>, parent: AstNode<'_>) -> bool {
    let kind = parent.kind();
    if parent.child_by_field_name("condition") == Some(node) {
        return false;
    }
    COMBINING_KINDS.contains(&kind)
        || STRING_KINDS.contains(&kind)
        || INTERPOLATION_KINDS.contains(&kind)
        || is_member(parent)
        || matches!(
            kind,
            "object" | "dictionary" | "array" | "list" | "element_binding_expression"
        )
}
