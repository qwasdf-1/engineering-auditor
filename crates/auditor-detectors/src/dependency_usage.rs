//! `unused_declared_dependency`: a production dependency a manifest declares
//! that no source file under the manifest's directory names. Every source
//! file under the directory counts as a use, tests, examples, and nested
//! packages included, so a dependency is reported only when nothing there
//! imports it. Dependencies that load by convention rather than by import
//! (type packages, compiler helpers, tool plugins, database drivers, servers
//! started from the command line, `-sys` link crates) are not reported.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use auditor_core::{Confidence, Finding, Severity};

use crate::dependency_manifests::{
    declared_dependencies, go_tools, normalized_distribution, Declared, Ecosystem,
};
use crate::{util, AuditedFile, Language};

pub(crate) fn unused_declared_dependency_findings(
    root: &Path,
    production: &[AuditedFile],
    context: &[AuditedFile],
    cache: &util::FileCache,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (ecosystem, directory) in manifest_directories(root, production) {
        let manifest_path = directory.join(ecosystem.manifest());
        let Ok(manifest) = std::fs::read_to_string(root.join(&manifest_path)) else {
            continue;
        };
        let sources = context
            .iter()
            .filter(|file| file.relative.starts_with(&directory) && searched(ecosystem, file))
            .collect::<Vec<_>>();
        let package = Package::new(ecosystem, &manifest, &sources);
        let relative = util::path_slash(&manifest_path);
        // The package's first production source file: it carries the
        // finding's source language, since the manifest is structured data.
        let source = production
            .iter()
            .filter(|file| {
                file.relative.starts_with(&directory) && written_in(ecosystem, &file.language)
            })
            .map(|file| util::path_slash(&file.relative))
            .min()
            .unwrap_or_default();
        for dependency in declared_dependencies(ecosystem, &manifest) {
            let used = package.implicit(&dependency.name)
                || sources
                    .iter()
                    .any(|file| uses(ecosystem, &dependency.name, cache.text(file)));
            if !used {
                let mut finding = finding(ecosystem, &dependency, &relative, &directory);
                finding.locations.push(auditor_core::Location {
                    path: source.clone(),
                    line: None,
                });
                findings.push(finding);
            }
        }
    }
    findings
}

fn finding(
    ecosystem: Ecosystem,
    dependency: &Declared,
    manifest: &str,
    directory: &Path,
) -> Finding {
    let scope = match util::path_slash(directory) {
        directory if directory.is_empty() => "the repository".to_string(),
        directory => format!("`{directory}/`"),
    };
    auditor_core::finding!(
        "unused_declared_dependency",
        "systems_economics",
        "Declared dependency is never imported",
        format!(
            "`{}` is a production dependency in {manifest}, but no source file under {scope} imports it",
            dependency.name
        ),
        Severity::Low,
        Confidence::Medium,
        manifest,
        Some(dependency.line),
    )
    .with_recommendation(
        "Remove the dependency, or move it to the section its use belongs to (development, build, or runtime-only), so builds stop fetching, compiling, and auditing code nothing imports.",
    )
    .with_evidence("dependency", dependency.name.as_str())
    .with_evidence("ecosystem", ecosystem.as_str())
}

/// Every directory holding a manifest above a production source file of the
/// manifest's ecosystem.
fn manifest_directories(root: &Path, production: &[AuditedFile]) -> BTreeSet<(Ecosystem, PathBuf)> {
    let mut checked = BTreeSet::new();
    let mut found = BTreeSet::new();
    for file in production {
        for ecosystem in Ecosystem::ALL {
            if !written_in(ecosystem, &file.language) {
                continue;
            }
            for directory in file.relative.ancestors().skip(1) {
                let key = (ecosystem, directory.to_path_buf());
                if checked.insert(key.clone())
                    && root.join(directory).join(ecosystem.manifest()).is_file()
                {
                    found.insert(key);
                }
            }
        }
    }
    found
}

fn written_in(ecosystem: Ecosystem, language: &Language) -> bool {
    match ecosystem {
        Ecosystem::Cargo => *language == Language::Rust,
        Ecosystem::Npm => matches!(language, Language::JavaScript | Language::TypeScript),
        Ecosystem::Go => *language == Language::Go,
        Ecosystem::Python => *language == Language::Python,
        Ecosystem::Maven => *language == Language::Java,
    }
}

