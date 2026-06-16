# C header auto-binding

**Status:** Epoch 3 pillar — see [`README.md`](README.md).

**Depends on:** E2-M13 (low-level tier / `Ptr<T>`), E2-M14 (manual `extern c`,
link diagnostics). Does not replace S59's primary surface: hand-written
`extern c` blocks remain the semantic floor.

**Related:** S59 rejected bindgen-style auto-generation as the **primary** FFI
surface. This plan is an **optional convenience layer** on top of manual
`extern c`.

---

## Goal

Swift-like `import raylib` ergonomics **without** clang inside the `jet`
compiler:

```
C header  →  jet-bind  →  Jet source  →  sema  →  Rust extern "C"  →  link
```

---

## Out of scope for Epoch 2 (E2-M14)

E2-M14 ships **manual** `extern c` only:

- Compile-time `import c "raylib.h"` magic
- `jet bind` inside the compiler (I6)
- Macro expansion beyond documented stubs

Registry packages may ship **hand-written** bindings without this pillar.

---

## Owner decisions — ratify before implementation

| ID | Question | Options | Rec |
|---|---|---|---|
| D-CBIND1 | Primary surface | **A** manual only · **B** tool-generated `.jet` in tree · **C** compile-time `import c` | **B** |
| D-CBIND2 | Tool location | **A** `jet bind` · **B** separate `jet-bind` binary · **C** compiler-integrated | **B** |
| D-CBIND3 | AST engine | **A** system libclang · **B** vendored bindgen (I6 waiver) · **C** packages only | **B** |
| D-CBIND4 | Pointer mapping | **A** `Ptr<T>` · **B** opaque newtypes · **C** reject pointers | **A** |
| D-CBIND5 | `char*` default | **A** `String` · **B** `Ptr<U8>` · **C** per-function in output | **C** |
| D-CBIND6 | Macros | **A** skip + stubs · **B** `#define` only · **C** full cpp | **A** |
| D-CBIND7 | Cache dir | **A** `.jet/c-bindings/<hash>/` · **B** `~/.jet/bindings/` · **C** project only | **A** |
| D-CBIND8 | Registry role | **A** encourage curated packages · **B** require bind manifest · **C** neither | **A** |

---

## Likely shape (if D-CBIND1=B, D-CBIND2=B)

```bash
jet-bind raylib.h --pkg raylib -o bindings/raylib.jet
```

```jet
import "bindings/raylib.jet";

fn main() {
    raylib.init_window(800, 600, "hi");
}
```

Generated output is ordinary Jet — users edit the `.jet` file, not hidden compiler state.

---

## Invariants

- **CBIND-I1** Generated bindings parse through normal sema.
- **CBIND-I2** No user-visible generated `unsafe`; S58 gates for pointers.
- **CBIND-I3** Translation failures are Jet diagnostics (R5).
- **CBIND-I4** `jet` compiler stays std-only; AST tooling in `jet-bind` (I6).
