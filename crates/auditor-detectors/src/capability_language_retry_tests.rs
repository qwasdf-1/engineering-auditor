//! `retry_without_backoff` capability boundaries for each Tier-1 language.
//! Positives retry one operation with no wait: a counted loop, and an open
//! loop that counts its attempts. Negatives hold the nearest forms that wait
//! or do not retry: a sleep, a same-file helper that waits, a loop trying a
//! different target each pass, a handler that gives up, an open loop that
//! reads rather than retries, and immediate retries in test code.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "retry_without_backoff";

pub(crate) fn retry_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let files: Vec<(&str, &str)> = match (language, positive) {
        ("python", true) => vec![("src/client.py", PYTHON_IMMEDIATE)],
        ("python", false) => vec![
            ("src/client.py", PYTHON_SAFE),
            ("tests/test_client.py", PYTHON_IMMEDIATE),
        ],
        ("javascript", true) => vec![("src/client.js", JS_IMMEDIATE)],
        ("javascript", false) => vec![
            ("src/client.js", JS_SAFE),
            ("src/client.test.js", JS_IMMEDIATE),
        ],
        ("typescript", true) => vec![("src/client.ts", JS_IMMEDIATE)],
        ("typescript", false) => vec![
            ("src/client.ts", JS_SAFE),
            ("src/client.test.ts", JS_IMMEDIATE),
        ],
        ("rust", true) => vec![("src/client.rs", RUST_IMMEDIATE)],
        ("rust", false) => vec![("src/client.rs", RUST_SAFE)],
        ("go", true) => vec![("client.go", GO_IMMEDIATE)],
        ("go", false) => vec![("client.go", GO_SAFE), ("client_test.go", GO_IMMEDIATE)],
        ("java", true) => vec![("src/main/java/Client.java", JAVA_IMMEDIATE)],
        ("java", false) => vec![
            ("src/main/java/Client.java", JAVA_SAFE),
            ("src/test/java/ClientTest.java", JAVA_IMMEDIATE),
        ],
        _ => return None,
    };
    Some(
        files
            .into_iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

const PYTHON_IMMEDIATE: &str = r#"import requests


def fetch(url):
    for _ in range(3):
        try:
            return requests.get(url, timeout=5)
        except requests.ConnectionError:
            pass
    raise RuntimeError(url)


def publish(queue, message, max_retries):
    attempts = 0
    while attempts < max_retries:
        attempts += 1
        try:
            queue.send(message)
            break
        except ConnectionError as error:
            last = error
"#;

const PYTHON_SAFE: &str = r#"import select
import time

import requests


def fetch(url):
    for attempt in range(3):
        try:
            return requests.get(url, timeout=5)
        except requests.ConnectionError:
            time.sleep(2 ** attempt)


def wait_readable(sock, timeout):
    return select.select([sock], [], [], timeout)


def receive(sock, retries):
    while retries:
        retries -= 1
        try:
            return sock.recv(1024)
        except OSError:
            wait_readable(sock, 0.5)


def first_reachable(urls):
    for i in range(len(urls)):
        url = urls[i]
        try:
            return requests.get(url, timeout=5)
        except requests.ConnectionError:
            pass


def strict(url):
    for _ in range(3):
        try:
            return requests.get(url, timeout=5)
        except requests.ConnectionError:
            raise


def read_all(stream):
    while True:
        try:
            chunk = stream.read()
            return chunk
        except BlockingIOError:
            continue
"#;

const JS_IMMEDIATE: &str = r#"async function load(url) {
  for (let i = 0; i < 3; i++) {
    try {
      return await fetch(url);
    } catch (error) {
      console.warn(error);
    }
  }
}

async function publish(queue, message, maxRetries) {
  let attempt = 0;
  while (attempt < maxRetries) {
    attempt++;
    try {
      await queue.send(message);
      return;
    } catch (error) {
      lastError = error;
    }
  }
}
"#;

const JS_SAFE: &str = r#"async function load(url) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      return await fetch(url);
    } catch (error) {
      await new Promise((resolve) => setTimeout(resolve, 100 * 2 ** attempt));
    }
  }
}

async function firstReachable(urls) {
  for (let i = 0; i < urls.length; i++) {
    const url = urls[i];
    try {
      return await fetch(url);
    } catch (error) {
      console.warn(error);
    }
  }
}

async function strict(url) {
  for (let i = 0; i < 3; i++) {
    try {
      return await fetch(url);
    } catch (error) {
      throw error;
    }
  }
}
"#;

const RUST_IMMEDIATE: &str = r#"pub fn get(url: &str) -> Result<String, Error> {
    let mut last = None;
    for _ in 0..3 {
        match fetch(url) {
            Ok(body) => return Ok(body),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap())
}

pub fn connect(address: &str, max_attempts: u32) -> Connection {
    let mut attempts = 0;
    loop {
        attempts += 1;
        if let Ok(connection) = open(address) {
            break connection;
        }
    }
}
"#;

