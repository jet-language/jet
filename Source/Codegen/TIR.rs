//! TIR — a small, *typed* intermediate representation for codegen (c109 Phase 1).
//!
//! ## Why this exists
//!
//! Today codegen (`emit_func` and friends) re-derives semantic facts while it
//! emits Rust: it calls `expr_jet_ty` to re-infer expression types and
//! `operand_is_integer` to re-decide which operator traps on overflow. That is
//! exactly the "codegen re-derives / falls back" smell that invariant I3 ("codegen
//! is dumb") forbids, and it is the bug class that produced the I2 holes the
//! checked-IR effort (`tools/Tower/docs/sidequests/checked-ir-design.md`) is
//! built to kill.
//!
//! The TIR is the fix. It is a distinct, post-sema representation whose defining
//! property is **TOTALITY**: every fact codegen needs is carried *concretely* on
//! the node — never re-inferred, never an `Option` codegen has to fall back from.
//! Every `TExpr` carries its resolved `Type`; every `Binary` carries its overflow
//! decision as a plain `bool`; every `Let` carries the resolved binding type. The
//! emitter (`emit_tir_func`) makes ZERO decisions: it pattern-matches TIR fields
//! and formats Rust. It never calls `expr_jet_ty` or `operand_is_integer`.
//!
//! ## Phase 1 scope (deliberately tiny)
//!
//! This is the foundational slice. It covers only the *simplest* top-level
//! functions — scalar/String params, arithmetic/logic/comparison, bindings,
//! assignments, returns, `if`, calls to plain functions and `print`. The gate
//! `tir_covers` decides, conservatively, whether a function is fully inside that
//! subset; anything outside stays on the existing AST `emit_func` path, untouched.
//! The two paths must produce byte-identical Rust (golden parity, `tests/golden.rs`),
//! which is how we prove the rest of the compiler is undisturbed.
//!
//! Later phases widen `tir_covers` and add TIR nodes until the AST codegen path
//! is deleted. So the rule for this module is: **add a node only when its
//! construct is in the covered subset, and make every field total.**

use super::*;
use crate::AST::{
    AccessConvention, BinOp, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt, IndexKind,
    LValue, OrFallback, Param, PatSlot, Pattern, Stmt, StrPart, SwitchArm, TryConvert, Type, UnOp,
    VariantPayload,
};
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// TIR types. Every node carries the facts codegen needs, pre-resolved (totality).
// ---------------------------------------------------------------------------

/// A lowered top-level function. `params` are already mangled to their Rust
/// names and carry their resolved Jet `Type`; `ret` is the resolved return type.
pub(crate) struct TFunc {
    /// Jet function name (unmangled) — the emitter mangles via `cx.mangle_name`.
    pub(crate) name: String,
    /// `(mangled rust name, resolved jet type, convention)` per parameter. The
    /// convention is kept so the emitter reproduces the `&`/by-value Rust form
    /// without re-deciding (it mirrors `rust_param_type`).
    pub(crate) params: Vec<(String, Type, AccessConvention)>,
    /// Resolved return type, or `None` for a unit-returning function.
    pub(crate) ret: Option<Type>,
    pub(crate) is_main: bool,
    pub(crate) body: Vec<TStmt>,
    /// c109 Phase 7: how this function is emitted. A top-level function gets
    /// `pub fn name(…)` at module scope; a method gets `pub fn user_name(<self>, …)`
    /// inside an `impl` block (indented), with the `self` receiver form per the
    /// resolved convention (or no receiver for a static method).
    pub(crate) kind: TFuncKind,
}

/// c109 Phase 7: the emission shape of a lowered function.
pub(crate) enum TFuncKind {
    /// A module-level free function — `pub fn name(params) { … }`.
    TopLevel,
    /// An inherent method inside `impl user_<T> { … }`. `self_conv` is the receiver
    /// convention for an instance method (`Read`→`&self`, `Mutate`→`&mut self`,
    /// `Move`→`self`), or `None` for a STATIC (associated) method (no `self`
    /// parameter). The method name is mangled (`user_<name>`) and emitted with `pub`.
    Method { self_conv: Option<AccessConvention> },
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub(crate) enum TStmt {
    /// `let [mut] name[: ty] = init;`. `mutable` reproduces `let mut`; `annotated`
    /// records whether the source binding carried an explicit type annotation, so
    /// the emitted Rust matches the AST path byte-for-byte (an inferred binding
    /// emits no `: ty`). `ty` is always total — inferred once here at lowering if
    /// the source omitted it.
    Let {
        name: String,
        ty: Type,
        annotated: bool,
        mutable: bool,
        init: TExpr,
    },
    /// `place [op]= value;` to a plain local (subset excludes indexed assigns).
    /// `op` is the compound-assignment operator (`+=` etc.) or `None` for `=`.
    Assign {
        /// The Rust *place* string for the local, already resolved (e.g. `user_x`
        /// or `(*user_x)` for a deref'd parameter). Codegen does not re-resolve it.
        place: String,
        op: Option<BinOp>,
        value: TExpr,
    },
    Return(Option<TExpr>),
    /// A call used for effect: `print(x);`, `helper(a);`.
    ExprStmt(TExpr),
    /// Statement-form `if`/`else`. `else_body` is `None` for a bare `if`.
    If {
        cond: TExpr,
        then_body: Vec<TStmt>,
        else_body: Option<Vec<TStmt>>,
    },
    /// `loop { … }` — an infinite loop (`Stmt::Loop`). `label` is the optional
    /// `@name` rendered as `'jet_<name>:` (resolved at lowering, never re-derived).
    Loop {
        label: Option<String>,
        body: Vec<TStmt>,
    },
    /// `loop cond { … }` — the while form (`Stmt::While`). Lowers to Rust `while`.
    While {
        label: Option<String>,
        cond: TExpr,
        body: Vec<TStmt>,
    },
    /// `loop i in start..end [step k]` — a numeric range loop (`ForKind::Range`).
    /// Jet's `..` is inclusive (S22 / D-SG8), so this lowers to `start..=end`,
    /// optionally `.step_by((k) as usize)`. The loop variable `var` is an `Int`
    /// local bound inside the body; its type is resolved here, not in emit.
    Range {
        label: Option<String>,
        var: String,
        start: TExpr,
        end: TExpr,
        step: Option<TExpr>,
        body: Vec<TStmt>,
    },
    /// `break` / `break @name` (label resolved at lowering).
    Break(Option<String>),
    /// `continue` / `continue @name`.
    Continue(Option<String>),
    /// c109 Phase 4: an exhaustive `when`/match on an enum subject (`Stmt::Switch`
    /// whose arms are all variant patterns). Lowers to a Rust `match`, mirroring
    /// `emit_pattern_match_switch` byte-for-byte. `subject` is the already-lowered
    /// subject expression; `clone_subject` reproduces the AST path's `(subj).clone()`
    /// when the subject reads as a borrow (a by-reference enum param), so the match
    /// owns the value. Each arm carries its resolved Rust pattern string and an
    /// optional range-guard string (both fully resolved at lowering — emit makes no
    /// pattern decision). `fallthrough` records whether the AST path appends the
    /// `_ => unreachable!("jet: exhaustiveness bug")` arm (true when there is no
    /// explicit `else`); sema already proved exhaustiveness (E0307), so the dead arm
    /// exists only because rustc cannot see that proof.
    EnumMatch {
        /// The fully-resolved Rust scrutinee string. For a by-reference subject it
        /// is `({rust_name}).clone()` (cloned so the match owns the value); for a
        /// by-value subject it is the subject's emitted form. Resolved at lowering.
        scrutinee: String,
        arms: Vec<TMatchArm>,
        else_body: Option<Vec<TStmt>>,
        fallthrough: bool,
    },
    /// c109 Phase 4: a `when`/match whose arms are all arm-head *range* patterns
    /// (`0..59 -> …`) over a scalar subject, plus a required `else`. The AST path
    /// (`emit_mixed_switch`) lowers this to an `if/else if … else` chain wrapped in
    /// a block that binds `_jet_switch_subject` to a borrow of the subject (the
    /// binding is unused in this form but emitted for parity). Each arm's `(lo, hi)`
    /// becomes `(subj >= lo && subj <= hi)`, reading the subject's resolved place.
    RangeSwitch {
        /// The subject's emitted Rust string, used both for the `_jet_switch_subject`
        /// borrow binding and inside each arm's range condition — exactly as the AST
        /// path re-emits `subject` (resolved once here).
        subject_str: String,
        arms: Vec<(i64, i64, Vec<TStmt>)>,
        else_body: Vec<TStmt>,
    },
    /// c109 Phase 5: indexed assignment `coll[i] = value` (`Stmt::Assign` with an
    /// `LValue::Index`). `is_map` is the resolved `IndexKind` (TOTAL, from sema):
    /// `true` → `jet_map_insert(&mut (base), (i).clone(), v)`; `false` →
    /// `(base)[i as usize] = v`. Both wrap the value in a `{ let __jet_v = …; … }`
    /// block, byte-for-byte the AST `LValue::Index` form. Compound ops (`+=`) on an
    /// index are not a Jet construct here (the parser/sema only admit a plain `=` to
    /// an index lvalue), so no `op` is carried.
    IndexAssign {
        base: TExpr,
        index: TExpr,
        is_map: bool,
        value: TExpr,
    },
    /// c109 Phase 5: collection iteration `loop x in coll` / `loop k, v in map`
    /// (`Stmt::For` with `ForKind::In`). The collection's emitted Rust string is
    /// resolved at lowering. `var2` distinguishes the two-binding map form (which
    /// iterates `(coll).iter()` and clones each key/value) from the single-binding
    /// form (`(coll).iter().cloned()`), reproducing `emit_for_in` exactly. The
    /// subset excludes method-call collections (`.chars()`/`.lines()` → Phase 7),
    /// so only the two plain `.iter()` shapes arise.
    ForIn {
        label: Option<String>,
        var: String,
        var2: Option<String>,
        collection_str: String,
        body: Vec<TStmt>,
    },
}

/// c109 Phase 4: one lowered arm of an exhaustive enum match. `pattern` is the
/// fully-resolved Rust match pattern (`user_Light::user_Red`,
/// `user_Conn::user_Active(user_id) | user_Conn::user_Reconnecting(user_id)`,
/// `user_Http::user_Good(__jet_range_0)`); `guard` is the optional `if …` range
/// guard. Both are computed once at lowering — emit only formats them.
pub(crate) struct TMatchArm {
    pub(crate) pattern: String,
    pub(crate) guard: Option<String>,
    pub(crate) body: Vec<TStmt>,
}

/// A lowered expression: a resolved `Type` plus its kind. `ty` is **total** — it
/// is never absent, and codegen never recomputes it.
pub(crate) struct TExpr {
    pub(crate) ty: Type,
    pub(crate) kind: TExprKind,
}

pub(crate) enum TExprKind {
    /// Integer literal with its D-SG9 width (`None` = default `Int`/i64). The
    /// width is the elaborated `(signed, bits)` sema attached to the AST node.
    IntLit(i64, Option<(bool, u8)>),
    FloatLit(f64),
    BoolLit(bool),
    CharLit(char),
    /// String literal / interpolation. Each part is literal text or an
    /// interpolated TExpr (totally typed, like every other node).
    StrLit(Vec<TStrPart>),
    /// A local or parameter, rendered as its already-resolved Rust *place*
    /// string (handles parameter deref). No env lookup at emit time.
    Local(String),
    /// Call to a plain top-level function. Each arg carries its emit decisions.
    Call { name: String, args: Vec<TCallArg> },
    /// `print(x)` — the one builtin the subset covers.
    Print(Box<TExpr>),
    /// Binary op. `overflow` is the *computed* decision (true → emit the
    /// trapping `jet_add`/… helper). It mirrors today's `operand_is_integer`
    /// logic but is decided here, at lowering, from the total operand types.
    /// `line` is the source line of the operator, resolved at lowering, so the
    /// trapping helper's panic location matches the AST path byte-for-byte (the
    /// emitter never touches `cx.src`).
    Binary {
        op: BinOp,
        overflow: bool,
        line: u32,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    Unary { op: UnOp, operand: Box<TExpr> },
    /// c109 Phase 3: a struct literal `S { f: v, … }`. `rust_type` is the already
    /// resolved Rust type head (`user_S` or `user_S::<…>`); each field carries its
    /// *mangled* Rust name and its value expression. No clone/coercion is applied
    /// at the literal site (mirrors the AST path: a field value is emitted as-is —
    /// the value's own move/clone facts already live in its sub-expression).
    StructLit {
        rust_type: String,
        fields: Vec<(String, TExpr)>,
    },
    /// c109 Phase 3: a struct field *read* `recv.field` in borrow position. The
    /// AST path never derefs/clones a plain field read (Rust reads the place;
    /// owning reads were already rewritten to a `.clone()` MethodCall in sema and
    /// are excluded from the subset). `field_rust` is the mangled field name.
    Field {
        recv: Box<TExpr>,
        field_rust: String,
    },
    /// c109 Phase 4: an enum literal `Enum.Variant`, `Variant(args)`, or a
    /// named-payload `Variant { f: v, … }`. The Rust head (`user_Enum::user_Variant`)
    /// is resolved at lowering. `payload` carries the resolved arg form. The subset
    /// admits only scalar/Char payload values, so no clone/box decision is ever
    /// needed (a scalar arg is never borrowed-in-env, never a boxed edge — the AST
    /// path's `emit_boxed_enum_arg` is a no-op for these), keeping emit decision-free.
    EnumLit {
        prefix: String,
        payload: TEnumPayload,
    },
    /// c109 Phase 5: a list literal `[a, b, c]`. Lowers to Rust `vec![…]`. Each
    /// element is lowered as-is (the AST path applies no clone/coercion at the
    /// literal site — `emit_expr` per element).
    ListLit(Vec<TExpr>),
    /// c109 Phase 5: a map literal `[k: v, …]` or empty `[:]`. The empty form
    /// lowers to `std::collections::BTreeMap::new()` (Rust infers the element
    /// types from the binding context); a non-empty form lowers to the
    /// `{ let mut _m = …; _m.insert((k).clone(), v); … _m }` builder, byte-for-byte
    /// the AST `Expr::MapLit` form.
    MapLit(Vec<(TExpr, TExpr)>),
    /// c109 Phase 5: indexing `coll[i]` (`Expr::Index`). `is_map` is the resolved
    /// `IndexKind` carried TOTALLY from sema (never re-inferred): `true` → the
    /// `jet_index_map` helper, `false` → `jet_index_vec`. `line` is the source line
    /// for the bounds/missing-key panic message, resolved at lowering.
    Index {
        base: Box<TExpr>,
        index: Box<TExpr>,
        is_map: bool,
        line: usize,
    },
    /// c109 Phase 5: an inclusive copy slice `coll[a..b]` (`Expr::Slice`). Lowers
    /// to the `jet_slice_vec` helper. `line` is the source line for the bounds
    /// panic, resolved at lowering.
    Slice {
        base: Box<TExpr>,
        start: Box<TExpr>,
        end: Box<TExpr>,
        line: usize,
    },
    /// c109 Phase 6: the sema-inserted `.clone()` on an owning non-Copy field read
    /// or borrowed value. The AST path emits `(recv).clone()` unconditionally; the
    /// TIR carries the lowered receiver and the result type (the receiver's type).
    Clone(Box<TExpr>),
    /// c109 Phase 6: a user-defined instance method call `recv.method(args)` on a
    /// covered struct/enum. All dispatch facts are resolved at lowering (totality):
    /// `recv` is the lowered receiver (emitted as the AST path emits it — autoref
    /// handles `&self`/`&mut self`/`self`); `method_rust` is the already-resolved
    /// Rust method name (mangled `user_<m>`, or the bare name for a trait-impl
    /// method, decided here from `cx.trait_methods`); each arg carries its
    /// borrow/clone decisions, mirroring `emit_call_args`.
    MethodCall {
        recv: Box<TExpr>,
        method_rust: String,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 7: a STATIC (associated) method call `Type.make(args)`. Resolved
    /// at lowering to `user_<Type>::user_<method>(args)` — `type_prefix` is the
    /// already-resolved Rust type head (`user_<Type>`), `method_rust` the mangled
    /// method name. Mirrors the AST type-name dispatch (Expression.rs ~L1644).
    StaticCall {
        type_prefix: String,
        method_rust: String,
        args: Vec<TCallArg>,
    },
    /// `if`-expression form (S68 / D-SG2). Both arms are value blocks.
    IfExpr {
        cond: Box<TExpr>,
        then_body: Vec<TStmt>,
        then_value: Box<TExpr>,
        else_body: Vec<TStmt>,
        else_value: Box<TExpr>,
    },
    /// c109 Phase 8: `value(x)` — a present optional (`Some(x)`).
    Present(Box<TExpr>),
    /// c109 Phase 8: bare `null` — an absent optional (`None`).
    Absent,
    /// c109 Phase 8: `ok(x)` — a success value of `T ? E` (`Ok(x)`).
    Ok(Box<TExpr>),
    /// c109 Phase 8: `err(e)` — a failure value of `T ? E` (`Err(e)`).
    Err(Box<TExpr>),
    /// c109 Phase 8: the `?` propagation operator (`Expr::Try`). The error
    /// conversion (`convert`) is the TOTAL sema fact (`TryConvert`): a `None` is a
    /// bare propagate, a `Fallible` calls `.to_error()`, a `Typed(fn)` calls the
    /// declared conversion. The frame-trace location (`file`, `line`, `fn_name`) is
    /// resolved at lowering so the emitted `jet_trace_err(…)?` matches the AST path
    /// byte-for-byte (the emitter never reads `cx.current_fn`/`cx.src`).
    Try {
        inner: Box<TExpr>,
        convert: TTryConvert,
        /// Pre-escaped Rust string literal for the source file (`escape_rust_str`).
        file: String,
        line: usize,
        /// Pre-escaped Rust string literal for the enclosing function name.
        fn_name: String,
    },
    /// c109 Phase 8: the `??` fallback operator (`Expr::OrFallback`). `is_option`
    /// is the TOTAL sema fact: `true` → the value is `T?` and lowers to a
    /// `match … { Some(v) => v, None => fb }`; `false` → the value is `T ? E` and
    /// lowers to `match … { Ok(v) => v, Err(_) => fb }`. The fallback is a value or
    /// an early `return` (the panic form is deferred — its `safe_locals_expr`
    /// reproduction is out of subset).
    OrFallback {
        value: Box<TExpr>,
        fallback: TOrFallback,
        is_option: bool,
    },
    /// c109 Phase 8: optional field/chain `base?.member` (`Expr::OptField`). The
    /// `flatten` fact (TOTAL, from sema) picks the combinator: `true` → `.and_then`
    /// (the field is itself optional), `false` → `.map`. Mirrors the AST path's
    /// `(base).clone().{and_then|map}(|__optv| __optv.{member})` exactly.
    OptField {
        base: Box<TExpr>,
        member_rust: String,
        flatten: bool,
    },
}

/// c109 Phase 8: the resolved error-conversion of a `?`, mirroring `AST::TryConvert`
/// (the total sema fact). Carried onto the TIR so the emitter never re-derives it.
pub(crate) enum TTryConvert {
    /// Error types match — bare `jet_trace_err(x, …)?`.
    None,
    /// Source error implements `Fallible` — `.map_err(|e| e.to_error())` (D-ERR2).
    Fallible,
    /// Declared `impl Source -> Target` conversion — `.map_err(<fn>)` (D-ERR-CONV);
    /// holds the mangled Rust conversion-function name.
    Typed(String),
}

/// c109 Phase 8: the resolved right-hand side of a `??` fallback (`AST::OrFallback`),
/// minus the Panic form (deferred). `Value` is an expression; `Return` is an early
/// `return [expr]` from the enclosing function.
pub(crate) enum TOrFallback {
    Value(Box<TExpr>),
    Return(Option<Box<TExpr>>),
}

pub(crate) enum TStrPart {
    Lit(String),
    Interp(TExpr),
}

/// c109 Phase 4: the resolved payload shape of an enum literal.
pub(crate) enum TEnumPayload {
    /// `Enum.Variant` — no payload, emits just the prefix.
    Unit,
    /// `Variant(a, b, …)` — positional payload values, emitted as `prefix(a, b)`.
    Positional(Vec<TExpr>),
    /// `Variant { f: v, … }` — named payload, emitted as `prefix { f: v, … }`.
    /// Each field's Rust name is already mangled at lowering.
    Named(Vec<(String, TExpr)>),
}

/// One lowered call argument, with the borrow/clone decisions already made (so
/// the emitter reproduces `emit_call_args` without consulting `cx.sigs`).
///
/// Emission order mirrors `emit_call_args` exactly: the clone wrapper (`.clone()`
/// or `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`).
pub(crate) struct TCallArg {
    pub(crate) value: TExpr,
    /// Emit `&(...)` around the value (a non-scalar passed by `Read` convention).
    pub(crate) borrow: bool,
    /// Emit `&mut (...)` around the value (a `Mutate`-convention argument). c109
    /// Phase 6: method args may be `Mutate`; the plain-call path never sets this.
    pub(crate) mut_borrow: bool,
    /// Emit `(...).clone()` (an implicit clone — a value passed by `Move`).
    pub(crate) clone: bool,
    /// Emit `Arc::clone(&...)` (a `Shared` value auto-cloned at the call site).
    /// c109 Phase 6: method/Arc args may set this; the plain-call path does not.
    pub(crate) arc_clone: bool,
}

// ---------------------------------------------------------------------------
// The gate: is this function fully inside the Phase-1 subset?
// ---------------------------------------------------------------------------

/// Conservative structural test: `true` only if `f` is a top-level plain
/// function whose entire body is inside the Phase-1 subset. The rule is
/// **exclude on any doubt** — a false negative just keeps the function on the
/// existing AST path (always safe), while a false positive risks an I2 bug. So
/// every check below bails to `false` the moment it sees anything unrecognised.
///
/// `cx` is consulted to exclude functions that reference program-level names the
/// subset does not lower — a comptime `const` (inlined at use) or a bare
/// function-as-value ident. Those use sites need codegen the TIR omits in Phase 1.
pub(crate) fn tir_covers(f: &Func, cx: &Cx) -> bool {
    // Signature shape: no generics, not an unsafe/pure-special function.
    if !f.type_params.is_empty() || f.is_unsafe || f.is_pure {
        return false;
    }
    // A method always has a `self` first parameter; the subset is top-level
    // functions only. (Top-level funcs never have `self`, but check anyway.)
    if f.params.iter().any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // `is_view_return` returns a borrow — outside the subset.
    if f.is_view_return {
        return false;
    }
    // Params must be scalars, String, or a covered struct type, with no defaults.
    for p in &f.params {
        if p.default.is_some() || !is_subset_param_ty(&p.ty, cx) {
            return false;
        }
    }
    // Return type, if present, must be a scalar, String, or a covered struct type.
    if let Some(rt) = &f.return_type {
        if !is_subset_param_ty(rt, cx) {
            return false;
        }
    }
    // Track parameter names so identifier references can be classified: a name
    // that is neither a local/param binding nor a builtin is a program-level
    // reference (const or fn-value), which the subset excludes.
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109 Phase 7: is this method (an inherent method of `type_name`) fully inside
/// the TIR subset? Covers two method classes:
///   - **instance methods** — a `self` first parameter (`self`/`mut self`/`view
///     self`/… via any convention), where `self.field` reads and covered-subset
///     constructs (Phases 1–6) make up the body. The `self` slot lowers to the
///     correct Rust receiver (`&self`/`&mut self`/`self`).
///   - **static methods** — no `self` parameter; an associated function on the
///     type (`Type.make(x) -> Type`). The body + every static call site
///     (`Type.make(x)` → `user_Type::user_make(x)`) are covered.
///
/// The owning `type_name` must itself be a covered struct or enum (so the receiver
/// place + field reads emit exactly as `emit_method` does). The rule is the same
/// **exclude on any doubt**: a false negative just keeps the method on the AST
/// path, a false positive risks a silent miscompile (a wrong `self` receiver).
pub(crate) fn tir_covers_method(f: &Func, type_name: &str, cx: &Cx) -> bool {
    // Signature shape: no generics, not unsafe/pure.
    if !f.type_params.is_empty() || f.is_unsafe || f.is_pure {
        return false;
    }
    // A `view`-returning method returns a borrow whose lowering is subtle — defer.
    if f.is_view_return {
        return false;
    }
    // The owning type must be a covered struct or enum (the receiver place and
    // every `self.field` read then emit exactly as `emit_method` produces them).
    let owner_ty = Type::Named(type_name.to_string());
    if !is_covered_struct_ty(&owner_ty, cx) && !is_covered_enum_ty(&owner_ty, cx) {
        return false;
    }
    // The self parameter (if any) must be the FIRST parameter, per the method
    // calling convention. A `self`-bearing method is an instance method; a method
    // with no `self` is a static (associated) function. Any non-first `self` is
    // malformed (sema rejects it) — exclude defensively.
    if f.params.iter().skip(1).any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // A `mut self` / `view self` reassignment (`self = …`) is a known AST-path I2
    // hole (the self slot does not deref on the LHS); the covered subset never
    // admits an assignment to `self` because `Stmt::Assign` to a `Local` named
    // `self` would emit the buggy form. Guard it: a body that assigns `self` is out.
    // (Field assignment `self.field = v` is already E0003 — not a Jet construct.)
    //
    // Non-self params + the return type must be covered value types (Self resolves
    // to the owning type). Self-typed params carry no default.
    for p in f.params.iter().filter(|p| p.name != Syntax::KW_SELF) {
        if p.default.is_some() || !is_subset_param_ty(&resolve_self_ty(&p.ty, type_name), cx) {
            return false;
        }
    }
    if let Some(rt) = &f.return_type {
        if !is_subset_param_ty(&resolve_self_ty(rt, type_name), cx) {
            return false;
        }
    }
    // `self` is a binding in scope for the body (its field reads + the implicit
    // match subject). Non-self params join it.
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    // The body must be entirely in-subset AND never assign to `self`.
    f.body.iter().all(|s| {
        !stmt_assigns_self(s) && stmt_in_subset(s, cx, &mut locals)
    })
}

/// Resolve a `Self` type reference to the owning concrete type. Other types pass
/// through unchanged. (In current Jet a literal `Self` return rarely type-checks —
/// sema treats `Self` and the concrete name as distinct — but resolving it here
/// keeps the gate total if a future sema unifies them.)
fn resolve_self_ty(ty: &Type, type_name: &str) -> Type {
    match ty {
        Type::Named(n) if n == "Self" => Type::Named(type_name.to_string()),
        _ => ty.clone(),
    }
}

/// True if any statement in this (recursively walked) tree assigns to a local
/// named `self`. Such an assignment is the known AST-path I2 hole for `mut self`
/// (the self slot does not deref on the LHS), so the gate excludes the whole
/// method. Only the constructs the subset already admits need be walked.
fn stmt_assigns_self(s: &Stmt) -> bool {
    match s {
        Stmt::Assign { target: LValue::Local { name, .. }, .. } => name == Syntax::KW_SELF,
        Stmt::Assign { .. } => false,
        Stmt::If(ifs) => {
            ifs.then_body.iter().any(stmt_assigns_self)
                || match &ifs.else_branch {
                    Some(ElseBranch::Else(body)) => body.iter().any(stmt_assigns_self),
                    Some(ElseBranch::ElseIf(next)) => stmt_assigns_self(&Stmt::If((**next).clone())),
                    None => false,
                }
        }
        Stmt::Loop { body, .. } | Stmt::While { body, .. } | Stmt::For { body, .. } => {
            body.iter().any(stmt_assigns_self)
        }
        Stmt::Switch { arms, else_body, .. } => {
            arms.iter().any(|a| a.body.iter().any(stmt_assigns_self))
                || else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_assigns_self))
        }
        _ => false,
    }
}

/// A param/return type the subset allows: scalar (Int/IntN/Float/F32/Bool),
/// Char, String, a covered *plain user struct* (c109 Phase 3), a covered
/// *plain user enum* (c109 Phase 4), a covered collection (Phase 5), or a covered
/// *optional* `T?` / *fallible* `T ? E` (c109 Phase 8). Traits, generics,
/// recursive (boxed) types are still out.
fn is_subset_param_ty(ty: &Type, cx: &Cx) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        || is_covered_fallible_ty(ty, cx)
}

/// c109 Phase 8: `ty` is an optional `T?` (`Type::Option`) or a fallible `T ? E`
/// (`Type::Result`) whose payload(s) are themselves covered *value* types. Both
/// lower through `cx.rust_type` (`Option<…>` / `Result<…, …>`) exactly as the AST
/// path does, so a covered-payload optional/fallible param/return needs no special
/// emit. A nested `T??` (Option of Option) never reaches here — sema rejects it —
/// but the recursion would handle it anyway. A list/map *of* options is still
/// excluded (`collection_elem_covered` does not admit `Option`/`Result`), because
/// element clone/coercion for those is deferred.
fn is_covered_fallible_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Option(inner) => fallible_payload_covered(inner, cx),
        Type::Result { ok, err } => {
            fallible_payload_covered(ok, cx) && fallible_payload_covered(err, cx)
        }
        _ => false,
    }
}

