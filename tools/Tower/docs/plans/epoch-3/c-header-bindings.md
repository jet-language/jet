# C header auto-binding (`jet bind`)

**Status:** ✅ shipped in **E2-M14** (S59) — `jet bind` is functional with a
**native std-only backend** (`Source/CBind.rs`). The remaining E3 work here is
optional: compile-time auto-invoke on cache miss (D-CBIND2's auto half) and
widening the bound type subset beyond function prototypes + scalars/`char*`.

**Spec:** the C FFI section of [`spec.md`](../../spec/spec.md) · All CBIND picks ratified.

---

## Pipeline

```
C header  →  jet bind  →  @bindgen module c.<lib>.__bindgen__ { … }
          →  .jet/bindings/c/<lib>.jet
          →  merge @extern overlay  →  extern "C"  →  link
```

**`jet bind`** subcommand generates the cache from a header (**D-CBIND2**);
compile-time auto-invoke on cache miss is the remaining E3 half. Backend:
**native std-only C-prototype parser** (`Source/CBind.rs`) — owner 2026-06-18
superseded the bindgen-crate route (**D-CBIND3**); no external crate, no libclang.

---

## Ratified

| ID | Decision |
|---|---|
| D-CBIND2 | Auto on compile + **`jet bind`** subcommand (`jet bind` ✅; auto-on-compile = E3) |
| D-CBIND3 | ~~Bindgen helper (I6)~~ → **native std-only parser** `Source/CBind.rs` (owner 2026-06-18) |
| D-CBIND5 | **`String`** at C string boundary |
| D-CBIND6 | **`#define` constants only**; skip function-like macros |
| D-CBIND1 / 4 / 7 / 8 | Generated cache, `Ptr<T>`, `.jet/bindings/c/`, curated packages |

---

## CLI

```bash
jet bind raylib.h --pkg raylib -o .jet/bindings/c/raylib.jet
```

---

## Invariants

- **CBIND-I1** Generated bindings parse through normal sema.
- **CBIND-I2** No user-visible generated `unsafe`; S58 gates for pointers.
- **CBIND-I3** Translation failures → **E3208**.
- **CBIND-I4** `jet` compiler std-only; bind tooling in helper (I6).
