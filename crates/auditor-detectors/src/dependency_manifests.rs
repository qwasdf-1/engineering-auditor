//! The dependencies a package manifest declares for its production code:
//! `Cargo.toml` `[dependencies]`, `package.json` `dependencies`, `go.mod`
//! direct requires, `pyproject.toml` project and Poetry dependencies, and
//! `pom.xml` compile-scope dependencies. Development, build, peer, test,
//! provided, and runtime sections are not production imports and are not
//! read. Manifests are read line by line; a construct the reader does not
//! model declares nothing.

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Ecosystem {
    Cargo,
    Npm,
    Go,
    Python,
    Maven,
}

impl Ecosystem {
    pub(crate) const ALL: [Self; 5] = [Self::Cargo, Self::Npm, Self::Go, Self::Python, Self::Maven];

    pub(crate) fn manifest(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo.toml",
            Self::Npm => "package.json",
            Self::Go => "go.mod",
            Self::Python => "pyproject.toml",
            Self::Maven => "pom.xml",
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Go => "go",
            Self::Python => "python",
            Self::Maven => "maven",
        }
    }
}

/// One declared dependency: its name as the manifest writes it (a Maven
/// dependency is `groupId:artifactId`) and the manifest line declaring it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) name: String,
    pub(crate) line: usize,
}

pub(crate) fn declared_dependencies(ecosystem: Ecosystem, manifest: &str) -> Vec<Declared> {
    match ecosystem {
        Ecosystem::Cargo => cargo_dependencies(manifest),
        Ecosystem::Npm => npm_dependencies(manifest),
        Ecosystem::Go => go_requires(manifest),
        Ecosystem::Python => python_dependencies(manifest),
        Ecosystem::Maven => maven_dependencies(manifest),
    }
}

/// `[dependencies]` keys of a package manifest, target-specific tables
/// included; the key is the name code imports, even for a renamed package.
/// A virtual workspace declares nothing, and names a `cargo-machete` or
/// `cargo-udeps` ignore list holds are skipped.
fn cargo_dependencies(manifest: &str) -> Vec<Declared> {
    if !manifest.lines().any(|line| line.trim() == "[package]") {
        return Vec::new();
    }
    let ignored = cargo_ignored(manifest);
    let mut declared = Vec::new();
    let mut in_table = false;
    let mut depth = 0i32;
    for (index, raw) in manifest.lines().enumerate() {
        let line = strip_toml_comment(raw).trim();
        if depth == 0 && line.starts_with('[') {
            let header = line.trim_matches(['[', ']']).replace(' ', "");
            let (table, single) = cargo_dependency_header(&header);
            in_table = table;
            if let Some(name) = single {
                declared.push(Declared {
                    name,
                    line: index + 1,
                });
            }
            continue;
        }
        if in_table && depth == 0 {
            if let Some(name) = toml_key(line) {
                declared.push(Declared {
                    name: name.split('.').next().unwrap_or(&name).to_string(),
                    line: index + 1,
                });
            }
        }
        depth += bracket_balance(line);
    }
    declared.retain(|dependency| !ignored.contains(&dependency.name));
    declared
}

/// Whether a table header opens a dependency table, and the dependency a
/// `[dependencies.name]` header declares by itself.
fn cargo_dependency_header(header: &str) -> (bool, Option<String>) {
    let body = header
        .strip_prefix("target.")
        .and_then(|rest| rest.rsplit_once(".dependencies").map(|(_, tail)| tail))
        .map(|tail| format!("dependencies{tail}"))
        .unwrap_or_else(|| header.to_string());
    if body == "dependencies" {
        return (true, None);
    }
    let single = body
        .strip_prefix("dependencies.")
        .map(|name| name.trim_matches('"').to_string());
    (false, single)
}

