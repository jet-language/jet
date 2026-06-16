# C header auto-binding (`jet bind`)

**Status:** bind **engine** — pairs with **E2-M14** (S59).

**Spec:** [`m14-c-ffi.md`](../epoch-2/m14-c-ffi.md) · All CBIND picks ratified.

---

## Pipeline

```
C header  →  jet bind  →  @bindgen module c.<lib>.__bindgen__ { … }
          →  .jet/bindings/c/<lib>.jet
          →  merge @extern overlay  →  extern "C"  →  link
```

Compile/build **auto-invokes** bind on cache miss; **`jet bind`** subcommand for manual refresh (**D-CBIND2**). Backend: **bindgen** helper crate (**D-CBIND3**, I6 waiver).

---

## Ratified

| ID | Decision |
|---|---|
| D-CBIND2 | Auto on compile + **`jet bind`** subcommand |
| D-CBIND3 | Bindgen helper (I6) |
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
