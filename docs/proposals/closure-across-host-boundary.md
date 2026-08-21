# Closure across the host boundary for curried compute transforms

Design record for card #2028. This document is the design deliverable. It does
not change the compiler, Prelude, examples, tests, or Tower state.

## Decision

Use a narrow, Prelude-owned compute tape handle for a curried transform. The
handle is an opaque `i64` at every host seam. It contains either:

- a deferred transform plan: the retained compute base callable, transform
  kind, target indexes, and result shape; or
- a tape-backed continuation: the output/anchor, target indexes, gradient type,
  and continuation kind for a VJP pull or gradient callable.

Both kinds use one call operation, `jet_compute_call_curried`. The handle table,
retention rules, tape construction, transform selection, VJP state, errors, and
result meaning belong to `Prelude/CoreLib/Top/Compute.rs`. AOT emit, the
Cranelift host, and interpreter ambient code only construct or marshal the
handle and its tensor arguments.

This is candidate (iii), the narrow compute answer. It does not introduce a
user-visible `Tape` type or syntax. It does not solve general first-class
closures. Candidate (i), a language-wide `(fn-ptr, env-handle)` closure table,
is a later card only if another surface needs a closure to persist across a
host frame. The compute handle has a private, compute-specific callback slot
inside the Prelude-owned record because an arbitrary `f` must still be invoked
when the derivative function is called. That slot is not a second semantic
mechanism or a general function-value feature.

This card owns the decision only. Guard edits, lowering arms, Prelude symbols,
examples, tests, gap retirement, commits, and Tower writes belong to the
follow-ups. The design does not reopen or modify compute stem #1757.

The design is deliberately deep at one seam:

| Term | Decision |
|---|---|
| Module | `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs` owns the handle and its meaning. |
| Interface | `jet_compute_curried_new`, `jet_compute_call_curried`, `jet_compute_curried_clone`, and `jet_compute_curried_drop`. |
| Adapters | AOT emit, Cranelift host functions, and interpreter ambient marshal values to that interface. |
| Depth | A caller does not know whether the handle holds a deferred plan or a VJP tape. It only supplies the typed tensor pack required by the handle kind. |
| Seam | The callable and its environment survive the frame that created `compute.gradient(f)`. |
| Leverage | Gradient, value-and-gradient, VJP continuations, JVP, and higher-order transforms share one lifetime and invocation path. |
| Locality | Transform policy and error meaning have one home. Tier files contain only representation conversion. |

## Why this is the right scope

The ratified surface already says that `compute.gradient` has two arities:
the direct function-plus-values form and the function-only form that returns a
derivative function (`docs/spec/syntax-decisions.md:5624-5635`). VJP already
returns callable `.pull` and `.grads` continuations
(`docs/spec/syntax-decisions.md:5637-5641`). The missing piece is not a
new spelling. It is the lifetime of the returned callable after the host frame
that made it.

The existing Prelude already has the semantic center. `Compute.rs` documents
that engines marshal callable arguments and typed results while one transform
dispatcher owns transform selection, scalar seeding, value detachment, and
lazy VJP state (`crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:2954-2998`).
The design extends that center to the returned callable instead of making each
engine manufacture a private closure convention.

## Current seams and the honest refusal

The current source has four relevant seams. The line numbers below match the
current working tree.

| Seam | Current evidence | Design consequence |
|---|---|---|
| Resident admission | `crates/jet-jit/src/jit/safety.rs:45-80` admits the lowered `[function, targets]` shape through `args.len() >= 2`, then checks the value count. For that valid shape, `value_count == 0` and `expected_values == 0`, so the predicate does not currently refuse it; it has no source-arity-one fact. | Replace the broad admission with an explicit curried shape check. Keep malformed shapes out. Do not treat the guard as the closure lifetime mechanism. |
| AOT emit | `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:212-223` rejects fewer than two TIR arguments and defines `args.len() == 2` as `transform`; `:339-405` emits the returned `Rc` closure. | The curried arm must create and call the Prelude handle. The `Rc` closure may remain only as a marshalling adapter, if needed. |
| TIR and resident JIT lowering | `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:308-393` appends the implicit `List<Int>` target, producing `[base, targets]` for the curried source form. `crates/jet-jit/src/jit/lower_ctx.rs:13792-13827` accepts that shape, passes zero as the input-list handle, and calls the private transform factory path. | Change source/TIR lowering, the resident guard, AOT emit, and JIT marshalling in one implementation diff. The JIT must stop manufacturing a second transform convention. |
| Interpreter and current JIT callable models | `crates/jet-codegen/src/Codegen/TIR/eval/compute_calls.rs:281-326` stores `EvalCallable::ComputeTransform` for `args.len() == 2`; `eval/mod.rs:1218-1265,3452-3550` snapshots and dispatches it. Its `ComputeLite` seam reconstructs the tape and applies the transform in `crates/jet-comptime/src/Comptime/ComputeLite.rs:1180-1214`. The JIT stores compute state in `ComputeState` (`crates/jet-jit/src/Compute.rs:556-573`), runs direct transform policy in `:871-1026`, and creates arity-specific factories in `:1028-1103,1127-1184`. | Move plan, tape, transform selection, and continuation lifetime to the Prelude. Both engines become adapters over the same handle and call operation. |

