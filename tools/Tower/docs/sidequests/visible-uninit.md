# Plan: Visible uninitialization (D-UNINIT1)

**Status: plan — awaiting owner decision D-UNINIT1.**

---

## Goal

Let expert-tier code skip the automatic zero-fill of a local binding, for measurable
performance wins in hot buffer paths (network I/O, codecs, parsers). The safety rail —
compile-time read-before-write proof — is preserved unconditionally. There is no mode
where an uninitialized read becomes undefined behavior.

---

## Pipeline touch points

### 1. Lexer

Add `uninit` as a reserved keyword. Outside a `use core.mem` context the lexer emits
the token normally; sema rejects it with a teaching error. This matches the pattern
used for other `core.mem`-gated vocabulary.

### 2. Parser

Accept `= uninit` in the initializer position of a local binding:

```
local-binding = name [ ":" Type ] ( "::" | ":=" ) ( expr | "uninit" )
```

`uninit` is not valid as:
- a struct field default
- a const or comptime expression
- a default parameter value

Each invalid position gets a distinct sub-code of E0420.

### 3. Sema — gate check

If `uninit` appears without `use core.mem` in scope, emit:

```
E0419  `= uninit` requires `use core.mem`
       add `use core.mem` at the top of this file to enable
       the expert low-level tier (see S58)
```

### 4. Sema — write-before-read dataflow

For a binding declared `= uninit`, sema tracks a `MaybeInit` lattice value
(`Uninit | Init | MaybeInit`) on every binding through control flow. A read of a
`MaybeInit` or `Uninit` binding emits:

```
E0420  read of possibly-uninitialized value `<name>`
       declared `= uninit` at <location>
       every path through this function must write `<name>`
       before reading it
       fix: write to `<name>` before this point, or remove `= uninit`
```

Whole-array writes (`sock.read(mut buffer)?`) mark the entire binding `Init`.
Partial-index writes (`buffer[0] = x`) mark only the indexed slot; a subsequent
whole-array read remains `MaybeInit` until all slots are provably written (v1 can
conservatively mark any partial write as `MaybeInit` and require the user to
write the whole thing or use a loop).

### 5. Codegen — lowering to `MaybeUninit`

```rust
// Jet:  buffer: [4096]u8 = uninit
// Rust (generated, inside the user's #unsafe-gated region or std/mem internals):
let mut buffer = MaybeUninit::<[u8; 4096]>::uninit();
// … after sema proves all paths write before read …
// At the first proven-safe read site:
let buffer = unsafe { buffer.assume_init() };
```

The `unsafe` block in generated Rust is produced only inside the codegen path for
`= uninit` bindings, which are already gated by `use core.mem` / `#unsafe` (S58).
Invariant I1 is not violated: no `unsafe` appears in generated Rust without a
corresponding user-written gate.

---

## v1 scope

- Stack-local bindings only (`let`/`var` — i.e., `name := uninit`). Heap allocation
  of uninit values is deferred.
- Whole-binding writes mark `Init`; partial-index writes mark `MaybeInit`
  (conservative; accepted for v1).
- Arrays and structs are the primary use case; uninit of scalar types is allowed but
  the compiler hints that zeroing a scalar is free.
- No `uninit` in `const`, comptime, or struct-field-default positions.

---

## Safety rail

Read-before-write is a **compile error on all paths**, not a runtime trap and not a
debug-only check. This is the hard line between Jet and Zig (`= undefined` traps only
in debug builds). The `MaybeUninit` lowering is the mechanism; sema carries the proof.

---

## Test plan

1. **E0419 snapshot** — `= uninit` without `use core.mem` → teaching error.
2. **E0420 snapshot (read before write)** — binding declared `= uninit`, read before
   any write on at least one path → E0420 with fix-it.
3. **E0420 snapshot (partial write)** — only some indices written before a whole read
   → E0420 (conservative v1 rule).
4. **Happy-path example** — `examples/features/uninit_buffer.jet` fills a 4 KB buffer
   and processes it; golden test checks output (I5).
5. **Invalid positions** — `= uninit` in const, struct field default, default param →
   distinct E0420 sub-code per position (I4).

---

## Open questions

- Should `uninit` be a hard keyword everywhere (preferred, simpler) or a contextual
  keyword valid only after `=` in a binding? Hard keyword avoids parser ambiguity;
  contextual avoids reserving a common name. Recommendation: hard keyword (consistent
  with `use core.mem` gating — it is always expert vocabulary).
- Partial-array init tracking: v1 uses conservative `MaybeInit` for any partial write.
  Future: per-index bitvector for fixed-size arrays where the size is a compile-time
  constant. Defer to v2.
- Should `uninit` bindings be forbidden from crossing `?` / error-propagation points?
  A write inside a fallible call might not execute if the call returns an error. Sema
  should treat fallible calls as non-writing for dataflow purposes (conservative).
