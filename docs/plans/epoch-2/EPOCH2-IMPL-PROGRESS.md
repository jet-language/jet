# Epoch-2 implementation stream — progress & resume log

**This is the resume doc for the EPOCH-2 implementation stream.** It is distinct
from `active-task.md` (which belongs to the parallel **jetpack/jetos** agent —
do not edit that file from this stream).

## Isolation / collision-avoidance

- All work happens in a dedicated git worktree on branch **`epoch-2-impl`**
  (`/home/nate/Projects/Github/jet-epoch2`), branched from the jetpack agent's
  HEAD (`cec262a`, branch `jetos-ratified-arc`).
- The jetpack/jetos agent owns: `src/jetpack/*`, `src/syntax.rs` config/U11–U18
  record-literal surface, the record-literal path in `src/parser.rs`/`src/sema.rs`,
  `docs/spec/diagnostics.md` codes **E09xx / E10xx / E12xx / E32xx (C FFI)**,
  `examples/jetpack-typed/`, `tests/jetpack.rs`. **E2-M14 (C FFI) is THEIRS** —
  they already committed it (`cec262a`). Do not touch any of these.
- This stream owns: `jet` CLI surface (`src/main.rs`), `src/diag.rs` JSON,
  `src/manifest.rs` edition field, references in `src/sema.rs` (view/ref region,
  NOT the record-literal region), new modules (`src/explain.rs`, dev server),
  and diagnostic codes **E20xx / E21xx / E23xx + L20xx / L21xx / L23xx**.
- Integration is a future merge of `epoch-2-impl` into the jetpack branch;
  disjoint code ranges + disjoint file regions keep it conflict-light.

## Milestone plan (sequential, fully-ratified, low-collision)

Order chosen for **collision-safety**, not strict numeric order: M2's
edition-marker piece must add a field to the manifest parser, but
`src/manifest.rs`→`packmanifest.rs` is **actively churning** under the jetpack
agent's pack.jet/payload.jet migration. So M2's manifest piece is deferred;
M3 and M5 touch disjoint regions and go first.

| Order | Milestone | Plan | Status |
|---|---|---|---|
| 1 | E2-M3 DX CLI (explain/doctor/--json) | `m3-dx-cli.md` | NOT STARTED |
| 2 | E2-M5 tier-2 references | `m5-references.md` | NOT STARTED |
| 3 | E2-M2 release policy/editions | `m2-release-policy.md` | DEFERRED — do docs/policy + `jet --version` banner anytime; defer edition-in-manifest until pack.jet migration settles |

Deferred (collision/dependency): E2-M14 (jetpack owns — already committed),
E2-M4 (`jet dev`, rides LSP foundation; revisit after M3/M5).

## Protocol per milestone (CLAUDE.md workflow loop)

test-first (ui fixture/example/golden) → spec in docs/spec → parser → sema →
codegen/CLI → `nix develop -c cargo test` green → `tests/decisions.rs` green →
docs updated → commit on `epoch-2-impl`. Validate the FULL suite before each
commit; never start a milestone on a red baseline.

## Resume pointer

**Current state:** worktree created, baseline verification in progress. Next:
implement E2-M2 test-first once baseline confirmed green.

(Update this section after each milestone commit so a fresh agent resumes here.)
