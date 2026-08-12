# JIT run-tier vs AOT behavioral parity audit — 2026-07-28

Method: every `examples/features/expected/**/*.out` (the AOT-verified golden truth channel,
`tests/golden.rs` `jet run --release`) re-run under default `jet run` (Cranelift tier with
interpreter deopt), stdout diffed. 367 comparable examples; display-dependent skips: 2
(`game/raylib_window`, `ui/ui_native_linux`).

Result: **344 pass, 23 fail.** Owner law D-VERDICT-687-1 / D-LENS-RUN1 ("never a
supported-feature gap; byte-identical output") is violated on every row below.

## Why existing enforcement is green anyway

1. `tests/jit_corpus_gate.txt` (`deopt_interp:` section) **commits run-tier E0956 failures as
   an accepted class** — `RunOutcome::Problems` in `tests/dev.rs::classify_corpus_stem` records
   the diagnostic into the manifest instead of failing. `ui/events: E0956 core.event.scope()`
   is literally checked in as acceptable.
2. Four `comptime/embed*`/`find*` examples hide in the gate's `frontend_rejected:` class —
   they fail sema in the gate's `CompileMode::Run` path yet pass `jet run --release`.
3. `tests/jit_gaps.txt` ratchets *compile* coverage only (now zero gaps) — compiled ≠ runs.
4. `tests/parity.rs` scopes the interpreter inventory to the *pure comptime* contract; its 347
   "Boundary" exemptions predate the interpreter becoming the runtime deopt tier (D-ONECORE1).

## Failures — E0956 runtime-tier gaps (works under `jet build`/`--release`, dies under `jet run`)

| Example | First error |
|---|---|
| collections/dynamic-array-view | split views can't run at compile time yet |
| comptime/embed | call `embed_file` can't run at compile time yet |
| comptime/embed_bytes | call `embed_bytes` can't run at compile time yet |
| comptime/find | call `find` can't run at compile time yet |
| comptime/find_empty | call `find` can't run at compile time yet |
| crypto/crypto_suite | `core.crypto.sha512_bytes()` at comptime (#1254 repro) |
| io/args_audit | `core.args.spec()` at comptime |
| io/args_spec | `core.args.spec()` at comptime |
| io/db | `core.db.open_memory()` at comptime |
| io/dir_entry | `core.files.list_dir()` at comptime (impure tier) |
| lowlevel/ffi | expr `ExternCall` |
| lowlevel/inline_asm | expr `ExternCall` |
| lowlevel/inline_c | expr `ExternCall` |
| types/generic_constructor_inference | static `Box.new` (#1254 repro) |
| ui/events | `core.event.scope()` at comptime |

## Failures — other divergences (triaged 2026-08-12, card #1648)

Every row was re-probed on 2026-08-12 (`target-audit-e3/divergence-triage.md`,
board audit #1869). Rows route to cards, not to a ledger.

| Example | 2026-07-28 symptom | 2026-08-12 result | Resolution |
|---|---|---|---|
| types/dimensional_quantities | E0112 under run only | changed shape: E2201 under `jet run`, codegen ICE under `--release` | card #1930 |
| concurrency/deadline_context | exit 70 under run | both tiers fail E1004 `core.tasks` has no `spawn`; fixed by the canonical spawn lowering (e892acc21) riding the bucket-1 integration (card #1929) | rides #1929 |
| concurrency/parallel_scan | stdout diff | IDENTICAL, matches golden | resolved |
| io/terminal_parity | stdout diff (tty-sensitive) | genuine tier divergence: secret-read error and stream ordering | card #1931 |
| memory/returned_views | stdout diff | run matches golden; `--release` ICEs on five generated-Rust E0308 | card #1932 |
| web/web_wasm_callback, web/web_wasm_list, web/web_wasm_list_string | stdout diff | IDENTICAL, match goldens | resolved |

## Policy fix

Tighten the existing c727 gate (`tests/dev.rs`): a `DeoptInterp` record carrying an error
diagnostic for an example whose AOT run is green becomes a **failure**, tolerated only via a
shrink-only burndown section (same D-LENS-RUN2 ratchet semantics as `jit_gaps.txt`). The
`frontend_rejected` class needs the same AOT cross-check. E0956's comptime wording must never
surface from the runtime tier (#1254's diagnostic slice). No new parallel gate (I8).

Raw run log: reproduce with default `jet run` per example; this doc is the curated result.
