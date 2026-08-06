# Build and config: one plane of facts

Status: proposal v2, 2026-08-06. Ballots D-CONF-* on the Tower card for this
rethink. Revised after owner feedback: `$` fact reads, manifest naming menu,
the text-vs-function law, build entry discovery, the full scope ladder, and
cross-references to the sibling rethinks.

## Executive summary

Jet has exactly one configuration fact a program can meet: `build.os`. It is
typed, the compiler knows it, and `#Known if` folds on it with zero runtime
cost. And it is caged: D-OSTARGET2 makes `build.os` legal in one dispatch
position only, and D-CANVASSTATE1 hides `build.profile` from programs on
purpose. The best mechanism on the whole plane exists — restricted to one
fact in one position.

Around that cage, the plane fragments. The manifest has two live parsers with
two vocabularies, and even the new one spells identity two ways. `Build.{
features: [...] }` reaches rustc as `--cfg` flags no Jet code can read.
`Build.{ env: {...} }` is a raw string table copied onto the compiler's
process environment — a live security finding. A program cannot read its own
`version:`. Facts live in text, but `fn build` can also set defaults, with no
law saying which home owns what. Generic modules take Tier-0 value parameters
that are configuration in all but name. Four ratified features share the word
"profile".

The one idea: **configuration is the program's knowledge about itself — one
plane of typed compile-time facts with one contribution law.** The manifest
declares facts. Profiles, the command line, and system config contribute
values to facts. Programs read facts with the `$` sigil — the
metaprogramming rethink's one splice, "read a compile-time value here" — so
`$build.settings.tls` folds to a constant and `build` never becomes a
reserved word. One law turns many contributions into one effective value
with one audit chain (`jet explain`). One boundary law says what lives in
manifest text (facts, for auditing) and what lives in `fn build` (actions,
for computing).

Why now. The landing zone is ratified and empty: D-ECO-DECL1 (the unified
`Package` root) is stamped "not an executable spelling today," and the
role-typed parser it needs is built but unwired from the compile path. The
type-v2 plane law (D-TYPE2-PLANE1) is now ratified: facts are registered,
nameable, reflectable. The metaprogramming slate (D-META-REG1) asks for one
registration table behind markers, planes, rights, and build facts. This
proposal is that table's build column.

Ten ballots: adopt the model (one plane, one parser); pick the manifest
vocabulary (naming menu); read facts as `$build.*`; typed settings replace
the `features:`/`env:` string tables; one contribution law across the full
scope ladder (item → block → function → module → file → package → layers);
the facts-in-text/actions-in-function boundary; build entry discovery (where
`fn build` lives, how many, how the CLI finds one); settings and module
value parameters as one substrate; provenance stamps; one meaning for
"profile".

What does not change: S26 stands — facts are values, never types. No macros
(D-METAMUTATE1). Secrets stay off the plane (E1265). Runtime data stays
behind the `Env` effect. The authority slate keeps ownership of what rights
exist. The failure slate keeps ownership of how `fn build` errors report.

## Glossary

- **Fact** — one named, typed thing the compiler knows about the program.
  Example: `$build.os`, `$build.package.version`, `$build.settings.tls`.
- **`$` (splice)** — the metaprogramming rethink's one sigil: read a
  compile-time value here (its S4). Fact reads ride it.
- **Plane** — a family of knowledge with one algebra (D-TYPE2-PLANE1,
  ratified). This proposal adds the program's own plane.
- **Scope** — where a written value applies in source: item, block,
  function, module, file, package.
- **Layer** — where a contribution comes from above the package
  declaration: profile, workspace, environment, system, fleet, command
  line.
- **Contribution** — one written value for a fact at one scope or layer.
- **Contribution law** — the rule that turns many contributions into one
  effective value, with `jet explain` showing the chain.
