//! `pass_through_wrapper` capability boundaries for each Tier-1 language.
//! Positives are private functions that pass every parameter on unchanged
//! to another function, plainly, spread, through a module, or awaited.
//! Negatives hold the nearest forms that add something or have a role: a
//! public wrapper, an added, changed, or reordered argument, a data
//! object's method, a constructor, no parameters, a decorator or
//! annotation, a method, a trait implementation, a platform shim, a
//! documented alias or test seam, an overload, type arguments, a type
//! predicate, and recursion.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "pass_through_wrapper";

pub(crate) fn pass_through_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    if detector != DETECTOR {
        return None;
    }
    let files: Vec<(&str, &str)> = match (language, positive) {
        ("rust", true) => vec![("src/config.rs", RUST_WRAPPERS)],
        ("rust", false) => vec![("src/config.rs", RUST_PURPOSEFUL)],
        ("python", true) => vec![("src/jobs.py", PYTHON_WRAPPERS)],
        ("python", false) => vec![("src/jobs.py", PYTHON_PURPOSEFUL)],
        ("javascript", true) => vec![("src/util.js", JS_WRAPPERS)],
        ("javascript", false) => vec![("src/util.js", JS_PURPOSEFUL)],
        ("typescript", true) => vec![("src/util.ts", TS_WRAPPERS)],
        ("typescript", false) => vec![("src/util.ts", TS_PURPOSEFUL)],
        ("go", true) => vec![("util.go", GO_WRAPPERS)],
        ("go", false) => vec![("util.go", GO_PURPOSEFUL), ("open_unix.go", GO_PLATFORM)],
        ("java", true) => vec![("src/main/java/Text.java", JAVA_WRAPPERS)],
        ("java", false) => vec![("src/main/java/Text.java", JAVA_PURPOSEFUL)],
        _ => return None,
    };
    Some(
        files
            .into_iter()
            .map(|(path, source)| (path.to_string(), source.to_string()))
            .collect(),
    )
}

const RUST_WRAPPERS: &str = r#"use std::path::Path;

fn load(path: &Path) -> Config {
    loaders::load(path)
}

fn parse_all(input: &str, strict: bool) -> Vec<Item> {
    parse_items(input, strict)
}

pub(crate) async fn fetch(url: &str) -> Body {
    http::get(url).await
}
"#;

const RUST_PURPOSEFUL: &str = r#"use std::path::Path;

pub fn load(path: &Path) -> Config {
    loaders::load(path)
}

fn parse_strict(input: &str) -> Vec<Item> {
    parse_items(input, true)
}

fn swap(a: u8, b: u8) -> u8 {
    combine(b, a)
}

fn wrap(bits: u32) -> Flags {
    Flags(bits)
}

fn decode(input: &str) -> Value {
    parse::<Value>(input)
}

fn now() -> Instant {
    Instant::now()
}

#[cfg(unix)]
fn open(path: &Path) -> Fd {
    sys::open(path)
}

/// Test seam: tests replace the clock.
fn sleep(duration: Duration) {
    clock::sleep(duration)
}

fn recurse(depth: u32) -> u32 {
    recurse(depth)
}

impl Loader for Disk {
    fn read(path: &Path) -> Config {
        loaders::load(path)
    }
}

impl Wrapper {
    fn len(&self, extra: usize) -> usize {
        measure(extra)
    }
}
"#;

const PYTHON_WRAPPERS: &str = r#"import loaders


def _load(name):
    return loaders.load(name)


def _run(job, *args, **kwargs):
    """Run the job."""
    return execute(job, *args, **kwargs)


async def _fetch(url, *, timeout):
    return await client_get(url, timeout=timeout)
"#;

const PYTHON_PURPOSEFUL: &str = r#"import functools
import hashlib

import loaders


def load(name):
    return loaders.load(name)


def _load_strict(name):
    return loaders.load(name, strict=True)


def _matches(text):
    return PATTERN.match(text)


def _make(value):
    return Widget(value)


@functools.cache
def _cached(name):
    return loaders.load(name)


def _now():
    return loaders.now()


class Repo:
    def _get(self, key):
        return lookup(key)


def _lazy_sha1(data):
    """Look the hash up at runtime: FIPS builds may lack it."""
    return hashlib.sha1(data)


try:
    from fast import parse as _parse
except ImportError:
    def _parse(text):
        return slow_parse(text)


def _first(items):
    return items.pop(0)
"#;

const JS_WRAPPERS: &str = r#"import * as path from "node:path";
import { readFile } from "node:fs/promises";

function join(a, b) {
  return path.join(a, b);
}

const load = async (file) => await readFile(file);

function log(...args) {
  console.log(...args);
}

const isList = (value) => Array.isArray(value);
"#;

const JS_PURPOSEFUL: &str = r#"import { readFile } from "node:fs/promises";

export function load(file) {
  return readFile(file);
}

