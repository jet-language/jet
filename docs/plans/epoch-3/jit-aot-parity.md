# JIT/AOT parity by construction — implementation plan (card #125)

Goal: one canonical typed IR after sema, consumed by both backends; a shared
runtime; a mechanical coverage gate; CI that makes divergence impossible to
ship. Jet competes on two fronts:

- **AOT wins the systems tier** — C, C++, Rust, Go, Zig, Odin: slower
  compile/install loop is acceptable when the payoff is better optimized native
  binaries, deployment performance, and expert control.
- **JIT wins the rapid-iteration tier** — JavaScript/TypeScript, Python, data
  analysis, web/app development, exploratory tooling: instant design/test/refine
  cycles matter, with some optimization left on the table by design.

`jet dev`/JIT is the fast design/build/refine tier; `jet build`/`jet run`
through AOT is the slower-to-compile, better-optimized release tier. Anything
that works through AOT must work through the dev tier in some backend path, with
the same observable behavior; backend coverage gaps are Jet's problem, not the
user's. AOT keeps emitting Rust. This plan is for an executing agent (Codex);
it changes no code by itself.

Everything in §1–§2 is **confirmed fact** (read from the code on 2026-07-06,
master @ a2861f18). §3 onward is **recommendation** unless marked otherwise.

---

## 1. Current architecture map (confirmed)

### 1.1 AOT path (`jet run` / `jet build`)

- Entry: `jet::compile_with_path` (`Source/lib.rs:71`) → `Loader` →
  `Sema::check_bundle` → `jet_codegen`.
- **The AOT backend is already TIR-only.** `emit_func`
  (`crates/jet-codegen/src/Codegen/Items.rs:1688`) gates on `TIR::tir_covers`,
  lowers via `TIR::lower_func`, emits via `TIR::emit_tir_func`; a gate miss is
  an ICE `panic!` (`Items.rs:1707`), never an AST fallback. The legacy AST
  emit path was deleted (c109). See `docs/spec/architecture.md:25-40` (R7).
  Methods/trait methods/test bodies/error-conv bodies route the same way
  (`Items.rs:1479-1539`, `Codegen/mod.rs:1064`).
- TIR lives in `crates/jet-codegen/src/Codegen/TIR/`:
  `mod.rs` (4037 lines — `TFunc`, `TStmt` ~30 variants, `TExprKind` ~120
  variants, `TBuiltinOp`, `THandleOp`, …), `subset.rs` (4558 — `tir_covers*`),
  `lower.rs` (7562 — AST→TIR), `emit.rs` (4008 — TIR→Rust text).
- TIR breadth: the variant set covers the whole language — structs, enums w/
  payloads, generics, lambdas/closures, pattern matching (`EnumMatch`,
  `MixedSwitch`, `RangeSwitch`), collections, serde (`JsonLit`, `DbValueLit`),
  http (`HttpClientMethod`, `HttpServerMethod`, `HttpRouterRegister`), UI/
  reactive/layout, `Transact`, `Unsafe`, extern/FFI (`ExternCall`), fan-out,
  math/SIMD, measurements.
- Runtime: **text preludes** embedded with `include_str!`
  (`crates/jet-codegen/src/Codegen/mod.rs:43,241-261`): `Prelude/Core.rs`
  (1488), `CoreLib.rs` (5781), `Scheduler.rs` (1257), `Ui.rs`, `UiGtk.rs`,
  `DevServer.rs`, `Mem.rs`, `Gc.rs`, `Layout.rs`. rustc compiles program +
  prelude text into the native binary.
- Rust-flavor baked into TIR: `TFunc.params` carry pre-mangled Rust names
  (`user_x`); `TFunc.generics` is a **rendered Rust clause string**
  (`TIR/mod.rs:285-289`), not structured data.

### 1.2 JIT path (`jet dev` default)

- CLI: `Source/CmdDevTools.rs:69,208,293` → `jet::Interpreter::dev_iteration`
  (`Source/Interpreter.rs:647`), `use_interpreter=false` by default (D-JIT2) →
  `dev_run_bundle` (`Source/Interpreter.rs:665`) wraps
  `jet_jit::CraneliftBackend::new(InterpreterBackend::new())`.
