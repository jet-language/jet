# Self-hosted optimizer: SSA backend fed by sema facts

Status: design for card #2059. This document changes no code and does not
ratify syntax. It records the eventual optimizer seam and the smaller
Cranelift steps that can land before self-hosting.

## Decision in one page

Jet should derive a private, typed SSA implementation from the existing
semantic TIR. The derived backend IR must consume a sema-owned fact channel
for types, ranges, effects, authority, access, views, freeze, constants,
contracts, layout, and escape state. It must not become a second semantic
source.

The shape is:

    checked AST -> typed TIR + sema facts -> private SSA -> target lowering

TIR remains the frozen executable contract. AOT and JIT remain its two
feature-identical executable lenses. The interpreter remains the reference
execution adapter. SSA is an implementation inside an optimized lens, not a
third lens and not a replacement for TIR.

The canonical optimized pass order is:

    inline -> SROA -> GVN -> LICM -> vectorize -> lower

CFG construction, memory SSA, constant propagation, bounds analysis, escape
analysis, and dead-code cleanup support that sequence. They do not invent
semantic facts. If a proof is absent, the optimizer keeps the checked Prelude
operation or the conservative memory dependency.

### Gates and non-goals

- New syntax: none. The design uses existing #Scalar, #Layout(c), and
  #Layout(columnar) decisions.
- New external dependency: none.
- I9 exception: none. An inline sequence is allowed only as a proven
  lowering of a Prelude operation. It cannot move policy, validation, default
  behavior, or error meaning into an engine.
- Owner-gated contract: the frozen-TIR amendment described below must be
  ratified before sema exports a general optimization-fact channel.
- This card does not implement the channel, the SSA module, or a pass.

## Current compiler boundary

Jet's pipeline is source, lexer, parser, checked AST, TIR, Rust emission, and
rustc for AOT. The frontend owns semantics and diagnostics; rustc is hidden
from the user (docs/spec/architecture.md:7-25). TIR is already the only
code-generation seam. It carries sema-approved decisions and is consumed
exhaustively by the engines (docs/spec/architecture.md:27-42;
docs/spec/tir.md:1-11).

The important law is stronger than "all engines can parse the same syntax."
One semantic core owns runtime meaning. Prelude and CoreLib own that meaning;
AOT emission, Cranelift, and interpreter ambient are marshalling adapters that
call the same operations. They must not re-encode policy, defaults, checks, or
error behavior (docs/spec/architecture.md:710-736). The optimizer therefore
optimizes a semantic operation such as exact integer addition or a checked
index. It does not replace that operation with a host-language policy.

This design follows the project fact law: facts move toward safety and useful
optimization; when the compiler no longer has a fact, the user must opt into
the relevant behavior (docs/spec/philosophy.md:60-77). The backend must not
reconstruct a weaker approximation when sema already proved the stronger fact.

## Sema fact inventory

The table separates facts that already exist from the places where the current
TIR records only part of them. "Missing" means missing from the optimizer
seam, not necessarily missing from sema.

