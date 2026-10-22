//! Boundaries for the dependency and error hygiene rules
//! `unused_declared_dependency` and `discarded_fallible_result` in each
//! Tier-1 language. Positives hold only sites the detector must report;
//! negatives hold the nearest used, conventional, or handled forms. A
//! dependency boundary lists its source file first, so the case's native
//! language is the source language rather than the manifest's.

use std::collections::BTreeMap;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

pub(crate) fn hygiene_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let files: &[(&str, &str)] = match (detector, language, positive) {
        ("unused_declared_dependency", "rust", true) => RUST_DEPENDENCIES,
        ("unused_declared_dependency", "rust", false) => RUST_DEPENDENCIES_USED,
        ("unused_declared_dependency", "python", true) => PY_DEPENDENCIES,
        ("unused_declared_dependency", "python", false) => PY_DEPENDENCIES_USED,
        ("unused_declared_dependency", "javascript", true) => JS_DEPENDENCIES,
        ("unused_declared_dependency", "javascript", false) => JS_DEPENDENCIES_USED,
        ("unused_declared_dependency", "typescript", true) => TS_DEPENDENCIES,
        ("unused_declared_dependency", "typescript", false) => TS_DEPENDENCIES_USED,
        ("unused_declared_dependency", "go", true) => GO_DEPENDENCIES,
        ("unused_declared_dependency", "go", false) => GO_DEPENDENCIES_USED,
        ("unused_declared_dependency", "java", true) => JAVA_DEPENDENCIES,
        ("unused_declared_dependency", "java", false) => JAVA_DEPENDENCIES_USED,
        ("discarded_fallible_result", "rust", true) => &[("src/store.rs", RUST_DISCARD)],
        ("discarded_fallible_result", "rust", false) => &[("src/store.rs", RUST_DISCARD_SAFE)],
        ("discarded_fallible_result", "python", true) => &[("src/worker.py", PY_DISCARD)],
        ("discarded_fallible_result", "python", false) => &[("src/worker.py", PY_DISCARD_SAFE)],
        ("discarded_fallible_result", "javascript", true) => &[("src/store.js", JS_DISCARD)],
        ("discarded_fallible_result", "javascript", false) => &[("src/store.js", JS_DISCARD_SAFE)],
        ("discarded_fallible_result", "typescript", true) => &[("src/store.ts", JS_DISCARD)],
        ("discarded_fallible_result", "typescript", false) => &[("src/store.ts", JS_DISCARD_SAFE)],
        ("discarded_fallible_result", "go", true) => &[("store/store.go", GO_DISCARD)],
        ("discarded_fallible_result", "go", false) => &[("store/store.go", GO_DISCARD_SAFE)],
        ("discarded_fallible_result", "java", true) => {
            &[("src/main/java/Store.java", JAVA_DISCARD)]
        }
        ("discarded_fallible_result", "java", false) => {
            &[("src/main/java/Store.java", JAVA_DISCARD_SAFE)]
        }
        _ => return None,
    };
    Some(
        files
            .iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

const RUST_DEPENDENCIES: &[(&str, &str)] = &[
    (
        "src/main.rs",
        "use anyhow::Result;\n\nfn main() -> Result<()> {\n    let value = serde_json::json!({});\n    println!(\"{value}\");\n    Ok(())\n}\n",
    ),
    (
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1\"\nserde_json = \"1\"\nleft-pad = \"1\"\nopenssl-sys = \"0.9\"\n\n[dev-dependencies]\ntempfile = \"3\"\n",
    ),
];

const RUST_DEPENDENCIES_USED: &[(&str, &str)] = &[
    (
        "src/main.rs",
        "use anyhow::Result;\n\nfn main() -> Result<()> {\n    let value = serde_json::json!({});\n    println!(\"{value}\");\n    Ok(())\n}\n",
    ),
    ("tests/smoke.rs", "use log::info;\n\n#[test]\nfn logs() {\n    info!(\"ok\");\n}\n"),
    (
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nanyhow = \"1\"\nserde_json = \"1\"\nlog = \"0.4\"\nopenssl-sys = \"0.9\"\nfeature-only = \"1\"\n\n[package.metadata.cargo-machete]\nignored = [\"feature-only\"]\n",
    ),
];

const PY_DEPENDENCIES: &[(&str, &str)] = &[
    ("src/app/main.py", "import requests\nimport yaml\n\n\ndef load(url):\n    return yaml.safe_load(requests.get(url, timeout=5).text)\n"),
    (
        "pyproject.toml",
        "[project]\nname = \"app\"\ndependencies = [\n  \"PyYAML>=6\",\n  \"requests\",\n  \"colorama\",\n]\n\n[project.optional-dependencies]\ntest = [\"pytest\"]\n",
    ),
];

const PY_DEPENDENCIES_USED: &[(&str, &str)] = &[
    ("src/app/main.py", "import yaml\nfrom dateutil import parser\n\n\ndef load(text):\n    return parser.parse(yaml.safe_load(text))\n"),
    (
        "pyproject.toml",
        "[project]\nname = \"app\"\ndependencies = [\"PyYAML\", \"python-dateutil>=2\", \"gunicorn\"]\n",
    ),
];

const JS_DEPENDENCIES: &[(&str, &str)] = &[
    ("src/index.js", "const express = require('express');\n\nmodule.exports = express();\n"),
    (
        "package.json",
        "{\n  \"name\": \"app\",\n  \"dependencies\": {\n    \"express\": \"4\",\n    \"left-pad\": \"1\",\n    \"@types/node\": \"20\"\n  }\n}\n",
    ),
];

const JS_DEPENDENCIES_USED: &[(&str, &str)] = &[
    ("src/index.js", "const express = require('express');\nconst fp = require('lodash/fp');\n\nmodule.exports = fp.identity(express());\n"),
    (
        "package.json",
        "{\n  \"name\": \"app\",\n  \"scripts\": {\"clean\": \"rimraf dist\"},\n  \"dependencies\": {\n    \"express\": \"4\",\n    \"lodash\": \"4\",\n    \"rimraf\": \"5\",\n    \"eslint-plugin-react\": \"7\",\n    \"tslib\": \"2\"\n  }\n}\n",
    ),
];

const TS_DEPENDENCIES: &[(&str, &str)] = &[
    ("src/index.ts", "import express from 'express';\n\nexport const app = express();\n"),
    (
        "package.json",
        "{\n  \"name\": \"app\",\n  \"dependencies\": {\n    \"express\": \"4\",\n    \"left-pad\": \"1\"\n  }\n}\n",
    ),
];

const TS_DEPENDENCIES_USED: &[(&str, &str)] = &[
    ("src/index.ts", "import express from 'express';\n\nexport const app = express();\n"),
    ("src/view.tsx", "export const View = () => <div />;\n"),
    (
        "package.json",
        "{\n  \"name\": \"app\",\n  \"scripts\": {\"build\": \"tsc -p .\"},\n  \"dependencies\": {\n    \"express\": \"4\",\n    \"react\": \"18\",\n    \"typescript\": \"5\"\n  }\n}\n",
    ),
];

const GO_DEPENDENCIES: &[(&str, &str)] = &[
    ("main.go", "package main\n\nimport \"github.com/spf13/cobra\"\n\nfunc main() {\n\t_ = cobra.Command{}\n}\n"),
    (
        "go.mod",
        "module example.com/app\n\ngo 1.24\n\nrequire (\n\tgithub.com/spf13/cobra v1.8.0\n\tgithub.com/pkg/errors v0.9.1\n\tgithub.com/inconshreveable/mousetrap v1.1.0 // indirect\n)\n",
    ),
];

const GO_DEPENDENCIES_USED: &[(&str, &str)] = &[
    ("main.go", "package main\n\nimport \"github.com/spf13/cobra/doc\"\n\nfunc main() {\n\t_ = doc.GenMarkdown\n}\n"),
    (
        "go.mod",
        "module example.com/app\n\ngo 1.24\n\nrequire (\n\tgithub.com/spf13/cobra v1.8.0\n\tgolang.org/x/tools v0.30.0\n\tgithub.com/inconshreveable/mousetrap v1.1.0 // indirect\n)\n\ntool golang.org/x/tools/cmd/stringer\n",
    ),
];

const JAVA_DEPENDENCIES: &[(&str, &str)] = &[
    (
        "src/main/java/App.java",
        "import com.google.common.collect.ImmutableList;\n\nclass App {\n    ImmutableList<String> names() {\n        return ImmutableList.of();\n    }\n}\n",
    ),
    (
        "pom.xml",
        "<project>\n  <dependencies>\n    <dependency>\n      <groupId>com.google.guava</groupId>\n      <artifactId>guava</artifactId>\n    </dependency>\n    <dependency>\n      <groupId>org.jsoup</groupId>\n      <artifactId>jsoup</artifactId>\n    </dependency>\n    <dependency>\n      <groupId>org.junit.jupiter</groupId>\n      <artifactId>junit-jupiter</artifactId>\n      <scope>test</scope>\n    </dependency>\n  </dependencies>\n</project>\n",
    ),
];

const JAVA_DEPENDENCIES_USED: &[(&str, &str)] = &[
    (
        "src/main/java/App.java",
        "import com.google.common.collect.ImmutableList;\nimport lombok.Data;\n\n@Data\nclass App {\n    ImmutableList<String> names;\n}\n",
    ),
    (
        "pom.xml",
        "<project>\n  <dependencies>\n    <dependency>\n      <groupId>com.google.guava</groupId>\n      <artifactId>guava</artifactId>\n    </dependency>\n    <dependency>\n      <groupId>org.projectlombok</groupId>\n      <artifactId>lombok</artifactId>\n    </dependency>\n    <dependency>\n      <groupId>org.postgresql</groupId>\n      <artifactId>postgresql</artifactId>\n    </dependency>\n    <dependency>\n      <groupId>org.springframework.boot</groupId>\n      <artifactId>spring-boot-starter-web</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n",
    ),
];

const RUST_DISCARD: &str = r#"use std::fs;
use std::io::Write;

pub fn save(path: &str, file: &mut fs::File, bytes: &[u8]) {
    let _ = file.write_all(bytes);
    _ = fs::rename(path, "done");
    file.sync_all().ok();
}
"#;

const RUST_DISCARD_SAFE: &str = r#"use std::fs;
use std::io::Write;

pub fn save(file: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes)?;
    let _ = std::io::stdout().flush();
    let _ = fs::remove_file("scratch");
    let _ = sender().send(1);
    Ok(())
}

