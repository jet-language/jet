# E2-M14 — C FFI

**Status:** draft — **blocked on D-CFFI1…D-CFFI3** (Group M14). Implements the
ratified S59 C-FFI gate.
**Depends on:** E2-M13 (pointer rules + `unsafe` gates carry the boundary).
Unblocks sqlite in the E2-M9 ring and TLS in E2-M10.
**Error codes:** E32xx block (claim in docs/spec/diagnostics.md).

## Goal

Connect Jet to the non-Rust ecosystem **without importing C's unsafety into
ordinary Jet**. The by-value boundary is safe; pointers cross only through the
E2-M13 gates. Rust FFI (`extern rust`) is unchanged.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred |
|---|---|---|---|
| D-CFFI1 | Jet-export to C in scope? | **A** — import-only first | A |
| D-CFFI2 | Header/library discovery | **A** — pkg-config + classic flags from `[dependencies:c]` | A |
| D-CFFI3 | C example to ship | **A** — one small C lib (e.g. a hash/compression lib) | A |

## Scope (from S59)

- **`extern c "header-or-lib" { … }`** blocks mirroring `extern rust`.
- **By-value boundary first** — scalars and `repr`-C structs cross by value
  safely without an `unsafe` block.
- **Pointers only through E2-M13 rules** — a C function taking/returning a
  pointer requires `import std.mem` + `unsafe`.
- **Linker flags/dependencies** from `[dependencies:c]` in `jet.toml`
  (pkg-config + classic flags, D-CFFI2).
- **Header/library discovery diagnostics** — a missing header or lib becomes a
  Jet diagnostic, not a raw linker error, where possible.
- **Jet-export to C (D-CFFI1)** — import-only first; exporting Jet functions for
  C callers is deferred unless the owner promotes it.

## Surface (example)

```jet
import std.mem;

extern c "xxhash" {
    fn XXH64(input: Ptr<U8>, len: USize, seed: U64) -> U64;  // pointer => gated
    fn XXH_versionNumber() -> Int;                            // by-value => safe
}

fn version() -> Int = XXH_versionNumber();   // no unsafe needed (by-value)
```

```toml
[dependencies:c]
xxhash = { pkg-config = "libxxhash" }
```

## Diagnostics to register

- **E3201** C header/library not found (names what pkg-config/flags were tried,
  suggests `[dependencies:c]`).
- **E3202** pointer crosses the C boundary outside an `unsafe`/`std.mem` gate
  (reuses E31xx voice).
- **E3203** non-C-ABI type used by value across `extern c` (names the field).

## Examples & tests

- `examples/features/49_cffi.jet` — calls a small C library (D-CFFI3) and prints
  a result; expected output golden-tested.
- ui fixtures for E3201–E3203.
- A test proving `extern rust` behavior is unchanged.

## Out of scope (deferred post–Epoch 2)

- **C-header auto-binding** (`jet bind`, compile-time `import c`,
  clang/bindgen in the compiler) — see docs/plans/post-epoch-2/c-header-bindings.md
  (D-CBIND1…8). Registry packages may ship hand-written bindings meanwhile.
- C++ ABI, name mangling, templates.
- Callbacks from C into Jet beyond the simplest by-value case (with export, if
  D-CFFI1 promoted).
- Varargs C functions.

## Exit criteria

- An example calls a small C library and prints a correct result.
- Pointer misuse is rejected unless inside the low-level gates.
- C build/link failures become Jet diagnostics where possible.
- Rust FFI remains unchanged.
- `nix develop -c cargo test` green.
