#[test]
fn files_with_syntax_errors_are_analyzed_as_recovered_or_marked_unparsed() {
    let root = temp_root("syntax_recovery");
    // A Java 21 record pattern the grammar cannot read, beside a method
    // that takes six parameters.
    write(
        &root,
        "src/main/java/app/Shapes.java",
        r#"package app;

class Shapes {
    static String name(Object shape) {
        if (shape instanceof Shape.Circle(double radius)) {
            return "circle " + radius;
        }
        return "other";
    }

    static int area(int a, int b, int c, int d, int e, int f) {
        return a * b + c * d + e * f;
    }

    static int perimeter(int width, int height) {
        return 2 * (width + height);
    }
}
"#,
    );
    write(&root, "src/main/java/app/Broken.java", "class Broken { {{{ ((( ;;; )))\n");
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    assert!(analysis.findings.iter().any(|finding| {
        finding.detector == "too_many_parameters"
            && finding.primary_path() == "src/main/java/app/Shapes.java"
    }));
    let syntax = |path: &str| {
        analysis
            .files
            .iter()
            .find(|file| file.path == path)
            .and_then(|file| file.syntax.clone())
    };
    assert_eq!(syntax("src/main/java/app/Shapes.java").as_deref(), Some("recovered"));
    assert_eq!(syntax("src/main/java/app/Broken.java").as_deref(), Some("unparsed"));
}
