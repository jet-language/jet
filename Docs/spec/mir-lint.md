# MIR Lint

This page is the compiler contract for MIR Lint, the typed checker over the
canonical MIR program. It is a second checker. It does not reuse the semantic
checker or the optimizer's legality verifier, so a lowering or optimization bug
is caught before an engine generates code from the wrong program. The Jet
compiler implements it in `Compiler/JetOptimizer/Source/Verification/Lint.jet`;
its rule-by-rule fixtures are `Compiler/JetOptimizer/Tests/LintFixtures.jet`
(assembled by `Compiler/JetOptimizer/Tests/run-lint-fixtures.mjs`). The
archived Rust bootstrap compiler does not carry the lint.

The owned-payload and owned-read Phi fixtures observe the stored destination
through `lf_borrowed_len`, a borrowed `core.builtin::list_len` receiver. This
observation must not consume the destination before its final owner is dropped.

## When it runs

- Once on the program the optimizer returns, on every compile and in every
  build mode, as the second half of the optimizer's one MIR checker
  (`Compiler/JetOptimizer/Source/Pipeline.jet` `mir_pass_check_wave`): per
  function, the legality rules (`Verification/Legality.jet`) run first and the
  lint rules run when they hold. That check runs once, on the final MIR; the
  passes are proven per rule by their own fixtures. The checker's own
  fixtures (rule order, program order, keep-going scan) are
  `Compiler/JetOptimizer/Tests/CheckFixtures.jet`
  (`run-lint-fixtures.mjs --check`).
- After every optimizer pass, and on the lowered input, when full MIR
  verification is on (`optimize_mir_program(program, full_verification: true)`,
  `JET_MIR_LINT_EVERY_PASS=1`), so a failure names the pass that broke the
  program.
- A failure is an internal compiler error. The report names the pass that
  produced the program, the rule family, the function, and the offending
  operation.
- The lint is linear in the size of the program. Ownership and the safety
  families share one forward dataflow over flat bit rows per function, with no
  per-block copies of maps. The safety families add work proportional to a
  function's views, writing closures, and task-group loans, which are a handful.

Native stop-point lowering and legality verification use the same checked
Prelude route effects. A route marked `Panic` in
`Compiler/JetFoundation/Source/Registry/PreludeEffects.jet` needs `Propagate`
before its result is bound or consumed; returning a `Result` is not a stop.
Semantic Prelude operations gain polls only from checked stop facts; existing
user, indirect, Core, callback and general-call stop classification is retained.
Checked slice routes carry `Panic` because bounds may fail dynamically.
Payload access selected by an enum's own discriminant remains an invariant:
record-import glue selects the matching tag before invoking a payload getter,
so those getters do not introduce an independent user-visible stop point.

The shared `mir_binary_can_trap` predicate consumes the operation and checked
overflow policy: `Trap` on arithmetic or shifts is stopping, while bitwise and
proven-total (`Unchecked`) operations are not. Exact Euclidean division and
remainder carry checked `Panic` route facts for zero-divisor failures.
The Rust seed represents this checked policy through `MirBinaryDispatch`:
`Codegen/TIR/routes.rs::binary_route` selects a trapping Prelude row when
overflow is checked (and for division/remainder/power that can fail for other
reasons), otherwise a primitive operation. Its `mir_binary_can_trap` consumes
that dispatch's checked `Panic` effect, without reconstructing symbol names.

The shared stop predicate also consumes the checked operand type for unary
negation and the exact accessed place for projected operations. Fixed-width
`Neg` may trap (including transparent tags, ranges, and quantities); exact
integer negation, floating negation, `Not`, and sema-proven-total `NumericCast`
do not gain stop points. A projected read, move, write, or replacement checks
its getter's `Panic` fact; only a write or replacement also checks its setter.
Total Map routes stay total. Merely registering a projected place does not
evaluate it. `ScopeExit` can forward a stop but is not independently
Panic-capable; `RequireStop` is explicitly stopping in both Rust and Jet.