/// An optional/fallible payload (`T` in `T?`, or `ok`/`err` in `T ? E`) the subset
/// can lower: a scalar, Char, String, a covered struct/enum, a covered collection,
/// or sema's default error type `Error` (`Type::Named("Error")`, which `cx.rust_type`
/// lowers to plain `String` — its construction/binding is a String, so no clone/box
/// decision the subset can't make).
fn fallible_payload_covered(ty: &Type, cx: &Cx) -> bool {
    if let Type::Named(n) = ty {
        if n == "Error" {
            return true;
        }
    }
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
}

/// c109 Phase 5: `ty` is a list `[E]` or map `[K, V]` the subset can lower. The
/// element/key/value types must themselves be covered *value* types — scalar,
/// Char, String, a covered struct/enum, or a nested covered collection — so the
/// literal/index/iteration lowerings reproduce the AST path without any clone/box
/// decision the subset can't make from total facts. A `FixedList` (`[E#N]`) is
/// excluded: its element-typed param/return uses `Vec<E>` like a list, but the
/// fixed-size construction/indexing semantics differ enough to defer (Phase 7).
fn is_covered_collection_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::List(inner) => collection_elem_covered(inner, cx),
        Type::Map { key, value } => {
            collection_elem_covered(key, cx) && collection_elem_covered(value, cx)
        }
        _ => false,
    }
}

/// A list/map element, key, or value type the subset can lower: a scalar, Char,
/// String, a covered struct/enum, or a nested covered collection. Anything else
/// (option, trait object, fn, tuple, generic var, foreign type) excludes the
/// owning collection.
fn collection_elem_covered(ty: &Type, cx: &Cx) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
}

/// c109 Phase 4: `ty` is a plain user enum the subset can lower. It must be a
/// bare `Type::Named(E)` that:
///  - is a known enum (`cx.enum_variants` has it), not a struct/trait/foreign/core
///    type (JSON, prelude, imported enums use different Rust heads/spellings);
///  - is NOT generic and has NO boxed (recursive) edge — a `Box<…>` payload needs
///    box/deref handling the subset deliberately avoids (recursive enums → later);
///  - is derivable `Clone` (`cx.cloneable`) — the exhaustive-match lowering clones a
///    by-reference subject (`(subj).clone()`), so the enum must be Clone in Rust;
///  - has every variant payload restricted to scalar/Char fields. A String/struct/
///    list/option payload would need clone/box decisions at the literal site and in
///    pattern bindings (`emit_boxed_enum_arg`, borrowed-payload clone) that the
///    subset cannot reproduce from total facts — exclude the whole enum on any.
fn is_covered_enum_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    enum_is_covered(name, cx)
}

fn enum_is_covered(name: &str, cx: &Cx) -> bool {
    // Foreign/core/JSON enums (different Rust head/variant spelling) are out.
    if cx.foreign_types.contains_key(name)
        || crate::Generics::is_type_var_name(name)
        || is_json_type_name(name)
        || core_enum_or_prelude(name)
    {
        return false;
    }
    let Some(variants) = cx.enum_variants.get(name) else {
        return false;
    };
    // A by-reference subject is cloned in the match lowering — require Clone.
    if !cx.cloneable.contains(name) {
        return false;
    }
    variants.iter().all(|(vname, payload)| {
        // Any boxed (recursive) edge is excluded — payload box/deref handling.
        let payload_tys: Vec<&Type> = match payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(t, _) => {
                if cx.boxed_edges.contains(&(name.to_string(), vname.clone())) {
                    return false;
                }
                vec![t]
            }
            VariantPayload::Named(fs) => {
                for f in fs {
                    let key = format!("{}.{}", vname, f.name);
                    if cx.boxed_edges.contains(&(name.to_string(), key)) {
                        return false;
                    }
                }
                fs.iter().map(|f| &f.ty).collect()
            }
        };
        // Every payload field must be a plain scalar or Char — no String/struct/
        // collection/option payloads (they bring clone/box decisions).
        payload_tys
            .iter()
            .all(|t| t.is_scalar() || matches!(t, Type::Char))
    })
}

/// A name that resolves to a compiler/core/prelude enum or opaque type rather
/// than a plain user enum — those are excluded from the enum subset.
fn core_enum_or_prelude(name: &str) -> bool {
    net_handle_rust_type(name).is_some() || alloc_handle_rust_type(name).is_some()
}

/// c109 Phase 3: `ty` is a plain user struct the subset can lower. It must be a
/// bare `Type::Named(S)` that:
///  - is a known struct (`cx.struct_fields` has it), not an enum/trait/generic;
///  - is NOT a compiler/prelude/foreign/core type (those use different Rust
///    heads and field spellings the subset does not emit);
///  - is NOT generic and has NO boxed (recursive) edge — a `Box<…>` field read
///    needs deref handling the subset deliberately avoids.
/// Field types may themselves be scalars/String/Char or another covered struct
/// (checked recursively, with a visited set to terminate); a non-covered field
/// type (list/map/option/enum/fn/boxed) excludes the owning struct.
fn is_covered_struct_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    struct_is_covered(name, cx, &mut HashSet::new())
}

fn struct_is_covered(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A struct that is a trait/enum or a non-user (foreign/core/prelude) type is
    // out. `cx.struct_fields` only holds user structs declared in this module.
    if cx.trait_names.contains(name)
        || cx.enum_variants.contains_key(name)
        || cx.foreign_types.contains_key(name)
        || net_handle_rust_type(name).is_some()
        || crate::Generics::is_type_var_name(name)
    {
        return false;
    }
    let Some(fields) = cx.struct_fields.get(name) else {
        return false;
    };
    if !seen.insert(name.to_string()) {
        // A cycle means a recursive (boxed) struct — excluded.
        return false;
    }
    let ok = fields.iter().all(|(fname, fty)| {
        // Any boxed (recursive) edge is excluded — field reads would need deref.
        if cx.boxed_edges.contains(&(name.to_string(), fname.clone())) {
            return false;
        }
        field_ty_covered(fty, cx, seen)
    });
    seen.remove(name);
    ok
}

/// A struct *field* type the subset can lower: scalar/String/Char, or another
/// covered struct. Compound/optional/enum/fn field types exclude the struct.
fn field_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    match ty {
        Type::Named(n) => struct_is_covered(n, cx, seen),
        _ => false,
    }
}

/// `locals` is the set of names bound as params/locals so far in this scope.
/// It is threaded so an `Expr::Ident` can be classified: a name that is not a
/// local must not be a const/fn-value (excluded). Bindings extend it in order.
fn stmt_in_subset(s: &Stmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    match s {
        Stmt::Val(b) => {
            // No destructuring patterns, no comptime, no uninit/arena views.
            let ok = b.pattern.is_none()
                && !b.is_comptime
                && !b.uninit
                && !b.arena_view
                && expr_in_subset(&b.init, cx, locals);
            // The binding's name is in scope for subsequent statements.
            locals.insert(b.name.clone());
            ok
        }
        Stmt::Assign { target, value, .. } => match target {
            LValue::Local { .. } => expr_in_subset(value, cx, locals),
            // c109 Phase 5: indexed assignment `coll[i] = v`. The base, index, and
            // value must all be in-subset; the `IndexKind` (List/Map) is carried
            // totally from sema and dispatched at lowering (never re-inferred). An
            // `IndexKind::Unknown` means sema did not resolve it — exclude (the AST
            // path falls back to an env type-inference the TIR must not reproduce).
            LValue::Index { base, index, kind, .. } => {
                !matches!(kind, IndexKind::Unknown)
                    && expr_in_subset(base, cx, locals)
                    && expr_in_subset(index, cx, locals)
                    && expr_in_subset(value, cx, locals)
            }
        },
        Stmt::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        Stmt::Return(None, _) => true,
        Stmt::Expr(e) => expr_in_subset(e, cx, locals),
        Stmt::If(ifs) => if_in_subset(ifs, cx, locals),
        // c109 Phase 2: control-flow loops. Each loop body is its own scope; check
        // it on a clone so a `let` inside the loop doesn't leak past it.
        Stmt::Loop { body, .. } => {
            let mut body_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
        }
        Stmt::While { cond, body, .. } => {
            if !expr_in_subset(cond, cx, locals) {
                return false;
            }
            let mut body_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
        }
        Stmt::For { var, var2, kind, body, .. } => match kind {
            // `loop i in start..end [step k]` — start/end/step must be in-subset
            // integer expressions; the loop var `i` is an Int local in the body.
            // The two-binding `key, value` form is map iteration (a collection),
            // outside this phase.
            ForKind::Range { start, end, step } if var2.is_none() => {
                if !expr_in_subset(start, cx, locals) || !expr_in_subset(end, cx, locals) {
                    return false;
                }
                if let Some(st) = step {
                    if !expr_in_subset(st, cx, locals) {
                        return false;
                    }
                }
                let mut body_locals = locals.clone();
                body_locals.insert(var.clone());
                body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
            }
            // c109 Phase 5: `loop x in coll` / `loop k, v in map` (ForKind::In). The
            // collection must be in-subset AND NOT a method-call receiver — a
            // `.chars()`/`.lines()` collection takes a different `emit_for_in`
            // branch (char iteration, streaming line reads) the subset does not
            // reproduce. The single-binding non-method form and the two-binding map
            // form are the only two shapes covered. The loop var(s) bind in the body
            // scope with an *unresolved* type (matching the AST slot's `jet_ty: None`).
            ForKind::In { collection } => {
                if matches!(collection, Expr::MethodCall { .. }) {
                    return false;
                }
                if !expr_in_subset(collection, cx, locals) {
                    return false;
                }
                let mut body_locals = locals.clone();
                body_locals.insert(var.clone());
                if let Some((v2, _)) = var2 {
                    body_locals.insert(v2.clone());
                }
                body.iter().all(|s| stmt_in_subset(s, cx, &mut body_locals))
            }
            // The Range form with a second binding (`k, v in a..b`) is not a Jet
            // construct; stay on the AST path defensively.
            _ => false,
        },
        // `break`/`continue`, labeled or not, carry no sub-expressions to check.
        // The parser only admits them inside a loop body, so they are always valid
        // where they appear; the label name is reproduced verbatim at lowering.
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => true,
        // c109 Phase 4: a `when`/match (`Stmt::Switch`). Covered only in the two
        // shapes the TIR reproduces exactly — an exhaustive enum match or an
        // all-range-arm scalar switch (see `switch_in_subset`).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => switch_in_subset(subject, arms, else_body, cx, locals),
        // unsafe, region, caps, comptime-if, context — all still out.
        _ => false,
    }
}

