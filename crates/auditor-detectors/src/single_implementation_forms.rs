//! The abstraction and implementation forms `single_implementation_abstraction`
//! reads in each language.

use auditor_ast::AstNode;

use super::{child_text, line_of, nodes, simple_name, Abstraction, Implementations};
use crate::Language;

pub(super) fn abstractions(language: &Language, root: AstNode<'_>) -> Vec<Abstraction> {
    match language {
        Language::Rust => rust_abstractions(root),
        Language::Java => java_abstractions(root),
        Language::Python => python_abstractions(root),
        Language::TypeScript => ts_abstractions(root),
        Language::Go => go_abstractions(root),
        _ => Vec::new(),
    }
}

pub(super) fn collect_implementations(
    language: &Language,
    root: AstNode<'_>,
    facts: &mut Implementations,
) {
    match language {
        Language::Rust => rust_implementations(root, facts),
        Language::Java => java_implementations(root, facts),
        Language::Python => python_implementations(root, facts),
        Language::TypeScript => ts_implementations(root, facts),
        Language::Go => go_implementations(root, facts),
        _ => {}
    }
}

fn nominal(name: &str, kind: &'static str, node: AstNode<'_>, public: bool) -> Abstraction {
    Abstraction {
        name: name.to_string(),
        kind,
        line: line_of(node),
        public,
        methods: Vec::new(),
    }
}

// Rust ---------------------------------------------------------------------

fn rust_abstractions(root: AstNode<'_>) -> Vec<Abstraction> {
    nodes(root, &["trait_item"])
        .into_iter()
        .filter(|item| !crate::ast_scan::is_inside_rust_test(*item))
        .filter_map(|item| {
            let name = item.child_by_field_name("name")?.text();
            let public = item
                .named_children()
                .any(|child| child.kind() == "visibility_modifier" && child.text() == "pub");
            Some(nominal(name, "Trait", item, public))
        })
        .collect()
}

/// `impl Trait for Type`; a blanket `impl<T: ..> Trait for T` (or `&T`)
/// covers every type.
fn rust_implementations(root: AstNode<'_>, facts: &mut Implementations) {
    for item in nodes(
        root,
        &["struct_item", "enum_item", "union_item", "type_item"],
    ) {
        if let Some(name) = item.child_by_field_name("name") {
            facts.types.insert(name.text().to_string());
        }
    }
    for item in nodes(root, &["impl_item"]) {
        let (Some(implemented), Some(target)) = (
            item.child_by_field_name("trait"),
            item.child_by_field_name("type"),
        ) else {
            continue;
        };
        let trait_name = simple_name(implemented.text());
        let target_name = simple_name(
            target
                .text()
                .trim_start_matches(['&', ' '])
                .trim_start_matches("mut "),
        );
        let generic = item
            .child_by_field_name("type_parameters")
            .is_some_and(|parameters| {
                parameters.named_children().any(|parameter| {
                    parameter.text().split([':', ' ']).next() == Some(target_name.as_str())
                })
            });
        if generic {
            facts.unbounded.insert(trait_name);
        } else {
            facts
                .by_name
                .entry(trait_name)
                .or_default()
                .insert(target_name);
        }
    }
}

// Java -----------------------------------------------------------------------

/// Interfaces with more than one abstract method (a single-method interface
/// is implemented by lambdas, which are not counted), and abstract classes.
fn java_abstractions(root: AstNode<'_>) -> Vec<Abstraction> {
    let mut found = Vec::new();
    for declaration in nodes(root, &["interface_declaration", "class_declaration"]) {
        let Some(name) = declaration.child_by_field_name("name") else {
            continue;
        };
        let modifiers = child_text(declaration, &["modifiers"]).unwrap_or_default();
        // A type declared in an interface is implicitly public.
        let in_interface = declaration
            .parent()
            .is_some_and(|parent| parent.kind() == "interface_body");
        let public = in_interface
            || modifiers
                .split_whitespace()
                .any(|word| word == "public" || word == "protected");
        if declaration.kind() == "interface_declaration" {
            if java_abstract_methods(declaration) > 1 {
                found.push(nominal(name.text(), "Interface", declaration, public));
            }
        } else if modifiers.split_whitespace().any(|word| word == "abstract")
            && declares_abstract_member(declaration, "method_declaration", "abstract")
        {
            found.push(nominal(name.text(), "Abstract class", declaration, public));
        }
    }
    found
}

