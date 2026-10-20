//! `network_call_without_timeout`: HTTP calls and clients whose library
//! default never times out, from a per-language client API table. Python
//! follows Bandit B113 (`requests` without `timeout`, `httpx` with
//! `timeout=None`). Clients with a bounded default (httpx, aiohttp, OkHttp,
//! the `reqwest::blocking` client, Node's `fetch`) are not in the table.

use auditor_ast::{AstNode, RepoAst};
use auditor_core::{Confidence, Finding, Severity};

use crate::api_identity::ImportScope;
use crate::ast_scan::{self, AstScan};
use crate::java_api_identity::JavaScope;
use crate::js_api_identity::{js_arguments, object_properties, JsScope};
use crate::python_api_identity::{keyword_argument, positional_arguments, PyScope};
use crate::rust_api_identity::RustScope;
use crate::{util, AnalysisMethod, AuditedFile, Language};

/// A call or client construction that can wait forever, and the API it
/// resolves to.
struct Site<'a> {
    node: AstNode<'a>,
    api: String,
}

fn supported(language: &Language) -> bool {
    matches!(
        language,
        Language::Python
            | Language::Go
            | Language::Java
            | Language::JavaScript
            | Language::TypeScript
            | Language::Rust
    )
}

pub(crate) fn network_call_without_timeout_findings(
    files: &[AuditedFile],
    method: AnalysisMethod,
    cache: &util::FileCache,
    repo_ast: &mut RepoAst,
) -> Vec<Finding> {
    // `axios.defaults.timeout = ..` anywhere bounds every global axios call.
    let mut axios_defaults = false;
    ast_scan::visit_ast_files(
        AstScan::new(files, method, cache, repo_ast),
        |language| matches!(language, Language::JavaScript | Language::TypeScript),
        |_, tree, _| axios_defaults |= tree.source().contains("defaults.timeout"),
    );
    let mut findings = Vec::new();
    ast_scan::visit_ast_files(
        AstScan::new(files, method, cache, repo_ast),
        supported,
        |file, tree, _| {
            let root = tree.root_node();
            // A client constructed in a file that sets a timeout anywhere
            // (`.timeout(..)`, `Timeout:`, `setReadTimeout`) is bounded there.
            let file_bounds_clients = mentions_timeout(tree.source());
            let sites = match file.language {
                Language::Python => python_sites(root),
                Language::Go => go_sites(root, file_bounds_clients),
                Language::Java => java_sites(root, file_bounds_clients),
                Language::JavaScript | Language::TypeScript => {
                    js_sites(root, file_bounds_clients, axios_defaults)
                }
                Language::Rust => rust_sites(root, file_bounds_clients),
                _ => Vec::new(),
            };
            let relative = util::path_slash(&file.relative);
            for site in sites {
                let line = site.node.start_position().0 + 1;
                findings.push(
                    auditor_core::finding!(
                        "network_call_without_timeout",
                        "systems_economics",
                        "Network call without a timeout",
                        format!(
                            "`{}` at line {line} has no timeout; a stalled peer blocks it indefinitely",
                            site.api
                        ),
                        Severity::High,
                        Confidence::Medium,
                        &relative,
                        Some(line),
                    )
                    .with_recommendation(
                        "Set a connect and read timeout on the call or its client, sized to the caller's latency budget.",
                    )
                    .with_evidence("api", site.api.as_str())
                    .with_evidence("cwe", "CWE-400"),
                );
            }
        },
    );
    findings
}

/// The resolved calls whose API is in `table`, shown as `label` names it.
fn table_sites<'a>(
    calls: Vec<(String, AstNode<'a>)>,
    table: &[&str],
    label: impl Fn(&str) -> &str,
) -> Vec<Site<'a>> {
    calls
        .into_iter()
        .filter(|(api, _)| table.contains(&api.as_str()))
        .map(|(api, node)| Site {
            api: label(&api).to_string(),
            node,
        })
        .collect()
}

