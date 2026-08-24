# Audits

Dated audit reports written by Jet audit skills via
`tower docs add --section audits`.

Keep only the latest useful pulse, or archive the dated file after the backlog
is carded. Re-run the skill for a fresh report; prefer
`tower docs archive <path>` over leaving stale audits in the Docs list.
See [`../plans/docs-cleanup-sweep.md`](../plans/docs-cleanup-sweep.md).

Before closing an audit, run `node scripts/agent/check-audit-dispositions.mjs`.
The check reads live and retired Tower records and lists every report with its
finding dispositions.
