# Slot IR optimization inventory (2026-10-21)

Status: the inventory tables (A-E), the old-backend measurements and the IR passes
(`Compiler/JetIR/Source/Opt*.jet`, run at O1/O2 between the IR build and selection) are complete; "Row map"
lists every row as done, pending (with its plan) or not needed (with the reason). This document remains a
deletion gate of the old middle: the pending rows are the work left before the old optimizer goes.

Deletion-gate item 5 of the single-slot-IR cutover (Tower #5073): every optimization of the old middle and back
end (JetOptimizer MIR passes, JetBackend LIR passes, x86-64 selection and register allocation, the lowering fast
paths) and of the Rust reference (`crates/jet-codegen`, plus the MIR pipeline it calls in
`crates/jet-foundation/src/MIROptimization.rs`), each with what it does, its measured value on the old backend,
and whether it applies to the slot IR (`Compiler/JetIR/Source/IR.jet`) and why. How the Rust tier used LLVM is
out of scope. The passes that apply and pay are ported onto the IR as O1/O2 passes
(`Compiler/JetIR/Source/Opt*.jet`); the rest have a concrete IR pass plan below.

Sources: the dev tree at e859595001 (`Compiler/JetOptimizer/**`, `Compiler/JetBackend/Source/{Opt,X64,Lower}/**`,
`Compiler/JetBackend/Docs/O1.md`, `Compiler/JetDriver/Source/Driver/PureFold.jet`,
`Compiler/JetCodegen/Source/Codegen/{LastUse,DropFlags,Functions}.jet`, `crates/jet-codegen/src/Codegen/**`,
`crates/jet-foundation/src/MIROptimization.rs`), the perf inputs (TextPerf NOTES, CodegenPort's
`codegenport-wip.patch`, IR-needed.md perf rows) and the measurements in "Old backend measurements".

## Old backend measurements

### Runtime per level (native perf cells)

Compiler: install-latest a60f428e0 (identity f87a18a1…); toolchain root = `git archive a60f428e0`. O0 = the
binary `jet run` leaves, O1 = `jet build`, O2 = `jet build --release`. Every output matched its expected file.
Each binary ran 5 times on one pinned CPU of a shared machine (other lanes building): the minimum is the robust
statistic, medians move by up to ~2x between runs, so differences under ~10% are noise. Seconds.

| cell | O0 min | O1 min | O2 min | O1/O0 | O2/O0 |
|---|---|---|---|---|---|
| closures/apply_calls | 2.319 | 2.155 | 2.341 | 0.93 | 1.01 |
| closures/map_filter_collect | 0.260 | 0.395 | 0.446 | 1.52 | 1.72 |
| collections/int_map | 0.656 | 0.680 | 0.674 | 1.04 | 1.03 |
| collections/list_fresh | 1.123 | 1.042 | 1.046 | 0.93 | 0.93 |
| collections/list_reuse | 1.092 | 0.965 | 0.949 | 0.88 | 0.87 |
| collections/temp_collection | 1.361 | 1.287 | 1.314 | 0.95 | 0.97 |
| numerics/collatz | 2.309 | 1.946 | 1.947 | 0.84 | 0.84 |
| numerics/fib | 0.028 | 0.024 | 0.024 | 0.86 | 0.86 |
| numerics/mandelbrot | 0.333 | 0.329 | 0.330 | 0.99 | 0.99 |
| numerics/multi_return | 0.715 | 0.746 | 0.856 | 1.04 | 1.20 |
| numerics/recursion_aggregate | 1.001 | 0.928 | 0.975 | 0.93 | 0.97 |
| numerics/sieve | 1.261 | 1.395 | 1.467 | 1.11 | 1.16 |
| text/byte_scan | 0.177 | 0.152 | 0.149 | 0.86 | 0.84 |
| text/split_join | 1.175 | 1.229 | 1.154 | 1.05 | 0.98 |
| text/string_fresh | 1.467 | 1.460 | 1.451 | 1.00 | 0.99 |

Receipt: `/mnt/jetscratch/lanes/IROpt/measure-old/{results.tsv,phases.tsv,README.md}`. The whole old O1+O2 LIR
pipeline is worth 0-16% on the scalar cells (collatz, fib, byte_scan, list_reuse) and nothing measurable on the
rest; two cells are slower at O1/O2 than at O0 (map_filter_collect, sieve; beyond noise at their minimums). The
old runtime cost sits in runtime calls the passes cannot see (TextPerf hotspots: `jet_string_bytes` copies, heap
range cursors, exact-Int bit operations through the limb runtime, boxed Option/Result), not in what the passes
rewrite. Older receipt: `discovery/perf/olevels/r29g/table.md` (2026-10-17, sieve only): O2/O0 0.98. There is no
per-pass switch in the old pipeline, so per-pass value is not separately measurable; the per-level ratio above
bounds the sum of every pass at that level.

### Compile time of the old middle (self-build of the compiler unit)

`discovery/perf/bc-rel6/s2/phase-diff.txt` (2026-10-21, 83ba0cbb, stage-2 build of the compiler at `jet build`
level): optimize (MIR) 16.0 s wall (107 s CPU), native.lower (MIR->LIR + O1 LIR passes + selection, streamed
per function) 29.5 s (165 s CPU), native.object 39.0 s (249 s CPU), whole build 248 s. Earlier full-scale trace
(`discovery/perf/CONTEXT-PERF.md`, 2026-10-17, O0): optimize 210 s, native.lower 1,169 s of 4,075 s. The native
cells' traces (`measure-old/phases.tsv`) are 20-60 ms per phase per cell: too small to separate passes.

## Inventory

Columns: what it does (limits) · old value · applies to the slot IR? · why / where it lives now. "Ported" rows
are in "Ported passes" below with their proof and measurement. "Selection" rows belong to the IR x86-64 selector
(`Compiler/JetIR/Source/X64*.jet`, lane IRx64b): they are code generation at every level, not O1/O2 passes, as
they were in the old backend (O1.md: "Instruction selection and register allocation improvements apply at every
level").

### A. JetOptimizer (MIR), driver and codegen MIR-level work

| id | pass (file) | what it does | old value | applies? | why |
|---|---|---|---|---|---|
| M0 | legality verification (Verification/Legality.jet:181,274) | schema/CFG/def-use validation before and after passes | verification, no runtime value; optimize.legality 2.1 s of a self-build (bc-rel5) | no (as optimization) | an IR verifier is a separate debug tool (plan P-V); not an optimization |
| M1 | clear batch facts (Pipeline.jet:767) | resets derived fact rows | none | no | the IR has no fact rows |
| M2 | inline-always (Transforms/InlineAlways.jet, InlineMapping.jet) | expands `#Inline(Always)` callees (non-recursive, no captures, no generator) | not measured separately | yes | covered by IR inlining (ported, O1/O2); honoring the attribute regardless of size needs the builder to carry it (plan P-IA) |
| M3 | unreachable-block elimination (ScalarAndCfg.jet:34) | drops blocks unreachable from entry | in the O0-O2 ratios | yes | ported: `unreachable` (code after Br/Return/Unreachable to the end of its region) and constant-branch removal |
| M4 | CFG simplification (ScalarAndCfg.jet:186) | Branch with equal targets or constant Bool -> Jump; phi-join forwarding | in the ratios | yes (branch part) | ported: `fold` (If/BrIf on a constant) and `structure`; phi forwarding does not apply: no phis, joins are structured End |
| M5 | exact constant folding (ScalarAndCfg.jet:420) | folds Int/Bool/Char Unary/Binary/Phi over constants, checked, no Float | in the ratios | yes | ported: `fold` (Int inline range, words with wrap/trap, Bool, Char, comparisons, identities) |
| M6 | bounds facts (Facts/Bounds.jet:9) | one fact per Index, "proven" iff the index kind is FixedListProof (a kind test) | none at run time: not read by native lowering (only Mono copies it; grep Lower/**) | no (as is) | a real bounds-check elimination on the IR is plan P-BCE |
| M7 | dead pure value elimination (Transforms/DeadValues.jet:24) | zero-use pure defs (constants, pure primitive/Prelude ops, Phi, pure Convert) | in the ratios | yes | ported: `dead` |
| M8 | fixed-reduction normalization (Transforms/FixedReduction.jet:42) | Float reductions in counted loops -> adjacent-pair trees for vector packing | none on native: consumed by Rust emit / digest / CLI only | no | there is no vectorizer in the native selection; becomes relevant with SIMD selection (plan P-VEC) |
| M9 | loop and vector facts (Facts/LoopVector.jet:127) | canonical loop shape, constant trip counts, vector eligibility | none on native except `checked_vector_facts` gating `lower_iter_takes` (Lower.jet:6514) | no (as facts) | the IR keeps loops explicit (Loop/End), so the facts are derivable per pass on demand (plan P-VEC, P-RANGE) |
| M10 | canonical loop facts (step 8) | recomputes bounds/loop facts after cleanup | none on native | no | as M6/M9 |
| M11 | acceleration facts (Facts/AccelerationDecisionLedger.jet:20) | PooledParallelChunks eligibility per vector fact | none on native (CLI `inspect`, digest) | no | a reporting consumer, re-homed with `jet inspect` (not an optimization) |
| M12 | canonical program order (Facts/CanonicalOrdering.jet:42) | sorts program tables | determinism only | no | the IR module is built in a deterministic order and content-hashed per function |
| M13 | decision rows (AccelerationDecisionLedger.jet:152) | ledger rows from loop/vector/bounds facts; Reach.jet reads them as mono edges | none on run time | no | the IR builder reaches functions through its own queue |
| M14 | MIR lint (Verification/Lint.jet) | typed/ownership lint, ICE gate; optimize.lint 4.0 s of a self-build | verification | no | IR verifier plan P-V |
| M15 | pure-call folding (Driver/PureFold.jet:89; Rust MIREval/PureFold.rs) | immutable locals initialized by effect-free calls over constants are evaluated at compile time and become constants (slice <= 256 instructions, output <= 256 KiB) | not measured separately | yes | needs the IR evaluator (IREval lane): plan P-PF |
| M16 | move at last use (Codegen/LastUse.jet; Rust tir_to_mir_last_use.rs) | the last consuming read of an owned local becomes a move | part of O0 semantics of the old path | yes | builder-side today (fresh temps move); the general slot form is plan P-LU (copy of a drop type whose source is not read again and is dropped later -> move, drop goes) |
| M17 | static drop-flag resolution (Codegen/DropFlags.jet; Rust tir_to_mir_drop_flags.rs) | constant-propagates drop flags so known drops are unconditional | O0 semantics | yes | the IR builder emits explicit Drop on the paths that own the value; any flag slot it emits is a Bool slot that `fold` resolves when it is a single-write constant |
| M18 | prepared-constant / known-constant inlining (Codegen/PreparedConstants.jet; Functions.jet:922; Rust mir.rs:2818) | checker-prepared constants and small known constant globals become literals | O0 semantics | by construction | the builder reads the checked TIR with these folded |
| M19 | counted-loop vector facts in codegen (Codegen/Statements.jet:802) | vector fact rows for counted loops from TIR facts | none on native | no | as M9 |

### B. JetBackend LIR passes (Opt/*.jet, O1.md P1-P6, W8, W9)

| id | pass | what it does | old value | applies? | why |
|---|---|---|---|---|---|
| P1.a/b | Simplify copy/move propagation, alias roots (Simplify.jet:57-100,367) | Move/Copy results alias their source (not Reown/Bitcast) | in the O1 ratio | yes | ported: `propagate` (single-write Move/Copy of a single-write or parameter source; a Copy of a value with a drop is a clone and stays) and `coalesce` (a temporary read once by a Move/Copy is written straight into its destination) |
| P1.c | binary constant folding (Simplify.jet:117) | Const op Const, no wrap, Div/Rem trap cases refused | in the O1 ratio | yes | ported: `fold` |
| P1.d | Overflows folding (Simplify.jet:168) | folds the LIR overflow test | in the O1 ratio | no | the IR has no Overflows op: exact-Int inline tests are inside selection; the equivalent is plan P-RANGE plus the selector's tested-inline set (selection item S-P3) |
| P1.e | compare folding (Simplify.jet:197) | Const cmp Const, no Float | in the O1 ratio | yes | ported: `fold` |
| P1.f | self-compare (Simplify.jet:381) | x cmp x, non-Float | in the O1 ratio | yes | ported: `fold` |
| P1.g | identities (Simplify.jet:236) | x+0, x-0, x\|0, x^0, x*1 -> x; x*0, x&0 -> 0; x-x, x^x -> 0; x&x, x\|x -> x; x<<0, x>>0, x/1, x&-1 | in the O1 ratio | yes | ported: `fold` (every form; `&-1` on Int and signed words, where -1 is every bit) |
| P1.h | sign test of Or (Simplify.jet:353) | `Or(a, c>=0) <0` -> test a | in the O1 ratio | no | an artifact of LIR inline tests; IR inline tests are selection's |
| P1.i | branch folding (Simplify.jet:448) | known condition -> Jump | in the O1 ratio | yes | ported: `fold` |
| P1.j | unreachable blocks (Simplify.jet:536) | | in the O1 ratio | yes | ported: `unreachable` |
| P1.k/l | empty-block threading, straight-line merge (Simplify.jet:582,699) | CFG cleanup | in the O1 ratio | yes (structured form) | ported: `structure` (a `br 0` that only falls through, empty Block/Loop/If) and Block unwrapping (a Block no Br targets) |
| P1.m | dead code (Simplify.jet:769) | no uses, no effect | in the O1 ratio | yes | ported: `dead` (plus the drop of an exact Int holding an inline constant) |
| P1.n | dead block params (Simplify.jet:918) | | in the O1 ratio | no | no block parameters: slots |
| P1.o | dense renumbering (Simplify.jet:973) | | compile time | yes | ported: compaction (instructions); unreferenced slots keep their row (plan P-SLOT: drop them so the frame shrinks) |
| P2 | strength reduction (Opt/Strength.jet) | word Mul by 2^k -> Shl, UDiv/URem by 2^k -> shift/mask, signed Div/Rem by 2^k -> bias sequence | in the O1 ratio (collatz 0.84) | yes, in selection | the IR ops are checked (trap/stop) and typed by slot; a power-of-two divisor or factor is a selection choice: done in the IR selector (90de4dd41, a constant Temp 2^k divisor: no stop test, shift/mask) |
| P3 | exact-Int inline facts (Opt/Facts.jet) | on the no-overflow edge of an inline test x is inline in the dominated blocks; later tests become Const 0 | in the O1 ratio | yes, split | within selection: S-P3, later (tested-inline set per region); across the function: plan P-RANGE (interval facts over slots, loop counters of range loops are inline, needs an IR flag for "operand known inline") |
| P4 | inlining (Opt/Inline.jet) | bottom-up (Tarjan SCC), callee <= 40 LIR instructions, caller budget 200, never recursive | in the O1 ratio (fib/apply_calls) | yes | ported: `inline` (same rules in IR instruction units, O2 60/400) |
| P5 | known code pointers (Opt/Inline.jet:43) | Call through a FunctionAddress -> direct Call | in the O1 ratio | yes | ported: `direct calls` |
| P6 | CSE / value numbering (Opt/CSE.jet) | dominator-scoped table of pure ops; block-local loads | in the O1 ratio | yes | plan P-CSE (region-scoped value numbering over single-write temps; the built IR re-materializes DataAddress/Const per located op) |
| W8 | LICM (Opt/LICM.jet) | invariant pure ops to a preheader, Div only by const != 0/-1 | in the O2 ratio (O2 ~ O1) | yes | plan P-LICM (structured: hoist before the Loop opener) |
| W9 | scalar replacement (Opt/Scalar.jet) | non-escaping StackSlot with disjoint 8-byte fields -> SSA values (Cytron phis) | in the O2 ratio | yes | ported: `scalar` (O2). No phi placement is needed: a field becomes a mutable slot |

### C. x86-64 code generation (old X64/Select.jet, X64/RegAlloc.jet; O1.md G1-G7)

All are selection techniques at every level; they apply to the IR selector, not as IR passes.

| id | technique | applies? | IR status |
|---|---|---|---|
| G1 | Const operands as imm32, imul/shift immediates, no register for a constant | yes | selection: done in IRx64b batch 1 (Const slots as immediates, skipped inline tests and divisor/count checks) |
| G2 | result takes a dying operand's register; edge/ABI hints | yes | selection: word compares read register homes directly (IRx64b batch 1); hints later |
| G3 | Float values in xmm registers | yes | selection, later (IR Float slots live in frame homes today) |
| G4 | address folding (SIB, LEA) | yes | IR Load/Store carry a constant offset already; base+index*scale for list elements is selection |
| G5 | fused compare/branch, overflow flag reuse, cold layout, inverted jcc | yes | selection (IRx64b batch 1: compare+If/BrIf fusion done; cold paths after the epilogue done) |
| G6 | frame addresses as rbp displacements | yes | selection: done (90de4dd41: Load/Store/CopyBytes whose base is a single-write StackAddress fold the displacement) |
| G7 | linear scan with per-block live-range splitting, slot sharing | yes | IR allocator: whole-function callee-saved homes + frame homes (X64Alloc.jet); splitting per region is selection, later |
| B.a | inline exact-Int drop/clone test | yes | done in the IR selector (ir_x64_int_route) |
| B.b-f | call forms, SysV plan, Div/shift register constraints, unsigned overflow flags, NaN-correct float compares | yes | done in the IR selector |
| B.h | DWARF lines and variable ranges | yes | done (ir_x64_variables) |
| B.i | LIR lint gate before selection | no | LIR-specific; IR verifier plan P-V |

### D. Lowering fast paths (old Lower/**)

| id | fast path | applies? | IR status |
|---|---|---|---|
| C.1 | exact Int constants in the inline range are plain constants | yes | by construction (builder: `lower_int_inline`) |
| C.2/C.3 | exact Int + - * inline with cold big-integer route | yes | done in selection |
| C.4 | cold blocks after hot code | yes | done in selection (slow paths after the epilogue) |
| C.5/C.6 | literal and inline-fast-path conversions | yes | builder/selection |
| C.7 | inline list indexing with a cold bounds stop | yes | builder (plan P-BCE removes the stop in counted loops) |
| C.8 | range-loop cursor | yes | by construction: the IR builder emits an inline counter slot (`ir_range_loop`): no cursor init/has_next/value/advance runtime calls (TextPerf R3, CodegenPort `lower_range_loop_*`); computed/negative steps: IRBuild queued |
| (wip) | exact-Int `& \| ^ << >>` inline (CodegenPort `lower_exact_bitwise`/`lower_exact_shift`, TextPerf R6: sha256 53% in jet_int_shl/shr) | yes | re-expressed on the IR: the builder emits BitAnd/BitOr/BitXor/Shl/Shr on Int slots (IRBuild) and the selector inlines them for inline operands (IRx64b), cold limb route otherwise |
| C.9-C.11 | chars/bytes loops, lazy cursors, slice windows | yes | builder |
| C.12 | loop back-edge copy elision | no | slots are mutable in place; nothing is copied on a back edge |
| C.13/C.14 | inline atomic receiver, inline foreign-result drop | yes | builder |

### E. Rust reference (crates/jet-codegen; MIR pipeline in jet-foundation)

| id | optimization | applies? | why |
|---|---|---|---|
| A1/A2 | TIR loop-form canonicalization: `while true` -> loop, `while false` removed, invariant Bool condition hoisted | yes | `while true`: the builder's Loop; `while false`: `fold` removes an If/BrIf on a constant; invariant condition hoisting is plan P-LICM |
| A3/A4 | Range -> CountedLoop for literal bounds | yes | by construction (C.8) for every Int range, literal or not |
| A5 | TIR expression constant folding | yes | ported: `fold` over the IR (covers literals and single-write constants) |
| A7 | anonymous union declaration merge | no | a declaration-table rewrite before codegen, not an IR transformation |
| A8 | known constant reads inlined | by construction | as M18 |
| B1-B6 | pure-call folding with the MIR evaluator | yes | plan P-PF (IR evaluator) |
| C1-C13 | MIROptimization.rs pipeline | as M0-M14 | the Jet JetOptimizer is its port; same rows |
| D1 | move_last_uses | yes | as M16 (plan P-LU) |
| D2 | resolve_drop_flags | yes | as M17 |
| E1/E2 | typed-fact enum equality/pattern folding | by construction | the checked TIR the builder reads carries the folded tests |
| F | NativeLoopCursor.rs, NativeNumericBridge.rs, MIREval.rs | no | runtime bridges and the interpreter, not optimizations |

## Ported passes

Entry `ir_optimize_module(m: &IRModule, level: Int) -> IROptReport` (Opt.jet), IR in, IR out, between
the IR build and `ir_x64_compile_module`; the level is the command's (`jet_cli_native_level`): 0 (`jet run`,
dev, comptime) leaves the module as built, 1 = `jet build`, 2 = `--release`. Non-SSA: "a slot's one write
reaches a read" is decided by region nesting (OptFacts), no SSA view. The "ported" entries in the tables above
refer to these passes.

| pass | file | level | old rows covered |
|---|---|---|---|
| direct calls (CallIndirect of a single-write FunctionAddress -> Call) | Opt.jet | 1, 2 | P5 |
| inlining, bottom-up over Tarjan SCCs; callee <= 40 IR instructions, caller budget 200 (O2: 60/400); never recursive, never a callee that writes a parameter, only where the result and argument slots have the callee's type rows; a callee whose arithmetic can stop only into its own source file (selection reports stops at the function's file) | OptInline.jet | 1, 2 | P4, M2 |
| simplify to a fixed point (<= 8 rounds): fold, unreachable, compare+not fusion (never Float), propagate, coalesce, dead, structure, Block unwrap | OptSimplify.jet | 1, 2 | M3-M5, M7, M17, P1.a-c, e-g, i-m, o, A5 |
| scalar replacement of non-escaping single-write StackAddress storage (disjoint same-typed <= 8-byte fields) | OptScalar.jet | 2 | W9 |

Contracts with selection (confirmed by IRx64b 2026-10-21): selection reads every operand, cold paths included,
before writing dst (so dst may equal an operand); the selector's self-located stops are exactly the set in
`ir_opt_located` (Int Div/Rem/FloorDiv/Mod/Shl/Shr; Word Div family; Word Add/Sub/Mul/Neg/Shl/Shr unless WRAP).

Proof (2026-10-21, dev c2c7ef82 plus the passes; seed `jet check` of Compiler/JetIR, its Tests and its Spike
clean, also on 6944afbd6):

- `Compiler/JetIR/Tests/X64Programs.jet` runs every hand-built program at O0, then through
  `ir_optimize_module` at O1 and O2. Each run must be identical to the old backend's build: stdout, stderr and
  exit, including the two stopping programs `div_zero` and `word_trap`. Result: 12 programs, 24/24 level rows
  IDENTICAL, "X64 PROGRAMS PASS". Instructions O0 -> O1: 5->5, 57->69, 174->276, 57->50, 84->90, 52->53,
  25->24, 17->17, 110->99, 17->15, 47->123, 54->61. O2 matched O1 on every program; scalar replacement found
  no candidate in them. Object bytes summed over the 12: 68504 at O0, 74192 at O1 (+8%). Inlined callees stay
  in the object as functions.
- Spike differential: `jet ir run` at O0, `jet ir build` at O1, `jet ir build --release` at O2, each against
  the old backend's cached run (byte-identical stdout, stderr and exit). The programs were the 280 the IR route
  builds identically at O0 (IRIntegrate3 int2). The first run was cut off at its 3600 s limit. The rerun on
  dev 6944afbd6 plus the pass patch (landed as 55aaeee3b) finished in 2072 s: 280/280 identical and golden at
  each of O0, O1 and O2. `ir.opt` is under the trace's 10 ms resolution per program. Object bytes summed over
  the 280: 3374656 (O0), 3524872 (O1, +4.5%), 3697936 (O2, +9.6%).
- Native perf cells at IR O0/O1/O2 (minimum of 5 runs, seconds; one build of the patched compiler, so the
  ratios are indicative): output matched the expected file on every built cell at every level. O1/O0 and
  O2/O0: closures/apply_calls 1.05/0.88, closures/map_filter_collect 0.94/1.67, collections/int_map
  1.20/1.21, collections/list_fresh 0.82/0.86, collections/list_reuse 0.79/0.79, collections/temp_collection
  0.99/0.98, numerics/collatz 1.01/1.04, numerics/fib 0.97/0.97, numerics/mandelbrot 0.79/0.84,
  numerics/multi_return 0.96/0.91, numerics/recursion_aggregate 1.03/1.04, text/byte_scan 0.89/0.89,
  text/split_join 1.00/1.01, text/string_fresh 0.98/0.98. Optimized code slower than O0 on int_map (O1, O2)
  and map_filter_collect (O2) is an open finding. numerics/sieve does not build at any level: the IR builder
  does not yet lower a range loop with a computed step.

## Row map

Every row of the tables above, by state.

- Done, IR pass (proof above): P5 (direct calls); P4, M2 (inline; the attribute itself is P-IA); M3, M4, M5,
  M7, M17 and D2, P1.a/b, P1.c, P1.e, P1.f, P1.g, P1.i, P1.j, P1.k/l, P1.m, P1.o (instructions; slot rows are
  P-SLOT), A5, A1/A2 (`while false`; the invariant-condition hoist is P-LICM) (simplify); W9 (scalar, O2).
- Done, selection (every level; X64Programs and the differential prove the selector): G1, G5, G6, P2, B.a,
  B.b-f, B.h, C.2/C.3, C.4.
- Done by construction (the builder emits the fast form): M18, A8, A3/A4, C.1, C.5/C.6, C.8, C.9-C.11, C.13/C.14,
  E1/E2, the exact-Int bitwise (wip) row.
- Pending, with their plan below: P1.d and P3 (P-RANGE), P6 (P-CSE), W8 and the A2 hoist (P-LICM), M16 and D1
  (P-LU), M15 and B1-B6 (P-PF, needs the IR evaluator), M6 and C.7 (P-BCE), the `#Inline(Always)` attribute of M2
  (P-IA), unreferenced slot rows of P1.o (P-SLOT), box elimination (P-BOX); selection: G2 (edge/ABI hints), G3
  (Float in xmm), G4 (base+index*scale), G7 (live-range splitting per region).
- Not needed: M0, M14, B.i (verification, not optimization: P-V); M1, M10, M11, M12, M13 (fact and ledger rows,
  the IR has none: no run-time value); M8, M9, M19 (no native vectorizer: P-VEC once SIMD selection exists);
  P1.h, P1.n (LIR artifacts: inline tests and block parameters); C.12 (slots are mutated in place); A7 (a
  declaration-table rewrite); F (runtime bridges and the interpreter); C1-C13 (the same rows as M0-M14).

## Plans for the rest

- P-RANGE: interval facts per slot (constants, range-loop counters with constant or invariant bounds and step,
  compares on the taken edge); needs an IR contract flag on exact-Int ops "operands/result known inline" (bits in
  `c`) so selection skips inline tests and spills; removes Drops of known-inline Ints.
- P-CSE: region-scoped value numbering of pure ops over single-write temps (the built IR repeats DataAddress and
  Const per located operation and per interpolation).
- P-LICM: invariant pure single-write instructions inside a Loop whose operands are written outside it move before
  the Loop opener; no trapping ops (Div only by a constant != 0, != -1).
- P-LU: copy-to-move at last use: a Copy of a drop type whose source is not read again before its Drop becomes a
  Move and that Drop goes (liveness over structured control, loops by fixed point).
- P-PF: pure-call folding with the IR evaluator (IREval): a Call of an effect-free function on constant slots is
  evaluated at compile time, budgets as PureFold (slice 256 instructions, output 256 KiB).
- P-BCE: list-index bounds stops inside range loops whose counter is proven below the list length.
- P-BOX: Option/Result box elimination (TextPerf R5): a `jet_rt_alloc` box that never escapes (only Load/Store at
  constant offsets and its own drop) becomes slots; needs the builder to mark the box drop as shallow.
- P-IA: honor `#Inline(Always)` regardless of size (the builder carries the attribute on IRFunction).
- P-SLOT: drop unreferenced slots after compaction so their frame homes go.
- P-ID: done (`fold` covers every identity of P1.g).
- P-VEC: loop/vector facts and fixed-reduction normalization once SIMD selection exists.
- P-V: an IR verifier (structure, slot ranges) for debug builds, replacing MIR legality/lint and LIR lint as the
  optimizer-bug gate.
