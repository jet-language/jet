# D-RUN-WHERE1 review: report kernel eligibility for any function

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-RUN-WHERE1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-RUN-WHERE1.

## Question

Should `jet explain kernel FILE:LINE` and hover report whether the canonical kernel checker can prove a function in a specified context?
`#Kernel` remains the checked declaration boundary, not an execution request.

## Findings that changed the ballot

1. **The scalar helper probes fail today.** D-GPU-HELPER-SCOPE1=A (ratified 2026-09-12, "Check the complete
   reachable helper in its kernel context") is not implemented for these probes.
   - k05: a `#Kernel` that calls the pure helper `triple(x) = x * 3` gets E1102, "cannot prove no reachable
     effects or opaque calls", at the marker.
   - k04, with a helper that prints, gets the same error.
   - Cause: `Compiler/JetSema/Source/Sema/Kernel.jet:156-160`. `sema_kernel_check_effects` fails when
     `call_edges.len() > 0` and reports `pending.span`, the marker.

   The report needs the ratified helper proof as build work. A cannot ship a permanent all-calls-fail substitute.
2. **The effects check keeps no line.** The check takes only effect names and call-edge names
   (`Kernel.jet:156`), so it cannot point at the print. The allocation check does keep a span
   (`Kernel.jet:146-149`, which k03 shows at 3:12). A's build work therefore includes effect facts that keep
   their line.
3. **`jet explain kernel k02_print.jet:2` today** prints the generic marker topic and ignores the location,
   with no error. Under A the location gets its meaning.
4. **The story said line 2.** The print in k02 is on line 3; line 2 is the marker. Fixed.
5. **Peer notes overclaimed.**
   - "Mojo: the parallel call is … checked there" is unsourced; it now reads "an explicit library call".
   - Julia's manual says avoiding data races is the user's job, and the note now says so.
   - Rust's Send and Sync are auto traits.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| k01 pure kernel | prints 42 |
| k02 kernel prints | E1102 at 2:1, the marker |
| k03 kernel builds a list | E1102 at 3:12, the literal |
| k04 kernel calls a printing helper | E1102 at 8:1, the marker |
| k05 kernel calls a pure helper | E1102 at 5:1, the marker |

No repository occurrence count is used as evidence of semantic coverage.

## Peers (sourced)

- Rust's Send and Sync are auto traits; Rayon's `par_iter` requests parallel iteration.
  <https://doc.rust-lang.org/nomicon/send-and-sync.html> and <https://docs.rs/rayon/latest/rayon/iter/trait.IntoParallelRefIterator.html>
- Julia's `Threads.@threads` marks the loop. The manual shows `@threads` exposing a data race and leaves
  avoiding races to the user. <https://docs.julialang.org/en/v1/manual/multi-threading/>
- The current Mojo functional source does not export the draft's `parallelize` example.
  The ballot no longer uses that comparison. <https://raw.githubusercontent.com/modular/modular/mojo/v1.2.0/Mojo/stdlib/std/algorithm/functional.mojo>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| The report shows a helper's lines (D-GPU-HELPER-SCOPE1=A) | Is the helper proof built? | **Failed**: k05 shows a pure helper is refused. A now includes the helper proof as build work |
| The effects error can name the print | Does the check have the line? | **Failed today**: no span is kept (`Kernel.jet:156-160`). It is now named build work |
| Builds pay nothing | Recording source facts and better marked diagnostics also cost resources | **Fails** as a zero-cost claim; no mandatory eligibility pass for unqueried functions is the narrower requirement |
| `jet explain kernel FILE:LINE` does not collide | Today it ignores the location | Holds: a fixed topic with an argument; no package lookup |
| Story | Line number | Fixed: line 3 |

## Remaining losses

- A plain function's label can change with its body. It is a report, not a promise.
- Queries can lack concrete context. Report not proved, and include the source snapshot and instantiation.

## Different-family review (GPT, 2026-11-10)

Reviewer: BallotReview7. Read the live ratified kernel-surface B, helper-scope A, acceleration A and honest-signature A decisions.
The latter permits inferred tool labels, not a second kernel execution path.

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| The marker launches parallel work | k01 calls the marked scalar function directly and prints 42. This is not a compute launch or worker-execution proof. The ratified declaration decision distinguishes checked declarations from invocation. | **Fails**; changed every ballot view to declaration boundary |
| Existing proof can report every check passed or failed | Origin/dev `Kernel.jet` returns at its first declaration, effect or body failure. `Items.jet` defines six flags but does not establish independent completed checks. | **Fails**; require one structured checker result with passed, failed and not proved |
| Pure helper support exists | Reran k04 and k05: exit 1, E1102 at caller markers. Ratified helper-scope A requires reachable-body proof. | **Fails today**; required before shipping A, not deferred behind a misleading preview |
| Errors identify effects and allocation | Reran k02: marker at 2:1. Reran k03: literal at 3:12. Source effect inputs contain names, not spans. | **Holds within stated limits**; line-preserving facts remain build work |
| Explain already accepts a location meaningfully | Ran `jet explain kernel k02_print.jet:2`; exit 0, generic syntax topic only. | **Fails today**; the query is proposed |
| A costs no build time | No measurements establish this; preserving spans and building diagnostics is work even without querying unmarked functions. | **Fails**; withdrew zero cost, bounded the new mandatory work |
| Every function has one stable eligibility label | Generics, targets, recursive helpers and unknown callees need context. An unproved function is not necessarily unsafe. | **Fails**; context-keyed answers, conservative unknowns, cancellation and invalidation required |
| B and D only hide information | B offers build-enforced explicit contracts without query infrastructure; D improves errors with smaller tool scope. | **Fails** as framing; both best cases now stated |
| C is only a hidden boundary | C removes author annotation work, but must replace the ratified marker rule explicitly and retain proof, behavior and selection gates. | **Holds within stated limits** after repair |
| Peer illustrations prove Jet's design superior | Read Rust auto-trait and Rayon documentation, Julia race warnings, and current Mojo source. None is a matched execution comparison. | **Fails**; illustrations labeled unexecuted, obsolete Mojo example removed |

Independent rerun outputs are `probes/review6-k01_ok.jet.run.txt` through `review6-k05_pure_helper.jet.run.txt`.
The inherited runner uses the `review6-` log prefix; no worker launch, GPU, O1/O2 or web claim follows from these runs.
Reviewed origin/dev `Compiler/JetSema/Source/Sema/Kernel.jet` and `Compiler/JetFoundation/Source/AST/Items.jet`.

Recommendation: agree with A only with the repaired shared-proof and helper requirements. No option added.
Do not post zero-cost claims, a marker-as-launch explanation, automatic pass flags, the obsolete Mojo example, or parallel execution claims from k01.
