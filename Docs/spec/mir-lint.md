# MIR Lint

This page is the compiler contract for MIR Lint, the typed checker over the
canonical MIR program. It is a second checker. It does not reuse the semantic
checker or the optimizer's legality verifier, so a lowering or optimization bug
is caught before an engine generates code from the wrong program. The Jet
compiler implements it in `Compiler/JetOptimizer/Source/Verification/Lint.jet`;
its rule-by-rule fixtures are `Compiler/JetOptimizer/Tests/LintFixtures.jet`
(assembled by `Compiler/JetOptimizer/Tests/run-lint-fixtures.mjs`). The
archived Rust bootstrap compiler does not carry the lint.

## When it runs

- Once on the program the optimizer returns, on every compile and in every
  build mode, including a program that skips the passes because it carries a
  complete seal.
- After every optimizer pass, and on the lowered input, when full MIR
  verification is on (`optimize_mir_program(program, full_verification: true)`),
  together with the legality re-verification after every pass. An ordinary
  compile verifies legality once on entry and trusts the passes after it, as
  the Rust reference's `validate_after` does.
- A failure is an internal compiler error. The report names the pass that
  produced the program, the rule family, the function, and the offending
  operation.
- The lint is linear in the size of the program. Ownership uses one forward
  dataflow over flat bit rows per function, with no per-block copies of maps.

## Type compatibility

The lint compares types with one relation, *compatible*. Each side is first
reduced to its representation class. Non-nominal tags, `InlineRange`,
`Quantity`, and `Shared` reduce to their inner type. `Int` and every `IntN`
form the integer class. `Float` and `Float32` form the float class. `List` and
`FixedList` form the list class. `Fn` and `SendFn` form the callable class.
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
| `ownership.move` | No value or place is used after it was moved on every path to the use, a value is consumed at most once, and only owned values are consumed. A write to a moved place or its parent restores it. A Bool local that is provably false (or true) on every path removes the branch edge it guards. Failure and unwind drop edges are entry points. |
| `effect.row` | Every effect in a direct callee's solved row is in the caller's solved row, itself or through a parent effect (`FS` covers `FS.Write`), unless the caller's row is maximal. |

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
