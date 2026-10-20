use super::*;
use auditor_ast::{AstLang, AstParser};

fn find<'a>(node: AstNode<'a>, text: &str) -> Option<AstNode<'a>> {
    if node.text() == text {
        return Some(node);
    }
    node.children().find_map(|child| find(child, text))
}

/// `names_only` of the expression `query` within `source`.
fn names_only_in(language: AstLang, source: &str, query: &str) -> bool {
    let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
    let node = find(tree.root_node(), query).unwrap_or_else(|| panic!("no node {query:?}"));
    names_only(node)
}

fn java(query: &str) -> bool {
    let source = format!("class A {{ void f(Handle h) {{ h.execute({query}); }} }}");
    names_only_in(AstLang::Java, &source, query)
}

#[test]
fn identifiers_formatted_into_java_queries_are_names() {
    assert!(java(
        r#""DELETE FROM \"%s\".migration_state WHERE phase = 'LOAD'".formatted(options.stagingSchema)"#
    ));
    assert!(java(
        r#""SELECT count(*) FROM \"%s\".tgt_%s".formatted(schema, table)"#
    ));
    assert!(java(r#""SELECT count(*) FROM " + table"#));
    assert!(java(r#"String.format("TRUNCATE %s", table)"#));
    assert!(java(
        "\"\"\"\n    SELECT setval(pg_get_serial_sequence('\"%1$s\"', 'ID'),\n      COALESCE((SELECT MAX(\"ID\") FROM \"%1$s\"), 1), true)\n    \"\"\".formatted(t)"
    ));
}

#[test]
fn values_formatted_into_java_queries_are_not_names() {
    assert!(!java(r#""SELECT * FROM t WHERE id = " + id"#));
    assert!(!java(
        r#""SELECT * FROM t WHERE name = '%s'".formatted(name)"#
    ));
    assert!(!java(r#""SELECT * FROM t WHERE id = %d".formatted(id)"#));
    assert!(!java(r#""SELECT * FROM t ORDER BY " + sort"#));
    assert!(!java(
        r#""SELECT * FROM " + table + " WHERE id IN (" + ids + ")""#
    ));
}

#[test]
fn a_local_bound_only_to_literals_is_a_constant() {
    let source = "class A { List f(Handle h) {\n  final String filters = \"\"\"\n    <#macro f>AND x</#macro>\n    \"\"\";\n  return h.createQuery(filters + \"\"\"\n    SELECT 1\n    \"\"\").list();\n} }\n";
    assert!(names_only_in(
        AstLang::Java,
        source,
        "filters + \"\"\"\n    SELECT 1\n    \"\"\""
    ));
    let chosen = "class A { List f(Handle h, boolean escape) {\n  final String clause = escape ? \" ESCAPE '\\\\'\" : \"\";\n  return h.createQuery(\"SELECT t FROM s WHERE t LIKE :p\" + clause).list();\n} }\n";
    assert!(names_only_in(
        AstLang::Java,
        chosen,
        "\"SELECT t FROM s WHERE t LIKE :p\" + clause"
    ));
    let reassigned = "class A { void f(Handle h, String id) {\n  String where = \"\";\n  where = \"id = \" + id;\n  h.execute(\"SELECT * FROM t WHERE \" + where);\n} }\n";
    assert!(!names_only_in(
        AstLang::Java,
        reassigned,
        "\"SELECT * FROM t WHERE \" + where"
    ));
}

#[test]
fn name_slots_are_read_in_each_language() {
    let csharp = "class T { bool D(int id) => db.Execute(\"delete from \" + TableName + \" where Id = @id\", new { id }) > 0; }";
    assert!(names_only_in(
        AstLang::CSharp,
        csharp,
        "\"delete from \" + TableName + \" where Id = @id\""
    ));
    let csharp_value =
        "class T { void D(string id) { cmd.CommandText = $\"DELETE FROM t WHERE id = {id}\"; } }";
    assert!(!names_only_in(
        AstLang::CSharp,
        csharp_value,
        "$\"DELETE FROM t WHERE id = {id}\""
    ));

    let js = "db.query(`SELECT * FROM ${schema}.users WHERE id = $1`, [id]);\ndb.query(`SELECT * FROM users WHERE id = ${id}`);\n";
    assert!(names_only_in(
        AstLang::JavaScript,
        js,
        "`SELECT * FROM ${schema}.users WHERE id = $1`"
    ));
    assert!(!names_only_in(
        AstLang::JavaScript,
        js,
        "`SELECT * FROM users WHERE id = ${id}`"
    ));

    let python = "c.execute(\"SELECT * FROM {}\".format(table))\nc.execute(f\"SELECT * FROM t WHERE id = {id}\")\nc.execute(\"SELECT * FROM t WHERE id = %s\" % id)\n";
    assert!(names_only_in(
        AstLang::Python,
        python,
        "\"SELECT * FROM {}\".format(table)"
    ));
    assert!(!names_only_in(
        AstLang::Python,
        python,
        "f\"SELECT * FROM t WHERE id = {id}\""
    ));
    assert!(!names_only_in(
        AstLang::Python,
        python,
        "\"SELECT * FROM t WHERE id = %s\" % id"
    ));

    let go = "package db\nfunc f() {\n  db.Query(fmt.Sprintf(\"SELECT * FROM %s\", table))\n  db.Query(fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", id))\n}\n";
    assert!(names_only_in(
        AstLang::Go,
        go,
        "fmt.Sprintf(\"SELECT * FROM %s\", table)"
    ));
    assert!(!names_only_in(
        AstLang::Go,
        go,
        "fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", id)"
    ));

    let kotlin = "fun f(st: Statement) {\n  st.executeQuery(\"SELECT * FROM $table\")\n  st.executeQuery(\"SELECT * FROM t WHERE id = $id\")\n}\n";
    assert!(names_only_in(
        AstLang::Kotlin,
        kotlin,
        "\"SELECT * FROM $table\""
    ));
    assert!(!names_only_in(
        AstLang::Kotlin,
        kotlin,
        "\"SELECT * FROM t WHERE id = $id\""
    ));
}
