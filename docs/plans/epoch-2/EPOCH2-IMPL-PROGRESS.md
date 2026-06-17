# Epoch-2 implementation stream — progress & resume log

**This is the resume doc for the EPOCH-2 implementation stream.**

## Current branch: master

All prior epoch-2 work (`epoch-2-impl`) and the jetpack/jetos track
(`jetos-ratified-arc`) have been merged into `master` (commit `5eec357`).
All future epoch-2 work is directly on `master`. No separate worktrees.

## Completed milestones

E2-M1 ✅ M2 ✅ M3 ✅ M4 ✅ M5 ✅ M13 ✅ M14 ✅

The E2-M3/M5 work lives in commits `7412fe7`→`fd02646` (now on master).

## Milestone plan (sequential, fully-ratified, low-collision)

Order chosen for **collision-safety**, not strict numeric order: M2's
edition-marker piece must add a field to the manifest parser, but
`src/manifest.rs`→`packmanifest.rs` is **actively churning** under the jetpack
agent's pack.jet/payload.jet migration. So M2's manifest piece is deferred;
M3 and M5 touch disjoint regions and go first.

| Order | Milestone | Plan | Status |
|---|---|---|---|
| 1 | E2-M3 DX CLI (explain/doctor/--json) | `m3-dx-cli.md` | ✅ DONE (chunks 1–6, all exit criteria met) |
| 2 | E2-M5 tier-2 references | `m5-references.md` | ✅ DONE (chunks A–B, matrix complete, soundness fuzz green) |
| 3 | E2-M2 release policy/editions | `m2-release-policy.md` | NEXT (collision-free parts) — policy docs + `jet --version` banner anytime; defer edition-in-manifest until pack.jet migration settles |

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

### E2-M5 completion record (branch `epoch-2-impl`)

Commits `3e4db31` (chunk A) + `fd02646` (chunk B). Exit criteria met:
- ✅ soundness matrix COMPLETE — every cell allowed-with-proof (positive fixture)
  or rejected-with-diagnostic+fixture; table filled in `m5-references.md`.
- ✅ no user-written lifetime names anywhere; diagnostics speak Jet words.
- ✅ `examples/features/35_zerocopy.jet` runs, golden-pinned, contrasts a
  borrowed (no-copy, lowers to `&String`) path vs a clone-heavy one.
- ✅ L2301 inlay hints wired in `src/lsp.rs` (D-REF3), tested.
- ✅ soundness fuzz target `tests/ref_soundness_fuzz.rs` (sema-accepted ⇒
  rustc-accepted, no ICE, no `unsafe`) green — found+closed a real chunk-A hole.
- ✅ full `nix develop -c cargo test` green (34 binaries).

Codes: E2301 (returned view outlives owner), E2302 (stored `ref` dangle —
**tightened**: a `ref` field has no sound v1 source except a `'static` const,
so non-const sources are now rejected, closing an ICE), E2303 (delegates to
E1102), **E2304** (view into an index/slice of a param — the helper copies),
L2301 (borrow advisory/inlay). Key allow: `view` into a **field of a parameter**
(incl. through a generic `Wrap<T>` param) — the zero-copy primitive.

## Resume pointer

**Current state:** E2-M3 ✅ and E2-M5 ✅ fully implemented + validated (full
suite green) on branch `epoch-2-impl`, commits `5050565`→`fd02646`.

## Resume pointer

**Current state:** master. All sidequests in `docs/plans/sidequests/` are resolved
EXCEPT `s19-amend-loop-unification.md` (loop keyword unification — `while`/`for`
must become teaching errors; `loop` with header disambiguation is the one form).
That sidequest requires parser work + example rewrites + snapshot bless.

**Next milestone to implement: E2-M6** (`m6-library-authoring.md`).

After M6: implement in dependency order per `docs/plans/EPOCH2-HANDOFF.md`.
Read the sidequest files in `docs/plans/sidequests/mN-*.md` alongside each plan.

(Update this section after each milestone commit so a fresh agent resumes here.)