fn if_in_subset(ifs: &IfStmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    if !expr_in_subset(&ifs.cond, cx, locals) {
        return false;
    }
    // Each branch scopes its own bindings; check on a clone so a `let` in the
    // `then` arm doesn't leak into the `else` arm's classification.
    let mut then_locals = locals.clone();
    if !ifs.then_body.iter().all(|s| stmt_in_subset(s, cx, &mut then_locals)) {
        return false;
    }
    match &ifs.else_branch {
        None => true,
        Some(ElseBranch::Else(body)) => {
            let mut else_locals = locals.clone();
            body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals))
        }
        Some(ElseBranch::ElseIf(next)) => if_in_subset(next, cx, locals),
    }
}

/// c109 Phase 4: is a `Stmt::Switch` (`when`/match) inside the subset? Covered in
/// exactly the two shapes the TIR reproduces byte-for-byte:
///   (A) **exhaustive enum match** — every arm is a variant pattern over a covered
///       enum subject (`switch_arm_pattern_owned` is Some, none are ranges). Lowers
///       to a Rust `match` (`emit_pattern_match_switch`).
///   (B) **range switch** — every arm is an arm-head range pattern (`0..59 -> …`)
///       over a scalar subject AND an `else` is present. Lowers to an if/else chain
///       (`emit_mixed_switch`).
/// Anything else (mixed comparison/Bool arms, optional/`ok`/`err` patterns, a
/// non-covered subject) stays on the AST path.
fn switch_in_subset(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    locals: &mut HashSet<String>,
) -> bool {
    if arms.is_empty() {
        return false;
    }
    // The subject must itself be in-subset (so it lowers + so `it` never escapes).
    if !expr_in_subset(subject, cx, locals) {
        return false;
    }
    // Shape A: all arms are variant patterns (exhaustive enum match).
    if arms
        .iter()
        .all(|a| arm_variant_pattern(cx, &a.cond, subject).is_some())
    {
        // Subject must be a covered enum (its variants are scalar-payload only).
        let subj_enum = arms.iter().find_map(|a| {
            arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
        });
        let Some(enum_name) = subj_enum else {
            return false;
        };
        if !enum_is_covered(&enum_name, cx) {
            return false;
        }
        for a in arms {
            let pat = arm_variant_pattern(cx, &a.cond, subject).expect("checked above");
            // Each arm's payload bindings extend the body scope; check on a clone.
            let mut body_locals = locals.clone();
            add_pattern_binding_names(&pat, &mut body_locals);
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    // Shape B: all arms are arm-head range patterns over a scalar subject, with an
    // `else`. (Range arms bind nothing.) The subject's type must resolve to an
    // integer/char local so the conditions type-check.
    if else_body.is_some()
        && arms.iter().all(|a| arm_head_range(&a.cond, subject).is_some())
    {
        // The subject must be a plain in-subset scalar place (an Ident local/param)
        // so `_jet_switch_subject`/the conditions read it directly. Anything more
        // complex is excluded (the AST path re-emits the subject per arm).
        if !matches!(subject, Expr::Ident(name, _) if locals.contains(name)) {
            return false;
        }
        for a in arms {
            let mut body_locals = locals.clone();
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    // Shape C (c109 Phase 8): a fallible/optional pattern match — every arm head is
    // an `ok(b)`/`err(b)`/`value(b)`/`null` pattern over the subject. Lowers to a
    // Rust `match` over the subject's `Result`/`Option`, exactly like the enum-match
    // shape but with `Ok(..)`/`Err(..)`/`Some(..)`/`None` patterns. The subject must
    // be in-subset (checked above) and resolve to a `Result`/`Option` — but a covered
    // subject already guarantees that here (its type came from a covered fn/local).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(&a.cond, subject).is_some())
    {
        for a in arms {
            let pat = arm_fallible_pattern(&a.cond, subject).expect("checked above");
            let mut body_locals = locals.clone();
            // `ok(b)`/`err(b)`/`value(b)` bind one name; `null` binds nothing.
            if let Some(b) = fallible_pattern_binding(&pat) {
                body_locals.insert(b);
            }
            if !a
                .body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut body_locals))
            {
                return false;
            }
        }
        if let Some(body) = else_body {
            let mut else_locals = locals.clone();
            if !body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals)) {
                return false;
            }
        }
        return true;
    }
    false
}

/// c109 Phase 8: an arm head that is a fallible/optional pattern test over the
/// subject — `subject == ok(b)` / `err(b)` / `value(b)` / `null`. Returns the
/// `Pattern::{Ok,Err,Present,Absent}`, else `None` (a variant/range/comparison arm).
fn arm_fallible_pattern(cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest { subject: s, pattern, .. } if pattern_subjects_match(s, subject) => {
            match pattern {
                Pattern::Ok { .. }
                | Pattern::Err { .. }
                | Pattern::Present { .. }
                | Pattern::Absent(_) => Some(pattern.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The single name an `ok(b)`/`err(b)`/`value(b)` pattern binds (`null` binds none).
fn fallible_pattern_binding(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Ok { binding, .. }
        | Pattern::Err { binding, .. }
        | Pattern::Present { binding, .. } => Some(binding.clone()),
        _ => None,
    }
}

/// Mirror codegen's `switch_arm_pattern_owned` (Statement.rs): an arm whose head
/// is a variant pattern over `subject`. Returns the `Pattern` (Variant or Or of
/// variants), or `None` for ranges / comparison / Bool arms. The arm head is a
/// `PatternTest` (`c == Active(id)`) or a bare-value `Binary(Eq, subject, Ident)`
/// that names a known variant. Range patterns at arm head deliberately return
/// `None` (they go through the mixed-switch path, shape B).
fn arm_variant_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest { subject: s, pattern, .. } if pattern_subjects_match(s, subject) => {
            if matches!(pattern, Pattern::Range { .. }) {
                return None;
            }
            // The subset covers only variant / or-of-variant patterns (no
            // optional/`ok`/`err` patterns — those are Phase 8).
            if pattern_is_variant_or_orvariant(pattern) {
                Some(pattern.clone())
            } else {
                None
            }
        }
        Expr::Binary(BinOp::Eq, lhs, rhs, _) if pattern_subjects_match(lhs, subject) => {
            if let Expr::Ident(variant, rhs_span) = rhs.as_ref() {
                if cx.variant_owner.contains_key(variant) {
                    return Some(Pattern::Variant {
                        variant: variant.clone(),
                        bindings: Vec::new(),
                        span: *rhs_span,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

/// True for a `Variant` pattern or an `Or` whose every alternative is a `Variant`.
/// Excludes optional/result patterns (Present/Absent/Ok/Err) — out of Phase 4.
fn pattern_is_variant_or_orvariant(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Variant { bindings, .. } => bindings
            .iter()
            // Only plain name-binds, wildcards, and ranges in payload slots are
            // covered (those are the slot kinds the TIR reproduces).
            .all(|s| matches!(s, PatSlot::Bind(_) | PatSlot::Wildcard | PatSlot::Range { .. })),
        Pattern::Or(alts, _) => {
            !alts.is_empty() && alts.iter().all(pattern_is_variant_or_orvariant)
        }
        _ => false,
    }
}

/// The owning enum of a variant (or or-of-variant) pattern, via `cx.variant_owner`.
fn variant_pattern_enum(cx: &Cx, pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { variant, .. } => cx.variant_owner.get(variant).cloned(),
        Pattern::Or(alts, _) => alts.iter().find_map(|a| variant_pattern_enum(cx, a)),
        _ => None,
    }
}

/// An arm-head range pattern (`lo..hi -> …`), as `(lo, hi)`. Mirrors the parser's
/// arm-head range lowering: a `PatternTest` whose pattern is `Pattern::Range`.
fn arm_head_range(cond: &Expr, subject: &Expr) -> Option<(i64, i64)> {
    match cond {
        Expr::PatternTest { subject: s, pattern: Pattern::Range { lo, hi, .. }, .. }
            if pattern_subjects_match(s, subject) =>
        {
            Some((*lo, *hi))
        }
        _ => None,
    }
}

/// Mirror codegen's `pattern_subjects_match` (Statement.rs): an arm subject names
/// the same ident as the switch subject, or is the implicit `it`.
fn pattern_subjects_match(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(na, _), Expr::Ident(nb, _)) => na == nb,
        (Expr::Ident(n, _), _) if n == Syntax::KW_IT => true,
        _ => false,
    }
}

/// Record the names a variant (or or-of-variant) pattern binds, so an arm body's
/// classification sees them as locals. Wildcard/Range slots bind nothing; an Or
/// pattern binds its first alt's names (all alts bind the same names — E0317).
fn add_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<String>) {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            for slot in bindings {
                if let PatSlot::Bind(name) = slot {
                    locals.insert(name.clone());
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                add_pattern_binding_names(first, locals);
            }
        }
        _ => {}
    }
}

fn expr_in_subset(e: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) => true,
        Expr::Str(parts, _) => parts.iter().all(|p| match p {
            StrPart::Lit(_) => true,
            StrPart::Interp(e) => expr_in_subset(e, cx, locals),
        }),
        // An ident must resolve to a local/param. A non-local name is a
        // program-level reference — a comptime `const` (inlined) or a bare
        // function-as-value — whose emission the Phase-1 TIR does not cover.
        Expr::Ident(name, _) => locals.contains(name),
        Expr::Unary(_, inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Binary(_, l, r, _) => {
            expr_in_subset(l, cx, locals) && expr_in_subset(r, cx, locals)
        }
        Expr::Call(c) => {
            // `print` is the one builtin the subset covers (exactly one arg).
            let is_print = c.name == Syntax::BUILTIN_PRINT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // Otherwise the callee must be a known *plain* top-level function:
            // in `cx.sigs`, not a local, and NOT an extern/FFI function or an
            // unqualified module import (those lower to different call forms the
            // subset does not emit).
            let is_plain_fn = !locals.contains(&c.name)
                && cx.sigs.contains_key(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && !cx.unqualified_inline.contains_key(&c.name)
                && !cx.unqualified_file.contains_key(&c.name);
            (is_print || is_plain_fn)
                && c.args.iter().all(|a| {
                    // No labels, no shared-auto-clone (Arc) in the subset.
                    a.label.is_none()
                        && !a.flags.shared_auto_clone
                        && arg_conv_in_subset(a)
                        && expr_in_subset(&a.expr, cx, locals)
                })
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            if !expr_in_subset(cond, cx, locals) {
                return false;
            }
            let mut then_locals = locals.clone();
            if !then_body.iter().all(|s| stmt_in_subset(s, cx, &mut then_locals)) {
                return false;
            }
            if !expr_in_subset(then_value, cx, &then_locals) {
                return false;
            }
            let mut else_locals = locals.clone();
            else_body.iter().all(|s| stmt_in_subset(s, cx, &mut else_locals))
                && expr_in_subset(else_value, cx, &else_locals)
        }
        // c109 Phase 3: a struct literal `S { f: v, … }`. Covered only when `S`
        // is a plain user struct the subset lowers, with no trait coercion or
        // cross-module namespace, and every field value is itself in-subset.
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            // A trait-object coercion (S48) or an imported-namespace struct uses
            // a different Rust head the subset does not emit — exclude.
            if as_trait.is_some() || import_ns.is_some() {
                return false;
            }
            // The named type must be a covered user struct (this also rejects the
            // prelude structs like HttpRequest, whose fields are spelled plainly).
            if !is_covered_struct_ty(&Type::Named(type_name.clone()), cx) {
                return false;
            }
            // Generic struct instantiation is out (the subset has no generics).
            if !type_args.is_empty() {
                return false;
            }
            fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals))
        }
        // c109 Phase 3: a struct field *read*. A non-Copy owning read was already
        // rewritten to a `.clone()` MethodCall by sema (which the subset excludes,
        // via the `MethodCall` arm below being absent — `_ => false`); what reaches
        // here is a borrow-position read. Cover it when the receiver is in-subset.
        // (`receiver.field` where the receiver is a known module/enum path is not a
        // `Field` value read — sema lowers those to other nodes — so a plain
        // in-subset receiver is the struct-value case.)
        Expr::Field(receiver, member, _) => {
            // `.clone` is never a real field; defensively exclude (sema's synthetic
            // clone is a MethodCall, not a Field, but a user `.clone` field read
            // would collide with the clone-emit special-case in the AST path).
            if member == "clone" {
                return false;
            }
            // c109 Phase 4: a *unit* enum literal reaches codegen as a `Field` whose
            // receiver is the enum-name ident (sema only re-types it; it does NOT
            // rewrite the node — only payload literals become `Expr::EnumLit`). The
            // AST path emits `user_<Enum>::user_<variant>` for this case. Cover it
            // when the enum is a covered scalar-payload enum and `member` is one of
            // its (unit) variants. A receiver that is a known local can't also be a
            // covered enum name, so the two branches never collide.
            if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                if !locals.contains(enum_name)
                    && enum_is_covered(enum_name, cx)
                    && cx.variant_owner.get(member).map(String::as_str)
                        == Some(enum_name.as_str())
                {
                    return true;
                }
                // A non-local ident receiver that is NOT a covered enum (a core/json/
                // numeric path, an imported namespace, a module alias) is excluded —
                // those use Rust heads/spellings the subset does not emit.
                if !locals.contains(enum_name) {
                    return false;
                }
            }
            // Otherwise this is a struct field *read* — in-subset iff the receiver is.
            expr_in_subset(receiver, cx, locals)
        }
        // c109 Phase 4: an enum literal `Enum.Variant`/`Variant(args)`/named. Covered
        // only when the named enum is a covered scalar-payload enum and every arg
        // value is itself in-subset (a scalar/Char value — the enum being covered
        // already guarantees the payload *types* are scalar, so no clone/box).
        Expr::EnumLit { type_name, variant, args, .. } => {
            if !enum_is_covered(type_name, cx) {
                return false;
            }
            // Defensive: the variant must belong to this enum (sema guaranteed it).
            if cx.variant_owner.get(variant).map(String::as_str) != Some(type_name.as_str()) {
                return false;
            }
            args.iter().all(|a| match a {
                EnumLitArg::Positional(e) => expr_in_subset(e, cx, locals),
                EnumLitArg::Named { expr, .. } => expr_in_subset(expr, cx, locals),
            })
        }
        // c109 Phase 5: a list literal `[a, b, c]`. Covered when every element is
        // itself in-subset. (An empty `[]` has no elements; sema requires a context
        // type — E0501 — which a covered binding/param/return supplies, so the
        // resulting `vec![]` is type-inferred by Rust from that context.)
        Expr::ListLit(elems, _) => elems.iter().all(|e| expr_in_subset(e, cx, locals)),
        // c109 Phase 5: a map literal `[k: v, …]` / `[:]`. Covered when every key
        // and value is in-subset. The empty `[:]` (no entries) is always covered.
        Expr::MapLit(entries, _) => entries
            .iter()
            .all(|(k, v)| expr_in_subset(k, cx, locals) && expr_in_subset(v, cx, locals)),
        // c109 Phase 5: indexing `coll[i]`. The `IndexKind` must be sema-resolved
        // (not `Unknown`) so the helper dispatch (`jet_index_map`/`jet_index_vec`)
        // is a total fact carried onto the TIR. Base + index must be in-subset.
        Expr::Index { base, index, kind, .. } => {
            !matches!(kind, IndexKind::Unknown)
                && expr_in_subset(base, cx, locals)
                && expr_in_subset(index, cx, locals)
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists only — the AST
        // path's `jet_slice_vec` is list-specific). Base/start/end must be in-subset.
        Expr::Slice { base, start, end, .. } => {
            expr_in_subset(base, cx, locals)
                && expr_in_subset(start, cx, locals)
                && expr_in_subset(end, cx, locals)
        }
        // c109 Phase 6: a method call. Covered in exactly two shapes:
        //   (a) the sema-inserted `.clone()` (an owning non-Copy field read /
        //       borrowed value in owning position) — `(recv).clone()`;
        //   (b) a user-defined instance method on a covered struct/enum type
        //       (`recv_type` is `Some(T)`, `(T, method)` ∈ `method_sigs`, and the
        //       method name is NOT one a core/stdlib/builtin lowering intercepts).
        // Everything else (core/stdlib/collection/string/numeric methods, static
        // calls — whose `recv_type` is `None` — fallible/optional, fan-out, …) stays
        // on the AST path.
        Expr::MethodCall { receiver, method, args, recv_type, .. } => {
            method_call_in_subset(receiver, method, args, recv_type, cx, locals)
        }
        // c109 Phase 8: optional constructors `value(x)` / `null`. Covered when the
        // inner value (if any) is in-subset — they lower to `Some(x)` / `None`.
        Expr::Present(inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Absent(_) => true,
        // c109 Phase 8: fallible constructors `ok(x)` / `err(e)`. Covered when the
        // inner value is in-subset — they lower to `Ok(x)` / `Err(e)`.
        Expr::Ok(inner, _) | Expr::Err(inner, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is a
        // total sema fact (`None`/`Fallible`/`Typed(fn)`), reproduced verbatim. The
        // inner fallible value must itself be in-subset (a user fallible fn call, a
        // local, an `ok`/`err` literal). A core/stdlib fallible call (e.g. `fs.read`)
        // is NOT in-subset (it stays on the AST path — Phase 10), so a `?` on one is
        // excluded automatically.
        Expr::Try(inner, _, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: the `??` fallback operator. `is_option` is total. The value
        // and the fallback must be in-subset. The Panic fallback form is deferred
        // (its `safe_locals_expr` reproduction is out of subset) — only Value and
        // early-`return` fallbacks are covered.
        Expr::OrFallback { value, fallback, .. } => {
            expr_in_subset(value, cx, locals) && orfallback_rhs_in_subset(fallback, cx, locals)
        }
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema). The base must be in-subset; the member read lowers to a plain
        // `.map`/`.and_then` closure access (no further dispatch).
        Expr::OptField { base, .. } => expr_in_subset(base, cx, locals),
        // Everything else (lambdas, fan-out, tuples, deref, …) is out.
        _ => false,
    }
}

/// c109 Phase 8: is a `??` fallback right-hand side in-subset? `Value` and early
/// `return [expr]` are covered; the `panic(…)` form is deferred (it reproduces
/// `emit_panic_stop`/`safe_locals_expr`, which depend on the full Slot env in a way
/// the TIR does not yet model — staying on the AST path is the safe choice).
fn orfallback_rhs_in_subset(fallback: &OrFallback, cx: &Cx, locals: &HashSet<String>) -> bool {
    match fallback {
        OrFallback::Value(e) => expr_in_subset(e, cx, locals),
        OrFallback::Return(None, _) => true,
        OrFallback::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        OrFallback::Panic { .. } => false,
    }
}

/// c109 Phase 6: is this `Expr::MethodCall` inside the subset? Two shapes only:
/// the synthetic `.clone()`, or a user-defined instance method on a covered type.
fn method_call_in_subset(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    // Shape (a): the sema-inserted `.clone()`. It takes no args; the receiver is an
    // owning field read / borrowed value, which must itself be in-subset. The AST
    // path emits `(recv).clone()` unconditionally (no `recv_type` needed) — match it.
    if method == "clone" {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    // Shape (c): a STATIC (associated) method call `Type.make(x)`. Phase 6 deferred
    // this (its `recv_type` is `None`). The AST path emits `user_<T>::user_<method>(…)`
    // when the receiver is a type name in `cx.type_names` (Expression.rs ~L1644). We
    // reproduce exactly that, and only that: the receiver is a bare type-name ident
    // (not a local), the type is a covered struct/enum, the method is a registered
    // user method (in `method_sigs`) that is NOT an enum *variant* (those emit an enum
    // literal, a different lowering) and NOT a builtin/special intercept.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            return static_method_call_in_subset(type_name, method, args, cx, locals);
        }
        return false;
    }
    // Shape (b): a user-defined instance method. The `recv_type` is the TOTAL sema
    // fact; a `None` was handled above (static). Anything else (a fallback-inferred
    // path) the subset must NOT reproduce — but `recv_type == Some` is the total
    // instance-method signal.
    let Some(ty) = recv_type else {
        return false;
    };
    // A name a core/stdlib/builtin/special lowering would intercept *before* the
    // user-method dispatch (`emit_builtin_method`, the `.raw()`/`.snapshot()`/alloc
    // special cases) is excluded — those have bespoke lowering keyed on the method
    // name, not on `method_sigs`, so routing them through the user-method TIR would
    // miscompile. Conservative: exclude any name the AST path special-cases.
    if is_intercepted_method_name(method) {
        return false;
    }
    // The receiver type must be a covered struct or enum (so the receiver place
    // emits exactly as the AST path does, and the method is a plain user method).
    let recv_ty = Type::Named(ty.clone());
    if !is_covered_struct_ty(&recv_ty, cx) && !is_covered_enum_ty(&recv_ty, cx) {
        return false;
    }
    // The method must be a user-defined method on that type (in `method_sigs`).
    let Some(sig) = cx.method_sigs.get(&(ty.clone(), method.to_string())) else {
        return false;
    };
    // The receiver expression must itself be in-subset (a covered local/param/field).
    if !expr_in_subset(receiver, cx, locals) {
        return false;
    }
    // Arity must match the resolved signature (sema guaranteed it, but be defensive).
    if args.len() != sig.len() {
        return false;
    }
    // Every argument must be in-subset and carry no call-site label. Unlike a plain
    // call, a method arg MAY use any of `Read`/`Move`/`Mutate` with implicit/Arc
    // clone — those are carried as total flags and emitted verbatim (mirroring
    // `emit_call_args`). The arg *type* is restricted to what the emitter can wrap:
    // a covered value type; a Fn-typed param would need the Box-coercion form, so
    // exclude any method whose param at this position is a function type.
    args.iter().zip(sig.iter()).all(|(a, (_, pty))| {
        a.label.is_none()
            && !matches!(pty, Type::Fn { .. })
            && expr_in_subset(&a.expr, cx, locals)
    })
}

