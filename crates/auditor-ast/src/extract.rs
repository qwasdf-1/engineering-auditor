use crate::{languages, AstLang, AstNode, AstNodeKind, AstTree};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl AstNode<'_> {
    pub fn declared_name(&self) -> Option<String> {
        let direct = |child: AstNode<'_>| {
            matches!(
                child.kind(),
                "name" | "identifier" | "simple_identifier" | "word"
            )
            .then(|| child.text().to_string())
        };
        // The `name` field first: a C# return type is also an `identifier`
        // and comes before it. An arrow function's lone `parameter` is not
        // its name.
        let parameter = self
            .child_by_field_name("parameter")
            .map(|node| node.start_byte());
        self.child_by_field_name("name")
            .and_then(direct)
            .or_else(|| {
                self.named_children()
                    .filter(|child| Some(child.start_byte()) != parameter)
                    .find_map(direct)
            })
            .or_else(|| {
                self.named_children()
                    .filter(|child| {
                        matches!(
                            child.kind(),
                            "declarator"
                                | "function_declarator"
                                | "signature"
                                // C and C++: `T *name(..)`, `T &name(..)`.
                                | "pointer_declarator"
                                | "reference_declarator"
                                | "parenthesized_declarator"
                        )
                    })
                    .find_map(|child| find_name_in_nested(&child))
            })
            .or_else(|| self.parent()?.named_children().find_map(direct))
    }
}

pub fn extract_functions(tree: &AstTree, _lang: &AstLang) -> Vec<FunctionSpan> {
    tree.function_spans().as_ref().to_vec()
}

pub(crate) fn extract_functions_uncached(tree: &AstTree) -> Vec<FunctionSpan> {
    let root = tree.root_node();
    let mut functions = Vec::new();
    for &kind in languages::function_node_kinds() {
        for node in root.descendants_of_kind(&AstNodeKind::from(kind)) {
            if let Some(name) = node.declared_name() {
                let (start_row, _) = node.start_position();
                let (end_row, _) = node.end_position();
                functions.push(FunctionSpan {
                    name,
                    start_line: start_row + 1,
                    end_line: end_row + 1,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
    }
    functions
}

fn find_name_in_nested(node: &AstNode<'_>) -> Option<String> {
    for child in node.named_children() {
        match child.kind() {
            "identifier" | "field_identifier" | "operator_name" | "destructor_name" => {
                return Some(child.text().to_string())
            }
            "declarator"
            | "function_declarator"
            | "array_declarator"
            | "reference_declarator"
            | "pointer_declarator"
            | "parenthesized_declarator"
            // C++ `Type::method`: the method's own name.
            | "qualified_identifier"
            | "signature"
            | "call_expression" => {
                if let Some(name) = find_name_in_nested(&child) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn structural_fingerprint(node: &AstNode<'_>) -> String {
    let mut fingerprint = String::new();
    build_fingerprint(node, &mut fingerprint);
    fingerprint
}

fn build_fingerprint(node: &AstNode<'_>, fingerprint: &mut String) {
    if !node.is_named() {
        return;
    }
    let kind = node.kind();
    if node.child_count() == 0 {
        fingerprint.push('[');
        fingerprint.push_str(kind);
        fingerprint.push(':');
        if matches!(
            kind,
            "identifier"
                | "string"
                | "string_content"
                | "number"
                | "true"
                | "false"
                | "integer"
                | "float"
                | "escape_sequence"
                | "character"
                | "comment"
                | "line_comment"
                | "block_comment"
        ) {
            fingerprint.push_str("__");
        } else {
            fingerprint.push_str(node.text());
        }
        fingerprint.push(']');
        return;
    }
    fingerprint.push('<');
    fingerprint.push_str(kind);
    for child in node.children() {
        build_fingerprint(&child, fingerprint);
    }
    fingerprint.push('>');
}

pub fn function_fingerprint(tree: &AstTree, span: &FunctionSpan) -> String {
    let root = tree.root_node();
    find_function_node(&root, span)
        .map(|node| structural_fingerprint(&node))
        .unwrap_or_default()
}

fn find_function_node<'a>(node: &AstNode<'a>, span: &FunctionSpan) -> Option<AstNode<'a>> {
    if node.start_byte() == span.start_byte && node.end_byte() == span.end_byte {
        return Some(*node);
    }
    let (start_row, _) = node.start_position();
    let (end_row, _) = node.end_position();
    if start_row + 1 > span.start_line || end_row + 1 < span.end_line {
        return None;
    }
    node.named_children()
        .find_map(|child| find_function_node(&child, span))
}

pub fn extract_comments(tree: &AstTree, lang: &AstLang) -> Vec<String> {
    let root = tree.root_node();
    languages::comment_node_kinds(lang)
        .iter()
        .flat_map(|kind| root.descendants_of_kind(&AstNodeKind::from(*kind)))
        .map(|node| node.text().to_string())
        .collect()
}

pub fn can_parse(lang: &AstLang) -> bool {
    languages::SUPPORTED_LANGUAGES.contains(lang)
}
