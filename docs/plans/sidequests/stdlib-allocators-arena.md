# Sidequest: stdlib memory allocators (arena, pool, bump, fixed)

## Goal

Ship the explicit-allocator array the expert tier (S58) and freestanding builds
(E2-M15) already advertise but do not have: an `Arena` (ratified location:
flat `core.mem.Arena`, D-REF2), plus `Bump`, `Pool`, and `Fixed` allocators, all
behind the existing `use core.mem` discovery gate. This is stdlib work (sema
signature tables + codegen dispatch + a Rust runtime helper module), not new
core syntax — the only surface tokens are the allocator type names and their
method names, which need owner ratification before code. The API must read in
the view/edit/take/share capability vocabulary (see
`sidequests/memory-capability-model.md`): an arena *owns* its backing store
(`take`), hands out values you *view*
or *edit*, and frees everything at scope end via S63 RAII.

## Current state (verified)

- **`core.mem` tier is live** (S58, E2-M13). Constants in `Source/Syntax.rs`:
  `CORE_MEM_MODULE = "core.mem"` (123-124), `TYPE_PTR = "Ptr"`,
  `MEM_FROM_ADDR`, `MEM_VOLATILE_READ`, `MEM_ADDRESS_OF` (126-138). No allocator
  constant exists yet.
- **Existing mem operations** are special-cased in the `core.mem` match arm in
  `Source/Sema.rs` (~6556-6592): `volatile_read` (gate E3101 if not `in_unsafe`),
  `address_of` (inert, ungated). Discovery gate E3102 lives at
  `Checker::e3102` / the alias check ~6479-6519.
- **Codegen dispatch** for `core.mem` is in `Source/Codegen/Expression.rs` (~979-980):
  `address_of` → `(&(x) as *const _ as usize as i64)`, `volatile_read` →
  `std::ptr::read_volatile(p)`. `Ptr<T>` lowers to `*mut T` (`Source/Codegen/Context.rs`
  ~209). No allocator codegen.
