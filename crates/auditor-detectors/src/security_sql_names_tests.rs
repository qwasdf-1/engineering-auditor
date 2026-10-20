use super::*;
use auditor_ast::{AstLang, AstParser};

fn find<'a>(node: AstNode<'a>, text: &str) -> Option<AstNode<'a>> {
    if node.text() == text {
        return Some(node);
    }
    node.children().find_map(|child| find(child, text))
}

/// `structure_only` of the expression `query` within `source`.
fn structure_only_in(language: AstLang, source: &str, query: &str) -> bool {
    let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
    let node = find(tree.root_node(), query).unwrap_or_else(|| panic!("no node {query:?}"));
    structure_only(node)
}

fn java(query: &str) -> bool {
    let source = format!("class A {{ void f(Handle h) {{ h.execute({query}); }} }}");
    structure_only_in(AstLang::Java, &source, query)
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
    assert!(structure_only_in(
        AstLang::Java,
        source,
        "filters + \"\"\"\n    SELECT 1\n    \"\"\""
    ));
    let chosen = "class A { List f(Handle h, boolean escape) {\n  final String clause = escape ? \" ESCAPE '\\\\'\" : \"\";\n  return h.createQuery(\"SELECT t FROM s WHERE t LIKE :p\" + clause).list();\n} }\n";
    assert!(structure_only_in(
        AstLang::Java,
        chosen,
        "\"SELECT t FROM s WHERE t LIKE :p\" + clause"
    ));
    let reassigned = "class A { void f(Handle h, String id) {\n  String where = \"\";\n  where = \"id = \" + id;\n  h.execute(\"SELECT * FROM t WHERE \" + where);\n} }\n";
    assert!(!structure_only_in(
        AstLang::Java,
        reassigned,
        "\"SELECT * FROM t WHERE \" + where"
    ));
}

#[test]
fn name_slots_are_read_in_each_language() {
    let csharp = "class T { bool D(int id) => db.Execute(\"delete from \" + TableName + \" where Id = @id\", new { id }) > 0; }";
    assert!(structure_only_in(
        AstLang::CSharp,
        csharp,
        "\"delete from \" + TableName + \" where Id = @id\""
    ));
    let csharp_value =
        "class T { void D(string id) { cmd.CommandText = $\"DELETE FROM t WHERE id = {id}\"; } }";
    assert!(!structure_only_in(
        AstLang::CSharp,
        csharp_value,
        "$\"DELETE FROM t WHERE id = {id}\""
    ));

    let js = "db.query(`SELECT * FROM ${schema}.users WHERE id = $1`, [id]);\ndb.query(`SELECT * FROM users WHERE id = ${id}`);\n";
    assert!(structure_only_in(
        AstLang::JavaScript,
        js,
        "`SELECT * FROM ${schema}.users WHERE id = $1`"
    ));
    assert!(!structure_only_in(
        AstLang::JavaScript,
        js,
        "`SELECT * FROM users WHERE id = ${id}`"
    ));

    let python = "c.execute(\"SELECT * FROM {}\".format(table))\nc.execute(f\"SELECT * FROM t WHERE id = {id}\")\nc.execute(\"SELECT * FROM t WHERE id = %s\" % id)\n";
    assert!(structure_only_in(
        AstLang::Python,
        python,
        "\"SELECT * FROM {}\".format(table)"
    ));
    assert!(!structure_only_in(
        AstLang::Python,
        python,
        "f\"SELECT * FROM t WHERE id = {id}\""
    ));
    assert!(!structure_only_in(
        AstLang::Python,
        python,
        "\"SELECT * FROM t WHERE id = %s\" % id"
    ));

    let go = "package db\nfunc f() {\n  db.Query(fmt.Sprintf(\"SELECT * FROM %s\", table))\n  db.Query(fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", id))\n}\n";
    assert!(structure_only_in(
        AstLang::Go,
        go,
        "fmt.Sprintf(\"SELECT * FROM %s\", table)"
    ));
    assert!(!structure_only_in(
        AstLang::Go,
        go,
        "fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", id)"
    ));

    let kotlin = "fun f(st: Statement) {\n  st.executeQuery(\"SELECT * FROM $table\")\n  st.executeQuery(\"SELECT * FROM t WHERE id = $id\")\n}\n";
    assert!(structure_only_in(
        AstLang::Kotlin,
        kotlin,
        "\"SELECT * FROM $table\""
    ));
    assert!(!structure_only_in(
        AstLang::Kotlin,
        kotlin,
        "\"SELECT * FROM t WHERE id = $id\""
    ));
}

