# Library reuse and linking

**Card:** #1421 (plan) → #1422 (build). **Ratified:** D-LIB-REUSE1=B,
D-LIB-EXPORT1=C, D-LIB-DYNTRUST1=A, D-LIB-NAME1=A (2026-08-06).
**Status:** ratified, unbuilt.

## Goal

Say how Jet reuses compiled dependency work, and what Jet hands to the world as
a compiled library. Both halves are now ratified. Nothing here is open.

## Words used here

- **Artifact** — a file a build produces and a later build can reuse.
- **Sealed package object** — one dependency compiled once, stored under a key,
  and restored instead of recompiled.
- **Artifact identity** — the exact key. It covers package sources, dependency
  artifact digests, compiler identity, target, and profile.
- **ABI** (application binary interface) — the fixed byte layout and calling
  rules two separately compiled programs must agree on to link.
- **Public stable ABI** — a promise that this layout never breaks, so a library
  compiled years ago still links today.
- **Version-keyed reuse** — no layout promise at all. Two artifacts link only
  when their keys match exactly, so layout can change freely between versions.
- **Static link** — the library's code is copied into the program at build time.
- **Dynamic load** — the program opens the library file while it runs.

## What exists today

Verified in the tree, not assumed.

- The build cache is **whole-program**. `CanonicalAST::ast_cache_key` keys on
  `SHA256(canonical AST bytes + profile + compiler version)` and stores the
  finished binary under `~/.cache/jet/build/<key>/bin`
  (`crates/jet-foundation/src/CanonicalAST.rs`). It reuses nothing per package.
- The **store, cache, and mirror layer is designed and partly built**: action
  identity (E4-JP2), binary cache and mirrors (E4-JP5, card #395, phase
  building), and the ratified law in D-JPK-CACHECONFIG1=D, D-JPK-CACHEAUTH1=D,
  D-JPK-STOREBACKEND1=D, and D-JPK-REPROCACHE1=D.
- `Library` is already **one of nine ratified output kinds** (D-ECO-OUTPUT-KINDS1=A,
  D-ECO-OUTPUT1=A). A package declares `outputs: .{ core: .Library.{ … } }`
  (D-CONF-NAME1=A). What that output *becomes as a file* was undecided until
  D-LIB-EXPORT1 and D-LIB-NAME1.
- Jet already ships a **sandboxed plugin mechanism**: `target: plugin` compiles
  to a WebAssembly component (D-PLUGIN1=B, D-DEP-WASM1=A, D-PLUGIN-EXPORT1=A,
  card c81). Its exported surface is the entry module's `pub` items, frozen by
  `Sema::ApiFreeze`. A plugin runs with **zero host capabilities**, and E1258
  states there is no gate to widen that.
- The FFI direction that works today is **inbound**: Jet consumes C, Python,
  Dart, and Rust libraries through generated bridges
  (`crates/jet-pkg-model/src/FFI.rs`, `CFFI.rs`). Nothing exports Jet as a
  native library for another language to call.

Two facts shaped this plan:

1. The pinned dynamic library overlaps the sandboxed wasm plugin. Both serve
   plugins, and I8 allows one mechanism. D-LIB-DYNTRUST1=A settles the boundary
   between them.
2. The example diagnostic in the ratified reuse ballot, `E0912`, is **retired**
   (`docs/spec/diagnostics.md`, retired by D-MEM1/S2). The version-mismatch
   error needs a fresh registered code and a UI snapshot (I4).

## Ratified law: version-keyed reuse, no public ABI

D-LIB-REUSE1=B is law. It has two halves.

**Half one — sealed package objects.** Each dependency compiles once into an
artifact keyed on exact identity. Later builds restore it. A compiler upgrade
empties the cache and rebuilds the world once, automatically. Generic function
bodies travel inside the artifact as typed intermediate code and instantiate at
the use site, so generics do not force source distribution. No cache path skips
parsing, sema, policy, or diagnostics.

**Half two — pinned Jet dynamic libraries.** A Jet program can load a Jet
library file at run time. Both sides must carry identical compiler identity. A
mismatch is a checked error before the file is mapped, never a crash.

