# Pragmatism method and report

## Evidence obligations

Freeze the domain/workload rows before probing. For each row, name one concrete job, what done means, and the Jet surfaces it touches. Walk the happy path with real examples or a minimal repro under `scripts/agent/jet-env`. Record every token the user writes even though the compiler or stdlib already knows the answer.

Classify each friction with the taxonomy below. Propose the smallest complete fix as default magic → optional reject → optional override. Kill a slice that breaks invariants, duplicates a mechanism, or hides expert control. New syntax, a new external stdlib dependency, an invariant carve-out, or a taste choice becomes a ballot title only unless the owner changes the report-only boundary.

For a broad run, the root's six-workload default is mandatory. A named narrower owner request is the only breadth reduction. If a source or workload is unavailable, mark it `unknown` with the exact reason; do not call the row complete and do not silently add another row.

## Friction taxonomy

| Kind | Meaning |
| --- | --- |
| `missing-default` | The compiler or stdlib already knows the answer, but the user must opt in. |
| `dead-end-magic` | A feature exists but fails at the last mile, such as units checking without useful printing. |
| `no-reject` | A default cannot be turned off for a type, package, or project. |
| `no-override` | A default cannot be replaced with a hand-written path. |
| `wrong-default` | A rare case is the default, so the common case pays the tax. |
| `domain-blind` | The surface ignores a workload's obvious needs. |
| `keep` | Default, reject, and override already line up; celebrate it. |

## Calibration cases

Re-verify these against the tree. They are pressure points, not settled law:

1. **Auto derives (S55 family).** Ask whether every useful trait derives by default, whether a user can reject derivation for a type or package, and whether a user can override it selectively.
2. **Dimensional or unit printing** (`examples/features/types/dimensional_quantities.jet`). Algebra and dimension checks exist, but `print(recovered)` may still omit useful units such as `12 meter`, `4 meter/second`, or `766 px`. Ask whether the last mile finishes the scientist's or UI author's job.

## Required report sections

```markdown
# Pragmatism audit — YYYY-MM-DD

## Thesis
One short paragraph: where Jet helps finish jobs and where it stops short.

## Domain scorecard
| Domain / workload | Job | Grade (ships / friction / blocked) | Top friction kind | Evidence |

## Findings
Ranked. Each gives kind, domain, evidence (file/example/command), beginner impact, expert reject/override status, agent-path grade (which of the five quantities it costs), smallest fix, and owner gate (`yes/no`, plus ballot title when yes).

## Defaults map
Table of “most likely use case → today's default → reject path → override path”. Mark holes.

## Celebrated pragmatism
Defaults that already ship the job; preserve them.

## Next actions
Ballot titles or card IDs only. Do not create cards in a report-only run.

## Finding dispositions
<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
<!-- /audit-dispositions -->
```

If this run exposes a method improvement, record it in the report. Do not edit this reference during the audit. Report completion stays separate from implementation completion.

## Anti-goals

- Do not add every Rust derive marker.
- Do not turn this into peer popularity or trust (`gauntlet`).
- Do not optimize ontology unity for its own sake (`isomorphic-ontology-audit`).
- Do not invent parallel mechanisms for one semantic job (I8).
- Do not lower safety to look pragmatic (I1).
