# TIR semantic core

This page is the compiler contract at the boundary between semantic analysis and
executable lenses. It covers totality, the read-only fact channel, lowering,
and coverage obligations. The implementation is
`Compiler/JetFoundation/Source/TIR/TIR.jet` and the exhaustive matches of
its consumers.

TIR is a semantic contract, not a commitment to one permanent
representation. A port may change representation, but it may not move semantic
choices into an executable lens or create a second meaning.

## One semantic core (R12 / D-ONECORE1=A)

There is one structured semantic core. Every executable lens—native AOT,
interpreter, and web—consumes that core exhaustively. Surface sugar
expands during lowering or an earlier sema rewrite into core nodes that those
lenses already handle. A coverage miss is an internal compiler error, never a
fallback.

TIR is post-sema and total: every fact codegen needs is carried concretely on
the node, never re-inferred through a fallback. The emitter pattern-matches
those fields and formats output; it does not re-run semantic decisions such as
expression typing or overflow classification. `tir_covers` is the coverage
proof for reachable functions.

Restoring a codegen function snapshot consumes its already-owned non-Copy
fields into the builder; only scalar Copy facts are read. Saving an enclosing
function's independent snapshot remains a distinct operation, not a borrowed
view that could be invalidated when the builder begins a nested function.

## Read-only fact channel

The frozen TIR exposes one typed, borrowed `TFactChannel` view. It projects facts
already selected by sema from existing TIR carriers; it is not a side table, a
third IR, or a third executable lens, and it adds no per-node heap allocation.
The initial fact/carrier contract is:

| Fact | TIR carrier | Missing fact |
|---|---|---|
| Type | `TExpr.ty`, typed locals and parameters, `TFunc.ret` | Keep the conservative typed operation. |
| Integer bounds | `Type::integer_range`, exact integer literals, `TNumericOp::InlineRange`, fixed-list proof | Keep the checked range or index operation. |
| Exclusivity | sema `AccessConvention` lowered to `TCallArg` borrow flags and `Borrow` nodes | Keep shared or unknown memory dependencies and wrappers. |
| Purity | sema `Func.is_pure` and `AutoVectorizationFacts.effect_free_body` | Do not apply a reorder or vector hint. |
| Comptime value | sema comptime binding facts lowered to `TExprKind::CtLit` | Keep runtime evaluation or serialization. |

Generic call evidence belongs to the caller's lexical scope, not the callee's
registration row. Both local and cross-module calls must carry that scope's
type parameters and canonical trait bounds into inference, including explicit
type arguments. Lowering consumes the checked substitutions and bound evidence;
it must not reconstruct a forwarded parameter's bounds from its spelling.

Compile-time fragment lowering binds direct nominal `$layout`, `$name`, and
`$fields` reads before ordinary field lowering, including reads inside reached
helper functions. These values use the fragment caller's selected target and
the checked nominal declarations through the shared target-aware reflection
builder; a type name is never an evaluator local. Lexical reflected-value
bindings still shadow nominal subjects. Jet-authored sema folds the same reads
to typed compile-time literals before MIR lowering.
Raw-pointer fields carry one address in the selected target's layout: their
size and stride use its pointer width, and alignment uses its pointer
alignment. The shared layout producer never needs the pointee's physical
layout to project these facts.

Fixed-array-to-list widening is a checked call-argument fact. MIR retains it
even when the argument comes from a place, so native emission widens the
array before lending the callee its growable-list borrow.

Cross-file generic calls lower in the declaring file's context, with canonical
nominal identities at their boundary. Callable parameter coercions use the
checked concrete signature and effective callable-return carrier, never an
unspecialized `fn(T)` template or a raw source return type. A generic target
without a complete checked instance is a lowering invariant failure;
it cannot fall back to the template's ordinary function name. The executable
example is [`package_members`](../../Examples/features/modules/package_members/run.jet).

Core runtime-carrier type imports follow the authoritative Core export ledger
even when no source nominal declaration exists. Import aliases preserve the
carrier's builtin identity, and checked constructors publish that identity
as their lowering receiver. A user module cannot acquire Core ownership merely
by using a Core-like name.

Compiler-created reflection values retain ordinary checked nominal declarations,
including their enum tags and payloads. A compiler-owned name does not imply a
native runtime carrier: reflection enums without one are declared by the artifact
through the same enum emission path as source declarations.
Typed variant comparisons resolve their owner from the checked subject's enum
declaration and require membership in that declaration's canonical variant menu.
This includes compiler-created reflection enums, not only source and runtime enums.

Checked function parameters and explicit nominal construction type arguments
publish their resolved identities on the AST consumed by lowering. Body locals,
patterns, and specialization keys must not reinterpret the same declaration
through different source spellings.
Both executable lenses consume the channel read-only. A missing field means
“not proven”; it never authorizes codegen to re-derive sema policy. An optimized
lens may derive a private SSA form internally, but that form is not a third
semantic representation or source of truth. Every TIR construct remains
exhaustive for AOT, interpreter, and web.

## Lowering rules

Engines must not special-case these surface forms:

| Surface | Core form | Authority |
|---|---|---|
| `freeze(x)` | `Clone`, `MaterializeView`, or `ExplicitCopy`, selected by the sema-approved source type; frozen provenance stays in capture metadata. | D-CONC-FREEZE1=A |
| `^place` in a value position | `Move` of the checked place; a non-Copy field projection becomes MIR `MovePlace`, leaving that field empty for remainder cleanup. The checker must not materialize a copy of the operand or erase its take provenance. | D-COPY-DEFAULT1=A |
| `task ^name { … }` | The existing task lambda with an explicit consuming capture; the task-crossing prover owns legality. | D-CONC-FREEZE1=A |

The freeze example is a lowering rule, not an engine-side capture check. A
self-host implementation must carry the same already-proved ownership facts to
each lens.

A projected owning `Move` supplies a root-name hint for leaf cleanup before MIR
emission. Name matches without their own `Close` split stored struct/tuple
ownership into live flags; ordinary roots retain atomic cleanup, and containers,
dynamic owners and nominal `Close` leaves remain indivisible. Shadowed same-name
`Close` roots remain atomic too. A move clears every covered leaf flag;
reinitialization sets those same flags. A move below a split root that covers no
registered leaf is explicitly refused, including a move inside an atomic leaf.
An owned nominal record whose stored fields are all Copy still registers one
atomic ownership leaf. Structural cleanup becoming empty must not erase its
move/live flag or make moving that whole field look like an unsupported move.
Cleanup consumes only the live remainder, including on early returns. An actual
projected move from an atomic root with its own `Close` is refused at the move
with a type-named internal error, not at registration. The consuming receiver
inside its own `Close` body instead gets structural remainder cleanup, never
another call to its own `Close`.

Inline blocks lower their statement tails through the canonical statement
lowerer. A tail that terminates control flow, including an exhaustive match
whose arms return, has no reachable value; its inert MIR placeholder is Unit.
A reachable non-expression tail is valid only for a checked Unit block.
If and exhaustive-match joins with no incoming continuation edge are sealed
as unreachable, not exposed as an open continuation after all arms terminate.

Native reference carriers (`View`, `ViewMut`, `Pin`, and the internal
allocator-view tag) do not own their referents. Their synthetic scope cleanup
must not read or move the reference after its last checked use: doing so would
extend the native borrow and reject a legal later reset or owner borrow. The
allocator or collection remains responsible for dropping its owned payload.
A discarded root-place read likewise needs no owned SSA copy; projected
accesses and retained context operands still keep their checked evaluation.

An emitted record retains each declaration type parameter even when none of
its stored fields uses it. Such parameters are carried by a zero-sized
`PhantomData` field, initialized by every record construction path; the marker
is not a source field or a serialized/displayed value. Native/runtime-owned
records retain their existing layouts rather than receiving emitter fields.
Canonical source-owned regex, web-table, and HTTP message records retain their declared field
layouts and generated local traits. A same-leaf native carrier is not an ABI
alias for such a record; all type projections use the source-owner decision.
Checked-text interpolation carries each hole's checked generic encoder arguments
through compile-time validation and runtime lowering. Its dynamic `from`
boundary invokes the declared checker once and preserves the original error;
only a successful outcome wraps the original text in the nominal type.
Imported function signatures retain each declared record's lexical owner through
nested list and generic types, while function-generic parameters keep their
lexical bindings. Generated C handle and glyph records use that same nominal
identity; equally shaped caller records are not interchangeable.
Arrow import preserves the checked batch owner and row type, and query consumes
that owner. Canonical Core-data query heads select the existing native Query
operation rows: collection remains the checked `[T] !DataError` materialization
boundary, not a source-record method shim.
Builtin `ParseError` is a valid typed failure domain for parsing operations.
Locally matching its outcome preserves the carrier and binds success/error
payloads; recognizing that domain does not declare a conversion into default
`Err`. Ordinary fallible bindings still propagate, so diagnostic-policy probes
handle a decoder call directly instead of first binding only its success value.

Synthesized integer literals use the same native-carrier boundary as parsed
literals. `IntLit` and the IR's `Int` constant carry only signed 64-bit values;
an exact `Int` outside that range uses `CtLit(BigInt)` and the IR's BigInt
constant with its exact decimal spelling and integer bounds. This includes
embedded comptime integers, prepared facts, Fraction numerators, unit values and
scales, and reconstructed fixed-allocation sizes. Fixed-width destinations
retain the checked numeric conversion. An out-of-range `Int` is never truncated
or silently changed to another constant kind.

