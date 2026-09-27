# Canvas parity matrix

This matrix maps the Jet AST surface to Canvas projection and editing
responsibilities. It is for Canvas clients and compiler contributors who need to
know whether a construct has graph semantics, source-backed editing, or only a
read-only fact view. The enum names are checked against the AST definitions in
[`items.rs`](../../../crates/jet-foundation/src/AST/items.rs),
[`statements.rs`](../../../crates/jet-foundation/src/AST/statements.rs),
[`expressions.rs`](../../../crates/jet-foundation/src/AST/expressions.rs),
[`types.rs`](../../../crates/jet-foundation/src/AST/types.rs),
[`patterns.rs`](../../../crates/jet-foundation/src/AST/patterns.rs), and
[`lvalues.rs`](../../../crates/jet-foundation/src/AST/lvalues.rs). The Canvas
projection and transaction tests live in
[`tests/canvas.rs`](../../../tests/canvas.rs); the wire contract is
[`canvas-protocol.md`](canvas-protocol.md). Terms such as stream follow the
[Jet vocabulary](../vocabulary.md).

A row acknowledges a construct in the contract; it does not promise a bespoke
graph gesture. The coverage labels mean:

- `graph`: Canvas projects dedicated graph semantics and may expose a checked
  graph operation.
- `source`: the construct is edited through Code lens or a source transaction;
  Canvas does not invent a graph representation.
- `readonly`: Canvas may show facts or a node, while edits go through source.
- `unsupported`: Canvas preserves source and reports the boundary instead of
  pretending to edit it.

## Items

- [Item::Func] status=graph function graph, signature/source edits.
- [Item::Struct] status=readonly type and typestate facts, source edits.
- [Item::Enum] status=readonly type facts, source edits.
- [Item::Distinct] status=readonly type facts, source edits.
- [Item::TypeAlias] status=readonly type facts, source edits.
- [Item::UnitFamily] status=readonly unit-family facts, source edits.
- [Item::Trait] status=readonly interface facts, source edits.
- [Item::Tag] status=readonly marker facts, source edits.
- [Item::EffectDecl] status=source effect declaration through Code lens and
  source transactions; no dedicated graph projection.
- [Item::Impl] status=readonly method and implementation facts, source edits.
- [Item::Const] status=readonly symbol facts, source edits.
- [Item::Test] status=readonly test scope, source edits.
- [Item::ExternRust] status=unsupported expert FFI surface, source edits only.
- [Item::Module] status=readonly Jetpack contribution facts, source edits.
- [Item::CModule] status=unsupported expert FFI surface, source edits only.
- [Item::CodeModule] status=readonly module facts, source edits.
- [Item::ErrorConv] status=readonly conversion facts, source edits.
- [Item::Migration] status=readonly schema-evolution facts, source edits.
- [Item::ProtocolDecl] status=readonly protocol facts, source edits.
- [Item::UserDerive] status=readonly derive facts, source edits.
- [Item::TemplateLoop] status=readonly item-template expansion facts, source
  edits.
- [Item::GenericModule] status=readonly module-template facts, source edits.
- [Item::ModuleAlias] status=readonly module-alias facts, source edits.
- [Item::MarkerDecl] status=readonly marker registry facts, source edits.
- [Item::FactDecl] status=readonly fact-declaration registry facts, source
  edits.

## Statements

- [Stmt::Expr] status=graph expression or action node.
- [Stmt::Val] status=graph binding node.
- [Stmt::Assign] status=graph assignment node.
- [Stmt::Return] status=graph return node.
- [Stmt::While] status=graph loop rail.
- [Stmt::For] status=graph loop rail.
- [Stmt::Switch] status=graph switch rail with source-backed pattern-arm
  add/edit/remove transactions.
- [Stmt::Break] status=graph control node.
- [Stmt::BreakValue] status=graph control node with its value in source detail.
- [Stmt::Continue] status=graph control node.
- [Stmt::BreakLabel] status=graph control node with its label in source detail.
- [Stmt::BreakLabelValue] status=graph control node with its label and value in
  source detail.
