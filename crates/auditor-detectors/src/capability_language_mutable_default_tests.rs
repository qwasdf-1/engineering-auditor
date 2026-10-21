//! `mutable_default_argument` capability boundary (Python only). The
//! positive holds every way a shared default leaves its call; the negative
//! holds the nearest safe forms: a `None` default, a default only read, a
//! default replaced before it is changed, a memo table, and immutable
//! defaults.

use std::collections::BTreeSet;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

const DETECTOR: &str = "mutable_default_argument";

pub(crate) fn mutable_default_capability_files(positive: bool) -> Vec<(String, String)> {
    let source = if positive { SHARED } else { SAFE };
    vec![("src/registry.py".to_string(), source.to_string())]
}

const SHARED: &str = r#"from collections import defaultdict


def register(name, names=[]):
    names.append(name)
    return len(names)


def tag(key, value, tags={}):
    tags[key] = value


def collect(value: int, values: list[int] = []) -> list[int]:
    values.append(value)
    return values


def merge(extra, merged=[]):
    merged += extra
    return len(merged)


class Report:
    def __init__(self, rows=[]):
        self.rows = rows


def options(overrides={}):
    return overrides


def pair(left=[], right={}):
    left.append(1)
    right["k"] = 1


def walk(node, seen=set()):
    seen.add(node.id)
    for child in node.children:
        walk(child, seen)


def group(pairs, groups=defaultdict(list)):
    for key, value in pairs:
        groups[key].append(value)
    del groups["skip"]
"#;

const SAFE: &str = r#"def register(name, names=None):
    if names is None:
        names = []
    names.append(name)
    return len(names)


def count(items=[]):
    return sum(1 for item in items if item)


def merge(extra, merged=[]):
    merged = merged or []
    merged.append(extra)
    return merged


def copy(rows=[]):
    rows = list(rows)
    rows.append(0)
    return rows


def fib(n, _cache={}):
    if n not in _cache:
        _cache[n] = n if n < 2 else fib(n - 1) + fib(n - 2)
    return _cache[n]


def lookup(key, memo={}):
    memo[key] = key
    return memo


def flags(names=(), limit=10, label="x"):
    return [name for name in names][:limit]


def build(sizes=[]):
    total = [size for size in sizes]
    total.append(1)
    return total
"#;

/// The positive reports exactly these parameters, once each; the negative
/// none.
#[test]
fn mutable_default_boundary_reports_exactly_the_shared_defaults() {
    let wanted: BTreeSet<&str> = [
        "names",
        "tags",
        "values",
        "merged",
        "rows",
        "overrides",
        "left",
        "right",
        "seen",
        "groups",
    ]
    .into_iter()
    .collect();
    for positive in [true, false] {
        let files = mutable_default_capability_files(positive);
        let mut case = CapabilityCase::new(DETECTOR, ScanProfile::Full, "", "", "", "");
        case.positive = files.clone();
        case.negative = files;
        let analysis = analyze_case(&case, true, ScanProfile::Full);
        let reported = analysis
            .findings
            .iter()
            .filter(|finding| finding.detector == DETECTOR)
            .map(|finding| {
                finding
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let expected = if positive {
            wanted.clone()
        } else {
            BTreeSet::new()
        };
        let unique = reported.iter().map(String::as_str).collect::<BTreeSet<_>>();
        assert_eq!(unique, expected, "positive={positive}: {reported:?}");
        assert_eq!(unique.len(), reported.len(), "duplicates: {reported:?}");
    }
}