`tests/fixtures/mir_stop_predicate.tsv` is the common operation/fact table for
the Rust `shared_stop_predicate_table` test and the Jet FrontFeed predicate
witness. Set `JET_STOP_PREDICATE_RESULTS` when exercising the Rust test, then
use `run-front-feed.mjs --stop-predicate --rust-predicate-results PATH --probe
BINARY` to compare every input/result byte against the exercised Jet predicate.
Preparation with `--assemble` does not claim parity or a successful golden.
Shield and transaction masks suppress cancellation `Check` polls, not
`Propagate` after Panic-capable operations. The runtime delivery contract is
`Compiler/JetRuntime/Source/Scheduler.jet::scheduler_delivered_status`: Shield
retains panic/fault statuses while deferring cancellation/deadline statuses.
Legality rejects cancellation checks and non-Panic propagation under a mask.

Normal-exit cleanup can publish a Panic. Its current action is consumed before
the call, and its propagation edge runs only the remaining registered actions,
once each. Already-run actions are not replayed; sibling exit paths retain
their own registrations. Propagation cleanup does not introduce another poll:
a second panic after any delivered stop is fatal in the runtime. Legality
rejects a stop edge that reuses a consuming call's moved input or deferred thunk.

Every compiler-generated propagation cleanup action has an explicit
`MIRScopeKind.StopCleanup` owner. It is not a user `Shield`: native entry reads
the actual raw `jet_rt_session_status`, passes that delivered kind to
`jet_rt_session_cleanup_enter`, and holds the returned opaque flags token.
`jet_rt_session_cleanup_leave` always restores that exact token, including
zero, without clearing or taking a stop record. Only the genuine old kind is
masked; entry with zero masks nothing, so the first new stop remains visible.
Each action enters separately, allowing a later action to capture the current
delivered kind. Runtime task/root provenance is never inferred by the compiler.
Legality requires one static entry, matched LIFO leaves on all live exits, and
no recursive poll within that owner. Both interpreters and Rust emission reject
this native-only scope rather than treating it as an inert lexical scope.

## Type compatibility

The lint compares types with one relation, *compatible*. Each side is first
reduced to its representation class. Non-nominal tags, `InlineRange`,
`Quantity`, and `Shared` reduce to their inner type. `Int` and every `IntN`
form the integer class. `Float` and `Float32` form the float class. `List` and
`FixedList` form the list class. `Fn` and `SendFn` form the callable class. A
string view (`View<str>`, `ViewMut<str>`) is in the String class: a
string-view local reads as `String`, and a view fills a `String` slot (an
`Option<String>` payload, a `String` parameter) as the same text handle.
These forms always match:

- a type variable: a nameless-argument `Apply` whose name is a generic
  parameter or associated type declared anywhere in the program, `Self`, or a
  dotted projection;
- `Never`, `Measure`, `TraitObject`, `Union`, and nominal names declared as
  `Alias` or `Distinct` (coercion targets and carriers checked elsewhere);
- the two unit spellings, `Unit` and the empty tuple, with each other.

Two `Apply` types match when their nominal IDs agree, or their leaf names agree
(module qualification differs between rows), and their arguments match
pairwise. Structural kinds match their own kind with compatible children.

Unqualified import module IDs use the checked canonical module key. Foreign
binding aliases obtain that key from their CFFI import-link row, not by guessing
the generated cache or foreign module's display name. Ordinary module imports
keep exact-key, owning-child, and unique-name resolution.

## Rule families

Every rule belongs to one family. The family name is part of the ICE text.

