# Mine for Jet: Vale and Valen memory management (2026-10-02)

Normal mining run with Tower logging. The owner asked for Vale's and Valen's
memory mechanics, so that Jet supports and also encourages them where they fit.
The requested end result is a proposal for Jet's memory model:
[memory-model-next-level-2026-10-02](../proposals/memory-model-next-level-2026-10-02.md).

**Revision, same day.** The first version reported four defects from live
probes. The probes ran on `target-rel/release/jet` built 2026-10-01 22:14,
which the owner says is obsolete. Every probe result is withdrawn. Three defect
cards and one owner question were deleted, and correction logs were added to
#3966, #4139, and #3645. Source citations below are reading evidence only and
prove nothing about current behavior.

## Verdict

Jet already has most of Vale's ideas. The most important one is not decided.

- **Jet's core matches Vale's.** The following exist in source or are ratified:
  - single ownership;
  - move-only `#SingleUse` values with an audited discard;
  - generation-checked `Pool` ids;
  - ballots for declared debts and fail-only cleanup;
  - package-level FFI authority;
  - deterministic map order;
  - record and replay.
- **The decision that matters most is unmade.** Vale's central lesson is that a
  value that must be settled cannot sit behind a counted reference, because the
  last release throws it away. Jet ratified counted keep-alive for links
  (D-MEMREF-LIFE1=A) and must-use-once values (D-LIN1) separately. That decision
  is ballot D-LINK-DEBT1.
- **One speedup is missing in source.** Jet proves purity, but no code found by
  search uses it to drop generation checks, alias checks, or counts on data that
  existed before a pure call. That is Vale's main region result.
- **Several edges need checking on a fresh build.** The source suggests
  candidate defects (listed under "Withdrawn probe findings"). None is
  confirmed.

## Sources and capture limits

All sources were `new` in `Docs/spec/reference/prior-art.md`. Retrieval date for
all sources: 2026-10-02. "Valen" in the request is Vale's successor language.

