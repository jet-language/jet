# Jet metaprogramming

This page explains Jet's metaprogramming rationale and user-facing shape.
Ratified law lives in `docs/spec/syntax-decisions.md`; examples here are
illustrative unless that source says otherwise.

Vocabulary: [Jet vocabulary](../../spec/vocabulary.md).

**The slogan: Jai power, Jet authority model.**

---

## 1. Glossary

- **comptime** — evaluation during compilation. Value-level only (S26): it
  computes values, it never creates types.
- **derive** — code generated from a type's shape (`#Codable`, user
  `derive T.Wire`). The boilerplate killer.
- **reflection** — reading a type's shape at compile time (`T.reflect()` →
  `TypeInfo`). The read half of derives.
- **splice** — `@name`, a typed hole filled in an item template during
  expansion (D-CTMARKER1).
- **build entry** — the compile-time `fn main` equivalent: one function per
  unit that orchestrates its build. Spelling open (D-BUILDENTRY1); this doc
  writes it `fn build(b: BuildContext)`.
- **effect tier** — how much world a compile-time construct may touch:
  Tier 0 pure, Tier 1 reproducible + lock-recorded, Tier 2 ambient + gated
  (D-CTEFFECT1).
- **authority handle** — an explicit handle to a slice of the world (a read root, a
  pinned URL, an env var), granted to build code instead of ambient authority.
- **generated item** — a typed Jet item template whose holes are filled during
  expansion and whose result enters ordinary sema like hand-written code
  (D-META-CODE1 / R11). A build boundary may materialize the checked item as
  Jet source for an addressed generated module.

## 2. North star

Jet's metaprogramming must be the best ever shipped: Jai-class power —
compile-time execution, code generation, whole-program awareness, builds as
programs — with none of Jai's costs. One language at every stage; no macro
sublanguage, no proc-macro shadow crates, no build-DSL.

Jai got the big thing right and four things fatally wrong:

1. **Invisible generation.** `#insert` splices strings into the compile;
   output never exists as source. Nothing to read, diff, debug, or point an
   LSP at.
2. **Unbounded effects.** `#run` can hit the network mid-compile. Zero
   supply-chain story.
3. **Action at a distance.** The message-loop metaprogram can rewrite any
   declaration anywhere; reading a file no longer tells you what it means.
4. **No determinism contract.** No reproducible-build or incrementality
   guarantee.

Each is a trade of safety/auditability for power. Jet's bet: keep the power,
refuse the trade, and pay for it in implementation effort (philosophy.md).
The test an enterprise security team must pass without reading prose: *what
code ran at build time, what authority did it have, what did it read or
write, and can I reproduce it offline?*

## 3. The ladder — one mechanism per job

Five rungs. Each rung is opt-in; the one below always suffices for simpler
jobs (I8). A beginner lives on rungs 0–1 and never learns the rest exist.

| Rung | Job | Mechanism |
|---|---|---|
| 0 | type-driven boilerplate | built-in derives (`#Codable`, …) |
| 1 | compile-time values | `comptime x = f();`, `comptime if`, `comptime { }` |
| 2 | pure eval + data embedding | whitelist Core, `embed_file`/`embed_bytes`, `find`, `fetch(url, sha256:)` |
| 3 | user derives | `T.reflect()` + typed `derive T.Trait { … }` item templates |
| 4 | whole-program build metaprogramming | **`fn build`** |

Rejected forever (D-METADEPTH1, load-bearing): token/AST macros, custom
syntax, attribute macros, comptime types. **One law spans every rung:
comptime never creates types.** Build code supplies typed item blocks that are
checked by the ordinary front end. Type creation happens where it always does:
in sema, over checked Jet items.

## 4. `fn build` — the auditable bottleneck

The owner's seed idea, kept whole: a compile-time entry point symmetric with
runtime `main`, mapping cleanly to `jet build`. One per unit. It is the
**only** place whole-program metaprogramming exists — everything below it
stays pure and value-level, so there is exactly one place to audit.

```jet
fn build(b: BuildContext) BuildPlan -[FS]> {
    migrations :: b.find("schema/*.sql")
    b.generate("db_client") {
        module db_client { … }
    }?
    return b.plan(sources: ["src/main.jet"], generated: ["db_client"])
}
```