function loadUtf8(file) {
  return readFile(file, "utf8");
}

const isWord = (text) => WORD.test(text);

function matches(text) {
  return pattern.test(text);
}

function make(value) {
  return new Widget(value);
}

function now() {
  return Date.now();
}

class Repo {
  get(key) {
    return lookup(key);
  }
}

function size(list) {
  return list.count();
}

/** @deprecated Use readFile. */
function read(file) {
  return readFile(file);
}

function stat(file) {
  return fsStat(file);
}

function exists(file) {
  return fsExists(file);
}

module.exports = { stat, check: exists };
"#;

const TS_WRAPPERS: &str = r#"import * as path from "node:path";
import { readFile } from "node:fs/promises";

function join(a: string, b: string): string {
  return path.join(a, b);
}

const load = async (file: string) => await readFile(file);

function log(...args: unknown[]): void {
  console.log(...args);
}

const isList = (value: unknown) => Array.isArray(value);
"#;

const TS_PURPOSEFUL: &str = r#"import { readFile } from "node:fs/promises";

export function load(file: string) {
  return readFile(file);
}

function loadUtf8(file: string) {
  return readFile(file, "utf8");
}

const isWord = (text: string) => WORD.test(text);

const isText = (value: unknown): value is string => isString(value);

function decode(text: string): Config {
  return parse<Config>(text);
}

function now(): number {
  return Date.now();
}

class Repo {
  get(key: string) {
    return lookup(key);
  }
}

/** @deprecated Use readFile. */
function read(file: string) {
  return readFile(file);
}
"#;

const GO_WRAPPERS: &str = r#"package util

import (
	"os"
	str "strings"
)

func readConfig(path string) ([]byte, error) {
	return os.ReadFile(path)
}

func joinAll(sep string, parts ...string) string {
	return joinParts(sep, parts...)
}

func lower(s string) string { return str.ToLower(s) }
"#;

const GO_PURPOSEFUL: &str = r#"package util

import (
	"os"
	"time"
)

func ReadConfig(path string) ([]byte, error) {
	return os.ReadFile(path)
}

func readLines(path string) ([]string, error) {
	return readAll(path, "\n")
}

func matches(s string) bool {
	return pattern.MatchString(s)
}

func (c *Client) get(key string) string {
	return lookup(key)
}

func now() time.Time {
	return time.Now()
}

// Deprecated: use os.ReadFile.
func readFile(path string) ([]byte, error) {
	return os.ReadFile(path)
}
"#;

const GO_PLATFORM: &str = r#"//go:build unix

package util

func open(path string) (int, error) {
	return unixOpen(path)
}
"#;

const JAVA_WRAPPERS: &str = r#"import java.util.Objects;

final class Text {
    private static boolean same(Object a, Object b) {
        return Objects.equals(a, b);
    }

    static String normal(String name) {
        return normalize(name);
    }

    private static String joinAll(String... parts) {
        return Joiner.joinAll(parts);
    }
}
"#;

const JAVA_PURPOSEFUL: &str = r#"import java.util.Objects;

final class Text {
    public static boolean same(Object a, Object b) {
        return Objects.equals(a, b);
    }

    private static String trimmed(String s) {
        return Strings.trim(s, ' ');
    }

    private static Object lookup(String key) {
        return CACHE.get(key);
    }

    private static boolean isWord(String s) {
        return WORD.matcher(s).matches();
    }

    private String name(String key) {
        return lookupName(key);
    }

    @VisibleForTesting
    static String hash(String s) {
        return Hashing.sha(s);
    }

    private static String pad(String s) {
        return pad(s, 2);
    }

    private static String pad(String s, int width) {
        return Strings.pad(s, width);
    }

    /** Kept for binary compatibility. */
    static String old(String s) {
        return Strings.fresh(s);
    }

    private static <T> T cast(Object o) {
        return Types.<T>cast(o);
    }
}
"#;

/// Each positive reports exactly these wrappers, and its negative none.
#[test]
fn pass_through_boundaries_report_exactly_the_private_wrappers() {
    let js = ["join", "load", "log", "isList"];
    let expected: &[(&str, &[&str])] = &[
        ("rust", &["load", "parse_all", "fetch"]),
        ("python", &["_load", "_run", "_fetch"]),
        ("javascript", &js),
        ("typescript", &js),
        ("go", &["readConfig", "joinAll", "lower"]),
        ("java", &["same", "normal", "joinAll"]),
    ];
    let mut failures = Vec::new();
    for (language, names) in expected {
        for positive in [true, false] {
            let files = pass_through_capability_files(DETECTOR, language, positive).unwrap();
            let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let reported = analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == DETECTOR)
                .filter_map(|finding| finding.identity.get("function").cloned())
                .collect::<BTreeSet<_>>();
            let wanted = if positive {
                names.iter().map(|name| name.to_string()).collect()
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