fn cargo_ignored(manifest: &str) -> BTreeSet<String> {
    let mut ignored = BTreeSet::new();
    let mut section = String::new();
    let mut collecting = false;
    for raw in manifest.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.starts_with('[') && !collecting {
            section = line.trim_matches(['[', ']']).replace(' ', "");
            continue;
        }
        let opens = matches!(
            (section.as_str(), toml_key(line).as_deref()),
            ("package.metadata.cargo-machete", Some("ignored"))
                | ("package.metadata.cargo-udeps.ignore", Some("normal"))
        );
        if opens || collecting {
            ignored.extend(quoted_strings(line));
            collecting = !line.contains(']');
        }
    }
    ignored
}

/// `dependencies` keys of a published `package.json`, each at the line that
/// names it inside that object.
fn npm_dependencies(manifest: &str) -> Vec<Declared> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(manifest) else {
        return Vec::new();
    };
    // A private package (a workspace root, a documentation site) is never
    // installed by a consumer, and its framework loads packages by
    // convention.
    if value.get("private").and_then(|private| private.as_bool()) == Some(true) {
        return Vec::new();
    }
    let Some(dependencies) = value
        .get("dependencies")
        .and_then(|value| value.as_object())
    else {
        return Vec::new();
    };
    let lines = manifest.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim_start().starts_with("\"dependencies\""))
        .unwrap_or(0);
    dependencies
        .keys()
        .map(|name| {
            let key = format!("\"{name}\"");
            let line = lines
                .iter()
                .enumerate()
                .skip(start)
                .find(|(_, line)| line.trim_start().starts_with(&key))
                .map_or(start + 1, |(index, _)| index + 1);
            Declared {
                name: name.clone(),
                line,
            }
        })
        .collect()
}

/// Direct `require` entries of `go.mod`; `// indirect` ones are only there
/// for the build list.
fn go_requires(manifest: &str) -> Vec<Declared> {
    go_directive(manifest, "require")
        .filter(|(_, entry)| !entry.contains("// indirect"))
        .filter_map(|(line, entry)| {
            Some(Declared {
                name: entry
                    .split_whitespace()
                    .next()?
                    .trim_matches('"')
                    .to_string(),
                line,
            })
        })
        .collect()
}

/// The paths a `go.mod` `tool` directive names: used by `go tool`, not by
/// an import.
pub(crate) fn go_tools(manifest: &str) -> Vec<String> {
    go_directive(manifest, "tool")
        .filter_map(|(_, entry)| entry.split_whitespace().next().map(str::to_string))
        .collect()
}

/// `(line, entry)` for each entry of a `go.mod` directive, in its
/// single-line (`require a v1`) or block (`require ( .. )`) form.
fn go_directive<'a>(
    manifest: &'a str,
    directive: &'a str,
) -> impl Iterator<Item = (usize, &'a str)> + 'a {
    let mut in_block = false;
    manifest
        .lines()
        .enumerate()
        .filter_map(move |(index, raw)| {
            let line = raw.trim();
            let entry = if in_block {
                in_block = !line.starts_with(')');
                line
            } else {
                let rest = line.strip_prefix(directive)?;
                if !rest.starts_with([' ', '\t', '(']) {
                    return None;
                }
                in_block = rest.trim_start().starts_with('(');
                rest.trim()
            };
            let empty =
                entry.is_empty() || entry.starts_with(['(', ')']) || entry.starts_with("//");
            (!empty).then_some((index + 1, entry))
        })
}

/// PEP 621 `[project] dependencies` and Poetry
/// `[tool.poetry.dependencies]`, as normalized distribution names. A
/// project whose dependencies are `dynamic` declares nothing here.
fn python_dependencies(manifest: &str) -> Vec<Declared> {
    let mut declared = Vec::new();
    let mut section = String::new();
    let mut in_list = false;
    for (index, raw) in manifest.lines().enumerate() {
        let line = strip_toml_comment(raw).trim();
        if !in_list && line.starts_with('[') {
            section = line.trim_matches(['[', ']']).replace(' ', "");
            continue;
        }
        let key = toml_key(line);
        if in_list || (section == "project" && key.as_deref() == Some("dependencies")) {
            in_list = !line.contains(']');
            declared.extend(requirement_names(line, index));
        } else if section == "tool.poetry.dependencies" {
            declared.extend(key.filter(|name| name != "python").map(|name| Declared {
                name: normalized_distribution(&name),
                line: index + 1,
            }));
        }
    }
    declared
}

fn requirement_names(line: &str, index: usize) -> impl Iterator<Item = Declared> + '_ {
    let values = line.split_once('=').map_or(line, |(key, value)| {
        if key.contains(['"', '\'']) {
            line
        } else {
            value
        }
    });
    quoted_strings(values)
        .into_iter()
        .filter_map(move |requirement| {
            let name = requirement
                .split(|character: char| "[<>=!~;@ (".contains(character))
                .next()?
                .trim();
            (!name.is_empty()).then(|| Declared {
                name: normalized_distribution(name),
                line: index + 1,
            })
        })
}

