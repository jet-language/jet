#!/usr/bin/env bash
# Jet adapter for Tools/perf/enum_codec_bench.py (#3151).
#   jet-adapter.sh --build   builds adapter.jet once (canonical core.encoding.json codec)
#   jet-adapter.sh <protocol options>   execs the prebuilt AOT binary; never compiles.
# JET must name a jet compiler binary (default: the checkout's target/release/jet),
# never a wrapper script that itself takes a jet slot.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
work=${ENUM_CODEC_JET_WORK:-$HOME/.cache/jet-dev/enum-codec-jet}
bin=$work/.jet/build/adapter
if [ "${1:-}" = "--build" ]; then
  jet=${JET:-$root/target/release/jet}
  mkdir -p "$work"
  cp "$root/Tools/perf/enum_codec/adapter.jet" "$work/adapter.jet"
  cp "$root/Tools/perf/enum_codec/package.jet" "$work/package.jet"
  cd "$work" && exec "$root/Tools/agent/jet-env" "$jet" build --release adapter.jet
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
