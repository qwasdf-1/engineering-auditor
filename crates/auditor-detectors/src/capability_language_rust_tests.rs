//! Rust capability boundaries for the L1 rules that read resolved `use`
//! paths and statement shapes. Each negative is the nearest safe form of its
//! positive.

pub(super) fn rust_l1_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_source, negative_source) = match detector {
        "shell_execution" => (
            "use std::process::Command;\n\npub fn run(line: &str) {\n    let mut command = Command::new(\"sh\");\n    command.arg(\"-c\").arg(line);\n    command.status().ok();\n}\n",
            "use std::process::Command;\n\npub fn run(line: &str) {\n    Command::new(\"git\").arg(\"-c\").arg(line).status().ok();\n}\n",
        ),
        "console_output_in_production" => (
            "pub fn render(value: &str) {\n    println!(\"{value}\");\n}\n",
            "pub fn render(value: &str) {\n    log::info!(\"{value}\");\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn prints() {\n        println!(\"ok\");\n    }\n}\n",
        ),
        "debugger_in_production" => (
            "pub fn total(value: u32) -> u32 {\n    dbg!(value) + 1\n}\n",
            "pub fn total(value: u32) -> u32 {\n    log::debug!(\"{value}\");\n    value + 1\n}\n",
        ),
        "bind_all_interfaces" => (
            "use std::net::TcpListener;\n\npub fn serve() -> std::io::Result<TcpListener> {\n    TcpListener::bind(\"0.0.0.0:8080\")\n}\n",
            "use std::net::TcpListener;\n\npub fn serve() -> std::io::Result<TcpListener> {\n    TcpListener::bind(\"127.0.0.1:8080\")\n}\n",
        ),
        "world_writable_file" => (
            "use std::os::unix::fs::PermissionsExt;\n\npub fn share(path: &std::path::Path) -> std::io::Result<()> {\n    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))\n}\n",
            "use std::os::unix::fs::PermissionsExt;\n\npub fn share(path: &std::path::Path) -> std::io::Result<()> {\n    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))\n}\n",
        ),
        "random_for_security" => (
            "pub fn token() -> u64 {\n    fastrand::u64(..)\n}\n",
            "use rand::RngCore;\n\npub fn token() -> u64 {\n    rand::rngs::OsRng.next_u64()\n}\n",
        ),
        "request_no_cert_verify" => (
            "pub fn client() -> reqwest::Result<reqwest::Client> {\n    reqwest::Client::builder().danger_accept_invalid_certs(true).build()\n}\n",
            "pub fn client() -> reqwest::Result<reqwest::Client> {\n    reqwest::Client::builder().danger_accept_invalid_certs(false).build()\n}\n",
        ),
        "ssl_with_bad_defaults" => (
            "use native_tls::{Protocol, TlsConnector};\n\npub fn connector() -> native_tls::Result<TlsConnector> {\n    TlsConnector::builder().min_protocol_version(Some(Protocol::Tlsv10)).build()\n}\n",
            "use native_tls::{Protocol, TlsConnector};\n\npub fn connector() -> native_tls::Result<TlsConnector> {\n    TlsConnector::builder().min_protocol_version(Some(Protocol::Tlsv12)).build()\n}\n",
        ),
        "ssh_no_host_key_verification" => (
            "impl client::Handler for Client {\n    type Error = russh::Error;\n\n    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {\n        Ok(true)\n    }\n}\n",
            "impl client::Handler for Client {\n    type Error = russh::Error;\n\n    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {\n        Ok(self.known_hosts.contains(key))\n    }\n}\n",
        ),
        "insecure_cryptography" => (
            "use md5::{Digest, Md5};\n\npub fn digest(data: &[u8]) -> Vec<u8> {\n    Md5::digest(data).to_vec()\n}\n",
            "use sha2::{Digest, Sha256};\n\npub fn digest(data: &[u8]) -> Vec<u8> {\n    Sha256::digest(data).to_vec()\n}\n",
        ),
        "empty_conditional_branch" => (
            "pub fn check(value: bool) {\n    if value {\n    } else {\n        work();\n    }\n}\n",
            "pub fn check(value: bool) {\n    if !value {\n        work();\n    }\n    if value {\n        // the caller handles this case\n    }\n}\n",
        ),
        "no_await_in_loop" => (
            "pub async fn fetch(items: Vec<u32>) {\n    for item in items {\n        handle(item).await;\n    }\n}\n",
            "pub async fn fetch(items: Vec<Item>, mut stream: Stream) {\n    while let Some(item) = stream.next().await {\n        handle(item).await;\n    }\n    for item in &items {\n        if ping(item).await {\n            break;\n        }\n    }\n    for item in &items {\n        item.close().await;\n    }\n    for item in &items {\n        tokio::time::sleep(item.delay).await;\n    }\n}\n",
        ),
        "unused_assignment" => (
            "pub fn total(items: &[u32]) -> u32 {\n    let mut result = 0;\n    result = items.iter().sum();\n    result\n}\n",
            "pub fn total(items: &[u32]) -> u32 {\n    let mut result = items.len() as u32;\n    println!(\"{result}\");\n    result = items.iter().sum();\n    #[cfg(feature = \"fast\")]\n    let mode = 1;\n    #[cfg(not(feature = \"fast\"))]\n    let mode = 2;\n    result + mode\n}\n",
        ),
        "sql_injection" => (
            "pub fn find(conn: &Connection, name: &str) {\n    conn.execute(&format!(\"SELECT id FROM users WHERE name = '{name}'\"), []).ok();\n}\n",
            "pub fn find(conn: &Connection, name: &str) {\n    conn.execute(\"SELECT id FROM users WHERE name = ?1\", [format!(\"{name}\")]).ok();\n}\n",
        ),
        "naming_convention" => (
            "pub fn ComputeTotal(values: &[u32]) -> u32 {\n    values.iter().sum()\n}\n\npub struct request_handler;\n\npub const max_size: usize = 8;\n",
            "use std::fmt;\n\npub struct RequestHandler;\n\npub const MAX_SIZE_2: usize = 8;\n\nimpl fmt::Display for RequestHandler {\n    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {\n        formatter.write_str(\"handler\")\n    }\n}\n\npub fn compute_total(values: &[u32]) -> u32 {\n    values.iter().sum()\n}\n",
        ),
        "hardcoded_credential" => (
            "pub const API_KEY: &str = \"sk_live_9f8e7d6c5b4a3210\";\n",
            "pub const API_KEY_ENV: &str = \"SERVICE_API_KEY\";\n\npub fn api_key() -> Option<String> {\n    std::env::var(API_KEY_ENV).ok()\n}\n",
        ),
        "hardcoded_tmp_directory" => (
            "pub fn scratch() -> std::path::PathBuf {\n    std::path::PathBuf::from(\"/tmp/scratch\")\n}\n",
            "pub fn scratch() -> std::path::PathBuf {\n    std::env::temp_dir().join(\"scratch\")\n}\n",
        ),
        _ => return None,
    };
    let source = if positive {
        positive_source
    } else {
        negative_source
    };
    Some(vec![("src/capability.rs".to_string(), source.to_string())])
}

