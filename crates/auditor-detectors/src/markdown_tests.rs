use super::*;

#[test]
fn heading_anchors_follow_rendered_text_setext_and_duplicates() {
    let anchors = collect_section_anchors(&[
        "### Cells that _Black_ will skip",
        "#### [Core](#core)",
        "Read a text file line-by-line ([Java][ReadFile]/[Kotlin][ReadFileKt])",
        "-----------------------------------------------------------------------",
        "## Options",
        "## Options",
    ]);
    for expected in [
        "cells-that-black-will-skip",
        "core",
        "read-a-text-file-line-by-line-javakotlin",
        "options",
        "options-1",
    ] {
        assert!(
            anchors.iter().any(|anchor| anchor == expected),
            "{expected}: {anchors:?}"
        );
    }
}

#[test]
fn non_links_schemes_and_encoded_targets_are_read_as_written() {
    let links = markdown_links(&[
        "return write_padded(ctx, [=](auto out) {",
        "Block size \\[4-7\\](default : 7)",
        "- [`ChaosLatencyStrategyOptions`](xref:Polly.Simmy.Latency.ChaosLatencyStrategyOptions)",
        "see [`operator[]`](operator%5B%5D.md) and [guide](guide.md \"Guide\")",
    ]);
    assert_eq!(
        links
            .iter()
            .map(|link| link.target.as_str())
            .collect::<Vec<_>>(),
        ["operator[].md", "guide.md"]
    );
}

#[test]
fn version_numbered_page_links_resolve_to_their_markdown_file() {
    let root = std::env::temp_dir().join(format!("ea-md-version-{}", std::process::id()));
    std::fs::create_dir_all(root.join("use")).unwrap();
    std::fs::write(root.join("use/migrating-to-1.0.0.md"), "# Migrating\n").unwrap();
    assert!(existing_doc_target(&root.join("use/migrating-to-1.0.0")).is_some());
    assert!(existing_doc_target(&root.join("use/missing-2.0.0")).is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn myst_targets_and_site_routes_resolve() {
    assert_eq!(
        myst_target("(labels/hug-parens)="),
        Some("labels/hug-parens".into())
    );
    assert_eq!(myst_target("(not a label)="), None);
    let root = std::env::temp_dir().join(format!("ea-md-routes-{}", std::process::id()));
    std::fs::create_dir_all(root.join("docs/advanced")).unwrap();
    std::fs::create_dir_all(root.join("site/src/pages")).unwrap();
    std::fs::create_dir_all(root.join("site/src/extend")).unwrap();
    std::fs::write(root.join("docs/advanced/transports.md"), "# T\n").unwrap();
    std::fs::write(root.join("site/src/extend/index.md"), "# E\n").unwrap();
    let mkdocs = Path::new("docs/async.md");
    assert!(resolves_as_site_route(
        &root,
        mkdocs,
        "../advanced/transports"
    ));
    assert!(!resolves_as_site_route(
        &root,
        mkdocs,
        "../advanced/missing"
    ));
    assert!(resolves_as_site_route(
        &root,
        Path::new("site/src/pages/index.md"),
        "extend/"
    ));
    std::fs::remove_dir_all(root).unwrap();
}
