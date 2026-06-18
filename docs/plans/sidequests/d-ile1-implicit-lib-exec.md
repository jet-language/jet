# D-ILE1 — Implicit lib/exec inference

**Status: done 2026-06-18 (option A).** Implementation notes vs. the original plan:

- **Inference** lives in the core provider (`src/jetpack/provider.rs`):
  `package_kind` returns `None` both when a package is unlisted *and* when its
  `packages:` entry omits `kind`; the provider then infers — a non-empty staged
  `bin/` or a **top-level `fn main`** (lexer-scanned, so comments/strings never
  false-match) ⇒ executable, else library. An explicit `library`/`executable`
  wins.
- **`pkg.jet` `kind`-optional** (`src/jetpack/packmanifest.rs`): a bare
  `packages: { deploy, web: library }` parses (`PackageEntry.kind: Option`).
- **Diagnostics dissolved** (owner, 2026-06-18): the planned `E0989` (two
  `fn main`) and `E0990` (bad `main` signature) duplicate existing **E0105**
  (`main` defined twice) and **E0122** (`main` takes no params / returns
  nothing) — reused, no new codes (I8).
- **R9 interpretation:** single-file `jet run`/`build file.jet` stays
  executable-requiring (E0101 if no `main`); the bare-file→library path is not
  added. Library inference is served by the provider/`pkg.jet` package route,
  which already has a library build path.
- **Example/tests:** `examples/simple_exec/main.jet` (+ `tests/cli.rs`
  `simple_exec_runs_without_a_manifest`); unit tests
  `packmanifest::…package_kind_is_optional_and_inferred` and
  `provider::…top_level_main_drives_kind_inference`.

Original plan below (kept for reference). Amends U10 / D-JPK-FILES.

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
   signature (arity/return type) → **`E0990`**.
4. **Sema** — two top-level `fn main()` in one inferred package (no `pkg.jet`) →
   **`E0989`** (help: add a `pkg.jet` `packages:` block to split them).
5. **`jet new`** still scaffolds `pkg.jet`; inference is the no-/partial-manifest
   fallback. `jet run file.jet` stays zero-ceremony (U7).
6. **Example** — `examples/simple_exec/`: single file, no `pkg.jet`, `fn main()`.
7. **Tests** — golden test for `examples/simple_exec/`; ui snapshots for
   `E_DUPMAIN` and `E_MAIN_SIG`; a `pkg.jet` fixture exercising inferred vs
   explicit `kind`.
8. **Diagnostics** — claim `E0989` (duplicate `main`) and `E0990` (bad `main`
   signature) in `docs/spec/diagnostics.md` first (I4), with a ui snapshot each.

## Out of scope

- The `pkg.jet`/`jetpack.toml` file-structure rename (tracked in
  `d-jpk-files-structure.md`). This sidequest assumes that layout and only adds
  kind inference on top of it.
