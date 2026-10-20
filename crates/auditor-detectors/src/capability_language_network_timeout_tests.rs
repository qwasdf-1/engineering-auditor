//! `network_call_without_timeout` boundaries for each Tier-1 client API
//! table. Positives hold only calls and clients the detector must report;
//! negatives hold the nearest bounded forms (an explicit timeout, a client
//! default that is bounded, a timeout set elsewhere in the file, options
//! the detector cannot see into).

use std::collections::BTreeMap;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

pub(crate) fn network_timeout_capability_files(
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (path, source) = match (language, positive) {
        ("python", true) => ("src/client.py", PYTHON_UNBOUNDED),
        ("python", false) => ("src/client.py", PYTHON_BOUNDED),
        ("go", true) => ("client.go", GO_UNBOUNDED),
        ("go", false) => ("client.go", GO_BOUNDED),
        ("java", true) => ("src/main/java/Client.java", JAVA_UNBOUNDED),
        ("java", false) => ("src/main/java/Client.java", JAVA_BOUNDED),
        ("javascript", true) => ("src/client.js", JS_UNBOUNDED),
        ("javascript", false) => ("src/client.js", JS_BOUNDED),
        ("typescript", true) => ("src/client.ts", JS_UNBOUNDED),
        ("typescript", false) => ("src/client.ts", JS_BOUNDED),
        ("rust", true) => ("src/client.rs", RUST_UNBOUNDED),
        ("rust", false) => ("src/client.rs", RUST_BOUNDED),
        _ => return None,
    };
    let mut files = vec![(path.to_string(), source.to_string())];
    if !positive {
        // Test code is out of scope.
        let test_path = match language {
            "python" => "tests/test_client.py",
            "go" => "client_test.go",
            "java" => "src/test/java/ClientTest.java",
            "javascript" => "src/client.test.js",
            "typescript" => "src/client.test.ts",
            _ => "tests/client.rs",
        };
        let unbounded = network_timeout_capability_files(language, true)?;
        files.push((test_path.to_string(), unbounded[0].1.clone()));
        if language == "rust" {
            files.push(("src/connect.rs".to_string(), RUST_TEST_MODULE.to_string()));
        }
    }
    Some(files)
}

const PYTHON_UNBOUNDED: &str = r#"import requests
import requests as http
import httpx
from urllib.request import urlopen


def fetch(url):
    requests.get(url)
    http.post(url, json={})
    requests.get(url, timeout=None)
    httpx.get(url, timeout=None)
    return urlopen(url)
"#;

const PYTHON_BOUNDED: &str = r#"import requests
import httpx
from urllib.request import urlopen


def fetch(url, options):
    requests.get(url, timeout=5)
    requests.post(url, **options)
    httpx.get(url)
    httpx.Client()
    urlopen(url, None, 10)
    session = requests.Session()
    return session.get(url)
"#;

const GO_UNBOUNDED: &str = r#"package client

import "net/http"

func Fetch(url string, req *http.Request) {
	http.Get(url)
	http.DefaultClient.Do(req)
	client := &http.Client{}
	client.Do(req)
}
"#;

const GO_BOUNDED: &str = r#"package client

import (
	"net/http"
	"time"
)

func Fetch(url string, req *http.Request, transport http.RoundTripper) {
	client := &http.Client{Timeout: 10 * time.Second}
	client.Do(req)
	custom := &http.Client{Transport: transport}
	custom.Get(url)
}
"#;

const JAVA_UNBOUNDED: &str = r#"import java.net.http.HttpClient;

class Client {
    HttpClient plain() {
        return HttpClient.newHttpClient();
    }

    HttpClient built() {
        return HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();
    }
}
"#;

const JAVA_BOUNDED: &str = r#"import java.net.http.HttpClient;
import java.time.Duration;