- Lowering input is the **same TIR**, via `TIR::lower_jit_program`
  (`TIR/mod.rs:109`), but restricted:
  - **entry module only** (`bundle.modules.get(bundle.entry)`) — any
    multi-module program lowers to `None`;
  - **generic functions skipped** (`!f.type_params.is_empty() → continue`,
    `mod.rs:126`); generic structs skipped (`mod.rs:135-137,160`);
  - output `JitProgram` (`mod.rs:74`) adds JIT-only metadata: spawn lambdas,
    `struct_fields`/`struct_field_types`/`enum_variants` maps.
- Gate: `jit_covers_*` (`crates/jet-jit/src/lib.rs:645` `jit_covers_expr`,
  `:900` `jit_covers_stmt`, `:1059` `jit_covers_program`, type predicates
  `:565-644`) — a hand-maintained structural whitelist over TIR ops **and**
  types. It is **all-or-nothing per program**: `try_resident`
  (`lib.rs:3568`) requires the whole program covered, else the **entire run**
  falls back to the tier-0 interpreter. There is no per-function or
  per-construct mixed execution.
- Value model: CLIF i64/f64 scalars plus **i64 handles into side heaps** on
  `JitRuntime` (`lib.rs:54-74`): `strings: Vec<String>`,
  `lists: Vec<Vec<i64>>` (Int elements only), `structs_f64: Vec<Vec<f64>>`
  (f64 fields only), channels/senders/tasks with i64 payloads. Host shims are
  `extern "C"` fns registered by symbol (`HostFns` `lib.rs:368` — arith traps,
  print family, string begin/push/eq/len/trim/upper/lower/replace,
  struct_new/get/set_f64; `Collections.rs` — 9 Int-list shims;
  `Concurrency.rs` — channel/spawn/join/taskgroup/select shims).
- **Shared-runtime precedent (the seed of this plan):** jet-jit's scheduler
  shims call `jet_codegen::scheduler::{JetSchedulerChannel, …}` —
  `crates/jet-codegen/src/lib.rs:10-27` compiles `Prelude/Scheduler.rs` into
  jet-codegen via `include!`, while the same file is `include_str!`-embedded
  into AOT binaries. One source, two compilations, real shared behavior.
  Scheduler is the only prelude with this dual life today.
