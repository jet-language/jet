#!/usr/bin/env bash
# Serde peer adapter for Tools/perf/enum_codec_bench.py (#3151).
# Builds the pinned peer once (release, offline) and execs it with the harness options.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
target=${ENUM_CODEC_SERDE_TARGET:-$root/target-enumcodec}
bin=$target/release/enum_codec_serde_peer
if [ ! -x "$bin" ]; then
  (cd "$root/Tools/perf/enum_codec/serde_peer" &&
    CARGO_TARGET_DIR=$target "$root/Tools/agent/jet-env" cargo build --release --offline -q) >&2
fi
exec "$bin" "$@"
