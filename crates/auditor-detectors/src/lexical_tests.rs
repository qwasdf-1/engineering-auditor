mod tests {
    use super::*;

    #[test]
    fn section_divider_detects_excess() {
        let source = "\
fn main() {
    // ==============
    let a = 1;
    // ==============
    let b = 2;
    // ==============
    let c = 3;
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = section_divider_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(
            findings
                .iter()
                .any(|finding| finding.detector == "section_divider_comment"),
            "expected section_divider_comment finding"
        );
    }

    #[test]
    fn section_divider_below_threshold_is_silent() {
        let source = "\
fn main() {
    // ===============
    let a = 1;
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = section_divider_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.detector == "section_divider_comment"),
            "expected no section_divider_comment for 1 divider"
        );
    }

    #[test]
    fn byte_layout_diagram_borders_are_not_section_dividers() {
        let source = "\
fun consumeHeader() {
    // +---+---+---+---+
    // |ID1|ID2|CM |FLG|
    // +---+---+---+---+
    source.require(10)
    // +---+---+
    // | XLEN  |
    // +---+---+
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = section_divider_per_file("src/Gzip.kt", &Language::Kotlin, &lines);
        assert!(findings.is_empty(), "diagram borders are not dividers: {findings:?}");
    }

    #[test]
    fn magic_literal_detects_cluster() {
        let source = "\
fn compute() -> i32 {
    42 + 7 + 100 + 365 + 1024
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = magic_literal_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(
            findings
                .iter()
                .any(|finding| finding.detector == "magic_literal_cluster"),
            "expected magic_literal_cluster finding"
        );
    }

    #[test]
    fn number_line_diagrams_are_not_dividers() {
        let lines = [
            "    //               <--------------------------- delta ---->",
            "    // --------------[------------------+-------------------]--------------",
            "    //                                       <---- rest ---->",
        ];
        assert!(lines
            .iter()
            .all(|line| !is_section_divider(&Language::Cpp, line)));
        assert!(is_section_divider(&Language::Cpp, "    // ---------- Parsing ----------"));
    }

    #[test]
    fn magic_literals_of_one_value_count_once() {
        let source = "\
fn mask(c: u32) -> u32 {
    (c & 0x10) + (c >> 16) + 0x3F + 63 + 0b10 + 0x01
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = magic_literal_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn magic_literal_below_threshold_is_silent() {
        let source = "\
fn simple() -> i32 {
    42
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = magic_literal_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(
            !findings
                .iter()
                .any(|finding| finding.detector == "magic_literal_cluster"),
            "expected no magic_literal_cluster for 1 literal"
        );
    }

    #[test]
    fn magic_literal_ignores_digits_embedded_in_identifiers() {
        let source = "\
fn slice_evidence() {
    let slice_10_tagged_snapshot = tagged_snapshot();
    let slice_11_tagged_snapshot = tagged_snapshot();
    let slice_12_tagged_snapshot = tagged_snapshot();
    consume(slice_10_tagged_snapshot, slice_11_tagged_snapshot, slice_12_tagged_snapshot);
}
";
        let lines: Vec<&str> = source.lines().collect();
        let findings = magic_literal_per_file("src/main.rs", &Language::Rust, &lines);
        assert!(
            findings.is_empty(),
            "digits inside identifiers are not numeric literals"
        );
        assert!(
            util::numeric_literals("let slice_10_tagged_snapshot = tagged_snapshot();").is_empty()
        );
        assert_eq!(util::numeric_literals("let value = input + 10;"), ["10"]);
        assert_eq!(
            util::numeric_literals("f(0x1F, 1_000, 3.25e-2, .5, 7e, x1, 2_abc)"),
            ["0x1F", "1000", "3.25e-2", "5", "7", "2"]
        );

    }

    #[test]
    fn magic_literal_ignores_digits_in_python_docstrings() {
        let source = r#"class Codes:
    def is_error(cls, value):
        """
        Returns `True` for 4xx or 5xx status codes, `False` otherwise.
        """
        return 400 <= value <= 599
"#;
        let lines: Vec<&str> = source.lines().collect();
        assert!(magic_literal_per_file("codes.py", &Language::Python, &lines).is_empty());
    }
}
