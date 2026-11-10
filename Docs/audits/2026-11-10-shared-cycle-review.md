# D-SHARED-CYCLE2 review: repair the write-time cycle check and build the opt-in collector

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-SHARED-CYCLE2. Earlier pass: BallotFix REPORT.md, section D-SHARED-CYCLE2.

## Question

Should Jet keep D-SHARED-CYCLE1=C and D-GC-OPTIN1=A by repairing today's E0221 write check? The repair
covers reach sets, function facts and unknown values. Or should it replace the check with a type-level rule
(B), or with one of C, D or E?

## Findings that changed the ballot

1. **Part 3 leans on an unbuilt precedent.** A function value's reach facts "travel through returns,
   parameters, lists, trait objects and package interfaces". For values stored in lists and fields, that
   needs preserved callable summaries. New c04 writes `fn(&Mobj)` and gets E0003.
   This establishes one grammar gap, not absence of every callable fact. A now requires complete summaries
   before shipping; unknown callback parameters cannot be called exact merely because they are parameters.
2. **Cause cited to the line.** The w7/s20 leak comes from `SharedCycles.jet:159`, where `.Lambda(_, _) -> return true`
   treats every lambda as fresh.
3. **Counts are not proof.** No type census is used to promise unchanged program shapes or complete migration.

## Probes (a0.0.43; outputs in probes/*/out43)

| Probe | Result | Board |
|---|---|---|
| w1, w2, w3, w5 safe weak-link shapes | E0221 | same |
| w4 frozen DAG | `7` | |
| w6 weak observers | `saw prices` | |
| w7 strong observer | `1` (accepted leak) | same |
| s19 closure parameter | `2` | |
| s20 hidden leak | `end`, no close | same |
| s21 / s22 | `end`, `closed 7` | same |
| a1, a2, a3, a5, b2 Pool/ID and inline | run (`notes is in docs`, `root has 1 kid`, …) | same |
| d1, d2, d5 strong loops | E0221 | same |
| c01 `#Policy(gc)` tree | E0221 (GC-OPTIN1 unbuilt) | same |
| c02 `-[!Mem.Gc]>` | E0750 | same |
| c03 both | E0221, E0750 | same |
| c04 `fn(&Mobj)` field (new) | E0003 | FNVAL-WRITE1 unbuilt |

## Peers (sourced)

- PEP 442: finalizers in a cycle run before it is freed, and resurrection keeps it.
  <https://peps.python.org/pep-0442/>
