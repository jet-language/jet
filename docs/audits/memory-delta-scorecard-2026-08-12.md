# Delta scorecard — blind ideal vs current+ratified Jet

Fixed rubric: 10 domains × 13 metrics = 130 cells. Sources: `local://blind-design.md` (ideal; grades ● win ◐ contested ○ lose, mechanism-argued, perf cells [INFERENCE]) and `local://advocate-dossier.md` (current; grades A–F from 57 live probes, tags S shipped / D defective / R ratified-unbuilt / O open / A absent).

Delta legend: **=** same shape (current substrate already is the ideal's mechanism, built or ratified) · **build** ideal mechanism accepted by current design but unbuilt/defective · **gap** ideal has a mechanism current design lacks entirely · **jet+** current Jet has something the ideal lacks.

Evidence column: who carries the stronger evidence for the delta judgment. `probe` = advocate live probe (strong), `mech` = blind mechanism argument (unmeasured), `conv` = independent convergence (both derived it separately — strongest).

## Metric 1 — Safety by default

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ● `heap:none` + budget proof at compile time | D- — freestanding gates dead (P10: heap+file-IO build clean) | build — same idea (E3301/E3303, D-TARGET-ALLOC1) registered, never fires | probe |
| Kernel | ● trust-fenced, policy per subsystem | C — unsafe islands audited, no volatile/interrupt story probed | build | probe |
| Game | ● pools+gen handles kill dangling-entity class | B — `#Policy(no_alloc)` transitive works; Pool regressed | build — Pool/Id IS the gen witness, broken today (P17) | conv + probe |
| RT | ● refusal policy: no pause mechanism exists in binary | B+ — no_alloc + zero_rc fire; no allocator cap | build | conv |
| SRV | ● escape → inferred rc/gc region, ledgered | B — ownership+close prevents leaks; long-lived pointing has no answer (Q1) | **gap — the decisive cell** | probe |
| PIPE | ● zero-copy + freeze for readers | A- — views checked end-to-end both tiers (P15, P16) | = | conv |
| SH | ● inferred everything | A — zero annotations, full checking (P01) | = | conv |
| WASM | ● foreign region, engine witness | C? — wasm builds; no interop surface exists | gap | probe |
| FFI | ◐ declared verbs, containment not proof | A- — E0702 one-rule boundary impossible to get wrong | jet+ at floor / gap at ceiling | probe |
| AGT | ● compile-time verdicts everywhere | B — checker excellent; tier parity broken (P12) | build — parity is I9, owed not designed | probe |

## Metric 2 — Read/write/reason ergonomics

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ◐ budget checker is the job | C — Fixed.over works; no 2KB profile path | build | probe |
| Kernel | ◐ policy lattice must be learned | B — reason-gated unsafe reads well | ≈= | probe |
| Game | ● one canonical spelling (pool+handles+frame arena) | D — Pool broken, disjoint windows rejected, recursive enums broken | build (three defects) | probe |
| RT | ◐ explicitness demanded by domain | B — facts are one line at right scope | ≈= | probe |
| SRV | ● region-per-request, nothing said twice | C — concurrency surface churned under examples | gap+build | probe |
| PIPE | ● zero-copy without signature infection | C+ — views can't enter collections/lambdas; forced `~` copies (P04j) | **gap — the `~`-wall cell** | probe |
| SH | ● nothing written | A | = | conv |
| WASM | ● handles just work | ? unprobed | gap | mech |
| FFI | ◐ three-verb ceremony | B — one signature form, no annotations | jet+ floor / gap ceiling | probe |
| AGT | ● few tokens, callsite-legible | A- — sigils at callsite = diff-readable, no context needed | **jet+ — sigil mirroring beats ideal's &/&mut** | probe |

## Metric 3 — Runtime performance + predictability

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ● fixed backings only | B? unmeasured [INFERENCE] | ≈= design, unmeasured both | neither |
| Kernel | ● | B? [INFERENCE] | ≈= | neither |
| Game | ● zero-alloc steady state checked | C — arenas+reset shipped; default tier corrupts Map (P12) | build | probe |
| RT | ● no pause mechanism in binary, WCET=1 cmp | B? — mechanism sound; JIT divergence forbids trusting dev tier | build | probe |
| SRV | ◐ rc cycles leak until traced | C — arena/reset works; #1883 disqualifying until fixed | build | probe |
| PIPE | ● bulk free, freeze fan-out | B+ — zero-copy parse both tiers | ≈= | probe |
| SH | ● | A | = | probe |
| WASM | ◐ handle indirection inherent | ? | ≈= (platform-bound) | mech |
| FFI | ● loan = zero-copy | C — mandatory copies every crossing | **gap** | probe |
| AGT | ● | C — dev oracle ≠ prod tier | build (I9) | probe |

## Metric 4 — Compile-time cost of checking

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ◐ budget proof slowest analysis | A — 0.8s verdicts | jet+ today (checker cheap; budget prover unbuilt) | probe |
| Kernel | ◐ | A | jet+ | probe |
| Game | ◐ monomorphization tax | A | jet+ | probe |
| RT | ◐ | A | jet+ | probe |
| SRV | ◐ | A | jet+ | probe |
| PIPE | ○ million-line flow analysis [INFERENCE] | A at probe scale; unmeasured at scale | unknown at scale — both honest | neither |
| SH | ● | A | = | probe |
| WASM | ◐ | A | jet+ | probe |
| FFI | ◐ | A | jet+ | probe |
| AGT | ◐ inference ripple re-verdicts callers | A — 0.8s; but release truth costs 26–30s when parity forces AOT confirmation | mixed | probe |

Note: current A grades measure the shipped checker on small programs; the ideal's ◐/○ price its *additional* analyses (budget proof, promotion inference). Deltas here are honest unknowns, not wins to bank.

## Metric 5 — Learnability (two-sentence test)

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ◐ | B — "no heap unless you make one" true but unenforced | build | probe |
| Kernel | ○ policy lattice before first driver | B | jet+ (fewer concepts today) | probe |
| Game | ◐ | B+ — arena-per-frame card probed true | ≈= | probe |
| RT | ○ witness tiers before first loop | B | jet+ (fewer concepts) | probe |
| SRV | ● | B | ≈= | mech |
| PIPE | ● | B+ | ≈= | probe |
| SH | ● | A — two sentences, probed true | = | conv |
| WASM | ● | ? | gap | mech |
| FFI | ◐ | A — "copy in, copy out, each side frees its own" | **jet+ — one-rule FFI beats three verbs on this metric** | probe |
| AGT | ● | A- | = | probe |

## Metric 6 — Expert control ceiling

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ● fixed/manual regions, budget | C — raw tier exists; typed board facts unbuilt | build | probe |
| Kernel | ● MMIO typed registers, trust allowlist | B — `*T` grammar + obligations shipped; asm unprobed | build | probe |
| Game | ● backing swap, steady-state policy | C — no allocator swap (#1853 open) | **gap — allocator/backing axis absent** | probe+board |
| RT | ● witness refusal | C — no swap/cap | gap | probe |
| SRV | ● epoch arenas, per-region choice | B — guards, #Transact exist | build | probe |
| PIPE | ● | B — `from a \| b` provenance is expert-grade | ≈= | probe |
| SH | ● (n/a-ish) | n/a | = | — |
| WASM | ◐ no layout control over JS heap | Absent | gap | probe |
| FFI | ● loan/gift/bind + deleters | D — no zero-copy, no transfer, no callbacks | **gap — decisive cell** | probe |
| AGT | ● policy pinning | B | build | probe |

## Metric 7 — Diagnostics + repair determinism (product)

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ● names op + call chain | B — E0420/E0103 taught well | ≈= | conv |
| Kernel | ● | A- — E0208/E3112 name exact wrapper | = | probe |
| Game | ● | B — E0632 what/why/fix perfect | = (crown shipped) | probe |
| RT | ● | B+ — E0921 names allocating op through call path | ≈= (path-spam defect) | probe |
| SRV | ● | B | ≈= | probe |
| PIPE | ● | C — E2307+E0108 contradict on same line (P04h) | build (defect) | probe |
| SH | ● | A- | = | probe |
| WASM | ● | ? | gap | mech |
| FFI | ◐ can only name the obligation | A- — E0702 why/fix exact | jet+ | probe |
| AGT | ● machine-applicable canonical fix first | B+ — no `jet fix`; fixes are prose | build — `jet fix` absent | probe |

## Metric 8 — FFI/embedded fit

| Domain | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| MCU | ● | D- — the freestanding gate is the fit and it is dead | build | probe |
| Kernel | ● | B — by-value rule wrong for kernel zero-copy | gap | probe |
| Game | ● | n/a unprobed | ? | — |
| RT | ● | C — copies on hot path | gap | probe |
| SRV | ● | B | ≈= | probe |
| PIPE | ● | B | ≈= | probe |
| SH | ● | n/a | = | — |
| WASM | ◐ | D — boundary absent beyond build | gap | probe |
| FFI | ◐ | B+ — bind generator exists, audit surfaces unprobed | mixed | probe |
| AGT | ● | B | build | probe |

## Metrics 9–13 — the five agent-optimality quantities

Advocate graded these globally (three probed regimes); per-domain rows collapse accordingly.

| Quantity | Blind ideal | Current+ratified | Delta | Evidence |
|---|---|---|---|---|
| (a) Verdict fidelity | ● all domains except FFI ○ (declared not proved) | C — A-grade checker, F-grade tier parity; sema-approved programs ICE in AOT (P03/P04d/P04i); dev tier corrupts Map (P12); freestanding dead (P10) | build — parity is owed by I9, not a design question; FFI ○ shared by both | probe |
| (b) Verdict latency | ● except PIPE ○, GAME/AGT ◐ (inference ripple) | A dev (0.75–0.8s × 57 probes) / C when AOT confirmation forced | jet+ today at probe scale; ideal honestly prices its extra analyses | probe |
| (c) Verdict actionability | ● format-mandated canonical fix, `jet fix` applies it | B+ — best-in-class text; no machine-apply; E0921 path-spam; E0361 span on comments | build — adopt fix-first format + `jet fix` | conv |
| (d) Context economy | ● except KRN/RT ◐ (domain demands tokens) | A- — zero annotations common case; sigil = 1 char; E0921 outlier | = — both models near-optimal; sigils are jet+ | probe |
| (e) Repair determinism | ● except SRV/AGT ◐ (witness choice = two repairs until policy pins) | B+ — one repair for E0202/E0121/E0632/E3112/E0702; nondeterministic at E0220-disjoint and the `~`-wall | mixed — ideal's promotion adds a second repair axis; current's walls remove repairs by removing capability | probe |

## Cells where current Jet beats the blind ideal (jet+)

| Cell | Why |
|---|---|
| AGT × ergonomics/context | Callsite capability mirroring (`&`/`^`/`~` at the callsite) — the ideal's `&`/`&mut` marks the callee, not the caller's diff. Probed (P01, P02b); no equivalent in the blind design. |
| FFI × learnability | One-rule by-value boundary ("each side frees its own") is a two-sentence card; three verbs are not. |
| KRN/RT × learnability | Fewer concepts on the expert floor today (no witness-tier lattice to learn first). |
| Compile cost (all) | Shipped checker verdicts in 0.8s; the ideal prices budget proofs and promotion inference it hasn't paid. |
| Teaching vocabulary | window/view/place/owner is consistent across E0212/E0220/E0631/E0632 — a probed asset the ideal doesn't have. |

## The decisive divergences (do not average)

| # | Divergence | Blind position | Current position | What settles it |
|---|---|---|---|---|
| 1 | **Escape past every scope** (cache, interner, observer list, graph) | Promote smallest region to cheapest dynamic witness (rc/gc), inferred + ledgered + refusable | Hard wall: `~` copy, `Shared` lock, or Pool (broken); C1 "moot in v1" | Owner call on priority 2 vs 3 — this is the single largest design delta and advocate Q1 proves no compiling answer exists today |
| 2 | **Allocator model** | Allocators are region backings; ambient destination region; swap = rebind | Arenas exist; no swap/wrap/cap; #1853 open confirms the felt gap | Convergent need; design question is backing-as-value vs allocator-parameter |
| 3 | **Concurrency story** | Same law: transfer = move region, share = freeze region, one writer per region | Separate mechanism: Shared/Cell/guards | Owner call: unify onto regions or keep two stories |
| 4 | **FFI ceiling** | loan/gift/bind typed verbs, zero-copy safe tier | By-value only; zero-copy = #Unsafe | Both are coherent; rubric FFI×perf/ceiling cells condemn by-value-only |
| 5 | **Witness spelling** | Witness is per-region and first-class (`region c: rc`) | Witness is per-type/per-API (Shared<T> = count, Pool = gen, arena = scope) | The ideal's frame subsumes current surfaces as points; adopting the frame ≠ changing the spellings |

## Bottom line

Current Jet ≈ the scope-witness column of the blind design plus better callsite ergonomics, minus the dynamic-witness tiers, minus the backing/ambient-region axis, with the enforcement spine (tier parity, dead gates, Pool, recursive enums) broken in ways no design change fixes. The blind ideal ≈ ratified Jet generalized to a lattice, with the promotion ledger as its riskiest novel piece (its own worst thing #1).
