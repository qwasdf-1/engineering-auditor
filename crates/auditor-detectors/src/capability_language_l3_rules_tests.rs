//! Boundaries for the Layer 3 rules `quadratic_front_removal` and
//! `catastrophic_backtracking_regex` in each language that applies.
//! Positives hold only sites the detector must report; negatives hold the
//! nearest constant-time or unambiguous forms.

use std::collections::BTreeMap;

use crate::ScanProfile;

use super::super::{analyze_case, CapabilityCase};

pub(crate) fn l3_rule_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let (path, source) = match (detector, language, positive) {
        ("quadratic_front_removal", "python", true) => ("src/walk.py", PY_FRONT),
        ("quadratic_front_removal", "python", false) => ("src/walk.py", PY_FRONT_SAFE),
        ("quadratic_front_removal", "javascript", true) => ("src/walk.js", JS_FRONT),
        ("quadratic_front_removal", "javascript", false) => ("src/walk.js", JS_FRONT_SAFE),
        ("quadratic_front_removal", "typescript", true) => ("src/walk.ts", JS_FRONT),
        ("quadratic_front_removal", "typescript", false) => ("src/walk.ts", JS_FRONT_SAFE),
        ("quadratic_front_removal", "java", true) => ("src/main/java/Walk.java", JAVA_FRONT),
        ("quadratic_front_removal", "java", false) => ("src/main/java/Walk.java", JAVA_FRONT_SAFE),
        ("quadratic_front_removal", "rust", true) => ("src/walk.rs", RUST_FRONT),
        ("quadratic_front_removal", "rust", false) => ("src/walk.rs", RUST_FRONT_SAFE),
        ("catastrophic_backtracking_regex", "python", true) => ("src/validate.py", PY_REGEX),
        ("catastrophic_backtracking_regex", "python", false) => ("src/validate.py", PY_REGEX_SAFE),
        ("catastrophic_backtracking_regex", "javascript", true) => ("src/validate.js", JS_REGEX),
        ("catastrophic_backtracking_regex", "javascript", false) => {
            ("src/validate.js", JS_REGEX_SAFE)
        }
        ("catastrophic_backtracking_regex", "typescript", true) => ("src/validate.ts", JS_REGEX),
        ("catastrophic_backtracking_regex", "typescript", false) => {
            ("src/validate.ts", JS_REGEX_SAFE)
        }
        ("catastrophic_backtracking_regex", "java", true) => {
            ("src/main/java/Validate.java", JAVA_REGEX)
        }
        ("catastrophic_backtracking_regex", "java", false) => {
            ("src/main/java/Validate.java", JAVA_REGEX_SAFE)
        }
        _ => return None,
    };
    Some(vec![(path.to_string(), source.to_string())])
}

const PY_FRONT: &str = r#"def drain(start):
    queue = [start]
    order = []
    while queue:
        node = queue.pop(0)
        order.append(node)
        queue.extend(children(node))
    return order


def consume(items: list[str]):
    for item in list(items):
        items.pop(0)
        handle(item)
"#;

const PY_FRONT_SAFE: &str = r#"from collections import deque


def drain(start):
    queue = deque([start])
    while queue:
        node = queue.popleft()
        queue.extend(children(node))


def last(items: list):
    while items:
        items.pop()


def head(items: list):
    first = items.pop(0)
    return first


def three(items: list):
    for _ in range(3):
        items.pop(0)


def by_key(table: dict):
    while table:
        table.pop(0)


class Walker:
    def drain(self):
        while self.queue:
            self.queue.pop(0)
"#;

const JS_FRONT: &str = r#"export function drain(start) {
  const queue = [start];
  const order = [];
  while (queue.length > 0) {
    const node = queue.shift();
    order.push(node);
    queue.push(...children(node));
  }
  return order;
}
"#;

const JS_FRONT_SAFE: &str = r#"export function drain(queue) {
  while (queue.length > 0) {
    visit(queue.shift());
  }
}

export function last(start) {
  const stack = [start];
  while (stack.length > 0) {
    visit(stack.pop());
  }
}

export function first(start) {
  const items = [start];
  return items.shift();
}
"#;

const JAVA_FRONT: &str = r#"import java.util.ArrayList;
import java.util.List;

class Walk {
    List<Node> drain(Node start) {
        List<Node> queue = new ArrayList<>();
        queue.add(start);
        List<Node> order = new ArrayList<>();
        while (!queue.isEmpty()) {
            Node node = queue.remove(0);
            order.add(node);
            queue.addAll(node.children());
        }
        return order;
    }
}
"#;

const JAVA_FRONT_SAFE: &str = r#"import java.util.ArrayDeque;
import java.util.LinkedList;
import java.util.List;

class Walk {
    void linked(Node start) {
        LinkedList<Node> queue = new LinkedList<>();
        queue.add(start);
        while (!queue.isEmpty()) {
            visit(queue.remove(0));
        }
    }

    void deque(Node start) {
        ArrayDeque<Node> queue = new ArrayDeque<>();
        queue.add(start);
        while (!queue.isEmpty()) {
            visit(queue.poll());
        }
    }

    void unknown(List<Node> queue) {
        while (!queue.isEmpty()) {
            visit(queue.remove(0));
        }
    }
}
"#;