### What each neighbour pays, and what Jet takes

| | Promise | Cost | Jet's position |
|---|---|---|---|
| **Rust** | none | every project recompiles every dependency from source; clean builds and CI pay full price | Jet refuses the recompile tax. Version-keyed reuse gets the speed without the promise. |
| **Swift** | public stable ABI since Swift 5 | permanent tax: resilient public types pay indirection forever, and layout choices are frozen before the language settles | Jet refuses the promise. Greenfield law already forbids a compatibility baseline until the owner declares one. |
| **Nix** | none; identity does the work | a changed input rebuilds | Jet copies this directly. The ratified store already works this way. |

The accepted loss is stated in the ratified tradeoff: **no artifact survives a
compiler upgrade.** Every upgrade rebuilds or re-downloads once. Correctness
never depends on layout luck.

## (a) Reuse model — cache tiers

No new law is needed here. The tiers already exist in ratified jetpack
decisions; sealed package objects are simply another object kind flowing
through them.

- **Local** — the machine's own store. First build fills it; the second build
  restores.
- **Team** — a bound mirror. `jet cache bind` maps roles to ordered endpoints
  with typed credential providers (D-JPK-CACHECONFIG1=D). A repository never
  names an endpoint or a key.
- **Public** — further mirrors in the same ordered list. The first hit that
  verifies digest and signature against the role's trust roots wins, wherever it
  came from. Location grants no trust.

Two ratified rules carry over unchanged and must not be re-encoded for this
artifact kind:

- **Writer authority** (D-JPK-CACHEAUTH1=D): shared namespaces accept only
  allowlisted builders, every upload carries signed provenance, and consumers
  re-verify on every hit.
- **Unreproducible outputs** (D-JPK-REPROCACHE1=D): divergent bytes land in an
  untrusted namespace and taint anything downstream that opts into them.

Worked example. Priya's three services share twelve packages.

```
$ jet build                 # first machine, cold
  http 2.1: compile (1.8s)
  json 1.4: compile (1.1s)
  flightdeck: compile + link (3.1s)

$ jet build                 # her teammate, same team mirror
  http 2.1, json 1.4: restored from cache (0.2s)
  flightdeck: compile + link (3.1s)

$ jet build                 # after upgrading the compiler
  cache empty for Jet 1.4.2 — rebuilding dependencies once
```

