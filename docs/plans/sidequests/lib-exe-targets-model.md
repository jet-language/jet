# Sidequest: lib vs. exe / targets model

**Status:** plan, awaiting owner sign-off — promoted from owner-todo.md 2026-06-19.

## Goal

Replace the rigid `library`/`executable` binary in pack.jet with fine-grained
*targets* — `library`, `binary`, `test`, `example`, `benchmark`, `plugin` — so a
single package can expose multiple purposes without becoming a special category.
Today a "library + CLI" package must pick one role and bolt the other on awkwardly;
targets let both co-exist explicitly.

## Current state

`PackageKind` in `Source/Jetpack/PackageManifest/mod.rs:66-69` has two variants:
`Library` and `Executable`. `PackageEntry` (`mod.rs:75-78`) carries
`kind: Option<PackageKind>`. Parsing lives in `parse_packages()` in
`Source/Jetpack/PackageManifest/ParseBlocks.rs:116-171`; it accepts bare name (kind
inferred as `None`), `name: library`, `name: executable`, or `name: { kind:
library/executable }`. The manifest constants `PACKAGE_KIND_LIBRARY = "library"`
and `PACKAGE_KIND_EXECUTABLE = "executable"` live in `Source/Syntax.rs`.

At build time, `Source/Loader.rs:58-86` re-classifies realized packages by whether
`StoreEntry.bin` is empty: empty → `PkgResolution.realized_libs: HashMap<String,
PathBuf>` (loader.rs:32); non-empty → `PkgResolution.realized_exes: HashSet<String>`
(loader.rs:35). Module resolution (`loader.rs:808-858`) enforces that `use <pkg>`
may only import realized libs, emitting E0982 for executables and E0983 for
unrealized libs.

The current model has no concept of test, example, benchmark, or plugin targets.
These are silently treated as either lib or exe depending on entry-point presence.

## Design

### The problem with lib / exe

Many real packages are more than one thing:

- A library that also ships a CLI tool.
- A library that also serves as a code generator (plugin).
- A library plus an example binary that demonstrates it.
- A library plus a benchmark suite.

A rigid two-way classification forces one of two bad outcomes: pick the "primary"
kind and treat the other as a special case, or add a proliferation of manifest flags
that re-invent target specificity informally.

### Targets

A *target* describes one build artifact a package produces. A package may declare
any number of targets. The six standard targets:

| Target | What it produces | Notes |
|--------|-----------------|-------|
| `library` | importable module artifact | default; what other packages `use` |
| `binary` | runnable executable | replaces `executable` |
| `test` | test runner executable | built by `jet test`, not shipped |
| `example` | runnable example binary | built on demand; shows library usage |
| `benchmark` | benchmark harness binary | built on demand |
| `plugin` | loadable artifact (code gen, proc macro equivalent) | future; gates on plugin design |

A package with no declared targets implicitly has one `library` target (keeps
existing behavior for pure libraries) or one `binary` target if an entry point is
detected (keeps existing behavior for executables). This preserves backwards
compatibility for simple packages.

### Manifest syntax (proposed)

Today, `parse_packages()` in `parse_blocks.rs` reads a `packages:` block where each
entry is `name: library`, `name: executable`, or `name: { kind: library/executable }`.
The proposal extends that per-entry block with a `targets:` field, replacing `kind:`.

```
// Today's pack.jet — single kind per package:
packages:
  mylib: library
  mycli: executable

// Proposed — targets list per package entry:
packages:
  mylib: { targets: [library] }

  mycli: { targets: [library, binary { name: "mycli", entry: "src/main.jet" }] }

  mydb: {
    targets: [
      library,
      example { name: "basic", entry: "examples/basic.jet" },
      benchmark { name: "throughput", entry: "bench/throughput.jet" },
    ]
  }
```

Each target entry is either a bare keyword (all defaults) or a block with optional
fields (`name`, `entry`). A package may have at most one `library` target.

### Simplicity vs. specificity tradeoff

The main risk is manifest complexity. A package author who just wants a library now
has to know what a "target" is even if they only have one. Mitigations:

1. **Zero-config default.** A package with no `targets:` field behaves exactly as
   today — one `library` or one `binary` inferred from the existing `kind:` field or
   entry-point detection. The word "target" is invisible unless you need it.
2. **Shorthand forms.** `targets: [library, binary]` is valid without blocks; the
   compiler derives names and entry points from `entry:` field or explicit
   declaration (see D-TGT4).
3. **Lint on the old form.** `kind: executable` is accepted but deprecated; the
   compiler emits an advisory suggesting migration to `targets: [binary]`.

### Interaction with capability model defaults

The memory capability model (`memory-capability-model.md`) distinguishes library
packages (infer + emit capability metadata) from executable packages (infer only).
With targets, the rule maps naturally: any package that declares a `library` target
emits capability metadata for its public API. Packages with only `binary`, `test`,
`example`, or `benchmark` targets do not emit public metadata. A package with both a
`library` and a `binary` target emits metadata for the library surface only. See
D-CAP5 in `memory-capability-model.md`.

