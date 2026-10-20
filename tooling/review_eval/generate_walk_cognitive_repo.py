#!/usr/bin/env python3
"""Generate the public multi-finding `walk_cognitive` shadow-review fixture."""

from __future__ import annotations

import sys
from pathlib import Path


SOURCE = r'''
#[derive(Clone, Copy)]
pub enum NodeKind {
    Branch,
    Loop,
    Call,
    Value,
}

pub struct Node {
    pub kind: NodeKind,
    pub weight: usize,
    pub children: Vec<Node>,
}

pub fn walk_cognitive(
    node: &Node,
    function_name: &str,
    score: &mut usize,
    nesting: usize,
    branch_count: &mut usize,
    operator_volume: &mut usize,
    include_nested_functions: bool,
) {
    let is_named_root = !function_name.is_empty() && nesting == 0;
    if include_nested_functions {
        if node.weight > 0 {
            if !node.children.is_empty() {
                if nesting < node.children.len() + 8 {
                    if function_name.starts_with("walk") {
                        if branch_count.saturating_add(*score) < usize::MAX {
                            *operator_volume += 1;
                        }
                    }
                }
            }
        }
    }
__HALSTEAD_VOLUME__
    if is_named_root {
        *operator_volume += function_name.len() + node.weight;
    }
    match node.kind {
        NodeKind::Branch => {
            *branch_count += 1;
            *score += 1 + nesting;
            if node.weight > 3 {
                *operator_volume += node.weight * 2 + nesting;
            } else if node.weight == 0 {
                *operator_volume += 1;
            } else {
                *operator_volume += node.weight + 1;
            }
        }
        NodeKind::Loop => {
            *branch_count += 1;
            *score += 1 + nesting;
            if node.children.is_empty() {
                *operator_volume += node.weight + nesting + 1;
            }
        }
        NodeKind::Call => {
            if include_nested_functions && node.weight > nesting {
                *score += 1;
                *operator_volume += node.weight + function_name.len();
            }
        }
        NodeKind::Value => {
            if node.weight > 8 && include_nested_functions {
                *operator_volume += node.weight - nesting.min(node.weight);
            }
        }
    }
    for child in &node.children {
        let child_nesting = match node.kind {
            NodeKind::Branch | NodeKind::Loop => nesting + 1,
            NodeKind::Call if include_nested_functions => nesting + 1,
            _ => nesting,
        };
        walk_cognitive(
            child,
            function_name,
            score,
            child_nesting,
            branch_count,
            operator_volume,
            include_nested_functions,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_counts_one_recursive_owner() {
        let tree = Node {
            kind: NodeKind::Branch,
            weight: 5,
            children: vec![Node {
                kind: NodeKind::Loop,
                weight: 2,
                children: vec![Node {
                    kind: NodeKind::Value,
                    weight: 13,
                    children: vec![],
                }],
            }],
        };
        let (mut score, mut branches, mut volume) = (0, 0, 0);
        walk_cognitive(&tree, "walk_cognitive", &mut score, 0, &mut branches, &mut volume, true);
        assert!(score >= 3);
        assert_eq!(branches, 2);
        assert!(volume > 0);
    }
}
'''


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} <output-directory>", file=sys.stderr)
        return 2
    root = Path(sys.argv[1])
    if root.exists():
        print(f"refusing to overwrite existing output: {root}", file=sys.stderr)
        return 2
    root.joinpath("src").mkdir(parents=True)
    root.joinpath("Cargo.toml").write_text(
        '[package]\nname = "walk-cognitive-review-fixture"\nversion = "0.1.0"\nedition = "2021"\n'
    )
    halstead_volume = "\n".join(
        f"    let metric_{index} = node.weight.wrapping_add({index}).wrapping_mul(nesting + 1);"
        for index in range(120)
    )
    halstead_volume += "\n    *operator_volume = operator_volume.wrapping_add(metric_119);"
    root.joinpath("src/lib.rs").write_text(
        SOURCE.replace("__HALSTEAD_VOLUME__", halstead_volume).lstrip()
    )
    root.joinpath("README.md").write_text(
        "# Generated walk_cognitive review fixture\n\n"
        "This clean-room fixture intentionally places cognitive complexity, branch count, "
        "Halstead volume, and parameter-count pressure on one recursive traversal owner.\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
