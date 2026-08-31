# Dogfood 5/5: finding-to-card disposition ledger

Source report: `docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md`.
Campaign parent: `#2386`. This ledger maps every actionable finding in the
report to its owning card, ballot, existing owner, or recorded decline. The
hostile closeout pass (`#2394`) fails the campaign if it finds value in the
report or the tested experience that has no row here.

The Jet canary under `dogfood/jetpack/` is frozen evidence. No card in this
slate edits it or resumes parity work; resumption stays owner-gated on `#2327`.

## Slate cards

| Card | Title | Phase | Gate |
| --- | --- | --- | --- |
| #2386 | Campaign parent: earned-preference 5/5 | ready | none |
| #2395 | Same-subject dispatch lint + graded fix + editor action | ready | none |
| #2387 | Ownership diagnostics name callee, reuse, and `~` site | ready | none |
| #2388 | Failure-domain provenance and public-carrier teaching | ready | none |
| #2389 | Project-aware `jet check` | decide | D-CHECKSCOPE1 |
| #2390 | Public typed package model | decide | D-PACKAGE-MODEL1 |
| #2391 | Idiom suites (dispatch, failure, finite-state, ownership, wire) | ready | none |
| #2392 | Compile-time cascades carry causes | ready | none |
| #2393 | Fresh-agent 5/5 rerun | blocked on slate | none |
| #2394 | Hostile closeout pass | blocked on #2393 | none |

Extended existing owners: `#1026` (new warm-canary criterion), `#1310`
(rerun blocker, logged), `#2324` (capsule points at suites, logged), `#1259`
(narrower idiom follow-up handed to `#2395`, logged).

## Ballot policy for this slate

Two genuine owner choices exist: what a clean default `jet check` promises
(default-behavior change, D-CHECKSCOPE1 on `#2389`) and where the public
package model lives (new public Core surface, D-PACKAGE-MODEL1 on `#2390`).
The dispatch lint, ownership and failure diagnostics, cascade ordering, and
idiom suites carry no ballot: each implements the ratified lint law
(D-LINTPOLICY1), fix-grade law (D-REPORT-FIXGRADE1), report law
(D-REPORT-LAW1), failure foundation (D-FAILURE-FOUNDATION1), or examples law
(D-EXAMPLES-SUITES1), on a defect path the report names directly. No syntax,
default semantics, or invariant changes hide in them.

Both ballots were authored cooperatively: a parallel prep session attached
the validated ballots to these cards while this campaign's cross-family
adversarial reviews ran. The adversarial findings (scope split for explicit
file checks, receipt honesty, authority-scoped package reads, the
`@build.*` scalar-fact split, compile-time-only gating) are law in each
card's plan and criteria and match the ballots' recommendations.

## Disposition rows

