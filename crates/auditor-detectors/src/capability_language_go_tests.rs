//! Go capability boundaries for the catalog language-claim tests.

/// Go boundaries for rules resolved through API identity. Each negative is
/// the nearest safe form of its positive, through the same import.
pub(super) fn go_capability_files(detector: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let (imports, positive_body, negative_body) = match detector {
        "shell_execution" => (
            "\"os/exec\"",
            "_ = exec.Command(\"sh\", \"-c\", command).Run()",
            "_ = exec.Command(\"git\", \"status\", command).Run()",
        ),
        "exit_in_production" => ("\"os\"", "os.Exit(1)", "_ = os.Getpid()"),
        "todo_unimplemented_in_production" => (
            "\"errors\"",
            "panic(\"not implemented\")",
            "panic(\"invariant violated\")",
        ),
        "console_output_in_production" => (
            "\"fmt\"",
            "fmt.Println(command)",
            "_ = fmt.Sprintf(\"%s\", command)",
        ),
        "bind_all_interfaces" => (
            "\"net\"",
            "_, _ = net.Listen(\"tcp\", \":8080\")",
            "_, _ = net.Listen(\"tcp\", \"127.0.0.1:8080\")",
        ),
        "world_writable_file" => (
            "\"os\"",
            "_ = os.WriteFile(command, nil, 0o666)",
            "_ = os.WriteFile(command, nil, 0o644)",
        ),
        "random_for_security" => (
            "random \"math/rand\"",
            "_ = random.Intn(10)",
            "_ = len(command)",
        ),
        "request_no_cert_verify" => (
            "\"crypto/tls\"",
            "_ = &tls.Config{InsecureSkipVerify: true}",
            "_ = &tls.Config{InsecureSkipVerify: false}",
        ),
        "ssl_with_bad_defaults" => (
            "\"crypto/tls\"",
            "_ = &tls.Config{MinVersion: tls.VersionTLS10}",
            "_ = &tls.Config{MinVersion: tls.VersionTLS12}",
        ),
        "ssh_no_host_key_verification" => (
            "\"golang.org/x/crypto/ssh\"",
            "_ = ssh.InsecureIgnoreHostKey()",
            "_ = ssh.FixedHostKey(nil)",
        ),
        "resource_not_closed" => (
            "\"os\"",
            "file, _ := os.Open(command)\n\t_, _ = file.Stat()",
            "file, _ := os.Open(command)\n\tdefer file.Close()\n\t_, _ = file.Stat()",
        ),
        "empty_conditional_branch" => (
            "\"strings\"",
            "if strings.TrimSpace(command) == \"\" {\n\t} else {\n\t\t_ = command\n\t}",
            "if strings.TrimSpace(command) != \"\" {\n\t\t_ = command\n\t}\n\tif command == \"\" {\n\t\t// the caller handles empty input\n\t}",
        ),
        "hardcoded_credential" => (
            "\"os\"",
            "apiKey := \"tok_9f8e7d6c5b4a3210\"\n\t_ = apiKey",
            "apiKeyEnv := \"SERVICE_API_KEY\"\n\t_ = os.Getenv(apiKeyEnv)",
        ),
        "hardcoded_tmp_directory" => (
            "(\n\t\"os\"\n\t\"path/filepath\"\n)",
            "_ = filepath.Join(\"/tmp/scratch\", command)",
            "_ = filepath.Join(os.TempDir(), command)",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/capability.go".to_string(),
        format!("package capability\n\nimport {imports}\n\nfunc inspect(command string) {{\n\t{body}\n}}\n"),
    )])
}

