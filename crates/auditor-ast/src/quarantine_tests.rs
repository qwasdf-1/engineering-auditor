use super::*;

fn c_tree(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&crate::languages::get_language(&AstLang::C).unwrap())
        .unwrap();
    parser.parse(source, None).unwrap()
}

#[test]
fn a_quarantine_inside_a_function_takes_the_whole_function() {
    let source =
        "int keep(void) {\n  return 1;\n}\n\nint cut(int a) {\n  int b = a;\n  return b;\n}\n";
    let tree = c_tree(source);
    let start = source.find("int b").unwrap();
    let inside = start..start + 9;
    let function = source.find("int cut").unwrap()..source.len() - 1;
    assert_eq!(
        cut_functions(tree.root_node(), std::slice::from_ref(&inside)),
        [function]
    );
    assert!(cut_functions(tree.root_node(), &[]).is_empty());
}

#[test]
fn inactive_branches_keep_the_first_live_branch() {
    let source = "#if 0\nold();\n#elif defined(X)\nnew();\n#else\nfallback();\n#endif\nkeep();\n";
    let removed = branches::inactive_branches(source)
        .into_iter()
        .map(|range| source[range].trim().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        removed,
        [
            "#if 0",
            "old();",
            "#elif defined(X)",
            "#else",
            "fallback();",
            "#endif"
        ]
    );
}
