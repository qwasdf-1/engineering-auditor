//! Security rules matched through resolved API identity: Python callee text
//! plus each language's API-identity site finder (Rust, Go, Java, JS/TS,
//! Kotlin, C#, C/C++).

use auditor_core::{Confidence, Severity};

use auditor_ast::AstNode;

use crate::ast_scan::{self, AstScan};
use crate::c_api_rules::{
    c_bind_all_interfaces, c_deprecated_tls_versions, c_disabled_certificate_checks,
    c_predictable_random, c_ssh_insecure_host_keys, c_world_writable_modes, c_xxe_parsers,
};
use crate::csharp_api_rules::{
    csharp_bind_all_interfaces, csharp_deprecated_tls_versions, csharp_disabled_certificate_checks,
    csharp_predictable_random, csharp_ssh_insecure_host_keys, csharp_unsafe_deserialization,
    csharp_world_writable_modes, csharp_xxe_parsers,
};
use crate::go_api_rules::{
    go_bind_all_interfaces, go_deprecated_tls_versions, go_insecure_skip_verify,
    go_math_rand_calls, go_ssh_insecure_host_keys, go_world_writable_modes,
};
use crate::java_api_rules::{
    java_bind_all_interfaces, java_deprecated_tls_versions, java_disabled_certificate_checks,
    java_predictable_random, java_ssh_insecure_host_keys, java_unsafe_deserialization,
    java_world_writable_modes, java_xxe_parsers,
};
use crate::js_api_rules::{
    js_bind_all_interfaces, js_deprecated_tls_versions, js_disabled_certificate_checks,
    js_predictable_random, js_ssh_insecure_host_keys, js_unsafe_deserialization,
    js_world_writable_modes, js_xxe_parsers,
};
use crate::kotlin_api_rules::{
    kotlin_bind_all_interfaces, kotlin_deprecated_tls_versions, kotlin_disabled_certificate_checks,
    kotlin_predictable_random, kotlin_ssh_insecure_host_keys, kotlin_unsafe_deserialization,
    kotlin_world_writable_modes, kotlin_xxe_parsers,
};
use crate::rust_api_rules::{
    rust_bind_all_interfaces, rust_deprecated_tls_versions, rust_disabled_certificate_checks,
    rust_predictable_random, rust_ssh_insecure_host_keys, rust_world_writable_modes,
};
use crate::security_random_context::in_security_context;
use crate::security_rules::{rule_finding, Rule};
use crate::{util, AuditorConfig, Language};
use auditor_core::Finding;

const UNSAFE_DESERIALIZATION: Rule = Rule {
    id: "unsafe_deserialization",
    title: "Unsafe deserialization call",
    severity: Severity::High,
    confidence: Confidence::High,
    recommendation: "Use a safe serialization format such as JSON or MessagePack.",
    cwe: "CWE-502",
};
const BIND_ALL_INTERFACES: Rule = Rule {
    id: "bind_all_interfaces",
    title: "Service binds to all network interfaces",
    severity: Severity::Low,
    confidence: Confidence::Low,
    recommendation: "Bind to 127.0.0.1 or a specific interface unless external access is required.",
    cwe: "CWE-605",
};
const WORLD_WRITABLE_FILE: Rule = Rule {
    id: "world_writable_file",
    title: "Permissive file permissions (Bandit B103)",
    severity: Severity::High,
    confidence: Confidence::High,
    recommendation: "Restrict file permissions to owner-only or group-only.",
    cwe: "CWE-732",
};
const RANDOM_FOR_SECURITY: Rule = Rule {
    id: "random_for_security",
    title: "Using random module for security-sensitive purpose (Bandit B311)",
    severity: Severity::Medium,
    confidence: Confidence::Low,
    recommendation: "Use the secrets module for security-sensitive randomness.",
    cwe: "CWE-330",
};
const REQUEST_NO_CERT_VERIFY: Rule = Rule {
    id: "request_no_cert_verify",
    title: "HTTPS request without certificate verification (Bandit B501)",
    severity: Severity::High,
    confidence: Confidence::High,
    recommendation: "Enable certificate verification or use a proper CA bundle.",
    cwe: "CWE-295",
};
const XML_XXE_VULNERABLE: Rule = Rule {
    id: "xml_xxe_vulnerable",
    title: "XML parsing with potential XXE (Bandit B313-317)",
    severity: Severity::High,
    confidence: Confidence::Low,
    recommendation: "Disable external entity processing when parsing XML.",
    cwe: "CWE-611",
};
const SSL_WITH_BAD_DEFAULTS: Rule = Rule {
    id: "ssl_with_bad_defaults",
    title: "SSL/TLS with deprecated protocol version (Bandit B502)",
    severity: Severity::High,
    confidence: Confidence::High,
    recommendation: "Use TLS v1.2 or higher and disable deprecated protocols.",
    cwe: "CWE-327",
};
const SSH_NO_HOST_KEY_VERIFICATION: Rule = Rule {
    id: "ssh_no_host_key_verification",
    title: "SSH host key verification disabled (Bandit B507)",
    severity: Severity::High,
    confidence: Confidence::Medium,
    recommendation: "Enable host key verification; avoid AutoAddPolicy and WarningPolicy.",
    cwe: "CWE-295",
};