This is an I9 divergence, not only an admission refusal. The card's wording
that the resident arity guard refuses the curried form is stale: the current
predicate admits the valid lowered shape. AOT uses an inline `Rc` transform
closure, resident JIT uses an arity-specific factory plus an environment
record, and the interpreter uses compute-specific callable records. Each can
represent the value, but no one representation owns the closure across all
host seams. The design fixes that split instead of adding a fourth convention.

The resident predicate has a second blocker for the card's full proof. Its
return gate currently accepts a tuple only for `gradient`
(`crates/jet-jit/src/jit/safety.rs:62-67`). Sema defines tuple results for
`value_and_gradient` and `jvp`, and an applied `VjpRun` result for `vjp`
(`crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs:864-874`). Therefore
`compute_autodiff` cannot become resident by fixing only the curried
`gradient` handle. Follow-up B must carry the exact checked result shapes for
all four transforms into its same lowering/admission change. It must not use a
blanket result-type acceptance rule.

## Same program: before and after

The existing executable specification is the same program on both sides:

```jet
use core.compute as compute

fn loss(w: Tensor, x: Tensor) :> Tensor :: compute.mul(w, x) ?? panic("loss")

fn run() {
    w :: compute.from_list([2.0]) ?? panic("w")
    x :: compute.from_list([4.0]) ?? panic("x")

    d_loss :: compute.gradient(loss)
    (dw, dx) :: d_loss(~w, ~x)
    print("transform_w:{compute.to_list(dw)}")
    print("transform_x:{compute.to_list(dx)}")

    curvature :: compute.gradient(d_loss)
    curvature_value :: curvature(w, x)
    print("curvature_wx:{compute.to_list(curvature_value.w.x)}")
}
```

Before this design, `d_loss` is an interpreter callable snapshot on the
interpreter path, an inline `Rc` closure in AOT, and an arity-specific factory
in resident JIT. The program has a meaning, but its tiers do not share one
persistent compute handle.

After the implementation cards, `compute.gradient(loss)` creates one
Prelude-owned curried handle. `d_loss(~w, ~x)` marshals that handle and the two
tensor values to `jet_compute_call_curried`; the call creates its input tape,
invokes the retained base callable, and applies the requested transform. The
second `compute.gradient(d_loss)` registers a new narrow compute base whose
invocation calls the first handle. That is the required higher-order and
outlives-the-frame proof.

The output remains byte-identical:

```text
transform_w:[4.0]
transform_x:[2.0]
curvature_wx:[1.0]
```

The full executable example is
`examples/features/tooling/compute_autodiff.jet`; its existing golden is
`examples/features/expected/tooling/compute_autodiff.out`.

## Candidate evaluation

Candidate (iii) is evaluated first because it is the smallest mechanism that
can meet all three criteria without silently making general first-class
closures part of this card.

### (iii) Narrow Prelude-owned compute tape/plan handle — selected

The handle is a compute-specific opaque value, not a user-visible type:

```text
TransformPlan {
    base: retained compute-callable adapter
    method: Gradient | ValueAndGradient | Vjp | Jvp
    targets: [Int]
    result_shape: compiler-proved schema
}

VjpContinuation {
    output: Tensor
    anchor: Tensor
    targets: [Int]
    gradient_ty: compiler-proved schema
    kind: Pull | Grads
}
```

`TransformPlan` is created by `jet_compute_curried_new`. It does not allocate a
tape until the returned callable is invoked. At invocation,
`jet_compute_call_curried` traces the input tensors, invokes `base`, starts the
VJP state, and calls the existing Prelude transform dispatcher. A VJP pull or
grads handle already has its tape-backed state and uses the same call entry;
the handle kind selects the seed/continuation operation inside the Prelude.

