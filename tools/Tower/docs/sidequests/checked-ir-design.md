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

## Latent AST-path bugs surfaced by this work (file as separate cards)

The phased coverage keeps exposing pre-existing codegen bugs (the gate routes around
them so they don't regress, but they should be fixed independently of c109):
- **`mut self` mutation is broken.** `self.field = v` is E0003 (field-assign isn't a Jet
  construct) and `self = v` emits `self = …` on `&mut user_T` → rustc E0308. So a `mut self`
  method can today only read/return. (Phase 7.)
- **Builtin-name method collision.** A user no-arg method named `get`/`len`/etc. is
  mis-dispatched by `emit_builtin_method` (name-keyed, not receiver-typed) on the AST path.
  (Phases 6–7.)
- **`Read`-convention struct params** in some arithmetic/move shapes hit E0507/E0308 on both
  paths (a sema convention-inference gap). (Phase 3.)
- **`.is_empty()` is typed `Int`, not `Bool`.** `Collections::builtin_method_return`
  (Source/Collections.rs) returns `Some(Some(Type::Int))` for `is_empty` on a list/map/string.
  So `e := xs.is_empty()` emits `let e: i64 = (…).is_empty()` (bool ≠ i64 → rustc E0308), and
  `if xs.is_empty()` is E0110 (Int isn't Bool) at sema. The method is unusable in any real
  program today — fix is a one-line `Type::Int` → `Type::Bool` in `list_method_return`/
  `map_method_return`/`string_method_return`. (Phase 9 gates it OUT so the TIR never claims a
  miscompiling function; both paths miscompile it identically.)
- **No-arg `.join()` is dead.** `emit_builtin_method` has a `"join" if args.is_empty()` arm,
  but sema requires `join(sep)` (E0311 on no-arg), so that arm never reaches codegen. (Phase 9
  excludes the no-arg form for the same reason.)
- **L0201 hardcodes `JSON.<call>` for ANY core call.** `expect_core_arg`
  (Source/Sema/CheckerCoreLib.rs ~L959) is the generic core-arg checker, but its
  D-L0201 "wasteful implicit clone" lint message text is hardcoded to
  `"…copied into the JSON value"` / `"`JSON.<call>` stores its own copy…"`. So
  `path.join(a, b)` with borrowed-dead `a`/`b` warns "`JSON.join` stores its own
  copy", naming the wrong type. A diagnostic-text bug only (codegen unaffected,
  parity holds) — surfaced while testing Phase 10's `path.join`. Fix: parameterise
  the message on the real receiver type/module, or scope the lint to JSON
  constructors. (Phase 10.)
- **Recursive (`Box<…>`) STRUCT construction is broken (I2 hole on the AST path).**
  `emit_struct_lit` (Expression.rs `Expr::StructLit`) emits `field: <expr>` with NO
  `Box::new(…)` wrapper, but a self-referential struct field has Rust type `Box<…>`
  (`struct_field_rust`/`field_rust_type`). So `Tree { value: 2, child: value(leaf) }`
  emits `user_child: Some(user_leaf)` where the field is `Box<Option<user_Tree>>` →
  rustc E0308 ("expected `Box<…>`, found `Option<…>`"). Recursive ENUM construction is
  fine (`emit_boxed_enum_arg` DOES wrap in `Box::new`), but struct literals don't.
  Masked because no example/test constructs a recursive struct. Phase 16 EXCLUDES
  recursive structs (the struct gate's visited set already did). Fix: wrap a boxed
  struct-lit field value in `Box::new(…)` in `emit_struct_lit`, keyed on
  `cx.boxed_edges`. (Surfaced in Phase 16.)
- **A borrowed non-Copy struct-lit field value isn't cloned (E0507, AST path).** A
  struct literal whose field value is a borrowed-in-env ident (`Person { name: n }`
  where `n: String` is a `Read` param → `&String`) emits `user_name: (*user_n)` →
  rustc E0507 ("cannot move out of `*user_n`"). `field_read_to_clone` rewrites owning
  *field reads* but not a bare borrowed ident used as a struct-lit value. This is the
  pre-existing `Read`-convention struct-param bug (Phase 3 latent list) in a new dress.
  Both paths miscompile identically (parity holds); NOT separately gated — the
  construction is the same broken Rust on both paths. Fix lives in sema's elaboration
  (clone a borrowed non-Copy struct-lit value), where the field-read clone already
  lives. (Surfaced in Phase 16.)
- **A payload/named enum-variant literal NEVER becomes an `Expr::EnumLit` node.** The
  parser produces `Expr::Field` for a unit variant (`Light.Red`) and `Expr::MethodCall`
  for a payload variant (`Expr.Wrap(x)`); sema type-checks the payload form via
  `check_enum_lit` *in place* but does NOT rewrite the node (CheckerInfer ~L2118). So
  `Expr::EnumLit` is constructed by neither the parser nor sema — it is only ever
  *matched*. Every payload/recursive enum CONSTRUCTION reaches codegen as a `MethodCall`
  routed to `emit_enum_lit` by `emit_method_call` (Expression.rs ~L1635). Not a bug per
  se, but a load-bearing fact: Phase 16 covers construction via a new variant-
  construction MethodCall shape, not the (effectively dead) `Expr::EnumLit` lowering.
  (Surfaced in Phase 16; consistent with the Phase-4/8 "the AST node that reaches
  codegen is what the gate must recognise" finding.)
- **A `view`-returning TRAIT method miscompiles (I2 hole on both paths).** `emit_trait_def`
  (Source/M9.rs ~L454) renders a trait method's DECLARATION return type via
  `rust_type_name(m.return_type)` WITHOUT consulting `m.is_view_return`, so the trait
  declaration emits `fn label(&self) -> String` while the impl (`emit_trait_method` /
  the TIR `emit_tir_trait_method`) emits `fn label(&self) -> &String` → rustc E0053
  ("incompatible type for trait"). Both paths produce identical (broken) Rust, so parity
  holds, but the construct is unusable. Phase 19 EXCLUDES view-returning trait methods for
  this reason (the TIR must not *claim* a miscompiling fn — the `is_empty` precedent); the
  borrow shape is otherwise the same total `TStmt::ViewReturn { wrap }` Phase 17 used for
  inherent/free view methods, so the lift is trivial once `emit_trait_def` threads
  `is_view_return` into the declared return type. (Surfaced in Phase 19.)
- **A method on a GENERIC struct doesn't type-check (sema gap).** A struct-body
  method on `struct Stack<T> { … fn size(self) -> Int { … } }` is NOT bound to the
  type — sema reports E0311 ("`size` isn't a method on this value") at the call site,
  and any method body that references the type var `T` additionally hits E0119 ("no
  type called `T`" — the impl-level type params aren't in scope in the method body).
  So there is NO valid Jet program with a generic-struct method today; the construct
  is unreachable on BOTH paths. (Phase 19's "probe showed byte-identical emit" built
  the AST directly in a `build_cx`-only unit test, bypassing sema's method binding.)
  Consequence: covering "generic-type methods" in the TIR is moot until sema binds
  type params into struct-body method scopes + registers the methods on the generic
  type. Logged in Phase 20 (the gate's `struct_is_generic` exclusion stays, correctly
  conservative). Fix lives in sema's method registration/scope, independent of c109.
- **`http.serve(addr, handler)` doesn't propagate the handler's `Fn(HttpRequest)->
  HttpResponse` type to a lambda arg (sema gap).** `infer_core_call`'s serve arm
  (CheckerCoreLib ~L741) `self.infer(&mut args[1].expr)` with NO expected type, so an
  UNANNOTATED serve lambda `(req) => …` hits E0801 ("tell me the type of `req`") and
  its body's `return HttpResponse{…}` is mis-attributed to the enclosing fn (E0113).
  So a serve handler lambda MUST annotate its param (`(req: HttpRequest) => …`). The
  HttpRequest/HttpResponse accessors therefore reach codegen only on a TYPED param
  (a free fn or annotated lambda), where the slot type is already total — which is how
  Phase 20 covers them (no lambda-param writeback needed). Fix: set the serve arm's
  expected type to `Fn(HttpRequest)->HttpResponse` before inferring the handler.
  (Surfaced in Phase 20.)
- **A bare `?? return` (no value) is unusable (sema/codegen mismatch).** `x ?? return`
  (no fallback value) emits `match … { None => return }`. Sema E0405 REQUIRES the enclosing
  fn to have a return type ("a bare return needs a function with a return type"); but rustc
  then rejects `return;` in a non-unit function (E0069). So a bare `?? return` cannot appear
  in any program that passes BOTH sema and rustc. Phase 19 confirms the shape ALREADY routes
  through the TIR (the gate's `orfallback_rhs_in_subset → Return(None) => true`, an earlier
  phase) and is byte-identical on both paths — the inventory note claiming it doesn't route
  was stale. The construct is just dead. Fix: make sema admit a bare `?? return` only in a
  unit-returning fn (then rustc accepts `return;`), OR make codegen emit `return ()` /
  diverge appropriately. (Surfaced in Phase 19.)

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
  `Expr::MethodCall.recv_type: Option<String>` (+ `resolved_ret: Option<Type>`, c109
  Phase 20 — the arg-dependent return type of a polymorphic core special);
  `Expr::OptField.flatten: bool`;
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

## Refinements learned in Phase 5 (collections)

- **`IndexKind` is the total fact; never re-derive Map-vs-List in emit.** Sema
  (`CheckerCore`/`CheckerInfer`) resolves each `Expr::Index`/`LValue::Index` `kind`
  to `List` or `Map`. The TIR carries it as a plain `is_map: bool` and the emitter
  dispatches `jet_index_map`/`jet_map_insert` vs `jet_index_vec`/`[i as usize] = v`
  off that bool alone. The gate **excludes any `IndexKind::Unknown`** — the AST path
  falls back to an env type-inference (`expr_jet_ty(base)`) for the unresolved case
  that the TIR must not reproduce; an unresolved kind means sema didn't run/resolve,
  so staying on the AST path is the safe choice.
- **The iteration loop var has an *unresolved* type — reproduce that partiality.**
  `emit_for_in` binds the loop var's slot with `jet_ty: None`, so a body `x + 1`
  resolves the var to `None` in `operand_is_integer` and does **not** trap. Carrying
  the resolved element type (`Some(Int)`) onto the TIR var would diverge — `x + 1`
  would wrongly emit `jet_add`. So `LowerEnv.locals` became `(place, Option<Type>)`
  and iteration vars (both single and the two-binding map `k`/`v`) bind as `None`.
  This is the recurring Phase-3 lesson: when a TIR fact is *more* total than the AST's
  re-derivation, parity demands reproducing the AST's partiality where it's
  load-bearing (here, the overflow-trap decision). Range-loop vars keep `Some(Int)`
  (the AST slot is `jet_ty: Some(Int)`), so they still trap — matching the AST path.
- **Method-call collections take a different `emit_for_in` branch — exclude them.**
  `loop c in s.chars()` / `loop line in h.lines()` lower to char iteration / streaming
  `BufRead::lines` reads, not `.iter().cloned()`. The gate excludes any iteration whose
  collection is an `Expr::MethodCall` (and methods are out of subset anyway → Phase 7),
  so only the two plain `.iter()` shapes (`.iter().cloned()` single; `.iter()` + per
  key/value `.clone()` two-binding) ever arise — both reproduced byte-for-byte.
- **Element clones live in the iteration form, not a TIR decision.** The single-binding
  form clones each element (`.iter().cloned()`); the two-binding map form clones key and
  value (`_jet_k.clone()`/`_jet_v.clone()`). These are fixed properties of the loop
  shape (the var owns its value), reproduced verbatim — no per-element clone fact the
  subset has to derive. Literal sites and field/index values are emitted as-is (mirroring
  `Expr::ListLit`/`MapLit`/`Index`): a value's own move/clone facts live in its
  sub-expression, exactly as Phases 3–4 established.
- **Empty `[]`/`[:]` need no special handling.** Sema's E0501 already requires a context
  type for an empty literal, and a covered binding/param/return supplies it; the emitted
  `vec![]` / `BTreeMap::new()` is type-inferred by Rust from that context — byte-identical
  to the AST path. The TIR's placeholder result-type for an empty literal is never read in
  emit (it only feeds totality/overflow facts that an empty collection can't reach).
- **`FixedList` (`[E#N]`) deferred.** A fixed-size list param/return uses `Vec<E>` like a
  list, but its construction/indexing semantics differ enough that it stays on the AST path
  (Phase 7) rather than risk a mis-lowering.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass) on a program exercising list literal + index + slice + index-assign +
  single-binding iteration + map literal `[:]` + map index + map insert + two-binding
  map iteration — 248 lines, byte-for-byte identical, then the instrumentation removed.

## Refinements learned in Phase 6 (methods + clones)

- **The sema-inserted `.clone()` is the high-value unlock, and it is dead simple to
  emit — but only because sema did the work.** `CheckerInfer::field_read_to_clone`
  rewrites a non-`Copy` owning field read (`return p.name`, a struct-lit field value)
  into an `Expr::MethodCall{method:"clone"}` *before* codegen (the Phase-3 finding).
  So the TIR's clone shape is just `(recv).clone()` with the receiver lowered
  in-subset — no deref/borrow decision, because the receiver is already the exact
  place the AST path's `emit_method_call` clone early-return would emit. This is what
  lets a covered function finally *return or store* a non-Copy field (the getter),
  which Phases 3–5 had to exclude. The clone decision lives in sema's elaboration; the
  TIR just carries the rewritten node.
- **`emit_builtin_method` dispatches on the method NAME, not the receiver type — so the
  gate must exclude by name.** A large set of arms (`len`/`push`/`get`/`map`/`filter`/
  `to_string`/`trim`/`keys`/… and the D-NUMOPS1 width/predicate/bit methods) fire on the
  method name alone, *before* the user-method dispatch at the bottom of `emit_method_call`.
  A user method sharing one of those names is lowered by that bespoke path on the AST
  side, never via `method_sigs`. The gate therefore carries an explicit
  `is_intercepted_method_name` superset (every name those paths mention, guarded or not)
  and excludes any match. Excluding extra is always safe (stays on the AST path); a
  missed name would be a silent mis-dispatch. The same set excludes the special-cased
  `clone`/`raw`/`snapshot`/`new`.
- **`recv_type: None` is the total signal for "not a user instance method" — never
  reproduce the AST fallback.** Sema sets `recv_type = Some(T)` only when the call
  resolves to a user method on a concrete type `T` (CheckerInfer ~L2349). A **static**
  call (`Type.m()`) returns from `check_static_method` *without* setting it, so its
  `recv_type` is `None`; the AST path then has a fallback (`receiver_struct_type` /
  `cx.type_names.contains`) the subset must not reproduce. Gating on `recv_type ==
  Some(covered T)` cleanly admits instance calls and excludes static calls, trait-object
  calls (a trait `recv_type` isn't a covered struct/enum type → excluded), and every
  core/handle method (their `recv_type` is a prelude/foreign name, excluded by
  `is_covered_*`). This is the Phase-5 `IndexKind::Unknown` lesson in a new dress: a
  partial sema fact means *stay on the AST path*.
- **The call-site receiver needs NO convention handling — Rust autoref does it.**
  `emit_method_call`'s final dispatch emits `(recv).{method}({args})` with the receiver
  emitted as-is (its place); Rust's method autoref supplies `&`/`&mut`/by-value to match
  `&self`/`&mut self`/`self`. So a `mut self` call site is byte-identical to a `self`
  one, and the TIR carries no `self` convention at all (the speculative `method_self_conv`
  table was removed as dead — totality means carrying facts the emitter *reads*, not
  facts that *might* matter). Only the **arguments** carry conventions, mirrored from
  `method_sigs` + `CallArg.flags` exactly as `emit_call_args` (clone/Arc wrapper first,
  then `&(…)`/`&mut (…)`).
- **Trait-impl method names are bare, decided at lowering from `cx.trait_methods`.**
  S62: a method from a trait impl (`(Type, m) ∈ trait_methods`) is called by its bare
  Rust name (the impl owns it), not `user_<m>`. The TIR resolves `method_rust` once at
  lowering off that set — the same check the AST path makes inline — so emit never
  consults `trait_methods`.
- **A method-call result type is rarely load-bearing, but kept total anyway.** A binding
  carries sema's `b.ty` (so the emitted `let` is unaffected by the method result type),
  and arithmetic on a method result doesn't trap in *either* path (the AST `expr_jet_ty`
  resolves a MethodCall via its receiver → a Named struct → `is_integer()==false`; the
  TIR's `ast_operand_is_integer` returns `None` for a MethodCall — both agree: no trap).
  Still, the TIR reads the resolved return from a new `cx.method_rets` table for totality
  per the design principle, never `unit_type()`-guessing when a real type exists.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass) on six programs: a covered-function owning-field clone (the getter unlock), a
  getter method, a user method with scalar args, a method with a String arg (implicit
  clone), a trait-impl method call (bare name), and an instance method on a covered enum
  — all byte-for-byte identical, then the instrumentation removed.

## Refinements learned in Phase 7 (method bodies + static methods)

- **A method BODY routes through the TIR by hooking `tir_covers_method` into
  `emit_method`** (inherent methods, `Source/Codegen/Items.rs`). The `self` slot
  is reproduced exactly as `emit_method` builds it: place `self`, **no deref**,
  type `None`. `self.field` then emits `(self).field` and a `when self` match emits
  `match self {` (no `(…).clone()`, because the clone fires only on a *deref'd* slot
  — a by-reference enum *param* — and `self`'s slot has `deref: false`). Carrying the
  self type as `None` (not `Some(T)`) is load-bearing for parity: it matches
  `emit_method`'s `jet_ty: None`, so any overflow-trap decision that consulted the
  self slot agrees. (In the covered subset this never differs — `self` is a
  struct/enum, never a bare arithmetic operand, and `self.field` is a `Field` →
  `None` either way — but the slot is built identically regardless.)
- **The `self` receiver form is the ONLY method-specific signature fact, carried as
  `TFuncKind::Method { self_conv: Option<AccessConvention> }`.** `Read`→`&self`,
  `Mutate`→`&mut self`, `Move`→`self`; a static method carries `None` (no receiver).
  The emitter (`emit_tir_method`) prints `    pub fn user_<m>(<self>, <params>) -> …`
  at indent 1 inside the `impl` block the caller already opened — byte-identical to
  `emit_method`. (Top-level functions keep `TFuncKind::TopLevel` → `emit_tir_toplevel`.)
- **`mut self`/`view self` reassignment (`self = …`) is a pre-existing AST-path I2
  hole and is gated OUT.** The self slot does not deref on an assignment LHS, so the
  AST path emits `self = …` where `self` is `&mut user_T` — rustc rejects it (E0308).
  This is independent of the TIR (it miscompiles on both paths identically), but the
  gate excludes any method whose body assigns to a local named `self`
  (`stmt_assigns_self`) so the TIR never *claims* a method that miscompiles. (Field
  assignment `self.field = v` is already E0003 — not a Jet construct — so the only
  `self` mutation a `mut self` method can express in-subset is reassignment, which is
  excluded; a covered `mut self` method therefore reads/returns like a `self` method
  but with a `&mut self` receiver. Fixing the `self`-reassignment lowering is a
  separate codegen-bug task, not a TIR-coverage one.)
- **Static (associated) methods are covered on BOTH sides.** The body lowers via the
  same `lower_method` (no self slot, `self_conv: None`). The CALL site (`Type.make(x)`)
  — which Phase 6 deferred (its `recv_type` is `None`) — is a new `TExprKind::StaticCall`
  resolved at lowering to `user_<Type>::user_<method>(args)`, mirroring the AST
  type-name dispatch (Expression.rs ~L1644). The gate (`static_method_call_in_subset`)
  admits it only when the receiver is a bare *type-name* ident (not a local), the type
  is covered, the method is a registered `method_sigs` entry that is NOT an enum
  *variant* (an `Enum.Variant(args)` receiver+method emits an enum literal, a different
  lowering) and NOT a builtin/special intercept (`new`, etc.).
- **`Self` is resolved to the owning type at lowering** (`resolve_self_ty`), for
  params and the return. (In current Jet a literal `-> Self` rarely type-checks — sema
  treats `Self` and the concrete type as distinct, E0113 — so realistic constructors
  return the concrete name; the resolution keeps the gate total either way.)
- **Single-uppercase-letter type names read as type vars** (`Generics::is_type_var_name`
  — `len()==1 && uppercase`), so `is_covered_struct_ty("C")` is false. A real trap for
  unit tests (use multi-letter names like `Cell`/`Acc`); irrelevant to real programs.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass on both `emit_func` and `emit_method`) on: a struct with static constructors
  (`origin`/`make`), a `self` getter, a `mut self` getter, a method returning a fresh
  struct, an enum method (`when self` match), a method calling another method on `self`
  with a String arg, and the real example suite (`10_structs.jet` `dist_sq`/`unit`,
  `63_named_args.jet` `Rect::new`/`area`) — all byte-for-byte identical, then the
  instrumentation removed. ~4 example methods now route through the TIR
  (`Point::dist_sq`, `Point::unit`, `Rect::new`, `Rect::area`).
- **Still on the AST path (deferred):** **trait-impl** method bodies (`emit_trait_method`
  has a distinct signature — bare name, no `pub`, always `&self`, self slot
  `jet_ty: Some(T)`; a separate, provable-parity hook); **delegation** (`using field`)
  methods; **generic-type** methods; **`view`-returning** methods (subtle borrow
  lowering); methods whose body uses any not-yet-covered construct (core/stdlib calls,
  `?`/optionals, lambdas → Phases 8–10); and the `self`-reassignment lowering bug.

## Refinements learned in Phase 8 (fallible + optional)

- **`T?`/`T ? E` need NO special type lowering — `cx.rust_type` already does it.**
  `Type::Option`→`Option<…>` and `Type::Result`→`Result<…, …>` are pre-existing
  `rust_type` arms, and the TIR emitter already routes params/returns through
  `rust_param_type`/`rust_return_type`. So covering optionals/fallibles was purely a
  *gate widening* (`is_covered_fallible_ty`: an `Option`/`Result` whose payload(s) are
  covered value types, incl. sema's default `Error`→`String`) plus the expression/
  statement nodes — no new type-emit code. A list/map *of* options stays excluded
  (`collection_elem_covered` does not admit `Option`/`Result`): element clone for an
  option-element collection is a separate, deferred decision.
- **`TryConvert` is the total fact; the `?` trace-frame location must be resolved at
  LOWERING, not emit.** The AST `Expr::Try` reads `cx.current_fn`/`cx.src`/`cx.file`
  *at emit time*; but `emit_tir_toplevel`/`emit_tir_method` set `cx.current_fn` only
  just before emitting the body, and lowering ran earlier. So the `TExprKind::Try`
  node carries the pre-escaped `file`/`fn_name` and the resolved `line` as total
  fields (the enclosing fn name threaded onto `LowerEnv.fn_name`). The three convert
  arms reproduce `Expression.rs` byte-for-byte: `None`→bare `jet_trace_err(x,…)?`,
  `Fallible`→`.map_err(|e| e.to_error())` (D-ERR2), `Typed(fn)`→`.map_err(<fn>)`
  (D-ERR-CONV). All three verified byte-identical (None in `13_errors`, Typed in a
  scalar-payload `impl Source -> Target`, Fallible shares the same code path).
- **The `??` Panic fallback form is deferred.** `OrFallback::{Value,Return}` are
  reproduced exactly (`match … { Ok(v)/Some(v) => v, Err(_)/None => fb }`, with the
  `is_option` total fact picking Result vs Option). The `panic(…)` form
  (`emit_or_fallback_rhs`→`emit_panic_stop`) depends on `safe_locals_expr`, which
  iterates the *full Slot env* (rust_name/deref/jet_ty, sorted) — a per-function state
  the TIR's `LowerEnv` does not model 1:1. Rather than risk a divergent locals dump,
  the gate excludes any `??` whose fallback is `panic(…)` (`orfallback_rhs_in_subset`)
  — conservative, stays on the AST path. Reproducing it is a clean follow-up.
- **`value(x)`/`null` and `ok(x)`/`err(e)` are trivial constructor nodes**
  (`Some(x)`/`None`/`Ok(x)`/`Err(e)`) — but a scalar-payload *error-enum* literal
  inside `err(…)` (`Bad.Code(1)`) **parses as a `MethodCall`, not an `EnumLit`**, and
  is only rewritten to `Expr::EnumLit` by *full sema* (Phase-4 finding, again). So a
  `build_cx`-only gate unit test can't see the rewrite (it sees a static-method-call
  shape, excluded); the constructor path is proven end-to-end by `tests/tir.rs`. A
  bare `err("msg")`/`ok(x)` over scalars/strings parses directly as `Expr::Ok`/`Err`,
  so those *are* gate-testable without sema.
- **`?.` optional chaining reads the `flatten` total fact** (sema): `true`→`.and_then`
  (the field is itself optional), `false`→`.map`, reproducing
  `(base).clone().{and_then|map}(|__optv| __optv.{member})` exactly. A nested
  `a?.b?.c` chains these; verified byte-identical incl. a mixed map/and_then chain.
- **`when ok/err/value/null` is a third switch shape (C).** It reuses the `EnumMatch`
  TStmt (a Rust `match` with `Ok(b)`/`Err(b)`/`Some(b)`/`None` patterns + the same
  `_ => unreachable!()` fallthrough for the no-`else` case), with the bound payload's
  type read from the subject's resolved `Result`/`Option` (totality) — reproducing
  `add_pattern_bindings`. The subject can be a covered *fallible user fn call* (its
  return type is total from `cx.fn_types`); a deref-clone scrutinee never arises (a
  fallible/optional subject in-subset is a value, not a by-reference enum param).
- **Verified byte-identical** via a forced-AST-path diff (temporary `JET_NO_TIR`
  bypass) on: `examples/features/13_errors.jet` (`T ? E` + `ok`/`err` + `?` + `??`
  value + `when ok/err`), `12_option.jet` (`T?` + `value`/`null` + `??`),
  `72_typed_error_families.jet` (unchanged — its String/nested-enum error payloads
  keep it on the AST path, no regression), and three crafted programs (a Typed
  error-conversion `?`, an optional `?.` chain incl. flattening, a `?? return`) — all
  byte-for-byte identical, then the instrumentation removed. ~5 example fns now route
  through the TIR (`13_errors` `parse_age`/`load`/`main`, `12_option`
  `find_even`/`main`).

## Refinements learned in Phase 9 (builtin collection/string methods)

- **A builtin collection/string call carries `recv_type == None` — that is the total
  routing signal.** Sema resolves `xs.push(1)`/`s.trim()`/`m.keys()` via
  `Collections::builtin_method_return` and leaves `recv_type` unset (it sets it ONLY for
  numeric width conversions — `is_numeric()` at CheckerInfer ~L2248 — and for user-instance /
  handle methods). So `recv_type.is_none()` + a covered builtin name + an in-subset *value*
  receiver uniquely identifies a builtin call: a struct/enum/handle/numeric receiver would
  have set `recv_type`, and a bare type-name (static-call) receiver isn't a local so it fails
  `expr_in_subset`. The builtin shape is tried in `method_call_in_subset` BEFORE the static and
  instance shapes (both keyed on `recv_type`), claiming builtins first. **No type info is
  needed in the gate** — the receiver type only matters for the *emit branch*, resolved at
  lowering. This is the Phase-6 lesson again: a partial sema fact (`recv_type == None`) is the
  total signal for "not a user/handle/numeric method."
- **The intercept set stays intact; a dedicated builtin GATE handles the routing — the literal
  "remove the name from `is_intercepted_method_name`" instruction is unsafe.** Removing a name
  un-guards the *user-method* instance/static shapes (3/4), and a user method named `len`/`get`/…
  on a struct is MISCOMPILED on the AST path by the builtin-name collision (the receiver-keyed
  `emit_builtin_method` fires before user dispatch — a noted latent bug). So routing such a
  user method through the TIR's user-method path would DIVERGE from the (buggy) AST output. The
  parity-preserving design adds a separate builtin shape gated on receiver type (List/Map/String
  via `recv_type == None`), which is disjoint from user methods (Named receiver → `recv_type` Some)
  — the two never collide. `is_intercepted_method_name` is kept whole so the user-method shapes
  still exclude collision names (correct: those stay on the AST path).
- **`emit_builtin_method` args are emitted PLAINLY — no clone/borrow wrappers (unlike
  `emit_call_args`).** Each `arg(i)` is a raw `emit_expr`; the `CallArg.flags`
  (implicit_clone/shared_auto_clone) and the param convention are IGNORED. So `TExprKind::
  BuiltinMethod` carries args as plain `Vec<TExpr>`, NOT `Vec<TCallArg>`, and emits them with
  no wrapper. Carrying conventions here would add spurious `&(…)`/`.clone()` and break parity.
- **The Map-vs-List-vs-String branch is `expr_jet_ty(receiver)` — reproduce its partiality
  exactly.** `len` forks String (`jet_char_len`) vs else (`.len() as i64`); `insert`/`remove`/
  `get` fork Map vs List. The AST keys these on `rty = expr_jet_ty(receiver, env)`, which is
  PARTIAL: `Ident`→slot type, `Str`→String, `Char`→Char, `MethodCall(chars/split)`→typed list,
  else recurse, and **everything else (notably a struct `Field` read) → `None`**. A `None` rty
  lands on the *default* (list/else) branch. `tir_recv_jet_ty` mirrors `expr_jet_ty` bit-for-bit
  (incl. `Field → None`), resolved once at lowering into a concrete `TBuiltinOp`, so emit makes
  no type decision (I3). A divergence here would flip a branch — the recurring "reproduce the
  AST's partiality where load-bearing" lesson.
- **`recv_type == None` is reused by static + builtin shapes; order matters.** Both the static
  call (`Type.make()`) and a builtin call have `recv_type == None`. The builtin shape is checked
  first (in both the gate and `lower_method_call`), but a static receiver (a bare type-name ident)
  is NOT in `locals`, so `expr_in_subset(receiver)` is false → the builtin shape declines and the
  static shape claims it. A builtin receiver (a list/string local/literal) IS in-subset → builtin
  shape claims it. The two are cleanly separated by receiver-is-a-value vs receiver-is-a-type-name.
- **Panic-frame lines are resolved at lowering from the real `method_span`/receiver span.**
  `remove`-on-list embeds `span_line_col(method_span.start)`; `slice` embeds
  `span_line_col(receiver.span().start)`. Both are read off the AST `MethodCall.method_span` /
  receiver span at lowering and carried as plain `usize` on the op, so emit (which never sees
  `cx.src`) reproduces the AST's `jet_list_remove(…, file, line)` / `jet_string_slice(…, file,
  line)` byte-for-byte. `cx.file`/`cx.root_prefix` are program-level, read at emit.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass on both
  `emit_func`/`emit_method`) on a program exercising the full covered surface — list ops
  (push/insert/reverse/sort/len/pop/first/last/get/index_of/contains), string ops (len/to_upper/
  to_lower/trim/split/starts_with/ends_with/replace/repeat/slice/chars/bytes/contains/to_string),
  map ops (insert/len/get/contains_key/keys/values/clear), and both `remove` forms (list helper +
  map clone) and `join(sep)` — byte-for-byte identical, then the instrumentation removed. The
  `is_empty` E0308 (latent bug) reproduces identically on both paths, confirming the gate's
  exclusion of it is conservative (it would miscompile on either path).

## Refinements learned in Phase 10 (core/stdlib calls)

- **A core call is a `MethodCall` whose receiver is a module-alias `Ident`, with
  `recv_type == None` — that triad is the total routing signal.** `infer_core_call`
  (CheckerInfer) returns WITHOUT setting `recv_type`, so a core call shares
  `recv_type == None` with the Phase-9 builtin shape and the Phase-7 static shape.
  It's separated by the receiver: `cx.core_imports.get(alias).is_some()` (a module
  alias, NOT a local, NOT a covered type-name). The core shape MUST be tried FIRST
  in both the gate and `lower_method_call` — a core method named `get`/`split`/
  `parse`/… would otherwise be claimed by the builtin shape's `return` (which then
  rejects it, since a module alias fails `expr_in_subset`), wrongly excluding the
  call. Disjointness holds: builtin needs a *value* receiver, static needs a
  *type-name* receiver, core needs a *module-alias* receiver.
- **`Sema::core_fixed_sig` is the authoritative total source — gate on it, type from
  it.** Gating coverage on `core_fixed_sig(module, method).is_some()` cleanly admits
  exactly the **type-monomorphic** calls and excludes every deferred one for free:
  the closure-takers (`spawn`/`serve`/`guard`) and handle-constructor specials
  (`tasks.channel`/`http.router`/`parse`/`dispatch`) aren't in the table (or map to
  `None`); the polymorphic math/random/io specials (`abs`/`min`/`max`/`clamp`/
  `pick`/`shuffle`/`input`/`eprint`) are typed by bespoke `check_core_call` logic,
  not the fixed table, so their return type isn't total without re-inference (I3) —
  correctly excluded. The table's return type is the node's total `ty` (a `None`
  return → `Unit`), which is what makes a fallible core call (`fs.read` →
  `Result<String, IOError>`) compose with Phase-8 `?`/`??`: the `?`/`??` unwrap
  reads the Ok payload off this `ty`.
- **Core-call args are emitted PLAINLY — the per-arm wrapper is baked into the emit
  match, not a TIR field.** `emit_core_call`'s `arg(i)` is a raw `emit_expr`; it
  ignores `CallArg.flags` (implicit_clone/Arc) AND the param convention. The `&(…)`/
  `&mut (…)`/by-move (`tcp_reply`) wrappers are hardcoded per `(module, method)` arm
  (e.g. `fs.write` → `(&(a0), &(a1))`, `net.tcp_read` → `(&mut (a0))`). So `CoreCall`
  carries args as a plain `Vec<TExpr>` (like Phase-9 `BuiltinMethod`, NOT
  `Vec<TCallArg>`), and `emit_tir_core_call` reproduces the `emit_core_call` match
  verbatim. Carrying conventions here would add spurious wrappers and break parity.
- **The `(module, method)` dispatch is decision-free in the I3 sense.** Unlike
  Phase-9's Map-vs-List branch (which needed `expr_jet_ty` → resolved into a
  `TBuiltinOp` enum), the core dispatch is a pure syntactic match on two
  already-resolved strings — no type inference — so the TIR carries `module`/`method`
  as strings and the emitter matches them directly. `cx.root_prefix`/`cx.ffi_crate`
  are program-level (read at emit, like Phase-9's `cx.file`), never a per-node
  decision. A regex call's `<ffi_crate>::jet_regex_*` form reproduces `regex_fn`
  (reading `cx.ffi_crate`) exactly.
- **A handle-PRODUCING call is coverable; a handle-USING method is not.** `files.open`
  → `Result<FileReader, IOError>`, `net.tcp_connect` → `TcpStream`, `time.start` →
  `Stopwatch` all emit a plain helper call (parity-exact) and are covered. Binding the
  result (`let f = files.open(p)?`) is fine; the moment a METHOD on the handle is
  called (`f.read_line()`), that method is itself out of subset (a handle `recv_type`,
  intercepted name) → excludes the enclosing fn. So covering the CALL never reaches an
  uncovered handle surface. This is the recurring seam: cover the node you can make
  total, let the next uncovered node exclude its function.
- **`fs.read(p)?` does NOT route through the TIR — but `fs.read(p) ?? fb` does.** A
  bare `?` on a core fallible (`IOError`/`JsonError` err) requires the enclosing fn to
  return `… ? IOError` (the err types match → `TryConvert::None`); but the gate's
  `is_covered_fallible_ty` excludes `… ? IOError` returns (only `Error`→String and
  covered value-type payloads are admitted), so the whole fn stays on the AST path. A
  `… ? Error` return would need `impl IOError: Fallible`, which sema rejects (IOError
  isn't user-definable). So core fallible composition through the TIR happens via the
  `??` value/return fallback (Phase 8), whose enclosing fn returns the unwrapped value
  type. Verified `fs.read(p) ?? "missing"` byte-identical.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass on both `emit_func`/`emit_method`) on three programs covering 24 core calls —
  fs (write/append/copy/rename/remove/create_dir/read/read_bytes/list_dir/exists/
  is_dir), math (sqrt/pow/floor/ceil/round), env (set/get/home_dir/current_dir), path
  (join/parent/extension/normalize), time (now/start), random (int/float/seed), json
  (parse/render/render_pretty), crypto (sha256/sha256_bytes), csv (parse/render), regex
  (is_match/find_all/replace_all, incl. the `cx.ffi_crate`-qualified form), log
  (info/warn/error/debug/set_level/setup), jet.time (now/format), and `fs.read ?? fb`
  composition — all byte-for-byte identical, then the instrumentation removed. A
  sentinel-injection probe confirmed the covered fns actually take the TIR path (24
  `CoreCall` nodes emitted across the programs).

## Refinements learned in Phase 11 (lambdas/closures + fan-out)

- **`Lambda.meta` is the whole capture/escape model — codegen lowers, never
  re-derives (I3).** Sema fills `escapes`/`needs_fn_mut`/`mut_captures`/
  `cloned_captures`; `lower_lambda` reads exactly those four facts and reproduces
  `emit_lambda` byte-for-byte: `move ` keyword iff `needs_fn_mut && !escapes` is
  *false* (i.e. emit `move ` unless it's a non-escaping FnMut), `Box::new(…)` iff
  `escapes`, and a `let _jet_cap_<n> = (place).clone();` prelude per cloned capture.
  No capture analysis runs in codegen — the TIR carries the rendered prelude/params/
  body and emit is a pure wrapper. The capture's *source place* comes from the
  **outer** env (it's an outer local); the cloned-capture name rebinds to
  `_jet_cap_<n>` with `deref:false, jet_ty:None` inside the lambda body, matching the
  AST slot exactly.
- **A lambda body is a scope; lower it on a cloned env extended with params +
  captures.** Same in-subset recursion as if-branches/loop-bodies (Phases 2–5). An
  expression body lowers + emits directly; a block body lowers its statements and
  emits `{ … }` at indent 1 — byte-for-byte `emit_lambda`'s `emit_stmts(…, 1, false)`.
- **The load-bearing NEW exclusion: a callee with a Fn-typed parameter.** Before
  Phase 11 no fn-value could appear in-subset as a call arg (lambdas + bare fn-name
  idents were both excluded), so `emit_call_args`'s `Box::new(…) as <fn-type>`
  coercion path was never reached on the TIR side. Now that lambdas are covered, a
  call like `apply((n)=>n+1, x)` or `twice(bump, 10)` would route through the plain
  `Call` lowering and MISCOMPILE (missing the Box coercion). The fix: the `Call` gate
  now excludes any callee whose signature has a `Type::Fn` param (`no_fn_param`).
  Conservative — the whole enclosing fn stays on the AST path. (Covering fn-typed
  values + the Box coercion is the deferred slice of this phase.)
- **Closure methods compose a lambda with `emit_builtin_method`'s closure arms.**
  Each arm (`map`/`map_mut`/`filter`/`each`/`each_mut`/`each_ref`/`map_each`/`find`/
  `any`/`all`/`sort_by`/`reduce`) became a `TClosureOp` resolved at lowering. The
  Map-vs-trait-object-list branch reads `tir_recv_jet_ty(receiver)` (the same
  `expr_jet_ty` the Phase-9 builtins use); the Fn-vs-FnMut branch reads the lambda
  arg's `needs_fn_mut` — both decided once, emit only formats. The gate requires the
  closure-arg position to be a **literal `Expr::Lambda`** (a fn-value there defaults
  to the non-mut form on the AST side, which needs the deferred fn-value emit). The
  args are emitted PLAINLY (raw `arg(i)`), like every builtin — the receiver is
  `.clone()`d inside the helper-call form, not a per-arg decision.
- **Fan-out's Ident-callee path is a synthetic single-arg call per item.** The AST
  `Expr::FanOut` routes an `Ident` callee through `emit_call` with a synthetic
  `CallArg { convention: Read, flags: Default }` per item, then `vec![…]`. Reproduced
  as a `TExprKind::Call` per item (the borrow wrapper comes from the callee's param-0
  convention; `implicit_clone` is false on the synthetic arg), wrapped in `FanOut`.
  Only the **plain-top-level-fn Ident** callee is covered — a fn-value callee
  (`(f)(item)`) needs the deferred fn-value emit. The result is `[T#N]` (S76), erased
  to `Vec<T>`; it doesn't unify with a plain `[T]` return (sema E0113), so fan-out is
  bound/indexed/destructured, not returned-as-`[T]`.
- **Deferred (stay on the AST path), with reason:**
  - **fn-typed values** — a fn stored in a binding, passed as an arg, or returned;
    the `Box::new(…) as <fn-type>` coercion (`emit_call_args` fn-arg path) +
    `CallValue` `(f)(args)` emit. Gated out via the new Fn-param `Call` exclusion and
    by `CallValue`/fn-value-ident never being in-subset.
  - **`tasks.spawn`/`http.serve`/`scope.guard`** — `spawn` uses a *distinct*
    `emit_spawn_lambda` form (`move |…|` with no `Box::new`); `serve` branches on
    router-vs-lambda and wraps a router in a fresh closure; `scope.guard` is not in
    `core_fixed_sig` (so excluded by `core_call_covered`). Each is a small bespoke
    emit shape; covering them is a clean follow-up now that lambdas are in subset.
  - **the `??` panic fallback** — still deferred (Phase 8's note): `emit_panic_stop`/
    `safe_locals_expr` dump the *full Slot env* (rust_name/deref/jet_ty, sorted), a
    per-function state the TIR's `LowerEnv` does not model 1:1. NOT lifted this phase
    — reproducing it byte-exact needs a richer TIR locals snapshot, and the
    `LowerEnv.locals` (name → place + `Option<Type>`) lacks the sorted Slot dump the
    panic frame embeds. Exclusion (`orfallback_rhs_in_subset` → `Panic => false`)
    stays; lifting it is the next follow-up.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass on both `emit_func`/`emit_method`) across the **entire 89-file example
  suite** — 0 differ — plus crafted probes exercising: list `map`/`filter`/`reduce`/
  `find`/`any`/`all` with expr-body lambdas, a Copy capture (`n + base`), a String
  list, a FnMut `each` (`jet_list_each_mut`, no `move`), `sort_by`, and fan-out over
  a plain fn. A routing sentinel confirmed the covered fns actually take the TIR path
  (`23_closures` `main`, `41_fan_out` `double`; ~86 example fns total route now).

## Refinements learned in Phase 12 (the tail)

- **A trait-impl method body needs its OWN hook + kind — `emit_trait_method` is a
  genuinely distinct signature, not `emit_method` with a flag.** The differences are
  all load-bearing for parity: a BARE method name (the trait owns it in Rust — no
  `user_` mangle), NO `pub`, an always-`&self` receiver (`emit_trait_method` ignores
  the source convention entirely), and a self slot typed `Some(Type::Named(T))` (NOT
  `None` like `emit_method`). So Phase 12 added `TFuncKind::TraitMethod { is_unsafe }`
  + `tir_covers_trait_method` + `lower_trait_method` + `emit_tir_trait_method`, hooked
  at the top of `emit_trait_method` (Source/Codegen/Items.rs) — which both
  `emit_trait_impl` (inline `impl Trait {}` / `impl T: Trait`) and
  `emit_external_trait_impl` (non-delegation) route through, so one hook unblocks all
  three trait-impl surfaces. Verified byte-identical on `25_traits.jet` (`Circle`/
  `Square` `area`/`name`).
- **The numeric methods are the ONE builtin slice keyed on `recv_type == Some` — the
  total routing signal is "a numeric type name."** Sema sets `recv_type =
  Some(recv_ty.name())` for a numeric receiver (CheckerInfer ~L2248), so a numeric
  predicate/bit/conversion method is uniquely a `MethodCall` whose `recv_type` parses
  via `AST::numeric_type_from_name` AND whose method is a covered nullary numeric op.
  This is disjoint from every other shape: Phase-9 builtins / Phase-10 core / Phase-7
  statics all need `recv_type == None`; user instance methods need a covered
  struct/enum `recv_type`. The width SOURCE for the widening-vs-narrowing decision is
  the total `recv_type` (the AST's `src = recv_type.or_else(rty.name())`, where
  `recv_type` is always `Some` here), so `numeric_conversion`/`conv_rust_target` are
  reproduced at lowering into a total `TNumericOp` (`Predicate`/`BitCount`/`CastAs`/
  `TryFrom`/`ToShow`) — emit makes no width decision (I3). A numeric `to_string`
  routes here too (it also sets `recv_type == Some`, so the Phase-9 `recv_type.is_none()`
  gate never claims it).
- **The `is_intercepted_method_name` set stays whole — the numeric gate is a separate
  shape tried BEFORE the instance-method `is_intercepted_method_name` exclusion.** The
  numeric names (`to_i32`/`is_nan`/…) are in the intercept superset (correct: a user
  method of that name on a covered struct must stay on the AST path, where the builtin-
  name collision would mis-dispatch it). Routing the numeric shape first, gated on a
  *numeric* `recv_type`, is disjoint from a user method (whose `recv_type` is a covered
  struct/enum), so the two never collide — the Phase-9 "keep the intercept set whole,
  add a disjoint receiver-typed shape" lesson, again.
- **Surfaced + FIXED a latent TIR-path parity bug: explicit `else { if … }` was being
  flattened to `} else if …`.** The AST `emit_if` (Statement.rs) renders `} else if`
  ONLY for `ElseBranch::ElseIf` — an explicit `else { … }` block stays `} else { … }`
  even when its body is a single `if`. The TIR `If` emit had been keying on the
  *else-body shape* (`if body == [If]`), which conflated the two and produced divergent
  (but still valid) Rust for an explicit `else { if … }`. Caught by the full-suite diff
  on `examples/showcase/jetgrep.jet` (`grep_dir`, an already-covered fn). Fixed by
  carrying the source distinction as `TStmt::If.else_is_elseif` (true only for
  `ElseBranch::ElseIf`); emit flattens iff that flag is set. This was a *false positive*
  in the parity sense (valid Rust, but not byte-identical) — exactly the drift the
  full-suite diff exists to catch.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass on `emit_func`/`emit_method`/`emit_trait_method`) across the **entire example
  suite** — 141 files, 0 differ (the `jetgrep` diff resolved by the `else`-block fix) —
  plus crafted probes exercising numeric widening (`to_i64` → `as`), narrowing (`to_u8`
  → `try_from`, unwrapped with `??`), int→float (`to_float`), float predicates
  (`is_nan`/`is_finite`), bit-pop (`count_ones`), numeric `to_string`, and both
  trait-impl forms (`impl Shape {}` inline + `impl Square: Shape`). A routing sentinel
  confirmed the covered methods take the TIR path (7 unique trait-method bodies route;
  53 free fns, 5 inherent methods route across the suite).

## Refinements learned in Phase 13 (the call & method surfaces)

- **A fn-typed VALUE has three syntactic shapes; all collapse to `emit_call_args`'
  Box-coercion + `emit_named_fn_value` + the `(f)(args)` emit.** (1) A bare top-level
  fn name in value position (`Expr::Ident` not a local/const, in `cx.fn_types` as a
  `Type::Fn`) emits `emit_named_fn_value`'s `Box::new(move |…| user_<name>(…)) as
  <fn-type>` wrapper — rendered ONCE at lowering into a `TFnValueKind::NamedFn`. (2) A
  call THROUGH a fn-value: `(f)(args)` parses as `Expr::CallValue`, but `f(args)` where
  `f` is a LOCAL parses as `Expr::Call { name: "f" }` (NOT CallValue) — the AST
  `emit_call`'s env-contains-name branch emits `(place)(args)` with args PLAIN
  (`emit_call_args(.., None)`). Both lower to `TFnValueKind::Call`. (3) A fn-typed ARG
  to a callee with a `Type::Fn` param routes through `emit_call_args`' coercion: the
  shared `lower_one_call_arg` (the single `emit_call_args` reproduction) carries a total
  `TFnCoerce { fn_type_rust, already_boxed }` — `already_boxed` (resolved at lowering)
  reproduces the AST's `s.starts_with("Box::new(")` heuristic (true for a bare fn-name
  value or a fn-typed local ident, so emit applies only ` as <fn-type>`, never
  re-boxing). The Phase-11 `no_fn_param`/`!matches!(pty, Type::Fn)` exclusions are
  **lifted**. A `Type::Fn` param/return is now a covered subset type (`is_covered_fn_ty`
  — it renders via `cx.rust_type`/`rust_fn_trait` exactly as the AST `rust_param_type`,
  by value, `param_place`'s deref matches `emit_func`).
- **The three closure-taking core calls are NOT in `core_fixed_sig` — each gets a
  bespoke node, not the plain `CoreCall`.** `tasks.spawn` uses `emit_spawn_lambda`
  (always `move |…|`, NEVER `Box::new` — a DISTINCT render from `emit_lambda`, so a
  separate `render_spawn_lambda`); `http.serve` (lambda handler) emits
  `jet_http_serve(&(addr), <lambda>)`; `scope.guard` emits `jet_scope_guard(<lambda>)`.
  Covered only with a LITERAL in-subset lambda in the closure-arg position (lambdas are
  Phase-11 in-subset, so the body lowers). `serve`'s router-handler branch is
  unreachable in subset (an HttpRouter value can only come from `http.router()`, not
  in `core_fixed_sig`). Their return types (`Task<T>`/Unit/`ScopeGuard`) match
  `infer_core_call`; rarely load-bearing.
- **Polymorphic core specials stay DEFERRED — confirmed NOT a total fact.** Their
  return type lives only in `infer_core_call`'s bespoke logic and is never written back
  onto the `Expr::MethodCall` node (only `recv_type` is annotated). Recovering it would
  re-run sema arg-type inference (I3 violation). `io.input` turned out to BE in
  `core_fixed_sig` (`String?` — already Phase-10-covered); only `io.eprint` + the
  math/random specials remain deferred.
- **"Handle method" splits by `recv_type`.** The handles whose method tables are
  `file_handle_method_return`/`net_method_return`/`alloc_method_return` carry
  `recv_type == Some(<handle>)` (FileReader/FileWriter/StdinHandle/TcpStream/
  TcpListener/HttpRequest/HttpResponse/Match/HttpRouter/Arena/…). Phase 13 covers the
  always-`let`-bound ones (FileReader/FileWriter/StdinHandle/TcpStream/TcpListener) as a
  total `THandleOp` keyed on `recv_type`, reproducing the `&mut`/`&` handle arms of
  `emit_builtin_method` byte-for-byte. Excluded with reason: **HttpRequest/HttpResponse**
  (arise only as a `http.serve` lambda param whose slot type is often `None` — the AST
  arm keys on `rty = expr_jet_ty`, which is then `None`, so it doesn't fire → covering
  would diverge); **Arena/Bump/Pool/Fixed** (the `mem.*.new` producer isn't a covered
  call); **`Match.group`** (the `Option<Match>` unwrap chain isn't cleanly reachable).
  Handles whose tables are `Collections::builtin_method_return` (`Stopwatch.elapsed_millis`,
  `Task`/`Channel`/`Sender` methods) carry `recv_type == None` — a Phase-9 `BuiltinMethod`
  gap, NOT this shape (deferred; only Stopwatch is reachable). The `is_intercepted_method_name`
  set stays whole; the handle shape is tried BEFORE the instance-method intercept check,
  gated on a handle `recv_type` — disjoint from user methods (Phase-9 lesson, again).
- **Surfaced + FIXED a latent TIR-path parity bug: a handle binding wasn't forced to
  `let mut`.** `emit_let` (Statement.rs) forces `let mut` for a FileReader/FileWriter/
  TcpStream/HttpRouter/Arena/Bump/Pool/Fixed binding EVEN when bound immutably (their
  methods take `&mut self`), and applies a `mut_fn` `as <fn-trait>` init coercion +
  `rust_fn_trait` annotation for an escaping-FnMut lambda binding. The TIR `Let` had
  carried only `mutable: b.mutable` and rendered the annotation via `cx.rust_type`,
  missing both — a latent bug invisible until Phase 13 made a handle binding+method
  routable (a handle bound but never method-called could not previously reach the TIR,
  because the method excluded the fn). Fixed by resolving `kw` + `ty_clause` (+ the
  `mut_fn` coercion) at LOWERING into total `TStmt::Let` fields, reproducing `emit_let`
  exactly. Caught by a TcpStream probe (`stream @= listener.accept() ?? return 1` →
  `let mut` vs `let`); the full-suite diff stayed 0 (no example binds a handle without
  a method, so it was unreachable before this phase).
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR`
  bypass on `emit_func`/`emit_method`/`emit_trait_method`) across the **entire example
  suite** — 141 files (118 emittable), 0 differ — plus crafted probes exercising:
  fn-typed param + Box-coercion + named-fn value + lambda arg + `(f)(x)` CallValue
  (`24_callbacks.jet` + crafted), `scope.guard`/`tasks.spawn` closure-core-calls, and
  FileWriter (`write_line`/`flush`) + TcpListener/TcpStream (`accept`/`local_addr`/
  `read`/`write`/`peer_addr`/`close`) handle methods (incl. the forced-`let mut` binding).
  A routing sentinel confirmed the covered fns take the TIR path (`apply_twice`,
  `scope_guard` `with_guards`/`question_path`/`main`, `serve`, `write_one` route).

## Refinements learned in Phase 14 (cross-module calls + FFI extern)

- **The module-call surface is FIVE forms across `emit_call` + `emit_method_call`,
  separated purely by which `cx` table holds the name — resolve the whole emitted path
  at lowering into a total `TModuleCallForm`, emit does NO table lookup (I3).** The
  forms: (1) qualified `mod.fn()` via `import_mods` → `{root}{user_mod}::user_{fn}`; (2)
  `pub use` re-export via `reexport_calls` → `{root}{real_mod}::user_{real_fn}`; (3)
  inline code module `alias.method()` via `code_modules` → `{root}user_{alias}__{method}`;
  (4) unqualified inline import via `unqualified_inline` → `{root}user_{alias__method}`;
  (5) unqualified file import via `unqualified_file` → `{root}{user_mod}::user_{fn}`.
  (1)/(2)/(5) collapse to `TModuleCallForm::Qualified{rust_mod, rust_fn}`; (3)/(4) to
  `InlineMangled{mangled}`. The ONLY program-level value emit reads is `cx.root_prefix`
  (prepended exactly where the AST path prepends it — the recurring Phase-9/10 lesson:
  `root_prefix`/`file`/`ffi_crate` stay emit-time program reads, never per-node decisions).
- **The dispatch ORDER inside `emit_call`/`emit_method_call` is load-bearing and the
  TIR must reproduce it.** `emit_call`: local-call → `extern_funcs` → `unqualified_inline`
  → `unqualified_file` → plain mangle. `emit_method_call` (Ident-receiver alias arms):
  `core_imports` (Phase 10) → `reexport_calls` → `import_mods` → `code_modules`. Both the
  gate and the lowering check in that exact order; a name in two tables (none overlap in
  practice, but the order is reproduced regardless) routes to the first.
- **Module-call args reuse `lower_one_call_arg` against the callee's IMPORT signature,
  not `cx.sigs`.** The conventions come from `cx.import_sigs[(alias, fn)]` (file/reexport
  forms) or `cx.sigs[mangled_key]` (inline forms) or `cx.import_sigs[(name, fn)]`
  (unqualified-file — the AST keys it on `(call.name, fn_name)`, which is often empty →
  `None` sig → plain args; reproduced verbatim). A non-scalar `Read` arg becomes `&(…)`
  exactly as `emit_call_args` does (verified: a String module-call arg emits `&("x".to_string())`).
- **FFI extern args are a DISTINCT form (`emit_extern_call_args`) — a non-scalar `Read`
  arg is `(…).clone()`, NOT `&(…)`.** `emit_call`'s `extern_funcs` arm uses
  `emit_extern_call_args`, which wraps a value in `(…).clone()` when `implicit_clone`
  OR (a non-scalar param AND not already implicit-cloned) — a by-value clone, never a
  borrow. Carried as a total `TExternArg{value, clone: bool}` resolved at lowering; the
  Arc (`shared_auto_clone`) form is excluded. The emitted call is `{ffi_crate}::{wrapper}(args)`
  with `cx.ffi_crate` read at emit (like Phase-10's regex form, falling back to `"jet_ffi"`
  exactly as the AST does). **I1 holds:** an extern call introduces no Rust `unsafe` by
  itself — the AST path emits none, and the TIR reproduces it byte-for-byte; the `@unsafe`
  gate surface (where `unsafe` *can* appear) is the separate, deferred Phase-17 work.
- **A cross-module call's result type needed a NEW total table: `cx.import_rets`.** The
  module-call RETURN type wasn't in `cx` (only the param conventions, in `import_sigs`).
  Added `import_ret_map` (mirroring `import_sig_map`: file-module pub funcs, C-boundary
  funcs, `pub use` re-exports) and wired it at every per-module + entry cx build. Inline
  code-module funcs already carry their type in `cx.fn_types` (under the mangled key). The
  call's `ty` is rarely load-bearing (a binding reads `b.ty`), but the design principle
  is to carry a real type when one exists, never `unit_type()`-guess — so the table feeds
  totality and lets a fallible module call compose with Phase-8 `?`/`??` if reached.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on `emit_func`/`emit_method`/`emit_trait_method`) across the **entire example suite** —
  120 emittable (incl. 2 crafted module probes), 0 differ — exercising every module form:
  inline qualified (`42`/`46`), file-module qualified (`43`/`44`), unqualified inline
  (`45`/`46`), unqualified file (`48`), `pub use` re-export (`47`), and FFI extern (`22`,
  incl. a multi-call String-arg probe — `(…).clone()` extern args). A routing sentinel
  confirmed the covered fns take the TIR path: all 8 module/FFI example `main`s route,
  plus the imported submodule fns (`47` `wrap`/`decorate`, the `42`/`45`/`46` module
  bodies). `22_ffi` runs end-to-end (`aGk=`). tests/tir.rs adds 5 cases (inline qualified,
  unqualified inline, file qualified + String arg, unqualified file, `pub use` re-export).

## Refinements learned in Phase 15 (remaining control flow + delegation)

- **The `??` panic locals dump needs a SEPARATE leak-faithful env replica — the
  AST codegen `env` LEAKS in a way `LowerEnv.locals` (which clones at every branch) does
  not.** `safe_locals_expr` (Statement.rs) dumps the FULL codegen `env`
  (`HashMap<String, Slot>`), filtered to scalar Int/Float/Bool, sorted by name, at the
  `??` panic site. The AST codegen `env` is a single `&mut` shared through branch bodies:
  a `let` inside a plain block / loop / mixed-or-range switch arm / comptime-if branch /
  enum-match `else` STAYS in the env after the block (sema scopes the *name* so it is
  never read for resolution, but `safe_locals_expr` dumps the raw env regardless). Only
  the two `emit_pattern_match_switch` arm bodies and a lambda body clone the env (no leak).
  `LowerEnv.locals` clones at every branch (correct for resolution), so it CANNOT feed the
  dump. The fix is a parallel `panic_locals: Rc<RefCell<HashMap>>` mirroring the env via a
  `bind` helper, SHARED across leaky branches (`clone_env` clones the Rc) and DEEP-COPIED
  at the two non-leaky boundaries (`fork_panic`), with the range-loop var save/restored
  exactly as `Statement.rs` does. Because branch-locals are scoped-DEAD (never read for
  resolution), this replica affects ONLY the panic dump — it can't regress any other
  decision. Verified byte-exact on a probe with `let`s leaking from a plain-if and a loop
  live at the `??` (`format!("a = {}, b = {}, c = {}, x = {}", …)`, b/c leaked — identical
  to the AST). **The `??` panic exclusion is LIFTED.** The Phase-8/11 deferral note (the
  reason the panic form was held back) is now resolved.
- **The panic *statement string* is rendered whole at lowering, like the `?` trace frame
  (Phase 8).** `render_panic_stop` reproduces `emit_panic_stop` byte-for-byte: the message
  (lowered from the message expr via the TIR — a `Str` emits its interpolation directly,
  any other expr is `(…).jet_show()`, exactly `emit_panic_message`), the source-line
  text/line/column/caret from `cx.src` at the `panic` span, the escaped file +
  `env.fn_name`, and the locals dump. Emit just splices the pre-rendered string into the
  `match … { Err(_)/None => <here> }` arm — no `cx.src`/`cx.current_fn` read (I3).
- **comptime-if emits ONLY the selected branch, INLINE, with no `if` — and its `let`s
  leak.** `emit_stmts` reads `Stmt::ComptimeIf.selected_then` (a total sema fact) and
  emits the chosen branch's statements on the SAME `&mut env` at the SAME indent (the
  unselected branch is name-resolution-only, D-WHEN2, and never reaches codegen). The TIR
  lowers the selected branch on the SAME `env` (so bindings leak, matching the shared AST
  env) into a flat `TStmt::Inline(Vec<TStmt>)` that emits its children at the parent
  indent with no wrapper. The gate classifies the SELECTED branch only (the dropped branch
  is irrelevant — it is never emitted); before sema resolves `selected_then` (a
  `build_cx`-only gate test) it defaults to the `then` branch.
- **The mixed comparison/Bool switch (shape D) is the LAST `emit_mixed_switch` slice —
  arm heads resolve to plain `emit_expr(cond)` strings.** `emit_switch_arm_cond` routes a
  variant/Eq-to-variant head through `emit_pattern_matches` and a range head through the
  range guard; everything else is `emit_expr(cond)`. Shape D covers only the latter
  (`arm_is_plain_cond` excludes any pattern-test / variant / range arm), so a switch with
  even ONE pattern arm stays on the AST path (conservative — a mixed variant+comparison
  switch is not covered). The `_jet_switch_subject = &(subject)` borrow binding is emitted
  for parity even when unused, and the arm conditions + bodies + `else` reproduce
  `emit_mixed_switch`'s exact `if/else if … else` rendering. Arm bodies use the SHARED env
  (leaky), like the AST.
- **A delegation method (`using field`) is purely structural — no body to lower.**
  `emit_delegation_method` (Items.rs) forwards `(self).<field>.<method>(args)` with the
  BARE trait method name (the trait owns it in Rust) and a signature rendered by the SAME
  `rust_param_type`/`rust_return_type` the AST uses (incl. a quirky two-space `  {` before
  the brace). A new `TFuncKind::Delegation { sig, fwd, has_return }` carries the
  fully-rendered signature line + forward call, resolved at lowering; `emit_tir_delegation`
  just splices them. The gate (`tir_covers_delegation_method`) returns `true`
  unconditionally — the forward is deterministic, nothing it produces can diverge.
- **Surfaced + FIXED a latent TIR-path divergence: a CORE-struct field read was mangled.**
  Lifting the `??` panic exclusion let `result @= process.run(…) ?? panic(…); result.code`
  route through the TIR — and the TIR `Field` emit was `mangle(member)` unconditionally,
  emitting `user_code` where the AST emits the PLAIN `code` (core structs declare
  unprefixed fields — B2, `core_struct_field_rust_name`). The hole pre-dated Phase 15 (a
  field read on a core/foreign struct) but was MASKED — those functions all carried a
  `??`/`?` that excluded them. Fixed by reproducing `core_struct_field_rust_name` in the
  `Field` lowering, keyed on the resolved `recv.ty` (ProcessResult/JSONError/UTF8Error/
  HttpRequest/HttpResponse). Caught by `tests/ice_regressions.rs::b2_…` the moment the
  panic form routed; the full-suite diff then stayed 0. (HttpRequest/HttpResponse *method*
  accessors remain excluded — those are MethodCalls, a separate deferred entry.)
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the
  **entire example suite** — 118 emittable, 0 differ — plus crafted probes exercising: a
  `?? panic(…)` with several live locals of mixed deref/type at the panic site, a
  `?? panic` AFTER a plain-if `let` and a loop-body `let` (the leak case — `b`/`c` leaked
  into the dump, byte-identical), a comptime-if (selected-`else`, inline, no `if`), a
  comptime else-if chain, a mixed comparison switch, and a delegation forward
  (`47_library` `App: Logger using logger`). All ran correctly end-to-end. tests/tir.rs
  adds 4 cases (comptime-if, mixed switch, delegation, `?? panic`); the TIR unit tests
  add `covers_comptime_if`/`covers_mixed_bool_switch`/`covers_or_fallback_panic_form`.

## Refinements learned in Phase 16 (payload-carrying & recursive types)

- **`emit_boxed_enum_arg` is the whole payload-clone/box decision, and it is now a
  TOTAL fact per arg.** Each enum-literal payload arg carries `TEnumArg { value, clone,
  boxed }` resolved at lowering, reproducing `emit_boxed_enum_arg` byte-for-byte: a
  non-scalar *single*-payload type (`enum_variant_payload_type` → `Single(t)` /
  single-field `Named`, NOT a multi-field named variant — those resolve to `None`,
  matching the AST) whose arg is a borrowed-in-env ident → `(…).clone()`; a recursive
  (`boxed_edge`) edge → `Box::new(…)`, applied in that order. For a scalar payload from
  a non-borrowed value both flags are false (the Phase-4 no-op), so emit is byte-
  identical. The borrowed test is exactly `expr_borrowed_in_env`: only an `Expr::Ident`
  with a deref'd slot (`LowerEnv::is_borrowed`) — every other arg form is false.
- **A payload/recursive enum is CONSTRUCTED via a `MethodCall`, not `Expr::EnumLit` —
  so the construction needed a NEW gate/lowering shape.** `Expr::EnumLit` is constructed
  by neither the parser nor sema (only matched); `Enum.Variant(args)` parses as a
  `MethodCall` that sema type-checks in place (`check_enum_lit`) without rewriting, and
  the AST `emit_method_call` routes it to `emit_enum_lit` (all-positional args). Phase 16
  adds shape (j) to `method_call_in_subset`/`lower_method_call`: `recv_type == None` +
  a type-name-ident receiver that is a covered enum + a method that names a variant →
  an `EnumLit` TIR node with the per-arg `TEnumArg`. Tried BEFORE the static shape
  (which excludes variants), matching the AST dispatch order. This is THE shape that
  makes string/struct/collection-payload + recursive (boxed) enum *construction* route.
- **Recursive (boxed) enums need NO deref in the function body — `Box` auto-derefs.**
  The only box-related emit decision is the `Box::new(…)` at construction (now total via
  `TEnumArg.boxed`). A bound payload binds the boxed value with `deref: false` (exactly
  the AST `add_pattern_bindings` slot), and Rust's `Box` auto-deref handles every field
  read / method call / arithmetic on it. The gate's enum `seen` set now ADMITS a
  self-reference (it's already under check) instead of excluding it, so a linked-list /
  expr-AST enum is covered; the `seen` set is threaded through nested collection element
  types so a `[Self]` payload terminates instead of looping.
- **Recursive STRUCTS stay EXCLUDED — broken on the AST path (latent I2 hole).**
  `emit_struct_lit` does not wrap a boxed struct-lit field value in `Box::new(…)`, so a
  recursive struct literal is rustc-rejected (E0308). Per the `is_empty` precedent (the
  TIR must not *claim* a function that miscompiles), the struct gate's visited set keeps
  excluding recursion. A borrowed non-Copy struct-lit field value (`Person { name: n }`,
  `n: &String`) is also broken (E0507) on both paths — same miscompile, parity holds, so
  it isn't separately gated, but it's logged in the latent-bug list. Both fixes live in
  `emit_struct_lit` / sema elaboration, independent of c109.
- **Struct/collection field + payload coverage is a pure GATE widening — no emit change.**
  `field_ty_covered` now admits a covered collection field (`[E]`/`[K, V]`); the struct-
  literal emit (`field: vec![…]`) is plain, byte-identical. Likewise an enum payload may
  be a covered struct / covered collection / another covered enum
  (`enum_payload_ty_covered`). The value's own move/clone facts live in its sub-expression
  (the recurring Phase-3/5 lesson), so no new clone/box decision arises at the field site.
- **Named (multi-field) variant payloads are unreachable, so the `Named` arg-clone never
  fires.** `Enum.Variant(label: v, …)` construction is REJECTED by sema (E0303 "requires
  labeled fields") before codegen — the labeled MethodCall form never reaches
  `emit_enum_lit`. The `TEnumPayload::Named` lowering (with its `"Variant.label"` edge,
  which never matches a variant name → `enum_variant_payload_type` returns `None` → no
  clone, exactly as the AST) is retained for the `Expr::EnumLit` path but is effectively
  dead, since that node is never constructed.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on `emit_func`/`emit_method`/`emit_trait_method`/delegation) across the **entire example
  suite** — 118 emittable, 0 differ — plus crafted probes exercising: a String-payload
  enum (match-binding + borrowed-`.clone()` construction `Msg.Text(s)` → `((*s)).clone()`),
  a recursive (boxed) enum (`Tree.Node(inner)` → `Box::new(((*inner)).clone())`, the
  borrowed-clone + boxed edge together; a nested-literal `Expr.Wrap(Expr.Num(1))`), a
  struct-payload enum (`Shape.Dot(p)` + a `p.x` field read from the bound struct), a
  collection-payload enum (`Data.Nums(xs)`), a struct-with-collection field, and a
  String-payload error enum in `T ? Oops` (`err(Oops.Msg("bad"))`) — all byte-for-byte
  identical and rustc-clean, then the instrumentation removed. ~17 more example fns route
  (201 → 218). tests/tir.rs adds 4 build+run cases; the TIR unit tests flip 4 `rejects_*`
  to `covers_*` and add `covers_recursive_enum_construction_with_clone_box` /
  `covers_struct_payload_enum` / `covers_collection_payload_enum`.

## Refinements learned in Phase 17 (generics, foreign types, view methods)

- **`view`-returning functions/methods are a tiny, fully-total slice — the borrow shape is
  a two-case `ViewWrap` resolved at lowering.** `emit_view_return` (Source/Codegen/Statement.rs)
  branches on the *AST node shape*: an `Ident` to a deref'd (by-reference) slot returns the
  BARE borrow `name` (the `(*…)` stripped); an `Ident` to a non-deref slot or a const returns
  `&name`/`&<const>`; a `Field` read returns `&(<place>)`; anything else passes to `emit_expr`
  unwrapped. `lower_view_return` reproduces this into a `TStmt::ViewReturn { value, wrap }`
  where `wrap ∈ {Addr, Bare}` is decided at lowering off the same node shape — emit only
  prefixes `&` (Addr) or emits the value as-is (Bare). The `view_return` flag rides on
  `LowerEnv` (threaded through `clone_env`/`fork_panic` so nested scopes keep it) and on the
  `TFunc.is_view` field (drives `rust_return_type(cx, ret, is_view)` → `&T`). Sema's E2301/E2304
  restrict a view return to an owned `Ident`/`Field` place, so only those shapes reach codegen
  — the gate needs no special view-return body check beyond the existing `stmt_in_subset`. The
  field read in a view return is NOT rewritten to `.clone()` by sema (only owning returns are),
  so the `Expr::Field` survives — exactly what `emit_view_return` wants. Inherent + free `view`
  fns are covered; a `view`-returning **trait** method keeps its gate exclusion (conservative —
  no example needs it, and the trait-method signature is fixed by the trait).
- **Generic FREE functions are a clean gate-widening + a rendered-clause TIR field — no new
  emit logic beyond printing the clause.** The `<T: Clone>` clause renders at lowering via
  `Generics::rust_type_param_list(&f.type_params, rust_extra_clone_bounds)` (every type param
  carries the extra `Clone` bound, EXACTLY `emit_func`), carried as a total `TFunc.generics`
  string emit prints verbatim after the name. A type-var param/return (`Type::Named(T)` where
  `Generics::is_type_var_name` holds — a single uppercase letter) is admitted by
  `is_subset_param_ty` and renders by value via `cx.rust_type`/`rust_param_type` (bare `T`, no
  `&`). The ONE load-bearing parity detail: a param TYPED as a bare type var (`item: T`) is
  forced to the `Move` convention for the slot DEREF (`param_place_generic` — by-value, no
  `(*…)`), mirroring `emit_func`'s `is_type_param` branch; a param typed `Stack<T>` is NOT a
  type-var param and keeps its source convention (`Read` → `&user_Stack<T>`, deref'd place).
  A `[T]` list element is admitted (`collection_elem_covered`), so a generic `[T]`-param/return
  fn routes too. Verified byte-identical: `id`/`pick`/`firstof`/`wrap`.
- **Generic STRUCT types are DEFERRED — the `Type::Apply` value type + turbofish stay on the
  AST path.** A function whose param/return is `Pair<T>` (a `Type::Apply`) exits the gate at
  the param/return-type check (`is_subset_param_ty` admits a bare type var but NOT an `Apply`),
  so `26_generic_types` `make_pair`/`empty_stack`/`push` stay on the AST path — byte-identical.
  Covering them needs the `user_Pair::<T> { … }` turbofish StructLit form, a generic-struct
  field type (`struct_is_covered` excludes type-var fields), and (`push`) a `[T]`-field builtin
  + the generic-value clone — a coherent follow-up slice, deferred to keep the most
  parity-sensitive surface I2-safe.
- **PRELUDE structs (HttpRequest/HttpResponse) construct via a distinct emit branch — a
  total `extra` field carries the injected member.** `emit_struct_lit`'s `is_prelude_struct`
  branch renders a `<root>Jet…` Rust head (via `net_handle_rust_type`) with PLAIN (unmangled)
  field names, and — for HttpRequest — appends an injected `params: BTreeMap::new()` field. The
  TIR `StructLit` node gained an `extra: Option<String>` (the injected line, resolved at
  lowering) and the field names are kept plain for the prelude case. The constructable prelude
  structs + the core/prelude/handle types (`core_rust_type_name`/`file_handle_rust_type`/
  `net_handle_rust_type`/`alloc_handle_rust_type` — ProcessResult/Json/Stopwatch/FileReader/
  TcpStream/Arena/…) are admitted as covered VALUE types (`is_covered_foreign_value_ty`), so a
  function passing/binding/returning one routes. A METHOD on any of them is still out of subset,
  so a function that *calls* one is excluded by that call — covering the value type never reaches
  an uncovered handle/accessor form (the recurring "cover the node you can make total, let the
  next uncovered node exclude its fn" seam). FOREIGN (imported user) structs/enums stay excluded
  (their construction needs the cross-module `import_ns` path — Phase 14 territory). Verified
  byte-identical: `build_resp` (HttpResponse) + `build_req` (HttpRequest, with the injected
  `params`).
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass on
  `emit_func`/`emit_method`/`emit_trait_method`/delegation) across the **entire example suite** —
  118 emittable, 0 differ — plus crafted probes exercising: a `view`-returning field accessor
  (`name_of` → `&((*user_field)).user_name`); generic free functions (`id`/`pick` type-var
  by-value, `firstof`/`wrap` over `[T]`); and prelude struct construction (`HttpResponse {…}` →
  `JetHttpResponse { …, headers: …::BTreeMap::new() }`, `HttpRequest {…}` with the injected
  `params`). A routing sentinel confirmed the covered fns take the TIR path (zerocopy `name_of`
  + `name_copy`; the crafted generic + prelude fns). 206 free fns route through `emit_func` now.
  tests/tir.rs adds `generic_free_fns`/`view_return_fn`/`prelude_struct_construction`; the TIR
  unit tests flip `rejects_generic_fn` → `covers_generic_fn` and add `rejects_generic_struct_fn`.

## Refinements learned in Phase 18 (@unsafe / #Unsafe / core.mem)

- **The `#Unsafe fn` keyword is the ONLY signature change — a function-level `unsafe`,
  NOT a body wrap.** `emit_func`/`emit_method`/`emit_trait_method` prepend `unsafe ` to
  the signature (`{vis}{unsafe_kw}fn …` / `pub {unsafe_kw}fn …` / `{pad}{unsafe_kw}fn …`)
  when `f.is_unsafe`; the body is emitted unchanged (the whole fn being `unsafe` is what
  lets its gated ops compile). So covering it was a `TFunc.is_unsafe` field (top-level +
  inherent method) — `TFuncKind::TraitMethod` already carried its own `is_unsafe` since
  Phase 12 — plus LIFTING the three gates' `f.is_unsafe` exclusion. `#Pure` stays excluded
  (no TIR representation). I1: the `unsafe ` prefix is emitted iff the source was
  `#Unsafe fn` — 1:1 with the source gate.
- **The `#Unsafe { … }` block is the ONLY producer of a Rust `unsafe { … }`, and its
  `#Audit("…")` annotation emits NOTHING.** `emit_stmts`'s `Stmt::Unsafe` arm is a dumb
  `unsafe { <body> }` — the `audit` field (the `#Audit("…")` reason) is dropped by codegen
  (sema validated it). Reproduced as `TStmt::Unsafe(Vec<TStmt>)`; emit is byte-for-byte
  `unsafe {\n … \n}`. The body's `let`s LEAK into the outer scope (the AST shares
  `&mut env`), so the gate checks + the lowering walk the body on the SAME `locals`/`env`
  (NOT a cloned scope) — like comptime-if/region (Phase 15). I1: this TIR node exists ONLY
  for a source `#Unsafe` region, so an `unsafe` block is never produced without its gate.
- **The `core.mem` POINTER ops are total despite NOT being in `core_fixed_sig`.**
  `address_of`/`volatile_read` route through the Phase-10 core-call shape (a `MethodCall`,
  `recv_type == None`, receiver `mem ∈ core_imports`), but Phase 10 gated on
  `core_fixed_sig(...).is_some()` — which excludes them (their types come from bespoke
  `infer_core_call` logic). Both ARE deterministic and resolved at lowering: `address_of`
  → `Int` (an inert `(&(x) as *const _ as usize as i64)` cast — no `unsafe`);
  `volatile_read(p)` → `ptr_elem(p.ty)` (the `T` of the `Ptr<T>` arg, recovered from the
  LOWERED arg's total `ty` — never `expr_jet_ty` in emit, I3) → `std::ptr::read_volatile(p)`
  (valid because the call only reaches codegen inside an `#Unsafe` region/fn, sema E3101 →
  already a Rust `unsafe` context). `core_call_covered` now admits the two `core.mem` ops
  explicitly, with the return type special-cased in `lower_method_call` from the lowered
  args. The emit arms reproduce `emit_core_call` byte-for-byte.
- **`mem.Ptr<T>.from_addr(addr)` is a DISTINCT AST node (`Expr::PtrFromAddr`), not a
  MethodCall.** It carries `elem: Type` on the node (total — the `<T>`), so the result
  type is `Ptr<elem>` and the Rust element type is `cx.rust_type(elem)`, both resolved at
  lowering (`TExprKind::PtrFromAddr { elem_rust, addr }`). Emit is byte-for-byte
  `emit_expr`'s arm: `(({addr}) as usize as *mut {elem})`. The cast itself is SAFE Rust
  (no `unsafe`) — only *using* the pointer (`volatile_read`) needs the surrounding gate —
  so a function with only `from_addr`/`address_of` (no `#Unsafe`) emits zero `unsafe`. The
  gate needed `expr_in_subset` + `lower_expr` arms (the gate/emit alone weren't enough — a
  missing `lower_expr` arm hit the `unreachable!` for a gate-admitted node, the recurring
  "wire all three: gate, lower, emit" lesson).
- **An inferred `p @= mem.Ptr<Int>.from_addr(addr)` still emits a type annotation
  (`let user_p: *mut i64 = …`)** — sema writes the resolved `Ptr<Int>` onto the binding's
  `b.ty` even for `@=`, so `emit_let`/the TIR `Let` render `: *mut i64`. Reproduced
  identically (the `ty_clause` reads `b.ty`), so parity holds.
- **The ARENA allocators stay deferred (not the pointer tier).** `mem.Arena.new()` /
  `.alloc` / `.reset` / `.free` and `#Context(allocator:)` are a separate, handle-shaped
  surface (a producer not in `core_fixed_sig` + `recv_type == Some(<allocator>)` methods +
  an `arena_view` binding the gate already excludes) — covering them is a clean follow-up,
  NOT part of the `#Unsafe`/pointer slice. Their only `unsafe` is the vetted `jet_mem`
  prelude (excluded from the I1 scan).
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the
  **entire example suite** — 118 emittable, 0 differ — incl. both `#Unsafe`-tier examples
  (`examples/features/48_lowlevel.jet` + `examples/showcase/lowlevel.jet`: `#Unsafe fn`,
  `#Unsafe { }`, `Ptr<Int>.from_addr`, `address_of`, `volatile_read`, and a call to an
  `#Unsafe fn` from inside the block). The I1 self-check (grep the TIR-path generated Rust,
  drop the `jet_mem` prelude, assert every `unsafe` is `unsafe {` or `unsafe fn`) passes:
  the only `unsafe` forms are `pub unsafe fn user_read_reg/read_int` (← `#Unsafe fn`) and
  `unsafe {` (← `#Unsafe { }`) — both trace 1:1 to a source gate; zero ungated `unsafe`.
  Both examples run end-to-end (`1337`/`1337`; `low-level read: 42`). +4 example fns route
  (`read_reg`/`main` in `48_lowlevel`, `read_int`/`main` in `showcase/lowlevel`). The
  existing `tests/golden.rs` I1 guard (every example's user code is `unsafe`-free except
  `48_lowlevel`, whose every `unsafe` must be gated) stays green on the TIR path.
  tests/tir.rs adds `unsafe_fn_block_and_ptr_ops` (build+run) + `unsafe_tier_emit_is_byte_exact`
  (byte-exact emit + I1 self-check); the TIR unit tests add `covers_unsafe_fn_with_ptr_ops`
  / `covers_unsafe_block_and_address_of`.

## Refinements learned in Phase 19 (generic structs, foreign types, arena/region)

- **Generic STRUCTS are a pure GATE widening + a turbofish head resolved at lowering — no
  new emit logic.** A generic struct's type-var FIELD (`first: T` in `Pair<T>`) is now
  admitted by `field_ty_covered` (it renders to the bare `T` via `cx.rust_type`, and a
  struct-lit field value is the type-var value itself — no clone/deref decision), so
  `struct_is_covered("Pair")` returns true. A `Type::Apply` (`Pair<T>`/`Stack<Int>`)
  param/return/local is admitted by a new `is_covered_generic_struct_ty` (base a covered
  struct, every arg covered-or-type-var). The turbofish StructLit (`user_Pair::<T> { … }`)
  reproduces `user_type_apply_rust` at lowering: `format!("user_{}::<{}>", name, args…)` —
  carried as the total `StructLit.rust_type` string, so emit decides nothing. The `[T]`-field
  builtin (`copy.items.push(item)`) routes via the Phase-9 builtin shape unchanged (the Field
  receiver `copy.items` is `tir_recv_jet_ty == None` → list/default branch, exactly the AST).
  The `copy := s` generic-struct clone is the Phase-6 sema-inserted `.clone()`. `26_generic_types`
  `make_pair`/`empty_stack`/`push` route, byte-identical, and run (`1`/`42`).
- **Generic-struct METHODS stay EXCLUDED — a separate deferred surface.** Admitting a
  generic struct as a covered VALUE type makes `is_covered_struct_ty("Box")` true, which would
  let `tir_covers_method`/`tir_covers_trait_method` claim a method on a generic struct
  (`impl<T> user_<T>`). A probe showed such a method emits byte-identically, but the `impl<T>`
  clause + turbofish receiver aren't validated across every method shape (view returns,
  generic-struct method args, etc.), so a new `struct_is_generic` (a struct with a type-var
  field) excludes it from both method gates — conservative (exclude on any doubt). Generic
  *methods* are the remaining generic residue.
- **FOREIGN (imported user) structs are a value-type widening + the `import_ns` head resolved
  at lowering.** A foreign struct/enum (in `cx.foreign_types`) is now a covered value type
  (`is_covered_foreign_value_ty` — it renders via `cx.rust_type` to `{root}{mod}::user_<Name>`,
  and a field read on it mangles `(n).user_title` exactly as `mangle` produces). Construction
  `alias.Note { … }` (`import_ns`) reproduces `emit_struct_lit`'s `import_ns` branch:
  `{root}{import_mods[alias]}::{mangle(Note)}[::<args>]` with MANGLED fields — resolved at
  lowering into the total `StructLit.rust_type`. No example exercises this (verified via a
  crafted 2-file module: `make` + `main` route, byte-identical, runs `hello`). FOREIGN ENUM
  *construction* has no reachable cross-module literal syntax (`note.Color.Red` is E0107), so
  foreign enums are covered as value types only. A borrowed-String foreign struct-lit field
  hits the SAME pre-existing E0507 bug as a local struct (latent bug #7) — parity holds, the
  literal-field case works end-to-end.
- **`Stopwatch.elapsed_millis()` is a Phase-9-style `recv_type == None` builtin gap.** Sema
  types it via `Collections::stopwatch_method_return` (leaving `recv_type == None`, NOT the
  `Some(<handle>)` of the Phase-13 handle shape), and the AST `emit_builtin_method` dispatches
  it on the method NAME alone. A new gate shape (d2) — `recv_type == None` + the
  `elapsed_millis` name + an in-subset `Stopwatch` value receiver (from the covered
  `time.start` producer) — lowers to the existing `THandleOp::StopwatchElapsedMillis`
  (`{root}jet_stopwatch_elapsed_millis(&(recv))`), byte-for-byte. Verified byte-identical +
  runs.
- **The arena/region surface is six coupled pieces, all byte-exact and total.** (1) The
  PRODUCER `mem.Arena.new(…)` — a MethodCall with `recv_type == Some(<Alloc>)` (the receiver
  `mem.Arena` is typed `Named(Arena)` via `infer_core_field`, then `.new()` dispatches through
  `alloc_method_return`), claimed by a new shape (k) tried FIRST (before the handle shape,
  mirroring `emit_method_call`'s constructor-first dispatch); the whole ctor tail
  (`jet_mem::Jet<Alloc>::new()` / `::with_capacity|with_slots|with_size((arg) as usize)`) is
  rendered at lowering into `TExprKind::AllocNew { ctor }`. (2) `alloc`/`reset`/`free` handle
  methods (`recv_type == Some(<Alloc>)`) — new `THandleOp::{AllocAlloc,AllocReset,AllocFree}`
  (`(recv).alloc(a0)` / `(recv).reset()` / `drop(recv)`); `alloc`'s result type is the arg's
  total `ty` (sema's `__alloc_infer__` sentinel resolved from the lowered arg, never
  re-inferred — I3). (3) The ARENA BINDING already forced `let mut` via the TIR `Let`'s
  `is_file_handle` set (Phase 13). (4) The `arena_view` BINDING (`x @= arena.alloc(v)`) —
  `emit_let`'s `arena_view` branch reproduced: `let <x> = <init>;` (NO type, NEVER `let mut`)
  with a DEREF'd slot place `(*<x>)`, so view reads emit `(*user_x)`. (5) `region r { … }` →
  `TStmt::Region` (a plain block, body leaks into the outer scope like comptime-if/unsafe).
  (6) `#Context(allocator: …) { … }` → `TStmt::ContextBlock` (an `_ctx_guard_<i> =
  jet_mem::jet_ctx_push_alloc(&v)` per `allocator` field / `_ctx_logger_<i> = v` per other
  field, then the leaky body). **I1 holds:** the arena `unsafe` lives ENTIRELY in the vetted
  `jet_mem` prelude (untouched, excluded from the I1 scan) — the TIR-path `main` of all three
  examples emits ZERO `unsafe`. `70_arena`/`75_arena_regions`/`77_smart_context` all route,
  byte-identical, and run end-to-end.
- **The bare `?? return` inventory entry was stale — it already routes.** `x ?? return` (no
  value) was claimed (in the AST-path inventory) not to route; in fact the gate's
  `orfallback_rhs_in_subset → Return(None) => true` (an earlier phase) admits it, and it emits
  byte-identically (`match … { None => return }`). The construct is unusable for an unrelated
  sema/codegen reason (E0405 demands a return type, rustc E0069 then rejects `return;` in a
  non-unit fn) — logged in the latent-bug list, NOT a TIR gap.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass on
  `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the **entire
  example suite** — 118 emittable, 0 differ — incl. the four Phase-19 example surfaces
  (`26_generic_types` generic structs; `70_arena`/`75_arena_regions`/`77_smart_context`
  arena/region/context) — plus crafted probes (a 2-file foreign-struct module, a Stopwatch
  program, a generic-struct method that emits byte-identically but stays excluded by
  `struct_is_generic`). A routing sentinel confirmed the covered fns take the TIR path
  (`make_pair`/`empty_stack`/`push`/`main` in `26`; the three arena example `main`s; the
  foreign `make`/`main`; the Stopwatch `main`). ~240 fn-routes across the suite now (was
  ~218). tests/tir.rs adds `generic_struct_fns`/`foreign_struct_construction`/
  `stopwatch_elapsed_millis`/`arena_alloc_reset_free`/`arena_region_block`/`smart_context_block`;
  the TIR unit tests flip `rejects_generic_struct_fn`/`rejects_generic_struct_literal` →
  `covers_*` and update `rejects_generic_method` (now excluded by `struct_is_generic`, not the
  bare-name check) + `handle_method_op_table` (arena ops now covered).

## Refinements learned in Phase 20 (polymorphic core specials, http accessors)

- **The polymorphic core specials' return type IS now a total fact — sema writes it
  onto a NEW `Expr::MethodCall.resolved_ret` field.** Phases 10/13 deferred
  `math.abs/min/max/clamp`, `random.pick/shuffle`, `io.eprint` because their return
  type is arg-type dependent (resolved by `infer_core_call`'s bespoke `check_core_call`
  logic, NOT `core_fixed_sig`) and was thrown away — the node carried only `recv_type`.
  The c109-spirit fix is to WRITE THE FACT BACK: a new `resolved_ret: Option<Type>` on
  `MethodCall`, populated in `infer_method_call` right after `infer_core_call` returns,
  gated on `is_polymorphic_core_special(module, method)` (a new `CheckerCoreLib` helper).
  Lowering reads it totally (I3) into the node's `ty`; emit makes no type decision. The
  EMITTED form is a fixed per-`(module, method)` string (`(x).abs()`, `(a).min(b)`,
  `jet_std_random_pick(&(xs))`, `eprintln!("{}", (x).jet_show())`), added to
  `emit_tir_core_call` byte-for-byte from `emit_core_call`, args emitted plainly. The
  specials join `core_call_covered` + the Phase-10 `CoreCall` shape (no new node kind).
  Adding the field touched only 5 construct sites (the rest match with `..`); existing
  behavior is unchanged (only the new path reads it). `random.pick` → `Int?` proves the
  writeback (the binding emits `let user_p: Option<i64>` from `resolved_ret`).
- **HttpRequest/HttpResponse accessors were unblocked by ONE table addition, not a sema
  change.** Phase 13 excluded them claiming the `http.serve` lambda param is unresolved
  (`p.ty == None`). That turned out to be a RED HERRING: `http.serve` REQUIRES an
  annotated handler param (the serve arm sets no expected type, so an unannotated
  `(req) =>` lambda is E0801 — a separate sema gap, logged). So the accessors reach
  codegen only on a TYPED param (a free fn `handle(req: HttpRequest)` or annotated
  lambda), where `recv_type == Some(HttpRequest|HttpResponse)` AND the slot type is
  already total — the AST `rty`-keyed `emit_builtin_method` arm fires identically. The
  fix is purely adding the eight accessor ops to `handle_method_op` (`method`/`path`/
  `body`/`header`/`param` on HttpRequest; `status`/`body`/`header` on HttpResponse) +
  the `THandleOp::Http{Req,Resp}{Field,Header,Param}` emit arms, byte-for-byte the AST
  arms (`(recv).<field>.clone()`, `(recv).headers.get(&a0).cloned()`,
  `jet_http_request_param(&(recv), &(a0))`). The handle-shape gate already keyed on
  `recv_type == Some(handle)` → `handle_method_op`, so the gate widened for free. The
  return types come from `net_method_return` via `handle_method_return_ty` (total).
  `57_http_server` `handle` now routes byte-identically.
- **A speculative lambda-param-type WRITEBACK was tried and REVERTED — it changes emit
  for trait-object closures.** Writing the inferred param type back onto `LambdaParam.ty`
  (the "make the slot total" idea from the phase brief) is UNSAFE: a `[Shape]`-list
  `.each((s) => …)` lambda whose param `s` was untyped would gain `s: Box<dyn Shape>`,
  and `emit_lambda` then declares `move |user_s: Box<dyn user_Shape>|` → a closure-arg
  type mismatch with `jet_list_each_ref`'s `&T` (rustc E0631). Caught by
  `tests/tir.rs::trait_impl_method_bodies`. The writeback isn't needed anyway (the http
  accessors work on typed params), so it was dropped — the http coverage stands on the
  `handle_method_op` addition alone. Lesson: a "more total" sema fact written onto a node
  that the AST emit path ALSO reads can diverge — only safe when the field is read by the
  TIR path exclusively (as `resolved_ret` is). The `resolved_ret` writeback is safe
  precisely because no existing emit/diagnostic reads it.
- **Generic-type METHODS are UNREACHABLE — a sema gap, not a TIR gap (DEFERRED).** A
  struct-body method on a generic struct (`struct Stack<T> { … fn size(self) … }`) is
  NOT bound to the type (E0311 at the call site) and a `T`-referencing body hits E0119
  ("no type called `T`"). So NO valid Jet program has a generic-struct method; the
  construct can't route on either path. Phase 19's "byte-identical probe" was a
  `build_cx`-only unit-test AST, bypassing sema's method binding — it does not reflect a
  real program. The `struct_is_generic` exclusion STAYS (correctly conservative); the
  fix is in sema's method registration/scope (logged in the latent-bug list), not c109.
- **Task/Channel/Sender methods stay DEFERRED — the producers aren't coverable as a
  small slice.** `Task.join/detach`, `Channel.receive/sender`, `Sender.send` carry
  `recv_type == None` (a Phase-9 builtin gap). They are reachable ONLY if their handle
  binds in a covered fn — which requires covering `Channel<T>`/`Task<T>`/`Sender<T>` as
  covered `Type::Apply` VALUE types AND the `Closed` error type (for `receive`'s Result)
  AND a return-type writeback for the `tasks.channel` producer (not in `core_fixed_sig`).
  Covering only the producer leaves the methods unreachable; covering partially risks
  parity divergence on the channel examples. This is a coherent coupled follow-up — LANDED
  WHOLE in Phase 21 (see "Refinements learned in Phase 21").
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the
  **entire example suite** — 118 emittable, 0 differ — plus crafted probes exercising:
  the polymorphic specials (`math.abs/min/max/clamp` + `random.pick/shuffle` + `io.eprint`,
  with `random.pick` → `Int?` proving the `resolved_ret` writeback) and HttpRequest/
  HttpResponse accessors (`method`/`path`/`header`/`param`/`status`/`body` on a typed
  param via `http.parse`). `57_http_server` `handle` (`req.method()`/`req.path()`) routes
  byte-identically. Instrumentation + temp files removed before commit. tests/tir.rs adds
  `polymorphic_core_specials` + `http_request_response_accessors`; the TIR unit tests add
  `polymorphic_core_specials_covered` and flip `handle_method_op_table` (HttpRequest/
  HttpResponse accessors now covered).

## Refinements learned in Phase 21 (Task/Channel/Sender concurrency)

- **The whole concurrency surface is the coupled slice Phase 20 deferred, landed
  byte-exact.** The five `recv_type == None` methods (`Task.join`/`Task.detach`,
  `Channel.receive`/`Channel.sender`, `Sender.send`) are a Phase-9 builtin gap — sema
  types them via `Collections::builtin_method_return`'s `Type::Apply` arms
  (`task_method_return`/`channel_method_return`/`sender_method_return`, Source/Collections.rs)
  and leaves `recv_type` unset, exactly like Stopwatch (shape d2). They became reachable
  by covering, *together*: (1) `Task<T>`/`Channel<T>`/`Sender<T>` as covered VALUE types
  (`is_covered_concurrency_ty` — a `Type::Apply` over a covered elem, rendered by
  `cx.rust_type` to `{root}jet_std::Jet{Task,Channel,Sender}<…>`); (2) the `Closed` err
  type as a covered fallible payload (`fallible_payload_covered`, for `receive`'s
  `Result<T, Closed>`, rendered via `core_rust_type_name` → `{root}jet_std::Closed`); (3)
  the `tasks.channel()` PRODUCER (a fixed-string `CoreCall`, added to `core_call_covered`
  — NOT in `core_fixed_sig` since its `Channel<T>` return type rides on the binding
  annotation, not the args). `tasks.spawn`'s `Task<T>` result was already total (Phase-13
  `CoreClosureCall` → `core_closure_call_return_ty`). Covering the producer/value types
  alone never *forces* a method, so an uncovered method still excludes its fn — the
  recurring "cover the value type, let the next uncovered node exclude its fn" seam — which
  is why the slice is safe to land whole.
- **The new gate shape (d3) keys on method NAME + ARITY, disjoint from every other
  shape.** `join`(0)/`detach`(0)/`receive`(0)/`sender`(0)/`send`(1)
  (`is_concurrency_method_name`) — the arity disambiguates `Task.join()` (0 args) from the
  list `join(sep)` (1 arg, claimed by the collection-builtin shape d, which runs first);
  `Sender.send(v)` is the 1-arg form. No other builtin uses these names, so name+arity is
  the total routing signal (the Stopwatch `elapsed_millis` precedent). `is_intercepted_method_name`
  already listed all five (kept whole), so the user-method shapes still correctly exclude them.
- **The method result type comes from the LOWERED receiver's `.ty`, not re-inference (I3).**
  `lower_method_call` reads the element `T` off the receiver's already-resolved
  `Task<T>`/`Channel<T>`/`Sender<T>` (the binding's annotated/inferred slot type, total):
  `join` → `T`; `detach`/`send` → Unit; `receive` → `Result<T, Closed>`; `sender` →
  `Sender<T>`. So a `sender()` result binds a typed `Sender<T>` local (chaining `s @=
  ch.sender()` then `s.send(…)`), and `receive()` composes with Phase-8 `?? panic(…)` (the
  `Result<T, Closed>` Ok payload is read off this `ty`). The five emit arms reproduce
  `emit_builtin_method`'s `Type::Apply`-receiver arms byte-for-byte (`(recv).join()`,
  `{ let _detach = (recv); }`, `(recv).receive()`, `(recv).sender()`, `(recv).send(a0)`) —
  the handle's prelude methods take `&self`, so the receiver/binding need no `let mut`
  (`Type::Apply` never matches `emit_let`'s `Type::Named`-keyed `is_file_handle` set, so a
  `val`-bound channel/sender/task stays a plain `let`, exactly as the AST renders).
- **`Task<Unit>` needed a tiny element widening: `Unit` is admitted ONLY as a concurrency
  element.** A `() => { … }` spawn closure that returns nothing yields `Task<Unit>`
  (`Type::Named("Unit")`), and the `[Task<Unit>]` worker list (34_parallel_scan) needs that
  element covered. `Unit` is not a covered value type generally (no binding/param surface),
  but it renders via `cx.rust_type` to `()` (Context.rs), so `JetTask<()>` is byte-
  identical — `concurrency_elem_covered` admits `Unit` plus any covered value type,
  scoped to the concurrency `Apply` arg where `Unit` can only appear as the erased result
  of a unit-returning task.
- **Surfaced an UNRELATED uncovered construct (parity-safe, not a bug): `if x ==
  value(binding)`.** `34_parallel_scan` `scan_parallel`/`paths_from_args` use an
  optional-binding test in an `if` condition (`if paths.get(i) == value(path) { … path … }`)
  — an `if`-let-style shape the TIR doesn't yet lower, so those fns stay on the AST path.
  Verified byte-identical on both paths (it just stays uncovered; no miscompile, no
  regression). NOT a concurrency gap: with the `if value()` removed, `scan_parallel` routes
  fully (channel + sender + `take(…)` spawn + join + receive + `[Task<Unit>]` + `?? panic`),
  byte-identical, and runs. Logged as the remaining coverable-but-uncovered example
  construct (an `if`-condition optional binding), independent of c109's type surfaces.
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass on
  `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the **entire
  example suite** — 118 emittable, 0 differ — incl. all four concurrency examples
  (`32_tasks` spawn/join, `80_detached_task` detach, `33_pipeline` channel send/receive,
  `34_parallel_scan` the parallel file scan) — plus crafted probes (a full
  channel+sender+spawn+join+receive program; a `scan_parallel` with the unrelated
  `if value()` removed, which routes fully). A routing sentinel confirmed the covered fns
  take the TIR path: all four example `main`s route (`32_tasks` `sum_range`+`main`;
  `80_detached` `main`; `33_pipeline` `main`; `34_parallel_scan` `scan_one`/`default_paths`/
  `gather_paths`/`copy_paths`/`main`). All four examples run end-to-end (`5050`; `launched`;
  `10`/`20`/`30`; the parallel scan). 235 top-level fns route through `emit_tir_toplevel`
  now. tests/tir.rs adds `task_spawn_join`/`task_detach`/`channel_send_receive` (build+run);
  the TIR unit tests add `concurrency_method_names`/`concurrency_value_types_covered`/
  `covers_concurrency_methods` and flip `core_call_covered("core.tasks","channel")` (now true).
- **After Phase 21 the AST-path inventory contains ONLY latent-codegen-bug-blocked +
  sema-unreachable constructs.** No function in the live example suite routes via the AST
  path for a *type/method surface* the TIR can't model — every emittable example fn either
  routes through the TIR or is held back by a pre-existing latent bug (`is_empty`, no-arg
  `join()`, `mut self`/`view self` reassignment, recursive structs, view-returning trait
  methods — the TIR must not *claim* a miscompiling fn), a sema-unreachable construct
  (generic-type methods), the dead bare `?? return`, OR an as-yet-uncovered but parity-safe
  surface form (the `if x == value(binding)` optional-binding condition above; the
  method-call-collection iteration `loop x in s.split(…)`). The remaining work toward
  deleting the AST path is the latent-bug FIXES (separate cards) + these last
  parity-safe construct forms, NOT a new type/method surface.

## Refinements learned in Phase 22 (method-call iteration + optional-binding conditions)

- **`emit_for_in` has FOUR branches keyed on the collection's `Expr::MethodCall` shape;
  the method-call form is carried as a total `ForIn.method_kind`, the receiver/collection
  string resolved at lowering.** `loop x in <coll>`: a `chars()` collection emits
  `for _jet_c in ({recv}).chars()` (binding `_jet_c`); a `lines()` collection on a
  `FileReader` emits the streaming `for _jet_raw_line in std::io::BufRead::lines(&mut
  ({recv}).inner)` with a mid-stream-error panic; a `lines()` on a `StdinHandle`/inline
  `io.stdin()` adds an extra `{ let mut _jet_stdin_h = …; }` block (the temporary must
  outlive the loop body) + a matching extra closing brace; ANY OTHER method falls to the
  `.iter().cloned()` default (`.split(…)` returns a `[String]` value, emitted whole). The
  load-bearing distinction: for `chars`/`lines` the emit reads the **receiver** (`recv`),
  for the default it reads the **whole collection** — so `ForIn.method_kind == None` holds
  the whole-call string and `Some(Chars|LinesFile|LinesStdin)` holds the receiver string.
  The FileReader-vs-stdin split mirrors the AST's `expr_jet_ty(receiver)` (reproduced by
  `tir_recv_jet_ty`, byte-identical) AND the inline-`io.stdin()` MethodCall-shape test,
  checked in the SAME order `emit_for_in` does (FileReader first). The panic line is the
  literal `0` the AST embeds (no inference).
- **The iteration var stays type-`None` in the method-call branch too** (the recurring
  Phase-5 partiality lesson): `_jet_c`/the line var bind with `jet_ty: None`, so they never
  enable the overflow trap and never enter the `??` panic dump (`safe_locals_expr` filters
  to scalar Int/Float/Bool). The two-binding map form is impossible for a method-call
  collection (single-binding only), so the gate excludes `var2.is_some()` there.
- **The optional-binding `if` condition is a NEW `TStmt::If.cond` enum (`TIfCond`), not a
  plain `TExpr` — `emit_if` has THREE condition heads.** Before Phase 22 `TStmt::If`
  carried `cond: TExpr` (always `if {expr} {`). `emit_if` actually forks on the condition
  shape (`if_pattern_test`): an `x == value(b)`/`ok(b)`/`err(b)` `Expr::PatternTest`
  → `if let {pat} = {subj} {` (the Rust pattern from `emit_if_let_pattern`, now
  `pub(crate)` and reused for byte-parity); an `x == null` (`Pattern::Absent`) →
  `if {subj}.is_none() {`; anything else → plain. So `cond` became `TIfCond::{Plain,IfLet,
  IsNone}` resolved at lowering. Only the DIRECT `PatternTest` forms are covered (the
  `Binary(And, …)` shape `if_pattern_test` also admits — which DROPS the `&&` rhs, a
  latent AST quirk — stays on the AST path, conservative); Variant/Or/Range `if`-condition
  patterns (only the prelude `JSON` enum uses them in the live suite — out of subset
  anyway) are excluded.
- **The if-let binding's scope is the SUBTLE parity point: the AST clones the env into a
  fresh `body_env` (deep copy) before `add_pattern_bindings`, so it is NON-leaky.** A plain
  / `is_none` `if` then-body is emitted on the SHARED `&mut env` (leaks into the `??` panic
  dump → `clone_env`, Rc-shared `panic_locals`), but the if-let then-body uses
  `env.clone()` (a deep copy → `fork_panic`, deep-copied `panic_locals`). So a `let` inside
  an if-let then-body does NOT leak into the enclosing fn's panic dump, exactly matching the
  AST's `body_env`. The bound name binds with its inner type read off the lowered subject's
  total `Option`/`Result` `.ty` (mirroring `add_pattern_bindings` — never re-inferred, I3).
- **Verified byte-identical** via a forced-AST-path diff (a temporary `JET_NO_TIR` bypass on
  `emit_func`/`emit_method`/`emit_trait_method` + the delegation hook) across the **entire
  example suite** — 118 emittable, 0 differ — exercising every form: char iteration
  (`17_strings`), `.split(…)` iteration (`16_wordcount`/`35_zerocopy`/`34_parallel_scan`
  `count_lines`), streaming stdin `.lines()` (`78_stdin_filter`), and the optional-binding
  condition (`34_parallel_scan` `paths_from_args`/`scan_parallel` — now routing fully). A
  routing sentinel confirmed the target fns take the TIR path (`scan_parallel`/
  `paths_from_args`/`count_lines`/`main` in 34; `78_stdin_filter` `main`); all run
  end-to-end. tests/tir.rs adds `method_call_collection_iteration` + `optional_binding_if_
  condition` (build+run); the TIR unit tests flip `rejects_method_call_collection_iteration`
  → `covers_method_call_collection_iteration` (chars + split) and add
  `covers_optional_binding_if_condition` (value-binding + `is_none`).
- **FileReader `.lines()` is reproduced but UNREACHABLE in the live suite** — the only
  FileReader-lines example (`49_stream` `count_lines`/`copy_upper`) is held back by a bare
  `?` on a core fallible (`files.open(src)?` with a `… ? IOError` return — excluded since
  Phase 10), so it never reaches the loop. The branch is covered for totality + tested via a
  crafted probe; it routes the moment a non-`?`-blocked FileReader-lines fn appears.
- **The "parity-safe coverable construct form" residue is now EMPTY.** What remains on the
  AST path is latent-bug-gated, unreachable/dead, OR an uncovered FEATURE surface (tuples,
  default params, distinct types, `#Pure`, `#Todo`, JSON/core-enum matching) — NOT a flagged
  construct form. See the corrected inventory below.

## Refinements learned in Phase 23 (pure / todo / default-params / named-args / distinct / tuples)

- **`#Pure fn` (S60) and `#Todo` holes are pure ERASURE/total-fact lifts — the easy wins.**
  `#Pure` is a sema-only check (E3401: a `#Pure fn` may only call other `#Pure fn`s);
  NO codegen path reads `f.is_pure` outside the gates, so a `#Pure fn` lowers + emits
  byte-identically to a plain fn (lift = delete the `is_pure` gate exclusions). `#Todo`
  (`Expr::Todo`) carries `expected_type` as a TOTAL sema fact (the inventory's note held);
  it lowers to a diverging `todo!("#Todo at {file}:{line} — expected {ty}")` (`TExprKind::
  Todo`), `cx.file`/line resolved at lowering, byte-for-byte the `Expr::Todo` emit. The
  gate EXCLUDES a `#Todo` whose `expected_type` is `None` (sema didn't resolve) — never
  guess the `(unknown)` fallback (the recurring "a partial sema fact ⇒ stay on AST" rule).
- **Default parameter values are a CALL-SITE rewrite, not a signature/body fact.** Sema
  (`CheckerItems` default-value filling, S61/D-NARG-D2) appends the omitted trailing
  args at every CALL — substituting earlier-param refs with the supplied arg expr
  (`substitute_param_refs`) — so by codegen the call's arg list is COMPLETE and the
  default expr is GONE from the AST. Codegen never reads `p.default` (the param emits
  `name: ty` regardless). So covering defaults = delete the `p.default.is_some()` gate
  exclusions; the call sites were already in subset (a normal complete-arg call). This is
  the cleanest possible lift — the "hard" feature dissolved into a sema fact already
  resolved upstream.
- **Call-site LABELS (named args, D-NARG1) are checked DOCUMENTATION — they never reorder.**
  D-NARG-D4 (E0125): a label must name the parameter at its OWN position; arguments stay
  in declared order. Codegen never reads `CallArg.label` (`emit_call_args` is purely
  positional). So a labeled arg emits byte-identically to an unlabeled one — the gate's
  `a.label.is_none()` checks on the plain/instance/static call shapes were relaxable for
  free (parity-safe). (The named-ENUM-payload reorder is a DIFFERENT mechanism —
  `EnumLitArg::Named`, not `CallArg.label` — and was already handled.) Note: relaxing
  labels did NOT change the routed-fn count, because the live-suite labeled-call `main`s
  have OTHER blockers — `63_named_args` `main` hits the `new`-name-collision intercept
  (`Rect.new` static call; `new` ∈ `is_intercepted_method_name`, a pre-existing
  conservative exclusion / latent builtin-name-collision bug), `65_io_prelude` `greet`
  hits the ambient `input()` builtin (not in `cx.sigs`, uncovered). Both unrelated to
  named args.
- **Distinct types render as their `user_<Name>` newtype already — the lift is a value-type
  gate + two tiny shapes.** A distinct type (`UserId @= distinct Int`, D-DIST1) renders via
  `cx.rust_type`'s `Type::Named` fallthrough to `user_<Name>` (the emitted `#[repr(
  transparent)] struct user_<Name>(pub Base)` is a SEPARATE item, not fn-TIR). So passing/
  binding/returning one is byte-identical with no new emit — `is_covered_distinct_ty` admits
  it (a new `cx.distinct_types: HashMap<Name, (base, is_numeric)>` populated in `build_cx`).
  Three constructs: (a) construction `UserId(x)` is a bare `Expr::Call` whose name is NOT in
  `cx.sigs` (so the AST `emit_call` falls through to `user_<Name>(args)` with NO sig → plain
  args) — covered as the `is_distinct_ctor` `Call` shape (the existing fallthrough `Call`
  lowering reproduces it; `call_return_type` now yields the distinct type); (b) `.raw()`
  (D-DIST3) is special-cased in `emit_method_call` BEFORE any user dispatch (`(recv).0`),
  with `recv_type == None` (sema's `.raw()` arm returns the base type WITHOUT the recv_type
  writeback) — covered as a dedicated `DistinctRaw` shape keyed on the method name (sema's
  E0311 guarantees `.raw()` only on a distinct, so an in-subset 0-arg `.raw()` is safe to
  claim); (c) `#Numeric` distinct `+`/`==` use the NATIVE Rust operator — `ast_operand_is_
  integer` returns `None` for a distinct-typed operand (a `Named` isn't `is_integer()`), so
  the overflow trap is never claimed, matching the AST path's plain `+` exactly.
- **Named tuples are a GENERATED-struct surface; the literal's canonical field order is the
  load-bearing fact.** A `(x: Int, y: Int)` tuple (S73/D-SG7, `Type::Tuple`) renders via
  `cx.rust_type` to `tuple_struct_name(...)` = `JetTup_<hash>` (an order-sensitive hash of
  the type's fields) — the struct + its `user_<field>` fields are emitted by `Tuples.rs` for
  every tuple SHAPE the program uses, so the value type is byte-identical with no new emit
  (`is_covered_tuple_ty`). A tuple LITERAL `(x: 1, y: 2)` reorders its values to the TYPE's
  CANONICAL field order (a `(y: 3, x: 4)` literal emits `JetTup_…{ user_x: 4, user_y: 3 }`)
  — so the `TupleLit` shape reads the canonical order from the literal's sema-attached
  `Type::Tuple` (`tuple_fields_plain`) and reorders the values at lowering; the gate EXCLUDES
  a literal whose `ty` is `None` (sema didn't resolve the shape — never guess the AST's
  empty-canonical `0i64` default). A tuple FIELD read `p.x` is the generic `Field` shape
  (`(p).user_x` — `struct_field_type` now resolves off `Type::Tuple` directly, since a tuple
  has no `cx.struct_fields` entry). A tuple DESTRUCTURE `(a, b) @= p.clone()` (S74,
  `BindPattern::Tuple`) is a ONE-`Stmt`-to-MANY-Rust-lines form — a new `TStmt::TupleDestructure`
  carries the `__jet_d{span}` borrow-temp + per-element `(tmp).user_<canonical-field>.clone()`
  binds (pairing pattern elements to the type's canonical fields BY POSITION, resolved off
  the lowered init's total `Type::Tuple`). The Struct/List destructure forms stay on AST (no
  live-suite use).
- **DEFERRED: the JSON/core-enum + capstone slice (surface 6) — a COUPLED foreign-enum slice,
  like Phase 21's concurrency.** The prelude `JSON` enum is a FOREIGN enum: construction
  (`JSON.Object(obj)` → `jet_std::Json::Object(…)`, a non-mangled `emit_core_json_lit`
  literal — distinct from `user_<Enum>::user_<V>`), pattern matching (`when`/`if` over
  `Object`/`Number`/`Text`/`Boolean`, with `core_json_pattern_types` payload typing + the
  `is_json` non-mangled-variant `emit_pattern_match_switch` branch), the JSON value type as
  a param/return, and the `json.render`/`parse` core-call interplay must ALL land together —
  covering one piece leaves the others unreachable or risks an I2 false-positive (the
  foreign-enum `recv_type`/value-type is NOT a covered struct/enum, so the existing enum
  coverage rejects it; widening it half-way would claim a miscompiling match). This is the
  correct conservative call (exclude on any doubt). The capstone family (`logbook` `parse`/
  `build`/`note_*`/`note_score`/`graph_json`, `30_json`/`73_json_coerce`/`74_regex`/
  `76_http_routes` `main`) is blocked by this JSON slice PLUS other uncovered nodes (regex
  `Match`, comptime tables, cross-module foreign structs) — each a separate coverage phase.
- **Verified byte-identical** via the forced-AST-path diff (a temporary `JET_NO_TIR` bypass
  on all four emit hooks) across the **entire example suite** — 118 emittable, 0 differ —
  exercising every Phase-23 form: `#Pure` (`62_pure`), `#Todo` (`58_todo_hole`), default
  params + earlier-param-ref defaults (`66_default_refs`), named args (`63_named_args`),
  distinct construction/`.raw()`/`#Numeric`-arith/`==` (`69_distinct_types`), and tuple
  literal/field/destructure/return (`40_tuples`). A routing sentinel confirmed the target
  fns take the TIR path (`62_pure` double/greeting/main; `58` not_yet/double/main; `66`
  box_dims/main + `Rect.square`; `69` greet/main; `40` bounds/main; `63` `Rect.new`/`area`/
  `scale` methods). 268 top-level fns now route through the TIR (was 235 after Phase 21).
  tests/tir.rs adds `pure_fn`/`todo_hole`/`default_param_values`/`named_args`/`distinct_types`/
  `named_tuples` (build+run); the TIR unit tests add `covers_pure_fn`/`covers_todo_hole`
  (exclusion when `expected_type` unset)/`covers_default_params`/`covers_distinct_value_type_
  and_ctor`/`covers_tuple_value_type`/`covers_named_args_at_call_site`/`covers_default_param_
  method`. After Phase 23 the AST-path residue is: latent-bug-gated + unreachable/dead + the
  ONE remaining coupled feature slice (JSON/core-enum + capstone).

## What still routes via the AST path (for Phase N — delete the AST path)

After Phase 22 the AST `emit_func`/`emit_method`/`emit_trait_method` paths are reached
for three disjoint classes of construct: (1) constructs that MISCOMPILE on both paths
today (latent codegen/sema bugs the TIR must not *claim*); (2) constructs genuinely
UNREACHABLE / dead in any valid Jet program; and (3) **as-yet-uncovered FEATURE
surfaces** that are not yet a TIR coverage phase. Phase 22 covered the last two
parity-safe construct *forms* the inventory had explicitly flagged — the
**method-call-collection iteration** (`.chars()`/`.lines()`/`.split(…)`) and the
**optional-binding `if` condition** (`if x == value(b)` / `x == null`) — so
`scan_parallel`/`paths_from_args` (34_parallel_scan) and `78_stdin_filter` `main` now
route, and there is no longer any flagged "coverable construct form" residue.

**Correction to the post-Phase-21 claim:** the live example suite is NOT yet at zero
AST-path routing. Beyond the latent-bug-gated + unreachable constructs, a non-trivial
set of functions still routes via the AST path because they use **feature surfaces no
TIR phase has covered yet** (these were never a "type/method surface" — the narrower
thing Phase 21 claimed exhausted — so they were silently outside that claim). The
**precise** residue after Phase 22, by class:

**(1) Latent-bug-gated (MISCOMPILE on both paths — the TIR must not claim them):**
- `is_empty` (typed `Int` not `Bool` → E0308), no-arg `join()` (dead — sema requires a
  separator), `mut self`/`view self` **reassignment** (`self = …` → E0308), **recursive
  (boxed) STRUCTS** (`emit_struct_lit` omits the `Box::new(…)` field wrap → E0308),
  **view-returning TRAIT methods** (`emit_trait_def` drops `is_view_return` → E0053).
  All on the latent-bug list; unblock only when the underlying codegen/sema bug is fixed.

**(2) UNREACHABLE / dead (no valid Jet program exercises them):**
- **generic-type METHODS** (a method on a generic struct doesn't type-check — E0311 at
  the call site, E0119 on a `T`-referencing body; a sema binding gap, `struct_is_generic`
  exclusion stays); the **bare `?? return`** (ALREADY routes, but is unusable — E0405
  demands a return type, then rustc E0069 rejects `return;`). Foreign-ENUM *construction*
  has no reachable cross-module literal syntax (`note.Color.Red` is E0107).

**(3) Uncovered FEATURE surfaces (parity-safe; a future coverage phase, NOT a latent
bug, NOT unreachable):** these keep real example functions on the AST path. **Phase 23
covered five of the six** (`#Pure`, `#Todo`, default params + named args, distinct types,
tuples) — the only class-(3) surface still routing is the JSON/core-enum + capstone
slice:
- ~~`#Pure fn`~~ — **COVERED (Phase 23)**: purity is sema-only (E3401), erased at codegen;
  `62_pure` `double`/`greeting`/`main` route.
- ~~default parameter values~~ + ~~named args~~ — **COVERED (Phase 23)**: sema fills omitted
  trailing args at the CALL SITE (codegen never reads `p.default`); labels never reorder
  (D-NARG-D4) and codegen ignores `CallArg.label`. `66_default_refs` `box_dims`/`Rect.square`/
  `main`, `63_named_args` `Rect.new`/`area`/`scale` (methods) route. (`63_named_args`/
  `65_io_prelude` `main`/`greet` still route via AST — blocked by the `new`-name-collision
  intercept and the ambient `input()` builtin respectively, NOT by named-args/defaults.)
- ~~distinct types~~ — **COVERED (Phase 23)**: `is_covered_distinct_ty` admits the value
  type; `Name(x)` ctor is the `is_distinct_ctor` `Call` shape; `.raw()` → `(recv).0`
  (`DistinctRaw`); `#Numeric` `+`/`==` use the native operator. `69_distinct_types`
  `greet`/`main` route.
- ~~tuple types~~ — **COVERED (Phase 23)**: `is_covered_tuple_ty`; `(x: 1, y: 2)` →
  `JetTup_<hash>{…}` canonical-ordered (`TupleLit`); `p.x` → `(p).user_x` (generic
  `Field`); `(a, b) @= p.clone()` → the borrow-temp + per-field-clone form
  (`TupleDestructure`). `40_tuples` `bounds`/`main` route.
- ~~`#Todo` holes~~ — **COVERED (Phase 23)**: `Expr::Todo` → diverging
  `todo!("#Todo at … — expected <ty>")` (`Todo`, reads sema's `expected_type`).
  `58_todo_hole` `not_yet`/`double`/`main` route.
- **STILL ON AST — the JSON / core-enum + capstone slice** (DEFERRED, see Phase 23
  refinements): `when`/`if` over the prelude `JSON` enum (`Object`/`Number`/`Text`/
  `Boolean` patterns + `if data == Object(entries)` binding), foreign-enum CONSTRUCTION
  (`JSON.Object(obj)` → `jet_std::Json::Object(…)`, a non-mangled foreign-enum literal),
  the JSON value type as a param/return, and the `json.render`/`parse` core-call interplay
  — a COUPLED slice (the value type + construction + matching + core calls must land
  together, like the Phase-21 concurrency slice; covering one piece leaves the others
  unreachable or risks an I2 false-positive). Plus **comptime-evaluated tables** and the
  capstone's cross-module foreign-type-heavy functions (the logbook `parse`/`build`/
  `note_*`/`note_score`/`graph_json` family, `30_json`/`73_json_coerce`/`74_regex`/
  `76_http_routes` `main`) — each blocked by an as-yet-uncovered node its body reaches
  (foreign-enum JSON, regex `Match`, comptime tables, cross-module foreign structs).

The remaining work toward deleting the AST path is therefore: the latent-bug FIXES
(separate cards), the unreachable/dead constructs (no action), plus the **one remaining
class-(3) coupled slice** — JSON/core-enum matching + the capstone foreign-type-heavy
functions. None of the residue is a "parity-safe construct form already flagged as
coverable" — that category stays EMPTY (Phase 22 cleared it; Phase 23 cleared five of
the six uncovered FEATURE surfaces).

## Phases

| # | Scope | Status |
|---|-------|--------|
| 0 | Inventory + TIR type-model design | ✅ done (4b89af5) |
| 1 | Simple functions: literals, operators, bindings, returns, if/else, calls, print | ✅ done (398138b) — 54 fns routed |
| 2 | Control flow: `loop`{infinite/while/range}, break/continue (+labels) | ✅ done (c109 Phase 2) — +3 example fns routed (e.g. fizzbuzz `main`). Excludes `loop x in <collection>` (ForKind::In, key/value map form) → Phase 5. |
| 3 | Structs: struct literals, field reads, struct-typed params/locals/returns | ✅ done (c109 Phase 3) — 57→65 covered fns. Covers plain (non-generic, non-recursive) user struct literals, borrow-position field reads (incl. nested/chained), struct params/locals/returns. Excludes: owning field reads (sema rewrites them to a `.clone()` MethodCall → **landed Phase 6**), trait-coerced literals (`as_trait`), imported-namespace/prelude structs, generic & recursive (`Box<…>`) structs, and **field assignment** (not a Jet construct — `s.field = value` is E0003; struct mutation is method-only, and a `mut self` method reassigning `self` is a separate pre-existing codegen bug, gated out in Phase 7). |
| 4 | Enums + `when`/match + patterns (incl. range/or patterns) | ✅ done (c109 Phase 4) — ~7 more example fns routed (`11_enums` `next`/`label`/`main`, `71_pattern_matching` `describe_conn`/`classify`/`score_grade`/`main`). Covers plain (non-generic, non-recursive, Clone-derivable) scalar/Char-payload user enums: unit/positional/named enum literals; exhaustive variant `match` (variant patterns, positional/named/wildcard/range payload slots, or-patterns, payload bindings); arm-head range switches (`lo..hi -> …` + `else`); enum params/locals/returns. Excludes: String/struct/collection-payload enums (clone/borrow at literal & binding site → Phase 7), recursive (`Box<…>`) enums, generic enums, JSON/foreign/prelude enums, fallible/optional patterns (`value`/`null`/`ok`/`err` → Phase 8), and mixed comparison/Bool switches (general `emit_mixed_switch`). |
| 5 | Collections: list/map literals, indexing/slicing, `loop x in collection` | ✅ done (c109 Phase 5) — list/map literals (incl. empty `[]`/`[:]`), `coll[i]` indexing (List/Map via the total sema `IndexKind`), `coll[a..b]` slicing, `coll[i] = v` index-assignment, and collection iteration (`loop x in coll` single-binding + `loop k, v in map` two-binding). List/map-typed params/locals/returns now allowed; element/key/value types may be scalar/Char/String/covered-struct/covered-enum/nested-collection. Excludes: collection *methods* (`.push`/`.len`/`.map`/etc. → Phase 7), method-call collections in iteration (`.chars()`/`.lines()` → Phase 7), `[E#N]` fixed-size lists, list-of-option/trait/fn/tuple element types, and any `Index`/index-assign whose `IndexKind` sema left `Unknown`. |
| 6 | Methods + method calls (recv_type → resolved target), trait-impl methods, the sema-inserted `.clone()` | ✅ done (c109 Phase 6) — +3 example fns routed (58→61) **plus** the clone path unblocks getters that move a non-Copy field out (Phases 3–5 excluded these). Covers: the synthetic `.clone()` MethodCall (owning field read / borrowed value); user-defined **instance** methods (`recv.m(args)`) on a covered struct/enum whose `recv_type` is a covered type and whose name is not a builtin/core/special intercept; trait-impl methods (bare name, no `user_` mangle, via `cx.trait_methods`); method args with `implicit_clone`/`shared_auto_clone`/`Read`/`Mutate`/`Move` conventions (mirroring `emit_call_args`). Excludes: **static** calls (`Type.m()` — `recv_type` is `None`); any method whose name a core/stdlib/collection/string/numeric lowering intercepts (`emit_builtin_method` + `.raw()`/`.snapshot()`/alloc); Fn-typed method args (Box-coercion form); core/foreign/prelude-type receivers; fallible/optional methods (→ Phase 8). The method *body* (it has `self`) routed via the AST path until **Phase 7** covered `self`-functions. |
| 6b | Remaining ownership facts: `take`/`mut`/`view` access conventions on free-function args, general Shared/Arc auto-clone outside method args | pending — the Phase-6 method-arg path already emits `Arc::clone(&…)` and `&mut (…)` from the total flags; extend the same to free `Call` args + view returns. |
| 7 | Method bodies + static methods | ✅ done (c109 Phase 7) — ~4 example methods routed (`10_structs` `Point::dist_sq`/`Point::unit`, `63_named_args` `Rect::new`/`Rect::area`). Covers: **inherent** method *bodies* (instance + static) on a covered struct/enum, hooked into `emit_method`. The `self` slot reproduces `emit_method` exactly (place `self`, no deref, type `None`); the receiver form is `&self`/`&mut self`/`self` per the resolved convention (`TFuncKind::Method`). **Static** (associated) methods are covered on both sides — the body (no self) and the call site (`Type.make(x)` → `user_<T>::user_<m>(x)`, a new `TExprKind::StaticCall`, which Phase 6 deferred). `Self` params/returns resolve to the owning type. Excludes: **trait-impl** method bodies (distinct signature — bare name, no `pub`, always `&self`, self `jet_ty: Some(T)`; a separate provable-parity hook); **delegation** (`using field`), **generic-type**, and **`view`-returning** methods; any `mut self`/`view self` method that **reassigns `self`** (`self = …` is a pre-existing AST-path I2 hole, gated out via `stmt_assigns_self`); methods whose body uses any not-yet-covered construct (core/stdlib calls, `?`/optionals, lambdas → Phases 8–10). |
| 8 | Fallible/optional: `?`/try (TryConvert), `??`, `T?` optionals, `ok`/`err` | ✅ done (c109 Phase 8) — ~5 example fns routed (`13_errors` `parse_age`/`load`/`main`, `12_option` `find_even`/`main`). Covers: **optional** `T?` (`Type::Option`) and **fallible** `T ? E` (`Type::Result`) params/locals/returns whose payloads are covered value types (incl. default `Error`→`String`); the `value(x)`/`null` and `ok(x)`/`err(e)` constructors; the `?` propagation operator carrying the total `TryConvert` (`None`/`Fallible`/`Typed(fn)` → bare / `.map_err(\|e\| e.to_error())` / `.map_err(<fn>)`, all wrapped in `jet_trace_err(…)?` with the trace-frame `file`/`line`/`fn_name` resolved at lowering); the `??` fallback (`OrFallback::{Value,Return}`, `is_option` total → `match` over Result/Option); `?.` optional chaining (the total `flatten` fact → `.and_then`/`.map`); and `when ok/err/value/null` matches (Shape C, reusing `EnumMatch`). Excludes: the `??` **panic** fallback form (`safe_locals_expr` reproduction deferred); String/struct/collection-payload error *enums* (non-covered enum → excluded); list/map *of* options; core/stdlib fallible calls (`fs.read` → Phase 10, but a USER fallible call is covered); nested `T??` (sema-rejected anyway). |
| 9 | Built-in collection/string methods (`emit_builtin_method`: list/map/string surface) | ✅ done (c109 Phase 9) — +6 example fns routed (`15_lists`/`20_list_remove_bounds`/`38_method_chain` `main`, `28_comptime_table` `make_table`, `34_parallel_scan` `copy_paths`/`main`). Covers the NON-closure list/map/string builtins (`len`/`push`/`pop`/`insert`/`remove`/`get`/`first`/`last`/`contains`/`index_of`/`reverse`/`sort`/`clear`/`join(sep)`/`keys`/`values`/`contains_key`/`chars`/`bytes`/`trim`/`split`/`starts_with`/`ends_with`/`replace`/`to_upper`/`to_lower`/`repeat`/`slice`/`to_string`) on a list/map/string receiver. The Map-vs-List-vs-String emit branch (`rty = expr_jet_ty(receiver)`) is resolved at lowering into a total `TBuiltinOp` (`TExprKind::BuiltinMethod`), reproducing the AST's `expr_jet_ty` partiality (incl. its `None` → default branch) exactly. Excludes: **closure-taking** methods (`map`/`filter`/`each`/`find`/`any`/`all`/`sort_by`/`reduce` → Phase 11); the **numeric** width/predicate/bit methods (`to_i32`/`is_nan`/`count_ones`/… → Phase 12, already carry `Some(recv_type)`); **handle** methods (FileWriter/TcpStream/HttpRequest/Router/Arena/… → Phase 10, also `Some(recv_type)`); `clone`/`raw`/`snapshot`/`new` (Phase 6 special cases); `Int.parse`/`Float.parse`/`String.from_bytes` (type-name-receiver statics); and **`is_empty`** + no-arg `join()` (both unusable today — see latent bugs). |
| 11 | Lambdas/closures (capture meta), fn-typed values, fan-out | ✅ done (c109 Phase 11, partial) — `23_closures` `main` now routes (map/filter/reduce/sort_by/each), `41_fan_out` `double` routes; ~86 example fns route through the TIR. Covers: **lambda/closure literals** (`Expr::Lambda`) reading `Lambda.meta.{escapes, needs_fn_mut, mut_captures, cloned_captures}` as TOTAL facts — the `move ` keyword (off `needs_fn_mut && !escapes`), `Box::new(…)` (off `escapes`), and the per-`cloned_capture` `let _jet_cap_<n> = (place).clone();` prelude all resolved at lowering, body lowered on a cloned env extended with params + captures (`TExprKind::Lambda`/`TLambda`); the **fan-out** operator `f.[a, b, c]` (S75/S76) for a plain top-level-fn callee, each item a synthetic single-arg call wrapped in `vec![…]` (`TExprKind::FanOut`); the **closure-taking collection methods** `map`/`filter`/`each`/`find`/`any`/`all`/`sort_by`/`reduce` (`TExprKind::ClosureMethod`/`TClosureOp`), the Map-vs-list-vs-trait-object and Fn-vs-FnMut emit branches resolved at lowering from `tir_recv_jet_ty` + the lambda's `needs_fn_mut`. Excludes (deferred): **fn-typed values** (a fn stored/passed/returned — the `Box::new(…) as <fn-type>` coercion in `emit_call_args`; the gate now excludes any plain call whose callee has a Fn-typed param so a lambda/fn-value never reaches the un-coerced path); the **closure-taking core calls** `tasks.spawn` (distinct `emit_spawn_lambda` form), `http.serve` (router-vs-lambda branch), `scope.guard` (not in `core_fixed_sig`); the `??` **panic** fallback (still needs the `safe_locals_expr` reproduction). |
| 10 | Core/stdlib calls, imports/modules, FFI, comptime-if, arena/unsafe | ✅ done (c109 Phase 10) — the **type-monomorphic** core/stdlib calls now route through the TIR (`TExprKind::CoreCall`). Covered: every `(module, method)` whose full signature is fixed by `Sema::core_fixed_sig` — `core.fs` (read/read_bytes/write/append/exists/is_dir/remove/create_dir/list_dir/copy/rename), `core.io` (args/read_all_input/stdin), `core.env` (get/set/current_dir/home_dir), `core.process` (exit/run), `core.math` (sqrt/pow/floor/ceil/round), `core.random` (int/float/seed), `core.time` (now/sleep/start), `core.json` + `jet.json` (parse/decode/render/render_pretty), `core.files` (open/create/append), `core.path` (join/parent/extension/normalize), `core.net` (all tcp_*), `jet.csv`/`jet.toml`/`jet.yaml` (parse/render), `jet.log` (info/warn/error/debug/set_level/set_trace_id/setup), `jet.time` (now/format), `jet.crypto` (sha256/sha256_bytes), `jet.http` (get/post), `jet.regex` (is_match/match/find/find_all/split/replace/replace_all). The `(module, method)` dispatch + per-arm `&(…)`/`&mut (…)`/move wrappers reproduce `emit_core_call` byte-for-byte; the return type comes from `core_fixed_sig` (totality), so a fallible core call composes with Phase-8 `?`/`??`. ~10 example fns route (across crafted programs exercising 24 covered core calls). Excludes (stay on AST path): closure-taking (`tasks.spawn`/`http.serve`/`scope.guard` → Phase 11); polymorphic math/random/io specials (`abs`/`min`/`max`/`clamp`/`pick`/`shuffle`/`io.input`/`io.eprint` — return type depends on arg type, not in the fixed table); handle-constructor specials not in the table (`tasks.channel`, `http.router`/`parse`/`dispatch`); `core.mem` ptr/alloc (`@unsafe`); and any use of a returned handle's METHOD surface (excludes the enclosing fn). Imports/modules (re-export, `import_mods`, inline code modules), FFI extern, comptime-if, arena/unsafe blocks are NOT yet covered (deferred within this phase). |
| 12 | The tail: trait-impl method bodies, numeric-conversion methods, + a parity fix | ✅ done (c109 Phase 12, partial) — +7 unique trait-method bodies routed (`Circle`/`Square` `area`/`name`, `Person` `announce`/`greeting`, `ConsoleLogger` `log`). Covers: **trait-impl method bodies** (`emit_trait_method`, both the inline `impl Trait {}` and `impl T: Trait` and external-trait-impl forms — hooked via the shared `emit_trait_method`), as a new `TFuncKind::TraitMethod` — bare name (the trait owns it, no `user_` mangle), no `pub`, always-`&self` receiver, self slot `jet_ty: Some(Type::Named(T))` (NOT `None` as for inherent methods); the **numeric** predicate/bit/width-conversion methods (D-NUMOPS1: `is_nan`/`is_infinite`/`is_finite`, `count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros`, `to_i8`…`to_u64`/`to_int`/`to_f32`/`to_f64`/`to_float`, and numeric `to_string`) on a numeric receiver (`recv_type == Some(<numeric>)`) — a new `TExprKind::NumericMethod`/`TNumericOp`, the widening-vs-narrowing branch (`numeric_conversion`/`conv_rust_target`) resolved at lowering from the total `recv_type` width. Also fixes a **latent TIR-path parity bug**: an explicit `else { if … }` block was being flattened to `} else if …` (the AST keys solely on `ElseBranch`, never the else-body shape) — `TStmt::If` now carries `else_is_elseif` so only a real `ElseBranch::ElseIf` flattens. Excludes (stay on AST path): **delegation** (`emit_delegation_method`, `using field`), **generic-type** & **`view`-returning** & **`@unsafe fn`** trait methods; **fn-typed values** (Box-coercion); **closure-taking core calls** (`tasks.spawn`/`http.serve`/`scope.guard`); polymorphic math/random/io core specials; the `??` **panic** fallback; modules/imports, FFI extern, comptime-if, arena/`core.mem`/`#Unsafe` (see the AST-path inventory below). |
| 13 | The call & method surfaces: fn-typed values, closure-core-calls, handle methods | ✅ done (c109 Phase 13, partial) — `24_callbacks.jet` `apply_twice` routes; `67_scope_guard.jet` `with_guards`/`question_path`/`main` route. Covers: **fn-typed values** — a `Type::Fn` param/return (now a covered subset type, `is_covered_fn_ty`); a bare top-level-fn name as a value (`Box::new(move \|…\| user_<name>(…)) as <fn-type>` via `emit_named_fn_value`, `TFnValueKind::NamedFn`); a call through a fn-value (`(f)(args)` as `Expr::CallValue`, and `f(args)` for a local-`f` as `Expr::Call`, both → `TFnValueKind::Call`); the `emit_call_args` `Box::new(…) as <fn-type>` arg-coercion (the shared `lower_one_call_arg` carries a total `TFnCoerce{fn_type_rust, already_boxed}`). The Phase-11 `no_fn_param`/`!Type::Fn` exclusions are lifted. The **closure-taking core calls** `tasks.spawn` (distinct `render_spawn_lambda` `move \|…\|` form), `http.serve` (lambda-handler branch), `scope.guard` (`TExprKind::CoreClosureCall`/`TCoreClosureKind`) with a literal in-subset lambda. The **handle methods** carrying `recv_type == Some(<handle>)` on FileReader/FileWriter/StdinHandle/TcpStream/TcpListener (`read_line`/`write_line`/`flush`/`accept`/`local_addr`/`read`/`write`/`peer_addr`/`close` — `TExprKind::HandleMethod`/`THandleOp`, the `&mut`/`&` handle arms of `emit_builtin_method`). Also fixes a **latent TIR-path parity bug**: a handle binding (FileReader/FileWriter/TcpStream/HttpRouter/Arena/…) wasn't forced to `let mut` (it must be, as its methods take `&mut self`); the TIR `Let` now resolves `kw`+`ty_clause` (+ the escaping-FnMut `as <fn-trait>` coercion) at lowering, reproducing `emit_let` exactly. Excludes (stay on AST path, with reason): **polymorphic core specials** (`math.abs/min/max/clamp`, `random.pick/shuffle`, `io.eprint` — return type NOT a total fact, never written onto the node → I3); **`recv_type == None` handle methods** (`Stopwatch.elapsed_millis`, Task/Channel/Sender — a Phase-9 builtin gap, not the `Some` shape); **HttpRequest/HttpResponse accessors** (serve-lambda-param slot may be `None` → AST `rty` arm wouldn't fire → divergence); **`Match.group`**, **Arena/Bump/Pool/Fixed** methods (producer not covered); bare **`?? return`** (an upstream `??` gate gap); the `??` **panic** fallback. |
| 14 | Cross-module calls + FFI extern | ✅ done (c109 Phase 14) — all 8 module/FFI example `main`s route (`22_ffi`, `42`–`48` module forms), plus imported submodule bodies (`47` `wrap`/`decorate`, the inline-module fns in `42`/`45`/`46`). Covers the FIVE cross-module call forms — qualified `mod.fn()` (`import_mods`), `pub use` re-export (`reexport_calls`), inline code module `alias.method()` (`code_modules`), unqualified inline import (`unqualified_inline`), unqualified file import (`unqualified_file`) — each resolved at lowering into a total `TExprKind::ModuleCall`/`TModuleCallForm` (`Qualified{rust_mod, rust_fn}` or `InlineMangled{mangled}`); emit only prepends `cx.root_prefix` (program-level) where the AST does. Also covers **FFI extern** (`extern rust`/`extern C`) — `emit_call`'s `extern_funcs` arm as a total `TExprKind::ExternCall{wrapper, args}` reproducing `emit_extern_call_args` (a non-scalar `Read` arg is `(…).clone()`, NOT `&(…)`); the call is `{ffi_crate}::{wrapper}(args)` with `cx.ffi_crate` read at emit. Module-call args resolve their conventions from `cx.import_sigs`/`cx.sigs`; a new `cx.import_rets` table (`import_ret_map`) supplies the call's total return type. The gate widens the `Call` arm (extern/unqualified) and `method_call_in_subset` (reexport/import_mods/code_modules), reproducing `emit_call`/`emit_method_call`'s dispatch ORDER exactly. **I1:** an extern call introduces no Rust `unsafe` by itself — reproduced byte-for-byte (no `unsafe` emitted); the audited `@unsafe` gate surface is the separate, deferred Phase-17 work. Excludes (stay on AST path): the `@unsafe`/`#Unsafe`/`core.mem` ptr+alloc surface, comptime-if, and the other AST-path inventory entries (mixed switches, payload/boxed/generic types, latent-bug-gated constructs). |
| 15 | Remaining control flow + delegation: comptime-if, mixed switches, delegation, `?? panic` | ✅ done (c109 Phase 15) — covers the FOUR remaining inventory entries. **comptime-if** (`Stmt::ComptimeIf`): the selected branch (sema's `selected_then`) emits inline at the same indent with no `if`, its `let`s leaking into the outer scope (`TStmt::Inline`). **Mixed comparison/Bool switches** (shape D — the general `emit_mixed_switch` `if/else if … else` chain, used when arms are NOT all-variant/range/fallible): each plain comparison arm head resolves to an `emit_expr` string, wrapped in the `_jet_switch_subject` block (`TStmt::MixedSwitch`); a switch with any pattern-test arm stays on the AST path (conservative). **Delegation trait methods** (`impl T: Trait using field`, `emit_delegation_method`): a structural forward `(self).<field>.<method>(args)` with the bare trait name (`TFuncKind::Delegation`, hooked in `emit_external_trait_impl`). **The `?? panic(…)` fallback** (Phase-8/11 deferral now LIFTED): `emit_panic_stop`/`safe_locals_expr` reproduced from a leak-faithful `panic_locals` Rc-replica that mirrors the AST codegen env's branch-leak semantics (shared across leaky branches, deep-copied at the two `emit_pattern_match_switch` arm bodies + lambda bodies, range-loop var save/restored); the whole `{ jet_panic_rich(…); }` statement string (message + sorted scalar-locals dump) is rendered at lowering (`TOrFallback::Panic`). Also fixes a **latent TIR-path divergence** the panic lift exposed: a CORE-struct field read (`result.code` on a `ProcessResult`) was mangled to `user_code` instead of the plain `code` (B2) — fixed by reproducing `core_struct_field_rust_name` in the `Field` lowering off the resolved `recv.ty`. Byte-identical across the entire example suite (118 emittable, 0 differ) + crafted probes (incl. a `?? panic` with leaked scalar locals from a plain-if and a loop body). The delegation example `47_library` (`App: Logger using logger`) routes. Excludes (stay on AST path): bare `?? return` (an upstream `??` gate gap); `@unsafe`/`#Unsafe`/`core.mem`; generic & `view`-returning methods; polymorphic core specials; `recv_type == None` handle methods; HttpRequest/HttpResponse method accessors; payload/boxed/generic types; latent-bug-gated constructs. |
| 16 | Payload-carrying & recursive types: string/struct/collection-payload enums, recursive (boxed) enums, collection struct fields | ✅ done (c109 Phase 16) — +17 example fns route (201 → 218). Covers: **string/struct/collection-payload enums** (a variant payload may be String, a covered struct, a covered collection, or another covered enum) — the per-arg borrowed-`.clone()` + `Box::new(…)` decisions of `emit_boxed_enum_arg` are now a TOTAL fact (`TEnumArg { value, clone, boxed }`) resolved at lowering, byte-for-byte. **Recursive (boxed) enums** (linked-list / expr-AST) — the enum gate's `seen` set now ADMITS a self-reference, and the only box-related emit decision is the `Box::new(…)` at construction (Rust auto-derefs the `Box` at every read/match site, so no deref node is needed). **Collection struct fields** (`field_ty_covered` admits `[E]`/`[K, V]`) and **collection enum payloads**. The construction routes through a NEW variant-construction `MethodCall` shape (shape (j)) — a payload variant never becomes an `Expr::EnumLit` node (parser → `Field`/`MethodCall`; sema type-checks in place without rewriting), so the AST `emit_method_call`→`emit_enum_lit` path is reproduced. Excludes (stay on AST path, with reason): **recursive STRUCTS** (broken on the AST path — `emit_struct_lit` omits the `Box::new(…)` field wrap → rustc E0308; a latent I2 hole logged in the bug list; the TIR must not claim a miscompiling fn); **generic** & **foreign/prelude** types; **`view`-returning** methods (borrow lowering) — all deferred to a later phase. |
| 17 | Generics, foreign/prelude types, view-returning methods | ✅ done (c109 Phase 17, partial) — covers three independent surfaces. **`view`-returning functions/methods** (`-> view T` borrow): `emit_view_return` reproduced as a total `TStmt::ViewReturn { value, wrap }` (`wrap ∈ {Addr, Bare}` resolved at lowering from the AST node shape), `TFunc.is_view` drives `rust_return_type → &T`, the `view_return` flag rides `LowerEnv` (threaded through `clone_env`/`fork_panic`). `35_zerocopy` `name_of` routes. **Generic FREE functions** whose params/return are type vars (`<T: Clone>` clause rendered at lowering into `TFunc.generics` via `Generics::rust_type_param_list` + `rust_extra_clone_bounds`; a type-var param admitted by `is_subset_param_ty`, forced `Move` for the slot deref via `param_place_generic` — EXACTLY `emit_func`'s `is_type_param`) or `[T]` lists (`collection_elem_covered` admits a type var). **Constructable PRELUDE structs** (HttpRequest/HttpResponse): `emit_struct_lit`'s `is_prelude_struct` branch (`Jet…` head, PLAIN fields, HttpRequest's injected `params` field) reproduced via a total `StructLit { …, extra: Option<String> }`; the core/prelude/handle types (ProcessResult/Json/Stopwatch/FileReader/TcpStream/Arena/…) admitted as covered VALUE types (`is_covered_foreign_value_ty`). Byte-identical across the whole example suite (118 emittable, 0 differ) + crafted probes (view field accessor, `id`/`pick`/`firstof`/`wrap` generics, `build_resp`/`build_req` prelude construction). 206 free fns route through `emit_func`. tests/tir.rs adds `generic_free_fns`/`view_return_fn`/`prelude_struct_construction`; unit tests flip `rejects_generic_fn` → `covers_generic_fn` + add `rejects_generic_struct_fn`. Excludes (stay on AST path, with reason): **generic STRUCT types/methods** (`Pair<T>` `Type::Apply` value type, turbofish `user_Pair::<T> { … }`, `[T]`-field builtins, generic-type methods — `26_generic_types` `make_pair`/`empty_stack`/`push`); **FOREIGN (imported user) structs/enums** (cross-module `import_ns` construction — Phase 14 surface); **`view`-returning trait methods** (gate keeps the exclusion — conservative); recursive STRUCTS, latent-bug-gated constructs, and the remaining inventory entries (polymorphic core specials, `recv_type == None` handle methods, HttpRequest/HttpResponse method accessors, `@unsafe`/`#Unsafe`/`core.mem`). |
| 18 | @unsafe / #Unsafe / core.mem pointer tier | ✅ done (c109 Phase 18) — covers the audited expert low-level tier. **`#Unsafe fn`** (S58, E2-M13/D-LL1): a function-level `unsafe` — the `unsafe ` keyword prefixes the signature (`{vis}{unsafe_kw}fn` / `pub {unsafe_kw}fn` / trait-method `{unsafe_kw}fn`), body unchanged — carried as `TFunc.is_unsafe` (the three gates' `f.is_unsafe` exclusion LIFTED; `TFuncKind::TraitMethod` already carried it since Phase 12). **`#Unsafe { … }`** audited region (`Stmt::Unsafe`): lowers to `unsafe { … }` (`TStmt::Unsafe`), the `#Audit("…")` annotation emits NOTHING; body `let`s leak into the outer scope (lowered/gated on the SAME env/locals, like comptime-if). **`core.mem` POINTER ops**: `mem.Ptr<T>.from_addr(addr)` (`Expr::PtrFromAddr` → `TExprKind::PtrFromAddr`, total `elem`, safe cast `(({addr}) as usize as *mut {T})`, no `unsafe`); `mem.address_of(x)` → `Int` (`(&(x) as *const _ as usize as i64)`); `mem.volatile_read(p)` → `ptr_elem(p.ty)` (`std::ptr::read_volatile(p)`) — both NOT in `core_fixed_sig` but deterministic, admitted by `core_call_covered` with the return type special-cased at lowering. **I1 holds:** every emitted `unsafe` is a gated form (`unsafe fn` / `unsafe {`) tied 1:1 to a source `#Unsafe`/`#Unsafe fn` gate — verified by grepping the TIR-path Rust (drop the vetted `jet_mem` prelude). Byte-identical across the whole example suite (118 emittable, 0 differ) incl. both `#Unsafe`-tier examples (`48_lowlevel` + `showcase/lowlevel`), which run end-to-end. +4 example fns route (`read_reg`/`main`, `read_int`/`main`). tests/tir.rs adds `unsafe_fn_block_and_ptr_ops` + `unsafe_tier_emit_is_byte_exact`; unit tests add `covers_unsafe_fn_with_ptr_ops` / `covers_unsafe_block_and_address_of`. Excludes (stay on AST path, with reason): **`core.mem` ARENA allocators** (`Arena`/`Bump`/`Pool`/`Fixed` — `mem.<A>.new()` producer + `.alloc`/`.reset`/`.free` handle methods + `arena_view` bindings; their `unsafe` is vetted-`jet_mem`-only) and **`#Context(allocator:)`** / **`region r { … }`** (plain-block + RAII-guard emit) — a clean follow-up; plus the prior inventory residue (generic structs, polymorphic core specials, `recv_type == None` handle methods, HttpRequest/HttpResponse accessors, recursive/foreign structs, latent-bug-gated constructs). |
| 19 | Generic structs, foreign types, arena/region, Stopwatch | ✅ done (c109 Phase 19) — covers the UNBLOCKED inventory residue. **Generic STRUCTS** (free functions): a type-var struct field admitted by `field_ty_covered`; a `Type::Apply` (`Pair<T>`/`Stack<Int>`) param/return/local admitted by `is_covered_generic_struct_ty`; the turbofish StructLit (`user_Pair::<T> { … }`) + foreign `import_ns` head resolved at lowering into the total `StructLit.rust_type`; the `[T]`-field builtin routes via the Phase-9 shape unchanged. `26_generic_types` `make_pair`/`empty_stack`/`push` route. **FOREIGN (imported user) structs** as covered value types (`cx.foreign_types` → `{root}{mod}::user_<Name>` via `cx.rust_type`) + `alias.Note { … }` construction (`emit_struct_lit`'s `import_ns` branch reproduced); foreign enums covered as value types (construction has no reachable literal syntax). **`Stopwatch.elapsed_millis()`** — a `recv_type == None` builtin gap (gate shape d2 → `THandleOp::StopwatchElapsedMillis`). **Arena/region/context**: the `mem.<Alloc>.new()` producer (`TExprKind::AllocNew`, ctor tail rendered at lowering, claimed FIRST mirroring `emit_method_call`); `alloc`/`reset`/`free` handle methods (`THandleOp::{AllocAlloc,AllocReset,AllocFree}`); the `arena_view` binding (`let <x> = …;` no-type/no-mut + deref'd slot); `region r { … }` (`TStmt::Region`, leaky block); `#Context(allocator:) { … }` (`TStmt::ContextBlock`, RAII guard + leaky body). `70_arena`/`75_arena_regions`/`77_smart_context` route. **I1 holds:** the arena `unsafe` lives entirely in the vetted `jet_mem` prelude — the TIR-path `main`s emit ZERO `unsafe`. Byte-identical across the whole example suite (118 emittable, 0 differ) + crafted probes (2-file foreign module, Stopwatch, generic-struct method that emits byte-identically but stays excluded). ~240 fn-routes (was ~218). tests/tir.rs adds `generic_struct_fns`/`foreign_struct_construction`/`stopwatch_elapsed_millis`/`arena_alloc_reset_free`/`arena_region_block`/`smart_context_block`; unit tests flip `rejects_generic_struct_fn`/`rejects_generic_struct_literal` → `covers_*`, update `rejects_generic_method` (now via `struct_is_generic`) + `handle_method_op_table`. Excludes (stay on AST path, with reason): **generic-type METHODS** (`impl<T> user_<T>` — `struct_is_generic` excludes; conservative until validated end-to-end); **`view`-returning trait methods** (MISCOMPILE on both paths — `emit_trait_def` drops `is_view_return` → rustc E0053, a NEW latent bug); polymorphic core specials; Task/Channel/Sender `recv_type == None` methods (producers not covered); HttpRequest/HttpResponse accessors; recursive STRUCTS; latent-bug-gated constructs. |
| 20 | Polymorphic core specials, http accessors, generic methods (assessed) | ✅ done (c109 Phase 20) — covers the two remaining COVERABLE residue surfaces. **Polymorphic core specials** (`math.abs/min/max/clamp`, `random.pick/shuffle`, `io.eprint`): their arg-type-dependent return type is now a TOTAL fact — a new `Expr::MethodCall.resolved_ret: Option<Type>` field, written by sema (`infer_method_call` after `infer_core_call`, gated on the new `is_polymorphic_core_special`), read at lowering into the node's `ty`. The specials join `core_call_covered` + the Phase-10 `CoreCall` shape; the fixed emit strings (`(x).abs()`, `(a).min(b)`, `jet_std_random_pick(&(xs))`, `eprintln!`) are added to `emit_tir_core_call` byte-for-byte (`random.pick` → `Int?` proves the writeback). Adding the field touched 5 construct sites (rest `..`); no existing emit/diagnostic reads it, so behavior is unchanged. **HttpRequest/HttpResponse accessors** (`method`/`path`/`body`/`header`/`param`/`status`): the Phase-13 "serve-lambda-param unresolved" exclusion was a RED HERRING — `http.serve` REQUIRES an annotated handler param (an unannotated `(req) =>` is E0801, a sema gap, logged), so the accessors reach codegen only on a TYPED param where `recv_type == Some` AND the slot type is already total. Covered purely by adding the eight ops to `handle_method_op` + the `THandleOp::Http{Req,Resp}{Field,Header,Param}` emit arms (byte-for-byte `emit_builtin_method`); the handle-shape gate widened for free. `57_http_server` `handle` routes byte-identically. A speculative lambda-param-type writeback was tried + REVERTED (it changes emit for `[Shape]`-list `.each((s)=>…)` trait-object closures → rustc E0631, caught by `trait_impl_method_bodies`; the http coverage doesn't need it). Byte-identical across the whole example suite (118 emittable, 0 differ) + crafted probes. tests/tir.rs adds `polymorphic_core_specials` + `http_request_response_accessors`; unit tests add `polymorphic_core_specials_covered` + flip `handle_method_op_table`. **Assessed + DEFERRED (with reason):** **generic-type METHODS** are UNREACHABLE — a method on a generic struct doesn't type-check in current Jet (E0311 at the call site; E0119 if the body uses `T`); Phase 19's "byte-identical probe" was a `build_cx`-only AST bypassing sema's method binding. The `struct_is_generic` exclusion stays; the fix is in sema (logged in the latent-bug list). **Task/Channel/Sender `recv_type == None` methods** need a coupled slice (cover `Channel<T>`/`Task<T>`/`Sender<T>` `Type::Apply` value types + the `Closed` err type + the `tasks.channel` producer return-type writeback together) — covering the producer alone leaves the methods unreachable; DEFERRED. After Phase 20 the AST-path residue is ONLY the latent-bug-gated constructs (`is_empty`, no-arg `join()`, `mut self`/`view self` reassignment, recursive structs, view-returning TRAIT methods), the unreachable generic-type methods, the deferred Task/Channel/Sender slice, and the dead bare `?? return`. |
| 21 | Task/Channel/Sender concurrency | ✅ done (c109 Phase 21) — the coupled concurrency slice Phase 20 deferred, landed WHOLE and byte-identical. **Value types**: `Task<T>`/`Channel<T>`/`Sender<T>` (`Type::Apply`) as covered param/local/return value types (`is_covered_concurrency_ty` — renders via `cx.rust_type` to `{root}jet_std::Jet{Task,Channel,Sender}<…>`); `[Task<Unit>]` worker lists (`concurrency_elem_covered` admits `Unit` → `()` as a concurrency element only); the `Closed` err type as a covered fallible payload (`receive`'s `Result<T, Closed>`). **Producer**: `tasks.channel()` (added to `core_call_covered` — a fixed-string `JetChannel::new()` `CoreCall`, NOT in `core_fixed_sig`; its `Channel<T>` return type rides on the binding annotation); `tasks.spawn`'s `Task<T>` result was already total (Phase-13 `CoreClosureCall`). **Methods** (the `recv_type == None` builtin gap, gate shape d3 keyed on name+arity, disjoint from every other shape): `Task.join`(0)/`Task.detach`(0), `Channel.receive`(0)/`Channel.sender`(0), `Sender.send`(1) → new `THandleOp::{TaskJoin,TaskDetach,ChannelReceive,ChannelSender,SenderSend}`, byte-for-byte `emit_builtin_method`'s `Type::Apply`-receiver arms. The result type reads `T` off the LOWERED receiver's `.ty` (totality, I3): `join` → `T`, `detach`/`send` → Unit, `receive` → `Result<T, Closed>` (composes with Phase-8 `?? panic`), `sender` → `Sender<T>`. The channel/sender/task value's prelude methods take `&self`, so a `val`-bound handle stays a plain `let` (`Type::Apply` never matches `emit_let`'s `Type::Named`-keyed `is_file_handle`) — byte-identical. All four concurrency examples route (`32_tasks` spawn/join, `80_detached_task` detach, `33_pipeline` channel send/receive, `34_parallel_scan` parallel scan) and run end-to-end; byte-identical across the whole example suite (118 emittable, 0 differ). 235 top-level fns route. **Surfaced an UNRELATED parity-safe uncovered construct** (`if x == value(binding)` optional-binding condition — keeps `scan_parallel`/`paths_from_args` on the AST path; byte-identical, no miscompile; removing it lets `scan_parallel` route fully). tests/tir.rs adds `task_spawn_join`/`task_detach`/`channel_send_receive`; unit tests add `concurrency_method_names`/`concurrency_value_types_covered`/`covers_concurrency_methods` and flip `core_call_covered("core.tasks","channel")`. **After Phase 21 the AST-path inventory is ONLY latent-bug-gated + sema-unreachable + parity-safe uncovered construct forms — NO coverable type/method surface remains.** |
| 22 | Method-call iteration + optional-binding conditions | ✅ done (c109 Phase 22) — the two parity-safe construct forms the inventory had flagged. **Method-call-collection iteration** (`loop x in <coll>.method(…)`): `emit_for_in`'s four `Expr::MethodCall` branches reproduced via a total `ForIn.method_kind: Option<TForInMethod>` — `chars()` (char iteration, receiver string), `lines()` on `FileReader` (streaming `BufRead::lines(&mut (recv).inner)`), `lines()` on `StdinHandle`/inline `io.stdin()` (streaming + the extra `{ let mut _jet_stdin_h = …; }` block + extra close), and the `.iter().cloned()` default for any other method (`.split(…)`, the whole call as the collection value). The FileReader-vs-stdin split mirrors `expr_jet_ty(receiver)` (via `tir_recv_jet_ty`) + the inline-`stdin` shape, in `emit_for_in`'s order; the iteration var stays `jet_ty: None` (Phase-5 partiality). **Optional-binding `if` condition**: `TStmt::If.cond` became a `TIfCond::{Plain,IfLet,IsNone}` — `x == value(b)`/`ok(b)`/`err(b)` → `if let {pat} = {subj}` (`emit_if_let_pattern`, now `pub(crate)`); `x == null` (`Pattern::Absent`) → `if {subj}.is_none()` — reproducing `emit_if`'s three heads. The if-let then-body uses `fork_panic` (deep-copied panic replica, NON-leaky — the AST clones into a fresh `body_env`); the bound name's inner type comes off the subject's lowered `Option`/`Result` `.ty` (I3). Only the DIRECT `PatternTest` forms (not the `Binary(And,…)` quirk, not Variant/Or/Range) are covered (conservative). `34_parallel_scan` `scan_parallel`/`paths_from_args` + `78_stdin_filter` `main` now route. Byte-identical across the whole example suite (118 emittable, 0 differ) + crafted probes; all run end-to-end. tests/tir.rs adds `method_call_collection_iteration` + `optional_binding_if_condition`; unit tests flip `rejects_method_call_collection_iteration` → `covers_*` (chars + split) and add `covers_optional_binding_if_condition`. **After Phase 22 the "parity-safe coverable construct form" residue is EMPTY** — the AST-path residue is latent-bug-gated + unreachable/dead + uncovered FEATURE surfaces (tuples, default params, distinct types, `#Pure`, `#Todo`, JSON/core-enum matching — each a NEW coverage phase, NOT a flagged form). FileReader `.lines()` is covered+tested but unreachable in the live suite (`49_stream` is `?`-blocked since Phase 10). |
| 23 | `#Pure` / `#Todo` / default params + named args / distinct types / tuples | ✅ done (c109 Phase 23) — five of the six uncovered FEATURE surfaces, all byte-identical. **`#Pure fn`** (S60): purity is sema-only (E3401), erased — lift = delete the `is_pure` gate exclusions; `62_pure` routes. **`#Todo`** (`Expr::Todo`, D-TOOL2): diverging `todo!("#Todo at {file}:{line} — expected {ty}")` (`TExprKind::Todo`, reads sema's total `expected_type`; gate excludes a `None` hole); `58_todo_hole` routes. **Default params** (S61/D-NARG-D2): sema fills omitted trailing args at the CALL SITE (codegen never reads `p.default`) — lift = delete the `p.default.is_some()` exclusions; `66_default_refs` (incl. earlier-param-ref defaults) + `Rect.square` route. **Named args** (D-NARG1): labels are checked documentation that NEVER reorder (D-NARG-D4) and codegen ignores `CallArg.label` — relaxed the `a.label.is_none()` gate checks (parity-safe); `63_named_args` methods route. **Distinct types** (D-DIST1/D-DIST3): `is_covered_distinct_ty` (+ a new `cx.distinct_types` table) admits the `user_<Name>` newtype value type; construction `Name(x)` is the `is_distinct_ctor` fallthrough `Call` (no sig → plain args → `user_<Name>(…)`); `.raw()` → `(recv).0` (`DistinctRaw`, recv_type `None`, keyed on the method name); `#Numeric` `+`/`==` use the native operator (`ast_operand_is_integer` returns `None` for a distinct, so no trap claimed); `69_distinct_types` routes. **Tuples** (S73/D-SG7): `is_covered_tuple_ty`; literal `(x:1,y:2)` → `JetTup_<hash>{…}` reordered to the type's CANONICAL field order (`TupleLit`, gate excludes a `ty: None` literal); field `p.x` → `(p).user_x` (generic `Field`; `struct_field_type` resolves off `Type::Tuple`); destructure `(a,b) @= p.clone()` → `__jet_d{span}` borrow-temp + per-element `(tmp).user_<f>.clone()` (`TStmt::TupleDestructure`, one-Stmt-to-many-lines); `40_tuples` routes. **DEFERRED — the JSON/core-enum + capstone slice**: the prelude `JSON` foreign enum (construction → `jet_std::Json::…`, non-mangled-variant matching via `core_json_pattern_types`/`is_json` branch, the value type, `json.render`/`parse` interplay) is a COUPLED slice (like Phase 21's concurrency) — covering one piece leaves the others unreachable / risks an I2 false-positive; the capstone family (`logbook` `parse`/`build`/`note_*`/`note_score`/`graph_json`, `30_json`/`73_json_coerce`/`74_regex`/`76_http_routes` `main`) is blocked by this slice + other uncovered nodes (regex `Match`, comptime tables, cross-module foreign structs). Byte-identical across the whole example suite (118 emittable, 0 differ); 268 top-level fns route (was 235). tests/tir.rs adds 6 build+run tests; unit tests add 7 gate tests. **After Phase 23 the residue is latent-bug-gated + unreachable/dead + the ONE remaining coupled feature slice (JSON/core-enum + capstone).** |
| N | Delete the AST `emit_func`/`expr_jet_ty`/`operand_is_integer` path; TIR is the only seam | pending — the residue is now (1) latent-bug FIXES (`is_empty`, no-arg `join()`, `mut self`/`view self` reassignment, recursive structs, view-returning trait methods, the `new`-name-collision intercept — separate cards), (2) unreachable/dead constructs (generic-type methods, bare `?? return`, ambient `input()`), and (3) the ONE remaining coverage phase: the **JSON/core-enum matching + capstone foreign-type-heavy** coupled slice (the prelude `JSON` enum value type + construction + `when`/`if` matching + `json.render`/`parse` interplay; regex `Match`; comptime tables; cross-module foreign structs). Phase 23 cleared the other five class-(3) feature surfaces (`#Pure`, `#Todo`, default params/named args, distinct types, tuples). The "parity-safe construct form already flagged as coverable" category is EMPTY. |

Phase ordering may adjust as coverage reveals coupling; the gate keeps the suite green
regardless of order. Each phase: extend `tir_covers`/`lower_func`/`emit_tir_func`, keep
the full suite green (golden = behavioral parity), commit one slice, update this table.

## Verification per phase

- `nix develop -c cargo test -- --test-threads=1` stays fully green.
- Golden examples produce byte-identical Rust (a diff test where practical).
- No new `unsafe` in generated code outside the audited gate (I1).