| Fact | Existing proof and storage | Current TIR carrier | SSA use and current gap |
| --- | --- | --- | --- |
| Static type and numeric representation | Every expression has a sema-resolved type. KnowledgeFact and KnowledgeVector also record type knowledge such as interval, exactness, dimensions, classification, and obligations (crates/jet-foundation/src/AST/types.rs:621-723). | TExpr.ty, typed locals, typed parameters, return type, and resolved method identities (crates/jet-codegen/src/Codegen/TIR/mod.rs:2554-2638, 3685-3690, 5981-6020). | Select machine representations and typed operations without runtime type rediscovery. The type is present, but no stable SSA ValueId or typed value-number key exists. |
| Integer intervals and bounds | distinct_range returns the proven interval for a distinct range type. integer_interval projects that interval; plain Int is explicitly not a proof and needs a runtime index check (crates/jet-sema/src/Sema/mod.rs:556-580). Sema also records exact-Int reachability (crates/jet-sema/src/Sema/mod.rs:1449-1467). | IndexKind::FixedListProof, TNumericOp::InlineRange, fixed-list forms, and uninit_fixed preserve selected proofs (crates/jet-foundation/src/AST/lvalues.rs:41-67; crates/jet-codegen/src/Codegen/TIR/mod.rs:4090-4100, 4632-4686). | Elide a check only when the interval covers the legal index range. The current seam does not carry a general per-index interval, induction relation, or proof origin, so generic loop bounds are rediscovered or left checked. |
| Access convention and ownership | AccessConvention distinguishes Read, exclusive Write, and Move (crates/jet-foundation/src/AST/types.rs:904-914). D-MEM1 defines unmarked reads, &T exclusive writes, and ^T consume/move; raw stored or returned references are not the safe surface (docs/spec/syntax-decisions.md:2115-2236). | TIR parameters, TCallArg borrow and mutable-borrow fields, Borrow, Clone, ExplicitCopy, Drop, and structured TPlace preserve selected decisions (crates/jet-codegen/src/Codegen/TIR/mod.rs:837-845, 4150-4170, 5981-6020). | Form ownership-aware memory SSA and remove copies or retains. Current lowering sees wrappers one call at a time; it has no unified live access window for the whole function. |
| View provenance and exclusivity | FlowFacts has a View plane. ViewPlace records owner and field, index, range, or fresh projections; overlap is disjoint only where sema can prove it, and dynamic projections remain conservative (crates/jet-sema/src/Sema/FlowFacts.rs:1-25, 550-605; crates/jet-sema/src/Sema/mod.rs:995-1165). SplitViews is emitted only after a disjoint partition is proved (crates/jet-codegen/src/Codegen/TIR/mod.rs:3162-3182). | SplitViews, Borrow, MaterializeView, return-view provenance, and closure capture provenance (crates/jet-codegen/src/Codegen/TIR/mod.rs:188-214, 4159-4170, 4805-4844). | Attach a scoped no-alias token to a read/write window. Do not infer global no-alias from a single view. The current TIR has no general fact at each load, store, or call site. |
| Effects and authority | EffectSummary stores direct effects, spans, call edges, solved summaries, regions, authority delegations, callbacks, autodiff and compute obligations, and memory facts (crates/jet-sema/src/Sema/Effects.rs:519-577, 722-743). Checker context accumulates effect and authority facts; memory evidence shares the same pre-TIR call graph (crates/jet-sema/src/Sema/mod.rs:1641-1697). | TFunc.is_pure, declared effects, unsafe gate, reactive state, contracts, region and transaction nodes, plus resolved function and method identities (crates/jet-codegen/src/Codegen/TIR/mod.rs:2554-2638, 3110-3125, 3485-3576). | Treat pure calls as candidates for GVN and LICM only when memory and allocation conditions also hold. The current TIR lacks a uniform call-site effect row and authority token; a function-level is_pure bit is too coarse for memory motion. |
| Freeze and deep immutability | The Frozen flow plane stores the source span of the proof and keeps the fact only when all joined paths retain it (crates/jet-sema/src/Sema/FlowFacts.rs:477-489). D-CONC-FREEZE1 makes freeze(x) deeply immutable and names the freeze site in a mutation diagnostic (docs/spec/syntax-decisions.md:7552-7560; crates/jet-sema/src/Sema/CheckerOwnership.rs:2257-2265, 2330-2336). | Spawn captures carry frozen_at_spawn and materialization decisions; explicit MaterializeView and freeze-related capture data survive lowering (crates/jet-codegen/src/Codegen/TIR/mod.rs:188-214, 4805-4844). | Mark reachable data immutable for load CSE and safe sharing. Freeze is not a stack-allocation proof: a frozen graph can escape to a task or heap. The current TIR does not carry an immutable bit on every reachable value. |
| Comptime-known values and constants | Comptime bindings are evaluated by sema and retain CtValue; immutable locals can retain a constant value (crates/jet-sema/src/Sema/CheckerCore/bindings.rs:983-1021, 1197-1223; crates/jet-sema/src/Sema/CheckerCore/scopes.rs:125-146). CtValue covers scalar, exact integer, bytes, collections, aggregates, enums, closures, and failure (crates/jet-foundation/src/AST/comptime.rs:694-732). | Constants have is_comptime, ct, and type; ComptimeName is folded before codegen; TIR also has CtLit and integer constants (crates/jet-foundation/src/AST/patterns.rs:408-431; crates/jet-foundation/src/AST/expressions.rs:834-846; crates/jet-codegen/src/Codegen/TIR/mod.rs:3752-3766). | Seed SCCP, constant folding, specialization, and loop trip-count facts. The values are scattered across constants, literals, and function-level tables rather than exposed as one source-linked fact channel. |
| Escape, promotion, and capture state | Sema completes automatic GC-promotion decisions after ownership and type checking; codegen is not meant to rediscover them (crates/jet-sema/src/Sema/MemoryFacts.rs:141-203). LambdaMeta records escaping, mutable/moved/cloned captures, frozen captures, materialization, and return provenance (crates/jet-foundation/src/AST/expressions.rs:364-416). | Lambda records capture and boxing decisions; TFunc records GC return/scope and return-view provenance (crates/jet-codegen/src/Codegen/TIR/mod.rs:2554-2638, 4805-4844). | Promote an owned, non-escaping aggregate to the stack and scalar-replace it. The current seam has capture and GC decisions, but not a complete per-allocation escape lattice. |
| Contracts and checked outcomes | Sema checks contracts and preserves their selected disposition. TIR has explicit contract and contract-scope statements (crates/jet-codegen/src/Codegen/TIR/mod.rs:3110-3125). | Contract nodes, Try conversions, and explicit Prelude/Core calls retain the observable check and outcome path (crates/jet-codegen/src/Codegen/TIR/mod.rs:4290-4307, 4227-4254). | Remove a check only with a sema-backed proof that its failure edge is unreachable. Otherwise preserve the check, source location, and outcome. The optimizer must never turn an outcome into a host panic or a different error. |
| Layout and storage representation | Struct layout is explicit for C and columnar forms (crates/jet-foundation/src/AST/items.rs:1584-1595). D-SOA1 defines #Layout(columnar) as struct-of-arrays with the same logical Vec API; C layout is the ABI boundary (docs/spec/syntax-decisions.md:2897-2902). | TIR Layout statements and the ForIn.columnar decision preserve selected layout behavior (crates/jet-codegen/src/Codegen/TIR/mod.rs:3401-3434, 3496-3510). | Permit field reorder and AoS-to-SoA only when physical layout is not observed. #Layout(c) is never transformed. The broad default physical-layout-unspecified rule is part of this design contract and must be recorded in the owning layout decision if it is not already recorded there. |