This directly matches the interpreter evidence: `ComputeTransform` describes a
deferred base/method/target/result plan, while `ComputePull` and `ComputeGrads`
are fully described by output/anchor/targets/gradient type. It meets the three
card criteria for the compute surface without claiming that all Jet closures
are now first-class across host frames.

### (i) General `(fn-ptr, env-handle)` closure table

This is the broad solution. A Prelude-owned table would retain a function
pointer and environment, with typed `jet_closure_invoke_*` entry points.

It solves arbitrary escaped closures, but it also chooses a language-wide
function-value ABI, typed result-family rules, capture mutation rules, thread
transfer rules, and a public lifetime model. None of those choices is required
to make `core.compute` work. It would make this card a general first-class
closure feature by accident.

Decision: later. The narrow compute callback slot must be shaped so that it can
be replaced by this general table without changing `jet_compute_call_curried`,
but candidate (i) is not built by #2028's implementation cards.

### (ii) Monomorphized thunk per transform site

Each curried call site could emit a thunk that captures `f` and invokes the
transform. This is close to the current AOT `Rc` closure and the current JIT
arity-specific factory.

It is rejected because every engine would own a different thunk construction
and lifetime convention. It also grows code by base arity and transform site,
keeps policy in emit/host code, and makes higher-order transforms a chain of
private thunks. It does not satisfy the one Prelude mechanism in I9.

## Prelude interface and calling convention

The implementation must add the following interface to the compute Prelude.
Names and argument roles are fixed by this design; the exact Rust result
carrier can follow the existing `JetComputeTransformResult` representation.

```text
jet_compute_curried_new(
    base: JetComputeBase,
    method: JetComputeTransformKind,
    targets: &[Int],
    result_shape: JetComputeResultShape,
) -> JetComputeHandle

jet_compute_call_curried(
    handle: JetComputeHandle,
    inputs: JetComputeInputPack,
) -> JetComputeCurriedResult

jet_compute_curried_clone(handle: JetComputeHandle) -> JetComputeHandle
jet_compute_curried_drop(handle: JetComputeHandle)
```

`JetComputeHandle` is `repr(transparent)` over a nonzero `i64`. Zero is
invalid. It is the same width and opaque-handle discipline used by the JIT
runtime. `JetComputeBase` is an internal callback descriptor, not a Jet type:
it contains a retained invoke adapter and environment token. Its invoke
contract accepts a tensor pack and returns the tensor result family expected by
the stored plan. The adapter may be implemented by AOT, Cranelift, or the
interpreter, but it may only marshal values and report the shared Prelude
result/error carrier.

The callback descriptor has one arity-independent logical shape:

```text
JetComputeBase {
    invoke(env: i64, inputs: JetComputeInputPack) -> JetComputeBaseResult
    env: i64
    retain(env: i64)
    release(env: i64)
}
```

`inputs` is a pack, not one callback ABI per base arity. A JIT adapter may
unpack that pack through its existing callable machinery, but it may not create
`factory_0` through `factory_6` transform semantics. AOT and interpreter
adapters provide the same logical invoke/retain/release contract. The Prelude
handle table owns the descriptor reference and invokes it only after taking a
snapshot, so the descriptor cannot point at a dead frame.

The common logical ABI is:

| Item | Contract |
|---|---|
| Handle | Opaque nonzero `i64`; copied only through clone/drop rules. No tier may inspect its table index or encode method meaning in it. |
| Method | One Prelude transform-kind value. A host does not branch on string spelling to choose semantics. |
| Targets | A Prelude-owned list of checked `Int` indexes. The host can carry a list handle, but cannot silently repair, sort, or default it. |
| Inputs | A tensor pack with the primal values, and tangent values when the stored kind is JVP. The Prelude validates arity and family. |
| Result | A shared compute result carrier: Tensor, typed gradient tuple, `VjpRun`-equivalent value, or value/tangent tuple. The adapter projects it to the TIR result type. |
| Failure | One Prelude compute error/trap path. A host reports the carrier; it does not invent a refusal, default, or error text. |

### Tier marshalling

