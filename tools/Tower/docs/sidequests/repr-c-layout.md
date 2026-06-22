# Plan: `repr(C)` struct layout control (D-REPRC1)

**Status: plan — awaiting owner decision D-REPRC1.**

Unblocks: **Yuki** (embedded — C-struct interop, MMIO register blocks),
**Marcus** (FFI interop with C numerical libs).

---

## Goal

Struct field layout is opaque today — the compiler picks order/padding, so a Jet
struct cannot reliably overlay a C struct or a memory-mapped register block.
Yuki needs a struct whose layout matches C's: declared field order, C padding
rules, no reordering. The user-facing goal: one annotation that pins layout so
`use c.<lib>` structs and `#Unsafe` MMIO casts are sound.

Verified: `syntax-decisions.md:661` mentions "layout/repr control" only as a
future audit-gate concern; no `repr` annotation exists (`grep repr Source/` →
nothing). FFI today is `extern rust` (`22_ffi.jet`) and the native C-prototype
binder D-CBIND3 — neither pins Jet-side struct layout.

## Pipeline touch points

- **parser**: a layout annotation on a `struct` item (form is the decision).
- **sema**: a layout-tagged struct is only allowed where its guarantees hold;
  e.g. it cannot contain a Jet-managed (non-`repr(C)`-safe) field like a growable
  `[T]` without a diagnostic. Interacts with c82 (fixed-size lists `[T#N]`): a
  `repr(C)` struct of fixed arrays is the firmware case.
- **codegen**: emit `#[repr(C)]` on the generated Rust struct (this is the one
  place generated Rust legitimately mirrors a layout attribute).
- **diagnostics**: a "field type X has no stable C layout" error.

## Invariants in play

- **I1** layout control is an expert-tier opt-in; default structs stay opaque/safe.
  `repr(C)` itself is safe (it only pins layout); the *unsafe cast* that uses it
  stays behind `#Unsafe`/`#Audit` (I1). repr does not weaken safety on its own.
- **I3** codegen stays dumb — it just stamps the attribute sema already validated.
- **I7** the annotation keyword lives in `Syntax.rs` with a decision id.

## Open questions (need owner decision — D-REPRC1)

1. **Surface spelling** — `#repr(c)` (attribute, matches D-ATTR1 markers),
   `#layout(c)` (parallels D-SOA1's `#layout(soa)`), a `c struct Foo` modifier, or
   `extern(c) struct`. Strong interaction with **D-SOA1** (`#layout(soa)`) — should
   C-repr and SOA be the *same* `#layout(…)` family?
2. **What repr modes** — just `c`, or also `packed`, `transparent`, explicit
   alignment `align(N)`? Embedded MMIO often needs `packed` and explicit align.
3. **Field-type restrictions** — which Jet types are legal in a `repr(C)` struct?
   (scalars + fixed arrays + other repr(C) structs; reject growable `[T]`, `Map`,
   `String` unless represented as a known C layout).
4. **Enum repr** — does C-repr extend to tagged enums (`#repr(c)` / `#repr(i32)`
   discriminant control) or structs only in v1?

## Test plan

1. `examples/features/repr_c.jet` — define a `repr(C)` struct mirroring a C
   header struct, pass it across `use c.<lib>` (or `extern rust`), read a field
   back; golden output (I5).
2. Negative: `repr(C)` on a struct with a growable field → diagnostic snapshot.
3. Layout assertion test (field offsets match the C ABI for the target).
4. (If `packed`/`align` in scope) a packed-struct size test.
