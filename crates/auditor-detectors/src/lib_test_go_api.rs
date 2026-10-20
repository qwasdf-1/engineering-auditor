fn go_findings(files: &[(&str, &str)]) -> Vec<(String, String, usize)> {
    let root = temp_root("go_api");
    for (path, source) in files {
        write(&root, path, source);
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| finding.primary_path().ends_with(".go"))
        .filter(|finding| !finding.detector.starts_with("go_") && finding.category == "correctness")
        .map(|finding| {
            (
                finding.detector.clone(),
                finding.primary_path().to_string(),
                finding.primary_line().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort();
    findings
}

#[test]
fn go_api_rules_resolve_aliases_and_respect_program_boundaries() {
    let library = r#"package store

import (
	run "os/exec"
	"context"
	"crypto/tls"
	"os"
)

func Shell(ctx context.Context, line string) error {
	return run.CommandContext(ctx, "/bin/bash", "-c", line).Run()
}

func Configure(cfg *tls.Config) {
	cfg.InsecureSkipVerify = true
}

func Prepare(dir string) error {
	return os.MkdirAll(dir, os.ModePerm)
}

func Leak(path string) int64 {
	file, _ := os.Open(path)
	info, _ := file.Stat()
	return info.Size()
}

func Handoff(path string) (*os.File, error) {
	file, err := os.Open(path)
	return file, err
}
"#;
    let program = r#"package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Println("usage")
	os.Exit(2)
}
"#;
    let shadowed = r#"package other

type runner struct{}

func (runner) Command(name string, args ...string) {}

func Use(exec runner) {
	exec.Command("sh", "-c", "true")
}
"#;
    assert_eq!(
        go_findings(&[
            ("store/store.go", library),
            ("cmd/tool/main.go", program),
            ("other/other.go", shadowed),
        ]),
        [
            ("request_no_cert_verify".to_string(), "store/store.go".to_string(), 15),
            ("resource_not_closed".to_string(), "store/store.go".to_string(), 23),
            ("shell_execution".to_string(), "store/store.go".to_string(), 11),
            ("world_writable_file".to_string(), "store/store.go".to_string(), 19),
        ]
    );
}
