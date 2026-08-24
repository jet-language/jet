# Audit finding dispositions

An audit is not closed until every actionable finding has one disposition. Add
this machine-readable section to the report before close:

```markdown
## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | card | #123 |
| F2 | decision | D-EXAMPLE1=A |
| F3 | no-action | archived: superseded by the 2026-08-24 report |
<!-- /audit-dispositions -->
```

Use one row per finding. Use `card` for work tracked by a Tower card, `decision`
for a ratified Tower decision, and `no-action` only with a concrete reason. Use
an `archived:` reason when the finding is historical, superseded, or retained
only as evidence. The validator reads card and decision status from live and
retired Tower records, so report prose cannot make a missing ledger row look
closed.
