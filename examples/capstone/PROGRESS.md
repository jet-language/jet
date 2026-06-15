# Forge capstone — progress log (superseded by Jetpack)

> **Archived 2026-06-15:** Forge is superseded by Jetpack. Useful implementation
> ideas were saved in
> [`docs/plans/jetpack-jetos/forge-salvage.md`](../../docs/plans/jetpack-jetos/forge-salvage.md),
> and `examples/capstone/forge/` was removed per D-JPK6. Do not use this file as
> a package-manager implementation guide; use
> [`docs/plans/jetpack-jetos/README.md`](../../docs/plans/jetpack-jetos/README.md).

Living status so any agent can resume. Update after every meaningful step.
See PLAN.md for the full design + build order.

## How to run things (verified)

- Build compiler once: `nix develop -c cargo build`
- Run a Jet **file**: `nix develop -c jet run <file.jet>`
- Run/test a Jet **project** (has jet.toml): from repo root,
  `nix develop -c bash -c 'cd examples/capstone/forge/packages/ansi && JET_ROOT=<repo-root> jet test'`
  (the `jet` dev wrapper needs `JET_ROOT` or to be invoked from a dir under the
  repo so it can find `target/debug/jet`). Repo root =
  `/Users/nathanbrown/Documents/GitHub/jet`.

## Status

- [x] Step 0: Explored language; built compiler; verified single-file + package runs.
      NOTE: had to fix `src/main.rs:91` include_str path (examples moved to
      `examples/features/`). Mechanical fix, build green.
- [x] Step 1: Scaffold dirs + PLAN.md + PROGRESS.md.
- [x] Step 2: `ansi` subpackage + tests. 5/5 green (`jet test ansi.jet`).
- [x] Step 3: `manifest` subpackage + demo/forge.json + tests. 5/5 green.
      Loads/validates forge.json by walking the std `JSON` enum.
- [x] Step 4: `taskrunner` + tests. 6/6 green. DFS topo sort + cycle
      detection + generic `first_or`. Made leaf (no ansi dep) so it
      file-tests in isolation; color is applied by the app layer.
- [x] Step 5: `nixbridge` + demo/fixtures + offline tests. 5/5 green.
      Parses `nix build --json`, resolves tools in parallel over a channel,
      assembles PATH. Online path redirects nix output to a file (sidesteps
      the ProcessResult-field compiler bug). Tool reports are sorted by name
      after parallel resolution so expected outputs are deterministic.
- [x] Step 6: main `forge` package. Complete. `main.jet` owns CLI parsing and
      dispatch, with primitive/opaque APIs into the four leaf packages. Added
      ergonomic `use` and `shell` commands so users can run nixpkgs packages
      and enter project tool environments without writing Nix.
- [x] Added `forge.env.jet`, a Jet-native environment spec that regenerates
      `demo/forge.json` and updates `[tool.forge]` metadata in `jet.toml`.
      `forge sync` runs the generator; `forge shell` syncs before creating
      activation scripts.

### MAJOR architecture constraint (verified)