### What sema already has, and what it does not yet export

Sema has enough information to make the optimizer useful today, but the
information is split among FlowFacts, effect summaries, AST metadata, and
selected TIR nodes. A fact is not useful to an SSA pass merely because it was
once proven. The proof must cross the TIR seam with:

- a stable source identity for diagnostics and profile attribution;
- a scope or program point;
- the subject: value, place, call, loop, allocation, or layout;
- the proposition and its validity window;
- the proof's conservative fallback.

The missing artifact is therefore not another analysis that guesses at
ownership. It is a read-only, source-linked fact channel produced by sema.

## TIR today and the SSA conversion gap

### What TIR provides

TIR is typed and total at the semantic level. Every TExpr has a resolved type
and every TStmt variant represents a sema decision; a coverage miss is an
internal compiler error, not a fallback (crates/jet-codegen/src/Codegen/TIR/mod.rs:1-25).
Calls carry resolved identities, argument ownership decisions, type arguments,
clone and widening decisions, and trait boxing decisions
(crates/jet-codegen/src/Codegen/TIR/mod.rs:4171-4193, 5981-6020).

Its core is a structured tree. If, branches, loops, ranges, matches, and
ForIn bodies are nested in TStmt; early exits and cleanup are represented as
statements and expressions (crates/jet-codegen/src/Codegen/TIR/mod.rs:3273-3351,
3401-3434, 3549-3576). Places remain structured as local, field, index, or
pool targets rather than pre-rendered Rust
(crates/jet-codegen/src/Codegen/TIR/mod.rs:837-845).

That is a good SSA input. Structured control flow gives well-defined
single-entry regions. Locals, places, calls, and explicit ownership operations
give enough boundaries to construct blocks, block arguments, memory
dependencies, and cleanup edges without recovering source semantics.

### What is missing

The current TIR declarations do not contain:

- explicit BlockId values, block parameters, or phi nodes;
- stable ValueId or InstId identities for typed value numbering;
- an explicit terminator for every control-flow edge;
- a uniform memory token or alias set for loads, stores, views, and calls;
- a per-site fact bundle with validity scope and fallback;
- a complete escape state for each allocation and aggregate;
- an expression-level origin on every TExpr. Some child nodes carry a line or
  source span, but TExpr itself is only ty plus kind
  (crates/jet-codegen/src/Codegen/TIR/mod.rs:3685-3690);
- a single target-neutral vector operation and reduction-order contract in the
  core declarations.

The JIT currently fills part of this gap during lowering. LowerCtx stores
variables in name-keyed maps and lowers one nested TStmt/TExpr tree in one
pass (crates/jet-jit/src/jit/lower_ctx.rs:76-115). Cranelift can create
blocks and block parameters for special paths, but that is backend-local
construction, not a reusable semantic CFG. A self-hosted optimizer should make
the construction explicit once, then run ordinary SSA passes over ValueId and
BlockId.

### Conversion rules

The structured-to-SSA conversion is mechanical:

1. Create a block for each TIR control-flow region and an explicit edge for
   each branch, loop back edge, break, continue, return, try outcome, and
   cleanup path.
2. Turn a local assignment into a new SSA value. At a join, create a block
   parameter for each incoming value. This is the phi equivalent and keeps
   ownership state explicit.
3. Turn a structured place into a typed location plus a memory token. A
   local scalar can become a value; a field, list element, view, pool slot, or
   opaque handle remains a memory operation.
4. Preserve the TIR operation and its source identity even when a pass can
   prove it redundant. A removed bounds check or clone retains a proof link
   in optimizer diagnostics and debug output.
5. Represent unsupported or unknown operations as opaque calls with their
   effect and alias summaries. Do not silently lower them to an equivalent
   Rust or host operation.

The result is SSA-convertible without a semantic rewrite. The conversion
should be a private implementation of the optimizer module. It must not be
serialized as a competing program format or become a second place where
language meaning is defined.

## Where the current JIT spends time

This section is code evidence and a hypothesis about the competitive gap, not
a benchmark claim. The relevant comparison is against a specialized
CPython/JIT path and V8's typed speculative paths. Jet has a stronger input
fact: sema can know the type and ownership without a runtime type guard. The
current JIT often does not carry that advantage through lowering.

