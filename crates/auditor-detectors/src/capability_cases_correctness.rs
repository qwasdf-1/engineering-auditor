use crate::{AnalysisMethod, ScanProfile};

use super::CapabilityCase;

fn file(path: &str, source: impl Into<String>) -> (String, String) {
    (path.to_string(), source.into())
}

pub(super) fn default_cases() -> Vec<CapabilityCase> {
    vec![
        CapabilityCase::new(
            "todo_unimplemented_in_production",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn execute() { todo!(\"implement\"); }\n",
            "src/lib.rs",
            "fn execute() { work(); }\n",
        ),
        CapabilityCase::new(
            "assert_in_production",
            ScanProfile::Gating,
            "src/service.py",
            "def execute(value):\n    assert value > 0\n    return value\n",
            "src/service.py",
            "def execute(value):\n    if value <= 0:\n        raise ValueError(value)\n    return value\n",
        )
        .negative_files(vec![
            file("src/service.py", "def execute(value):\n    if value <= 0:\n        raise ValueError(value)\n    return value\n"),
            file("src/lib.rs", "fn execute(value: i32) { assert!(value > 0); }\n"),
            file(
                "src/stream.py",
                "def read(stream, size):\n    assert isinstance(stream, Iterable)\n    assert size is not None and isinstance(size, int)\n    return stream\n",
            ),
            file("src/guards.ts", "assertNoStartOnlyOptions(options)\nassertHostedCompletionText(text)\n"),
            file("src/model.rs", "fn build() { let assertions = common_authority_assertions(); consume(assertions); }\n"),
        ]),
        CapabilityCase::new(
            "eval_in_production",
            ScanProfile::Gating,
            "src/main.py",
            "result = eval(expression)\n",
            "src/main.py",
            "# eval(expression) is forbidden\nresult = parse(expression)\n",
        )
        .negative_files(vec![
            file("src/evaluation.rs", "fn handle_eval(command: EvalCommand) { run_sigmap_retrieval(command); }\n"),
            file("src/main.py", "result = parse(expression)\n"),
        ]),
        CapabilityCase::new(
            "hardcoded_credential",
            ScanProfile::Gating,
            "src/config.py",
            "password = \"supersecret\"\n",
            "src/config.py",
            "password = os.getenv(\"DB_PASSWORD\")\n",
        )
        .negative_files(vec![
            file("src/ui.ts", "const visible = state.apiKeyForm ? '*'.repeat(state.draft.length) : state.draft\n"),
            file(
                "src/profile.ts",
                "const apiKeyProfileId = `${providerStem(providerPackId)}.${accountLabel}`\n",
            ),
            file("src/mask.ts", "const ApiKeyPaste = \"*\"\n"),
            file("src/oauth.rs", "let access_token = required_string_field(payload, \"access_token\")?;\n"),
            file("src/config.py", "password = os.getenv(\"DB_PASSWORD\")\n"),
            file(
                "src/main/java/SystemProperties.java",
                "class SystemProperties {\n    public static final String JAVA_NET_SOCKS_PASSWORD = \"java.net.socks.password\";\n}\n",
            ),
            file(
                "src/Url.kt",
                "internal const val PASSWORD_ENCODE_SET = \" \\\"':;<=>@[]^`{}|/\\\\?#\"\n",
            ),
        ]),
        CapabilityCase::new(
            "unsafe_deserialization",
            ScanProfile::Gating,
            "src/load.py",
            "data = pickle.loads(blob)\n",
            "src/load.py",
            "data = json.loads(blob)\n",
        ),
        CapabilityCase::new(
            "bind_all_interfaces",
            ScanProfile::Gating,
            "src/server.py",
            "server.bind((\"0.0.0.0\", 8080))\n",
            "src/server.py",
            "server.bind((\"127.0.0.1\", 8080))\n",
        ),
        CapabilityCase::new(
            "try_except_pass",
            ScanProfile::Full,
            "src/service.py",
            "try:\n    risky()\nexcept:\n    pass\n",
            "src/service.py",
            "try:\n    risky()\nexcept ValueError as error:\n    handle(error)\n",
        ),
        CapabilityCase::new(
            "world_writable_file",
            ScanProfile::Full,
            "src/files.py",
            "os.chmod(path, 0o777)\n",
            "src/files.py",
            "os.chmod(path, 0o600)\n",
        )
        .negative_files(vec![file(
            "src/permissions.py",
            "path.chmod(mode & ~stat.S_IWUSR & ~stat.S_IWGRP & ~stat.S_IWOTH)\nchild.chmod(child.stat().st_mode | stat.S_IWUSR)\n",
        )]),
        CapabilityCase::new(
            "hardcoded_tmp_directory",
            ScanProfile::Full,
            "src/files.py",
            "temp_path = \"/tmp/unsafe\"\n",
            "src/files.py",
            "handle, temp_path = tempfile.mkstemp()\n",
        ),
        CapabilityCase::new(
            "flask_debug_true",
            ScanProfile::Full,
            "src/server.py",
            "app.run(debug=True)\n",
            "src/server.py",
            "app.run(debug=False)\n",
        ),
        CapabilityCase::new(
            "random_for_security",
            ScanProfile::Full,
            "src/token.py",
            "import random\ntoken = random.randint(0, 100000)\n",
            "src/token.py",
            "import secrets\ntoken = secrets.randbelow(100000)\n",
        ),
        CapabilityCase::new(
            "request_no_cert_verify",
            ScanProfile::Full,
            "src/client.py",
            "response = requests.get(url, verify=False)\n",
            "src/client.py",
            "response = requests.get(url, verify=True)\n",
        ),
        CapabilityCase::new(
            "xml_xxe_vulnerable",
            ScanProfile::Full,
            "src/load.py",
            "document = xml.etree.ElementTree.parse(path)\n",
            "src/load.py",
            "document = json.loads(payload)\n",
        ),
        CapabilityCase::new(
            "ssl_with_bad_defaults",
            ScanProfile::Full,
            "src/tls.py",
            "wrapped = ssl.wrap_socket(sock, ssl_version=ssl.PROTOCOL_SSLv3)\n",
            "src/tls.py",
            "context = ssl.create_default_context()\nwrapped = context.wrap_socket(sock)\n",
        ),
        CapabilityCase::new(
            "ssh_no_host_key_verification",
            ScanProfile::Full,
            "src/ssh.py",
            "client.set_missing_host_key_policy(paramiko.AutoAddPolicy())\n",
            "src/ssh.py",
            "client.set_missing_host_key_policy(paramiko.RejectPolicy())\n",
        ),
        CapabilityCase::new(
            "assignment_in_operand",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn execute() {\n    if value = compute() { work(); }\n}\n",
            "src/lib.rs",
            "fn execute() {\n    let value = compute();\n    if value == expected { work(); }\n}\n",
        )
        .negative_files(vec![
            file(
                "src/ui.ts",
                "if (capturePinned !== pinned) update.capturePinned = pinned\nif (/[<>{}=]/.test(content)) continue\n",
            ),
            file(
                "src/range.rs",
                "if !(1..=5).contains(&count) { reject(); }\n",
            ),
            file(
                "src/walrus.py",
                "if (matched := pattern.search(line)):\n    consume(matched)\n",
            ),
        ]),
        CapabilityCase::new(
            "resource_not_closed",
            ScanProfile::Full,
            "src/files.py",
            "handle = open(path)\nconsume(handle)\n",
            "src/files.py",
            "with open(path) as handle:\n    consume(handle)\n",
        )
        .negative_files(vec![
            file("src/files.py", "with open(path) as handle:\n    consume(handle)\n"),
            file("src/store.rs", "fn connect(path: &Path) -> Result<Connection> { Ok(Connection::open(path)?) }\n"),
            file("src/ui.ts", "if (!menu || !active) setMenuOpen(false)\n"),
        ]),
        CapabilityCase::new(
            "empty_conditional_branch",
            ScanProfile::Full,
            "src/main.js",
            "function execute(value) { if (value) {} }\n",
            "src/main.js",
            "function execute(value) { if (value) { work(); } }\n",
        )
        .negative_files(vec![file(
            "src/guards.js",
            "function execute(value) { if (!value) return; work(value); }\n",
        )]),
        CapabilityCase::new(
            "unreachable_code",
            ScanProfile::Gating,
            "src/lib.rs",
            "fn execute(value: i32) {\n    return;\n    consume(value);\n}\n",
            "src/lib.rs",
            "fn execute(value: i32) {\n    consume(value);\n}\n",
        )
        .negative_files(vec![
            file(
                "src/lib.rs",
                "fn execute(value: i32) -> i32 {\n    #[cfg(not(debug_assertions))]\n    return value;\n    #[cfg(debug_assertions)]\n    return value + 1;\n}\n",
            ),
            file(
                "src/flow.py",
                "def pick(value):\n    if value == 1:\n        return 'one'\n    elif value == 2:\n        return 'two'\n    return 'many'\n\n\ndef stream():\n    raise RuntimeError()\n    yield b''\n",
            ),
        ])
        .method(AnalysisMethod::Ast),
        CapabilityCase::new(
            "constant_condition",
            ScanProfile::Full,
            "src/lib.rs",
            "fn execute(value: i32) {\n    if true { consume(value); }\n}\n",
            "src/lib.rs",
            "fn execute(value: i32) {\n    if value > 0 { consume(value); }\n}\n",
        )
        .negative_files(vec![
            file(
                "src/lib.rs",
                "fn execute(value: i32) {\n    if value > 0 { consume(value); }\n}\n",
            ),
            file(
                "src/Scan.java",
                "class Scan {\n  int find(int[] values) {\n    int i = 0;\n    for (; true; i++) {\n      if (values[i] == 0) break;\n    }\n    return i;\n  }\n}\n",
            ),
        ])
        .method(AnalysisMethod::Ast),
        CapabilityCase::new(
            "cpp_raw_ptr_ownership",
            ScanProfile::Full,
            "src/main.cpp",
            "int* value = new int(42);\n",
            "src/main.cpp",
            "auto value = std::make_unique<int>(42);\n",
        ),
        CapabilityCase::new(
            "ruby_double_bang",
            ScanProfile::Full,
            "src/main.rb",
            "enabled = !!value\n",
            "src/main.rb",
            "enabled = !value.nil?\n",
        ),
        CapabilityCase::new(
            "csharp_linq_enumeration",
            ScanProfile::Full,
            "src/Handler.cs",
            "class Handler { void Process(IEnumerable<int> items) { var a = items.Count(); var b = items.ToList(); var c = items.ToArray(); } }\n",
            "src/Handler.cs",
            "class Handler { void Process(IEnumerable<int> items) { var all = items.ToList(); var count = all.Count; } }\n",
        ),
        CapabilityCase::new(
            "kotlin_empty_when",
            ScanProfile::Full,
            "src/Main.kt",
            "fun choose(value: Int) {\n    when (value) {\n    }\n}\n",
            "src/Main.kt",
            "fun choose(value: Int) {\n    when (value) {\n        1 -> println(value)\n        else -> Unit\n    }\n}\n",
        ),
        CapabilityCase::new(
            "exit_in_production",
            ScanProfile::Gating,
            "src/service.rs",
            "fn execute() { std::process::exit(1); }\n",
            "src/service.rs",
            "fn execute() -> Result<(), Error> { Err(Error) }\n",
        ),
        CapabilityCase::new(
            "unhandled_termination_in_production",
            ScanProfile::Full,
            "src/main.rs",
            "fn execute() { panic!(\"failed\"); }\n",
            "src/main.rs",
            "fn execute() -> Result<(), Error> { Err(Error) }\n",
        ),
        CapabilityCase::new(
            "sql_injection",
            ScanProfile::Full,
            "src/db.go",
            "query, err := db.Query(fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", userID))\n",
            "src/db.py",
            "cursor.execute(\"SELECT * FROM users WHERE id = ?\", (user_id,))\n",
        )
        .negative_files(vec![
            file("src/ui.ts", "selectedSessionIndex: clamp(state.selectedSessionIndex + delta, 0, maxIndex)\nconst selected = selectedCommandSubmission(state, 'session')\n"),
            file("src/db.py", "cursor.execute(\"SELECT * FROM users WHERE id = ?\", (user_id,))\n"),
        ])
        .method(AnalysisMethod::Ast),
        CapabilityCase::new(
            "shell_execution",
            ScanProfile::Gating,
            "src/run.py",
            "subprocess.call(command, shell=True)\n",
            "src/run.py",
            "subprocess.run([\"ls\", path], shell=False, check=True)\n",
        )
        .negative_files(vec![
            file("src/run.py", "subprocess.run([\"ls\", path], shell=False, check=True)\n"),
            file("src/run.rs", "let output = std::process::Command::new(\"git\").arg(\"status\").output()?;\n"),
        ]),
        CapabilityCase::new(
            "insecure_cryptography",
            ScanProfile::Full,
            "src/hash.go",
            "package hash\nimport \"crypto/md5\"\nfunc digest(data []byte) { md5.Sum(data) }\n",
            "src/hash.go",
            "package hash\nimport \"crypto/sha256\"\nfunc digest(data []byte) { sha256.Sum256(data) }\n",
        )
        .negative_files(vec![
            file("src/ui.ts", "if (prefix.includes(' ')) return commandOverrides(state)\n"),
            file("src/hash.go", "package hash\nimport \"crypto/sha256\"\nfunc digest(data []byte) { sha256.Sum256(data) }\n"),
        ])
        .method(AnalysisMethod::Ast),
        CapabilityCase::new(
            "console_output_in_production",
            ScanProfile::Full,
            "src/main.js",
            "console.log(\"ready\");\n",
            "src/main.js",
            "logger.info(\"ready\");\n",
        )
        .negative_files(vec![
            file("src/main.js", "logger.info(\"ready\");\n"),
            file("bin/cli.js", "console.error(\"usage: cli <file>\");\n"),
            file("scripts/build.js", "console.log(\"built\");\n"),
            file("sandbox/server.js", "console.log(\"listening\");\n"),
            file("gulpfile.js", "console.log(\"hello\");\n"),
        ]),
        CapabilityCase::new(
            "debugger_in_production",
            ScanProfile::Gating,
            "src/main.js",
            "debugger;\nwork();\n",
            "src/main.js",
            "work();\n",
        ),
        CapabilityCase::new(
            "empty_catch_block",
            ScanProfile::Gating,
            "src/main.js",
            "try { risky(); } catch (error) {}\n",
            "src/main.js",
            "try { risky(); } catch (error) { logger.error(error); }\n",
        )
        .negative_files(vec![
            file(
                "src/main.js",
                "try { risky(); } catch (error) { logger.error(error); }\n",
            ),
            file(
                "src/fallback.ts",
                "const report = request().catch(() => ({} as Record<string, unknown>));\n",
            ),
        ]),
        CapabilityCase::new(
            "no_await_in_loop",
            ScanProfile::Full,
            "src/main.js",
            "async function execute(items) { for (const item of items) { await handle(item); } }\n",
            "src/main.js",
            "async function execute(item) { return await handle(item); }\n",
        )
        .negative_files(vec![file(
            "src/stream.js",
            "async function execute(items) { for await (const item of items) { await handle(item); } }\n",
        )]),
        CapabilityCase::new(
            "require_atomic_updates",
            ScanProfile::Full,
            "src/main.js",
            "async function add(value) { total += await value; }\n",
            "src/main.js",
            "async function add(value) { const resolved = await value; total = resolved; }\n",
        ),
        CapabilityCase::new(
            "dead_code",
            ScanProfile::Full,
            "src/main.rs",
            "fn main() {}\nfn unused_helper() {}\n",
            "src/main.rs",
            "fn main() { helper(); }\nfn helper() {}\npub fn external_api() {}\n#[allow(dead_code)]\nfn documented_anchor() {}\n",
        ),
        CapabilityCase::new(
            "orphan_symbol",
            ScanProfile::Full,
            "src/config.rs",
            "const DB_NAME: &str = \"app\";\nstatic MAX_RETRIES: usize = 3;\n",
            "src/config.rs",
            "pub const DB_NAME: &str = \"app\";\n",
        )
        .positive_files(vec![
            file(
                "src/config.rs",
                "const DB_NAME: &str = \"app\";\nstatic MAX_RETRIES: usize = 3;\n",
            ),
            file("src/main.rs", "fn main() {}\n"),
        ])
        .negative_files(vec![
            file("src/config.rs", "pub const DB_NAME: &str = \"app\";\n"),
            file(
                "src/main.rs",
                "use crate::config::DB_NAME;\nconst _: &str = DB_NAME;\n",
            ),
            file(
                "src/alloc.rs",
                "use std::alloc::System;\n\n#[global_allocator]\nstatic ALLOC: System = System;\n\nstruct Tokens(Vec<u8>);\n\nimpl std::ops::Deref for Tokens {\n    type Target = Vec<u8>;\n    fn deref(&self) -> &Vec<u8> {\n        &self.0\n    }\n}\n",
            ),
        ]),
        CapabilityCase::new(
            "go_log_fatal",
            ScanProfile::Gating,
            "src/main.go",
            "package main\nimport \"log\"\nfunc main() { log.Fatal(\"failed\") }\n",
            "src/main.go",
            "package main\nimport \"log\"\nfunc main() { log.Print(\"failed\") }\n",
        ),
        CapabilityCase::new(
            "go_errcheck",
            ScanProfile::Gating,
            "src/main.go",
            "package main\nfunc write() { file.Write(data) }\n",
            "src/main.go",
            "package main\nfunc write() { if _, err := file.Write(data); err != nil { handle(err) } }\n",
        )
        .negative_files(vec![
            file("src/main.go", "package main\nfunc write() { if _, err := file.Write(data); err != nil { handle(err) } }\n"),
            file("_examples/custom/main.go", "package main\nfunc write() { file.Write(data) }\n"),
            file("src/writer.go", "package main\nfunc (w *writer) Write(p []byte) (int, error) { return w.inner.Write(p) }\nfunc (w *writer) Close() error { n, _ := w.buf.Write(nil); _ = n; return w.inner.Close() }\n"),
        ]),
        CapabilityCase::new(
            "unused_assignment",
            ScanProfile::Gating,
            "src/main.go",
            "package main\nfunc compute() int {\n    value := 42\n    value = 7\n    return value\n}\n",
            "src/main.go",
            "package main\nfunc main() {\n    value := 42\n    println(value)\n}\n",
        ),
        CapabilityCase::new(
            "go_repeated_string_literal",
            ScanProfile::Full,
            "src/main.go",
            "package main\n// Call `main` once; a backtick in a comment opens no raw string.\nfunc main() {\n    a := \"hello\"\n    b := \"hello\"\n    c := \"hello\"\n}\n",
            "src/main.go",
            "package main\nfunc main() {\n    a := \"hello\"\n    b := \"hello\"\n}\n",
        )
        .negative_files(vec![file(
            "src/main.go",
            "package main\n\n// Use `go vet` or a '`' rune; neither opens a raw string.\nvar tick = '`'\n\ntype Article struct {\n\tTitle string `db:\"title\" json:\"title\" xml:\"title\"`\n}\n\nconst script = `\nif [ \"${out}\" = \"x\" ]; then\n  echo \"${out}\"\nfi\necho \"${out}\"\n`\n\nfunc main() {\n\ta := \"hello\"\n\tb := \"hello\"\n\t_, _ = a, b\n}\n",
        )]),
        CapabilityCase::new(
            "java_inefficient_string_buffer",
            ScanProfile::Full,
            "src/Main.java",
            "public class Main { void build() { StringBuffer value = new StringBuffer(); } }\n",
            "src/Main.java",
            "public class Main { void build() { StringBuilder value = new StringBuilder(); } }\n",
        ),
        CapabilityCase::new(
            "kt_non_null_assertion",
            ScanProfile::Gating,
            "src/Main.kt",
            "fun length(value: String?): Int { return value!!.length }\n",
            "src/Main.kt",
            "fun length(value: String?): Int { return value?.length ?: 0 }\n",
        ),
        CapabilityCase::new(
            "kt_require_check_misuse",
            ScanProfile::Full,
            "src/Main.kt",
            "fun validate(value: String) {\n    require(false)\n}\n",
            "src/Main.kt",
            "fun validate(value: String) {\n    require(value.isNotEmpty())\n}\n",
        ),
        CapabilityCase::new(
            "unsafe_type_cast",
            ScanProfile::Full,
            "src/Main.kt",
            "fun cast(value: Any): String { return value as String }\n",
            "src/Main.kt",
            "import a.Value as Alias\nfun cast(value: Any): String? { return value as? String }\n",
        ),
    ]
}