**A type can only be named in the file that defines it.** Not across packages
*and not even across files of one package* — neither `pkg.Type` (doesn't parse)
nor bare `Type` (E0119) resolves elsewhere. Consequences for the app:

- Values of a "foreign" type still flow fine by **inference**: `val p =
  manifest.load(..) or ...;` then call `manifest.x(p)`, read `p.pub_field`,
  `switch` on it. You just can't *write the type name* in another file.
- So packages must expose **primitive / opaque** public APIs: take & return
  `String`/`Int`/`Bool`/`List<prim>`/`Map<prim,prim>`, or take an opaque value
  the caller obtained from the same package and pass it straight back.
- Any function that needs to *name* a rich type must live in that type's file.
- The app's own `enum Command` + its parser + dispatcher must all live in ONE
  file (you can't pass a `Command` to a function in another file). Final shape:
  top-level `main.jet` owns the CLI and dispatch, depending on the 4 leaf
  packages whose rich types stay internal.

- [x] Step 7: Captured deterministic expected outputs under
      `forge/demo/expected/`, added `forge/README.md`, and completed the final
      run-through.

## Final verification (2026-06-14)

All commands below were run through `nix develop` with
`JET_ROOT=/Users/nathanbrown/Documents/GitHub/jet` where needed:

- `jet test ansi.jet` from `forge/packages/ansi`: 5 passed, 0 failed.
- `jet test manifest.jet` from `forge/packages/manifest`: 5 passed, 0 failed.
- `jet test taskrunner.jet` from `forge/packages/taskrunner`: 6 passed, 0 failed.
- `jet test nixbridge.jet` from `forge/packages/nixbridge`: 5 passed, 0 failed.
- `jet build main.jet` from `forge`: built `build/main`.
- `./build/main list --no-color`: matched `demo/expected/list.out`.
- `./build/main plan build --no-color`: matched `demo/expected/plan-build.out`.
- `./build/main run build --no-color`: matched `demo/expected/run-build.out`.
- `PATH=/usr/bin ./build/main env --no-color`: matched `demo/expected/env.out`.
- `PATH=/usr/bin ./build/main doctor --no-color`: matched `demo/expected/doctor.out`.
- `./build/main use jq --no-color -- command -v jq`: resolved a real
  `/nix/store/...-jq-.../bin/jq` path.
- `./build/main shell --no-color`, then `source build/forge-env.sh`: resolved
  real Nix store paths for `jq`, `rg`, and `hello`.
- `jet run forge.env.jet`: regenerated `demo/forge.json` and `jet.toml`.
- `./build/main sync --no-color`: ran the Jet env generator from the Forge CLI.

## Decisions made

- Project = **Forge**, a Nix-backed dev-env + task runner. Manifest is JSON
  (`forge.json`) because v1 std has no TOML parser.
- Nix path degrades gracefully + has an offline fixture mode so the green
  battery needs neither network nor Nix.

## Gotchas discovered

- `jet run`/`jet test` inside a project: the dev `jet` wrapper resolves the
  debug binary by walking up for `Cargo.toml`+`flake.nix`, or via `JET_ROOT`.
  When `cd`'d into a subdir of the repo it works; set `JET_ROOT` to be safe.
- v1 string escapes are `\n \t \" \\` only — **no `\u{...}`**. Build other
  bytes (e.g. ESC 0x1B) via `String.from_bytes(list)` where the list is a
  `List<U8>` built from `val b: U8 = 27;` bindings (a bare `[27]` is `List<Int>`
  and will NOT coerce to `List<U8>`).
- **Trait-value coercion is narrow:** concrete→trait works in an *annotated
  collection literal* (`val xs: List<Shape> = [Circle{..}, Square{..}]`, like
  example 25), but NOT at a `val x: Trait = Concrete{}` binding nor implicitly
  at a call site `f(Concrete{})`. Design dynamic dispatch around `List<Trait>`.
  → ansi dropped its `Styler` trait (uses a `Theme` struct w/ enabled flag);
  the trait+dispatch showcase lives in `taskrunner` (`List<Step>`).
- **Moving a non-`Copy` value (enum/struct) into a struct literal** triggers a
  rustc ICE under default param access. Fix at the Jet level: mark the param
  `take` when it's consumed, or `.clone()` when reading a non-`Copy` field out
  of a borrowed `self`. (Compiler-side this is arguably a P0, but out of scope
  for the capstone; the `take`/`clone` spelling is the idiomatic answer anyway.)
- `jet test <file.jet>` runs a single file's test blocks; `jet test` with no arg
  in a project looks for `main.jet`. Test library packages by file path.
- **JSON walking:** `.get(key)`/`["key"]` on a map bound directly from a
  `== Object(entries)` pattern mis-codegens as *list* indexing (rustc error).
  Funnel the map through a function that **returns `Map<String,JSON>`** so the
  binding is properly typed, then call `.get` on that typed value. Use `take`
  at call sites the compiler flags (L0201 implicit-clone warning tells you).
- **`?`/inference fuel bug (compiler):** a `Map<String,JSON>` (or other
  non-trivial) binding borrowed ~5+ times via helper calls that use `?`
  degrades — the binding's type falls back to `Int` and `?` then errors with a
  nonsensical "can't pass E into a function that returns E". Minimal repro
  saved in notes. Workarounds: (a) **annotate** the `val` types explicitly
  (raises threshold to ~6 borrows), (b) keep each function's `?` count and
  repeated-borrow count low by splitting into helpers, (c) don't reuse one
  `Map` binding many times. Pure-`Int` `?` chains of 8 are fine — it's tied to
  heavier type constraints. (Arguably a P0 sema bug; out of scope to fix here.)
  **Adopted rule for the whole capstone: use `value or return err(...)` for
  multi-step fallible flows, not `?`.** It avoids the bug AND attaches a precise
  error per step.
- **Cross-file/cross-package struct FIELD access needs `pub` on each field**
  (S18), not just `pub struct`. Mark exported structs' fields `pub`.
- `.get(stringKey)` on a map reached via **struct field** (`p.env.get(..)`) also
  mis-codegens as list-indexing → bind it to a typed local first
  (`val env: Map<String,String> = p.env.clone(); env.get(..)`).
- **switch** on a named variable uses that variable's name in arms
  (`switch e { e == Variant(x) -> ...}`); on an expression uses `it`
  (`switch f() { it == ok(v) -> ...}`).
- Don't write `for x in expr.field {` — the `{` is read as a struct literal.
  Bind first: `val items = expr.field.clone(); for x in items {`.
- **Cross-package access reaches `pub fn`s only, not types/static methods.**
  `otherpkg.SomeType` / `otherpkg.Type.static()` won't resolve from another
  package — expose free `pub fn`s for anything cross-package. (Single-file
  `jet test file.jet` also doesn't resolve a package's deps, so keep each
  subpackage leaf + independently testable; the top `forge` pkg owns the graph.)
- **Generic `==` over a type parameter is unsupported** — `x == item` where
  `x: T` errors even with `<T: Comparable>`/`Equatable` (it attempts structural
  equality). Generic *ordering* via example-25's `<T: Comparable>` + `>` works;
  for `==`, use a concrete type. Showcase generics with comparison-free generics
  (e.g. `fn first_or<T>(xs: List<T>, fallback: T) -> T`).
- A trailing comma is allowed in struct literals but NOT in `fn` parameter lists.
- `list.pop()` returns `T?` and can't be ignored — `val _ = list.pop() or "";`.
- A struct field can't be named `ok` (reserved). `expr or panic(...)` isn't a
  valid statement on its own — bind it: `val _x = expr or panic(...);`.
- **Nested `switch`-on-expression collides on the implicit `it`.** Switch on
  named locals instead (`val r = f(); switch r { r == ok(v) -> ... }`), or split
  a level into its own function.
- **`ProcessResult` field access is broken (compiler I2 bug):** `result.code` /
  `.output` / `.errors` codegen to `user_code` etc., but the std Rust struct
  uses unprefixed fields → rustc error. The `Expr::Field` node carries no type,
  so there's no low-risk codegen fix. **Workaround used in nixbridge:** never
  read `ProcessResult`; have the command redirect output to a temp file
  (`bash -c "nix build --json X > f"`) and `fs.read` the file; detect success by
  whether the file parses. Treat `process.run` as ok=spawned, err=not-found.
- The `jet run` launcher strips `--...` flags before the program sees them.
  Build Forge with `jet build main.jet`, then run `./build/main ... --no-color`
  when verifying Forge's own flags.
- Do not run multiple top-level `jet run main.jet ...` commands in parallel from
  the same package directory: they race on the shared `build/main` output. Leaf
  package tests can run in parallel because they build in separate directories.
- Real nixpkgs packages can expose outputs named `bin` rather than `out`
  (`nixpkgs#jq` does this on the current nixpkgs). `nixbridge` accepts either
  output name.
