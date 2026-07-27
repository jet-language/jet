# Jet metaprogramming — the vision + implementation plan

**Status:** canonical metaprogramming design. Owner direction recorded
2026-07-01. The five `fn build` ballots on card **c1nixrpd** ratified
2026-07-01: **D-BUILDENTRY1=B, D-BUILDPOLICY1=A, D-BUILDSCOPE1=A,
D-BUILDGEN1=A, D-METADEPTH2=B**. §§1–13 are the design rationale; **§15 is the
executable implementation plan** for those five (epoch e4, card c1nixrpd).
Supersedes and replaces `epoch-5/jai-secure-metaprogramming.md` and the older
Jai import reports. Ratified law lives only in
`docs/spec/syntax-decisions.md`; where §§1–13 show illustrative syntax it is
flagged, but §15 tracks the ratified option shapes exactly.

**The slogan: Jai power, Jet authority model.**

---

## 1. Glossary

- **comptime** — evaluation during compilation. Value-level only (S26): it
  computes values, it never creates types.
- **derive** — code generated from a type's shape (`#Codable`, user
  `derive T.Wire`). The boilerplate killer.
- **reflection** — reading a type's shape at compile time (`T.reflect()` →
  `TypeInfo`). The read half of derives.
- **splice** — `$name`, weaving a compile-time value into generated source
  (D-CTMARKER1).
- **build entry** — the compile-time `fn main` equivalent: one function per
  unit that orchestrates its build. Spelling open (D-BUILDENTRY1); this doc
  writes it `fn build(b: BuildContext)`.
- **effect tier** — how much world a compile-time construct may touch:
  Tier 0 pure, Tier 1 reproducible + lock-recorded, Tier 2 ambient + gated
  (D-CTEFFECT1).
- **capability** — an explicit handle to a slice of the world (a read root, a
  pinned URL, an env var), granted to build code instead of ambient authority.
- **generated source** — Jet text emitted by metaprogramming that re-enters
  lexer → parser → sema like hand-written code (D-CTCODEGEN1 / R11).

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

| Rung | Job | Mechanism | Status |
|---|---|---|---|
| 0 | type-driven boilerplate | built-in derives (`#Codable`, …) | shipped (S55) |
| 1 | compile-time values | `comptime x = f();`, `comptime if`, `comptime { }` | shipped (S26/S57, D-CTMARKER1) |
| 2 | pure eval + data embedding | whitelist Core (D-CTCORE1), `embed_file`/`embed_bytes` (D-CTIO1), `find` (D-CTFIND1), `fetch(url, sha256:)` (D-NETDEP1) | shipped |
| 3 | user derives | `T.reflect()` (D-METAREFLECT1) + `derive T.Trait` emitting source fragments with `$` splices (D-METADERIVE1) | shipped |
| 4 | whole-program build metaprogramming | **`fn build`** — this document | ratified 2026-07-01, card c1nixrpd (§15) |

Rejected forever (D-METADEPTH1, load-bearing): token/AST macros, custom
syntax, attribute macros, comptime types. **One law spans every rung:
comptime never creates types.** Rung 4 honors it by staging — build code
computes *values*, one of which is source text handed back to the ordinary
front end. Type creation happens where it always does: in sema, over real
source.

## 4. `fn build` — the auditable bottleneck

The owner's seed idea, kept whole: a compile-time entry point symmetric with
runtime `main`, mapping cleanly to `jet build`. One per unit. It is the
**only** place whole-program metaprogramming exists — everything below it
stays pure and value-level, so there is exactly one place to audit.

```jet
fn build(b: BuildContext) =[FS]=> BuildPlan ? {
    migrations :: b.find("schema/*.sql")
    b.generate("db_client", gen_db(migrations))?
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
4. **Effects under capabilities.** Touch the world only through declared,
   tiered, recorded handles (§5).

Entry spelling (`fn build` by name vs `#Build` marker vs manifest pointer) is
**D-BUILDENTRY1**. Note honestly: the "lifecycle verbs" law
(`jet <verb>` → `fn <verb>()`) is an open proposal, not ratified; the
recommendation stands without it.

## 5. Effects and capabilities — declare / permit / cap

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
| **Declare** | on the code: `#Impure("why") =[FS, Net]=>` | what this build fn needs; travels with the file; statically readable |
| **Permit** | pkg.jet `build:` block, or a flag/prompt for a lone file | whether this project grants it |
| **Cap** | workspace.jet policy block | org ceiling no member grant can exceed |

The declare layer is why **a single file works with no manifest** — the lone
script's `fn build` carries its own declaration and gets a per-invocation
grant. The permit layer makes it a package. The cap layer makes it an
enterprise. Same fn, unchanged, at every rung.

Tier-2 capabilities are exact handles, deny-by-default:

- fs: declared read/write roots; no implicit `$HOME`
- network: fixed-output fetch by default; ambient only to allowlisted domains
- env: explicit allowlist, values recorded or redacted by policy
- exec: command allowlist, argv captured, tool digest recorded; no shell
  strings by default
