#!/usr/bin/env bash
# Jet adapter for Tools/Perf/EnumCodecBench.py (#3151).
#   JetAdapter.sh --build   builds adapter.jet once (canonical core.encoding.json codec)
#   JetAdapter.sh <protocol options>   execs the prebuilt AOT binary; never compiles.
# JET must name the admitted native jet compiler binary, never a wrapper script
# that itself takes a jet slot.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
. "$root/Tools/Agent/roots.sh"
work=${ENUM_CODEC_JET_WORK:-$JET_STATE_ROOT/enum-codec-jet}
bin=$work/.jet/build/adapter
if [ "${1:-}" = "--build" ]; then
  jet=${JET:?JET must name the admitted native jet compiler}
  mkdir -p "$work"
  cp "$root/Tools/Perf/EnumCodec/adapter.jet" "$work/adapter.jet"
  cp "$root/Tools/Perf/EnumCodec/package.jet" "$work/package.jet"
  cd "$work" && exec "$root/Tools/Agent/JetEnv.jet" "$jet" build --release adapter.jet
fi
# Forms the canonical codec cannot build AOT today (internal/untagged: ICE at
# MIRRust.rs:7150; adjacent: no ratified marker) exit 77 = unavailable, never a verdict.
for ((i = 1; i <= $#; i++)); do
  if [ "${!i}" = "--form" ]; then
    j=$((i + 1))
    case "${!j}" in external) ;; *) echo "form ${!j} unavailable for the Jet AOT adapter" >&2; exit 77 ;; esac
  fi
done
if [ ! -x "$bin" ]; then
  echo "jet adapter is not built; run: JET=<jet binary> $0 --build" >&2
  exit 2
fi
exec "$bin" "$@"