pub fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = buffer.write_all(bytes);
    // Ignore flush errors: the process is exiting.
    let _ = output().flush();
    buffer
}

struct Guard(fs::File);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.sync_all();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn flushes() {
        let _ = super::open().flush();
    }
}
"#;

const PY_DISCARD: &str = r#"import asyncio


async def refresh():
    await asyncio.sleep(0)


class Worker:
    async def flush(self):
        pass

    async def run(self):
        refresh()
        self.flush()
        asyncio.create_task(refresh())
        asyncio.sleep(1)
"#;

const PY_DISCARD_SAFE: &str = r#"import asyncio


async def refresh():
    await asyncio.sleep(0)


def compute():
    return 1


@sync_wrapper
async def wrapped():
    pass


class Worker:
    def flush(self):
        pass

    async def run(self):
        await refresh()
        compute()
        wrapped()
        self.flush()
        task = asyncio.create_task(refresh())
        await task
        async with asyncio.TaskGroup() as group:
            group.create_task(refresh())
"#;

const JS_DISCARD: &str = r#"async function load() {
  return 1;
}

const save = async () => {};

export class Store {
  async persist() {}

  run() {
    load();
    save();
    this.persist();
    fetchData().then((value) => console.log(value));
  }
}
"#;

const JS_DISCARD_SAFE: &str = r#"async function load() {
  return 1;
}

