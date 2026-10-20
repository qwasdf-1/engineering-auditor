"""Seeded recall cases in TypeScript; see `cases.py` for the shapes."""

TYPESCRIPT = {
    "repeated_sort_in_loop": {
        "direct": {"src/service.ts": """\
function inspect(requests: Request[], candidates: number[]) {
  for (const request of requests) {
    candidates.sort();
    consume(request, candidates[0]);
  }
}
"""},
        "helper": {"src/service.ts": """\
function refresh(candidates: number[]) {
  candidates.sort();
}

function inspect(requests: Request[], candidates: number[]) {
  for (const request of requests) {
    refresh(candidates);
    consume(request, candidates[0]);
  }
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { refresh } from './ranking';

function inspect(requests: Request[], candidates: number[]) {
  for (const request of requests) {
    refresh(candidates);
    consume(request, candidates[0]);
  }
}
""",
            "src/ranking.ts": """\
export function refresh(candidates: number[]) {
  candidates.sort();
}
""",
        },
        "negative": {"src/service.ts": """\
function inspect(requests: Request[], candidates: number[]) {
  candidates.sort();
  for (const request of requests) {
    consume(request, candidates[0]);
  }
}
"""},
    },
    "n_plus_one_query_in_loop": {
        "direct": {"src/service.ts": """\
function load(ids: number[], repo: Repository) {
  for (const id of ids) consume(repo.find_by_id(id));
}
"""},
        "helper": {"src/service.ts": """\
function loadOne(repo: Repository, id: number) {
  return repo.find_by_id(id);
}

function load(ids: number[], repo: Repository) {
  for (const id of ids) consume(loadOne(repo, id));
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { loadOne } from './store';

function load(ids: number[], repo: Repository) {
  for (const id of ids) consume(loadOne(repo, id));
}
""",
            "src/store.ts": """\
export function loadOne(repo: Repository, id: number) {
  return repo.find_by_id(id);
}
""",
        },
        "negative": {"src/service.ts": """\
function load(ids: number[], repo: Repository) {
  consume(repo.find_many(ids));
}
"""},
    },
    "per_item_external_io": {
        "direct": {"src/service.ts": """\
function fetchAll(ids: string[]) {
  for (const id of ids) consume(fetch(id));
}
"""},
        "helper": {"src/service.ts": """\
function details(id: string) {
  return fetch(id);
}

function fetchAll(ids: string[]) {
  for (const id of ids) consume(details(id));
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { details } from './remote';

function fetchAll(ids: string[]) {
  for (const id of ids) consume(details(id));
}
""",
            "src/remote.ts": """\
export function details(id: string) {
  return fetch(id);
}
""",
        },
        "negative": {"src/service.ts": """\
function fetchAll(ids: string[]) {
  const response = fetch(ids.join(','));
  consume(response);
}
"""},
    },
    "loop_invariant_regex_compilation": {
        "direct": {"src/service.ts": """\
function inspect(items: string[]) {
  for (const item of items) {
    const pattern = new RegExp('^fixed$');
    consume(pattern, item);
  }
}
"""},
        "helper": {"src/service.ts": """\
function matcher() {
  return new RegExp('^fixed$');
}

function inspect(items: string[]) {
  for (const item of items) {
    const pattern = matcher();
    consume(pattern, item);
  }
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { matcher } from './patterns';

function inspect(items: string[]) {
  for (const item of items) {
    const pattern = matcher();
    consume(pattern, item);
  }
}
""",
            "src/patterns.ts": """\
export function matcher() {
  return new RegExp('^fixed$');
}
""",
        },
        "negative": {"src/service.ts": """\
function inspect(items: string[]) {
  const pattern = new RegExp('^fixed$');
  for (const item of items) {
    consume(pattern, item);
  }
}
"""},
    },
    "loop_artifact_rewrite": {
        "direct": {"src/service.ts": """\
function rewrite(paths: string[]) {
  for (const path of paths) {
    const raw = fs.readFileSync(path);
    fs.writeFileSync('report.json', raw);
  }
}
"""},
        "helper": {"src/service.ts": """\
function save(raw: string) {
  fs.writeFileSync('report.json', raw);
}

function rewrite(paths: string[]) {
  for (const path of paths) {
    const raw = fs.readFileSync(path);
    save(raw);
  }
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { save } from './report';

function rewrite(paths: string[]) {
  for (const path of paths) {
    const raw = fs.readFileSync(path);
    save(raw);
  }
}
""",
            "src/report.ts": """\
export function save(raw: string) {
  fs.writeFileSync('report.json', raw);
}
""",
        },
        "negative": {"src/service.ts": """\
function rewrite(paths: string[]) {
  const raw = fs.readFileSync('input');
  for (const path of paths) consume(path, raw);
}
"""},
    },
    "blocking_io_in_async_context": {
        "direct": {"src/service.ts": """\
async function load() {
  consume(fs.readFileSync('input'));
}
"""},
        "helper": {"src/service.ts": """\
function readInput() {
  return fs.readFileSync('input');
}

async function load() {
  consume(readInput());
}
"""},
        "cross_file": {
            "src/service.ts": """\
import { readInput } from './inputs';

async function load() {
  consume(readInput());
}
""",
            "src/inputs.ts": """\
export function readInput() {
  return fs.readFileSync('input');
}
""",
        },
        "negative": {"src/service.ts": """\
function load() {
  consume(fs.readFileSync('input'));
}
"""},
    },
}
