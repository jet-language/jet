# Epoch 2 — status (single source)

The one live status file for Epoch 2. High-level state first, per-milestone
deferrals at the bottom. Branch is `master`; check git history + `cargo test`
over prose.

> **Epoch 2 officially wrapped — 2026-06-19.** The 18 per-milestone plan files
> (`m1-…`–`m18-…`) were removed; their decisions live in `syntax-decisions.md`,
> behavior in `spec.md` + `examples/`, errors in `diagnostics.md`, and this file is
> the durable record (`README.md` keeps the consolidated overview + dependency
> order). Every remaining loose end is tracked as an Epoch-3 card in the dashboard
> board (`tools/pipeline/board.json`); see the "Moved to Epoch 3" list below.

**Date:** 2026-06-18. **Verdict: Epoch 2 GA complete.** All 18 milestones have
landed on `master`, and the two real remaining language gaps closed this session:
the **Jet module system** (D-MOD1–4) and a functional **`jet bind`** (native
std-only backend). The owner re-scoped the long-pole/nicety items out of the E2
GA bar: full debugger → E3, adoption docs → E3, jetos → post-E3, **package
build-from-source + M9 wave-2 → E3**, and **M11 property testing / doctests /
coverage → E3** (all syntax-gated or ergonomics, not core language). Full suite
green; the diagnostic gaps and the test-suite flake are fixed. Nothing in the E2
GA scope remains open.

## Test health

