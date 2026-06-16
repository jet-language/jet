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
| 1 | E2-M3 DX CLI (explain/doctor/--json) | `m3-dx-cli.md` | ✅ DONE (chunks 1–6, all exit criteria met) |
| 2 | E2-M5 tier-2 references | `m5-references.md` | NEXT |
| 3 | E2-M2 release policy/editions | `m2-release-policy.md` | DEFERRED — do docs/policy + `jet --version` banner anytime; defer edition-in-manifest until pack.jet migration settles |

Deferred (collision/dependency): E2-M14 (jetpack owns — already committed),
E2-M4 (`jet dev`, rides LSP foundation; revisit after M3/M5).

### E2-M3 completion record (branch `epoch-2-impl`)

Commits `7412fe7`→`44dc85f`. Exit criteria (m3-dx-cli.md) all met:
- ✅ golden tests pin human + `--json` output; CI mode deterministic + ANSI-free
- ✅ every diagnostic points to `jet explain` via a dim, gated "learn more" footer
- ✅ `jet doctor` actionable + offline by default (`--online`/`--fix`)
- ✅ no-args `jet` greets/orients; typo'd subcommands/flags → E2101/E2102
- ✅ completions (bash/zsh/fish) + man pages from one `cli_spec` source (drift-tested)
- ✅ unified fix engine (`src/fixengine.rs`) shared by `jet fix` + LSP
- ✅ full `nix develop -c cargo test` green; no new crates (I6)

New codes registered (E21xx/L21xx range, disjoint from jetpack's E09/E1x/E32xx):
E2101 (unknown subcommand), E2102 (unknown flag), L2101 (doctor advisory).
New modules: `src/explain.rs`, `src/diagjson.rs`, `src/doctor.rs`,
`src/cli_spec.rs`, `src/fixengine.rs`.

**Deferred from M3 (gated, not skipped sloppily):**
- **Digit separators (D-SUGAR1 `1_000_000`)** — ballot is OPEN/unratified; per
  I7 syntax gate NOT implemented. `examples/features/34_digits.jet` therefore
  not created. Implement when D-SUGAR1 is ratified.
- `jet fix --json` planned-edit emission (the same edits are already exposed via
  `jet check --json`); package-command `--json` (lives behind jetpack files).

## Protocol per milestone (CLAUDE.md workflow loop)

test-first (ui fixture/example/golden) → spec in docs/spec → parser → sema →
codegen/CLI → `nix develop -c cargo test` green → `tests/decisions.rs` green →
docs updated → commit on `epoch-2-impl`. Validate the FULL suite before each
commit; never start a milestone on a red baseline.

## Resume pointer

**Current state:** E2-M3 fully implemented + validated (full suite green) on
branch `epoch-2-impl`, commits `5050565`→`44dc85f`. **Next: E2-M5 tier-2
references** (`m5-references.md`) — harden `view` returns / `ref` fields
soundness matrix, register E23xx codes, add LSP inlay hints (D-REF3); D-REF2
(arenas) is OPEN/optional, not a blocker. Touch only the view/ref region of
`src/sema.rs` + `src/lsp.rs` + E23xx in `docs/spec/diagnostics.md` — NOT the
record-literal path (jetpack territory).

After M5: revisit E2-M2 (policy docs + `jet --version` banner are collision-free
and can land anytime; the edition-in-manifest piece waits on the pack.jet
migration).

(Update this section after each milestone commit so a fresh agent resumes here.)
