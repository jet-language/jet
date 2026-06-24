# c77 — Three-mode execution + JIT dev runtime (hot-reload)
**Decisions:** D-JIT1 (ratified 2026-06-22, option D), D-HOTSWAP1 (ratified 2026-06-22, option B), D-DEVMODE1 (ratified 2026-06-22, option A + Q2 hard rule)
**Gate:** none — all three decisions are UNBLOCKED. Cranelift JIT dep (D-JIT1 option D+) needs separate owner dep-approval (I6 runtime exception) before landing; plain D (interpreter tier-0) is unblocked now.

---

## What is decided

**D-JIT1 (D):** `jet serve` ships hot-reload on the existing comptime interpreter
(`Source/Comptime/`), behind a stable `JitBackend` seam. A Cranelift JIT is a later tier-1;
interpreter stays permanent tier-0. rustc-in-the-loop is rejected (I2 hazard).

**D-HOTSWAP1 (B):** The hot-reload unit is a module. A type-stable edit swaps code and keeps
the module's live state. A type/layout-changing edit does a clean, announced,
connection-drained restart. The type-surface check is a sema job (I3 — never "try rustc").

**D-DEVMODE1 (A + Q2 hard rule):** `jet dev <entry>` is the single dev command. It
auto-detects run-to-completion programs (rerun on save) vs resident programs (hot-swap on
save). Experts override with `--restart`/`--swap`/`--watch=off` flags. Q2 hard rule: dev
(interpreter) output MUST be byte-identical to the release (rustc) build — any divergence is
a release blocker, not a warning. A `tests/dev.rs` mode diffs every golden example through
both paths.

---

## Current state

`Source/Interpreter.rs` implements the `jet dev` watch-and-rerun loop for run-to-completion
programs (D-DEV4, E2-M4). `Source/Comptime/` is the interpreter engine. `Source/CmdDevTools.rs`
wires the CLI. `Source/main.rs` dispatches `jet dev`. What is missing:

1. `JitBackend` seam trait — a stable interface so the Cranelift tier can slot in later.
2. Resident-program detection and hot-swap loop (currently `jet dev` only reruns, never swaps).
3. `jet serve` verb (currently unimplemented or stub).
4. Module-level type-stability check (D-HOTSWAP1 B).
5. Byte-identity differential test harness (D-DEVMODE1 Q2).

---

## Plan

### Step 1 — `JitBackend` seam (`Source/JitBackend.rs`, new file)

```rust
/// Stable seam between `jet dev`/`jet serve` and the execution tier (D-JIT1).
/// Tier 0: interpreter (always available). Tier 1: Cranelift (future, owner-approved dep).
pub trait JitBackend {
    /// Run `fn main()` in the bundle; return stdout/stderr or diagnostics.
    fn run(&mut self, bundle: &ProgramBundle) -> RunOutcome;
    /// Hot-swap a module whose type surface is unchanged (D-HOTSWAP1 B).
    /// Returns Err if a type-stability check fails (type_stable_check must have passed first).
    fn hot_swap(&mut self, module_name: &str, bundle: &ProgramBundle) -> Result<(), Vec<Diagnostic>>;
    /// Signal a clean restart (type-layout changed or explicit --restart).
    fn restart(&mut self, bundle: &ProgramBundle) -> RunOutcome;
}

/// Tier-0: wraps the existing comptime interpreter.
pub struct InterpreterBackend { /* ... */ }
impl JitBackend for InterpreterBackend { /* delegate to Comptime */ }
```

`Source/Interpreter.rs` is refactored to implement `JitBackend` for `InterpreterBackend`. The
existing `dev_iteration` fn becomes a thin wrapper that creates an `InterpreterBackend` and
calls `run`.

### Step 2 — Resident-program detection (`Source/Interpreter.rs`, `Source/CmdDevTools.rs`)

