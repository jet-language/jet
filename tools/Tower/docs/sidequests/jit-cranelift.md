# c139 — JIT tier-1: Cranelift backend over the JitBackend seam

**Status:** plan (needs a second-agent vet before it reaches the owner).
**Decision:** D-JITDEP1 ratified 2026-06-24 — Cranelift approved as a runtime-side dep
(never in compiler `Source/`; I6 holds; scoped + owner-signed like D-REGEX1). Production
stays AOT (compile-to-Rust); the JIT is the resident dev-loop tier only.
**Frozen successors:** c140 (own bytecode VM, zero-dep) → c141 (own native JIT). This
plan must not paint either into a corner.
**Open gate this plan surfaces:** **D-JIT2** — where the Cranelift dep physically lives
(workspace member vs. cfg-gated in-crate vs. out-of-tree component). See
`tools/Tower/docs/ballots/decision-ballots.md`. **Stop on the seam-location work until
D-JIT2 is ratified;** everything else below is decidable now.

---

## 1. The I6 question (the central tension)

I6: "Zero external crates in the compiler (`Source/`), ever." Cranelift is an external
crate. D-JITDEP1 already ruled *that the dep is allowed runtime-side and I6 holds* — but
not *where the crate physically sits*. Two facts make that a real, unanswered question:

1. The repo is **one crate today** — a single root `Cargo.toml` (`name = "jet"`, no
   `[workspace]`, no members). That crate already ships **two binaries** — `jet`
   (`Source/main.rs`) and `jetpack` (`Source/Bin/Jetpack.rs`, which delegates into
   `jet::Jetpack`) — plus the `jet` library (`Source/lib.rs`). The only other
   `Cargo.toml` in the tree is `editors/zed/wasm-src/` (crate `jet-zed-extension`,
   `cdylib`), a fully standalone crate not referenced from the root and not part of any
   workspace. `Source/JitBackend.rs` is `pub mod JitBackend` *in the `jet` crate*. So
   "not in `Source/`" cannot mean "another file in the same crate": a `Cargo.toml` dep is
   reachable from every file (both bins and the lib), so the I6 wall would be
   convention-only, not enforced.
2. The D-REGEX1 analogy in D-JITDEP1 is imperfect. `regex` lives inside a **Jet Core
   sub-library** — Jet-language code that compiles to Rust and pulls the crate as an
   ordinary package dep. The Cranelift JIT is **Rust toolchain code** consuming TIR and
   satisfying `JitBackend`; it is not a shipped Core package, so it has no Core
   sub-library to live in.

The options (full consequences in the **D-JIT2** ballot card):

