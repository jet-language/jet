# Audit finding dispositions

Read this file before every audit, research, mining, frequency, gauntlet, or
first-principles run that uses the shared report contract. It owns the repeated
publication, workflow-boundary, and finding-disposition mechanics below. The
method skill still owns its question, evidence, investigation, artifact,
write boundary, and stopping rule.

## Shared method boundary

- Select one named method as the primary outcome. Do not start an undeclared or
  automatic follow-on workflow.
- A method may declare bounded evidence helpers, including independent
  research work. Helpers remain acyclic, return to the named method owner, and
  cannot expand the requested outcome.
- A passive read of a referenced method, ontology, lens, spec, or report is not
  an extra workflow.
- Frontmatter describes a skill for discovery. Do not infer host enforcement
  from frontmatter. Qualify any claim about host invocation limits, routing, or
  write enforcement with observed host evidence; do not claim untested
  behavior.

## Shared publication and closeout

Use the artifact location and method-specific publication command declared by
the method. These rules apply to reports unless the method declares a different
retained artifact:

- Use the project-approved non-serve CLI or method-owned installer. Never
  hand-edit Tower board JSON.

For a standard dated report, use the non-serve Tower CLI form
`node plugins/tower/tower.mjs docs add --section <audits|research> --id
<skill>-YYYY-MM-DD --title "…" --file -`. Use `docs update` only for the same
day when the method permits it. A method-owned installer, such as a
checkpointed report installer, remains the source for that method.
- Add a new dated report only at the method's declared `docs/audits/` or
  `docs/research/` location. Revise only the same day's report when the method
  permits it. Never overwrite another day's report or write reports under
  `docs/plans/`.
- Keep report completion separate from implementation completion. A report can
  be complete while a recommendation remains unimplemented.
- Report-only methods do not create Tower cards, decisions, ballots, or
  implementation edits unless the owner explicitly changes that boundary.
  They may cite existing Tower IDs read-only. For a report-only finding with no
  permitted write, use `no-action` with a concrete reason such as `report-only
  run: recommendation recorded; implementation is not authorized here`.
- A method that explicitly owns Tower logging or a proposal/card/ballot
  deliverable keeps that obligation. This shared contract does not authorize
  writes for another method.
- Follow `AGENTS.md` and the current authority chain. Do not infer host
  behavior from skill prose.

## Finding dispositions

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
closed. Existing Tower IDs may be cited read-only; a disposition row does not
authorize a new Tower write.