Pattern payload and field types instantiate the checked subject's owner
parameters before binding locals; a declaration's bare `T`/`E` does not escape
into its concrete match. Associated types in a trait declaration use `Self::X`.
Pattern-generated structural capture tuples use the canonical tuple type
instance and field label, exactly like ordinary tuple field reads; they do not
require a nominal declaration. Literal-field tests, bound fields (including
read/write windows), capture tuples, and dispatch-subject projections share
that field-identity path in both compiler implementations.
Indexed and optional payload projections use that same resolver. It strips
checked tags and the single-argument `SharedGuard`, `CellReadGuard`,
`CellEditGuard`, and `Pin` window carriers, but not arbitrary generic wrappers,
`Shared`, `Option`, or `Result`. Tuple labels and unsigned decimal positions
identify canonical tuple-instance fields; applied record fields identify their
nominal declaration, not their generic instance. Named enum payloads retain
the variant's field owner key. Numeric math-lane projections resolve only for
`F32x4`, `F64x2`, `Vec2`, `Vec3`, and `Vec4`, preserving the stored `x/y/z/w`
field order. Alias, distinct, and unit-family rows do not acquire stored fields.
An unsupported checked owner reports the member, function, original owner
kind/rendering, and normalized owner kind/rendering rather than hiding the
missing semantic row.
The field-position scanner parenthesizes typed byte literals in conditions
(`digit < (U8{48})`), so both bootstrap parsers distinguish the literal braces
from a conditional body without changing unsigned-position semantics.
Structural Comparable/Equatable records may derive Rust `Eq`, `PartialOrd`,
and `Ord` only when every field supports that native order. A custom capability
implementation, floating field, unordered carrier, or native opaque carrier
does not gain a different derived ordering silently.


Shared collection kernels narrow owned exact `Int` arguments through
Foundation's borrowed `JetInt::to_native_i64` ABI check on every host. The seam
belongs to the carrier, so it also works when Foundation is embedded beneath
a generated program's crate root; the flat primitive-conversion fragment has
no dependency on that root's module layout. A native-width overflow uses the
registry-backed `E3010` stop; an include host supplies only the stop transport,
not another conversion rule or runtime dependency.

Generic native callback kernels share one argument-ownership adapter across
static, builtin-receiver, and closure-receiver routes. Split aggregate builders
receive owned vectors; indexed builders receive an owned index and a borrowed
item. The adapter translates those host conventions to the checked callable's
read/write/move parameters rather than passing the boxed Jet callable directly
to a Rust `FnOnce`/`FnMut` parameter.

An eager collection adapter retains the checked failure contract of a callable
value, just like a direct call to that value. A sequence's `map` or `filter`,
and a list's one-argument sort key, therefore count a `fn(T) -> U` parameter's
default `Err` route as a failure source for the enclosing function. Lowering
uses the corresponding fallible kernel rather than declaring the enclosing
function infallible while emitting a fallible call. Literal lambdas keep their
own inferred propagation; explicitly written optional or typed failure
contracts retain their existing carrier projections.
An eager adapter inside a lambda propagates through that lambda's carrier to
the enclosing owner; nesting does not erase the callable value's failure route.

The `Iterable.iter` protocol retains its declared raw iterator return, rather
than adding an implicit `Err` wrapper that its protocol ABI cannot accept.
A user Iterable enters the same runtime loop cursor as other checked sources:
MIR consumes its source once, `iter` runs once, and `Iterator.next` absence
ends iteration. Native emission supplies the owner's loop-source bridge.

A callable returning a view names the checked relationship between its
borrowed owner arguments and that return with a higher-ranked lifetime.
Returned static views remain static. The callable metadata and the ordinary
function-value type use the same provenance facts; neither guesses a lifetime
from the order of argument types.

Rust emission reads primitive `Copy` place carriers directly, without invoking
`Clone`. A display interpolation of text appends its borrowed contents to the
result buffer; it does not allocate a separate display string. Other formatting
selectors still use their checked formatting kernels.

Read-only concrete String parameters, captures, and callable inputs use `&str`
in Rust emission. Text kernels and heterogeneous collection queries borrow that
view directly; an owned return or stored value materializes one String. An
abstract generic Read formal still requires `&T`: it borrows an existing String
owner when available, and materializes a view only at that owned-carrier boundary.
Native optional-text readers project the optional payload as `Option<&str>`,
without copying it into an `Option<String>`.
## Core keepers

Engines must handle literals (`IntLit`, `FloatLit`, `BoolLit`, `CharLit`, and
plain `StrLit`), `Local`, `Call`, `MethodCall`/`BuiltinMethod`/`CoreCall`,
`ListLit`, `TupleLit`, `StructLit`, `EnumLit`, `Field`, `Index`,
`Binary`/`Unary`, `If`/`IfExpr`, `Match`-shaped statements, `Let`/`Assign`,
`Loop`/`While`/`CountedLoop`/`Range`, `Return`/`Break`/`Continue`,
`Clone`/`Borrow`/`Drop`, `Print`, `Lambda`, `Inline` (empty comptime
elision), and the host, select, and task nodes defined by `mod.rs`. Prefer
deleting a wide node over adding a new one.

## Proof obligations

- Keep `jit_coverage_audit` as the coverage check; a hand-maintained gap list
  is not a substitute for the structural proof.
- `Examples/features/concurrency/freeze_capture.jet` is the cross-lens fixture
  for AOT, default `jet run`, and forced interpretation. Comptime returns an
  already-owned `CtValue` for `freeze`.
- The REPL task boundary remains E1802, and web has no separate freeze policy or
  engine-side capture check.

These checks protect the contract's boundaries. They do not turn the current
Rust host into a permanent architecture commitment, and they do not let a
future `Compiler/` implementation silently introduce a parallel semantic core.