- Trap model: no Rust unwind may cross a JIT frame (cranelift-jit 0.112.3
  emits no unwind tables — established I1 finding, card #125 log 2026-07-03).
  Instead: `JitRuntime.trapped` flag + `LowerCtx::emit_trap_check`
  (`lib.rs:1464`) branch-to-epilogue; `resident_invoke` converts to the same
  E0953 the interpreter reports.
- Worker threads see the runtime via a thread-local raw pointer
  (`ACTIVE_RUNTIME`, `crates/jet-jit/src/Concurrency.rs:11`).
- Hot-swap: resident `JITModule` + live heap in thread-locals
  (`lib.rs:44-49`); heap preserved on type-stable swap, reset on restart.

### 1.3 Interpreter / fallback path (tier-0)

- `Source/Interpreter.rs::run_checked` (`:581`): a pre-execution
  `boundary_scan` (`:122`) declines with E2201 on: `core.tasks/mem/files/env/
  process/random/time` imports (`native_module_feature` `:200`), `extern
  rust`, C modules, `#Unsafe`, typed CLI entry `fn run(args: T)` (`:168`),
  `&`-writeback args (`:325`). Then executes via `crate::Comptime::run_main` —
  the comptime tree-walker in `crates/jet-comptime/src/Comptime/`
  (`Interpreter.rs` 1524, `Methods.rs` 1959, `Builtins.rs` 582,
  `JsonInterp.rs` 436). This is a **third, independent semantic
  implementation** — it walks the AST, not the TIR.
- Comptime E0956/E0951 leaks are rewrapped as E2201
  (`dev_boundary_from_comptime`, `Source/Interpreter.rs:620`).
- Backend seam: `JitBackend` trait + `RunOutcome` in `jet-foundation`
  (`Source/JitBackend.rs` re-exports; `InterpreterBackend` there).
- `jet debug` steps this same interpreter (`debug_boundary_scan`, E2203).

### 1.4 Tests (current enforcement)

All in `tests/dev.rs` unless noted:

| Test | Line | What it proves | Gap |
|---|---|---|---|
| `interpreter_matches_compiled_binary` | 125 | tier-0 stdout == AOT stdout or named boundary | stdout only |
| `dev_default_matches_compiled_binary` | 215 | default backend (JIT+fallback) == AOT; card exit gate. Last: 125 ran / 132 boundary | stdout only |
| `interpreter_matches_expected_golden` | 294 | tier-0 vs `expected/*.out` | — |
| `cranelift_three_way_differential_battery` | 865 | JIT == interpreter == AOT on jit-covered stems w/ goldens (38 ran) | only covered stems |
| `jit_coverage_audit` | 798 | prints covered/gap lists. Last: **79 covered / 178 gaps** | `#[ignore]`d, informational, no ratchet |
| `jit_covers_implies_tir_lowers` | 900 | jit ⊆ tir | — |
| hot-swap / trap tests | 1010,1078 | resident state, trap-then-continue | — |

Corpus: 306 `.jet` files under `examples/features/` (topic dirs).
`tests/golden.rs` is the AOT executable spec (I5); it also greps generated
Rust for the bare word `unsafe` (trap when editing prelude text).
**Neither battery compares stderr or exit codes** (`dev.rs:184-188,269-274`).

---

## 2. Gap inventory (confirmed, categorized)

### 2.1 Core/TIR representation & consumption gaps

- G1 — `lower_jit_program` reads the **entry module only**; multi-module
  programs never lower for JIT (`TIR/mod.rs:110`).
- G2 — **No TIR monomorphization.** AOT emits generic Rust and lets rustc
  monomorphize; JIT skips generic fns/structs entirely (`mod.rs:126,135`).
- G3 — TIR carries Rust-rendered artifacts (`TFunc.generics` string,
  pre-mangled names). Consumable by the JIT today, but generic instantiation
  and any future non-Rust backend need structured data.
- G4 — All-or-nothing coverage: one uncovered construct anywhere sends the
  whole program to a different engine (`try_resident`, `lib.rs:3568`).

### 2.2 Backend lowering gaps (jit_covers ≪ tir_covers)

TIR has ~150 executable-node variants; the JIT lowers roughly: scalars,
string literals/interp + 5 string methods, Int-only lists (subset of
methods), f64-field flat structs, payload-less enums (discriminants),
numeric named tuples, if/loops/labels/inc-dec/compare-chains/short-circuit,
direct calls, print, spawn/channels/taskgroup-all-race-any/select with Int
payloads. Uncovered (the 178-gap tail): enum payloads, Optional/Result as
values, Map/Set/Deque, non-Int list elements, nested/String-field structs,
lambdas/closures/HOF/fn-values, `EnumMatch`/`MixedSwitch`/patterns, `Try`/
`OrFallback` general forms, serde (JSON/CSV/TOML/YAML, decode), text
(BigInt/Decimal/dates/unicode), crypto, io helpers (archive/compress/db/
log/path), http/net, ui/reactive/layout, `Transact`, `Unsafe`/mem, extern
FFI, fan-out, generics, module calls.

### 2.3 Runtime ABI / helper gaps

- G5 — The side-heap value model (`Vec<Vec<i64>>`, `Vec<Vec<f64>>`)
  **cannot represent** compound values in general (nested, mixed-type,
  String fields, enum payloads). It is a dead end for parity.
- G6 — No shared string/format/display code: `render_float`
  (`jet-jit/src/lib.rs:96`) hand-mirrors `JetShow for f64`; every Display/
  Debug behavior is re-derived per backend. CoreLib.rs (5781 lines) exists
  only as AOT text.
- G7 — No effects story for the JIT: files/env/time/random/process/net have
  zero host shims; those programs are pre-scan E2201 boundaries.
- G8 — Scheduler is shared (the good exception); its payloads are i64-only
  in the shims.

### 2.4 Test / enforcement gaps

- G9 — Batteries compare stdout only; stderr and exit codes unchecked.
- G10 — `jit_coverage_audit` is `#[ignore]`d and informational — coverage can
  silently regress.
- G11 — Coverage measured per example, not per TIR op; no machine-readable
  op table.
- G12 — Nothing forces a new language feature to add JIT lowering: new TIR
  variants compile fine with the whitelist defaulting to "uncovered". This is
  why every feature needs "separate JIT rediscovery" today.

---

## 3. Target architecture (recommendation)

### 3.1 Canonical Core/TIR contract

TIR stays where it is (`jet-codegen/src/Codegen/TIR/`) and remains the single
post-sema representation. Additions, not a rewrite:

- `lower_program_tir(bundle)` replaces `lower_jit_program`: lowers **all**
  modules' covered items (reuse the per-module `Cx` the AOT path already
  builds), still producing `JitProgram`-style metadata.