```text
$ jet build
   generated  db_client.jet (214 lines) <- schema/0001.sql..0007.sql
   compiled   ledger 0.3.0
```

Rules stricter than runtime `main`:

- **Opt-in.** No `fn build` → the batteries default pipeline runs. `jet build`
  works with zero config forever; beginners never see any of this.
- **Explicitly rooted.** Only the root unit's entry runs. Imported modules
  have no hidden build hooks; a dependency's entry runs sandboxed under the
  dependency defaults (§8), never with your authority.
- **`BuildContext` is the only authority path.** A build step does not "have
  a machine"; it has a context holding exactly the granted handles.
- **It returns a plan, it never mutates compiler state.** The driver compiles
  what the plan names.

Four powers, in ratification order:

1. **Configure.** Typed targets, profiles (rides shipped D-BUILDPROFILE1),
   assets. Replaces make/build.rs/configure.
2. **Generate.** Emit whole modules as real Jet source (§6).
3. **Observe + enforce.** Read the checked program, reject builds with
   first-class diagnostics (§7 — the D-METADEPTH2 vote).
4. **Effects under authority handles.** Touch the world only through declared,
   tiered, recorded handles (§5).

Entry spelling (`fn build` by name vs `#Build` marker vs manifest pointer) is
**D-BUILDENTRY1**. Note honestly: the "lifecycle verbs" law
(`jet <verb>` → `fn <verb>()`) is an open proposal, not ratified; the
recommendation stands without it.

## 5. Effects and rights — declare / permit / cap

D-CTEFFECT1 (ratified) gives the tiers:

- **Tier 0 — pure.** Always on. Ordinary comptime.
- **Tier 1 — reproducible effects.** World-touching but content-addressed and
  recorded in `.jet/lock`: `embed_file`, `find(glob)`, `fetch(url, sha256:)`.
  Same inputs on every machine or the build fails. This is the enterprise
  sweet spot — most of Jai's build-time convenience with nothing hidden from
  caches, CI, or auditors.
- **Tier 2 — ambient effects.** env, exec, clock, random, unpinned network,
  arbitrary fs. Requires BOTH the audited `#Impure("reason")` gate in source
  AND permission at build time. CI is hermetic unless an expert opens it.

On top of the tiers, one grant chain with three layers — each a different
mechanism, so nothing is declared twice (owner direction 2026-07-01,
spelling open as **D-BUILDSCOPE1** / **D-BUILDPOLICY1**):

| Layer | Lives | Job |
|---|---|---|
| **Declare** | on the code: `#Impure("why") -[FS, Net]>` | what this build fn needs; travels with the file; statically readable |
| **Permit** | package.jet `build:` block, or a flag/prompt for a lone file | whether this project grants it |
| **Cap** | workspace.jet policy block | org ceiling no member grant can exceed |

The declare layer is why **a single file works with no manifest** — the lone
script's `fn build` carries its own declaration and gets a per-invocation
grant. The permit layer makes it a package. The cap layer makes it an
enterprise. Same fn, unchanged, at every rung.

Tier-2 rights are exact handles, deny-by-default:

- fs: declared read/write roots; no implicit `$HOME`
- network: fixed-output fetch by default; ambient only to allowlisted domains
- env: explicit allowlist, values recorded or redacted by policy
- exec: command allowlist, argv captured, tool digest recorded; no shell
  strings by default
- time/random: deterministic injected clock/RNG by default
- secrets: never visible to dependency build code; if supported, named rights,
  never ambient env reads

## 6. Generated items — materialized, additive, addressed

The direct answer to Jai's `#insert`, and the cornerstone of auditability.
D-META-CODE1 and D-META-BODY1 fix the pipeline law: typed item templates are
checked with the ordinary grammar, their holes are filled at expansion, and
the resulting items enter sema like hand-written source. No generation path
may inject unchecked nodes past the sema gatekeeper; errors pin to the user's
trigger site. A build boundary may then materialize the checked items as an
addressed `.jet` module.

This vision adds three surface rules (home/addressing balloted as
**D-BUILDGEN1**):

1. **Materialized.** Generated modules are real `.jet` files (recommended
   home: `.jet/generated/<package>/`, hash-recorded in `.jet/lock`, never
   committed). Open one, read it, set a breakpoint in it, let the LSP
   go-to-def into it. Jai cannot do this; Rust's cargo-expand is a forensic
   tool, not a surface.