/// c109 Phase 7: is a STATIC method call `Type.make(args)` inside the subset? The
/// AST path (Expression.rs ~L1644) emits `user_<Type>::user_<method>(args)` for a
/// `MethodCall` whose receiver is an ident in `cx.type_names`. We admit exactly that
/// case, conservatively:
///   - `type_name` is NOT a local (a local shadowing a type would be a field/method
///     access, not a static call);
///   - `type_name` is a covered struct or enum (so its `user_<T>` prefix is right);
///   - `method` is NOT an enum *variant* of `type_name` — a `Enum.Variant(args)`
///     receiver+method emits an enum literal (a different lowering, Expression.rs
///     ~L1635), so exclude it (Phase 4 covers enum literals via `Expr::EnumLit`/
///     unit `Expr::Field`, not this MethodCall shape);
///   - `method` is NOT a builtin/special intercept (`new`, etc.);
///   - `(type_name, method)` is a registered user method (`method_sigs`);
///   - every arg is in-subset, unlabeled, and not Fn-typed.
fn static_method_call_in_subset(
    type_name: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if locals.contains(type_name) {
        return false;
    }
    if is_intercepted_method_name(method) {
        return false;
    }
    let ty = Type::Named(type_name.to_string());
    if !is_covered_struct_ty(&ty, cx) && !is_covered_enum_ty(&ty, cx) {
        return false;
    }
    // An enum-name receiver whose `method` names a variant emits an enum literal,
    // not a static call — exclude (it never reaches `method_sigs` on the AST path).
    if let Some(variants) = cx.enum_variants.get(type_name) {
        if variants.iter().any(|(v, _)| v == method) {
            return false;
        }
    }
    let Some(sig) = cx.method_sigs.get(&(type_name.to_string(), method.to_string())) else {
        return false;
    };
    if args.len() != sig.len() {
        return false;
    }
    args.iter().zip(sig.iter()).all(|(a, (_, pty))| {
        a.label.is_none()
            && !matches!(pty, Type::Fn { .. })
            && expr_in_subset(&a.expr, cx, locals)
    })
}

/// Method names a core/stdlib/builtin/special lowering intercepts *before* the
/// user-method dispatch (`emit_method_call` → `emit_builtin_method` and the
/// `.raw()`/`.snapshot()`/`mem.*.new` special cases, Source/Codegen/Expression.rs).
/// A user method sharing one of these names is emitted by that bespoke lowering on
/// the AST path, not by `method_sigs`, so the TIR must NOT claim it — exclude.
/// The list is intentionally a superset (every name those paths mention, guarded
/// or not): an extra exclusion only keeps a function on the AST path (always safe).
fn is_intercepted_method_name(method: &str) -> bool {
    matches!(
        method,
        // Special-cased in `emit_method_call` / `emit_expr` (clone is the synthetic
        // path, handled separately above; raw/snapshot/new have bespoke lowering).
        "clone" | "raw" | "snapshot" | "new"
        // String / list / map / collection builtins (`emit_builtin_method`).
        | "parse" | "from_bytes" | "len" | "is_empty" | "push" | "pop" | "insert"
        | "remove" | "get" | "post" | "put" | "delete" | "first" | "last"
        | "contains" | "index_of" | "reverse" | "sort" | "join" | "detach"
        | "receive" | "sender" | "send" | "clear" | "chars" | "bytes" | "trim"
        | "split" | "starts_with" | "ends_with" | "replace" | "to_upper"
        | "to_lower" | "repeat" | "slice" | "keys" | "values" | "contains_key"
        | "to_string" | "map" | "filter" | "each" | "find" | "any" | "all"
        | "sort_by" | "reduce"
        // Numeric predicates / bit ops / width conversions (D-NUMOPS1).
        | "is_nan" | "is_infinite" | "is_finite"
        | "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros"
        | "to_i8" | "to_i16" | "to_i32" | "to_i64" | "to_int" | "to_u8" | "to_u16"
        | "to_u32" | "to_u64" | "to_f32" | "to_f64" | "to_float"
        // Stopwatch / file / stdin / net / http / regex / alloc handle methods.
        | "elapsed_millis" | "write_line" | "flush" | "read_line" | "lines"
        | "alloc" | "reset" | "free" | "accept" | "local_addr" | "read" | "write"
        | "peer_addr" | "close" | "method" | "path" | "body" | "header" | "param"
        | "status" | "group"
    )
}

/// A call argument is in-subset only if its convention is one the emitter
/// reproduces: a `Read` borrow or a by-`Move` value (with an optional implicit
/// clone). `Mutate` args would need `&mut place` handling we don't yet emit.
fn arg_conv_in_subset(a: &crate::AST::CallArg) -> bool {
    !matches!(a.convention, AccessConvention::Mutate)
}

// ---------------------------------------------------------------------------
// Lowering: AST -> TIR. This is where every fact is resolved ONCE.
// ---------------------------------------------------------------------------

/// Per-function lowering environment: a local name -> (Rust place string, type).
/// Built from params, extended by `let` bindings. The "place" already accounts
/// for parameter deref, so `Local` emission needs no further resolution.
///
/// The type is `Option<Type>`: a binding can carry a *resolved* type, or `None`
/// when the AST path's slot had `jet_ty: None` and we must reproduce that
/// partiality. The load-bearing case (c109 Phase 5) is a `loop x in coll`
/// iteration variable: `emit_for_in` binds its slot with `jet_ty: None`, so
/// `operand_is_integer`/`expr_jet_ty` resolve the var to `None` and it never
/// enables the overflow trap. Carrying `Some(elem_ty)` here would diverge —
/// `x + 1` would wrongly trap. So the iteration var is stored as `None`,
/// matching the AST path bit-for-bit (the Phase-3 "reproduce the AST's
/// partiality where it is load-bearing" lesson, again).
struct LowerEnv {
    locals: HashMap<String, (String, Option<Type>)>,
    /// c109 Phase 8: the enclosing function's unmangled Jet name, used by a `?`
    /// (`TExprKind::Try`) to embed the trace-frame function name — exactly the value
    /// the AST path reads from `cx.current_fn` at emit time (set to `f.name`).
    fn_name: String,
}

impl LowerEnv {
    fn place_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
    fn ty_of(&self, name: &str) -> Option<Type> {
        self.locals.get(name).and_then(|(_, t)| t.clone())
    }
    /// c109 Phase 4: a name reads as a borrow when its resolved place is a deref
    /// (`(*name)`) — a by-reference parameter slot. The match lowering clones such
    /// a subject so the `match` owns the value, mirroring `emit_pattern_match_switch`.
    fn is_borrowed(&self, name: &str) -> bool {
        matches!(self.locals.get(name), Some((place, _)) if place.starts_with("(*"))
    }
    /// The bare Rust binding name (without the deref wrapper), e.g. `user_light`
    /// for a slot whose place is `(*user_light)`. Used by the match-subject clone,
    /// which clones the borrow itself (`(user_light).clone()`), not `(*user_light)`.
    fn rust_name_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) if place.starts_with("(*") && place.ends_with(')') => {
                place[2..place.len() - 1].to_string()
            }
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    let mut env = LowerEnv {
        locals: HashMap::new(),
        fn_name: f.name.clone(),
    };
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        let place = param_place(&rust_name, p);
        env.locals
            .insert(p.name.clone(), (place, Some(p.ty.clone())));
        params.push((rust_name, p.ty.clone(), p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.clone(),
        is_main: f.name == "main",
        body,
        kind: TFuncKind::TopLevel,
    }
}

/// c109 Phase 7: lower an inherent method (instance or static) of `type_name` to a
/// `TFunc`. Mirrors `emit_method`'s slot construction exactly:
///  - the `self` parameter (if any) becomes a slot whose place is the bare `self`
///    (rust_name `self`, NO deref — `self.field` reads emit `(self).field`, and a
///    `when self` match scrutinee emits `self` with no clone, exactly as the AST
///    path does for a `&self`/`&mut self`/`self` receiver) and whose type is `None`
///    (matching `emit_method`'s `jet_ty: None` so overflow decisions are identical);
///  - non-self params get the same `param_place` deref logic as a free function.
/// The `self_conv` (instance) / `None` (static) and the resolved return type drive
/// the receiver/signature in `emit_tir_func`.
pub(crate) fn lower_method(f: &Func, type_name: &str, cx: &Cx) -> TFunc {
    let mut env = LowerEnv {
        locals: HashMap::new(),
        fn_name: f.name.clone(),
    };
    let mut params = Vec::new();
    let mut self_conv: Option<AccessConvention> = None;
    let mut is_static = true;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            // The self slot: place `self`, no deref, type None (parity with emit_method).
            env.locals
                .insert(Syntax::KW_SELF.to_string(), ("self".to_string(), None));
            self_conv = Some(p.convention);
            is_static = false;
            continue;
        }
        let rust_name = mangle(&p.name);
        let place = param_place(&rust_name, p);
        // A `Self`-typed param resolves to the owning type for totality.
        let pty = resolve_self_ty(&p.ty, type_name);
        env.locals.insert(p.name.clone(), (place, Some(pty.clone())));
        params.push((rust_name, pty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    // An instance method carries `Some(conv)`; a static method carries `None`.
    let kind = TFuncKind::Method {
        self_conv: if is_static { None } else { self_conv },
    };
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.as_ref().map(|t| resolve_self_ty(t, type_name)),
        is_main: false,
        body,
        kind,
    }
}

/// The Rust place a parameter reads as, mirroring `emit_func`'s `deref` logic:
/// a `Read` parameter of non-scalar type (String/Char) is a `&T` and must be
/// dereferenced; `Mutate` is `&mut T` (deref'd); `Move`/scalar-`Read` is by value.
fn param_place(rust_name: &str, p: &Param) -> String {
    let deref = match p.convention {
        AccessConvention::Read if p.ty.is_scalar() => false,
        AccessConvention::Read => true,
        AccessConvention::Mutate => true,
        AccessConvention::Move => false,
    };
    if deref {
        format!("(*{})", rust_name)
    } else {
        rust_name.to_string()
    }
}

fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    stmts.iter().map(|s| lower_stmt(s, cx, env)).collect()
}

fn lower_stmt(s: &Stmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match s {
        Stmt::Val(b) => {
            let init = lower_expr(&b.init, cx, env);
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            let annotated = b.ty.is_some();
            let ty = b.ty.clone().unwrap_or_else(|| init.ty.clone());
            env.locals
                .insert(b.name.clone(), (mangle(&b.name), Some(ty.clone())));
            TStmt::Let {
                name: b.name.clone(),
                ty,
                annotated,
                mutable: b.mutable,
                init,
            }
        }
        Stmt::Assign {
            target, op, value, ..
        } => match target {
            LValue::Local { name, .. } => TStmt::Assign {
                place: env.place_of(name),
                op: *op,
                value: lower_expr(value, cx, env),
            },
            // c109 Phase 5: `coll[i] = v`. The `IndexKind` is resolved by sema; carry
            // it as the total `is_map` fact (the gate excluded `Unknown`). No compound
            // op on an index lvalue (parser admits only `=`).
            LValue::Index { base, index, kind, .. } => {
                let base_t = lower_expr(base, cx, env);
                let index_t = lower_expr(index, cx, env);
                let value_t = lower_expr(value, cx, env);
                TStmt::IndexAssign {
                    base: base_t,
                    index: index_t,
                    is_map: matches!(kind, IndexKind::Map),
                    value: value_t,
                }
            }
        },
        Stmt::Return(Some(e), _) => TStmt::Return(Some(lower_expr(e, cx, env))),
        Stmt::Return(None, _) => TStmt::Return(None),
        Stmt::Expr(e) => TStmt::ExprStmt(lower_expr(e, cx, env)),
        Stmt::If(ifs) => lower_if(ifs, cx, env),
        // c109 Phase 2: control-flow loops. Loop bodies are their own scope —
        // lower on a cloned env so bindings inside don't leak out.
        Stmt::Loop { body, label, .. } => {
            let mut branch = clone_env(env);
            TStmt::Loop {
                label: label_name(label),
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::While { cond, body, label, .. } => {
            let cond = lower_expr(cond, cx, env);
            let mut branch = clone_env(env);
            TStmt::While {
                label: label_name(label),
                cond,
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::For { var, var2, kind, body, label, .. } => match kind {
            ForKind::Range { start, end, step } => {
                let start = lower_expr(start, cx, env);
                let end = lower_expr(end, cx, env);
                let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                // The loop var is an `Int` local for the body's scope only.
                let mut branch = clone_env(env);
                branch
                    .locals
                    .insert(var.clone(), (mangle(var), Some(Type::Int)));
                TStmt::Range {
                    label: label_name(label),
                    var: var.clone(),
                    start,
                    end,
                    step,
                    body: lower_stmts(body, cx, &mut branch),
                }
            }
            // c109 Phase 5: collection iteration `loop x in coll` / `loop k, v in map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection } => {
                let collection_str = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                let mut branch = clone_env(env);
                branch
                    .locals
                    .insert(var.clone(), (mangle(var), None));
                if let Some((v2, _)) = var2 {
                    branch
                        .locals
                        .insert(v2.clone(), (mangle(v2), None));
                }
                TStmt::ForIn {
                    label: label_name(label),
                    var: var.clone(),
                    var2: var2.as_ref().map(|(n, _)| n.clone()),
                    collection_str,
                    body: lower_stmts(body, cx, &mut branch),
                }
            }
        },
        Stmt::Break(_) => TStmt::Break(None),
        Stmt::Continue(_) => TStmt::Continue(None),
        Stmt::BreakLabel(name, _) => TStmt::Break(Some(name.clone())),
        Stmt::ContinueLabel(name, _) => TStmt::Continue(Some(name.clone())),
        // c109 Phase 4: a `when`/match. The gate already classified it as either an
        // exhaustive enum match (shape A) or an all-range scalar switch (shape B).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => lower_switch(subject, arms, else_body, cx, env),
        _ => unreachable!("statement not in TIR subset"),
    }
}

/// Pull the bare label name out of an `@name` loop label, dropping the span. The
/// emitter renders it as `'jet_<name>:` (mirroring `loop_label_prefix`).
fn label_name(label: &Option<(String, Span)>) -> Option<String> {
    label.as_ref().map(|(n, _)| n.clone())
}

fn lower_if(ifs: &IfStmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    let cond = lower_expr(&ifs.cond, cx, env);
    // Each branch gets its own scope; bindings inside must not leak. Clone the
    // env so a `let` in the `then` arm is not visible after the `if`.
    let then_body = {
        let mut branch = clone_env(env);
        lower_stmts(&ifs.then_body, cx, &mut branch)
    };
    let else_body = match &ifs.else_branch {
        None => None,
        Some(ElseBranch::Else(body)) => {
            let mut branch = clone_env(env);
            Some(lower_stmts(body, cx, &mut branch))
        }
        // `else if` nests as an else-body holding a single `If`.
        Some(ElseBranch::ElseIf(next)) => {
            let mut branch = clone_env(env);
            Some(vec![lower_if(next, cx, &mut branch)])
        }
    };
    TStmt::If {
        cond,
        then_body,
        else_body,
    }
}

/// c109 Phase 4: lower a `when`/match. The gate (`switch_in_subset`) has already
/// proved one of the two covered shapes; pick the matching lowering.
fn lower_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // Shape B: all arm-head ranges + else → if/else chain (`emit_mixed_switch`).
    if else_body.is_some() && arms.iter().all(|a| arm_head_range(&a.cond, subject).is_some()) {
        return lower_range_switch(subject, arms, else_body, cx, env);
    }
    // Shape C (c109 Phase 8): all arms are fallible/optional patterns → a Rust match
    // over the subject's Result/Option (`Ok(..)`/`Err(..)`/`Some(..)`/`None`).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(&a.cond, subject).is_some())
    {
        return lower_fallible_match(subject, arms, else_body, cx, env);
    }
    // Shape A: exhaustive enum match (`emit_pattern_match_switch`).
    lower_enum_match(subject, arms, else_body, cx, env)
}

/// c109 Phase 8: lower a fallible/optional pattern match (`when … { it == ok(n) ->
/// … }`). Reuses the `EnumMatch` TStmt — the scrutinee is the subject's emitted form
/// (a covered fallible/optional value: a user fallible fn call, an optional local,
/// etc.; no by-reference clone arises since those subjects are not deref'd enum
/// params), and each arm's pattern is the Rust `Ok(b)`/`Err(b)`/`Some(b)`/`None`,
/// mirroring `emit_match_pattern`. Binding payload types come from the subject's
/// resolved Result/Option type (totality), reproducing `add_pattern_bindings`.
fn lower_fallible_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's resolved type carries the ok/err/present payload types. Lower the
    // subject once to get both its emitted string and its total type.
    let subject_t = lower_expr(subject, cx, env);
    let subject_ty = subject_t.ty.clone();
    // A by-reference enum param is cloned in the enum-match path; a fallible/optional
    // subject in-subset is never a deref'd slot (it is a fn-call value or an owned
    // local), so the scrutinee is the plain emitted form — matching the AST path,
    // whose `subj` clone branch only fires for a deref'd `Ident`.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&subject_t, cx),
    };
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern = arm_fallible_pattern(&arm.cond, subject).expect("gate proved fallible arm");
        let pat = tir_fallible_pattern(&pattern);
        let mut body_env = clone_env(env);
        tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty);
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard: None, body });
    }
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path (`emit_pattern_match_switch`) appends
    // `_ => unreachable!(…)` so rustc sees a complete match (sema proved E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

