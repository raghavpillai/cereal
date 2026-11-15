#!/bin/bash
set -euo pipefail
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
cargo test --locked
cargo build --locked
cereal_smoke_dir="$(mktemp -d /tmp/cereal-smoke.XXXXXX)"
trap 'rm -rf "$cereal_smoke_dir"' EXIT
CEREAL_DATA_DIR="$cereal_smoke_dir" ./target/debug/cereal --smoke-test
