#!/usr/bin/env python3
"""Generate one deterministic lifecycle-review evaluation repository."""

from __future__ import annotations

import sys
from pathlib import Path


SOURCES = {
    "immutable_equivalent_input": """
pub fn normalize_repeatedly(values: &mut Vec<i32>) {
    for _ in 0..values.len() {
        values.sort();
    }
}
""",
    "intervening_mutation": """
pub fn insert_and_order(values: &mut Vec<i32>, additions: &[i32]) -> Vec<bool> {
    fn append_for_next_iteration(values: &mut Vec<i32>, addition: i32) {
        values.push(addition);
    }

    let mut previously_present = Vec::with_capacity(additions.len());
    for addition in additions {
        values.sort();
        previously_present.push(values.binary_search(addition).is_ok());
        append_for_next_iteration(values, *addition);
    }
    previously_present
}
""",
    "distinct_configurations": """
pub fn order_for_consumers(values: &mut Vec<i32>, replacements_by_consumer: &[i32]) -> Vec<Vec<i32>> {
    let mut ordered_snapshots = Vec::with_capacity(replacements_by_consumer.len());
    for replacement in replacements_by_consumer {
        // Contract: each consumer requires its replacement applied before ordering, and receives
        // the resulting snapshot. The replacement is observable consumer input, not scratch state.
        apply_consumer_configuration(values, *replacement);
        values.sort();
        ordered_snapshots.push(values.clone());
    }
    ordered_snapshots
}

fn apply_consumer_configuration(values: &mut [i32], replacement: i32) {
    if let Some(first) = values.first_mut() {
        *first = replacement;
    }
}
""",
    "test_setup_only": """
pub fn production_identity(value: i32) -> i32 {
    value
}

pub fn prepare_test_fixture(values: &mut Vec<i32>) {
    if !cfg!(test) {
        return;
    }
    for _ in 0..values.len() {
        values.sort();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prepares_independent_examples() {
        let mut values = vec![3, 1, 2];
        super::prepare_test_fixture(&mut values);
        assert_eq!(values, [1, 2, 3]);
    }
}
""",
    "freshness_constraint": """
use std::time::{SystemTime, UNIX_EPOCH};

pub fn publish_fresh_snapshots(
    values: &mut Vec<i32>,
    required_epochs: &[u64],
) -> Vec<(u64, Vec<i32>)> {
    fn refresh_for_epoch(values: &mut Vec<i32>, observed: u64) {
        values.push((observed % i32::MAX as u64) as i32);
    }

    let mut published_snapshots = Vec::with_capacity(required_epochs.len());
    for required_epoch in required_epochs {
        let observed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after epoch")
            .as_secs();
        assert!(observed >= *required_epoch);
        refresh_for_epoch(values, observed);
        values.sort();
        // Contract: every published snapshot must reflect the values refreshed for its own
        // observed epoch. Reusing an earlier sorted snapshot would omit the newly added value.
        published_snapshots.push((observed, values.clone()));
    }
    published_snapshots
}
""",
    "retention_risk": """
pub fn authorize_requests(request_count: usize) {
    for _ in 0..request_count {
        // Policy requires a new equivalent parser for each authorization and destruction
        // immediately afterward. Retaining one parser across iterations is forbidden.
        let mut secret_parser = SecretParser::new(b"allow:primary".to_vec());
        authorize(&secret_parser);
        secret_parser.authentication_pattern.fill(0);
    }
}

struct SecretParser {
    authentication_pattern: Vec<u8>,
}

impl SecretParser {
    fn new(authentication_pattern: Vec<u8>) -> Self {
        Self { authentication_pattern }
    }

}

fn authorize(_secret_parser: &SecretParser) {}
""",
    "missing_evidence": """
/// The callback bodies are supplied by callers and are unavailable here. `refresh` has no
/// behavioral guarantee: it may mutate `values`, leave them unchanged, or produce equivalent
/// contents across calls. `consume` has no declared per-execution freshness requirement.
pub fn process_unknown_source(
    values: &mut Vec<i32>,
    iterations: usize,
    mut refresh: impl FnMut(&mut Vec<i32>),
    mut consume: impl FnMut(&[i32]),
) {
    for _ in 0..iterations {
        refresh(values);
        values.sort();
        consume(values);
    }
}
""",
    "unsupported_numeric_benefit": """
pub fn normalize_repeatedly(values: &mut Vec<i32>) {
    for _ in 0..values.len() {
        // An unmeasured design note predicts a 70% latency reduction if this sort is hoisted;
        // no benchmark, trace, or runtime measurement in the repository supports that number.
        values.sort();
    }
}
""",
}


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[2] not in SOURCES:
        choices = ", ".join(sorted(SOURCES))
        print(
            f"usage: {Path(sys.argv[0]).name} <output-directory> <fixture-kind>; "
            f"fixture-kind is one of: {choices}",
            file=sys.stderr,
        )
        return 2
    root = Path(sys.argv[1])
    if root.exists():
        print(f"refusing to overwrite existing output: {root}", file=sys.stderr)
        return 2
    root.joinpath("src").mkdir(parents=True)
    fixture_kind = sys.argv[2]
    root.joinpath("Cargo.toml").write_text(
        "[package]\n"
        f'name = "lifecycle-eval-{fixture_kind.replace("_", "-")}"\n'
        'version = "0.1.0"\n'
        'edition = "2021"\n'
    )
    root.joinpath("src/lib.rs").write_text(SOURCES[fixture_kind].lstrip())
    root.joinpath("README.md").write_text(
        "# Generated lifecycle evaluation fixture\n\n"
        f"Fixture kind: `{fixture_kind}`.\n"
        "The source is generated clean-room input for the public Layer 6 evaluation corpus.\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