Auto-detect: scan the AST for a `loop { … }` or `task.spawn` at the top level of `fn main`.
If found, classify as resident (hot-swap mode). Otherwise, run-to-completion (rerun mode).

Add `DevMode` enum:

```rust
pub enum DevMode { RunToCompletion, Resident }
pub fn detect_dev_mode(bundle: &ProgramBundle) -> DevMode { … }
```

Implement in `Source/Interpreter.rs`. `CmdDevTools.rs` calls `detect_dev_mode` then picks
the loop branch. `--restart` / `--swap` flags override detection.

### Step 3 — Hot-swap loop (D-HOTSWAP1 B)

In `CmdDevTools.rs`, the watch loop for resident programs:

1. File-change event triggers re-parse + re-sema (same as today).
2. Call `type_stable_check(old_bundle, new_bundle) -> TypeStabilityResult`.
3. If stable: call `backend.hot_swap(module_name, new_bundle)`. Emit `[hot-swap] <module>` to stderr.
4. If unstable (type/layout changed): emit `[restart] layout changed in <module>: <field>`.
   Drain connections (for server programs, wait for in-flight requests up to 2s), call
   `backend.restart(new_bundle)`.

**Sema: `type_stable_check`** (`Source/Sema/mod.rs` or new `Source/Sema/HotSwap.rs`):

Walk both bundles' registered struct/enum types. A change is unstable if:
- A field is added, removed, or retyped.
- A variant is added or removed from an enum.
- A function's signature changes (param types or return type).

This is a sema check over the type registry — no rustc involved (I3). Emit **E2210**
("hot-swap rejected: `T::field` changed type from `A` to `B`; restarting") when unstable.

**Diagnostic (I4)**

E2210 — hot-swap type-stability rejection. Add to `docs/spec/diagnostics.md`; snapshot at
`tests/ui/e2210_hotswap_rejected.txt`.

### Step 4 — `jet serve` verb (`Source/main.rs`, `Source/CmdDevTools.rs`)

`jet serve <entry>` is an alias for `jet dev <entry> --swap` (force resident mode). Add to
`main.rs` verb dispatch. `jet serve` without a file emits a teaching error naming `jet dev`.

### Step 5 — Byte-identity differential harness (D-DEVMODE1 Q2)

`tests/dev.rs`: for each `.jet` file in `examples/features/expected/`, run both:
- `Interpreter::run(bundle)` → stdout
- `lib::compile_and_run(bundle)` → stdout

Diff the two. Any divergence is a test failure (a release blocker). This replaces the
aspirational comment in `Source/Interpreter.rs` ("The bytes it produces are identical to the
compiled program (I2); the differential battery in `tests/dev.rs` is the enforcement.") with
a real test.

---

## Files touched

| File | Change |
|------|--------|
| `Source/JitBackend.rs` (new) | `JitBackend` trait + `InterpreterBackend` struct |
| `Source/lib.rs` | `pub mod JitBackend` |
| `Source/Interpreter.rs` | refactor to `InterpreterBackend`; `detect_dev_mode` |
| `Source/CmdDevTools.rs` | hot-swap loop, `--restart`/`--swap`/`--watch=off` flags, `jet serve` |
| `Source/Sema/mod.rs` or `Source/Sema/HotSwap.rs` (new) | `type_stable_check` |
| `Source/main.rs` | `jet serve` verb dispatch, new flags |
| `docs/spec/diagnostics.md` | E2210 entry |
| `tests/ui/e2210_hotswap_rejected.txt` | snapshot |
| `tests/dev.rs` | byte-identity differential harness |

---

## Decision verdict

No decision needed — D-JIT1, D-HOTSWAP1, D-DEVMODE1 are all ratified and UNBLOCKED.

**Cranelift tier (D-JIT1 option D+):** needs separate owner dep-approval before a
`CraneliftBackend` can be added. The seam (`JitBackend` trait) is landed now so the dep
approval is the only gate.