- CPython uses trial-deletion-style root accounting; its current free-threaded collector also pauses threads and scans the heap.
  <https://raw.githubusercontent.com/python/cpython/main/InternalDocs/garbage_collector.md>
  (also <https://devguide.python.org/internals/garbage_collector/>)
- Swift ARC: strong cycles leak unless weak or unowned.
  <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/>
- Rust `Weak` breaks `Rc` cycles, and the compiler does not refuse a loop. <https://doc.rust-lang.org/std/rc/struct.Weak.html>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Function facts travel through lists and fields | Is that established by c04? | **Fails**; one grammar rejection is not a summary-system census |
| Lambda treated as fresh contributes to the leak | Read origin/dev SharedCycles and run retention probe | **Holds**; exact byte count was not measured |
| All decoded cache values are fresh | Custom decoders can intern values or invoke callbacks | **Fails**; require actual reach/mutation summaries |
| Every loop is freed because some write closes it | Aliases, opaque captures, root accounting, scheduling and cleanup are unproved | **Fails** as a completed proof; named acceptance obligations replace it |
| Probe results | Rerun on a0.0.43 | Hold (22 of 22) |

## Remaining losses

- Conservative alias and unknown-call summaries may still refuse safe programs.
- Every cyclic Shared participant must opt in; unopted cyclic members cannot be silently collected.
- Consuming cleanup cannot traverse or resurrect dying component members; some resources need explicit settlement.
- Cross-task cycles remain subject to the ratified thread-safe collector proof.

## Different-family review (GPT, 2026-11-10)

Reviewer: BallotReview7. Read CYCLE1=C, GC-OPTIN1=A, MEMREF-TASK1=A,
MEMREF-LIFE1=A and DENY-COST1=A. The latter requires absence of reachable collector work, not only a store rule.

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| Current hidden callback loop leaks | Reran s20/s21/s22. New review7-retained returns only a weak handle; its upgrade succeeds after the builder's external handle ends, and Close never prints. | **Holds** as retained payload, not measured bytes |
| Current helper false refusals exist | Reran w1/w2/w3/w5 and inline/Pool alternatives. | **Holds** for these matched shapes |
| Two parameter/result facts prove every mutation safe | Heap aliases can acquire new edges after a summary snapshot; swaps, erased payloads and callbacks need mutation facts. | **Fails**; require heap-edge tracking and conservative fixed points |
| Local lambdas and callback parameters always have exact facts | A callback parameter can come from stored or opaque code. | **Fails**; only established summaries may be precise |
| Decoders only create fresh cells | Custom decode implementations can intern existing state or invoke hooks. No probe or source proof supported the blanket claim. | **Fails**; removed the exemption |
| Opting one cell in authorizes collecting unopted cyclic members | GC-OPTIN1 reserves collection to opted types. | **Fails**; all cyclic Shared participants must opt in; acyclic owned leaves may still close by ordinary ownership |
| Allocation threshold plus task exit proves eventual freeing | A long-lived idle task may drop its last root without allocating again. | **Fails**; candidate scheduling must survive root loss and run at quiescent points |
| PEP 442 establishes deterministic Close order and safe resurrection | PEP 442 says finalizer order is undefined. Python finalization is not Jet's consuming Close; revival after consumption can expose invalid resources. | **Fails**; one consuming cleanup mechanism, no traversal/resurrection of dying members, explicit settlement where needed |
| Checking opted writes as unopted proves !Mem.Gc | A drop, allocator slow path or indirect call can still initiate collector work. | **Fails**; all reachable work must obey denial |
| Per-task collection covers every graph | MEMREF-LIFE1 requires cross-task cyclic graphs to remain rejected until a thread-safe collector is proven. | **Fails** globally; restriction applies to full reachable components |
| B necessarily misses callbacks and moves every tree to Pool | Conservative capture bounds can make B complete; owned recursive values and opted graph types are other representations. | **Fails** as framing; B strengthened |
| Keeping today's leak is a viable C | I1 and CYCLE1 already forbid that safe-code leak. | **Fails**; C now means the conservative policy with mandatory safety repairs, a smaller precision change |
| D puts a collector under every program | Acyclic-only programs need no cyclic collector. | **Fails**; narrow D to potentially cyclic graphs and name the opt-in reversal |
| E only reverses CYCLE1 | Safe leak permission also changes I1's leak-freedom invariant. | **Fails**; an explicit invariant exception is required |
| Diagnostics can always show the exact loop | Abstract reachability may only show a possible edge. | **Fails**; show uncertainty, not invented exact paths |

All 22 original subjects were rerun with a0.0.43; logs are `probes/*/review7-*.run.txt`.
The added retention probe and its output are in `probes/cycle/review7-retained*`.
Read origin/dev `Compiler/JetSema/Source/Sema/SharedCycles.jet`.
Read PEP 442, CPython's current GC design, Swift's ARC source documentation and Rust Weak documentation.
Peer illustrations were not executed; no collector timing, byte-retention or tier-parity result is claimed.

Recommendation: agree conditionally with A only after these proof and lifecycle requirements.
B is a fair complete conservative alternative; C is repaired rather than offered as permission to keep an invariant violation.
No option added. Do not post the free-or-leak proof as complete, decoder freshness, transitive unopted collection,
Python-style consuming resurrection, weak links as a universal prompt-close fix, or universal program-shape acceptance.
