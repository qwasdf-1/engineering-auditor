//! Query-provenance sites for Kotlin and C#: an SQL text built from runtime
//! values (string templates, interpolation, concatenation, `format`) handed
//! to a JDBC, ADO.NET, Entity Framework, or Dapper execution sink. Upper-snake
//! constants are configuration, not input; EF Core's `*Interpolated` methods
//! parameterize their interpolations and are not sinks.

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::csharp_api_identity::csharp_arguments;
use crate::kotlin_api_identity::kotlin_arguments;
use crate::security_sql::{has_sql_keyword, is_runtime_value};

/// JDBC, JPA, and Spring `JdbcTemplate` methods that run their SQL argument.
const JVM_SINKS: &[&str] = &[
    "executeQuery",
    "executeUpdate",
    "executeLargeUpdate",
    "execute",
    "addBatch",
    "prepareStatement",
    "prepareCall",
    "createQuery",
    "createNativeQuery",
    "queryForObject",
    "queryForList",
    "queryForMap",
    "query",
    "update",
];

/// Kotlin calls of a JDBC-style sink whose first argument builds SQL from
/// runtime values.
pub(crate) fn kotlin_sql_sites(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter(|call| {
            let method = call
                .named_child(0)
                .filter(|callee| callee.kind() == "navigation_expression")
                .map(|callee| callee.text().rsplit('.').next().unwrap_or_default());
            method.is_some_and(|method| JVM_SINKS.contains(&method))
                && kotlin_arguments(*call).first().is_some_and(|(_, query)| {
                    has_sql_keyword(query.text()) && kotlin_builds(*query)
                })
        })
        .collect()
}

fn kotlin_builds(node: AstNode<'_>) -> bool {
    match node.kind() {
        "string_literal" => node.named_children().any(|part| {
            matches!(
                part.kind(),
                "interpolated_identifier" | "interpolated_expression"
            )
        }),
        "additive_expression" => node
            .named_children()
            .any(|operand| kotlin_builds(operand) || kotlin_runtime_value(operand)),
        "call_expression" => node
            .named_child(0)
            .is_some_and(|callee| callee.text().ends_with("format")),
        "parenthesized_expression" => node.named_children().any(kotlin_builds),
        _ => false,
    }
}

fn kotlin_runtime_value(operand: AstNode<'_>) -> bool {
    operand.kind() != "string_literal"
        && operand.kind() != "additive_expression"
        && is_runtime_value(operand)
}

/// ADO.NET, Entity Framework, and Dapper sinks: `new SqlCommand(sql, ..)`,
/// `cmd.CommandText = sql`, `FromSqlRaw(sql)`, `ExecuteSqlRaw(sql)`, and
/// Dapper's `Query`/`Execute` family, where `sql` is built from runtime
/// values.
pub(crate) fn csharp_sql_sites(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    const METHOD_SINKS: &[&str] = &[
        "FromSqlRaw",
        "ExecuteSqlRaw",
        "ExecuteSqlRawAsync",
        "SqlQueryRaw",
        "Query",
        "QueryAsync",
        "QueryFirst",
        "QueryFirstOrDefault",
        "QuerySingle",
        "QuerySingleOrDefault",
        "QueryFirstAsync",
        "QueryFirstOrDefaultAsync",
        "QuerySingleAsync",
        "QuerySingleOrDefaultAsync",
        "Execute",
        "ExecuteAsync",
        "ExecuteScalar",
        "ExecuteScalarAsync",
        "ExecuteReader",
        "ExecuteReaderAsync",
    ];
    let unsafe_query = |query: AstNode<'_>| has_sql_keyword(query.text()) && csharp_builds(query);
    let mut sites = ast_scan::nodes_of_kind(
        root,
        &["invocation_expression", "object_creation_expression"],
    )
    .into_iter()
    .filter(|call| {
        let sink = match call.kind() {
            "object_creation_expression" => call
                .child_by_field_name("type")
                .is_some_and(|kind| kind.text().ends_with("SqlCommand")),
            _ => call
                .child_by_field_name("function")
                .and_then(|function| function.child_by_field_name("name"))
                .is_some_and(|name| METHOD_SINKS.contains(&name.text())),
        };
        sink && csharp_arguments(*call)
            .first()
            .is_some_and(|(_, query)| unsafe_query(*query))
    })
    .collect::<Vec<_>>();
    sites.extend(
        ast_scan::nodes_of_kind(root, &["assignment_expression"])
            .into_iter()
            .filter(|assignment| {
                assignment
                    .child_by_field_name("left")
                    .is_some_and(|left| left.text().rsplit('.').next() == Some("CommandText"))
                    && assignment
                        .child_by_field_name("right")
                        .is_some_and(unsafe_query)
            }),
    );
    sites.sort_by_key(|node| node.start_byte());
    sites
}

fn csharp_builds(node: AstNode<'_>) -> bool {
    match node.kind() {
        "interpolated_string_expression" => node
            .named_children()
            .any(|part| part.kind() == "interpolation"),
        "binary_expression" => node.named_children().any(|operand| {
            csharp_builds(operand)
                || (!operand.kind().contains("string")
                    && operand.kind() != "binary_expression"
                    && is_runtime_value(operand))
        }),
        "invocation_expression" => node
            .child_by_field_name("function")
            .is_some_and(|function| {
                matches!(
                    function.text(),
                    "string.Format" | "String.Format" | "string.Concat" | "String.Concat"
                )
            }),
        "parenthesized_expression" => node.named_children().any(csharp_builds),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    #[test]
    fn built_queries_reach_kotlin_and_csharp_sinks() {
        let kotlin = r#"fun f(st: Statement, id: String) {
    st.executeQuery("SELECT * FROM t WHERE id = $id")
    st.executeQuery("SELECT * FROM t WHERE id = " + id)
    st.executeQuery("SELECT * FROM t WHERE id = " + TABLE_ID)
    st.executeQuery("SELECT * FROM t")
}
"#;
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::Kotlin, kotlin)
            .unwrap();
        assert_eq!(kotlin_sql_sites(tree.root_node()).len(), 2);
        let csharp = r#"class C { void F(SqlConnection c, string id) {
    var cmd = new SqlCommand("SELECT * FROM t WHERE id = " + id, c);
    cmd.CommandText = $"DELETE FROM t WHERE id = {id}";
    db.Database.ExecuteSqlInterpolated($"DELETE FROM t WHERE id = {id}");
    c.Query<T>("SELECT * FROM t WHERE id = @id", new { id });
} }
"#;
        let tree = AstParser::new()
            .unwrap()
            .parse(&AstLang::CSharp, csharp)
            .unwrap();
        assert_eq!(csharp_sql_sites(tree.root_node()).len(), 2);
    }
}