/// Files whose text can show a use: the ecosystem's sources, and for npm
/// the JSON and YAML configuration that names plugins and presets.
/// Manifests and lockfiles list every dependency and show none used.
fn searched(ecosystem: Ecosystem, file: &AuditedFile) -> bool {
    let name = file
        .relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match ecosystem {
        Ecosystem::Npm => {
            matches!(
                file.language,
                Language::JavaScript | Language::TypeScript | Language::Json | Language::Yaml
            ) && !matches!(
                name,
                "package.json" | "package-lock.json" | "npm-shrinkwrap.json" | "pnpm-lock.yaml"
            )
        }
        Ecosystem::Maven => matches!(file.language, Language::Java | Language::Kotlin),
        _ => written_in(ecosystem, &file.language),
    }
}

/// What a package says about itself beyond its source imports.
struct Package {
    ecosystem: Ecosystem,
    /// npm: the manifest's fields other than its dependency lists (scripts,
    /// tool configuration).
    manifest_text: String,
    /// npm: a JSX or TSX file under the package, compiled against React's
    /// runtime without importing it.
    jsx: bool,
    /// Go: the tools a `tool` directive names.
    tools: Vec<String>,
}

impl Package {
    fn new(ecosystem: Ecosystem, manifest: &str, sources: &[&AuditedFile]) -> Self {
        let mut package = Self {
            ecosystem,
            manifest_text: String::new(),
            jsx: false,
            tools: Vec::new(),
        };
        match ecosystem {
            Ecosystem::Npm => {
                package.manifest_text = npm_manifest_text(manifest);
                package.jsx = sources.iter().any(|file| {
                    file.relative
                        .extension()
                        .is_some_and(|extension| extension == "tsx" || extension == "jsx")
                });
            }
            Ecosystem::Go => package.tools = go_tools(manifest),
            _ => {}
        }
        package
    }

    fn implicit(&self, name: &str) -> bool {
        match self.ecosystem {
            Ecosystem::Cargo => name.ends_with("-sys") || name.ends_with("_sys"),
            Ecosystem::Npm => {
                npm_conventional(name)
                    || (self.jsx && matches!(name, "react" | "react-dom" | "preact"))
                    || mentions_word(&self.manifest_text, name)
                    || npm_binary(name)
                        .is_some_and(|binary| mentions_word(&self.manifest_text, binary))
            }
            Ecosystem::Go => self
                .tools
                .iter()
                .any(|tool| tool == name || tool.starts_with(&format!("{name}/"))),
            Ecosystem::Python => PYTHON_RUNTIME_ONLY.contains(&name),
            Ecosystem::Maven => {
                let artifact = name.split_once(':').map_or(name, |(_, artifact)| artifact);
                MAVEN_RUNTIME_ONLY
                    .iter()
                    .any(|marker| artifact.contains(marker))
            }
        }
    }
}

fn npm_manifest_text(manifest: &str) -> String {
    const LISTS: &[&str] = &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
        "peerDependenciesMeta",
        "bundledDependencies",
        "bundleDependencies",
        "overrides",
        "resolutions",
    ];
    serde_json::from_str::<serde_json::Value>(manifest)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|mut object| {
            object.retain(|key, _| !LISTS.contains(&key.as_str()));
            serde_json::Value::Object(object).to_string()
        })
        .unwrap_or_default()
}

/// Packages loaded by a compiler, a type checker, or a tool's naming
/// convention rather than by an import.
fn npm_conventional(name: &str) -> bool {
    const HELPERS: &[&str] = &[
        "tslib",
        "@babel/runtime",
        "@swc/helpers",
        "regenerator-runtime",
        "core-js",
    ];
    const PLUGIN_PREFIXES: &[&str] = &[
        "eslint-plugin-",
        "eslint-config-",
        "babel-plugin-",
        "babel-preset-",
        "prettier-plugin-",
        "postcss-",
        "stylelint-",
    ];
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    name.starts_with("@types/")
        || HELPERS.contains(&name)
        || PLUGIN_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix) || unscoped.starts_with(prefix))
        || (name.starts_with('@') && matches!(unscoped, "eslint-plugin" | "eslint-config"))
}

/// The command a package installs under another name.
fn npm_binary(name: &str) -> Option<&'static str> {
    match name {
        "typescript" => Some("tsc"),
        "@nestjs/cli" => Some("nest"),
        "@angular/cli" => Some("ng"),
        "@vue/cli-service" => Some("vue-cli-service"),
        _ => None,
    }
}

/// Distributions an application loads by URL scheme, entry point, or command
/// line rather than by import.
const PYTHON_RUNTIME_ONLY: &[&str] = &[
    "gunicorn",
    "uvicorn",
    "hypercorn",
    "daphne",
    "psycopg2",
    "psycopg2-binary",
    "psycopg",
    "psycopg-binary",
    "asyncpg",
    "aiosqlite",
    "pymysql",
    "mysqlclient",
    "oracledb",
    "cx-oracle",
    "pyodbc",
    "setuptools",
    "wheel",
    "pip",
    "tzdata",
];

