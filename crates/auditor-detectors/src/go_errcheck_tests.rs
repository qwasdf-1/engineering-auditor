use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{analyze_repository, ScanProfile, SourceDiscoveryOptions};

/// The lines `go_errcheck` reports in one Go source file.
fn errcheck_lines(name: &str, source: &str) -> Vec<usize> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root: PathBuf =
        std::env::temp_dir().join(format!("ea-errcheck-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.go"), source).unwrap();
    let analysis = analyze_repository(
        &root,
        SourceDiscoveryOptions::default(),
        ScanProfile::Gating,
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(root);
    let mut lines = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "go_errcheck")
        .filter_map(|finding| finding.primary_line())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines
}

#[test]
fn go_errcheck_ignored_return() {
    assert_eq!(
        errcheck_lines(
            "goerr",
            "package main\nfunc main() {\n    f.Write(data)\n}\n"
        ),
        [3]
    );
}

#[test]
fn go_errcheck_skips_infallible_buffers_and_resultless_methods() {
    let source = "package main\nimport (\n    \"bytes\"\n    \"crypto/sha256\"\n    \"net/http\"\n)\ntype LogEntry interface {\n    Write(status int, header http.Header)\n}\nfunc main(entry LogEntry) {\n    var buf bytes.Buffer\n    buf.Write(data)\n    digest := sha256.New()\n    digest.Write(data)\n    entry.Write(200, nil)\n}\n";
    assert_eq!(errcheck_lines("goerrskip", source), Vec::<usize>::new());
}

#[test]
fn go_errcheck_still_reports_fallible_writers() {
    let source = "package main\nimport \"os\"\nfunc main(buf *os.File) {\n    var other bytes.Buffer\n    buf.Write(data)\n    s.buf.Write(data)\n}\n";
    assert_eq!(errcheck_lines("goerrkeep", source), [5, 6]);
}

#[test]
fn go_errcheck_reads_statements_not_line_text() {
    // A name containing `err` no longer hides a call, and a result passed
    // on, assigned, returned, or tested is used.
    let source = "package main\n\nimport (\n\t\"io\"\n\t\"os\"\n\t\"os/exec\"\n)\n\nfunc main(stderr *os.File, w io.Writer, r io.Reader) error {\n\tstderr.Close()\n\tos.Rename(\"a\", \"b\")\n\tos.WriteFile(\"x\", nil, 0o644)\n\texec.Command(\"ls\").Run()\n\tcmd := exec.Command(\"make\")\n\tcmd.Start()\n\tdefer cmd.Wait()\n\tgo io.Copy(w, r)\n\tdefer func() { stderr.Close() }()\n\tconsume(stderr.Close())\n\t_ = os.Remove(\"x\")\n\tif err := os.Remove(\"y\"); err != nil {\n\t\treturn err\n\t}\n\treturn os.Remove(\"z\")\n}\n";
    assert_eq!(
        errcheck_lines("goerrast", source),
        [10, 11, 12, 13, 15, 16, 17, 18]
    );
}

#[test]
fn go_errcheck_needs_a_known_error_result() {
    // Unknown package functions, local functions, `Start` on a value that
    // is not a command, and a `Close` that takes arguments are not read
    // as returning an error.
    let source = "package main\n\nimport (\n\t\"fmt\"\n\t\"os/exec\"\n\t\"strings\"\n)\n\nfunc main(server *Server, list *List, other bool) {\n\tfmt.Println(\"done\")\n\tstrings.ToUpper(\"x\")\n\tcleanup()\n\tserver.Start()\n\tlist.Close(1)\n\tcmd := exec.Command(\"ls\")\n\tif other {\n\t\tcmd = nil\n\t}\n\tcmd.Run()\n}\n";
    assert_eq!(errcheck_lines("goerrknown", source), Vec::<usize>::new());
}