- time/random: deterministic injected clock/RNG by default
- secrets: never visible to dependency build code; if supported, named
  capabilities, never ambient env reads

## 6. Generated source — materialized, additive, addressed

The direct answer to Jai's `#insert`, and the cornerstone of auditability.
D-CTCODEGEN1 (ratified) already fixes the pipeline law: generated code
re-enters lexer → parser → sema exactly like hand-written source; no
generation path may inject nodes past the sema gatekeeper; errors pin to the
user's trigger site with the fragment as optional context.

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
            fix:  "add `fn archive(self) => Archived` to {ty.name}")
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

Layering: **workspace ⊃ payloads ⊃ packages ⊃ modules.** A payload *contains*
packages; `pkg.jet` (ratified name, U10 revised) defines one payload — the
publish/version/fetch unit. The monorepo surface is `workspace.jet`
(D-WORKSPACE1/2, implemented), never pkg.jet.

| Scale | Files | Build entry | Grant |
|---|---|---|---|
| single file | none | in the file, beside `fn main` | per-invocation (flag/prompt) |
| project | `pkg.jet` (+ `env.jet` dev shell) | package scope | pkg.jet `build:` |
| multi-package payload | one `pkg.jet`, several `packages:` | one entry per payload | same |
| monorepo | `workspace.jet` + N `pkg.jet` | member entries + optional workspace entry | workspace policy caps all members |
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
| `#run` arbitrary fn at compile time | rungs 1–2 (pure/whitelisted values) or `fn build` (capability-gated) |
| `#insert` strings into bodies | `fn build` generates whole modules, materialized (§6) |
| macros / `#expand` | rejected forever — derives + generation cover the jobs |
| message-loop mutation of user code | rejected — additive generation + read-only enforce |
| whole-program checks via message loop | §7 observe/enforce, no mutation |
| build script side effects | tiers + capabilities + policy (§5) |
| build profiles / metaprogram build files | D-BUILDPROFILE1 (shipped) + `fn build` |

The expert loses nothing they need — every Jai showcase (custom
serialization, org-wide rules, baked data, generated bindings, asset
pipelines, build orchestration) maps to a rung. What they lose is the
ability to be *invisible*. That is the point.

## 12. The build-graph expansion (ratified)

`fn build` is ordinary Jet code. Helpers, loops, parsers, branches, reusable
generators, and policy functions do the work; builder calls appear only where
values cross into the build graph. `BuildPlan` is the returned graph boundary,
not a JSON-shaped DSL.

Full parity with CMake/Bazel/Ninja/Gradle means `BuildPlan` grows into a typed
graph. D-E4EXIT1=C makes that part of the Epoch 5 exit bar, and the 2026-07-06
ballots ratified the graph shape:

- **D-BUILDTARGET1=A / #219:** targets are registered with
  `b.add_executable`, `b.add_library`, `b.add_test`, `b.add_bench`,
  `b.add_asset_bundle`, `b.add_doc`, `b.add_install`, `b.add_package`, and
  `b.add_publish`; each returns a typed handle.
- **D-BUILDACTION1=A / #220:** `b.action(name, inputs, outputs, run, caps)`;
  outputless command targets are explicit, visible, uncached, and
  capability-gated.
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
fn build(b: BuildContext) =[FS]=> BuildPlan ? {
    schema :: b.embed("schema/app.sql")?
    b.generate("db_client", make_db_client(schema))?

    app :: b.add_executable("ledger",
        sources: ["src/run.jet"],
        generated: ["db_client"])

    return b.plan(default: app)
}

