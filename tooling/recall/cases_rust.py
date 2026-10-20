"""Seeded recall cases in Rust; see `cases.py` for the shapes."""

RUST = {
    "repeated_sort_in_loop": {
        "direct": {"src/lib.rs": """\
fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {
    for request in requests {
        candidates.sort();
        consume(request, &candidates);
    }
}
"""},
        "helper": {"src/lib.rs": """\
fn refresh(candidates: &mut Vec<i32>) {
    candidates.sort();
}

fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {
    for request in requests {
        refresh(&mut candidates);
        consume(request, &candidates);
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod ranking;
use ranking::refresh;

fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {
    for request in requests {
        refresh(&mut candidates);
        consume(request, &candidates);
    }
}
""",
            "src/ranking.rs": """\
pub fn refresh(candidates: &mut Vec<i32>) {
    candidates.sort();
}
""",
        },
        "negative": {"src/lib.rs": """\
fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {
    candidates.sort();
    for request in requests {
        consume(request, &candidates);
    }
}
"""},
    },
    "n_plus_one_query_in_loop": {
        "direct": {"src/lib.rs": """\
fn load(ids: Vec<i64>, repo: &Repository) {
    for id in ids {
        let row = repo.find_by_id(id);
        consume(row);
    }
}
"""},
        "helper": {"src/lib.rs": """\
fn load_one(repo: &Repository, id: i64) -> Row {
    repo.find_by_id(id)
}

fn load(ids: Vec<i64>, repo: &Repository) {
    for id in ids {
        let row = load_one(repo, id);
        consume(row);
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod store;
use store::load_one;

fn load(ids: Vec<i64>, repo: &Repository) {
    for id in ids {
        let row = load_one(repo, id);
        consume(row);
    }
}
""",
            "src/store.rs": """\
pub fn load_one(repo: &Repository, id: i64) -> Row {
    repo.find_by_id(id)
}
""",
        },
        "negative": {"src/lib.rs": """\
fn load(ids: Vec<i64>, repo: &Repository) {
    let rows = repo.find_many(&ids);
    for row in rows {
        consume(row);
    }
}
"""},
    },
    "per_item_external_io": {
        "direct": {"src/lib.rs": """\
fn fetch_all(ids: Vec<u64>, client: Client) {
    for id in ids {
        consume(client.get(id));
    }
}
"""},
        "helper": {"src/lib.rs": """\
fn details(client: &Client, id: u64) -> Response {
    client.get(id)
}

fn fetch_all(ids: Vec<u64>, client: Client) {
    for id in ids {
        consume(details(&client, id));
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod remote;
use remote::details;

fn fetch_all(ids: Vec<u64>, client: Client) {
    for id in ids {
        consume(details(&client, id));
    }
}
""",
            "src/remote.rs": """\
pub fn details(client: &Client, id: u64) -> Response {
    client.get(id)
}
""",
        },
        "negative": {"src/lib.rs": """\
fn fetch_all(ids: Vec<u64>, client: Client) {
    let response = client.get(ids);
    for item in response {
        consume(item);
    }
}
"""},
    },
    "loop_invariant_regex_compilation": {
        "direct": {"src/lib.rs": """\
use regex::Regex;

fn inspect(items: &[String]) {
    for item in items {
        let pattern = Regex::new("^fixed$").unwrap();
        consume(pattern.is_match(item));
    }
}
"""},
        "helper": {"src/lib.rs": """\
use regex::Regex;

fn matcher() -> Regex {
    Regex::new("^fixed$").unwrap()
}

fn inspect(items: &[String]) {
    for item in items {
        let pattern = matcher();
        consume(pattern.is_match(item));
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod patterns;
use patterns::matcher;

fn inspect(items: &[String]) {
    for item in items {
        let pattern = matcher();
        consume(pattern.is_match(item));
    }
}
""",
            "src/patterns.rs": """\
use regex::Regex;

pub fn matcher() -> Regex {
    Regex::new("^fixed$").unwrap()
}
""",
        },
        "negative": {"src/lib.rs": """\
use regex::Regex;

fn inspect(items: &[String]) {
    let pattern = Regex::new("^fixed$").unwrap();
    for item in items {
        consume(pattern.is_match(item));
    }
}
"""},
    },
    "loop_artifact_rewrite": {
        "direct": {"src/lib.rs": """\
fn rewrite(paths: Vec<PathBuf>) {
    for path in paths {
        let raw = std::fs::read_to_string(path).unwrap();
        std::fs::write("report.json", raw).unwrap();
    }
}
"""},
        "helper": {"src/lib.rs": """\
fn save(raw: &str) {
    std::fs::write("report.json", raw).unwrap();
}

fn rewrite(paths: Vec<PathBuf>) {
    for path in paths {
        let raw = std::fs::read_to_string(path).unwrap();
        save(&raw);
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod report;
use report::save;

fn rewrite(paths: Vec<PathBuf>) {
    for path in paths {
        let raw = std::fs::read_to_string(path).unwrap();
        save(&raw);
    }
}
""",
            "src/report.rs": """\
pub fn save(raw: &str) {
    std::fs::write("report.json", raw).unwrap();
}
""",
        },
        "negative": {"src/lib.rs": """\
fn rewrite(paths: Vec<PathBuf>) {
    let raw = std::fs::read_to_string("input").unwrap();
    for path in paths {
        consume(path, &raw);
    }
}
"""},
    },
    "blocking_io_in_async_context": {
        "direct": {"src/lib.rs": """\
async fn load() {
    let raw = std::fs::read_to_string("input.txt").unwrap();
    consume(raw);
}
"""},
        "helper": {"src/lib.rs": """\
fn read_input() -> String {
    std::fs::read_to_string("input.txt").unwrap()
}

async fn load() {
    let raw = read_input();
    consume(raw);
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod input;
use input::read_input;

async fn load() {
    let raw = read_input();
    consume(raw);
}
""",
            "src/input.rs": """\
pub fn read_input() -> String {
    std::fs::read_to_string("input.txt").unwrap()
}
""",
        },
        "negative": {"src/lib.rs": """\
fn load() {
    let raw = std::fs::read_to_string("input.txt").unwrap();
    consume(raw);
}
"""},
    },
}
