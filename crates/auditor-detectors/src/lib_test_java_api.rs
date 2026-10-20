fn java_findings(files: &[(&str, &str)]) -> Vec<(String, usize)> {
    let root = temp_root("java_api");
    for (path, source) in files {
        write(&root, path, source);
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| finding.primary_path().ends_with("Store.java"))
        .filter(|finding| finding.category == "correctness")
        .map(|finding| {
            (
                finding.detector.clone(),
                finding.primary_line().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort();
    findings
}

#[test]
fn java_api_rules_resolve_declarations_and_respect_ownership_and_contracts() {
    let library = r#"package store;

import java.io.*;
import java.util.Iterator;
import javax.xml.parsers.DocumentBuilderFactory;

class Store implements Iterator<String> {
    private final Runtime runtime = Runtime.getRuntime();

    void shell(String line) throws IOException {
        runtime.exec(new String[] {"/bin/bash", "-c", line});
    }

    long leak(String path) throws IOException {
        FileInputStream stream = new FileInputStream(path);
        return stream.skip(1);
    }

    InputStream handoff(String path) throws IOException {
        FileInputStream stream = new FileInputStream(path);
        return stream;
    }

    void closed(String path) throws IOException {
        FileInputStream stream = new FileInputStream(path);
        stream.read();
        stream.close();
    }

    Object newDocument() throws Exception {
        Object document = DocumentBuilderFactory.newInstance().newDocumentBuilder().newDocument();
        javax.xml.transform.TransformerFactory.newInstance().newTransformer().transform(null, null);
        return document;
    }

    public void remove() {
        throw new UnsupportedOperationException();
    }

    public boolean hasNext() { return false; }
    public String next() { return null; }
}

class Random {
    int nextInt() { return 4; }
    void use() { new Random().nextInt(); }
}
"#;
    let program = r#"package store;

class Tool {
    public static void main(String[] args) {
        System.out.println("usage");
        System.exit(2);
    }
}
"#;
    assert_eq!(
        java_findings(&[
            ("src/main/java/store/Store.java", library),
            ("src/main/java/store/ToolStore.java", program),
        ]),
        [
            ("resource_not_closed".to_string(), 15),
            ("shell_execution".to_string(), 11),
        ]
    );
}

#[test]
fn java_blocking_io_is_async_on_unscheduled_reactive_chains_only() {
    let source = r#"package store;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import reactor.core.publisher.Flux;
import reactor.core.publisher.Mono;
import reactor.core.scheduler.Schedulers;

class Store {
    Mono<String> onLoop(Mono<Path> path) {
        return path.map(p -> read(p) + Files.readString(p));
    }

    Flux<String> fromFactory(java.util.List<Path> paths) {
        return Flux.fromIterable(paths).filter(p -> p != null).map(p -> Files.readString(p));
    }

    Mono<String> scheduled(Mono<Path> path) {
        return path.publishOn(Schedulers.boundedElastic()).map(p -> Files.readString(p));
    }

    Mono<String> deferred(Path path) {
        return Mono.fromCallable(() -> Files.readString(path)).subscribeOn(Schedulers.boundedElastic());
    }

    Mono<String> unresolved(Path path) {
        return repository(path).map(p -> Files.readString(p));
    }
}
"#;
    let root = temp_root("java_reactive");
    write(&root, "src/main/java/store/Store.java", source);
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut lines = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "blocking_io_in_async_context")
        .map(|finding| finding.primary_line().unwrap_or_default())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    assert_eq!(lines, [13, 17]);
}
