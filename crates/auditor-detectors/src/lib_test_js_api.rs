fn js_findings(files: &[(&str, &str)]) -> Vec<(String, String, usize)> {
    let root = temp_root("js_api");
    for (path, source) in files {
        write(&root, path, source);
    }
    let analysis =
        analyze_repository(&root, SourceDiscoveryOptions::default(), ScanProfile::Full).unwrap();
    let mut findings = analysis
        .findings
        .iter()
        .filter(|finding| finding.category == "correctness")
        .filter(|finding| finding.detector != "console_output_in_production")
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
fn js_api_rules_resolve_bindings_and_respect_shadowing_ownership_and_scripts() {
    let library = r#"const { spawn } = require('node:child_process');
import * as fs from 'fs';

export function run(line, process) {
  spawn('make', [line], { shell: true });
  process.exit(1);
  setTimeout(() => line, 10);
  setTimeout('tick()', 10);
}

export function leak(path) {
  const fd = fs.openSync(path, 'r');
  fs.readSync(fd, Buffer.alloc(4));
}

export function handoff(path) {
  const fd = fs.openSync(path, 'r');
  return fd;
}
"#;
    let script = "#!/usr/bin/env node\nprocess.exit(2);\n";
    let typed = r#"import { createHash } from 'node:crypto';

export function digest(input: string): string {
  return createHash('sha1').update(input).digest('hex');
}
"#;
    assert_eq!(
        js_findings(&[
            ("src/lib.js", library),
            ("src/tool.js", script),
            ("src/digest.ts", typed),
        ]),
        [
            ("eval_in_production".to_string(), "src/lib.js".to_string(), 8),
            ("insecure_cryptography".to_string(), "src/digest.ts".to_string(), 4),
            ("resource_not_closed".to_string(), "src/lib.js".to_string(), 12),
            ("shell_execution".to_string(), "src/lib.js".to_string(), 5),
        ]
    );
}
