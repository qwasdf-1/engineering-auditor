"""Seeded recall cases for the algorithmic, systems, and optimization layers.

Each case is a small repository with one known problem, written in one of
four shapes:

- ``direct``: the expensive operation sits in the loop (or async function).
- ``helper``: it sits in a same-file helper the loop calls.
- ``cross_file``: that helper is imported from another module.
- ``negative``: the work is hoisted or batched; the detector must stay silent.

The ``direct`` sources follow the detectors' capability examples, so a miss
there is a regression rather than a gap.
"""

from __future__ import annotations

from dataclasses import dataclass

SHAPES = ("direct", "helper", "cross_file", "negative")


@dataclass(frozen=True)
class Case:
    detector: str
    language: str
    shape: str
    files: dict[str, str]

    @property
    def id(self) -> str:
        return f"{self.detector}/{self.language}/{self.shape}"

    @property
    def positive(self) -> bool:
        return self.shape != "negative"


# Per detector and language: the four shapes. A cross-file case is a pair of
# files; the others are one file.
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
fn fetch(client: &Client, id: u64) -> Response {
    client.get(id)
}

fn fetch_all(ids: Vec<u64>, client: Client) {
    for id in ids {
        consume(fetch(&client, id));
    }
}
"""},
        "cross_file": {
            "src/lib.rs": """\
mod remote;
use remote::fetch;

fn fetch_all(ids: Vec<u64>, client: Client) {
    for id in ids {
        consume(fetch(&client, id));
    }
}
""",
            "src/remote.rs": """\
pub fn fetch(client: &Client, id: u64) -> Response {
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

PYTHON = {
    "repeated_sort_in_loop": {
        "direct": {"app/service.py": """\
def inspect(requests: list[Request], candidates: list[int]):
    for request in requests:
        candidates.sort()
        consume(request, candidates[0])
"""},
        "helper": {"app/service.py": """\
def refresh(candidates: list[int]):
    candidates.sort()


def inspect(requests: list[Request], candidates: list[int]):
    for request in requests:
        refresh(candidates)
        consume(request, candidates[0])
"""},
        "cross_file": {
            "app/service.py": """\
from app.ranking import refresh


def inspect(requests: list[Request], candidates: list[int]):
    for request in requests:
        refresh(candidates)
        consume(request, candidates[0])
""",
            "app/ranking.py": """\
def refresh(candidates: list[int]):
    candidates.sort()
""",
        },
        "negative": {"app/service.py": """\
def inspect(requests: list[Request], candidates: list[int]):
    candidates.sort()
    for request in requests:
        consume(request, candidates[0])
"""},
    },
    "n_plus_one_query_in_loop": {
        "direct": {"app/service.py": """\
def load(ids: list[int], repo: Repository):
    for item_id in ids:
        consume(repo.find_by_id(item_id))
"""},
        "helper": {"app/service.py": """\
def load_one(repo: Repository, item_id: int):
    return repo.find_by_id(item_id)


def load(ids: list[int], repo: Repository):
    for item_id in ids:
        consume(load_one(repo, item_id))
"""},
        "cross_file": {
            "app/service.py": """\
from app.store import load_one


def load(ids: list[int], repo: Repository):
    for item_id in ids:
        consume(load_one(repo, item_id))
""",
            "app/store.py": """\
def load_one(repo: Repository, item_id: int):
    return repo.find_by_id(item_id)
""",
        },
        "negative": {"app/service.py": """\
def load(ids: list[int], repo: Repository):
    consume(repo.find_many(ids))
"""},
    },
    "per_item_external_io": {
        "direct": {"app/service.py": """\
def fetch_all(ids, client):
    for item_id in ids:
        consume(client.get(item_id))
"""},
        "helper": {"app/service.py": """\
def fetch(client, item_id):
    return client.get(item_id)


def fetch_all(ids, client):
    for item_id in ids:
        consume(fetch(client, item_id))
"""},
        "cross_file": {
            "app/service.py": """\
from app.remote import fetch


def fetch_all(ids, client):
    for item_id in ids:
        consume(fetch(client, item_id))
""",
            "app/remote.py": """\
def fetch(client, item_id):
    return client.get(item_id)
""",
        },
        "negative": {"app/service.py": """\
def fetch_all(ids, client):
    response = client.get(ids)
    for item in response:
        consume(item)
"""},
    },
    "loop_invariant_regex_compilation": {
        "direct": {"app/service.py": """\
import re


def inspect(items):
    for item in items:
        pattern = re.compile('^fixed$')
        consume(pattern, item)
"""},
        "helper": {"app/service.py": """\
import re


def matcher():
    return re.compile('^fixed$')


def inspect(items):
    for item in items:
        pattern = matcher()
        consume(pattern, item)
"""},
        "cross_file": {
            "app/service.py": """\
from app.patterns import matcher


def inspect(items):
    for item in items:
        pattern = matcher()
        consume(pattern, item)
""",
            "app/patterns.py": """\
import re


def matcher():
    return re.compile('^fixed$')
""",
        },
        "negative": {"app/service.py": """\
import re


def inspect(items):
    pattern = re.compile('^fixed$')
    for item in items:
        consume(pattern, item)
"""},
    },
    "loop_artifact_rewrite": {
        "direct": {"app/service.py": """\
from pathlib import Path


def rewrite(paths):
    for path in paths:
        raw = Path(path).read_text()
        Path('report.json').write_text(raw)
"""},
        "helper": {"app/service.py": """\
from pathlib import Path


def save(raw):
    Path('report.json').write_text(raw)


def rewrite(paths):
    for path in paths:
        raw = Path(path).read_text()
        save(raw)
"""},
        "cross_file": {
            "app/service.py": """\
from pathlib import Path

from app.report import save


def rewrite(paths):
    for path in paths:
        raw = Path(path).read_text()
        save(raw)
""",
            "app/report.py": """\
from pathlib import Path


def save(raw):
    Path('report.json').write_text(raw)
""",
        },
        "negative": {"app/service.py": """\
from pathlib import Path


def rewrite(paths):
    raw = Path('input').read_text()
    for path in paths:
        consume(path, raw)
"""},
    },
    "blocking_io_in_async_context": {
        "direct": {"app/service.py": """\
from pathlib import Path


async def load():
    consume(Path('input').read_text())
"""},
        "helper": {"app/service.py": """\
from pathlib import Path


def read_input():
    return Path('input').read_text()


async def load():
    consume(read_input())
"""},
        "cross_file": {
            "app/service.py": """\
from app.inputs import read_input


async def load():
    consume(read_input())
""",
            "app/inputs.py": """\
from pathlib import Path


def read_input():
    return Path('input').read_text()
""",
        },
        "negative": {"app/service.py": """\
from pathlib import Path


def load():
    consume(Path('input').read_text())
"""},
    },
}

SOURCES = {"rust": RUST, "python": PYTHON}


def all_cases() -> list[Case]:
    cases = []
    for language, detectors in SOURCES.items():
        for detector, shapes in detectors.items():
            for shape in SHAPES:
                cases.append(Case(detector, language, shape, shapes[shape]))
    return cases
