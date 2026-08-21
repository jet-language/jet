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
derivative function (`docs/spec/syntax-decisions.md:5536-5547`). VJP already
returns callable `.pull` and `.grads` continuations
(`docs/spec/syntax-decisions.md:5549-5553`). The missing capability is not a
new spelling. It is the lifetime of the returned callable after the host frame
that made it.

The existing Prelude already has the semantic center. `Compute.rs` documents
that engines marshal callable arguments and typed results while one transform
dispatcher owns transform selection, scalar seeding, value detachment, and
lazy VJP state (`crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:2949-2993`).
The design extends that center to the returned callable instead of making each
engine manufacture a private closure convention.

## Current seams and the honest refusal

The current source has four relevant paths. The line numbers below are from
the working tree used for this design; the card's older line references moved
in the meantime.

| Seam | Current evidence | Design consequence |
|---|---|---|
| Resident admission | `crates/jet-jit/src/jit/safety.rs:45-80` admits compute transforms through `args.len() >= 2`, then checks the value count. There is no separate source-arity-one branch. | Add an explicit curried transform shape in the implementation diff. The resident predicate must accept exactly the lowered `[function, targets]` form and still reject a malformed shape. |
| AOT emit | `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:214-249` owns the `args.len() < 2` admission and defines `args.len() == 2` as `transform`. The later branch currently renders an inline Rust closure (`:341-407`) when this TIR shape reaches it. | Keep direct calls and curried calls distinct. The curried arm must create the Prelude handle and call the shared entry, not make AOT's `Rc` closure the semantic convention. |
| TIR lowering and JIT lowering | `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:232-318` routes the four compute methods and appends the implicit `List<Int>` target at `:280-304`. The card's older lowering guard is now the module/method guard at `:241-244`; the concrete resident arity refusal is `crates/jet-jit/src/jit/lower_ctx.rs:13659-13706`, especially `:13664-13666`. | The same implementation diff must make the lowering arm produce the curried handle shape, narrow the resident guard, and update the AOT arm. No tier may accept a new shape alone. |
| Interpreter callable model | `crates/jet-codegen/src/Codegen/TIR/eval/compute_calls.rs:281-326` already stores `EvalCallable::ComputeTransform` for `args.len() == 2`. `EvalCallable::ComputePull` and `ComputeGrads` carry output/anchor/targets/gradient type (`eval/mod.rs:1189-1239`). `call_callable` snapshots those records before dispatching (`eval/mod.rs:3337-3439`). | This is the semantic model to reproduce in the shared handle. The interpreter's current callable arena is evidence of the required lifetime, not a private compute implementation to preserve. |
| Current JIT convention | `crates/jet-jit/src/Compute.rs:397-414` stores VJP states in JIT `ComputeState`; `:968-1026` turns a curried transform into an arity-specific factory function and an env record. | Move the curried plan/tape ownership to the Prelude handle. Leave ordinary collection and system callback slots alone. `Compute.rs` becomes an i64/list/result adapter around the shared operation. |

This is an honest refusal today, not a tier divergence. The interpreter can
represent and execute the curried value; resident JIT admission and the native
lowering path do not have a cross-frame representation with shared ownership.
The design therefore fixes the seam, not just one guard.

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

Before this design, `d_loss` is an interpreter callable snapshot when the
interpreter path is used. The resident path has no shared persistent compute
handle: the native shape is refused or falls through to the interpreter. The
program still has a meaning, but its default execution is not resident.

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
| Collection callbacks | `functions_compile::lower_collection_callable_lambda` says the collection operation stays in the shared Prelude while the JIT supplies callback ABI and opaque captures (`crates/jet-jit/src/jit/functions_compile.rs:701-727`). `lower_collection_callback` binds `fn_ptr/env` and `sync_collection_captures` publishes mutable captures (`crates/jet-jit/src/jit/lower_ctx.rs:28697-28777`). | Reuse: an opaque retained environment, no stack capture, and explicit write-back. Subsumes only the idea of a durable callback adapter. It does not replace collection capture synchronization or collection operation symbols. |
| JIT spawn-site callbacks | `TCoreClosureKind::Spawn` and `OnInterrupt` carry explicit callback sites and engine adapters (`crates/jet-codegen/src/Codegen/TIR/mod.rs:4536-4553`); `first_spawn_site` locates the authoritative TIR site (`crates/jet-jit/src/jit/safety.rs:5915-5931`). | Reuse: TIR owns the site fact and the engine marshals it. Leave as-is: spawn, interrupt, reactive, and UI callbacks have event/thread/lifecycle contracts outside pure compute. |
| ParaMap and ParaFilter | `TClosureOp::ParaMap` is lowered through the existing native iterator adapter (`crates/jet-jit/src/jit/lower_ctx.rs:28834-28843`) and AOT emits the shared list operation (`crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs:3873-3877`). | Leave as-is. ParaMap's ordered/parallel scheduling and callback shape are collection semantics. The compute handle may use the same opaque callback discipline, but it must not inherit ParaMap's scheduling or private host policy. |
| Interpreter callable snapshots | `EvalCallable::ComputeTransform`, `ComputePull`, and `ComputeGrads` are cloned before invocation (`crates/jet-codegen/src/Codegen/TIR/eval/mod.rs:1189-1239`, `:3337-3439`). | Subsumes the compute-specific records and their escape behavior into the Prelude handle. Generic lambda capture snapshots and scope write-back remain the interpreter's general callable machinery. |