/// Rust boundaries for the L5 loop facts; each negative is the nearest
/// correct form of its positive.
pub(super) fn rust_fact_capability_files(
    detector: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (positive_source, negative_source) = match detector {
        "immutable_string_concatenation_in_loop" => (
            "pub fn render(items: &[String]) -> String {\n    let mut output = String::new();\n    for item in items {\n        output = format!(\"{output}{item}\");\n    }\n    output\n}\n",
            "pub fn render(items: &[String]) -> String {\n    let mut output = String::new();\n    for item in items {\n        output.push_str(item);\n    }\n    output\n}\n",
        ),
        "known_size_preallocation" => (
            "pub fn lengths(items: &[String]) -> Vec<usize> {\n    let mut result = Vec::new();\n    for item in items {\n        result.push(item.len());\n    }\n    result\n}\n",
            "pub fn lengths(items: &[String]) -> Vec<usize> {\n    let mut result = Vec::with_capacity(items.len());\n    for item in items {\n        result.push(item.len());\n    }\n    let mut kept = Vec::new();\n    for item in items {\n        if !item.is_empty() {\n            kept.push(item.len());\n        }\n    }\n    consume(kept);\n    result\n}\n",
        ),
        "range_for_each_iteration" => (
            "pub fn emit(limit: usize) {\n    (0..limit).for_each(|value| consume(value));\n}\n",
            "pub fn emit(limit: usize, items: &[usize]) {\n    for value in 0..limit {\n        consume(value);\n    }\n    items.iter().for_each(|value| consume(*value));\n}\n",
        ),
        _ => return None,
    };
    let source = if positive {
        positive_source
    } else {
        negative_source
    };
    Some(vec![("src/capability.rs".to_string(), source.to_string())])
}