| Hot path | Observed implementation | Cost and consequence |
| --- | --- | --- |
| Generic integer arithmetic | The general Int Binary path dispatches Add, Sub, Mul, Div, Pow, and comparisons through host functions (crates/jet-jit/src/jit/lower_ctx.rs:27243-27305; host declaration and result plumbing at 10297-10377). The registry maps Int operations to jet_jit_int_* (crates/jet-jit/src/Numeric.rs:639-660). | A typed TIR operation becomes a Cranelift call boundary for every operation. The runtime then rechecks the tagged representation. jet_jit_int_add/sub/mul enter the runtime (crates/jet-jit/src/Numeric.rs:107-117), where each operation distinguishes small signed-63 values from arena-backed exact integers and repacks overflow (crates/jet-rt/src/lib.rs:895-932, 1020-1050). The fast direct-int-sum recognizer proves that a cheaper path is possible, but it is a special case (crates/jet-jit/src/jit/lower_ctx.rs:4398-4495). |
| List indexing | Generic TIR indexing lowers to a host list getter followed by a trap check (crates/jet-jit/src/jit/lower_ctx.rs:18741-18779). The getter checks the runtime length and produces the E3010 outcome on a miss (crates/jet-jit/src/Collections.rs:1379-1396). Proven fixed-list getters exist, but the generic path does not consume a general TIR interval fact (crates/jet-jit/src/Collections.rs:1476-1482). | A loop can pay a call, handle conversion, length lookup, and outcome branch per element. Repeated loop-header length loads are visible in the lowering code (crates/jet-jit/src/jit/lower_ctx.rs:1959-1964). Hoisting or deleting this work requires a typed length and bounds proof at the loop site. |
| Heap values and argument wrappers | LowerCtx has a general host-call boundary that declares a function, emits a call, and extracts its result (crates/jet-jit/src/jit/lower_ctx.rs:302-314). Call arguments may request clone, list widening, function coercion, or trait boxing; the lowering path handles these wrappers and can reject unsupported combinations (crates/jet-jit/src/jit/lower_ctx.rs:10431-10500). | Jet is not a blanket boxed-everything JIT: scalar CLIF values can be direct and heap values are commonly arena handles. The waste is at abstraction boundaries: repeated clone/retain work, handle-based collection access, and explicit trait boxes where TIR requires them. Without escape and ownership facts, the JIT cannot safely scalar-replace or stack-promote the aggregate. |
| Cloning aggregates | Scalar clone is a no-op, but non-scalars go through recursive or host-backed cloning (crates/jet-jit/src/jit/lower_ctx.rs:23060-23089). Struct cloning creates a record and clones each field (23301-23327); list cloning allocates an output list and iterates through elements (23330-23367). | Value semantics are correct but expensive in hot loops when sema already knows a value is uniquely owned, moved, frozen, or non-escaping. The optimizer needs the ownership fact, not a heuristic that removes a clone. |
| CFG and local state | LowerCtx keeps name-keyed Variable and type maps (crates/jet-jit/src/jit/lower_ctx.rs:88-115) while walking nested TIR. | The backend repeatedly reconstructs definitions, joins, and loop state. A canonical SSA conversion would expose dominance, block arguments, and use-def chains to GVN, LICM, and vectorization. |

The practical conclusion is narrower than "box removal everywhere." The JIT
needs to lower concrete sema-approved scalar operations to direct machine
values, carry heap values with explicit ownership and alias classes, and call
the Prelude only at the remaining semantic boundary. This gives Jet's static
facts a chance to beat dynamic guards and hidden-class recovery without
changing language meaning.

## Target optimizer module

### Deep module and seam

The optimizer should be one deep module:

- Interface: receive one TIR function, its read-only fact bundle, and a target
  profile; return target-neutral optimized SSA or a conservative lowering
  plan.
- Implementation hidden behind the interface: CFG construction, block
  arguments, memory SSA, alias windows, pass scheduling, layout legality,
  vector legality, and target cost modeling.
- Seam: the frozen TIR contract and its sema-owned fact channel.
- Adapters: Cranelift and future native AOT lower the optimized operations;
  the interpreter handles the same TIR operations and calls the same Prelude
  symbols when it deoptimizes.

The interface should be small enough that the caller does not know whether
the implementation used SSA, memory SSA, or a target-specific cost model.
That is the needed depth: the hard analysis lives once, while engine adapters
remain thin. TIR remains the semantic interface and keeps locality of meaning.
This is the leverage point: one sema proof can remove work in both executable
lenses without teaching either engine a new rule.

The optimizer must not accept raw AST, source text, Rust, or an independently
resolved symbol name. It accepts resolved TIR identities and facts. A malformed
TIR/fact relationship is an internal compiler error. A missing or
non-applicable fact is normal and selects the conservative operation.

### Proposed amendment to the frozen-TIR contract

R12 and #668 require one semantic-core TIR and exactly two
feature-identical executable lenses (docs/spec/architecture.md:710-736;
docs/spec/tir.md:1-11). The following is the minimum amendment needed to carry
optimizer facts without introducing a third lens:

> Amend the frozen TIR contract so that sema may attach one source-linked
> optimization-fact channel to each function, binding, place, call, loop,
> allocation, and access site. A fact bundle may contain the resolved type,
> range or bounds, effect and authority row, access and alias window, view
> provenance, freeze and immutability state, comptime value, contract proof,
> escape state, and layout facts. The target profile supplies target legality.
> This channel is part of the executable TIR contract, is produced by sema, and
> is consumed read-only by the
> JIT and optimized AOT lens, and is erased before runtime. A missing fact
> means "not proven"; consumers must preserve the checked Prelude operation.
> SSA is a private derived implementation and is not a third semantic IR or
> a parallel source of truth. Every TIR construct remains exhaustively
> handled by AOT, JIT, and reference interpretation.