const RUST_FRONT: &str = r#"pub fn drain(start: u32) -> Vec<u32> {
    let mut queue = vec![start];
    let mut order = Vec::new();
    while !queue.is_empty() {
        let node = queue.remove(0);
        order.push(node);
        queue.extend(children(node));
    }
    order
}
"#;

const RUST_FRONT_SAFE: &str = r#"use std::collections::VecDeque;

pub fn drain(start: u32) {
    let mut queue = VecDeque::new();
    queue.push_back(start);
    while let Some(node) = queue.pop_front() {
        queue.extend(children(node));
    }
}

pub fn deque_remove(start: u32) {
    let mut queue: VecDeque<u32> = VecDeque::from(vec![start]);
    while !queue.is_empty() {
        queue.remove(0);
    }
}

pub fn last(start: u32) {
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        visit(node);
    }
}
"#;

const PY_REGEX: &str = r#"import re

EMAIL = re.compile(r"^([a-zA-Z0-9]+)*@example\.com$")
WORDS = re.compile("(\\w+\\s?)*$")


def strip_quotes(text):
    return re.sub(r'"(?:[^"\\]+|\\.)*"', "", text)
"#;

const PY_REGEX_SAFE: &str = r#"import re

SAFE = re.compile(r"^[a-z]+(?:,[a-z]+)*$")
VERSION = re.compile(r"(\.\d+)*")
LABEL = "(a+)+"
OTHER = custom.compile(r"(a+)+")


def build(suffix):
    return re.compile(r"(\w+)" + suffix)
"#;

const JS_REGEX: &str = r#"export const email = /^([a-zA-Z0-9]+)*@example\.com$/;
export const words = new RegExp('(\\w+\\s?)*$');

export function strip(text) {
  return text.replace(/"(?:[^"\\]+|\\.)*"/g, '');
}
"#;

const JS_REGEX_SAFE: &str = r#"export const safe = /^[a-z]+(?:,[a-z]+)*$/;
export const version = /(\.\d+)*/;
export const label = '(a+)+';

export function build(prefix) {
  return new RegExp(prefix + '(a+)+');
}
"#;

const JAVA_REGEX: &str = r#"import java.util.regex.Pattern;

class Validate {
    static final Pattern EMAIL = Pattern.compile("^([a-zA-Z0-9]+)*@example\\.com$");

    boolean words(String text) {
        return text.matches("(\\w+\\s?)*$");
    }

    String strip(String text) {
        return text.replaceAll("\"(?:[^\"\\\\]+|\\\\.)*\"", "");
    }
}
"#;

const JAVA_REGEX_SAFE: &str = r#"import java.util.regex.Pattern;

class Validate {
    static final Pattern SAFE = Pattern.compile("^[a-z]+(?:,[a-z]+)*$");
    static final Pattern POSSESSIVE = Pattern.compile("(a++)+");
    String label = "(a+)+";

    String[] parts(String text) {
        return text.split(",");
    }
}
"#;

/// A detector, a language, and what its positive reports (the backticked
/// subject of each message, with counts).
type Boundary = (&'static str, &'static str, &'static [(&'static str, usize)]);

#[test]
fn l3_rule_boundaries_report_exactly_the_quadratic_and_ambiguous_sites() {
    const REGEXES: &[(&str, usize)] = &[
        ("([a-zA-Z0-9]+)*", 1),
        ("(\\w+\\s?)*", 1),
        ("(?:[^\"\\\\]+|\\\\.)*", 1),
    ];
    let expected: &[Boundary] = &[
        (
            "quadratic_front_removal",
            "python",
            &[("drain", 1), ("consume", 1)],
        ),
        ("quadratic_front_removal", "javascript", &[("drain", 1)]),
        ("quadratic_front_removal", "typescript", &[("drain", 1)]),
        ("quadratic_front_removal", "java", &[("drain", 1)]),
        ("quadratic_front_removal", "rust", &[("drain", 1)]),
        ("catastrophic_backtracking_regex", "python", REGEXES),
        ("catastrophic_backtracking_regex", "javascript", REGEXES),
        ("catastrophic_backtracking_regex", "typescript", REGEXES),
        ("catastrophic_backtracking_regex", "java", REGEXES),
    ];
    let mut failures = Vec::new();
    for (detector, language, subjects) in expected {
        for positive in [true, false] {
            let files = l3_rule_capability_files(detector, language, positive).unwrap();
            let mut case = CapabilityCase::new(detector, ScanProfile::Full, "", "", "", "");
            case.positive = files.clone();
            case.negative = files;
            let analysis = analyze_case(&case, true, ScanProfile::Full);
            let mut reported = BTreeMap::<String, usize>::new();
            for finding in analysis
                .findings
                .iter()
                .filter(|finding| finding.detector == *detector)
            {
                let subject = finding
                    .message
                    .split('`')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                *reported.entry(subject).or_default() += 1;
            }
            let wanted = if positive {
                subjects
                    .iter()
                    .map(|(subject, count)| (subject.to_string(), *count))
                    .collect()
            } else {
                BTreeMap::new()
            };
            if reported != wanted {
                failures.push(format!(
                    "{detector} {language} {}: reported {reported:?}, expected {wanted:?}",
                    if positive { "positive" } else { "negative" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
