# TIR semantic core

This page is the compiler contract at the boundary between semantic analysis and
executable lenses. It covers totality, the read-only fact channel, lowering,
and coverage obligations. The current host implementation is
`crates/jet-codegen/src/Codegen/TIR/mod.rs` and its exhaustive matches; the
staged self-host port under `Compiler/JetFoundation/Source/TIR/TIR.jet` must
preserve the same contract.

TIR is a semantic contract, not a commitment to Rust as a permanent
implementation and not a claim that the self-host port is already the active
compiler. A port may change representation, but it may not move semantic
choices into an executable lens or create a second meaning.

## One semantic core (R12 / D-ONECORE1=A)

There is one structured semantic core. Every executable lens—Rust AOT emit,
interpreter, and web—consumes that core exhaustively. Surface sugar
expands during lowering or an earlier sema rewrite into core nodes that those
lenses already handle. A coverage miss is an internal compiler error, never a
fallback.

TIR is post-sema and total: every fact codegen needs is carried concretely on
the node, never re-inferred through a fallback. The emitter pattern-matches
those fields and formats output; it does not re-run semantic decisions such as
expression typing or overflow classification. `tir_covers` is the coverage
proof for reachable functions.

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
| `task ^name { … }` | The existing task lambda with an explicit consuming capture; the task-crossing prover owns legality. | D-CONC-FREEZE1=A |

The freeze example is a lowering rule, not an engine-side capture check. A
self-host implementation must carry the same already-proved ownership facts to
each lens.

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

Pattern payload and field types instantiate the checked subject's owner
parameters before binding locals; a declaration's bare `T`/`E` does not escape
into its concrete match. Associated types in a trait declaration use `Self::X`.
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