This amendment does not require a serialized schema, versioned interchange
format, or compatibility reader. It is an in-memory contract between the
existing sema and TIR seam. Any later TIR shape change must amend #668 rather
than create a parallel optimizer representation.

### SSA data model

The IR is target-neutral but preserves Jet's semantic distinctions until
lowering.

| SSA component | Design |
| --- | --- |
| Identity | Function, block, instruction, and value IDs are dense internal IDs. Every value has one Jet type and one source origin. GVN keys use semantic operation, type, operands, and memory token, not the Rust carrier type. |
| Block | A block has typed block parameters, instructions, and one terminator. A branch, loop edge, match arm, try outcome, break, continue, or cleanup edge is explicit. Block parameters are the phi form. |
| Terminator | Branch, switch, jump, return, checked trap/outcome, and unreachable. A trap is an explicit Prelude-defined outcome, not a host panic. |
| Scalar values | Exact Int, fixed-width signed or unsigned integers, Float widths, Bool, Char, and semantic casts remain distinct. Exact Int may lower to an immediate fast representation with a Prelude-compatible spill path. |
| Aggregate values | Tuples, structs, enums, lists, strings, buffers, outcomes, and closures retain nominal type and ownership state. Extract, Insert, Aggregate, and Copy make SROA legal without erasing value semantics. |
| Views and places | A view carries owner, projection or range, access mode, and validity window. A place is a typed location, not an arbitrary pointer. Field, fixed-index, dynamic-index, range, pool, and layout-field places remain distinguishable. |
| Memory | Memory SSA has tokens for disjoint alias regions and an unknown region. Load and store dependencies use tokens. A proven view window can split a region; an unknown call joins to the unknown token. |
| Calls | A call carries resolved function or method identity, argument convention, result type, effect row, authority obligations, alias summary, and whether it can retain or return a view. There is no name-based redispatch in the optimizer. |
| Checks | Bounds, contract, overflow, conversion, stale pool ID, and outcome checks are explicit. A pass may remove one only when a sema fact proves its failure edge unreachable. Otherwise it lowers to the same Prelude operation. |
| Ownership | Move, borrow-read, borrow-write, clone, drop, and materialize-view are explicit operations. Ownership state is Owned, BorrowedRead, BorrowedWrite, Moved, or Frozen, with an escape state alongside it. |
| Vectors | Lane types, splat, lane arithmetic, lane load/store, and reductions are explicit. A reduction carries its required order. #Scalar is a no-vectorization boundary, not a different meaning. |
| Opaque effects | FFI, Shared locking, Pool generation checks, task spawn/await, transactions, unsafe gates, unknown effects, and observable allocation are opaque barriers unless sema gives a stronger fact. |

### Ownership and view memory model

The memory model is intentionally stricter than a generic pointer IR:

1. A read view is shared for the validity window. A write view from &T is
   exclusive for the call or borrow window. A move from ^T consumes the
   source. The optimizer may use those facts to prove non-overlap, but may
   not manufacture a longer lifetime.
2. A no-alias fact is scoped. Two views with different owners, disjoint
   constant fields, or disjoint constant ranges can use separate memory
   tokens. Dynamic projections remain potentially overlapping, matching
   ViewPlace.overlaps in sema.
3. A returned view carries its declared owner/provenance relation. A view
   cannot be stack-promoted past an owner boundary merely because its current
   instruction has no store.
4. Deep freeze marks the reachable value graph immutable. It enables
   read-only sharing and load reuse when the memory token confirms no external
   mutation. It does not imply no escape, no allocation, or a stack lifetime.
5. Shared, Pool/Id, FFI, and unsafe operations are separate alias or
   authority domains. Lock acquisition, generation validation, foreign calls,
   and unsafe gates are barriers unless an explicit sema fact describes their
   behavior.
6. A memory transformation must preserve drop, retain/release, task capture,
   transaction rollback, and observable allocation behavior. A faster
   representation is valid only when it is observationally equivalent.

This gives vectorization the useful property that it needs: no-alias is a
proof about a set of accesses for one loop, not a global promise about all
values with a similar type.

### Layout freedom

The default backend contract for this design is physical-layout-unspecified.
An ordinary struct's field order, padding, and aggregate storage may change;
the optimizer may use field reordering, scalar replacement, and AoS-to-SoA
when no language operation observes physical layout. The logical type,
field names, ownership rules, and Prelude behavior do not change.

#Layout(c) is an ABI observation and is an optimization fence. D-SOA1's
#Layout(columnar) is the explicit logical columnar storage choice and must be
represented by the same TIR decision in every engine. The self-hosted
optimizer may choose or refine a physical layout only inside those existing
rules; it must not add a layout spelling or a second columnar mechanism.

Because the current source law explicitly names C and columnar layouts but
does not state the broad default in the files cited above, the implementation
follow-up must record the physical-layout-unspecified sentence in the owning
layout decision before relying on it for general field reordering. This is a
design dependency, not permission for the optimizer to guess.

## Fact-to-pass map

Every pass gets a fact lookup and a conservative answer. Facts are inputs to
legality, not merely cost hints.

