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

## Refinements learned in Phase 2 (control-flow loops)

- **Loop labels carry only the name, not the span.** The AST stores `Option<(String, Span)>`
  and emits via `loop_label_prefix`; the TIR resolves this to `Option<String>` at lowering and
  the emitter has its own `tir_label_prefix` so it never reaches back into the AST-side helper.
  Same `'jet_<name>:` rendering — parity holds.
- **`break`/`continue` need no expression checking.** The parser only admits them inside a loop
  body, so they are always valid where they appear; the gate accepts them unconditionally and
  the label name reproduces verbatim. No loop-nesting validation in codegen (sema's job).
- **Jet loop surface maps to three different AST nodes:** infinite `loop {}` → `Stmt::Loop`;
  `loop cond {}` → `Stmt::While`; `loop i in a..b [step k]` → `Stmt::For{ForKind::Range}`.
  The range form's inclusive `..` (S22/D-SG8) → Rust `..=`, and `step` → `.step_by((k) as usize)`,
  reproduced byte-for-byte from `Statement.rs`. The two-binding `key, value` map form and
  `ForKind::In` collection loops are explicitly excluded (Phase 5 — they need collections).
- **Loop bodies are scopes.** Both the gate and lowering recurse into loop bodies on a *cloned*
  locals/env so a `let` inside a loop (or the range loop's `i` var, bound as `Int`) does not leak
  past it. This mirrors the existing `if`-branch scoping exactly.

## Refinements learned in Phase 3 (structs)

- **Sema rewrites owning field reads into `.clone()` MethodCalls *before* codegen.**
  `CheckerInfer::infer` (Source/Sema/CheckerInfer.rs) replaces a non-`Copy` struct-field
  read in owning position (`return p.name`, a struct-lit field value, etc.) with an
  `Expr::MethodCall{method:"clone"}` via `field_read_to_clone`. So by the time `tir_covers`
  runs, an owning field read is a *method call*, which the subset excludes (`_ => false`).
  The only `Expr::Field` nodes that survive to codegen are **borrow-position** reads (inside
  `print(..)`, arithmetic, an interpolation, a comparison), which the AST path emits as a
  plain `(recv).field` with no deref/clone. The TIR reproduces that exactly. **Consequence:**
  Phase 3 covers field *reads* in borrow position only; any function that moves a non-`Copy`
  field out (the common "getter returning a String field") stays on the AST path until
  **Phase 6/7** make the clone fact total. This is the right seam — the clone decision lives
  in sema's elaboration, not codegen.
- **Field assignment `s.field = value` is not a Jet construct.** `expr_to_lvalue`
  (Source/Parser/Expressions.rs) only admits `Ident`/`Index`; `LValue` has no `Field`
  variant; a field assign is rejected at parse time (E0003). Struct mutation is method-only
  (`mut self`) — a Phase 7 concern. There was nothing to lower for "field assignment."
- **A struct-field operand must not enable the overflow trap.** The AST `operand_is_integer`
  resolves a field read to `None` (`expr_jet_ty` has no `Field` arm), so `p.x + p.y` emits a
  plain `+`, never `jet_add` — even though both fields are `Int`. The TIR's `overflow` flag
  therefore **cannot** be computed from `TExpr.ty` (which is total even for a field); it must
  replay `operand_is_integer` on the *AST operands* (`ast_operand_is_integer`). A literal or
  resolvable local operand still traps (`p.x + 1` → `jet_add`), matching the AST path
  bit-for-bit. General lesson: when a TIR field is *more* total than the AST's re-derivation,
  the parity-preserving decision is the AST's, not the "better" one — reproduce the AST's
  partiality where it is load-bearing.
- **Gate struct coverage by walking the resolved layout, not the surface.** `struct_is_covered`
  consults `cx.struct_fields`/`boxed_edges`/`enum_variants`/`foreign_types` to admit only a
  plain user struct whose every field is scalar/String/Char or another covered struct, with a
  visited set to terminate on (and exclude) recursion. This keeps the gate cx-aware and total,
  and naturally excludes prelude structs (HttpRequest), foreign/imported types, and recursive
  (boxed) structs whose field reads would need deref handling.

## Refinements learned in Phase 4 (enums + when/match + patterns)

- **A *unit* enum literal is an `Expr::Field`, not `Expr::EnumLit`.** Sema's `infer_field`
  (Source/Sema/CheckerInfer.rs) only *re-types* `Light.Yellow` (via `check_enum_lit`); it
  does **not** rewrite the AST node. Only payload literals (`Conn.Active(42)`) parse/stay as
  `Expr::EnumLit`. So the TIR gate + lowering had to cover **both** surfaces: `Expr::Field`
  whose receiver is a covered-enum-name ident (→ `user_<Enum>::user_<variant>`) and
  `Expr::EnumLit`. This was the load-bearing trap — without it the obvious `11_enums` `next`
  stayed on the AST path. General lesson (again, Phase-3 flavored): the AST node that reaches
  codegen is what the gate must recognise, not the surface syntax.
- **The `when`/match is a `Stmt::Switch`; the `if subject { arm -> … }` form is the same node.**
  There is no separate matching `Expr`. The AST path forks into two lowerings the TIR
  reproduces exactly: `emit_pattern_match_switch` (an exhaustive Rust `match`, used when *all*
  arms are variant patterns) and `emit_mixed_switch` (an `if/else if … else` chain, used for
  arm-head range patterns). Phase 4 covers shape A (all-variant) and the all-range-arm-with-
  `else` slice of shape B; a *mixed* comparison/Bool switch stays on the AST path.
- **The match scrutinee clone is `(rust_name).clone()`, not `(*name).clone()`.** A by-reference
  enum param (`Read` non-scalar → deref'd slot) is cloned so the `match` owns the value, but
  `emit_pattern_match_switch` clones the *borrow* (`slot.rust_name`), not the deref'd place.
  The TIR resolves the scrutinee string at lowering, stripping the `(*…)` wrapper for the clone
  case — reproducing the borrow-clone exactly. Requires the enum derive `Clone`, so the gate
  excludes non-cloneable enums.
- **Scalar-only payloads keep clone/box decisions out of the TIR.** A String/struct/collection
  payload would route through `emit_boxed_enum_arg` (borrowed-payload `.clone()`) and boxed
  recursive edges (`Box::new(…)`) — decisions the subset can't reproduce from total facts. So
  the gate admits an enum only when **every variant payload field is scalar/Char** (no boxed
  edge). For those, `emit_boxed_enum_arg` is a provable no-op, so a literal arg / a bound payload
  emits as-is — byte-parity. String-payload enums, recursive (boxed) enums, and fallible/optional
  patterns (`value`/`null`/`ok`/`err`) are deferred (Phase 7/8).
- **Pattern strings + range guards are resolved at lowering, reproducing the Statement.rs
  formatters.** `tir_match_pattern`/`tir_range_guard`/`tir_add_pattern_bindings` mirror
  `emit_match_pattern`/`emit_range_guard`/`add_pattern_bindings` for the user-enum case (the
  subset excludes JSON/foreign enums, so only `user_<Enum>::user_<V>` heads arise). A payload
  range slot (`Good(200..299)`) binds `__jet_range_i` and emits an `if …` guard; an or-pattern
  reuses the first alt's bindings/ranges (E0317 guarantees all alts bind alike). Verified
  byte-identical to the forced-AST path for `11_enums` + `71_pattern_matching` (incl. both
  `main`s).

## Phases

| # | Scope | Status |
|---|-------|--------|
| 0 | Inventory + TIR type-model design | ✅ done (4b89af5) |
| 1 | Simple functions: literals, operators, bindings, returns, if/else, calls, print | ✅ done (398138b) — 54 fns routed |
| 2 | Control flow: `loop`{infinite/while/range}, break/continue (+labels) | ✅ done (c109 Phase 2) — +3 example fns routed (e.g. fizzbuzz `main`). Excludes `loop x in <collection>` (ForKind::In, key/value map form) → Phase 5. |
| 3 | Structs: struct literals, field reads, struct-typed params/locals/returns | ✅ done (c109 Phase 3) — 57→65 covered fns. Covers plain (non-generic, non-recursive) user struct literals, borrow-position field reads (incl. nested/chained), struct params/locals/returns. Excludes: owning field reads (sema rewrites them to a `.clone()` MethodCall → Phase 7), trait-coerced literals (`as_trait`), imported-namespace/prelude structs, generic & recursive (`Box<…>`) structs, and **field assignment** (not a Jet construct — `s.field = value` is E0003; struct mutation is method-only → Phase 7). |
| 4 | Enums + `when`/match + patterns (incl. range/or patterns) | ✅ done (c109 Phase 4) — ~7 more example fns routed (`11_enums` `next`/`label`/`main`, `71_pattern_matching` `describe_conn`/`classify`/`score_grade`/`main`). Covers plain (non-generic, non-recursive, Clone-derivable) scalar/Char-payload user enums: unit/positional/named enum literals; exhaustive variant `match` (variant patterns, positional/named/wildcard/range payload slots, or-patterns, payload bindings); arm-head range switches (`lo..hi -> …` + `else`); enum params/locals/returns. Excludes: String/struct/collection-payload enums (clone/borrow at literal & binding site → Phase 7), recursive (`Box<…>`) enums, generic enums, JSON/foreign/prelude enums, fallible/optional patterns (`value`/`null`/`ok`/`err` → Phase 8), and mixed comparison/Bool switches (general `emit_mixed_switch`). |
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
