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