### Implementation sketch

1. Replace `PackageKind` with a `Target` enum in `Source/Jetpack/PackageManifest/mod.rs`.
2. `PackageEntry` gains `targets: Vec<Target>` (replaces `kind: Option<PackageKind>`).
3. `parse_packages()` in `parse_blocks.rs` grows target-block parsing; old `kind:`
   key is parsed and translated to a single-element `targets` list with a
   deprecation lint.
4. `PkgResolution` (`Source/Loader.rs`) changes `realized_libs`/`realized_exes` to a
   map from package name to `Vec<ResolvedTarget>`, where each `ResolvedTarget`
   carries kind + artifact path. E0982/E0983 still enforce that only `library`
   targets are importable.
5. `Source/Syntax.rs` gets `TARGET_LIBRARY`, `TARGET_BINARY`, `TARGET_TEST`, etc.
   with decision IDs (I7).

## Decisions for the owner

**D-TGT1 — Replace lib/exe or augment it?**

Option A: keep `library`/`executable` as shorthand and add targets as an extension.
Old `kind:` continues to work; `targets:` is the new preferred form.

Option B: targets replace lib/exe entirely. Old `kind:` is deprecated (warning) then
removed.

```jet
// Option A: old form still first-class
name: executable

// Option B: old form deprecated, targets are canonical
targets: [binary]
```

Recommendation: Option B. `kind: executable` becomes a one-line migration lint; the
model is simpler with one concept (targets) rather than two (kind + targets).

---

**D-TGT2 — Which targets ship in the first increment?**

All six targets or a subset? The most immediately useful are `library`, `binary`,
`test`, and `example`. `benchmark` and `plugin` can follow when their tooling exists.

```jet
// First increment:
targets: [library, binary, test, example]

// Later (when benchmark/plugin tooling exists):
targets: [benchmark, plugin]
```

Recommendation: ship `library`, `binary`, `test`, `example` first. `benchmark` and
`plugin` are gated on their respective tooling designs.

---

**D-TGT3 — Manifest spelling: bare keyword vs. block**

When a target needs no extra fields, should it always be a bare keyword, or must it
always be a block?

```jet
// Bare keyword (proposed):
targets: [library, binary]

// Block-only form:
targets: [
    library {},
    binary {},
]
```

Recommendation: bare keyword allowed; block required only when fields are specified.
Consistent with how `PackageEntry` already accepts `name: library` without a block.

---

**D-TGT4 — Convention for default binary entry point**

Should a bare `binary` target (no `entry:` field) imply a conventional path, and if
so, which one? The owner has ruled against designs that dictate file structure, so
this is a genuine open question rather than a recommendation.

```
// Option A: require explicit entry always (no convention)
targets: [binary { entry: "src/main.jet" }]   // must specify

// Option B: allow bare keyword; compiler searches a fixed set of conventions
//           (e.g. src/main.jet, then <pkg-name>.jet) and errors if ambiguous
targets: [binary]   // resolved by convention search

// Option C: bare keyword is valid only when the package has exactly one .jet
//           file at the root level
targets: [binary]   // valid only if unambiguous
```

Recommendation: leave open for owner decision. Option A (require `entry:` always) is
the safest default — no file-structure mandate, no ambiguity. Options B/C trade
convenience for structure assumptions.

---

**D-TGT5 — `test` target vs. `@test fn` declaration (S82)**

S82 (`@test fn name { }`) is ratified syntax for inline test functions. Does the
`test` target add a *separate* harness binary (Cargo-style), or does it just collect
all `@test fn` declarations from the package's source files?

```jet
// Option A: separate test target with its own entry (integration tests):
targets: [library, test { entry: "tests/integration.jet" }]

// Option B: test target is implicit — jet test always builds @test fns:
// (no explicit target needed; S82 fns are collected automatically)
```

Recommendation: Option B for unit tests (S82 fns collected automatically, no target
declaration needed). Option A for integration test files that live outside the main
source tree. Both may coexist.

## Acceptance checklist

- [ ] Failing example: a pack.jet with `targets: [library, binary]` builds both
      artifacts; `jet test` runs `@test fn`s without a separate target declaration.
- [ ] D-TGT1 through D-TGT5 resolved by owner.
- [ ] `Source/Syntax.rs` updated with target keyword constants + decision IDs (I7).
- [ ] `PackageKind` replaced (or extended) with `Target` enum in
      `Source/Jetpack/PackageManifest/mod.rs`.
- [ ] `parse_packages()` in `Source/Jetpack/PackageManifest/ParseBlocks.rs` handles
      `targets:` block; old `kind:` key emits deprecation lint.
- [ ] `PkgResolution` in `Source/Loader.rs` updated; E0982/E0983 still enforce
      library-only imports.
- [ ] `docs/spec/syntax-decisions.md` row added for each new target keyword.
- [ ] `docs/spec/spec.md` section updated for package manifest shape.
- [ ] `nix develop -c cargo test` green; no invariant bent.