- **`set_allocator` is referenced but unimplemented.** It appears *only* in the
  E3303 fix-text (`Source/Sema.rs` ~367: "configure an arena or fixed allocator
  with `mem.set_allocator(…)`"). There is no signature, no codegen, no example.
  E3303 (freestanding-needs-allocator, ~359-373) is the one diagnostic that
  already presumes this API.
- **No `mem` module file or `Arena` type exists.** `find … -name '*alloc*'`
  hits only the C tree-sitter `alloc.h`. The Rust runtime prelude is
  `Source/Prelude/Core.rs` + `Source/Prelude/Std.rs`; there is no allocator helper.
- **Std signatures** are a flat `match (module, name)` in `std_fixed_sig`
  (`Source/Sema.rs` ~10225+). Return/param types are `ast::Type` values; there is
  no `Arena`/handle variant — opaque std types would be `Type::Named("Arena")`
  (the `Type` enum is `Source/AST.rs:18`; `Shared(Box<Type>)` at 30 is the closest
  existing "handle" precedent).
- **Known-module list** in `Source/Loader.rs` (~695, 715) already includes
  `core.mem`; no submodule needed (D-REF2 says flat).
- **Ratified placement (D-REF2, 2026-06-17, recorded in
  `docs/spec/syntax-decisions.md`):** "ship arenas; live directly in
  `core.mem` (not a submodule); surface the API as `core.mem.Arena` or
  equivalent flat path." **D-LL3 (ratified):** narrow mem core PLUS an opt-in
  wider expert API, name TBD — the pool/bump/fixed breadth lands under that wider
  bucket.
- **Example** `examples/features/48_lowlevel.jet` is the audited end-to-end mem
  example; allocators would extend it or add a sibling.

## Implementation approach (workflow loop)

Per-allocator increments; land `Arena` first (it is the ratified, freestanding-
blocking one), then `Fixed`, `Bump`, `Pool` behind the same gate.

1. **Failing test first.** Add `examples/features/49_arena.jet` (allocate in an
   arena, use the value, let it free at scope end) + its `.expected`, and a UI
   fixture `tests/ui/mem_arena_gate/` proving E3102 fires when `Arena` is named
   without `use core.mem`.
2. **Spec.** Extend the E2-M13 section of `docs/spec/spec.md` (line ~349-372,
   which today says allocators are "deferred (unratified)") to record the
   ratified `Arena` and the wider-API allocators, the capability story, and the
   freestanding link to E3303.
3. **`Source/Syntax.rs`** (I7). Add constants under the S58 block: `MEM_ARENA =
   "Arena"`, `MEM_BUMP`, `MEM_POOL`, `MEM_FIXED`, and method names
   (`MEM_ALLOC_NEW`, `MEM_ALLOC_ALLOC`, `MEM_ALLOC_RESET`, `MEM_SET_ALLOCATOR`)
   — **only after the owner ratifies the names** (see Decisions). Each carries
   its decision ID in a doc comment.
4. **Parser.** Likely **zero grammar change**: `mem.Arena.new(...)` reuses the
   same `alias.Type.method(...)` path the existing `mem.Ptr<T>.from_addr` tail
   uses (`Source/Parser.rs` ~3716, ~4264) and ordinary method calls. Confirm
   `arena.alloc(...)` parses as a normal method call on a `Named("Arena")`
   value. If `Arena` needs a generic slot (`Arena.alloc<T>()`), that is the only
   parser-touch risk — prefer inferring `T` from the initializer to avoid it.
5. **Sema.** (a) Register `Arena`/`Bump`/`Pool`/`Fixed` as built-in opaque
   `Type::Named` std types gated by `use core.mem` (reuse the E3102 path).
   (b) Add signatures to `std_fixed_sig` and/or extend the `core.mem` match arm:
   constructors return the handle (`take` ownership), `alloc(value)` returns the
   stored value at the inferred type, `reset()` takes `edit self`. (c) Decide
   the gate level: constructing/using an allocator is **memory-safe** (the whole
   point of safe arenas) → it should *not* require `@unsafe`, only `use
   core.mem` (E3102). Only raw `Ptr` ops keep E3101. (d) Wire E3303's promised
   `set_allocator` for freestanding.
6. **Codegen (dumb, I3).** Map each handle to a vetted Rust impl in a new
   `Source/Prelude/mem.rs` (emitted like `core.rs`/`std.rs`). The internal Rust may
   use `unsafe`/an external crate (I6 allows stdlib externals until end of
   Epoch 3; the I1 amendment D-LL1 allows generated `unsafe` in vetted std/mem
   internals). `mem.Arena.new()` → `JetArena::new()`; `arena.alloc(v)` →
   `arena.alloc(v)`; scope-end free is Rust `Drop` (S63 RAII, already the
   contract). No `unsafe` leaks into user-visible generated code outside the
   helper module.
7. **fmt.** Allocator calls are ordinary method calls → covered by existing
   formatting. Only if a new path/generic form is added does `Source/Formatter.rs`
   (~1560-1567, the `Ptr` tail) need a sibling arm.
8. **Diagnostics.** Reuse E3102 (discovery gate) for naming an allocator without
   `use core.mem`; reuse E3303 for the freestanding case. Likely **one new**
   diagnostic: using an arena-allocated value after the arena is reset/dropped —
   phrased in capability words ("this value lives in `arena`; `arena` was
   reset/ended here", per the owner-todo diagnostics list), code in the E33xx
   band. Add its `tests/ui/` snapshot (I4 — no snapshot, no diagnostic).
9. **Examples/tests (I5).** `49_arena.jet` runnable + golden; the capability
   interaction (arena allocates → value is `view`/`edit` → freed at scope end)
   demonstrated; freestanding example wiring `set_allocator`.

## Decisions needed before coding (owner)

These are the surface-syntax / API-shape choices an agent may not pick. Placement
(`core.mem.Arena`, flat) is already ratified (D-REF2); these refine the shape.

See the StructuredOutput `decisions` for the per-option before/after Jet code.
Summary of what needs a ruling:

- **D-ALLOC-A — constructor + allocate spelling.** `mem.Arena.new()` +
  `arena.alloc(value)` (method) vs. an allocator-parameter style
  (`make(Node, in: arena)`) vs. capacity-typed constructor.
- **D-ALLOC-B — does an arena-allocated value need `@unsafe`?** Recommend **no**
  (safe by default; arenas are the *safe* expert primitive) — gate only with
  `use core.mem` (E3102). Confirm.
- **D-ALLOC-C — which allocators ship, and the wider-API name (D-LL3 leftover).**
  `Arena` is ratified. Bundle `Bump`/`Pool`/`Fixed` now or stage them? And what
  is the "wider expert API" namespace name D-LL3 left TBD (e.g. keep them flat in
  `core.mem`, or group under `core.mem.alloc`)?
- **D-ALLOC-D — reset/free verb + use-after-reset diagnostic wording** in
  capability vocabulary.

## Test / acceptance checklist

- [ ] `49_arena.jet` runs, golden output matches (I5).
- [ ] Naming `Arena` without `use core.mem` → E3102, snapshot pinned
      (`tests/ui/mem_arena_gate/`).
- [ ] Arena allocation needs **no** `@unsafe` (if D-ALLOC-B = no): a gated-but-
      safe example compiles clean; a raw-`Ptr` op still demands `@unsafe`
      (E3101) — both pinned.
- [ ] Use-after-reset/free → the new E33xx diagnostic, capability-worded,
      snapshot pinned (I4).
- [ ] Freestanding: a `--freestanding` program that allocates compiles once
      `mem.set_allocator(arena)` is configured; without it, E3303 (existing
      snapshot still valid).
- [ ] Scope-end free verified (RAII/Drop, S63) — no leak, value unusable after.
- [ ] `jet fmt` round-trips allocator calls unchanged.
- [ ] No `unsafe` in user-visible generated code; only inside the vetted
      `Source/Prelude/mem.rs` helper (I1 amendment D-LL1).
- [ ] All allocator type/method tokens live in `Source/Syntax.rs` with decision IDs
      (I7); every new diagnostic in `docs/spec/diagnostics.md` (I4).
