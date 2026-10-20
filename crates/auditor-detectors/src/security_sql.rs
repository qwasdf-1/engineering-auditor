use auditor_core::{Confidence, Finding, Severity};

use crate::ast_scan::{self, AstScan};
use crate::security_sql_names::{is_runtime_value, names_only};
use crate::{util, Language};

pub(crate) fn sql_injection_findings(scan: AstScan<'_, '_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            matches!(
                language,
                Language::Go
                    | Language::Python
                    | Language::Rust
                    | Language::JavaScript
                    | Language::TypeScript
                    | Language::Java
                    | Language::Kotlin
                    | Language::CSharp
            )
        },
        |file, tree, _| {
            let relative = util::path_slash(&file.relative);
            let root = tree.root_node();
            let sites = match file.language {
                Language::Python => python_sql_sites(root),
                Language::Kotlin => crate::tier_two_sql::kotlin_sql_sites(root),
                Language::CSharp => crate::tier_two_sql::csharp_sql_sites(root),
                Language::Rust => rust_sql_sites(root),
                Language::JavaScript | Language::TypeScript | Language::Java => {
                    built_query_sites(&file.language, root)
                }
                _ => ast_scan::nodes_of_kind(root, &["call_expression"])
                    .into_iter()
                    .filter(|node| {
                        is_sql_sink(ast_scan::callee(*node))
                            && has_sql_keyword(node.text())
                            && has_dynamic_query(*node)
                            && !first_argument(*node).is_some_and(names_only)
                    })
                    .collect(),
            };
            for node in sites {
                let line = node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "sql_injection",
                        "correctness",
                        "Potential SQL injection via string building (AST-detected)",
                        format!("SQL query with string construction at line {line}"),
                        Severity::High,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation("Use parameterized queries instead of string building.")
                    .with_evidence("pattern", &node.text()[..node.text().len().min(80)])
                    .with_evidence("analysis", "ast"),
                );
            }
        },
    );
    findings
}

pub(crate) fn has_sql_keyword(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["select", "insert", "update", "delete"]
        .iter()
        .any(|keyword| util::count_word(&lower, keyword) > 0)
}

fn is_sql_sink(callee: &str) -> bool {
    matches!(
        callee
            .rsplit('.')
            .next()
            .unwrap_or(callee)
            .to_ascii_lowercase()
            .as_str(),
        "query" | "queryrow" | "querycontext" | "queryrowcontext" | "exec" | "execcontext"
    )
}