| Pass | Sema/TIR fact lookup | Legal transformation | Conservative fallback |
| --- | --- | --- | --- |
| CFG construction and SSA conversion | Structured TIR regions, reachability, exhaustive enum match, explicit Return, Break, Try, and Unreachable nodes (crates/jet-codegen/src/Codegen/TIR/mod.rs:3332-3351, 4263-4275). | Build explicit blocks and block parameters; preserve every cleanup and outcome edge. | Keep an opaque edge or emit an ICE if TIR is malformed. Never infer exhaustiveness from a target compiler. |
| Mem2reg and local promotion | Typed TLocal, declaration/move/uninitialized flow facts, and access conventions (crates/jet-codegen/src/Codegen/TIR/mod.rs:628-754; crates/jet-sema/src/Sema/FlowFacts.rs:590-721). | Replace a local slot with SSA values when its address/place is not observed; insert block parameters at joins. | Keep a local memory slot and its dependencies. |
| Direct-call resolution and inline | Resolved TMethodRef, function identity, is_inline, is_inline_always, size, purity/effect row, and ownership signature (crates/jet-codegen/src/Codegen/TIR/mod.rs:763-811, 2554-2638). | Inline a known body when the call has no forbidden effect, recursion or size cost. Preserve call-site provenance and ownership. | Keep the direct call. Unknown or foreign calls are opaque. |
| SCCP and constant propagation | CtValue, CtLit, immutable-local constant values, exact integer reachability, literal widths, and interval facts (crates/jet-foundation/src/AST/comptime.rs:694-732; crates/jet-codegen/src/Codegen/TIR/mod.rs:3752-3766; crates/jet-sema/src/Sema/mod.rs:1449-1467). | Fold pure scalar and aggregate operations whose exact semantics are known; specialize fixed loop bounds and branch outcomes. | Keep the operation. Do not fold through an effect, a possible trap, NaN-sensitive operation, or unknown exact-Int spill. |
| SROA and aggregate scalarization | Nominal type shape, field facts, owned/no-escape state, no address or view escape, and physical-layout legality. | Split an owned local struct or tuple into fields; remove temporary aggregates and redundant copies. | Keep the aggregate. Never split #Layout(c), a foreign-visible value, a shared handle, or an escaping view. |
| GVN | Semantic type, stable ValueId operands, pure operation, frozen operands, and memory token. | Reuse equal pure values and immutable loads. Treat equivalent exact arithmetic only when overflow, spill, NaN, and trap behavior match. | Keep distinct computations when effects, memory, allocation, authority, or order may differ. |
| Dead-code and branch cleanup | Effect summary, authority obligations, contract/outcome edges, Unreachable, and sema reachability. | Remove only values and branches with no observable effect, ownership obligation, trap, diagnostic, or cleanup. | Preserve the statement or lower it as an opaque effect. |
| Alias and memory dependence | ViewPlace owner/projection, access mode, validity window, SplitViews, EditDisjoint, frozen state, Pool/Shared domain, and call retention summary (crates/jet-codegen/src/Codegen/TIR/mod.rs:4692-4803; crates/jet-sema/src/Sema/mod.rs:1038-1165). | Separate proven-disjoint memory tokens; reorder independent reads; retain write ordering. | Join to the unknown token for dynamic overlap, unknown calls, FFI, Shared, Pool, or unsafe. |
| Bounds-check elimination | Distinct interval, fixed-list proof, inline range, fixed length, and future loop induction/range fact (crates/jet-foundation/src/AST/lvalues.rs:41-67; crates/jet-codegen/src/Codegen/TIR/mod.rs:4090-4100, 4683-4686). | Remove a bounds check when every iteration index is proven in range; hoist one check when the proof covers the loop. | Call the checked Prelude list/view operation and retain its E3010 or equivalent outcome. |
| Escape analysis and stack promotion | Sema GC promotion, capture escape/materialization, ownership moves, return-view provenance, no address-taking, no task/foreign/shared/pool escape (crates/jet-sema/src/Sema/MemoryFacts.rs:141-203; crates/jet-foundation/src/AST/expressions.rs:364-416). | Put a non-escaping owned aggregate in the frame; scalarize it; remove matching heap allocation and retain/release. | Keep the heap or arena representation and all ownership operations. |
| LICM | Loop and range structure, pure/readonly effect row, invariant operands, alias tokens, freeze, bounds and trap facts. | Hoist pure invariant computation, invariant length, or a proven-safe check out of the loop. | Keep it in the loop when it may observe mutation, allocate, trap, acquire authority, or change an outcome. |
| Vectorization | Counted/ForIn loop shape, lane types, fixed trip or remainder behavior, no cross-iteration dependency fact, view no-alias windows, pure body, and #Scalar boundary. | Form D-SIMD1/2 portable lane operations and lower through the same Prelude-backed mechanism. Use a scalar remainder with identical order. | Keep scalar loop. Do not speculate on aliasing or silently reorder a reduction. |
| Lowering | Resolved operation identity, semantic type, effect/authority row, layout fact, target profile, and Prelude symbol. | Select direct machine instructions or a Prelude call only when the selected sequence is equivalent for all reachable cases. | Use the canonical Prelude/Cranelift/interpreter adapter path, with the same check and error meaning. |

