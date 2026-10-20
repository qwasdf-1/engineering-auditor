"""Seeded recall cases in Python; see `cases.py` for the shapes."""

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
def details(client, item_id):
    return client.get(item_id)


def fetch_all(ids, client):
    for item_id in ids:
        consume(details(client, item_id))
"""},
        "cross_file": {
            "app/service.py": """\
from app.remote import details


def fetch_all(ids, client):
    for item_id in ids:
        consume(details(client, item_id))
""",
            "app/remote.py": """\
def details(client, item_id):
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