**Relation to incremental compilation.** Sealed objects are the link and restore
layer. They do not replace module-level sema dirty sets (#1026), and they must
not pre-empt D-INCR-UNIT1, which is still open on #1498. A package restore skips
recompiling that package. It never skips checking the package being edited.

**Relation to the two lenses.** I9 permits no tier difference. The same artifact
identity and the same restore path serve `jet build`, `jet run`, and `jet dev`.
The lens changes how the current package is compiled, never which dependency
work is reused.

## (b) What Jet hands to the world

Three audiences, one output kind. `Library` covers all of them, and fields on
it say which artifacts the build produces (D-LIB-NAME1=A). No new output kind
enters the ratified closed set.

**Jet calls Jet.** Sealed package objects, linked statically. No ABI and no
export surface. Already covered by D-LIB-REUSE1=B.

**Another language calls Jet** (D-LIB-EXPORT1=C). `jet build` produces a static
library, a shared library, a C header, and a generated binding file for each
language the package names. The exported surface is the entry module's `pub`
items, frozen by `Sema::ApiFreeze`, so a breaking change is a diagnostic. The
same frozen surface is the single source for the header, the bindings, and the
version check. This mirrors the inbound binder direction (D-FFI-UNIFY1=A,
D-FFI-PY1=A) rather than adding a second mechanism.

```jet
# pkg.jet
name: "flightlog"
outputs: .{
  core: .Library.{ native: true, entry: Flightlog, bindings: [c, python, swift] }
}
```

```
$ jet build
  built target/libflightlog.so, libflightlog.a, flightlog.h
  built target/bindings/flightlog.py, Flightlog.swift
```

Jet owns the ownership rules at that boundary. The exported surface states who
frees a returned buffer and what a foreign caller may hold across calls, and
sema checks it (I3).

**A Jet program loads Jet at run time** (D-LIB-DYNTRUST1=A). A `Library` marked
loadable builds a `.jetlib` file pinned to one compiler identity. The loaded
library declares its effects like any package. The host states its grant at the
load site, and a library asking for more is refused before it is mapped.

```jet
# the mod declares what it needs
effects: .{ read: ["./mods/f16"] }

# the host grants a narrower or equal set
mods := Mod.load_all("./mods", grant: .{ read: ["./mods"] })?
```

The compiler already proves a package cannot use an effect it did not declare,
so that declaration is enforceable rather than advisory. This is why a loaded
native library needs no sandbox. The sandboxed wasm plugin keeps its own job:
code the host author did not compile and does not trust.

## (c) ABI stance

Jet makes **no public stable ABI promise**, by ratified decision. Reuse is
exact-match only.

The native export under D-LIB-EXPORT1=C does not change this. A C-facing
boundary uses the C calling convention, which C froze decades ago. Jet borrows
that convention at the edge and promises nothing about its own internal layout.
The promise Jet refuses is the Swift-style one: that *Jet's own* types and calls
keep their layout across compiler versions.

Practical consequences to enforce:

- A compiled Jet artifact records compiler identity. A mismatch is refused with
  a registered diagnostic before anything is linked or mapped.
- No closed-source Jet package can be shipped as a binary that outlives a
  compiler release. That is the intended outcome, not a gap to fix later.
- Any future request for a stable Jet ABI needs a fresh owner decision, because
  it also needs an explicit compatibility baseline under greenfield law.

## (d) Both passes

**Beginner.** Nothing is typed and nothing is configured. The first build
compiles; the second build is fast. A compiler upgrade prints one line saying
the dependencies rebuild once. There is no cache flag to learn, no clean step to
remember, and no stale-artifact failure mode to debug. When an artifact cannot
be trusted, it is rebuilt silently rather than offered with a warning.

**Expert.** Everything is inspectable and controllable:

- `jet cache bind` sets mirror order, roles, and credential providers.
- `jet build` prints the namespace of every cache write.
- `jet explain --lens cache` states why each hit was trusted or refused.
- `jet prove --lens reproducibility` names the first differing path between two
  builders.
- `--offline` beats every mirror.
- Exporting a library artifact is opt-in and declared in the package, never a
  side effect of building.
- The load site names its grant, so a reader of the host sees what a loaded
  library may do.

## Ratified answers

| Decision | Outcome |
|---|---|
| **D-LIB-REUSE1=B** | Sealed package objects, plus pinned Jet libraries loaded at run time. No public stable ABI. |
| **D-LIB-EXPORT1=C** | `Library` also emits a native static and shared library, a C header, and generated bindings per named language. |
| **D-LIB-DYNTRUST1=A** | A loaded library declares its effects; the host grants a set at the load site; anything more is refused before mapping. |
| **D-LIB-NAME1=A** | A field on the ratified `Library` output, not a new output kind. Loadable files use the `.jetlib` suffix. |

## Build order

1. **#1422 — sealed package objects.** Per-package compile actions in the action
   IR, full artifact identity, restore from local and bound mirrors, static
   link, typed generic bodies in the artifact. Still gated on D-INCR-UNIT1 for
   the incremental-unit boundary.
2. **Loadable library and loader.** The `Library` fields, the `.jetlib`
   artifact, the compiler-identity pin, and the effect grant check. Two fresh
   diagnostic codes are needed, each with what/why/fix text and a UI snapshot.
   `E0912` from the reuse-ballot example is retired and must not be reused.
3. **Native export and bindings.** Native artifacts, the generated C header, and
   the per-language binding generator, all driven by the frozen `pub` surface.

Each step honours I2 (a rustc rejection of generated code stays an internal
error), I3 (every check lives in sema, never in a link failure), I6 (no new
external dependency in a compiler seam), and I9 (one meaning across every tier).
