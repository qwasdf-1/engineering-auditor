use crate::ScanProfile;

use super::CapabilityCase;

fn file(path: &str, source: impl Into<String>) -> (String, String) {
    (path.to_string(), source.into())
}

pub(super) fn default_cases() -> Vec<CapabilityCase> {
    vec![
        CapabilityCase::new(
            "nested_quadratic_iteration",
            ScanProfile::Full,
            "src/lib.rs",
            "fn inspect(items: Vec<Item>) {\n    for left in &items {\n        for right in &items { if left.id == right.id { consume(left, right); } }\n    }\n}\n",
            "src/lib.rs",
            "fn inspect(rows: Vec<Vec<i32>>) {\n    for row in rows {\n        for value in row { consume(value); }\n    }\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn inspect(left: Vec<i32>, right: Vec<i32>) {\n    for value in left { consume(value); }\n    for value in right { consume(value); }\n}\n"),
            file("src/no_dependence.rs", "fn inspect(items: Vec<Item>) {\n    for left in &items {\n        for right in &items { consume(right); }\n        consume(left);\n    }\n}\n"),
            file("src/distinct_fields.rs", "struct Options { strategy_ids: Vec<String>, recent_turns: Vec<u8> }\nfn inspect(options: &Options) {\n    for strategy in &options.strategy_ids {\n        for turns in &options.recent_turns { consume(strategy, turns); }\n    }\n}\n"),
            file("src/tree.py", "def inspect(node):\n    for child in node.children:\n        for sub in child.children:\n            consume(child, sub)\n"),
        ]),
        CapabilityCase::new(
            "repeated_sort_in_loop",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {\n    for request in requests {\n        candidates.sort();\n        consume(request, &candidates);\n    }\n}\n",
            "src/lib.rs",
            "fn inspect(mut rows: Vec<Vec<i32>>) {\n    for row in &mut rows { row.sort(); }\n}\n",
        )
        .negative_files(vec![
            file(
                "tool.py",
                "def render(layer_statuses):\n    for layer_num in range(5):\n        dets = sorted(layer_statuses[layer_num].items())\n        consume(dets)\n",
            ),
            file(
                "src/mutated.rs",
                "fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {\n    for request in requests {\n        candidates.push(request.score);\n        candidates.sort();\n    }\n}\n",
            ),
            file(
                "src/comparator.rs",
                "fn inspect(requests: Vec<Request>, mut candidates: Vec<i32>) {\n    for request in requests { candidates.sort_by(compare); consume(request); }\n}\n",
            ),
            file(
                "src/loop_local.rs",
                "fn inspect(groups: Vec<Vec<i32>>) {\n    for group in groups {\n        let mut values = group.to_vec();\n        values.sort();\n        consume(values);\n    }\n}\n",
            ),
        ]),
        CapabilityCase::new(
            "cache_miss_opportunity",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn inspect(data: &str) -> bool { data.to_ascii_lowercase() == data.to_ascii_lowercase() && data.to_ascii_lowercase().is_empty() }\n",
            "src/lib.rs",
            "fn inspect(data: &[u8]) -> bool { data.is_ascii() && data.is_ascii() && data.is_ascii() }\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn inspect(writer: &mut Writer, a: i32, b: i32) { writer.close(); writer.close(); writer.close(); consume(max(a, b), max(a, 0), max(b, 0)); }\n"),
            file("src/parse.rs", "fn inspect(error: &str) { consume(parse_marker(error, \"one\"), parse_marker(error, \"two\"), parse_marker(error, \"three\")); }\n"),
            file("src/cheap.rs", "fn inspect(blockers: Vec<Error>) { consume(blockers.is_empty(), blockers.is_empty(), blockers.is_empty()); }\n"),
        ]),
        CapabilityCase::new(
            "repeated_format_allocation",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn render(items: Vec<i32>, mode: &str) {\n    for item in items {\n        consume(item, format!(\"mode={mode}\"));\n    }\n}\n",
            "src/lib.rs",
            "fn render(items: Vec<i32>) {\n    for item in items { consume(format!(\"{item}\")); }\n}\n\nfn role_rows(items: Vec<i32>, batch: &Batch) {\n    for item in items { consume(item, format!(\"{:?}\", batch.state).to_lowercase()); }\n}\n",
        )
        .negative_files(vec![file(
            "src/main/java/Names.java",
            "class Names {\n    static final int MAX = 255;\n    int depth(String name) {\n        int dim = 0;\n        while (dim < name.length()) {\n            dim++;\n            if (dim > MAX) {\n                throw new IllegalArgumentException(String.format(\"Maximum %d exceeded\", MAX));\n            }\n        }\n        return dim;\n    }\n}\n",
        )]),
        CapabilityCase::new(
            "sort_for_extremum",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn smallest(mut items: Vec<i32>) -> Option<i32> {\n    items.sort();\n    items.first().copied()\n}\n",
            "src/lib.rs",
            "fn ordered(mut items: Vec<i32>) {\n    items.sort();\n    for item in items { consume(item); }\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn ordered(mut items: Vec<i32>) {\n    items.sort();\n    for item in items { consume(item); }\n}\n"),
            file("src/comparator.rs", "fn smallest(mut items: Vec<i32>) -> Option<i32> {\n    items.sort_by(compare);\n    items.first().copied()\n}\n"),
            file("src/top_k.rs", "fn smallest(mut items: Vec<i32>) -> Vec<i32> {\n    items.sort();\n    items.iter().take(5).copied().collect()\n}\n"),
        ]),
        CapabilityCase::new(
            "full_sort_for_top_k",
            ScanProfile::Full,
            "src/lib.rs",
            "fn smallest(mut items: Vec<i32>) -> Vec<i32> {\n    items.sort();\n    items.iter().take(5).copied().collect()\n}\n",
            "src/lib.rs",
            "fn smallest(mut items: Vec<i32>) -> Option<i32> {\n    items.sort();\n    items.iter().take(1).copied().next()\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn smallest(mut items: Vec<i32>) -> Option<i32> {\n    items.sort();\n    items.iter().take(1).copied().next()\n}\n"),
            file("src/comparator.rs", "fn smallest(mut items: Vec<i32>) -> Vec<i32> {\n    items.sort_by(compare);\n    items.iter().take(5).copied().collect()\n}\n"),
            file("src/unbounded.rs", "fn smallest(mut items: Vec<i32>, k: usize) -> Vec<i32> {\n    items.sort();\n    items.iter().take(k).copied().collect()\n}\n"),
        ]),
        CapabilityCase::new(
            "repeated_collection_traversal",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn inspect(items: &[i32]) {\n    consume(items.iter().any(|item| *item > 0));\n    consume(items.iter().find(|item| **item == 2));\n    consume(items.iter().count());\n}\n",
            "src/lib.rs",
            "fn inspect(items: &[i32]) { consume(items.iter().any(is_ready)); }\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn inspect(items: &[i32]) { consume(items.iter().any(is_ready)); }\n"),
            file("src/effects.rs", "fn inspect(items: &[i32]) {\n    consume(items.iter().any(|item| check(item)));\n    consume(items.iter().find(|item| verify(item)));\n    consume(items.iter().count());\n}\n"),
            file("src/mutation.rs", "fn inspect(items: &mut Vec<i32>) {\n    consume(items.iter().any(|item| *item > 0));\n    items.push(3);\n    consume(items.iter().find(|item| **item == 2));\n    consume(items.iter().count());\n}\n"),
            file("src/distinct_fields.rs", "struct Groups { ready: Vec<i32>, valid: Vec<i32>, targets: Vec<i32> }\nfn inspect(groups: &Groups) {\n    consume(groups.ready.iter().any(|item| *item > 0));\n    consume(groups.valid.iter().any(|item| *item > 0));\n    consume(groups.targets.iter().any(|item| *item > 0));\n}\n"),
            file("src/mutable_call.rs", "fn mutate(items: &mut Vec<i32>) { items.push(4); }\nfn inspect() {\n    let mut items = vec![1, 2, 3];\n    consume(items.iter().any(|item| *item > 0));\n    mutate(&mut items);\n    consume(items.iter().find(|item| **item == 2));\n    consume(items.iter().count());\n}\n"),
            file("src/exclusive_match.rs", "enum Mode { First, Second }\nfn inspect(items: &[i32], mode: Mode) {\n    match mode {\n        Mode::First => {\n            consume(items.iter().any(|item| *item > 0));\n            consume(items.iter().find(|item| **item == 2));\n        }\n        Mode::Second => consume(items.iter().count()),\n    }\n}\n"),
            file("src/exclusive_if.rs", "fn inspect(items: &[i32], first: bool) {\n    if first {\n        consume(items.iter().any(|item| *item > 0));\n        consume(items.iter().find(|item| **item == 2));\n    } else {\n        consume(items.iter().count());\n    }\n}\n"),
        ]),
        CapabilityCase::new(
            "loop_invariant_regex_compilation",
            ScanProfile::Gating,
            "src/lib.rs",
            "use regex::Regex;\nfn inspect(items: &[String]) {\n    for item in items {\n        let pattern = Regex::new(\"^fixed$\").unwrap();\n        consume(pattern.is_match(item));\n    }\n}\n",
            "src/lib.rs",
            "use regex::Regex;\nfn inspect(items: &[String]) {\n    for item in items { let pattern = Regex::new(item).unwrap(); consume(pattern); }\n}\n",
        )
        .negative_files(vec![file(
            "src/check.ts",
            "let emojiRegex: RegExp | undefined;\nexport function check(inputs: string[]) {\n  for (const input of inputs) {\n    if (!emojiRegex) {\n      emojiRegex = new RegExp(\"^\\\\p{Emoji}$\", \"u\");\n    }\n    consume(emojiRegex.test(input));\n  }\n}\n",
        )]),
        CapabilityCase::new(
            "immutable_string_concatenation_in_loop",
            ScanProfile::Gating,
            "src/render.py",
            "def render(items: list[str]):\n    output = \"\"\n    for item in items:\n        output += item\n    return output\n",
            "src/render.py",
            "def render(items):\n    output = []\n    for item in items:\n        output.append(str(item))\n    return \"\".join(output)\n",
        ),
        CapabilityCase::new(
            "hot_path_linear_membership",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn select(ids: Vec<String>, wanted: Vec<String>) {\n    for id in ids {\n        if wanted.contains(&id) { consume(id); }\n    }\n}\n",
            "src/lib.rs",
            "fn select(ids: Vec<String>, wanted: HashSet<String>) {\n    for id in ids {\n        if wanted.contains(&id) { consume(id); }\n    }\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn select(ids: Vec<String>, wanted: HashSet<String>) {\n    for id in ids {\n        if wanted.contains(&id) { consume(id); }\n    }\n}\n"),
            file("src/render.ts", "function renderStatus(label: string, values: string[]) {\n  for (const value of values) { consume(value) }\n  return label.includes('ready')\n}\n"),
            file("src/bounded.rs", "fn select(ids: Vec<String>) {\n    let wanted = [\"a\", \"b\"];\n    for id in ids { if wanted.contains(&id) { consume(id); } }\n}\n"),
            file("src/mutated.rs", "fn select(ids: Vec<String>, mut wanted: Vec<String>) {\n    for id in ids { wanted.push(id.clone()); if wanted.contains(&id) { consume(id); } }\n}\n"),
        ]),
        CapabilityCase::new(
            "n_plus_one_query_in_loop",
            ScanProfile::Full,
            "src/lib.rs",
            "fn load(ids: Vec<i64>, repo: &Repository) {\n    for id in ids {\n        let row = repo.find_by_id(id);\n        consume(row);\n    }\n}\n",
            "src/lib.rs",
            "fn save(ids: Vec<i64>, connection: &Connection) {\n    for id in ids { connection.execute(\"insert\", id); }\n}\n",
        )
        .negative_files(vec![
            file("src/lib.rs", "fn save(ids: Vec<i64>, connection: &Connection) {\n    for id in ids { connection.execute(\"insert\", id); }\n}\n"),
            file("src/invariant.rs", "fn load(ids: Vec<i64>, repo: &Repository) {\n    for id in ids { consume(repo.find_by_id(42), id); }\n}\n"),
            file("src/batch.rs", "fn load(ids: Vec<i64>, repo: &Repository) { consume(repo.find_many(ids)); }\n"),
        ]),
        CapabilityCase::new(
            "loop_carried_copy_accumulation",
            ScanProfile::Full,
            "src/lib.rs",
            "fn collect(items: Vec<i32>) -> Vec<i32> {\n    let mut result: Vec<i32> = Vec::new();\n    for item in items { result = [result, vec![item]].concat(); }\n    result\n}\n",
            "src/lib.rs",
            "fn collect(items: Vec<i32>) -> Vec<i32> {\n    let mut result: Vec<i32> = Vec::new();\n    for item in items { result.push(item); }\n    result\n}\n",
        )
        .negative_files(vec![
            file("src/string.rs", "fn render(items: Vec<String>) -> String {\n    let mut result = String::new();\n    for item in items { result = format!(\"{result}{item}\"); }\n    result\n}\n"),
            file("src/conditional.rs", "fn collect(items: Vec<i32>) -> Vec<i32> {\n    let mut result: Vec<i32> = Vec::new();\n    for item in items { if item > 0 { result = [result, vec![item]].concat(); } }\n    result\n}\n"),
        ]),
        CapabilityCase::new(
            "invariant_collection_copy_in_loop",
            ScanProfile::Full,
            "src/lib.rs",
            "fn inspect(requests: Vec<i32>, items: Vec<i32>) {\n    for request in requests { consume(request, items.clone().len()); }\n}\n",
            "src/lib.rs",
            "fn inspect(requests: Vec<i32>, items: Vec<i32>) {\n    for request in requests { consume(request, items.len()); }\n}\n",
        )
        .negative_files(vec![
            file("src/mutated.rs", "fn inspect(requests: Vec<i32>, mut items: Vec<i32>) {\n    for request in requests { items.push(request); consume(items.clone().len()); }\n}\n"),
            file("src/custom.rs", "fn inspect(requests: Vec<i32>, items: Vec<Custom>) {\n    for request in requests { consume(request, items.clone().len()); }\n}\n"),
        ]),
        CapabilityCase::new(
            "repeated_recursive_subproblem",
            ScanProfile::Full,
            "src/lib.rs",
            "fn fib(n: u64) -> u64 {\n    if n < 2 { return n; }\n    fib(n - 1) + fib(n - 2)\n}\n",
            "src/lib.rs",
            "fn factorial(n: u64) -> u64 {\n    if n < 2 { return 1; }\n    n * factorial(n - 1)\n}\n",
        )
        .negative_files(vec![
            file("src/branched.rs", "fn choose(n: u64, left: bool) -> u64 {\n    if n < 2 { return n; }\n    if left { choose(n - 1, left) } else { choose(n - 2, left) }\n}\n"),
            file("src/memo.rs", "fn fib(n: u64, memo: &mut Cache) -> u64 {\n    if n < 2 { return n; }\n    fib(n - 1, memo) + fib(n - 2, memo)\n}\n"),
        ]),
        CapabilityCase::new(
            "nested_cross_collection_traversal",
            ScanProfile::Full,
            "src/lib.rs",
            "fn match_items(left: Vec<Item>, right: Vec<Item>) {\n    for outer in left {\n        for inner in &right { if outer.id == inner.id { consume(&outer, inner); } }\n    }\n}\n",
            "src/lib.rs",
            "fn visit(left: Vec<Item>, right: Vec<Item>) {\n    for outer in left { consume(outer); }\n    for inner in right { consume(inner); }\n}\n",
        )
        .negative_files(vec![
            file("src/cartesian.rs", "fn pairs(left: Vec<i32>, right: Vec<i32>) -> Vec<(i32, i32)> {\n    let mut pairs = Vec::new();\n    for outer in left { for inner in &right { pairs.push((outer, *inner)); } }\n    pairs\n}\n"),
            file("src/bounded.rs", "fn compare(left: Vec<i32>) {\n    let right = [1, 2, 3];\n    for outer in left { for inner in right { consume(outer, inner); } }\n}\n"),
        ]),
        CapabilityCase::new(
            "manual_collection_copy_loop",
            ScanProfile::Full,
            "src/lib.rs",
            "fn copy(source: &[u8], target: &mut [u8]) {\n    for i in 0..source.len() {\n        target[i] = source[i];\n    }\n}\n",
            "src/lib.rs",
            "fn transform(source: &[u8], target: &mut [u8]) {\n    for i in 0..source.len() {\n        target[i] = source[i] + 1;\n    }\n}\n",
        ),
        CapabilityCase::new(
            "eager_fallback_computation",
            ScanProfile::Full,
            "src/lib.rs",
            "fn resolve(value: Option<String>) -> String { value.unwrap_or(String::from(\"fallback\")) }\n",
            "src/lib.rs",
            "fn resolve(value: Option<String>) -> String { value.unwrap_or_else(expensive_default) }\n",
        ),
        CapabilityCase::new(
            "intermediate_collection_materialization",
            ScanProfile::Full,
            "src/lib.rs",
            "fn count(items: &[i32]) -> usize { items.iter().filter(|item| **item > 0).collect::<Vec<_>>().len() }\n",
            "src/lib.rs",
            "fn count(items: &[i32]) -> usize { items.iter().filter(|item| **item > 0).count() }\n",
        )
        .negative_files(vec![file(
            "src/side_effect.py",
            "def total(items: list[int]):\n    return sum([transform(item) for item in items])\n",
        )]),
        CapabilityCase::new(
            "range_for_each_iteration",
            ScanProfile::Full,
            "src/Main.kt",
            "fun emit(limit: Int) {\n    (0 until limit).forEach { value -> consume(value) }\n}\n",
            "src/Main.kt",
            "fun emit(limit: Int) {\n    for (value in 0 until limit) { consume(value) }\n}\n",
        ),
        CapabilityCase::new(
            "loop_invariant_computation",
            ScanProfile::Full,
            "src/lib.rs",
            "fn render(items: &[i32], limit: i32) {\n    for item in items { consume(item, limit.abs()); }\n}\n",
            "src/lib.rs",
            "fn render(items: &[i32]) {\n    for item in items { consume(item, item.abs()); }\n}\n",
        )
        .negative_files(vec![
            file("src/mutated.rs", "fn render(items: &[i32], mut limit: i32) {\n    for item in items { limit += item; consume(limit.abs()); }\n}\n"),
            file("src/effect.rs", "fn render(items: &[i32], limit: i32) {\n    for item in items { consume(item, calculate(limit)); }\n}\n"),
        ]),
        CapabilityCase::new(
            "known_size_preallocation",
            ScanProfile::Full,
            "source.go",
            "package parity\n\nfunc copyValues(items []int) []int {\n    result := make([]int, 0)\n    for _, item := range items {\n        result = append(result, item)\n    }\n    return result\n}\n",
            "source.go",
            "package parity\n\nfunc copyValues(items []int) []int {\n    result := make([]int, 0, len(items))\n    for _, item := range items { result = append(result, item) }\n    return result\n}\n",
        )
        .negative_files(vec![
            file("source.go", "package parity\n\nfunc filtered(items []int) []int {\n    result := make([]int, 0)\n    for _, item := range items { if item > 0 { result = append(result, item) } }\n    return result\n}\n"),
            file("unknown.go", "package parity\n\nfunc grow() []int {\n    result := make([]int, 0)\n    for next() { result = append(result, read()) }\n    return result\n}\n"),
            file("nested.go", "package parity\n\nfunc duplicate(items []int) []int {\n    result := make([]int, 0)\n    for _, item := range items { for i := 0; i < 2; i++ { result = append(result, item) } }\n    return result\n}\n"),
        ]),
        CapabilityCase::new(
            "bulk_collection_extension",
            ScanProfile::Full,
            "src/lib.rs",
            "fn extend_all(items: &[String], target: &mut Vec<String>) {\n    for item in items { target.push(item.clone()); }\n}\n",
            "src/lib.rs",
            "fn extend_all(items: &[String], target: &mut Vec<String>) { target.extend(items.iter().cloned()); }\n",
        )
        .negative_files(vec![
            file("src/transform.rs", "fn extend_all(items: &[String], target: &mut Vec<String>) {\n    for item in items { target.push(item.to_uppercase()); }\n}\n"),
            file("src/filter.rs", "fn extend_all(items: &[String], target: &mut Vec<String>) {\n    for item in items { if !item.is_empty() { target.push(item.clone()); } }\n}\n"),
            file("src/side_effect.rs", "fn extend_all(items: &[String], target: &mut Vec<String>) {\n    for item in items { consume(item); target.push(item.clone()); }\n}\n"),
            file("src/nested.rs", "fn extend_all(items: &[String], target: &mut Vec<String>) {\n    for item in items { for _ in 0..2 { target.push(item.clone()); } }\n}\n"),
        ]),
        CapabilityCase::new(
            "map_entry_opportunity",
            ScanProfile::Full,
            "src/lib.rs",
            "use std::collections::HashMap;\nfn insert_missing(map: &mut HashMap<String, usize>, key: String) {\n    if !map.contains_key(&key) {\n        map.insert(key, String::from(\"value\").len());\n    }\n}\n",
            "src/lib.rs",
            "use std::collections::HashMap;\nfn insert_missing(map: &mut HashMap<String, usize>, key: String) {\n    map.entry(key).or_insert_with(|| String::from(\"value\").len());\n}\n",
        )
        .negative_files(vec![
            file("src/overwrite.rs", "use std::collections::HashMap;\nfn overwrite(map: &mut HashMap<String, usize>, key: String) {\n    if map.contains_key(&key) { map.insert(key, 1); }\n}\n"),
            file("src/distinct.rs", "use std::collections::HashMap;\nfn insert_other(map: &mut HashMap<String, usize>, key: String, other: String) {\n    if !map.contains_key(&key) { map.insert(other, 1); }\n}\n"),
            file("src/effect.rs", "use std::collections::HashMap;\nfn insert_effect(map: &mut HashMap<String, usize>, key: String) {\n    if !map.contains_key(&key) { map.insert(key, calculate()); }\n}\n"),
        ]),
    ]
}
