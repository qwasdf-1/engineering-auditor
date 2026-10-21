//! C capability boundaries for the L3–L5 facts C resolves: `qsort`, plain
//! recursion, cJSON, libcurl, POSIX `regcomp` and `opendir`, and `<math.h>`.
//! Each negative is the nearest correct form of its positive (a hoisted
//! sort or compilation, a sort of a fresh per-iteration copy).

pub(super) fn c_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_body, negative_body): (&str, &str) = match detector {
        "repeated_sort_in_loop" => (
            "void inspect(const struct request *requests, size_t count, int *candidates, size_t n) {\n    for (size_t i = 0; i < count; i++) {\n        qsort(candidates, n, sizeof(int), compare_ints);\n        consume(&requests[i], candidates[0]);\n    }\n}",
            "void inspect(int *candidates, size_t n) {\n    qsort(candidates, n, sizeof(int), compare_ints);\n    for (size_t i = 0; i < n; i++) {\n        consume(candidates[i]);\n    }\n}\n\nvoid groups(const int *groups, size_t count) {\n    for (size_t i = 0; i < count; i++) {\n        int values[16];\n        memcpy(values, &groups[i * 16], sizeof values);\n        qsort(values, 16, sizeof(int), compare_ints);\n        consume(values[0]);\n    }\n}",
        ),
        "sort_for_extremum" => (
            "int smallest(int *items, size_t n) {\n    qsort(items, n, sizeof(int), compare_ints);\n    return items[0];\n}",
            "void ordered(int *items, size_t n) {\n    qsort(items, n, sizeof(int), compare_ints);\n    emit(items, n);\n}",
        ),
        "repeated_recursive_subproblem" => (
            "int fib(int n) {\n    if (n < 2) {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}",
            "int factorial(int n) {\n    if (n < 2) {\n        return 1;\n    }\n    return n * factorial(n - 1);\n}",
        ),
        "repeated_parse_scan_serialize" => (
            "void process(char **items, size_t n) {\n    for (size_t i = 0; i < n; i++) {\n        cJSON *parsed = cJSON_Parse(items[i]);\n        scan(parsed);\n        write_out(cJSON_Print(parsed));\n    }\n}",
            "void process(char **items, size_t n) {\n    for (size_t i = 0; i < n; i++) {\n        cJSON *parsed = cJSON_Parse(items[i]);\n        write_out(cJSON_Print(parsed));\n    }\n}",
        ),
        "per_item_external_io" => (
            "void fetch_all(CURL *curl, char **urls, size_t n) {\n    for (size_t i = 0; i < n; i++) {\n        curl_easy_setopt(curl, CURLOPT_URL, urls[i]);\n        curl_easy_perform(curl);\n    }\n}",
            "void fetch_all(CURL *curl, const char *batch) {\n    curl_easy_setopt(curl, CURLOPT_URL, batch);\n    curl_easy_perform(curl);\n}",
        ),
        "repeated_expensive_initialization" => (
            "void validate(char **items, size_t n) {\n    regex_t re;\n    for (size_t i = 0; i < n; i++) {\n        regcomp(&re, \"[a-z]+\", REG_EXTENDED);\n        consume(&re, items[i]);\n        regfree(&re);\n    }\n}",
            "void validate(char **items, size_t n) {\n    regex_t re;\n    regcomp(&re, \"[a-z]+\", REG_EXTENDED);\n    for (size_t i = 0; i < n; i++) {\n        consume(&re, items[i]);\n    }\n    regfree(&re);\n}",
        ),
        "loop_invariant_regex_compilation" => (
            "void inspect(char **items, size_t n) {\n    regex_t re;\n    for (size_t i = 0; i < n; i++) {\n        regcomp(&re, \"^fixed$\", REG_EXTENDED);\n        consume(&re, items[i]);\n        regfree(&re);\n    }\n}",
            "void inspect(char **items, size_t n) {\n    regex_t re;\n    regcomp(&re, \"^fixed$\", REG_EXTENDED);\n    for (size_t i = 0; i < n; i++) {\n        consume(&re, items[i]);\n    }\n    regfree(&re);\n}",
        ),
        "request_path_repository_scan" => (
            "void handle_request(void) {\n    DIR *dir = opendir(\".\");\n    consume(dir);\n}",
            "void refresh_index(void) {\n    DIR *dir = opendir(\".\");\n    consume(dir);\n}",
        ),
        "cache_miss_opportunity" => (
            "int inspect(double data) {\n    return fabs(data) == fabs(data) && fabs(data) > 0;\n}",
            "int inspect(double data) {\n    return fabs(data) == fabs(data);\n}",
        ),
        "loop_invariant_computation" => (
            "void render(const double *items, size_t n, double limit) {\n    for (size_t i = 0; i < n; i++) {\n        consume(items[i], fabs(limit));\n    }\n}",
            "void render(const double *items, size_t n) {\n    for (size_t i = 0; i < n; i++) {\n        consume(items[i], fabs(items[i]));\n    }\n}",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/capability.c".to_string(),
        format!(
            "#include <dirent.h>\n#include <math.h>\n#include <regex.h>\n#include <stdlib.h>\n#include <string.h>\n#include <cjson/cJSON.h>\n#include <curl/curl.h>\n\n{body}\n"
        ),
    )])
}
