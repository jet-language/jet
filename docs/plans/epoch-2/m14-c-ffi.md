# E2-M14 — C FFI

**Status:** draft — **blocked on D-CFFI1…D-CFFI3** (Group M14). Implements the
ratified S59 C-FFI gate (amended D-CFFI2).
**Depends on:** E2-M13 (pointer rules + `@unsafe` gates carry the boundary).
Unblocks sqlite in the E2-M9 ring and TLS in E2-M10.
**Error codes:** E32xx block (claim in docs/spec/diagnostics.md).

## Goal

Connect Jet to the non-Rust ecosystem **without importing C's unsafety into
ordinary Jet**. D-language-grade ergonomics: declare, link, run — with Jet
ownership at the boundary and **hangar-pinned deps when Jetpack is present**.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-CFFI1 | Jet-export to C in scope? | **A** — import-only first | A | ✅ ratified 2026-06-16 — A: import-only C FFI first |
| D-CFFI2 | Header/library discovery + syntax | **A** — hangar if dep, else pkg-config; `extern c raylib { }` | A | ✅ ratified 2026-06-16 — see S59 + syntax below |
| D-CFFI3 | C example to ship | **A** — raylib showcase | A | ✅ ratified 2026-06-16 — ship a raylib showcase |

See also [`c-ffi-syntax-examples.md`](c-ffi-syntax-examples.md) for variant A/B/C/D/E
raylib walkthroughs. **Surface syntax ballot:** [`docs/spec/decision-ballots.md`](../../spec/decision-ballots.md)
(**D-CFFI2-SYN**, options A–I) — blocked until owner pick.

**Provisional spelling (ballot A — re-open for review):**

```jet
extern c raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}

fn main() {
    init_window(800, 600, "pong");
    close_window();
}
```

**Resolution order:**

1. Jetpack / `payload.jet` has matching dep → **hangar** (content-hash pinned).
2. Else → **`pkg-config raylib`** (system install).
3. Missing → **E3201** naming both fixes.

**Explicit overrides:**

```jet
extern c system raylib { … }    // force system pkg-config
extern c hangar raylib { … }    // force hangar dep
```

**Single-file story (Sam, indie dev):**

```bash
$ jet run pong.jet
# no payload.jet → pkg-config raylib
# clear error if absent: install system lib OR add dep to payload.jet
```

**Team story (Jetpack):**

```jet
// payload.jet
deps: { raylib: nixpkgs:raylib#5.5.0 }
```

```bash
$ jetpack build    # hangar realizes exact hash; no system raylib
```

By-value calls need no `@unsafe`; pointers require `import std.mem` + `@unsafe`.

## Scope (from S59)

- **`extern c <link-name> { … }`** blocks (identifier preferred).
- **By-value boundary first** — scalars and `repr`-C structs without `@unsafe`.
- **Pointers only through E2-M13** — `@unsafe` + `Ptr<T>`.
- **Linker discovery** per D-CFFI2 (hangar → pkg-config).
- **Jet-export to C** — import-only first (D-CFFI1).

## Diagnostics to register

- **E3201** C library not found — tried hangar key + pkg-config; suggest dep or install.
- **E3202** pointer crosses boundary outside `@unsafe` / `std.mem` gate.
- **E3203** non-C-ABI type used by value across `extern c`.

## Examples & tests

- `examples/features/49_cffi.jet` — raylib or small C lib (D-CFFI3).
- ui fixtures for E3201–E3203.

## Out of scope (Epoch 3)

- **C-header auto-binding** — [`docs/plans/epoch-3/c-header-bindings.md`](../epoch-3/c-header-bindings.md)
- C++ ABI, varargs, callbacks from C into Jet (without export promotion).

## Exit criteria

- Example calls a C library with correct output.
- Hangar + pkg-config paths both tested.
- Pointer misuse rejected outside gates.
- Rust FFI unchanged.
- `nix develop -c cargo test` green.