class Client {
    HttpClient built() {
        return HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(5)).build();
    }

    HttpClient plain() {
        return HttpClient.newHttpClient();
    }

    okhttp3.OkHttpClient okhttp() {
        return new okhttp3.OkHttpClient();
    }
}
"#;

const JS_UNBOUNDED: &str = r#"import axios from 'axios';
const https = require('node:https');

export async function fetchAll(url, body) {
  await axios.get(url);
  await axios.post(url, body, { headers: {} });
  await axios({ url, method: 'get' });
  const api = axios.create({ baseURL: url });
  https.get(`https://${url}/health`, (res) => res.resume());
  return api;
}
"#;

const JS_BOUNDED: &str = r#"import axios from 'axios';
const https = require('node:https');

export async function fetchAll(url, body, config) {
  await axios.get(url, { timeout: 5000 });
  await axios.post(url, body, config);
  await axios(config);
  await axios.get(url, { ...defaults });
  await fetch(url);
  https.get({ host: url, timeout: 5000 });
  return config;
}

export function guarded(url) {
  const request = https.request(url);
  request.setTimeout(5000);
  return request;
}
"#;

const RUST_UNBOUNDED: &str = r#"use reqwest::Client;

pub async fn fetch(url: &str) -> reqwest::Result<String> {
    let client = Client::new();
    let _built = reqwest::Client::builder().gzip(true).build()?;
    client.get(url).send().await?;
    reqwest::get(url).await?.text().await
}
"#;

/// A production file whose only client is in its inline test module.
const RUST_TEST_MODULE: &str = r#"pub fn serve() {}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn connects() {
        let client = reqwest::Client::new();
        client.get("http://127.0.0.1").send().await.unwrap();
    }
}
"#;

const RUST_BOUNDED: &str = r#"use std::time::Duration;

use reqwest::Client;

pub fn fetch(url: &str) -> reqwest::Result<String> {
    let _client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    let blocking = reqwest::blocking::Client::new();
    reqwest::blocking::get(url)?;
    blocking.get(url).send()?.text()
}
"#;

/// The APIs (with finding counts) each positive must report, and the
/// matching negative must report none.
#[test]
fn network_timeout_boundaries_report_exactly_the_unbounded_calls() {
    type Expected = (&'static str, &'static [(&'static str, usize)]);
    let expected: &[Expected] = &[
        (
            "python",
            &[
                ("requests.get", 2),
                ("requests.post", 1),
                ("httpx.get", 1),
                ("urllib.request.urlopen", 1),
            ],
        ),
        (
            "go",
            &[
                ("net/http.Get", 1),
                ("http.DefaultClient.Do", 1),
                ("http.Client{}", 1),
            ],
        ),
        (
            "java",
            &[
                ("HttpClient.newHttpClient", 1),
                ("HttpClient.newBuilder", 1),
            ],
        ),
        (
            "javascript",
            &[
                ("axios.get", 1),
                ("axios.post", 1),
                ("axios", 1),
                ("axios.create", 1),
                ("https.get", 1),
            ],
        ),
        (
            "typescript",
            &[
                ("axios.get", 1),
                ("axios.post", 1),
                ("axios", 1),
                ("axios.create", 1),
                ("https.get", 1),
            ],
        ),
        (
            "rust",
            &[
                ("reqwest::Client::new", 1),
                ("reqwest::Client::builder", 1),
                ("reqwest::get", 1),
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (language, apis) in expected {
        for positive in [true, false] {
            let files = network_timeout_capability_files(language, positive).unwrap();
            let mut case = CapabilityCase::new(
                "network_call_without_timeout",
                ScanProfile::Full,
                "",
                "",
                "",
                "",
            );
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let mut reported = BTreeMap::<String, usize>::new();
            for finding in analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == "network_call_without_timeout")
            {
                let api = finding
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                *reported.entry(api).or_default() += 1;
            }
            let wanted = if positive {
                apis.iter()
                    .map(|(api, count)| (api.to_string(), *count))
                    .collect()
            } else {
                BTreeMap::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
