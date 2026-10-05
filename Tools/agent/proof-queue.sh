#!/usr/bin/env bash
# Main starts at most two services, outside the build lanes.
set -euo pipefail
agent_tools="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$agent_tools/host-env.sh"
: "${JET_SCRATCH_ROOT:?configure JET_SCRATCH_ROOT in the host environment or jet/env}"
lane="${2:-}"
if [ "$#" -ne 2 ] || [ "$1" != --lane ] || [[ "$lane" != 1 && "$lane" != 2 ]]; then
  echo 'usage: proof-queue.sh --lane 1|2' >&2
  exit 64
fi
mkdir -p "$JET_PROOFQ_ROOT"
# -o keeps the service lock out of proof-job descendants.
exec flock -n -o "$JET_PROOFQ_ROOT/lane$lane.lock" node "$agent_tools/proof-queue.mjs" --lane "$lane"
