# c160 — Compiler internal seams refactor (D-COMPILERLIB1=A)

**Decision:** D-COMPILERLIB1=A **ratified 2026-06-25** (Epoch 3). Factor `Source/`
into internal Rust library seams — `lexer` / `parser` / `sema` / `tir` / `codegen` /
`comptime` + a `driver` that composes them — each owning its types behind a small
documented API, with today's coarse `lib.rs` (`check_*` / `compile_*` / `render_*`)
kept as a **thin façade** built on the seams.
**Gate:** none. I6-safe — these are the compiler's OWN internal crates/modules, no
external deps, no carve-out. Owner-Q3 (exact seam boundaries) left *confirmable*; the
ratified decision already names the seams, so they are baked below (§2), not blocked.
**Scope:** Epoch 3 refactor. No user-facing syntax, no behavior change → no ballot.

---

## Why

Two costs the seams remove:

1. **Forked pipeline.** The LSP re-derives the compile pipeline instead of calling each
   stage as a library. A documented per-seam API lets the build driver *and* the LSP
   drive lex → parse → sema → tir → codegen as composed library calls, and inspect the
   typed IR between sema and codegen, off one path.
2. **Self-host boundaries.** Crisp crate-by-crate seams give the future self-host port a
   clean unit of work each (rustc / Roslyn precedent), instead of one monolith to port
   big-bang.

This is the *merits* framing the owner ratified — LSP/driver unification + self-host
boundaries + I6-machine-safety. It is **not** a "reduce big files" refactor; line-count
reduction is incidental.

---

## 1. Current reality (verified 2026-06-25, file:line)

The coarse-grained module split that an earlier draft of this card proposed is **already
done** — it is the foundation this seam work builds on, not pending work:

- `Source/Codegen/TIR.rs` is now the directory `Source/Codegen/TIR/`:
  `mod.rs` (3,180 — TIR node types + totality contract), `lower.rs` (4,714 — AST→TIR
  lowering), `emit.rs` (2,032 — `emit_tir_func` at `emit.rs:12`, TIR→Rust), `subset.rs`
  (3,460 — the coverage predicate family: `tir_covers` at `subset.rs:22`,
  `tir_covers_test_body:82`, `tir_covers_error_conv_body:91`, `tir_covers_method:111`,
  `tir_covers_trait_method:178`). (An earlier draft claimed `tir_covers` did not exist —
  that is now wrong; it does.)
- `Source/Sema/CheckerInfer.rs` is now the directory `Source/Sema/CheckerInfer/`:
  `expr.rs` (1,282), `calls.rs` (2,334), `mod.rs`.

So the seam boundaries below are mostly a matter of **publishing a documented API per
existing module + extracting a `driver`**, not carving up monoliths from scratch.

The today's-façade entry points (`Source/lib.rs`) the ratified text calls out as the
thin layer to keep:

- `check_*`: `check_with_path` (`lib.rs:188`), `check_for_eval` (`lib.rs:203`),
  `check_document` (`lib.rs:543`), re-export `check_pure_program_root` (`lib.rs:534`).
- `compile_*`: `compile` (`:177`), `compile_with_path` (`:181`), `compile_freestanding`
  (`:249`), `compile_tests_with_path[_cov]` (`:328`/`:338`), `compile_benches_with_path`
  (`:365`), `compile_with_mode` (`:464`), `compile_rust` (`:528`); private composers
  `compile_bundle_path[_opts]` (`:240`/`:253`).
- `render_*`: `render_diagnostics` / `render_all_colored` / `render_all_json` /
  `render_all_linked` re-exports (`lib.rs:532`).

The pipeline these façade fns compose today is spread across `Loader::load_entry_*`
(lex+parse+module resolution), `CFFI::assemble`, `Sema::check_bundle` /
`check_bundle_freestanding`, `FFI::prepare`, and `Codegen::emit_bundle*`
(`lib.rs:253–313`, `:464–525`). **That composition is exactly the `driver` seam.**

---

## 2. The seven seams (Owner-Q3 baked — confirmable)

The ratified decision names them; the mapping onto the current tree:

