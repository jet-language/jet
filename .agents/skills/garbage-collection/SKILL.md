---
name: garbage-collection
description: >-
  Delete dead code machinery and stale docs, plans, and agent outputs. Prefer
  deletion.
---

# Garbage Collection

Collect garbage in:

- **Code:** dead machinery, unreachable trees, speculative extension points,
  parallel mechanisms for one job.
- **Docs / plans / outputs:** stale plans, obsolete proposals, unused agent
  notes, duplicate write-ups that are no longer authority.

Implement only high-confidence ungated deletes on owned paths; otherwise report
the candidate without creating another artifact. Baseline with
`scripts/agent/jet-env` before code deletes.
