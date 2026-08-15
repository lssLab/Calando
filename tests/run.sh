#!/bin/sh
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo fmt --manifest-path "$ROOT/Cargo.toml" --all -- --check
cargo clippy --manifest-path "$ROOT/Cargo.toml" --all-targets --locked -- -D warnings
cargo test --manifest-path "$ROOT/Cargo.toml" --all-targets --locked
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked
for file in \
  "$ROOT"/*.sh "$ROOT"/packaging/*.sh "$ROOT"/packaging/release/*.sh \
  "$ROOT"/runtime/hooks/*.sh "$ROOT"/runtime/notifications/*.sh \
  "$ROOT"/runtime/bin/memory-status "$ROOT"/runtime/bin/memory-supervisor; do
  /bin/sh -n "$file"
done