/// A class body member of `kind` whose text carries `marker` before its
/// name: an abstract method (`abstract void run();`, an `abstract` TS
/// member), a Python method decorated `@abstractmethod`. An abstract class
/// without one is a base for shared code, not a variation point.
fn declares_abstract_member(class: AstNode<'_>, kind: &str, marker: &str) -> bool {
    class.child_by_field_name("body").is_some_and(|body| {
        body.named_children().any(|member| {
            member.kind().contains(kind)
                && member
                    .text()
                    .split(['(', ':', '='])
                    .next()
                    .is_some_and(|head| head.contains(marker))
        })
    })
}

/// Interface methods without a body that are not `default` or `static`.
fn java_abstract_methods(interface: AstNode<'_>) -> usize {
    interface
        .child_by_field_name("body")
        .map(|body| {
            body.named_children()
                .filter(|member| {
                    member.kind() == "method_declaration"
                        && member.child_by_field_name("body").is_none()
                        && !child_text(*member, &["modifiers"]).is_some_and(|modifiers| {
                            modifiers.contains("default") || modifiers.contains("static")
                        })
                })
                .count()
        })
        .unwrap_or_default()
}

/// `extends`/`implements` of classes, enums, and records, and anonymous
/// classes; an interface extended by another heads a hierarchy, which
/// counts as unbounded.
fn java_implementations(root: AstNode<'_>, facts: &mut Implementations) {
    let declarations = nodes(
        root,
        &[
            "class_declaration",
            "enum_declaration",
            "record_declaration",
            "interface_declaration",
        ],
    );
    for declaration in declarations {
        let Some(name) = declaration.child_by_field_name("name") else {
            continue;
        };
        for implemented in java_supertypes(declaration) {
            if declaration.kind() == "interface_declaration" {
                facts.unbounded.insert(implemented);
            } else {
                record(facts, implemented, name.text().to_string());
            }
        }
    }
    for creation in nodes(root, &["object_creation_expression"]) {
        let anonymous = creation
            .named_children()
            .any(|child| child.kind() == "class_body");
        if let Some(implemented) = creation.child_by_field_name("type").filter(|_| anonymous) {
            let label = format!("anonymous class at line {}", line_of(creation));
            record(facts, simple_name(implemented.text()), label);
        }
    }
}

fn record(facts: &mut Implementations, implemented: String, implementer: String) {
    facts
        .by_name
        .entry(implemented)
        .or_default()
        .insert(implementer);
}

/// Every type a declaration's `extends`/`implements` clauses name.
fn java_supertypes(declaration: AstNode<'_>) -> Vec<String> {
    declaration
        .named_children()
        .filter(|child| {
            matches!(
                child.kind(),
                "superclass" | "super_interfaces" | "extends_interfaces"
            )
        })
        .flat_map(java_types)
        .collect()
}

/// The type names a `superclass`, `super_interfaces`, or
/// `extends_interfaces` clause lists.
fn java_types(clause: AstNode<'_>) -> Vec<String> {
    let list = clause
        .named_children()
        .find(|child| child.kind() == "type_list")
        .map(|list| list.named_children().collect::<Vec<_>>())
        .unwrap_or_else(|| clause.named_children().collect());
    list.into_iter()
        .map(|node| simple_name(node.text()))
        .collect()
}

// Python -------------------------------------------------------------------

/// Classes deriving from `ABC` or using `metaclass=ABCMeta`. A `Protocol`
/// is implemented structurally, without naming it, so it is not read.
fn python_abstractions(root: AstNode<'_>) -> Vec<Abstraction> {
    nodes(root, &["class_definition"])
        .into_iter()
        .filter_map(|class| {
            let name = class.child_by_field_name("name")?.text();
            let bases = python_bases(class);
            let abstract_base = bases.iter().any(|base| base == "ABC")
                || class
                    .child_by_field_name("superclasses")
                    .is_some_and(|arguments| arguments.text().contains("ABCMeta"));
            (abstract_base
                && !bases.iter().any(|base| base == "Protocol")
                && declares_abstract_member(class, "decorated_definition", "abstractmethod"))
            .then(|| nominal(name, "Abstract base class", class, !name.starts_with('_')))
        })
        .collect()
}