| Finding | Category | Disposition |
| --- | --- | --- |
| F01 canonical-form discoverability | docs/examples | Owned by `#2391`: one golden-backed suite per idiom becomes the single source of truth. |
| F02 self-teaching failure effects | semantic | Owned by `#2388` (hover/diagnostics state the effective contract) plus the `#2391` failure suite. The ratified implicit route (`#2172`, D-FAILURE-FOUNDATION1) already removed the `Bool !E` + `Ok(false)` ceremony the port paid. |
| F03 failure-domain propagation churn | semantic | Resolved in law by `#2172`/D-FAILURE-FOUNDATION1 (implicit route, auto conversion). Residual diagnostic quality owned by `#2388`. |
| F04 public failure-carrier visibility | diagnostics | Owned by `#2388`: new registered visibility diagnostic naming the declaration to change. |
| F05 diagnostic names source helper | diagnostics | Owned by `#2388`: E2404 provenance (callee, span, effective contract, two graded repairs). |
| F06 root-cause localization and cascades | diagnostics | Owned by `#2392`: caused_by links plus root-first ordering at the projection seam. |
| F07 project-aware tier-complete check | tooling/check | Owned by `#2389`; depth is the owner's pick on D-CHECKSCOPE1. |
| F08 ownership-boundary diagnostics | diagnostics | Owned by `#2387`: E0121 gains callee + move site; E0120 gains a graded `~` edit. |
| F09 brace/interpolation authoring | docs/examples | Owned by `#2391` wire-output suite (pitfalls demonstrated in-file). |
| F10 package entry follows source graph | tooling/check | Resolved by closed `#2352`. The check-time proof of entry resolution is owned by `#2389`. |
| F11 isolated file checks carry context | tooling/check | Owned by `#2389`: file check resolves the owning graph or teaches the missing context. |
| F12 public typed package/profile models | stdlib/Core | Owned by `#2390`; surface home is the owner's pick on D-PACKAGE-MODEL1. Reuses `#610`/`#425`/`#653`/`#1517`/`#2098`; precedent `#696`. |
| F13 deterministic structured output | stdlib/Core | No new mechanism (I8): canonical JCS, typed writers, and framing handles are shipped. Teaching owned by `#2391`; default-run Codable parity owned by `#1310`. |
| F14 demand-driven incremental graph | build-perf | Existing owners `#666`/`#1023`/`#1026`/`#2346` and the ratified DEVR cone/reuse laws. Extended `#1026` with a warm-canary criterion pinned to the 8.096 s METRICS baseline. |
| F15 same-subject dispatch lint | lint/idiom | Owned by `#2395`: detector, graded rewrite, LSP action, quiet-cases. Extends `#1259`'s teaching without touching L0507 shapes. |
| F16 table-driven CLI parsing | lint/idiom | Evidence for `#2395`; canonical replacement shown in the `#2391` dispatch suite. |
| F17 one-pass CLI Result dispatch | lint/idiom | Canonical form exists (`#2173`, D-RESULT-DECON2). Teaching owned by `#2391`; nesting pressure evidence feeds `#2395`. |
| F18 package-kind dispatch | lint/idiom | Evidence for `#2395` (alias grouping with `|`). |
| F19 typed manifest parser state | lint/idiom | Owned by `#2391` finite-state suite (typed parser state over booleans). |
| F20 table-driven lock parsing | lint/idiom | Evidence for `#2395`; early-guard quiet case is a named criterion there. |
| F21 selector-key subject table | lint/idiom | Evidence for `#2395` (alias values). |
| F22 journal record-kind dispatch | lint/idiom | Evidence for `#2395` (dispatch-once-then-validate shape in the `#2391` suite). |
| F23 provider dispatch table | lint/idiom | Evidence for `#2395`. |
| F24 stringly parser state guidance | docs/examples | Owned by `#2391` finite-state suite: enum, variant group, tag, and typestate replace strings plus `seen_*` booleans. Mechanisms are shipped; no compiler work. |
| F25 deepen the large plan module | docs/examples | Absorbed into `#2391`: suite structure demonstrates parse/facts/render separation. The frozen canary itself is not edited. |
| F26 typed report values over wire JSON | docs/examples | Owned by `#2391` wire-output suite (byte-exact canonical writer + `#Codable` round trip). |
| F27 typed error-carrier consistency | docs/examples | Owned by `#2388` (visibility + contract teaching) and the `#2391` failure suite. Laws `#2172`/`#1712`/`#2326` already ratified the model. |
| F28 continuous worker-lane validation | process | Product half owned by `#2389` (cheap continuous check). Process half is a named protocol requirement on `#2393`. |
| F29 Codable across TIR deopt | bug | Existing owner `#1310` (ready); listed as a rerun blocker on `#2393`. |
| F30 default-tier evaluator/Core gaps | bug | Resolved by closed `#2252`. Regression net: registry conformance corpus (`#2285`/`#2286`). |
| F31 imported-module AOT roots | bug | Resolved by closed `#2350`. |
| F32 nested package output resolution | bug | Resolved by closed `#2352`. |
| F33 struct-typed Result matching | bug | Resolved by closed `#2354`; canonical form `#2173`/D-RESULT-DECON2. |
| F34 formatter preserves return arms | bug | Resolved by closed `#2355`. |
| F35 passthrough argv after `--` | bug | Resolved by closed `#2369`. |
| F36 imported-module LSP symbols | bug | Resolved by closed `#2370`. |
| F37 hidden FFI identity in cache keys | bug | Resolved by closed `#2371`; remaining warm cost is F14. |
| F38 read-only oracle lock mutation | bug | Resolved in the shipped Rust jetpack (recorded in METRICS.md); no Jet-side card. |
| F39 sovereign package boundaries | process | Standing constraint, recorded as explicit non-goals on `#2389` and `#2395`: automation optimizes inside declared packages, never redefines them. No card needed. |
| F40 owner-gated parity resumption | process | Owned by the `#2327` hard gate (building, owner go required). The slate honors it: no card touches `dogfood/jetpack/**`. |
| F41 modular AOT test codegen/discovery | tooling/check | Root defects resolved by closed `#2350` and `#2066`. No reproducible defect remains to card; the `#2393` rerun re-proves the workflow and reopens owners on failure. |
| F42 shrink first-plausible-to-green | process | The campaign aggregate: parent `#2386`; measured as correction-pass count in the `#2393` protocol. |
| F43 matched Rust comparison envelope | rerun-protocol | Owned by `#2393`: every task runs a matched Rust arm from cold context. |
| F44 matched experienced authoring time | rerun-protocol | Owned by `#2393`: wall time per arm is a named measure. |
| F45 Rust LSP latency baseline | rerun-protocol | Owned by `#2393`: protocol requires a working rust-analyzer baseline or records the gap explicitly. |
| F46 long-term maintenance cost | rerun-protocol | Declined for this campaign: a longitudinal measure cannot fit a bounded rerun. Recorded as an explicit protocol non-goal on `#2393`; the owner may commission a follow-up study. |
| F47 full provider/package-universe parity | rerun-protocol | Owner-gated: full parity is exactly the work paused by `#2327`. Not measurable until the owner reopens it; the rerun uses non-Jetpack matched tasks instead. |
| F48 expert preference after defect removal | rerun-protocol | Owned by `#2393`: the blind preference question after both arms is this measurement. |
| F49 repeatable rerun/missing-data protocol | rerun-protocol | Owned by `#2393`: the protocol adopts the METRICS.md matched-input and `not measured` laws verbatim. |

