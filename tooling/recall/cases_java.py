"""Seeded recall cases in Java; see `cases.py` for the shapes."""


def _java(imports: str, body: str, name: str = "Service") -> str:
    return f"package app;\n\n{imports}\n\nclass {name} {{\n{body}\n}}\n"


JAVA_DIR = "src/main/java/app/"

JAVA = {
    "repeated_sort_in_loop": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void inspect(List<Request> requests, List<Integer> candidates) {
        for (Request request : requests) {
            Collections.sort(candidates);
            consume(request, candidates.get(0));
        }
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void refresh(List<Integer> candidates) {
        Collections.sort(candidates);
    }

    void inspect(List<Request> requests, List<Integer> candidates) {
        for (Request request : requests) {
            refresh(candidates);
            consume(request, candidates.get(0));
        }
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void inspect(List<Request> requests, List<Integer> candidates) {
        for (Request request : requests) {
            Ranking.refresh(candidates);
            consume(request, candidates.get(0));
        }
    }"""),
            JAVA_DIR + "Ranking.java": _java("import java.util.*;", """\
    static void refresh(List<Integer> candidates) {
        Collections.sort(candidates);
    }""", "Ranking"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void inspect(List<Request> requests, List<Integer> candidates) {
        Collections.sort(candidates);
        for (Request request : requests) {
            consume(request, candidates.get(0));
        }
    }""")},
    },
    "n_plus_one_query_in_loop": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void load(List<Integer> ids, Repository repo) {
        for (int id : ids) {
            consume(repo.findById(id));
        }
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    Row loadOne(Repository repo, int id) {
        return repo.findById(id);
    }

    void load(List<Integer> ids, Repository repo) {
        for (int id : ids) {
            consume(loadOne(repo, id));
        }
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void load(List<Integer> ids, Repository repo) {
        for (int id : ids) {
            consume(Store.loadOne(repo, id));
        }
    }"""),
            JAVA_DIR + "Store.java": _java("", """\
    static Row loadOne(Repository repo, int id) {
        return repo.findById(id);
    }""", "Store"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.util.*;", """\
    void load(List<Integer> ids, Repository repo) {
        consume(repo.findAllById(ids));
    }""")},
    },
    "per_item_external_io": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.net.URI;\nimport java.net.http.*;\nimport java.util.*;", """\
    void fetchAll(List<String> urls, HttpClient client) throws Exception {
        for (String url : urls) {
            consume(client.send(HttpRequest.newBuilder(URI.create(url)).build(), HttpResponse.BodyHandlers.ofString()));
        }
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.net.URI;\nimport java.net.http.*;\nimport java.util.*;", """\
    HttpResponse<String> details(HttpClient client, String url) throws Exception {
        return client.send(HttpRequest.newBuilder(URI.create(url)).build(), HttpResponse.BodyHandlers.ofString());
    }

    void fetchAll(List<String> urls, HttpClient client) throws Exception {
        for (String url : urls) {
            consume(details(client, url));
        }
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.net.http.*;\nimport java.util.*;", """\
    void fetchAll(List<String> urls, HttpClient client) throws Exception {
        for (String url : urls) {
            consume(Remote.details(client, url));
        }
    }"""),
            JAVA_DIR + "Remote.java": _java("import java.net.URI;\nimport java.net.http.*;", """\
    static HttpResponse<String> details(HttpClient client, String url) throws Exception {
        return client.send(HttpRequest.newBuilder(URI.create(url)).build(), HttpResponse.BodyHandlers.ofString());
    }""", "Remote"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.net.URI;\nimport java.net.http.*;\nimport java.util.*;", """\
    void fetchAll(String batch, HttpClient client) throws Exception {
        consume(client.send(HttpRequest.newBuilder(URI.create(batch)).build(), HttpResponse.BodyHandlers.ofString()));
    }""")},
    },
    "loop_invariant_regex_compilation": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.util.regex.Pattern;", """\
    void inspect(String[] items) {
        for (String item : items) {
            Pattern pattern = Pattern.compile("^fixed$");
            consume(pattern, item);
        }
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.util.regex.Pattern;", """\
    Pattern matcher() {
        return Pattern.compile("^fixed$");
    }

    void inspect(String[] items) {
        for (String item : items) {
            Pattern pattern = matcher();
            consume(pattern, item);
        }
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.util.regex.Pattern;", """\
    void inspect(String[] items) {
        for (String item : items) {
            Pattern pattern = Patterns.matcher();
            consume(pattern, item);
        }
    }"""),
            JAVA_DIR + "Patterns.java": _java("import java.util.regex.Pattern;", """\
    static Pattern matcher() {
        return Pattern.compile("^fixed$");
    }""", "Patterns"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.util.regex.Pattern;", """\
    void inspect(String[] items) {
        Pattern pattern = Pattern.compile("^fixed$");
        for (String item : items) {
            consume(pattern, item);
        }
    }""")},
    },
    "loop_artifact_rewrite": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.nio.file.*;\nimport java.util.*;", """\
    void rewrite(List<Path> paths) throws Exception {
        for (Path path : paths) {
            String raw = Files.readString(path);
            Files.writeString(Path.of("report.json"), raw);
        }
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.nio.file.*;\nimport java.util.*;", """\
    void save(String raw) throws Exception {
        Files.writeString(Path.of("report.json"), raw);
    }

    void rewrite(List<Path> paths) throws Exception {
        for (Path path : paths) {
            String raw = Files.readString(path);
            save(raw);
        }
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.nio.file.*;\nimport java.util.*;", """\
    void rewrite(List<Path> paths) throws Exception {
        for (Path path : paths) {
            String raw = Files.readString(path);
            Report.save(raw);
        }
    }"""),
            JAVA_DIR + "Report.java": _java("import java.nio.file.*;", """\
    static void save(String raw) throws Exception {
        Files.writeString(Path.of("report.json"), raw);
    }""", "Report"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.nio.file.*;\nimport java.util.*;", """\
    void rewrite(List<Path> paths) throws Exception {
        String raw = Files.readString(Path.of("input"));
        for (Path path : paths) {
            consume(path, raw);
        }
    }""")},
    },
    "blocking_io_in_async_context": {
        "direct": {JAVA_DIR + "Service.java": _java("import java.io.*;\nimport java.nio.file.*;\nimport java.util.concurrent.*;", """\
    CompletableFuture<String> load(Path path) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return Files.readString(path);
            } catch (IOException error) {
                throw new UncheckedIOException(error);
            }
        });
    }""")},
        "helper": {JAVA_DIR + "Service.java": _java("import java.io.*;\nimport java.nio.file.*;\nimport java.util.concurrent.*;", """\
    String readInput(Path path) {
        try {
            return Files.readString(path);
        } catch (IOException error) {
            throw new UncheckedIOException(error);
        }
    }

    CompletableFuture<String> load(Path path) {
        return CompletableFuture.supplyAsync(() -> readInput(path));
    }""")},
        "cross_file": {
            JAVA_DIR + "Service.java": _java("import java.nio.file.*;\nimport java.util.concurrent.*;", """\
    CompletableFuture<String> load(Path path) {
        return CompletableFuture.supplyAsync(() -> Inputs.readInput(path));
    }"""),
            JAVA_DIR + "Inputs.java": _java("import java.io.*;\nimport java.nio.file.*;", """\
    static String readInput(Path path) {
        try {
            return Files.readString(path);
        } catch (IOException error) {
            throw new UncheckedIOException(error);
        }
    }""", "Inputs"),
        },
        "negative": {JAVA_DIR + "Service.java": _java("import java.io.*;\nimport java.nio.file.*;\nimport java.util.concurrent.*;", """\
    CompletableFuture<String> load(Path path, Executor io) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return Files.readString(path);
            } catch (IOException error) {
                throw new UncheckedIOException(error);
            }
        }, io);
    }""")},
    },
}
