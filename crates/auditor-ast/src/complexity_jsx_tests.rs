use crate::{
    extract_functions, function_complexity, function_node_kinds, AstLang, AstNodeKind, AstParser,
};

/// Halstead counts as (unique operators, unique operands, total operators,
/// total operands) from both the standalone and the combined traversal, which
/// must agree.
fn counts(lang: &AstLang, source: &str, fn_name: &str) -> (usize, usize, usize, usize) {
    let tree = AstParser::new().unwrap().parse(lang, source).unwrap();
    let function = extract_functions(&tree, lang)
        .into_iter()
        .find(|function| function.name == fn_name)
        .unwrap_or_else(|| panic!("function {fn_name} not found"));
    let root = tree.root_node();
    for kind in function_node_kinds() {
        for node in root.descendants_of_kind(&AstNodeKind::from(*kind)) {
            if node.start_position().0 + 1 != function.start_line {
                continue;
            }
            let combined = function_complexity(&node, fn_name).halstead;
            let standalone = crate::halstead(&node);
            let pick = |m: &crate::HalsteadMetrics| {
                (
                    m.unique_operators,
                    m.unique_operands,
                    m.total_operators,
                    m.total_operands,
                )
            };
            assert_eq!(pick(&combined), pick(&standalone));
            return pick(&combined);
        }
    }
    panic!("function {fn_name} not found in AST");
}

const PLAIN: &str = "function total(items, rate) {\n  const base = items.length * rate;\n  if (base > 10 && rate !== 0) {\n    return base ?? 'none';\n  }\n  return null;\n}\n";

#[test]
fn halstead_without_jsx_is_unchanged() {
    // The same counts held before JSX markup was excluded.
    for lang in [AstLang::JavaScript, AstLang::TypeScript] {
        assert_eq!(counts(&lang, PLAIN, "total"), (9, 8, 10, 11));
    }
}

#[test]
fn jsx_markup_is_not_counted() {
    let logic = "function Card(props) {\n  const label = props.title ?? 'none';\n  return;\n}\n";
    let markup = "function Card(props) {\n  const label = props.title ?? 'none';\n  return <section className=\"card wide\" id=\"x\" hidden><h1 data-kind=\"t\">Title text</h1><Icon.Star size=\"2\" /></section>;\n}\n";
    for lang in [AstLang::JavaScript, AstLang::TypeScript] {
        assert_eq!(counts(&lang, markup, "Card"), counts(&lang, logic, "Card"));
    }
}

#[test]
fn code_in_expression_containers_is_still_counted() {
    let source = "function List(props) {\n  return <ul id={props.id} {...props.rest}>{props.show && <b key={props.k}>bold</b>}</ul>;\n}\n";
    // Operators `return`, the spread `...`, and `&&`; operands `props` x4,
    // `id`, `rest`, `show`, and `k`. Tag names, `key`, the `id` attribute, the
    // text, and the angle brackets add nothing, including the `<b>` nested in
    // the container.
    assert_eq!(counts(&AstLang::TypeScript, source, "List"), (3, 5, 3, 8));
}
