# Build and config: one plane of facts

Status: proposal, 2026-08-06. Ballots D-CONF-* on the Tower card for this rethink.
Companion proposals: type-system-v2 (carriers + knowledge), authority-one-model,
concurrency (work is a value), corelib overhaul.

## Executive summary

Jet has exactly one configuration fact a program can meet: `build.os`. It is
typed, the compiler knows it, and `#Known if` folds on it with zero runtime
cost. And it is caged: D-OSTARGET2 makes `build.os` legal in one dispatch
position only, and D-CANVASSTATE1 hides `build.profile` from programs on
purpose. The best mechanism on the whole plane exists — restricted to one fact
in one position.

Around that cage, the plane fragments. The manifest has two live parsers with
two vocabularies: the compile path reads the old flat shape, and tooling reads
the ratified role-typed shape (D-SHAPE5a/b). `Build.{ features: [...] }`
reaches rustc as `--cfg` flags, but no Jet construct can read a feature, so
the field configures nothing. `Build.{ env: {...} }` is a raw string table
copied onto the compiler's process environment — a live security finding —
and the program cannot read it either. A program cannot read its own
`version:`; authors restate the string in source. jetos ratified a priority
rule (`OptionValue`) for system options while D-MARK-SCOPE1 ratified
nearest-wins for policy markers, and no rule at all covers config keys.
Generic modules take Tier-0 value parameters that are configuration in all
but name. Four ratified features share the word "profile".

The one idea: **configuration is the program's knowledge about itself — one
plane of typed compile-time facts with one contribution law.** This is the
type-system-v2 model one level up. There, a type is a carrier plus knowledge
about a value. Here, the build is a program plus knowledge about the program.
The manifest declares facts. Profiles, the command line, and system config
contribute values to facts. `#Known if` branches on facts and folds. One law
turns many contributions into one effective value with one audit chain
(`jet explain`).

Why now. The landing zone is ratified and empty: D-ECO-DECL1 (the unified
`Package` root) is stamped "not an executable spelling today," and the
role-typed parser it needs is built but unwired from the compile path. The
authority slate (D-AUTHORITY-*) is on the board asking for one manifest
schema. Landing both on one fact plane builds the substrate once.

What the ballots ask: adopt the model (one plane, one parser); open the cage —
let programs read a real `build.*` record (this amends D-OSTARGET2's
subject-only rule and D-CANVASSTATE1's hidden-profile rule, both named in the
ballot); replace the `features:`/`env:` string tables with declared, typed,
readable settings; ratify one contribution law from block scope to fleet
scope (this amends the same-path-conflict rule of D-ECO-SLICENAME1, named in
the ballot); unify settings with generic-module value parameters; add build
provenance facts; give "profile" one meaning.

What does not change: S26 stands — facts are values, never types, and never
touch dispatch. No macros (D-METAMUTATE1). Secrets stay off the plane
(E1265). Runtime data stays behind the `Env` effect. The D-ECO nouns and
shape stay. The authority slate keeps ownership of what rights exist; this
plane only gives rights the same scope and merge substrate as every other
fact.

## Glossary

- **Fact** — one named, typed thing the compiler knows about the program.
  Example: `build.os`, `build.package.version`, `build.settings.tls`.
- **Plane** — a family of knowledge with one algebra, as in type-system-v2.
  This proposal adds the program's own plane.
- **Scope (echelon)** — where a written value applies. Source scopes: block,
  function, module, package. Contribution layers above the package
  declaration: profile, workspace, environment, system, fleet, command line.
- **Contribution** — one written value for a fact at one scope or layer.
- **Contribution law** — the rule that turns many contributions into one
  effective value, with `jet explain` showing the chain.
