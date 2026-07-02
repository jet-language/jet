# Epoch 5 — Jai metaprogramming

**Charter (owner, 2026-06-26 restructure):** E5 = Jai metaprogramming (renumbered from E4 in the 2026-07-02 epoch swap with jetpack/jetos; historical E4-M* / D-E4EXIT1 identifiers keep their names) — Jai-class
build-time power under Jet's authority model. Canonical design + executable
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

## Milestones

| # | Work | Plan | Gate |
|---|---|---|---|
| E4-M1 | build entry: detection, `BuildContext`/`BuildPlan`, plan→compile | §15.2 | none — buildable now |
| E4-M2 | generated source: materialize, additive-only, lock-hashed, `--locked` | §15.3 | E4-M1 |
| E4-M3 | authority: tiers, `#Impure` + permit, dependency deny, provenance | §15.4 | E4-M1; flag seam awaits D-BUILDFLAGS1 |
| E4-M4 | observe + enforce: `ProgramInfo` snapshot, `b.error` | §15.6 | E4-M1 (parallel with M2/M3) |
| E4-M5 | scope: entry homes, grant chain flag ⊂ pkg ⊂ workspace, `jet audit` | §15.5 | E4-M1 + E4-M3 |
| E4-M6 | build-graph expansion (targets/actions/…) | §12 | **D-E4EXIT1 decides scope** |

Build order (§15.1 DAG): **M1 → M4 ∥ (M2, M3) → M5 → M6**.

## Cards

| Card | Holds |
|---|---|
| c1nixrpd #95 | the §15 plan, E4-M1..M6 (both gates ratified 2026-07-02; card ready) |
| ~~c2iqs6m #38~~ | merged into c1nixrpd §15.4 (2026-07-02) — do not reopen |
| c147 #14 | serde bound-override reserve (D-SERDE11=A) — evidence-gated, not an exit item |
| c154 (e7) | rung C (message loop / user macros) — frozen, not e5 |

## Exit criteria

Gated on D-E4EXIT1. Under every option: §15's five exit-criteria blocks all
pass (each has its examples, `tests/ui` fixtures, and targeted driver tests
green); `jet explain-build` / `jet audit-effects` work; a program with no
`fn build` behaves byte-identically to today. Option C additionally requires
D-BUILDTARGET1 + D-BUILDACTION1 ratified and shipped; option B all six §12
bundles.