function compute() {
  return 2;
}

const guarded = async () => {
  try {
    await load();
  } catch (error) {
    report(error);
  }
};

load();

export class Store {
  async persist() {}

  async run() {
    await load();
    void this.persist();
    compute();
    guarded();
    Promise.resolve().then(() => compute());
    load().catch(() => {});
    fetchData().then(onValue, onError);
    return load();
  }
}
"#;

const GO_DISCARD: &str = r#"package store

import (
	"database/sql"
	"os"
)

func Save(f *os.File, tx *sql.Tx, data []byte) {
	_ = f.Sync()
	_ = tx.Commit()
	_ = os.Rename("a", "b")
	_ = os.WriteFile("p", data, 0o644)
}
"#;

const GO_DISCARD_SAFE: &str = r#"package store

import "os"

func Save(f *os.File, logger *Logger, cache *Cache) error {
	_ = logger.Sync()
	cache.Sync()
	if err := f.Sync(); err != nil {
		return err
	}
	return os.Rename("a", "b")
}
"#;

const JAVA_DISCARD: &str = r#"import java.io.File;

class Store {
    void save(File dir, File target) {
        dir.mkdirs();
        new File("scratch").delete();
        target.renameTo(new File("done"));
    }
}
"#;

const JAVA_DISCARD_SAFE: &str = r#"import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.Map;