/// c109 Phase 8: the Rust match pattern for a fallible/optional pattern, mirroring
/// `emit_match_pattern`'s Ok/Err/Present/Absent arms (Statement.rs).
fn tir_fallible_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        _ => unreachable!("non-fallible pattern in fallible match (gate)"),
    }
}

/// c109 Phase 8: bind the ok/err/present payload to its resolved type, read from the
/// subject's Result/Option type. Mirrors `add_pattern_bindings`'s Ok/Err/Present
/// arms (the binding's `jet_ty` is the inner type so any arithmetic on it traps
/// exactly as the AST path; `null` binds nothing).
fn tir_add_fallible_binding(pattern: &Pattern, env: &mut LowerEnv, subject_ty: &Type) {
    let (binding, ty) = match (pattern, subject_ty) {
        (Pattern::Ok { binding, .. }, Type::Result { ok, .. }) => {
            (binding.clone(), Some((**ok).clone()))
        }
        (Pattern::Err { binding, .. }, Type::Result { err, .. }) => {
            (binding.clone(), Some((**err).clone()))
        }
        (Pattern::Present { binding, .. }, Type::Option(inner)) => {
            (binding.clone(), Some((**inner).clone()))
        }
        // The subject type didn't resolve to the expected shape (impossible for a
        // covered subject — sema validated it); bind with no type (matches the AST
        // path's `jet_ty: None` fallback).
        (Pattern::Ok { binding, .. }, _)
        | (Pattern::Err { binding, .. }, _)
        | (Pattern::Present { binding, .. }, _) => (binding.clone(), None),
        // `null` (Absent) binds nothing.
        _ => return,
    };
    env.locals.insert(binding.clone(), (mangle(&binding), ty));
}

fn lower_enum_match(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The match owns the value. Mirror `emit_pattern_match_switch`: a by-reference
    // subject (a deref'd enum param) is cloned as `({rust_name}).clone()` — the
    // borrow itself is cloned, NOT the deref'd place. Any other subject emits its
    // plain form.
    let scrutinee = match subject {
        Expr::Ident(name, _) if env.is_borrowed(name) => {
            format!("({}).clone()", env.rust_name_of(name))
        }
        _ => emit_tir_expr(&lower_expr(subject, cx, env), cx),
    };
    // Resolve the owning enum once — drives the Rust variant prefix in patterns.
    let enum_type = arms.iter().find_map(|a| {
        arm_variant_pattern(cx, &a.cond, subject).and_then(|p| variant_pattern_enum(cx, &p))
    });
    // The subject's resolved Jet type carries the variant binding payload types.
    let subject_ty = expr_ast_jet_ty(subject, env);
    let mut tarms = Vec::new();
    for arm in arms {
        let pattern =
            arm_variant_pattern(cx, &arm.cond, subject).expect("gate proved variant arm");
        let pat = tir_match_pattern(cx, &pattern, enum_type.as_deref());
        let guard = tir_range_guard(&pattern);
        // The arm body sees the variant's payload bindings, typed from the layout.
        let mut body_env = clone_env(env);
        tir_add_pattern_bindings(cx, &pattern, &mut body_env, subject_ty.as_ref());
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard, body });
    }
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    // No explicit `else` → the AST path appends `_ => unreachable!(…)` so rustc
    // sees a complete match (sema already proved exhaustiveness — E0307).
    let fallthrough = else_body.is_none();
    TStmt::EnumMatch {
        scrutinee,
        arms: tarms,
        else_body: else_lowered,
        fallthrough,
    }
}

fn lower_range_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's emitted string — used for the borrow binding and each range
    // condition, exactly as `emit_mixed_switch` re-emits the subject.
    let subject_str = emit_tir_expr(&lower_expr(subject, cx, env), cx);
    let mut tarms = Vec::new();
    for arm in arms {
        let (lo, hi) = arm_head_range(&arm.cond, subject).expect("gate proved range arm");
        let mut branch = clone_env(env);
        let body = lower_stmts(&arm.body, cx, &mut branch);
        tarms.push((lo, hi, body));
    }
    let else_lowered = {
        let body = else_body.as_ref().expect("range switch requires else (gate)");
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    };
    TStmt::RangeSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
}

/// TIR-local reproduction of codegen's `emit_match_pattern` (Statement.rs) for the
/// user-enum case the subset covers. Builds the Rust match pattern string from the
/// resolved enum type and variant slots — pure formatting, no type inference. The
/// subset excludes JSON/foreign enums, so this only handles `user_<Enum>::user_<V>`.
fn tir_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    let resolved = enum_type
        .map(|t| t.to_string())
        .or_else(|| variant_pattern_enum(cx, pattern));
    let prefix = resolved
        .as_deref()
        .map(|t| format!("user_{}", t))
        .unwrap_or_else(|| "user_TYPE".to_string());
    match pattern {
        Pattern::Variant { variant, bindings, .. } => {
            if bindings.is_empty() {
                format!("{}::{}", prefix, mangle(variant))
            } else {
                let slot_pats: Vec<String> = bindings
                    .iter()
                    .enumerate()
                    .map(|(i, s)| match s {
                        PatSlot::Bind(n) => mangle(n),
                        PatSlot::Wildcard => "_".to_string(),
                        PatSlot::Range { .. } => format!("__jet_range_{}", i),
                    })
                    .collect();
                if slot_pats.len() == 1 {
                    format!("{}::{}({})", prefix, mangle(variant), slot_pats[0])
                } else {
                    let fields: Vec<String> = slot_pats
                        .iter()
                        .enumerate()
                        .map(|(i, p)| format!("f{i}: {p}"))
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, mangle(variant), fields.join(", "))
                }
            }
        }
        Pattern::Or(alts, _) => {
            let pats: Vec<String> = alts
                .iter()
                .map(|a| tir_match_pattern(cx, a, resolved.as_deref()))
                .collect();
            pats.join(" | ")
        }
        // The gate admits only variant / or-of-variant patterns into shape A.
        _ => unreachable!("non-variant pattern in enum match (gate)"),
    }
}

/// TIR-local reproduction of codegen's `emit_range_guard` (Statement.rs): a payload
/// range slot becomes `__jet_range_i >= lo && __jet_range_i <= hi`. `None` when no
/// slot is a range. Or-patterns reuse the first alt's ranges (all alts bind alike).
fn tir_range_guard(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { bindings, .. } => {
            let guards: Vec<String> = bindings
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    if let PatSlot::Range { lo, hi } = s {
                        Some(format!(
                            "__jet_range_{} >= {} && __jet_range_{} <= {}",
                            i, lo, i, hi
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            if guards.is_empty() {
                None
            } else {
                Some(guards.join(" && "))
            }
        }
        Pattern::Or(alts, _) => alts.first().and_then(tir_range_guard),
        _ => None,
    }
}

/// TIR-local reproduction of codegen's `add_pattern_bindings`/`variant_binding_types`
/// for the user-enum case: bind each `Bind` slot to its payload field type, read
/// from the resolved enum layout (`cx.enum_variants`). Wildcard/Range slots bind
/// nothing. Or-patterns bind the first alt's names (all alts bind alike — E0317).
fn tir_add_pattern_bindings(
    cx: &Cx,
    pattern: &Pattern,
    env: &mut LowerEnv,
    _subject_ty: Option<&Type>,
) {
    match pattern {
        Pattern::Variant { variant, bindings, .. } => {
            let tys = variant_payload_types(cx, variant);
            for (i, slot) in bindings.iter().enumerate() {
                if let PatSlot::Bind(b) = slot {
                    // Payload types are scalar/Char (the enum is covered), so the
                    // binding is a by-value local; default to Int if unresolved
                    // (impossible for a covered enum — sema validated the access).
                    let ty = tys
                        .as_ref()
                        .and_then(|ts| ts.get(i).cloned())
                        .unwrap_or(Type::Int);
                    env.locals.insert(b.clone(), (mangle(b), Some(ty)));
                }
            }
        }
        Pattern::Or(alts, _) => {
            if let Some(first) = alts.first() {
                tir_add_pattern_bindings(cx, first, env, _subject_ty);
            }
        }
        _ => {}
    }
}

/// The payload field types a variant binds, from the resolved enum layout. Mirrors
/// `variant_binding_types` (Statement.rs) for user enums (JSON enums are excluded
/// from the subset).
fn variant_payload_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    let owner = cx.variant_owner.get(variant)?;
    let variants = cx.enum_variants.get(owner)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Unit => Some(Vec::new()),
        VariantPayload::Single(t, _) => Some(vec![t.clone()]),
        VariantPayload::Named(fields) => Some(fields.iter().map(|f| f.ty.clone()).collect()),
    }
}

/// Resolve the subject's Jet type for binding payloads, mirroring `expr_jet_ty`'s
/// reach (only an Ident resolves via its slot). Enough for the covered subset (the
/// subject is an enum-typed local/param). Other forms resolve to `None` (the
/// payload types come from `cx.enum_variants` regardless).
fn expr_ast_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        _ => None,
    }
}

fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
    }
}

