# jetpack config — design brief (Phase 1 fluent-surface analysis)

> **What this is:** the ground-level design brief behind the jetpack pack-file
> surface. It records the working slice in `examples/jetos/`, the language/compiler
> walls found building it, the B1–B4 compiler bugs, and the two paths (A/B) to the
> *exact* fluent surface the owner asked for
> (`apps.installed.append([firefox, fastfetch, btop])`). It is the ground-level
> companion to the Phase 2 vision in
> `docs/plans/jetpack-jetos/jetos-design.md`.
>
> The directive-vs-fluent analysis here (Path A/B, D-JP1…5) feeds decision
> **D-JPK3** in the consolidated plan (docs/plans/jetpack-jetos/README.md); read
> the README first for phase order, then this brief for the detail.

## 1. What was asked for

A Nix-flake replacement, configured in Jet, with:

- a **minimal-by-default** `config.jet`;
- a clean, boilerplate-free library API — `apps.installed.append([firefox, …])`;
- **dendritic & modular** config (one file per feature);
- **import-tree** composition (drop a file in, it's wired automatically);
- an `nh`-class CLI: beautiful, informative, easy;
- a drop-in-or-better Nix experience;
- a comparison of the flake structure **with and without pure evaluation**.

## 2. What shipped today (`examples/jetos/`)

A complete, tested vertical slice, built only on **ratified v1 syntax**:

- **`lib/jetpack.jet`** — the module-facing directive API (`installed`,
  `packages`, `services`, `option`/`suggest`/`force`) + the merge engine
  (lists combine+dedup+sort; scalars resolve by `default < normal < force`;
  same-level conflicts reported with both source files) + canonical JSON
  render. 6 unit tests.
- **dendritic modules** under `modules/**` and a **host** under `hosts/`,
  each a liftable file returning `[JSON]` directives.
- **`generated/tree.jet`** — the import-tree (what `jetos sync` regenerates).
- **`jetos.jet`** — the `nh`-style CLI: `check/list/diff/switch/generations/
  sync/help`, colored, with a real `+`/`-`/`~` diff and generations.
- **`demo/verify.sh`** — green end to end.

A real machine config reads beautifully:

```jet
// modules/apps/desktop.jet
pub fn contribute() -> [JSON] {
    return [
        pkg.installed(["firefox", "fastfetch", "btop", "vlc"]);
        pkg.services(["pipewire", "bluetooth"]);
        pkg.suggest("sys.desktop.environment", "gnome");
    ];
}
```

This is **directive-style**, not the fluent `apps.installed.append([...])` the
owner sketched. §4 explains why, exactly, and §5 gives the path to the fluent
form.

## 3. Compiler fix + bugs found (do these next)

Building this surfaced real compiler issues. One was **fixed** here; the rest
are workarounds in the example and want proper fixes.

**Fixed — cross-module call paths (codegen).** A 2-level import graph
(`config → tree → {jetpack, modules}`) failed to compile: an imported file's
body is emitted into a nested `mod user_<x>`, but its calls to *other* imported
modules emitted bare `user_jetpack::…` instead of `super::user_jetpack::…`.
Fixed by applying the existing `root_prefix` at the cross-module call site and
the struct-literal site (`src/codegen.rs`). This is what makes multi-file,
multi-level Jet programs codegen at all — high-value beyond jetos.

**Open bugs (worked around in the example):**

| # | Symptom | Where it bit |
|---|---------|--------------|
| B1 | `JSON.Text(x)` where `x` is a *view* param moves it → rustc ICE (sema should insert a clone or reject; never ICE — I2/I3). | every directive constructor; worked around with `.clone()` |
| B2 | Field access on a **std** struct mangles the field name (`result.code` → `user_code`) → rustc ICE. | `process.run(...).code` — avoided by not touching the field |
| B3 | `.get(k)` on a `Map` bound via an `Object(root)` pattern lowers to **list indexing** (`"k".to_string() as usize`) → rustc ICE. Passing the map to a typed `Map` param fixes the dispatch. | `root.get("generations")` — routed through a helper |
| B4 | `for k, v in recv.field { … }` parses `recv.field {` as a **struct literal**. Ending the subject in `()` (`recv.field.clone()`) disambiguates. | `for k, v in sys.options` |

These are good first issues: each has a one-line repro and a clear expected
behavior.

## 4. The language walls (why directives, not a fluent builder)

A module file in v1 Jet **cannot**:

1. **name another file's struct type** — `fn f(cfg: jet.Config)` fails to parse
   (`expected ',' … found '.'`). Confirmed; also noted in `manifest.jet`.
2. **call another file's methods** — holding an imported value, `v.method()`
   is `E0102: no method` unless the method is defined in the *same* file.
3. **import with `..`** — file imports are sandboxed to the entry's folder.

Consequences for a shared, mutable `cfg`:

- A fluent `cfg.apps.installed.append([...])` needs `cfg` to be a value of a
  **shared type** passed between files with **methods** — walls #1 and #2 both
  forbid that. The previous "forge" capstone hit the same walls and degraded to
  funneling everything through JSON strings.
- What *does* cross a file boundary cleanly: **primitive types** and the
  **compiler-provided `JSON`** type. So a module returns `[JSON]`
  directives, and the merge engine (which owns no shared user type) folds them.
  This is exactly OS-I2 in `jetos.md` ("modules communicate only through
  declared options"), just realized with today's types.

Directives are therefore the honest v1 surface. They are clean and declarative —
but they are not the sketched fluent API.

## 5. Getting the exact fluent surface — two paths

The owner's sketch is `apps.installed.append([firefox, fastfetch, btop])`. To
make that work *cross-file*, the `apps`/`installed` values and their `.append`
method must be visible everywhere — which only **compiler-provided** types are.
Two ways to get there.

### Path A — first-party `jetpack` module (recommended, smaller)

Make `jetpack` a built-in module like `std.json`/`std.fs` (the M10 pattern:
typed signatures in sema + a Rust prelude template). Then its `Config`/scope
types and their methods are universal, and modules write the real thing:

```jet
import jetpack as apps;            // reserved root, per jetos.md

pub fn contribute(mut sys: jetpack.System) {
    sys.apps.installed.append(["firefox", "fastfetch", "btop"]);
    sys.services.enable(["pipewire"]);
    sys.set("sys.desktop.environment", "cinnamon");
}
```

- **Cost:** one new built-in module (types + methods + merge in Rust), plus
  resolving the "pass a built-in mutable struct between files" story (built-in
  types don't have the user-type wall). Reuses M10 machinery; no new *language*
  syntax, so no ballot needed beyond reserving the `jetpack` root (already
  reserved in jetos.md §10).
- **Keeps** today's `examples/jetos/` CLI, merge semantics, and tests; only the
  module-author surface changes from directives to methods.
- **Bonus:** fixing B1–B4 above is largely a prerequisite, so the work pays
  double.

### Path B — language-level options (`option`/`when`), the jetos.md vision

The full §5 of `docs/plans/jetpack-jetos/jetos-design.md`: `option a.b.c: T = default "doc"`,
`when expr { … }`, prefix priorities (`default`/`force`), a compiler option
registry + merge engine, and `jet eval --pure`. This is the grander design and
the right *end state*, but it's gated on unratified ballots (D-OS1…7) and on
M12 layer 3. It is **Path A plus new syntax plus pure-eval** — do it after A
proves the merge model on real configs.

**Recommendation:** ship **Path A** as the next milestone (call it the jetpack
config module). It delivers the exact fluent surface, needs no new syntax, and
de-risks Path B by hardening the merge engine first.

## 6. With vs. without pure evaluation (the requested comparison)

A Nix flake is **pure**: `outputs` is a function of its locked `inputs`; the
same inputs always evaluate to the same store paths; evaluation has no side
effects. Mapping that onto Jet:

| Concern | Without pure eval (today, shipped) | With pure eval (`jet eval --pure`, S60) |
|---|---|---|
| What evaluates the config | `jet build config.jet` → a native binary that prints the tree | the compiler interprets a verified-pure Jet subset; no codegen, no `rustc` |
| Speed | seconds (a full compile per change) | milliseconds (tree-walk), matching `tsc --watch`/dev mode |
| Purity guarantee | by convention — `config.jet` *should* be pure, but nothing stops a module calling `process.run` | enforced — `pure fn` rejects IO/time/random/FFI at compile time |
| Reproducibility | depends on the author not reaching for IO | structural: identical inputs ⇒ identical tree, always |
| Caching / `--as-of` | not sound (a module could read the clock) | sound — pure inputs are content-addressable, so generations and `switch --as-of <date>` are trustworthy |
| Flake parity | "flake-shaped": locked source + modules → merged tree → provider | true flake semantics: a pure function of locked inputs |

The structure is the **same** either way — `inputs` (the nixpkgs source +
lockfile) → modules → merge → system tree → provider. Pure evaluation doesn't
change the shape; it upgrades the *guarantee* from "the author was careful" to
"the compiler proved it," which is exactly what makes Nix's caching and
rollback trustworthy. Today's slice is the without-pure version; Path A keeps
it; Path B (with `pure fn` modules + `jet eval --pure`) closes the gap.

## 7. Open decisions for the owner

| ID | Question | Options | Rec |
|----|----------|---------|-----|
| D-JP1 | Next milestone for the fluent surface | A: first-party `jetpack` module · B: jump to `option`/`when` + pure-eval | **A** |
| D-JP2 | Module author surface in v1 | keep directives (`pkg.installed([...])`) · wait for Path A | ship directives now, swap to A when ready |
| D-JP3 | Fix B1–B4 before or with Path A | before (they block clean codegen) · alongside | **before** — they're small and unblock all multi-file Jet |
| D-JP4 | Supersede the `forge` capstone | keep both · replace forge with jetos | replace — two "Nix in Jet" capstones is confusing; jetos is the better one |
| D-JP5 | Package names as bare idents (`firefox`) vs strings (`"firefox"`) | bare needs a generated `pkgs.*` namespace from a nixpkgs index · strings are honest for 100k pkgs | strings in v1; revisit a typed `pkgs` namespace with Path A |
