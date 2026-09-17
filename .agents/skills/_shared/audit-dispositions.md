# Audit finding dispositions

Use this contract when deciding an audit's output and write boundary; load publication mechanics when producing a retained report. The selected method owns its question, evidence, and completion. This file owns shared permissions, publication, and finding dispositions.

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

## Scope and permissions

The owner's requested outcome wins over method defaults. Infer a clear scope from the request and existing authority; ask only when materially different outcomes or an owner-only choice remain unresolved.

| Requested outcome | Authorized result |
| --- | --- |
| Chat-only explanation or proposal; no changes | Return the requested answer. No retained report, board writes, or implementation. |
| Report-only audit | Produce the requested report and cite existing Tower records read-only. Do not create cards or ballots. |
| Normal mining, gauntlet, or first-principles invocation with method-owned Tower deliverables | Complete that method's declared report and card/ballot obligations. An explicit report-only instruction overrides those defaults. |
| Implementation explicitly requested | Follow the approved implementation scope and its proof requirements. A recommendation alone does not authorize this transition. |

Owner-only choices are those named by `AGENTS.md`, not every implementation detail. Answer factual questions from evidence; leave unresolved owner choices to the owner. Evidence gaps are reported as gaps, not permission to widen the task.

## Shared publication and closeout

Use the artifact location and method-specific publication command declared by
the method. These rules apply to reports unless the method declares a different
retained artifact:

- Use the project-approved non-serve CLI or method-owned installer. Never
  hand-edit Tower board JSON.

For a standard dated report, use the non-serve Tower CLI form
`scripts/agent/jet-env node plugins/tower/tower.mjs docs add --section <audits|research> --id <skill>-YYYY-MM-DD --title "…" --file - --by <agent>`. Use `docs update` only for the same day when the method permits it. A method-owned installer, such as a
checkpointed report installer, remains the source for that method.
- Add a new dated report only at the method's declared `docs/audits/` or
  `docs/research/` location. Revise only the same day's report when the method
  permits it. Never overwrite another day's report or write reports under
  `docs/plans/`.
- Keep report completion separate from implementation completion. A report can
  be complete while a recommendation remains unimplemented.
- Report-only methods do not create Tower cards, decisions, ballots, or
  implementation edits unless the owner explicitly changes that boundary.
  Cite an existing card or decision with that disposition read-only. If no
  existing record covers the finding and new writes are forbidden, use
  `no-action` with a concrete reason such as `report-only run: recommendation
  recorded; implementation is not authorized here`.
- A method that explicitly owns Tower logging or a proposal/card/ballot
  deliverable keeps that obligation. This shared contract does not authorize
  writes for another method.
- Follow `AGENTS.md` and the current authority chain. Do not infer host
  behavior from skill prose.

## Finding dispositions

Every actionable finding in a retained audit report gets a disposition. Include this machine-readable section before publication; a chat-only answer need not imitate a retained report's format:

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

## Completion

Stop after the declared scope is accounted for, the requested artifact is delivered, and any authorized publication or board obligations are recorded and read back. Report blocked or unavailable evidence explicitly. Do not pursue an unrelated agenda, implement a proposal, or require a particular number of negative findings to finish.
