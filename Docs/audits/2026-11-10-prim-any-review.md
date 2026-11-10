# D-PRIM-ANY1 review: an explicit Any box on the trait-value carrier

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-PRIM-ANY1. Earlier pass: BallotFix REPORT.md, section D-PRIM-ANY1.

## Question

Should Jet get an explicit `Any(value)` box that keeps any owned value, including files, functions and duties,
with checked reads and preserved duties? A reuses trait-erasure semantics but adds an inline representation.

## Findings that changed the ballot

1. **The cleanup paragraph still named a descriptor.** A's own "No descriptor" paragraph removes the type
   record, yet "Cleanup" said drops run "through the descriptor". The text now says cleanup is selected by
   the key word, which is what `Compiler/JetIR/Source/BuildTraits.jet:1-6` does today: "the last owner drops
   the payload and frees the box (ir_trait_drop_body)". The internal label "S63" is also removed.
2. **The cost of today's box is now measured, not disclaimed.** `probes/cost/trait_box.jet` and `enum_box.jet`
   build one million small `Square` and `Rect` values and sum their areas. One version uses a `[Shape]` of
   trait values, the other a closed enum matched by a function. Built with `jet build` and run 5 times each
   (output 9500005 for both):

   | Form | runs (s) | median | peak RSS |
   |---|---|---|---|
   | trait values (today's counted heap box) | 0.119 0.114 0.119 0.136 0.115 | 0.119 | 78.6 MB |
   | closed enum | 0.106 0.110 0.110 0.111 0.107 | 0.110 | 78.6 MB |

   This is an end-to-end comparison, not an allocation-cost measurement or a bound on inline savings.
   The programs also differ in representation, dispatch and cleanup. No Any implementation was measured.
   The different-family rerun below uses fresh builds and alternating process wall times.
3. **Origin/dev source check.** BuildTraits computes a key from seven SHA-256 bytes, then adds one,
   and rejects equal keys for distinct encountered identities. Prelude Traits declares consuming infallible Close.
   Neither fact proves complete plugin cleanup or effect-free destruction.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result | Board claim |
|---|---|---|
| p01 `[Any]{3, "three"}` | E0112 | same |
| p02, p13 Dyn | `true`, `wrong type`; `true`, `not text`, `14` | same |
| p03 trait list | `10` | same |
| p05 closed enum | `flag true`, `size 14` | same |
| p10 `.Square(sq)` on a trait value | E0305, E0107 | same |
| p11 unknown name | E0112 | same |
| p14 copy a trait value | E0211 | same |
| c01 branch on `#Secret` | passes (no tag error) | branch tagging unbuilt, same |

## Peers (sourced)

- Rust: `Box<dyn Any>`; `downcast` returns `Err(self)` on a mismatch; `Any` needs `'static`.
  <https://doc.rust-lang.org/std/any/index.html>
- Swift: existentials use "a fixed-size buffer, which is three pointers" and inline values that fit.
  <https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeLayout.rst>
- Go: `any` converts implicitly, and a type switch needs no exhaustiveness. <https://go.dev/ref/spec#Type_switches>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| No descriptor | Cleanup paragraph | **Failed**: it still named one. Fixed |
| No cost comparison claimed | Can the delta isolate carrier allocation? | **Fails**; it compares complete trait and enum programs |
| Second layout is irremovable | Move all trait carriers to inline storage | **Fails**; added G exposes that alternative and its costs |
| Close needs no effect | Omitted bounds are inferred; an effectful Close executes in the new review7 probe | **Fails**; retain cleanup effect contracts |
| Branch tagging unbuilt | c01 | Holds on a0.0.43 |

## Remaining losses

- A keeps a second backend layout; added G removes that split through a coordinated trait-layout cutover.
- Four words per Any carrier do not include allocations owned by its payload or retained Type metadata.
- Four ownership/sendability forms also need normalized cleanup and on_fail effect contracts.
- A container holding a duty must settle or hand on that duty.
- Tagged delivery requires branch tagging and protected movement; a general checker gap is not an exception.

## Different-family review (GPT, 2026-11-10)

Reviewer: BallotReview7. Read live D-OWES1=A, D-LIN-CONTAINER1=A, D-EFFECT-OMIT1=A,
D-MEM-WIPE1=A, D-SECRET-CT1=A and D-PRIM-TYPEKEY2=A.
TYPEKEY2 and SECRET-CT were ratified during the ballot work; the proposal now treats them as binding, not open choices.

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| Any is missing, so all open plugin storage is missing | p01 fails, but p03 dispatches through traits. Interfaces retain unknown plugin state without arbitrary concrete recovery. | **Fails** as framing; B strengthened |
| Existing carrier works unchanged for an empty automatic trait | Origin/dev BuildTraits discovers implementers from method-bearing functions and slots. An empty trait needs an erasure-site payload census. | **Fails**; registration, generic and methodless payloads are required work |
| Close has no written row, therefore no effect | The ratified omission rule says omitted rows are inferred. New `review7-close.jet` executes `print` from Close and prints `closed 7`. | **Fails**; cleanup and on_fail effects survive erasure |
| Four forms cover every static obligation | Duties also carry effect rows; tags, region provenance and authority remain relevant. | **Fails**; carrier forms do not erase those facts |
| Failed take finishes an Owes box | A mismatch retains its original payload and therefore its duty under container and handed-on rules. | **Fails** if treated as ordinary consuming completion; explicitly preserve duty |
| Every exit cleans | Ratified OWES excludes abort, kill and power loss. Cancellation and panic require actual unwinding. | **Holds within stated limits**, now explicit |
| An existing secrecy hole means Any adds no risk | c01's leak function compiles but is not called by run. Neither runtime leakage nor safe erased identity is established. | **Fails**; tagged identity and dispatch must be checked before delivery |
| Side storage wiped on drop is complete protection | Ratified MEM-WIPE includes controlled moves, registers, swap and crash contexts. | **Fails**; preserve protection during construction, moves and extraction too |
| Current trait key glue proves open plugin cleanup | BuildTraits emits a finite key-selection routine over known implementers; a live loaded payload requires admitted cleanup and code lifetime. | **Fails** as current proof; require loader integration and collision admission |
| Descriptor and allocation costs disappear | Small carrier storage is inline, but payload buffers, side storage, dispatch code and Type metadata remain. | **Fails** globally; narrow allocation claim to fitting nonsensitive carrier payload |
| The trait/enum delta isolates 9 ns allocation cost and bounds savings | Rebuilt both programs; outputs match, but dispatch and layout differ too. Alternating rerun has different medians. | **Fails**; no causal allocation or predicted Any gain claim |
| C necessarily adds another runtime mechanism | A top-type front end can lower to canonical trait erasure. | **Fails**; distinguish surface model from mechanism |
| D cannot meet the plugin workload | Typed resource fields beside an open data box still perform it. | **Fails**; D lacks erased resource recovery, not plugin behavior |
| E accepts wrong types but A prevents them | A makes erasure explicit; Any construction still accepts any eligible type. | **Fails** as safety framing; E's uniformity is a real strength |
| F rewrites every trait use | Only implicit erasure construction sites change; reads need not. No retired-spelling jet fix service is permitted. | **Fails**; narrowed migration scope and removed rewrite promise |
| A's second layout is unavoidable | Unified inline trait representation removes it at the cost of larger carriers and a coordinated cutover. | **Fails**; added G |

Independent p01-p14 and c01 logs: `probes/review7-*.run.txt`. p02 prints `true`, `wrong type`;
p03 prints `10`; p05 prints `flag true`, `size 14`; p13 prints `true`, `not text`, `14`.
p10 reports E0305 and E0107; p14 reports E0211. None runs the proposed Any API.

Fresh `jet build` succeeded for both cost subjects. Five alternating executions all exited 0 and printed 9500005.
Wall times, including process launch:

| Form | Five runs, seconds | Median, seconds |
|---|---|---|
| Existing trait values | 0.134161, 0.086548, 0.076587, 0.074678, 0.071074 | 0.076587 |
| Closed enum | 0.140065, 0.064662, 0.068160, 0.066419, 0.070565 | 0.068160 |

`probes/review7-measure.txt` retains exact results. No independent peak-memory measurement was available.
The earlier peak-memory figures remain attributed historical observations, not this review's proof.
Current Swift ABI source confirms the three-pointer buffer **plus metadata and witnesses**;
Rust Any documents non-static reference exclusion and checked recovery; Go specifies the empty interface and type switches.
These peer illustrations were read, not executed or benchmarked.

Recommendation: conditional agreement with A's explicit surface, not its old safety or speed claims.
G is a strong added alternative; no measurement decides A versus G.
Do not post a predicted inline speedup, effect-free cleanup, obsolete TypeKey projections, a tagged-safety deferral, or a descriptor-free global cost claim.
