//! C# capability boundaries for the L3 collection facts (`List<T>`, LINQ,
//! indexers, .NET `Regex`), the L4 effects (`System.IO`, `HttpClient`,
//! JSON, `async` methods), and the L5 receiver facts (`Math`, `string`,
//! `List<T>` capacity and `AddRange`, `TryAdd`, `GetValueOrDefault`). Each negative is the nearest correct form of its
//! positive, and covers the C# spellings the facts must not misread
//! (`First(predicate)`, `Any()` without one, `Remove(value)`, the `Async`
//! file APIs).

pub(super) fn csharp_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (usings, positive_body, negative_body): (&str, &str, &str) = match detector {
        "repeated_sort_in_loop" => (
            "",
            "void Inspect(List<Request> requests, List<int> candidates) {\n        foreach (var request in requests) {\n            candidates.Sort();\n            Consume(request, candidates[0]);\n        }\n    }",
            "void Inspect(List<List<int>> groups) {\n        foreach (var group in groups) {\n            var values = new List<int>(group);\n            values.Sort();\n            Consume(values);\n        }\n    }",
        ),
        "sort_for_extremum" => (
            "",
            "int Smallest(List<int> items) {\n        items.Sort();\n        return items[0];\n    }\n\n    int Largest(List<int> items) {\n        items.Sort();\n        return items[items.Count - 1];\n    }",
            "List<int> Ordered(List<int> items) {\n        items.Sort();\n        return items;\n    }\n\n    int FirstLarge(List<int> items) {\n        items.Sort();\n        return items.First(item => item > 3);\n    }",
        ),
        "full_sort_for_top_k" => (
            "",
            "List<int> Smallest(List<int> items) {\n        items.Sort();\n        return items.GetRange(0, 5);\n    }",
            "int Smallest(List<int> items) {\n        items.Sort();\n        return items[0];\n    }",
        ),
        "repeated_collection_traversal" => (
            "",
            "void Inspect(List<Item> items) {\n        Consume(items.Any(item => item.Ready));\n        Consume(items.All(item => item.Valid));\n        Consume(items.Count(item => item.Target));\n    }",
            "void Distinct(List<Item> ready, List<Item> valid, List<Item> targets) {\n        Consume(ready.Any(item => item.Ready));\n        Consume(valid.All(item => item.Valid));\n        Consume(targets.Count(item => item.Target));\n    }\n\n    void Changed(List<Item> items) {\n        Consume(items.Any(item => item.Ready));\n        Mutate(items);\n        Consume(items.All(item => item.Valid));\n        Consume(items.Count(item => item.Target));\n    }\n\n    void Counted(List<Item> items) {\n        Consume(items.Any());\n        Consume(items.Count());\n        Consume(items.Any());\n    }",
        ),
        "hot_path_linear_membership" => (
            "",
            "void Choose(List<string> ids, List<string> wanted) {\n        foreach (var id in ids) {\n            if (wanted.Contains(id)) {\n                Consume(id);\n            }\n        }\n    }",
            "void Choose(List<string> ids, HashSet<string> wanted) {\n        foreach (var id in ids) {\n            if (wanted.Contains(id)) {\n                Consume(id);\n            }\n        }\n    }",
        ),
        "n_plus_one_query_in_loop" => (
            "",
            "void Load(List<int> ids, Repository repo) {\n        foreach (var id in ids) {\n            Consume(repo.FindById(id));\n        }\n    }",
            "void Load(List<int> ids, Repository repo) {\n        Consume(repo.FindAllById(ids));\n    }",
        ),
        "loop_carried_copy_accumulation" => (
            "",
            "List<int> Collect(List<int> items) {\n        var result = new List<int>();\n        foreach (var item in items) {\n            result = result.Concat(new[] { item }).ToList();\n        }\n        return result;\n    }",
            "List<int> Collect(List<int> items) {\n        var result = new List<int>();\n        foreach (var item in items) {\n            result.Add(item);\n        }\n        return result;\n    }",
        ),
        "nested_quadratic_iteration" => (
            "",
            "void Pairs(List<Item> items) {\n        foreach (var left in items) {\n            foreach (var right in items) {\n                if (left.Id == right.Id) {\n                    Consume(left, right);\n                }\n            }\n        }\n    }",
            "void Pairs(Options options) {\n        foreach (var left in options.Left) {\n            foreach (var right in options.Right) {\n                Consume(left, right);\n            }\n        }\n    }",
        ),
        "nested_cross_collection_traversal" => (
            "",
            "void Compare(List<Item> left, List<Item> right) {\n        foreach (var outer in left) {\n            foreach (var inner in right) {\n                if (outer.Id == inner.Id) {\n                    Consume(outer, inner);\n                }\n            }\n        }\n    }",
            "void Visit(List<Item> left, List<Item> right) {\n        foreach (var outer in left) {\n            Consume(outer);\n        }\n        foreach (var inner in right) {\n            Consume(inner);\n        }\n    }\n\n    void Derived(List<Item> left) {\n        foreach (var outer in left) {\n            var parts = outer.Parts();\n            foreach (var inner in parts) {\n                if (outer.Id == inner.Id) {\n                    Consume(outer, inner);\n                }\n            }\n        }\n    }",
        ),
        "repeated_recursive_subproblem" => (
            "",
            "int Fib(int n) {\n        if (n < 2) {\n            return n;\n        }\n        return Fib(n - 1) + Fib(n - 2);\n    }",
            "int Factorial(int n) {\n        if (n < 2) {\n            return 1;\n        }\n        return n * Factorial(n - 1);\n    }",
        ),
        "quadratic_front_removal" => (
            "",
            "void Drain(List<int> queue) {\n        while (queue.Count > 0) {\n            Consume(queue[0]);\n            queue.RemoveAt(0);\n        }\n    }",
            "void Drain(Queue<int> queue, List<int> values) {\n        while (queue.Count > 0) {\n            Consume(queue.Dequeue());\n        }\n        while (values.Count > 0) {\n            values.Remove(0);\n        }\n    }",
        ),
        "catastrophic_backtracking_regex" => (
            "using System.Text.RegularExpressions;\n",
            "bool Check(string input) {\n        return new Regex(@\"^(\\w+\\s?)+$\").IsMatch(input) || Regex.IsMatch(input, \"^(a+)+$\");\n    }",
            "bool Check(string input, Regex words) {\n        return new Regex(\"^(a+)+$\", RegexOptions.NonBacktracking).IsMatch(input)\n            || Regex.IsMatch(input, @\"^\\w+$\")\n            || words.Replace(input, \"(a+)+\") == input;\n    }",
        ),
        "loop_artifact_rewrite" => (
            "using System.IO;\n",
            "void Rewrite(List<string> paths) {\n        foreach (var path in paths) {\n            var raw = File.ReadAllText(path);\n            File.WriteAllText(\"report.json\", raw);\n        }\n    }",
            "void Rewrite(List<string> paths) {\n        var raw = File.ReadAllText(\"input\");\n        foreach (var path in paths) {\n            Consume(path, raw);\n        }\n    }",
        ),
        "latest_only_unbounded_cache" => (
            "",
            "void Update(List<Entry> cache, Entry entry) {\n        cache.Add(entry);\n        Consume(cache[cache.Count - 1]);\n    }",
            "void Update(List<Entry> cache, Entry entry) {\n        cache.Add(entry);\n        cache.RemoveAt(0);\n        Consume(cache[cache.Count - 1]);\n    }",
        ),
        "repeated_parse_scan_serialize" => (
            "using System.Text.Json;\n",
            "void Process(List<string> items) {\n        foreach (var item in items) {\n            var parsed = JsonSerializer.Deserialize<Record>(item);\n            Scan(parsed);\n            Write(JsonSerializer.Serialize(parsed));\n        }\n    }",
            "void Process(List<string> items) {\n        foreach (var item in items) {\n            var parsed = JsonSerializer.Deserialize<Record>(item);\n            Write(JsonSerializer.Serialize(parsed));\n        }\n    }",
        ),
        "blocking_io_in_async_context" => (
            "using System.IO;\nusing System.Threading.Tasks;\n",
            "async Task<string> Load(string path) {\n        await Task.Yield();\n        return File.ReadAllText(path);\n    }",
            "async Task<string> Load(string path) {\n        return await File.ReadAllTextAsync(path);\n    }\n\n    string LoadNow(string path) {\n        return File.ReadAllText(path);\n    }",
        ),
        "per_item_external_io" => (
            "using System.Net.Http;\nusing System.Threading.Tasks;\n",
            "async Task FetchAll(List<string> urls, HttpClient client) {\n        foreach (var url in urls) {\n            Consume(await client.GetStringAsync(url));\n        }\n    }",
            "async Task FetchAll(string batch, HttpClient client) {\n        Consume(await client.GetStringAsync(batch));\n    }\n\n    async Task FetchPages(List<string> pages, HttpClient client) {\n        foreach (var page in pages) {\n            Consume(await client.GetStringAsync(page));\n        }\n    }",
        ),
        "repeated_expensive_initialization" => (
            "using System.Text.RegularExpressions;\n",
            "void Validate(List<string> items) {\n        foreach (var item in items) {\n            var parser = new Regex(\"[a-z]+\");\n            Consume(parser, item);\n        }\n    }",
            "void Validate(List<string> items) {\n        var parser = new Regex(\"[a-z]+\");\n        foreach (var item in items) {\n            Consume(parser, item);\n        }\n    }\n\n    Regex Read(Reader reader) {\n        while (reader.Read()) {\n            if (reader.Done) {\n                return new Regex(\"[a-z]+\");\n            }\n        }\n        return null;\n    }",
        ),
        "request_path_repository_scan" => (
            "using System.IO;\n",
            "void HandleRequest() {\n        Consume(Directory.GetFiles(\".\"));\n    }",
            "void RefreshIndex() {\n        Consume(Directory.GetFiles(\".\"));\n    }",
        ),
        "cache_miss_opportunity" => (
            "",
            "bool Inspect(double data) {\n        return Math.Abs(data) == Math.Abs(data) && Math.Abs(data) > 0;\n    }",
            "bool Inspect(double data) {\n        return Math.Abs(data) == Math.Abs(data);\n    }",
        ),
        "repeated_format_allocation" => (
            "",
            "void Render(List<int> items, string mode) {\n        foreach (var item in items) {\n            Consume(item, string.Format(\"mode={0}\", mode));\n        }\n    }",
            "void Render(List<int> items) {\n        foreach (var item in items) {\n            Consume(string.Format(\"{0}\", item));\n        }\n    }",
        ),
        "loop_invariant_computation" => (
            "",
            "void Render(List<double> items, double limit) {\n        foreach (var item in items) {\n            Consume(item, Math.Abs(limit));\n        }\n    }",
            "void Render(List<double> items) {\n        foreach (var item in items) {\n            Consume(item, Math.Abs(item));\n        }\n    }",
        ),
        "loop_invariant_regex_compilation" => (
            "using System.Text.RegularExpressions;\n",
            "void Inspect(string[] items) {\n        foreach (var item in items) {\n            var pattern = new Regex(\"^fixed$\");\n            Consume(pattern, item);\n        }\n    }",
            "void Inspect(string[] items) {\n        var pattern = new Regex(\"^fixed$\");\n        foreach (var item in items) {\n            Consume(pattern, item);\n        }\n    }\n\n    void Cached(string[] items) {\n        Regex pattern = null;\n        foreach (var item in items) {\n            if (pattern == null) {\n                pattern = new Regex(\"^fixed$\");\n            }\n            Consume(pattern, item);\n        }\n    }",
        ),
        "manual_collection_copy_loop" => (
            "",
            "int[] Duplicate(int[] source) {\n        var target = new int[source.Length];\n        for (int index = 0; index < source.Length; index++) {\n            target[index] = source[index];\n        }\n        return target;\n    }",
            "int[] Duplicate(int[] source) {\n        var target = new int[source.Length];\n        for (int index = 0; index < source.Length; index++) {\n            target[index] = source[index] * 2;\n        }\n        return target;\n    }",
        ),
        "immutable_string_concatenation_in_loop" => (
            "",
            "string Join(List<string> items) {\n        var result = \"\";\n        foreach (var item in items) {\n            result += item;\n        }\n        return result;\n    }",
            "string Join(List<string> items) {\n        var result = new System.Text.StringBuilder();\n        foreach (var item in items) {\n            result.Append(item);\n        }\n        return result.ToString();\n    }\n\n    int Total(List<int> items) {\n        var total = 0;\n        foreach (var item in items) {\n            total += item;\n        }\n        return total;\n    }",
        ),
        "intermediate_collection_materialization" => (
            "",
            "int Count(List<int> items) {\n        return items.Where(item => item > 0).ToList().Count;\n    }",
            "int Count(List<int> items) {\n        return items.Count(item => item > 0);\n    }\n\n    bool Any(List<int> items) {\n        return items.Where(item => Check(item)).ToList().Any();\n    }",
        ),
        "range_for_each_iteration" => (
            "",
            "void Visit(int count) {\n        Enumerable.Range(0, count).ToList().ForEach(i => Consume(i));\n    }",
            "void Visit(List<int> items, int count) {\n        items.ForEach(i => Consume(i));\n        for (int i = 0; i < count; i++) {\n            Consume(i);\n        }\n    }",
        ),
        "known_size_preallocation" => (
            "",
            "List<string> Render(List<int> items) {\n        var result = new List<string>();\n        foreach (var item in items) {\n            result.Add(item.ToString());\n        }\n        return result;\n    }",
            "List<string> Render(List<int> items) {\n        var result = new List<string>(items.Count);\n        foreach (var item in items) {\n            result.Add(item.ToString());\n        }\n        return result;\n    }\n\n    List<string> Filtered(List<int> items) {\n        var result = new List<string>();\n        foreach (var item in items) {\n            if (item > 0) {\n                result.Add(item.ToString());\n            }\n        }\n        return result;\n    }",
        ),
        "bulk_collection_extension" => (
            "",
            "List<string> ExtendAll(List<string> target, List<string> items) {\n        foreach (var item in items) {\n            target.Add(item);\n        }\n        return target;\n    }",
            "List<string> ExtendAll(List<string> target, List<string> items) {\n        foreach (var item in items) {\n            target.Add(item.Trim());\n        }\n        return target;\n    }\n\n    void Extend(IList<string> target, IEnumerable<string> items) {\n        foreach (var item in items) {\n            target.Add(item);\n        }\n    }",
        ),
        "map_entry_opportunity" => (
            "",
            "void InsertMissing(Dictionary<string, int> map, string key) {\n        if (!map.ContainsKey(key)) {\n            map[key] = \"value\".Length;\n        }\n    }\n\n    void AddMissing(Dictionary<string, int> map, string key) {\n        if (!map.ContainsKey(key)) {\n            map.Add(key, 1);\n        }\n    }",
            "void Overwrite(Dictionary<string, int> map, string key, string other) {\n        if (map.ContainsKey(key)) {\n            map[key] = 1;\n        }\n        if (!map.ContainsKey(key)) {\n            map[other] = 1;\n        }\n        if (!map.ContainsKey(key)) {\n            map.Add(key, Compute());\n        }\n    }",
        ),
        "eager_fallback_computation" => (
            "",
            "List<string> Resolve(Dictionary<string, List<string>> map, string key) {\n        return map.GetValueOrDefault(key, new List<string>());\n    }",
            "List<string> Resolve(Dictionary<string, List<string>> map, string key) {\n        return map.GetValueOrDefault(key) ?? new List<string>();\n    }\n\n    int Count(Dictionary<string, int> counts, string key) {\n        return counts.GetValueOrDefault(key, 0);\n    }",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/Capability/Service.cs".to_string(),
        format!(
            "using System;\nusing System.Collections.Generic;\nusing System.Linq;\n{usings}\nnamespace Capability;\n\nclass Service {{\n    {body}\n}}\n"
        ),
    )])
}