fn python_bases(class: AstNode<'_>) -> Vec<String> {
    class
        .child_by_field_name("superclasses")
        .map(|arguments| {
            arguments
                .named_children()
                .filter(|base| matches!(base.kind(), "identifier" | "attribute" | "subscript"))
                .map(|base| simple_name(base.text()))
                .collect()
        })
        .unwrap_or_default()
}

fn python_implementations(root: AstNode<'_>, facts: &mut Implementations) {
    for class in nodes(root, &["class_definition"]) {
        let Some(name) = class.child_by_field_name("name") else {
            continue;
        };
        for base in python_bases(class) {
            facts
                .by_name
                .entry(base)
                .or_default()
                .insert(name.text().to_string());
        }
    }
}

// TypeScript -----------------------------------------------------------------

/// Abstract classes. An interface is mostly a structural shape, implemented
/// by objects that never name it, so it is not read.
fn ts_abstractions(root: AstNode<'_>) -> Vec<Abstraction> {
    nodes(root, &["abstract_class_declaration"])
        .into_iter()
        .filter_map(|class| {
            let name = class.child_by_field_name("name")?.text();
            let public = class
                .parent()
                .is_some_and(|parent| parent.kind() == "export_statement");
            declares_abstract_member(class, "abstract_method_signature", "abstract")
                .then(|| nominal(name, "Abstract class", class, public))
        })
        .collect()
}

fn ts_implementations(root: AstNode<'_>, facts: &mut Implementations) {
    for class in nodes(
        root,
        &["class_declaration", "abstract_class_declaration", "class"],
    ) {
        let name = class
            .child_by_field_name("name")
            .map(|name| name.text().to_string())
            .unwrap_or_else(|| format!("class expression at line {}", line_of(class)));
        let Some(heritage) = class
            .named_children()
            .find(|child| child.kind() == "class_heritage")
        else {
            continue;
        };
        for clause in heritage.named_children() {
            for parent in clause.named_children() {
                facts
                    .by_name
                    .entry(simple_name(parent.text()))
                    .or_default()
                    .insert(name.clone());
            }
        }
    }
}

// Go -------------------------------------------------------------------------

/// Interfaces of method elements only: an embedded interface or a type set
/// is not read.
fn go_abstractions(root: AstNode<'_>) -> Vec<Abstraction> {
    if go_build_constrained(root) {
        return Vec::new();
    }
    nodes(root, &["type_spec"])
        .into_iter()
        .filter_map(|spec| {
            let name = spec.child_by_field_name("name")?.text();
            let interface = spec
                .child_by_field_name("type")
                .filter(|kind| kind.kind() == "interface_type")?;
            let mut methods = Vec::new();
            for element in interface.named_children() {
                match element.kind() {
                    "method_elem" | "method_spec" => {
                        methods.push(element.child_by_field_name("name")?.text().to_string());
                    }
                    "comment" => {}
                    _ => return None,
                }
            }
            if methods.is_empty() {
                return None;
            }
            Some(Abstraction {
                name: name.to_string(),
                kind: "Interface",
                line: line_of(spec),
                public: name.starts_with(|c: char| c.is_ascii_uppercase()),
                methods,
            })
        })
        .collect()
}

/// A file built only for some platforms or tags (`//go:build`): it
/// redeclares what another variant of the file declares.
fn go_build_constrained(root: AstNode<'_>) -> bool {
    root.named_children()
        .take_while(|child| child.kind() == "comment")
        .any(|comment| {
            let text = comment.text();
            text.starts_with("//go:build") || text.starts_with("// +build")
        })
}

fn go_implementations(root: AstNode<'_>, facts: &mut Implementations) {
    for method in nodes(root, &["method_declaration"]) {
        let (Some(receiver), Some(name)) = (
            method.child_by_field_name("receiver"),
            method.child_by_field_name("name"),
        ) else {
            continue;
        };
        let Some(declaration) = receiver.named_child(0) else {
            continue;
        };
        let Some(receiver_type) = declaration.child_by_field_name("type") else {
            continue;
        };
        let receiver_name = simple_name(receiver_type.text().trim_start_matches('*'));
        facts
            .methods
            .entry(receiver_name)
            .or_default()
            .insert(name.text().to_string());
    }
}