- **A — new workspace member `jet-jit/`.** Convert to a Cargo workspace; `Source/`
  stays the dep-free `jet` crate (both `jet` and `jetpack` bins + the lib stay inside
  it, undisturbed); a sibling crate owns the Cranelift dep and `impl JitBackend for
  CraneliftBackend`. The `jet` binary depends on it only behind `--features jit`. I6
  stays **machine-checkable** (the `jet` crate's lockfile has no Cranelift). Matches R7
  and the seam's "second `impl JitBackend`, zero caller churn" design. Cost: one-time
  workspace conversion + a stable TIR accessor surfaced for the sibling crate (TIR types
  are `pub(crate)` today). The standalone `editors/zed/wasm-src` crate is unaffected — it
  is not referenced from the root, so a root workspace does not pull it in.
- **B — cfg-gated in-crate dep + an explicit I6 carve-out.** `cranelift-*` in the one
  `Cargo.toml` under an off-by-default `jit` feature; `CraneliftBackend` in
  `Source/Jit/Cranelift.rs` behind `#[cfg(feature = "jit")]`; amend I6 to name a
  standing runtime-tier exception. Smallest diff; I6 becomes a documented exception
  rather than an enforced wall.
- **C — out-of-tree optional toolchain component.** `jet-jit` as a separately-installed
  plugin the `jet` binary links only when present. Strongest isolation; most plumbing;
  undercuts the "fast `jet serve`, just works" value because the headline feature is no
  longer default.

**Recommendation: lay out, do not pick** (owner-facing per the syntax/dep protocol).
Engineering lean is A — it is the only option that keeps I6 enforced and is the cleanest
host for the frozen c140/c141 successors (each becomes another member behind the same
trait). The rest of this plan is written so it holds under whichever the owner picks; the
only thing that changes is the crate boundary and how TIR is surfaced.

---

## 2. What tier-1 delivers, and where it sits

Today there are two execution paths:

- **AOT** (`jet run` / `jet build`): front end → TIR → Rust source → `rustc` → native
  binary (`jet::compile_with_path`, then the driver shells `rustc`). Correct and fast at
  runtime, but every change pays a full `rustc` compile.
- **Tier-0 interpreter** (`jet dev` / `jet serve`): the comptime tree-walker
  (`Source/Comptime/`) behind the `JitBackend` seam (`InterpreterBackend`,
  `Source/JitBackend.rs`). No `rustc` in the loop (I2). But it is run-to-completion with
  **no resident heap** — `hot_swap` and `restart` both funnel to `run_checked`, so a
  "swap" is honestly just "re-run the new bundle." It cannot preserve live state across
  an edit, and a tree-walker is slow for compute-heavy dev programs.

**Tier-1 (Cranelift) fills the gap:** a resident process that JIT-compiles TIR to native
code, holds the heap between edits, and re-links a changed module in place — so
`jet serve` gets genuine live-state hot-swap (the D-HOTSWAP1 promise the interpreter can
only approximate) and near-native dev-loop speed without a `rustc` compile.

**Placement** (no new ratified verb needed; reuse the c77 surface):

- `jet serve <entry>` and `jet dev <entry> --swap` select the resident/swap path
  (`run_serve` → `run_dev`, then `run_resident_swap` / `run_resident_restart` in
  `Source/CmdDevTools.rs`). Those helpers construct `InterpreterBackend::new()` directly
  today (`CmdDevTools.rs:194,212`) — there is no long-lived `&mut dyn JitBackend` held
  across the watch loop yet, so tier-1 also makes that *construction* the selection seam.
  Tier-1 makes that construction **tier-selecting**:
  if the JIT is available (feature/component present per D-JIT2) and the program is in
  the JIT-covered subset, build a `CraneliftBackend`; else fall back to
  `InterpreterBackend` (tier-0 stays permanent — D-JIT1).
- Expose an explicit override so the choice is testable and debuggable:
  `jet serve --engine=jit|interp` (and the same on `jet dev`). Default = auto-select.
  This flag is a CLI affordance, not new language syntax (no ballot needed), but confirm
  with the owner before shipping the user-visible name.
- `jet run`/`jet build` are untouched — they stay AOT (I2/I3; the JIT never enters the
  release path).

---

## 3. The lowering path: TIR → Cranelift IR

The TIR (`Source/Codegen/TIR/`) is the right and only input. Its defining property is
**totality** (`TIR/mod.rs`): every node carries its resolved facts concretely — every
`TExpr` its `Type`, every `Binary` its overflow decision as a `bool`, every `Let` its
binding type. That is exactly what a backend needs: like the Rust emitter
(`emit_tir_func`, which "makes ZERO decisions"), the Cranelift backend pattern-matches
TIR fields and emits CLIF. **All checking stays in sema (I3 / R1); the JIT is a dumb
backend, the same contract `rustc` gets.** rustc-style "try it and see" is forbidden.

This reuses the existing coverage discipline instead of inventing a parallel one. The
emitter already gates on `tir_covers` (`TIR/subset.rs`), which conservatively decides
whether a function is fully inside the lowered subset and **excludes on any doubt**. Note
`tir_covers` is now *wide* — through its Phase 23 it admits generics, structs/enums,
methods, `#Pure`/`#Unsafe fn`s, view returns, and default params (`TIR/subset.rs:22`).
The JIT gets its own, separate predicate `jit_covers` with the same exclude-on-doubt
discipline but a deliberately **narrow** v1 (below) — its coverage set is independent of
`tir_covers`, so a `jit_covers` slice is a JIT engineering choice, not "whatever TIR
already lowers." A function outside `jit_covers` falls back to the tier-0 interpreter for
that run, never to a wrong answer.

```
front end → sema (owns all semantics, I3) → TIR (typed, total)
                                              ├─ emit_tir_func → Rust → rustc   (AOT, jet run/build)
                                              └─ lower_tir_clif → CLIF → JIT     (tier-1, jet serve)   ← new
```

**First vertical slice (`jit_covers` v1)** — deliberately the *smallest* useful subset
(narrower than `tir_covers`): top-level non-generic functions over scalars/`String`;
arithmetic/logic/comparison with
the carried overflow-trap decision; `let`/assignment/return; `if`; calls to plain
functions and `print`. This is enough to JIT-execute hello-world, arithmetic, and
function calls end to end.

**Deferred (stays tier-0 until later phases widen `jit_covers`):** generics/monomorph,
closures, structs/enums and methods, collections, the Core runtime surface (`fs`, `env`,
`net`), tasks/FFI/`@unsafe`, and `view`/borrow returns. The interpreter is the permanent
floor (D-JIT1), so deferral is safe, not a stub.

**Runtime symbols.** `print`, allocation, panic/trap reporting, and any Core call the
slice needs are resolved as host symbols the JIT links to — small Rust shims in the JIT
crate that produce **byte-identical** output to the AOT path (the Q2 hard rule). The
trap/overflow behavior must match the AOT path's checked-arithmetic decision the TIR
already carries.

---

## 4. Staged milestones (each with an exit criterion)

- **M0 — seam wiring (blocked on D-JIT2).** Establish the crate boundary the owner
  picks; surface a stable TIR view to the backend; stub `CraneliftBackend` that
  delegates everything to the interpreter. *Exit:* `jet serve --engine=jit` builds and
  runs every example with output identical to `--engine=interp` (because it still
  delegates). Proves the wiring without a single CLIF instruction.
- **M1 — first JIT execution (the vertical slice).** Implement `jit_covers` v1 +
  `lower_tir_clif` for that subset; JIT-execute `01_hello.jet`, arithmetic, and
  multi-function programs natively; others fall back to tier-0. *Exit:* the slice's
  examples produce byte-identical stdout vs. the AOT binary AND vs. the interpreter, in
  the differential battery; a covered compute loop is measurably faster than tier-0.
- **M2 — live-state hot-swap.** Resident process holds the heap; a type-stable edit
  (`Sema::HotSwap::type_stable_check`, already the gate) re-links the changed module in
  place without losing state; a type/layout-changing edit does the announced clean
  restart (D-HOTSWAP1). *Exit:* a `jet serve` session preserves a counter across a
  type-stable edit; a type-changing edit restarts cleanly; both announced as today.
- **M3 — widen coverage.** Grow `jit_covers` toward parity (structs/enums, methods,
  collections, more Core), each addition gated by the differential battery. *Exit:* the
  JIT covers the same example set as the AOT path, or each gap is a named, tested
  fallback to tier-0 (never a silent wrong answer).

Difficulty is not a milestone boundary; correctness and the differential battery are.

---

## 5. Risks

- **Cranelift API churn.** Cranelift's `cranelift-*` crates move fast. Confine every
  Cranelift type behind the JIT crate's own thin adapter (one module that builds the
  `Module`/`Context`/`FunctionBuilder`); the rest of the JIT speaks our own small CLIF-
  building helpers. Pin the version; upgrades touch one file. This is also the seam that
  makes c140 (bytecode VM) a *replacement of that one adapter*, not a rewrite.