| Family | Rule |
|---|---|
| `value.typed` | Every operand value has exactly one defining row with a type. An instruction result's type is compatible with its value row. An instruction without a result carries no type. |
| `type.well-formed` | Every type in a value, local, place, or parameter row satisfies its kind rule below and its stored layout matches its kind. |
| `operation.typing` | Each of the 62 operations satisfies its operand and result rule below. |
| `semantic.typing` | Each of the 44 semantic operations satisfies its rule below. |
| `call.signature` | A direct call names its callee by function ID. The argument count fits the callee's parameters, each plain argument is compatible with its parameter, and the result is compatible with the callee's return type. |
| `enum.variant` | Enum construction, tests, and payload reads name a declared variant of the owner's declaration, with the declared payload arity and compatible payload values. An owner the program does not declare (a Core enum) is not checked. |
| `struct.field` | Struct construction names fields declared on the struct, without duplicates, and supplies every field that has no default and is not computed. Field reads and field paths name a field declared on some struct or in the program's field rows. |
| `terminator.typing` | Each of the 8 terminators satisfies its rule below. |
| `ownership.move` | No value or place is used after it was moved on every path to the use, a value is consumed at most once, and only owned values are consumed. A write to a moved place or its parent restores it. A Bool local that is provably false (or true) on every path removes the branch edge it guards, and the same Bool facts are followed along each path out of a move: a place use is a second move only when some feasible path reaches it from the move without a restoring write (a cleanup move guarded by the drop flag the move cleared runs only where the place was refilled). An owner's remainder drop (a whole-owner `MovePlace` whose every user is a releasing `Drop`: its scope-end cleanup) releases only the fields still in place, so it is a second move only after a move of the owner itself or of a prefix, never after a move of one of its fields (Foundation `mir_remainder_drop_results`, the classification MIR legality and last-use selection read). Failure and unwind drop edges are entry points. |
| `effect.row` | Every effect in a direct callee's solved row is in the caller's solved row, itself or through a parent effect (`FS` covers `FS.Write`), unless the caller's row is maximal. |
| `alias.exclusive` | A place that one operation's argument writes, directly or through a closure argument that writes a captured place, overlaps no other argument, receiver, or captured place of that operation. A closure that writes a captured place is not used after another access reached that place while the closure was live. A place lent to a task-group child does not overlap another live child's loan when either writes, and the parent does not read a place a live child writes. |
| `view.valid` | A view is not used after a path containing its owner was moved, written, or resized. A place lent to a task-group child does not change before the group's scope exits or the child's handle is joined. A returned view points only into a parameter the caller keeps (inside the declared `from` sources, when there are any), a capture, or a static, never into a local, an owned parameter, or a temporary. |
| `send.boundary` | A sendable closure (`SendFn`) captures no value or place whose type holds a `Cell`, a `View`/`ViewMut`, or a non-sendable `Fn`. Its place captures are allowed only for a task-group child, or for reading in a parallel worker. A write capture, or a non-Copy value read straight out of a place, is allowed only for a task-group child. A channel send carries no such value. |

## Safety families

`alias.exclusive`, `view.valid`, and `send.boundary` are Jet's independent
second proof of exclusivity, view validity, and sendability. On the Rust path,
rustc's borrow checker and `Send` checks re-prove them for the generated code.
The native backend has no such re-check. These families use only facts the
MIR already carries, never sema's alias graph:

The callee index retains capture access and return-view provenance as well as
ordinary parameters and solved effects. Function, type, value, place and block
lookups use raw `U64` IDs; decimal identities belong only to the separate
String-keyed safety/path tables. A physical transfer demands an owner only
when its `Yes` consumption fact has `requires_owner: true`; a `Through`
carrier transfer does not add that demand.

- **Places.** Two places overlap when they share a root and no projection
  separates them. Different fields are disjoint, and so are index projections
  that name two distinct `Int` constants. Every other index, range, payload, or
  dereference may overlap.
- **Views and SSA generations.** A view origin records every owner in the
  current direct/indirect callee's `return_view_provenance`; builtin producers
  derive their owners from every operand place. A narrowed projection is
  conservatively checked as its owner's whole place. Borrowed carrier origins
  propagate to a fixed point through def-use edges, including arbitrarily long
  Move chains and every Phi input. Phi generations take only their selected
  predecessor's active/stale state. A holder retains its own generation:
  re-evaluating an origin in a loop cannot refresh an older stored window.
- **Flow joins.** Move and known guard facts use must-intersection. Active
  origins, stale generations, holders and outstanding child loans form a
  separate may-flow and use union: a fact on any incoming edge survives.
