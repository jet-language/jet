#!/usr/bin/env bash
# Sync docs/08-stdlib.md from docs/stdlib.md.
set -euo pipefail
cd "$(dirname "$0")/.."
UPDATE_DOCS=1 cargo test gen_stdlib_doc -- --nocapture
