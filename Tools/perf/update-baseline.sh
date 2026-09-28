#!/usr/bin/env sh
# c121 — refresh the committed perf baseline.
#
# Run this deliberately after an intentional performance change, then commit
# Tools/perf/baseline.json. CI (ci-perf-check.sh) gates future builds against it.

set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
exec "$ROOT/Tools/perf/dashboard.sh" --baseline