#[test]
fn names_taken_from_request_input_are_not_configured_names() {
    let express = "app.get('/t', (req, res) => {\n  const { table } = req.query;\n  db.query('SELECT * FROM ' + table);\n  db.query('SELECT * FROM ' + config.table);\n});\n";
    assert!(!structure_only_in(
        AstLang::JavaScript,
        express,
        "'SELECT * FROM ' + table"
    ));
    assert!(structure_only_in(
        AstLang::JavaScript,
        express,
        "'SELECT * FROM ' + config.table"
    ));

    let flask =
        "@app.route('/t')\ndef show():\n    cur.execute('SELECT * FROM %s' % request.args['t'])\n";
    assert!(!structure_only_in(
        AstLang::Python,
        flask,
        "'SELECT * FROM %s' % request.args['t']"
    ));

    let spring = "class C { List f(@RequestParam String table) {\n  return jdbc.query(\"SELECT * FROM \\\"%s\\\"\".formatted(table));\n} }\n";
    assert!(!structure_only_in(
        AstLang::Java,
        spring,
        "\"SELECT * FROM \\\"%s\\\"\".formatted(table)"
    ));

    let gin = "package p\nfunc h(c *gin.Context) {\n  t := c.Query(\"t\")\n  db.Query(fmt.Sprintf(\"SELECT * FROM %s\", t))\n}\n";
    assert!(!structure_only_in(
        AstLang::Go,
        gin,
        "fmt.Sprintf(\"SELECT * FROM %s\", t)"
    ));
}

#[test]
fn fragments_a_query_builder_composes_are_structure() {
    assert!(java(r#""SELECT * FROM t WHERE " + buildCondition()"#));
    assert!(java(
        r#""SELECT * FROM t WHERE " + buildCondition() + " " + buildSorting()"#
    ));
    assert!(java(r#""SELECT m.id FROM (%s) AS m".formatted(innerSql)"#));
    assert!(java(
        r#""INSERT INTO \"%s\".src_%s (%s) VALUES (%s)".formatted(schema, name, columns, placeholders)"#
    ));
    assert!(java(
        r#""insert " + TableName + " (" + cols + ") values (" + params + ")""#
    ));
    let go = "package s\nfunc f(b *B, field string) {\n  db.Query(\"SELECT id FROM feeds WHERE \" + strings.Join(b.conditions, \" AND \") + \" LIMIT \" + strconv.Itoa(b.size))\n  db.Query(`SELECT true FROM users WHERE id <> $1 AND ` + pq.QuoteIdentifier(field) + `=$2`)\n}\n";
    assert!(structure_only_in(
        AstLang::Go,
        go,
        "\"SELECT id FROM feeds WHERE \" + strings.Join(b.conditions, \" AND \") + \" LIMIT \" + strconv.Itoa(b.size)"
    ));
    assert!(structure_only_in(
        AstLang::Go,
        go,
        "`SELECT true FROM users WHERE id <> $1 AND ` + pq.QuoteIdentifier(field) + `=$2`"
    ));
}

#[test]
fn values_stay_values_beside_fragments() {
    assert!(!java(r#""SELECT * FROM t WHERE x LIKE " + pattern"#));
    assert!(!java(r#""SELECT * FROM t LIMIT " + count"#));
    assert!(!java(
        r#""SELECT * FROM t WHERE " + cond + " AND id = " + id"#
    ));
    let express = "app.get('/t', (req, res) => {\n  db.query('SELECT * FROM t WHERE ' + req.query.filter);\n});\n";
    assert!(!structure_only_in(
        AstLang::JavaScript,
        express,
        "'SELECT * FROM t WHERE ' + req.query.filter"
    ));
    let rust = "fn f(c: &Connection, id: &str) {\n    c.execute(&format!(\"DELETE FROM t WHERE id = {}\", id), []);\n    c.execute(&format!(\"DELETE FROM {}\", table), []);\n}\n";
    assert!(!structure_only_in(
        AstLang::Rust,
        rust,
        "format!(\"DELETE FROM t WHERE id = {}\", id)"
    ));
    assert!(structure_only_in(
        AstLang::Rust,
        rust,
        "format!(\"DELETE FROM {}\", table)"
    ));
}
