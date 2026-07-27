# Canvas Parity Matrix

Canvas is source-backed. A row means the form is acknowledged by the Canvas
contract; it does not mean every form has a bespoke graph gesture. Status values:

- `graph`: projected with dedicated graph semantics.
- `source`: fully editable through Code lens/source transaction.
- `readonly`: visible/cataloged, edits go through source.
- `unsupported`: deliberately not graph-editable; Canvas must keep source truth
  and avoid pretending support.

## Items

- [Item::Func] status=graph function graph, signature/source edits.
- [Item::Struct] status=readonly type facts, source edits.
- [Item::Enum] status=readonly type facts, source edits.
- [Item::Distinct] status=readonly type facts, source edits.
- [Item::TypeAlias] status=readonly type facts, source edits.
- [Item::UnitFamily] status=readonly type facts, source edits.
- [Item::Trait] status=readonly interface facts, source edits.
- [Item::Tag] status=readonly marker facts, source edits.
- [Item::Impl] status=readonly method facts, source edits.
- [Item::Const] status=readonly symbol facts, source edits.
- [Item::Test] status=readonly marker scope, source edits.
- [Item::Bench] status=readonly marker scope, source edits.
- [Item::ExternRust] status=unsupported expert FFI surface, source edits only.
- [Item::Module] status=readonly Jetpack contribution facts, source edits.
- [Item::CModule] status=unsupported expert FFI surface, source edits only.
- [Item::CodeModule] status=readonly module facts, source edits.
- [Item::ErrorConv] status=readonly conversion facts, source edits.
- [Item::Migration] status=readonly schema evolution facts, source edits.
- [Item::StateDecl] status=readonly typestate facts, source edits.
- [Item::ProtocolDecl] status=readonly protocol facts, source edits.
- [Item::UserDerive] status=readonly derive facts, source edits.
- [Item::GenericModule] status=readonly module-template facts, source edits.
- [Item::ModuleAlias] status=readonly module alias facts, source edits.

## Statements

- [Stmt::Expr] status=graph expression/action node.
- [Stmt::Val] status=graph binding node.
- [Stmt::Assign] status=graph assignment node.
- [Stmt::Return] status=graph return node.
- [Stmt::If] status=graph branch node.
- [Stmt::While] status=graph loop rail.
- [Stmt::For] status=graph loop rail.
- [Stmt::Switch] status=graph switch rail.
- [Stmt::Break] status=graph control node.
- [Stmt::BreakValue] status=graph control node with value in source detail.
- [Stmt::Continue] status=graph control node.
- [Stmt::BreakLabel] status=graph control node with label in source detail.
- [Stmt::BreakLabelValue] status=graph control node with label and value in source detail.
- [Stmt::ContinueLabel] status=graph control node with label in source detail.
- [Stmt::Loop] status=graph loop rail.
- [Stmt::CountedLoop] status=graph loop rail.
- [Stmt::Val metadata] status=`#Meta` projects to binding-node `meta` JSON (D-CANVASMETA1), details-panel UI pending #377.
- [Func metadata] status=`#Meta` projects to function `meta` JSON (D-CANVASMETA1), details-panel UI pending #377.
- [Stmt::Off] status=readonly switched-off statement (D-CANVASSTATE1), node badge UI pending, source edits.
- [Stmt::DebugOnly] status=readonly debug-build statement (D-CANVASSTATE1), node badge UI pending, source edits.
- [Stmt::Unsafe] status=readonly expert gate, source edits.
- [Stmt::Impure] status=readonly expert gate, source edits.
- [Stmt::Reactive] status=readonly effect registration, source edits.
- [Stmt::Shield] status=readonly cancellation shield region, source edits; projection:tests/canvas.rs::canvas_projects_and_source_edits_shield_region.
- [Stmt::SuppressMustUse] status=readonly expert suppression, source edits.
- [Stmt::Region] status=readonly lifetime region, source edits.
- [Stmt::Policy] status=graph scoped policy region with declared keys in the title, nested body projection, and source edits; projection:tests/canvas.rs::canvas_projects_policy_region.
- [Stmt::TaskGroup] status=readonly task scope, source edits.
- [Stmt::Layout] status=readonly layout scope, source edits.
- [Stmt::Caps] status=readonly effect restriction, source edits.
- [Stmt::Grant] status=readonly capability grant, source edits.
- [Stmt::ComptimeIf] status=readonly comptime branch, source edits.
- [Stmt::ComptimeSwitch] status=readonly comptime switch, source edits.
- [Stmt::ComptimeBlock] status=readonly comptime block, source edits.
- [Stmt::ContextBlock] status=readonly ambient context block, source edits.
- [Stmt::Live] status=readonly terminal mode block, source edits.
- [Stmt::AssumeDet] status=readonly expert determinism block, source edits.
- [Stmt::Transact] status=readonly transaction block, source edits.
- [Stmt::Yield] status=readonly stream yield, source edits.
- [Stmt::ScopeMember] status=readonly marker-scope member, source edits.