/// PEP 503 normalization: lower case, runs of `-`, `_`, `.` as one `-`.
pub(crate) fn normalized_distribution(name: &str) -> String {
    let mut normalized = String::with_capacity(name.len());
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalized
}

/// Compile-scope `<dependency>` entries directly under `<dependencies>`,
/// outside `<dependencyManagement>`, plugins, and profiles. An aggregator
/// (`<packaging>pom</packaging>`) passes its dependencies to its modules
/// and declares nothing it imports itself.
fn maven_dependencies(manifest: &str) -> Vec<Declared> {
    if manifest.contains("<packaging>pom</packaging>") {
        return Vec::new();
    }
    let mut declared = Vec::new();
    let mut skipped = 0usize;
    let mut current: Option<MavenDependency> = None;
    for (index, raw) in manifest.lines().enumerate() {
        let line = raw.trim();
        skipped = maven_skip_depth(line, skipped);
        if skipped > 0 {
            continue;
        }
        if line.starts_with("<dependency>") {
            current = Some(MavenDependency::new(index + 1));
        }
        if let Some(dependency) = current.as_mut() {
            dependency.read(line);
        }
        if line.contains("</dependency>") {
            declared.extend(current.take().and_then(MavenDependency::declared));
        }
    }
    declared
}

/// The nesting of elements whose dependencies are not the module's own,
/// after `line`.
fn maven_skip_depth(line: &str, depth: usize) -> usize {
    ["dependencyManagement", "plugins", "profiles", "exclusions"]
        .iter()
        .fold(depth, |depth, element| {
            let opened = usize::from(line.contains(&format!("<{element}>")));
            let closed = usize::from(line.contains(&format!("</{element}>")));
            (depth + opened).saturating_sub(closed)
        })
}

struct MavenDependency {
    line: usize,
    group: Option<String>,
    artifact: Option<String>,
    scope: Option<String>,
    kind: Option<String>,
}

impl MavenDependency {
    fn new(line: usize) -> Self {
        Self {
            line,
            group: None,
            artifact: None,
            scope: None,
            kind: None,
        }
    }

    fn read(&mut self, line: &str) {
        let element = |name: &str| {
            line.split_once(&format!("<{name}>"))
                .and_then(|(_, rest)| rest.split_once(&format!("</{name}>")))
                .map(|(value, _)| value.trim().to_string())
        };
        self.group = self.group.take().or_else(|| element("groupId"));
        self.artifact = self.artifact.take().or_else(|| element("artifactId"));
        self.scope = self.scope.take().or_else(|| element("scope"));
        self.kind = self.kind.take().or_else(|| element("type"));
    }

    fn declared(self) -> Option<Declared> {
        let compile = self.scope.as_deref().is_none_or(|scope| scope == "compile");
        let jar = self.kind.as_deref().is_none_or(|kind| kind == "jar");
        let group = self.group?;
        let artifact = self.artifact?;
        (compile && jar && !group.contains("${") && !artifact.contains("${")).then(|| Declared {
            name: format!("{group}:{artifact}"),
            line: self.line,
        })
    }
}

fn strip_toml_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), _) if character == open => quote = None,
            (None, '#') => return &line[..index],
            _ => {}
        }
    }
    line
}