- **Sendable carriers.** Union members and Alias/Distinct targets are traversed
  recursively, as are nominal fields and variant payloads. Shared data and
  SendFn values use their explicit sendable representation.
- **Writing closures.** A closure that is not sendable and captures a place
  through a write capture parameter keeps a write of that place live. Any other
  read or write of an overlapping place makes it stale, and a later use of the
  closure, or of a local holding it, is an `alias.exclusive` failure.
- **Tasks.** A spawn is a `CoreClosureCall` of kind `Spawn`. It is a task-group
  child when its values carry the group (three values: group, site, label).
  Each place the child captures, or reads straight out of as a non-Copy value,
  is lent until its own TaskGroup scope exits or its uniquely identified task
  handle is joined. A nested unrelated group cannot clear the loan; joining a
  replacement handle cannot clear the overwritten child's loan. An ambiguous
  handle conservatively stays outstanding until its group's scope exits.
  A parallel worker is joined inside its `para_*` call.
- **Copy facts.** A value whose ownership mode is `Copy`, or whose layout is a
  scalar ABI, is copied out of its place and aliases nothing.

The hand-built mutation controls in `LintFixtures.jet` bypass sema entirely:
each pair feeds clean/planted MIR directly to the independent lint. The named
diagnostics identify the language checks these MIR shapes back; these are not
receipts from compiling a checker with sema checks deleted. The programs are
`projected_write_window_overlap`
(E0204), `view_owner_changed` (E0212), `ownership_call_consume_provenance`
(E0121), `task_capture`, `taskgroup_overlapping_write_borrows`, and
`taskgroup_lent_owner_moved` (E1101), `local_cell_crossing`,
`string_view_task_capture`, and `unsendable_send` (E1102), and
`view_outlives_owner` (E2305). The fixtures also cover the closure-laundered
mutations of #4621.
The regression matrix additionally covers one-predecessor invalidations and
loans, Phi-selected generations, twelve-hop Move chains, multiple declared
owners, Union/Alias/Distinct sendability, nested groups and overwritten handles.
`run-lint-fixtures.mjs OUT CHECKER` executes the assembled unit and writes a
strict receipt with source hashes, checker hash, expected/passed case counts
and failures. A dirty source tree records a null admitted head; only a clean,
exact-head receipt can be admitted by the standing gate.

Two sema rules have no MIR counterpart, so the lint cannot back them:

- E1113 (a write through a frozen value). `freeze(x)` lowers to an ordinary
  owned copy, and every capture of it copies again, so a later write races with
  nothing in the MIR. The lint needs a frozen fact, such as a `Freeze` copy
  fact, before it can prove the rule.
- E0204 on a scalar. In `both(&x, x)` with an `Int`, the read argument is
  copied out before the call. The MIR is memory-safe, and sema's rule is
  stricter than that.

Source-owned Core declarations remain in sema's registry, but are excluded from
the executable MIR Core-call projection, including comptime and REPL fragments.
Their calls lower through the owning source function. MIR host-route verification
and emission still reject a source-owned declaration used as a host call.

Shared-guard `map` and `split` consume their source guard. Lowering a
named guard therefore emits the same owned-place move as task consumption;
the guard's cleanup flag clears before its scope exits. A plain `ReadPlace`
is not a substitute for this ownership fact. `wait` borrows its guard: the
kernel reacquires the same lock, so the guard stays live after the call.
A fully moved destructure temp also clears its cleanup flag; completeness is
checked against the original binding list, without a copied moved-field ledger.

## Type kinds (23)

