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
   needs facts in function types. New probe c04 writes `State: struct { action: fn(&Mobj) }`, the form of
   D-FNVAL-WRITE1=A (ratified 2026-11-05), and gets E0003. Function types carry no per-parameter facts yet.
   This is the same build dependency found for D-REGION-ALLOC1 (its r15), and Main logged it on #5036.
   A now states a sound interim rule: a function value read back out of storage counts as reaching every
   cell. Local lambdas and callback parameters keep exact facts, so w6 (`saw prices`) and the s22 listener
   still build. This is a new, named loss.
2. **Cause cited to the line.** The w7/s20 leak comes from `SharedCycles.jet:159`, where `.Lambda(_, _) -> return true`
   treats every lambda as fresh.
3. **Census wording.** The census paragraph named an agent's file ("G6Lead's cycle-census.tsv"). It now says
   "a type census" and stays labelled as the seed's. On dev, 5 tests/ui files still expect E0221:
   `shared_strong_cycle{,_guard,_push,_self}.stderr` and `shared_strong_cycle_cross_module/stderr`.

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
- CPython GC: trial deletion with `gc_ref`, no stack scan.
  <https://raw.githubusercontent.com/python/cpython/main/InternalDocs/garbage_collector.md>
  (also <https://devguide.python.org/internals/garbage_collector/>)
- Swift ARC: strong cycles leak unless weak or unowned.
  <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/automaticreferencecounting/>
- Rust `Weak` breaks `Rc` cycles, and the compiler does not refuse a loop. <https://doc.rust-lang.org/std/rc/struct.Weak.html>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Function facts travel through lists and fields | Are function-type facts built? (c04) | **Failed**: unbuilt. Sound interim rule added |
| Lambda treated as fresh causes the leak | Code | Holds: SharedCycles.jet:159 |
| Decoded caches are accepted | Part 4 unknowns | Holds: a decoder's result reaches only cells it creates |
| Free-or-leak proof | Every loop closed by a write | Holds given parts 1-3, including the interim rule |
| Probe results | Rerun on a0.0.43 | Hold (22 of 22) |

## Remaining losses

- Reparenting is refused. The fixes are a weak link, an ID or the opt-in.
- A close in a collected loop runs at collection. A weak link gives a prompt close (s22).
- Until D-FNVAL-WRITE1=A is built, function values read back out of storage count as reaching every cell.