| Tier | Construction | Invocation | Result conversion | Policy forbidden in the tier |
|---|---|---|---|---|
| AOT | `core_calls.rs` packages the lowered base closure as `JetComputeBase` and calls `jet_compute_curried_new`. The generated Rust may use a typed internal wrapper, but the handle crossing the semantic seam is still the opaque `i64`. | Generated code calls `jet_compute_call_curried` with the handle and typed `JetTensor` pack. | The AOT adapter destructures the shared result carrier into the already-sema-checked return type. | No inline `Rc` transform algorithm, target defaulting, scalar seed, VJP state policy, or private error meaning. |
| Cranelift JIT | `lower_ctx.rs` supplies an existing resident callable adapter token, target list handle, method code, and result schema to the `jet_compute_*` host seam. `Compute.rs` retains only the adapter bookkeeping needed to turn i64/list values into Prelude values. | The host function named `jet_compute_call_curried` takes the curried i64 handle and input list handle, calls the included Prelude compute implementation, and returns the ordinary tensor/record handle. | `Compute.rs` allocates or looks up the returned tensor/record handles and checks the trap carrier. | No `run_transform`/factory arity policy, transform selection, VJP state ownership, or custom error text in `Compute.rs`. |
| Interpreter ambient | `eval_core_compute_call` packages the existing callable snapshot's base/method/targets/result facts through `jet_compute_curried_new`. The evaluator runtime may retain an adapter token for the base callback. | `call_callable` routes a compute handle to `jet_compute_call_curried`; it does not re-run a separate `ComputeLite` transform ladder. | CtValue conversion wraps the shared tensor/result carrier back into the TIR value. | No `EvalCallable::ComputeTransform`/`ComputePull`/`ComputeGrads` semantic fork after migration, no private gradient seed, and no interpreter-only error behavior. |

The existing direct transform symbols remain valid Prelude internals for the
direct function-plus-values form: `jet_compute_trace_inputs`,
`jet_compute_vjp_begin`, `jet_compute_transform`,
`jet_compute_vjp_pull_or_panic`, and
`jet_compute_vjp_unit_grads_or_panic`. The new curried interface composes those
functions in one Prelude-owned operation. It does not move their meaning into
the adapters.

## Ownership, lifetime, and reentrancy

### Ownership

`jet_compute_curried_new` allocates one owning table entry. The entry retains
the base callback environment and, after the first call, any tape/VJP state
needed by a continuation. `clone` increments the entry's ownership. `drop`
decrements it; the last drop releases the callback environment and tape. A
returned or captured handle therefore remains valid after the creating lexical
frame ends. There is no pointer into a stack frame, TIR temporary, or JIT
activation record.

The runtime may clear a run-scoped table only after its live handle count is
zero. An expired or invalid handle is a Prelude compute error, not a host
segfault and not a tier-specific diagnostic. Generated AOT cleanup, JIT result
drop/arena cleanup, and interpreter callable cleanup all call the same clone
and drop contract. The implementation must prove both repeated calls and
escaped calls; a one-shot table is not sufficient.

The callback environment follows the same rule: retain before returning the
handle, release only after the last handle drops. The JIT may use its existing
`JitCallableSlot` representation as the adapter's backing storage, but the
curried handle and its lifetime are owned by the Prelude table. The interpreter
may use its callable snapshot arena as the callback backing storage, but the
compute plan is not owned by the evaluator's transform match arms.

### Reentrancy

The Prelude handle table must never hold its mutable table lock while invoking
the base callback. `jet_compute_call_curried` takes a retained snapshot of the
plan/state, releases the table borrow, invokes the callback, and reacquires the
table only to publish new tape state or release temporary references. This
allows:

```text
host frame
  -> jet_compute_call_curried(outer)
       -> base adapter
            -> Jet call of an inner curried handle
                 -> jet_compute_call_curried(inner)
```

The same rule covers higher-order `curvature :: compute.gradient(d_loss)`.
No host adapter may rely on a single mutable “current transform” slot. Shared
tape state must use the existing owned compute state (`Arc`/locked state where
needed) and must not borrow a JIT activation or evaluator scope across the
callback.

The first implementation card should keep the callback's execution tier and
thread affinity equal to its creating runtime. Cross-thread transfer is not a
new promise in this design; if a later surface needs `Send` closure handles, it
must use the general closure decision rather than silently extending this
compute-specific handle.

## Callback precedents and boundaries

The repository already has useful closure-crossing precedents. The design
reuses their ABI and lifetime lessons, but does not merge their semantic
domains into compute.

