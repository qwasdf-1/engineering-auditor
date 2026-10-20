mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-errorprone-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn make_file(root: &std::path::Path, rel: &str, text: &str) -> AuditedFile {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        let language = if rel.ends_with(".rs") {
            crate::Language::Rust
        } else if rel.ends_with(".js") || rel.ends_with(".ts") {
            crate::Language::JavaScript
        } else if rel.ends_with(".go") {
            crate::Language::Go
        } else {
            crate::Language::Python
        };
        AuditedFile {
            absolute: path,
            relative: std::path::PathBuf::from(rel),
            language,
            bytes: text.len() as u64,
            role: crate::SourceRole::Production,
        }
    }

    fn test_cache(file: &AuditedFile) -> util::FileCache {
        util::FileCache::new(std::slice::from_ref(file))
    }

    fn ast_scan<'files, 'ast>(
        files: &'files [AuditedFile],
        cache: &'files util::FileCache,
        repo_ast: &'ast mut auditor_ast::RepoAst,
    ) -> crate::ast_scan::AstScan<'files, 'ast> {
        crate::ast_scan::AstScan::new(files, crate::AnalysisMethod::Ast, cache, repo_ast)
    }

    fn assignment_in_operand_findings(
        files: &[AuditedFile],
        cache: &util::FileCache,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::assignment_in_operand_findings(ast_scan(files, cache, &mut repo_ast))
    }

    fn resource_not_closed_findings(
        files: &[AuditedFile],
        cache: &util::FileCache,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::resource_not_closed_findings(ast_scan(files, cache, &mut repo_ast))
    }

    fn empty_conditional_branch_findings(
        files: &[AuditedFile],
        method: crate::AnalysisMethod,
        cache: &util::FileCache,
        _repo_ast: &auditor_ast::RepoAst,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::empty_conditional_branch_findings(crate::ast_scan::AstScan::new(
            files,
            method,
            cache,
            &mut repo_ast,
        ))
    }

    macro_rules! ast_detector_adapter {
        ($name:ident) => {
            fn $name(files: &[AuditedFile], cache: &util::FileCache) -> Vec<Finding> {
                let mut repo_ast = auditor_ast::RepoAst::new();
                super::$name(ast_scan(files, cache, &mut repo_ast))
            }
        };
    }

    ast_detector_adapter!(exit_in_production_findings);
    ast_detector_adapter!(console_output_in_production_findings);
    ast_detector_adapter!(debugger_in_production_findings);
    ast_detector_adapter!(empty_catch_block_findings);
    ast_detector_adapter!(no_await_in_loop_findings);

    #[test]
    fn errorprone_detects_assignment_in_condition() {
        let root = temp_root("assign");
        let file = make_file(&root, "src/main.rs", "if x = 5 {\n    return;\n}\n");
        let cache = test_cache(&file);
        let findings = assignment_in_operand_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "assignment_in_operand"),
            "expected assignment_in_operand finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn errorprone_skips_if_let_assignment() {
        let root = temp_root("iflet");
        let file = make_file(
            &root,
            "src/service.rs",
            "if let Some(x) = y {\n    return x;\n}\n",
        );
        let cache = test_cache(&file);
        let findings = assignment_in_operand_findings(&[file], &cache);
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "assignment_in_operand"),
            "if let should not be flagged as assignment_in_operand"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn assignment_after_condition_is_not_in_operand() {
        let root = temp_root("assign_after_condition");
        let file = make_file(
            &root,
            "src/main.ts",
            "if (capturePinned !== pinned) update.capturePinned = pinned\n",
        );
        let cache = test_cache(&file);
        assert!(assignment_in_operand_findings(&[file], &cache).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn errorprone_detects_resource_not_closed() {
        let root = temp_root("resource");
        let file = make_file(
            &root,
            "src/main.py",
            "handle = open(path)\nconsume(handle)\n",
        );
        let cache = test_cache(&file);
        let findings = resource_not_closed_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "resource_not_closed"),
            "expected resource_not_closed finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rust_raii_open_is_not_a_resource_leak() {
        let root = temp_root("resource_raii");
        let file = make_file(
            &root,
            "src/main.rs",
            "let connection = Connection::open(path)?;\nconsume(connection);\n",
        );
        let cache = test_cache(&file);
        assert!(resource_not_closed_findings(&[file], &cache).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn best_practices_detects_console_log() {
        let root = temp_root("console");
        let file = make_file(&root, "src/main.js", "console.log(\"hello\");\n");
        let cache = test_cache(&file);
        let findings = console_output_in_production_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "console_output_in_production"),
            "expected console_output_in_production finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn best_practices_detects_debugger() {
        let root = temp_root("dbg");
        let file = make_file(&root, "src/main.js", "debugger;\n");
        let cache = test_cache(&file);
        let findings = debugger_in_production_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "debugger_in_production"),
            "expected debugger_in_production finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_await_in_loop_detects_await_in_for() {
        let root = temp_root("await_loop");
        let file = make_file(
        &root,
        "src/main.js",
        "async function process(items) {\n    for (const item of items) {\n        await handler(item);\n    }\n}\n",
    );
        let cache = test_cache(&file);
        let findings = no_await_in_loop_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "no_await_in_loop"),
            "expected no_await_in_loop finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn empty_catch_block_detected_in_js() {
        let root = temp_root("ecb_js");
        let file = make_file(
            &root,
            "src/main.js",
            "try {\n    risky();\n} catch (e) {}\n",
        );
        let cache = test_cache(&file);
        let findings = empty_catch_block_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "empty_catch_block"),
            "expected empty_catch_block finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn catch_value_fallback_is_not_an_empty_handler() {
        let root = temp_root("ecb_value_fallback");
        let file = make_file(
            &root,
            "src/main.ts",
            "const report = request().catch(() => ({} as Record<string, unknown>));\n",
        );
        let cache = test_cache(&file);
        let findings = empty_catch_block_findings(&[file], &cache);
        assert!(
            findings.is_empty(),
            "an object-valued promise fallback is not an empty catch block"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exit_in_production_detected_in_rust() {
        let root = temp_root("exit_rust");
        let file = make_file(
            &root,
            "src/service.rs",
            "fn main() {\n    std::process::exit(0);\n}\n",
        );
        let cache = test_cache(&file);
        let findings = exit_in_production_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "exit_in_production"),
            "expected exit_in_production finding for std::process::exit"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exit_in_production_does_not_claim_go_support() {
        let root = temp_root("exit_go");
        let file = make_file(
            &root,
            "src/service.go",
            "package service\nfunc handle() {\n    os.Exit(1)\n}\n",
        );
        let cache = test_cache(&file);
        let findings = exit_in_production_findings(&[file], &cache);
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exit_in_production_does_not_claim_python_support() {
        let root = temp_root("exit_other");
        let file = make_file(&root, "src/service.py", "sys.exit(0)\n");
        let cache = test_cache(&file);
        let findings = exit_in_production_findings(&[file], &cache);
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpp_raw_ptr_ownership_detected() {
        let root = temp_root("cpp_raw");
        let file = {
            let path = root.join("src/main.cpp");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "int* p = new int(42);\n").unwrap();
            AuditedFile {
                absolute: path,
                relative: std::path::PathBuf::from("src/main.cpp"),
                language: crate::Language::Cpp,
                bytes: 22,
                role: crate::SourceRole::Production,
            }
        };
        let cache = test_cache(&file);
        let findings = cpp_raw_ptr_ownership_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "cpp_raw_ptr_ownership"),
            "expected cpp_raw_ptr_ownership finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpp_raw_ptr_with_smart_ptr_is_silent() {
        let root = temp_root("cpp_smart");
        let file = {
            let path = root.join("src/main.cpp");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "auto p = std::make_unique<int>(42);\n").unwrap();
            AuditedFile {
                absolute: path,
                relative: std::path::PathBuf::from("src/main.cpp"),
                language: crate::Language::Cpp,
                bytes: 34,
                role: crate::SourceRole::Production,
            }
        };
        let cache = test_cache(&file);
        let findings = cpp_raw_ptr_ownership_findings(&[file], &cache);
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "cpp_raw_ptr_ownership"),
            "expected no cpp_raw_ptr_ownership for smart pointer"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ruby_double_bang_detected() {
        let root = temp_root("ruby_dbang");
        let file = {
            let path = root.join("src/main.rb");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "!!value\n").unwrap();
            AuditedFile {
                absolute: path,
                relative: std::path::PathBuf::from("src/main.rb"),
                language: crate::Language::Ruby,
                bytes: 8,
                role: crate::SourceRole::Production,
            }
        };
        let cache = test_cache(&file);
        let findings = ruby_double_bang_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "ruby_double_bang"),
            "expected ruby_double_bang finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn csharp_linq_enumeration_detected() {
        let root = temp_root("csharp_linq");
        let file = {
            let path = root.join("src/Handler.cs");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
            &path,
            "using System.Linq;\nclass Handler {\n    void Process(IEnumerable<int> items) {\n        var a = items.Count();\n        var b = items.ToList();\n        var c = items.ToArray();\n    }\n}\n",
        ).unwrap();
            AuditedFile {
                absolute: path,
                relative: std::path::PathBuf::from("src/Handler.cs"),
                language: crate::Language::CSharp,
                bytes: 135,
                role: crate::SourceRole::Production,
            }
        };
        let cache = test_cache(&file);
        let findings = csharp_linq_enumeration_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "csharp_linq_enumeration"),
            "expected csharp_linq_enumeration finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn empty_conditional_branch_detected_via_ast() {
        let root = temp_root("empty_branch_ast");
        let file = make_file(
            &root,
            "src/test.js",
            "function test() {\n    if (true) {}\n}\n",
        );
        let cache = test_cache(&file);
        let method = crate::AnalysisMethod::Auto;
        let mut repo_ast = auditor_ast::RepoAst::new();
        let text = cache.text(&file);
        let _ = repo_ast.get_or_parse(&file.absolute, &auditor_ast::AstLang::JavaScript, text);
        let findings =
            empty_conditional_branch_findings(&[file], method, &cache, &repo_ast);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "empty_conditional_branch"),
            "expected empty_conditional_branch finding"
        );
    }

    #[test]
    fn no_await_in_loop_skips_outside_loop() {
        let root = temp_root("await_no_loop");
        let file = make_file(
            &root,
            "src/main.js",
            "async function process(item) {\n    return await handler(item);\n}\n",
        );
        let cache = test_cache(&file);
        let findings = no_await_in_loop_findings(&[file], &cache);
        assert!(
            !findings.iter().any(|f| f.detector == "no_await_in_loop"),
            "expected no no_await_in_loop finding for await outside loop"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_await_in_loop_skips_for_await_iteration_boundary() {
        let root = temp_root("await_for_await");
        let file = make_file(
            &root,
            "src/main.js",
            "async function consume(items) { for await (const item of items) { await handle(item); } }\n",
        );
        let cache = test_cache(&file);
        let findings = no_await_in_loop_findings(&[file], &cache);
        assert!(!findings
            .iter()
            .any(|finding| finding.detector == "no_await_in_loop"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn empty_conditional_branch_skips_single_statement_guards() {
        let root = temp_root("guard_branch");
        let file = make_file(
            &root,
            "src/main.js",
            "function execute(value) { if (!value) return; work(value); }\n",
        );
        let cache = test_cache(&file);
        let mut repo_ast = auditor_ast::RepoAst::new();
        let text = cache.text(&file);
        let _ = repo_ast.get_or_parse(
            &file.absolute,
            &auditor_ast::AstLang::JavaScript,
            text,
        );
        let findings = empty_conditional_branch_findings(
            std::slice::from_ref(&file),
            crate::AnalysisMethod::Auto,
            &cache,
            &repo_ast,
        );
        assert!(!findings
            .iter()
            .any(|finding| finding.detector == "empty_conditional_branch"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn require_atomic_updates_detects_compound_await() {
        let root = temp_root("atomic");
        let file = make_file(
            &root,
            "src/main.js",
            "async function add(x) {\n    total += await x;\n}\n",
        );
        let cache = test_cache(&file);
        let findings = require_atomic_updates_findings(&[file], &cache);
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "require_atomic_updates"),
            "expected require_atomic_updates finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn require_atomic_updates_skips_simple_await() {
        let root = temp_root("atomic_simple");
        let file = make_file(
            &root,
            "src/main.js",
            "async function get(id) {\n    return await db.find(id);\n}\n",
        );
        let cache = test_cache(&file);
        let findings = require_atomic_updates_findings(&[file], &cache);
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "require_atomic_updates"),
            "expected no require_atomic_updates finding for simple await"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_await_in_loop_skips_await_in_while() {
        let root = temp_root("await_while");
        let file = make_file(
            &root,
            "src/main.js",
            "async function poll() {\n    while (condition) {\n        await tick();\n    }\n}\n",
        );
        let cache = test_cache(&file);
        let findings = no_await_in_loop_findings(&[file], &cache);
        // A `while` loop polls or consumes events; each pass waits on purpose.
        assert!(
            !findings.iter().any(|f| f.detector == "no_await_in_loop"),
            "a while loop is not a collection loop"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
