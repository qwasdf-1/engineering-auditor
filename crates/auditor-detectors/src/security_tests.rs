mod tests {
    use super::*;
    use crate::security_literal_rules::shannon_entropy;
    use crate::security_rules::{
        bind_all_interfaces_findings, flask_debug_true_findings,
        hardcoded_credential_findings, hardcoded_tmp_directory_findings,
        random_for_security_findings, request_no_cert_verify_findings,
        ssh_no_host_key_verification_findings, ssl_with_bad_defaults_findings,
        try_except_pass_findings, unsafe_deserialization_findings,
        world_writable_file_findings, xml_xxe_vulnerable_findings,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "engineering-auditor-security-{name}-{}-{nanos}",
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
        } else if rel.ends_with(".java") {
            crate::Language::Java
        } else if rel.ends_with(".kt") {
            crate::Language::Kotlin
        } else if rel.ends_with(".cs") {
            crate::Language::CSharp
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

    fn sql_injection_findings(
        files: &[AuditedFile],
        method: crate::AnalysisMethod,
        cache: &util::FileCache,
        _repo_ast: &auditor_ast::RepoAst,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::sql_injection_findings(crate::ast_scan::AstScan::new(
            files,
            method,
            cache,
            &mut repo_ast,
        ))
    }

    fn insecure_cryptography_findings(
        files: &[AuditedFile],
        method: crate::AnalysisMethod,
        cache: &util::FileCache,
        _repo_ast: &auditor_ast::RepoAst,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::insecure_cryptography_findings(crate::ast_scan::AstScan::new(
            files,
            method,
            cache,
            &mut repo_ast,
        ))
    }

    fn shell_execution_findings(
        files: &[AuditedFile],
        cache: &util::FileCache,
    ) -> Vec<Finding> {
        let mut repo_ast = auditor_ast::RepoAst::new();
        super::shell_execution_findings(crate::ast_scan::AstScan::new(
            files,
            crate::AnalysisMethod::Ast,
            cache,
            &mut repo_ast,
        ))
    }

    fn security_findings(
        files: &[AuditedFile],
        cache: &util::FileCache,
        config: &AuditorConfig,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut repo_ast = auditor_ast::RepoAst::new();
        findings.extend(hardcoded_credential_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(unsafe_deserialization_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(bind_all_interfaces_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(try_except_pass_findings(files, cache, config));
        findings.extend(world_writable_file_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(hardcoded_tmp_directory_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(flask_debug_true_findings(files, cache, config));
        let structure =
            crate::structure_index::StructureIndex::build(crate::ast_scan::AstScan::context(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ));
        let graph = crate::call_graph::CallGraph::build(&structure);
        findings.extend(random_for_security_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
            (&structure, &graph),
        ));
        findings.extend(request_no_cert_verify_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(xml_xxe_vulnerable_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(ssl_with_bad_defaults_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings.extend(ssh_no_host_key_verification_findings(
            crate::ast_scan::AstScan::new(
                files,
                crate::AnalysisMethod::Ast,
                cache,
                &mut repo_ast,
            ),
            config,
        ));
        findings
    }

    #[test]
    fn security_detects_hardcoded_password() {
        let root = temp_root("pw");
        let file = make_file(&root, "src/config.py", "password = \"supersecret\"\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "hardcoded_credential"),
            "expected hardcoded_credential finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn security_skips_env_var() {
        let root = temp_root("pw_env");
        let file = make_file(
            &root,
            "src/config.py",
            "password = os.getenv(\"DB_PASS\")\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "hardcoded_credential"),
            "expected no hardcoded_credential finding for env var"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    fn credential_targets(rel: &str, text: &str) -> Vec<String> {
        let root = temp_root("credential_names");
        let file = make_file(&root, rel, text);
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        fs::remove_dir_all(&root).unwrap();
        findings
            .iter()
            .filter(|f| f.detector == "hardcoded_credential")
            .filter_map(|f| f.identity.get("target").cloned())
            .collect()
    }

    #[test]
    fn credential_named_layout_constants_are_not_credentials() {
        let rust = "const CREDENTIAL_STORE_DIR: &str = \"credentials\";\n\
            const SECRET_BACKEND_CONFIG_KEY: &str = \"secret_backend_kind\";\n\
            pub const CREDENTIAL_MATERIAL_KIND: &str = \"credential_material\";\n\
            pub const CREDENTIAL_SCHEMA_VERSION: &str = \"1.0.0\";\n\
            const CREDENTIAL_ACCESS_OWNER: &str = \"vault-credential-access\";\n\
            const SECRET_KEY: &str = \"k3J9vQ2mX8pL5nR7\";\n";
        assert_eq!(credential_targets("src/layout.rs", rust), ["SECRET_KEY"]);
        let script = "const routes = {\n  api_key: \"api_key\",\n  oauth: \"browser_login\",\n};\n\
            const secretsPath = \"secrets\";\nconst dbPassword = \"hunter2hunter\";\n";
        assert_eq!(credential_targets("src/routes.ts", script), ["dbPassword"]);
        let python = "password = \"password1\"\nsecret_store_dir = \"secrets\"\n";
        assert_eq!(credential_targets("src/settings.py", python), ["password"]);
    }

    #[test]
    fn literals_that_restate_name_or_describe_a_credential_are_not_credentials() {
        let script = "export const ACCESS_TOKEN_NOT_FOUND = 'access_token/access_token_not_found';\n\
            const grants = { CLIENT_CREDENTIALS: \"clientCredentials\" };\n\
            const keys = { SECRET_ENVIRONMENTS: \"secretEnvironments\" };\n\
            const init = { credentials: \"include\" };\n\
            const SECRET_KEY = `(?:api[-_]?key|private[-_]?key)`;\n\
            const DB_PASSWORD = \"SuperSecretPassword\";\n\
            const POSTHOG_API_KEY = 'phc_9CipPajQC22mSkk2wxe2TXsUA0Ysyupe8dt5KQQ';\n";
        assert_eq!(
            credential_targets("src/errors.ts", script),
            ["DB_PASSWORD", "POSTHOG_API_KEY"]
        );
        let java = "class Mapper {\n\
            static final String SECRET_REF = \"x-secret-ref\";\n\
            static final String PARAMETER_PROJECT_ACL_API_KEY_ID = \"projectAclApiKeyId\";\n\
            static final String ADMIN_PASSWORD = \"Tr0ub4dor&3xyz\";\n}\n";
        assert_eq!(
            credential_targets("src/main/java/app/Mapper.java", java),
            ["ADMIN_PASSWORD"]
        );
        let go = "package cli\n\nconst flagResetPasswordHelp = \"Reset user password\"\n";
        assert_eq!(credential_targets("internal/cli/cli.go", go), Vec::<String>::new());
    }

    #[test]
    fn security_detects_unsafe_pickle() {
        let root = temp_root("pickle");
        let file = make_file(&root, "src/main.py", "data = pickle.loads(blob)\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "unsafe_deserialization"),
            "expected unsafe_deserialization finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entropy_detects_high_entropy_string_with_credential_keyword() {
        let root = temp_root("entropy_pos");
        let file = make_file(
            &root,
            "src/config.py",
            "api_key = \"X7k9#mP2rL5@vN8*qW1!zC4&bJ6\"\n",
        );
        let cache = test_cache(&file);
        // Verify the test string has high entropy
        let e = shannon_entropy("X7k9#mP2rL5@vN8*qW1!zC4&bJ6");
        assert!(e >= 4.5, "test string entropy {} should be >= 4.5", e);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "hardcoded_credential"),
            "expected hardcoded_credential finding for high-entropy string with api_key"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entropy_skips_low_entropy_string_with_credential_keyword() {
        let root = temp_root("entropy_neg");
        let file = make_file(&root, "src/config.py", "api_key = \"default_key_123\"\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "hardcoded_credential" && f.title.contains("high-entropy")),
            "expected no entropy-based finding for low-entropy string"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entropy_skips_high_entropy_without_credential_keyword() {
        let root = temp_root("entropy_noctx");
        let file = make_file(
            &root,
            "src/utils.py",
            "label = \"X7k9#mP2rL5@vN8*qW1!zC4&bJ6\"\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        let entropy_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.detector == "hardcoded_credential" && f.title.contains("high-entropy"))
            .collect();
        assert!(
            entropy_findings.is_empty(),
            "expected no entropy-based finding without credential context"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entropy_with_custom_threshold() {
        let root = temp_root("entropy_cfg");
        let file = make_file(
            &root,
            "src/config.py",
            "secret = \"X7k9#mP2rL5@vN8*qW1!zC4&bJ6\"\n",
        );
        let cache = test_cache(&file);
        let mut config = AuditorConfig::default();
        config.detectors.insert(
            "hardcoded_credential".to_string(),
            crate::config::DetectorConfig {
                enabled: None,
                threshold: None,
                severity: None,
                entropy_threshold: Some(10.0),
                min_length: None,
                max_length: None,
                ..Default::default()
            },
        );
        let findings = security_findings(&[file], &cache, &config);
        assert!(
            !findings
                .iter()
                .any(|f| f.detector == "hardcoded_credential" && f.title.contains("high-entropy")),
            "expected no entropy-based finding with threshold above string entropy"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entropy_skips_short_strings() {
        let root = temp_root("entropy_short");
        let file = make_file(&root, "src/config.py", "token = \"abc123\"\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        let entropy_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.detector == "hardcoded_credential" && f.title.contains("high-entropy"))
            .collect();
        assert!(
            entropy_findings.is_empty(),
            "expected no entropy-based finding for short strings (< 8 chars)"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn security_detects_unsafe_yaml() {
        let root = temp_root("yaml");
        let file = make_file(&root, "src/main.py", "data = yaml.load(content)\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "unsafe_deserialization"),
            "expected unsafe_deserialization for yaml.load"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn yaml_load_with_a_safe_loader_is_not_unsafe_deserialization() {
        let root = temp_root("yaml_safe");
        let file = make_file(
            &root,
            "src/main.py",
            "a = yaml.load(content, Loader=yaml.SafeLoader)\nb = yaml.load(content, yaml.CSafeLoader)\nc = yaml.load(content, Loader=yaml.UnsafeLoader)\n",
        );
        let cache = test_cache(&file);
        let lines = security_findings(&[file], &cache, &AuditorConfig::default())
            .into_iter()
            .filter(|finding| finding.detector == "unsafe_deserialization")
            .filter_map(|finding| finding.locations.first().and_then(|location| location.line))
            .collect::<Vec<_>>();
        assert_eq!(lines, [3]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sql_injection_detects_f_string_concat() {
        let root = temp_root("sqli");
        let file = make_file(&root, "src/db.go", "selectQuery, err := db.Query(fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", user_id))\n");
        let cache = test_cache(&file);
        let findings = sql_injection_findings(
            &[file],
            crate::AnalysisMethod::Ast,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(
            findings.iter().any(|f| f.detector == "sql_injection"),
            "expected sql_injection finding for f-string SQL"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sql_injection_skips_parametrized_query() {
        let root = temp_root("sqli_safe");
        let file = make_file(
            &root,
            "src/db.go",
            "package db\nfunc load(userID int) { db.Query(\"SELECT * FROM users WHERE id = ?\", userID) }\n",
        );
        let cache = test_cache(&file);
        let findings = sql_injection_findings(
            &[file],
            crate::AnalysisMethod::Ast,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(
            !findings.iter().any(|f| f.detector == "sql_injection"),
            "expected no sql_injection finding for parametrized query"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sql_injection_follows_queries_built_in_locals() {
        let root = temp_root("sqli_locals");
        let python = "def find(cur, user_id):\n    q = \"SELECT * FROM users WHERE id = \" + user_id\n    cur.execute(q)\n\ndef safe(cur, user_id):\n    q = \"SELECT * FROM users WHERE id = %s\"\n    cur.execute(q, (user_id,))\n\ndef appended(cur, name):\n    q = \"SELECT * FROM users WHERE 1=1\"\n    if name:\n        q += \" AND name = '\" + name + \"'\"\n    cur.execute(q)\n\ndef table(cur, table):\n    q = \"SELECT * FROM \" + table\n    cur.execute(q)\n\ndef replaced(cur, user_id):\n    q = \"SELECT * FROM users WHERE id = \" + user_id\n    q = \"SELECT 1\"\n    cur.execute(q)\n";
        let files = [
            make_file(&root, "src/a.py", python),
            make_file(
                &root,
                "src/b.js",
                "async function f(db, id) {\n  const sql = `SELECT * FROM t WHERE id = ${id}`;\n  return db.query(sql);\n}\n",
            ),
            make_file(
                &root,
                "src/c.go",
                "package c\nfunc f(db *sql.DB, id string) {\n\tq := \"SELECT * FROM t WHERE id = \" + id\n\tdb.Query(q)\n}\n",
            ),
            make_file(
                &root,
                "src/D.java",
                "class D { void f(Statement st, String id) throws Exception {\n  String q = \"SELECT * FROM t WHERE id = \" + id;\n  st.executeQuery(q);\n} }\n",
            ),
            make_file(
                &root,
                "src/E.cs",
                "class E { void F(SqlConnection c, string id) {\n  var sql = $\"DELETE FROM t WHERE id = {id}\";\n  var cmd = new SqlCommand(sql, c);\n} }\n",
            ),
            make_file(
                &root,
                "src/F.kt",
                "fun f(st: Statement, id: String) {\n    val q = \"SELECT * FROM t WHERE id = $id\"\n    st.executeQuery(q)\n}\n",
            ),
            make_file(
                &root,
                "src/g.rs",
                "fn f(conn: &Connection, id: &str) {\n    let q = format!(\"SELECT * FROM t WHERE id = {}\", id);\n    conn.execute(&q, []);\n}\n",
            ),
        ];
        let cache = util::FileCache::new(&files);
        let mut found = sql_injection_findings(
            &files,
            crate::AnalysisMethod::Ast,
            &cache,
            &auditor_ast::RepoAst::new(),
        )
        .into_iter()
        .map(|finding| {
            let location = finding.locations.first().unwrap();
            (
                location.path.clone(),
                location.line,
                match finding.evidence.get("query_built_at") {
                    Some(auditor_core::EvidenceValue::String(line)) => Some(line.clone()),
                    _ => None,
                },
            )
        })
        .collect::<Vec<_>>();
        found.sort();
        let built = |line: &str| Some(line.to_string());
        assert_eq!(
            found,
            [
                ("src/D.java".to_string(), Some(3), built("line 2")),
                ("src/E.cs".to_string(), Some(3), built("line 2")),
                ("src/F.kt".to_string(), Some(3), built("line 2")),
                ("src/a.py".to_string(), Some(3), built("line 2")),
                ("src/a.py".to_string(), Some(13), built("line 12")),
                ("src/b.js".to_string(), Some(3), built("line 2")),
                ("src/c.go".to_string(), Some(4), built("line 3")),
                ("src/g.rs".to_string(), Some(3), built("line 2")),
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sql_injection_requires_sql_words_and_a_query_sink() {
        let root = temp_root("sqli_selected_identifier");
        let file = make_file(
            &root,
            "src/ui.ts",
            "selectedSessionIndex: clamp(state.selectedSessionIndex + delta, 0, maxIndex)\n",
        );
        let cache = test_cache(&file);
        let findings = sql_injection_findings(
            &[file],
            crate::AnalysisMethod::Auto,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shell_execution_detects_shell_true() {
        let root = temp_root("shell");
        let file = make_file(&root, "src/run.py", "subprocess.call(cmd, shell=True)\n");
        let cache = test_cache(&file);
        let findings = shell_execution_findings(&[file], &cache);
        assert!(
            findings.iter().any(|f| f.detector == "shell_execution"),
            "expected shell_execution finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shell_execution_skips_safe_subprocess() {
        let root = temp_root("shell_safe");
        let file = make_file(
            &root,
            "src/run.py",
            "result = subprocess.check_output(cmd)\n",
        );
        let cache = test_cache(&file);
        let findings = shell_execution_findings(&[file], &cache);
        assert!(
            !findings.iter().any(|f| f.detector == "shell_execution"),
            "expected no shell_execution finding for safe subprocess"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shell_execution_skips_argument_vector_commands() {
        let root = temp_root("shell_rust_safe");
        let file = make_file(
            &root,
            "src/run.rs",
            "let output = std::process::Command::new(\"git\").arg(\"status\").output()?;\n",
        );
        let cache = test_cache(&file);
        assert!(shell_execution_findings(&[file], &cache).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn insecure_cryptography_detects_md5() {
        let root = temp_root("crypto");
        let file = make_file(
            &root,
            "src/hash.go",
            "package hash\nfunc digest(data []byte) { md5.Sum(data) }\n",
        );
        let cache = test_cache(&file);
        let findings = insecure_cryptography_findings(
            &[file],
            crate::AnalysisMethod::Ast,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "insecure_cryptography"),
            "expected insecure_cryptography finding for md5"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn insecure_cryptography_detects_des() {
        let root = temp_root("crypto_des");
        let file = make_file(
            &root,
            "src/cipher.go",
            "package cipher\nfunc encrypt(key []byte) { des.NewCipher(key) }\n",
        );
        let cache = test_cache(&file);
        let findings = insecure_cryptography_findings(
            &[file],
            crate::AnalysisMethod::Ast,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "insecure_cryptography"),
            "expected insecure_cryptography finding for DES"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn insecure_cryptography_uses_call_name_boundaries() {
        let root = temp_root("crypto_boundary");
        let file = make_file(
            &root,
            "src/ui.ts",
            "if (prefix.includes(' ')) return commandOverrides(state)\n",
        );
        let cache = test_cache(&file);
        let findings = insecure_cryptography_findings(
            &[file],
            crate::AnalysisMethod::Auto,
            &cache,
            &auditor_ast::RepoAst::new(),
        );
        assert!(findings.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn bind_all_interfaces_detects_0_0_0_0() {
        let root = temp_root("bind");
        let file = make_file(
            &root,
            "src/server.py",
            "server.bind((\"0.0.0.0\", 8080))\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "bind_all_interfaces"),
            "expected bind_all_interfaces finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn try_except_pass_detected() {
        let root = temp_root("trypass");
        let file = make_file(&root, "src/main.py", "try:\n    risky()\nexcept: pass\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "try_except_pass"),
            "expected try_except_pass finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn world_writable_file_detected() {
        let root = temp_root("chmod");
        let file = make_file(&root, "src/config.py", "os.chmod('secret.txt', 0o777)\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "world_writable_file"),
            "expected world_writable_file finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn world_writable_file_skips_permission_removal_and_owner_write() {
        let root = temp_root("chmod_safe");
        let file = make_file(
            &root,
            "src/config.py",
            "path.chmod(mode & ~stat.S_IWUSR & ~stat.S_IWGRP & ~stat.S_IWOTH)\nchild.chmod(child.stat().st_mode | stat.S_IWUSR)\nos.chmod(path, 0o600)\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(!findings
            .iter()
            .any(|finding| finding.detector == "world_writable_file"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hardcoded_tmp_directory_detected() {
        let root = temp_root("tmpdir");
        let file = make_file(&root, "src/main.py", "temporary_path = '/tmp/unsafe'\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "hardcoded_tmp_directory"),
            "expected hardcoded_tmp_directory finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hardcoded_tmp_directory_skips_non_python_mounts_and_secure_mktemp() {
        let root = temp_root("tmpdir_safe");
        let rust = make_file(
            &root,
            "src/main.rs",
            "command.arg(\"--tmpfs\").arg(\"/tmp:rw,noexec,nosuid,size=64m\");\n",
        );
        let python = make_file(
            &root,
            "src/main.py",
            "handle, temporary_path = tempfile.mkstemp()\n",
        );
        let files = [rust, python];
        let cache = util::FileCache::new(&files);
        let findings = security_findings(&files, &cache, &AuditorConfig::default());
        assert!(!findings
            .iter()
            .any(|finding| finding.detector == "hardcoded_tmp_directory"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn hardcoded_credential_skips_api_key_prompt_copy() {
        let root = temp_root("credential_prompt");
        let file = make_file(
            &root,
            "src/composer.ts",
            "const PLACEHOLDERS = { ApiKeyPaste: \"paste your API key\" };\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(!findings
            .iter()
            .any(|finding| finding.detector == "hardcoded_credential"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn flask_debug_true_detected() {
        let root = temp_root("flask");
        let file = make_file(&root, "src/app.py", "app.run(debug=True)\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "flask_debug_true"),
            "expected flask_debug_true finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn random_for_security_detected() {
        let root = temp_root("random_sec");
        let file = make_file(
            &root,
            "src/token.py",
            "token = random.randint(0, 1000000)\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "random_for_security"),
            "expected random_for_security finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn request_no_cert_verify_detected() {
        let root = temp_root("noverify");
        let file = make_file(
            &root,
            "src/client.py",
            "requests.get('https://example.com', verify=False)\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "request_no_cert_verify"),
            "expected request_no_cert_verify finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn xml_xxe_vulnerable_detected() {
        let root = temp_root("xxe");
        let file = make_file(&root, "src/parse.py", "tree = etree.parse(source)\n");
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings.iter().any(|f| f.detector == "xml_xxe_vulnerable"),
            "expected xml_xxe_vulnerable finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ssl_with_bad_defaults_detected() {
        let root = temp_root("sslbad");
        let file = make_file(
            &root,
            "src/tls.py",
            "ssl.wrap_socket(ssl_version=ssl.PROTOCOL_SSLv3)\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "ssl_with_bad_defaults"),
            "expected ssl_with_bad_defaults finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ssh_no_host_key_verification_detected() {
        let root = temp_root("sshbad");
        let file = make_file(
            &root,
            "src/ssh.py",
            "client.set_missing_host_key_policy(paramiko.AutoAddPolicy())\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default());
        assert!(
            findings
                .iter()
                .any(|f| f.detector == "ssh_no_host_key_verification"),
            "expected ssh_no_host_key_verification finding"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn random_helpers_are_reported_where_callers_make_secrets() {
        let root = temp_root("random_helpers");
        let file = make_file(
            &root,
            "src/ids.js",
            "function randomString(n) {\n  let s = '';\n  for (let i = 0; i < n; i++) { s += chars[Math.floor(Math.random() * chars.length)]; }\n  return s;\n}\nfunction wrap(n) { return randomString(n); }\nexport function login() {\n  const sessionToken = wrap(32);\n  const label = randomString(4);\n  return sessionToken + label;\n}\n",
        );
        let cache = test_cache(&file);
        let findings = security_findings(&[file], &cache, &AuditorConfig::default())
            .into_iter()
            .filter(|finding| finding.detector == "random_for_security")
            .collect::<Vec<_>>();
        let reported = findings
            .iter()
            .map(|finding| {
                let via = match finding.evidence.get("via") {
                    Some(auditor_core::EvidenceValue::String(via)) => via.clone(),
                    _ => String::new(),
                };
                (finding.locations[0].line, via)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            reported,
            [(
                Some(8),
                "wrap (src/ids.js:6) -> randomString (src/ids.js:1)".to_string()
            )]
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
