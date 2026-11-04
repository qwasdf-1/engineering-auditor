//! Scans of small fixture repositories, shared by the reviewer tests.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use auditor_detectors::{
    AnalysisMethod, AnalysisRequest, NoGraphProvider, ScanProfile, ScanScope,
    SourceDiscoveryOptions,
};
use auditor_report::{parse_repository_map_reference, ColorMode, NoReviewArtifactLoader};
use auditor_review::{load_and_validate_repository_map, RepositoryMap, RepositoryView};
use auditor_scan::{run_scan, ScanFormat, ScanRequest};
use serde_json::Value;

pub const PYTHON_APP: &[(&str, &str)] = &[
    ("README.md", "# App\n\nA small item service.\n"),
    ("app/__init__.py", ""),
    (
        "app/main.py",
        "from app.api.routes import create_app\nfrom app.store.db import Store\n\n\ndef main():\n    app = create_app(Store())\n    app.run()\n\n\nif __name__ == \"__main__\":\n    main()\n",
    ),
    ("app/api/__init__.py", ""),
    (
        "app/api/routes.py",
        "from flask import Flask\n\nfrom app.core.audit import record\nfrom app.store.db import Store\n\n\ndef create_app(store: Store):\n    app = Flask(__name__)\n\n    @app.route(\"/items\")\n    def list_items():\n        items = []\n        for item_id in store.ids():\n            items.append(store.load(item_id))\n        record(\"list\", len(items))\n        return {\"items\": items}\n\n    return app\n",
    ),
    ("app/store/__init__.py", ""),
    (
        "app/store/base.py",
        "class BaseStore:\n    def ids(self):\n        return []\n\n    def load(self, item_id):\n        raise NotImplementedError\n",
    ),
    (
        "app/store/db.py",
        "import sqlite3\nimport subprocess\n\nfrom app.core.audit import record\nfrom app.store.base import BaseStore\n\n\nclass Store(BaseStore):\n    def __init__(self, path=\"app.db\"):\n        self.connection = sqlite3.connect(path)\n\n    def ids(self):\n        return [row[0] for row in self.connection.execute(\"SELECT id FROM items\")]\n\n    def load(self, item_id):\n        record(\"load\", item_id)\n        return self.connection.execute(\"SELECT * FROM items WHERE id = %s\" % item_id).fetchone()\n\n    def backup(self, target):\n        subprocess.call(\"cp app.db \" + target, shell=True)\n        self.ids()\n",
    ),
    ("app/core/__init__.py", ""),
    (
        "app/core/audit.py",
        "from app.store.db import Store\n\n\ndef record(event, value):\n    try:\n        print(event, value)\n    except:\n        pass\n\n\ndef default_store():\n    return Store()\n",
    ),
    (
        "tests/test_store.py",
        "from app.store.db import Store\n\n\ndef test_ids():\n    assert Store(\":memory:\").ids() == []\n",
    ),
];

pub const GO_SERVICE: &[(&str, &str)] = &[
    ("go.mod", "module example.com/svc\n\ngo 1.21\n"),
    (
        "cmd/svc/main.go",
        "package main\n\nimport (\n\t\"net/http\"\n\n\t\"example.com/svc/internal/handlers\"\n)\n\nfunc main() {\n\tmux := http.NewServeMux()\n\tmux.HandleFunc(\"/users\", handlers.Users)\n\thttp.ListenAndServe(\":8080\", mux)\n}\n",
    ),
    (
        "internal/handlers/users.go",
        "package handlers\n\nimport (\n\t\"net/http\"\n\n\t\"example.com/svc/internal/store\"\n)\n\nfunc Users(w http.ResponseWriter, r *http.Request) {\n\tfor _, id := range store.IDs() {\n\t\tgo store.Load(id)\n\t}\n\tw.WriteHeader(http.StatusOK)\n}\n",
    ),
    (
        "internal/store/store.go",
        "package store\n\nimport \"os\"\n\nfunc IDs() []string {\n\treturn nil\n}\n\nfunc Load(id string) {\n\tfile, _ := os.Open(id)\n\tfile.Close()\n}\n",
    ),
];

pub struct Fixture {
    pub root: PathBuf,
    pub repo: PathBuf,
    pub report: Value,
    pub map: RepositoryMap,
}

impl Fixture {
    pub fn scan(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ea-reviewer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let repo = root.join("repo");
        for (path, content) in files {
            let path = repo.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let report_path = root.join("artifacts/audit.json");
        std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        run_scan(
            ScanRequest {
                repo_root: repo.clone(),
                analysis: AnalysisRequest {
                    discovery: SourceDiscoveryOptions {
                        include_references: false,
                        method: AnalysisMethod::Auto,
                    },
                    profile: ScanProfile::Full,
                    scope: ScanScope::All,
                    repository_map_output: Some(report_path.clone()),
                    excluded_paths: BTreeSet::from([report_path.clone()]),
                    history_root: None,
                },
                external_inputs: Vec::new(),
                baseline: None,
                format: ScanFormat::Json,
                output: Some(report_path.clone()),
                color: ColorMode::Plain,
                summary_only: false,
                review_loader: Box::new(NoReviewArtifactLoader),
            },
            &NoGraphProvider,
        )
        .unwrap();
        let text = std::fs::read_to_string(&report_path).unwrap();
        let reference = parse_repository_map_reference(&text).unwrap();
        let map = load_and_validate_repository_map(&report_path, &repo, &reference).unwrap();
        Self {
            report: serde_json::from_str(&text).unwrap(),
            root,
            repo,
            map,
        }
    }

    pub fn view(&self) -> RepositoryView<'_> {
        RepositoryView::new(&self.report, &self.map).unwrap()
    }

    pub fn symbol_id(&self, name: &str, kind: &str) -> String {
        self.map
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == kind)
            .map(|symbol| symbol.id.clone())
            .unwrap_or_else(|| panic!("fixture declares {kind} {name}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every cited ID shows in the text, and every bracketed ID is issued.
pub fn assert_cites_only_issued(markdown: &str, issued: &BTreeSet<String>) {
    for id in issued {
        assert!(markdown.contains(id.as_str()), "issued {id} is not shown");
    }
    for citation in markdown
        .split('[')
        .skip(1)
        .filter_map(|rest| rest.split(']').next())
    {
        for id in citation.split(", ") {
            assert!(issued.contains(id), "shown {id} is not issued");
        }
    }
}
