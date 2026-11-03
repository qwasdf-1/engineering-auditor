#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_repo() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ea-mcp-{nonce}"));
        std::fs::create_dir_all(root.join("src")).expect("temporary repository");
        std::fs::write(root.join("src/lib.rs"), "pub fn value() -> usize { 1 }\n")
            .expect("source fixture");
        root
    }

    fn call_tool(server: &McpServer, id: u64, name: &str, arguments: Value) -> Value {
        let response = server.handle(
            &json!({
                "id": id,
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {"arguments": arguments, "name": name}
            })
            .to_string(),
        );
        serde_json::from_str(&response).expect("JSON-RPC response")
    }

    #[test]
    fn exposes_scan_job_and_catalog_tools_without_review_tools() {
        let server = McpServer::default();
        let response: Value = serde_json::from_str(
            &server.handle(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#),
        )
        .expect("tools list response");
        let tools = response["result"]["tools"].as_array().expect("tools");
        assert!(tools.iter().all(|tool| tool["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Experimental: "))));
        let names = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from(["audit_catalog", "audit_job_get", "audit_scan_start"])
        );
        let removed = call_tool(&server, 2, "audit_review_start", json!({}));
        assert_eq!(removed["error"]["code"], -32602);
    }

    #[test]
    fn mcp_text_results_are_readable_markdown_not_embedded_json() {
        let server = McpServer::default();
        let response = call_tool(&server, 1, "audit_catalog", json!({"profile": "gating"}));
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("readable content");
        assert!(text.starts_with("# audit catalog result\n"));
        assert!(text.contains("## Output"));
        assert!(!text.trim_start().starts_with('{'));
        assert!(response["result"]["structuredContent"]["detectors"].is_array());
    }

    #[test]
    fn asynchronous_scan_job_writes_artifact() {
        let root = temp_repo();
        let output = root.join("audit.json");
        let server = McpServer::default();
        let start = call_tool(
            &server,
            1,
            "audit_scan_start",
            json!({"output": output, "profile": "full", "repo_root": root}),
        );
        let job_id = start["result"]["structuredContent"]["job_id"]
            .as_str()
            .expect("job id")
            .to_string();
        let mut completed = None;
        for attempt in 0..100 {
            let response = call_tool(
                &server,
                100 + attempt,
                "audit_job_get",
                json!({"job_id": job_id}),
            );
            let record = &response["result"]["structuredContent"];
            if matches!(record["status"].as_str(), Some("complete" | "failed")) {
                completed = Some(record.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let completed = completed.expect("scan job finishes");
        assert_eq!(completed["status"], "complete", "{completed}");
        assert!(output.exists());
        auditor_report::validate_report_json(&std::fs::read_to_string(&output).expect("report"))
            .expect("valid report");
        std::fs::remove_dir_all(root).expect("remove temporary repository");
    }
}
