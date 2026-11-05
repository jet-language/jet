# roots.sh: the dev tool roots for shell tools; source it (`. Tools/agent/roots.sh`).
# Tools/agent/roots.mjs is the same table for node tools. Repository tools find
# their state, scratch, queues, published compilers and seeds through these and
# nothing else. A machine sets its own values in the environment or in
# ${XDG_CONFIG_HOME:-~/.config}/jet/env, which this file reads first; without
# them the defaults derive from $HOME and the XDG directories, so a fresh clone
# needs no setup. No committed file names one machine's copy
# (personal-info-guard.sh).
#
#   JET_STATE_ROOT    per-user dev state and caches  ($XDG_CACHE_HOME/jet-dev)
#   JET_SCRATCH_ROOT  large shared scratch           ($JET_STATE_ROOT)
#   JET_RUN_ROOT      land queue, locks, slot ledger ($JET_SCRATCH_ROOT/run)
#   JET_BASE_ROOT     published compilers            ($JET_SCRATCH_ROOT/discovery/base)
#   JET_TARGET_ROOT   build target dirs              ($JET_SCRATCH_ROOT/targets)
#   JET_SEEDS         seed store                     ($XDG_DATA_HOME/jet-seeds)
#   JET_SEED          the admitted seed dir          ($JET_SEEDS/current)
#   JET_DEV_WORKTREE  the checkout the lander commits to (this checkout; a
#                     POSIX sh caller that sources this file sets it first)

if [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/jet/env" ]; then
  . "${XDG_CONFIG_HOME:-$HOME/.config}/jet/env"
fi
export JET_STATE_ROOT="${JET_STATE_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/jet-dev}"
export JET_SCRATCH_ROOT="${JET_SCRATCH_ROOT:-$JET_STATE_ROOT}"
export JET_RUN_ROOT="${JET_RUN_ROOT:-$JET_SCRATCH_ROOT/run}"
export JET_BASE_ROOT="${JET_BASE_ROOT:-$JET_SCRATCH_ROOT/discovery/base}"
export JET_TARGET_ROOT="${JET_TARGET_ROOT:-$JET_SCRATCH_ROOT/targets}"
export JET_SEEDS="${JET_SEEDS:-${XDG_DATA_HOME:-$HOME/.local/share}/jet-seeds}"
export JET_SEED="${JET_SEED:-$JET_SEEDS/current}"
if [ -z "${JET_DEV_WORKTREE:-}" ] && [ -n "${BASH_VERSION:-}" ]; then
  eval 'JET_DEV_WORKTREE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"'
fi
export JET_DEV_WORKTREE