The required hot-path sequence is therefore:

1. Inline known calls using resolved identities and sema cost/effect facts.
2. Run SROA and ownership-aware local promotion.
3. Run GVN over typed values and memory tokens.
4. Run LICM after memory and trap legality are known.
5. Run vectorization using explicit no-alias and loop facts.
6. Lower to Cranelift or native AOT, preserving Prelude calls and semantic
   outcomes.

Bounds, alias, escape, and constant facts are analyses consulted by this
sequence. They are not a second pass pipeline that can disagree with sema.

## Staged landing plan

### Land in the Cranelift JIT now

These steps use facts already present in TIR. They improve TIR-to-Cranelift
lowering without waiting for self-hosting or changing the semantic contract.

| Slice | Use existing evidence | Boundary |
| --- | --- | --- |
| Mechanical CFG and local SSA conversion inside LowerCtx | TStmt control-flow shape, typed TLocal, existing Cranelift block parameters, and resolved TExpr types. | Private JIT implementation only. Do not add a second semantic IR or change TIR meaning. |
| Typed scalar lowering | TExpr.ty, literal widths, TNumericOp, is_scalar, and existing fixed-width TIR decisions. Keep direct CLIF values for fixed-width scalars and a proven small exact-Int fast path. | Overflow, exact-Int spill, diagnostics, and policy remain the Prelude contract. The fallback calls the same semantic operation. |
| Bounds and length reuse | IndexKind::FixedListProof, InlineRange, fixed-list metadata, and loop structure. Hoist a stable length and use proven fixed getters where the TIR proof covers the access. | No generic interval inference in the JIT. If the TIR proof does not cover the access, retain the checked getter and E3010 path. |
| Ownership-aware wrapper reduction | TCallArg clone/borrow/widen/trait decisions, Borrow, Clone, ExplicitCopy, SplitViews, and EditDisjoint. | Remove only redundant carrier work proven by TIR. Do not infer no-escape or delete a required value-semantic clone. |
| Direct calls and small pure inlining | Resolved function/method identity, TFunc inline/pure fields, and current effect nodes. | Unknown, foreign, Shared, Pool, task, transaction, and unsafe paths stay opaque. |
| Portable lane lowering | Existing D-SIMD1/2 lane forms, D-SIMD3's AOT-default policy, and #Scalar. | Lower already-selected lane forms through one Prelude-backed lane family and preserve left-to-right reduction semantics. Full loop vectorization remains the self-hosted slice; no JIT-only vector syntax or policy. |
| Evidence counters | Count host calls for scalar arithmetic/indexing, runtime tag checks, allocations, clones, deopts, vector width, and bounds checks in the benchmark corpus. | Counters validate the design; they do not authorize a semantic shortcut. Do not add a jit_gaps parking entry. |

The immediate JIT target is typed slots, direct scalar operations, direct calls,
hoisted proven invariants, and fewer wrapper boundaries. This addresses the
observed jet_int_add tag branch and per-element collection path while
remaining a thin TIR lens.

### Wait for the self-hosted optimizer

These steps need the #668 amendment and a sema-to-optimizer implementation.

| Slice | Required fact or implementation | Result |
| --- | --- | --- |
| General fact export | Per-site type, interval, loop induction, access window, alias class, effect/authority row, freeze, comptime, contract, escape, and layout facts. | No rediscovery of ownership, purity, or bounds in a backend. |
| Private typed SSA module | Function/block/value IDs, block arguments, memory SSA, cleanup edges, semantic operations, and source origins. | One deep optimizer implementation shared by the optimized AOT lens and any JIT prepass that adopts it; TIR remains the contract. |
| Full pass pipeline | Inline, SROA, GVN, LICM, vectorize, and target-aware lowering, with SCCP, alias, escape, bounds, and DCE support. | AOT kernels can reach or exceed the generated Rust/LLVM code-quality baseline while retaining Jet facts directly. |
| Ownership-aware layout optimization | Physical-layout-unspecified default, field reorder, scalar replacement, AoS-to-SoA, D-SOA1 columnar, and C-layout fences. | Hardware-friendly storage without changing logical Jet types or ABI-observable data. |
| Native AOT backend | Lower the same semantic SSA operations to hardware instructions and Prelude calls. | Remove redundant Rust parsing, borrow checking, and LLVM rediscovery while preserving executable meaning. |
| Cross-tier proof | Compile the same TIR through AOT, default jet run, and interpreter/deopt; compare output, outcomes, diagnostics, and reduction bits. | Satisfy I9 and prevent an optimizer-only semantic fork. |

Self-hosting is not permission to skip optimization. The compiler-speed plan
states the physics clearly: transpilation can approach cargo/rustc optimized
AOT parity, while self-hosting wins by removing redundant frontend work and
making incrementality and optimization budgets explicit
(docs/plans/compiler-speed.md:113-154).

## I9, SIMD law, and the Rust-parity exception

### I9: one meaning, one Prelude

The SSA operation set is a typed representation of Prelude and Core
operations. It is not a new runtime library. The following rules are
mandatory:

- A bounds fact can remove a check only because the failure is proven
  unreachable. It cannot change the checked operation's success value or
  error.
