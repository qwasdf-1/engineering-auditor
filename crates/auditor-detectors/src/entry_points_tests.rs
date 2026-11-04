use super::{entry_points, EntryKind};
use crate::structure_index::{index_sources, StructureIndex};
use crate::Language;

/// `kind path:line symbol detail` for every entry point.
fn described(index: &StructureIndex) -> Vec<String> {
    entry_points(index)
        .into_iter()
        .map(|entry| {
            let file = &index.files()[entry.file];
            let symbol = entry
                .symbol
                .map(|(symbol_file, symbol)| {
                    index.files()[symbol_file].symbols[symbol].name.clone()
                })
                .unwrap_or_else(|| "-".to_string());
            format!(
                "{} {}:{} {} {}",
                entry.kind.as_str(),
                file.path,
                entry.line,
                symbol,
                entry.detail
            )
        })
        .collect()
}

#[test]
fn python_mains_commands_signals_and_process_runs() {
    let index = index_sources(
        "entry-python",
        &[
            (
                "app/cli.py",
                Language::Python,
                "import click\nimport signal\n\n\n@click.command()\ndef serve():\n    signal.signal(signal.SIGTERM, stop)\n\n\ndef stop(signum, frame):\n    pass\n\n\ndef main():\n    serve()\n\n\nif __name__ == \"__main__\":\n    main()\n",
            ),
            (
                "app/worker.py",
                Language::Python,
                "from multiprocessing import Process\n\n\nclass Worker(Process):\n    def run(self):\n        pass\n",
            ),
        ],
    );
    assert_eq!(
        described(&index),
        [
            "cli_command app/cli.py:6 serve click/typer command",
            "main app/cli.py:18 - __main__ guard",
            "main app/cli.py:19 main called from the __main__ guard",
            "process_run app/worker.py:5 run run of multiprocessing.Process subclass Worker",
        ],
        "a handler registered by a bare name outside its registering function stays unrecognized"
    );
}

#[test]
fn python_route_decorators_and_django_urls() {
    let index = index_sources(
        "entry-python-http",
        &[
            (
                "app/api.py",
                Language::Python,
                "from fastapi import APIRouter\n\nrouter = APIRouter()\n\n\n@router.get(\"/users/{id}\")\nasync def user(id: int):\n    return id\n\n\n@cache.get(key)\ndef cached():\n    pass\n",
            ),
            (
                "app/views.py",
                Language::Python,
                "def index(request):\n    return None\n",
            ),
            (
                "app/urls.py",
                Language::Python,
                "from django.urls import path\n\nfrom .views import index\n\nurlpatterns = [\n    path(\"\", index),\n    path(\"admin/\", admin.site.urls),\n    path(\"legacy/\", legacy.view),\n]\n",
            ),
        ],
    );
    assert_eq!(
        described(&index),
        [
            "http_handler app/api.py:7 user GET /users/{id}",
            "http_handler app/urls.py:6 index ANY ",
            "http_handler app/urls.py:8 - ANY legacy/",
        ]
    );
}

#[test]
fn javascript_routes_need_a_function_handler() {
    let index = index_sources(
        "entry-js",
        &[
            (
                "src/server.js",
                Language::JavaScript,
                "import express from 'express';\nimport { list } from './users.js';\n\nconst app = express();\napp.get('/health', (req, res) => res.send('ok'));\napp.post('/users', auth, list);\napi.get('/remote', config);\n",
            ),
            (
                "src/users.js",
                Language::JavaScript,
                "export function list(req, res) {\n  res.json([]);\n}\n",
            ),
        ],
    );
    let described = described(&index);
    assert!(
        described
            .iter()
            .any(|entry| entry.starts_with("http_handler src/server.js:5 ")
                && entry.ends_with(" GET /health")),
        "the inline handler: {described:?}"
    );
    assert!(
        described.contains(&"http_handler src/server.js:6 list POST /users".to_string()),
        "{described:?}"
    );
    assert!(
        !described.iter().any(|entry| entry.contains("/remote")),
        "a client call passing a non-function is no route: {described:?}"
    );
}