## Open decisions

| Decision | Card | Choice |
| --- | --- | --- |
| D-CHECKSCOPE1 | #2389 | What a clean plain `jet check` promises. |
| D-PACKAGE-MODEL1 | #2390 | Where the public package model lives. |

## Dependencies

- `#2393` (rerun) is blocked by `#2395`, `#2387`, `#2388`, `#2389`, `#2390`,
  `#2391`, `#2392`, and `#1310`.
- `#2394` (hostile pass) is blocked by `#2393`.
- `#2389` and `#2390` wait in decide on their ballots; everything else is
  ready now.

## #2393 rerun record (2026-08-30)

The protocol is recorded in
[`docs/audits/fresh-agent-5-of-5-rerun-2026-08-30.md`](../audits/fresh-agent-5-of-5-rerun-2026-08-30.md).
The rerun did not start. The prerequisite slate is still open, so there are no
raw scorecards, category scores, wall-time samples, diagnostic encounters, or
preference answers to report. `not measured` stays `not measured`; it is not a
zero or a pass.

| Item | 2026-08-30 state | Evidence or next owner |
| --- | --- | --- |
| Protocol | recorded | Rerun report, protocol section |
| Agent sourcing | not measured | `#2393` after the prerequisite slate closes |
| Matched task arms | not measured | Rerun report, four frozen task contracts |
| Raw per-agent scorecards | none captured | Do not substitute the retrospective ten-agent table |
| Seven-category medians | not measured | Fixed pass bar in the rerun report |
| Blind preference | not measured | Asked only after both arms finish |
| Testimony closure | mapped; unresolved rows remain | Ledger below and disposition rows above |
| Campaign verdict | `FAIL — gate not met` | A blocked run cannot claim 5/5 |

The separate cold-agent harness is also blocked. Its checked-in scoreboard has
zero rows because the required OpenAI and Anthropic adapters have no configured
credentials. That artifact is a preflight record, not a scorecard for this
campaign.

### Testimony closure ledger

The original testimony table is at
`docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md:280-291`.
Every friction and improvement statement has a row below. The preference
qualifiers are recorded as `T-PREF`; they remain open until the blind question
is answered.

| ID | Original statement | Finding(s) | Owner or evidence | State at rerun gate |
| --- | --- | --- | --- | --- |
| T01 | Hand-written state machines and brace/interpolation confusion; teach canonical table-driven parsing. | F01, F09, F19, F24 | `#2391` finite-state, dispatch, and wire suites | `open` — no fresh-agent proof |
| T02 | The 970-line scanner and renderer caused repeated ownership edits; add typed profile parsing and deterministic output. | F12, F13, F25, F26 | `#2390`, `#2391` | `open` — owner choice and suite proof remain |
| T03 | Project-root imports and incomplete module checks; check from any source file with project context. | F07, F10, F11 | `#2389` | `open` — ballot and rerun proof remain |
| T04 | Direct fallible-call matching propagated instead of binding; make Result matching reliable. | F33 | `#2354` is closed; the dogfood report records the fix at `:221` | `resolved in implementation; rerun confirmation open` |
| T05 | Entry resolution needed adapters and several checks; follow the checked source graph. | F10, F31, F32 | `#2352` is closed; `#2389` owns project-check proof | `partial — implementation fixed; experience proof open` |
| T06 | Reusing values across consuming Core APIs required many `~` copies; improve move diagnostics. | F08 | `#2387` | `open` — no diagnostic proof |
| T07 | Cross-module AOT and test reachability needed correction; make modular AOT discovery reliable. | F31, F41 | `#2350` and `#2066`; dogfood report `:219` and `:288` | `resolved in implementation; rerun confirmation open` |
| T08 | Error-domain mismatches appeared at call sites; give fix-its that name the source helper. | F03, F05 | `#2388` | `open` — no diagnostic proof |
| T09 | View materialization and literal-brace syntax caused repair work; improve ownership guidance. | F08, F09 | `#2387`, `#2391` | `open` — no suite or diagnostic proof |
| T10 | Helper error domains and public error visibility were hard to learn; give first-class guidance. | F03, F04, F05 | `#2388` | `open` — no diagnostic proof |
| T-PREF | Agents gave only production-negative qualifiers: no production preference, cautious/conditional/guarded yes, or “not package tooling yet.” | F48 | `#2393` blind preference question | `open` — no preference answers |

Two non-testable scope statements remain explicit. F46 is declined for this
bounded campaign because long-term maintenance needs a longitudinal study. F47
stays owner-gated with `#2327` because full provider and package-universe parity
is the paused Jetpack replacement work. Neither is silently dropped.

## Next burndown scope

Once the two ballots are ratified: burn `#2395`, `#2387`, `#2388`, `#2392`,
`#2391` (independent paths: parser/sema lint seam, ownership checker,
fallible checker + registration, diagnostics projection, examples) plus
`#2389` and `#2390` per their ratified outcomes, with `#1310` in the same
wave. Then run `#2393`, and finish with `#2394`.
