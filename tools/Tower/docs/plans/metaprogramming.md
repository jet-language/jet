# Jet metaprogramming — the vision

**Status:** canonical metaprogramming design. Owner direction recorded
2026-07-01; open ballots live on card c95 (`D-BUILDENTRY1`, `D-BUILDPOLICY1`,
`D-BUILDSCOPE1`, `D-BUILDGEN1`, `D-METADEPTH2`). Supersedes and replaces
`epoch-4/jai-secure-metaprogramming.md` and the older Jai import reports.
Ratified law lives only in `docs/spec/syntax-decisions.md`; where this doc
shows unratified syntax, it is illustrative.

**The slogan: Jai power, Jet authority model.**

---

## 1. Glossary

- **comptime** — evaluation during compilation. Value-level only (S26): it
  computes values, it never creates types.
- **derive** — code generated from a type's shape (`#[Codable]`, user
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
| 0 | type-driven boilerplate | built-in derives (`#[Codable]`, …) | shipped (S55) |
| 1 | compile-time values | `comptime x = f();`, `comptime if`, `comptime { }` | shipped (S26/S57, D-CTMARKER1) |
| 2 | pure eval + data embedding | whitelist Core (D-CTCORE1), `embed_file`/`embed_bytes` (D-CTIO1), `find` (D-CTFIND1), `fetch(url, sha256:)` (D-NETDEP1) | ratified; fetch backend pending |
| 3 | user derives | `T.reflect()` (D-METAREFLECT1) + `derive T.Trait` emitting source fragments with `$` splices (D-METADERIVE1) | shipped |
| 4 | whole-program build metaprogramming | **`fn build`** — this document | open ballots, card c95 |

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
fn build(b: BuildContext) #(Fs) -> BuildPlan ? {
    migrations #= b.find("schema/*.sql")
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
| **Declare** | on the code: `#Impure("why") #(Fs, Net)` | what this build fn needs; travels with the file; statically readable |
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
            fix:  "add `fn archive(self) -> Archived` to {ty.name}")
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
`jet explain-build` (what ran, what it read, why this rebuilt) and
`jet audit` / `jet audit-effects` (every declared gate across the resolved
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

## 12. The build-graph expansion (later, ballots held)

`fn build` returning a source list is the MVP shape, not the final one. Full
parity with CMake/Bazel/Ninja/Gradle — so no one ever reaches for Make —
means growing `BuildPlan` into a typed graph. Held-back scope, deliberately
not balloted until the five open ballots resolve:

- **Targets:** executable, static/shared library, test, bench, doc, asset
  bundle, custom action, install/package/publish, plugin, (later) system
  image. Tests/benches become graph nodes, not ad-hoc paths.
- **Actions:** declared inputs/outputs/argv/env/caps — a step with no
  declared outputs is a side effect, not a build step. Shell strings are a
  Tier-2 legacy escape hatch.
- **Transitive usage requirements:** CMake's one durable idea, typed —
  exported flags, link libs, generated headers propagate through target deps
  under explicit rules, never ambient globals.
- **Toolchains:** typed host/target triples, SDKs, signing identities;
  digests in lock.
- **Cache:** action keys = content hashes + tool digest + argv + env +
  toolchain + compiler version + policy; local first, remote later, remote
  execution only after sandboxing is solid.
- **Scheduler:** parallel with resource pools (cpu/memory/linker/console);
  deterministic ordering where output order matters.
- **Probes:** typed `find_program`/`pkg_config`/`has_header`/`compile_check`
  replacing configure scripts; each declares whether it is reproducible.
- **Introspection:** `jet graph`, `jet query`, why-did-this-rebuild,
  what-generated-this-file, compile-database and IDE-model export.
- **Build plugins:** same sandbox/capability model (WASM Component substrate,
  D-PLUGIN1/D-DEP-WASM1 posture); org policy can deny third-party plugins.
- **Legacy interop:** CMake/Make/Gradle called as Tier-2 legacy actions with
  declared inputs/outputs — a migration ramp under policy, never the
  foundation.

Each of these becomes its own decision bundle (`D-BUILDACTION1`,
`D-BUILDTARGET1`, `D-BUILDTOOLCHAIN1`, `D-BUILDCACHE1`, `D-BUILDPROBE1`,
`D-BUILDLOCK1`) when the foundation ballots land.

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
D-NETDEP1 fetch backend (pending impl) · D-CTCODEGEN1 source re-entry ·
D-METAREFLECT1 reflection (shipped) · D-METADERIVE1 user derives (shipped) ·
D-METADEPTH1 ceiling · D-BUILDPROFILE1 profiles (shipped) ·
D-WORKSPACE1/2 + D-MONOREF1 workspace · U10 `pkg.jet`.

**Open ballots (card c95, Decide lane):**

| Ballot | Decides |
|---|---|
| D-BUILDENTRY1 | entry spelling |
| D-BUILDPOLICY1 | build authority defaults |
| D-BUILDSCOPE1 | entry home per scale + declare/permit/cap chain |
| D-BUILDGEN1 | generated-source home + additive law surface |
| D-METADEPTH2 | rung-B observe/enforce vote |

**Frozen:** c154 (e7) — full Jai message loop / user macros, rung C.

**Sequencing after ratification:** finish `fetch(url, sha256:)` backend →
entry + BuildContext + generate + plan (MVP: executable/library/test/custom
targets, `.jet/generated/`, local action cache, lock records) → explain/audit
surfaces → workspace composition + policy → enforce API (if D-METADEPTH2=B)
→ build-graph expansion bundles (§12).
