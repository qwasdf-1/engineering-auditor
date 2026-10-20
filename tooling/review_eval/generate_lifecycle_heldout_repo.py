#!/usr/bin/env python3
"""Generate one held-out lifecycle-review repository for offline prompt improvement.

These fixtures are never used to draft or revise a candidate; they are evaluated
only after a candidate is frozen on its training cases.
"""

from __future__ import annotations

import generate_lifecycle_repo as lifecycle


HELD_OUT_SOURCES = {
    "visible_read_only_helper": """
pub fn median_each_round(values: &mut Vec<i32>, rounds: usize) -> Vec<i32> {
    let mut medians = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        values.sort_unstable();
        medians.push(middle(values));
    }
    medians
}

fn middle(values: &[i32]) -> i32 {
    values[values.len() / 2]
}
""",
    "separate_output_write": """
pub fn record_rounds(values: &mut Vec<i32>, ledger: &mut Vec<i64>, rounds: usize) {
    for round in 0..rounds {
        values.sort();
        append_round(ledger, round, values[0]);
    }
}

fn append_round(ledger: &mut Vec<i64>, round: usize, smallest: i32) {
    ledger.push(round as i64 * 1_000 + i64::from(smallest));
}
""",
    "visible_replacement_mutation": """
pub fn replace_and_rank(values: &mut Vec<i32>, incoming: &[i32]) -> Vec<i32> {
    let mut minimums = Vec::with_capacity(incoming.len());
    for value in incoming {
        values.sort();
        minimums.push(values[0]);
        overwrite_smallest(values, *value);
    }
    minimums
}

fn overwrite_smallest(values: &mut [i32], replacement: i32) {
    if let Some(first) = values.first_mut() {
        *first = replacement;
    }
}
""",
    "opaque_trait_refresh": """
/// Implementations are supplied by downstream crates and none is visible here. The trait
/// documents no guarantee about whether `refresh` changes, preserves, or reorders `values`.
pub trait Refresh {
    fn refresh(&mut self, values: &mut Vec<i32>);
}

pub fn rank_after_refresh(
    values: &mut Vec<i32>,
    source: &mut dyn Refresh,
    rounds: usize,
) -> Vec<Option<i32>> {
    let mut smallest = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        source.refresh(values);
        values.sort();
        smallest.push(values.first().copied());
    }
    smallest
}
""",
}


if __name__ == "__main__":
    lifecycle.SOURCES = HELD_OUT_SOURCES
    raise SystemExit(lifecycle.main())
