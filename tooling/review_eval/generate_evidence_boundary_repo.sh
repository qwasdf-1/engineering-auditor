#!/usr/bin/env sh
set -eu

if [ "$#" -ne 1 ]; then
  printf '%s\n' 'usage: generate_evidence_boundary_repo.sh <output-directory>' >&2
  exit 2
fi

root=$1
mkdir -p "$root/src"
printf '%s\n' '[package]' 'name = "evidence-boundary-case"' 'version = "0.1.0"' 'edition = "2021"' > "$root/Cargo.toml"
printf '%s\n' \
  'use std::process::Command;' \
  '' \
  'pub fn run_operator_input(value: &str) -> bool {' \
  '    Command::new("sh").arg("-c").arg(value).status().is_ok()' \
  '}' > "$root/src/lib.rs"
printf '%s\n' \
  '# Evidence Boundary Case' \
  '' \
  'This document claims the command input is validated. The implementation is authoritative.' > "$root/README.md"