| Seam | Owns | Today | API to publish |
|------|------|-------|----------------|
| `lexer` | source → tokens | `Source/Lexer/` | `lex(src) -> (Vec<Token>, Vec<Diagnostic>)` |
| `parser` | tokens → AST | `Source/Parser/` | `parse(&[Token]) -> Result<Program, Vec<Diagnostic>>` + module parse |
| `sema` | AST → checked AST + facts | `Source/Sema/` | `check_bundle(&mut ProgramBundle, CompileMode) -> Vec<Diagnostic>` (+ the typed-facts accessors the LSP needs) |
| `tir` | checked AST → typed/total IR | `Source/Codegen/TIR/{mod,lower,subset}.rs` | lower entry + the `Type`/`TFunc`/`TExpr` accessors (today `pub(crate)`) |
| `codegen` | TIR → Rust source | `Source/Codegen/` (`emit.rs`, `Items.rs`, `Context.rs`, `mod.rs`) | `emit_bundle(&bundle, mode, ffi) -> String` |
| `comptime` | pure-Core evaluation | `Source/Comptime/` | `run_main* ` / `DevSink` (already close to an API) |
| `driver` | composes all of the above | **new** `Source/Driver/` (extracted from `lib.rs` + `Loader`) | the `load → cffi → sema → ffi → codegen` sequence, returning `CompileOutput` |

`lib.rs` keeps only the `check_*`/`compile_*`/`render_*` thin wrappers, each now a 1–3
line call into `Driver`. The LSP's forked pipeline collapses onto the same `Driver`
calls (and gains the between-sema-and-codegen IR inspection point the decision promises).

**Owner-Q3 status:** *confirmable, not blocking.* The seam list is fixed by the ratified
text; the only open nuance is whether `tir` and `codegen` are one seam or two (they share
`Source/Codegen/`). Recommendation baked above: **two** — `tir` is the typed-IR product
the LSP wants to inspect, `codegen` is the dumb Rust emitter (I3); keeping them distinct
makes the inspection point a real API boundary. If the owner prefers a single `codegen`
seam, only the table's two rows merge; nothing else changes.

---

## 3. Module-seam vs. workspace-crate

D-COMPILERLIB1=A says "internal Rust library seams … the compiler's OWN internal crates."
Two faithful readings, both I6-safe:

- **3a — module seams in one crate (recommended first step).** Each seam is a module tree
  with a documented `pub` API and a `#![deny]` on cross-seam reach into private items;
  `Driver` is a new module. Zero workspace churn, the API discipline is real, and it is
  the prerequisite for 3b regardless.
- **3b — workspace members.** Promote each seam to a workspace-member crate once the
  module APIs are stable. This is what gives the self-host port crate-by-crate units and
  is the same workspace conversion D-JIT2=A already mandates for `jet-jit/` (c139) — so
  the workspace skeleton will exist anyway. Do 3b *after* 3a proves the API surface.

Sequencing: 3a first (publish + document each seam API, extract `Driver`, retarget
`lib.rs` and the LSP onto it), then 3b (lift the documented seams into member crates) as
a mechanical follow-on. No decision separates them; 3b is the natural completion.

---

## 4. Execution order

1. **`Driver` extraction.** Move the `load → cffi → sema → ffi → codegen` composition out
   of `lib.rs`/`Loader` into `Source/Driver/`; make `lib.rs`'s `compile_*`/`check_*` thin
   callers. Retarget the LSP's pipeline onto `Driver`. *Exit:* full suite green; LSP and
   build path share one composition.
2. **Publish per-seam APIs.** For each of lexer/parser/sema/tir/codegen/comptime, surface
   a documented `pub` entry + the accessors the driver and LSP need (notably promoting the
   `tir` `Type`/`TFunc`/`TExpr` surface from `pub(crate)` to a documented `pub` view —
   the same surfacing c139's `jet-jit/` needs). *Exit:* each seam callable as a library;
   no caller reaches a seam's private internals.
3. **(3b) workspace members.** Lift each seam into a member crate; `jet`/`jetpack` bins +
   `lib.rs` façade depend on them. *Exit:* `cargo tree` shows the seam graph; I6 stays
   machine-checkable (no external deps in any compiler seam crate).

Each step: pure refactor, no behavior change. `nix develop -c cargo build && cargo test`
green after each. Commit per step.

---

## 5. Verification

- **No behavior change.** The golden battery, `tests/decisions.rs`, and every ui snapshot
  must pass *unchanged* throughout — that is the proof the refactor is semantics-neutral.
- **Façade stability.** `lib.rs`'s public `check_*`/`compile_*`/`render_*` signatures are
  unchanged (external callers — bins, tests, LSP — keep compiling untouched).
- **I6 machine-check.** After 3b, a lockfile/`cargo tree` check over each compiler seam
  crate shows zero external crates (the same check D-JIT2=A relies on for `jet`).

---

## Status

**READY** (Epoch 3, pure refactor, no behavior change). D-COMPILERLIB1=A ratified; seam
boundaries baked (§2); Owner-Q3 confirmable, not blocking. No ballot — no user-facing
syntax. No open gate.
