//! C# capability boundaries for the L3 collection facts: `List<T>`, LINQ,
//! indexers, and .NET `Regex`. Each negative is the nearest correct form of
//! its positive, and covers the C# spellings the facts must not misread
//! (`First(predicate)`, `Any()` without one, `Remove(value)`).

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