| ID | Source | Kind | Read |
|---|---|---|---|
| UavYVf0UEoc | [Advanced Memory Management in Vale (with Evan Ovadia)](https://www.youtube.com/watch?v=UavYVf0UEoc), Developer Voices, 2024-04-17, 69:31 | video | full transcript, creator subtitles (en-GB) |
| valen | [The Golden Spike, and Resurrecting the Vale(n) Programming Language](https://verdagon.dev/blog/golden-spike-reviving-vale-valen), plus linked [Group Borrowing](https://verdagon.dev/blog/group-borrowing) and [Nick Smith's proposal](https://gist.github.com/nmsmith/cdaa94aa74e8e0611221e65db8e41f7b) | articles | full |
| genrefs | [Generational References](https://verdagon.dev/blog/generational-references) (updated 2023-07-09), [Hybrid-Generational Memory](https://verdagon.dev/blog/hybrid-generational-memory), [References guide](https://vale.dev/guide/references) | articles, docs | full |
| regions | [Zero-Cost Borrowing with Vale Regions](https://verdagon.dev/blog/zero-cost-borrowing-regions-overview), [First Regions Prototype](https://verdagon.dev/blog/first-regions-prototype), [Seamless, Fearless, Structured Concurrency](https://verdagon.dev/blog/seamless-fearless-structured-concurrency) | articles | full |
| linear | [Higher RAII and the Seven Arcane Uses of Linear Types](https://verdagon.dev/blog/higher-raii-uses-linear-types), [Higher RAII in 7DRL](https://verdagon.dev/blog/higher-raii-7drl), [Linear Types and Borrowing](https://verdagon.dev/blog/linear-types-borrowing) | articles | full |
| other | [Memory Safety Grimoire](https://verdagon.dev/grimoire/grimoire), [Perfect Replayability](https://verdagon.dev/blog/perfect-replayability-prototyped) and its linked design, [Fearless FFI](https://verdagon.dev/blog/fearless-ffi) | articles | full |

Limits:

- YouTube returned HTTP 429 for one auto-caption track. The creator subtitle
  track was captured, so the transcript is high-confidence.
- Comments were not captured. Audience evidence was not needed.
- Images in the regions prototype post were not decoded.
- The Mojo "Interior Origins" adoption is Nick Smith's claim and was not checked.
- Vale is archived. Valen's prototype has group borrowing (not for closures),
  zero-sized cross-language structs only, and generational references
  "temporarily disabled".
- **No valid live probes.** A fresh build of the working tree failed in
  `jet-devserver` under `#![deny(warnings)]`. The probes then ran on an obsolete
  binary, and their results are withdrawn.

## Findings

### F1. Links holding a value that owes a job are undecided

Vale: "We can't put a linear type into an Rc because Rc throws away its contents
when the last alias disappears" ([linear-types-borrowing](https://verdagon.dev/blog/linear-types-borrowing),
"Addressing the Drawbacks"). Jet's ratified link fallback (D-MEMREF-LIFE1=A) is
exactly that counted keep-alive, and Jet's ratified `#SingleUse` (D-LIN1) is
exactly such a value. Ballot D-LINK-DEBT1 on #4251 recommends A: the owner
settles, and links check a generation and never keep the value alive.

### F2. Purity is proven but not used to skip memory checks

Inside a pure call, data that existed before the call is frozen, so Vale skips
its generation checks (video 38:20-44:16). Vale's prototype matched
bounds-check-only speed with every generation check removed
([first-regions-prototype](https://verdagon.dev/blog/first-regions-prototype),
"The Benchmarks"; one workload, one laptop). Jet has `fn … -[]>` purity. A search
of `crates/` and `Compiler/` found no code that uses it to drop `Pool`
generation checks, alias checks, or counts. Card #4246 requires a paired
performance cell.

### F3. Replay does not record task order

Vale records each channel message's sequence number and each mutex version so
threaded runs replay exactly (video 56:12-58:27; replay design "Deterministic
Parallelism"). Jet's ratified capture (D-JREPLAY1=A, D-RUN-RECORD1=A) records
Time, or Time/Rand/IO/Net when sensitive, but not task order. Ballot
D-REPLAY-ORDER1 on #4252 recommends recording it always.

### F4. Native foreign code isolation is unmeasured

Vale's Fearless FFI uses copied boundary data, scrambled checked handles, a
separate foreign stack (macOS prototype), and optional WebAssembly or subprocess
sandboxing ([fearless-ffi](https://verdagon.dev/blog/fearless-ffi)). Jet gates
FFI per package and offers WASM sandbox packages. What native extern code can
reach in Jet memory is an evidence gap. Card #4248.

### F5. Nothing teaches users to declare obligations

Evan Ovadia's heuristic is that comments like "remember to", "forget", or
"responsible for" mark a missing linear type (video 19:13-19:44). The owner
asked that Jet encourage these mechanisms. Card #4253, blocked by D-OWES1
(#4139).

## Withdrawn probe findings

These came from the obsolete binary. They are recorded only as candidates to
re-check on a fresh build. No cards exist for them.

| Candidate | Source reading that motivates a re-check |
|---|---|
| A stale `Pool` id may become valid after its slot's generation wraps | `MathTaskMem.rs:3082-3187` stores a `u32` generation and bumps it with `wrapping_add(1)`; Vale retires the slot at the maximum |
| A `^` parameter may discharge a `#SingleUse` duty without settling it | `crates/jet-sema/src/Sema/Registration.rs:736-741`: parameters never carry the consume duty |
| `#SingleUse` duty may be lost in lists and struct fields | D-LIN-CONTAINER1=A is ratified; #3966 is open |
| Two write lends with run-time indexes may be rejected, and E0204's fix may suggest `~` | D-MEMREF-EXCL1=A says to check at run time where proof fails |
| `jet run --record` may reject programs plain `jet run` accepts | none from source; probe only |

## What Jet already has

| Vale or Valen mechanism | Jet | Evidence |
|---|---|---|
| Linear types, audited discard | `#SingleUse`, E0140-E0143, `consume` in `#Unsafe` | `Examples/features/effects/single_use*.jet` |
| Higher RAII (settle with arguments) | any `fn` taking `^T` plus extra arguments | language rule |
| Named settlers and failure action | D-OWES1 option A (open) | #4139 |
| Fail-only cleanup | D-DEFER-FAIL1=A, `scope.on_fail(...)` | #4201 |
| Generational indexes | `Pool<T>` / `Id<T>`, stale id stops | `pool_stale_id.jet` |
| Optional hints that never change meaning | D-AUTOPIN1=A, D-DENY-COST1=A, D-COPY-DEFAULT1=A | #3645, #3741 |
| Deterministic iteration | maps iterate in key order | `loop_forms.jet:39-43` |
| Record and replay | D-JREPLAY1=A, D-RUN-RECORD1=A | spec decisions |
| FFI dependency allow list | package `authority` `FFI` | `Examples/features/lowlevel/ffi.jet:4` |
| Sandboxed dependencies | `target: sandbox` WASM packages | spec "Sandboxed WASM packages" |
| Structured parallel loops | `para_map`, task groups, scoped borrow bands | `parallel_iter.jet` |

## Corrections and disputed claims

- The requested "onPanic" idea is published as `onException` in the source.
- Vale's overhead figures (10.84% for generational references, 25.29% for
  naive counting) come from one benchmark with no machine configuration. They
  are not evidence for Jet.
- "At least 65% of references were temporaries" (video 40:05) names no program
  and has no artifact.
- Hybrid-generational memory was abandoned after 31-32 design attempts
  (grimoire side note 16; golden spike note 21).
- Random 64-bit generations detect misuse only probabilistically
  (generational-references note 23). Jet's safety claim needs a deterministic
  check.
- Valen's generational references are "temporarily disabled". Valen's safety
  rests on group borrowing, a draft with no benchmarks.

## Standing-lens answers

**Beat vectors.**

- **Same mechanism everywhere.** One generation check covers ids and links, and
  one owner settles every debt.
- **Replay at the effect boundary.** Jet's authority roots already name every
  nondeterministic input.
- **Inspectable costs.** `jet inspect choices` and cost denials let users see
  and forbid each inserted check.

All three are designed but not fully built.

**Avoid list.**

| Mistake | Evidence | Jet exposure |
|---|---|---|
| Probabilistic generation checks | generational-references note 23 | none; keep deterministic |
| Tethering bits and queued frees | hybrid-generational memory, abandoned | none |
| Region annotation syntax (`r'`, `'r!`) | regions overview | none; purity is inferred and spelled `-[]>` |
| Counted references holding must-settle values | linear-types-borrowing §9 | open until D-LINK-DEBT1 |
| Generation counters that wrap | Vale retires slots | to be re-checked on a fresh build |

**AI-driven development.**

- *Verdict fidelity* gains if the withdrawn candidates turn out real and are
  fixed.
- *Context economy* holds, because nothing proposed adds annotations beginners
  must write.

**Surfaces.**

- *Covered in source:* `#SingleUse`, `consume`, `Pool.add/remove/[]`,
  `defer close`, `scope.on_fail`, `-[]>`, and the `FFI` authority.
- *Worth checking on a fresh build:* everything in the withdrawn table.
- *Missing:* the `@T` link, `#Owes`, and task-order capture.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | card | #4251 |
| F2 | card | #4246 |
| F3 | card | #4252 |
| F4 | card | #4248 |
| F5 | card | #4253 |
| W1 | no-action | withdrawn: probed on an obsolete binary (owner direction); re-check Pool generation wrap on a fresh build |
| W2 | no-action | withdrawn: probed on an obsolete binary (owner direction); re-check `^` duty discharge on a fresh build |
| W3 | no-action | withdrawn: probed on an obsolete binary (owner direction); container duty is already #3966 |
| W4 | no-action | withdrawn: probed on an obsolete binary (owner direction); re-check indexed write lends on a fresh build |
| W5 | no-action | withdrawn: probed on an obsolete binary (owner direction); re-check `--record` on a fresh build |
<!-- /audit-dispositions -->

Strongest unverified assumption: removing checks inside pure calls (F2) will
measurably speed up real Jet programs. Vale measured it on one workload, and
Jet has no paired cell yet.
