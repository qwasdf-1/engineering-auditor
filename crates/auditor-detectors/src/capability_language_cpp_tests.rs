//! C++ capability boundaries for the L3 collection facts (STL algorithms
//! over iterator pairs and ranges, container mutators and selections,
//! `std::regex`), the L4 effects (declared constructions, nlohmann::json,
//! `std::filesystem`), and the L5 forms (`<cmath>`, `fmt::format`, `insert`
//! ranges, `try_emplace`, `value_or`). Each negative is the nearest correct form of its positive
//! (a comparator sort, an `std::fill` between traversals, a back removal).

pub(super) fn cpp_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_body, negative_body): (&str, &str) = match detector {
        "repeated_sort_in_loop" => (
            "void inspect(const std::vector<Request>& requests, std::vector<int>& candidates) {\n    for (const auto& request : requests) {\n        std::sort(candidates.begin(), candidates.end());\n        consume(request, candidates[0]);\n    }\n}",
            "void inspect(const std::vector<std::vector<int>>& groups) {\n    for (const auto& group : groups) {\n        std::vector<int> values = group;\n        std::sort(values.begin(), values.end());\n        consume(values);\n    }\n}",
        ),
        "sort_for_extremum" => (
            "int smallest(std::vector<int>& items) {\n    std::sort(items.begin(), items.end());\n    return items.front();\n}\n\nint largest(std::vector<int>& items) {\n    std::ranges::sort(items);\n    return items.back();\n}",
            "std::vector<int> ordered(std::vector<int>& items) {\n    std::sort(items.begin(), items.end());\n    return items;\n}\n\nint first_by(std::vector<int>& items) {\n    std::sort(items.begin(), items.end(), compare);\n    return items.front();\n}",
        ),
        "repeated_collection_traversal" => (
            "void inspect(const std::vector<Item>& items) {\n    consume(std::any_of(items.begin(), items.end(), [](const Item& item) { return item.ready; }));\n    consume(std::all_of(items.begin(), items.end(), [](const Item& item) { return item.valid; }));\n    consume(std::count_if(items.begin(), items.end(), [](const Item& item) { return item.target; }));\n}",
            "void distinct(const std::vector<Item>& ready, const std::vector<Item>& valid, const std::vector<Item>& targets) {\n    consume(std::any_of(ready.begin(), ready.end(), [](const Item& item) { return item.ready; }));\n    consume(std::all_of(valid.begin(), valid.end(), [](const Item& item) { return item.valid; }));\n    consume(std::count_if(targets.begin(), targets.end(), [](const Item& item) { return item.target; }));\n}\n\nvoid changed(std::vector<Item>& items) {\n    consume(std::any_of(items.begin(), items.end(), [](const Item& item) { return item.ready; }));\n    std::fill(items.begin(), items.end(), Item{});\n    consume(std::all_of(items.begin(), items.end(), [](const Item& item) { return item.valid; }));\n    consume(std::count_if(items.begin(), items.end(), [](const Item& item) { return item.target; }));\n}",
        ),
        "hot_path_linear_membership" => (
            "void choose(const std::vector<std::string>& ids, const std::vector<std::string>& wanted) {\n    for (const auto& id : ids) {\n        if (std::find(wanted.begin(), wanted.end(), id) != wanted.end()) {\n            consume(id);\n        }\n    }\n}",
            "void choose(const std::vector<std::string>& ids, const std::unordered_set<std::string>& wanted) {\n    for (const auto& id : ids) {\n        if (wanted.count(id) > 0) {\n            consume(id);\n        }\n    }\n}",
        ),
        "n_plus_one_query_in_loop" => (
            "void load(const std::vector<int>& ids, Repository& repo) {\n    for (int id : ids) {\n        consume(repo.find_by_id(id));\n    }\n}",
            "void load(const std::vector<int>& ids, Repository& repo) {\n    consume(repo.find_many(ids));\n}",
        ),
        "nested_quadratic_iteration" => (
            "void pairs(const std::vector<Item>& items) {\n    for (const auto& left : items) {\n        for (const auto& right : items) {\n            if (left.id == right.id) {\n                consume(left, right);\n            }\n        }\n    }\n}",
            "void pairs(const Options& options) {\n    for (const auto& left : options.left) {\n        for (const auto& right : options.right) {\n            consume(left, right);\n        }\n    }\n}",
        ),
        "nested_cross_collection_traversal" => (
            "void compare(const std::vector<Item>& left, const std::vector<Item>& right) {\n    for (const auto& outer : left) {\n        for (const auto& inner : right) {\n            if (outer.id == inner.id) {\n                consume(outer, inner);\n            }\n        }\n    }\n}",
            "void visit(const std::vector<Item>& left, const std::vector<Item>& right) {\n    for (const auto& outer : left) {\n        consume(outer);\n    }\n    for (const auto& inner : right) {\n        consume(inner);\n    }\n}",
        ),
        "repeated_recursive_subproblem" => (
            "int fib(int n) {\n    if (n < 2) {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}",
            "int factorial(int n) {\n    if (n < 2) {\n        return 1;\n    }\n    return n * factorial(n - 1);\n}",
        ),
        "quadratic_front_removal" => (
            "void drain(std::vector<int>& queue) {\n    while (!queue.empty()) {\n        consume(queue.front());\n        queue.erase(queue.begin());\n    }\n}",
            "void drain(std::deque<int>& queue, std::vector<int>& stack) {\n    while (!queue.empty()) {\n        consume(queue.front());\n        queue.pop_front();\n    }\n    while (!stack.empty()) {\n        stack.erase(stack.end() - 1);\n    }\n}",
        ),
        "catastrophic_backtracking_regex" => (
            "bool check(const std::string& input) {\n    std::regex words(R\"(^(\\w+\\s?)+$)\");\n    return std::regex_match(input, words) || std::regex_match(input, std::regex(\"^(a+)+$\"));\n}",
            "bool check(const std::string& input) {\n    std::regex words(R\"(^\\w+$)\");\n    return std::regex_match(input, words) || std::regex_match(input, std::regex(\"^a+$\"));\n}",
        ),
        "latest_only_unbounded_cache" => (
            "void update(std::vector<Entry>& cache, const Entry& entry) {\n    cache.push_back(entry);\n    consume(cache.back());\n}",
            "void update(std::vector<Entry>& cache, const Entry& entry) {\n    cache.push_back(entry);\n    cache.erase(cache.begin());\n    consume(cache.back());\n}",
        ),
        "repeated_parse_scan_serialize" => (
            "void process(const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        auto parsed = nlohmann::json::parse(item);\n        scan(parsed);\n        write(parsed.dump());\n    }\n}",
            "void process(const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        auto parsed = nlohmann::json::parse(item);\n        write(parsed.dump());\n    }\n}",
        ),
        "per_item_external_io" => (
            "void fetch_all(const std::vector<std::string>& urls, HttpClient& client) {\n    for (const auto& url : urls) {\n        consume(client.get(url));\n    }\n}",
            "void fetch_all(const std::string& batch, HttpClient& client) {\n    consume(client.get(batch));\n}\n\nvoid fetch_pages(const std::vector<std::string>& pages, HttpClient& client) {\n    for (const auto& page : pages) {\n        consume(client.get(page));\n    }\n}",
        ),
        "repeated_expensive_initialization" => (
            "void validate(const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        std::regex parser(\"[a-z]+\");\n        consume(parser, item);\n    }\n}",
            "void validate(const std::vector<std::string>& items) {\n    std::regex parser(\"[a-z]+\");\n    for (const auto& item : items) {\n        consume(parser, item);\n    }\n}\n\nstd::regex first(const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        if (item.empty()) {\n            return std::regex(\"[a-z]+\");\n        }\n    }\n    return std::regex(\"x\");\n}",
        ),
        "request_path_repository_scan" => (
            "void handle_request() {\n    for (const auto& entry : std::filesystem::directory_iterator(\".\")) {\n        consume(entry);\n    }\n}",
            "void refresh_index() {\n    for (const auto& entry : std::filesystem::directory_iterator(\".\")) {\n        consume(entry);\n    }\n}",
        ),
        "cache_miss_opportunity" => (
            "bool inspect(double data) {\n    return std::abs(data) == std::abs(data) && std::abs(data) > 0;\n}",
            "bool inspect(double data) {\n    return std::abs(data) == std::abs(data);\n}",
        ),
        "repeated_format_allocation" => (
            "void render(const std::vector<int>& items, const std::string& mode) {\n    for (int item : items) {\n        consume(item, fmt::format(\"mode={}\", mode));\n    }\n}",
            "void render(const std::vector<int>& items) {\n    for (int item : items) {\n        consume(fmt::format(\"{}\", item));\n    }\n}",
        ),
        "loop_invariant_computation" => (
            "void render(const std::vector<double>& items, double limit) {\n    for (double item : items) {\n        consume(item, std::abs(limit));\n    }\n}",
            "void render(const std::vector<double>& items) {\n    for (double item : items) {\n        consume(item, std::abs(item));\n    }\n}",
        ),
        "loop_invariant_regex_compilation" => (
            "void inspect(const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        std::regex pattern(\"^fixed$\");\n        consume(pattern, item);\n    }\n}",
            "void inspect(const std::vector<std::string>& items) {\n    std::regex pattern(\"^fixed$\");\n    for (const auto& item : items) {\n        consume(pattern, item);\n    }\n}",
        ),
        "manual_collection_copy_loop" => (
            "void duplicate(const std::vector<int>& source, std::vector<int>& target) {\n    for (size_t index = 0; index < source.size(); index++) {\n        target[index] = source[index];\n    }\n}",
            "void duplicate(const std::vector<int>& source, std::vector<int>& target) {\n    for (size_t index = 0; index < source.size(); index++) {\n        target[index] = source[index] * 2;\n    }\n}",
        ),
        "immutable_string_concatenation_in_loop" => (
            "std::string join(const std::vector<std::string>& items) {\n    std::string result = \"\";\n    for (const auto& item : items) {\n        result = result + item;\n    }\n    return result;\n}",
            "std::string join(const std::vector<std::string>& items) {\n    std::string result = \"\";\n    for (const auto& item : items) {\n        result += item;\n    }\n    return result;\n}",
        ),
        "bulk_collection_extension" => (
            "void extend_all(std::vector<std::string>& target, const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        target.push_back(item);\n    }\n}",
            "void extend_all(std::vector<std::string>& target, const std::vector<std::string>& items) {\n    for (const auto& item : items) {\n        target.push_back(trim(item));\n    }\n}",
        ),
        "map_entry_opportunity" => (
            "void insert_missing(std::map<std::string, int>& map, const std::string& key) {\n    if (map.find(key) == map.end()) {\n        map[key] = 1;\n    }\n}\n\nvoid emplace_missing(std::unordered_map<std::string, int>& map, const std::string& key) {\n    if (!map.count(key)) {\n        map.emplace(key, 1);\n    }\n}",
            "void overwrite(std::map<std::string, int>& map, const std::string& key) {\n    if (map.find(key) != map.end()) {\n        map[key] = 1;\n    }\n    if (map.find(key) == map.end()) {\n        map[key] = compute();\n    }\n}",
        ),
        "eager_fallback_computation" => (
            "std::vector<int> resolve(const std::optional<std::vector<int>>& value) {\n    return value.value_or(std::vector<int>{});\n}",
            "int resolve(const std::optional<int>& value) {\n    return value.value_or(0);\n}\n\nstd::string named(const std::optional<std::string>& value) {\n    return value.value_or(build_default());\n}",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/capability.cpp".to_string(),
        format!(
            "#include <algorithm>\n#include <deque>\n#include <regex>\n#include <string>\n#include <unordered_set>\n#include <vector>\n\n{body}\n"
        ),
    )])
}