| Kind | Rule |
|---|---|
| `Int`, `Float`, `Bool`, `String`, `Char`, `Float32` | Leaf; layout matches the kind. |
| `List(T)` | `T` well-formed. |
| `Map{K, V}` | `K` and `V` well-formed; `K` is not callable. |
| `Shared(T)` | `T` well-formed. |
| `Option(T)` | `T` well-formed. |
| `Result{ok, err}` | Both well-formed. |
| `Fn(sig)` | Parameters and return well-formed; call metadata and the parameter contract, when present, have one row per parameter. |
| `SendFn{params, ret, conventions}` | One convention per parameter; all types well-formed. |
| `Apply{name, args}` | Non-empty name; arguments well-formed. |
| `TraitObject(bounds)` | At least one bound. |
| `Tuple(fields)` | Field names are unique; fields well-formed. |
| `FixedList{elem, len}` | `elem` well-formed. |
| `IntN{signed, bits}` | `1 <= bits <= 64`. |
| `InlineRange{base, lo, hi}` | `lo <= hi`; `base` is an integer. |
| `Tagged{marker, inner}` | A user marker has a name; `inner` well-formed. |
| `Union(members)` | At least two members, each well-formed. |
| `Quantity{base, dimension}` | `base` is numeric. |
| `Measure(m)` | Leaf. |

## Operations (62)

"Result `T`" means the instruction result is compatible with `T`. "Bool"
means the Bool class.

| Operation | Rule |
|---|---|
| `Parameter{index}` | `index` names a parameter; result is its type. |
| `Capture{slot}` | `slot` names a capture parameter; result is its type. |
| `Global{name}` | Name is non-empty; has a result. |
| `Phi{incoming}` | At least one edge; each edge block is a predecessor; each value is compatible with the result. |
| `ReadPlace(p)`, `MovePlace{p}` | Place exists; result is the place type. |
| `WritePlace{p, v}`, `ReplacePlace{p, v}` | Place exists; `v` is compatible with the place type; no result. |
| `InitializeUninit{p}` | Place exists; no result. |
| `Copy{v, fact}`, `Move{v}`, `AttachTag{v}` | Result is the type of `v`, except `Copy{v, view_materialize}`, whose result is the owning collection `v`'s copy kernel builds (`View<str>` → `String`, `View<T>`/`[T]` → `[T]`). (MIR legality, not lint, checks `fact` against `v`: `scalar` needs a Copy-ABI value, `view_materialize` a view with a copy kernel; #4318.) |
| `TraitBox{v, target}` | Has a result. |
| `Constant(c)` | Int → integer, Float → float, Bool → Bool, Char → Char, String → String, Unit → unit, for a result whose type is not nominal. |
| `Unary{Neg, v}` | `v` is numeric; result is `v`'s type. |
| `Unary{Not, v}` | `v` is Bool or an integer; result is `v`'s type. |
| `Binary{op, l, r}` | Comparisons (`== != < > <= >=`) produce Bool. Primitive `&& ||` take and produce Bool. Primitive arithmetic and bit operators take compatible operands (shifts: integers) and produce the left operand's type. |
| `BuildString` | Result is String or nominal. |
| `BuildList{values}` | Result is a list or nominal; without a trait coercion each element is compatible with the element type. |
| `BuildMap{entries}` | Result is a map or nominal; keys and values compatible with a map result. |
| `EnumIs{owner, variant}` | Result Bool; `enum.variant` on owner. |
| `EnumPayload{owner, variant, index}` | `enum.variant`; `index` is below the payload arity; result is that payload's type. |
| `OptionIsSome`, `ResultIsOk` | Subject is an Option / Result; result Bool. |
| `OptionValue` | Subject `Option(T)`; result `T`. |
| `ResultValue{ok}` | Subject `Result{T, E}`; result `T` or `E`. |
| `PatternCapture` | Has a result. |
| `PatternMatched` | Result Bool. |
| `ProjectMembers{members}` | At least one member; each is a declared field; no duplicates. |
| `Index{kind, base, index}` | List-like kinds take an integer index; a `List(T)` read yields `T`; a `Map{K, V}` index is `K` and yields `V` or `V?`. |
| `Slice` | `start` and `end` are integers. |
| `Range{start, end}` | `start` compatible with `end`. |
| `Field{field}` | `struct.field`: field declared; result is its type. |
| `Struct{type_id, fields}` | `struct.field` against the declaration. |
| `Enum{type_id, variant, args}` | `enum.variant` against the declaration. |
| `Tuple{fields}` | Result is a tuple with one slot per field. |
| `Present{v}` | Result `Option(T)` with `v` compatible with `T`. |
| `Absent` | Result is an Option. |
| `ResultOk{v}`, `ResultErr{v}` | Result `Result{T, E}` with `v` compatible with `T` / `E`. |
| `Convert{conversion}` | Has a result; a numeric cast goes between numeric types. |
| `Call` | `call.signature` and `effect.row` for `User`, `Associated`, and `Method` callees. |
| `IndirectCall{callee}` | Callee is callable; argument count matches its parameters when there is no spread or variadic. |
| `Closure{function, captures}` | Function exists; one capture per capture parameter; result is callable. |
| `PtrFromAddr{addr}` | `addr` is an integer. |
| `Deref`, `Todo`, `Never` | Operands defined. |
| `RawAddressOf{p}`, `AddressOf{p}` | Place exists. |
| `CoreCall{call}` | Core row exists. |
| `Semantic(op)` | `semantic.typing`. |
| `LoopRangeInit{start, end, step}` | `start` compatible with `end`. |
| `LoopRangeHasNext`, `LoopIterHasNext` | Result Bool. |
| `LoopRangeValue`, `LoopRangeAdvance`, `LoopIterInit`, `LoopIterValue`, `LoopIterAdvance` | Operands defined. |
| `ScopeEnter{scope}`, `ScopeExit{scope}` | Scope exists. |
| `Drop{v}` | Operand defined; `ownership.move`. |

