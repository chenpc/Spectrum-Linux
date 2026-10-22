#!/usr/bin/env bash
# One entry point for the whole gate: build, unit + UI tests, clippy, then `xtask ci`
# (fmt, token drift, and the snapshot check with `cargo test --all-features`).
#
# egui_kittest runs headless; no display server is needed. The repo toolchain is used when present.
set -euo pipefail
cd "$(dirname "$0")"

if [ -d .toolchain/cargo/bin ]; then
  export RUSTUP_HOME="$PWD/.toolchain/rustup"
  export CARGO_HOME="$PWD/.toolchain/cargo"
  export PATH="$PWD/.toolchain/cargo/bin:$PATH"
fi

cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask ci
