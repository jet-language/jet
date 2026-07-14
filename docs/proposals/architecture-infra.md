# Architecture & infrastructure audit — 2026-07-11

Measured state (full-repo census, investigator-verified), what it means,
and the action per finding: a ballot where a real choice exists, an
agent-lane card where the fix is mechanical, nothing where the state is
already right.

## Measured

- **267k LOC Rust** across 14 seam crates + a **45k-LOC root `Source/`
  monolith** (79 files). architecture.md calls the root "a thin facade
  and binary host" — it is neither: it holds the REPL (2.7k),
  CmdCompile, the CLI dispatch, debugger, and dev-server glue.
- **9 files > 2500 LOC**: Comptime/Methods.rs 4.0k, Sema/Registration.rs
  3.4k, Sema/Bundle.rs 3.2k, Parser/Statements.rs 3.1k,
  Prelude/Core.rs 2.8k, Source/REPL/mod.rs 2.7k, codegen/scheduler.rs
  2.7k, jetpack Provider.rs 2.5k, Store.rs 2.5k.
- **Tests**: 101 integration files, 1,637 `#[test]`s; largest
  tests/jetpack.rs 7.0k LOC. verify-full.sh already tiers (parallel,
  repo-local TMPDIR, Canvas gate, D-CI3 red-flag CI).
- **Surface census**: 62 registered markers (20 `@` / 42 `#`), derived from
  `Syntax::CONTRACT_MARKERS` and `Syntax::DIRECTIVE_MARKERS`; 111 KW_ entries,
  ~70 core modules, 587 diagnostic codes.
- **Census/law drift in Syntax.rs**: `view` remained lexer-reserved after
  D-MEM1 retired that keyword job. The other questioned entries are live:
  synthetic dispatch subject `it` is foundational M4 syntax, `Clock` is the
  D-DET1 injectable type, and `taskgroup` is D-TASKSCOPE1=A. Bare
  `#Wasm`/`#Js`/`#Suppress` are absent under D-MARK-TARGET1=A and
  D-MARK-DISCARD1=A. `Experimental`/`Tested`/`Hardened` are closed values of
  `#Meta(maturity: ...)`, not standalone markers (D-MARK-META1=B).
- **Docs**: the two dated review documents are proposals/audit records and
  belong in `docs/proposals/`. `docs/design/` is no longer empty: it contains
  active frontend design artifacts and remains in place.
- **Dual agent-config dirs**: intentional. `.agents/` owns tool-neutral shared
  prompts/skills; `.claude/agents/` owns Claude Code harness definitions.
- The root workspace excludes `corelib/core.archive/pkgs/archive` because it
  is independently built first-party package source with external `zip`/`tar`
  dependencies, not an I6 compiler workspace member.

## Ballots (card #508)

- **D-ARCH-SOURCE1 — dissolve the Source/ monolith.** The root crate
  should be what architecture.md already claims: `main.rs` + rustc
  invocation/ICE banner (R5 pins CmdCompile at the root edge). REPL,
  debugger and CLI registry/dispatch moved to seam crates; `jet-devserver` now owns dependency-free watch policy and HTTP/static transport. Full `CmdDevWeb` relocation waits on `Source/Canvas` and root web-artifact/diagnostic orchestration becoming inward seams; no callback adapter or root back-edge substitutes for that ownership move.
  (`jet-cli`, `jet-repl`, `jet-debug`) with the same I6/path-dep rules,
  truthfulness tests extended. Options: full dissolution / extract
  interactive tiers only / status quo.

## Cards, no ballot needed (mechanical, agent lane)

- **Module splits**: the nine >2500-LOC files split along their existing
  section comments; no behavior change, snapshot-pinned.
- **Test-file splits**: tests/jetpack.rs (7k) and tests/tir.rs (4.8k)
  split by feature family so targeted `--test` runs stay cheap.
- **CLI seam**: `crates/jet-cli` owns the one command/flag registry,
  completion/man generation, offline diagnostic reference, and hybrid help
  UI. The root package re-exports those APIs without wrapper source.
- **Syntax.rs census-drift sweep**: unreserve retired `view`; record the live
  law for `it`/`Clock`/`taskgroup`; reconcile the marker-plane matrix with
  D-MARK-META1=B and card #498; regenerate editor grammars.
- **Docs hygiene**: fold the two unique review/audit records into
  `docs/proposals/`; preserve the now-populated `docs/design/`; document the
  two agent-config roles and the `core.archive` workspace exclusion.
- **Prelude source audit**: the vendored
  `corelib/core.archive/pkgs/archive/src/lib.rs` is the sole archive runtime
  source. Both CoreProvider's standalone build and the hidden FFI bridge consume
  it; no copied embedded runtime is maintained.

## Already right (no action)

- Seam-crate dependency direction is machine-pinned
  (tests/workspace_crates.rs, truthfulness.rs).
- CI shape (D-CI3=A) and verify scheduler (D-VERIFY-SCHED1) are ratified
  and match the measured scripts.
- Tower app (`Tower/`) vs data (`.tower/`) split is deliberate.
- Diagnostics registry: 587 codes, banded, every code snapshot-pinned —
  healthy.

Surface-census actions (marker growth law, secrets-module merge, core
namespace tidy) are syntax/stdlib decisions, ballot-ed on card #509 —
see `surface-condensation.md` v2 section.
