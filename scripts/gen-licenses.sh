#!/usr/bin/env bash
# Regenerate src-tauri/THIRD_PARTY_LICENSES.html so the bundle ships fresh
# attribution for every Rust dependency. Run before tagging a release, or
# after any Cargo.toml / Cargo.lock change that touches deps.
#
# Requires: cargo install cargo-about --locked --features cli

set -euo pipefail

cd "$(dirname "$0")/../src-tauri"

if ! command -v cargo-about >/dev/null 2>&1; then
  echo "cargo-about not found. Install with:" >&2
  echo "  cargo install cargo-about --locked --features cli" >&2
  exit 1
fi

cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html
echo "Regenerated src-tauri/THIRD_PARTY_LICENSES.html"
