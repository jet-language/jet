# D-REGION-ALLOC1 review: values leave the arena block only by an explicit copy

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-REGION-ALLOC1. Earlier pass: BallotFix REPORT.md, section D-REGION-ALLOC1.

## Question

Inside `#Context(allocator: arena) { … }`, should a value that would outlive the block be refused (E0631)
unless it leaves through `.copy()`? Or should it move out by itself, as D-CONTEXT1=A ratified?

## Findings that changed the ballot

1. **A's precedent for function-type facts is unbuilt.** A closes the r8 hole, where a closure capturing
   arena data is accepted, by carrying escape facts in function and trait-method types, "the way
   D-FNVAL-WRITE1 keeps write parameters". D-FNVAL-WRITE1=A was ratified on 2026-11-05. New probe r15 declares
   `State: struct { action: fn(&Mobj) }` and gets E0003 "Expected a type, found `&`".
   This proves that grammar case is missing, not that every callable fact is absent.
   A now requires complete callable provenance before shipping instead of presenting a temporary blanket refusal as completed design.
2. **package.api needs one more column.** D-API-SURFACE1's A lists each row's effects, error sets, markers
   and `#if` conditions. Escape facts are not among them, so A names them as one more column there.
3. **Line range and wording.** `ir_context_block` is at `Compiler/JetIR/Source/BuildTransact.jet:250-275`
   (was 244-275) and still installs only a deadline (`jet_ctx_push_deadline`). The lesson, detail, rec and
   the code comments said "your code" and "you did not write", and the short rec said "the block you
   approved". All are reworded.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result | Board claim |
|---|---|---|
| r1 request handler | `ada=3;lin=3` | same |
| r2 expression form | E0733 UnknownTag | same |
| r3 context.scratch | E0107 UnknownName | same |
| r4 frame loop | `3`, `frame 2` | same |
| r5 parser | `6` | same |
| r6 return arena view | E0631 | same |
| r7 list after close | `made in the arena` | (block does not retarget today) |
| r8 / r8b closure capture | `5` / `made in the arena` | same: the hole |
| r9 task inside | `from the task` | |
| r10 generic allocator | E0904 | same |
| r11 / r11b region block | `7` / E0201 | |
| r12 Shared inside | `inside` | |
| r13 / r13b push under !Mem.Alloc | `1` / `1` | same: known gap |
| r14 store view in list | E0201 | |
| r15 `fn(&Mobj)` field (new) | E0003 | FNVAL-WRITE1 unbuilt |

No occurrence count is used as proof of complete migration or unchanged output after allocator retargeting starts.

## Peers (sourced)

- Odin: the implicit context allocator. <https://odin-lang.org/docs/overview/#implicit-context-system>
- Zig: an allocator argument on every allocating call. <https://ziglang.org/documentation/master/#Choosing-an-Allocator>
- Rust bumpalo: collections carry the arena's lifetime. <https://docs.rs/bumpalo/latest/bumpalo/>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Function values carry escape facts like write marks | Is the precedent built? (r15) | **Holds within limits**: one grammar case fails; complete escape summaries remain build work |
| Escape facts recorded in package.api | D-API-SURFACE1 row contents | Conditional on that open ballot; canonical package checking must enforce them regardless |
| One copy fixes every library store | Fresh internal allocation, shared graphs and read-only dependencies | **Fails**; copying an argument cannot repair all retaining stores |
| Probe results | Rerun on a0.0.43 | Hold |
| Plain language | "you"/"your" | **Failed**. Fixed |

## Remaining losses

- Caching dependencies may need updates or calls moved outside the block.
- Destination-directed copy needs a proven use graph; ambiguous targets are refused.
- Shared contents, pools, resources and erased callables need complete graph provenance.
- Automatic relocation remains a strong alternative for unchanged library reuse.

## Different-family review (GPT, 2026-11-10)

Reviewer: BallotReview7. Read live CONTEXT1=A, ALLOC-SITE1=A, JAI-CONTEXT1=A,
FNVAL-WRITE1=A, ALLOC-TRAIT1=A and HTTP-REQMEM1=A; API-SURFACE1 is still open.
The newer CONTEXT1 already assigns spawned tasks the program allocator and their own scratch.

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| A merely implements the owner's approved block | CONTEXT1 explicitly relocates escaping owned values automatically. | **Fails**; A is an escape-policy amendment, E preserves the ratification |
| Current output proves arena behavior | Reran all r subjects. r1 and r4 outputs agree; origin/dev BuildTransact:250-275 ignores non-deadline guards. | **Fails**; current output is heap behavior |
| Closure success proves an actual dangling read | r8 and r8b accept arena-view captures, while r6 directly returning a view gets E0631. No memory-corruption measurement was performed. | **Holds within limits** as a static-coverage inconsistency, not observed use-after-free |
| One `.copy()` selects the correct destination | A temporary or indirect call may hide or conflict over the destination; copying normally inside the block would allocate there. | **Fails** as an unspecified guarantee; define canonical copy placement through proven uses and reject ambiguity |
| A Pool can leave by copy and all IDs stay valid | An ID carries owner identity; byte-cloning a pool does not establish ownership-preserving identity. | **Fails**; older stable identities may leave, otherwise require a valid copy contract or refuse |
| Shared program storage automatically makes contents safe | A persistent cell can contain arena-backed text or erased payloads. | **Fails**; check the complete contents graph |
| All new resources must be opened outside | A stable independent handle need not depend on arena bytes. Conversely a wrapper can hide such bytes. | **Fails** as a blanket rule; provenance, not creation time, decides |
| Copying caller arguments repairs unknown callees | A callee may allocate fresh data and cache it in older state without retaining an argument. | **Fails**; summaries include fresh retention, and some repairs require dependency changes |
| r15 proves no function-type facts exist | It only rejects fn(&Mobj). | **Fails**; no broad absence claim follows |
| Task heap startup is a new amendment | Newer CONTEXT1 already states it. | **Fails**; corrected decision history |
| B only loses library convenience | Zig's allocator guide explicitly recommends allocator parameters for libraries; polymorphic APIs support composition without twins. | **Fails** as framing; B's explicit dependency benefit stated |
| C is shown under Unsafe | Original code only said so in a trailing comment. | **Fails**; actual #Unsafe block now appears, with manual copy for matched output |
| D has no user context but still displays one | Original D code retained #Context and an unused arena. | **Fails**; removed both, and denied private Core allocator powers |
| E must test and copy at every store | Static provenance can eliminate checks and many stores contain no new arena dependency. No A/E implementation was measured. | **Fails**; implicit cost remains a policy tradeoff, not an every-store speed result |
| Every exit rewinds | Unwinding and cancellation need cleanup; kill/abort cannot promise it. Nested marks and borrowers complicate ordering. | **Holds within stated limits** after specifying once-only unwind and exclusions |

Independent logs are `probes/review7-*.run.txt`. All 18 r subjects were exercised with a0.0.43.
r10 gives E0904; r11b/r14 give E0201; r13/r13b still print 1 under the allocation-denial probes.
None establishes destination-copy behavior, complete allocator denial, performance or tier parity.
Read Odin's context and allocator sections, Zig's allocator guide and bumpalo documentation.
bumpalo explicitly distinguishes arena reclamation from running Drop; wrappers are needed for destructors.
Peer illustrations were not executed.

Recommendation: A is defensible with the repaired semantics, but reviewer prefers existing E for unchanged-library reuse.
No option added. Do not post the owner-approved slogan, universal one-line copy fixes, pool-ID preservation,
new task-policy reversal, heap probes as arena proof, or an every-store overhead comparison.