- **Setting** — a config key the package declares: a name, a Tier-0 type,
  and a default. (Spelled `settings:` to leave the ratified noun **Config**
  — D-ECO-SLICENAME1's typed contribution value — untouched.)
- **Moment** — when a fact's value is fixed: by the compiler (`build.os`),
  by declaration (manifest), by contribution (profile, CLI, system), or by
  computation (`fn build`, computed module fields).
- **Output** — a typed projection of the package graph (D-ECO-OUTPUT-KINDS1).

## The one idea

**Configuration is the program's knowledge about itself: one plane of typed
compile-time facts, declared once, merged by one contribution law, readable
wherever the program can name them.**

Type-system-v2 says a type is a carrier plus knowledge, and the knowledge
erases before codegen. The build/config plane is the same model pointed at
the whole program. Every fact has a name, a type, a scope, and a moment.
Contributions merge by one law. The effective value is readable at comptime
and erased into the built artifact. The beginner never writes any of this:
`jet run` works with zero files, and the first fact a beginner meets is
`build.package.version`, which is just there. The expert gets the full
ledger: declare typed settings, contribute per profile or per fleet, compute
contributions in `fn build`, and audit any effective value with
`jet explain`.

## Evidence: the shadow systems

Every row does the same underlying job — state a typed fact about the
program — through a different mechanism.

| # | Mechanism | Home | Defect |
|---|-----------|------|--------|
| 1 | `build.os` dispatch subject | `docs/spec/syntax-decisions.md:3423-3425` (D-OSTARGET2) | The right mechanism, caged: one fact, legal in one position; `build.profile` deliberately hidden (`docs/spec/spec.md:813`, D-CANVASSTATE1). |
| 2 | Legacy flat manifest parser (`payload:`, `effects:`, `build:`) | `crates/jet-pkg-model/src/PackageManifest/mod.rs` (1,645 lines); wired at `crates/jet-driver/src/Loader.rs:349` | What `jet build` reads from `package.jet` today. Pre-dates the ratified shape. |
| 3 | Role-typed `Package` parser (`identity:`, `outputs:`, `policy: .{}`) | `crates/jet-pkg-model/src/Package.rs` (2,830 lines); D-SHAPE5a/b | Ratified shape; never called by the compile path. Tooling only. One file, two parsers. |
| 4 | `policy:` governance namespace | both parsers (D-POLICY-WORD1=A) | The namespace is ratified; the defect is two spellings across two parsers for its keys. |
| 5 | Build profiles (`Build.{ optimize: full }`) | `PackageManifest/mod.rs:55-108` (D-BUILDPROFILE1) | Sound — but "profile" also names rows 6, 7a, 7b. |
| 6 | `TargetProfile` (board/triple identity) | `crates/jet-foundation/src/TargetProfile.rs:12`; driver use in `crates/jet-driver/src/Driver/mod.rs` | Internal only; its user surface is deliberately unbuilt ("until a follow-up surface ballot lands", `TargetProfile.rs:3-4`). Third meaning of "profile". |
| 7 | Environment profiles (`profiles:` per D-ENV-PROFILE1; `--env-profile` and an env-namespace `--profile` per D-ENV-FACET1); package/user profiles | `crates/jet-foundation/src/Syntax/jetpack_config.rs:540-548`; `docs/proposals/ecosystem-shape.md:55` (D-JPK-PROFILE1) | Meanings three and four of "profile", across three flags. |
| 8 | `Build.{ features: [...] }` | `crates/jet-foundation/src/Syntax/package_files.rs:508` → `Source/main.rs:267-269` | Reaches rustc as `--cfg`; zero Jet-side readers exist. Write-only config. |
| 9 | `Build.{ env: {...} }` | `Source/main.rs:272-275` | Raw string table onto rustc's process env. Security finding (`docs/audits/security-deep-scan-2026-08-03-full.md:69724`). Unreadable by the program. |
| 10 | Effect budget (`effects: { allow: [...] }`) | `crates/jet-pkg-model/src/EffectBudget.rs` (D-EFFBUDGET1) | Authority fact with its own schema; D-AUTHORITY-MANIFEST1 already targets it. |
| 11 | `#Policy(...)` scope ladder | `crates/jet-foundation/src/Policy.rs` (D-MARK-SCOPE1) | The right source-scope law — applied only to memory facts. |
| 12 | jetos module options (`OptionValue.{ value, priority }`) | `docs/plans/epoch-7/native-jetos.md:41-45` (D-JOS-PRIORITY-SURFACE2) | A second override rule, scoped to option contributions, beside row 11. |
| 13 | Generic-module value params (`module retry<count: Int>`) | `docs/spec/spec.md:1867-1912` (D-GENMOD-VALUE1) | Tier-0 typed values configuring a module — configuration under another name. |
| 14 | Computed module fields | `crates/jet-env-model/src/ModuleEval/Computed.rs` (D-MODCOMPUTE1) | Derived facts; pure dependency graph; no spec section of its own. |
| 15 | `fn build(b: BuildContext)` | `crates/jet-comptime/src/Comptime/Build/` (D-BUILDENTRY1, built) | Computes plans and defaults; its results are not readable facts. |
| 16 | CLI flags (`--profile`, `--release`, `--allow-*`) | `crates/jet-cli/src/CLI.rs` | Contributions with their own vocabulary, outside any declared record. |
| 17 | Manifest `version:` | `identity:`/`payload:` block | The program cannot read it. Authors restate the string in source. |
| 18 | `env.jet` dev shell | repo root `env.jet:1-6`; L0204 | Cannot express shell hooks; `flake.nix` stays a hand-synced shadow copy. |
| 19 | GC ceilings (`MAX_OBJECTS` = 1,000,000 and friends) | `crates/jet-rt/src/__gc.rs:16-21` | Real limits with no declared fact and no dial. |

## The model

### Axes

Three axes place every row of the table.

- **Fact** — what is known. Identity (`name`, `version`), machine (`os`,
  `arch`, board), profile, declared settings, policy keys, authority rights,
  provenance (git commit, toolchain).
- **Scope** — where a written value applies. Source scopes: block →
  function → module → package (the ratified D-MARK-SCOPE1 ladder).
  Contribution layers above the package declaration: profile → workspace →
  environment → system → fleet → command line.
- **Moment** — when the value is fixed. Fixed by the compiler (`build.os`),
  declared in source (manifest field), contributed (profile, CLI, system
  config), or computed (`fn build`, computed module fields). Runtime data is
  off the plane by type: an environment variable read is `core.env.get`
  behind the `Env` effect, and comptime denies it (E0951, E1265).

### The contribution law

**One fact, one type, one effective value, one visible chain.**

The law has two halves, one per axis direction, plus one audited exception:

- **In source scopes, the nearest scope wins.** A block beats its function,
  a function beats its module, a module beats its package. This is
  D-MARK-SCOPE1, word for word, extended from memory-policy keys to all
  facts.
- **Across contribution layers, the most explicit writer wins.** A profile
  beats the declaration's default; the environment beats the profile; a flag
  typed on the command line today beats every standing file. Two writers at
  the same layer with different values are an error naming both sources —
  the ratified Config composition rule ("unequal scalar facts conflict",
  ecosystem-shape, with D-ECO-COMPOSE2 as the expert escape) kept at each
  layer.
- **`.Force` is the audited exception.** At system and fleet layers, an
  expert override (`OptionValue.{ value, priority }`, the ratified
  D-JOS-PRIORITY-SURFACE2 surface) pins a value against later layers,
  including the command line. That is its job: a fleet operator pins a
  canary, and `jet explain` names the pin.

Safety facts only tighten, at every scope and layer. No contribution may
come from the ambient environment — every contribution has a written home.
`jet explain <fact>` prints the whole chain.

Amendment note: introducing ranked layers (declaration < profile < ... <
CLI) amends the Config composition conflict rule (the ecosystem-shape law
that carries D-ECO-COMPOSE2's escape) for *setting* contributions across
layers. Same-layer conflicts still error with provenance. The noun Config
(D-ECO-SLICENAME1) is untouched. D-CONF-MERGE1 names this amendment; the
owner decides there.

Ratified rules that become theorems of this law:

- D-MARK-SCOPE1: "nearest declaration wins, unmentioned keys inherit,
  `jet explain` reports the effective value plus every declaration it
  overrode" — the source-scope half.
- Package policy tighten-only ("can forbid unsafe code but can never
  authorize an unsafe operation") — the safety clause.
- D-BUILDPROFILE1's "profiles are selected by explicit flag, never by
  ambient environment" — the no-ambient clause.
- D-JOS-PRIORITY-SURFACE2 (plain values for ordinary contributions; expert
  overrides carry `.Default`/`.Force`/`.Priority(n)`) — the audited
  exception, kept with its exact spelling.
- E0951/E1265 (comptime purity; secrets denied at comptime) — the moment
  boundary: runtime data cannot sneak onto the plane.

### The connections

- A setting and `build.os` are the same thing: a fact. Conditional
  compilation on a setting needs no new construct — `#Known if` already
  folds on closed values (D-OSTARGET2's mechanism, freed from its cage).
- A build profile and a `#Policy` scope are the same thing: a named bundle
  of contributions at one home.
- A package is a configured module. Generic-module value parameters
  (D-GENMOD-VALUE1) and settings are one substrate: Tier-0 typed values
  that specialize code before it runs.
- jetos option priorities and the marker scope ladder are two halves of one
  contribution law, not two laws.
- The manifest is Jet source, so config validation is type checking. CUE
  proved this shape at industry scale: schema and value are one kind of
  thing, and merging is checking. Jet gets it free because `package.jet` is
  already typed source.
- Elixir's compile-time/runtime config boundary — their unsolved newcomer
  trap — is already a type in Jet: on the plane means comptime fact; off
  the plane means `core.env` behind the `Env` effect. The boundary is an
  effect, not a folder name.

## The surface

The heart of the proposal. Each item names its ballot and its status:
ratified, amended, or new.

### 1. One manifest, one parser (D-CONF-PLANE1 — implements D-SHAPE5a/b and D-ECO-DECL1; deletes the legacy parser)

Today one file, `package.jet`, is read by two parsers. The compile path
(`Loader.rs:349`) reads the legacy flat vocabulary. Tooling, Canvas, and
migration read the ratified role-typed vocabulary. Fields from the ratified
shape that the legacy parser has no vocabulary for — `outputs:`,
`defaults:`, `members:` — never reach the build.

Before — the flat vocabulary the compile path understands:

```jet
// package.jet — legacy vocabulary (what jet build reads)
payload: { name: "aviary", version: "0.4.1", edition: "2026" }
packages: { aviary: library }
effects: { allow: [Net, FS] }
build: {
    release: Build.{ optimize: full },
}
```

After — the ratified role-typed shape becomes the only shape (`proposed`
for the compile path; the shape itself is ratified):

```jet
// package.jet
identity: .{ name: "aviary", version: "0.4.1" }
outputs: .{
    app: .Executable.{ entry: run }
}
settings: .{ tls: Bool = true }            // proposed (D-CONF-KEY1)
build: .{ release: .{ optimize: .Full } }  // proposed spelling; shipped form is Build.{ optimize: full }
```

Spelling note: `docs/proposals/ecosystem-shape.md` (S4, proposed, not
ratified) sketches the opposite assignment — optimize bundles under
`profiles:` and `build:` reserved for execution knobs. D-CONF-KEY1 and
D-CONF-PLANE1 settle the home; `profiles:` stays off the table either way
because D-ENV-PROFILE1 owns it.

The legacy parser (1,645 lines) is deleted. One vocabulary, one schema —
and the schema is a Jet record, so a wrong field is a normal manifest
diagnostic (E1206). Authority fields (`effects:`, grants) keep their
current law until D-AUTHORITY-MANIFEST1 lands its schema on this plane.

### 2. Open the cage: programs read their own facts (D-CONF-READ1 — new; amends D-OSTARGET2 and D-CANVASSTATE1)

Today `build.os` is legal only as the subject of one dispatch form, `build`
is otherwise an ordinary identifier (E0107 at runtime), and `build.profile`
is ratified as "not a user-typeable comptime value" (`spec.md:813`,
D-CANVASSTATE1). This ballot proposes the amendment by name: `build.*`
becomes a real closed comptime record, readable in value position.

Before:

```jet
// version.jet — the string is restated by hand on every release
fn run() {
    print("aviary 0.4.1")
}
```

After (`proposed`):

```jet
fn run() {
    print("aviary {build.package.version}")
}
```

The record covers `build.package.name`, `build.package.version`,
`build.os`, `build.target.arch`, `build.profile`, `build.settings.<key>`,
and `build.stamp.*` (D-CONF-STAMP1). Every fact is a comptime value; reads
fold to constants; nothing survives to runtime. For a bare script with no
manifest, identity facts fall back to the file name and `0.0.0`.

### 3. Declared settings replace the string tables (D-CONF-KEY1 — new; amends D-BUILDPROFILE1 by deleting `features:` and `env:`)

Before — write-only strings:

```jet
// package.jet
build: {
    release: Build.{ features: ["tls"], env: { "API_BASE": "https://api.example.com" } },
}
// No Jet code can read "tls" or API_BASE. The env table goes to
// rustc's process environment instead — the audited injection hole.
```

After (`proposed`):

```jet
// package.jet
settings: .{
    tls: Bool = true,
    api_base: String = "https://api.example.com",
}
build: .{ release: .{ settings: .{ tls: true } } }
```

```jet
// source — branch folds at compile time, dead arm is not compiled
#Known if build.settings.tls == {
    true -> use core.crypto.tls
    else -> {}
}

fn base_url() => String { return build.settings.api_base }
```

```sh
jet build --set tls=false            # proposed: CLI contribution
jet explain build.settings.tls       # proposed: CLI > profile > default
```

`Build.{ features }` and `Build.{ env }` are deleted. The security finding
closes because no user string reaches the compiler's environment. The block
is spelled `settings:`, not `config:`, so the ratified noun **Config**
(D-ECO-SLICENAME1) and the `configs:` field keep their exact meaning.

### 4. One contribution law from block to fleet (D-CONF-MERGE1 — extends D-MARK-SCOPE1; amends the Config composition conflict rule; keeps the D-JOS surface)

Before: three states. Nearest-wins for memory policy (D-MARK-SCOPE1).
`OptionValue` priorities for jetos option contributions
(D-JOS-PRIORITY-SURFACE2). Hard conflict with provenance for unequal
same-path Config facts (the composition law, D-ECO-COMPOSE2 escape) — and
no rule for settings.

After (`proposed`): the one law of the model — nearest wins in source
scopes, most-explicit wins across layers, same-layer conflicts error with
provenance, `.Force` pins from system/fleet, and `jet explain` prints every
chain.

### 5. Settings and generic-module value params: one substrate (D-CONF-MODULE1 — unifies with D-GENMOD-VALUE1; amendments named in the ballot)

Today these are separate features over the identical Tier-0 value set:

```jet
module cache<slots: Int> { pub fn capacity() => Int { return slots * 64 } }
module small = cache<4>
```

After (`proposed`): one value substrate, one evaluator, one diagnostic
family — and facts become legal value arguments:

```jet
module tuned = cache<build.settings.cache_slots>   // proposed
```

The parameter-list grammar of D-GENMOD-VALUE1 (no defaults, no named
arguments) is unchanged; defaults and names live at the settings
declaration only. Two touched rules are named in the ballot: the
closed-expression rule (facts become legal arguments) and the
`[T#capacity]` layout carve-out (a fact-fed `Int` may size a fixed array,
exactly as a different literal instantiation does today).

### 6. Provenance facts (D-CONF-STAMP1 — new; adds one Tier-1 locked input, amendment named in the ballot)

```jet
// proposed
fn run() {
    print("built from {build.stamp.git ?? "no-vcs"} with jet {build.stamp.toolchain}")
}
```

`build.stamp.git` is a `String?` fact: the commit hash, `-dirty` suffixed
for an unclean tree, absent (`None`) outside a repository. Reading it is a Tier-1
locked input under D-CTEFFECT1 — recorded in `.jet/lock` like `find`
results (D-CTFIND1/2), so `--locked` builds reprint the same stamp. A
timestamp fact is deliberately absent: wall-clock time is not a fact about
the source, and the lock cannot pin it.

### 7. One meaning for "profile" (D-CONF-WORD1 — rename)

Four ratified features share the word: build optimize bundles
(D-BUILDPROFILE1), the board identity `TargetProfile` (surface deliberately
unbuilt), environment profiles (D-ENV-PROFILE1), and package/user profiles
(D-JPK-PROFILE1). The ballot proposes: **profile** keeps one meaning — the
optimize bundle behind `--profile`/`--release` — and the other three are
renamed, each with its amendment named. The board identity is a machine
description and joins the machine vocabulary (`--target`) before its
surface ships, so the rename costs nothing.

## What it looks like

### Beginner: zero files, then the first fact

```jet
// hello.jet — no manifest anywhere
fn run() {
    print("hello")
}
```

`jet run hello.jet` works today and keeps working. The first fact arrives
with zero ceremony:

```jet
fn run() {
    print("hello from {build.package.name} {build.package.version}")   // proposed
}
```

For a bare script the identity facts fall back to the file name and
`0.0.0`. No manifest is demanded.

### The rich middle: a service with a setting and two profiles

```jet
// package.jet
identity: .{ name: "birdfeed", version: "1.2.0" }
outputs: .{ serve: .Service.{ entry: run } }
settings: .{ metrics: Bool = false }                  // proposed
build: .{
    release: .{ optimize: .Full, settings: .{ metrics: true } },   // proposed
}
```

```jet
// main.jet
#Known if build.settings.metrics == {                  // proposed
    true -> use core.metrics
    else -> {}
}

fn run() =[Net, Time, Log]=> {
    #Known if build.settings.metrics == {              // proposed
        true -> metrics.serve(port: 9100)
        else -> {}
    }
    serve(port: 8080)
}
```

```sh
jet run                          # metrics off — the declared default
jet build --release              # metrics on — the profile contributes true
jet build --release --set metrics=false   # proposed: CLI wins — the most explicit writer
jet explain build.settings.metrics        # proposed: the chain: CLI > profile > default
```

### Expert: computed contribution, pinned fleet, audited chain

```jet
// package.jet
identity: .{ name: "relay", version: "3.0.0" }
settings: .{ shard_count: Int = 4 }                    // proposed

fn build(b: BuildContext) => BuildPlan ? {
    // computed contribution: shard count follows the machine
    #Known if build.target.arch == {                   // proposed read
        .Arm64 -> b.contribute(shard_count, 8)?        // proposed API
        else -> {}
    }
    return b.plan()
}
```

```jet
// fleet module (system scope) — option contribution, ratified D-JOS surface;
// binding a package setting into it is proposed
options: [
    relay.settings.shard_count: OptionValue.{ value: 16, priority: .Force },
]
```

```sh
jet explain relay.settings.shard_count    # proposed
# fleet prod        16   (.Force pin)       <- effective
# fn build          8    (computed, arm64)
# package.jet       4    (declaration)
```

Every value in the chain is typed, scoped, and written down. Nothing is
ambient.

## What this unlocks

- **Ordinary programs** print their own version and build provenance
  without restating strings.
- **Libraries** ship one package with a `tls`-style switch that downstream
  profiles set — the Cargo-features use case — as a readable, typed
  setting instead of a write-only string, with no cross-graph leakage
  because contributions have homes and same-layer conflicts error.
- **Embedded and firmware** declare board machines and tune real limits:
  the GC ceilings in `crates/jet-rt/src/__gc.rs:16-21` can become declared
  policy facts with a dial instead of hard faults.
- **Simulation and science** parameterize a whole program the way
  `module retry<count: Int>` parameterizes a module — same substrate, same
  proofs, zero runtime cost.
- **Fleets** pin one fact across a thousand machines with `.Force` and
  audit it with one `jet explain` — the NixOS story with a typed chain.
- **Security review** reads one ledger. The manifest injection hole closes
  with the `env:` table; the authority slate lands its schema on the same
  plane and inherits scope, merge, and explain for free.

## What stays

- **S26.** Comptime computes values only. A fact never creates,
  parameterizes, or selects a type, and never affects dispatch. `#Known if`
  folding on closed values is the ratified dispatch-free mechanism. The one
  ratified value-to-layout carve-out (`[T#capacity]`, D-FIXARR1) is
  extended to facts only if D-CONF-MODULE1 is adopted, and is named there.
- **No macros, no AST mutation** (D-METAMUTATE1, D-METADEPTH1/2).
- **Comptime I/O stays closed**: `embed_file`/`embed_bytes` (D-CTIO1) and
  `find` (D-CTFIND1/2, Tier 1). D-CONF-STAMP1 proposes exactly one more
  Tier-1 locked input and names the amendment.
- **Secrets stay denied at comptime** (E1265). Runtime data lives behind
  the `Env` effect — the plane's hard boundary.
- **The D-ECO vocabulary and shape** (Package, Config, Output, flat
  members, `jet split`/`jet fold`, `jet deploy`) — this proposal lands on
  that substrate. The noun Config keeps its ratified meaning; the new block
  is spelled `settings:`.
- **`fn build(b: BuildContext)`** and the built action/target graph
  (D-BUILDENTRY1 family) — unchanged; it gains the power to contribute
  facts.
- **`policy:`** stays the one governance namespace (D-POLICY-WORD1).
- **Authority ownership.** What rights exist, the rights tree, and gates
  belong to D-AUTHORITY-*. This plane supplies scope + merge + explain.
- **No-ambient rule.** Same flags plus same source means the same binary.

## Decisions for the owner

| Ballot | Question | Recommends |
|--------|----------|------------|
| D-CONF-PLANE1 | Adopt the model: one fact plane, one contribution law, one manifest parser (delete the legacy parser) | adopt |
| D-CONF-READ1 | Open the cage: `build.*` becomes a readable record (amends D-OSTARGET2, D-CANVASSTATE1) | adopt |
| D-CONF-KEY1 | Declared typed settings; delete `features:`/`env:` string tables (amends D-BUILDPROFILE1) | adopt |
| D-CONF-MERGE1 | One contribution law block→fleet (amends the Config composition conflict rule; keeps D-JOS surface) | adopt |
| D-CONF-MODULE1 | Settings and generic-module value params: one substrate (amends two D-GENMOD-VALUE1 clauses, named) | adopt |
| D-CONF-STAMP1 | Provenance facts (`build.stamp.*`), Tier-1 lock-recorded; no timestamp | adopt |
| D-CONF-WORD1 | "Profile" = optimize bundle only; the other three meanings are renamed | adopt |

Each ballot stands alone. Any subset can be adopted.

## Implementation shape

- **Phase A — re-found, no surface change.** Wire the compile path to the
  role-typed `Package` parser; delete the legacy parser; all tests green.
  Register the fact plane internally; `build.os` becomes its first row.
- **Phase B — land ratified-but-unbuilt work on the substrate.** D-ECO-DECL1
  root, Hangar receipts, System/Fleet outputs, `jet deploy` — built once,
  on the plane.
- **Phase C — balloted surface work.** Each adopted D-CONF-* lands as one
  coherent greenfield migration that deletes the replaced form: the
  `build.*` record, settings (with `features:`/`env:` deletion), the
  contribution law, module-param unification, stamps, the renames.

I9 note: fact reads fold at comptime, so every tier (AOT, JIT, interpreter,
web) sees the same folded program. The plane adds no per-tier semantics.
