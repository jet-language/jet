---
name: structure-cleanup
description: >-
  Reorganize owned Jet code or prose for clearer navigation without changing
  semantics, APIs, diagnostics, or goldens. Use only for structure-only work.
---

# Structure Cleanup

Improve layout, naming, and cohesion only. A structure cleanup must not change
semantics, public APIs, diagnostics, snapshots, or golden output. If behavior or
an acceptance contract must change, stop and use ordinary implementation plus
[`verify`](../verify/SKILL.md).

Name the owned paths and current scope. Preserve the full path cutover for
moves: callers, imports, fixtures, embedded paths, harness lists, generated
uses, and links. Use existing modules and links where possible; route a dead
machinery or stale-artifact deletion to [`garbage-collection`](../garbage-collection/SKILL.md).

Use `scripts/agent/jet-env` for a relevant code baseline or code-path check. A
prose-only layout change does not require compiler checks. Land only owned
structure work; when a written outcome is requested, return the concise result
to the current Tower completion owner, otherwise report it in chat.