| Precedent | What it proves | What the compute handle subsumes or leaves alone |
|---|---|---|
| Collection callbacks | `functions_compile::lower_collection_callable_lambda` says the collection operation stays in the shared Prelude while the JIT supplies callback ABI and opaque captures (`crates/jet-jit/src/jit/functions_compile.rs:701-727`). `lower_collection_callback` binds `fn_ptr/env` and `sync_collection_captures` publishes mutable captures (`crates/jet-jit/src/jit/lower_ctx.rs:28910-28975`). | Reuse: an opaque retained environment, no stack capture, and explicit write-back. Subsumes only the idea of a durable callback adapter. It does not replace collection capture synchronization or collection operation symbols. |
| JIT spawn-site callbacks | `TCoreClosureKind::Spawn` and `OnInterrupt` carry explicit callback sites and engine adapters (`crates/jet-codegen/src/Codegen/TIR/mod.rs:4542-4555`); `first_spawn_site` locates the authoritative TIR site (`crates/jet-jit/src/jit/safety.rs:5962-5970`). | Reuse: TIR owns the site fact and the engine marshals it. Leave as-is: spawn, interrupt, reactive, and UI callbacks have event/thread/lifecycle contracts outside pure compute. |
| ParaMap and ParaFilter | `TClosureOp::ParaMap` is lowered through the existing native iterator adapter (`crates/jet-jit/src/jit/lower_ctx.rs:29047-29052`) and AOT emits the shared list operation (`crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs:3920-3922`). | Leave as-is. ParaMap's ordered/parallel scheduling and callback shape are collection semantics. The compute handle may use the same opaque callback discipline, but it must not inherit ParaMap's scheduling or private host policy. |
| Interpreter callable snapshots | `EvalCallable::ComputeTransform`, `ComputePull`, and `ComputeGrads` are cloned before invocation (`crates/jet-codegen/src/Codegen/TIR/eval/mod.rs:1218-1265`, `:3452-3553`). | Subsumes the compute-specific records and their escape behavior into the Prelude handle. Generic lambda capture snapshots and scope write-back remain the interpreter's general callable machinery. |

The current JIT callable slot is a useful adapter shape: it is an opaque
`fn_ptr` plus `env` (`crates/jet-jit/src/jit/runtime_host.rs:230-239`) and its
binding checks the function address before handing it to generated code
(`runtime_host.rs:3091-3125`). It is not the semantic owner of a compute tape.
The implementation must not copy that table into a second compute table with
different lifetime rules.

## Criterion 2: one diff, two guards, one lowering arm

The implementation card for lowering must make the admission and lowering
change inseparable. The intended diff shape is:

1. `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:308-393` keeps the
   one `core.compute` transform lowering path and makes its source-arity-one
   case explicit. It appends the target list exactly once. The resulting TIR
   shape is `[base, targets]` for a curried transform and
   `[base, values..., targets]` for a direct transform.
2. `crates/jet-jit/src/jit/safety.rs:50-53` narrows the native predicate to
   accept the valid `[base, targets]` case with zero value arguments, plus the
   existing direct value cases. It does not turn `args.len() >= 2` into a
   blanket acceptance rule.
3. `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:218-223` changes in
   the same patch. Its `args.len() == 2` branch calls
   `jet_compute_curried_new` and emits a function value backed by the returned
   handle; the direct branch continues to marshal values through the Prelude.
   The old AOT-only inline transform closure is removed as the semantic path.
4. `crates/jet-jit/src/jit/lower_ctx.rs:13792-13827` changes in that same
   patch. It branches on the canonical TIR shape, passes an explicit empty
   input pack for a curried plan, and calls the new `jet_compute_*` entry. It
   must not keep a separate `base_arity`-indexed factory convention for the
   returned callable.

The proof is incomplete if any one of these files changes in a later card or
if the guard is widened without the lowering arm. No new syntax is involved;
the existing ratified `compute.gradient(f)` form supplies the source shape.

## Criterion 3: resident parity proof

The tri-tier implementation card must use the existing
`examples/features/tooling/compute_autodiff.jet` program and its full golden,
not a reduced direct-call-only fixture. The proof plan is:

1. Rebuild the `jet` binary before smoke tests because the Prelude is embedded
   in the compiler binary.
2. Run the example through default `jet run`. Capture stdout and the tier
   trace. Require the resident `tier1 native` path and reject any `tier0 interp`
   or deopt marker for the compute calls, including `d_loss`, `curvature`, the
   JVP callable, and VJP pull/grads.
