//! Java capability boundaries for the catalog language-claim tests.

/// Java boundaries for rules resolved through API identity. Each negative is
/// the nearest safe form of its positive, through the same import.
pub(super) fn java_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector == "assert_in_production" {
        let source = if positive {
            "package capability;\n\npublic class Capability {\n    public void resize(int size) {\n        assert size > 0;\n    }\n}\n"
        } else {
            "package capability;\n\npublic class Capability {\n    private void resize(int size) {\n        assert size > 0;\n    }\n\n    public void grow(int size) {\n        if (size <= 0) throw new IllegalArgumentException(\"size\");\n    }\n}\n"
        };
        return Some(vec![(
            "src/main/java/capability/Capability.java".to_string(),
            source.to_string(),
        )]);
    }
    let (imports, positive_body, negative_body): (&str, &str, &str) = match detector {
        "shell_execution" => (
            "",
            "new ProcessBuilder(\"sh\", \"-c\", command).start();",
            "new ProcessBuilder(\"git\", \"status\", command).start();",
        ),
        "exit_in_production" => ("", "System.exit(1);", "Runtime.getRuntime().gc();"),
        "todo_unimplemented_in_production" => (
            "",
            "throw new UnsupportedOperationException(\"not implemented\");",
            "throw new UnsupportedOperationException(\"read-only view\");",
        ),
        "console_output_in_production" => (
            "",
            "System.out.println(command);",
            "String.format(\"%s\", command);",
        ),
        "eval_in_production" => (
            "import javax.script.ScriptEngineManager;",
            "new ScriptEngineManager().getEngineByName(\"js\").eval(command);",
            "new ScriptEngineManager().getEngineByName(\"js\").getFactory();",
        ),
        "unsafe_deserialization" => (
            "import java.io.ObjectInputStream;\nimport java.io.DataInputStream;",
            "new ObjectInputStream(input).readObject();",
            "new DataInputStream(input).readUTF();",
        ),
        "bind_all_interfaces" => (
            "import java.net.InetSocketAddress;",
            "new InetSocketAddress(\"0.0.0.0\", 8080);",
            "new InetSocketAddress(\"127.0.0.1\", 8080);",
        ),
        "world_writable_file" => (
            "import java.nio.file.attribute.PosixFilePermissions;",
            "PosixFilePermissions.fromString(\"rw-rw-rw-\");",
            "PosixFilePermissions.fromString(\"rw-r--r--\");",
        ),
        "random_for_security" => (
            "import java.util.Random;\nimport java.security.SecureRandom;",
            "new Random().nextInt(10);",
            "new SecureRandom().nextInt(10);",
        ),
        "request_no_cert_verify" => (
            "import javax.net.ssl.HttpsURLConnection;",
            "HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> true);",
            "HttpsURLConnection.setDefaultHostnameVerifier((host, session) -> host.equals(command));",
        ),
        "ssl_with_bad_defaults" => (
            "import javax.net.ssl.SSLContext;",
            "SSLContext.getInstance(\"TLSv1\");",
            "SSLContext.getInstance(\"TLSv1.3\");",
        ),
        "ssh_no_host_key_verification" => (
            "import com.jcraft.jsch.Session;",
            "session.setConfig(\"StrictHostKeyChecking\", \"no\");",
            "session.setConfig(\"StrictHostKeyChecking\", \"yes\");",
        ),
        "xml_xxe_vulnerable" => (
            "import javax.xml.parsers.DocumentBuilderFactory;",
            "DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();\n        factory.newDocumentBuilder().parse(input);",
            "DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();\n        factory.setFeature(\"http://apache.org/xml/features/disallow-doctype-decl\", true);\n        factory.newDocumentBuilder().parse(input);",
        ),
        "insecure_cryptography" => (
            "import java.security.MessageDigest;",
            "MessageDigest.getInstance(\"MD5\");",
            "MessageDigest.getInstance(\"SHA-256\");",
        ),
        "resource_not_closed" => (
            "import java.io.FileInputStream;",
            "FileInputStream stream = new FileInputStream(command);\n        stream.read();",
            "try (FileInputStream stream = new FileInputStream(command)) {\n            stream.read();\n        }",
        ),
        "assignment_in_operand" => (
            "",
            "boolean empty = false;\n        if (empty = command.isEmpty()) { input.close(); }",
            "String line;\n        java.io.BufferedReader reader = null;\n        while ((line = reader.readLine()) != null) { input.close(); }",
        ),
        "empty_catch_block" => (
            "",
            "try { input.close(); } catch (java.io.IOException e) {}",
            "try { input.close(); } catch (java.io.IOException ignored) {}\n        try { input.close(); } catch (java.io.IOException e) { /* already closed */ }",
        ),
        "empty_conditional_branch" => (
            "",
            "if (command.isEmpty()) {} else { input.close(); }",
            "if (!command.isEmpty()) { input.close(); }\n        if (command.isEmpty()) { /* the caller handles empty input */ }",
        ),
        "unused_assignment" => (
            "",
            "String mode = \"a\";\n        mode = command.trim();\n        input.read(mode.getBytes());",
            "String mode = command;\n        input.read(mode.getBytes());\n        mode = command.trim();\n        input.read(mode.getBytes());\n        int count = 0;\n        count += input.read();",
        ),
        "sql_injection" => (
            "",
            "session.createNativeQuery(\"SELECT id FROM users WHERE name = '\" + command + \"'\");",
            "session.createNativeQuery(\"SELECT id FROM users WHERE name = :name\").setParameter(\"name\", command);\n        session.createNativeQuery(\"SELECT id FROM \" + TABLE);",
        ),
        "hardcoded_credential" => (
            "",
            "String apiKey = \"tok_9f8e7d6c5b4a3210\";",
            "String apiKeyEnv = \"SERVICE_API_KEY\";\n        String apiKey = System.getenv(apiKeyEnv);",
        ),
        "hardcoded_tmp_directory" => (
            "",
            "java.nio.file.Path scratch = java.nio.file.Path.of(\"/tmp/scratch\");",
            "java.nio.file.Path scratch = java.nio.file.Files.createTempDirectory(\"scratch\");",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/main/java/capability/Capability.java".to_string(),
        format!(
            "package capability;\n\n{imports}\n\nclass Capability {{\n    void inspect(String command, java.io.InputStream input, Session session) throws Exception {{\n        {body}\n    }}\n}}\n"
        ),
    )])
}

