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
- **Surface census**: 62 registered markers (21 `@` / 41 `#`), derived from
  `Syntax::CONTRACT_MARKERS` and `Syntax::DIRECTIVE_MARKERS`; 111 KW_ entries,
  ~70 core modules, 587 diagnostic codes.
- **Census/law drift in Syntax.rs**: `view` listed as a keyword
  (D-MEM1 retired it), `it`/`Clock`/`taskgroup` entries of unclear
  status, `#Wasm`/`#Js`/`#Suppress` pending removal (ratified), maturity
  trio previously occupied standalone marker arrays but now lives only in
  `#Meta(maturity: ...)` (D-MARK-META1=B).
- **Docs**: 72 files / 8 subdirs; `docs/design/` is empty; `reviews/`
  holds 2 files that are proposals by another name.
- **Dual agent-config dirs**: `.agents/` (10 files) and
  `.claude/agents/` (3) — cross-tool intent unverified.
- Workspace excludes `corelib/core.archive/pkgs/archive` ad hoc.

## Ballots (card #508)

- **D-ARCH-SOURCE1 — dissolve the Source/ monolith.** The root crate
  should be what architecture.md already claims: `main.rs` + rustc
  invocation/ICE banner (R5 pins CmdCompile at the root edge). REPL,
  debugger, CLI registry/dispatch, dev server move to seam crates
  (`jet-cli`, `jet-repl`, `jet-debug`) with the same I6/path-dep rules,
  truthfulness tests extended. Options: full dissolution / extract
  interactive tiers only / status quo.

## Cards, no ballot needed (mechanical, agent lane)

- **Module splits**: the nine >2500-LOC files split along their existing
  section comments; no behavior change, snapshot-pinned.
- **Test-file splits**: tests/jetpack.rs (7k) and tests/tir.rs (4.8k)
  split by feature family so targeted `--test` runs stay cheap.
- **Syntax.rs census-drift sweep**: prune `view` (D-MEM1), resolve
  `it`/`Clock`/`taskgroup` status against ratified law, move maturity
  trio to the `@` arrays (D-MATURITY1), drop `#Wasm`/`#Js`/`#Suppress`
  (rides #498); `jet devtools grammars` + re-bless after.
- **Docs hygiene**: delete empty `docs/design/`; fold `docs/reviews/`
  into `docs/proposals/`; verify `.agents/` vs `.claude/agents/` intent
  and de-duplicate or document why both exist; document the
  `corelib/...` workspace exclusion or remove it.

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