- **Executable memory / W^X.** A JIT writes machine code then executes it — the
  classic W^X hazard the board card flags. Use Cranelift's JITModule, which already
  manages code memory and finalization; do not hand-roll `mmap`/`mprotect`. Keep
  executable pages out of the AOT path entirely (the release binary never JITs). Treat
  any JIT crash/miscompile as a **P0 differential-battery failure**, not a runtime
  warning.
- **Platform coverage.** Cranelift targets x86-64 and aarch64 (Linux/macOS) well; other
  targets are weaker. Mitigation: tier-1 is a *dev-loop accelerator with a permanent
  tier-0 fallback* — on an unsupported host, `jet serve` silently uses the interpreter.
  Production is AOT via rustc, which keeps full platform reach. So platform gaps degrade
  dev speed, never correctness or shipping.
- **Not painting the frozen successors into a corner.** The contract every tier honors
  is the `JitBackend` trait + TIR-as-input. c140 (bytecode VM) and c141 (native JIT) are
  *additional `impl JitBackend`s consuming the same TIR*. Two rules keep that open: (1)
  no Cranelift type may leak past the JIT crate's adapter or into the seam; (2) the
  TIR-subset gate (`jit_covers`) is engine-named, so a successor can declare a different
  coverage set without disturbing tier-0 or AOT. Under D-JIT2 option A this is enforced
  by crate boundaries; under B/C, by the adapter discipline above.