Green: build clean, 0 failures across every test binary, including the five new
diagnostic tests (the 2026-06-18 full run was interrupted by a concurrent
re-bless run only at `showcase.rs`, which these changes don't touch). The
`tests/pure.rs` `store_*` flake (three tests raced on the process-global
`JET_STORE_DIR`) is fixed by routing them through a `with_store` helper guarded
by `STORE_LOCK` (mirrors `tests/pkg.rs`); that fix had two clean full runs.

> Gate CI on cargo's own exit code, not a piped `tail` — a `cargo test | tail`
> pipe returns tail's status and masks failures.

## Milestone status

| M | Title | Status | Gap if not done |
|---|---|---|---|
| M1 | Concurrency | ✅ | — |
| M2 | Release policy/editions | ✅ | edition-in-manifest rides the pack.jet migration |
| M3 | DX CLI | ✅ | one gated digit-sep example pending D-SUGAR1 (ballot status unresolved in the m3 plan) |
| M4 | `jet dev` | ✅ | — |
| M5 | Tier-2 references | ✅ | arenas (D-REF2) deferred by design |
| M6 | Library authoring | ✅ | D-ERR1 error carrier still `String` (unratified) |
| M7 | Streaming I/O | ✅ | — |
| M8 | Packages/supply-chain | ✅* | live registry upload + signed cache = design-only |
| M9 | First-party libraries | **wave 1 (E2); wave 2 → E3** | regex/archive/db moved to E3 with package-build-from-source (owner, 2026-06-18); wave-1 rings are compiler-known modules |
| M10 | Networking/services | ✅ | — |
| M11 | Testing/docs/bench | ✅ (core); niceties → E3 | `test`/snapshot/`todo`/`jet bench` shipped; property testing + doctests + coverage → E3 (syntax-gated, owner 2026-06-18) |
| M12 | Debug/observe | ✅ | `?` error-traces (E3002) now operational; DAP step-through → Epoch 3 |
| M13 | Low-level tier | ✅ | — |
| M14 | C FFI | ✅ | `jet bind` functional via native std-only backend (`src/cbind.rs`); auto-invoke-on-cache-miss → E3 |
| M15 | Freestanding/cross | ✅ | E3303 now operational; real QEMU CI stays doc-only (D-CROSS3) |
| M16 | Pure eval / layer 3 | ✅ | E3402/E3403 now operational; E3401 trace is single-level (transitive deferred) |
| M17 | Epoch 2 GA | ✅ | suite green; showcases + size budgets + GA checklist in; DAP debugger + adoption docs → Epoch 3 |
| M18 | REPL | ✅ | move-across-lines, `:run`, `--project` deferred |

\*M8 publish/version/resolve works; live git-registry upload and the signed cache
are designed, not shipped.

## Done 2026-06-18

- **Suite flake fixed** (see Test health).
- **Five diagnostics made operational** (were registered-but-unreachable; owner
  directive). Each fires from real source with a test:
  - **E3202** — `Ptr<T>` by value in a C FFI signature. `check_c_module`
    redirects pointer types off E3203. `tests/ui/cffi_e3202_ptr_boundary`.
  - **E3303** — List/Map literal under `--freestanding` (String exempt, so the
    freestanding examples still compile). Guards in `infer_list_lit`/
    `infer_map_lit`. `tests/ui/freestanding_e3303`.
  - **E3002** — `?` propagating an error. `jet_trace_err` prelude helper
    (debug-only) wraps each `?` in codegen. `tests/observe.rs::error_return_trace_frames`.
  - **E3403** — time/random inside a `pure fn` (covers `jet eval --pure`, which
    requires every fn to be pure). `in_pure` Checker flag +
    `is_nondeterministic_std` guard in `infer_std_call`. `tests/ui/pure_e3403`.
  - **E3402** — ambient-I/O builtin in a sandboxed module-field/build eval.
    `check_build_io` before `comptime::evaluate` in `modeval`.
    `modeval::ambient_io_in_build_is_e3402`. (fs/net names included for the
    future build-from-source path; print/eprint/input make it reachable today.)
- **Docs reconciled** — ballots pruned to the four open module-system decisions
  (D-MOD1–4); jetos + ratified Epoch-2/REPL ballots left the queue. Roadmap
  reflects the re-scoping.
- **Jet module system implemented** (D-MOD1–4, ratified 2026-06-18: Rust's model,
  keyword `module` + `.` scoping, Rust-exact `pub use` re-export). The scaffolding
  on `master` was half-built — inline-module bodies were *not* type-checked (a
  type error leaked to rustc, an I2 violation), inline sibling calls emitted
  undefined names, `use alias.Item` for *file* modules hit a pass-ordering bug,
  inline visibility was unenforced, and `pub use` re-export didn't resolve.
  All fixed: sema rewrites inline sibling calls to mangled names + checks their
  bodies; the unqualified-import pass runs after file aliases register; a
  `reexports` map carries `pub use` through sema and codegen (with correct
  borrow/move conventions). Executable spec: `examples/features/42`–`49`; ui
  snapshots `module_inline_private` (E0609) and `module_inline_type_error`
  (E0113). This closes "cross-file language walls v1".

## Remaining Epoch 2 work

**None.** Everything in the E2 GA scope has landed. `jet bind` (M14) was the last
in-scope implementation item — done 2026-06-18 via a native std-only parser
(`src/cbind.rs`, supersedes D-CBIND3=B's bindgen route): `jet bind <header.h>
[--pkg lib] [-o out]` translates C function prototypes (scalars, `char*`→String,
`void`) into a `@bindgen` cache at `.jet/bindings/c/<lib>.jet`; unbindable
declarations are skipped + reported (never faked); E3208 is the honest
parse-failure path; e2e `tests/cffi.rs::jet_bind_native_backend_end_to_end`.

**Moved to Epoch 3** (owner, 2026-06-18):
- **Package build-from-source + M9 wave-2** (`jet.regex`/`jet.archive`/`jet.db`).
  The compiler is fine (`jet build`/`run` compile jet → Rust → `rustc` → binary,
  I2/I3); the missing piece is the jetpack step that compiles a dependency *from
  its Jet source* in `provider.rs::realize()`, on top of which wave-2 ships as
  real packages. → [`../epoch-3/package-build-from-source.md`](../epoch-3/package-build-from-source.md).
- **M11 property testing + doctests + coverage** — syntax-gated ergonomics, not
  core language. → [`../epoch-3/testing-docs-ergonomics.md`](../epoch-3/testing-docs-ergonomics.md).
- DAP / full source-level debugger; adoption documentation; jetos surface/platform
  (post-E3).

The module system (D-MOD1–4) is ratified and implemented, so "cross-file language
walls v1" are resolved and there are no open ballots.

## Per-milestone deferrals (durable detail)

Only items still owed; resolved ones are dropped. The diagnostics listed as
deferred in earlier drafts (E3002/E3303/E3402/E3403/E3202) are now done (above).

- **M8** — live git-registry upload is validated locally + explains the push
  path; signed binary/source cache is design-only (out of scope per plan).
- **M9** — wave-2 (regex/archive/db) **moved to Epoch 3** (owner, 2026-06-18)
  together with package-build-from-source; see
  `../epoch-3/package-build-from-source.md`. Wave-1 rings are compiler-known
  modules. `jet.db`/sqlite also waits on the `jet bind` backend.
- **M11** — testing core (`test`/snapshot/`todo`/`jet bench`) shipped. Doctests
  (need a `///` doc-comment convention), coverage output, and property testing
  (need a property-test surface syntax + shrinking design) all **moved to E3** —
  syntax-gated, owner 2026-06-18. → `../epoch-3/testing-docs-ergonomics.md`.
  `jet doc` rendered docs stay in E3 with adoption docs.
- **M12** — DAP step-through + GDB/LLDB integration → Epoch 3. E3001 rich panics
  and E3002 `?` traces are in.
- **M14** — `jet bind` is done (native std-only backend, `src/cbind.rs`).
  Compile-time auto-invoke on cache miss and Phase-3 cache hash-invalidation are
  deferred to E3 (see `../epoch-3/c-header-bindings.md`). E3202 is reachable.
- **M15** — E3303 done. E3303-via-allocator-config and a real aarch64/QEMU CI run
  stay doc-only (D-CROSS3 makes the local QEMU harness the minimum; see
  `docs/embedded.md`).
- **M16** — E3402/E3403 done. E3401's call-trace shows one level (the direct
  impure callee); transitive chains deferred. `jet eval --pure` rich rendering of
  struct/list returns deferred.
- **M17** — DAP debugger + a single adoption-story doc → Epoch 3. Showcase set,
  size budgets, and the GA checklist (`tests/ga.rs`) are in.
- **M18** — move-semantics across REPL inputs, `:run` (compile session to temp
  file), and `--project` manifest mode deferred.
