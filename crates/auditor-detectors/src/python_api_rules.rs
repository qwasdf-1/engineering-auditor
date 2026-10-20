//! Python sites for the L1 rules resolved through API identity. Like the Go,
//! Java, and JS/TS tables, these finders return nodes; the owners construct
//! the findings.

use std::path::{Path, PathBuf};

use auditor_ast::AstNode;

use crate::ast_scan;
use crate::python_api_identity::{keyword_argument, positional_arguments, py_string, PyScope};
use crate::AuditedFile;

/// Debugger entry points (ruff `T100`, flake8-debugger).
const DEBUGGER_APIS: &[&str] = &[
    "breakpoint",
    "celery.contrib.rdb.set_trace",
    "debugpy.breakpoint",
    "debugpy.wait_for_client",
    "ipdb.post_mortem",
    "ipdb.set_trace",
    "pdb.pm",
    "pdb.post_mortem",
    "pdb.set_trace",
    "pudb.set_trace",
    "pudb.start",
    "sys.breakpointhook",
];

/// Calls in `root` whose resolved identity is one of `apis`.
pub(crate) fn py_calls_to<'a>(root: AstNode<'a>, apis: &[&str]) -> Vec<AstNode<'a>> {
    crate::api_identity::calls_named(PyScope::new(root).calls(root), apis)
}

pub(crate) fn py_debugger_calls(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    py_calls_to(root, DEBUGGER_APIS)
}

/// `sys.exit`, `os._exit`, `os.abort`, the `exit`/`quit` site builtins, and
/// `raise SystemExit`, outside the file's program code.
pub(crate) fn py_exits<'a>(file: &AuditedFile, root: AstNode<'a>) -> Vec<AstNode<'a>> {
    let scope = PyScope::new(root);
    let mut exits = scope
        .calls(root)
        .into_iter()
        .filter(|(api, _)| {
            matches!(
                api.as_str(),
                "sys.exit" | "os._exit" | "os.abort" | "exit" | "quit"
            )
        })
        .map(|(_, call)| call)
        .collect::<Vec<_>>();
    exits.extend(
        ast_scan::nodes_of_kind(root, &["raise_statement"])
            .into_iter()
            .filter(|statement| {
                statement
                    .named_child(0)
                    .map(|raised| raised.child_by_field_name("function").unwrap_or(raised))
                    .and_then(|raised| scope.resolve(raised))
                    .as_deref()
                    == Some("SystemExit")
            }),
    );
    exits.sort_by_key(|node| node.start_byte());
    outside_program_code(file, root, exits)
}

/// `print` (unless it writes to a file other than stdout/stderr),
/// `sys.stdout.write`, `sys.stderr.write`, and `pprint`, outside the file's
/// program code.
pub(crate) fn py_console_output<'a>(file: &AuditedFile, root: AstNode<'a>) -> Vec<AstNode<'a>> {
    let scope = PyScope::new(root);
    let outputs = scope
        .calls(root)
        .into_iter()
        .filter(|(api, call)| match api.as_str() {
            "print" => keyword_argument(*call, "file").is_none_or(|target| {
                matches!(
                    scope.resolve(target).as_deref(),
                    Some("sys.stdout" | "sys.stderr")
                )
            }),
            "sys.stdout.write" | "sys.stderr.write" | "pprint.pprint" | "pprint.pp" => true,
            _ => false,
        })
        .map(|(_, call)| call)
        .collect();
    outside_program_code(file, root, outputs)
}

/// A `# TODO` comment.
pub(crate) fn py_todo_nodes(root: AstNode<'_>) -> Vec<AstNode<'_>> {
    ast_scan::todo_comments(root, &["comment"])
}

/// Broken hashes and ciphers (Bandit B303, B304, B305, B324): `hashlib` MD5
/// and SHA-1 unless `usedforsecurity=False`, PyCryptodome `Crypto.Hash` and
/// `Crypto.Cipher` modules, and `cryptography` algorithms and ECB mode.
pub(crate) fn py_weak_cryptography(root: AstNode<'_>) -> Vec<(AstNode<'_>, &'static str)> {
    PyScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| Some((call, weak_python_api(&api, call)?)))
        .collect()
}

