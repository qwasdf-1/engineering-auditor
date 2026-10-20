//! Python capability boundaries for the L1 rules that read resolved API
//! identity and statement shapes. Each negative is the nearest safe form of
//! its positive.

pub(super) fn python_l1_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_source, negative_source) = match detector {
        "exit_in_production" => (
            "import sys as system\n\ndef stop(code):\n    system.exit(code)\n",
            "import sys\n\ndef stop(code):\n    raise RuntimeError(code)\n\nif __name__ == \"__main__\":\n    sys.exit(stop(1))\n",
        ),
        "todo_unimplemented_in_production" => (
            "def run():\n    # TODO: retry on timeout\n    return 1\n",
            "def run():\n    # retries are handled upstream\n    return 1\n",
        ),
        "console_output_in_production" => (
            "def render(value):\n    print(value)\n",
            "import logging\n\ndef render(value, out):\n    print(value, file=out)\n    logging.info(value)\n\nif __name__ == \"__main__\":\n    print(\"demo\")\n",
        ),
        "debugger_in_production" => (
            "from pdb import set_trace\n\ndef run(value):\n    set_trace()\n    return value\n",
            "def run(value, breakpoint):\n    breakpoint()\n    return value\n",
        ),
        "insecure_cryptography" => (
            "import hashlib\n\ndef digest(data):\n    return hashlib.md5(data).hexdigest()\n",
            "import hashlib\n\ndef digest(data):\n    return hashlib.md5(data, usedforsecurity=False).hexdigest()\n",
        ),
        "empty_catch_block" => (
            "def load(path):\n    try:\n        return read(path)\n    except Exception:\n        pass\n",
            "def load(path):\n    try:\n        return read(path)\n    except KeyError:\n        pass\n    try:\n        return read(path)\n    except Exception:\n        pass  # best effort: the cache is optional\n",
        ),
        "empty_conditional_branch" => (
            "def check(value):\n    if value:\n        pass\n    else:\n        work(value)\n",
            "def check(value):\n    if not value:\n        work(value)\n    if value:\n        pass  # the caller handles truthy values\n",
        ),
        "no_await_in_loop" => (
            "async def fetch(items):\n    for item in items:\n        await handle(item)\n",
            "import asyncio\n\nasync def fetch(items):\n    async for item in items:\n        await handle(item)\n    for attempt in range(3):\n        if await ping(attempt):\n            break\n    for item in items:\n        await asyncio.sleep(item)\n    for hook in items:\n        await hook(item)\n    await asyncio.gather(*(handle(item) for item in items))\n",
        ),
        "unused_assignment" => (
            "def total(items):\n    result = None\n    result = sum(items)\n    return result\n",
            "def total(items):\n    result = len(items)\n    log(result)\n    result = sum(items)\n    return result\n",
        ),
        "sql_injection" => (
            "def find(cursor, name):\n    cursor.execute(f\"SELECT id FROM users WHERE name = '{name}'\")\n",
            "def find(cursor, name):\n    cursor.execute(\"SELECT id FROM users WHERE name = %s\", (name.strip() + \"\",))\n",
        ),
        "naming_convention" => (
            "def computeTotal(values):\n    return sum(values)\n\nclass request_handler:\n    pass\n",
            "from http.server import BaseHTTPRequestHandler\n\nclass Handler(BaseHTTPRequestHandler):\n    def do_GET(self):\n        pass\n\nclass _Private:\n    def __repr__(self):\n        return 'p'\n\ndef compute_total(values):\n    return sum(values)\n",
        ),
        _ => return None,
    };
    let source = if positive {
        positive_source
    } else {
        negative_source
    };
    Some(vec![("src/capability.py".to_string(), source.to_string())])
}

/// Python boundaries for the L3 collection and L5 loop facts, typed through
/// annotations; each negative is the nearest correct form of its positive.
pub(super) fn python_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_source, negative_source) = match detector {
        "repeated_sort_in_loop" => (
            "def inspect(requests: list[Request], candidates: list[int]):\n    for request in requests:\n        candidates.sort()\n        consume(request, candidates[0])\n",
            "def inspect(groups: list[list[int]]):\n    for group in groups:\n        values = list(group)\n        values.sort()\n        consume(values)\n",
        ),
        "sort_for_extremum" => (
            "def smallest(items: list[int]) -> int:\n    items.sort()\n    return items[0]\n",
            "def ordered(items: list[int]) -> list[int]:\n    items.sort(key=rank)\n    return items\n",
        ),
        "full_sort_for_top_k" => (
            "def smallest(items: list[int]) -> list[int]:\n    items.sort()\n    return items[:5]\n",
            "def smallest(items: list[int]) -> int:\n    items.sort()\n    return items[0]\n",
        ),
        "repeated_collection_traversal" => (
            "def inspect(items: list[Item]):\n    consume(items.count(first))\n    consume(items.index(second))\n    consume(items.count(third))\n",
            "def distinct(ready: list[Item], valid: list[Item], targets: list[Item]):\n    consume(ready.count(first))\n    consume(valid.index(second))\n    consume(targets.count(third))\n\ndef changed(items: list[Item]):\n    consume(items.count(first))\n    items.append(second)\n    consume(items.index(second))\n    consume(items.count(third))\n",
        ),
        "hot_path_linear_membership" => (
            "def select(ids: list[str], wanted: list[str]):\n    for item_id in ids:\n        if item_id in wanted:\n            consume(item_id)\n",
            "def select(ids: list[str], wanted: set[str], text: str):\n    for item_id in ids:\n        if item_id in wanted or item_id in text:\n            consume(item_id)\n\ndef allowed(rows, registry):\n    allow = {\n        (entry[\"path\"], entry[\"kind\"]) for entry in registry.get(\"allow\", [])\n    }\n    index = {entry[\"path\"]: entry for entry in registry.get(\"rows\", [])}\n    for row in rows:\n        if (row.path, row.kind) in allow or row.path in index:\n            consume(row)\n",
        ),
        "invariant_collection_copy_in_loop" => (
            "def inspect(requests: list[int], items: list[int]):\n    for request in requests:\n        consume(request, len(items.copy()))\n",
            "def inspect(requests: list[int], items: list[int]):\n    for request in requests:\n        items.append(request)\n        consume(len(items.copy()))\n",
        ),
        "manual_collection_copy_loop" => (
            "def duplicate(source: list[int], target: list[int]):\n    for index in range(len(source)):\n        target[index] = source[index]\n",
            "def duplicate(source: list[int], target: list[int]):\n    for index in range(len(source)):\n        target[index] = source[index] * 2\n",
        ),
        "map_entry_opportunity" => (
            "def group(groups: dict[str, list[int]], key: str, value: int):\n    if key not in groups:\n        groups[key] = []\n    groups[key].append(value)\n",
            "def group(groups: dict[str, list[int]], key: str, value: int):\n    if key in groups:\n        groups[key] = []\n    groups.setdefault(key, []).append(value)\n",
        ),
        _ => return None,
    };
    let source = if positive {
        positive_source
    } else {
        negative_source
    };
    Some(vec![("src/capability.py".to_string(), source.to_string())])
}