- **Setting** — a config key the package declares: a name, a Tier-0 type,
  and a default. (Spelled `settings:` so the ratified noun **Config** —
  D-ECO-SLICENAME1's typed contribution value — keeps its meaning.)
- **Action** — build work: probe a tool, run a step, generate code. Actions
  live in `fn build`, never in manifest text.
- **Output** — a typed projection of the package graph
  (D-ECO-OUTPUT-KINDS1).

## The one idea

**Configuration is the program's knowledge about itself: one plane of typed
compile-time facts, declared once, merged by one contribution law, readable
anywhere with `$`.**

Type-system-v2 (ratified) says a type is a carrier plus knowledge, and the
knowledge erases before codegen. The build/config plane is the same model
pointed at the whole program. Every fact has a name, a type, a scope, and a
moment. Contributions merge by one law. The effective value folds into the
built artifact.

The beginner never writes any of this: `jet run` works with zero files, and
the first fact a beginner meets is `$build.package.version`, which is just
there. The expert gets the full ledger: declare typed settings, contribute
per profile or per fleet, compute contributions in `fn build`, and audit any
effective value with `jet explain`.

## Evidence: the shadow systems

| # | Mechanism | Home | Defect |
|---|-----------|------|--------|
| 1 | `build.os` dispatch subject | `docs/spec/syntax-decisions.md:3423-3425` (D-OSTARGET2) | The right mechanism, caged: one fact, one position; `build.profile` deliberately hidden (`docs/spec/spec.md:813`, D-CANVASSTATE1). |
| 2 | Legacy flat manifest parser (`payload:`, `effects:`, `build:`) | `crates/jet-pkg-model/src/PackageManifest/mod.rs` (1,645 lines); wired at `crates/jet-driver/src/Loader.rs:349` | What `jet build` reads from `package.jet` today. Pre-dates the ratified shape. |
| 3 | Role-typed `Package` parser (`identity:`, `outputs:`, `policy: .{}`) | `crates/jet-pkg-model/src/Package.rs` (2,830 lines); D-SHAPE5a/b | Ratified shape; never called by the compile path. Accepts identity two ways (`identity: .{ }` block and bare `name:`/`version:` — `examples/jetpack/epoch5/package.jet`). |
| 4 | `policy:` governance namespace | both parsers (D-POLICY-WORD1=A) | The namespace is ratified; the defect is two spellings across two parsers for its keys. |
| 5 | Build profiles (`Build.{ optimize: full }`) | `PackageManifest/mod.rs:55-108` (D-BUILDPROFILE1) | Sound — but "profile" also names rows 6, 7. |
| 6 | `TargetProfile` (board/triple identity) | `crates/jet-foundation/src/TargetProfile.rs:12` | Internal; user surface deliberately unbuilt. Third meaning of "profile". |
| 7 | Environment profiles (`profiles:` per D-ENV-PROFILE1; `--env-profile` and an env-namespace `--profile` per D-ENV-FACET1); package/user profiles (D-JPK-PROFILE1) | `crates/jet-foundation/src/Syntax/jetpack_config.rs:540-548`; `docs/proposals/ecosystem-shape.md:55` | Meanings three and four, across three flags. |
| 8 | `Build.{ features: [...] }` | `package_files.rs:508` → `Source/main.rs:267-269` | Reaches rustc as `--cfg`; zero Jet-side readers. Write-only config. |
| 9 | `Build.{ env: {...} }` | `Source/main.rs:272-275` | Raw string table onto rustc's process env. Security finding (`security-deep-scan-2026-08-03-full.md:69724`). |
| 10 | Effect budget (`effects: { allow: [...] }`) | `crates/jet-pkg-model/src/EffectBudget.rs` (D-EFFBUDGET1) | Authority fact with its own schema; D-AUTHORITY-MANIFEST1 targets it. |
| 11 | `#Policy(...)` scope ladder | `crates/jet-foundation/src/Policy.rs` (D-MARK-SCOPE1) | The right source-scope law — applied only to memory facts, on four of six scopes. |
| 12 | jetos module options (`OptionValue.{ value, priority }`) | `docs/plans/epoch-7/native-jetos.md:41-45` (D-JOS-PRIORITY-SURFACE2) | A second override rule beside row 11. |
| 13 | Generic-module value params (`module retry<count: Int>`) | `docs/spec/spec.md:1867-1912` (D-GENMOD-VALUE1) | Configuration under another name. |
| 14 | Computed module fields | `crates/jet-env-model/src/ModuleEval/Computed.rs` (D-MODCOMPUTE1) | Derived facts; no spec section of its own. |
| 15 | `fn build(b: BuildContext)` + BuildContext defaults | `crates/jet-comptime/src/Comptime/Build/` (D-BUILDENTRY1, built; D-BUILDCTX-FLAGS1) | Actions and fact-setting share one bag; no law says which home owns what. |
| 16 | CLI flags (`--profile`, `--release`, `--allow-*`) | `crates/jet-cli/src/CLI.rs` | Contributions with their own vocabulary. |
| 17 | Manifest `version:` | `identity:`/`payload:` block | The program cannot read it; authors restate the string. |
| 18 | `env.jet` dev shell | repo root `env.jet:1-6`; L0204 | Cannot express shell hooks; `flake.nix` stays a shadow copy. |
| 19 | GC ceilings (`MAX_OBJECTS` etc.) | `crates/jet-rt/src/__gc.rs:16-21` | Real limits with no declared fact and no dial. |

## The model

### Axes

- **Fact** — what is known. Identity, machine, profile, settings, policy
  keys, authority rights, provenance.
- **Scope** — where a written value applies in source. The full ladder:
  item → block → function → module → file → package. D-MARK-SCOPE1 ratified
  four rungs (block, function, module, package); this proposal adds **item**
  (one declaration, the marker placement unit) and **file** (the `#PubFile`
  precedent) as a named extension in D-CONF-MERGE1.
- **Layer** — where a contribution comes from, above the package
  declaration: profile → workspace → environment → system → fleet → command
  line.
- **Moment** — when the value is fixed. Fixed by the compiler (`$build.os`),
  declared (manifest), contributed (profile, CLI, system), or computed
  (`fn build`, computed module fields). Runtime data is off the plane by
  type: `core.env.get` behind the `Env` effect; comptime denies it (E0951,
  E1265).

### The contribution law

**One fact, one type, one effective value, one visible chain.**

- **In source scopes, the nearest scope wins.** An item beats its block, a
  block its function, a function its module, a module its file, a file its
  package. D-MARK-SCOPE1's rule, extended to all facts and all six rungs.
- **Across layers, the most explicit writer wins.** A profile beats the
  declared default; the environment beats the profile; a flag typed today
  beats every standing file. Two writers at the same layer with different
  values are an error naming both sources — the ratified Config composition
  rule ("unequal scalar facts conflict", ecosystem-shape, D-ECO-COMPOSE2
  escape) kept at each layer.
- **`.Force` is the audited exception.** At system and fleet layers, the
  ratified `OptionValue` override (D-JOS-PRIORITY-SURFACE2) pins a value
  against later layers, including the command line, and `jet explain` names
  the pin.

Safety facts only tighten, at every scope and layer. No contribution comes
from the ambient environment. `jet explain <fact>` prints the whole chain.

### The boundary law (facts in text, actions in the function)

**Manifest text declares facts. `fn build` performs actions. A computed
contribution is a fact too — recorded, locked, and visible in the chain.**

The split exists for auditing: a reviewer reads the manifest and knows what
the package *is* — its identity, settings, policy, dependencies — without
executing anything. `fn build` holds what the build *does*: probes, actions,
generated code, and computed contributions to declared facts. `fn build` may
contribute a value to a declared fact (`b.contribute(...)`, recorded in
`.jet/lock` and named by `jet explain`); it may never mint an undeclared
fact, so the manifest stays the complete index of what is configurable.
D-BUILDCTX-FLAGS1 (BuildContext default-setting, ratified) is scoped by this
law and named in D-CONF-SPLIT1. Honest pushback, argued in the ballot: a
flat "details in text only" rule fails, because some facts are only knowable
by computation — a probed toolchain, a `find()` member list. The law keeps
those as *computed contributions to declared facts* instead of banning them.

### Ratified rules that become theorems

- D-MARK-SCOPE1 — the source-scope half of the contribution law.
- Package policy tighten-only ("can forbid unsafe code but can never
  authorize an unsafe operation") — the safety clause.
- D-BUILDPROFILE1's "selected by explicit flag, never by ambient
  environment" — the no-ambient clause.
- D-JOS-PRIORITY-SURFACE2 — the audited exception, kept with its spelling.
- E0951/E1265 — the moment boundary: runtime data cannot sneak onto the
  plane.
- D-MODCOMPUTE1 (computed module fields: pure, dependency-ordered, no
  ambient authority) — the computed-contribution rules, already ratified.

### The connections

- A setting and `$build.os` are the same thing: a fact. Conditional
  compilation needs no new construct — `#Known if` folds on closed values.
- A build profile and a `#Policy` scope are the same thing: a named bundle
  of contributions.
- A package is a configured module: value parameters and settings are one
  Tier-0 substrate.
- jetos option priorities and the marker scope ladder are two halves of one
  contribution law.
- The manifest is Jet source, so config validation is type checking (CUE's
  lattice lesson, free of charge).
- The compile-time/runtime config boundary is an effect, not a folder name
  (the trap Elixir documents and cannot check).
- Fact reads and splices are one sigil: `$build.settings.tls` in a source
  file and `$tname` in a derive body are the same act — read a compile-time
  value here (metaprogramming S4).

## Relation to the sibling rethinks

- **Metaprogramming (one compile-time program, #1508).** Three shared
  seams, aligned by construction. (1) `$` is the one compile-time read;
  fact reads adopt it (D-CONF-READ1 option A), so the build plane needs no
  reserved word and no second spelling. (2) D-META-REG1 proposes one
  registration table behind markers, planes, rights, and build facts; this
  proposal's fact registry is that table's build column — if D-META-REG1 is
  adopted, they are one table, not two. (3) `b.generate` writing real code
  (its S6) is the action half of the boundary law.
- **Type-system-v2 (ratified).** D-TYPE2-PLANE1 is now law: every plane
  registered, nameable, reflectable, prelude-visible. The build plane
  follows it exactly; facts erase like every other knowledge plane.
- **Failure (one report, three routes, #1507).** `fn build` returns
  `BuildPlan ?` — build failures are reports on the value route. Build
  stops and exit codes ride D-FAIL-EXIT1's law. Nothing here invents an
  error shape.
- **Authority (one model, #1500).** Rights are one fact family on this
  plane; D-AUTHORITY-MANIFEST1 owns the schema of what a package may do.
  This plane supplies scope, merge, and explain.
- **Concurrency (work is a value, #1505).** Schedule data (`Every(5min)`)
  is typed values; nothing on this plane conflicts.
- **Corelib overhaul (#1495).** D-CORE-DOCTRINE1's API rules (typed enums
  over strings, zero-config defaults, named magic) are the yardstick the
  naming ballot applies to the manifest vocabulary.

## The surface

### 1. One manifest, one parser (D-CONF-PLANE1)

Unchanged in substance from v1: the role-typed shape becomes the only
vocabulary, the legacy parser (1,645 lines) is deleted, and a wrong field is
a normal manifest diagnostic (E1206). Implements D-SHAPE5a/b and D-ECO-DECL1
on the compile path.

### 2. The manifest vocabulary (D-CONF-NAME1 — new ballot, naming menu)

The field names are judged as a coherent set, by the corelib doctrine, not
inherited. Today three identity spellings coexist (`payload: { }`,
`identity: .{ }`, bare `name:`/`version:`). The recommended scheme (`bare
identity, ratified nouns`) — every line `proposed`:

```jet
// package.jet
name: "birdfeed"
version: "1.2.0"
deps: .{ httpkit: "^2" }
outputs: .{ serve: .Service.{ entry: run } }
settings: .{ metrics: Bool = false }
build: .{ release: .{ optimize: full, settings: .{ metrics: true } } }
policy: .{ no_alloc: false }
members: find("./packages")
```

Bare `name:`/`version:` kill the wrapper ceremony (the epoch5 example
already writes them). The ballot's menu carries three genuine alternatives:
keep the `identity: .{ }` wrapper; the one-literal `Package.{ ... }` root
(implementing D-ECO-DECL1's normative-future spelling now); a verb scheme
(`needs:`/`makes:`). Each option shows this same complete manifest.

### 3. Open the cage with `$` (D-CONF-READ1 — revised; amends D-OSTARGET2 and D-CANVASSTATE1)

Fact reads ride the metaprogramming splice: **`$build.*` — read a
compile-time value here.** This replaces v1's bare `build.*` record and
deletes its worst cost: `build` stays an ordinary identifier, because the
`$` sigil carries the compile-time meaning. One spelling covers compiler
facts, package identity, settings, and stamps.

Before:

```jet
fn run() {
    print("birdfeed 0.3.0")        // restated by hand, drifts every release
}
```

After (`proposed`):

```jet
fn run() {
    print("birdfeed {$build.package.version}")
}
```

The record: `$build.package.name`, `$build.package.version`, `$build.os`,
`$build.target.arch`, `$build.profile`, `$build.settings.<key>`,
`$build.stamp.*`. Reads fold to constants. Bare scripts read fallback
identity (file name, `0.0.0`). The ratified dispatch subject respells
`#Known if $build.os == { ... }` — named in the amendment. If the owner
declines the metaprogramming `$` law, the ballot's fallback option is the
reserved bare namespace.

### 4. Declared settings replace the string tables (D-CONF-KEY1)

As v1, with `$` spelling (`proposed`):

```jet
// package.jet
settings: .{
    tls: Bool = true,
    api_base: String = "https://api.example.com",
}
build: .{ release: .{ settings: .{ tls: true } } }
```

```jet
#Known if $build.settings.tls == {
    true -> use core.crypto.tls
    else -> {}
}

fn base_url() => String { return $build.settings.api_base }
```

```sh
jet build --set tls=false
jet explain build.settings.tls     # chain: CLI > profile > default
```

`Build.{ features }` and `Build.{ env }` are deleted; the audited injection
hole closes.

### 5. One contribution law, full ladder (D-CONF-MERGE1 — revised)

The source ladder gains its missing rungs: **item** (one declaration — where
a marker already sits) and **file** (`#PubFile` precedent). Six rungs, then
six layers, one law, one `jet explain` chain. Amendments unchanged from v1.

### 6. Facts in text, actions in the function (D-CONF-SPLIT1 — new ballot)

The boundary law above, as an owner decision with real alternatives:
everything-in-text (Cargo's failure: `build.rs` grew anyway),
everything-in-function (Zig's failure: opaque to tools), or the split with
recorded computed contributions (recommended). Scopes D-BUILDCTX-FLAGS1 by
name.

### 7. Build entry discovery (D-CONF-ENTRY1 — new ballot)

Where `fn build` lives, how many exist, and how the CLI finds one. The
recommended rule, mirroring `fn run`:

- One `fn build` per package, in any of the package's files; the compiler
  discovers it the way it discovers `fn run`.
- Two candidates in one package is a compile error naming both sites.
- A workspace runs member builds in dependency order, root last — the
  ratified D-BUILDENTRY1 order, kept.
- `jet build <name>` resolves the package by name through the members
  graph and runs its build. No pointer field is required; an explicit
  `entry:` on an Output stays available for the rare override.
- No `fn build` at all means the batteries pipeline — the beginner default.

### 8. Settings and module value parameters: one substrate (D-CONF-MODULE1 — clarified)

What this means, plainly: Jet has two features that accept the same five
value types (Bool, Int, Char, String, fieldless enum), evaluate them with
the same purity rules, and reject the same expressions. One is module value
parameters. The other is settings. Today they are checked by two code paths
and cannot mix. The ballot makes them one substrate with one visible gain —
a fact becomes a legal value argument:

```jet
// ratified today
module cache<slots: Int> {
    pub fn capacity() => Int { return slots * 64 }
}
module small = cache<4>              // a literal works

// proposed — the same instantiation, fed by a fact
module tuned = cache<$build.settings.cache_slots>
```

Nothing on the module page changes. Two D-GENMOD-VALUE1 clauses are amended
by name (closed-expression rule; `[T#capacity]` layout carve-out). The
metaprogramming proposal's S5 (instances expose members by name) composes:
`tuned.Buffer`, never a mangled name.

### 9. Provenance facts (D-CONF-STAMP1)

As v1: `$build.stamp.git` (`String?`, `-dirty` suffix, absent outside a
repository), `$build.stamp.toolchain`; Tier-1 locked inputs under
D-CTEFFECT1; no timestamp, ever.

### 10. One meaning for "profile" (D-CONF-WORD1)

As v1: profile = optimize bundle; the board identity joins the machine
vocabulary (`--target`); environment profiles become presets (D-ENV-PROFILE1
+ D-ENV-FACET1 amendments); package/user profiles take the generation word.

## Beginner magic, expert control — the rungs

Every rung keeps the rung below untouched. Nothing demands a manifest until
a fact is written.

| Rung | You write | You get |
|------|-----------|---------|
| Script | one `.jet` file, no manifest | `jet run` works; `$build.package.name` reads the file name |
| First fact | `{$build.package.version}` in a print | the version, folded, no manifest required (fallback `0.0.0`) |
| Package | `name:`, `version:` in `package.jet` | real identity; `jet build <name>` finds it |
| Settings | `settings: .{ tls: Bool = true }` | typed switches; `--set`, profiles, `jet explain` |
| Actions | `fn build(b) => BuildPlan ?` | probes, steps, generated code, computed contributions |
| Fleet | option contributions with `.Force` | pinned facts across machines, one audit chain |

Expert control at every rung: every fact is registered (nameable,
reflectable per D-TYPE2-PLANE1), every contribution has a written home,
every effective value answers `jet explain`, and every escape (`#Impure`,
Tier-2 grants, `.Force`) is spelled and audited.

## What this unlocks

Unchanged from v1: self-describing programs; one-package libraries with
typed switches; embedded boards with real dials (the GC ceilings gain a
declared home); parameterized simulation; fleet pins with one audit chain;
one security ledger, with the injection hole closed.

## What stays

- **S26** — facts are values, never types; `#Known if` folding is the
  dispatch-free mechanism; the `[T#capacity]` extension is named in
  D-CONF-MODULE1.
- **No macros, no AST mutation** (D-METAMUTATE1, D-METADEPTH1/2).
- **Comptime I/O closed**: `embed_file`/`embed_bytes` (D-CTIO1), `find`
  (D-CTFIND1/2); stamps add exactly one Tier-1 input, named in
  D-CONF-STAMP1.
- **Secrets denied at comptime** (E1265); runtime data behind `Env`.
- **The D-ECO vocabulary and shape**; the noun Config keeps its ratified
  meaning.
- **`fn build` and the built action graph** (D-BUILDENTRY1 family), now
  with a law for what belongs inside it.
- **`policy:`** stays the one governance namespace (D-POLICY-WORD1).
- **Authority ownership** (D-AUTHORITY-*), **failure ownership**
  (D-FAIL-*).
- **No-ambient rule**: same flags plus same source means the same binary.

## Decisions for the owner

| Ballot | Question | Recommends |
|--------|----------|------------|
| D-CONF-PLANE1 | Adopt the model: one fact plane, one parser | adopt |
| D-CONF-NAME1 | The manifest vocabulary (naming menu; bare identity recommended) | A |
| D-CONF-READ1 | `$build.*` fact reads (amends D-OSTARGET2, D-CANVASSTATE1; rides the metaprogramming `$` law) | A |
| D-CONF-KEY1 | Typed settings; delete `features:`/`env:` (amends D-BUILDPROFILE1) | adopt |
| D-CONF-MERGE1 | One contribution law, six scopes + six layers (extends D-MARK-SCOPE1; amends the composition conflict rule) | adopt |
| D-CONF-SPLIT1 | Facts in text, actions in the function; computed contributions recorded (scopes D-BUILDCTX-FLAGS1) | adopt |
| D-CONF-ENTRY1 | Build entry discovery: one per package, `fn run`-style, CLI by name | adopt |
| D-CONF-MODULE1 | Settings and module value params: one substrate (amends two D-GENMOD-VALUE1 clauses) | adopt |
| D-CONF-STAMP1 | Provenance facts, Tier-1 locked; no timestamp | adopt |
| D-CONF-WORD1 | "Profile" = optimize bundle only; three renames | adopt |

Each ballot stands alone. D-CONF-READ1's `$` spelling assumes the
metaprogramming one-splice law; its fallback option covers a decline.

## Implementation shape

- **Phase A — re-found, no surface change.** Compile path adopts the
  role-typed parser; legacy parser deleted; fact registry lands (shared
  with D-META-REG1 if adopted); `$build.os` is row one.
- **Phase B — land ratified-but-unbuilt work on the substrate.**
  D-ECO-DECL1 root, Hangar receipts, System/Fleet outputs, `jet deploy`.
- **Phase C — balloted surface work**, each a coherent greenfield migration
  deleting the replaced form.

I9 note: fact reads fold at comptime; every tier sees one folded program.
The plane adds no per-tier semantics.