- A TIR **monomorphization pass** (`instantiate.rs`): given call sites with
  concrete type args (sema already resolves them), produce concrete `TFunc`
  clones for the JIT. AOT keeps emitting generic Rust (rustc monomorphizes) —
  no golden churn.
- `TFunc` gains structured generic info (param names + bounds) alongside the
  rendered string; the string stays for emit.rs.
- Contract doc: a short section in `docs/spec/architecture.md` defining TIR
  totality + the two-consumer rule (every new variant must be handled by both
  emitters — see 3.5).

### 3.2 Runtime ABI contract — `crates/jet-rt`

Generalize the Scheduler.rs dual-compile pattern:

- New workspace crate `jet-rt` (internal code only — **no external deps, no
  I6 issue**; cranelift stays in jet-jit per D-JITDEP1).
- Runtime sources move (incrementally, file by file) to a form that compiles
  both ways: `include!`d into `jet-rt` as a real linked crate (JIT host
  calls) and `include_str!`-embedded text for AOT emission (byte-identical to
  today's prelude text → goldens unchanged). Order: strings/format/JetShow →
  collections → scheduler (relocate the existing dual-compile) → serde/text →
  io/effects.
- **Shim rule (I1-critical):** every JIT-callable entry point is an
  `extern "C"` shim that (a) wraps its body in `catch_unwind` so no Rust
  panic ever unwinds into a JIT frame, (b) converts panics/traps to the
  existing `trapped`-flag mechanism, (c) uses the same diagnostic text AOT
  binaries print.
- Threading: keep the `ACTIVE_RUNTIME` pattern but move it into jet-rt so
  scheduler workers and shims share one access path.

### 3.3 Value ABI — `JetVal`

Replace i64 side-heap handles with a uniform runtime value:

- Unboxed CLIF i64/f64/i8 for scalars (as today, fast path preserved).
- One pointer-sized handle for everything compound: String, List<T>, Map,
  Set, struct/enum instances (field vec + discriminant + payload), tuples,
  Optional/Result, closures (fn ptr + captures), task/channel handles.
  Implemented in jet-rt (think `CtValue`'s shape, but owned by the runtime
  and shared-by-construction with the prelude's semantics).
- Per-run arena ownership: allocations reset by `reset_run_heap` between
  runs (existing model), preserved across type-stable hot-swap (existing
  rules). No GC needed for the dev tier; document the arena lifetime.

### 3.4 AOT emitter contract

Unchanged in role: `emit.rs` pattern-matches TIR and formats Rust, zero
decisions, exhaustive match, gate miss = ICE. Only additions: keep emitting
from the same prelude text jet-rt now owns.

### 3.5 JIT emitter contract + coverage gate ("parity by construction")

- `LowerCtx::lower_expr`/`lower_stmt` become **exhaustive matches over
  `TExprKind`/`TStmt` with no `_` wildcard**. Every arm returns
  `Lowered(Value)` or `Err(Unsupported { op: &'static str, reason })`.
  Adding a TIR variant then **fails the jet-jit build** until someone writes
  the arm (real lowering or an explicit `Unsupported`). That is the
  permanent invariant the owner asked for: parity is enforced by the
  compiler, not by agent diligence.
- Delete the `jit_covers_*` whitelist. The gate becomes **try-compile**:
  attempt full lowering; on the first `Unsupported`, record the op name and
  fall back. (`try_compile_bundle` at `lib.rs:3622` is the seed.)
- **Gap manifest**: a committed file (`tests/jit_gaps.toml` or `.txt`)
  listing every op still `Unsupported`, with reason, plus every example stem
  allowed to fall back. CI fails on any unlisted gap and on manifest growth
  (ratchet — it only shrinks, or grows only with an in-diff justification).

### 3.6 Unsupported-feature diagnostic policy

While gaps remain: fallback runs the program on the next tier automatically.
The user-facing contract is reproducibility, not "every construct must be
native-compiled by Cranelift today." `jet dev` may show terse dev-status text
when it chooses a slower tier, but no source change, flag, or workaround is
required. E2201 survives only for programs no tier can execute.

Fallback ladder:

1. JIT-native execution when covered and safe for the resident process.
2. Shared-runtime host shims for effects and platform helpers when those can
   preserve resident dev semantics.
3. AOT subprocess fallback for native/foreign/platform cases where in-process
   JIT hosting would risk safety, process integrity, or semantic drift.

The observable contract for every AOT-runnable program is identical stdout,
stderr, exit code, diagnostics, panics, and side effects under `jet dev` and
the AOT path. Speed may differ; semantics must not. JIT may intentionally skip
AOT-grade optimization to preserve edit-run latency; AOT may intentionally spend
more compile time to beat systems-language release performance.

---

## 4. Migration plan (thin vertical slices; dev loop stays working throughout)

Every phase: targeted tests while iterating, full suite before claiming done,
Tower log entry per slice. Owner policy is now settled in §7; no parity ballot
blocks Phase 0+ work.

**Phase 0 — Baseline + measurement (no behavior change)**
- Files: `tests/dev.rs`, new `tests/jit_gaps.*` manifest.
- Add stderr + exit-code comparison to both batteries (G9). Expect a
  triage pass: some examples may legitimately differ today — each divergence
  found is a real P0-class bug to fix or an honest boundary to record.
- De-`#[ignore]` `jit_coverage_audit` into a ratchet against a committed
  baseline (G10); add a per-TIR-op coverage report (G11).
- Accept: full suite green; baseline files committed; any stderr/exit-code
  divergences fixed or manifested.

**Phase 1 — `crates/jet-rt` + strings/format/display**
- Files: new `crates/jet-rt/`, `crates/jet-codegen/src/lib.rs` (dual-include
  seam), `crates/jet-codegen/src/Prelude/CoreLib.rs`/`Core.rs` (split the
  string/JetShow portions into dual-compilable files — **AOT emitted text
  must stay byte-identical**; goldens are the proof), `crates/jet-jit`
  (string shims re-route to jet-rt impls; delete `render_float` in favor of
  the shared `JetShow`).
- Traps to respect: prelude is `include_str!`-embedded → rebuild `jet` before
  smoke tests; `tests/golden.rs` greps the bare word "unsafe" in generated
  Rust — keep it out of moved comments.
- Accept: goldens byte-identical; dev/three-way batteries green; shim rule
  (catch_unwind + trap flag) applied to every new entry point; conformance
  unit tests in jet-rt.

**Phase 2 — `JetVal` value model (replaces side heaps), migrated in slices**
- Files: `crates/jet-rt` (value types, arena), `crates/jet-jit/src/lib.rs`
  (LowerCtx value handling), `Collections.rs`/`Concurrency.rs` (shims).
- Slice order: String handles → lists of any element type → structs with
  arbitrary (incl. nested/String) fields → enums with payloads →
  Optional/Result as values → tuples/maps/sets. Each slice widens lowering,
  adds a `jit_covers_*`-style regression test, and moves stems out of the
  gap manifest.
- Hot-swap/trap tests must stay green each slice (`cranelift_hot_swap_*`,
  `cranelift_trap_then_hot_swap_continues`).
- Accept per slice: three-way battery grows; audit ratchet improves; no
  regression in `dev_default_matches_compiled_binary` counts.

**Phase 3 — Exhaustive-match gate + gap manifest (kill `jit_covers`)**
- Files: `crates/jet-jit/src/lib.rs` (delete `jit_covers_*` whitelist
  ~600 lines; make `lower_expr`/`lower_stmt` exhaustive, wildcard-free, every
  arm Lowered/Unsupported), `tests/dev.rs` (gate tests switch to
  try-compile + manifest assertions).
- This phase is where "each feature needs separate JIT rediscovery" dies:
  after it, an unhandled variant is a compile error, not a silent gap.
- Accept: `jit_covers_implies_tir_lowers` replaced by manifest test; grep
  proves no `_ =>` in the two lowering matches; coverage numbers unchanged
  or better.

**Phase 4 — Whole-bundle lowering + TIR monomorphization**
- Files: `TIR/mod.rs` (`lower_program_tir` over all modules; structured
  generics on `TFunc`), new `TIR/instantiate.rs`, `crates/jet-jit`
  (multi-module symbol naming — reuse the existing `alias__fn` mangle).
- Accept: multi-module examples (`modules/*`) and generic examples
  (`types/generic_*`) leave the manifest; AOT goldens untouched.

**Phase 5 — Stdlib subsystems through jet-rt (bulk coverage)**
- One PR per subsystem, each moving prelude code to dual-compile + adding
  shims + lowering for the corresponding TIR ops (`CoreCall`,
  `BuiltinMethod`, `NumericMethod`, `ClosureMethod`, serde/text/crypto/io
  variants): collections methods → serde (JSON/CSV/TOML/YAML/decode) → text
  (BigInt/Decimal/dates/unicode) → crypto → io helpers.
- Accept per subsystem: its example stems leave the manifest; three-way
  battery covers them.

**Phase 6 — Effects + final fallback ladder**
- `jet dev` executes real ambient effects through jet-rt shims
  (files/env/time/random/process) where resident execution is correct, and the
  last-resort fallback for anything not safely JIT-hosted becomes
  **compile-and-run via the AOT path** (correct but slower, no hot-reload),
  replacing interpreter-E2201 for runnable programs.
  `Source/Interpreter.rs::boundary_scan` shrinks accordingly; the interpreter
  remains for comptime + `jet debug`.
- Accept: `dev_default_matches_compiled_binary` boundary count collapses to
  the manifest's permanent entries only.

**Phase 7 — Concurrency payloads, net/http**
- Generalize scheduler shim payloads from i64 to `JetVal`; http/net through
  jet-rt when resident-safe, otherwise transparent AOT fallback.

**Phase 8 — UI/web/reactive**
- Per-surface implementation choice: JIT lowering, existing resident dev-server
  runtime (`DevServer.rs`), or transparent AOT-subprocess fallback. The user
  gets one `jet dev` behavior contract either way: same program semantics as
  AOT, faster when the resident path can host it.

**Phase 9 — FFI/`#Unsafe` policy**
- Extern-rust/C/foreign-library programs use **AOT-subprocess fallback** unless
  a future audited host proves in-process execution is equally safe. The JIT
  must not load crash-prone foreign code into the resident process just to claim
  coverage. This is still full parity: the program runs under `jet dev` with
  AOT-identical behavior; it may just run in the slower fallback tier.

**Phase 10 — Permanence**
- `docs/spec/architecture.md`: new rule (suggest **R12 — Two consumers, one
  IR**): every executable TIR variant is handled by both the Rust emitter and
  the CLIF lowerer, exhaustively, wildcard-free; a new feature PR is
  incomplete without both arms + example + golden + battery inclusion.
- AGENTS.md workflow loop gains the same line. Card #125 closes here.

---

## 5. CI / test plan

- **Parity batteries** (exist, extend): `interpreter_matches_compiled_binary`
  + `dev_default_matches_compiled_binary` compare stdout + stderr + exit code
  (Phase 0). Keep the honest-boundary assertion (no silent skips).
- **Three-way battery** (exists): grows automatically as the gate widens; keep
  the `ran >= N` floor ratcheting upward per phase.
- **Coverage ratchet** (new, Phase 0): `jit_coverage_audit` enforced against
  a committed baseline count + stem list; fails on regression; must be
  updated (only shrinking gaps) when coverage grows.
- **Gap manifest test** (new, Phase 3): every `Unsupported` op and every
  fallback stem must be listed with a reason; unlisted → fail; listed-but-
  now-covered → fail (stale entry).
- **TIR-op coverage table** (new, Phase 0/3): machine-readable report of
  TExprKind/TStmt variant → {aot: always, jit: lowered|unsupported(reason)};
  after Phase 3 this is generated from the exhaustive match itself.
- **Runtime ABI conformance** (new, Phase 1+): jet-rt unit tests run against
  the linked copy; AOT text-side behavior is already proven by
  `tests/golden.rs` + examples (same source text). Shim-boundary tests: every
  shim survives an induced panic (catch_unwind → trap flag, process alive) —
  extend `cranelift_trap_then_hot_swap_continues`.
- **How a new language feature proves parity** (the permanent rule): adding a
  TIR variant without a CLIF arm fails to compile (Phase 3); adding an
  example without a golden fails I5 (`tests/golden.rs`); the batteries then
  enforce byte parity automatically. No new per-feature JIT test is required
  beyond a regression test when the lowering is non-trivial.
- Existing traps to respect: run suites via `nix develop -c`, serialized;
  check `df -h /tmp` on weird failures; never trust a subagent's green.

## 6. Tower plan

- **#125 stays the umbrella card** (phase: building). Rewrite its `plan`
  field to point at this document + the phase list; log a `[handoff]` entry
  noting the plan location and the 2026-07-06 owner direction in §7.
- **New subcards** (agent lane, workOrder after #125): 
  1. `jet-rt extraction + strings/display` (Phase 1)
  2. `JIT JetVal value model` (Phase 2)
  3. `exhaustive lowering gate + gap manifest` (Phase 3)
  4. `whole-bundle + TIR monomorphization` (Phase 4)
  5. one card per Phase-5 subsystem (collections, serde, text, crypto, io)
  6. `dev effects + fallback ladder` (Phase 6/7)
  7. `FFI/unsafe dev parity fallback` (Phase 9)
  8. `parity permanence: R12 + docs` (Phase 10)
  All writes via the tower CLI with `--by`/`--expect-rev`; never hand-edit
  `.tower/tower.json`.
- **Done means**: `dev_default_matches_compiled_binary` shows every example
  either byte-identical (stdout+stderr+exit) with the AOT binary or listed in
  the committed manifest with an implementation-host reason under the §7
  parity contract; the manifest contains only transparent internal fallback
  boundaries; no `jit_covers` whitelist exists; R12 is in architecture.md;
  full suite green, verified by a separate jet-verify pass.

## 7. Owner direction and remaining decisions

### Owner direction recorded 2026-07-06

Jet must replace both systems languages and rapid-iteration dynamic stacks.
The AOT compiler is how Jet competes with and beats C, C++, Rust, Go, Zig, and
Odin: native binaries, strong optimization, safety by default, expert control
behind explicit gates. The JIT/dev tier is how Jet competes with and beats
JavaScript/TypeScript, Python, data-analysis notebooks, web dev loops, and other
interactive scripting stacks: fast edit-run feedback, resident state where safe,
and no release-build wait while designing.

`jet dev`/JIT exists for rapid design, testing, and refinement. AOT exists for
slower compile time in exchange for better release optimization and binary
performance. A program developed under `jet dev` must be 100% reproducible when
built through AOT: same behavior, diagnostics, stdout, stderr, exit code,
panics, and side effects. Anything possible through AOT must be possible under
`jet dev` in some backend path. If JIT-native execution cannot host a feature
yet, Jet falls back internally (shared host shim, resident platform runtime, or
AOT subprocess). The user does not carry the backend gap.

The intended user story: build/test/design/refine at JIT speed, then compile
the final product through AOT and get the same program with stronger release
optimization. JIT is not a second language, a subset, or a toy interpreter. AOT
is not a behavior-changing release compiler. They are two performance tiers for
one semantic contract.

This answers the previously suspected owner gates:

- effectful programs run for real under `jet dev`; E2201 is not an acceptable
  long-term boundary for AOT-runnable programs;
- FFI/`#Unsafe`/native-platform features do not need in-process JIT execution
  to satisfy parity; transparent AOT-subprocess fallback is valid when it is the
  correct safety/reproducibility host;
- full parity means behavior parity with a performance-tier distinction, not
  mandatory Cranelift lowering for every feature.

No open owner ballot remains for the parity contract. New ballots are needed
only if implementation discovers a user-visible choice not covered by this
contract: new syntax, new external dependency, an invariant carve-out, or a
proposed permanent user-visible limitation.

Agent-decidable (record in card log): jet-rt crate creation (internal code, no
I6 surface), JetVal representation, gate mechanics, manifest format, exact
fallback-status wording.

### Risks

- **Rust-emitting AOT stays** → TIR keeps some Rust flavor (mangled names,
  rendered generics) and can drift Rust-ward. Mitigation: the R12 contract +
  structured-generics addition; de-Rust-ify a field only when it blocks the
  JIT (no big-bang IR cleanup).
- **Prelude split churn**: moving CoreLib text into dual-compile files risks
  byte-diffs in generated Rust (golden breakage) and the golden "unsafe"
  substring grep. Mitigation: byte-identical emission is an explicit
  acceptance criterion per Phase-1/5 slice.
- **Panic safety at the shim boundary**: any rt code that panics under a JIT
  frame is UB territory. Mitigation: the shim rule (catch_unwind inside the
  shim, trap flag) is a stated ABI contract with its own test.
- **Effects in-process + resident state**: real file/env effects run inside
  the dev server; per-run arena reset must also reset/close effect resources
  (open files, sockets). Design the rt resource table with run-scoped
  cleanup from day one.
- **Tasks/channels + JetVal**: scheduler payloads move from i64 to handles;
  cross-thread handle access needs the arena to be thread-safe or
  channel-transfer to deep-copy (the AOT semantics decide — copy what the
  compiled program copies).
- **Hot-swap type-stability**: the new value model must preserve the current
  "type-stable edit keeps heap" rules; regression tests exist and must stay
  green every slice.
- **Interpreter divergence during migration**: until Phase 6, the interpreter
  remains a live tier; every slice that widens JIT coverage removes
  interpreter exposure, never adds interpreter features (stop investing in
  tree-walker coverage now).

---

## Executor Checklist (ordered; hand to Codex as-is)

1. Read `.agents/prompts/OrchestrationPrompt.md` first and run as the
   orchestrating agent described there. Then read this doc,
   `docs/spec/architecture.md`, card #125 (`.tower/tower.json`, card num 125),
   `crates/jet-codegen/src/Codegen/TIR/mod.rs` header,
   `crates/jet-jit/src/lib.rs` header.
2. Rewrite #125 `plan` to reference this doc + phases; create the §6 subcards
   with blockedBy edges; set workOrder.
3. Phase 0: add stderr+exit-code comparison to both batteries in
   `tests/dev.rs`; triage divergences (fix or manifest); de-ignore
   `jit_coverage_audit` into a committed ratchet baseline; add the TIR-op
   coverage report. Full suite green; commit.
4. Phase 1: create `crates/jet-rt`; move string/format/JetShow prelude code to
   dual-compile (byte-identical AOT text — goldens prove it); re-route jet-jit
   string shims through jet-rt with the catch_unwind shim rule; delete
   `render_float` duplication. Targeted tests then full suite; commit.
5. Phase 2 slices (one commit each, ratchet must improve): JetVal String →
   lists (any elem) → structs (any fields) → enums (payloads) →
   Optional/Result → tuples/maps/sets. Keep hot-swap + trap tests green.
6. Phase 3: delete `jit_covers_*`; exhaustive wildcard-free `lower_expr`/
   `lower_stmt` returning Lowered/Unsupported; try-compile gate; committed
   gap manifest + manifest tests; regenerate op table from the match.
7. Phase 4: `lower_program_tir` over all modules; structured generics on
   `TFunc`; `TIR/instantiate.rs` monomorphization; multi-module + generic
   stems leave the manifest.
8. Phase 5: subsystem PRs (collections → serde → text → crypto → io), each
   moving prelude code to jet-rt + lowering the corresponding TIR ops.
9. Phase 6/7: effect shims in jet-rt; AOT-subprocess fallback
    replaces interpreter-E2201 for runnable programs; shrink
    `boundary_scan`; generalize scheduler payloads; net/http.
10. Phase 8: UI/web/reactive — lower or record transparent fallback entries.
11. Phase 9: FFI/unsafe transparent AOT fallback entries + dev-status text.
12. Phase 10: add R12 to `docs/spec/architecture.md` + AGENTS.md workflow
    line; final full-suite + jet-verify pass; close #125 with the §6 done
    definition; delete this plan's completed phases from the doc or mark the
    doc historical.

Verification contract for every step: targeted `nix develop -c cargo test
--test dev <name>` while iterating; full `nix develop -c cargo test` before
any card/phase claim; rebuild before smoke-testing (`target/debug/jet` is
what the dev-shell `jet` execs); check `/tmp` before trusting odd failures.