- An exact-Int fast path can use a machine add only when its range and
  overflow/spill behavior are equivalent. The slow path and any diagnostic
  come from the same Prelude semantics.
- An effect row is a legality boundary. The optimizer cannot reorder I/O,
  authority, allocation, locking, transaction, task, or unknown operations
  because a target compiler happens to permit it.
- The interpreter does not implement a second optimized meaning. A deopt
  marshals to the same Prelude operation.
- AOT Rust emission remains a spelling and verification adapter. rustc
  rejection is never a Jet semantic diagnostic, and rustc is never the source
  of a missing sema proof.

These rules implement I9's "one Prelude, dumb engines" requirement
(docs/spec/architecture.md:710-736) and the one-mechanism philosophy
(docs/spec/philosophy.md:166-216).

### D-SIMD1, D-SIMD2, and D-SIMD3

D-SIMD1 and D-SIMD2 define portable lane values, constructors, elementwise
operations, reductions, and fixed-list bridges. D-SIMD3=B makes AOT
auto-vectorization the default, retains #Scalar as the explicit scalar
boundary, and requires the portable lane family and left-to-right
bit-identical reduction fold (docs/spec/syntax-decisions.md:2904-2922).

The self-hosted vectorizer inherits that mechanism. It does not add a SIMD
syntax, a JIT-only lane type, or a second reduction implementation. A no-alias
and no-cross-iteration-dependency fact makes vectorization legal; D-SIMD3
selects the default policy. #Scalar stops the vectorizer at an explicit
boundary. Scalar fallback and vector lanes call the same Prelude-backed
operation and preserve reduction order.

### Rust-parity exception

During the transpile era, Jet may use generated Rust plus rustc/LLVM as the
temporary optimized AOT implementation. Rust supplies the ownership
verification boundary and LLVM supplies mature native optimization
(docs/spec/architecture.md:935-942). The performance target is to meet or
beat the Rust/cargo generated-code baseline for the relevant kernels. The
compiler-speed plan describes this as the limit of transpilation and the
reason for a self-hosted backend (docs/plans/compiler-speed.md:147-154).

This exception is about output quality while transpiling. It does not allow
rustc to prove Jet semantics, does not make LLVM the only place where bounds
or alias facts exist, does not help the JIT, and does not weaken I9. The
self-hosted optimizer must be measured against that same generated-Rust
baseline and must meet or beat it without reintroducing a second semantic
authority.

## Rejected alternatives

### Add more annotations to generated LLVM/Rust

Useful as an interim AOT hint, but not the design. The facts have already
crossed the frontend/TIR seam and are scattered or discarded by the time
rustc/LLVM sees generated Rust. LLVM cannot improve the Cranelift JIT, cannot
see Jet's view lifetime and authority model in full, and must re-infer facts
that sema already proved.

### Create a separate optimizer IR beside TIR

Rejected as a semantic architecture. It creates a third representation to
keep feature-identical, duplicates lowering decisions, and conflicts with
R12/#668's one TIR contract. A private SSA form is allowed only as a derived
implementation with TIR plus fact channel as its sole input and no independent
syntax, parser, or semantic authority.

### Trust rustc or do nothing

Rejected for both targets. It leaves the per-operation exact-Int dispatch,
collection checks, clone wrappers, and missed ownership-based vectorization in
the JIT, and it asks AOT to rediscover the facts. It cannot satisfy the
hardware-ceiling kernel goal or the default jet run goal.

### Make the JIT speculative and guard-heavy

Rejected as the default for statically proven code. V8-style guards and
whole-program deopts are useful when types are unknown; Jet's sema facts
should make the common path guard-free. Unsupported or genuinely unknown
operations may remain explicit slow paths, but a whole program must not deopt
because the optimizer failed to consume a fact.

### Put policy in host helpers

Rejected by I9. Host helpers may marshal values, call Prelude, and return
results. They may not reimplement bounds policy, defaults, error text,
authority, freeze, or memory semantics. An optimizer fast path is acceptable
only when it is a semantics-preserving lowering of the Prelude operation.

## Follow-up and proof obligations

After owner ratification of the #668 amendment and the default layout
statement, implementation work should split into these bounded cards:

1. Export the source-linked fact channel from sema into frozen TIR.
2. Build the structured-TIR-to-SSA conversion and memory model.
3. Land Cranelift typed lowering, direct scalar paths, and proven bounds
   reuse.
4. Land the classic pass pipeline and ownership-aware vectorizer.
5. Land native AOT lowering and retire the corresponding Rust-only
   rediscovery.
6. Run the cross-tier parity and Rust/CPython/Node kernel corpus.

Each card must name its applicable TIR constructs and execution tiers. Closure
requires AOT and default jet run parity, interpreter parity when deopt can
reach the surface, no new jit_gaps entry, and measured evidence for:

- host calls and runtime tag checks per arithmetic operation;
- bounds checks and length loads per loop;
- clone, retain/release, allocation, and trait-box counts;
- vector width, scalar fallback, and reduction bit identity;
- deopts and unsupported whole-program paths;
- output quality against generated Rust and throughput against CPython and
  Node on the campaign corpus.

The design is complete when these measurements can attribute a win to a sema
fact lookup, not to a duplicated engine rule.