fn lower_expr(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    match e {
        Expr::Int(n, _, width) => TExpr {
            ty: int_lit_type(width),
            kind: TExprKind::IntLit(*n, *width),
        },
        Expr::Float(v, _) => TExpr {
            ty: Type::Float,
            kind: TExprKind::FloatLit(*v),
        },
        Expr::Bool(b, _) => TExpr {
            ty: Type::Bool,
            kind: TExprKind::BoolLit(*b),
        },
        Expr::Char(c, _) => TExpr {
            ty: Type::Char,
            kind: TExprKind::CharLit(*c),
        },
        Expr::Str(parts, _) => {
            let tparts = parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => TStrPart::Lit(s.clone()),
                    StrPart::Interp(e) => TStrPart::Interp(lower_expr(e, cx, env)),
                })
                .collect();
            TExpr {
                ty: Type::String,
                kind: TExprKind::StrLit(tparts),
            }
        }
        Expr::Ident(name, _) => {
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            TExpr {
                ty,
                kind: TExprKind::Local(env.place_of(name)),
            }
        }
        Expr::Unary(op, inner, _) => {
            let operand = lower_expr(inner, cx, env);
            let ty = operand.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::Unary {
                    op: *op,
                    operand: Box::new(operand),
                },
            }
        }
        Expr::Binary(op, l, r, span) => {
            let lhs = lower_expr(l, cx, env);
            let rhs = lower_expr(r, cx, env);
            // Overflow decision, computed here once — this is the fact today's
            // `operand_is_integer` re-derives in codegen. It must mirror that
            // function EXACTLY (Codegen/Expression.rs): only a *resolvable*
            // integer operand traps. A struct-field read resolves to `None` in the
            // AST path (`expr_jet_ty` has no `Field` arm), so it does NOT trap —
            // hence we can't just inspect `TExpr.ty`, which is total even for a
            // field. We instead replay `operand_is_integer` on the AST operands.
            // `operand_is_integer` inspects only the LEFT spine of nested
            // arithmetic, so check the left operand first, then the right.
            let overflow = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && (ast_operand_is_integer(l, env) == Some(true)
                    || ast_operand_is_integer(r, env) == Some(true));
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
            // A comparison/logical op yields Bool; arithmetic keeps the operand type.
            let ty = if op.is_comparison() || matches!(op, BinOp::And | BinOp::Or) {
                Type::Bool
            } else {
                lhs.ty.clone()
            };
            TExpr {
                ty,
                kind: TExprKind::Binary {
                    op: *op,
                    overflow,
                    line,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            }
        }
        Expr::Call(call) => {
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Print(Box::new(arg)),
                };
            }
            // Resolve the callee's signature so each arg's borrow/clone is decided
            // here, totally — mirroring `emit_call_args` for scalar/String params.
            let sig = cx.sigs.get(&call.name);
            let args = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let value = lower_expr(&a.expr, cx, env);
                    let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                    // String (`Read`) → `&(...)`. Scalars never borrow.
                    let borrow = matches!(
                        &conv,
                        Some((AccessConvention::Read, t)) if !t.is_scalar()
                    );
                    // An implicit clone (a String passed by value). The plain-call
                    // subset excludes Arc auto-clone and `Mutate` args (gate), so
                    // those wrappers are always off here.
                    let clone = a.flags.implicit_clone;
                    TCallArg {
                        value,
                        borrow,
                        mut_borrow: false,
                        clone,
                        arc_clone: false,
                    }
                })
                .collect();
            let ret = call_return_type(cx, &call.name);
            TExpr {
                ty: ret,
                kind: TExprKind::Call {
                    name: call.name.clone(),
                    args,
                },
            }
        }
        // c109 Phase 6: a method call. The gate (`method_call_in_subset`) admitted
        // exactly the synthetic `.clone()` or a user instance method on a covered
        // type; lower accordingly. Every dispatch fact is resolved here (totality).
        Expr::MethodCall { receiver, method, args, recv_type, .. } => {
            lower_method_call(receiver, method, args, recv_type, cx, env)
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let c = lower_expr(cond, cx, env);
            // Value blocks scope their own bindings (like lambda block bodies).
            let mut then_env = clone_env(env);
            let t_body = lower_stmts(then_body, cx, &mut then_env);
            let t_val = lower_expr(then_value, cx, &mut then_env);
            let mut else_env = clone_env(env);
            let e_body = lower_stmts(else_body, cx, &mut else_env);
            let e_val = lower_expr(else_value, cx, &mut else_env);
            // Both arms share a type (sema guaranteed it); take the then arm's.
            let ty = t_val.ty.clone();
            TExpr {
                ty,
                kind: TExprKind::IfExpr {
                    cond: Box::new(c),
                    then_body: t_body,
                    then_value: Box::new(t_val),
                    else_body: e_body,
                    else_value: Box::new(e_val),
                },
            }
        }
        // c109 Phase 3: a struct literal. The gate already proved the type is a
        // plain covered user struct (no trait coercion, no import namespace, no
        // generic args), so the Rust head is `user_<name>` and field names mangle.
        // Field values are lowered as-is — no clone/coercion at the literal site
        // (mirrors the AST path; a value's own move/clone facts live in itself).
        Expr::StructLit {
            type_name, fields, ..
        } => {
            let tfields = fields
                .iter()
                .map(|(n, _, fe)| (mangle(n), lower_expr(fe, cx, env)))
                .collect();
            TExpr {
                ty: Type::Named(type_name.clone()),
                kind: TExprKind::StructLit {
                    rust_type: format!("user_{}", type_name),
                    fields: tfields,
                },
            }
        }
        // c109 Phase 3: a struct field read in borrow position. Resolve the field
        // type ONCE here from the receiver's resolved struct type (totality). A
        // covered function never reaches here with a non-struct receiver (sema
        // guarantees field reads target struct values).
        Expr::Field(receiver, member, _) => {
            // c109 Phase 4: a *unit* enum literal (`Light.Yellow`) reaches codegen as
            // a `Field` whose receiver is the enum-name ident (sema re-types but does
            // not rewrite the node). The gate proved this is a covered enum + unit
            // variant; emit `user_<Enum>::user_<variant>` (the AST path's form).
            if let Expr::Ident(enum_name, _) = receiver.as_ref() {
                if env.ty_of(enum_name).is_none()
                    && cx.variant_owner.get(member).map(String::as_str)
                        == Some(enum_name.as_str())
                {
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix: format!("user_{}::{}", enum_name, mangle(member)),
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
            }
            let recv = lower_expr(receiver, cx, env);
            let field_ty = struct_field_type(cx, &recv.ty, member).unwrap_or(Type::Int);
            TExpr {
                ty: field_ty,
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field_rust: mangle(member),
                },
            }
        }
        // c109 Phase 4: an enum literal. The gate proved the enum is covered (all
        // payloads scalar/Char), so no arg is ever borrowed-in-env or a boxed edge
        // — the AST path's `emit_boxed_enum_arg` is a no-op for these, so each arg
        // lowers as-is with no clone/box (decision-free, byte-parity).
        Expr::EnumLit { type_name, variant, args, .. } => {
            let prefix = format!("user_{}::{}", type_name, mangle(variant));
            let payload = if args.is_empty() {
                TEnumPayload::Unit
            } else if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
                let pos = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Positional(e) => lower_expr(e, cx, env),
                        _ => unreachable!("all positional in this branch"),
                    })
                    .collect();
                TEnumPayload::Positional(pos)
            } else {
                // Named-payload variant: each field carries its mangled Rust name.
                let named = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Named { label, expr } => {
                            (mangle(label), lower_expr(expr, cx, env))
                        }
                        // A positional arg mixed with named is a sema error that
                        // never reaches a covered function; default to a field.
                        EnumLitArg::Positional(e) => {
                            (String::new(), lower_expr(e, cx, env))
                        }
                    })
                    .collect();
                TEnumPayload::Named(named)
            };
            TExpr {
                ty: Type::Named(type_name.clone()),
                kind: TExprKind::EnumLit { prefix, payload },
            }
        }
        // c109 Phase 5: a list literal. Lowers each element as-is (mirrors the AST
        // `vec![…]` form — no clone/coercion at the literal site). The result type
        // is `[E]` with `E` taken from the first element; an empty `[]` has no
        // element to read, so its element type is unresolved (`Int` placeholder),
        // but the emitted `vec![]` is type-inferred by Rust from the binding context.
        Expr::ListLit(elems, _) => {
            let telems: Vec<TExpr> = elems.iter().map(|e| lower_expr(e, cx, env)).collect();
            let elem_ty = telems.first().map(|e| e.ty.clone()).unwrap_or(Type::Int);
            TExpr {
                ty: Type::List(Box::new(elem_ty)),
                kind: TExprKind::ListLit(telems),
            }
        }
        // c109 Phase 5: a map literal `[k: v, …]` / `[:]`. Keys/values lower as-is;
        // the result type is `[K, V]` from the first entry (empty `[:]` → unresolved
        // placeholder, type-inferred by Rust from context like `vec![]`).
        Expr::MapLit(entries, _) => {
            let tentries: Vec<(TExpr, TExpr)> = entries
                .iter()
                .map(|(k, v)| (lower_expr(k, cx, env), lower_expr(v, cx, env)))
                .collect();
            let (kt, vt) = tentries
                .first()
                .map(|(k, v)| (k.ty.clone(), v.ty.clone()))
                .unwrap_or((Type::String, Type::Int));
            TExpr {
                ty: Type::Map {
                    key: Box::new(kt),
                    value: Box::new(vt),
                },
                kind: TExprKind::MapLit(tentries),
            }
        }
        // c109 Phase 5: indexing `coll[i]`. The `IndexKind` (List/Map) is the total
        // sema fact (`is_map`); the helper line is resolved at lowering. The result
        // type is the list element / map value type, read from the base's resolved
        // type (totality) — never re-inferred in emit.
        Expr::Index { base, index, span, kind } => {
            let base_t = lower_expr(base, cx, env);
            let index_t = lower_expr(index, cx, env);
            let result_ty = match &base_t.ty {
                Type::List(elem) => (**elem).clone(),
                Type::Map { value, .. } => (**value).clone(),
                Type::FixedList { elem, .. } => (**elem).clone(),
                _ => Type::Int,
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Index {
                    base: Box::new(base_t),
                    index: Box::new(index_t),
                    is_map: matches!(kind, IndexKind::Map),
                    line,
                },
            }
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists). Lowers to the
        // `jet_slice_vec` helper; the result is a list of the same element type.
        Expr::Slice { base, start, end, span } => {
            let base_t = lower_expr(base, cx, env);
            let start_t = lower_expr(start, cx, env);
            let end_t = lower_expr(end, cx, env);
            let result_ty = base_t.ty.clone();
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Slice {
                    base: Box::new(base_t),
                    start: Box::new(start_t),
                    end: Box::new(end_t),
                    line,
                },
            }
        }
        // c109 Phase 8: `value(x)` → `Some(x)`. The result type is `T?` where `T` is
        // the inner's resolved type (totality). Mirrors `Expr::Present`.
        Expr::Present(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Option(Box::new(t.ty.clone())),
                kind: TExprKind::Present(Box::new(t)),
            }
        }
        // c109 Phase 8: bare `null` → `None`. The element type is unresolved here
        // (`Int` placeholder) — like an empty `vec![]`, Rust infers it from the
        // binding/return context. Mirrors `Expr::Absent`.
        Expr::Absent(_) => TExpr {
            ty: Type::Option(Box::new(Type::Int)),
            kind: TExprKind::Absent,
        },
        // c109 Phase 8: `ok(x)` → `Ok(x)`. The result is a `Result` whose ok type is
        // the inner's; the err type is unresolved here (Rust infers it from the
        // function return context, exactly as the AST path's bare `Ok(x)` does).
        Expr::Ok(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(t.ty.clone()),
                    err: Box::new(Type::Named("Error".to_string())),
                },
                kind: TExprKind::Ok(Box::new(t)),
            }
        }
        // c109 Phase 8: `err(e)` → `Err(e)`. The err type is the inner's; the ok type
        // is unresolved here (inferred from the function return context).
        Expr::Err(inner, _) => {
            let t = lower_expr(inner, cx, env);
            TExpr {
                ty: Type::Result {
                    ok: Box::new(Type::Int),
                    err: Box::new(t.ty.clone()),
                },
                kind: TExprKind::Err(Box::new(t)),
            }
        }
        // c109 Phase 8: the `?` propagation operator. The `TryConvert` decision is the
        // total sema fact — reproduce it exactly (none/Fallible/Typed). The result
        // type is the inner `Result`'s ok type (the `?` unwraps it). The trace-frame
        // location is resolved here so emit never reads `cx.current_fn`/`cx.src`.
        Expr::Try(inner, span, convert) => {
            let inner_t = lower_expr(inner, cx, env);
            // `?` unwraps a `Result<T, E>` to `T` (the value type). If the inner type
            // resolved to a Result, take its ok type; else fall back to the inner type
            // (never load-bearing in the covered subset — a `?` result feeds a binding
            // carrying sema's `b.ty`, or an `ok(...)` wrap whose own type is total).
            let result_ty = match &inner_t.ty {
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tconvert = match convert {
                TryConvert::None => TTryConvert::None,
                TryConvert::Fallible => TTryConvert::Fallible,
                TryConvert::Typed(fn_name) => TTryConvert::Typed(fn_name.clone()),
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: result_ty,
                kind: TExprKind::Try {
                    inner: Box::new(inner_t),
                    convert: tconvert,
                    file: escape_rust_str(&cx.file),
                    line,
                    fn_name: escape_rust_str(&env.fn_name),
                },
            }
        }
        // c109 Phase 8: the `??` fallback operator. `is_option` is the total sema fact
        // (Result vs Option). The value + fallback are lowered; the result type is the
        // unwrapped value type (Some/Ok payload). Mirrors `emit_or_fallback`.
        Expr::OrFallback { value, fallback, is_option, .. } => {
            let value_t = lower_expr(value, cx, env);
            let result_ty = match &value_t.ty {
                Type::Option(inner) => (**inner).clone(),
                Type::Result { ok, .. } => (**ok).clone(),
                other => other.clone(),
            };
            let tfallback = match fallback {
                OrFallback::Value(e) => TOrFallback::Value(Box::new(lower_expr(e, cx, env))),
                OrFallback::Return(None, _) => TOrFallback::Return(None),
                OrFallback::Return(Some(e), _) => {
                    TOrFallback::Return(Some(Box::new(lower_expr(e, cx, env))))
                }
                // The gate excludes the Panic form, so this never arises.
                OrFallback::Panic { .. } => unreachable!("gate excludes panic fallback"),
            };
            TExpr {
                ty: result_ty,
                kind: TExprKind::OrFallback {
                    value: Box::new(value_t),
                    fallback: tfallback,
                    is_option: *is_option,
                },
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. The `flatten` fact is total
        // (from sema): true → `.and_then`, false → `.map`. The result type is `T?`;
        // resolving the inner field type here is not load-bearing (emit only formats
        // the combinator + member access), so carry the base's optional type.
        Expr::OptField { base, member, flatten, .. } => {
            let base_t = lower_expr(base, cx, env);
            TExpr {
                ty: base_t.ty.clone(),
                kind: TExprKind::OptField {
                    base: Box::new(base_t),
                    member_rust: mangle(member),
                    flatten: *flatten,
                },
            }
        }
        _ => unreachable!("expression not in TIR subset"),
    }
}

/// Replay codegen's `operand_is_integer` (Codegen/Expression.rs) on an AST
/// operand, using the lowering env for identifier types. The result MUST match
/// that function bit-for-bit so the TIR's overflow-trap decision is identical to
/// the AST path's. Like the original: literals/negation/nested-arithmetic-left
/// resolve structurally; an `Ident` resolves via its slot type; everything else
/// (notably a struct-field read) is unresolved (`None`) and so never traps.
fn ast_operand_is_integer(e: &Expr, env: &LowerEnv) -> Option<bool> {
    match e {
        Expr::Int(..) => Some(true),
        Expr::Float(..) => Some(false),
        Expr::Unary(UnOp::Neg, inner, _) => ast_operand_is_integer(inner, env),
        Expr::Binary(_, l, _, _) => ast_operand_is_integer(l, env),
        // Mirror `expr_jet_ty`: only `Ident`/`Str`/`Char` resolve here. A `Field`
        // (and anything else) resolves to `None` — exactly as the AST path does,
        // so a field operand never enables the overflow trap.
        Expr::Ident(name, _) => env.ty_of(name).map(|t| t.is_integer()),
        Expr::Str(..) => Some(false),
        Expr::Char(..) => Some(false),
        _ => None,
    }
}

/// Look up a field's declared type on a resolved struct receiver type. Returns
/// `None` when the receiver is not a known struct or the field is absent — both
/// impossible for a covered function (sema validated the access).
fn struct_field_type(cx: &Cx, recv_ty: &Type, field: &str) -> Option<Type> {
    let Type::Named(name) = recv_ty else {
        return None;
    };
    cx.struct_fields
        .get(name)?
        .iter()
        .find(|(f, _)| f == field)
        .map(|(_, t)| t.clone())
}

/// The type of an integer literal given its elaborated width.
fn int_lit_type(width: &Option<(bool, u8)>) -> Type {
    match width {
        Some((signed, bits)) => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        None => Type::Int,
    }
}

fn unit_type() -> Type {
    Type::Named("Unit".to_string())
}

/// The resolved return type of a called plain function: its declared return
/// type if known, else `Unit`. (In the subset, callees return scalar/String/Unit.)
/// Read from `cx.fn_types`, which sema-built `Type::Fn { ret, .. }` per function.
fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret: Some(r), .. }) => (**r).clone(),
        _ => unit_type(),
    }
}

/// c109 Phase 6: lower a method call. The gate proved it is the synthetic `.clone()`
/// or a user instance method on a covered type; resolve every dispatch fact here.
fn lower_method_call(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    // The sema-inserted `.clone()`: emit `(recv).clone()`, result is the receiver's
    // type (a clone preserves it). Mirrors `emit_method_call`'s `clone` early return.
    if method == "clone" {
        let recv = lower_expr(receiver, cx, env);
        let ty = recv.ty.clone();
        return TExpr {
            ty,
            kind: TExprKind::Clone(Box::new(recv)),
        };
    }
    // c109 Phase 7: a STATIC method call `Type.make(args)`. The gate
    // (`static_method_call_in_subset`) proved the receiver is a covered type-name
    // ident and `method` is a registered static method. Mirror the AST path
    // (Expression.rs ~L1644): `user_<Type>::user_<method>(args)`.
    if recv_type.is_none() {
        let Expr::Ident(type_name, _) = receiver else {
            unreachable!("gate proved static receiver is a type-name ident");
        };
        let sig = cx
            .method_sigs
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .unwrap_or_default();
        let targs = lower_method_args(args, &sig, env, cx);
        let ret_ty = cx
            .method_rets
            .get(&(type_name.clone(), method.to_string()))
            .cloned()
            .flatten()
            .map(|t| resolve_self_ty(&t, type_name))
            .unwrap_or_else(unit_type);
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::StaticCall {
                // The AST path uses `cx.type_prefix(type_name)` = `user_<T>`.
                type_prefix: cx.type_prefix(type_name),
                method_rust: mangle(method),
                args: targs,
            },
        };
    }
    // A user instance method on a covered type. `recv_type` is total (gate proved
    // `Some`). Resolve the param conventions from `method_sigs` and the Rust method
    // name (trait-impl methods keep their bare name; others get the `user_` mangle).
    let ty_name = recv_type.clone().expect("gate proved recv_type is Some");
    let sig = cx
        .method_sigs
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .unwrap_or_default();
    let recv = lower_expr(receiver, cx, env);
    let targs = lower_method_args(args, &sig, env, cx);
    // S62: a trait-impl method is called by its bare name (the trait impl owns it);
    // a plain user method is `user_<method>`. This mirrors `emit_method_call`'s
    // `trait_methods` check exactly — decided here, total, never re-derived in emit.
    let method_rust = if cx
        .trait_methods
        .contains(&(ty_name.clone(), method.to_string()))
    {
        method.to_string()
    } else {
        mangle(method)
    };
    // The result type, read from the resolved method return (total fact). It is
    // rarely load-bearing in emit (a binding carries sema's `b.ty`; arithmetic on a
    // method result doesn't trap — matching the AST `expr_jet_ty`/`operand_is_integer`),
    // but the TIR keeps it total per the design principle.
    let ret_ty = cx
        .method_rets
        .get(&(ty_name.clone(), method.to_string()))
        .cloned()
        .flatten()
        .unwrap_or_else(unit_type);
    TExpr {
        ty: ret_ty,
        kind: TExprKind::MethodCall {
            recv: Box::new(recv),
            method_rust,
            args: targs,
        },
    }
}