/// DB-API `execute`/`executemany`/`executescript`, Django `raw`, and
/// SQLAlchemy `text` given a query built by f-string, `%`, `+`, or
/// `.format` (Bandit B608). Only the query argument counts: bound parameters
/// may be computed freely.
fn python_sql_sites(root: auditor_ast::AstNode<'_>) -> Vec<auditor_ast::AstNode<'_>> {
    let scope = crate::python_api_identity::PyScope::new(root);
    let mut sites = scope
        .calls(root)
        .into_iter()
        .map(|(api, call)| (Some(api), call))
        .chain(
            ast_scan::nodes_of_kind(root, &["call"])
                .into_iter()
                .map(|call| (None, call)),
        )
        .filter(|(api, call)| {
            let sink = match api {
                Some(api) => matches!(
                    api.as_str(),
                    "sqlalchemy.text" | "sqlalchemy.sql.text" | "sqlalchemy.sql.expression.text"
                ),
                None => {
                    matches!(
                        ast_scan::callee(*call).rsplit('.').next(),
                        Some("execute" | "executemany" | "executescript" | "raw")
                    ) && ast_scan::callee(*call).contains('.')
                }
            };
            sink && first_argument(*call)
                .is_some_and(|query| built_from_values(query, has_dynamic_query))
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    sites.sort_by_key(|call| call.start_byte());
    sites.dedup_by_key(|call| call.start_byte());
    sites
}

/// sqlx `query`/`query_as`/`query_scalar`, Diesel `sql_query`, and the
/// rusqlite/postgres `execute`/`query*`/`prepare`/`batch_execute` methods
/// given a query built by `format!` or `+`. The checked `query!` macros are
/// not calls.
fn rust_sql_sites(root: auditor_ast::AstNode<'_>) -> Vec<auditor_ast::AstNode<'_>> {
    ast_scan::nodes_of_kind(root, &["call_expression"])
        .into_iter()
        .filter(|call| {
            matches!(
                ast_scan::callee(*call).rsplit(['.', ':']).next(),
                Some(
                    "query"
                        | "query_as"
                        | "query_scalar"
                        | "sql_query"
                        | "execute"
                        | "query_row"
                        | "query_one"
                        | "query_opt"
                        | "prepare"
                        | "batch_execute"
                        | "execute_batch"
                )
            ) && !ast_scan::is_inside_rust_test(*call)
                && first_argument(*call)
                    .is_some_and(|query| built_from_values(query, has_dynamic_query))
        })
        .collect()
}

/// JS/TS driver and ORM query calls (`query`, `execute`, `raw`, Prisma's
/// `$queryRawUnsafe`) and JDBC, JPA, Hibernate, and `JdbcTemplate` calls
/// whose query argument is a template with substitutions, a `String.format`,
/// or a concatenation with a value that is neither a literal nor an
/// `UPPER_SNAKE` constant.
fn built_query_sites<'a>(
    language: &Language,
    root: auditor_ast::AstNode<'a>,
) -> Vec<auditor_ast::AstNode<'a>> {
    let (call_kind, sinks): (&str, &[&str]) = if *language == Language::Java {
        (
            "method_invocation",
            &[
                "executeQuery",
                "executeUpdate",
                "executeLargeUpdate",
                "execute",
                "addBatch",
                "prepareStatement",
                "prepareCall",
                "createQuery",
                "createNativeQuery",
                "createSQLQuery",
                "queryForObject",
                "queryForList",
                "queryForMap",
                "query",
                "update",
            ],
        )
    } else {
        (
            "call_expression",
            &[
                "query",
                "execute",
                "raw",
                "$queryRawUnsafe",
                "$executeRawUnsafe",
                "unsafe",
            ],
        )
    };
    ast_scan::nodes_of_kind(root, &[call_kind])
        .into_iter()
        .filter(|call| {
            let name = if *language == Language::Java {
                call.child_by_field_name("name").map(|name| name.text())
            } else {
                call.child_by_field_name("function").map(|function| {
                    function
                        .child_by_field_name("property")
                        .unwrap_or(function)
                        .text()
                })
            };
            name.is_some_and(|name| sinks.contains(&name))
                && first_argument(*call)
                    .is_some_and(|query| built_from_values(query, builds_from_values))
        })
        .collect()
}

/// A query with SQL keywords built from runtime values (by `builds`) that
/// are not all table, schema, or column names.
pub(crate) fn built_from_values(
    query: auditor_ast::AstNode<'_>,
    builds: impl Fn(auditor_ast::AstNode<'_>) -> bool,
) -> bool {
    has_sql_keyword(query.text()) && builds(query) && !names_only(query)
}

/// A query expression built from runtime values.
fn builds_from_values(node: auditor_ast::AstNode<'_>) -> bool {
    match node.kind() {
        "template_string" => node
            .named_children()
            .any(|part| part.kind() == "template_substitution"),
        "binary_expression" => node
            .named_children()
            .any(|operand| builds_from_values(operand) || is_runtime_value(operand)),
        "method_invocation" => node
            .child_by_field_name("name")
            .is_some_and(|name| matches!(name.text(), "format" | "formatted")),
        "parenthesized_expression" => node.named_children().any(builds_from_values),
        _ => false,
    }
}

fn first_argument(call: auditor_ast::AstNode<'_>) -> Option<auditor_ast::AstNode<'_>> {
    call.child_by_field_name("arguments")?
        .named_children()
        .find(|argument| !argument.kind().ends_with("comment"))
}

fn has_dynamic_query(node: auditor_ast::AstNode<'_>) -> bool {
    if matches!(
        node.kind(),
        "binary_expression" | "interpolation" | "formatted_string" | "template_string"
    ) {
        return true;
    }
    // Python `+` or `%` building, `.format(..)`, and Rust `format!`.
    if node.kind() == "binary_operator"
        || node.kind() == "call" && ast_scan::callee(node).ends_with(".format")
        || node.kind() == "macro_invocation"
            && node
                .child_by_field_name("macro")
                .is_some_and(|name| name.text() == "format")
    {
        return true;
    }
    if node.kind() == "call_expression"
        && matches!(
            ast_scan::callee(node).to_ascii_lowercase().as_str(),
            "fmt.sprintf" | "fmt.printf"
        )
    {
        return true;
    }
    node.named_children().any(has_dynamic_query)
}
