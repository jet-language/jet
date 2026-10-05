#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
# Memory-safe, serialized cargo for every agent in this checkout.
# usage: safe-cargo.sh <cargo args...>     e.g. safe-cargo.sh check -p jet-jit --message-format=short
# - one cargo at a time machine-wide (flock), so builds never stack up;
# - hard 14 GB cap (systemd scope): an overrun kills this cargo, never the machine;
# - 2 jobs, no incremental, scratch TMPDIR.
set -u
# Failed transient scopes linger and collide with reused PIDs ("already loaded"): clear them first.
systemctl --user reset-failed 2>/dev/null
# Pip's lock-serialized build targets use incremental compilation (owner, 2026-09-30).
case "$(basename "${CARGO_TARGET_DIR:-target}")" in target-integ|target-check|target-proof) export JET_CARGO_INCREMENTAL=1;; esac
# One machine-wide lane for all of Main's cargo (every target dir), so builds
# never stack; workers' heavy jobs use the separate unitcheck lane (<= 12 GB).
lock=$HOME/.cache/jet-dev/laneA.lock
exec flock -o "$lock" systemd-run --user --slice=jetwork.slice --scope -q --unit="jw-$(basename "$0" .sh)-$$-$(date +%s%N)" -p MemoryMax=${SAFE_CARGO_MEM:-16G} -p MemorySwapMax=0 \
  bash -c 'ulimit -c 0; exec "$@"' safe-cargo env CARGO_BUILD_JOBS=${SAFE_CARGO_JOBS:-2} TMPDIR=$HOME/.cache/jet-dev/scratch \
  timeout ${SAFE_CARGO_TIMEOUT:-2400} $JET_REPO/Tools/agent/jet-env env ${CARGO_ENCODED_RUSTFLAGS:+CARGO_ENCODED_RUSTFLAGS=$CARGO_ENCODED_RUSTFLAGS} cargo "$@"
