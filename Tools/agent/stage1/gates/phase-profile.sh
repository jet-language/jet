#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
set -eu
[ "$#" -eq 2 ] || { echo 'usage: phase-profile.sh <jetc0> <rung-project>' >&2; exit 2; }
exec python3 "$(dirname "$0")/phase-profile.py" "$1" "$2"
