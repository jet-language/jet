# C header auto-binding (deferred post–Epoch 2)

**Status:** deferred — **not** in Epoch 2 scope. Owner direction: explore after
E2-M17 GA unless promoted earlier.

**Depends on:** E2-M13 (low-level tier / `Ptr<T>`), E2-M14 (manual `extern c`,
`[dependencies:c]`, link diagnostics). Does not replace S59's ratified primary
surface: hand-written `extern c` blocks remain the semantic floor.

**Related:** S59 rejected bindgen-style auto-generation as the **primary** FFI
surface for v2. This plan covers an **optional convenience layer** on top of
manual `extern c`, not a parallel calling convention.

---

## Goal

Let users approach a Swift-like `import raylib` experience **without** making
clang/bindgen a compiler dependency or bypassing sema:

```
C header  →  [translation tool]  →  Jet source (structs + extern c)  →  sema  →  Rust extern "C"  →  link lib
```

The translation layer is analogous to Jetpack's provider translators (D-JPK5),
not to Jet→Rust codegen: it **generates Jet declarations** that sema checks.

---

## Out of scope for Epoch 2 (E2-M14)

E2-M14 ships **manual** `extern c` only. Explicitly deferred to this plan:

- Compile-time `import c "raylib.h"` magic
- `jet bind` / `jet-bind` header scraping
- clang/libclang or bindgen inside the `jet` compiler (I6)
- Registry policy requiring auto-generated bindings
- Macro expansion beyond documented stubs

Epoch 2 may ship a **small C library example** with hand-written bindings (e.g.
a tiny libc helper). Registry packages (e.g. `jet-raylib`) with curated
hand-written bindings are encouraged and do not require this milestone.

---

## Owner decisions — ratify before any implementation

| ID | Question | Options | Rec |
|---|---|---|---|
| D-CBIND1 | Primary surface | **A** manual `extern c` only; tool is optional · **B** tool-generated `.jet` checked into tree · **C** compile-time `import c` | **B** |
| D-CBIND2 | Tool location | **A** `jet bind` subcommand · **B** separate `jet-bind` binary (I6-friendly) · **C** compiler-integrated | **B** |
| D-CBIND3 | AST engine | **A** system `libclang` · **B** vendored bindgen in isolated crate (jetpack-style I6 waiver) · **C** no AST; packages only | **B** |
| D-CBIND4 | Pointer mapping | **A** `Ptr<T>` (S58) · **B** opaque newtypes · **C** reject raw pointers at boundary | **A** |
| D-CBIND5 | `char*` default | **A** copy to `String` · **B** `Ptr<u8>` · **C** per-function annotation in output | **C** |
| D-CBIND6 | Macros | **A** skip + manual stubs · **B** `#define` constants only · **C** full cpp | **A** |
| D-CBIND7 | Cache dir | **A** `.jet/c-bindings/<hash>/` · **B** `~/.jet/bindings/` · **C** always write into project | **A** |
| D-CBIND8 | Registry role | **A** encourage curated binding packages · **B** require bind manifest in packages · **C** neither | **A** |

Open follow-ups (promote to ballots when planning starts): function-pointer
policy, callback wrapping, cross-target `#ifdef` splits, who owns generated-file
license headers.

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

Generated output is ordinary Jet: `struct` layouts + `extern c { ... }` blocks
users can edit. Bad translations are fixed in the `.jet` file, not hidden in the
compiler.

---

## Invariants

- **CBIND-I1** Generated bindings must parse through the same lexer/parser/sema
  as hand-written `extern c`.
- **CBIND-I2** No generated `unsafe` in user-visible Jet; pointers use S58
  gates.
- **CBIND-I3** Translation failures are Jet diagnostics (what/why/fix), not raw
  clang spew (R5 spirit).
- **CBIND-I4** The `jet` compiler binary stays std-only; AST tooling lives in
  `jet-bind` or an isolated feature crate (I6).

---

## Exit criteria (when promoted to a numbered milestone)

- `jet-bind` emits a `.jet` file from a fixture header; output compiles via
  E2-M14 `extern c`.
- Transcript or snapshot tests for a minimal C library (e.g. `add(int,int)`).
- Documented policy for `char*`, opaque pointers, and skipped macros.
- At least one registry package documents "hand-written vs `jet-bind`"
  maintenance expectations.