fn make_db_client(schema: String) => String {
    tables :: parse_tables(schema)
    out := "module db_client {\n"

    loop table; tables {
        out += make_table_api(table)
    }

    return out + "}\n"
}
```

Asset pipeline plus tests:

```jet
fn build(b: BuildContext) =[FS, Exec]=> BuildPlan ? {
    atlas :: b.action("pack-sprites",
        inputs: b.find("assets/sprites/*.png")?,
        outputs: ["build/sprites.atlas"],
        run: ["atlas-pack", "assets/sprites", "build/sprites.atlas"],
        caps: #(FS, Exec))

    b.generate("sprite_ids", make_sprite_enum(atlas.outputs[0]))?

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
fn build(b: BuildContext) => BuildPlan ? {
    require_timeouts(b.program)
    require_archival(b.program)

    service :: b.add_executable("service", sources: ["src/run.jet"])
    return b.plan(default: service)
}

fn require_timeouts(p: ProgramInfo) {
    loop f; p.functions() {
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

fn run() => Unit ? {
    source :: files.read("src/run.jet")?
    parsed :: jc.parse(source)?
    checked :: jc.check(parsed)?

    loop f; checked.functions() {
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

## 14. Status map

**Ratified substrate (already law):** S26/S57 comptime bindings ·
D-CTMARKER1 `$` splices + `comptime { }` · D-CTCORE1 pure whitelist ·
D-CTIO1 embed · D-CTFIND1/2 find · D-CTEFFECT1 tiers + `#Impure` ·
D-NETDEP1 fetch backend (shipped — `Comptime/Methods.rs::eval_net_fetch`,
sha256-pinned, lock-recorded) · D-CTCODEGEN1 source re-entry ·
D-METAREFLECT1 reflection (shipped) · D-METADERIVE1 user derives (shipped) ·
D-METADEPTH1 ceiling · D-BUILDPROFILE1 profiles (shipped) ·
D-WORKSPACE1/2 + D-MONOREF1 workspace · U10 `pkg.jet`.

**Ratified 2026-07-01 (card c1nixrpd, e4) — plan in §15:**

| Ballot | Outcome | Decides |
|---|---|---|
| D-BUILDENTRY1 | B | `fn build(b: BuildContext) => BuildPlan ?`, run by `jet build` when root defines one, else default pipeline |
| D-BUILDPOLICY1 | A | tiered authority, `BuildContext`-only; Tier 2 needs `#Impure` + permission; deps denied Tier 2 by default |
| D-BUILDSCOPE1 | A | entry lives in the unit's own file at every rung; grant chain flag → pkg.jet `build:` → workspace `policy:` |
| D-BUILDGEN1 | A | generated modules materialize under `.jet/generated/<package>/`, never committed, additive-only, lock-hashed |
| D-METADEPTH2 | B | read-only post-sema program snapshot + structured `b.error` from the build entry only |

**Ratified 2026-07-06:** D-BUILDTARGET1=A, D-BUILDACTION1=A,
D-BUILDTOOLCHAIN1=A, D-BUILDPROBE1=A, D-BUILDCACHE1=A, D-BUILDREMOTE1=A,
D-BUILDSCHED1=A, D-BUILDQUERY1=A, D-BUILDLEGACY1=A, D-BUILDPLUGIN1=A,
D-FRONTENDAPI1=A, D-DSLBLOCK1=A, D-METAMUTATE1=A.

**Parked:** c147 #14 remains a frozen evidence-gated serde-bound reserve.
`$` splice + `comptime {}` on #94 are shipped lower rungs, not a mutation
surface. #38 stays closed as authority/effects duplicate of #95.

**Sequencing after ratification:** `fetch(url, sha256:)` backend shipped →
entry + BuildContext + generate + plan foundation → typed targets/actions
(D-BUILDTARGET1=A + D-BUILDACTION1=A) → explain/audit surfaces → workspace
composition + policy → enforce API (D-METADEPTH2=B, now ratified) →
follow-on build-system parity bundles (§12).

---

## 15. Implementation plan — card c1nixrpd (e4)

Executable plan for the five ratified foundation ballots, plus the now-ratified
target/action graph gates from D-E4EXIT1=C. Do not call #95 implementation
ready until the typed target/action graph is implemented coherently; a
source-list plan no longer meets the exit bar. Read §§4–8 for rationale; this section is
what an implementer builds. Everything runs through the **existing comptime
interpreter** (`crates/jet-comptime`) — extend it, do not greenfield.
The build entry is a `CtValue`-level program the driver runs after sema; its
methods dispatch beside the shipped `emit`/`embed`/`find` in
`Comptime/Methods.rs`. `BuildContext`/`BuildPlan` are `CtValue::Struct`s.

### 15.0 Prerequisite — SATISFIED

`fetch(url, sha256:)` (D-NETDEP1) is **shipped**: `crates/jet-comptime/src/
Comptime/Methods.rs::eval_net_fetch` — string URL + required `sha256:` label,
content verified against the pin, recorded as a `ComptimeInput` (`url:{url}` +
hash) in the lock inputs. `find`/`embed` (D-CTFIND1/D-CTIO1) likewise ship.
No upstream implementation gate remains for the foundation mechanics. Graph
exit still waits on target/action graph implementation.

### 15.1 Sequencing DAG

```
        D-BUILDENTRY1 (B)  ── root: entry detection + BuildContext + BuildPlan + plan→compile
          |        |        \
          v        v         \
   D-BUILDGEN1  D-BUILDPOLICY1 \
      (A)          (A)          v
          \        /       D-METADEPTH2 (B)   (rides D-METAREFLECT1 TypeInfo)
           v      v
        D-BUILDSCOPE1 (A)   ── needs POLICY1's grant model to place flag/pkg/workspace layers
```

- **ENTRY1 is the root.** Nothing runs until `jet build` can find, execute, and
  compile the plan from a root `fn build`.
- **GEN1** and **POLICY1** are independent of each other; both need ENTRY1.
- **SCOPE1** needs ENTRY1 + POLICY1 (the grant chain *is* the placement of
  POLICY1's permission layers per scale).
- **METADEPTH2** needs ENTRY1 only, and rides shipped **D-METAREFLECT1**: its
  snapshot scales the existing `TypeInfo` surface. Runs in parallel with
  GEN1/POLICY1. Requires one driver staging change (§15.6): run sema → hand the
  entry a *checked* snapshot → then compile the plan.

Build order: **ENTRY1 → METADEPTH2 ∥ (GEN1, POLICY1) → SCOPE1 → typed
targets/actions**. The last step is ratified by D-BUILDTARGET1=A and
D-BUILDACTION1=A; implementation lives on #219/#220.

### 15.2 D-BUILDENTRY1=B — the build entry

**Ratified semantics.** `jet build` runs a root-defined
`fn build(b: BuildContext) => BuildPlan ?` when present, otherwise the
batteries default pipeline runs (opt-in, zero-config forever). No marker, no
name magic beyond the reserved function name; the typed `BuildContext`
parameter is the visible authority boundary. Only the selected **root** unit's
entry runs — imported modules never get a hidden build hook; a dependency's
entry runs only sandboxed when that dependency is itself built (§15.5). The
entry returns a plan; it never mutates compiler state — the driver compiles
what the plan names.

**API surface (Jet).**
```jet
fn build(b: BuildContext) => BuildPlan ? {   // optional; =[effects]=> added by POLICY1
    return b.plan(sources: ["src/main.jet"])
}

// BuildContext (Tier-0/1 methods; all already-ratified value ops)
b.find(glob: String) => [String] ?     // D-CTFIND1, Tier 1
b.embed(path: String) => String ?       // D-CTIO1,  Tier 1
b.fetch(url: String, sha256: String) => Bytes ?   // D-NETDEP1, Tier 1 (backend shipped)
b.plan(sources: [String], generated: [String], assets: [String]) => BuildPlan
```
`BuildPlan` is an opaque `CtValue::Struct`. The foundation can prove
entry/run/materialization with `sources`/`generated`/`assets`, but the shipped
Epoch 5 shape must use the typed target/action graph ratified by
D-BUILDTARGET1=A and D-BUILDACTION1=A.

**Lands in.**
- Detection + selection + staging: `crates/jet-driver/src/Driver/mod.rs` — new
  `compile_build_entry(...)` beside the existing
  `compile_bundle_path_with_entry`/`swap_entry_point` (the shipped `fn dev`
  path is the exact template). CLI wiring: `Source/CmdCompile.rs`
  (`jet build`), `crates/jet-cli/src/CLI.rs`.
- `BuildContext`/`BuildPlan` values + method dispatch: new
  `crates/jet-comptime/src/Comptime/Build.rs`, called from the method router in
  `Comptime/Methods.rs`. `find`/`embed`/`fetch` reuse the shipped builtins
  (`Comptime/Builtins.rs`) — the BuildContext handle just scopes them.
- Driver flow: sema-check the root → if a well-formed `fn build` exists, run it
  via `jet-comptime` to obtain a `BuildPlan` → compile the sources/generated
  the plan names. No `fn build` → current default path, untouched.

**Diagnostics (new, E35xx block — free).**
- **E3501** — build entry has the wrong signature.
  what: "`fn build` must take one `BuildContext` and return `BuildPlan ?`"
  why: "the build entry is a typed contract: its parameter is the only
  authority it gets, and its result is the plan the compiler builds"
  fix: "write `fn build(b: BuildContext) => BuildPlan ?`"
  fixture: `tests/ui/build_entry_bad_sig.{jet,stderr}`
- **E3520** — two build entries for one unit (file `fn build` **and** a
  `pkg.jet`/`workspace.jet` entry). (Shared with SCOPE1, §15.5.)
  fixture: `tests/ui/build_entry_conflict.{jet,stderr}`

**Example (I5).** `metaprogramming/build_entry.jet` — a `fn build` returning a
two-source plan; expected `jet build` summary line
`compiled  <name> 0.1.0`. (Examples tree is being reorganized into topic dirs
concurrently; cite the topic path, fall back to `examples/features/NNN_…` if
the reorg hasn't landed when you add it.)

**Targeted tests.** `crates/jet-driver/tests/build_entry.rs` — (a) root with
`fn build` selects and runs it; (b) no `fn build` → default pipeline unchanged;
(c) an *imported* module defining `fn build` does **not** run (no hidden
hook); (d) E3501 on bad signature.

**Exit criteria.** `jet build` on a file with `fn build` compiles exactly the
plan's sources; on a file without one, byte-identical behavior to today; the
golden example runs; E3501 fixture blessed.

### 15.3 D-BUILDGEN1=A — generated source

**Ratified semantics.** `b.generate(name, source)` emits a real `.jet` module
materialized under `.jet/generated/<package>/<name>.jet` (never committed),
re-entering lexer → parser → sema like hand-written code (D-CTCODEGEN1, the
shipped derive path). **Additive-only:** generation may add modules, never
mutate or shadow user-written source. Rounds run in bounded, deterministic
order; a later round may read an earlier round's output; a cycle is a compile
error naming the chain (no loop-until-quiescent). Output hashes + source-input
hashes are recorded in `.jet/lock`; `--locked` verifies or rejects drift;
`jet build --emit-generated` copies the files somewhere visible on demand.

**API surface (Jet).**
```jet
b.generate(name: String, source: String) => Unit ?   // adds .jet/generated/<pkg>/<name>.jet
return b.plan(sources: ["src/main.jet"], generated: ["api_client"])
```

**Lands in.**
- Materialization + staging rounds + additive check: new
  `crates/jet-driver/src/Jetpack/Generated.rs`. `b.generate` in
  `Comptime/Build.rs` collects `(name, source)` pairs into the `BuildPlan`;
  the driver writes them, then feeds them through the **existing** front-end
  re-entry path already used by user derives (D-CTCODEGEN1) — do not invent a
  second injection path (R11/I3).
- Lock records: extend `crates/jet-driver/src/Lock.rs` with a
  `generated: [{name, input_hash, output_hash}]` section; `--locked` compare in
  the same file.

**Diagnostics.**
- **E3510** — generated module name collides with / would shadow user-written
  source. what: "`b.generate(\"{name}\")` would shadow the module you wrote at
  {path}" · why: "generation is additive: what you wrote is always what
  compiles" · fix: "rename the generated module, or delete the hand-written
  one" · fixture `tests/ui/build_generate_shadow.{jet,stderr}`.
- **E3511** — generation cycle (a round reads output a later round changes, or
  a name is generated twice). what: "generation rounds form a cycle: {chain}" ·
  why: "generated source must reach a fixed order, not loop until quiescent" ·
  fix: "break the dependency between these generators" · fixture
  `tests/ui/build_generate_cycle.{jet,stderr}`.
- **E3512** — `--locked` generated-source drift. what: "generated `{name}`
  does not match the hash in `.jet/lock`" · why: "`--locked` guarantees the
  build reproduces the recorded artifact byte-for-byte" · fix: "re-run
  `jet build` without `--locked` to refresh, or restore the pinned inputs" ·
  fixture `tests/ui/build_locked_drift.{jet,stderr}`.

**Example (I5).** `metaprogramming/build_generate.jet` — generate a small
module from an `embed`ed data file, reference it from the plan; expected
summary `generated  <name>.jet (<n> lines) <- <input>  sha256:…` then
`compiled …`.

**Targeted tests.** In `crates/jet-driver/tests/build_generate.rs` — file lands
under `.jet/generated/`, re-enters sema (a type error *in* generated source
pins to the trigger site, not rustc — I2/I3), lock hash recorded, `--locked`
drift → E3512, name collision → E3510, two-round staging observes round-1
output, cycle → E3511.

**Exit criteria.** Generated file exists on disk, is compiled, is lock-hashed;
all four fixtures blessed; `--locked` round-trips clean and rejects drift; the
example runs.

### 15.4 D-BUILDPOLICY1=A — authority defaults

**Ratified semantics.** Build code gets Tier 0 (pure) + locked Tier 1
(`find`/`embed`/`fetch`, content-addressed, lock-recorded) by default. Tier 2
(env, exec, clock, random, unpinned net, arbitrary fs) requires **both** the
audited `#Impure("reason")` gate in source **and** permission at build time
(CLI flag/prompt, or project/org policy). `BuildContext` is the only authority
path — a step has exactly its granted handles, never "a machine." Dependencies
get **no** Tier 2 even when the root grants itself Tier 2, unless policy names
that dependency. Every granted capability is recorded in lock/provenance.

**API surface (Jet).**
```jet
fn build(b: BuildContext) =[FS]=> BuildPlan ? {        // Tier-1 effect declaration
    migrations :: b.find("schema/*.sql")                 // Tier 1: locked, ambient-free
    #Impure("probe local openssl for a legacy C dep") {  // Tier 2: gated + permitted
        b.exec(["pkg-config", "--libs", "openssl"])?
    }
    return b.plan(sources: ["src/main.jet"])
}
```
CLI: `jet build --profile=ci --locked` is hermetic by default — Tier 2 opens
only via an explicit grant (today's ratified blanket is `--allow-impure`,
D-CTEFFECT1/E3411; the build-grant spelling is balloted as D-BUILDFLAGS1);
`jet inspect audit-effects` (static, executes nothing).

**Lands in.**
- Tier gating: extend `crates/jet-comptime/src/Comptime/Purity.rs` (the shipped
  D-CTEFFECT1 tier machinery) so a BuildContext Tier-2 method
  (`exec`/`env`/unpinned `fetch`) is legal only inside a `#Impure` region
  **and** only when the resolved grant permits that effect. `#Impure` parsing
  already exists; reuse it.
- Grant resolution: new `crates/jet-driver/src/Jetpack/BuildPolicy.rs` — merges
  CLI flag/prompt + pkg.jet `build:` + workspace `policy:` into an effective
  capability set handed to the interpreter run.
- Provenance: extend `Lock.rs` with the selected entry, its declared effects,
  executed `#Impure` regions (+ reason text), and allowed external tool
  invocations (argv + tool digest). `jet inspect audit-effects` reads statically in
  `Source/CmdDevTools.rs`.

**I6 guard.** `exec`/network capabilities that need real OS/network work use the
**FFI bridge posture** (stdlib bridge template text, hash-pinned), never a new
crate in `Source/`/`crates/jet-*` compiler code. Unpinned network is Tier 2 by
construction; pinned `fetch` (Tier 1) is the D-NETDEP1 backend.

**Diagnostics.**
- **E3502** — Tier-2 build effect used without a `#Impure` gate. what:
  "`b.{op}` touches the ambient world and must be inside `#Impure(\"reason\")`"
  · why: "build effects that aren't pure or locked have to be declared where an
  auditor can see them" · fix: "wrap it in `#Impure(\"why you need it\")`" ·
  fixture `tests/ui/build_effect_ungated.{jet,stderr}`.
- **E3503** — Tier-2 effect gated but not permitted by policy. what: "this
  build asks for `{Effect}`, which the project has not granted" · why: "ambient
  authority is deny-by-default so CI stays hermetic" · fix: "add `{Effect}` to
  the `build:` block in pkg.jet, or pass `--allow-{effect}`" · fixture
  `tests/ui/build_effect_denied.{jet,stderr}`.
- **E3504** — a **dependency's** build requested authority the root denies.
  what: "dependency `{dep}` build asks for `{Effect}`, denied by default" ·
  why: "dependency build code never gets ambient authority unless you name it"
  · fix: "grant it explicitly in workspace `policy:` `grants`, or drop the
  dependency" · fixture `tests/ui/build_dep_authority.{jet,stderr}`.

**Example (I5).** `metaprogramming/build_effects.jet` — Tier-1 `find` plus a
Tier-2 `exec` gated by `#Impure` and permitted via the file's per-invocation
grant; expected summary shows the effect line.

**Targeted tests.** `crates/jet-driver/tests/build_policy.rs` — Tier-1 runs
with zero grant; ungated Tier-2 → E3502; gated-but-unpermitted → E3503;
dependency Tier-2 denied → E3504; lock records the `#Impure` reason + argv.

**Exit criteria.** Default build is Tier-0/1 only; Tier-2 needs gate + permit;
three fixtures blessed; provenance visible in `.jet/lock` and via
`jet inspect audit-effects`; a dependency cannot escalate.

### 15.5 D-BUILDSCOPE1=A — entry home + grant chain

**Ratified semantics.** The entry lives in the unit's own definition file at
every rung — `fn build` beside `fn main` in a single file, inside `pkg.jet` for
a package, inside `workspace.jet` for a workspace. No new filenames. The grant
chain mirrors containment: per-invocation flag/prompt (single file) → `pkg.jet`
`build:` standing grant (package) → `workspace.jet` `policy:` ceiling
(workspace) that no member grant can exceed. A workspace entry runs member
builds in dependency order and may add workspace-level targets and cap members;
it may **never** mutate a member's plan. `jet inspect audit` reads all three layers
without executing anything.

**API surface (manifest, not grammar).**
```jet
// pkg.jet
payload: .{ name: "atlasgen", version: "1.2.0" }
packages: .{ atlasgen: library }
build: .{ allow: #(FS) }                    // standing grant for this package's fn build

fn build(b: BuildContext) =[FS]=> BuildPlan ? { ... }

// workspace.jet
module workspace {
    members: find("./packages")
    policy: .{ deny: #(Net, Exec), grants: .{ "somedep": #(Net) } }  // ceiling + per-dep escape
}
```

**Lands in.**
- Parse `build:` block in pkg.jet:
  `crates/jet-driver/src/Jetpack/PackageManifest/ParseBlocks.rs`. Parse
  `policy:` block in `crates/jet-driver/src/Jetpack/WorkspaceFile.rs`.
- Home selection + grant-chain merge: `BuildPolicy.rs` (from §15.4) resolves
  which file holds the entry for the current scale and stacks the three grant
  layers (flag ⊂ pkg ⊂ workspace ceiling).
- Workspace composition: dependency-ordered member runs live in
  `crates/jet-driver/src/Jetpack/mod.rs` (workspace resolution already there);
  a member plan is read-only to the workspace entry.

**Diagnostics.**
- **E3520** — conflicting or misplaced build entry (file `fn build` **and** a
  manifest entry, or an entry where the scale forbids one). what: "two build
  entries for `{unit}`: {locations}" · why: "one unit has exactly one build
  entry so audit has one place to look" · fix: "keep the `fn build` in
  {chosen}, remove the other" · fixture
  `tests/ui/build_entry_conflict.{jet,stderr}` (shared with E3520 in §15.2).
- **E3504** (from §15.4) is the workspace-ceiling enforcement point: a member
  grant exceeding the workspace `policy:` cap reuses E3504's message shape.

**Example (I5).** `metaprogramming/build_workspace.jet` (+ a `workspace.jet`
fixture) — a member `fn build` requesting `=[FS]=>` under a workspace `policy:`
that denies `#(Net, Exec)`; expected: member builds run in order, the cap
holds. Reuse `metaprogramming/build_entry.jet` for the single-file rung.

**Targeted tests.** `crates/jet-driver/tests/build_scope.rs` — same `fn build`
works as single file (flag grant), package (pkg.jet grant), and workspace
member under a ceiling; workspace entry runs members in dependency order;
member-plan mutation attempt is impossible by API (read-only handle);
conflicting entries → E3520; `jet inspect audit` prints all three layers, executes
nothing.

**Exit criteria.** One `fn build` survives single-file → package → workspace
unchanged; grant chain resolves flag ⊂ pkg ⊂ workspace; ceiling caps a member;
E3520 blessed; `jet inspect audit` static-reads the chain.

### 15.6 D-METADEPTH2=B — observe + enforce (rung B)

**Ratified semantics.** The selected **root** build entry (and only it, never
at import time) receives a **post-sema, read-only snapshot** of the whole
checked program and a diagnostic-emission API whose signature structurally
requires `code + what/why/fix` (I4 by construction). No mutation, no macros; a
dependency's rules never run in your build. Reading the checked program cannot
change what any file means — local reasoning and the LSP model survive. Rung C
(mutation / message loop / user macros) stays frozen on c154.

**Rides D-METAREFLECT1 — exactly which types scale up.** The shipped
reflection surface in `crates/jet-comptime/src/Comptime/Reflect.rs` today
produces, for **one** `StructDef`, a `TypeInfo` `CtValue::Struct` with
`FieldInfo`/`MethodInfo`/`TypeParamInfo` children (`build_struct_type_info`,
`build_field_info`, `build_method_info`, `build_type_param_info`). METADEPTH2
scales this **up one level of containment**, reusing those builders unchanged:

| New | Built from | Reuses |
|---|---|---|
| `ProgramInfo` | the checked `ProgramBundle` | wraps `PackageInfo` list |
| `PackageInfo` | each package/module in the bundle | wraps `TypeInfo` + `FunctionInfo` lists |
| `FunctionInfo` | each free `Func` (not just methods) | `build_method_info`'s field logic, plus `.span`, `.effects`, `.reaches_panic()` |
| `TypeInfo` (extend) | existing `build_struct_type_info` | add `.span`, and helper predicates `.implements(trait)` / `.has_method(name)` derived from existing `.methods`/`trait_impls` |

AST-cheap (available today from the AST): `name`, `fields`, `methods`,
`type_params`, `markers`, `has_method`, `implements` (from `trait_impls`),
`span`. **Sema-computed** (must be lifted from checker output, not the raw
AST): `FunctionInfo.effects` (from the already-produced effect facts —
`Driver::check_file_with_effect_facts` exposes them) and
`.reaches_panic()` (a reachability query over the checked call graph). Wire
these two from sema results into the snapshot; do not recompute in comptime.

**API surface (Jet).**
```jet
fn build(b: BuildContext) => BuildPlan ? {
    for ty in b.program.types() {
        if ty.implements("Entity") and not ty.has_method("archive") {
            b.error(ty.span, code: "ORG01",
                what: "entity type {ty.name} has no archive method",
                why:  "company policy: every entity must be archivable for GDPR export",
                fix:  "add `fn archive(self) => Archived` to {ty.name}")
        }
    }
    for f in b.program.functions() {
        if f.effects.has("Net") and f.reaches_panic() {
            b.error(f.span, code: "ORG02", what: "…", why: "…", fix: "…")
        }
    }
    return b.plan(sources: ["src/main.jet"])
}

// snapshot surface
b.program.types() => [TypeInfo]
b.program.functions() => [FunctionInfo]
TypeInfo:     .name .fields .methods .markers .span .implements(String) .has_method(String)
FunctionInfo: .name .params .span .effects (.has(String)) .reaches_panic()
b.error(span, code: String, what: String, why: String, fix: String) => Unit
```

**Lands in.**
- Snapshot builders: extend `crates/jet-comptime/src/Comptime/Reflect.rs`
  (`build_program_info`, `build_package_info`, `build_function_info`; extend
  `build_struct_type_info` with `.span`). Predicate methods dispatch in
  `Comptime/Methods.rs` (or a new `Comptime/Enforce.rs`) beside the shipped
  reflection method calls.
- `b.error` collection: `b.error` pushes a `Diagnostic` into the interpreter's
  emitted-diagnostics buffer; the driver surfaces them through the normal
  diagnostic channel. `b.error(span, code, what, why, fix)` maps **directly**
  onto `crate::Diagnostics::Diagnostic::error(code, what, why, fix, Some(span))`
  — the exact five-field constructor `jet-sema` already uses
  (`Sema/Diagnostics.rs`). This *is* the I4 structural guarantee: there is no
  `b.error` overload that omits a field.
- Driver staging change (the one new sequencing step): run sema to produce the
  **checked** bundle → build the `ProgramInfo` snapshot from it → run the build
  entry (which may emit enforce diagnostics and returns the plan) → if any
  enforce diagnostic is an error, fail the build before codegen → else compile
  the plan. In `Driver/mod.rs::compile_build_entry`.

**Diagnostics.**
- **E3530** — an enforce rule used a reserved compiler-style code. what:
  "build rule code `{code}` is reserved: `E`/`W` + digits belong to the
  compiler" · why: "org rule codes must be distinguishable from compiler
  diagnostics in logs and dashboards" · fix: "use a project prefix like
  `ORG01`" · fixture `tests/ui/build_enforce_reserved_code.{jet,stderr}`.
- User-emitted enforce diagnostics carry the user's own code (`ORG01`, …) and
  render in the standard what/why/fix format — they are product output, not
  fixed compiler codes, so they need no diagnostics.md entry; E3530 (the guard
  on *how* rules emit) does.

**Example (I5).** `metaprogramming/build_enforce.jet` — the `Entity`/`archive`
rule over `b.program.types()`; ship two variants so golden covers both paths: a
clean program (build succeeds) and one missing `archive` (build fails with the
`ORG01` diagnostic at the type's span). Expected stderr is the rendered
what/why/fix block.

**Targeted tests.** Extend `Reflect.rs`'s existing unit test with a
`build_program_info` shape assertion (program → packages → types/functions).
`crates/jet-driver/tests/build_enforce.rs` — snapshot is post-sema (sees
checked effects), `b.error` fails the build and pins to the right span,
`reaches_panic`/`effects.has` reflect sema facts, reserved code → E3530, an
imported module's enforce loop does **not** run (root-only). Add a `fmt`
round-trip only if any new *grammar* appears — none does here (`fn build`,
`b.error`, `for` are shipped syntax), so no formatter change; still add the
example to the golden set.

**Exit criteria.** The build entry reads a whole-program snapshot built by
scaling `TypeInfo`; `b.error` emits I4-shaped diagnostics that fail the build
at the correct span; only the root entry runs rules; reserved-code guard
blessed; the two golden variants pass.

### 15.7 Invariant guards (verify before "done")

- **I3 — checking stays in sema.** The build entry is never a checking
  strategy. All language type/borrow/effect checking stays in `jet-sema`;
  METADEPTH2 *reads* sema's output and adds **policy** diagnostics only.
  Codegen stays dumb (I3): generated source (§15.3) goes through the normal
  sema gate (R11/D-CTCODEGEN1), never injected past it; a bad generated program
  is caught by sema and pinned to the trigger site, never handed to rustc
  (I2). No "run the build to see if it type-checks."
- **I4 — structured diagnostics.** Every new compiler code
  (E3501/3502/3503/3504/3510/3511/3512/3520/3530) gets a `docs/spec/
  diagnostics.md` entry (what/why/fix, snapshot-pinned) **and** a
  `tests/ui/*.stderr` fixture — no fixture, no diagnostic. `b.error`'s five-arg
  signature makes user enforce diagnostics I4 by construction.
- **No hidden execution at import time.** Only the selected **root** `fn build`
  runs. Imported/dependency modules never execute a build hook in your build
  (anti-Cargo-`build.rs`); `comptime {}` stays value-only. A dependency's entry
  runs only sandboxed under dependency defaults when that dependency is itself
  built (§15.5, D-BUILDPOLICY1). Test (c) in §15.2 and the root-only test in
  §15.6 are the regression guards.
- **I6 — zero compiler deps.** Tier-2 OS/network capabilities use the FFI
  bridge posture (stdlib bridge template text, hash-pinned), never a new crate
  in compiler code. Pinned `fetch` is the D-NETDEP1 backend.
- **I8 — one mechanism.** `fn build` is the single whole-program build path;
  `comptime {}` is value-only; there is no second macro/DSL route. Generation is
  additive-only; enforce is read-only. Any second way to do one of these jobs is
  rejected with a pointer to the existing path.

### 15.8 Ambiguity resolutions (decided 2026-07-02 — do not re-derive)

Implementers follow these as written; none is open:

- **`Bytes` type** for `b.fetch`/`b.embed_bytes` return — **reuse the shipped
  comptime bytes value** (the type `eval_net_fetch`/`embed_bytes` already
  produce). Mint nothing.
- **Per-invocation grant UX** for the single-file rung — owner-facing;
  **balloted as D-BUILDFLAGS1** (raised 2026-07-02 on c1nixrpd). Build
  POLICY1's grant resolution behind a small seam so the flag/prompt spelling
  drops in when ratified; everything else in §15.4 is unblocked.
- **`.reaches_panic()` precision** — **MVP over-approximates**: any
  transitive `panic`/unhandled path counts. Tighten later behind the same
  method; the golden example asserts the over-approximate answer.
- **Effect vocabulary** — `FunctionInfo.effects` and POLICY1/SCOPE1 grants
  draw from the ratified D-EFF4 ten-effect set (`Net, FS, IO, DB, Time, Rand,
  Env, Exec, Log, GPU`). No parallel name set.
- **Card id drift** — the live card is **c1nixrpd** (moved to e4, 2026-07-02,
  per the owner's 2026-06-26 restructure: E4 = Jai metaprogramming). The
  `c154 (e7)` references are the *frozen rung-C* card and are correct as-is —
  do not repoint them.
