//! Read queries against a data store: the query methods and the receivers
//! proven to be stores. Findings-free.

use auditor_ast::SymbolKind;

use super::CallShape;
use crate::structure_index::{StructureFile, StructureSymbol};

/// Receiver names that read as a data store.
const STORE_NAMES: &[&str] = &["repo", "repository", "database", "connection", "session"];

/// Receiver words that name a data store (`db`, `userDb`, `db_conn`). Whole
/// words only: `feedback` and `dbg` are not stores.
const STORE_WORDS: &[&str] = &["db", "conn"];

/// Methods that run the SQL text they are given: DB-API `execute` and JDBC
/// `executeQuery`. Batch forms (`executemany`) are the fix, not the problem.
const SQL_RUNNERS: &[&str] = &["execute", "execute_query"];

/// Declared types that name a data store, lowercased.
const STORE_TYPES: &[&str] = &[
    "repository",
    "database",
    "connection",
    "session",
    "queryclient",
    "querymanager",
    "persistencemanager",
    "entitymanager",
    "sql.db",
    "sql.tx",
    "sql.conn",
];

/// JavaScript modules whose export is itself a query interface
/// (`const { R } = require("redbean-node")`).
const STORE_MODULES: &[&str] = &["redbean-node", "knex"];

/// Words that make a lookup write when the record is missing
/// (`getOrCreateById`).
const WRITE_WORDS: &[&str] = &["create", "insert", "save", "update", "upsert", "put", "add"];

/// `FindByID` -> `find_by_id`, so Go and Java method names meet the table.
fn snake_case(name: &str) -> String {
    let characters = name.chars().collect::<Vec<_>>();
    let mut snake = String::new();
    for (index, character) in characters.iter().enumerate() {
        let previous = index.checked_sub(1).map(|previous| characters[previous]);
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_lowercase()
                    || (previous.is_uppercase() && next.is_some_and(|next| next.is_lowercase()))
            });
        if starts_word {
            snake.push('_');
        }
        snake.extend(character.to_lowercase());
    }
    snake
}

/// `shape` reads from a data store in `owner`: a query method on a proven
/// store, or a SQL runner given a `SELECT` literal, whose text proves the
/// store whatever the receiver is called (`cur.execute("SELECT ..", ..)`).
pub(super) fn is_store_query(file: &StructureFile, owner: u32, shape: &CallShape) -> bool {
    is_read_query(&shape.method) && query_receiver_is_proven(file, owner, &shape.root)
        || runs_select(shape)
}

/// A call that may be a store query, before its receiver is checked.
pub(super) fn may_be_store_query(shape: &CallShape) -> bool {
    is_read_query(&shape.method) || runs_select(shape)
}

fn runs_select(shape: &CallShape) -> bool {
    SQL_RUNNERS.contains(&snake_case(&shape.method).as_str())
        && starts_with_select_literal(&shape.arguments)
}

/// The first argument is a string literal whose SQL starts with `SELECT`:
/// `"SELECT .."`, `'select ..'`, a Python `f"SELECT .."` or `r"""SELECT .."""`,
/// a Go raw string, or a C# `@"SELECT .."` / `$"SELECT .."`.
fn starts_with_select_literal(arguments: &str) -> bool {
    const PREFIXES: &str = "rRfFbBuU@$";
    let text = arguments.trim_start();
    let Some(quote) = text
        .find(['"', '\'', '`'])
        .filter(|&index| index <= 2 && text[..index].chars().all(|c| PREFIXES.contains(c)))
    else {
        return false;
    };
    let sql = text[quote..]
        .trim_start_matches(['"', '\'', '`'])
        .trim_start();
    sql.get(..6)
        .is_some_and(|word| word.eq_ignore_ascii_case("select"))
        && sql[6..].starts_with(|c: char| !(c.is_alphanumeric() || c == '_'))
}

/// `receiver` is a data store in `owner`: named as one, or declared with a
/// store type as a parameter or local of `owner` or as a field, or bound
/// from a store module.
pub(super) fn query_receiver_is_proven(file: &StructureFile, owner: u32, receiver: &str) -> bool {
    let name = receiver.to_ascii_lowercase();
    STORE_NAMES.iter().any(|signal| name.contains(signal))
        || snake_case(receiver)
            .split('_')
            .any(|word| STORE_WORDS.contains(&word))
        || file.symbols.iter().any(|symbol| {
            symbol.name == receiver && (typed_store(owner, symbol) || imported_store(file, symbol))
        })
}

fn typed_store(owner: u32, symbol: &StructureSymbol) -> bool {
    let in_scope = match symbol.kind {
        SymbolKind::Parameter | SymbolKind::Variable => symbol.parent == Some(owner),
        SymbolKind::Field => true,
        _ => false,
    };
    in_scope
        && symbol.declaration.as_deref().is_some_and(|declaration| {
            let lower = declaration.to_ascii_lowercase();
            STORE_TYPES.iter().any(|signal| lower.contains(signal))
        })
}

/// A module-level binding declared on the line that imports a store module.
fn imported_store(file: &StructureFile, symbol: &StructureSymbol) -> bool {
    symbol.kind == SymbolKind::Variable
        && symbol.parent.is_none()
        && file.imports.iter().any(|import| {
            import.line == symbol.line
                && import
                    .module
                    .as_deref()
                    .is_some_and(|module| STORE_MODULES.contains(&module))
        })
}

pub(super) fn is_read_query(method: &str) -> bool {
    let method = snake_case(method);
    matches!(
        method.as_str(),
        "query_one"
            | "query_row"
            | "fetch_one"
            | "fetchrow"
            | "find_one"
            | "find_unique"
            | "find_first"
            | "query"
    ) || point_lookup(&method)
}

/// A read of one record by key: `find_by_id`, `get_object_by_uuid`,
/// `find_user_by_id`, `load_by_key`; not `get_or_create_by_id`.
fn point_lookup(method: &str) -> bool {
    let words = method.split('_').collect::<Vec<_>>();
    let [verb, middle @ .., key] = words.as_slice() else {
        return false;
    };
    let Some(by) = middle.iter().position(|word| *word == "by") else {
        return false;
    };
    matches!(*verb, "get" | "find" | "load" | "fetch")
        && matches!(*key, "id" | "uuid" | "key")
        && !middle[..by].iter().any(|word| WRITE_WORDS.contains(word))
}

#[cfg(test)]
#[path = "algorithmic_facts_queries_tests.rs"]
mod tests;
