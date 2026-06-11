# M7 — Rust FFI (interop tier)

**Blocked on decisions:** S50 (extern syntax). Depends on M5 (types worth
passing) and M6 (multi-file driver work).
**Error codes:** E0701+.

## Goal

Call vetted Rust functions across an **owned/copied boundary** — interop
without importing Rust's type system (philosophy C2). Jet stays small;
the escape hatch is explicit, visible, and safe.

## Surface (uses ballot recommendation — substitute ratified choice)

```jet
extern rust "rand@0.8" {
    // Jet signature; the body lives in the named crate.
    fn random_range(low: Int, high: Int) -> Int = "rand::random_range";
}

fn main() {
    print(random_range(1, 6));
}
```

- `extern rust "<crate>[@<version>]" { … }` declares a block of foreign
  functions. Each entry is a normal Jet signature plus `= "rust::path"`
  naming the target item. Version pins are required for non-std
  (E0701) — reproducibility without a manifest.
- `extern rust "std" { … }` works for std items with no dependency.
- Allowed boundary types (both directions): `Int`, `Float`, `Bool`,
  `String`, `Char`, `List[…]`/`Map[…]`/`T?`/`T or E` **of allowed
  types**, and user structs/enums whose fields are allowed. Everything
  passes **by value** (move/copy/clone at the boundary; M2 call rules
  apply — default params still read, so codegen clones into the call).
  No `mut`/`view`/`ref`, no callbacks/closures, no trait objects, no
  lifetimes (E0702 — "this type can't cross into Rust"; the message
  suggests wrapping/flattening).
- **Safety stance:** extern functions are trusted to be safe Rust
  (I1 still bans `unsafe` in *generated* code; the foreign crate is the
  vetted exception, stated in docs). A panic inside the foreign call is
  caught at the boundary and becomes the M4 runtime report (exit 70),
  so foreign bugs still never print rustc/Rust panics raw.

## Driver & build model

This is the milestone's real work. When a program's import graph
contains any `extern rust` with a crate dependency:

1. The driver materializes a **hidden** cargo project under
   `~/.cache/jet/ffi/<hash-of-deps>/` (Cargo.toml generated from the
   pinned crate list, src/lib.rs containing thin `pub fn` wrappers that
   do the type conversions and `catch_unwind`).
2. `cargo build --release` runs once per dep-set (cached thereafter);
   absence of cargo → E0703 with install instructions.
3. The user's generated .rs is compiled as before with `--extern` flags
   pointing at the built rlibs. The user-facing flow is still just
   `jet run file.jet` (R9: no manifest, no cargo project in the user's
   directory, ever).
4. Network/build failures from cargo are reported as **tool errors**
   (E0704: "couldn't fetch `rand@0.8`") quoting cargo only inside an
   indented block — cargo never speaks raw (extends R5's spirit).

Sema validates everything it can (signature types, duplicate externs →
E0105); a wrong `rust::path` or wrong foreign signature surfaces as a
build failure of the *wrapper* crate → reported as E0705 naming the
extern line ("the Rust item `rand::random_range` doesn't match this
signature"), never as an ICE — this is the one place rustc errors map
to user errors, because the user asserted a foreign fact.

## Diagnostics to register

E0701 missing version pin · E0702 type can't cross the FFI boundary ·
E0703 cargo not installed · E0704 dependency fetch/build failed ·
E0705 foreign signature mismatch.
Teaching: E0031 `unsafe` → not in Jet; whole blocks live behind
`extern rust`.

## Examples & tests

- `examples/18_ffi.jet` — calls a real crate function (pick one with a
  tiny dep tree and deterministic output for goldens — e.g. a hashing or
  base64 crate rather than rand).
- ui fixtures for E0701/E0702/E0705 (+ fixed companions).
- Tests gate on cargo presence (skip with a notice when absent), mirror
  of the existing rustc-presence gating.

## Out of scope

Exporting Jet to Rust, callbacks/closures over the boundary, borrowed
returns, async, raw pointers, build.rs crates with system deps (document
"pure-Rust crates only" in the error text when the build fails),
auto-binding generation. Registry/manifest integration is M12 (the
manifest will later list FFI deps; the inline pin keeps working).