/// Artifacts found through a service loader, a JDBC URL, or a Spring Boot
/// starter's aggregation rather than an import.
const MAVEN_RUNTIME_ONLY: &[&str] = &[
    "starter",
    "-bom",
    "runtime",
    "-impl",
    "logback-classic",
    "slf4j-simple",
    "slf4j-jdk14",
    "slf4j-reload4j",
    "log4j-core",
    "log4j-slf4j",
    "jcl-over-slf4j",
    "jul-to-slf4j",
    "log4j-over-slf4j",
    "postgresql",
    "mysql-connector",
    "mariadb-java-client",
    "ojdbc",
    "sqlite-jdbc",
    "mssql-jdbc",
    "h2",
    "hsqldb",
    "derby",
    "jakarta.el",
    "javax.el",
];

/// Whether `text` names the dependency the way code imports it.
fn uses(ecosystem: Ecosystem, name: &str, text: &str) -> bool {
    match ecosystem {
        Ecosystem::Cargo => rust_uses(&name.replace('-', "_"), text),
        Ecosystem::Npm => ["'", "\"", "`"].iter().any(|quote| {
            text.contains(&format!("{quote}{name}{quote}"))
                || text.contains(&format!("{quote}{name}/"))
        }),
        Ecosystem::Go => {
            text.contains(&format!("\"{name}\"")) || text.contains(&format!("\"{name}/"))
        }
        Ecosystem::Python => python_uses(&python_modules(name), text),
        Ecosystem::Maven => java_uses(name, text),
    }
}

/// A path through the crate (`name::`), `use name`, `extern crate name`, or
/// the crate inside a `use` group (`use {name as alias, ..}`).
fn rust_uses(crate_name: &str, text: &str) -> bool {
    word_positions(text, crate_name).any(|end| {
        let rest = text[end..].trim_start();
        let before = text[..end - crate_name.len()].trim_end();
        let grouped = (before.ends_with('{') || before.ends_with(','))
            && (rest.starts_with(',') || rest.starts_with('}') || rest.starts_with("as "));
        text[end..].starts_with("::")
            || before.ends_with("use")
            || before.ends_with("extern crate")
            || grouped
    })
}

/// The top-level modules a distribution may install: its normalized name, a
/// known mapping, and the names left after common affixes.
fn python_modules(distribution: &str) -> Vec<String> {
    const KNOWN: &[(&str, &str)] = &[
        ("pillow", "PIL"),
        ("pyyaml", "yaml"),
        ("beautifulsoup4", "bs4"),
        ("scikit-learn", "sklearn"),
        ("scikit-image", "skimage"),
        ("python-dateutil", "dateutil"),
        ("attrs", "attr"),
        ("protobuf", "google"),
        ("opencv-python", "cv2"),
        ("opencv-python-headless", "cv2"),
        ("pyjwt", "jwt"),
        ("python-dotenv", "dotenv"),
        ("pyopenssl", "OpenSSL"),
        ("dnspython", "dns"),
        ("pycryptodome", "Crypto"),
        ("pycryptodomex", "Cryptodome"),
        ("pyserial", "serial"),
        ("gitpython", "git"),
        ("markdown-it-py", "markdown_it"),
        ("python-multipart", "multipart"),
        ("msgpack-python", "msgpack"),
        ("pywin32", "win32api"),
        ("pymupdf", "fitz"),
        ("faiss-cpu", "faiss"),
        ("ruamel-yaml", "ruamel"),
        ("email-validator", "email_validator"),
        ("importlib-metadata", "importlib_metadata"),
    ];
    let distribution = normalized_distribution(distribution);
    let mut modules = KNOWN
        .iter()
        .filter(|(known, _)| *known == distribution)
        .map(|(_, module)| module.to_string())
        .collect::<Vec<_>>();
    let underscored = distribution.replace('-', "_");
    modules.push(underscored.clone());
    for affix in ["python_", "py"] {
        if let Some(rest) = underscored
            .strip_prefix(affix)
            .filter(|rest| rest.len() > 2)
        {
            modules.push(rest.to_string());
        }
    }
    for affix in ["_binary", "_python", "_py", "_slim"] {
        if let Some(rest) = underscored.strip_suffix(affix) {
            modules.push(rest.to_string());
        }
    }
    if let Some((first, _)) = underscored.split_once('_') {
        modules.push(first.to_string());
    }
    modules
}