class Store {
    boolean save(File dir, Map<String, String> cache, StringBuilder text, Record record)
            throws IOException {
        if (!dir.mkdirs()) {
            throw new IOException("mkdirs");
        }
        cache.remove("key");
        text.delete(0, 1);
        record.delete();
        Files.delete(dir.toPath());
        return dir.renameTo(new File("done"));
    }
}
"#;

/// A detector, a language, and what its positive reports (the backticked
/// subject of each message, with counts).
type Boundary = (&'static str, &'static str, &'static [(&'static str, usize)]);

#[test]
fn hygiene_boundaries_report_exactly_the_unused_and_discarded_sites() {
    const LEFT_PAD: &[(&str, usize)] = &[("left-pad", 1)];
    const PROMISES: &[(&str, usize)] = &[
        ("load", 1),
        ("save", 1),
        ("this.persist", 1),
        ("fetchData().then", 1),
    ];
    let expected: &[Boundary] = &[
        ("unused_declared_dependency", "rust", LEFT_PAD),
        ("unused_declared_dependency", "python", &[("colorama", 1)]),
        ("unused_declared_dependency", "javascript", LEFT_PAD),
        ("unused_declared_dependency", "typescript", LEFT_PAD),
        (
            "unused_declared_dependency",
            "go",
            &[("github.com/pkg/errors", 1)],
        ),
        (
            "unused_declared_dependency",
            "java",
            &[("org.jsoup:jsoup", 1)],
        ),
        (
            "discarded_fallible_result",
            "rust",
            &[
                ("file.write_all", 1),
                ("fs::rename", 1),
                ("file.sync_all", 1),
            ],
        ),
        (
            "discarded_fallible_result",
            "python",
            &[
                ("refresh", 1),
                ("self.flush", 1),
                ("asyncio.create_task", 1),
                ("asyncio.sleep", 1),
            ],
        ),
        ("discarded_fallible_result", "javascript", PROMISES),
        ("discarded_fallible_result", "typescript", PROMISES),
        (
            "discarded_fallible_result",
            "go",
            &[
                ("f.Sync", 1),
                ("tx.Commit", 1),
                ("os.Rename", 1),
                ("os.WriteFile", 1),
            ],
        ),
        (
            "discarded_fallible_result",
            "java",
            &[
                ("dir.mkdirs", 1),
                ("new File(\"scratch\").delete", 1),
                ("target.renameTo", 1),
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (detector, language, subjects) in expected {
        for positive in [true, false] {
            let files = hygiene_capability_files(detector, language, positive).unwrap();
            let mut case = CapabilityCase::new(detector, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let mut reported = BTreeMap::<String, usize>::new();
            for finding in analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == *detector)
            {
                let subject = finding
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                *reported.entry(subject).or_default() += 1;
            }
            let wanted = if positive {
                subjects
                    .iter()
                    .map(|(subject, count)| (subject.to_string(), *count))
                    .collect()
            } else {
                BTreeMap::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{detector} {language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