#[test]
fn java_kotlin_and_csharp_route_annotations() {
    let index = index_sources(
        "entry-jvm",
        &[
            (
                "src/UserController.java",
                Language::Java,
                "package app;\n\n@RestController\nclass UserController {\n    @GetMapping(\"/users\")\n    List<User> list() { return null; }\n\n    public static void main(String[] args) {}\n}\n",
            ),
            (
                "src/Api.kt",
                Language::Kotlin,
                "package app\n\nclass Api {\n    @PostMapping(\"/items\")\n    fun create(): Item = Item()\n}\n\nfun main() {}\n",
            ),
            (
                "src/ItemsController.cs",
                Language::CSharp,
                "namespace App;\n\npublic class ItemsController {\n    [HttpGet(\"items/{id}\")]\n    public Item Get(int id) => null;\n}\n",
            ),
        ],
    );
    assert_eq!(
        described(&index),
        [
            "http_handler src/Api.kt:4 create POST /items",
            "main src/Api.kt:8 main fun main",
            "http_handler src/ItemsController.cs:4 Get GET items/{id}",
            "http_handler src/UserController.java:5 list GET /users",
            "main src/UserController.java:8 main static main",
        ]
    );
}

#[test]
fn go_and_rust_routes_and_mains() {
    let index = index_sources(
        "entry-go-rust",
        &[
            (
                "cmd/server/main.go",
                Language::Go,
                "package main\n\nimport \"net/http\"\n\nfunc health(w http.ResponseWriter, r *http.Request) {}\n\nfunc main() {\n\thttp.HandleFunc(\"GET /health\", health)\n\thttp.HandleFunc(\"/ready\", func(w http.ResponseWriter, r *http.Request) {})\n\tcache.Get(\"/key\", value)\n}\n",
            ),
            (
                "src/routes.rs",
                Language::Rust,
                "#[get(\"/users\")]\nasync fn users() -> String {\n    String::new()\n}\n\npub fn router() -> Router {\n    Router::new().route(\"/items\", get(items).post(create))\n}\n\nasync fn items() {}\nasync fn create() {}\n",
            ),
        ],
    );
    let described = described(&index);
    for expected in [
        "http_handler cmd/server/main.go:8 health GET /health",
        "main cmd/server/main.go:7 main func main",
        "http_handler src/routes.rs:2 users GET /users",
        "http_handler src/routes.rs:7 items GET /items",
        "http_handler src/routes.rs:7 create POST /items",
    ] {
        assert!(
            described.contains(&expected.to_string()),
            "{expected} not in {described:?}"
        );
    }
    assert!(
        described.iter().any(
            |entry| entry.starts_with("http_handler cmd/server/main.go:9 ")
                && entry.ends_with("ANY /ready")
        ),
        "{described:?}"
    );
    assert!(
        !described.iter().any(|entry| entry.contains("/key")),
        "{described:?}"
    );
}

#[test]
fn entry_kinds_have_stable_names() {
    assert_eq!(
        [
            EntryKind::Main,
            EntryKind::CliCommand,
            EntryKind::HttpHandler,
            EntryKind::ProcessRun,
            EntryKind::SignalHandler,
        ]
        .map(EntryKind::as_str),
        [
            "main",
            "cli_command",
            "http_handler",
            "process_run",
            "signal_handler"
        ]
    );
}

#[test]
fn routes_inside_rust_test_modules_are_not_entry_points() {
    let index = index_sources(
        "entry-rust-tests",
        &[(
            "src/lib.rs",
            Language::Rust,
            "#[get(\"/live\")]\nasync fn live() {}\n\n#[cfg(test)]\nmod tests {\n    #[get(\"/fixture\")]\n    async fn fixture() {}\n\n    #[test]\n    fn routes() {\n        let app = Router::new().route(\"/x\", get(handler));\n    }\n}\n",
        )],
    );
    assert_eq!(
        described(&index),
        ["http_handler src/lib.rs:2 live GET /live"]
    );
}