2. **Additive only.** Generation may ADD modules; it may never mutate or
   shadow user-written source. What you wrote is what compiles. Local
   reasoning survives; code review reviews the truth.
3. **Bounded staging.** Generation rounds run in declared, deterministic
   order; a later round may observe an earlier round's output; a cycle is a
   compile error naming the chain. No loop-until-quiescent.

`--locked` verifies generated hashes or rejects drift; stale generated files
are cleaned by graph ownership. This closes Make/Ninja missing-dependency
bugs and the Jai injection risk with one rule.

## 7. Observe + enforce — policy as code (D-METADEPTH2)

D-METADEPTH1 ratified the ceiling at reflection + derives and said rung B —
a read-only, lint-style rejection pass — "rises only by a future vote."
**D-METADEPTH2 is that vote**, scoped to the build entry:

- the entry receives a **post-sema, read-only snapshot** of the whole program
  through the existing `TypeInfo` surface scaled up (program → packages →
  types/functions);
- it emits diagnostics through an API whose signature structurally requires
  code + what/why/fix — I4 quality by construction;
- it runs only at the selected root entry, never at import time; a
  dependency's rules do not run in your build.

```jet
for ty in b.program.types() {
    if ty.implements("Entity") and not ty.has_method("archive") {
        b.error(ty.span, code: "ORG01",
            what: "entity type {ty.name} has no archive method",
            why:  "company policy: every entity must be archivable for GDPR export",
            fix:  "add `fn archive(self) Archived ->` to {ty.name}")
    }
}
```

This is the Roslyn-analyzer shape — the one industry success story of typed
read-only compiler APIs — and it is Blow's own message-loop showcase
(whole-program domain rules) with the mutation removed. Teams write org rules
in Jet, with the compiler's error quality, against a stable reflection API
instead of compiler internals. Rung C (mutation, message loop, user macros)
stays frozen on c154 (e7) and would need its own future vote.

## 8. Scale ladder — solo to enterprise, one model

Layering: **workspace ⊃ Packages ⊃ modules.** A Package contains its outputs;
`package.jet` (ratified name, U10 revised) defines one publish/version/fetch
unit. Workspace membership is a declaration-discovered Config, not a second
Package root.

| Scale | Files | Build entry | Grant |
|---|---|---|---|
| single file | none | in the file, beside `fn main` | per-invocation (flag/prompt) |
| project | `package.jet` (+ Config contributions) | package scope | package.jet `build:` |
| multi-package payload | one `package.jet`, several `packages:` | one entry per payload | same |
| monorepo | one workspace Config + N `package.jet` | member entries + optional workspace entry | workspace policy caps all members |
| enterprise | same + policy block | workspace entry runs org rules (§7) | hermetic CI is the default |

Workspace composition: the workspace entry runs member builds in dependency
order; it may add workspace-level targets and cap members; it may never
mutate a member's plan. Exact homes and chain spelling: **D-BUILDSCOPE1**.

Root vs dependency defaults — where enterprise adoption is won or lost:

- **Root:** may run its entry under project policy; Tier 2 only with explicit
  permission; owns its generated output root.
- **Dependency:** Tier 0 + locked Tier 1 only, sandboxed, no repo-wide read
  access, no Tier 2 even when the root grants itself Tier 2 — unless policy
  explicitly grants that dependency. Its generated outputs are part of its
  store fingerprint; its entry is visible in lock/provenance.

Organizations tolerate powerful root builds. They reject dependency build
code that reads the host by default. Jet ships the second posture out of the
box.

## 9. Lock, provenance, audit

`.jet/lock` is the durable audit surface for everything compile-time:

- package graph + tree hashes; workspace member index
- Tier-1 `comptime_inputs` (embed/find/fetch: path or URL + sha256, sorted
  result sets)
- generated source hashes
- build profile, target, compiler version
- selected build entry + its declared effects
- executed `#Impure` regions, including reason text
- policy-allowed external tool invocations (argv + tool digest)

Derived from it: SBOM, SLSA-style provenance (builder identity, external
parameters, resolved deps, output digests), and the human surfaces —
`jet inspect explain-build` (what ran, what it read, why this rebuilt) and
`jet inspect audit` / `jet inspect audit-effects` (every declared gate across the resolved
dependency graph, read statically, nothing executed). Determinism makes
caching and reproducible builds fall out of the same records.

