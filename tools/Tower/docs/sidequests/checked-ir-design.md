# Checked IR (c109) — living design & phase tracker

**Card:** c109 (P0) — Introduce a checked IR boundary before codegen.
**Status:** in progress. Option 1 (boundary-hardening) landed (commit 0f19565).
This doc is the durable design + phase tracker for the **full** checked-IR build,
which spans many commits and outlives any single working context.

## Goal (from the card + plan)

Codegen today consumes the **AST plus side registries** (`Source/Codegen/Context.rs`
`Cx`) and re-derives semantic facts (types, call resolution, name mangling,
coercions, clone insertion). That violates the spirit of I3 ("codegen is dumb").

Introduce a **checked IR** (a TIR — typed intermediate representation) that
carries only sema-approved facts. Sema lowers AST → TIR after type/ownership
checking; codegen consumes TIR and is a pure, decision-free translator.

## Invariants this must preserve

- **I2** — rustc must never reject generated code. Every phase ships with golden
  parity: the lowered Rust is identical (or provably equivalent) to before.
- **I3** — all checking stays in sema; the TIR lowering happens *after* sema, and
  codegen makes **no** semantic decision.
- **R7** — backend swappability: the TIR is the clean seam a future Cranelift/LLVM
  backend consumes instead of `Codegen/`.

## Strategy (incremental, parity-gated)

Per the plan: start with simple functions + local expressions (no ownership
effects, FFI, comptime, package boundaries), prove golden parity, then expand
module by module. Each phase is a complete, committed, suite-green slice.

The TIR is introduced **alongside** the AST path: a function is lowered to TIR and
codegenned from TIR only once its constructs are covered; everything else stays on
the AST path until its phase lands. A per-function feature gate decides the path,
so the suite stays green throughout. The gate shrinks to nothing when coverage is
complete, at which point the AST codegen path is deleted.

## Fact inventory (what the TIR must carry)

The read-only inventory (Phase 0) established the key facts. Full report archived
in the commit message / session; the load-bearing conclusions:

**A. Sema already annotates most per-node facts onto the AST.** These fields exist
specifically to carry checked results into codegen:
- `Expr::Int` width `Option<(bool,u8)>`; `Expr::Index`/`LValue::Index` `kind: IndexKind`;
  `Expr::MethodCall.recv_type: Option<String>`; `Expr::OptField.flatten: bool`;
  `Expr::Try` `TryConvert::{None,Fallible,Typed(fn)}`; `Expr::StructLit.as_trait`;
  `Expr::Todo.expected_type`.
- `CallArg.flags.{implicit_clone, shared_auto_clone}`.
- `Binding.{ty, arena_view, ct, uninit}`; `ConstDef.ct`; `Stmt::ComptimeIf.selected_then`.
- `Lambda.meta.{escapes, needs_fn_mut, mut_captures, cloned_captures}`.

**B. Codegen STILL re-derives types and decides.** The smells the TIR removes:
- `expr_jet_ty` / `expr_jet_ty_with_cx` (Codegen/Expression.rs ~714/752) — codegen
  *re-infers* expression types (for destructure element types, width conversions,
  map-vs-list dispatch, method receiver fallback). This is the core violation.
- `operand_is_integer` (Expression.rs ~739) — re-checks operand types to pick the
  overflow-trapping operator.
