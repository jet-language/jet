#!/usr/bin/env bash
# Full native closeout: compiler registries plus the complete Jet testkit suites.
# JET selects an already-built admitted compiler; this never builds Jet itself.
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec node "$repo/Tools/agent/verify-full.mjs" "$@"