The current JIT callable slot is a useful adapter shape: it is an opaque
`fn_ptr` plus `env` (`crates/jet-jit/src/jit/runtime_host.rs:230-239`) and its
binding checks the function address before handing it to generated code
(`runtime_host.rs:3083-3117`). It is not the semantic owner of a compute tape.
The implementation must not copy that table into a second compute table with
different lifetime rules.

## Criterion 2: one diff, two guards, one lowering arm

The implementation card for lowering must make the admission and lowering
change inseparable. The intended diff shape is:

1. `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs` keeps the one
   `core.compute` transform lowering path and makes its source-arity-one case
   explicit. It appends the target list exactly once. The resulting TIR shape
   is `[base, targets]` for a curried transform and `[base, values..., targets]`
   for a direct transform.
2. `crates/jet-jit/src/jit/safety.rs:50-53` narrows the native predicate to
   accept the valid `[base, targets]` case with zero value arguments, plus the
   existing direct value cases. It does not turn `args.len() >= 2` into a
   blanket acceptance rule.
3. `crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:220-223` changes in
   the same patch. Its `args.len() == 2` branch calls
   `jet_compute_curried_new` and emits a function value backed by the returned
   handle; the direct branch continues to marshal values through the Prelude.
   The old AOT-only inline transform closure is removed as the semantic path.
4. `crates/jet-jit/src/jit/lower_ctx.rs:13664-13679` changes in that same
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
6. Remove `tests/jit_gaps.txt:405` for `tooling/compute_autodiff` and move its
   current `tests/jit_corpus_gate.txt:533` classification out of the deopt
   list. A green output comparison with an interpreter fallback is not
   criterion 3.

The evidence is byte identity plus a no-deopt trace, not only equal numerical
values. If a tier has to re-encode a default, seed, target, result-shape, or
error rule to get the comparison green, the implementation fails I9 and this
card remains open.

## Follow-up implementation cards

The following are ready-to-mint Tower payloads. They are not written to Tower
in this working session because the task rules explicitly prohibit board
writes, commits, and edits under `plugins/tower/**`. Each card must reference
#2028, and #2028 must receive the reciprocal `refs` after Tower assigns the
card IDs. The criteria are copied verbatim from #2028 so that implementation
work cannot silently narrow the design.

### Follow-up A — Prelude compute handle

Scope: implement the handle table, callback descriptor, clone/drop lifetime,
reentrant invocation, and `jet_compute_curried_new` /
`jet_compute_call_curried` in `Prelude/CoreLib/Top/Compute.rs`; convert AOT/JIT/
interpreter callers to thin adapters. This card owns the mechanism, not the
resident guard or final corpus proof.

Copied criteria:

1. One mechanism carries a Jet closure across the host boundary, owned by the
   Prelude, with AOT, the Cranelift hosts and the interpreter ambient all
   marshalling to it rather than each holding a convention.
2. `compute.gradient(f)` in its curried single-argument form lowers natively,
   and the arity guard that currently refuses it is narrowed in the same diff
   as the lowering arm that handles it.
3. `compute_autodiff` runs on default `jet run` without deopting to the
   interpreter, with output byte-identical to AOT and to the interpreter.

### Follow-up B — lowering and guard narrowing

Scope: implement the one-diff shape in the criterion-2 section: source/TIR
curried lowering, resident safety admission, AOT emit, and Cranelift call
marshalling to the Prelude handle. This card may depend on Follow-up A but may
not create a second callable convention.

Copied criteria:

1. One mechanism carries a Jet closure across the host boundary, owned by the
   Prelude, with AOT, the Cranelift hosts and the interpreter ambient all
   marshalling to it rather than each holding a convention.
2. `compute.gradient(f)` in its curried single-argument form lowers natively,
   and the arity guard that currently refuses it is narrowed in the same diff
   as the lowering arm that handles it.
3. `compute_autodiff` runs on default `jet run` without deopting to the
   interpreter, with output byte-identical to AOT and to the interpreter.

### Follow-up C — tri-tier proof and gap retirement

Scope: add the resident trace/no-deopt proof, full AOT/default/interpreter
byte comparison, higher-order/VJP escape coverage, and remove the durable
`compute_autodiff` gap classification. This card closes only after the
mechanism and lowering cards are integrated.

Copied criteria:

1. One mechanism carries a Jet closure across the host boundary, owned by the
   Prelude, with AOT, the Cranelift hosts and the interpreter ambient all
   marshalling to it rather than each holding a convention.
2. `compute.gradient(f)` in its curried single-argument form lowers natively,
   and the arity guard that currently refuses it is narrowed in the same diff
   as the lowering arm that handles it.
3. `compute_autodiff` runs on default `jet run` without deopting to the
   interpreter, with output byte-identical to AOT and to the interpreter.

## Criterion status for #2028

This design makes the decision and records the proof shape. It does not claim
implementation evidence:

1. **Open — design decided, implementation absent.** The chosen mechanism and
   tier marshalling table are above; no Prelude entry points have been added.
2. **Open — implementation absent.** The same-diff guard/lowering shape is
   specified above; no guard or lowering arm has been changed.
3. **Open — implementation absent.** The resident, byte-identical proof plan
   is specified above; `compute_autodiff` remains an implementation follow-up.
