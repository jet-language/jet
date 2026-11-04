# Pre-cutover closure audit (#6563 c1, 2026-11-04)

The owner ruled D-AUDIT-PRECUT1 = D on 2026-11-04.
Every pre-cutover row from the closure audit gets re-proved on today's compiler.
That is 3,670 rows on 1,344 archived cards: the closure audit's rows (`Docs/audits/closure-audit-2026-11/`) closed before the Rust cutover.

## What was run

- Compiler: the pinned base `jet@0.0.1#006bccd5bacdc20cced20cbccd43220c86fb24430f5433b427243a6b78c1bf8d`, built from dev `9c7ddf1359`.
- Tree: a `git archive` export of `9c7ddf1359`, used as `JET_TOOLCHAIN_ROOT`.
- Suites: the golden, ui and case suites on o0/o1/o2 (`jet run`, `jet build`, `jet build --release`).
- Probes: 34 small programs in `probes/`, one claim each, for rows that no shipped test covers. `probes/run.mjs` runs them; `probes/RESULTS.txt` holds their output.

The base's jetpack bootstrap failed on `9c7ddf1359`.
Its log says the Jetpack compiler slice drifted from the compiler's packages (`RegistrationFacts.jet`, `Declarations/Effects.jet` and `Declarations/InBodyMarkers.jet` are not listed).
So the 35 cases that need jetpack did not run, and jetpack rows stay unproven.

## Verdicts

| Verdict | Rows |
| --- | ---: |
| VERIFIED | 827 |
| REOPEN | 2,403 |
| OBSOLETE | 440 |

- **VERIFIED** names the commit (`9c7ddf1359`) and the exact proof.
  The proof is passing tests or probes (592 rows), a ruling recorded in `Docs/spec` (162 rows), or a cited source read or Tower test.
  A row is VERIFIED only when at least one chosen test passes and none fails.
- **REOPEN** names the reason and the live card that owns the requirement.
  Archived cards cannot take criteria, so each REOPEN row points at a ready, planning or building card.
  - 1,161 rows: no dev test, golden, case or snapshot proves them today.
  - 566 rows: a chosen test or probe fails, including known-failing entries.
  - 419 rows: the same text is already a criterion on a live card, so no write is needed.
  - 171 rows: they need jetpack, which did not build (to #3590).
  - The rest: unrecorded rulings (to #5982 or #5983), goldens with no expected output (to #6538), refused seams (to #6317) and cited gaps.
- **OBSOLETE** cites its reason.
  Most rows name Rust sources, Rust tiers or `scripts/` tools that the cutover deleted (431 rows).
  The others are merge notes or one-time chores on a past tree.

## Defects the probes found

- `p1897_stack_overflow`: o0 and o1 name `recurse` and its source line; o2 prints `` Stack overflow in `??` `` with no location.
- `p3478_union_equality`: comparing two `Int | String` values is an internal compiler error on every tier (`ir.build unlowered: comparison of Int | String`).
- `pu4_union_constant`: a module constant holding an anonymous union is an internal compiler error on every tier (`const_text: dst is Pointer, not Word|String`).
- `panonunion_shapes`: the same fault, with every union shape in one program; `pu1`–`pu4` and `p3478` split it per card.

Notes from writing the probes:

- `String.split` returns `Iter<String>`; calling `.flatten()` on `[Iter<String>]` gives E0108 ("callback result is not valid for `.flatten`"), which hides the real cause.
- User structs named `Cell` or `Tally` collide with builtin names (E0106).
- A parameterless type alias over a union is refused (`jet.AliasWithoutParameters`), so #3474 is OBSOLETE.

## Files

- `PRECUT-AUDIT.tsv`: one row per pre-cutover row. Columns: `ref`, `card`, `type`, `verdict`, `commit`, `proof`, `target`, `reason`, `text`.
- `TESTS.tsv`: every suite result the verdicts used (verdict, test id, tier variant).
- `probes/`: the probe sources, expected outputs, `run.mjs` and `RESULTS.txt`.

The board writes (one criterion per REOPEN requirement, on its live card) are run from Main's brief `$JET_SCRATCH_ROOT/lanes/Main/briefs/6563-precut-cmds.md`.

## Rerunning the probes

```sh
node Docs/audits/precut-2026-11/probes/run.mjs <jet> <toolchain-root> Docs/audits/precut-2026-11/probes <out.json> [filter]
```

`<jet>` is a base compiler such as `$JET_SCRATCH_ROOT/discovery/base/<sha>/jet`; `<toolchain-root>` is an export of the same commit.
Each run works in a fresh `probes/work/run-<time>` directory.
