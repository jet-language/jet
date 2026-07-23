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

Prefer deletion. Do not replace withdrawn items with heavier machinery. Owner
gates stay ballots. First-party compiler/stdlib breadth is product — do not
outsource it away.

Implement only high-confidence ungated deletes on owned paths; otherwise report
only. Baseline with `scripts/agent/jet-env` before code deletes.

Write one Tower scratch note summarizing what was removed or proposed.