/// c109 Phase 6: lower method-call arguments, mirroring `emit_call_args`
/// (Source/Codegen/Expression.rs). The clone/Arc wrappers and the borrow/mut-borrow
/// wrappers are decided here from the total facts (`CallArg.flags` + the resolved
/// param convention/type), never re-derived in emit. The gate excluded Fn-typed
/// params, so no Box-coercion form arises.
fn lower_method_args(
    args: &[crate::AST::CallArg],
    sig: &[(AccessConvention, Type)],
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let value = lower_expr(&a.expr, cx, env);
            let conv = sig.get(i).map(|(c, t)| (*c, t.clone()));
            // Clone wrappers (applied to the raw value first, exactly as emit_call_args).
            let clone = a.flags.implicit_clone;
            let arc_clone = a.flags.shared_auto_clone;
            // Borrow wrappers (applied after the clone wrapper). A `Read` non-scalar
            // (non-Fn) is `&(…)`; a `Mutate` is `&mut (…)`.
            let (borrow, mut_borrow) = match &conv {
                Some((AccessConvention::Read, t))
                    if !t.is_scalar() && !matches!(t, Type::Fn { .. }) =>
                {
                    (true, false)
                }
                Some((AccessConvention::Mutate, _)) => (false, true),
                _ => (false, false),
            };
            TCallArg {
                value,
                borrow,
                mut_borrow,
                clone,
                arc_clone,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Emission: TIR -> Rust. PURE formatting. No type inference, no decisions.
// ---------------------------------------------------------------------------

/// Emit a covered function from its TIR, reusing the same pure formatting helpers
/// as `emit_func` so the output is byte-identical to the AST path (golden parity).
/// The only difference is that every decision is *read off the TIR* rather than
/// recomputed — there is no `expr_jet_ty` / `operand_is_integer` call anywhere.
pub(crate) fn emit_tir_func(tir: &TFunc, cx: &Cx, out: &mut String) {
    match &tir.kind {
        TFuncKind::TopLevel => emit_tir_toplevel(tir, cx, out),
        TFuncKind::Method { self_conv } => emit_tir_method(tir, *self_conv, cx, out),
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }` (or `fn main`).
/// Byte-identical to `emit_func`'s output.
fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, false)),
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .map(|(rust_name, ty, conv)| {
            format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{vis}fn {name}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        params = params,
        ret = ret_clause,
    ));
    emit_tir_stmts(&tir.body, cx, out, 1);
    out.push_str("}\n\n");
}

/// c109 Phase 7: an inherent method, emitted INSIDE an `impl user_<T> { … }` block
/// (the caller `emit_type_impl` already opened it). Byte-identical to `emit_method`:
/// `    pub fn user_<name>(<self>, <params>) -> <ret> {\n … \n    }\n`. The `self`
/// receiver form comes from `self_conv` (`Read`→`&self`, `Mutate`→`&mut self`,
/// `Move`→`self`); a static method (`self_conv == None`) emits no receiver.
fn emit_tir_method(tir: &TFunc, self_conv: Option<AccessConvention>, cx: &Cx, out: &mut String) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, false)),
        None => String::new(),
    };
    let mut params: Vec<String> = Vec::new();
    if let Some(conv) = self_conv {
        params.push(
            match conv {
                AccessConvention::Read => "&self",
                AccessConvention::Mutate => "&mut self",
                AccessConvention::Move => "self",
            }
            .to_string(),
        );
    }
    for (rust_name, ty, conv) in &tir.params {
        params.push(format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty)));
    }
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}pub fn {name}({params}){ret} {{\n",
        name = mangle(&tir.name),
        params = params.join(", "),
        ret = ret_clause,
    ));
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

fn emit_tir_stmts(stmts: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    for s in stmts {
        emit_tir_stmt(s, cx, out, indent);
    }
}

fn emit_tir_stmt(s: &TStmt, cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    match s {
        TStmt::Let {
            name,
            ty,
            annotated,
            mutable,
            init,
        } => {
            let kw = if *mutable { "let mut" } else { "let" };
            let ty_clause = if *annotated {
                format!(": {}", cx.rust_type(ty))
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(name),
                ty_clause,
                emit_tir_expr(init, cx),
            ));
        }
        TStmt::Assign { place, op, value } => {
            let v = emit_tir_expr(value, cx);
            match op {
                Some(op) => out.push_str(&format!("{}{} {}= {};\n", pad, place, op.spell(), v)),
                None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
            }
        }
        TStmt::Return(Some(e)) => {
            out.push_str(&format!("{}return {};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::Return(None) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        TStmt::ExprStmt(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            out.push_str(&format!("{}if {} {{\n", pad, emit_tir_expr(cond, cx)));
            emit_tir_stmts(then_body, cx, out, indent + 1);
            match else_body {
                None => out.push_str(&format!("{}}}\n", pad)),
                Some(body) => {
                    // Match the AST path's `else if` flattening: a one-statement
                    // else-body holding a single `If` is emitted as `} else if …`.
                    if let [TStmt::If { .. }] = body.as_slice() {
                        out.push_str(&format!("{}}} else ", pad));
                        let mut nested = String::new();
                        emit_tir_stmt(&body[0], cx, &mut nested, indent);
                        out.push_str(nested.trim_start_matches(&pad as &str));
                    } else {
                        out.push_str(&format!("{}}} else {{\n", pad));
                        emit_tir_stmts(body, cx, out, indent + 1);
                        out.push_str(&format!("{}}}\n", pad));
                    }
                }
            }
        }
        // c109 Phase 2: control-flow loops. Each mirrors the AST emit path
        // (Statement.rs) byte-for-byte; all decisions are read off the TIR.
        TStmt::Loop { label, body } => {
            out.push_str(&format!("{}{}loop {{\n", pad, tir_label_prefix(label)));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::While { label, cond, body } => {
            out.push_str(&format!(
                "{}{}while {} {{\n",
                pad,
                tir_label_prefix(label),
                emit_tir_expr(cond, cx)
            ));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Range {
            label,
            var,
            start,
            end,
            step,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            let s = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            // S22 (D-SG8): `..` is inclusive → `..=`; `step` becomes `.step_by`.
            match step {
                Some(step) => {
                    let st = emit_tir_expr(step, cx);
                    out.push_str(&format!(
                        "{}{}for {} in (({})..=({})).step_by(({}) as usize) {{\n",
                        pad,
                        lbl,
                        mangle(var),
                        s,
                        e,
                        st
                    ));
                }
                None => {
                    out.push_str(&format!(
                        "{}{}for {} in ({})..=({}) {{\n",
                        pad,
                        lbl,
                        mangle(var),
                        s,
                        e
                    ));
                }
            }
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        TStmt::Break(label) => match label {
            Some(name) => out.push_str(&format!("{}break 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}break;\n", pad)),
        },
        TStmt::Continue(label) => match label {
            Some(name) => out.push_str(&format!("{}continue 'jet_{};\n", pad, name)),
            None => out.push_str(&format!("{}continue;\n", pad)),
        },
        // c109 Phase 4: an exhaustive enum match. Mirrors `emit_pattern_match_switch`
        // (Statement.rs) byte-for-byte; every pattern/guard string was resolved at
        // lowering. Arm bodies emit at indent+2.
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            fallthrough,
        } => {
            out.push_str(&format!("{}match {} {{\n", pad, scrutinee));
            for arm in arms {
                match &arm.guard {
                    Some(guard) => {
                        out.push_str(&format!("{}    {} if {} => {{\n", pad, arm.pattern, guard))
                    }
                    None => out.push_str(&format!("{}    {} => {{\n", pad, arm.pattern)),
                }
                emit_tir_stmts(&arm.body, cx, out, indent + 2);
                out.push_str(&format!("{}    }}\n", pad));
            }
            match else_body {
                Some(body) => {
                    out.push_str(&format!("{}    _ => {{\n", pad));
                    emit_tir_stmts(body, cx, out, indent + 2);
                    out.push_str(&format!("{}    }}\n", pad));
                }
                None if *fallthrough => {
                    // Sema proved exhaustiveness (E0307); this dead arm exists only
                    // so rustc sees a complete match (I2/I3).
                    out.push_str(&format!(
                        "{}    _ => unreachable!(\"jet: exhaustiveness bug\"),\n",
                        pad
                    ));
                }
                None => {}
            }
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 4: an all-range scalar switch. Mirrors `emit_mixed_switch`
        // (Statement.rs): a wrapping block binds `_jet_switch_subject` (unused here,
        // emitted for parity), then an `if/else if … else` chain of range tests.
        TStmt::RangeSwitch {
            subject_str,
            arms,
            else_body,
        } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner_pad = "    ".repeat(indent + 1);
            out.push_str(&format!(
                "{}let _jet_switch_subject = &({});\n",
                inner_pad, subject_str
            ));
            for (i, (lo, hi, body)) in arms.iter().enumerate() {
                let kw = if i == 0 { "if" } else { "} else if" };
                out.push_str(&format!(
                    "{}{} ({} >= {} && {} <= {}) {{\n",
                    inner_pad, kw, subject_str, lo, subject_str, hi
                ));
                emit_tir_stmts(body, cx, out, indent + 2);
            }
            out.push_str(&format!("{}}} else {{\n", inner_pad));
            emit_tir_stmts(else_body, cx, out, indent + 2);
            out.push_str(&format!("{}}}\n", inner_pad));
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 5: indexed assignment `coll[i] = v`. Mirrors the AST
        // `LValue::Index` form byte-for-byte: a map insert clones the key; a vec
        // assign casts the index to `usize`. Both wrap the value in a block.
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
        } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            let v = emit_tir_expr(value, cx);
            if *is_map {
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; jet_map_insert(&mut ({b}), ({i}).clone(), __jet_v); }}\n",
                ));
            } else {
                out.push_str(&format!(
                    "{pad}{{ let __jet_v = {v}; ({b})[{i} as usize] = __jet_v; }}\n",
                ));
            }
        }
        // c109 Phase 5: collection iteration. Mirrors `emit_for_in` for the two
        // plain `.iter()` shapes (method-call collections are excluded by the gate):
        //   single: `for _jet_item in (coll).iter().cloned() { let var = _jet_item; … }`
        //   map k,v: `for (_jet_k, _jet_v) in (coll).iter() { let k = _jet_k.clone();
        //             let v = _jet_v.clone(); … }`
        TStmt::ForIn {
            label,
            var,
            var2,
            collection_str,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            match var2 {
                Some(v2) => {
                    out.push_str(&format!(
                        "{}{}for (_jet_k, _jet_v) in ({}).iter() {{\n",
                        pad, lbl, collection_str
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_k.clone();\n",
                        pad,
                        mangle(var)
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_v.clone();\n",
                        pad,
                        mangle(v2)
                    ));
                }
                None => {
                    out.push_str(&format!(
                        "{}{}for _jet_item in ({}).iter().cloned() {{\n    {}let {} = _jet_item;\n",
                        pad,
                        lbl,
                        collection_str,
                        pad,
                        mangle(var)
                    ));
                }
            }
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

/// Mirror `loop_label_prefix` (Codegen/Utils.rs) for a resolved label name:
/// `'jet_<name>: ` or empty. Kept here so the TIR emitter never reaches back
/// into the AST-side helper with an `Option<(String, Span)>`.
fn tir_label_prefix(label: &Option<String>) -> String {
    match label {
        Some(n) => format!("'jet_{}: ", n),
        None => String::new(),
    }
}

fn emit_tir_expr(e: &TExpr, cx: &Cx) -> String {
    match &e.kind {
        // D-SG9: width suffix is read straight off the literal — no re-inference.
        TExprKind::IntLit(n, width) => match width {
            Some((signed, bits)) => format!("{}{}{}", n, if *signed { 'i' } else { 'u' }, bits),
            None => format!("{}i64", n),
        },
        TExprKind::FloatLit(v) => format!("{:?}", v),
        TExprKind::BoolLit(b) => b.to_string(),
        TExprKind::CharLit(c) => format!("{:?}", c),
        TExprKind::StrLit(parts) => emit_tir_str(parts, cx),
        TExprKind::Local(place) => place.clone(),
        TExprKind::Print(arg) => {
            format!("println!(\"{{}}\", ({}).jet_show())", emit_tir_expr(arg, cx))
        }
        TExprKind::Call { name, args } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("{}({})", cx.mangle_name(name), arg_str)
        }
        // c109 Phase 6: the synthetic `.clone()`. Mirrors `emit_method_call`'s
        // `clone` early return: `(recv).clone()`, no deref/borrow decision (the
        // receiver was already lowered to the place the AST path would clone).
        TExprKind::Clone(recv) => {
            format!("({}).clone()", emit_tir_expr(recv, cx))
        }
        // c109 Phase 6: a user instance method call. Mirrors `emit_method_call`'s
        // final dispatch (`(recv).{method}({args})`): Rust's method autoref handles
        // the `&self`/`&mut self`/`self` receiver convention, so codegen emits the
        // receiver place as-is. The method name + arg wrappers were resolved at
        // lowering — emit only formats.
        TExprKind::MethodCall {
            recv,
            method_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("({}).{}({})", emit_tir_expr(recv, cx), method_rust, arg_str)
        }
        // c109 Phase 7: a static method call. Mirrors the AST type-name dispatch:
        // `user_<Type>::user_<method>(args)`. All facts resolved at lowering.
        TExprKind::StaticCall {
            type_prefix,
            method_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("{}::{}({})", type_prefix, method_rust, arg_str)
        }
        TExprKind::Binary {
            op,
            overflow,
            line,
            lhs,
            rhs,
        } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            if *overflow {
                // Trapping helper: source location was resolved at lowering, so
                // the panic message matches the AST path exactly.
                let (file, line) = (&cx.file, *line);
                let method = match op {
                    BinOp::Add => "jet_add",
                    BinOp::Sub => "jet_sub",
                    BinOp::Mul => "jet_mul",
                    BinOp::Div => "jet_div",
                    _ => unreachable!("overflow flag only set for +,-,*,/"),
                };
                format!("({}).{}(({}), {:?}, {})", ls, method, rs, file, line)
            } else {
                format!("(({}) {} ({}))", ls, op.spell(), rs)
            }
        }
        TExprKind::Unary { op, operand } => {
            let i = emit_tir_expr(operand, cx);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        // c109 Phase 3: `user_S { f: v, … }`. The Rust head and mangled field
        // names were resolved at lowering; values format like any other node.
        TExprKind::StructLit { rust_type, fields } => {
            let parts = fields
                .iter()
                .map(|(field_rust, v)| format!("{}: {}", field_rust, emit_tir_expr(v, cx)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", rust_type, parts)
        }
        // c109 Phase 3: `(recv).field`. Mirrors the AST `Expr::Field` emit form
        // exactly (no deref, no clone — owning reads were rewritten to a `.clone()`
        // MethodCall in sema and excluded from the subset).
        TExprKind::Field { recv, field_rust } => {
            format!("({}).{}", emit_tir_expr(recv, cx), field_rust)
        }
        // c109 Phase 4: an enum literal. Prefix + payload were resolved at lowering;
        // emit only formats. Mirrors `emit_enum_lit` for the scalar-payload subset.
        TExprKind::EnumLit { prefix, payload } => match payload {
            TEnumPayload::Unit => prefix.clone(),
            TEnumPayload::Positional(vals) => {
                let pos = vals
                    .iter()
                    .map(|v| emit_tir_expr(v, cx))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", prefix, pos)
            }
            TEnumPayload::Named(fields) => {
                let parts = fields
                    .iter()
                    .map(|(name, v)| format!("{}: {}", name, emit_tir_expr(v, cx)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", prefix, parts)
            }
        },
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            let c = emit_tir_expr(cond, cx);
            let then_block = emit_tir_value_block(then_body, then_value, cx);
            let else_block = emit_tir_value_block(else_body, else_value, cx);
            format!("if {} {} else {}", c, then_block, else_block)
        }
        // c109 Phase 5: `[a, b, c]` → `vec![a, b, c]`. Mirrors the AST `Expr::ListLit`.
        TExprKind::ListLit(elems) => {
            let parts = elems
                .iter()
                .map(|e| emit_tir_expr(e, cx))
                .collect::<Vec<_>>()
                .join(", ");
            format!("vec![{}]", parts)
        }
        // c109 Phase 5: `[k: v, …]` / `[:]`. Mirrors the AST `Expr::MapLit` exactly:
        // empty → `BTreeMap::new()`; non-empty → the `_m.insert((k).clone(), v)` builder.
        TExprKind::MapLit(entries) => {
            if entries.is_empty() {
                "std::collections::BTreeMap::new()".to_string()
            } else {
                let mut s =
                    String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                for (k, v) in entries {
                    s.push_str(&format!(
                        "_m.insert(({}).clone(), {}); ",
                        emit_tir_expr(k, cx),
                        emit_tir_expr(v, cx)
                    ));
                }
                s.push_str("_m }");
                s
            }
        }
        // c109 Phase 5: `coll[i]`. Dispatch on the total `is_map` fact (never
        // re-inferred). Mirrors the AST `Expr::Index` form: a map index borrows the
        // key (`&(i)`), a vec index does not.
        TExprKind::Index { base, index, is_map, line } => {
            let b = emit_tir_expr(base, cx);
            let i = emit_tir_expr(index, cx);
            if *is_map {
                format!("jet_index_map(&({}), &({}), {:?}, {})", b, i, cx.file, line)
            } else {
                format!("jet_index_vec(&({}), {}, {:?}, {})", b, i, cx.file, line)
            }
        }
        // c109 Phase 5: `coll[a..b]` → `jet_slice_vec`. Mirrors the AST `Expr::Slice`.
        TExprKind::Slice { base, start, end, line } => {
            let b = emit_tir_expr(base, cx);
            let a = emit_tir_expr(start, cx);
            let e = emit_tir_expr(end, cx);
            format!("jet_slice_vec(&({}), {}, {}, {:?}, {})", b, a, e, cx.file, line)
        }
        // c109 Phase 8: `value(x)` → `Some(x)` / `null` → `None`. Mirrors the AST
        // `Expr::Present`/`Expr::Absent` exactly.
        TExprKind::Present(inner) => format!("Some({})", emit_tir_expr(inner, cx)),
        TExprKind::Absent => "None".to_string(),
        // c109 Phase 8: `ok(x)` → `Ok(x)` / `err(e)` → `Err(e)`. Mirrors the AST
        // `Expr::Ok`/`Expr::Err`.
        TExprKind::Ok(inner) => format!("Ok({})", emit_tir_expr(inner, cx)),
        TExprKind::Err(inner) => format!("Err({})", emit_tir_expr(inner, cx)),
        // c109 Phase 8: the `?` propagation operator. Mirrors `Expr::Try` byte-for-byte
        // (Expression.rs): a debug trace frame wraps the value, then the error is
        // converted per the total `TryConvert`, then `?` propagates. `file`/`fn_name`
        // were pre-escaped at lowering; `line` is plain.
        TExprKind::Try { inner, convert, file, line, fn_name } => {
            let v = emit_tir_expr(inner, cx);
            match convert {
                // S80/D-LIB3: error implements Fallible → `.map_err(|e| e.to_error())`.
                TTryConvert::Fallible => format!(
                    "jet_trace_err({}.map_err(|e| e.to_error()), {}, {}, {})?",
                    v, file, line, fn_name
                ),
                // D-ERR-CONV: declared `impl Source -> Target` → `.map_err(<fn>)`.
                TTryConvert::Typed(conv_fn) => format!(
                    "jet_trace_err({}.map_err({}), {}, {}, {})?",
                    v, conv_fn, file, line, fn_name
                ),
                // Error types match — bare propagate.
                TTryConvert::None => {
                    format!("jet_trace_err({}, {}, {}, {})?", v, file, line, fn_name)
                }
            }
        }
        // c109 Phase 8: the `??` fallback operator. Mirrors `emit_or_fallback`
        // (Statement.rs): a `Result` value unwraps `Ok`, an `Option` value unwraps
        // `Some`; the fallback runs on `Err(_)`/`None`. Decision read off the total
        // `is_option` flag — no re-inference.
        TExprKind::OrFallback { value, fallback, is_option } => {
            let v = emit_tir_expr(value, cx);
            let fb = emit_tir_orfallback_rhs(fallback, cx);
            if *is_option {
                format!("match {} {{ Some(__jet_v) => __jet_v, None => {} }}", v, fb)
            } else {
                format!("match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}", v, fb)
            }
        }
        // c109 Phase 8: optional chaining `base?.member`. Mirrors `Expr::OptField`:
        // `(base).clone().{and_then|map}(|__optv| __optv.{member})`. The combinator is
        // the total `flatten` fact (flatten → `and_then`, else → `map`).
        TExprKind::OptField { base, member_rust, flatten } => {
            let combinator = if *flatten { "and_then" } else { "map" };
            format!(
                "({}).clone().{}(|__optv| __optv.{})",
                emit_tir_expr(base, cx),
                combinator,
                member_rust
            )
        }
    }
}

/// c109 Phase 8: format a `??` fallback right-hand side, mirroring
/// `emit_or_fallback_rhs` (Statement.rs) for the Value and early-`return` forms (the
/// Panic form is excluded by the gate).
fn emit_tir_orfallback_rhs(fallback: &TOrFallback, cx: &Cx) -> String {
    match fallback {
        TOrFallback::Value(e) => emit_tir_expr(e, cx),
        TOrFallback::Return(None) => "return".to_string(),
        TOrFallback::Return(Some(e)) => format!("return {}", emit_tir_expr(e, cx)),
    }
}

fn emit_tir_value_block(stmts: &[TStmt], value: &TExpr, cx: &Cx) -> String {
    let mut inner = String::new();
    emit_tir_stmts(stmts, cx, &mut inner, 1);
    format!("{{ {} {} }}", inner, emit_tir_expr(value, cx))
}

/// c109 Phase 6: format call/method arguments, reproducing `emit_call_args`
/// (Source/Codegen/Expression.rs) byte-for-byte. The clone wrapper (`.clone()` or
/// `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`). All four decisions
/// are total TIR flags — emit makes no convention decision.
fn emit_tir_call_args(args: &[TCallArg], cx: &Cx) -> String {
    args.iter()
        .map(|a| {
            let mut s = emit_tir_expr(&a.value, cx);
            // emit_call_args applies implicit_clone XOR shared_auto_clone (the AST
            // path uses `if … else if …`); the gate/lowering never set both.
            if a.clone {
                s = format!("({}).clone()", s);
            } else if a.arc_clone {
                s = format!("std::sync::Arc::clone(&{})", s);
            }
            if a.borrow {
                s = format!("&({})", s);
            } else if a.mut_borrow {
                s = format!("&mut ({})", s);
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_tir_str(parts: &[TStrPart], cx: &Cx) -> String {
    if parts.len() == 1 {
        if let TStrPart::Lit(s) = &parts[0] {
            return format!("{:?}.to_string()", s);
        }
    }
    let mut body = String::from("{ let mut _jet_s = String::new(); ");
    for p in parts {
        match p {
            TStrPart::Lit(s) => {
                if !s.is_empty() {
                    body.push_str(&format!("_jet_s.push_str({:?}); ", s));
                }
            }
            TStrPart::Interp(e) => {
                body.push_str(&format!("_jet_s.push_str(&({}).jet_show()); ", emit_tir_expr(e, cx)));
            }
        }
    }
    body.push_str("_jet_s }");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::Item;

    /// Parse `src` (no full sema needed — `tir_covers` is structural plus
    /// program-table lookups that `build_cx` fills) and return whether the
    /// named function is covered by the Phase-1 TIR gate.
    fn covers(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let f = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Func(f) if f.name == fn_name => Some(f),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no fn {fn_name}"));
        tir_covers(f, &cx)
    }

    /// c109 Phase 7: parse `src` and return whether the named method on `type_name`
    /// (a struct or enum inherent method) is covered by the method gate. Looks up
    /// the method in the type's `methods` list. As with `covers`, the
    /// sema-dependent facts a method body needs (`recv_type` on inner method calls)
    /// are not filled by `build_cx` alone, so the gate paths that consult them are
    /// proven by `tests/tir.rs` + the byte-parity check; here we exercise the
    /// sema-independent structural gating (self receiver, static shape, param/return
    /// types, the `self`-assignment exclusion).
    fn covers_method(src: &str, type_name: &str, method: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let methods: &[Func] = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Struct(s) if s.name == type_name => Some(s.methods.as_slice()),
                Item::Enum(e) if e.name == type_name => Some(e.methods.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no type {type_name}"));
        let f = methods
            .iter()
            .find(|m| m.name == method)
            .unwrap_or_else(|| panic!("no method {type_name}.{method}"));
        tir_covers_method(f, type_name, &cx)
    }

    #[test]
    fn covers_simple_arithmetic_fn() {
        assert!(covers("fn add(a: Int, b: Int) -> Int {\n return (a + b)\n}\n", "add"));
    }

    #[test]
    fn covers_print_and_string_param() {
        assert!(covers("fn greet(s: String) {\n print(\"hi {s}\")\n}\n", "greet"));
    }

    #[test]
    fn covers_if_else_chain() {
        let src = "fn f(n: Int) -> Int {\n if (n > 0) {\n return 1\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_generic_fn() {
        assert!(!covers("fn id<T>(x: T) -> T {\n return x\n}\n", "id"));
    }

    #[test]
    fn covers_list_param() {
        // c109 Phase 5: a list parameter is now inside the subset (was excluded
        // through Phase 4).
        assert!(covers("fn sum(xs: [Int]) -> Int {\n return 0\n}\n", "sum"));
    }

    #[test]
    fn covers_option_param() {
        // c109 Phase 8: an optional-typed param (`Int?`) is now inside the subset
        // (was excluded through Phase 7). The payload is a covered value type.
        assert!(covers("fn f(p: Int?) -> Int {\n return 0\n}\n", "f"));
    }

    #[test]
    fn rejects_list_of_option_param_still() {
        // A list whose element is itself optional (`[Int?]`) is still excluded — the
        // collection element-coverage does not admit optionals (clone/coercion for an
        // option-element collection is deferred), even though a bare `Int?` is covered.
        assert!(!covers("fn f(xs: [Int?]) -> Int {\n return 0\n}\n", "f"));
    }

    #[test]
    fn rejects_method_call_in_body() {
        // A method call (`.bumped()`) is not a covered construct.
        let src = "struct C { n: Int }\nimpl C {\n fn bumped(self) -> Int {\n return (self.n + 1)\n }\n}\nfn use_it(c: Int) -> Int {\n return c\n}\nfn caller() -> Int {\n x @= C { n: 1 }\n return x.bumped()\n}\n";
        assert!(!covers(src, "caller"));
    }

    // c109 Phase 3: structs.

    #[test]
    fn covers_struct_param_and_scalar_field_read() {
        // A plain struct param with a scalar field read (borrow position) and a
        // struct literal + struct return are all in the subset.
        let src = "struct Point { x: Int\n y: Int }\nfn sum_pt(p: Point) -> Int {\n return (p.x + p.y)\n}\nfn origin() -> Point {\n return Point { x: 0, y: 0 }\n}\n";
        assert!(covers(src, "sum_pt"));
        assert!(covers(src, "origin"));
    }

    #[test]
    fn covers_nested_struct() {
        // A struct field whose type is itself a covered struct, with a chained
        // field read and a nested literal.
        let src = "struct Inner { v: Int }\nstruct Outer { inner: Inner\n tag: Int }\nfn deep(o: Outer) -> Int {\n return (o.inner.v + o.tag)\n}\n";
        assert!(covers(src, "deep"));
    }

    #[test]
    fn rejects_recursive_boxed_struct() {
        // A self-referential struct needs a `Box<…>` field; reading through it
        // requires deref handling the subset deliberately avoids — exclude.
        let src = "struct Node { value: Int\n next: Node }\nfn val(n: Node) -> Int {\n return n.value\n}\n";
        assert!(!covers(src, "val"));
    }

    #[test]
    fn rejects_struct_with_list_field() {
        // A non-scalar/non-struct field type (a list) is outside the subset, so
        // the owning struct is not covered as a param.
        let src = "struct Bag { items: [Int] }\nfn first_tag(b: Bag) -> Int {\n return 0\n}\n";
        assert!(!covers(src, "first_tag"));
    }

    #[test]
    fn rejects_generic_struct_literal() {
        // A generic struct (`Pair<Int> { … }`) carries non-empty `type_args` and
        // its field types reference type vars — both outside the subset (no
        // generics in Phase 3). The owning fn stays on the AST path.
        let src = "struct Pair<T> { first: T\n second: T }\nfn mk() -> Pair<Int> {\n return Pair<Int> { first: 1, second: 2 }\n}\n";
        assert!(!covers(src, "mk"));
    }

    // c109 Phase 2: control-flow loops are now covered.

    #[test]
    fn covers_range_loop() {
        let src = "fn f() {\n loop n in 1..3 {\n print(n)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_range_loop_with_step() {
        let src = "fn f() {\n loop n in 0..10 step 2 {\n print(n)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_infinite_loop_with_break() {
        let src = "fn f() {\n x @= 0\n loop {\n x = (x + 1)\n if (x > 3) {\n break\n }\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_while_form() {
        let src = "fn f() {\n x @= 0\n loop (x < 3) {\n x = (x + 1)\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_labeled_loops() {
        let src = "fn f() {\n @outer loop {\n loop n in 1..3 {\n if (n == 2) {\n break @outer\n }\n }\n break\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_collection_loop_over_literal() {
        // c109 Phase 5: `loop x in [list literal]` (ForKind::In) is now covered
        // (was deferred to this phase through Phase 4).
        let src = "fn f() {\n loop x in [1, 2, 3] {\n print(x)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    // c109 Phase 4: enums + when/match + patterns.

    #[test]
    fn covers_enum_unit_match() {
        // A unit-variant enum, an enum literal, and an exhaustive variant match.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn next(light: Light) -> Light {\n if light {\n Red -> { return Light.Yellow }\n Yellow -> { return Light.Green }\n Green -> { return Light.Red }\n }\n}\n";
        assert!(covers(src, "next"));
    }

    #[test]
    fn covers_enum_payload_or_and_wildcard() {
        // Scalar-payload enum, or-pattern with a shared binding, and a wildcard slot.
        let src = "enum Conn {\n Active(Int)\n Reconnecting(Int)\n Idle(Int)\n Closed\n}\nfn d(c: Conn) -> String {\n if c {\n c == Active(id) | Reconnecting(id) -> { return \"live:{id}\" }\n c == Idle(_) -> { return \"idle\" }\n c == Closed -> { return \"closed\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "d"));
    }

    #[test]
    fn covers_enum_payload_range_pattern() {
        // A range pattern in a payload slot (guard-emitted) plus a wildcard slot.
        let src = "enum Http {\n Good(Int)\n Fail(Int)\n}\nfn classify(r: Http) -> String {\n if r {\n r == Good(200..299) -> { return \"ok\" }\n r == Good(_) -> { return \"other\" }\n r == Fail(_) -> { return \"err\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "classify"));
    }

    #[test]
    fn covers_arm_head_range_switch() {
        // An all-range arm-head scalar switch with an `else` (mixed-switch path).
        let src = "fn grade(score: Int) -> String {\n if score {\n 0..59 -> { return \"F\" }\n 60..100 -> { return \"P\" }\n else -> { return \"?\" }\n }\n}\n";
        assert!(covers(src, "grade"));
    }

    #[test]
    fn covers_enum_local_and_literal_in_main() {
        // An enum-typed local bound from a literal, passed to a covered helper.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn label(l: Light) -> String {\n if l {\n Red -> { return \"r\" }\n Yellow -> { return \"y\" }\n Green -> { return \"g\" }\n }\n}\nfn main() {\n start @= Light.Red\n print(label(start))\n}\n";
        assert!(covers(src, "main"));
    }

    #[test]
    fn rejects_string_payload_enum() {
        // A String payload would need clone/borrow decisions at the literal site and
        // in pattern bindings the subset can't reproduce — excluded.
        let src = "enum Msg {\n Text(String)\n Ping\n}\nfn show(m: Msg) -> String {\n if m {\n m == Text(s) -> { return s }\n m == Ping -> { return \"ping\" }\n }\n return \"\"\n}\n";
        assert!(!covers(src, "show"));
    }

    #[test]
    fn rejects_recursive_enum() {
        // A self-referential enum needs a boxed payload — pattern/literal lowering
        // would need box/deref handling the subset avoids.
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn depth(t: Tree) -> Int {\n if t {\n t == Leaf(n) -> { return n }\n t == Node(inner) -> { return 1 }\n }\n return 0\n}\n";
        assert!(!covers(src, "depth"));
    }

    #[test]
    fn rejects_mixed_comparison_switch() {
        // A switch mixing a comparison/Bool arm with a range arm is the general
        // mixed-switch the subset does not cover (only all-range + else).
        let src = "fn f(n: Int) -> String {\n if n {\n 0..10 -> { return \"low\" }\n n > 100 -> { return \"high\" }\n else -> { return \"mid\" }\n }\n}\n";
        assert!(!covers(src, "f"));
    }

    // c109 Phase 5: collections. (Index/slice/index-assign coverage needs the
    // sema-resolved `IndexKind`, which `build_cx` alone does not fill, so those are
    // proven by the byte-parity check + `tests/tir.rs`; here we gate the
    // sema-independent constructs: list/map literals, list/map-typed params, and
    // collection iteration.)

    #[test]
    fn covers_list_literal_and_param() {
        // A list literal returned from a covered fn, and a list-typed param.
        let src = "fn build() -> [Int] {\n return [1, 2, 3]\n}\nfn accept(xs: [Int]) -> Int {\n return 0\n}\n";
        assert!(covers(src, "build"));
        assert!(covers(src, "accept"));
    }

    #[test]
    fn covers_map_literal_and_param() {
        // An empty and a non-empty map literal, plus a map-typed param.
        let src = "fn empty() -> [String, Int] {\n return [:]\n}\nfn one() -> [String, Int] {\n return [\"a\": 1]\n}\nfn accept(m: [String, Int]) -> Int {\n return 0\n}\n";
        assert!(covers(src, "empty"));
        assert!(covers(src, "one"));
        assert!(covers(src, "accept"));
    }

    #[test]
    fn covers_single_binding_iteration() {
        // `loop x in <list>` over a list-typed param is now covered (Phase 5).
        let src = "fn f(xs: [Int]) {\n loop x in xs {\n print(x)\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_two_binding_map_iteration() {
        // `loop k, v in <map>` (the two-binding map form) is covered.
        let src = "fn f(m: [String, Int]) {\n loop k, v in m {\n print(\"{k}={v}\")\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_method_call_collection_iteration() {
        // `loop c in s.chars()` iterates a method-call collection — a different
        // `emit_for_in` branch (char iteration) the subset does not reproduce.
        let src = "fn f(s: String) {\n loop c in s.chars() {\n print(c)\n }\n}\n";
        assert!(!covers(src, "f"));
    }

    #[test]
    fn rejects_list_of_option_param() {
        // A list whose element is an option (`[Int?]`) is not a covered value type
        // (optionals are Phase 8); the owning collection is excluded.
        let src = "fn f(xs: [Int?]) -> Int {\n return 0\n}\n";
        assert!(!covers(src, "f"));
    }

    // c109 Phase 6: methods + clones. (The gate paths that need a sema-resolved
    // `recv_type` are proven by the byte-parity check + `tests/tir.rs`; `build_cx`
    // alone does not fill `recv_type`. Here we gate the sema-independent facts:
    // covered method *signatures* are registered, and a covered function bodyless
    // of method calls is unaffected.)

    #[test]
    fn covers_struct_param_with_method_caller() {
        // A struct with a user method: the method body (has `self`) is excluded,
        // but a free function taking the struct and reading a scalar field is still
        // covered (Phase 3 baseline — methods don't disturb the existing coverage).
        let src = "struct Calc {\n base: Int\n fn add(self, x: Int) -> Int {\n return (self.base + x)\n }\n}\nfn peek(c: Calc) -> Int {\n return c.base\n}\n";
        assert!(covers(src, "peek"));
    }

    #[test]
    fn builtin_method_names_are_excluded() {
        // A user method whose NAME collides with a collection/string builtin
        // (`len`, `push`, `map`, …) is intercepted by `emit_builtin_method` on the
        // AST path — the TIR must NOT claim it. `is_intercepted_method_name` is the
        // guard; these names stay false regardless of the receiver being a user type.
        for name in [
            "len", "push", "pop", "get", "map", "filter", "each", "find", "sort",
            "join", "to_string", "clone", "raw", "snapshot", "new", "to_i32",
            "is_nan", "chars", "trim", "keys", "values",
        ] {
            assert!(
                is_intercepted_method_name(name),
                "{name} should be excluded (AST builtin/special lowering)"
            );
        }
        // A plain user method name is not intercepted.
        assert!(!is_intercepted_method_name("bumped"));
        assert!(!is_intercepted_method_name("combine"));
        assert!(!is_intercepted_method_name("code"));
    }

    // c109 Phase 7: method bodies + static methods.

    #[test]
    fn covers_instance_method_body() {
        // A `self` getter on a covered struct, body reading `self.field` — covered.
        // (Multi-letter type name; a single uppercase letter reads as a type var.)
        let src = "struct Cell {\n n: Int\n fn value(self) -> Int {\n return self.n\n }\n}\n";
        assert!(covers_method(src, "Cell", "value"));
    }

    #[test]
    fn covers_mut_self_method_body() {
        // A `mut self` receiver (→ `&mut self`) whose body only reads is covered.
        let src = "struct Acc {\n total: Int\n fn doubled(mut self) -> Int {\n return (self.total + self.total)\n }\n}\n";
        assert!(covers_method(src, "Acc", "doubled"));
    }

    #[test]
    fn covers_static_constructor() {
        // A static (no-`self`) associated function returning the owning type.
        let src = "struct Cell {\n n: Int\n fn make(v: Int) -> Cell {\n return Cell { n: v }\n }\n}\n";
        assert!(covers_method(src, "Cell", "make"));
    }

    #[test]
    fn covers_enum_instance_method() {
        // A `when self` match in an enum method body is covered.
        let src = "enum Dir {\n North\n South\n fn code(self) -> Int {\n if self {\n North -> { return 0 }\n South -> { return 1 }\n }\n }\n}\n";
        assert!(covers_method(src, "Dir", "code"));
    }

    #[test]
    fn rejects_self_reassignment_method() {
        // A `mut self` method that reassigns `self` (`self = …`) is a known AST-path
        // I2 hole (the self slot doesn't deref on the LHS) — the gate excludes it.
        let src = "struct Acc {\n n: Int\n fn reset(mut self) {\n self = Acc { n: 0 }\n }\n}\n";
        assert!(!covers_method(src, "Acc", "reset"));
    }

    #[test]
    fn rejects_generic_method() {
        // A method on a generic type isn't covered (no generics in the subset). The
        // owning type `Box<T>` is not a covered struct, so the gate bails.
        let src = "struct Box<T> {\n v: T\n fn get(self) -> T {\n return self.v\n }\n}\n";
        assert!(!covers_method(src, "Box", "get"));
    }

    #[test]
    fn rejects_intercepted_static_name() {
        // A static method named `new` collides with the alloc/special intercept
        // (`mem.*.new`) — the AST path special-cases the name, so the TIR static
        // call gate must NOT claim it. (The method body itself may still route, but
        // its *call* `Type.new()` stays on the AST path; here we check the body gate
        // is independent — `new` as a *static body* is still a plain method def.)
        // The static *call*-site exclusion is covered by `is_intercepted_method_name`.
        assert!(is_intercepted_method_name("new"));
    }

    // c109 Phase 8: fallible + optional.

    #[test]
    fn covers_fallible_return_and_try() {
        // A `T ? Error` return (default-error fallible) with `ok`/`err` over scalar
        // values and `?` propagation of a covered fallible call — all in-subset
        // (Phase 8). (`Error` lowers to `String`; the constructors here take a scalar
        // and a String literal, which parse as `Expr::Ok`/`Expr::Err` directly — no
        // sema EnumLit rewrite needed, so `build_cx` alone proves the gate. A
        // scalar-payload *error enum* literal is `Bad.Code(1)`, which parses as a
        // MethodCall and is only rewritten to an `EnumLit` by full sema; that path is
        // proven end-to-end by `tests/tir.rs::fallible_try_and_or_fallback`.)
        let src = "fn f(x: Int) -> Int ? Error {\n if x == 0 {\n return err(\"bad\")\n }\n return ok(x)\n}\nfn g(x: Int) -> Int ? Error {\n n @= f(x)?\n return ok((n + 1))\n}\n";
        assert!(covers(src, "f"));
        assert!(covers(src, "g"));
    }

    #[test]
    fn covers_optional_return_and_chaining() {
        // A `T?` return with `value`/`null`, plus `?.` chaining over a covered struct.
        // (Multi-letter struct name; a single uppercase letter reads as a type var.)
        let src = "struct Addr {\n city: String\n}\nfn opt(x: Int) -> (Int?) {\n if x > 0 {\n return value(x)\n }\n return null\n}\nfn ch(a: (Addr?)) -> (String?) {\n return a?.city\n}\n";
        assert!(covers(src, "opt"));
        assert!(covers(src, "ch"));
    }

    #[test]
    fn covers_or_fallback_value_and_return() {
        // `??` with a value fallback and with an early-`return` fallback.
        let src = "fn v(x: (Int?)) -> Int {\n return x ?? 0\n}\nfn r(x: (Int?)) -> Int {\n return x ?? return -1\n}\n";
        assert!(covers(src, "v"));
        assert!(covers(src, "r"));
    }

    #[test]
    fn rejects_or_fallback_panic_form() {
        // The `panic(…)` fallback form is deferred (its `safe_locals_expr`
        // reproduction is out of subset) — the owning fn stays on the AST path.
        let src = "fn p(x: (Int?)) -> Int {\n return x ?? panic(\"missing\")\n}\n";
        assert!(!covers(src, "p"));
    }

    #[test]
    fn rejects_string_payload_error_enum() {
        // A `T ? E` whose error enum has a String payload is excluded — the error
        // enum is not a covered (scalar-payload) enum, so its construction/binding
        // would need clone decisions the subset can't make.
        let src = "enum Oops {\n Msg(String)\n}\nfn f(x: Int) -> Int ? Oops {\n if x == 0 {\n return err(Oops.Msg(\"bad\"))\n }\n return ok(x)\n}\n";
        assert!(!covers(src, "f"));
    }
}
