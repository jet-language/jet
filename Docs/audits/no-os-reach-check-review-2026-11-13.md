# Adversarial review: checking Core and packages on a chip with no heap (D-CORE-TARGET1=A, rework)

Date: 2026-11-13. Author: Claude Opus, lane G3Core2, who also wrote the
reworked ballot. This file is the claim-by-claim evidence the rework rests on,
with each claim attacked as the review rule in AGENTS.md asks. It does not
replace the fresh-agent review the rule requires: until a reviewer who did not
write A signs the verdict below, no text may call A better than the designs it
is compared with.

Code basis: dev at a03eea6ac0 for line numbers (lines checked again on
1e3a93d615 where noted). Probes are in
`$JET_SCRATCH_ROOT/lanes/G3Core2/probe/b6144/`. `user_list`, `core_call`,
`deny_alloc_core`, `deny_alloc_user`, `deny_alloc_direct` and `cost` ran on the
batch 3b compiler; `deny_alloc_value`, `deny_alloc_generic` and
`deny_alloc_round` ran on the batch-8 compiler (a03eea6ac0). A is not built, so
every statement about A's behavior is a prediction, and is marked as one.

## What A claims

On a no-OS machine with no allocator, A puts an implied `!Mem.Alloc` denial on
each program root and runs the heap proof Jet already has (the one behind
`-[!Mem.Alloc]>`) from it. It deletes the Core test in
`sema_scope_environment_no_os` and the per-literal E3303 checks, adds growth
operations as heap events, follows a generic call's known type argument to its
bound method, and shows a finding outside the program's package at the
program's own call.

1. No exception for Core: one proof checks Core and every package.
2. No new walk, and no measurable cost.
3. Errors land on the programmer's own line, with the full path.
4. Only reached code is judged.
5. This beats how Rust, Zig, Swift and C report the same problem.

## Measured facts

| # | Read or probe | Observed |
|---|---|---|
| R1 | `Scopes.jet:561-562` | `sema_scope_environment_no_os` returns false for every Core module (`module_is_core`). |
| R2 | `TargetDenials.jet:74`, `:91` | Core functions are skipped, and a Core callee's denied effect is reported at the caller (E1123). |
| R3 | `Effects/Checks.jet:4006` (1e3a93d615) | `sema_effect_memory_search` is a breadth-first walk over `call_edges` that visits each node once. |
| P1 | `core_call`, `--target=board.sensor_v1` | A call to `combo.count_from`, which grows a list, passes. |
| P2 | `deny_alloc_core` | E0921 with the path `main.label -> core.compute.profile_f32_strict`, so the proof enters Core; its caret is past the end of the file. |
| P3 | `deny_alloc_direct` | `out := [Int]{}` then `out.push(n)` passes `-[!Mem.Alloc]>`: growth is not an event. |
| P4 | `deny_alloc_value` | A list built in a function passed by value is found: E0921 at `main.jet:3:5`, path `main.total -> main.grow`. |
| P5 | `deny_alloc_generic` | `measure<T: Sizer>(item)` called with a concrete `Box`: E0921, "cannot be proved through the open dispatch". |
| P6 | `deny_alloc_round` | `math.round(x)` on a `Float` under `-[!Mem.Alloc]>`: E0921 open dispatch at `Math.jet`, caret at `main.jet:11:1` past the end of the file. |
| P7 | `cost` | Checking five core.math calls six times took about 240 ms with and without the denial (236 to 325 ms with, 237 to 404 ms without). |

## Claim 1: no exception for Core

**Holds** [INFERENCE: not built]. The proof has no Core test (R3), and P2 shows
it enters Core. A deletes the only two Core tests on this path (R1, R2).

## Claim 2: no new walk, no measurable cost

**Holds within stated limits.** P7 measured no difference above noise on a
small program. Attack: the cost grows with the reached call graph, and P7 is a
small one. The walk visits each node once (R3), so it is linear in reached
functions; a large-program measurement is a gap for the build card.

## Claim 3: errors on the programmer's line

**Holds** [INFERENCE: not built]. Today the caret is wrong (P2, P6). A places
the finding at the last call inside the program's package, with the path and
the allocating line in the note.

Attack: an author of a package cannot see the error in place. Mitigation:
checking the package itself, with the same machine, reports it in place.

## Claim 4: only reached code is judged

**Holds within stated limits.** The walk follows direct calls and functions
passed by value (P4). It refused too much in one case: bound generic dispatch
on a known type (P5, P6). Without the fix, a heapless program could not call
`math.round`, which never allocates. A therefore follows the instantiation's
bound method. Dispatch with no known target, such as a trait value, stays
refused; the ballot names that as a loss, with the fix "seal the target set".

## Claim 5: better than Rust, Zig, Swift and C

**Holds within stated limits.**

- Rust: a `no_std` crate cannot name `std`; a dependency that uses `std`
  fails inside its own source when it compiles
  (docs.rust-embedded.org/book/intro/no-std.html). Rust's best case: the
  error is in the crate that broke the rule, and `alloc` can be added with a
  global allocator. What Rust does that A does not: the error is per crate,
  before any program uses it. A reports in the package when the package is
  checked for that machine, and at the call otherwise.
- Zig: functions that allocate take an `Allocator` argument, so the need is in
  the signature and visible at every call (ziglang.org/documentation). Zig's
  best case is stronger than inference: no hidden heap use. A's answer is the
  written `-[!Mem.Alloc]>` mark, which makes the promise explicit where the
  author wants it, and the implied denial everywhere else.
- Swift: `@available` refuses a call at the caller's line
  (docs.swift.org, attributes). That is A's placement.
- C: a freestanding build fails at link time with no source line
  (gcc.gnu.org, C dialect options). A is strictly better here.

## Verdict

A holds, within the limits of claims 2 and 4. Gaps for the build card
(#6144): a large-program timing of the implied denial; a test that
`math.round` on `Float` passes on `board.sensor_v1`; a test that a trait value
with unsealed implementations is refused with the seal fix; the list growth
events (push, insert, append, map insertion, text building). A fresh-agent
review of this file and of the ballot is still owed.
