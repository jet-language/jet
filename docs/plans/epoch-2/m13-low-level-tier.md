# E2-M13 — Expert low-level tier

**Status:** draft — **blocked on D-LL1…D-LL3** (Group M13). Implements the
ratified S58 low-level gate. **D-LL1 ratifies the I1 amendment wording** — until
then, no generated `unsafe` may ship.
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
| D-LL2 | `unsafe` audit story | **A** — structured audit comment + lint | A | OPEN — tracked in the cross-cutting "attributes" thread |
| D-LL3 | `std.mem` API breadth | **A** — narrow: `Ptr<T>`, alloc, layout, volatile | A | ✅ ratified 2026-06-16 — A: narrow `std.mem` core PLUS an opt-in wider expert API (name TBD) |

**The I1 amendment (D-LL1) is the gating decision.** I1 today: *no `unsafe` in
the language or generated code, ever (v1)*. The amendment: generated `unsafe`
exists **only** inside user-written gated regions (`import std.mem` + `unsafe {}`)
or vetted std/mem internals. The exact wording must be ratified into
docs/spec/architecture.md before any code emits `unsafe`.

## Scope (from S58)

- **Discovery gate:** `import std.mem` is required to name any low-level item.
- **`unsafe { … }` audit gate** and the `unsafe fn` contract.
- **`Ptr<T>`,** pointer deref/math, transmute-class casts.
- **Explicit allocators,** including arenas and fixed allocators (coordinate with
  E2-M5 arenas and E2-M15 freestanding).
- **Layout/repr controls.**
- **Volatile / MMIO wrappers.**
- **Audit model (D-LL2):** every `unsafe` block carries a structured audit
  comment; a lint flags missing/empty audits.

## Surface (example — everything is gated)

```jet
import std.mem;                          // discovery gate

fn read_reg(addr: Int) -> Int {
    unsafe {                             // audit gate
        // SAFETY: addr is a valid MMIO register mapped by the platform HAL.
        val p = mem.Ptr<Int>.from_addr(addr);
        return mem.volatile_read(p);
    }
}
```
Using `mem.Ptr` or `volatile_read` **outside** an `unsafe` block in a module that
imported `std.mem` is **E3101**.

## Diagnostics to register

- **E3101** low-level operation used outside an `unsafe` gate.
- **E3102** `std.mem` item named without `import std.mem`.
- **E3103** `unsafe fn` called without an `unsafe` block.
- **L3101** `unsafe` block missing a `// SAFETY:` audit comment (D-LL2).

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
