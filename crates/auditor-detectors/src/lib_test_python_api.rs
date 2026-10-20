fn python_findings(files: &[(&str, &str)]) -> Vec<(String, String, usize)> {
    let root = temp_root("python_api");
    for (path, source) in files {
        write(&root, path, source);
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.detector.as_str(),
                "exit_in_production" | "console_output_in_production" | "no_await_in_loop"
            )
        })
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
fn python_exits_and_prints_outside_program_code_are_reported() {
    let manifest = "[project]\nname = \"tool\"\n\n[project.scripts]\ntool = \"tool:main\"\n";
    let package = "from ._command import main\n\ndef helper():\n    print(\"library\")\n";
    let cli = r#"import sys

def main():
    print("usage")
    sys.exit(2)

def fail():
    raise SystemExit(1)
"#;
    let demo = r#"def render(value):
    return value

if sys.platform != "win32" and __name__ == "__main__":
    print(render(1))
    exit(0)
"#;
    let worker = r#"import asyncio

async def drain(queue, items):
    while True:
        await queue.get()
    for item in items:
        await queue.put(item)
    for item in items:
        if await queue.put(item):
            return item
    for item in items:
        consume([await queue.put(value) for value in item])
"#;
    assert_eq!(
        python_findings(&[
            ("pyproject.toml", manifest),
            ("tool/__init__.py", package),
            ("tool/_command.py", cli),
            ("tool/demo.py", demo),
            ("tool/worker.py", worker),
        ]),
        [
            (
                "console_output_in_production".to_string(),
                "tool/__init__.py".to_string(),
                4
            ),
            ("exit_in_production".to_string(), "tool/_command.py".to_string(), 8),
            ("no_await_in_loop".to_string(), "tool/worker.py".to_string(), 7),
            ("no_await_in_loop".to_string(), "tool/worker.py".to_string(), 12),
        ]
    );
}

#[test]
fn python_sorted_copies_read_at_an_endpoint_are_selections() {
    let root = temp_root("python_sorted");
    write(
        &root,
        "src/rank.py",
        "def best(scores: list[int]) -> int:\n    return sorted(scores)[-1]\n\ndef podium(scores: list[int]) -> list[int]:\n    return sorted(scores)[:3]\n\ndef middle(scores: list[int]) -> int:\n    return sorted(scores)[len(scores) // 2]\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| matches!(finding.detector.as_str(), "sort_for_extremum" | "full_sort_for_top_k"))
        .map(|finding| (finding.detector.clone(), finding.primary_line().unwrap_or_default()))
        .collect::<Vec<_>>();
    findings.sort();
    assert_eq!(
        findings,
        [
            ("full_sort_for_top_k".to_string(), 5),
            ("sort_for_extremum".to_string(), 2),
        ]
    );
}

#[test]
fn python_credential_names_are_not_credentials() {
    let root = temp_root("python_credential_names");
    write(
        &root,
        "src/settings.py",
        "API_KEY_ENV = \"SERVICE_API_KEY\"\nTOKEN_HEADER = \"X-Auth-Token\"\nAPI_KEY = \"sk_live_9f8e7d6c5b4a3210\"\n",
    );
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let lines = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "hardcoded_credential")
        .map(|finding| finding.primary_line().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(lines, [3]);
}

#[test]
fn python_ctypes_bindings_keep_the_c_api_names() {
    let root = temp_root("python_ctypes_names");
    write(
        &root,
        "src/console.py",
        "import ctypes\n\ndef GetStdHandle(handle):\n    return ctypes.windll.kernel32.GetStdHandle(handle)\n",
    );
    write(&root, "src/helpers.py", "def PERCENT(text):\n    return text\n");
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let paths = analysis
        .findings
        .iter()
        .filter(|finding| finding.detector == "naming_convention")
        .map(|finding| finding.primary_path().to_string())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["src/helpers.py"]);
}
