---
name: structure-cleanup
description: >-
  Structure-only cleanup: simplify layout and remove dead indirection with no
  behavior change.
---

# Structure Cleanup

Improve navigation and cohesion only. No semantic, API, diagnostic, or golden
changes — if behavior must change, stop and use ordinary implementation plus
`verify` instead.

Name owned paths. Record baseline commands with `scripts/agent/jet-env`. Prefer
deletion and existing modules. Stage only owned paths.

When the owner wants a written report, use `tower docs add --section audits …`; otherwise
report in chat and land the cleanup.