## 10. Threat model (what the walls are for)

Build-time metaprogramming is code execution before the binary exists. The
model must make these visible and controllable, not documented:

- a dependency's build reads `$HOME` secrets, CI env, SSH agents, cloud
  metadata
- a build downloads unpinned bytes and silently changes the binary
- a generator shells out to tools that differ per machine
- generated code bypasses checking or points diagnostics at code nobody wrote
- a malicious package exfiltrates the repo during build
- a poisoned compile-time cache is reused
- a build depends on time/random/host paths and stops reproducing

Posture: never "users should audit build scripts." The toolchain makes
authority explicit, enforceable, machine-readable.

## 11. What Jet refuses, and the one-path answer

| Jai power | Jet path |
|---|---|
| `#run` arbitrary fn at compile time | rungs 1–2 (pure/whitelisted values) or `fn build` (authority-gated) |
| `#insert` strings into bodies | `fn build` generates whole modules, materialized (§6) |
| macros / `#expand` | rejected forever — derives + generation cover the jobs |
| message-loop mutation of user code | rejected — additive generation + read-only enforce |
| whole-program checks via message loop | §7 observe/enforce, no mutation |
| build script side effects | tiers + rights + policy (§5) |
| build profiles / metaprogram build files | D-BUILDPROFILE1 + `fn build` |

The expert loses nothing they need — every Jai showcase (custom
serialization, org-wide rules, baked data, generated bindings, asset
pipelines, build orchestration) maps to a rung. What they lose is the
ability to be *invisible*. That is the point.

## 12. The build-graph expansion (ratified)

`fn build` is ordinary Jet code. Helpers, loops, parsers, branches, reusable
generators, and policy functions do the work; builder calls appear only where
values cross into the build graph. `BuildPlan` is the returned graph boundary,
not a JSON-shaped DSL.

`BuildPlan` is a typed graph boundary for parity with CMake, Bazel, Ninja,
and Gradle. The ratified decisions below define its shape:

- **D-BUILDTARGET1=A / #219:** targets are registered with
  `b.add_executable`, `b.add_library`, `b.add_test`,
  `b.add_asset_bundle`, `b.add_doc`, `b.add_install`, `b.add_package`, and
  `b.add_publish`; each returns a typed handle.
- **D-BUILDACTION1=A / #220:** `b.action(name, inputs, outputs, run, caps)`;
  outputless command targets are explicit, visible, uncached, and
  authority-gated.
- **D-BUILDTOOLCHAIN1=A / #221:** default host toolchain is inferred;
  non-default builds use typed toolchain handles with recorded host/target
  triples, SDKs, signing identities, and tool digests.
- **D-BUILDPROBE1=A / #221:** typed `find_program`, `pkg_config`,
  `has_header`, and `compile_check`; each result is reproducible or ambient.
- **D-BUILDCACHE1=A / #222:** action key = inputs + outputs + argv + env +
  caps + tool digest + target + policy + toolchain + compiler version +
  generated source hashes.
- **D-BUILDREMOTE1=A / #222:** remote cache and remote execution are separate
  policy grants; remote execution waits on sandbox/provenance proof.
- **D-BUILDSCHED1=A / #223:** deterministic graph scheduler with automatic
  parallelism and named pools: cpu, memory, linker, console, gpu.
- **D-BUILDQUERY1=A / #224:** `jet inspect graph`, `jet inspect query build`, and
  `jet inspect explain-build <target/file/action>` share graph/provenance data with
  the LSP.
- **D-BUILDLEGACY1=A / #225:** CMake/Make/Gradle/npm/cargo wrappers are Tier-2
  legacy actions with declared inputs, outputs, and caps; optional graph import
  stays inside the wrapper.
- **D-BUILDPLUGIN1=A / #226:** one plugin contract covers first-party Jet build
  libraries and packaged/third-party WASM component plugins under policy.
- **D-FRONTENDAPI1=A / #227:** public read-only `core.compiler`
  lexer/parser/check/semindex/source-map APIs plus CLI JSON mirror.
- **D-DSLBLOCK1=A / #128:** stdlib-only PascalCase directive DSL blocks, fixed
  in `Syntax.rs`, not third-party grammar mutation.
- **D-METAMUTATE1=A / #15:** Jai-style AST mutation/message loop/user macros
  are rejected; additive generation, graph APIs, DSL blocks, and front-end APIs
  carry the power surface.