/// An `import` or `from` statement whose top-level module is one of
/// `modules`, or the module named as a string (a plugin or lazy import).
fn python_uses(modules: &[String], text: &str) -> bool {
    let matches = |module: &str| {
        let top = module.split('.').next().unwrap_or(module).trim();
        modules
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(top))
    };
    text.lines().map(str::trim).any(|line| {
        if let Some(rest) = line.strip_prefix("from ") {
            return rest.split_whitespace().next().is_some_and(matches);
        }
        if let Some(rest) = line.strip_prefix("import ") {
            return rest
                .split(',')
                .filter_map(|part| part.split_whitespace().next())
                .any(&matches);
        }
        false
    }) || modules.iter().any(|module| {
        text.contains(&format!("\"{module}\"")) || text.contains(&format!("'{module}'"))
    })
}

/// A package under the group's first two segments (`com.google.`), or an
/// import of the artifact's own first segment (`import lombok.`).
fn java_uses(name: &str, text: &str) -> bool {
    let Some((group, artifact)) = name.split_once(':') else {
        return false;
    };
    let prefix = group.split('.').take(2).collect::<Vec<_>>().join(".");
    let artifact_root = artifact.split(['-', '.']).next().unwrap_or(artifact);
    text.contains(&format!("{prefix}."))
        || (artifact_root.len() > 2
            && (text.contains(&format!("import {artifact_root}."))
                || text.contains(&format!("import static {artifact_root}."))))
}

/// End offsets of `word` in `text` where it is not part of a longer
/// identifier.
fn word_positions<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(word).filter_map(move |(start, _)| {
        let end = start + word.len();
        let identifier = |character: char| character.is_alphanumeric() || character == '_';
        let before = text[..start].chars().next_back().is_some_and(identifier);
        let after = text[end..].chars().next().is_some_and(identifier);
        (!before && !after).then_some(end)
    })
}

/// Whether `text` holds `word` delimited by anything but identifier,
/// `-`, `/`, and `@` characters, as a script command or configured name.
fn mentions_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let end = start + word.len();
        let joined = |character: char| character.is_alphanumeric() || "_-/@.".contains(character);
        !text[..start].chars().next_back().is_some_and(joined)
            && !text[end..].chars().next().is_some_and(joined)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_each_ecosystems_import_spelling() {
        assert!(rust_uses(
            "serde_json",
            "let v = serde_json::to_string(&x);"
        ));
        assert!(rust_uses("anyhow", "use anyhow;"));
        assert!(rust_uses(
            "serde_json",
            "use {\n    serde_json as json,\n};"
        ));
        assert!(rust_uses(
            "lazy_static",
            "#[macro_use]\nextern crate lazy_static;"
        ));
        assert!(!rust_uses("log", "fn log() {} // catalog::x"));
        assert!(uses(Ecosystem::Npm, "lodash", "import x from 'lodash/fp';"));
        assert!(!uses(
            Ecosystem::Npm,
            "lodash",
            "import x from 'lodash-es';"
        ));
        assert!(uses(
            Ecosystem::Go,
            "github.com/a/b",
            "import \"github.com/a/b/c\""
        ));
        assert!(!uses(
            Ecosystem::Go,
            "github.com/a/b",
            "import \"github.com/a/bc\""
        ));
        assert!(uses(Ecosystem::Python, "PyYAML", "import yaml\n"));
        assert!(uses(
            Ecosystem::Python,
            "python-dateutil",
            "from dateutil import parser\n"
        ));
        assert!(uses(
            Ecosystem::Python,
            "typing-extensions",
            "import os, typing_extensions as t\n"
        ));
        assert!(!uses(
            Ecosystem::Python,
            "requests",
            "from .requests import x\n"
        ));
        assert!(uses(
            Ecosystem::Maven,
            "com.google.guava:guava",
            "import com.google.common.collect.X;"
        ));
        assert!(uses(
            Ecosystem::Maven,
            "org.projectlombok:lombok",
            "import lombok.Data;"
        ));
        assert!(!uses(
            Ecosystem::Maven,
            "org.jsoup:jsoup",
            "import java.util.List;"
        ));
    }

    #[test]
    fn npm_conventions_and_scripts_count_as_uses() {
        assert!(npm_conventional("@types/node"));
        assert!(npm_conventional("@typescript-eslint/eslint-plugin"));
        assert!(npm_conventional("eslint-plugin-react"));
        assert!(!npm_conventional("express"));
        assert!(declared_dependencies(
            Ecosystem::Npm,
            r#"{"private":true,"dependencies":{"a":"1"}}"#
        )
        .is_empty());
        let text = npm_manifest_text(
            r#"{"scripts":{"build":"tsc -p . && rimraf dist"},"dependencies":{"chalk":"1"}}"#,
        );
        assert!(mentions_word(&text, "rimraf"));
        assert!(mentions_word(&text, npm_binary("typescript").unwrap()));
        assert!(!mentions_word(&text, "chalk"));
    }
}