fn mentions_timeout(text: &str) -> bool {
    text.to_ascii_lowercase().contains("timeout")
}

/// Whether the function enclosing `node` mentions a timeout, as a later
/// `request.setTimeout(..)` or a `timeout` variable does.
fn enclosing_function_mentions_timeout(node: AstNode<'_>) -> bool {
    std::iter::successors(node.parent(), |parent| parent.parent())
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
                    | "function"
            )
        })
        .is_some_and(|function| mentions_timeout(function.text()))
}

const REQUESTS_VERBS: &[&str] = &[
    "get", "options", "head", "post", "put", "patch", "delete", "request",
];
const HTTPX_ATTRIBUTES: &[&str] = &[
    "get",
    "options",
    "head",
    "post",
    "put",
    "patch",
    "delete",
    "request",
    "stream",
    "Client",
    "AsyncClient",
];

fn python_sites(root: AstNode<'_>) -> Vec<Site<'_>> {
    PyScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(api, call)| {
            let timeout = keyword_argument(*call, "timeout");
            let explicit_none = timeout.is_some_and(|value| value.text() == "None");
            // `**options` may carry the timeout.
            let unset = timeout.is_none()
                && !call
                    .child_by_field_name("arguments")
                    .is_some_and(|arguments| arguments.text().contains("**"));
            match api.split_once('.') {
                Some(("requests", verb)) if REQUESTS_VERBS.contains(&verb) => {
                    explicit_none || unset
                }
                Some(("httpx", attribute)) if HTTPX_ATTRIBUTES.contains(&attribute) => {
                    explicit_none
                }
                _ => {
                    api == "urllib.request.urlopen"
                        && (explicit_none || (unset && positional_arguments(*call).len() < 3))
                }
            }
        })
        .map(|(api, node)| Site { node, api })
        .collect()
}