/// The bare or quoted key of a `key = value` line.
fn toml_key(line: &str) -> Option<String> {
    let (key, _) = line.split_once('=')?;
    let key = key.trim().trim_matches(['"', '\'']);
    let valid = !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character));
    valid.then(|| key.to_string())
}

/// Brackets and braces a line opens minus those it closes, outside strings.
fn bracket_balance(line: &str) -> i32 {
    let mut balance = 0;
    let mut quote = None;
    for character in line.chars() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), _) if character == open => quote = None,
            (None, '[' | '{') => balance += 1,
            (None, ']' | '}') => balance -= 1,
            _ => {}
        }
    }
    balance
}

fn quoted_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut current: Option<(char, String)> = None;
    for character in text.chars() {
        match current.as_mut() {
            Some((open, value)) if character == *open => {
                strings.push(std::mem::take(value));
                current = None;
            }
            Some((_, value)) => value.push(character),
            None if matches!(character, '"' | '\'') => current = Some((character, String::new())),
            None => {}
        }
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(ecosystem: Ecosystem, manifest: &str) -> Vec<(String, usize)> {
        declared_dependencies(ecosystem, manifest)
            .into_iter()
            .map(|dependency| (dependency.name, dependency.line))
            .collect()
    }

    #[test]
    fn reads_cargo_production_tables() {
        let manifest = r#"[package]
name = "app"

[dependencies]
serde = { version = "1", features = [
    "derive",
] }
regex-lite = "0.1" # comment
tokio.workspace = true

[dependencies.log]
version = "0.4"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
proptest = "1"

[package.metadata.cargo-machete]
ignored = ["libc"]
"#;
        assert_eq!(
            names(Ecosystem::Cargo, manifest),
            [
                ("serde".to_string(), 5),
                ("regex-lite".to_string(), 8),
                ("tokio".to_string(), 9),
                ("log".to_string(), 11),
            ]
        );
        assert!(names(Ecosystem::Cargo, "[workspace]\n[dependencies]\nx = \"1\"\n").is_empty());
    }

    #[test]
    fn reads_npm_go_python_and_maven() {
        let npm = "{\n  \"devDependencies\": {\"jest\": \"1\"},\n  \"dependencies\": {\n    \"left-pad\": \"1\",\n    \"@scope/pkg\": \"2\"\n  }\n}\n";
        assert_eq!(
            names(Ecosystem::Npm, npm),
            [("@scope/pkg".to_string(), 5), ("left-pad".to_string(), 4)]
        );
        let go = "module m\n\nrequire github.com/a/b v1.0.0\nrequire (\n\tgithub.com/c/d v1.2.0\n\tgithub.com/e/f v0.1.0 // indirect\n)\n";
        assert_eq!(
            names(Ecosystem::Go, go),
            [
                ("github.com/a/b".to_string(), 3),
                ("github.com/c/d".to_string(), 5)
            ]
        );
        let python = "[project]\nname = \"x\"\ndependencies = [\n  \"PyYAML>=6\",\n  \"typing_extensions; python_version<'3.11'\",\n]\n[project.optional-dependencies]\ntest = [\"pytest\"]\n[tool.poetry.dependencies]\npython = \"^3.9\"\nrequests = \"^2\"\n";
        assert_eq!(
            names(Ecosystem::Python, python),
            [
                ("pyyaml".to_string(), 4),
                ("typing-extensions".to_string(), 5),
                ("requests".to_string(), 11)
            ]
        );
        let maven = "<project>\n<dependencyManagement><dependencies>\n<dependency><groupId>a</groupId><artifactId>managed</artifactId></dependency>\n</dependencies></dependencyManagement>\n<dependencies>\n<dependency>\n<groupId>com.google.guava</groupId>\n<artifactId>guava</artifactId>\n</dependency>\n<dependency>\n<groupId>org.junit</groupId>\n<artifactId>junit</artifactId>\n<scope>test</scope>\n</dependency>\n</dependencies>\n</project>\n";
        assert_eq!(
            names(Ecosystem::Maven, maven),
            [("com.google.guava:guava".to_string(), 6)]
        );
    }
}