const RUST_SAFE: &str = r#"use std::thread;
use std::time::Duration;

pub fn get(url: &str) -> Result<String, Error> {
    for attempt in 0..3 {
        match fetch(url) {
            Ok(body) => return Ok(body),
            Err(_) => thread::sleep(Duration::from_millis(100 << attempt)),
        }
    }
    Err(Error::Exhausted)
}

pub fn next_frame(stream: &mut Stream) -> Option<Frame> {
    loop {
        match stream.poll() {
            Some(frame) => return Some(frame),
            None => {}
        }
    }
}

pub fn strict(url: &str) -> Result<String, Error> {
    for _ in 0..3 {
        match fetch(url) {
            Ok(body) => return Ok(body),
            Err(error) => return Err(error),
        }
    }
    Err(Error::Exhausted)
}

#[cfg(test)]
mod tests {
    #[test]
    fn retries() {
        for _ in 0..3 {
            match super::fetch("x") {
                Ok(_) => return,
                Err(_) => {}
            }
        }
    }
}
"#;

const GO_IMMEDIATE: &str = r#"package client

func Get(url string) (*http.Response, error) {
	var resp *http.Response
	var err error
	for i := 0; i < 3; i++ {
		resp, err = client.Get(url)
		if err == nil {
			break
		}
	}
	return resp, err
}

func Publish(message string, maxRetries int) error {
	attempts := 0
	for attempts < maxRetries {
		attempts++
		err := queue.Send(message)
		if err != nil {
			continue
		}
		return nil
	}
	return errExhausted
}
"#;

const GO_SAFE: &str = r#"package client

import "time"

func Get(url string) (*http.Response, error) {
	var resp *http.Response
	var err error
	for attempt := 0; attempt < 3; attempt++ {
		resp, err = client.Get(url)
		if err == nil {
			break
		}
		time.Sleep(time.Duration(attempt) * time.Second)
	}
	return resp, err
}

func Publish(ctx context.Context, message string, maxRetries int) error {
	for attempts := 0; attempts < maxRetries; attempts++ {
		err := queue.Send(message)
		if err == nil {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(time.Second):
		}
	}
	return errExhausted
}

func FirstReachable(urls []string) (*http.Response, error) {
	for i := 0; i < len(urls); i++ {
		url := urls[i]
		resp, err := client.Get(url)
		if err == nil {
			return resp, nil
		}
	}
	return nil, errExhausted
}
"#;

const JAVA_IMMEDIATE: &str = r#"class Client {
    String get(String url) throws Exception {
        for (int attempt = 0; attempt < 3; attempt++) {
            try {
                return http.send(url);
            } catch (IOException e) {
                log.warn("retry", e);
            }
        }
        throw new IllegalStateException();
    }

    void publish(String message, int maxRetries) {
        int retries = 0;
        do {
            try {
                queue.send(message);
                return;
            } catch (IOException e) {
                retries++;
            }
        } while (retries < maxRetries);
    }
}
"#;

const JAVA_SAFE: &str = r#"class Client {
    String get(String url) throws Exception {
        for (int attempt = 0; attempt < 3; attempt++) {
            try {
                return http.send(url);
            } catch (IOException e) {
                Thread.sleep(100L << attempt);
            }
        }
        throw new IllegalStateException();
    }

    String firstReachable(String[] urls) throws Exception {
        for (int i = 0; i < urls.length; i++) {
            String url = urls[i];
            try {
                return http.send(url);
            } catch (IOException e) {
                log.warn("next", e);
            }
        }
        throw new IllegalStateException();
    }

    String strict(String url) throws Exception {
        for (int attempt = 0; attempt < 3; attempt++) {
            try {
                return http.send(url);
            } catch (IOException e) {
                throw e;
            }
        }
        throw new IllegalStateException();
    }
}
"#;

#[test]
fn retry_boundaries_report_exactly_the_immediate_retries() {
    let expected: &[(&str, &[&str])] = &[
        (
            "python",
            &[
                "return requests.get(url, timeout=5)",
                "queue.send(message) break",
            ],
        ),
        (
            "javascript",
            &[
                "{ return await fetch(url); }",
                "{ await queue.send(message); return; }",
            ],
        ),
        (
            "typescript",
            &[
                "{ return await fetch(url); }",
                "{ await queue.send(message); return; }",
            ],
        ),
        ("rust", &["fetch(url)", "open(address)"]),
        ("go", &["client.Get(url)", "queue.Send(message)"]),
        (
            "java",
            &[
                "{ return http.send(url); }",
                "{ queue.send(message); return; }",
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (language, operations) in expected {
        for positive in [true, false] {
            let files = retry_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
                .filter_map(|finding| finding.identity.get("operation").cloned())
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                operations
                    .iter()
                    .map(|operation| operation.to_string())
                    .collect()
            } else {
                BTreeSet::new()
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