- [Stmt::ContinueLabel] status=graph control node with its label in source
  detail.
- [Stmt::Loop] status=graph loop rail.
- [Stmt::CountedLoop] status=graph loop rail.
- [Stmt::Unsafe] status=readonly expert gate, source edits.
- [Stmt::Impure] status=readonly expert comptime-effect gate, source edits.
- [Stmt::Reactive] status=readonly effect registration, source edits.
- [Stmt::Shield] status=readonly cancellation-shield region, source edits.
- [Stmt::Switched] status=graph statement-state node with a source-backed
  `#Off` or `#DebugOnly` marker toggle and nested source edits (D-CANVASSTATE1).
- [Stmt::Region] status=readonly lifetime region, source edits.
- [Stmt::Policy] status=graph scoped policy region with declared keys in the
  title, nested-body projection, and source edits.
- [Stmt::TaskGroup] status=readonly task scope, source edits.
- [Stmt::Layout] status=readonly layout scope, source edits.
- [Stmt::AuthorityScope] status=readonly effect restriction and named
  `Authority` handle, source edits.
- [Stmt::ComptimeIf] status=readonly comptime branch, source edits.
- [Stmt::ComptimeSwitch] status=readonly comptime switch, source edits.
- [Stmt::ComptimeBlock] status=readonly comptime block, source edits.
- [Stmt::ContextBlock] status=readonly ambient context block, source edits.
- [Stmt::Live] status=readonly terminal-mode block, source edits.
- [Stmt::AssumeDet] status=readonly expert determinism block, source edits.
- [Stmt::Transact] status=readonly transaction block, source edits.
- [Stmt::Yield] status=readonly stream yield, source edits.
- [Stmt::ScopeMember] status=readonly marker-scope member, source edits.
- [Stmt::DeferClose] status=readonly resource cleanup, source edits.

`Stmt::Switched` is the AST representation for both statement markers. `#Off`
and `#DebugOnly` are marker data, not separate `Stmt` variants; Canvas must not
present them as separate enum rows. Likewise, an `if` statement is represented
by the relevant graph projection and does not add another statement enum variant
to this matrix.

Binding metadata remains source-backed. `#Meta` facts may appear on binding and
function nodes; Details can edit scalar, enum, reference, collection, and nested
values only through the existing checked source transactions
(D-CANVASMETA1). A stale revision or invalid nested value leaves the original
source intact.

## Palette staging

Core entries marked `needs_canvas_defaults` or `method_only` are
`stageable`: the palette can show a dashed local node without writing source.
A compatible input wire materializes the checked `insert_call` transaction.

Entries marked `needs_unsafe_region`, `type_member`, `type_only`, or `value_only`
remain unavailable with their stable reason codes. The palette must show the
reason rather than silently inventing a receiver, default, or source edge.

## Expressions

- [Expr::Str] status=graph literal or expression node.
- [Expr::StrMatchLit] status=readonly string-match pattern literal, source edits.
- [Expr::BinMatchLit] status=readonly binary-match pattern literal, source edits.
- [Expr::Int] status=graph literal node.
- [Expr::Float] status=graph literal node.
- [Expr::Bool] status=graph literal node.
- [Expr::Unit] status=graph unit value node.
- [Expr::Char] status=graph literal node.
- [Expr::ListLit] status=graph collection node.
- [Expr::MemberSpread] status=readonly member-spread detail, source edits.
- [Expr::Spread] status=readonly spread detail, source edits.
- [Expr::MapLit] status=graph collection node.
- [Expr::Index] status=graph index node.
- [Expr::Slice] status=graph slice node.
- [Expr::Range] status=source through Code lens and source transactions; no
  dedicated graph projection.