fn weak_python_api(api: &str, call: AstNode<'_>) -> Option<&'static str> {
    if let Some(function) = api.strip_prefix("hashlib.") {
        return weak_hashlib(function, call);
    }
    if let Some(module) = api
        .strip_prefix("Crypto.")
        .or_else(|| api.strip_prefix("Cryptodome."))
    {
        return weak_pycryptodome(module.strip_suffix(".new")?);
    }
    weak_cryptography_package(api.strip_prefix("cryptography.")?)
}

/// `hashlib.md5(..)`, `hashlib.new('sha1', ..)`, unless the call declares
/// `usedforsecurity=False`.
fn weak_hashlib(function: &str, call: AstNode<'_>) -> Option<&'static str> {
    if keyword_argument(call, "usedforsecurity").is_some_and(|value| value.text() == "False") {
        return None;
    }
    let name = match function {
        "new" => positional_arguments(call)
            .first()
            .and_then(|name| py_string(*name))?
            .to_ascii_lowercase(),
        other => other.to_string(),
    };
    match name.as_str() {
        "md4" | "md5" => Some("MD5"),
        "sha" | "sha1" => Some("SHA-1"),
        _ => None,
    }
}

/// A PyCryptodome `Crypto.Hash` or `Crypto.Cipher` module's `new`.
fn weak_pycryptodome(module: &str) -> Option<&'static str> {
    match module {
        "Hash.MD2" | "Hash.MD4" | "Hash.MD5" => Some("MD5"),
        "Hash.SHA" | "Hash.SHA1" => Some("SHA-1"),
        "Cipher.DES" | "Cipher.DES3" => Some("DES"),
        "Cipher.ARC2" | "Cipher.ARC4" => Some("RC4"),
        "Cipher.Blowfish" => Some("Blowfish"),
        _ => None,
    }
}

/// A `cryptography` hash, cipher algorithm, or mode.
fn weak_cryptography_package(path: &str) -> Option<&'static str> {
    let (owner, name) = path.rsplit_once('.')?;
    let family = owner.rsplit('.').next()?;
    match (family, name) {
        ("hashes", "MD5") => Some("MD5"),
        ("hashes", "SHA1") => Some("SHA-1"),
        ("modes", "ECB") => Some("ECB mode"),
        ("algorithms", "ARC4") => Some("RC4"),
        ("algorithms", "TripleDES") => Some("DES"),
        ("algorithms", "Blowfish") => Some("Blowfish"),
        _ => None,
    }
}

/// `sites` without those in program code: an entry script (`__main__.py`
/// or a `#!` file), an `if __name__ == "__main__":` block, or a function a
/// `pyproject.toml` script entry point names. Exiting and printing are a
/// program's interface, as for Go `package main`.
fn outside_program_code<'a>(
    file: &AuditedFile,
    root: AstNode<'a>,
    sites: Vec<AstNode<'a>>,
) -> Vec<AstNode<'a>> {
    let name = file.relative.file_name().and_then(|name| name.to_str());
    if sites.is_empty() || name == Some("__main__.py") || root.text().starts_with("#!") {
        return Vec::new();
    }
    let entries = entry_functions(file);
    let program = root
        .named_children()
        .filter(|statement| is_main_guard(*statement))
        .chain(
            ast_scan::nodes_of_kind(root, &["function_definition"])
                .into_iter()
                .filter(|function| {
                    is_module_level(*function)
                        && function
                            .child_by_field_name("name")
                            .is_some_and(|name| entries.iter().any(|entry| entry == name.text()))
                }),
        )
        .map(|node| node.start_byte()..node.end_byte())
        .collect::<Vec<_>>();
    sites
        .into_iter()
        .filter(|site| !program.iter().any(|span| span.contains(&site.start_byte())))
        .collect()
}

fn is_main_guard(statement: AstNode<'_>) -> bool {
    statement.kind() == "if_statement"
        && statement
            .child_by_field_name("condition")
            .is_some_and(is_main_condition)
}

