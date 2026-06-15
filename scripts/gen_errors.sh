#!/usr/bin/env bash
# Generate docs/reference/errors/E####.md from tests/ui snapshots.
set -euo pipefail
cd "$(dirname "$0")/.."
UPDATE_DOCS=1 cargo test gen_error_pages -- --nocapture