- [Expr::Ident] status=graph reference node.
- [Expr::Call] status=graph function node.
- [Expr::Unary] status=graph operator node.
- [Expr::Binary] status=graph operator node.
- [Expr::CompareChain] status=graph operator node.
- [Expr::UnitLit] status=readonly unit literal, source edits.
- [Expr::Deref] status=readonly unsafe pointer expression, source edits.
- [Expr::RawOf] status=readonly unsafe pointer expression, source edits.
- [Expr::Copy] status=graph copy expression.
- [Expr::Place] status=readonly checked place acquisition, source edits.
- [Expr::Field] status=graph field node.
- [Expr::OptField] status=graph optional-field node.
- [Expr::MethodCall] status=graph function or variant node.
- [Expr::StructLit] status=graph construction node.
- [Expr::TypedLit] status=graph typed-literal node.
- [Expr::EnumLit] status=graph variant node.
- [Expr::Tainted] status=readonly taint marker, source edits.
- [Expr::Present] status=graph optional-present node.
- [Expr::Absent] status=graph optional-absent node.
- [Expr::Todo] status=readonly typed goal, source edits.
- [Expr::NoElse] status=readonly dispatch no-else arm, source edits.
- [Expr::ReduceMarker] status=readonly SIMD marker, source edits.
- [Expr::PatternTest] status=graph pattern-test node.
- [Expr::Ok] status=graph fallible-ok node.
- [Expr::Err] status=graph fallible-err node.
- [Expr::Try] status=graph fallible-propagation node.
- [Expr::OrFallback] status=graph fallback node.
- [Expr::If] status=graph expression branch node.
- [Expr::TupleLit] status=graph tuple node.
- [Expr::Lambda] status=graph lambda node.
- [Expr::CallValue] status=graph call-value node.
- [Expr::PtrFromAddr] status=readonly unsafe pointer constructor, source edits.
- [Expr::ComptimeName] status=readonly comptime-name mention, source edits.
- [Expr::Paren] status=graph grouped-expression detail.
- [Expr::IncDec] status=graph increment/decrement node.

## Types

- [Type::Int] status=readonly type detail.
- [Type::Float] status=readonly type detail.
- [Type::Bool] status=readonly type detail.
- [Type::String] status=readonly type detail.
- [Type::Char] status=readonly type detail.
- [Type::List] status=readonly type detail.
- [Type::Map] status=readonly type detail.
- [Type::Shared] status=readonly type detail.
- [Type::Option] status=readonly type detail.
- [Type::Result] status=readonly type detail.
- [Type::Fn] status=readonly type detail.
- [Type::Named] status=readonly type detail.
- [Type::Apply] status=readonly type detail.
- [Type::TraitObject] status=readonly type detail.
- [Type::Tuple] status=readonly type detail.
- [Type::FixedList] status=readonly type detail.
- [Type::IntN] status=readonly type detail.
- [Type::InlineRange] status=readonly inline-range type detail.
- [Type::Float32] status=readonly type detail.
- [Type::Tagged] status=readonly type detail.
- [Type::Union] status=readonly type detail.
- [Type::Quantity] status=readonly quantity type detail.
- [Type::Measure] status=readonly measure type detail.

## Patterns

- [Pattern::Variant] status=graph source-backed arm authoring.
- [Pattern::Present] status=graph optional pattern detail.
- [Pattern::Absent] status=graph optional pattern detail.
- [Pattern::Ok] status=graph fallible pattern detail.
- [Pattern::Err] status=graph fallible pattern detail.
- [Pattern::Range] status=graph source-backed range-pattern detail.
- [Pattern::Or] status=graph source-backed or-pattern detail.
- [Pattern::Struct] status=graph struct-pattern detail.
- [Pattern::StrMatch] status=readonly string-match pattern, source edits.
- [Pattern::BinMatch] status=readonly binary-match pattern, source edits.

## Binding patterns

- [BindPattern::Struct] status=graph binding detail.
- [BindPattern::List] status=graph binding detail.
- [BindPattern::Tuple] status=graph binding detail.
- [BindPattern::Refutable] status=graph checked refutable-binding detail.

## Assignment targets

- [LValue::Local] status=graph assignment target.
- [LValue::Index] status=graph assignment target.
- [LValue::Field] status=graph assignment target.
