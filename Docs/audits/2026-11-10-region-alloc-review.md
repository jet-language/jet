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
   `State: struct { action: fn(&Mobj) }` and gets E0003 "Expected a type, found `&`". Function types carry no
   per-parameter facts today. A now states the build order: until those facts exist, a call through a
   function value or trait method inside the block counts as keeping. That is stricter and never unsound, and
   it is a new, named loss.
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

Census: `#Context(allocator:` appears in 5 Jet files: one example, three tests and one parser message. It is
not in Core or Jetpack. This is unchanged.

## Peers (sourced)

- Odin: the implicit context allocator. <https://odin-lang.org/docs/overview/#implicit-context-system>
- Zig: an allocator argument on every allocating call. <https://ziglang.org/documentation/master/#Choosing-an-Allocator>
- Rust bumpalo: collections carry the arena's lifetime. <https://docs.rs/bumpalo/latest/bumpalo/>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Function values carry escape facts like write marks | Is the precedent built? (r15) | **Failed**: unbuilt. A now treats such calls as keeping until it lands |
| Escape facts recorded in package.api | D-API-SURFACE1 row contents | Held only with one added column, now stated |
| No silent library copy | Store in a dependency | Holds: the same E0631 as a local store |
| Probe results | Rerun on a0.0.43 | Hold |
| Plain language | "you"/"your" | **Failed**. Fixed |

## Remaining losses

- An unmodified caching library is refused inside the block until it adds one `.copy()`.
- One `.copy()` per exit, offered by E0631.
- Until D-FNVAL-WRITE1=A is built, every function-value call inside the block counts as keeping.
