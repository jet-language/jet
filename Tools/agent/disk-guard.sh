#!/usr/bin/env bash
# Start through jet-env, outside every build lane (Node and lsof required).
set -euo pipefail
agent_tools="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$agent_tools/host-env.sh"
export JET_REPO="${JET_REPO:-$(cd "$agent_tools/../.." && pwd)}"
: "${JET_SCRATCH_ROOT:?configure JET_SCRATCH_ROOT in the host environment or jet/env}"
mkdir -p "$JET_DEV_ROOT" "$JET_PROOFQ_ROOT"
if [ "${1:-}" = --check ]; then
  exec node "$agent_tools/disk-guard.mjs" "$@"
fi
# One guard owns moves/evictions; checks do not wait behind the daemon.
exec flock -n -o "$JET_DEV_ROOT/disk-guard.lock" node "$agent_tools/disk-guard.mjs" "$@"