pub(crate) fn unsafe_deserialization_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        UNSAFE_DESERIALIZATION,
        |callee, call| match callee {
            "pickle.loads" | "pickle.load" => true,
            "yaml.load" => !has_safe_yaml_loader(call),
            _ => false,
        },
        &[
            (Language::Java, java_unsafe_deserialization),
            (Language::JavaScript, js_unsafe_deserialization),
            (Language::TypeScript, js_unsafe_deserialization),
            (Language::Kotlin, kotlin_unsafe_deserialization),
            (Language::CSharp, csharp_unsafe_deserialization),
        ],
    )
}

/// `yaml.load(.., Loader=yaml.SafeLoader)` and the C and base loaders
/// construct only plain values (Bandit B506 exempts them).
fn has_safe_yaml_loader(call: &str) -> bool {
    ["SafeLoader", "CSafeLoader", "BaseLoader"]
        .iter()
        .any(|loader| util::count_word(call, loader) > 0)
}

pub(crate) fn bind_all_interfaces_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        BIND_ALL_INTERFACES,
        |callee, call| {
            (callee.ends_with(".bind") || callee.ends_with(".listen")) && call.contains("0.0.0.0")
        },
        &[
            (Language::Rust, rust_bind_all_interfaces),
            (Language::Go, go_bind_all_interfaces),
            (Language::Java, java_bind_all_interfaces),
            (Language::JavaScript, js_bind_all_interfaces),
            (Language::TypeScript, js_bind_all_interfaces),
            (Language::Kotlin, kotlin_bind_all_interfaces),
            (Language::CSharp, csharp_bind_all_interfaces),
            (Language::C, c_bind_all_interfaces),
            (Language::Cpp, c_bind_all_interfaces),
        ],
    )
}

pub(crate) fn world_writable_file_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        WORLD_WRITABLE_FILE,
        |callee, call| callee.ends_with(".chmod") && dangerous_chmod_mode(call),
        &[
            (Language::Rust, rust_world_writable_modes),
            (Language::Go, go_world_writable_modes),
            (Language::Java, java_world_writable_modes),
            (Language::JavaScript, js_world_writable_modes),
            (Language::TypeScript, js_world_writable_modes),
            (Language::Kotlin, kotlin_world_writable_modes),
            (Language::CSharp, csharp_world_writable_modes),
            (Language::C, c_world_writable_modes),
            (Language::Cpp, c_world_writable_modes),
        ],
    )
}

fn dangerous_chmod_mode(line: &str) -> bool {
    if !line.contains("chmod") {
        return false;
    }
    let Some((_, mode)) = line.rsplit_once(',') else {
        return false;
    };
    let token = mode.trim().trim_end_matches([')', ';']).replace('_', "");
    let parsed = if let Some(value) = token.strip_prefix("0o") {
        u32::from_str_radix(value, 8).ok()
    } else if let Some(value) = token.strip_prefix("0x") {
        u32::from_str_radix(value, 16).ok()
    } else if token.starts_with('0') && token.len() > 1 {
        u32::from_str_radix(&token[1..], 8).ok()
    } else {
        token.parse::<u32>().ok()
    };
    parsed.is_some_and(|mode| mode & 0o033 != 0)
}

pub(crate) fn random_for_security_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings_where(
        scan,
        RANDOM_FOR_SECURITY,
        |callee, _| callee.starts_with("random.") && callee != "random.seed",
        &[
            (Language::Rust, rust_predictable_random),
            (Language::Go, go_math_rand_calls),
            (Language::Java, java_predictable_random),
            (Language::JavaScript, js_predictable_random),
            (Language::TypeScript, js_predictable_random),
            (Language::Kotlin, kotlin_predictable_random),
            (Language::CSharp, csharp_predictable_random),
            (Language::C, c_predictable_random),
            (Language::Cpp, c_predictable_random),
        ],
        in_security_context,
    )
}

pub(crate) fn request_no_cert_verify_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        REQUEST_NO_CERT_VERIFY,
        |callee, call| {
            (callee.starts_with("requests.")
                || callee.starts_with("session.")
                || callee.to_ascii_lowercase().contains("http"))
                && (call.contains("verify=False") || call.contains("verify = False"))
        },
        &[
            (Language::Rust, rust_disabled_certificate_checks),
            (Language::Go, go_insecure_skip_verify),
            (Language::Java, java_disabled_certificate_checks),
            (Language::JavaScript, js_disabled_certificate_checks),
            (Language::TypeScript, js_disabled_certificate_checks),
            (Language::Kotlin, kotlin_disabled_certificate_checks),
            (Language::CSharp, csharp_disabled_certificate_checks),
            (Language::C, c_disabled_certificate_checks),
            (Language::Cpp, c_disabled_certificate_checks),
        ],
    )
}

