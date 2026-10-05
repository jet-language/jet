#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Fail closed: no fit or incomplete ladder is unavailable, never a pass.
set -eu
[ "$#" -eq 1 ] || { echo 'usage: growth-gate.sh <ladder-run-dir>' >&2; exit 2; }
exec node "$(dirname "$0")/growth-gate.mjs" "$1"
