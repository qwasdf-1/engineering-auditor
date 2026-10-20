#!/usr/bin/env sh
set -eu

if [ "$#" -ne 1 ]; then
  printf '%s\n' 'usage: generate_abstention_repo.sh <output-directory>' >&2
  exit 2
fi

root=$1
mkdir -p "$root/src"
printf '%s\n' '[package]' 'name = "abstention-boundary-case"' 'version = "0.1.0"' 'edition = "2021"' > "$root/Cargo.toml"
printf '%s\n' \
  'pub fn normalize(value: &str) -> String {' \
  '    value.trim().to_ascii_lowercase()' \
  '}' > "$root/src/lib.rs"
printf '%s\n' \
  '# Abstention Boundary Case' \
  '' \
  'No repository graph or co-edit journal is supplied for this case.' > "$root/README.md"