/// Go boundaries for the L3 collection, L4 effect and topology, and L5
/// receiver facts. Each negative is the nearest correct form of its positive.
pub(super) fn go_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if let Some(files) = go_topology_files(detector, positive) {
        return Some(files);
    }
    let (imports, positive_body, negative_body): (&str, &str, &str) = match detector {
        "repeated_sort_in_loop" => (
            "\"sort\"",
            "func inspect(requests []Request, candidates []int) {\n\tfor _, request := range requests {\n\t\tsort.Ints(candidates)\n\t\tconsume(request, candidates[0])\n\t}\n}",
            "func inspect(groups [][]int) {\n\tfor _, group := range groups {\n\t\tvalues := append([]int(nil), group...)\n\t\tsort.Ints(values)\n\t\tconsume(values)\n\t}\n}",
        ),
        "sort_for_extremum" => (
            "\"sort\"",
            "func smallest(items []int) int {\n\tsort.Ints(items)\n\treturn items[0]\n}",
            "func ordered(items []int) []int {\n\tsort.Ints(items)\n\treturn items\n}",
        ),
        "full_sort_for_top_k" => (
            "\"sort\"",
            "func smallest(items []int) []int {\n\tsort.Ints(items)\n\treturn items[:5]\n}",
            "func smallest(items []int) int {\n\tsort.Ints(items)\n\treturn items[0]\n}",
        ),
        "repeated_collection_traversal" => (
            "\"slices\"",
            "func inspect(items []Item) {\n\tconsume(slices.ContainsFunc(items, func(item Item) bool { return item.Ready }))\n\tconsume(slices.ContainsFunc(items, func(item Item) bool { return item.Valid }))\n\tconsume(slices.IndexFunc(items, func(item Item) bool { return item.Target }))\n}",
            "func distinct(ready []Item, valid []Item, targets []Item) {\n\tconsume(slices.ContainsFunc(ready, func(item Item) bool { return item.Ready }))\n\tconsume(slices.ContainsFunc(valid, func(item Item) bool { return item.Valid }))\n\tconsume(slices.IndexFunc(targets, func(item Item) bool { return item.Target }))\n}\n\nfunc changed(items []Item) {\n\tconsume(slices.ContainsFunc(items, func(item Item) bool { return item.Ready }))\n\tmutate(items)\n\tconsume(slices.ContainsFunc(items, func(item Item) bool { return item.Valid }))\n\tconsume(slices.IndexFunc(items, func(item Item) bool { return item.Target }))\n}",
        ),
        "hot_path_linear_membership" => (
            "\"slices\"",
            "func choose(ids []string, wanted []string) {\n\tfor _, id := range ids {\n\t\tif slices.Contains(wanted, id) {\n\t\t\tconsume(id)\n\t\t}\n\t}\n}",
            "func choose(ids []string, wanted map[string]bool) {\n\tfor _, id := range ids {\n\t\tif wanted[id] {\n\t\t\tconsume(id)\n\t\t}\n\t}\n}",
        ),
        "n_plus_one_query_in_loop" => (
            "",
            "func load(ids []int, repo Repository) {\n\tfor _, id := range ids {\n\t\tconsume(repo.FindByID(id))\n\t}\n}",
            "func load(ids []int, repo Repository) {\n\tconsume(repo.FindMany(ids))\n}",
        ),
        "nested_cross_collection_traversal" => (
            "",
            "func compare(left []Item, right []Item) {\n\tfor _, outer := range left {\n\t\tfor _, inner := range right {\n\t\t\tif outer.ID == inner.ID {\n\t\t\t\tconsume(outer, inner)\n\t\t\t}\n\t\t}\n\t}\n}",
            "func visit(left []Item, right []Item) {\n\tfor _, outer := range left {\n\t\tconsume(outer)\n\t}\n\tfor _, inner := range right {\n\t\tconsume(inner)\n\t}\n}",
        ),
        "loop_artifact_rewrite" => (
            "\"os\"",
            "func rewrite(paths []string) {\n\tfor _, path := range paths {\n\t\traw, _ := os.ReadFile(path)\n\t\t_ = os.WriteFile(\"report.json\", raw, 0o644)\n\t}\n}",
            "func rewrite(paths []string) {\n\traw, _ := os.ReadFile(\"input\")\n\tfor _, path := range paths {\n\t\tconsume(path, raw)\n\t}\n}",
        ),
        "latest_only_unbounded_cache" => (
            "",
            "func update(cache []Entry, entry Entry) []Entry {\n\tcache = append(cache, entry)\n\tconsume(cache[len(cache)-1])\n\treturn cache\n}",
            "func update(cache []Entry, entry Entry) []Entry {\n\tcache = append(cache, entry)\n\tif len(cache) > 10 {\n\t\tcache = cache[1:]\n\t}\n\tconsume(cache[len(cache)-1])\n\treturn cache\n}",
        ),
        "repeated_parse_scan_serialize" => (
            "\"encoding/json\"",
            "func process(items [][]byte) {\n\tfor _, item := range items {\n\t\tvar parsed Record\n\t\t_ = json.Unmarshal(item, &parsed)\n\t\tscan(parsed)\n\t\tencoded, _ := json.Marshal(parsed)\n\t\twrite(encoded)\n\t}\n}",
            "func process(items [][]byte) {\n\tfor _, item := range items {\n\t\tvar parsed Record\n\t\t_ = json.Unmarshal(item, &parsed)\n\t\tencoded, _ := json.Marshal(parsed)\n\t\twrite(encoded)\n\t}\n}",
        ),
        "per_item_external_io" => (
            "\"net/http\"",
            "func fetchAll(urls []string) {\n\tfor _, url := range urls {\n\t\tresponse, _ := http.Get(url)\n\t\tconsume(response)\n\t}\n}",
            "func fetchAll(batch string) {\n\tresponse, _ := http.Get(batch)\n\tconsume(response)\n}",
        ),
        "repeated_expensive_initialization" => (
            "\"regexp\"",
            "func validate(items []string) {\n\tfor _, item := range items {\n\t\tparser := regexp.MustCompile(\"[a-z]+\")\n\t\tconsume(parser, item)\n\t}\n}",
            "func validate(items []string) {\n\tparser := regexp.MustCompile(\"[a-z]+\")\n\tfor _, item := range items {\n\t\tconsume(parser, item)\n\t}\n}",
        ),
        "request_path_repository_scan" => (
            "\"os\"",
            "func handleRequest() {\n\tentries, _ := os.ReadDir(\".\")\n\tconsume(entries)\n}",
            "func refreshIndex() {\n\tentries, _ := os.ReadDir(\".\")\n\tconsume(entries)\n}",
        ),
        "cache_miss_opportunity" => (
            "\"math\"",
            "func inspect(data float64) bool {\n\treturn math.Abs(data) == math.Abs(data) && math.Abs(data) > 0\n}",
            "func inspect(data float64) bool {\n\treturn math.Abs(data) == math.Abs(data)\n}",
        ),
        "repeated_format_allocation" => (
            "\"fmt\"",
            "func render(items []int, mode string) {\n\tfor _, item := range items {\n\t\tconsume(item, fmt.Sprintf(\"mode=%s\", mode))\n\t}\n}",
            "func render(items []int) {\n\tfor _, item := range items {\n\t\tconsume(fmt.Sprintf(\"%d\", item))\n\t}\n}\n\nfunc annotate(name string, annotations map[string]string) {\n\tfor key := range annotations {\n\t\tswitch key {\n\t\tcase \"ext\":\n\t\t\tconsume(fmt.Sprintf(\"ext=%s\", name))\n\t\t}\n\t}\n}",
        ),
        "loop_invariant_regex_compilation" => (
            "\"regexp\"",
            "func inspect(items []string) {\n\tfor _, item := range items {\n\t\tpattern := regexp.MustCompile(\"^fixed$\")\n\t\tconsume(pattern, item)\n\t}\n}",
            "func inspect(items []string) {\n\tfor _, item := range items {\n\t\tpattern := regexp.MustCompile(item)\n\t\tconsume(pattern)\n\t}\n}",
        ),
        "loop_invariant_computation" => (
            "\"math\"",
            "func render(items []float64, limit float64) {\n\tfor _, item := range items {\n\t\tconsume(item, math.Abs(limit))\n\t}\n}",
            "func render(items []float64) {\n\tfor _, item := range items {\n\t\tconsume(item, math.Abs(item))\n\t}\n}",
        ),
        "bulk_collection_extension" => (
            "\"strings\"",
            "func extendAll(target []string, items []string) []string {\n\tfor _, item := range items {\n\t\ttarget = append(target, item)\n\t}\n\treturn target\n}",
            "func extendAll(target []string, items []string) []string {\n\tfor _, item := range items {\n\t\ttarget = append(target, strings.TrimSpace(item))\n\t}\n\treturn target\n}",
        ),
        "manual_collection_copy_loop" => (
            "",
            "func duplicate(source []int) []int {\n\ttarget := make([]int, len(source))\n\tfor index := range source {\n\t\ttarget[index] = source[index]\n\t}\n\treturn target\n}",
            "func duplicate(source []int) []int {\n\ttarget := make([]int, len(source))\n\tfor index := range source {\n\t\ttarget[index] = source[index] * 2\n\t}\n\treturn target\n}",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    let imports = if imports.is_empty() {
        String::new()
    } else {
        format!("import {imports}\n\n")
    };
    Some(vec![(
        "src/capability.go".to_string(),
        format!("package capability\n\n{imports}{body}\n"),
    )])
}

/// Go topology boundaries: packages of one module, resolved through `go.mod`.
fn go_topology_files(detector: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let module = (
        "go.mod".to_string(),
        "module example.com/capability\n\ngo 1.22\n".to_string(),
    );
    let file = |path: &str, source: &str| (path.to_string(), source.to_string());
    Some(match (detector, positive) {
        ("architectural_layering", _) => {
            let (import, target, target_source) = if positive {
                ("example.com/capability/src/app", "src/app/service.go", "package app\n\ntype UserService struct{}\n")
            } else {
                ("example.com/capability/src/domain/model", "src/domain/model/model.go", "package model\n\ntype User struct{}\n")
            };
            let package = import.rsplit('/').next().unwrap_or_default();
            let kind = if positive { "UserService" } else { "User" };
            vec![
                module,
                file(
                    "src/domain/user.go",
                    &format!("package domain\n\nimport \"{import}\"\n\nfunc Check() any {{ return {package}.{kind}{{}} }}\n"),
                ),
                file(target, target_source),
                file(
                    "auditor.toml",
                    "[layering.layer.domain]\nglob = src/domain/**/*.go\n\n[layering.layer.app]\nglob = src/app/**/*.go\n\n[layering.rule.domain]\ndeny = app\n",
                ),
            ]
        }
        ("package_coupling_metrics", true) => vec![
            module,
            file("packages/core/src/model.go", "package core\n\ntype Model struct{}\n"),
            file(
                "packages/app/src/service.go",
                "package app\n\nimport core \"example.com/capability/packages/core/src\"\n\ntype Processor interface{ Process(model core.Model) }\n\ntype Service struct{}\n",
            ),
            file(
                "packages/web/src/handler.go",
                "package web\n\nimport core \"example.com/capability/packages/core/src\"\n\ntype Handler struct{ model core.Model }\n",
            ),
        ],
        ("package_coupling_metrics", false) => vec![
            module,
            file("packages/core/src/model.go", "package core\n\ntype Model struct{}\n"),
        ],
        _ => return None,
    })
}