## Expressions

- [Expr::Str] status=graph literal/expression node.
- [Expr::StrMatchLit] status=readonly pattern literal, source edits.
- [Expr::BinMatchLit] status=readonly pattern literal, source edits.
- [Expr::Int] status=graph literal node.
- [Expr::Float] status=graph literal node.
- [Expr::Bool] status=graph literal node.
- [Expr::Char] status=graph literal node.
- [Expr::ListLit] status=graph collection node.
- [Expr::Spread] status=readonly spread detail, source edits.
- [Expr::MapLit] status=graph collection node.
- [Expr::Index] status=graph index node.
- [Expr::Slice] status=graph slice node.
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
- [Expr::OptField] status=graph optional field node.
- [Expr::MethodCall] status=graph function or variant node.
- [Expr::StructLit] status=graph construction node.
- [Expr::EnumLit] status=graph variant node.
- [Expr::Tainted] status=readonly taint marker, source edits.
- [Expr::Present] status=graph optional present node.
- [Expr::Absent] status=graph optional absent node.
- [Expr::Todo] status=readonly typed hole, source edits.
- [Expr::ReduceMarker] status=readonly SIMD marker, source edits.
- [Expr::PatternTest] status=graph pattern-test node.
- [Expr::Ok] status=graph fallible ok node.
- [Expr::Err] status=graph fallible err node.
- [Expr::Try] status=graph fallible propagation node.
- [Expr::OrFallback] status=graph fallback node.
- [Expr::If] status=graph expression branch node.
- [Expr::TupleLit] status=graph tuple node.
- [Expr::Lambda] status=graph lambda node.
- [Expr::TypedLit] status=graph typed literal node.
- [Expr::CallValue] status=graph call-value node.
- [Expr::PtrFromAddr] status=readonly unsafe pointer constructor, source edits.
- [Expr::FanOut] status=graph fan-out node.
- [Expr::ComptimeSplice] status=readonly comptime splice, source edits.
- [Expr::Paren] status=graph grouped expression detail.
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
- [Type::Float32] status=readonly type detail.
- [Type::Tagged] status=readonly type detail.
- [Type::Union] status=readonly type detail.

## Patterns

- [Pattern::Variant] status=shipped source-backed arm authoring.
- [Pattern::Present] status=graph optional pattern detail.
- [Pattern::Absent] status=graph optional pattern detail.
- [Pattern::Ok] status=graph fallible pattern detail.
- [Pattern::Err] status=graph fallible pattern detail.
- [Pattern::Range] status=graph source-backed range pattern detail.
- [Pattern::Or] status=graph source-backed or-pattern detail.
- [Pattern::Struct] status=graph struct pattern detail.
- [Pattern::StrMatch] status=readonly string-match pattern, source edits.
- [Pattern::BinMatch] status=readonly binary-match pattern, source edits.

## Binding Patterns

- [BindPattern::Struct] status=graph binding detail.
- [BindPattern::List] status=graph binding detail.
- [BindPattern::Tuple] status=graph binding detail.

## Assignment Targets

- [LValue::Local] status=graph assignment target.
- [LValue::Index] status=graph assignment target.
- [LValue::Field] status=graph assignment target.