fn go_sites(root: AstNode<'_>, file_bounds_clients: bool) -> Vec<Site<'_>> {
    let scope = ImportScope::go(root);
    // The package-level helpers use `http.DefaultClient`, which has no
    // timeout and cannot be given one per call.
    let mut sites = table_sites(
        scope.calls(root),
        &[
            "net/http.Get",
            "net/http.Post",
            "net/http.PostForm",
            "net/http.Head",
        ],
        |api| api,
    );
    for call in ast_scan::nodes_of_kind(root, &["call_expression"]) {
        let receiver = call
            .child_by_field_name("function")
            .filter(|callee| callee.kind() == "selector_expression")
            .and_then(|callee| callee.child_by_field_name("operand"));
        if receiver
            .and_then(|receiver| scope.resolve(receiver))
            .as_deref()
            == Some("net/http.DefaultClient")
        {
            sites.push(Site {
                node: call,
                api: format!("http.DefaultClient.{}", go_method(call)),
            });
        }
    }
    if file_bounds_clients {
        return sites;
    }
    // A client with a custom `Transport` may bound its dials and headers there.
    for literal in ast_scan::nodes_of_kind(root, &["composite_literal"]) {
        let client = literal
            .child_by_field_name("type")
            .and_then(|kind| scope.resolve(kind))
            .is_some_and(|kind| kind == "net/http.Client");
        let configures_transport = literal
            .child_by_field_name("body")
            .is_some_and(|body| body.text().contains("Transport"));
        if client && !configures_transport {
            sites.push(Site {
                node: literal,
                api: "http.Client{}".to_string(),
            });
        }
    }
    sites
}

fn go_method(call: AstNode<'_>) -> &str {
    call.child_by_field_name("function")
        .and_then(|callee| callee.child_by_field_name("field"))
        .map_or("Do", |field| field.text())
}

/// `java.net.http.HttpClient` has no connect timeout unless its builder sets
/// one, and its requests none unless each `HttpRequest` sets one.
fn java_sites(root: AstNode<'_>, file_bounds_clients: bool) -> Vec<Site<'_>> {
    if file_bounds_clients {
        return Vec::new();
    }
    table_sites(
        JavaScope::new(root).calls(root),
        &[
            "java.net.http.HttpClient.newHttpClient",
            "java.net.http.HttpClient.newBuilder",
        ],
        |api| api.trim_start_matches("java.net.http."),
    )
}

fn js_sites(root: AstNode<'_>, file_bounds_clients: bool, axios_defaults: bool) -> Vec<Site<'_>> {
    JsScope::new(root)
        .calls(root)
        .into_iter()
        .filter_map(|(api, call)| {
            let api = api.replace(".default", "");
            let unbounded = match api.as_str() {
                "axios.create" => !file_bounds_clients && !axios_defaults && lacks_timeout(call, 0),
                "http.get" | "http.request" | "https.get" | "https.request" => {
                    node_request_lacks_timeout(call)
                }
                _ => !axios_defaults && axios_call_lacks_timeout(&api, call),
            };
            unbounded.then_some(Site { node: call, api })
        })
        .collect()
}

/// Whether an axios call's config, by its position, sets no timeout.
fn axios_call_lacks_timeout(api: &str, call: AstNode<'_>) -> bool {
    let config = match api {
        // `axios(url, config)` takes a URL string first; any other first
        // argument is the config (`axios(config)`).
        "axios" => {
            let first_is_url = js_arguments(call)
                .first()
                .is_some_and(|first| matches!(first.kind(), "string" | "template_string"));
            usize::from(first_is_url)
        }
        "axios.request" => 0,
        "axios.get" | "axios.delete" | "axios.head" | "axios.options" => 1,
        "axios.post" | "axios.put" | "axios.patch" => 2,
        _ => return false,
    };
    lacks_timeout(call, config)
}

/// Whether the argument at `index` is absent, or an object literal without
/// a `timeout` (or a spread that may carry one). Any other expression may
/// hold a timeout.
fn lacks_timeout(call: AstNode<'_>, index: usize) -> bool {
    match js_arguments(call).get(index) {
        None => true,
        Some(config) if config.kind() == "object" => {
            !config
                .named_children()
                .any(|property| property.kind() == "spread_element")
                && !object_properties(*config)
                    .iter()
                    .any(|(key, _)| key == "timeout")
        }
        Some(_) => false,
    }
}

/// Node's `http.request`: options without `timeout`, and no
/// `request.setTimeout(..)` in the enclosing function.
fn node_request_lacks_timeout(call: AstNode<'_>) -> bool {
    let arguments = js_arguments(call);
    let options = arguments
        .iter()
        .take(2)
        .find(|argument| argument.kind() == "object");
    let opaque_options = arguments
        .first()
        .is_some_and(|first| !matches!(first.kind(), "object" | "string" | "template_string"));
    let options_lack_timeout = options.is_none_or(|options| {
        !object_properties(*options)
            .iter()
            .any(|(key, _)| key == "timeout")
    });
    !opaque_options && options_lack_timeout && !enclosing_function_mentions_timeout(call)
}

/// `reqwest::get` builds a default client per call, which has no timeout;
/// so do `Client::new()` and a builder that never sets one. An inline
/// `#[cfg(test)]` module is test code.
fn rust_sites(root: AstNode<'_>, file_bounds_clients: bool) -> Vec<Site<'_>> {
    RustScope::new(root)
        .calls(root)
        .into_iter()
        .filter(|(_, call)| !ast_scan::is_inside_rust_test(*call))
        .filter(|(api, _)| match api.as_str() {
            "reqwest::get" => true,
            "reqwest::Client::new" | "reqwest::Client::default" | "reqwest::Client::builder" => {
                !file_bounds_clients
            }
            _ => false,
        })
        .map(|(api, node)| Site { node, api })
        .collect()
}
