# E2-M13 — Expert low-level tier

**Status:** ready to implement — **D-LL1** ✅, **D-LL2** ✅, **D-LL3** ✅ ratified.
Implements S58. **D-LL1** I1 amendment must be recorded in `architecture.md` before
codegen emits `unsafe`.
**Depends on:** E2-M5 (references). Unblocks E2-M14 (C FFI pointers) and E2-M15
(freestanding allocator story).
**Error codes:** E31xx block (claim in docs/spec/diagnostics.md).

## Goal

Provide C/C++/Rust/Zig-class control behind explicit gates, while ordinary Jet
stays memory-safe and pays no runtime cost for this tier (E2-V6 = credible for
systems programmers, still gated). Beginner docs never require this tier.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-LL1 | **I1 amendment wording** | **A** — generated `unsafe` only inside user-gated regions or vetted std/mem internals | A (else block M13) | ✅ ratified 2026-06-16 — A: amend I1 for user-gated `unsafe` |
| D-LL2 | `@unsafe` audit story | — | — | ✅ **B** — `@audit("…")` on `@unsafe` blocks |
| D-LL3 | `std.mem` API breadth | **A** — narrow: `Ptr<T>`, alloc, layout, volatile | A | ✅ ratified 2026-06-16 — A: narrow `std.mem` core PLUS an opt-in wider expert API (name TBD) |

**The I1 amendment (D-LL1) is the gating decision.** I1 today: *no `unsafe` in
the language or generated code, ever (v1)*. The amendment: generated `unsafe`
exists **only** inside user-written gated regions (`use core.mem` + `@unsafe { … }`)
or vetted std/mem internals. The exact wording must be ratified into
docs/spec/architecture.md before any code emits `unsafe`.

## Scope (from S58)

- **Discovery gate:** `use core.mem` is required to name any low-level item.
- **`@audit("…")` + `@unsafe { … }` audit gate** (D-LL2 ✅).
- **`Ptr<T>`,** pointer deref/math, transmute-class casts.
- **Explicit allocators,** including arenas and fixed allocators (coordinate with
  E2-M5 arenas and E2-M15 freestanding).
- **Layout/repr controls.**
- **Volatile / MMIO wrappers.**
- **Audit model (D-LL2 ✅):** `@audit("…")` required immediately before each `@unsafe` block; lint **L3101** if missing.

## Surface (example — everything is gated)

```jet
use core.mem;

fn read_reg(addr: Int) -> Int {
    @audit("addr is a valid MMIO register mapped by the platform HAL")
    @unsafe {
        val p = mem.Ptr<Int>.from_addr(addr);
        return mem.volatile_read(p);
    }
}
```
Using `mem.Ptr` or `volatile_read` **outside** an `@unsafe` block in a module that
used `core.mem` is **E3101**.

## Diagnostics to register

- **E3101** low-level operation used outside an `unsafe` gate.
- **E3102** `core.mem` item named without `use core.mem`.
- **E3103** `@unsafe fn` called without an `@unsafe` block.
- **L3101** `@unsafe` block missing `@audit("…")` (D-LL2).

## Examples & tests

- `examples/features/48_lowlevel.jet` — a small, audited `unsafe` example whose
  output is tested against the compiled Rust.
- ui fixtures for E3101–E3103 and the L3101 audit lint.
- A test proving memory-safe Jet code emits **no** `unsafe` (the I1 amendment
  boundary holds).

## Out of scope

- Inline assembly.
- Lifting `unsafe` into ordinary Jet (the whole point is the gate).
- A general FFI surface (E2-M14) beyond the pointer rules this milestone defines.
- Custom calling conventions / naked functions.

## Exit criteria

- Beginner docs never require this tier.
- Every unsafe operation outside the gates produces a diagnostic.
- Unsafe examples are small, audited, and tested against Rust output.
- Memory-safe Jet code pays no runtime cost and emits no `unsafe`.
- The I1 amendment is ratified in docs/spec/architecture.md.
- `nix develop -c cargo test` green.
