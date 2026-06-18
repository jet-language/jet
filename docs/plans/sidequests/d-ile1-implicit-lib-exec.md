# D-ILE1 — Implicit lib/exec inference

**Status: ratified 2026-06-18 (option A)** — recorded in `syntax-decisions.md`;
ready to implement. Amends U10 / D-JPK-FILES.

Package **kind is inferred from `fn main()` presence**, not required. The owner's
framing: a `pkg.jet` is a package *definition* the user can manage by hand; the
`kind` (library | executable) is inferred unless stated. Two levels:

- **No `pkg.jet`** — a file/dir with a top-level `fn main()` is an **executable**;
  without one, a **library**.
- **With `pkg.jet`** — in the `packages: { … }` block (U10) `kind` is **optional**:
  a module with `fn main()` is `executable`, otherwise `library`. The user may
  still write it explicitly (`deploy: executable`) to override or document.

## Plan

1. **`src/loader.rs` / core provider** — when no `pkg.jet` is found, scan the
   entry file (and package-root files) for a top-level `fn main()`:
   - found → compile as executable;
   - not found → compile as library (cdylib/rlib).
2. **`pkg.jet` `packages:` parsing** (`src/manifest.rs` / jetpack manifest) —
   make the per-package `kind` optional. When omitted, resolve it by inference:
   look up the named module; `fn main()` present → `executable`, else `library`.
   An explicit `library`/`executable` always wins.
3. **Sema** — if an inferred (or declared) executable has a wrong `fn main()`
   signature (arity/return type) → `E_MAIN_SIG`.
4. **Sema** — two top-level `fn main()` in one inferred package (no `pkg.jet`) →
   `E_DUPMAIN` (help: add a `pkg.jet` `packages:` block to split them).
5. **`jet new`** still scaffolds `pkg.jet`; inference is the no-/partial-manifest
   fallback. `jet run file.jet` stays zero-ceremony (U7).
6. **Example** — `examples/simple_exec/`: single file, no `pkg.jet`, `fn main()`.
7. **Tests** — golden test for `examples/simple_exec/`; ui snapshots for
   `E_DUPMAIN` and `E_MAIN_SIG`; a `pkg.jet` fixture exercising inferred vs
   explicit `kind`.
8. **Diagnostics** — claim `E_DUPMAIN` and `E_MAIN_SIG` in `docs/spec/diagnostics.md`
   first (I4), with a ui snapshot each.

## Out of scope

- The `pkg.jet`/`jetpack.toml` file-structure rename (tracked in
  `d-jpk-files-structure.md`). This sidequest assumes that layout and only adds
  kind inference on top of it.
