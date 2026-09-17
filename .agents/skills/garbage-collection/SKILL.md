---
name: garbage-collection
description: >-
  Delete high-confidence dead Jet machinery or stale owned artifacts. Use when
  an approved current scope names a deletion; not for layout reorganization or
  owner-protected audits and final reports.
---

# Garbage Collection

Prefer deletion, but delete only a short, owned, high-confidence, ungated slice.
An unused declaration is not automatically dead. Before deleting code, trace
callers, generated uses, fixture and path consumers, and the current approved
Tower scope; preserve the program's meaning and complete any required cutover.
Use `docs/spec/contributing/agent-engineering.md` for this boundary.

Audits and final reports are owner-only deletion targets. Agents may move them
intact when authorized, but must not delete them, create a cleanup report,
archive bucket, duplicate plan, or preservation ledger. If a candidate needs an
owner decision, report the candidate without creating another artifact. Route
pure layout or navigation changes to [`structure-cleanup`](../structure-cleanup/SKILL.md).

Work only on named owned paths. For a code deletion, use
`scripts/agent/jet-env` for any relevant baseline command; prose-only deletion
needs no compiler check. Keep the deletion within the current criterion and
scope rather than using cleanup to broaden the task.
