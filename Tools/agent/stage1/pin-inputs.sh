#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Pin the retained jetc0's build inputs: copy the retained repo (jetc0.env repo,
# a live worktree that overlay-sync.sh rewrites) into $KEEP/inputs, once per
# retained jetc0 (lib.sh pin_inputs). Run it right after the stage-zero test
# retains jetc0 (stage0-rel.sh does); every stage-one script pins on first use
# otherwise, with a warning when the repo already changed after jetc0.
#
# usage: [JETC0_KEEP=DIR] pin-inputs.sh
# Takes the stage-zero lock unless STAGE1_LOCKED=1 (set it when the caller
# already holds ~/.cache/jet-dev/stage0.lock, as stage0-rel.sh's caller does).
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"
load_keep
echo "pin: $KEEP_PIN ($(tr '\n' ' ' < "$KEEP_PIN/.pinned"))"