### 12.1 Practice shape

Build files must feel like Jet programs, not manifest data. Simple generation:

```jet
fn build(b: BuildContext) BuildPlan -[FS]> {
    schema :: b.embed("schema/app.sql")
    b.generate("db_client") {
        module db_client { … }
    }

    app :: b.add_executable("ledger",
        sources: ["src/run.jet"],
        generated: ["db_client"])

    return b.plan(default: app)
}

// The block may contain ordinary items and compile-time loops over typed data.
```

Asset pipeline plus tests:

```jet
fn build(b: BuildContext) BuildPlan -[FS, Exec]> {
    atlas :: b.action("pack-sprites",
        inputs: b.find("assets/sprites/*.png"),
        outputs: ["build/sprites.atlas"],
        run: ["atlas-pack", "assets/sprites", "build/sprites.atlas"],
        caps: #(FS, Exec))

    b.generate("sprite_ids") {
        module sprite_ids { … }
    }

    game :: b.add_executable("game",
        sources: ["src/game.jet"],
        generated: ["sprite_ids"],
        deps: [atlas])

    b.add_test("game-tests",
        sources: ["tests/game.jet"],
        deps: [game])

    return b.plan(default: game)
}
```

Org policy as code:

```jet
fn build(b: BuildContext) BuildPlan -> {
    require_timeouts(b.program)
    require_archival(b.program)

    service :: b.add_executable("service", sources: ["src/run.jet"])
    return b.plan(default: service)
}

fn require_timeouts(p: ProgramInfo) {
    loop f in p.functions() {
        if f.effects.has("Net") and not f.params.has("timeout") {
            f.error(code: "ORG_NET01",
                what: "network function has no timeout",
                why: "company services must fail predictably",
                fix: "add a `timeout` parameter")
        }
    }
}
```

Public front-end toolkit use, outside the compiler:

```jet
use core.compiler as jc

fn run() {
    source :: files.read("src/run.jet")
    parsed :: jc.parse(source)
    checked :: jc.check(parsed)

    loop f in checked.functions() {
        if f.effects.has("Net") {
            print("{f.name} touches the network")
        }
    }
}
```

### 12.2 Adversarial hardening

- If examples read like JSON builders, lead with helper functions, parsing,
  loops, reusable generators, and typed handles. `b.add_*` declares each target
  once; `b.plan(...)` remains the graph handoff.
- If source-list MVP cannot replace Make/CMake/Bazel, build #95 through the
  ratified typed target/action graph before calling it implementation-ready.
- If actions become shell in nicer clothes, cached actions require declared
  outputs. Side-effect commands are separate, visible, uncached, and gated.
- If policy feels hostile for solo users, support single-file TTY prompt and
  `--allow-<effect>` grants. Package/workspace policy appears only when scale
  needs it.
- If build concepts overwhelm beginners, no `fn build` means default pipeline.
  `jet run file.jet` stays the first experience.
- If Bazel transitions/aspects tempt hidden mutation, use typed target configs
  and read-only graph/program queries. No arbitrary rewrite pass.
- If parser/lexer exposure leaks internals, expose a versioned read-only
  `core.compiler` value API. Internal compiler crates stay private.
- If DSL blocks become reader macros, keep a fixed stdlib whitelist in
  `Syntax.rs`; reject third-party grammar mutation.
- If generated source becomes unreadable noise, require materialized files,
  lock hashes, source provenance, LSP navigation, and `jet inspect explain-build`.
- If legacy interop smuggles ambient authority, keep wrappers Tier-2 with
  declared inputs/outputs/caps. CI can ban them.

## 13. Tooling — the Blueprint test

Every design choice above is also an LSP choice (Blueprint north star):

- rungs 0–3 are pure/deterministic → the LSP evaluates them live: hover a
  `comptime` binding and see its value; hover a derive and see its emitted
  fragment
- generated source is real files → go-to-def lands in readable code;
  breakpoints work; review diffs work
- reflection is one typed `TypeInfo` handle → completable, documentable
- enforce rules are ordinary diagnostics → squiggles in the editor, same as
  compiler errors
- no mutation anywhere → the LSP never has to run a metaprogram to know what
  a file means

Jai's model structurally cannot deliver this list. It is Jet's moat.