- `numeric_conversion` (Expression.rs ~1018) — width source/target from `recv_type`.
- Optionality: `recv_type: Option<String>` has a *codegen fallback inference* when
  `None`. That Optionality is the bug class (the c109 Option-1 `.lines()` I2 hole was
  exactly a codegen path reached because a fact wasn't total).

**C. `Cx` (Codegen/Context.rs) is program-level metadata**, not per-node: signatures
(`sigs`, `method_sigs`, `import_sigs`), type shape (`struct_fields`, `enum_variants`,
`variant_owner`, `boxed_edges`), derive sets (`cloneable`, `comparable`, `partial_ord`),
mangling/imports (`import_mods`, `foreign_types`, `reexport_calls`, `core_imports`,
`code_modules`, `unqualified_*`), FFI (`ffi_crate`, `extern_funcs`). The TIR keeps a
*resolved* program-metadata table, but codegen reads it without deciding.

**D. The local env** (`HashMap<String, Slot>`, Slot = `{rust_name, deref, jet_ty}`) is
the per-function type/binding environment, rebuilt during emission. In the TIR, slot
facts (rust_name, deref, resolved type) are produced by lowering, not re-derived.

## TIR design principle: TOTALITY

The TIR is a distinct, post-sema typed representation whose defining property is that
**every fact codegen needs is total (non-`Option`) and pre-resolved**:
- every TIR expression carries its resolved `Type` (no `expr_jet_ty` in codegen);
- every call/method node carries its resolved target + param conventions;
- every index carries a resolved `Map`/`List` kind (no `IndexKind::Unknown`);
- every clone/coerce/convert decision is a concrete TIR node or flag.

If lowering cannot supply a fact, that is a *sema bug* surfaced at lowering time — never
a codegen guess. This kills the entire "codegen re-derives / falls back" class (I3), and
gives a clean seam a future non-Rust backend consumes (R7).

`expr_jet_ty`, `operand_is_integer`, `numeric_conversion`, and the `recv_type` fallback
all disappear from codegen as their constructs move onto the TIR.

## TIR type model (draft — refined per phase)

A new module tree `Source/TIR/` (or `Source/Codegen/TIR.rs` to start):
- `TFunc { name (mangled), params: Vec<TParam>, ret: Option<RType>, body: Vec<TStmt> }`
- `TStmt::{ Let{slot, init}, Assign{place, value}, Expr(TExpr), Return(Option<TExpr>),
  If{cond, then, else}, … }`
- `TExpr { ty: Type, kind: TExprKind }` — **`ty` is total**.
- `TExprKind::{ IntLit(i128, width), Bool, Str(parts), Local(SlotId), Call{target, args},
  MethodCall{recv, target, args}, Binary{op, overflow: bool, l, r}, … }`
- `TCallTarget` is *resolved*: a concrete function id + param conventions, not a name to
  re-look-up.
- A `LoweredProgram { metadata: ProgMeta, funcs: Vec<TFunc>, … }` where `ProgMeta` is the
  resolved, read-only program tables (struct layouts, enum variants, derive sets, import
  paths) distilled from today's `Cx`.

Lowering lives in sema-adjacent code (`Source/Sema/Lower.rs` or `Source/TIR/lower.rs`),
runs AFTER `check_bundle`, and consumes the already-annotated AST. Codegen gets a second
entry `emit_tir(&LoweredProgram)` that translates TIR → Rust with no `Cx`-style inference.

## Incremental strategy (parity-gated, suite stays green)

The TIR path runs ALONGSIDE the AST path. A per-function predicate `tir_covers(f)` returns
true only for functions whose constructs the current TIR phase handles; covered functions
codegen via `emit_tir`, all others via the existing AST `emit_func`. Both must produce
byte-identical Rust (golden parity). `tir_covers` widens each phase; when it covers
everything, the AST codegen path is deleted (final phase).

## Refinements learned in Phase 1 (apply to all later phases)

- **The gate is `tir_covers(f, cx)` — cx-aware, not `f`-only.** It must consult program
  tables: comptime consts inline at use sites (a bare ident emits the value, not
  `user_<name>`), and extern/FFI + unqualified-module-import calls emit different forms.
  Without `cx` those are false positives → I2 bugs. Gate is conservative: exclude on any
  doubt (false negative = stays on AST path = safe; false positive = unsafe).
- **Byte parity pulls in a few presentation facts beyond types** — all resolved at
  lowering (still total): a `Let` records `annotated: bool` (so an inferred binding emits
  no `: ty`); a `Binary` carries the source `line: u32` (the overflow-trap helper embeds
  it). When adding constructs, carry whatever resolved facts the existing emit path uses,
  as total TIR fields — never re-derive in emit.
- **Mirror existing emit decisions exactly** to preserve parity (e.g. `operand_is_integer`
  inspects only the left spine of nested arithmetic; the TIR `overflow` flag reproduces
  that). Parity drift = a golden failure.
- TIR lives in `Source/Codegen/TIR.rs`; `lower_func`/`emit_tir_func`/`tir_covers` are the
  three entry points each phase extends. `emit_tir_func` reuses pure formatting helpers
  (`mangle_name`, `rust_type`, the `jet_add`/… helper names, interpolation) but takes all
  *decisions* from TIR fields.

## Phases

| # | Scope | Status |
|---|-------|--------|
| 0 | Inventory + TIR type-model design | ✅ done (4b89af5) |
| 1 | Simple functions: literals, operators, bindings, returns, if/else, calls, print | ✅ done (398138b) — 54 fns routed |
| 2 | Control flow: `loop`{infinite/while/range}, break/continue (+labels), `when`-less | pending |
| 3 | Structs: struct literals, field access/assign, struct-typed params/locals/returns | pending |
| 4 | Enums + `when`/match + patterns (incl. range/or patterns) | pending |
| 5 | Collections: list/map literals, indexing/slicing, `loop x in collection` | pending |
| 6 | Ownership facts: clone insertion, `take`/`mut`/`view`, Shared/Arc auto-clone | pending |
| 7 | Methods + method calls (recv_type → resolved target), trait-impl methods | pending |
| 8 | Fallible/optional: `?`/try (TryConvert), `??`, `T?` optionals, `ok`/`err` | pending |
| 9 | Lambdas/closures (capture meta), fn-typed values, fan-out | pending |
| 10 | Core/stdlib calls, imports/modules, FFI, comptime-if, arena/unsafe | pending |
| N | Delete the AST `emit_func`/`expr_jet_ty`/`operand_is_integer` path; TIR is the only seam | pending |

Phase ordering may adjust as coverage reveals coupling; the gate keeps the suite green
regardless of order. Each phase: extend `tir_covers`/`lower_func`/`emit_tir_func`, keep
the full suite green (golden = behavioral parity), commit one slice, update this table.

## Verification per phase

- `nix develop -c cargo test -- --test-threads=1` stays fully green.
- Golden examples produce byte-identical Rust (a diff test where practical).
- No new `unsafe` in generated code outside the audited gate (I1).
