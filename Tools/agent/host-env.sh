#!/usr/bin/env bash
# Source host defaults; this file does not choose a machine's scratch filesystem.
jet_host_env="${JET_HOST_ENV_FILE:-${XDG_CONFIG_HOME:-$HOME/.config}/jet/env}"
if [ -f "$jet_host_env" ]; then
  source "$jet_host_env"
fi
if [ -n "${JET_SCRATCH_ROOT:-}" ]; then
  export JET_SCRATCH_ROOT
fi
unset jet_host_env
export JET_DEV_ROOT="${JET_DEV_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/jet-dev}"
export JET_PROOFQ_ROOT="${JET_PROOFQ_ROOT:-$JET_DEV_ROOT/proofq}"