/// Java boundaries for the L3 collection, L4 effect and topology, and L5
/// receiver facts. Each negative is the nearest correct form of its positive.
pub(super) fn java_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if let Some(files) = java_topology_files(detector, positive) {
        return Some(files);
    }
    let (imports, positive_body, negative_body): (&str, &str, &str) = match detector {
        "repeated_sort_in_loop" => (
            "import java.util.*;",
            "void inspect(List<Request> requests, List<Integer> candidates) {\n        for (Request request : requests) {\n            Collections.sort(candidates);\n            consume(request, candidates.get(0));\n        }\n    }",
            "void inspect(List<List<Integer>> groups) {\n        for (List<Integer> group : groups) {\n            List<Integer> values = new ArrayList<>(group);\n            Collections.sort(values);\n            consume(values);\n        }\n    }",
        ),
        "sort_for_extremum" => (
            "import java.util.*;",
            "int smallest(List<Integer> items) {\n        Collections.sort(items);\n        return items.get(0);\n    }",
            "List<Integer> ordered(List<Integer> items) {\n        Collections.sort(items);\n        return items;\n    }",
        ),
        "full_sort_for_top_k" => (
            "import java.util.*;",
            "List<Integer> smallest(List<Integer> items) {\n        Collections.sort(items);\n        return items.subList(0, 5);\n    }",
            "int smallest(List<Integer> items) {\n        Collections.sort(items);\n        return items.get(0);\n    }",
        ),
        "repeated_collection_traversal" => (
            "import java.util.*;",
            "void inspect(List<Item> items) {\n        consume(items.stream().anyMatch(item -> item.ready));\n        consume(items.stream().allMatch(item -> item.valid));\n        consume(items.stream().noneMatch(item -> item.target));\n    }",
            "void distinct(List<Item> ready, List<Item> valid, List<Item> targets) {\n        consume(ready.stream().anyMatch(item -> item.ready));\n        consume(valid.stream().allMatch(item -> item.valid));\n        consume(targets.stream().noneMatch(item -> item.target));\n    }\n\n    void changed(List<Item> items) {\n        consume(items.stream().anyMatch(item -> item.ready));\n        mutate(items);\n        consume(items.stream().allMatch(item -> item.valid));\n        consume(items.stream().noneMatch(item -> item.target));\n    }",
        ),
        "hot_path_linear_membership" => (
            "import java.util.*;",
            "void choose(List<String> ids, List<String> wanted) {\n        for (String id : ids) {\n            if (wanted.contains(id)) {\n                consume(id);\n            }\n        }\n    }",
            "void choose(List<String> ids, Set<String> wanted) {\n        for (String id : ids) {\n            if (wanted.contains(id)) {\n                consume(id);\n            }\n        }\n    }",
        ),
        "n_plus_one_query_in_loop" => (
            "import java.util.*;",
            "void load(List<Integer> ids, Repository repo) {\n        for (int id : ids) {\n            consume(repo.findById(id));\n        }\n    }",
            "void load(List<Integer> ids, Repository repo) {\n        consume(repo.findAllById(ids));\n    }",
        ),
        "loop_carried_copy_accumulation" => (
            "import java.util.*;\nimport java.util.stream.Stream;",
            "List<Integer> collect(List<Integer> items) {\n        List<Integer> result = List.of();\n        for (int item : items) {\n            result = Stream.concat(result.stream(), Stream.of(item)).toList();\n        }\n        return result;\n    }",
            "List<Integer> collect(List<Integer> items) {\n        List<Integer> result = new ArrayList<>();\n        for (int item : items) {\n            result.add(item);\n        }\n        return result;\n    }",
        ),
        "nested_cross_collection_traversal" => (
            "import java.util.*;",
            "void compare(List<Item> left, List<Item> right) {\n        for (Item outer : left) {\n            for (Item inner : right) {\n                if (outer.id == inner.id) {\n                    consume(outer, inner);\n                }\n            }\n        }\n    }",
            "void visit(List<Item> left, List<Item> right) {\n        for (Item outer : left) {\n            consume(outer);\n        }\n        for (Item inner : right) {\n            consume(inner);\n        }\n    }\n\n    void derived(List<Item> left) {\n        for (Item outer : left) {\n            List<Item> parts = outer.parts();\n            for (Item inner : parts) {\n                if (outer.id == inner.id) {\n                    consume(outer, inner);\n                }\n            }\n        }\n    }",
        ),
        "loop_artifact_rewrite" => (
            "import java.nio.file.*;\nimport java.util.*;",
            "void rewrite(List<Path> paths) throws Exception {\n        for (Path path : paths) {\n            String raw = Files.readString(path);\n            Files.writeString(Path.of(\"report.json\"), raw);\n        }\n    }",
            "void rewrite(List<Path> paths) throws Exception {\n        String raw = Files.readString(Path.of(\"input\"));\n        for (Path path : paths) {\n            consume(path, raw);\n        }\n    }",
        ),
        "latest_only_unbounded_cache" => (
            "import java.util.*;",
            "void update(List<Entry> cache, Entry entry) {\n        cache.add(entry);\n        consume(cache.get(cache.size() - 1));\n    }",
            "void update(List<Entry> cache, Entry entry) {\n        cache.add(entry);\n        cache.remove(0);\n        consume(cache.get(cache.size() - 1));\n    }",
        ),
        "repeated_parse_scan_serialize" => (
            "import com.fasterxml.jackson.databind.ObjectMapper;\nimport java.util.*;",
            "void process(List<String> items, ObjectMapper mapper) throws Exception {\n        for (String item : items) {\n            Record parsed = mapper.readValue(item, Record.class);\n            scan(parsed);\n            write(mapper.writeValueAsString(parsed));\n        }\n    }",
            "void process(List<String> items, ObjectMapper mapper) throws Exception {\n        for (String item : items) {\n            Record parsed = mapper.readValue(item, Record.class);\n            write(mapper.writeValueAsString(parsed));\n        }\n    }",
        ),
        "blocking_io_in_async_context" => (
            "import java.io.*;\nimport java.nio.file.*;\nimport java.util.concurrent.*;",
            "CompletableFuture<String> load(Path path) {\n        return CompletableFuture.supplyAsync(() -> {\n            try {\n                return Files.readString(path);\n            } catch (IOException error) {\n                throw new UncheckedIOException(error);\n            }\n        });\n    }",
            "CompletableFuture<String> load(Path path, Executor io) {\n        return CompletableFuture.supplyAsync(() -> {\n            try {\n                return Files.readString(path);\n            } catch (IOException error) {\n                throw new UncheckedIOException(error);\n            }\n        }, io);\n    }",
        ),
        "per_item_external_io" => (
            "import java.net.URI;\nimport java.net.http.*;\nimport java.util.*;",
            "void fetchAll(List<String> urls, HttpClient client) throws Exception {\n        for (String url : urls) {\n            consume(client.send(HttpRequest.newBuilder(URI.create(url)).build(), HttpResponse.BodyHandlers.ofString()));\n        }\n    }",
            "void fetchAll(String batch, HttpClient client) throws Exception {\n        consume(client.send(HttpRequest.newBuilder(URI.create(batch)).build(), HttpResponse.BodyHandlers.ofString()));\n    }",
        ),
        "repeated_expensive_initialization" => (
            "import java.util.*;\nimport java.util.regex.Pattern;",
            "void validate(List<String> items) {\n        for (String item : items) {\n            Pattern parser = Pattern.compile(\"[a-z]+\");\n            consume(parser, item);\n        }\n    }",
            "void validate(List<String> items) {\n        Pattern parser = Pattern.compile(\"[a-z]+\");\n        for (String item : items) {\n            consume(parser, item);\n        }\n    }",
        ),
        "request_path_repository_scan" => (
            "import java.nio.file.*;",
            "void handleRequest() throws Exception {\n        consume(Files.list(Path.of(\".\")));\n    }",
            "void refreshIndex() throws Exception {\n        consume(Files.list(Path.of(\".\")));\n    }",
        ),
        "cache_miss_opportunity" => (
            "",
            "boolean inspect(double data) {\n        return Math.abs(data) == Math.abs(data) && Math.abs(data) > 0;\n    }",
            "boolean inspect(double data) {\n        return Math.abs(data) == Math.abs(data);\n    }",
        ),
        "repeated_format_allocation" => (
            "import java.util.*;",
            "void render(List<Integer> items, String mode) {\n        for (int item : items) {\n            consume(item, String.format(\"mode=%s\", mode));\n        }\n    }",
            "void render(List<Integer> items) {\n        for (int item : items) {\n            consume(String.format(\"%d\", item));\n        }\n    }",
        ),
        "loop_invariant_computation" => (
            "import java.util.*;",
            "void render(List<Double> items, double limit) {\n        for (double item : items) {\n            consume(item, Math.abs(limit));\n        }\n    }",
            "void render(List<Double> items) {\n        for (double item : items) {\n            consume(item, Math.abs(item));\n        }\n    }",
        ),
        "manual_collection_copy_loop" => (
            "",
            "int[] duplicate(int[] source) {\n        int[] target = new int[source.length];\n        for (int index = 0; index < source.length; index++) {\n            target[index] = source[index];\n        }\n        return target;\n    }",
            "int[] duplicate(int[] source) {\n        int[] target = new int[source.length];\n        for (int index = 0; index < source.length; index++) {\n            target[index] = source[index] * 2;\n        }\n        return target;\n    }",
        ),
        "eager_fallback_computation" => (
            "import java.util.*;",
            "List<String> resolve(Optional<List<String>> value) {\n        return value.orElse(new ArrayList<>());\n    }",
            "List<String> resolve(Optional<List<String>> value) {\n        return value.orElseGet(ArrayList::new);\n    }\n\n    String named(Optional<String> value) {\n        return value.orElse(buildDefault());\n    }",
        ),
        "intermediate_collection_materialization" => (
            "import java.util.*;\nimport java.util.stream.Collectors;",
            "int count(List<Integer> items) {\n        return items.stream().filter(item -> item > 0).collect(Collectors.toList()).size();\n    }",
            "long count(List<Integer> items) {\n        return items.stream().filter(item -> item > 0).count();\n    }",
        ),
        "known_size_preallocation" => (
            "import java.util.*;",
            "List<String> render(List<Integer> items) {\n        List<String> result = new ArrayList<>();\n        for (int item : items) {\n            result.add(String.valueOf(item));\n        }\n        return result;\n    }",
            "List<String> render(List<Integer> items) {\n        List<String> result = new ArrayList<>(items.size());\n        for (int item : items) {\n            result.add(String.valueOf(item));\n        }\n        return result;\n    }\n\n    List<String> filtered(List<Integer> items) {\n        List<String> result = new ArrayList<>();\n        for (int item : items) {\n            if (item > 0) {\n                result.add(String.valueOf(item));\n            }\n        }\n        return result;\n    }",
        ),
        "bulk_collection_extension" => (
            "import java.util.*;",
            "List<String> extendAll(List<String> target, List<String> items) {\n        for (String item : items) {\n            target.add(item);\n        }\n        return target;\n    }",
            "List<String> extendAll(List<String> target, List<String> items) {\n        for (String item : items) {\n            target.add(item.trim());\n        }\n        return target;\n    }\n\n    List<Node> copies(List<Node> target, List<Node> items) {\n        for (Node item : items) {\n            target.add(item.clone());\n        }\n        return target;\n    }",
        ),
        "map_entry_opportunity" => (
            "import java.util.*;",
            "void insertMissing(Map<String, Integer> map, String key) {\n        if (!map.containsKey(key)) {\n            map.put(key, \"value\".length());\n        }\n    }",
            "void overwrite(Map<String, Integer> map, String key, String other) {\n        if (map.containsKey(key)) {\n            map.put(key, 1);\n        }\n        if (!map.containsKey(key)) {\n            map.put(other, 1);\n        }\n    }",
        ),
        _ => return None,
    };
    let body = if positive {
        positive_body
    } else {
        negative_body
    };
    Some(vec![(
        "src/main/java/capability/Capability.java".to_string(),
        format!("package capability;\n\n{imports}\n\nclass Capability {{\n    {body}\n}}\n"),
    )])
}

