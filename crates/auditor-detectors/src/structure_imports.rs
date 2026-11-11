#[path = "structure_imports/parse.rs"]
mod parse;
#[path = "structure_imports/resolve.rs"]
mod resolve;
#[cfg(test)]
#[path = "structure_imports/tests.rs"]
mod tests;
#[path = "structure_imports/uses.rs"]
mod uses;

pub(super) use parse::{first_quoted, macro_changes, parse_import};
#[cfg(test)]
pub(super) use resolve::normalize_path;
pub(super) use resolve::{import_bindings, resolve_imports};
pub(super) use uses::{go_package_names, GoPackageNames};