3. Run the same source through release AOT and the forced interpreter. Compare
   stdout bytes, stderr bytes where the harness defines them, exit status, and
   the complete expected output. The required output is the existing
   `examples/features/expected/tooling/compute_autodiff.out`.
4. Exercise the escape and reentrancy cases already in the example: `d_loss`
   is called after construction, `curvature` differentiates the returned
   callable, and `vjp_run.pull` is called after the VJP frame returns.
5. Strengthen the focused parity test beside the implementation. The current
   `tests/compute_extended.rs:34-41` checks AOT/default equality and output
   fragments, while `tests/tir_support/mod.rs:160-190` has the full forced
   interpreter/AOT comparison helper. The follow-up must assert the resident
   trace and compare the complete golden across all three tiers.
6. The current tree has no `tests/jit_gaps.txt`. Move
   `tooling/compute_autodiff` at `tests/jit_corpus_gate.txt:534` out of the
   `deopt_interp` list only after the no-deopt proof. Do not invent a gap-file
   deletion. A green output comparison with an interpreter fallback is not
   criterion 3.

The evidence is byte identity plus a no-deopt trace, not only equal numerical
values. If a tier has to re-encode a default, seed, target, result-shape, or
error rule to get the comparison green, the implementation fails I9 and this
card remains open.

## Follow-up implementation cards

The following are ready-to-mint Tower payloads. They are not written to Tower
in this working session because the task rules explicitly prohibit board
writes, commits, and edits under `plugins/tower/**`. Each card must reference
#2028, and #2028 must receive reciprocal `refs` after Tower assigns card IDs.
The parent criteria are split by owner so each follow-up has one proof owner.

### Follow-up A — Prelude compute handle

Scope: implement the Prelude handle table, callback descriptor, clone/drop
lifetime, reentrant invocation, and `jet_compute_curried_new` /
`jet_compute_call_curried` in `Prelude/CoreLib/Top/Compute.rs`. Define the
adapter contract. Do not edit lowering, engine admission, or corpus state.

Exit evidence: the handle and call operation own plan state, tape state,
transform selection, continuation lifetime, and error meaning. A separate
test or proof must cover repeated calls, escaped calls, and reentrant calls.

### Follow-up B — lowering and guard narrowing

Scope: depend on Follow-up A. In one diff, implement source/TIR curried
lowering, resident safety admission, AOT emit, Cranelift marshalling, and
interpreter ambient marshalling. Use only the Prelude handle interface. Do not
create a second callable convention.

Exit criteria:

1. One mechanism carries a Jet closure across the host boundary, owned by the
   Prelude, with AOT, the Cranelift hosts, and the interpreter ambient all
   marshalling to it.
2. `compute.gradient(f)` in its curried single-argument form lowers natively,
   and the arity guard narrows in the same diff as the lowering arm.

### Follow-up C — tri-tier proof and gap retirement

Scope: depend on Follow-ups A and B. Add the resident trace/no-deopt proof,
full AOT/default/interpreter byte comparison, higher-order and VJP escape
coverage, and move the `compute_autodiff` corpus row after proof.

Exit criterion: `compute_autodiff` runs on default `jet run` without deopting
to the interpreter, with output byte-identical to AOT and to the interpreter.

## Criterion status for #2028

This design makes the decision and records the proof shape. It does not claim
implementation evidence:

1. **Design done; implementation open.** The selected mechanism is recorded
   above. Follow-up B owns the parent implementation criterion.
2. **Open; re-homed to Follow-up B.** No guard or lowering arm changed.
3. **Open; re-homed to Follow-up C.** `compute_autodiff` remains in
   `tests/jit_corpus_gate.txt:534` until the resident proof passes.

## Compiler finding

The card's statement that the resident arity guard currently refuses the
curried form is false for the present compiler. The lowerer already appends
the target list, so `compute.gradient(f)` reaches the resident predicate as
`[base, targets]`; the predicate computes zero value arguments and zero
expected values, then admits that shape. The AOT guard has the same fact:
`args.len() == 2` passes its `args.len() < 2` check and enters the existing
inline `Rc` closure arm. The compiler problem is the missing shared closure
owner and the resident whole-program fallback, not missing user syntax. This
document treats the guards as narrowing points so the follow-up keeps their
admission contract in lockstep with the new lowering/handle arm.