- **I2 boundary.** rustc must never enter the serve loop and Cranelift must never enter
  the release path. Keep the JIT behind the seam and behind the feature/component gate so
  a default compiler build neither pulls Cranelift nor can JIT.

---

## 6. Testing — proving JIT == AOT == interpreter

The precedent exists and is reused, not reinvented: `tests/dev.rs ::
interpreter_matches_compiled_binary` (`tests/dev.rs:65`) already runs **every**
`examples/features/*.jet` through the interpreter (via `jet::Interpreter::dev_iteration`)
and diffs its stdout byte-for-byte against the `rustc`-compiled binary (built from
`jet::compile_with_path`), recognizing a fixed set of named boundary codes
(`E2201/E2202/E0952/E0956/E0953`) instead of a silent skip. `tests/comptime_diff.rs`
does the same at comptime. Both skip cleanly when `rustc` is absent.

- **Three-way differential.** Extend that battery to add the JIT as a third lane: for
  every example the JIT covers (`jit_covers`), assert
  `jit_stdout == interpreter_stdout == compiled_binary_stdout`, byte-for-byte. Mechanism
  note: the test drives the interpreter through `dev_iteration`, **not** through the
  `JitBackend`/`InterpreterBackend` seam, so the JIT lane likewise needs a parallel entry
  point (a `jet_iteration`-style call that builds a `CraneliftBackend` and runs the
  covered subset) — not a swapped seam impl inside the test. Any divergence is a release
  blocker (Q2 hard rule, D-DEVMODE1), not a warning.
- **Honest fallback, never a silent skip.** An example the JIT does not cover must take
  the recorded tier-0 fallback (matching the interpreter lane), exactly as the existing
  battery requires a *named boundary* rather than a silent skip. The test reports the
  JIT-covered / fell-back split so coverage can never quietly shrink.
- **Hot-swap behavior.** A small scripted `jet serve` session test: type-stable edit
  preserves state; type-changing edit restarts and announces — asserted against the
  D-HOTSWAP1 contract.
- **No new diagnostics expected.** The JIT emits no user diagnostics (I2/I3); sema owns
  all errors. If the JIT ever cannot lower a function sema accepted, that is an internal
  fallback to tier-0, logged for the battery — never a user-facing message and never an
  ICE in the release path.

---

## Open decisions this plan surfaces

- **D-JIT2 (new, queued):** where the Cranelift dep physically lives — workspace member
  / cfg-gated in-crate / out-of-tree component. **Blocks M0.** Owner-facing.
- **`jet serve --engine=` flag name** (minor, CLI-only, not language syntax): confirm
  the user-visible spelling with the owner before shipping; no ballot required.

Everything else (lowering via TIR, `jit_covers` discipline, the three-way differential
battery, milestone exits) is decidable now and needs no further ratification.