/// Java topology boundaries: packages under a Maven source root.
fn java_topology_files(detector: &str, positive: bool) -> Option<Vec<(String, String)>> {
    let file = |path: &str, source: &str| (path.to_string(), source.to_string());
    Some(match (detector, positive) {
        ("architectural_layering", _) => {
            let (import, target, target_source) = if positive {
                (
                    "org.acme.app.UserService",
                    "src/main/java/org/acme/app/UserService.java",
                    "package org.acme.app;\n\npublic class UserService {}\n",
                )
            } else {
                (
                    "org.acme.domain.model.User",
                    "src/main/java/org/acme/domain/model/User.java",
                    "package org.acme.domain.model;\n\npublic class User {}\n",
                )
            };
            let kind = import.rsplit('.').next().unwrap_or_default();
            vec![
                file(
                    "src/main/java/org/acme/domain/Check.java",
                    &format!("package org.acme.domain;\n\nimport {import};\n\npublic class Check {{\n    Object check() {{ return new {kind}(); }}\n}}\n"),
                ),
                file(target, target_source),
                file(
                    "auditor.toml",
                    "[layering.layer.domain]\nglob = src/main/java/org/acme/domain/**/*.java\n\n[layering.layer.app]\nglob = src/main/java/org/acme/app/**/*.java\n\n[layering.rule.domain]\ndeny = app\n",
                ),
            ]
        }
        ("package_coupling_metrics", true) => vec![
            file(
                "packages/core/src/main/java/org/acme/core/Model.java",
                "package org.acme.core;\n\npublic class Model {}\n",
            ),
            file(
                "packages/app/src/main/java/org/acme/app/Service.java",
                "package org.acme.app;\n\nimport org.acme.core.Model;\n\npublic interface Service { void process(Model model); }\n",
            ),
            file(
                "packages/web/src/main/java/org/acme/web/Handler.java",
                "package org.acme.web;\n\nimport org.acme.core.Model;\n\npublic class Handler { Model model; }\n",
            ),
        ],
        ("package_coupling_metrics", false) => vec![file(
            "packages/core/src/main/java/org/acme/core/Model.java",
            "package org.acme.core;\n\npublic class Model {}\n",
        )],
        _ => return None,
    })
}
