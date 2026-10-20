#[path = "structure_imports/parse.rs"]
mod parse;
#[path = "structure_imports/resolve.rs"]
mod resolve;

pub(super) use parse::{first_quoted, parse_import};
#[cfg(test)]
pub(super) use resolve::normalize_path;
pub(super) use resolve::{import_bindings, resolve_imports};
