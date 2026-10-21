//! C++ capability boundaries for the L3 collection facts: STL algorithms
//! over iterator pairs and ranges, container mutators and selections, and
//! `std::regex`. Each negative is the nearest correct form of its positive
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
