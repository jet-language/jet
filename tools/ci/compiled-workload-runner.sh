#!/usr/bin/env bash
# Card #1414: stable CI entry point for the real compiled-workload producer.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec node "$ROOT/tools/ci/compiled-workload-runner.mjs" "$@"