## Semantic operations (44)

All operands of every semantic operation are defined (`value.typed`).

| Operation | Rule |
|---|---|
| `LayoutCompare` | Result Bool. |
| `RequireStop{condition}` | Condition, when present, is Bool. |
| `StructLiteral` | `struct.field` against the declaration. |
| `ColumnarRead{index, column}` | Index is an integer; column is a declared field. |
| `CellGuardProject`, `SharedGuardMap`, `SharedGuardSplit` | Every path field is declared. |
| `CarrierFact{field}` | Field is declared. |
| `BuiltinMethod{aggregate_fields}` | Every aggregate field is declared. |
| `OptionLift2{function}` | Function is callable. |
| `OverflowOption` | Result is an Option (`checked_*`) or an integer (wrapping, saturating, trapping, and rotate routes). |
| `NumericMethod`, `NumericBinaryMethod` | Receiver is numeric. |
| `HostBorrowCallback{callable}`, `CCallback{lambda}`, `HTTPRouterRegister{handler}` | The named value is callable. `ClosureMethod{receiver}` names the collection or carrier the closure route runs over, not the closure. |
| `CoreClosureCall{closure}` | Closure, when present, is callable. |
| `GCEdit{index}` | Index, when present, is an integer. |
| `TypedTextInterp` | One more literal than holes; one HTML proof flag per hole. |
| `PluginInvoke` | One argument per signature parameter. |
| `AmbientInput{prompt}` | Prompt, when present, is a String. |
| `DataEntriesToMap{local}` | Local exists. |
| `CursorTakePattern`, `ReaderTakePattern` | Receiver place exists. |
| `MathBuiltin`, `PreciseBuiltin`, `Print`, `ReflectOf`, `LayoutLiteral`, `SharedGuardWait`, `ConditionNotify`, `AllocNew`, `StaticPreludeCall`, `DecodeUnder`, `HardwareCall`, `TextPatternMatch`, `BinaryPatternMatch`, `HandleMethod`, `TaskGroup`, `Select`, `PolicyFunction`, `InterruptFunction`, `HostCall` | Operands defined; Prelude route shape is the legality verifier's contract. |

## Terminators (8)

| Terminator | Rule |
|---|---|
| `Jump`, `Continue` | Target block exists. |
| `Branch` | Condition is Bool; both targets exist. |
| `Switch` | Subject defined; each arm condition is Bool; every target exists. |
| `Return{value}` | A value is compatible with the function's return type; no value only in a generator, a function returning unit or `Never`, or a fallible function whose failure carrier is `Result` with a unit success (its implicit `Ok(())`). |
| `Yield{value, resume}` | The function is a generator; the value is compatible with its item type; the resume target exists. |
| `Break` | Target exists. |
| `Unreachable` | No operands. |
