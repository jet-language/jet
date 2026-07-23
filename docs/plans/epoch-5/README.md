# Epoch 5 — Metaprogramming

**Charter (owner, 2026-06-26 restructure, amended by D-METAMUTATE1=A):** E5 =
Jai-class build-time power under Jet's authority model, without Jai-style AST
mutation/message-loop/user macros. Historical E4-M* / D-E4EXIT1 identifiers keep
their names. Canonical design + executable
implementation plan: [`metaprogramming.md`](metaprogramming.md) (§15 is the
plan; do not duplicate it here).

**Slogan: Jai power, Jet authority model.**

## Gates

| Gate | Status |
|---|---|
| D-BUILDENTRY1=B, D-BUILDGEN1=A, D-BUILDPOLICY1=A, D-BUILDSCOPE1=A, D-METADEPTH2=B | ratified 2026-07-01 |
| `fetch(url, sha256:)` backend (D-NETDEP1) | shipped — no prereq remains |
| **D-E4EXIT1** — exit bar (MVP / +targets+actions / full §12 graph) | **ratified 2026-07-02 = C** |
| **D-BUILDFLAGS1** — single-file Tier-2 grant flag spelling | **ratified 2026-07-02 = A** — flag seam unblocked |
| **D-BUILDTARGET1=A**, **D-BUILDACTION1=A**, **D-BUILDTOOLCHAIN1=A**, **D-BUILDPROBE1=A**, **D-BUILDCACHE1=A**, **D-BUILDREMOTE1=A**, **D-BUILDSCHED1=A**, **D-BUILDQUERY1=A**, **D-BUILDLEGACY1=A**, **D-BUILDPLUGIN1=A**, **D-FRONTENDAPI1=A**, **D-DSLBLOCK1=A**, **D-METAMUTATE1=A** | ratified 2026-07-06 |

## Milestones

| # | Work | Plan | Gate |
|---|---|---|---|
| E4-M1 | build entry: detection, `BuildContext`/`BuildPlan`, plan→compile | §15.2 | none — buildable now |
| E4-M2 | generated source: materialize, additive-only, lock-hashed, `--locked` | §15.3 | E4-M1 |
| E4-M3 | authority: tiers, `#Impure` + permit, dependency deny, provenance | §15.4 | E4-M1; flag seam awaits D-BUILDFLAGS1 |
| E4-M4 | observe + enforce: `ProgramInfo` snapshot, `b.error` | §15.6 | E4-M1 (parallel with M2/M3) |
| E4-M5 | scope: entry homes, grant chain flag ⊂ pkg ⊂ workspace, `jet inspect audit` | §15.5 | E4-M1 + E4-M3 |
| E4-M6 | build-graph expansion (targets/actions/…) | §12 | decisions ratified; implement typed graph cards #219-#227 |

Build order (§15.1 DAG): **M1 → M4 ∥ (M2, M3) → M5 → M6**.

## Cards

| Card | Holds |
|---|---|
| c1nixrpd #95 | canonical foundation plan; decisions ratified; blocked on #219/#220 typed graph implementation |
| ~~c2iqs6m #38~~ | merged into c1nixrpd §15.4 (2026-07-02) — do not reopen |
| ~~c1mixqcn #94~~ | shipped `$` splice + `comptime {}` lower rung — not a macro/mutation card |
| c2kizq8n #128 | stdlib-only DSL block ballot D-DSLBLOCK1 |
| c147 #14 | serde bound-override reserve (D-SERDE11=A) — frozen, evidence-gated, not an exit item |
| ~~c154 #15~~ | closed rejected — D-METAMUTATE1=A; no AST mutation/message-loop/user macros |
| c0p8ieer #219 | D-BUILDTARGET1=A typed targets — ready to implement |
| c0q9le33 #220 | D-BUILDACTION1=A declared actions — ready to implement |
| c0ravub0 #221 | D-BUILDTOOLCHAIN1=A + D-BUILDPROBE1=A — ready to implement |
| c0s8ogje #222 | D-BUILDCACHE1=A + D-BUILDREMOTE1=A — ready to implement |
| c0t7j5xl #223 | D-BUILDSCHED1=A scheduler/resources — ready to implement |
| c0u55hlv #224 | D-BUILDQUERY1=A graph inspection UX — ready to implement |
| c0v2uuor #225 | D-BUILDLEGACY1=A legacy interop — ready to implement |
| c0w12hlg #226 | D-BUILDPLUGIN1=A sandboxed build plugins — ready to implement |
| c0wzd1dn #227 | D-FRONTENDAPI1=A public front-end toolkit — ready to implement |

## Exit criteria

Gated on D-E4EXIT1=C as ratified. §15's five exit-criteria blocks still pass:
each has examples, `tests/ui` fixtures, targeted driver tests, `jet
explain-build` / `jet inspect audit-effects`, and no-`fn build` byte-identical
behavior. #95 is no longer owner-decision blocked; implementation now runs
through the typed target/action graph first, because full build parity needs
typed targets and declared actions at the graph boundary. Follow-on cards keep
toolchains, probes, cache, remote execution, scheduler resources, graph UX,
legacy interop, plugins, and front-end APIs from becoming orphaned scope.