pub(crate) fn xml_xxe_vulnerable_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        XML_XXE_VULNERABLE,
        |callee, _| {
            [
                "etree.parse",
                "elementtree.parse",
                "etree.fromstring",
                "sax.parse",
                "minidom.parse",
                "lxml.",
                "xml.parsers",
            ]
            .iter()
            .any(|pattern| callee.to_ascii_lowercase().contains(pattern))
        },
        &[
            (Language::Java, java_xxe_parsers),
            (Language::JavaScript, js_xxe_parsers),
            (Language::TypeScript, js_xxe_parsers),
            (Language::Kotlin, kotlin_xxe_parsers),
            (Language::CSharp, csharp_xxe_parsers),
            (Language::C, c_xxe_parsers),
            (Language::Cpp, c_xxe_parsers),
        ],
    )
}

/// Sites in a file of a language with resolved API identity.
type ApiSites = for<'a> fn(AstNode<'a>) -> Vec<AstNode<'a>>;

/// Python calls whose callee and call text satisfy `matches`, plus the sites
/// each language's API-identity finder in `sites` returns.
fn call_findings(
    scan: AstScan<'_, '_>,
    rule: Rule,
    matches: impl Fn(&str, &str) -> bool,
    sites: &[(Language, ApiSites)],
) -> Vec<Finding> {
    call_findings_where(scan, rule, matches, sites, |_| true)
}

/// [`call_findings`] reporting only the sites `keep` accepts.
fn call_findings_where(
    scan: AstScan<'_, '_>,
    rule: Rule,
    matches: impl Fn(&str, &str) -> bool,
    sites: &[(Language, ApiSites)],
    keep: impl Fn(AstNode<'_>) -> bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        scan,
        |language| {
            *language == Language::Python || sites.iter().any(|(owner, _)| owner == language)
        },
        |file, tree, _| {
            let rel = util::path_slash(&file.relative);
            if let Some((_, finder)) = sites.iter().find(|(owner, _)| *owner == file.language) {
                for site in finder(tree.root_node())
                    .into_iter()
                    .filter(|site| keep(*site))
                {
                    findings.push(rule_finding(
                        rule,
                        &rel,
                        site.start_position().0 + 1,
                        site.text(),
                    ));
                }
                return;
            }
            for call in ast_scan::nodes_of_kind(tree.root_node(), &["call"]) {
                let Some(callee) = call.child_by_field_name("function") else {
                    continue;
                };
                if matches(callee.text(), call.text()) && keep(call) {
                    findings.push(rule_finding(
                        rule,
                        &rel,
                        call.start_position().0 + 1,
                        call.text(),
                    ));
                }
            }
        },
    );
    findings
}

pub(crate) fn ssl_with_bad_defaults_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        SSL_WITH_BAD_DEFAULTS,
        |callee, call| {
            callee == "ssl.wrap_socket"
                && [
                    "ssl.protocol_sslv2",
                    "ssl.protocol_sslv3",
                    "ssl.protocol_tlsv1",
                ]
                .iter()
                .any(|version| call.to_ascii_lowercase().contains(version))
        },
        &[
            (Language::Rust, rust_deprecated_tls_versions),
            (Language::Go, go_deprecated_tls_versions),
            (Language::Java, java_deprecated_tls_versions),
            (Language::JavaScript, js_deprecated_tls_versions),
            (Language::TypeScript, js_deprecated_tls_versions),
            (Language::Kotlin, kotlin_deprecated_tls_versions),
            (Language::CSharp, csharp_deprecated_tls_versions),
            (Language::C, c_deprecated_tls_versions),
            (Language::Cpp, c_deprecated_tls_versions),
        ],
    )
}

pub(crate) fn ssh_no_host_key_verification_findings(
    scan: AstScan<'_, '_>,
    _config: &AuditorConfig,
) -> Vec<Finding> {
    call_findings(
        scan,
        SSH_NO_HOST_KEY_VERIFICATION,
        |callee, call| {
            callee.ends_with(".set_missing_host_key_policy")
                && (call.contains("AutoAddPolicy") || call.contains("WarningPolicy"))
        },
        &[
            (Language::Rust, rust_ssh_insecure_host_keys),
            (Language::Go, go_ssh_insecure_host_keys),
            (Language::Java, java_ssh_insecure_host_keys),
            (Language::JavaScript, js_ssh_insecure_host_keys),
            (Language::TypeScript, js_ssh_insecure_host_keys),
            (Language::Kotlin, kotlin_ssh_insecure_host_keys),
            (Language::CSharp, csharp_ssh_insecure_host_keys),
            (Language::C, c_ssh_insecure_host_keys),
            (Language::Cpp, c_ssh_insecure_host_keys),
        ],
    )
}