/// `__name__ == "__main__"`, alone or as an operand of `and`.
fn is_main_condition(condition: AstNode<'_>) -> bool {
    match condition.kind() {
        "boolean_operator" => {
            condition
                .child_by_field_name("operator")
                .is_some_and(|operator| operator.text() == "and")
                && condition.named_children().any(is_main_condition)
        }
        "parenthesized_expression" => condition.named_children().any(is_main_condition),
        _ => {
            let compact = condition
                .text()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .replace('\'', "\"");
            matches!(
                compact.as_str(),
                "__name__==\"__main__\"" | "\"__main__\"==__name__"
            )
        }
    }
}

fn is_module_level(function: AstNode<'_>) -> bool {
    std::iter::successors(function.parent(), |parent| parent.parent())
        .all(|ancestor| !matches!(ancestor.kind(), "function_definition" | "class_definition"))
}

/// Names of the functions in `file` that a `[project.scripts]`,
/// `[project.gui-scripts]`, or `[tool.poetry.scripts]` entry names, in the
/// entry module itself or in the module it imports that function from.
fn entry_functions(file: &AuditedFile) -> Vec<String> {
    let Some(project) = nearest_pyproject(file) else {
        return Vec::new();
    };
    let Ok(manifest) = std::fs::read_to_string(project.join("pyproject.toml")) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for (module, function) in script_entries(&manifest) {
        let Some(entry) = module_file(&project, &module) else {
            continue;
        };
        if entry == file.absolute
            || imported_from(&entry, &function, &project).as_deref()
                == Some(file.absolute.as_path())
        {
            names.push(function);
        }
    }
    names
}

fn nearest_pyproject(file: &AuditedFile) -> Option<PathBuf> {
    let depth = file.relative.components().count();
    file.absolute
        .ancestors()
        .skip(1)
        .take(depth)
        .find(|directory| directory.join("pyproject.toml").is_file())
        .map(Path::to_path_buf)
}

/// `(module, function)` of each script entry point (`httpx = "httpx:main"`).
fn script_entries(manifest: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut in_scripts = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_scripts = matches!(
                line,
                "[project.scripts]" | "[project.gui-scripts]" | "[tool.poetry.scripts]"
            );
            continue;
        }
        let Some((_, value)) = line.split_once('=').filter(|_| in_scripts) else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        if let Some((module, function)) = value.split_once(':') {
            let function = function.split('.').next().unwrap_or_default().trim();
            entries.push((module.trim().to_string(), function.to_string()));
        }
    }
    entries
}

/// The source file of a dotted module under a project root or its `src`.
fn module_file(project: &Path, module: &str) -> Option<PathBuf> {
    let relative = module.replace('.', "/");
    [project.to_path_buf(), project.join("src")]
        .into_iter()
        .flat_map(|base| {
            [
                base.join(format!("{relative}.py")),
                base.join(&relative).join("__init__.py"),
            ]
        })
        .find(|candidate| candidate.is_file())
}

/// The module file `entry` imports `function` from
/// (`from ._main import main`).
fn imported_from(entry: &Path, function: &str, project: &Path) -> Option<PathBuf> {
    let source = std::fs::read_to_string(entry).ok()?;
    source.lines().find_map(|line| {
        let (module, names) = line.trim().strip_prefix("from ")?.split_once(" import ")?;
        names
            .split([',', '(', ')'])
            .map(|name| name.split(" as ").next().unwrap_or_default().trim())
            .any(|name| name == function)
            .then_some(())?;
        let module = module.trim();
        let dots = module
            .chars()
            .take_while(|character| *character == '.')
            .count();
        if dots == 0 {
            return module_file(project, module);
        }
        let package = entry.ancestors().nth(dots)?;
        let rest = module[dots..].replace('.', "/");
        [
            package.join(format!("{rest}.py")),
            package.join(&rest).join("__init__.py"),
        ]
        .into_iter()
        .find(|candidate| !rest.is_empty() && candidate.is_file())
    })
}
