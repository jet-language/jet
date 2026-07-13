# Jet Holistic Review — 2026-07-07

## Scope

Reviewed the governing specs, live Tower state, build health, syntax drift, retired-syntax references, unsafe-gate wording, active flagship work, and CI-quality card status.

Baseline commands:

- `df -h /tmp` — 17G free, 49% used.
- `nix develop -c cargo build` — pass.
- `nix develop -c cargo test --test syntax_reconciliation` — pass.
- `nix develop -c cargo fmt --check` — pass.
- `nix develop -c cargo test` — interrupted after hanging in `tests/dev.rs::dev_default_matches_compiled_binary`.
- `timeout 180 nix develop -c cargo test --test dev dev_default_matches_compiled_binary -- --nocapture` — exit 124 after the one-test run printed the >60s warning.

Tower state at rev 836:

- `#9` is `building`: flagship vertical slices. Recent log says raylib, web storage/events, MMIO write, slices, web build, and golden checks were verified; card is still not done.
- `#211` is `ready`: CI and quality gates. D-CI1/D-CI2 are ratified; no active work started in this pass.
- Open owner questions: none.
- Owner-blocked decisions: none.
- This review was logged on `#211` as a quality handoff; no phase change.

## Findings

### P0/P1: D-S14-PAUSE drift in parser/LSP behavior

`docs/spec/syntax-decisions.md` and `docs/spec/diagnostics.md` say retired `let` teaching is paused and E0009 is retired. Current parser/LSP tests still expect E0009 for `let x = 1`:

- `crates/jet-parser/src/Parser/Statements.rs`
- `Source/LSP/mod.rs`
- `tests/lsp.rs`
- `tests/cli/fix_let.jet`
- `editors/vscode/README.md`
- `editors/zed/README.md`

This is implementation-vs-canon drift. It is ungated because the owner already ratified D-S14-PAUSE, but it needs a deliberate parser/LSP/snapshot sweep rather than a comment-only edit.

### P1: Retired `jet.raylib` internal alias should be audited

Sema and codegen still accept `"jet.raylib"` beside `"core.raylib"` in internal dispatch tables:

- `crates/jet-sema/src/Sema/CheckerCoreLib.rs`
- `crates/jet-codegen/src/Codegen/TIR/emit.rs`

D-CORENS1 says the public namespace is `core.*`, not `jet.*`. If this alias is unreachable from user code, add a test that proves it. If reachable, remove it and snapshot the diagnostic.

### P1: Flagship card is close but not closed

`#9` has strong recent verification logs, including slices, web build, target profiles, diagnostic snapshots, and golden examples. It remains `building`, not `verify` or `done`. Full `nix develop -c cargo test` still needs a current run before closure.

### P1: CI coverage gap remains live

`#211` captures the right plan: every `tests/*.rs` target assigned to exactly one CI shard, warning policy, stale Tower path removal, nightly perf/fuzz/coverage. No code was changed for this card in this audit pass.

## Changes Made

- Updated expert-tier wording in `docs/spec/philosophy.md` from stale `std/mem` + bare `unsafe` wording to `core.mem` + `#Unsafe("reason")`.
- Updated stale `@unsafe` comments to `#Unsafe`.
- Updated lexer comments for retired `~~`: it is lexed so parser can emit E0325, not an unbuilt S83 path.
- Updated stale `core.raylib` skeleton/no-op comments now that D-FLAGSHIP-RAYLIB1 landed.

## Owner-Gated Work

No new owner-gated syntax, dependency, invariant carve-out, or product decision was found in this pass. No ballots created.

## Cards Created After Owner Follow-Up

- `#266` — Reconcile D-S14-PAUSE with live `let`/E0009 behavior.
- `#267` — Expand syntax_reconciliation to catch retired teaching paths.
- `#268` — Prove or remove the internal `jet.raylib` namespace alias.
- `#269` — Centralize Core module canonicalization.
- `#270` — Unblock full suite hang in `dev_default_matches_compiled_binary`.

## Next Ungated Fixes

1. Reconcile D-S14-PAUSE for `let`/E0009 across parser, LSP, fix engine, editor docs, snapshots, and tests.
2. Prove or remove the internal `jet.raylib` alias.
3. Run current full suite and move `#9` to `verify` only if it passes.
4. Start `#211` with warning sweep and CI shard manifest.
