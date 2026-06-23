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
    AccessConvention, BinOp, BindPattern, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt,
    IndexKind, Lambda, LambdaBody, LValue, OrFallback, Param, PatSlot, Pattern, Stmt, StrPart,
    SwitchArm, TryConvert, Type, UnOp, VariantPayload,
};
use crate::Diagnostics::Span;
use crate::Syntax;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
    /// c109 Phase 17: the function returns `-> view T` (a borrow). Drives
    /// `rust_return_type(cx, ret, is_view)` so the signature renders `&T`, and the body's
    /// returns lowered via `lower_view_return` (`TStmt::ViewReturn`).
    pub(crate) is_view: bool,
    /// c109 Phase 17: the rendered Rust generic clause (`<T: Clone>` / `<T, U>` / empty),
    /// resolved at lowering via `Generics::rust_type_param_list(&f.type_params, …)` exactly
    /// as `emit_func` does (with the `rust_extra_clone_bounds` every type param carries).
    /// Emitted verbatim after the function name; empty for a non-generic function.
    pub(crate) generics: String,
    pub(crate) is_main: bool,
    /// c109 Phase 18: an `#Unsafe fn` (S58, E2-M13/D-LL1) lowers to a Rust `unsafe fn`
    /// (the `unsafe ` keyword prefixes the signature), so the body may use gated pointer
    /// ops directly — calling it is already gated to an `#Unsafe` block in sema (E3103).
    /// I1: this is true ONLY when the source function was `#Unsafe fn`; no `unsafe` is
    /// ever emitted without that source gate. Applies to `TopLevel`/`Method`; a trait
    /// method carries its own `is_unsafe` on `TFuncKind::TraitMethod`.
    pub(crate) is_unsafe: bool,
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
    /// c109 Phase 12: a trait-impl method inside `impl Trait for user_<T> { … }` (the
    /// caller `emit_trait_impl`/`emit_external_trait_impl` opened the block). Distinct
    /// from an inherent `Method`: the method name is BARE (the trait owns it — no
    /// `user_` mangle) and there is NO `pub`. `self_conv` is the receiver convention
    /// (`Read`→`&self`, `Mutate`→`&mut self`, `Move`→`self`) — D-MUTSELF1: a `mut self`
    /// trait method gets `&mut self` and may mutate the receiver in place. `is_unsafe`
    /// reproduces the `unsafe fn` prefix for an `@unsafe fn` trait method (S58/D-LL1 —
    /// the body may use gated ops; calling it is already gated to an `@unsafe` block).
    TraitMethod { is_unsafe: bool, self_conv: AccessConvention },
    /// c109 Phase 15: a DELEGATION trait method (`using field`) — `emit_delegation_method`
    /// (Source/Codegen/Items.rs). The whole method is structural: a forwarding call
    /// `(self).<field>.<method>(<args>)` to the delegated field, with the BARE trait
    /// method name (no `user_` mangle). There is NO body to lower — the forward string is
    /// resolved at lowering. The signature reproduces `emit_delegation_method`'s exact
    /// shape (a quirky two-space `  {` before the brace, `&self` receiver, no `pub`).
    /// `has_return` decides whether the forward line ends in `;` (unit) or not (returns).
    /// `sig` is the fully-rendered signature line (`    fn name(params)  {\n` with its
    /// quirky double space) and `fwd` the forwarding call — both resolved at lowering.
    Delegation { sig: String, fwd: String, has_return: bool },
}

/// c109 Phase 17: how a `-> view T` return wraps its value, resolved at lowering from
/// the AST node shape (`emit_view_return`, Source/Codegen/Statement.rs):
///  - `Addr` — prefix `&` (an owned place whose address is taken: a non-deref ident, a
///    const, or a field read `&(<place>)`);
///  - `Bare` — emit the value as-is (an already-borrowed ident reads `name`, the deref'd
///    place stripped at lowering, OR a non-ident/field expr that `emit_view_return` passes
///    straight to `emit_expr`).
pub(crate) enum ViewWrap {
    Addr,
    Bare,
}

/// c109 Phase 22: the method-call-collection iteration form on a `loop x in <coll>`,
/// resolved at lowering from `emit_for_in`'s `Expr::MethodCall` branches
/// (Source/Codegen/Statement.rs). Each carries the receiver's emitted Rust string;
/// `file`/the panic line are program/source facts. The plain `.iter().cloned()` form
/// (incl. a non-special method-call collection like `.split(…)`, which `emit_for_in`
/// routes to its `else` default) is represented by `ForIn.method_kind == None`.
pub(crate) enum TForInMethod {
    /// `loop c in s.chars()` — char iteration: `for _jet_c in ({recv}).chars()`,
    /// binding `let <var> = _jet_c;`.
    Chars,
    /// `loop line in reader.lines()` on a `FileReader` — streaming `BufRead::lines`
    /// over the reader's `inner`, with a mid-stream-error panic (line `0`, `cx.file`).
    LinesFile,
    /// `loop line in io.stdin().lines()` / a `StdinHandle` — the same streaming read,
    /// but the receiver is materialised into a `_jet_stdin_h` local inside an extra
    /// block (so the `io.stdin()` temporary outlives the loop body), with a matching
    /// extra closing brace.
    LinesStdin,
}

/// c109 Phase 22: an `if` condition, resolved at lowering from the AST node shape
/// (`emit_if`/`if_pattern_test`, Source/Codegen/Statement.rs):
///  - `Plain` — a boolean expression: `if {cond} {`.
///  - `IfLet` — an optional-binding test (`x == value(b)` → `Some(b)`, `ok(b)`/`err(b)`,
///    a variant `c == Active(id)`): `if let {pat_str} = {subj} {`. The bound name(s)
///    are in scope in the then-branch (the binding's resolved type is bound at lowering,
///    mirroring `add_pattern_bindings`).
///  - `IsNone` — an `x == null` test (`Pattern::Absent`): `if {subj}.is_none() {`.
pub(crate) enum TIfCond {
    Plain(TExpr),
    IfLet { pat_str: String, subj: TExpr },
    IsNone { subj: TExpr },
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub(crate) enum TStmt {
    /// `let [mut] name[: ty] = init;`. All presentation facts are resolved at
    /// lowering, reproducing `emit_let` (Source/Codegen/Statement.rs) byte-for-byte:
    /// `kw` is `"let"` or `"let mut"` (the `mut` accounts for the source `mutable`
    /// flag AND the forced-mut cases — a handle binding FileReader/FileWriter/
    /// TcpStream/HttpRouter/Arena/… needs `let mut` even when bound immutably, and an
    /// escaping FnMut lambda binding); `ty_clause` is the rendered `": <type>"` (empty
    /// for an inferred binding; a Fn type renders via `rust_fn_trait`, others via
    /// `rust_type`). The binding's resolved type is carried on the `LowerEnv` slot (for
    /// downstream facts), so it is not duplicated on the node.
    Let {
        name: String,
        kw: &'static str,
        ty_clause: String,
        init: TExpr,
    },
    /// c109 Phase 23: a TUPLE-destructuring binding `(a, b) @= <init>` (S74,
    /// `BindPattern::Tuple`). Reproduces `emit_stmt`'s destructure form byte-for-byte:
    /// a `let {tmp} = &({init});` temp (borrowed — never moves out of a shared ref, I2),
    /// then one `let[ mut] {elem_rust} = ({tmp}).{field_rust}.clone();` per element,
    /// pairing the pattern's elements to the tuple type's CANONICAL fields by position
    /// (resolved at lowering off the init's total `Type::Tuple`). `tmp` is the
    /// `__jet_d{span}` name the AST uses (resolved at lowering); `kw` is `"let"`/`"let mut"`.
    TupleDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        /// `(elem_rust_name, field_rust_name)` per bound element, canonical order.
        binds: Vec<(String, String)>,
    },
    /// c109: a STRUCT-destructuring binding `Type { x, y } @= <init>` (S74,
    /// `BindPattern::Struct`). Reproduces `emit_stmt`'s `BindPattern::Struct` arm
    /// byte-for-byte: a `let {tmp} = &({init});` borrow temp, then one
    /// `let[ mut] {field_rust} = ({tmp}).{field_rust}.clone();` per bound field
    /// (the pattern's field name is BOTH the bound local and the struct field
    /// read, mangled once). The field's resolved type comes from `cx.struct_fields`
    /// (the init's `Type::Named`/`Apply` name), resolved at lowering for the slot.
    StructDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        /// `field_rust_name` per bound field, source order.
        fields: Vec<String>,
    },
    /// c109 Phase 26: a LIST-destructuring binding `[a, b, c] @= <init>` (S74,
    /// `BindPattern::List`). Reproduces `emit_stmt`'s `BindPattern::List` arm
    /// byte-for-byte: a `let {tmp} = &({init});` borrow temp, then one
    /// `let[ mut] {elem_rust} = jet_unpack_vec({tmp}, {want}, {i}, {file:?}, {line});`
    /// per element. `want` is the element count, `i` the position, and `file`/`line`
    /// the destructure span's source location (resolved at lowering for the
    /// bounds-mismatch panic). Each element binds a non-deref slot whose type
    /// reproduces `expr_jet_ty(init)`'s `Some(List(inner))` partiality (a non-`List`
    /// init — e.g. a `[T#N]` fan-out result — yields `None`, matching the AST).
    ListDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        want: usize,
        file: String,
        line: usize,
        /// `elem_rust_name` per bound element, source order.
        elems: Vec<String>,
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
    /// c109 Phase 17: a `return <e>` from a `-> view T` function (a borrow). Reproduces
    /// `emit_view_return` (Source/Codegen/Statement.rs) byte-for-byte: the returned value
    /// is the lowered expression `value`, wrapped per `wrap` (resolved at lowering from the
    /// AST node shape) — `&value` for a place that needs its address taken, the bare deref'd
    /// place for an already-borrowed ident, or `value` unwrapped. `emit` reads `wrap` only.
    ViewReturn { value: TExpr, wrap: ViewWrap },
    /// A call used for effect: `print(x);`, `helper(a);`.
    ExprStmt(TExpr),
    /// Statement-form `if`/`else`. `else_body` is `None` for a bare `if`.
    /// `cond` (c109 Phase 22) is a `TIfCond`: a plain boolean expr, an optional-binding
    /// `if let <pat> = <subj>` (an `x == value(b)`/`ok(b)`/`err(b)`/variant condition),
    /// or an `<subj>.is_none()` test (`x == null`) — reproducing `emit_if`'s three
    /// condition shapes (Source/Codegen/Statement.rs).
    /// `else_is_elseif` distinguishes the source `ElseBranch`: `true` for a real
    /// `else if` chain (`ElseBranch::ElseIf` — the else-body is the synthesised nested
    /// `If`, emitted as `} else if …`), `false` for an explicit `else { … }` block
    /// (`ElseBranch::Else`, emitted as `} else { … }` even when the block holds a
    /// single `if`). The AST path keys solely on the `ElseBranch` variant; the TIR
    /// must NOT flatten an explicit `else { if … }` into `else if` (a parity drift).
    If {
        cond: TIfCond,
        then_body: Vec<TStmt>,
        else_body: Option<Vec<TStmt>>,
        else_is_elseif: bool,
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
    /// c109 Phase 5/22: collection iteration `loop x in coll` / `loop k, v in map`
    /// (`Stmt::For` with `ForKind::In`). The collection's emitted Rust string is
    /// resolved at lowering. `var2` distinguishes the two-binding map form (which
    /// iterates `(coll).iter()` and clones each key/value) from the single-binding
    /// form (`(coll).iter().cloned()`), reproducing `emit_for_in` exactly.
    /// `method_kind` (c109 Phase 22) carries the method-call-collection iteration
    /// form (`.chars()` char iteration, `.lines()` streaming reads) resolved at
    /// lowering off the same `emit_for_in` branch; `None` is the plain `.iter()`
    /// form (incl. a non-special method-call collection like `.split(…)`, which the
    /// AST routes to the `.iter().cloned()` default). When `method_kind` is set the
    /// `collection_str` holds the *receiver* string (not the whole method call), and
    /// `var2` is always `None` (a method-call collection is single-binding only).
    ForIn {
        label: Option<String>,
        var: String,
        var2: Option<String>,
        collection_str: String,
        method_kind: Option<TForInMethod>,
        body: Vec<TStmt>,
    },
    /// c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema picked the
    /// branch (`selected_then`); the AST `emit_stmts` emits ONLY that branch's
    /// statements INLINE at the same indent (no `if`, no block — and its `let`s leak
    /// into the outer scope, exactly like a plain block). The TIR carries the lowered
    /// statements of the selected branch and emits them with no wrapper. When the
    /// selected branch is `else` but there is no else-body (or sema didn't resolve),
    /// this holds an empty vec (emits nothing).
    Inline(Vec<TStmt>),
    /// c109 Phase 15: a MIXED comparison/Bool `when` switch (`emit_mixed_switch`,
    /// Source/Codegen/Statement.rs) — the general `if/else if … else` form used when the
    /// arms are NOT all-variant (that is shape A, a Rust `match`), NOT all-range (shape
    /// B, `RangeSwitch`), and NOT all-fallible (shape C). Each arm head is a plain
    /// comparison/Bool expression. The AST path wraps the chain in a block that binds
    /// `_jet_switch_subject = &(subject)` (emitted for parity even when unused), then an
    /// `if/else if …` chain over each arm's condition, with the `else`/fallthrough form
    /// reproduced exactly. Each arm's condition is resolved to a Rust string at lowering
    /// (emit makes no decision). `else_body` is the optional `else` arm.
    MixedSwitch {
        subject_str: String,
        arms: Vec<(String, Vec<TStmt>)>,
        else_body: Option<Vec<TStmt>>,
    },
    /// c109 Phase 18: an audited `#Unsafe { … }` gate region (`Stmt::Unsafe`, S58,
    /// E2-M13/D-LL1). The AST `emit_stmts` lowers it straight to a Rust `unsafe { … }`
    /// block; the `#Audit("…")` annotation (the `audit` field) emits NOTHING (codegen is
    /// dumb — sema validated the audit). I1: this TIR node exists ONLY for a source
    /// `#Unsafe` region, so the emitted `unsafe { … }` is always 1:1 with a source gate.
    /// The body's `let`s LEAK into the outer scope (the AST shares `&mut env`), so the
    /// body is lowered on the SAME `LowerEnv` (not a cloned scope).
    Unsafe(Vec<TStmt>),
    /// c109 Phase 19: an explicit `region r { … }` (D-REGION1 opt B). Lowers to a plain
    /// Rust block `{ … }` — a lexical scope. The region's escape bound (E0631) and arena
    /// drop ordering (S63 RAII) are enforced entirely in sema; codegen is dumb (I3). The
    /// body's `let`s LEAK into the outer scope (the AST shares `&mut env`), so the body is
    /// lowered on the SAME `LowerEnv`.
    Region(Vec<TStmt>),
    /// c109 Phase 19: a `#Context(field: value) { … }` smart-context block (D-CTX1). Lowers
    /// to a plain block with one RAII guard per field (in declaration order) BEFORE the
    /// body: an `allocator` field → `let _ctx_guard_<i> = jet_mem::jet_ctx_push_alloc(&<v>);`
    /// (a safe fn — the unsafe cast is inside the vetted jet_mem zone, I1 holds); any other
    /// field (logger) → `let _ctx_logger_<i> = <v>;` (v1 no-op; the value still runs). Each
    /// `(is_allocator, value)` guard is resolved at lowering. The body leaks like a region.
    ContextBlock {
        guards: Vec<(bool, TExpr)>,
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
    /// c109 Phase 24: a comptime CONST ident inlined at the use site. Carries the
    /// pre-rendered Rust value string (`cx.consts[name]`, total — the same string
    /// `emit_expr`'s `Ident` arm splices), so emit just emits it verbatim. The const's
    /// `TExpr.ty` is a placeholder (never read — a const operand resolves to `None` in
    /// `ast_operand_is_integer`, so it never enables the overflow trap, and a covered
    /// const use — interpolation, a binding RHS — reads the binding/`.jet_show()` type,
    /// not this).
    ConstInline(String),
    /// Call to a plain top-level function. Each arg carries its emit decisions.
    Call { name: String, args: Vec<TCallArg> },
    /// `print(x)` — the one builtin the subset covers.
    Print(Box<TExpr>),
    /// c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). A bare call
    /// (no module alias) lowering to `{root}jet_std_io_input(None|Some(&(prompt)))`,
    /// byte-for-byte the `emit_call` ambient-input branch (Source/Codegen/Expression.rs
    /// ~L1778). `prompt` is `Some` when a String prompt arg is given, else `None`.
    AmbientInput { prompt: Option<Box<TExpr>> },
    /// c109 Phase 26: a `require(cond[, msg])` / `require_eq(a, b)` / `panic(msg)`
    /// rich-runtime-report builtin (S36). The ENTIRE emit string (`{ if !(cond) {
    /// jet_panic_rich(…); } }` in the default build, or the `test_mode` `{ if !(cond) {
    /// return Err(…); } }` form) is rendered at lowering — byte-for-byte
    /// `emit_require`/`emit_require_eq`/`emit_panic_stop` (Source/Codegen/Statement.rs).
    /// Every input (the source line / col / caret, the escaped file + fn name, the
    /// sorted scalar-locals snapshot via `render_safe_locals`, the test-mode flag) is
    /// total at lowering, so emit reads nothing from `cx.src`/`cx.current_fn` (I3).
    RequireStop(String),
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
        /// Each field carries its mangled Rust name, its value expression, and a
        /// `boxed` flag. c109: a self-referential struct field has Rust type `Box<…>`
        /// (`cx.boxed_edges`), so its construction value must be wrapped `Box::new(…)`
        /// (E0308 otherwise) — exactly as `emit_struct_lit` does on the AST path. The
        /// flag is resolved at lowering (a total fact), never re-derived in emit.
        fields: Vec<(String, TExpr, bool)>,
        /// c109 Phase 17: an extra raw field line appended verbatim after the user fields
        /// (e.g. HttpRequest's injected `params: std::collections::BTreeMap::new()`).
        /// `None` for a plain user struct.
        extra: Option<String>,
        /// c109 Phase 30: a TRAIT-OBJECT coercion (`Circle {…}` in a `[Shape]` list, S48).
        /// When `Some(trait_rust)` — the already-resolved `Generics::user_trait_rust` name —
        /// the whole literal is wrapped `Box::new({lit}) as Box<dyn {trait_rust}>`, exactly as
        /// `emit_struct_lit`'s `as_trait` branch. `None` for a non-coerced literal.
        as_trait: Option<String>,
    },
    /// c109 Phase 3: a struct field *read* `recv.field` in borrow position. The
    /// AST path never derefs/clones a plain field read (Rust reads the place;
    /// owning reads were already rewritten to a `.clone()` MethodCall in sema and
    /// are excluded from the subset). `field_rust` is the mangled field name.
    /// `boxed` is set for a self-referential (recursive) edge — its Rust type is
    /// `Box<…>`, so the read is wrapped `(*(…))` to deref to the inner type, exactly
    /// as the AST `boxed_field_read` (Expression.rs). The flag is total (resolved
    /// from `cx.boxed_edges` at lowering — never re-derived in emit, per I3).
    Field {
        recv: Box<TExpr>,
        field_rust: String,
        boxed: bool,
    },
    /// c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (`Expr::PtrFromAddr`, S58, E2-M13).
    /// Builds a raw `*mut T` from an integer address. The cast itself is safe in Rust
    /// (only *using* the pointer needs `unsafe`, supplied by the surrounding `#Unsafe`
    /// region/fn), so this introduces no `unsafe` by itself. `elem_rust` is the already
    /// resolved Rust element type (`cx.rust_type(elem)`); `addr` is the address expr.
    /// Reproduces `emit_expr`'s `PtrFromAddr` arm: `(({addr}) as usize as *mut {elem})`.
    PtrFromAddr {
        elem_rust: String,
        addr: Box<TExpr>,
    },
    /// c109 Phase 19: an arena allocator constructor `mem.Arena.new([capacity: N])`
    /// (D-ALLOC1). The receiver is `Field(Ident(mem-alias), <AllocType>)` with method
    /// `new`. `rust_type` is the resolved `jet_mem::Jet<Alloc>` head; `ctor` is the fully
    /// rendered constructor call tail (`::new()` or `::with_capacity(N as usize)` /
    /// `::with_slots(...)` / `::with_size(...)`), reproducing `emit_method_call`'s arena
    /// constructor branch (Expression.rs ~L1515) byte-for-byte. The allocator's only
    /// `unsafe` lives in the vetted `jet_mem` prelude (I1 scan excludes it).
    AllocNew {
        ctor: String,
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
    /// c109 Phase 24: a prelude `JSON` enum construction (`JSON.Null` /
    /// `JSON.Boolean(b)` / `JSON.Number(n)` / `JSON.Text(s)` / `JSON.Array(xs)` /
    /// `JSON.Object(map)`). The JSON enum is FOREIGN: its variants render non-mangled
    /// (`{root}jet_std::Json::Object`, NOT `user_…`), distinct from a user enum's
    /// `EnumLit`. `variant` is the bare variant name (`Object`/`Text`/…). `arg` is the
    /// payload `TExpr` plus the resolved `implicit_clone` flag (sema's `CallArg.flags`,
    /// total) — `true` → `(…).clone()`, reproducing `emit_core_json_lit` (Expression.rs)
    /// byte-for-byte. `JSON.Null` has no arg (`None`). The `{root}jet_std::Json` prefix
    /// is rendered at emit (`cx.root_prefix` is program-level, read there).
    JsonLit {
        variant: String,
        arg: Option<Box<(TExpr, bool)>>,
    },
    /// c109 Phase 5: a list literal `[a, b, c]`. Lowers to Rust `vec![…]`. Each
    /// element is lowered as-is (the AST path applies no clone/coercion at the
    /// literal site — `emit_expr` per element).
    ListLit(Vec<TExpr>),
    /// c109 Phase 23: a named-tuple literal `(x: 1, y: 2)` (S73/D-SG7). The generated
    /// struct name (`JetTup_<hash>`) and the CANONICAL field order are resolved at
    /// lowering from the literal's sema-attached `Type::Tuple`; each field's value is
    /// reordered to that canonical order (a `(y: 3, x: 4)` literal becomes
    /// `JetTup_…{ user_x: 4, user_y: 3 }`). Reproduces `emit_expr`'s `TupleLit` arm
    /// byte-for-byte — `struct_name { user_<f>: <v>, … }`. `fields` are the already
    /// mangled-name + lowered-value pairs in canonical order.
    TupleLit {
        struct_name: String,
        fields: Vec<(String, TExpr)>,
    },
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
    /// c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)` where `step`
    /// is a `fn(...)` FIELD (not a user method). Emits `(({recv}).{field_rust})({args})`,
    /// byte-for-byte the AST `emit_method_call` fn-field branch (Expression.rs ~L1573).
    /// `field_rust` is the mangled `user_<field>`; args emit PLAINLY (the AST passes
    /// `None` to `emit_call_args` — no param convention, only each arg's own clone flags).
    FnFieldCall {
        recv: Box<TExpr>,
        field_rust: String,
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
    /// c109 Phase 9: a built-in collection/string method (`emit_builtin_method`).
    /// The receiver-type dispatch (`expr_jet_ty(receiver)` → Map/List/String) is
    /// resolved at lowering into a concrete `op`, so emit makes no type decision
    /// (I3). The args are lowered as PLAIN expressions — `emit_builtin_method`
    /// emits each arg via a raw `emit_expr`, with NO clone/borrow convention
    /// wrappers (unlike `emit_call_args`), so the TIR carries no `TCallArg` here.
    BuiltinMethod {
        recv: Box<TExpr>,
        op: TBuiltinOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 10: a core/stdlib module call `alias.method(args)` where `alias`
    /// is a core import (`cx.core_imports`). The `(module, method)` dispatch in
    /// `emit_core_call` (Source/Codegen/Expression.rs) is a pure syntactic match on
    /// two already-resolved strings — NO type inference (I3) — so the TIR carries
    /// `module`/`method` as resolved strings and the emitter reproduces the match
    /// byte-for-byte. The args are lowered as PLAIN expressions: `emit_core_call`'s
    /// `arg(i)` is a raw `emit_expr`, ignoring `CallArg.flags` and the param
    /// convention; the per-arm `&(…)`/`&mut (…)`/move wrappers are baked into each
    /// emit arm, not a TIR field. `cx.root_prefix`/`cx.ffi_crate` are program-level
    /// (read at emit, like Phase 9's `cx.file`), never a per-node decision.
    CoreCall {
        module: String,
        method: String,
        args: Vec<TExpr>,
    },
    /// `if`-expression form (S68 / D-SG2). Both arms are value blocks.
    IfExpr {
        cond: Box<TExpr>,
        then_body: Vec<TStmt>,
        then_value: Box<TExpr>,
        else_body: Vec<TStmt>,
        else_value: Box<TExpr>,
    },
    /// c109 Phase 23: a `#Todo` typed hole (`Expr::Todo`, D-TOOL2, E2-M11). Emits a
    /// diverging `todo!("#Todo at {file}:{line} — expected {ty}")` (Expression.rs
    /// `Expr::Todo`). The `expected_type` is the TOTAL sema fact (sema fills it onto
    /// the AST node); `line` is the source line resolved at lowering. `cx.file` is
    /// program-level (read at emit, like every other `cx.file` use). `todo!()` is
    /// diverging in Rust so it type-checks in any expression position (I1: no unsafe).
    Todo { line: usize, expected_type: String },
    /// c109 Phase 23: `.raw()` on a distinct type (`Expr::MethodCall { method: "raw" }`,
    /// D-DIST3). The AST `emit_method_call` special-cases this BEFORE any user dispatch:
    /// `({recv}).0` (the newtype's inner field). The receiver is lowered as-is; the
    /// result `ty` is the distinct base type (total, read from `cx.distinct_types`).
    DistinctRaw(Box<TExpr>),
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
    /// c109 Phase 11: a lambda/closure literal (`Expr::Lambda`). Every capture/
    /// escape/Fn-vs-FnMut decision is the TOTAL sema fact (`Lambda.meta`), resolved
    /// at lowering — emit reads them, never recomputes capture analysis (I3). The
    /// `prep` holds the per-`cloned_capture` `let _jet_cap_<n> = (place).clone();`
    /// prelude (resolved from the *outer* env at lowering, since the cap's source
    /// place is an outer local); `params` is the already-rendered `name[: ty]` list;
    /// `body` is the lowered closure body; `is_move`/`boxed` reproduce the AST path's
    /// `move ` keyword (off `needs_fn_mut`/`escapes`) and `Box::new(…)` (off `escapes`)
    /// wrappers. The whole thing is wrapped in `{ <prep> <closure> }` when `prep` is
    /// non-empty — byte-for-byte `emit_lambda` (Source/Codegen/Expression.rs).
    Lambda(Box<TLambda>),
    /// c109 Phase 11: the fan-out operator `f.[a, b, c]` ≡ `[f(a), f(b), f(c)]`
    /// (S75/S76 — result `[T#N]`, erased to `Vec`). `calls` are the already-lowered
    /// per-item call expressions (a `Call`/`Print`/`CallValue` form, resolved at
    /// lowering exactly as the AST path routes an `Ident` callee through `emit_call`
    /// and any other callee through `(f)(item)`). Emit just wraps them in `vec![…]`.
    FanOut { calls: Vec<TExpr> },
    /// c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/
    /// `find`/`any`/`all`/`sort_by`/`reduce`). The receiver-type + Fn-vs-FnMut
    /// dispatch (`emit_builtin_method`'s closure arms) is resolved at lowering into a
    /// concrete `op`; emit only formats. `recv` is the lowered receiver, `args` the
    /// lowered closure arg(s) (a `reduce` carries the seed first, then the lambda) —
    /// emitted PLAINLY, exactly as `emit_builtin_method`'s `arg(i)`.
    ClosureMethod {
        recv: Box<TExpr>,
        op: TClosureOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 12: a numeric predicate / bit-population / width-conversion method
    /// (D-NUMOPS1: `is_nan`/`count_ones`/`to_i32`/…) on a numeric receiver. These
    /// carry `recv_type == Some(<numeric name>)` (sema sets it for numeric receivers
    /// — CheckerInfer ~L2248). The receiver width source/target and the
    /// widening-vs-narrowing decision are resolved at lowering into a total
    /// `TNumericOp` (reproducing `numeric_conversion`/`conv_rust_target` exactly), so
    /// emit makes no type decision (I3). No args (all numeric methods are nullary).
    NumericMethod {
        recv: Box<TExpr>,
        op: TNumericOp,
    },
    /// c109 Phase 28: an overflow opt-out builtin `wrapping(e)`/`saturating(e)`/
    /// `checked(e)` (D-NUMOPS1). The AST `emit_call` (Source/Codegen/Expression.rs
    /// ~L1756) lowers the single integer `Expr::Binary` argument to Rust's matching
    /// method: `(lhs).{prefix}_{op}(rhs)` where `prefix ∈ {wrapping, saturating,
    /// checked}` and `op ∈ {add, sub, mul, div}`. PLAIN operands (no overflow trap).
    /// `prefix` + `op` are resolved at lowering (total facts), emit only assembles.
    OverflowOpt {
        prefix: String,
        op: &'static str,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// c109 Phase 13: a method ON a handle (FileReader/FileWriter/StdinHandle/
    /// Stopwatch/TcpListener/TcpStream/HttpRequest/HttpResponse) — the handle arms of
    /// `emit_builtin_method` (Source/Codegen/Expression.rs). The handle-receiver
    /// dispatch (`rty == Some(Named(<handle>))`) is resolved at lowering into a total
    /// `THandleOp`, so emit makes no type decision (I3). Args are emitted PLAINLY
    /// (`emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    HandleMethod {
        recv: Box<TExpr>,
        op: THandleOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 13: a closure-taking core/stdlib call — `tasks.spawn`,
    /// `http.serve`, `scope.guard`. These are NOT in `core_fixed_sig` and each has a
    /// bespoke emit shape (`emit_core_call`, Source/Codegen/Expression.rs) the plain
    /// `CoreCall` cannot reproduce: `spawn` wraps a `emit_spawn_lambda` (`move |…|`,
    /// NEVER `Box::new`) in `JetTask::spawn(…)`; `serve` (lambda handler) emits
    /// `jet_http_serve(&(addr), <lambda>)`; `guard` emits `jet_scope_guard(<lambda>)`.
    /// The closure body is lowered + rendered at lowering (the lambda is in subset —
    /// Phase 11), so emit only assembles. `kind` selects the bespoke shape.
    CoreClosureCall {
        kind: TCoreClosureKind,
    },
    /// c109 Phase 13: a fn-typed-VALUE form. Either a bare function name used as a
    /// value (`Expr::Ident` resolving to a top-level fn) or a call THROUGH a fn-value
    /// (`Expr::CallValue` — `(f)(args)`). A bare fn-name value emits the
    /// `Box::new(move |…| name(…)) as <fn-type>` wrapper (`emit_named_fn_value`,
    /// Source/Codegen/Statement.rs), resolved at lowering into `wrapper`. A
    /// `CallValue` emits `({callee})({args})` with the args lowered PLAINLY (the AST
    /// `Expr::CallValue` passes `None` to `emit_call_args` → no clone/borrow/convention
    /// wrappers). `kind` selects the form.
    FnValue {
        kind: TFnValueKind,
    },
    /// c109 Phase 14: a cross-module function call. The various module-call forms
    /// (qualified `mod.fn()` via `import_mods`, `pub use` re-exports via
    /// `reexport_calls`, inline code modules via `code_modules`, and the unqualified
    /// inline/file imports in `emit_call`) all resolve at LOWERING to a fully-decided
    /// `TModuleCallForm` — emit makes no table lookup or decision (I3). `args` carry
    /// their borrow/clone wrappers, resolved exactly as `emit_call_args` does from the
    /// callee's import signature. `cx.root_prefix` is the only program-level value the
    /// emitter reads (like Phase 9/10's `cx.file`/`cx.root_prefix`), placed exactly
    /// where the AST path prepends it.
    ModuleCall {
        form: TModuleCallForm,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 14: an FFI extern call (`extern rust`/`extern C`). `emit_call`'s
    /// `extern_funcs` arm emits `{ffi_crate}::{wrapper}(args)` with args lowered via
    /// `emit_extern_call_args` (a DISTINCT arg form — a non-scalar `Read` param is
    /// `(…).clone()`, NOT `&(…)`). `wrapper` is the resolved FFI symbol; `args` carry
    /// the resolved per-arg clone decision. `cx.ffi_crate` is program-level (read at
    /// emit, like Phase 10's regex form). I1: an extern call introduces no Rust
    /// `unsafe` by itself — this reproduces the AST emit byte-for-byte, which emits no
    /// `unsafe`.
    ExternCall {
        wrapper: String,
        args: Vec<TExternArg>,
    },
}

/// c109 Phase 14: a resolved cross-module call form. Each variant pre-resolves the
/// path pieces of one `emit_call`/`emit_method_call` module-call arm; emit prepends
/// `cx.root_prefix` exactly where the AST path does (or omits it where the AST does).
pub(crate) enum TModuleCallForm {
    /// `import_mods` qualified call (`mod.fn()`) and `reexport_calls` (`pub use`) —
    /// both emit `{root}{rust_mod}::{rust_fn}(args)`. `rust_mod` is the resolved Rust
    /// module name (`user_<stem>`); `rust_fn` is the mangled function name.
    Qualified { rust_mod: String, rust_fn: String },
    /// `code_modules` qualified call (`alias.method()`) and unqualified inline import —
    /// both emit `{root}user_{mangled}(args)` where `mangled` is `alias__method`.
    InlineMangled { mangled: String },
}

/// c109 Phase 14: a resolved FFI extern call argument (see `TExprKind::ExternCall`).
/// `emit_extern_call_args` wraps the value in `(…).clone()` when the arg has an
/// `implicit_clone` flag OR its param is a non-scalar `Read` (resolved here into one
/// total `clone` bool; the `shared_auto_clone`/Arc form is excluded from the subset).
pub(crate) struct TExternArg {
    pub(crate) value: TExpr,
    pub(crate) clone: bool,
}

/// c109 Phase 13: the three closure-taking core-call shapes (see
/// `TExprKind::CoreClosureCall`). Each holds the already-rendered closure string
/// (`spawn_closure` is the distinct `emit_spawn_lambda` form; `serve`/`guard` use the
/// plain `emit_lambda` form) plus, for `serve`, the lowered address arg.
pub(crate) enum TCoreClosureKind {
    /// `tasks.spawn(<lambda>)` → `{root}jet_std::JetTask::spawn(<spawn_closure>)`.
    Spawn { spawn_closure: String },
    /// `http.serve(addr, <lambda>)` → `{root}jet_http_serve(&(<addr>), <closure>)`.
    Serve { addr: Box<TExpr>, closure: String },
    /// `scope.guard(<lambda>)` → `{root}jet_scope_guard(<closure>)`.
    Guard { closure: String },
}

/// c109 Phase 13: the two fn-typed-value forms (see `TExprKind::FnValue`).
pub(crate) enum TFnValueKind {
    /// A bare function name used as a value. `wrapper` is the already-rendered
    /// `Box::new(move |…| user_<name>(…)) as <fn-type>` string (`emit_named_fn_value`),
    /// produced at lowering so emit only echoes it.
    NamedFn { wrapper: String },
    /// A call through a fn-value `(f)(args)`. `callee` lowers to its place (a local
    /// of `Type::Fn`, or another fn-value form); args are lowered plainly.
    Call { callee: Box<TExpr>, args: Vec<TCallArg> },
}

/// c109 Phase 12: a resolved numeric method form, one per `emit_builtin_method`
/// numeric arm (Source/Codegen/Expression.rs). The width source/target and the
/// widening-vs-narrowing branch (which `numeric_conversion` decides from the source
/// width name) are decided ONCE at lowering — the variant encodes the chosen form so
/// emit only formats.
pub(crate) enum TNumericOp {
    /// `is_nan`/`is_infinite`/`is_finite` → `({recv}).{method}()` (bool).
    Predicate(String),
    /// `count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros` →
    /// `(({recv}).{method}() as i64)` (Rust returns u32 → widen to Int).
    BitCount(String),
    /// A widening / float-targeted / float-sourced conversion → `(({recv}) as {dst})`.
    CastAs { dst_rust: String },
    /// An integer-narrowing conversion → the checked `<{dst}>::try_from(...)` form
    /// returning `Result<T, String>`. Both strings resolved at lowering.
    TryFrom { dst_rust: String, dst_spelling: String },
    /// `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string`
    /// arm of `emit_builtin_method`, which fires for any receiver type).
    ToShow,
}

/// c109 Phase 11: a resolved closure-taking collection-method op, one per
/// `emit_builtin_method` closure arm (Source/Codegen/Expression.rs). The
/// receiver-type branch (Map vs list vs trait-object list) and the Fn-vs-FnMut
/// branch (off the lambda arg's `needs_fn_mut` meta) are decided ONCE at lowering;
/// the variant encodes the chosen form so emit only formats.
pub(crate) enum TClosureOp {
    /// `map` on a list — `jet_list_map((recv).clone(), f)`.
    Map,
    /// `map` on a list whose lambda is FnMut — `jet_list_map_mut((recv).clone(), f)`.
    MapMut,
    /// `filter` — `jet_list_filter((recv).clone(), f)`.
    Filter,
    /// `each` on a list — `jet_list_each((recv).clone(), f)`.
    Each,
    /// `each` on a list whose lambda is FnMut — `jet_list_each_mut((recv).clone(), f)`.
    EachMut,
    /// `each` on a list of trait objects — `jet_list_each_ref(&(recv), f)`.
    EachRef,
    /// `each` on a map — `jet_map_each((recv).clone(), f)`.
    EachMap,
    /// `find` — `jet_list_find((recv).clone(), f)`.
    Find,
    /// `any` — `jet_list_any((recv).clone(), f)`.
    Any,
    /// `all` — `jet_list_all((recv).clone(), f)`.
    All,
    /// `sort_by` — `{ jet_list_sort_by(&mut recv, f); }`.
    SortBy,
    /// `reduce` — `jet_list_reduce((recv).clone(), seed, f)`.
    Reduce,
}

/// c109 Phase 11: a fully-resolved lambda/closure, every fact carried total from
/// `Lambda.meta`. `prep` is the rendered clone-capture prelude (`let _jet_cap_<n> =
/// (place).clone();\n    ` per cloned capture); `params` the rendered `name[: ty]`
/// param list; `body` the rendered closure body string (an expression body, or a
/// `{ … }` block) — rendered at lowering from the lowered body so emit stays a pure
/// wrapper; `is_move`/`boxed` reproduce the AST wrappers.
pub(crate) struct TLambda {
    pub(crate) prep: String,
    pub(crate) params: Vec<String>,
    pub(crate) body: String,
    pub(crate) is_move: bool,
    pub(crate) boxed: bool,
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

/// c109 Phase 8: the resolved right-hand side of a `??` fallback (`AST::OrFallback`).
/// `Value` is an expression; `Return` is an early `return [expr]` from the enclosing
/// function. c109 Phase 15: `Panic` reproduces `emit_panic_stop`/`safe_locals_expr`
/// (the `a ?? panic(…)` form) — all of its inputs (the panic message, source line,
/// column, caret width, function name, file, and the sorted scalar-locals snapshot)
/// are resolved at lowering into a single pre-rendered Rust string, so emit makes no
/// decision (I3) and never reaches into `cx.src`/`cx.current_fn` for it.
pub(crate) enum TOrFallback {
    Value(Box<TExpr>),
    Return(Option<Box<TExpr>>),
    /// The fully-rendered `{ jet_panic_rich(…); }` statement string, resolved at
    /// lowering — byte-identical to `emit_panic_stop`'s output. The interpolated panic
    /// message (which itself may contain lowered sub-expressions) and the locals
    /// snapshot are baked in here.
    Panic(String),
}

pub(crate) enum TStrPart {
    Lit(String),
    Interp(TExpr),
}

/// c109 Phase 4/16: the resolved payload shape of an enum literal.
pub(crate) enum TEnumPayload {
    /// `Enum.Variant` — no payload, emits just the prefix.
    Unit,
    /// `Variant(a, b, …)` — positional payload values, emitted as `prefix(a, b)`.
    Positional(Vec<TEnumArg>),
    /// `Variant { f: v, … }` — named payload, emitted as `prefix { f: v, … }`.
    /// Each field's Rust name is already mangled at lowering.
    Named(Vec<(String, TEnumArg)>),
}

/// c109 Phase 16: one enum-literal payload argument with its resolved
/// borrow/box decisions. Reproduces `emit_boxed_enum_arg` (Expression.rs) as a
/// TOTAL fact decided at lowering: a non-scalar payload field whose value is a
/// borrowed-in-env ident gets `(…).clone()`; a recursive (`boxed_edge`) payload
/// gets `Box::new(…)`. For a scalar payload from a non-borrowed value both are
/// false (the Phase-4 no-op case), so emit is byte-identical.
pub(crate) struct TEnumArg {
    pub(crate) value: TExpr,
    /// Wrap the value in `(…).clone()` (non-scalar payload, borrowed-in-env arg).
    pub(crate) clone: bool,
    /// Wrap (after the clone) in `Box::new(…)` — a recursive boxed edge.
    pub(crate) boxed: bool,
}

/// c109 Phase 9: a resolved built-in collection/string method op. Each variant is
/// one emit form from `emit_builtin_method` (Source/Codegen/Expression.rs). The
/// receiver-type dispatch (`rty = expr_jet_ty(receiver)` → Map vs List vs String)
/// is decided ONCE at lowering — the variant encodes the chosen branch, so emit
/// only formats. Line numbers (for the bounds/remove panic frames) are resolved at
/// lowering; `cx.file`/`cx.root_prefix` are read at emit (program-level, not a
/// per-node decision). Args are emitted plainly (no clone/borrow wrappers), exactly
/// as `emit_builtin_method`'s `arg(i)` does.
pub(crate) enum TBuiltinOp {
    /// `len` on a `String` → `jet_char_len(&(recv))` (char count, not byte len).
    LenString,
    /// `len` on a list/map → `(recv).len() as i64`.
    LenList,
    /// `is_empty()` on a list/map/string → `(recv).is_empty()` (Bool).
    IsEmpty,
    /// `push(x)` → `(recv).push(a0)`.
    Push,
    /// `pop()` → `(recv).pop()`.
    Pop,
    /// `insert(k, v)` on a map → `(recv).insert((a0).clone(), a1)`.
    InsertMap,
    /// `insert(i, v)` on a list → `(recv).insert(a0 as usize, a1)`.
    InsertList,
    /// `remove(k)` on a map → `(recv).remove(&(a0).clone())`.
    RemoveMap,
    /// `remove(i)` on a list → `jet_list_remove(&mut (recv), a0, file, line)`.
    RemoveList { line: usize },
    /// `get(k)` on a map → `(recv).get(&(a0).clone()).cloned()`.
    GetMap,
    /// `get(i)` on a list → `(recv).get(a0 as usize).cloned()`.
    GetList,
    /// `first()` → `(recv).first().cloned()`.
    First,
    /// `last()` → `(recv).last().cloned()`.
    Last,
    /// `contains(x)` → `(recv).contains(&a0)` (list element / String substring).
    Contains,
    /// `index_of(x)` → `(recv).iter().position(|x| *x == a0).map(|i| i as i64)`.
    IndexOf,
    /// `reverse()` → `(recv).reverse()`.
    Reverse,
    /// `sort()` (no comparator) → `(recv).sort()`.
    Sort,
    /// `join(sep)` → `(recv).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join((a0).as_str())`.
    JoinSep,
    /// `clear()` → `(recv).clear()`.
    Clear,
    /// `chars()` → `(recv).chars().collect::<Vec<char>>()`.
    Chars,
    /// `bytes()` → `{root}jet_string_bytes(&(recv))`.
    Bytes,
    /// `trim()` → `(recv).trim().to_string()`.
    Trim,
    /// `split(sep)` → `jet_string_split(&(recv), &a0)`.
    Split,
    /// `starts_with(s)` → `(recv).starts_with(&a0)`.
    StartsWith,
    /// `ends_with(s)` → `(recv).ends_with(&a0)`.
    EndsWith,
    /// `replace(from, to)` → `(recv).replace(&a0, &a1)`.
    Replace,
    /// `to_upper()` → `(recv).to_uppercase()`.
    ToUpper,
    /// `to_lower()` → `(recv).to_lowercase()`.
    ToLower,
    /// `repeat(n)` → `(recv).repeat(a0 as usize)`.
    Repeat,
    /// `slice(a, b)` → `jet_string_slice(&(recv), a0, a1, file, line)`.
    Slice { line: usize },
    /// `keys()` → `(recv).keys().cloned().collect::<Vec<_>>()`.
    Keys,
    /// `values()` → `(recv).values().cloned().collect::<Vec<_>>()`.
    Values,
    /// `contains_key(k)` → `(recv).contains_key(&a0)`.
    ContainsKey,
    /// `to_string()` (on a String receiver) → `(recv).jet_show()`.
    ToString,
    /// c109 Phase 24: `Match.group(n)` (D-REGEX1) → `(recv).get((a0) as usize).cloned().
    /// flatten()`. The `Match` renders to `Vec<Option<String>>`; `group` is plain
    /// indexing into it (the result is `String?`).
    MatchGroup,
}

/// c109 Phase 13: a resolved handle-method op, one per handle arm of
/// `emit_builtin_method` (Source/Codegen/Expression.rs). The handle-receiver branch
/// (keyed on `rty == Some(Named(<handle>))`) is decided ONCE at lowering from the
/// total `recv_type` — emit only formats. Args are emitted plainly (raw `arg(i)`).
/// `{root}` denotes `cx.root_prefix` (program-level, read at emit).
pub(crate) enum THandleOp {
    /// FileReader: `read_line()` → `{root}jet_std_file_reader_read_line(&mut (recv))`.
    FileReaderReadLine,
    /// FileWriter: `write_line(s)` → `{root}jet_std_file_writer_write_line(&mut (recv), &(a0))`.
    FileWriterWriteLine,
    /// FileWriter: `flush()` → `{root}jet_std_file_writer_flush(&mut (recv))`.
    FileWriterFlush,
    /// StdinHandle: `read_line()` → `{root}jet_std_io_stdin_read_line(&mut (recv))`.
    StdinReadLine,
    /// Stopwatch: `elapsed_millis()` → `{root}jet_stopwatch_elapsed_millis(&(recv))`.
    StopwatchElapsedMillis,
    /// TcpListener: `accept()` → `{root}jet_net_tcp_accept(&(recv))`.
    TcpListenerAccept,
    /// TcpListener: `local_addr()` → `{root}jet_net_listener_local_addr(&(recv))`.
    TcpListenerLocalAddr,
    /// TcpStream: `read()` → `{root}jet_net_tcp_read(&mut (recv))`.
    TcpStreamRead,
    /// TcpStream: `write(s)` → `{root}jet_net_tcp_write(&mut (recv), &(a0))`.
    TcpStreamWrite,
    /// TcpStream: `peer_addr()` → `{root}jet_net_tcp_peer_addr(&(recv))`.
    TcpStreamPeerAddr,
    /// TcpStream: `local_addr()` → `{root}jet_net_tcp_local_addr(&(recv))`.
    TcpStreamLocalAddr,
    /// TcpStream: `close()` → `{ drop(recv); }`.
    TcpStreamClose,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `alloc(v)` → `(recv).alloc(a0)` (hands back a
    /// `&mut T` view into the allocator's storage). The arg is emitted plainly.
    AllocAlloc,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `reset()` → `(recv).reset()`.
    AllocReset,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `free()` → `drop(recv)`.
    AllocFree,
    /// c109 Phase 20: HttpRequest `method()`/`path()`/`body()` → `(recv).<field>.clone()`.
    HttpReqField(&'static str),
    /// c109 Phase 20: HttpRequest `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HttpReqHeader,
    /// c109 Phase 20: HttpRequest `param(name)` → `{root}jet_http_request_param(&(recv), &(a0))`.
    HttpReqParam,
    /// c109 Phase 20: HttpResponse `status()`/`body()` → `(recv).<field>.clone()`.
    HttpRespField(&'static str),
    /// c109 Phase 20: HttpResponse `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HttpRespHeader,
    /// c109 Phase 21: Task `join()` → `(recv).join()` (the no-arg `join` arm of
    /// `emit_builtin_method`, Source/Codegen/Expression.rs ~L967 — shared with the dead
    /// list no-arg join, but here it's the JetTask method). Returns the task's value `T`.
    TaskJoin,
    /// c109 Phase 21: Task `detach()` → `{ let _detach = (recv); }` (D-DETACH1 —
    /// fire-and-forget; drops the JoinHandle). Returns unit.
    TaskDetach,
    /// c109 Phase 21: Channel `receive()` → `(recv).receive()` → `Result<T, Closed>`.
    ChannelReceive,
    /// c109 Phase 21: Channel `sender()` → `(recv).sender()` → `Sender<T>`.
    ChannelSender,
    /// c109 Phase 21: Sender `send(v)` → `(recv).send(a0)`. Returns unit.
    SenderSend,
    /// c109 Phase 25: HttpRouter `get`/`post`/`put`/`delete` route registration
    /// (D-ROUTE1=A). Emits `{root}jet_http_router_register(&mut (recv), "<VERB>".to_string(),
    /// <path>, <handler>)` where `<path>` is the lowered first arg (args[0]) and `<handler>`
    /// is a pre-rendered boxed-closure string (`emit_router_handler` reproduction, resolved
    /// at lowering). `verb` is the uppercase HTTP method literal.
    HttpRouterRegister { verb: &'static str, handler: String },
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
    /// c109 Phase 13: the Fn-typed-parameter coercion (`emit_call_args`' fn-arg
    /// path). When `Some(<fn-type rust string>)`, the value is wrapped
    /// `Box::new(value) as <fn-type>` — unless it is ALREADY boxed (a bare fn-name
    /// value emits its own `Box::new(…)`, or the value is a fn-typed local ident), in
    /// which case only the ` as <fn-type>` suffix is applied. `already_boxed` carries
    /// that resolved decision so emit makes none. This is mutually exclusive with the
    /// borrow/clone wrappers (a Fn param is never borrowed/cloned — `emit_call_args`
    /// skips `&(…)` for `Type::Fn`).
    pub(crate) fn_coerce: Option<TFnCoerce>,
}

/// c109 Phase 13: the resolved Fn-typed-argument coercion (`emit_call_args`).
pub(crate) struct TFnCoerce {
    /// The target fn-type, rendered as a Rust type string (`cx.rust_type(ty)`).
    pub(crate) fn_type_rust: String,
    /// Whether the value already produces a `Box::new(…)` (a bare fn-name value, or a
    /// fn-typed local ident) — so emit applies only ` as <fn-type>`, never re-boxing.
    /// Reproduces `emit_call_args`' `already_boxed` decision, resolved at lowering.
    pub(crate) already_boxed: bool,
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
    // c109 Phase 18: an `#Unsafe fn` (S58) IS covered — it lowers to a Rust `unsafe fn`
    // (the `is_unsafe` flag drives the signature prefix), and its gated body ops are
    // covered below.
    // c109 Phase 23: a `#Pure fn` (S60, E2-M16) IS covered — purity is a sema-only
    // check (E3401: a `#Pure fn` may only call other `#Pure fn`s), validated entirely
    // in sema; codegen erases the annotation (no codegen path reads `f.is_pure` outside
    // these gates). So a `#Pure fn` lowers + emits byte-identically to a plain fn — the
    // body's calls are ordinary `Call`/`MethodCall` nodes covered by earlier phases.
    // c109 Phase 17: GENERIC free functions are covered when every type parameter is a
    // plain `<T>` / bounded `<T: Trait>` form (the clause renders via `render_generics`)
    // and the body uses only type-var values by-value (returned/passed/stored). A generic
    // STRUCT instantiation/method (`make_pair`/`push` — turbofish struct lits, `[T]`-field
    // builtins) is deferred: exclude any function whose param/return mentions a generic
    // struct type or whose body constructs one. The type-var param/return types are
    // admitted by `is_subset_param_ty` (`is_type_var_name`); a generic struct `Apply` type
    // is NOT covered (stays excluded), so such a function exits at the param/return check.
    if f.type_params.is_empty() {
        // Non-generic: no type-var should appear (defensive — sema wouldn't allow it).
    }
    // A method always has a `self` first parameter; the subset is top-level
    // functions only. (Top-level funcs never have `self`, but check anyway.)
    if f.params.iter().any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // c109 Phase 17: a `-> view T` function returns a borrow. The body's returns lower
    // via `lower_view_return` (`TStmt::ViewReturn`), reproducing `emit_view_return`
    // byte-for-byte. The returned value is an in-subset `Ident`/`Field` (sema's E2301/
    // E2304 reject index/slice and a non-owning local, so only those shapes reach codegen);
    // `stmt_in_subset` validates every return is in-subset. No special exclusion needed.
    // Params must be scalars, String, or a covered value type. c109 Phase 23: a
    // DEFAULT parameter value (`h: Int = w`, S61/D-NARG-D2) is covered — sema fills
    // omitted trailing args at every CALL SITE (`CheckerItems::default-value filling`,
    // substituting earlier-param refs with the supplied arg), so by codegen the call's
    // args are complete and the default expr is GONE from the AST. Codegen never reads
    // `p.default` (the signature emits the same `name: ty` regardless), so a defaulted
    // param lowers byte-identically — the only thing the default touches is the call,
    // already handled. No `p.default` exclusion needed.
    for p in &f.params {
        if !is_subset_param_ty(&p.ty, cx) {
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
    // Signature shape: no generics. c109 Phase 18: an `#Unsafe fn` method IS covered
    // (it lowers to an `unsafe fn`, the `is_unsafe` flag driving the prefix). c109
    // Phase 23: a `#Pure fn` method IS covered (purity is sema-only; codegen erases it).
    if !f.type_params.is_empty() {
        return false;
    }
    // c109 Phase 17: a `view`-returning method returns a borrow, lowered via
    // `lower_view_return` (`TStmt::ViewReturn`) — covered (the body's returns are
    // validated in-subset below, and `emit_view_return` is reproduced byte-for-byte).
    // The owning type must be a covered struct or enum (the receiver place and
    // every `self.field` read then emit exactly as `emit_method` produces them).
    let owner_ty = Type::Named(type_name.to_string());
    if !is_covered_struct_ty(&owner_ty, cx) && !is_covered_enum_ty(&owner_ty, cx) {
        return false;
    }
    // c109 Phase 19: a method on a GENERIC struct (`impl<T> user_<T>`) is the deferred
    // "generic-type method" surface — exclude it (the owning struct is a covered value
    // type, but the method's `impl<T>` clause + turbofish receiver are not yet validated
    // across every method shape; stay conservative — exclude on any doubt).
    if struct_is_generic(type_name, cx) {
        return false;
    }
    // The self parameter (if any) must be the FIRST parameter, per the method
    // calling convention. A `self`-bearing method is an instance method; a method
    // with no `self` is a static (associated) function. Any non-first `self` is
    // malformed (sema rejects it) — exclude defensively.
    if f.params.iter().skip(1).any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // D-MUTSELF1: self-mutation in a `mut self` method is now FULLY lowered — the
    // `mut self` slot derefs (`(*self)`), so whole-`self` `self = New{}` emits
    // `(*self) = New{}` and `self.field = v` emits `((*self)).field = v`, both
    // byte-for-byte with the AST path (the prior I2 hole is closed). The covered
    // subset therefore admits both, and the old `stmt_assigns_self` exclusion is gone.
    //
    // Non-self params + the return type must be covered value types (Self resolves
    // to the owning type). c109 Phase 23: a default param value is covered (sema fills
    // call-site args; codegen never reads `p.default`) — same as the free-fn gate.
    for p in f.params.iter().filter(|p| p.name != Syntax::KW_SELF) {
        if !is_subset_param_ty(&resolve_self_ty(&p.ty, type_name), cx) {
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
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109 Phase 12: is this TRAIT-IMPL method (a method of `impl Trait for type_name`)
/// fully inside the TIR subset? Distinct from `tir_covers_method` because the trait
/// method emits via a different function (`emit_trait_method`): bare name, no `pub`,
/// receiver per convention (D-MUTSELF1), self slot `jet_ty: Some(Type::Named(type_name))`. Same rule —
/// **exclude on any doubt**. The owning type must be a covered struct/enum; the body
/// must be in-subset and never reassign `self`.
///
/// Conservative exclusions beyond the inherent-method gate:
///  - `is_unsafe` (`@unsafe fn`) is excluded — its body may use gated pointer ops the
///    subset does not lower, and the `unsafe fn` prefix is a separate emit concern.
///  - a trait method ALWAYS has a `self` receiver (a trait method without `self` is a
///    static trait method, rare; exclude it — the emit hook always renders a receiver).
pub(crate) fn tir_covers_trait_method(f: &Func, type_name: &str, cx: &Cx) -> bool {
    // Signature shape: no generics. c109 Phase 18: an `#Unsafe fn` trait method IS
    // covered (`TFuncKind::TraitMethod.is_unsafe` already drives the `unsafe ` prefix
    // in `emit_tir_trait_method`).
    //
    // c109 (this phase): a `view`-returning trait method is NOW COVERED. The latent
    // AST-path I2 hole it depended on is fixed — `emit_trait_def` (Source/Traits.rs) now
    // threads `m.is_view_return` into the declared return type, so the trait says
    // `-> &String` to match the impl's `-> &String` (was E0053). The borrow shape is
    // the existing total `TStmt::ViewReturn { wrap }` Phase 17 used for inherent/free
    // view methods: `lower_trait_method` sets `env.view_return = f.is_view_return` and
    // `TFunc.is_view`, so lowering routes returns through `lower_view_return` and the
    // emit renders `-> &T`.
    // c109 Phase 23: a `#Pure` trait method is covered (purity is sema-only; erased).
    if !f.type_params.is_empty() {
        return false;
    }
    // The owning type must be a covered struct or enum.
    let owner_ty = Type::Named(type_name.to_string());
    if !is_covered_struct_ty(&owner_ty, cx) && !is_covered_enum_ty(&owner_ty, cx) {
        return false;
    }
    // c109 Phase 19: a trait method on a GENERIC struct is the deferred generic-type
    // method surface — exclude (conservative, as in `tir_covers_method`).
    if struct_is_generic(type_name, cx) {
        return false;
    }
    // A trait method must have `self` as its FIRST parameter (the receiver `&self`/
    // `&mut self`/`self` per convention). A trait method with no `self` (static trait
    // fn) emits no receiver — exclude it (the emit hook always renders a receiver).
    let Some(first) = f.params.first() else {
        return false;
    };
    if first.name != Syntax::KW_SELF {
        return false;
    }
    // No further `self` parameters (malformed — sema rejects, but be defensive).
    if f.params.iter().skip(1).any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // Non-self params + the return type must be covered value types (Self resolves
    // to the owning type). No defaults on a trait method (sema enforces it).
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
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    // D-MUTSELF1: self-mutation is fully lowered (the `mut self` slot derefs), so a
    // trait method that assigns `self` / `self.field` is now covered like any other.
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
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

/// A param/return type the subset allows: scalar (Int/IntN/Float/F32/Bool),
/// Char, String, a covered *plain user struct* (c109 Phase 3), a covered
/// *plain user enum* (c109 Phase 4), a covered collection (Phase 5), or a covered
/// *optional* `T?` / *fallible* `T ? E` (c109 Phase 8). Traits, generics,
/// recursive (boxed) types are still out.
fn is_subset_param_ty(ty: &Type, cx: &Cx) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_type_var_param_ty(ty)
        || is_covered_trait_object_ty(ty, cx)
        || is_covered_distinct_ty(ty, cx)
        || is_covered_tuple_ty(ty, cx)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        || is_covered_fallible_ty(ty, cx)
        || is_covered_fn_ty(ty, cx)
        || is_covered_foreign_value_ty(ty, cx)
        || is_covered_generic_struct_ty(ty, cx)
        || is_covered_concurrency_ty(ty, cx)
        || is_covered_shared_ty(ty, cx)
}

/// c109 Phase 6b: a `Shared<T>` (`Type::Shared`) usable as a param/return/local value
/// type. `cx.rust_type` already renders it to `std::sync::Arc<{T}>` and `rust_param_type`
/// borrows a `Read` non-scalar to `&std::sync::Arc<{T}>` — both shared with the AST path,
/// so the signature is byte-identical. A `Read` param reads as `(*user_h)` (`param_place`'s
/// non-scalar deref). The element `T` must itself be a covered value type. Admitting the
/// type is what lets a fn with a `Shared<T>` param route; passing one to a free call
/// auto-clones the Arc via `lower_one_call_arg`'s `arc_clone` (the gate now admits it).
fn is_covered_shared_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Shared(inner) if is_subset_param_ty(inner, cx))
}

/// c109 Phase 21: a concurrency handle type `Task<T>` / `Channel<T>` / `Sender<T>`
/// (a `Type::Apply` with one type arg) usable as a param/return/local *value* type.
/// `cx.rust_type` (Source/Codegen/Context.rs) already renders these to
/// `{root}jet_std::Jet{Task,Channel,Sender}<{T}>`, so passing/binding/returning one is
/// byte-identical to the AST path with no new emit. The element type `T` must itself be a
/// covered value type. A METHOD on one (`join`/`detach`/`receive`/`sender`/`send`) carries
/// `recv_type == None` (a Phase-9 builtin gap) and is covered by a dedicated shape — but
/// covering the value type never *forces* a method, so an uncovered method still excludes
/// its fn (the recurring "cover the value type, let the next uncovered node exclude its fn"
/// seam). These are NOT `Type::Named` (so they never match `emit_let`'s `is_file_handle`
/// set — their prelude methods take `&self`, so the binding stays a plain `let`, exactly as
/// the AST path renders).
fn is_covered_concurrency_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Channel" | "Sender")
        && args.len() == 1
        && concurrency_elem_covered(&args[0], cx)
}

/// c109 Phase 21: a `Task<T>`/`Channel<T>`/`Sender<T>` element type. Any covered value
/// type, PLUS `Unit` (`Type::Named("Unit")`) — the result type of a `() => { … }` spawn
/// closure that returns nothing (`tasks.spawn(take(s) () => { s.send(…) })` →
/// `Task<Unit>`, the `[Task<Unit>]` worker list in 34_parallel_scan). `Unit` renders via
/// `cx.rust_type` to `()` (Source/Codegen/Context.rs), so `JetTask<()>` is byte-identical
/// to the AST path. (`Unit` is not a covered value type generally — it has no binding/
/// param surface of its own — so it's admitted only here, where it can only appear as the
/// erased result of a unit-returning task.)
fn concurrency_elem_covered(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || is_subset_param_ty(ty, cx)
}

/// c109 Phase 19: a GENERIC struct application `Pair<T>` / `Stack<Int>` (a `Type::Apply`)
/// usable as a param/return/local value type. The base name must be a covered user struct
/// (`struct_is_covered` — which now admits type-var fields, Phase 19), and every type
/// argument must itself be a covered value type OR a bare type variable. The Rust head is
/// `user_<Name>::<args>` (the turbofish from `user_type_apply_rust`), resolved at lowering.
/// `cx.rust_type` already renders `Type::Apply` to that head, so param/return/local typing
/// is byte-identical to the AST path. (A non-generic `Type::Apply` would be malformed;
/// sema only produces `Apply` for a generic struct/enum instantiation.)
fn is_covered_generic_struct_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    // The base must be a known user struct (not an enum/trait/foreign/prelude type).
    if !cx.struct_fields.contains_key(name) {
        return false;
    }
    if !struct_is_covered(name, cx, &mut HashSet::new()) {
        return false;
    }
    // Every type argument is a covered value type or a bare type variable (`T`).
    args.iter()
        .all(|a| is_type_var_param_ty(a) || is_subset_param_ty(a, cx))
}

/// c109 Phase 23: a DISTINCT type (`UserId @= distinct Int`, D-DIST1) usable as a
/// param/return/local *value* type. A distinct type renders via `cx.rust_type` to its
/// newtype `user_<Name>` (the `Type::Named` fallthrough in Context.rs), and the emitted
/// `#[repr(transparent)]` newtype is `Copy` iff its base is (sema/codegen derive set) —
/// but the param convention (`Read`→deref for a non-scalar Named) is decided exactly as
/// for a struct, so passing/binding/returning one is byte-identical to the AST path with
/// no new emit. Construction is the `is_distinct_ctor` `Call` shape; `.raw()` is the
/// dedicated DistinctRaw method shape; `+`/`==` on a `#Numeric` distinct emit the native
/// operator (`ast_operand_is_integer` returns `None` for a distinct-typed operand, so the
/// overflow trap is never claimed — matching the AST path's plain `+`).
fn is_covered_distinct_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(name) if cx.distinct_types.contains_key(name))
}

/// c109 Phase 23: a named-tuple type `(x: Int, y: Int)` (S73/D-SG7, `Type::Tuple`)
/// usable as a param/return/local value type. A tuple renders via `cx.rust_type` to a
/// generated `#[derive(Debug, Clone, PartialEq[, …])]` struct `JetTup_<hash>` (with
/// `user_<field>` fields) emitted by `Tuples.rs` for every tuple SHAPE the program uses
/// — so passing/binding/returning one is byte-identical to the AST path with no new
/// emit. A tuple field read is the generic `Field` shape (`(t).user_<f>`); construction
/// is the `TupleLit` shape; destructuring is the `BindPattern::Tuple` `let` form;
/// `==`/`!=` is native (the derived `PartialEq`). Every field type must itself be a
/// covered value type (so a field read / destructure element emits in-subset).
fn is_covered_tuple_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Tuple(fields) = ty else {
        return false;
    };
    !fields.is_empty() && fields.iter().all(|(_, t)| is_subset_param_ty(t, cx))
}

/// c109 Phase 17: a bare type-PARAMETER type (`T` in a generic `fn id<T>(x: T)`). A
/// single-uppercase `Type::Named` reads as a type var (`Generics::is_type_var_name`),
/// rendered by `cx.rust_type`/`rust_param_type` as the bare letter (by-value, no `&`).
/// Admitting it lets a generic free function whose params/return are type-vars (or covered
/// concrete types) route through the TIR. A generic STRUCT type (`Pair<T>`, `Type::Apply`)
/// is NOT admitted here — that surface (turbofish construction, `[T]`-field builtins) is
/// deferred, so such a function exits the gate at the param/return type check.
fn is_type_var_param_ty(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if crate::Generics::is_type_var_name(n))
}

/// c109 Phase 17: a FOREIGN/PRELUDE type usable as a param/return/local *value* type.
/// These all render through `cx.rust_type` already (a prelude handle/core struct → its
/// `Jet…`/`jet_std::…` Rust name), so passing/binding/returning one is byte-identical to
/// the AST path with no new emit. Only the constructable PRELUDE STRUCTS
/// (HttpRequest/HttpResponse — `net_handle_rust_type` + a struct-literal form) and the
/// CORE structs (ProcessResult/Stopwatch/Json/…) are admitted as value types here; a
/// foreign *imported user* struct/enum needs cross-module `import_ns` construction (a
/// Phase-14 surface) and stays excluded. A METHOD on any of these is still out of subset
/// (handle/prelude methods → Phase 13's residue), so a function that *calls* a method on
/// one is excluded by that call — covering the value type never reaches an uncovered
/// method form.
fn is_covered_foreign_value_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    // c109 Phase 19: a FOREIGN (imported user) struct/enum used as a value type. It
    // renders via `cx.rust_type` to `{root}{mod}::user_<Name>` (Context.rs), and a field
    // read on it mangles (`(n).user_title`) exactly as `mangle` produces — byte-identical
    // to the AST path with no new emit. Construction (`alias.Note { … }`) routes via the
    // `import_ns` StructLit shape; a method on it is still out of subset, so a fn that
    // calls one is excluded by that call (the recurring "cover the value type, let the next
    // uncovered node exclude its fn" seam).
    if cx.foreign_types.contains_key(name) {
        return true;
    }
    // c109 Phase 24: a regex `Match` value (`if m == value(mat)` binds `mat: Match`). It
    // renders via `cx.rust_type` to `Vec<Option<String>>`; the only method on it
    // (`.group(n)`) is the dedicated builtin shape.
    if name == "Match" {
        return true;
    }
    // A prelude struct constructable via a struct literal, or a core/prelude struct that
    // renders to its own Rust name. (FileReader/TcpStream/Arena/… are opaque handles — no
    // literal form — but are valid value types; admit the constructable + core ones, plus
    // the opaque handles, all of which `cx.rust_type` renders.)
    is_prelude_struct_name(name)
        || core_rust_type_name(name).is_some()
        || file_handle_rust_type(name).is_some()
        || net_handle_rust_type(name).is_some()
        || alloc_handle_rust_type(name).is_some()
}

/// c109 Phase 17: a PRELUDE STRUCT name with a struct-literal construction form — the
/// HTTP request/response types (`net_handle_rust_type` + the `is_prelude_struct` branch in
/// `emit_struct_lit`). These get a Rust head `<root>Jet…` with PLAIN (unmangled) fields,
/// and HttpRequest additionally an injected `params: BTreeMap::new()` field.
fn is_prelude_struct_name(name: &str) -> bool {
    matches!(name, "HttpRequest" | "HttpResponse")
}

/// c109 Phase 19: is a FOREIGN (imported user) struct literal `alias.Type { … }` in
/// subset? The AST `emit_struct_lit` `import_ns` branch (Source/Codegen/Expression.rs)
/// emits `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED field names.
/// Cover it when: the import alias resolves in `cx.import_mods` (so the module head is
/// total), the type is a registered cross-module type (`cx.foreign_types`), and every
/// turbofish type arg is a covered/type-var value. The field VALUES are checked in-subset
/// by the caller; the foreign struct's field *types* live in another module and don't
/// affect the emit (the head + mangled field names are the whole shape). A trait-coerced
/// foreign literal (`as_trait`) is excluded by the caller.
fn foreign_struct_lit_in_subset(
    type_name: &str,
    type_args: &[Type],
    import_ns: Option<&str>,
    cx: &Cx,
) -> bool {
    let Some(alias) = import_ns else {
        return false;
    };
    if !cx.import_mods.contains_key(alias) {
        return false;
    }
    if !cx.foreign_types.contains_key(type_name) {
        return false;
    }
    type_args
        .iter()
        .all(|a| is_type_var_param_ty(a) || is_subset_param_ty(a, cx))
}

/// c109 Phase 13: a `fn(…) -> …` parameter/return type the subset lowers. The fn-type
/// renders via `cx.rust_type` (`Box<dyn Fn(…) -> … [+ Send + Sync]>`) exactly as the
/// AST `rust_param_type`/`rust_return_type` do — passed/returned by value (no `&`,
/// `param_place`'s deref matches `emit_func`'s slot). The param/return + arg types must
/// themselves be covered value types so the rendered fn-trait is well-formed and the
/// arg lowering can wrap it. A higher-order fn param (a fn taking/returning a fn) is
/// admitted recursively.
fn is_covered_fn_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Fn { params, ret } => {
            params.iter().all(|p| is_subset_param_ty(p, cx))
                && ret.as_ref().map(|r| is_subset_param_ty(r, cx)).unwrap_or(true)
        }
        _ => false,
    }
}

/// c109 Phase 30: a TRAIT-OBJECT param/return/local type (`s: Shape` where `Shape` is a
/// user trait → `Type::TraitObject("Shape")`, or a bare `Type::Named("Shape")` naming a
/// trait). It renders via `cx.rust_type` to `Box<dyn user_Shape>` (Context.rs), and the
/// param convention is decided by `rust_param_type`'s trait-object arm (`Read` → `&Box<dyn
/// …>`, the slot deref'd to `(*user_s)` by `param_place` — a non-scalar `Read` param). A
/// METHOD on it is the dedicated trait-object dispatch shape (`recv_type == Some(<trait>)`,
/// dynamic dispatch via the bare method name); a non-method use (pass/bind/return) is
/// byte-identical to the AST path. The trait must be a known user trait (`cx.trait_names`),
/// never a foreign/prelude name.
fn is_covered_trait_object_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::TraitObject(t) => cx.trait_names.contains(t),
        Type::Named(n) => cx.trait_names.contains(n),
        _ => false,
    }
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
    // c109 Phase 30: a type-variable payload (`T` in a generic fn's `T?` return —
    // `largest<T: Comparable>() -> (T?)`). A type var renders via `cx.rust_type` to the
    // bare letter (`Option<T>`), and `value(best)`/`null` lower to `Some(user_best)`/`None`
    // byte-identically (no clone/box decision). A type var only appears where a type param
    // is in scope (sema guarantees), so an `Option<T>` payload is total.
    if is_type_var_param_ty(ty) {
        return true;
    }
    if let Type::Named(n) = ty {
        if n == "Error" {
            return true;
        }
        // c109 Phase 21: `Closed` is the err type of `Channel.receive()` →
        // `Result<T, Closed>` (Source/Collections.rs `channel_method_return`). It renders
        // via `cx.rust_type` to `{root}jet_std::Closed` (`core_rust_type_name`), so a
        // `T ? Closed` payload (the unwrap target of `ch.receive() ?? …`) is byte-identical.
        if n == "Closed" {
            return true;
        }
        // c109 Phase 24: a regex `Match` (the payload of `re.match()`'s `Match?`). It
        // renders via `cx.rust_type` to `Vec<Option<String>>` (Context.rs), so an
        // `Option<Match>` payload is byte-identical; the `.group(n)` method is the
        // dedicated builtin shape.
        if n == "Match" {
            return true;
        }
    }
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        // c109 Phase 24: a FOREIGN value-type payload (`Note?` on a `ParsedResult` field —
        // `Note` is an imported struct). It renders via `cx.rust_type` to its own Rust
        // head; an `Option<Note>` is byte-identical (the `value(n)`/`null` constructor is
        // in-subset, the field read plain/sema-cloned).
        || is_covered_foreign_value_ty(ty, cx)
}

/// c109 Phase 5: `ty` is a list `[E]` or map `[K, V]` the subset can lower. The
/// element/key/value types must themselves be covered *value* types — scalar,
/// Char, String, a covered struct/enum, or a nested covered collection — so the
/// literal/index/iteration lowerings reproduce the AST path without any clone/box
/// decision the subset can't make from total facts. A `FixedList` (`[E#N]`, B2) is
/// covered exactly like a `List`: `cx.rust_type` renders it to `Vec<E>` (identical
/// to a list), indexing reads the element type off the base, and a fan-out
/// expression already produces a `[T#N]` value — so a `[E#N]` param/return/element
/// is byte-identical to the list case once its element type is covered.
fn is_covered_collection_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::List(inner) => collection_elem_covered(inner, cx),
        Type::FixedList { elem, .. } => collection_elem_covered(elem, cx),
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
        // c109 Phase 17: a type-variable element (`[T]` in a generic fn). A type var only
        // appears where a type param is in scope (sema guarantees), and renders by value
        // via `cx.rust_type` (`Vec<T>`), so a `[T]` list param/return/local is covered.
        || is_type_var_param_ty(ty)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        // c109 Phase 21: a `[Task<Unit>]` worker list (34_parallel_scan) — a concurrency
        // handle element renders via `cx.rust_type` (`Vec<Jet…<…>>`) like any value type.
        || is_covered_concurrency_ty(ty, cx)
        // c109 Phase 30: a TRAIT-OBJECT element (`[Shape]` → `Vec<Box<dyn user_Shape>>`).
        // Each element is a `Box::new(<lit>) as Box<dyn …>` (the trait-coerced literal),
        // and `.each` over such a list dispatches via `jet_list_each_ref` (the `EachRef`
        // closure op, already built — `list_carries_trait`). The element renders via
        // `cx.rust_type` to `Box<dyn user_<Trait>>`, byte-identical to the AST path.
        || is_covered_trait_object_ty(ty, cx)
        // c109 Phase 24: a FOREIGN value-type element — the prelude JSON enum (`[JSON]` /
        // `[String, JSON]`) OR a cross-module imported user struct/enum (`[String, Note]`
        // where `Note` is an `import_ns` struct). These render via `cx.rust_type` to their
        // own Rust head ({root}jet_std::Json / {root}{mod}::user_<Name>), and a foreign
        // element is moved/cloned by its own sub-expression (a construction or a bound
        // value), so the owning collection's `.iter().cloned()` / per-key/value clone is
        // byte-identical. (A foreign METHOD is still out of subset, so a fn that calls one
        // is excluded by that call — the recurring "cover the value type, let the next
        // uncovered node exclude its fn" seam.)
        || is_covered_foreign_value_ty(ty, cx)
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
    enum_is_covered_inner(name, cx, &mut HashSet::new())
}

/// c109 Phase 16: an enum is covered when every variant payload field is a covered
/// VALUE type — scalar/Char/String, a covered struct, a covered collection, or
/// (recursively) another covered enum (the recursion may go through a `boxed_edge`,
/// reproduced as a `Box::new(…)` at the literal site via `TEnumArg.boxed`). The
/// `seen` set terminates on a recursive (boxed) edge: a self-reference admits the
/// enum (it's already being checked), so a linked-list / expr-AST enum is covered.
/// String/struct/collection payloads route through `emit_boxed_enum_arg`'s borrowed
/// `.clone()` (reproduced at lowering), so they are byte-parity safe.
fn enum_is_covered_inner(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if crate::Generics::is_type_var_name(name)
        || is_json_type_name(name)
        || core_enum_or_prelude(name)
    {
        return false;
    }
    // c109 Phase 24: a FOREIGN (imported) enum (`NoteType`/`ParseError`, matched in
    // search.jet/index.jet). Its variants ARE registered in `cx.enum_variants` /
    // `cx.variant_owner` (`register_foreign_enum_variants`, Imports.rs), so matching it
    // resolves the owning enum + variant prefix (`emit_match_pattern` emits the foreign
    // `{root}{mod}::user_<T>::user_<V>` head via `cx.foreign_types`). A foreign enum is
    // NOT in `cx.cloneable` (that set tracks only local types), so we DON'T require it
    // here; instead we require every variant payload to be a covered VALUE type — a
    // covered payload (scalar/String/covered struct/enum/collection) is itself always
    // `Clone` in Rust, so the foreign enum's generated `#[derive(Clone)]` holds and the
    // match scrutinee's unconditional `(subj).clone()` (the AST `emit_pattern_match_switch`
    // clones a by-ref subject regardless of `cx.cloneable`) is valid. The construction
    // side has no reachable cross-module literal syntax (`note.NoteType.User` is E0107),
    // so a foreign enum is only ever MATCHED / passed, never constructed in another module.
    let is_foreign = cx.foreign_types.contains_key(name);
    let Some(variants) = cx.enum_variants.get(name) else {
        return false;
    };
    if !is_foreign && !cx.cloneable.contains(name) {
        return false;
    }
    // A recursive edge back to this enum admits it (already under check) — the box
    // decision is total. Insert before recursing so a self-reference terminates here.
    if !seen.insert(name.to_string()) {
        return true;
    }
    let ok = variants.iter().all(|(_vname, payload)| {
        let payload_tys: Vec<&Type> = match payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(t, _) => vec![t],
            VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
        };
        payload_tys.iter().all(|t| enum_payload_ty_covered(t, cx, seen))
    });
    seen.remove(name);
    ok
}

/// c109 Phase 16: an enum-variant payload field type the subset can lower —
/// scalar/Char/String, a covered struct, a covered collection, or another covered
/// enum (recursion permitted; the boxed edge is reproduced at the literal site).
/// The `seen` set is threaded through every enum reference (including ones reached
/// via a nested collection element) so a `[Self]` / recursive-through-collection
/// payload terminates instead of looping.
fn enum_payload_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    // c109 Phase 24: a FOREIGN (imported) struct/enum payload (`Query.Kind(NoteType)`
    // where `NoteType` lives in another module). It renders via `cx.rust_type` to
    // `{root}{mod}::user_<Name>`; a payload arg is moved/cloned by `lower_enum_arg`
    // (the borrowed-`.clone()` decision is total), so a foreign payload is byte-parity
    // safe. (A foreign METHOD is still out of subset — the recurring value-type seam.)
    if is_covered_foreign_value_ty(ty, cx) {
        return true;
    }
    match ty {
        Type::Named(n) => {
            if cx.enum_variants.contains_key(n) {
                enum_is_covered_inner(n, cx, seen)
            } else {
                is_covered_struct_ty(ty, cx)
            }
        }
        // A collection payload: its element/key/value types must each be a covered
        // value type, with enum references re-checked under the SAME `seen` guard.
        Type::List(inner) => enum_payload_ty_covered(inner, cx, seen),
        Type::Map { key, value } => {
            enum_payload_ty_covered(key, cx, seen) && enum_payload_ty_covered(value, cx, seen)
        }
        _ => false,
    }
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

/// c109: is `name` a user struct the subset can CONSTRUCT (a struct literal)? Admits
/// self-referential (boxed) fields — a recursive struct such as
/// `Tree { value: Int, child: Tree? }`. Construction is byte-identical to the AST path once
/// the `Box::new(…)` field wrap is reproduced at lowering (a total `boxed` flag from
/// `cx.boxed_edges`); a boxed field READ derefs the `Box` (`TExprKind::Field { boxed }`).
/// A boxed-edge field's value is checked separately by `expr_in_subset` at the gate site;
/// here we only verify the struct's field TYPES are admissible. Generic/foreign/prelude
/// structs are out. The visited set terminates the recursion at a boxed cycle.
fn struct_lit_constructible(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A name that is a genuinely-declared user struct (in `cx.struct_fields`) is a
    // concrete type, never a type variable — even a single-uppercase-letter name like
    // `P`. The `is_type_var_name` heuristic only excludes *undeclared* single-letter
    // names (true generic type vars `T`/`U`), so guard it on non-declaration.
    let is_type_var = crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
    if cx.trait_names.contains(name)
        || cx.enum_variants.contains_key(name)
        || cx.foreign_types.contains_key(name)
        || net_handle_rust_type(name).is_some()
        || is_type_var
        || struct_is_generic(name, cx)
    {
        return false;
    }
    let Some(fields) = cx.struct_fields.get(name) else {
        return false;
    };
    if !seen.insert(name.to_string()) {
        // A cycle through a boxed edge — admitted (the field below proved boxed).
        return true;
    }
    let ok = fields.iter().all(|(fname, fty)| {
        // A boxed (recursive) edge: its payload struct must itself be constructible.
        // The boxed-payload type unwraps Option/the bare Named to the struct name.
        if cx.boxed_edges.contains(&(name.to_string(), fname.clone())) {
            return boxed_field_payload_constructible(fty, cx, seen);
        }
        // A non-boxed field: the ordinary covered-field rule.
        field_ty_covered(fty, cx, seen)
    });
    seen.remove(name);
    ok
}

/// The payload of a boxed (recursive) struct field — `Tree` in `child: Tree?`
/// (`Option<Tree>`) or a bare `Tree`. The payload struct must be constructible.
fn boxed_field_payload_constructible(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_constructible(inner, cx, seen),
        Type::Named(n) => struct_lit_constructible(n, cx, seen),
        _ => false,
    }
}

/// c109 Phase 19: is `name` a GENERIC user struct (one with a type-var field)? A generic
/// struct's fields reference type vars (`first: T`); `struct_is_covered` now admits those
/// (so a generic struct is a covered VALUE type — Phase 19 covers turbofish construction +
/// `Type::Apply` params). But a METHOD on a generic struct (`impl<T> user_<T>`) is a
/// SEPARATE deferred surface (the inventory's "generic-type method"), so the method gates
/// exclude an owning type that is generic. (Free generic functions are covered by Phase 17;
/// generic STRUCT free functions by Phase 19; generic METHODS stay on the AST path.)
fn struct_is_generic(name: &str, cx: &Cx) -> bool {
    cx.struct_fields
        .get(name)
        .map(|fields| fields.iter().any(|(_, fty)| ty_mentions_type_var(fty)))
        .unwrap_or(false)
}

/// True if `ty` references a bare type variable anywhere (a `Type::Named(T)` with
/// `is_type_var_name`, or nested inside a list/map). Used to detect a generic struct.
fn ty_mentions_type_var(ty: &Type) -> bool {
    match ty {
        Type::Named(n) => crate::Generics::is_type_var_name(n),
        Type::List(inner) => ty_mentions_type_var(inner),
        Type::Map { key, value } => ty_mentions_type_var(key) || ty_mentions_type_var(value),
        _ => false,
    }
}

fn struct_is_covered(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A struct that is a trait/enum or a non-user (foreign/core/prelude) type is
    // out. `cx.struct_fields` only holds user structs declared in this module.
    // A declared user struct is a concrete type, never a type var (see
    // `struct_lit_constructible`): a single-uppercase-letter struct name (`P`) is real.
    let is_type_var = crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
    if cx.trait_names.contains(name)
        || cx.enum_variants.contains_key(name)
        || cx.foreign_types.contains_key(name)
        || net_handle_rust_type(name).is_some()
        || is_type_var
    {
        return false;
    }
    let Some(fields) = cx.struct_fields.get(name) else {
        return false;
    };
    if !seen.insert(name.to_string()) {
        // A cycle through a boxed edge — admitted (the edge below proved boxed and the
        // payload struct is itself covered). The boxed field READ derefs the `Box`
        // (`TExprKind::Field { boxed: true }`); construction wraps `Box::new(…)`.
        return true;
    }
    let ok = fields.iter().all(|(fname, fty)| {
        // A boxed (recursive) edge: its payload struct must itself be covered. The read
        // derefs the `Box` (total fact), so the edge is now in-subset.
        if cx.boxed_edges.contains(&(name.to_string(), fname.clone())) {
            return boxed_field_payload_covered(fty, cx, seen);
        }
        field_ty_covered(fty, cx, seen)
    });
    seen.remove(name);
    ok
}

/// The payload type of a boxed (recursive) struct edge — `Tree` in `child: Tree?`
/// (`Option<Tree>`) or a bare `Tree`. The payload struct must itself be a covered
/// value type (so its own reads/construction lower). Mirrors
/// `boxed_field_payload_constructible` but uses the value-coverage rule.
fn boxed_field_payload_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_covered(inner, cx, seen),
        Type::Named(n) => struct_is_covered(n, cx, seen),
        _ => false,
    }
}

/// A struct *field* type the subset can lower: scalar/String/Char, or another
/// covered struct. Compound/optional/enum/fn field types exclude the struct.
fn field_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    // c109 Phase 19: a generic struct's field may be a bare type VARIABLE (`first: T`
    // in `Pair<T>`). It renders to the bare `T` via `cx.rust_type` and a struct-lit
    // field value is the type-var value itself (by value), so a type-var field needs no
    // clone/deref decision — admit it. (A struct with a type-var field is only ever
    // *used* as a `Type::Apply` — `Pair<Int>` — which `is_covered_generic_struct_ty`
    // gates; a bare `Pair` never type-checks in sema.)
    if is_type_var_param_ty(ty) {
        return true;
    }
    // c109 Phase 24: a FOREIGN value-type field/element — a cross-module imported user
    // struct/enum or the prelude JSON enum. It renders via `cx.rust_type` to its own
    // Rust head; a struct-lit field value / collection element is the value itself (by
    // value), so a foreign-typed field needs no clone/deref decision at the field site.
    // (Reading a foreign field is in-subset; a foreign METHOD still excludes the fn.)
    if is_covered_foreign_value_ty(ty, cx) {
        return true;
    }
    // c109 Phase 24: a covered ENUM field (`note_type: NoteType` on a `Note` struct). An
    // enum field renders to `user_<Enum>` and a field read is a plain place / sema-cloned
    // `.clone()` (the Phase-3/6 owning-field rewrite) — byte-identical, no new decision.
    // (Previously `field_ty_covered` admitted only scalar/String/struct/collection fields,
    // so any struct with an enum field stayed on the AST path.)
    if is_covered_enum_ty(ty, cx) {
        return true;
    }
    // c109 Phase 24: an OPTIONAL / FALLIBLE field (`note: Note?`, `error_msg: String?` on
    // a `ParsedResult` struct) whose payload is a covered value type. It renders via
    // `cx.rust_type` to `Option<…>`/`Result<…,…>`; a struct-lit field value (`value(n)` →
    // `Some(n)`, `null` → `None`) is in-subset and emitted as-is, and a field read is a
    // plain place / sema-cloned `.clone()` — byte-identical, no new field-site decision.
    if is_covered_fallible_ty(ty, cx) {
        return true;
    }
    // c109 Phase 27: a FUNCTION-typed field (`step: fn(Int) -> Int` on a `Worker`
    // struct). It renders via `cx.rust_type` to `Box<dyn Fn(...) -> ...>` exactly as
    // the AST `struct_field_rust` does; a struct-lit field value (a lambda / a bare
    // fn-name) lowers in-subset and is emitted as-is (NO ` as <fn-type>` coercion at
    // the literal site — the AST `emit_struct_lit` field value is a plain `emit_expr`),
    // and a fn-field READ / CALL routes through the Phase-27 `FnFieldCall` shape. The
    // param/ret types are only RENDERED (never inspected for a decision), so any Fn
    // signature is admissible.
    if matches!(ty, Type::Fn { .. }) {
        return true;
    }
    match ty {
        Type::Named(n) => struct_is_covered(n, cx, seen),
        // c109 Phase 16: a collection field (`[E]` / `[K, V]`) whose element/key/value
        // types are covered value types. The struct-literal emit is plain
        // (`field: vec![…]`), byte-identical to the AST path. A list/map *element*
        // that is itself a covered struct/enum/collection is admitted (the Phase-5
        // collection coverage), so no clone/box decision arises at the field site.
        Type::List(inner) => field_ty_covered(inner, cx, seen),
        // c109 (B2): a fixed-size-list field (`row: [Int#3]`). It renders to `Vec<E>`
        // like a list field (`cx.rust_type`), so a struct-lit field value / field read
        // is byte-identical to the list case once its element type is covered.
        Type::FixedList { elem, .. } => field_ty_covered(elem, cx, seen),
        Type::Map { key, value } => {
            field_ty_covered(key, cx, seen) && field_ty_covered(value, cx, seen)
        }
        _ => false,
    }
}

/// `locals` is the set of names bound as params/locals so far in this scope.
/// It is threaded so an `Expr::Ident` can be classified: a name that is not a
/// local must not be a const/fn-value (excluded). Bindings extend it in order.
fn stmt_in_subset(s: &Stmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    match s {
        Stmt::Val(b) => {
            // c109 Phase 19: an `arena_view` binding (`x @= arena.alloc(v)` / `x ::
            // arena.alloc(v)`) IS covered — it lowers to a plain `let <x> = <init>;` (no
            // type, no mut) with a deref'd slot, exactly as `emit_let`'s `arena_view`
            // branch (the init is a covered `arena.alloc(v)` handle call). The escape/
            // use-after-reset rules (E0631/E0632) are enforced entirely in sema.
            match &b.pattern {
                // c109 Phase 23: a TUPLE-destructuring binding `(a, b) @= <init>` (S74,
                // `BindPattern::Tuple`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name from `(tmp).user_<canonical-field>.clone()` (pairing
                // elems to the type's canonical fields BY POSITION). Covered when the init
                // is in-subset (its lowered `.ty` is a `Type::Tuple` — sema guarantees a
                // tuple pattern destructures a tuple value, so the canonical field names
                // are total at lowering). The Struct/List destructure forms stay on the
                // AST path (no live-suite use; can be a later slice).
                Some(BindPattern::Tuple { elems, .. }) => {
                    let ok = !b.is_comptime
                        && !b.uninit
                        && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109 Phase 26: a LIST-destructuring binding `[a, b, c] @= <init>` (S74,
                // `BindPattern::List`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name via `jet_unpack_vec(tmp, want, i, file, line)`
                // (a runtime bounds-checked element move). Covered when the init is
                // in-subset; the element type partiality (`expr_jet_ty`'s
                // `Some(List(inner))`-only match) is reproduced at lowering. The
                // fan-out result-list destructure (`41_fan_out` `main`) is exactly this.
                Some(BindPattern::List { elems, .. }) => {
                    let ok = !b.is_comptime
                        && !b.uninit
                        && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109: a STRUCT-destructuring binding `Type { x, y } @= <init>`
                // (S74, `BindPattern::Struct`). The AST `emit_stmt` borrows the init
                // into a temp, then binds each field via `(tmp).user_<field>.clone()`
                // (the pattern's field name is both the bound local and the read).
                // Covered when the init is in-subset; the per-field type comes from
                // `cx.struct_fields` at lowering (total — sema proved the pattern
                // destructures a struct value).
                Some(BindPattern::Struct { fields, .. }) => {
                    let ok = !b.is_comptime
                        && !b.uninit
                        && expr_in_subset(&b.init, cx, locals);
                    for f in fields {
                        locals.insert(f.name.clone());
                    }
                    ok
                }
                // Forward-safety default: a future BindPattern variant defaults to the
                // safe exclusion. Currently unreachable — Tuple/List/Struct are all matched.
                #[allow(unreachable_patterns)]
                Some(_) => false,
                None => {
                    // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr`. Sema
                    // evaluates the value into `b.ct` and the AST `emit_let` emits it as
                    // literal data (`let <name>[: <ty>] = <ct.serialize()>;`) — the runtime
                    // `init` expr is NEVER emitted, so it need not be in-subset. Covered
                    // whenever the resolved value is present (`b.ct.is_some()`); the literal
                    // serialization is total. `b.uninit` cannot co-occur with comptime.
                    let ok = if b.is_comptime {
                        !b.uninit && b.ct.is_some()
                    } else {
                        !b.uninit && expr_in_subset(&b.init, cx, locals)
                    };
                    // The binding's name is in scope for subsequent statements.
                    locals.insert(b.name.clone());
                    ok
                }
            }
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
            // D-MUTSELF1: a field-assignment `place.field = v`. The base place (a
            // field-read expr, e.g. `self`) and the value must both be in-subset; the
            // place is rendered through the same field-read lowering, so its
            // resolution is total. Compound ops (`+=`, S17) ride the same path.
            LValue::Field { base, .. } => {
                expr_in_subset(base, cx, locals) && expr_in_subset(value, cx, locals)
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
            // c109 Phase 5/22: `loop x in coll` / `loop k, v in map` (ForKind::In).
            // A method-call collection (`.chars()`/`.lines()`/`.split(…)`) takes a
            // distinct `emit_for_in` branch; Phase 22 reproduces each (`forin_method_
            // collection_in_subset`). A non-method-call collection is the plain
            // `.iter()` form (single- or two-binding map). The loop var(s) bind in the
            // body scope with an *unresolved* type (matching the AST slot's `jet_ty:
            // None`, so they never enable the overflow trap).
            ForKind::In { collection } => {
                // The TWO-BINDING map form (`loop k, v in map`) ALWAYS emits
                // `({coll}).iter()` (the `var2` branch of `emit_for_in` fires first,
                // before the `.chars()`/`.lines()` method-call branches). So a method-call
                // collection in the two-binding position (notably an owning-field-read
                // `idx.notes` that sema rewrote to `idx.notes.clone()` — the Phase-3
                // finding) is just a plain in-subset collection value, not one of the
                // single-binding `chars`/`lines` special forms. Check it as a plain
                // expr. The SINGLE-binding form keeps the method-call classification
                // (`.chars()`/`.lines()`/`.split(…)` → `forin_method_collection_in_subset`).
                if var2.is_some() {
                    if !expr_in_subset(collection, cx, locals) {
                        return false;
                    }
                } else if let Expr::MethodCall { .. } = collection {
                    // A single-binding method-call collection: the form must be one
                    // `emit_for_in` reproduces (`chars`/`lines`/`.iter().cloned()` default).
                    if !forin_method_collection_in_subset(collection, cx, locals) {
                        return false;
                    }
                } else if !expr_in_subset(collection, cx, locals) {
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
        // c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema picks the
        // branch (`selected_then`); codegen emits ONLY that branch's statements inline.
        // The gate must classify the SELECTED branch (the unselected one is dropped and
        // never reaches codegen — it is name-resolution-only, D-WHEN2). Its statements
        // leak into the outer scope (the AST shares `&mut env`), so they extend `locals`.
        // Before sema resolves `selected_then` (a `build_cx`-only gate test), default to
        // the `then` branch so the gate is still exercised; at real codegen
        // `selected_then` is always set.
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => {
            let chosen: &[Stmt] = match selected_then {
                Some(true) | None => then_body,
                Some(false) => else_body.as_deref().unwrap_or(&[]),
            };
            chosen.iter().all(|s| stmt_in_subset(s, cx, locals))
        }
        // c109 Phase 18: an audited `#Unsafe { … }` gate region (`Stmt::Unsafe`). The AST
        // `emit_stmts` lowers it to `unsafe { … }` and emits the body on the SAME `&mut
        // env` (so the body's `let`s LEAK into the outer scope). The gate checks the body
        // on the same `locals` (matching that leak). The `#Audit("…")` annotation emits
        // nothing. I1: this is the source gate — the only place a Rust `unsafe` block is
        // produced — so admitting it here cannot introduce an ungated `unsafe`.
        Stmt::Unsafe { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1) lowers to a plain Rust
        // block; the body's `let`s LEAK into the outer scope (the AST shares `&mut env`),
        // so the gate checks the body on the SAME `locals`.
        Stmt::Region { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1) — a plain block
        // with a per-field guard. Each field value + the body must be in-subset (the body
        // leaks like a region).
        Stmt::ContextBlock { fields, body, .. } => {
            fields.iter().all(|(_, v, _)| expr_in_subset(v, cx, locals))
                && body.iter().all(|s| stmt_in_subset(s, cx, locals))
        }
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1/D-QUAL1)
        // erases to a plain Rust block — `emit_stmt`'s `Stmt::Caps` arm is byte-for-byte
        // identical to `Stmt::Region` (`{ <body> }` on the SAME `&mut env`, so the body's
        // `let`s LEAK into the outer scope). The cap set is enforced entirely in sema
        // (E0741); codegen is dumb (I3). Check the body on the SAME `locals` (it leaks
        // like a region), reusing the covered `TStmt::Region` lowering.
        Stmt::Caps { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // Forward-safety default: a future Stmt variant defaults to the safe AST path
        // (I2 — a false negative keeps a fn off the TIR; a false positive would be unsafe).
        // Currently unreachable because every variant above is matched.
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

/// c109 Phase 22: is a method-call collection iteration (`loop x in <coll>` where
/// `<coll>` is an `Expr::MethodCall`) in-subset? Mirrors `emit_for_in`'s
/// `Expr::MethodCall` branches (Source/Codegen/Statement.rs):
///  - `.chars()` — char iteration; only the *receiver* (a string) is emitted, so it
///    must be in-subset.
///  - `.lines()` — streaming `BufRead::lines`; the receiver is a `FileReader`/
///    `StdinHandle` (or inline `io.stdin()`), again emitted on its own, so it must be
///    in-subset. (Both lines shapes route here; the FileReader-vs-stdin split is
///    resolved at lowering off `tir_recv_jet_ty`/the inline-`stdin` shape.)
///  - any other method — the `.iter().cloned()` default, which emits the WHOLE method
///    call as the collection value, so the whole call must be in-subset (e.g. a
///    Phase-9 `.split(…)` builtin returns a `[String]` value).
fn forin_method_collection_in_subset(
    collection: &Expr,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let Expr::MethodCall {
        receiver, method, ..
    } = collection
    else {
        return false;
    };
    match method.as_str() {
        "chars" | "lines" => expr_in_subset(receiver, cx, locals),
        _ => expr_in_subset(collection, cx, locals),
    }
}

/// c109 Phase 22: classify an `if` condition. Returns `None` if the condition is not
/// in-subset; otherwise returns the binding name(s) the condition introduces into the
/// then-branch scope (empty for a plain/`is_none` condition). Mirrors `emit_if`'s three
/// condition shapes via `if_pattern_test` (Source/Codegen/Statement.rs):
///  - a plain boolean expr → in-subset iff `expr_in_subset`, no bindings;
///  - an `x == null` test (`Pattern::Absent`) → `is_none`, subject in-subset, no bindings;
///  - an optional-binding test (`value(b)`/`ok(b)`/`err(b)`) → if-let, subject in-subset,
///    the binding `b` in scope. Variant/Or/Range patterns in an `if` condition stay on
///    the AST path (conservative — not covered here).
fn if_cond_in_subset(cond: &Expr, cx: &Cx, locals: &HashSet<String>) -> Option<Vec<String>> {
    // The `x == null` (`Pattern::Absent`) form: `if {subj}.is_none()`.
    if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = cond
    {
        return expr_in_subset(subject, cx, locals).then(Vec::new);
    }
    // The optional-binding (if-let) form — only a DIRECT `PatternTest` (not the
    // `Binary(And, …)` shape `if_pattern_test` also admits, which we leave on the AST
    // path). Covered patterns: `value(b)`/`ok(b)`/`err(b)` (single binding). Variant/
    // Or/Range patterns are excluded (conservative).
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if !expr_in_subset(subject, cx, locals) {
            return None;
        }
        // c109 Phase 24: a JSON variant if-let (`if data == Object(entries)` /
        // `if port == Number(n)`). The prelude JSON enum is matched via a single-payload
        // variant pattern (`Object`/`Number`/`Text`/`Boolean`/`Array`) binding one name.
        // The Rust if-let pattern (`{root}jet_std::Json::Object(user_entries)`) is produced
        // by the JSON-aware `emit_if_let_pattern` (reused at lowering), and the binding's
        // type comes from `core_json_pattern_types` (totality). Cover ONLY the JSON-variant
        // single-bind case (a user-enum variant if-let stays on the AST path — conservative,
        // not yet covered as an if-condition form); `Null` is the `Absent`-style form, but
        // `data == Null` would parse as a variant pattern with no binding (not used in the
        // live suite — excluded here, single-bind only).
        if let Pattern::Variant { variant, bindings, span: _ } = pattern {
            if is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Bind(_))
            {
                if let PatSlot::Bind(b) = &bindings[0] {
                    return Some(vec![b.clone()]);
                }
            }
            // c109 (B4): a USER-enum variant if-let (`if m == Ping(n)`). Covered when
            // the variant is a single-payload variant (one `Bind` slot) of a covered
            // user enum — the AST `emit_if` already emits the correct
            // `if let user_E::user_V(user_b) = <subj>` head. The subject was checked
            // above; require the owning enum to be covered so the prefix/payload are
            // total. Multi-bind / unit variants stay on the AST path (the single-bind
            // shape mirrors the JSON-variant if-let exactly).
            if !is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Bind(_))
            {
                if let Some(owner) = cx.variant_owner.get(variant) {
                    if enum_is_covered(owner, cx) {
                        if let PatSlot::Bind(b) = &bindings[0] {
                            return Some(vec![b.clone()]);
                        }
                    }
                }
            }
            return None;
        }
        return match pattern {
            Pattern::Present { binding, .. }
            | Pattern::Ok { binding, .. }
            | Pattern::Err { binding, .. } => Some(vec![binding.clone()]),
            _ => None,
        };
    }
    // A plain boolean condition.
    expr_in_subset(cond, cx, locals).then(Vec::new)
}

fn if_in_subset(ifs: &IfStmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    let Some(cond_bindings) = if_cond_in_subset(&ifs.cond, cx, locals) else {
        return false;
    };
    // Each branch scopes its own bindings; check on a clone so a `let` in the
    // `then` arm doesn't leak into the `else` arm's classification. An optional-binding
    // condition introduces its binding(s) into the then-branch scope.
    let mut then_locals = locals.clone();
    for b in &cond_bindings {
        then_locals.insert(b.clone());
    }
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
        && arms.iter().all(|a| arm_head_range(cx, &a.cond, subject).is_some())
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
        .all(|a| arm_fallible_pattern(cx, &a.cond, subject).is_some())
    {
        for a in arms {
            let pat = arm_fallible_pattern(cx, &a.cond, subject).expect("checked above");
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
    // Shape D (c109 Phase 15): a MIXED comparison/Bool switch — the general
    // `emit_mixed_switch` `if/else if … else` chain used when the arms are NOT all
    // variant (shape A), NOT all range (shape B), and NOT all fallible (shape C). Every
    // arm head must be a PLAIN in-subset comparison/Bool expression — i.e. the
    // `_ => emit_expr(cond)` branch of `emit_switch_arm_cond` (NOT a variant/Eq-variant
    // pattern, which would route through `emit_pattern_matches`, and NOT a range head).
    // Conservative: a single pattern-test arm in the chain excludes the whole switch
    // (stays on the AST path). The `else` is optional (`emit_mixed_switch` handles both).
    if arms.iter().all(|a| arm_is_plain_cond(cx, &a.cond, subject)) {
        for a in arms {
            if !expr_in_subset(&a.cond, cx, locals) {
                return false;
            }
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
    false
}

/// c109 Phase 15: an arm head that `emit_switch_arm_cond` would emit as a PLAIN
/// expression (`_ => emit_expr(cond)`) — NOT a variant/Eq-to-variant pattern (which it
/// routes through `emit_pattern_matches`) and NOT an arm-head range (shape B). This is
/// the comparison/Bool arm of the general mixed switch (shape D).
fn arm_is_plain_cond(cx: &Cx, cond: &Expr, subject: &Expr) -> bool {
    // A variant or Eq-to-variant arm → `emit_pattern_matches` (excluded here).
    if arm_variant_pattern(cx, cond, subject).is_some() {
        return false;
    }
    // An arm-head range → shape B / `emit_pattern_matches` Range (excluded here).
    if arm_head_range(cx, cond, subject).is_some() {
        return false;
    }
    // Any other pattern test (`ok`/`err`/`value`/`null`/`present`/wildcard) → not a
    // plain comparison; exclude (those are shape C or unsupported).
    if matches!(cond, Expr::PatternTest { .. }) {
        return false;
    }
    true
}

/// c109 Phase 8: an arm head that is a fallible/optional pattern test over the
/// subject — `subject == ok(b)` / `err(b)` / `value(b)` / `null`. Returns the
/// `Pattern::{Ok,Err,Present,Absent}`, else `None` (a variant/range/comparison arm).
fn arm_fallible_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest { subject: s, pattern, .. } if pattern_subjects_match(cx, s, subject) => {
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
        Expr::PatternTest { subject: s, pattern, .. } if pattern_subjects_match(cx, s, subject) => {
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
        Expr::Binary(BinOp::Eq, lhs, rhs, _) if pattern_subjects_match(cx, lhs, subject) => {
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
fn arm_head_range(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<(i64, i64)> {
    match cond {
        Expr::PatternTest { subject: s, pattern: Pattern::Range { lo, hi, .. }, .. }
            if pattern_subjects_match(cx, s, subject) =>
        {
            Some((*lo, *hi))
        }
        _ => None,
    }
}

/// Mirror codegen's `pattern_subjects_match` (Statement.rs): an arm subject names
/// the same ident as the switch subject, is the implicit `it`, or (B1) reads
/// identically in source — a NON-IDENT subject (`h.val`, `pick()`, `xs[0]`) compared
/// spanlessly via its source slice, matching the AST so a non-ident pattern switch
/// routes through the SAME `lower_enum_match` / `lower_fallible_match` the AST's
/// `emit_pattern_match_switch` uses.
fn pattern_subjects_match(cx: &Cx, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(na, _), Expr::Ident(nb, _)) => na == nb,
        (Expr::Ident(n, _), _) if n == Syntax::KW_IT => true,
        _ => {
            let sa = cx.src.get(a.span().start..a.span().end);
            let sb = cx.src.get(b.span().start..b.span().end);
            matches!((sa, sb), (Some(x), Some(y)) if x == y)
        }
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
        // An ident must resolve to a local/param, OR (c109 Phase 13) be a bare
        // function name used as a VALUE: a non-local, non-const name in `cx.fn_types`
        // with a `Type::Fn` type. The latter emits `emit_named_fn_value`'s
        // `Box::new(move |…| …) as <fn-type>` wrapper. A non-local that is a const
        // (inlined) or an unqualified module import is still out.
        Expr::Ident(name, _) => {
            // c109 Phase 24: a comptime CONST ident (`PAGE_HEADER`) inlines its pre-rendered
            // Rust value at the use site (`cx.consts[name]` — a TOTAL string fact, the same
            // `emit_expr` Ident arm reads). Admit it when it is a known const not shadowed by
            // a local. (A const used as an arithmetic operand resolves to `None` in
            // `ast_operand_is_integer` — `env.ty_of(const)` is `None` — exactly as the AST
            // path's `operand_is_integer`, so the overflow trap is never wrongly claimed.)
            (cx.consts.contains_key(name) && !locals.contains(name))
                || locals.contains(name)
                || ident_is_named_fn_value(name, cx, locals)
        }
        Expr::Unary(_, inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Binary(_, l, r, _) => {
            expr_in_subset(l, cx, locals) && expr_in_subset(r, cx, locals)
        }
        Expr::Call(c) => {
            // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
            // parses as `Expr::Call { name: "f" }`, NOT `Expr::CallValue`. The AST path
            // (`emit_call`, env-contains-name branch) emits `(place)(args)` with args
            // lowered PLAINLY (`emit_call_args(.., None, ..)`). Cover it: the name is a
            // local (not a const) and every arg is in-subset + unlabeled.
            if locals.contains(&c.name) && !cx.consts.contains_key(&c.name) {
                return c
                    .args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
            }
            // `print` is the one builtin the subset covers (exactly one arg).
            let is_print = c.name == Syntax::BUILTIN_PRINT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // c109 Phase 26: the rich-runtime-report builtins `require(cond[, msg])`,
            // `require_eq(left, right)`, and `panic(msg)` (S36). Each is a bare
            // `Expr::Call` whose name is the builtin (not in `cx.sigs`, not shadowed by a
            // local) and whose argument count matches the AST `emit_require`/
            // `emit_require_eq`/`emit_panic_stop` shape. The whole statement string is
            // rendered at lowering (`TExprKind::RequireStop`). Every arg expr (cond/msg/
            // operands) must be in-subset (they are lowered + emitted via the TIR). Sema
            // validated the shape (arg count, `panic`'s 1 message arg). Excluded if a
            // user fn / local shadows the name (then the plain-call branch claims it).
            let is_require = c.name == Syntax::BUILTIN_REQUIRE
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && (c.args.len() == 1 || c.args.len() == 2);
            let is_require_eq = c.name == Syntax::BUILTIN_REQUIRE_EQ
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 2;
            let is_panic = c.name == Syntax::BUILTIN_PANIC
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() == 1;
            // c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). A bare
            // `Expr::Call { name: "input" }` with NO user `input` fn (`!cx.sigs`) and no
            // local shadow lowers to the SAME `jet_std_io_input(None|Some(&(arg)))` form
            // as `io.input(...)` (the AST `emit_call` ambient-input branch, Expression.rs
            // ~L1778; sema mirrors it in CheckerInfer + returns `Result<String, IOError>`).
            // 0 args → `(None)`, 1 arg (a String prompt) → `(Some(&(arg)))`. Reproduced
            // byte-for-byte in `emit_tir_ambient_input`. Disjoint from a plain fn (those
            // ARE in `cx.sigs`) and the local-call branch (shadowing local handled above).
            let is_ambient_input = c.name == Syntax::BUILTIN_INPUT
                && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
                && c.args.len() <= 1;
            // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
            // `checked(e)` (D-NUMOPS1). The AST `emit_call` (Expression.rs ~L1756) claims
            // them when the name is one of the three AND not shadowed by a user fn
            // (`!cx.sigs`); the sole argument is one integer `Expr::Binary` (`+`/`-`/`*`/`/`),
            // lowered to `(ls).{name}_{add|sub|mul|div}(rs)` with PLAIN operands (no trap).
            // Sema validated the shape. The operands must be in-subset; `checked` yields
            // `T?`, the others `T`. Handled by a bespoke `TExprKind::OverflowOpt` — return
            // early here so the generic-call arg machinery below doesn't also claim it.
            if matches!(
                c.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !cx.sigs.contains_key(&c.name)
                && !locals.contains(&c.name)
            {
                return matches!(
                    c.args.first().map(|a| &a.expr),
                    Some(Expr::Binary(
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div,
                        ..
                    ))
                ) && c.args.len() == 1
                    && c.args.iter().all(|a| {
                        a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
                    });
            }
            // Otherwise the callee must be a known *plain* top-level function:
            // in `cx.sigs`, not a local, and NOT an extern/FFI function or an
            // unqualified module import (those lower to different call forms — covered
            // separately below in c109 Phase 14).
            let is_plain_fn = !locals.contains(&c.name)
                && cx.sigs.contains_key(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && !cx.unqualified_inline.contains_key(&c.name)
                && !cx.unqualified_file.contains_key(&c.name);
            // c109 Phase 23: a DISTINCT-type constructor `UserId(expr)` (D-DIST1) is a
            // bare `Expr::Call` whose name is a distinct type (not in `cx.sigs` — so the
            // AST `emit_call` falls through to `user_<Name>(args)` with NO sig, plain args).
            // The TIR's fallthrough `Call` form reproduces that exactly (sig lookup misses
            // → `lower_one_call_arg` with `conv: None` → plain arg, then `user_<Name>(…)`).
            // Sema validated the single-arg base-typed shape (E2 distinct checks); we admit
            // it when the name is a known distinct type, not shadowed by a local.
            let is_distinct_ctor = !locals.contains(&c.name)
                && cx.distinct_types.contains_key(&c.name);
            // c109 Phase 14: FFI extern + unqualified module-import calls are now
            // covered. Each lowers to its own resolved call form (`emit_call`'s
            // `extern_funcs`/`unqualified_inline`/`unqualified_file` arms). The
            // priority MUST match `emit_call`: extern is checked before the unqualified
            // arms, and a LOCAL/print/plain-fn callee was already claimed above. These
            // are all top-level (non-local) names, so they are disjoint from the
            // local-call branch. The extern arg form uses `emit_extern_call_args`
            // (a non-scalar `Read` arg is `(…).clone()`, not `&(…)`) — reproduced in
            // lowering; the Arc (`shared_auto_clone`) form stays excluded.
            let is_extern = !locals.contains(&c.name) && cx.extern_funcs.contains_key(&c.name);
            let is_unqual_inline = !locals.contains(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && cx.unqualified_inline.contains_key(&c.name);
            let is_unqual_file = !locals.contains(&c.name)
                && !cx.extern_funcs.contains_key(&c.name)
                && cx.unqualified_file.contains_key(&c.name);
            // c109 Phase 13: a callee with a **Fn-typed parameter** is now covered.
            // The arg routes through `emit_call_args`'s `Box::new(…) as <fn-type>`
            // coercion (`lower_one_call_arg` reproduces it from total facts). The Fn
            // arg itself must be in-subset (a lambda, a fn-name value, or a fn-typed
            // local). No special exclusion remains — the Box-coercion is total.
            // c109 Phase 23: a call-site LABEL (`f(width: 4.0)`, S61/D-NARG1) is allowed.
            // Labels are checked DOCUMENTATION (D-NARG-D4): sema validates each label names
            // the parameter at its OWN position (E0125) — labels NEVER reorder arguments —
            // and codegen never reads `CallArg.label` (`emit_call_args` is purely
            // positional). So a labeled arg emits byte-identically to an unlabeled one.
            (is_print || is_ambient_input || is_require || is_require_eq || is_panic || is_plain_fn || is_distinct_ctor || is_extern || is_unqual_inline || is_unqual_file)
                && c.args.iter().all(|a| {
                    // c109 Phase 6b: a `Shared<T>` arg auto-cloning the Arc
                    // (`shared_auto_clone`) is COVERED for the plain-fn / distinct-ctor /
                    // unqualified-import paths — all route through `lower_one_call_arg`,
                    // which reproduces `emit_call_args`' `Arc::clone(&…)` from the total
                    // flag (and the receiving `Shared<T>` param renders identically via the
                    // shared `rust_param_type`). It stays EXCLUDED on the `is_extern` path
                    // only: extern args use `lower_extern_call_arg`, which does not carry
                    // the Arc form (the FFI boundary takes a `(…).clone()`, not an Arc).
                    // Labels are sema-only (documentation), checked at their own position.
                    (!a.flags.shared_auto_clone || !is_extern)
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
            // c109 Phase 30: a TRAIT-OBJECT coercion (S48 — `Circle {…}` in a `[Shape]`
            // list). The AST wraps the rendered literal `Box::new(<lit>) as Box<dyn
            // user_<Trait>>` (`emit_struct_lit`'s `as_trait` branch). Covered when the trait
            // is a known user trait and the base is a PLAIN covered user struct (no import_ns,
            // no type_args — a coerced foreign/generic literal is not a construct any covered
            // program produces, so stay conservative and require the plain form). The fields
            // are checked in the plain-struct path below. A coercion to a non-trait name (or a
            // foreign/generic coerced literal) stays excluded.
            if let Some(trait_name) = as_trait {
                if !cx.trait_names.contains(trait_name)
                    || import_ns.is_some()
                    || !type_args.is_empty()
                    || is_prelude_struct_name(type_name)
                    || !is_covered_struct_ty(&Type::Named(type_name.clone()), cx)
                {
                    return false;
                }
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 19: a FOREIGN (imported user) struct literal — a `import_ns`
            // namespace head (`{root}{mod}::{user_<Name>}[::<args>]`, mangled fields).
            // Covered when the named foreign type is a covered foreign struct and the
            // import alias resolves; the head is resolved at lowering (`lower_expr`).
            if import_ns.is_some() {
                return foreign_struct_lit_in_subset(
                    type_name,
                    type_args,
                    import_ns.as_deref(),
                    cx,
                ) && fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`
            // → the turbofish `user_Pair::<T> { … }`). The base must be a covered struct
            // and every type arg covered/type-var (`is_covered_generic_struct_ty`). The
            // turbofish head is resolved at lowering via `user_type_apply_rust`.
            if !type_args.is_empty() {
                if !is_covered_generic_struct_ty(
                    &Type::Apply {
                        name: type_name.clone(),
                        args: type_args.clone(),
                    },
                    cx,
                ) {
                    return false;
                }
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109: an UNqualified cross-module FOREIGN struct literal (`Note { … }` with
            // no `import_ns` — sema resolves the bare imported type, no `use` of the type
            // needed). The AST `emit_struct_lit` plain branch now prefixes the foreign
            // module (`{root}user_<mod>::user_<Note>`) via `user_type_apply_rust`,
            // reproduced at lowering. Cover it when the type is a registered foreign type
            // (`cx.foreign_types`); the field VALUES are checked in-subset below. (A
            // foreign type is NOT a `is_covered_struct_ty` — its fields live in another
            // module — so this needs its own admission.)
            if cx.foreign_types.contains_key(type_name) {
                return fields.iter().all(|(_, _, e)| expr_in_subset(e, cx, locals));
            }
            // c109 Phase 17: a PRELUDE struct literal (HttpRequest/HttpResponse) — the
            // `is_prelude_struct` branch of `emit_struct_lit` (a `<root>Jet…` head, PLAIN
            // field names, and an auto `params: BTreeMap::new()` for HttpRequest).
            // Reproduced in `lower_expr`'s StructLit arm. Otherwise the named type must be a
            // covered user struct (`user_<name>` head, mangled fields).
            // c109: a recursive (boxed) struct is CONSTRUCTIBLE (the boxed field value is
            // wrapped `Box::new(…)`, a total fact at lowering) even though it is not a
            // covered VALUE type (a boxed field READ needs deref, kept on the AST path).
            if !is_prelude_struct_name(type_name)
                && !is_covered_struct_ty(&Type::Named(type_name.clone()), cx)
                && !struct_lit_constructible(type_name, cx, &mut HashSet::new())
            {
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
                // c109 Phase 24: the `JSON.Null` unit construction reaches codegen as a
                // `Field` (the AST `emit_expr` Field arm emits `{root}jet_std::Json::Null`,
                // Expression.rs ~L222). Cover it (the only no-arg JSON variant).
                if !locals.contains(enum_name)
                    && is_json_type_name(enum_name)
                    && member == "Null"
                {
                    return true;
                }
                // c109 Phase 28: a numeric BOUNDS constant (`U8.MAX`/`I32.MIN`/
                // `Float.INFINITY`/… — D-NUMOPS1) reaches codegen as a `Field` whose
                // receiver is a numeric type NAME and `member` is one of the per-type
                // const names. The AST `emit_expr` Field arm (Expression.rs ~L224) emits
                // `{rust_type}::{member}` (e.g. `u8::MAX`, `f64::INFINITY`). Cover it: a
                // non-local numeric type name + a known const member. The rendered value
                // + result type are resolved at lowering (`numeric_type_from_name`).
                if !locals.contains(enum_name)
                    && crate::AST::numeric_type_from_name(enum_name).is_some()
                    && is_numeric_bounds_const(member)
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
            // c109: a boxed (recursive) struct field READ (`t.child` where the field is
            // `Box<…>`) is now covered — the read derefs the Box (`(*(…))`, a total
            // `boxed` fact lowered from `cx.boxed_edges`, mirroring the AST `boxed_field_read`).
            // In-subset iff the receiver is.
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
        // c109 Phase 23: a named-tuple literal `(x: 1, y: 2)` (S73/D-SG7). Covered when
        // sema resolved the tuple TYPE (`ty.is_some()` — the canonical field order +
        // struct name come from it; an unresolved `ty` would force the AST's empty-
        // canonical `0i64` default, which the TIR must not guess) and every field value
        // is in-subset. The literal's values are reordered to the type's canonical field
        // order at lowering, reproducing `emit_expr`'s `TupleLit` arm.
        Expr::TupleLit(fields, _, ty) => {
            matches!(ty, Some(Type::Tuple(_)))
                && fields.iter().all(|(_, e)| expr_in_subset(e, cx, locals))
        }
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
        // c109 Phase 23: a `#Todo` typed hole. Covered when sema filled the expected
        // type (`expected_type.is_some()`); a `None` (sema didn't run/resolve) stays on
        // the AST path so the TIR never guesses the `(unknown)` fallback.
        Expr::Todo { expected_type, .. } => expected_type.is_some(),
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
        // c109 Phase 11: a lambda/closure literal. Covered when its body is in-subset
        // (lowered on the outer scope extended with the lambda's params + cloned
        // captures) and every capture/escape decision is a total `Lambda.meta` fact.
        Expr::Lambda(lam) => lambda_in_subset(lam, cx, locals),
        // c109 Phase 11: fan-out `f.[a, b, c]` (S75/S76). Covered when the callee is
        // in-subset (a plain top-level fn ident, or any in-subset callee value) and
        // every item is in-subset.
        Expr::FanOut { callee, items, .. } => {
            fan_out_callee_in_subset(callee, cx, locals)
                && items.iter().all(|i| expr_in_subset(i, cx, locals))
        }
        // c109 Phase 13: a call THROUGH a fn-value `(f)(args)` (`Expr::CallValue`).
        // Covered when the callee is in-subset (a fn-typed local, a fn-name value, or
        // a lambda) and every arg is in-subset. The AST path emits `({callee})({args})`
        // with args lowered plainly (`emit_call_args(.., None, ..)`), so no convention
        // facts are needed — any in-subset arg works; labels are still excluded.
        Expr::CallValue { callee, args, .. } => {
            expr_in_subset(callee, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals))
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (`Expr::PtrFromAddr`, S58). The
        // address expr must be in-subset. The cast itself is safe Rust (no `unsafe`); it
        // is only constructible inside `use core.mem` + an `#Unsafe` region (sema
        // E3101/E3102), so it never appears in a non-unsafe context. `elem` is a total
        // type on the node — emit needs no inference.
        Expr::PtrFromAddr { addr, .. } => expr_in_subset(addr, cx, locals),
        // Everything else (tuples, deref, …) is out.
        _ => false,
    }
}

/// c109 Phase 13: is `name` a bare top-level function used as a VALUE? It must be a
/// non-local, non-const name in `cx.fn_types` whose type is a `Type::Fn`. Such a name
/// emits `emit_named_fn_value`'s `Box::new(move |…| user_<name>(…)) as <fn-type>`
/// (Source/Codegen/Statement.rs). A const (inlined value) or an unqualified module
/// import is NOT a fn-value, so this stays narrow.
fn ident_is_named_fn_value(name: &str, cx: &Cx, locals: &HashSet<String>) -> bool {
    !locals.contains(name)
        && !cx.consts.contains_key(name)
        && matches!(cx.fn_types.get(name), Some(Type::Fn { .. }))
}

/// c109 Phase 8/15: is a `??` fallback right-hand side in-subset? `Value` and early
/// `return [expr]` are covered (Phase 8). c109 Phase 15: the `panic(…)` form is now
/// covered too — `emit_panic_stop`/`safe_locals_expr` is reproduced from a faithful
/// `panic_locals` env replica resolved at lowering. The panic message expression must
/// be in-subset (it is lowered into the rendered panic string). `panic(…)` always takes
/// exactly one message argument (the parser builds `OrFallback::Panic{args}` from it).
fn orfallback_rhs_in_subset(fallback: &OrFallback, cx: &Cx, locals: &HashSet<String>) -> bool {
    match fallback {
        OrFallback::Value(e) => expr_in_subset(e, cx, locals),
        OrFallback::Return(None, _) => true,
        OrFallback::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        OrFallback::Panic { args, .. } => {
            args.len() == 1
                && args[0].label.is_none()
                && expr_in_subset(&args[0].expr, cx, locals)
        }
    }
}

/// c109 Phase 11: is a lambda/closure literal in-subset? The body must be entirely
/// in-subset when classified against the outer scope extended with the lambda's
/// params (new locals) and its captures. The capture/escape/Fn-vs-FnMut facts are
/// all total (`Lambda.meta`), so nothing is re-derived; the gate only proves the
/// body lowers. A `take_names` capture is an outer local (already in `locals`); a
/// param shadows. The body sees: outer locals (captures resolve via them — the AST
/// rebinds a cloned capture to `_jet_cap_<n>` but the *name* stays in scope) plus
/// the params.
fn lambda_in_subset(lam: &Lambda, cx: &Cx, locals: &HashSet<String>) -> bool {
    let mut body_locals = locals.clone();
    for p in &lam.params {
        body_locals.insert(p.name.clone());
    }
    match &lam.body {
        LambdaBody::Expr(e) => expr_in_subset(e, cx, &body_locals),
        LambdaBody::Block(stmts) => stmts
            .iter()
            .all(|s| stmt_in_subset(s, cx, &mut body_locals)),
    }
}

/// c109 Phase 11: is a fan-out callee (`f` in `f.[a, b, c]`) in-subset? The AST
/// path routes an `Ident` callee through `emit_call` (handling builtins) and any
/// other callee through `(f)(item)` (a fn-value call). We cover ONLY the cleanest,
/// byte-reproducible case: an `Ident` that resolves to a *plain top-level function*
/// (in `cx.sigs`, not a local, not an extern/FFI or unqualified-module-import call,
/// not a builtin like `print`/`panic`). Those lower exactly as the Phase-1 `Call`
/// arm does (a synthetic single-arg call). A fn-value callee (`(f)(item)`) needs the
/// deferred Fn-typed-value emit, so it stays on the AST path.
fn fan_out_callee_in_subset(callee: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    let Expr::Ident(name, _) = callee else {
        return false;
    };
    !locals.contains(name)
        && cx.sigs.contains_key(name)
        && !cx.extern_funcs.contains_key(name)
        && !cx.unqualified_inline.contains_key(name)
        && !cx.unqualified_file.contains_key(name)
        // Exclude the ambient builtins `emit_call` special-cases before the plain
        // dispatch (a user-defined fn of the same name is in `cx.sigs`, so the
        // `contains_key` above already admits it — but a bare builtin name with no
        // user sig would have failed `contains_key`; guard anyway for clarity).
        && name != Syntax::BUILTIN_PRINT
        && name != Syntax::BUILTIN_PANIC
        && name != Syntax::BUILTIN_INPUT
        && name != Syntax::BUILTIN_REQUIRE
        && name != Syntax::BUILTIN_REQUIRE_EQ
        && name != Syntax::BUILTIN_EXPECT
        && name != Syntax::BUILTIN_WRAPPING
        && name != Syntax::BUILTIN_SATURATING
        && name != Syntax::BUILTIN_CHECKED
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
    // c109 Phase 23: `.raw()` on a distinct type (D-DIST3). The AST `emit_method_call`
    // special-cases `method == "raw"` BEFORE any user dispatch, unconditionally emitting
    // `({recv}).0`. Sema (CheckerInfer ~L2039) admits `.raw()` ONLY on a distinct-type
    // value (E0311 otherwise), so any `.raw()` that survives to codegen is on a distinct
    // — covering an in-subset 0-arg `.raw()` is safe (and `recv_type` is `None` here,
    // since sema's `.raw()` arm returns the base type without the recv_type writeback).
    if method == Syntax::METHOD_DISTINCT_RAW {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    // Shape (m) [c109 Phase 27]: a CALL THROUGH a fn-typed struct field — `w.step(4)`
    // where `step: fn(Int) -> Int` is a field on a covered struct, NOT a user method.
    // Sema (CheckerInfer ~L2329) sets `recv_type == Some(<StructType>)` and re-routes the
    // node through `infer_call_value`, but registers NO `method_sigs` entry (it is a field,
    // not a method). The AST `emit_method_call` (Expression.rs ~L1573) detects this case
    // FIRST — a `struct_fields` entry whose type is `Type::Fn` — and emits
    // `(({recv}).{user_<field>})({args})` with PLAIN args (`emit_call_args(.., None, ..)`).
    // We mirror that order: tried before the user-method/static shapes so a fn-field whose
    // name happens to match a method name resolves to the field, exactly as the AST path.
    if fn_field_call_in_subset(receiver, method, args, recv_type, cx, locals) {
        return true;
    }
    // Shape (l) [c109 Phase 24]: a JSON construction `JSON.Boolean(b)` / `JSON.Number(n)`
    // / `JSON.Text(s)` / `JSON.Array(xs)` / `JSON.Object(map)`. The receiver is the
    // bare `Ident("JSON")` (a type name, NOT a local), and `method` is a JSON variant.
    // Sema (`check_core_json_lit`) types it as `JSON` WITHOUT setting `recv_type` (so
    // `recv_type == None`), and the AST `emit_method_call` routes it through
    // `emit_core_json_lit` (Expression.rs ~L1633) BEFORE the user enum-lit / instance
    // shapes. Tried here FIRST among the type-name receivers: `JSON` is not a core
    // import alias (so the core shape declines), not a local, not a user enum/struct
    // name. The single payload arg must be in-subset. `JSON.Null` is the no-arg Field
    // form, handled in `expr_in_subset`'s `Field` arm, not here.
    if let Expr::Ident(type_name, _) = receiver {
        if !locals.contains(type_name)
            && is_json_type_name(type_name)
            && is_json_variant(method)
        {
            return args.iter().all(|a| {
                a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
            });
        }
    }
    // Shape (e) [c109 Phase 10]: a core/stdlib module call `alias.method(args)` where
    // `alias` is a core import. Sema leaves `recv_type == None` for core calls
    // (`infer_core_call` returns without setting it). A core call is uniquely a
    // `MethodCall` whose receiver is an `Ident(alias)` with `alias ∈ cx.core_imports`
    // — disjoint from the builtin shape (which needs a *value* receiver) and the
    // static shape (a covered *type-name* receiver). Tried BEFORE the builtin shape:
    // a core method named `get`/`split`/… would otherwise be claimed (and rejected,
    // since a module alias is not a local) by the builtin shape's `return`. The
    // covered set is the type-monomorphic core calls (`core_call_covered`); the
    // polymorphic math/random/io specials + every closure-taking / handle-constructor
    // call stay on the AST path.
    if recv_type.is_none() {
        if let Expr::Ident(alias, _) = receiver {
            if !locals.contains(alias) {
                if let Some(module) = cx.core_imports.get(alias) {
                    // c109 Phase 13: the three closure-taking core calls (`tasks.spawn`/
                    // `http.serve`/`scope.guard`) — NOT in `core_fixed_sig`, each a
                    // bespoke emit shape with a literal-lambda closure arg.
                    if core_closure_call_in_subset(module, method, args, cx, locals) {
                        return true;
                    }
                    return core_call_covered(module, method)
                        && args.iter().all(|a| {
                            a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
                        });
                }
                // Shape (i) [c109 Phase 14]: a qualified cross-module call
                // `alias.method(args)` — a `pub use` re-export (`reexport_calls`), a
                // file/dir-module import (`import_mods`), or an inline code module
                // (`code_modules`). The AST `emit_method_call` checks these in this
                // exact order (after `core_imports`, already handled above). Each
                // lowers to its resolved `{root}{mod}::{fn}` / `{root}user_{a}__{m}`
                // form. Args carry their import-signature conventions, reproduced via
                // `lower_one_call_arg`; the Arc form stays excluded.
                let is_module_alias = cx
                    .reexport_calls
                    .contains_key(&(alias.clone(), method.to_string()))
                    || cx.import_mods.contains_key(alias)
                    || cx.code_modules.contains(alias.as_str());
                if is_module_alias {
                    return args.iter().all(|a| {
                        a.label.is_none()
                            && !a.flags.shared_auto_clone
                            && arg_conv_in_subset(a)
                            && expr_in_subset(&a.expr, cx, locals)
                    });
                }
            }
        }
    }
    // Shape (k) [c109 Phase 19]: the arena allocator constructor `mem.Arena.new(…)`
    // (D-ALLOC1). The receiver is `Field(Ident(mem-alias), <AllocType>)`, method `new`.
    // Sema sets `recv_type == Some(<AllocType>)` (the receiver `mem.Arena` is typed
    // `Named(Arena)` via `infer_core_field`, then `.new()` dispatches through
    // `alloc_method_return`). The AST `emit_method_call` claims it via its FIRST branch
    // (the `mem.<Alloc>.new()` constructor, Expression.rs ~L1515) BEFORE any `rty`-keyed
    // arm — so we mirror that and try it FIRST, before the handle shape. The optional
    // `capacity:`/`slots:`/`size:` arg is admitted (a label is allowed HERE — the AST reads
    // `arg(0)` ignoring the label, choosing the ctor by allocator type, not label).
    if alloc_new_type(receiver, method, cx, locals).is_some() {
        return args.len() <= 1
            && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d) [c109 Phase 9]: a built-in collection/string method
    // (`emit_builtin_method`) — `len`/`push`/`get`/`keys`/`trim`/`split`/… on a
    // list/map/string receiver. Sema resolves these via `Collections::
    // builtin_method_return` and leaves `recv_type == None` (it sets `recv_type`
    // only for the numeric width conversions — Phase 12 — and for user instance /
    // handle methods). So `recv_type.is_none()` + a covered builtin name + an
    // in-subset *value* receiver uniquely identifies a builtin collection/string
    // call: the receiver must be a collection/string (the program type-checked, and
    // a struct/enum/handle/numeric receiver would have set `recv_type`). A bare
    // type-name ident (a static-call receiver) is NOT in `locals`, so it fails
    // `expr_in_subset` and is excluded here, falling through to the static shape.
    //
    // The Map-vs-List-vs-String emit branch (`rty = expr_jet_ty(receiver)`) is
    // resolved at LOWERING from the receiver's total type (reproducing the AST's
    // `expr_jet_ty`, incl. its `None` → default-branch partiality), never re-derived
    // in emit. Tried BEFORE the static/instance shapes (both keyed on the same
    // `recv_type`) to claim builtins first.
    if recv_type.is_none() && is_covered_builtin_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args.iter().all(|a| {
                a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
            });
    }
    // Shape (d2) [c109 Phase 19]: `Stopwatch.elapsed_millis()`. The AST
    // `emit_builtin_method` dispatches `elapsed_millis` on the method NAME alone (it
    // fires before any `rty` test, Expression.rs ~L1023), and sema types it via
    // `Collections::stopwatch_method_return` — leaving `recv_type == None` (NOT the
    // `Some(<handle>)` of the Phase-13 handle shape). So it is a Phase-9-style builtin
    // gap: a `MethodCall` with `recv_type == None`, a covered builtin name, an in-subset
    // value receiver (a `Stopwatch` `let`-bound from the covered `time.start` producer).
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`). Tried after the collection builtins so a list/map/string
    // `elapsed_millis` (impossible — no such method) can't be misclaimed.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        return expr_in_subset(receiver, cx, locals);
    }
    // Shape (d3) [c109 Phase 21]: a Task/Channel/Sender concurrency method. Like
    // Stopwatch (d2), sema types these via `Collections::builtin_method_return`'s
    // `Type::Apply` arms (`task_method_return`/`channel_method_return`/
    // `sender_method_return`, Source/Collections.rs) and leaves `recv_type == None` (a
    // Phase-9 builtin gap). The AST `emit_builtin_method` dispatches them on the method
    // NAME alone (`join`/`detach`/`receive`/`sender`/`send`). The names + arg counts are
    // disjoint from every other shape: `Task.join()` is the 0-arg `join` (the 1-arg list
    // `join(sep)` is claimed by shape d above); `detach`/`receive`/`sender` (0 args) and
    // `send` (1 arg) are used by no other builtin. The receiver is a `Task`/`Channel`/
    // `Sender` value `let`-bound from a covered producer (`tasks.channel()` / `ch.sender()`
    // / `tasks.spawn(…)`). Tried after the collection builtins so a list/map/string method
    // can't be misclaimed.
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d4) [c109 Phase 24]: `Match.group(n)` (D-REGEX1). Sema sets `recv_type ==
    // Some("Match")` (the `Match` receiver type, CheckerInfer's user/handle-method
    // writeback), and the AST `emit_builtin_method` dispatches it on the method NAME
    // guarded by `rty == Some(Named("Match"))` (Expression.rs ~L1132). Keyed on
    // `recv_type == Some("Match")` + `group`/1 — disjoint from every user instance method
    // (whose `recv_type` is a covered struct/enum, never `Match`) and from the numeric
    // shape (a numeric `recv_type`). The receiver is a `Match` value (`if m == value(mat)`
    // binding). Lowered to `BuiltinMethod`/`TBuiltinOp::MatchGroup`. The result is `String?`.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (f) [c109 Phase 11]: a closure-taking collection method (`map`/`filter`/
    // `each`/`find`/`any`/`all`/`sort_by`/`reduce`). Like the Phase-9 builtin shape it
    // carries `recv_type == None` and an in-subset *value* receiver. The Fn-vs-FnMut
    // emit branch reads the lambda arg's `needs_fn_mut` meta, so the closure-arg
    // position MUST be a literal `Expr::Lambda` (a fn-value there defaults to the
    // non-mut form on the AST side, but covering that needs the deferred fn-value
    // emit — exclude). `reduce` takes (seed, lambda); the rest take (lambda).
    if recv_type.is_none() && closure_method_in_subset(method, args, cx, locals) {
        return expr_in_subset(receiver, cx, locals);
    }
    // Shape (g) [c109 Phase 12]: a numeric predicate / bit-population / width
    // conversion (`is_nan`/`count_ones`/`to_i32`/… — D-NUMOPS1). Sema sets
    // `recv_type == Some(<numeric name>)` for a numeric receiver (CheckerInfer
    // ~L2248), so a numeric method is uniquely a `MethodCall` whose `recv_type` parses
    // as a numeric type name (`Int`/`Float`/`F32`/`I8..U64`) and whose `method` is a
    // covered numeric op. All numeric ops are nullary (no args). The width source is
    // the total `recv_type`, so the widening/narrowing decision is total at lowering.
    if let Some(numeric_name) = recv_type {
        if crate::AST::numeric_type_from_name(numeric_name).is_some()
            && is_covered_numeric_method(method, args.len())
        {
            return expr_in_subset(receiver, cx, locals);
        }
    }
    // Shape (h2) [c109 Phase 25]: HttpRouter route registration `router.get(path, handler)`
    // / `.post`/`.put`/`.delete` (D-ROUTE1=A). Sema sets `recv_type == Some("HttpRouter")`.
    // The AST `emit_builtin_method` keys these on `rty == Some(HttpRouter)` BEFORE the
    // generic `get`/`post` collection arms, and emits the handler via `emit_router_handler`
    // (a boxed `Fn(HttpRequest)->HttpResponse` closure). We cover it when the receiver is
    // in-subset, the path arg is in-subset, and the handler arg is one `emit_router_handler`
    // reproduces byte-for-byte: a bare top-level-fn name (NOT a local → the `Box::new(move
    // |__req| user_<fn>(&__req)) as …` wrapper) or an in-subset literal lambda. Tried BEFORE
    // the numeric/handle/builtin shapes so the HttpRouter `get`/`post` is claimed here.
    if recv_type.as_deref() == Some("HttpRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        return router_register_in_subset(receiver, args, cx, locals);
    }
    // Shape (h) [c109 Phase 13]: a method ON a handle (FileReader/FileWriter/
    // StdinHandle/Stopwatch/TcpListener/TcpStream). Sema sets `recv_type ==
    // Some(<handle>)` (CheckerInfer, via the handle `*_method_return` tables). The AST
    // emit branch (`emit_builtin_method`) keys on `rty = expr_jet_ty(receiver)`; for
    // these handles the receiver is ALWAYS a `let`-bound local from a covered
    // handle-producing core call (`files.open`/`time.start`/`net.tcp_connect`/…) or
    // another covered handle method (`listener.accept()`), so its slot type is total
    // (`Some(<handle>)`) — `rty == recv_type` always, and the rty-keyed branch fires
    // identically. (c109 Phase 20: HttpRequest/HttpResponse accessors are NOW covered —
    // sema writes the `http.serve` lambda-param type back onto `p.ty`, so the slot type
    // is total even for an unannotated `(req)` param; the AST `rty`-keyed handle arm then
    // fires identically. They join `handle_method_op`.) Disjoint from
    // the numeric shape (a handle name isn't numeric) and the instance/static shapes
    // (a handle name isn't a covered struct/enum).
    if let Some(handle) = recv_type {
        if handle_method_op(handle, method, args.len()).is_some() {
            return expr_in_subset(receiver, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // Shape (j) [c109 Phase 16]: an enum-variant CONSTRUCTION `Enum.Variant(args)`.
    // The parser/sema never produce an `Expr::EnumLit` node for a payload variant —
    // a `Type.Variant(args)` stays a `MethodCall` (sema type-checks it via
    // `check_enum_lit` in place but does NOT rewrite the node). The AST `emit_method_call`
    // (Expression.rs ~L1635) routes such a call to `emit_enum_lit` when the receiver is
    // a known enum and `method` is a variant. This is THE shape that constructs
    // string/struct/collection-payload and recursive (boxed) enum values. We cover it
    // when the enum is covered and every (positional) arg is in-subset; the
    // borrowed-clone/`Box::new` decisions are resolved at lowering (`lower_enum_arg`),
    // reproducing `emit_boxed_enum_arg` byte-for-byte. Tried BEFORE the static shape
    // (which excludes variants), matching the AST dispatch order.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !locals.contains(type_name) {
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        return enum_is_covered(type_name, cx)
                            && args.iter().all(|a| {
                                a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
                            });
                    }
                }
            }
        }
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
    // Shape (n) [c109 Phase 30]: DYNAMIC dispatch on a TRAIT-OBJECT receiver
    // (`s.name()`/`s.area()` where `s: Shape` is a `Box<dyn user_Shape>`). Sema sets
    // `recv_type == Some(<trait>)` with the trait in `cx.trait_names`; the AST
    // `emit_method_call` (Expression.rs ~L1657) keys on `cx.trait_names.contains(rt)` and
    // emits `({recv}).{method}({args})` — the BARE method name (vtable dispatch), args
    // lowered PLAINLY (`emit_call_args(.., None, ..)`). Disjoint from the user-instance
    // shape below (a trait name is never a covered struct/enum) and from the
    // numeric/handle/builtin shapes (a trait name isn't any of those). Covered when the
    // receiver is in-subset and every arg is in-subset + unlabeled.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            return expr_in_subset(receiver, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // Shape (b): a user-defined instance method. The `recv_type` is the TOTAL sema
    // fact; a `None` was handled above (static). Anything else (a fallback-inferred
    // path) the subset must NOT reproduce — but `recv_type == Some` is the total
    // instance-method signal.
    let Some(ty) = recv_type else {
        return false;
    };
    // The method must be a user-defined method on that type (in `method_sigs`). A real
    // `method_sigs` entry is the TOTAL "this is a user method on `ty`" signal: the AST
    // `emit_method_call` now dispatches to the user method (`user_<method>`) BEFORE
    // `emit_builtin_method` whenever `recv_type == Some(T)` and `(T, method) ∈ method_sigs`
    // (the builtin-name-collision fix), so a user method SHADOWING a builtin name
    // (`get`/`len`/…) routes here, not through the name-keyed builtin path.
    let Some(sig) = cx.method_sigs.get(&(ty.clone(), method.to_string())) else {
        // No user method: a name a core/stdlib/builtin/special lowering would intercept
        // *before* the user dispatch (`emit_builtin_method`, the `.raw()`/`.snapshot()`/
        // alloc special cases) has bespoke name-keyed lowering — exclude it (those are
        // covered by their own shapes, not the user-method TIR).
        return false;
    };
    // A builtin-name method with NO `method_sigs` entry was already excluded above. With a
    // real user method present, the intercepted-name check (`is_intercepted_method_name`,
    // still used by the static-call shape) no longer applies — the AST path dispatches to
    // the user method. The `clone`/`raw` special forms returned earlier in this function;
    // `snapshot`/`new` fire their AST special cases only for non-instance receivers (an
    // `expect(...)` call / a type-name ident), so an INSTANCE method of that name with a
    // `method_sigs` entry reaches here and routes to the user method on BOTH paths.
    // The receiver type must be a covered struct or enum (so the receiver place
    // emits exactly as the AST path does, and the method is a plain user method).
    let recv_ty = Type::Named(ty.clone());
    if !is_covered_struct_ty(&recv_ty, cx) && !is_covered_enum_ty(&recv_ty, cx) {
        return false;
    }
    // The receiver expression must itself be in-subset (a covered local/param/field).
    if !expr_in_subset(receiver, cx, locals) {
        return false;
    }
    // Arity must match the resolved signature (sema guaranteed it, but be defensive).
    if args.len() != sig.len() {
        return false;
    }
    // Every argument must be in-subset. Unlike a plain call, a method arg MAY use any
    // of `Read`/`Move`/`Mutate` with implicit/Arc clone — those are carried as total
    // flags and emitted verbatim (mirroring `emit_call_args`). c109 Phase 13: a Fn-typed
    // param routes through the `Box::new(…) as <fn-type>` coercion (`lower_one_call_arg`).
    // c109 Phase 23: a call-site LABEL (`r.scale(factor: 0.5)`, D-NARG1) is allowed —
    // labels are sema-validated documentation that never reorder (D-NARG-D4) and codegen
    // never reads `CallArg.label`, so a labeled arg emits identically.
    args.iter().zip(sig.iter()).all(|(a, (_, _pty))| {
        expr_in_subset(&a.expr, cx, locals)
    })
}

/// c109 Phase 27: is `recv.method(args)` a call THROUGH a fn-typed struct FIELD (not a
/// user method)? Returns the field's `Type::Fn` when so. `recv_type` is the total sema
/// fact (`Some(<StructType>)`, set by CheckerInfer's fn-field arm); the field must exist
/// on a COVERED struct with a `Type::Fn` type and the same name as `method`. Mirrors the
/// AST `emit_method_call` fn-field branch's guard (`struct_fields` lookup + `Type::Fn`).
fn fn_field_call_ty<'a>(
    method: &str,
    recv_type: &Option<String>,
    cx: &'a Cx,
) -> Option<&'a Type> {
    let ty_name = recv_type.as_ref()?;
    if !is_covered_struct_ty(&Type::Named(ty_name.clone()), cx) {
        return None;
    }
    let fields = cx.struct_fields.get(ty_name)?;
    let (_, fty) = fields.iter().find(|(n, _)| n == method)?;
    matches!(fty, Type::Fn { .. }).then_some(fty)
}

/// c109 Phase 27: is `w.step(4)` (a call through a fn-typed struct field) in-subset? The
/// receiver and every arg must be in-subset; args are emitted PLAINLY by the AST path
/// (`emit_call_args(.., None, ..)`), so no convention/Arc-clone fact applies — exclude any
/// labeled / Arc-clone arg defensively (sema never produces them here, but stay strict).
fn fn_field_call_in_subset(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if fn_field_call_ty(method, recv_type, cx).is_none() {
        return false;
    }
    expr_in_subset(receiver, cx, locals)
        && args.iter().all(|a| {
            a.label.is_none()
                && !a.flags.shared_auto_clone
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
    // c109 Phase 25: a STATIC constructor `Type.new(args)` is the Phase-7 static-call
    // shape (`recv_type == None`, receiver a covered type-name ident, `(Type, "new") ∈
    // method_sigs`) — NOT a builtin/instance intercept. `emit_builtin_method` has no
    // `new` arm, and the only `new` special-case (`MEM_ALLOC_NEW`, D-ALLOC1) fires ONLY
    // for a `Field(mem_alias, AllocType)` receiver, never an `Ident(Type)` receiver. So
    // the AST path falls through `emit_builtin_method` (returns None) to the type-name
    // static dispatch (Expression.rs ~L1644) → `user_<Type>::user_new(args)` — exactly
    // what the StaticCall lowering reproduces. We therefore admit `new` HERE (the
    // static shape) while `is_intercepted_method_name` keeps the INSTANCE-method intercept
    // (shape b) whole: a user instance method named `new`/`get`/… stays on the AST path.
    if method != Syntax::MEM_ALLOC_NEW && is_intercepted_method_name(method) {
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
    // c109 Phase 13: a Fn-typed static-method param routes through the Box-coercion
    // (`lower_one_call_arg`). c109 Phase 23: a call-site LABEL (`Rect.new(width: 4.0)`,
    // D-NARG1) is allowed — labels never reorder (D-NARG-D4) and codegen ignores them.
    args.iter().zip(sig.iter()).all(|(a, (_, _pty))| {
        expr_in_subset(&a.expr, cx, locals)
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
fn arg_conv_in_subset(_a: &crate::AST::CallArg) -> bool {
    // c109 Phase 26: ALL three call-arg conventions are now in-subset. `Read` (`&(…)`
    // for a non-scalar) and `Move` (plain value — `take`-marked args, `08_ownership`'s
    // `archive(take "vault")`) were already admitted; `Mutate` (`&mut (…)`,
    // `bump(mut score)`) is the lift. `lower_one_call_arg` already resolves all three
    // borrow wrappers from the sig convention (`emit_call_args`' `match conv` —
    // `Read`/non-scalar → `&(…)`, `Mutate` → `&mut (…)`, else plain), reproduced
    // byte-for-byte in `emit_tir_call_args`. No convention is excluded.
    true
}

/// c109 Phase 11: is `method` a closure-taking collection method the TIR lowers,
/// with in-subset args? Covers `map`/`filter`/`each`/`find`/`any`/`all`/`sort_by`
/// (1 arg: a lambda) and `reduce` (2 args: a seed value + a lambda). The closure-arg
/// position MUST be a literal `Expr::Lambda` (the Fn-vs-FnMut emit branch reads its
/// `needs_fn_mut` meta; a fn-value there defaults to the non-mut form on the AST
/// side, but covering it needs the deferred fn-value emit). The seed (`reduce`) and
/// the lambda body must be in-subset. No labels.
fn closure_method_in_subset(
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if !crate::Collections::is_closure_method(method) {
        return false;
    }
    if args.iter().any(|a| a.label.is_some()) {
        return false;
    }
    match method {
        "reduce" => {
            // (seed, lambda). The seed is any in-subset value; the lambda must be a
            // literal in-subset closure.
            args.len() == 2
                && expr_in_subset(&args[0].expr, cx, locals)
                && matches!(&args[1].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals))
        }
        // (lambda). map/filter/each/find/any/all/sort_by.
        _ => {
            args.len() == 1
                && matches!(&args[0].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals))
        }
    }
}

/// c109 Phase 9: is `method` (with `nargs` arguments) a built-in collection/string
/// method the TIR lowers? This is the NON-closure, non-numeric, non-handle slice of
/// `emit_builtin_method` (Source/Codegen/Expression.rs), restricted to the list/map/
/// string surface (`Source/Collections.rs`). The closure-taking methods (`map`/
/// `filter`/`each`/`find`/`any`/`all`/`sort_by`/`reduce` — `Collections::
/// is_closure_method`) are deferred to the lambda phase; the numeric width/predicate/
/// bit methods (`to_i32`/`is_nan`/`count_ones`/… — D-NUMOPS1) and the handle methods
/// (FileWriter/TcpStream/HttpRequest/… — Phase 10) carry a `Some(recv_type)`, so the
/// gate's `recv_type.is_none()` guard already excludes them; this name list is the
/// final filter. The arg count disambiguates `join()` (no separator) vs `join(sep)`.
fn is_covered_builtin_name(method: &str, nargs: usize) -> bool {
    // Closure-taking methods are NEVER covered here (Phase 11), even by name.
    if crate::Collections::is_closure_method(method) {
        return false;
    }
    matches!(
        (method, nargs),
        // List + map shared.
        ("len", 0) | ("is_empty", 0) | ("clear", 0)
        // List-only.
        | ("push", 1) | ("pop", 0) | ("first", 0) | ("last", 0)
        | ("index_of", 1) | ("reverse", 0) | ("sort", 0) | ("join", 1)
        // List + map: insert/remove/get (the Map vs List branch resolves at lowering).
        | ("insert", 2) | ("remove", 1) | ("get", 1)
        // List + string: contains.
        | ("contains", 1)
        // Map-only.
        | ("keys", 0) | ("values", 0) | ("contains_key", 1)
        // String-only.
        | ("chars", 0) | ("bytes", 0) | ("trim", 0) | ("split", 1)
        | ("starts_with", 1) | ("ends_with", 1) | ("replace", 2)
        | ("to_upper", 0) | ("to_lower", 0) | ("repeat", 1) | ("slice", 2)
        // `to_string` (String/Bool/Char receiver — those carry `recv_type == None`;
        // a numeric `to_string` sets `recv_type` and so is excluded by the guard).
        | ("to_string", 0)
    )
    // NOTE: `is_empty` (now Bool-typed in `Collections::*_method_return` after the
    // c109 fix; lowered to `TBuiltinOp::IsEmpty`) is covered above. `join()` (no
    // separator) stays excluded: sema requires `join(sep)` (E0311 on no-arg), so the
    // no-arg form never reaches codegen — its AST arm is dead.
}

/// c109 Phase 21: is `(method, nargs)` a Task/Channel/Sender concurrency method
/// (`emit_builtin_method`'s `Type::Apply`-receiver arms)? `Task.join()`/`Task.detach()`,
/// `Channel.receive()`/`Channel.sender()`, `Sender.send(v)`. The arg count disambiguates
/// `Task.join()` (0 args) from the list `join(sep)` (1 arg, shape d) and `Sender.send(v)`
/// (1 arg) — every name+arity here is disjoint from every other covered shape.
fn is_concurrency_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("join", 0) | ("detach", 0) | ("receive", 0) | ("sender", 0) | ("send", 1)
    )
}

/// c109 Phase 12: resolve a numeric method (`is_nan`/`count_ones`/`to_i32`/…) into a
/// total `TNumericOp`, reproducing `emit_builtin_method`'s numeric arms +
/// `numeric_conversion`/`conv_rust_target` (Source/Codegen/Expression.rs) EXACTLY.
/// `src_name` is the receiver's numeric type name (the AST path's `src =
/// recv_type.or_else(rty.name())`, where `recv_type` is always `Some` for a numeric
/// method — so the source width is total here). The widening-vs-narrowing decision
/// (which `numeric_conversion` makes from the source/target int ranges) is decided
/// HERE, never in emit. Returns `None` for a name this doesn't own (defensive — the
/// gate already restricted to the covered set).
fn resolve_numeric_op(method: &str, src_name: &str) -> Option<TNumericOp> {
    // Float predicates → `(recv).{method}()`.
    if let "is_nan" | "is_infinite" | "is_finite" = method {
        return Some(TNumericOp::Predicate(method.to_string()));
    }
    // Integer bit-population queries → `((recv).{method}() as i64)`.
    if let "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" = method {
        return Some(TNumericOp::BitCount(method.to_string()));
    }
    // `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string` arm).
    if method == "to_string" {
        return Some(TNumericOp::ToShow);
    }
    // Width conversion. Mirror `conv_rust_target` + `numeric_conversion`.
    let (dst_rust, dst_spelling, dst_int) = conv_rust_target_tir(method)?;
    let Some((dsigned, dbits)) = dst_int else {
        // Float target (int→float / float→float): always representable — `as`.
        return Some(TNumericOp::CastAs {
            dst_rust: dst_rust.to_string(),
        });
    };
    // The AST path's `src = recv_type.or_else(rty.name())`; here `recv_type` is the
    // total numeric name, so `parse_int_name(src_name)` is the source int width.
    match parse_int_name_tir(src_name) {
        Some((ssigned, sbits)) => {
            let (slo, shi) = crate::AST::int_range(ssigned, sbits);
            let (dlo, dhi) = crate::AST::int_range(dsigned, dbits);
            if dlo <= slo && shi <= dhi {
                // Widening — infallible `as`.
                Some(TNumericOp::CastAs {
                    dst_rust: dst_rust.to_string(),
                })
            } else {
                // Narrowing — checked `try_from` returning `Result<T, String>`.
                Some(TNumericOp::TryFrom {
                    dst_rust: dst_rust.to_string(),
                    dst_spelling: dst_spelling.to_string(),
                })
            }
        }
        // Float (or unknown) source → integer target: a saturating `as` cast.
        None => Some(TNumericOp::CastAs {
            dst_rust: dst_rust.to_string(),
        }),
    }
}

/// c109 Phase 12: TIR-local copy of `conv_rust_target` (Source/Codegen/Expression.rs)
/// — the Rust type, spelling, and integer `(signed, bits)` (or `None` for a float) a
/// `to_*` width-conversion method targets. Kept in sync with the AST path.
fn conv_rust_target_tir(method: &str) -> Option<(&'static str, &'static str, Option<(bool, u8)>)> {
    Some(match method {
        "to_i8" => ("i8", "I8", Some((true, 8))),
        "to_i16" => ("i16", "I16", Some((true, 16))),
        "to_i32" => ("i32", "I32", Some((true, 32))),
        "to_i64" | "to_int" => ("i64", "Int", Some((true, 64))),
        "to_u8" => ("u8", "U8", Some((false, 8))),
        "to_u16" => ("u16", "U16", Some((false, 16))),
        "to_u32" => ("u32", "U32", Some((false, 32))),
        "to_u64" => ("u64", "U64", Some((false, 64))),
        "to_f32" => ("f32", "F32", None),
        "to_f64" | "to_float" => ("f64", "Float", None),
        _ => return None,
    })
}

/// c109 Phase 12: TIR-local copy of `parse_int_name` (Source/Codegen/Expression.rs) —
/// parse a numeric type name to `(signed, bits)`, `None` for floats/non-numeric.
fn parse_int_name_tir(name: &str) -> Option<(bool, u8)> {
    match name {
        "Int" => Some((true, 64)),
        "Float" | "F32" | "F64" => None,
        _ => {
            let signed = name.starts_with('I');
            if (signed || name.starts_with('U')) && name.len() > 1 {
                name[1..].parse::<u8>().ok().map(|b| (signed, b))
            } else {
                None
            }
        }
    }
}

/// c109 Phase 12: is `method` (with `nargs` args) a numeric predicate / bit-op /
/// width-conversion method the TIR lowers? This is the D-NUMOPS1 slice of
/// `emit_builtin_method` keyed on a numeric receiver (`recv_type == Some(numeric)`):
/// the float predicates (`is_nan`/`is_infinite`/`is_finite`), the integer bit-pop
/// queries (`count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros`), and the
/// width conversions (`to_i8`…`to_u64`/`to_int`/`to_f32`/`to_f64`/`to_float`). All
/// are nullary. `to_string` on a numeric receiver is NOT here — it sets
/// `recv_type == Some(numeric)` too, but the AST routes it through the plain
/// `to_string` arm (`(recv).jet_show()`), which is the Phase-9 `BuiltinMethod` shape;
/// a numeric `to_string` carries `recv_type == Some`, so it never reaches the Phase-9
/// `recv_type.is_none()` gate — it must be covered here as a distinct op.
fn is_covered_numeric_method(method: &str, nargs: usize) -> bool {
    nargs == 0
        && matches!(
            method,
            "is_nan" | "is_infinite" | "is_finite"
                | "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros"
                | "to_i8" | "to_i16" | "to_i32" | "to_i64" | "to_int" | "to_u8"
                | "to_u16" | "to_u32" | "to_u64" | "to_f32" | "to_f64" | "to_float"
                | "to_string"
        )
}

/// c109 Phase 28: is `member` a per-type numeric bounds constant (`U8.MAX`,
/// `I32.MIN`, `Float.INFINITY`, …)? Mirrors the AST `emit_expr` Field arm's filter
/// (Source/Codegen/Expression.rs ~L226) exactly.
fn is_numeric_bounds_const(member: &str) -> bool {
    matches!(
        member,
        "MIN" | "MAX" | "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON"
    )
}

/// c109 Phase 10: is a core/stdlib call `(module, method)` one the TIR lowers? The
/// covered set is exactly the **type-monomorphic** core calls — those whose full
/// signature (param conventions + return type) is fixed by `Sema::core_fixed_sig`.
/// That table is the authoritative total source: its return type gives the node's
/// total `ty` (for `?`-unwrap and binding inference), and `emit_core_call`
/// (Source/Codegen/Expression.rs) has a matching emit arm for every one of these.
///
/// Gating on `core_fixed_sig(...).is_some()` cleanly EXCLUDES the deferred calls:
///   - **closure-taking** (`tasks.spawn`, `http.serve`, `scope.guard`) — not in the
///     table / typed `None` → Phase 11 lambdas;
///   - **polymorphic** math/random/io specials (`math.abs`/`min`/`max`/`clamp`,
///     `random.pick`/`shuffle`, `io.input`/`io.eprint`) — return type depends on the
///     arg type, resolved by bespoke `check_core_call` logic, not the fixed table, so
///     a total `ty` would need re-inference (I3) → deferred;
///   - **handle-constructor** specials NOT in the table (`tasks.channel`,
///     `http.router`/`parse`/`dispatch`) and `core.mem` ptr/alloc (`@unsafe`).
/// A handle-PRODUCING call that IS in the table (`files.open` → `FileReader`,
/// `net.tcp_connect` → `TcpStream`, `time.start` → `Stopwatch`, …) is covered: the
/// CALL emits a plain helper call (parity-exact), and any later METHOD on the
/// returned handle is itself out of subset → excludes the enclosing function.
fn core_call_covered(module: &str, method: &str) -> bool {
    // c109 Phase 18: the low-level `core.mem` pointer ops (`address_of`/`volatile_read`,
    // S58). NOT in `core_fixed_sig` (their types come from bespoke sema logic), but both
    // are deterministic and reproducible from total facts: `address_of(x) -> Int` is an
    // inert address cast (no `unsafe`); `volatile_read(p) -> ptr_elem(p)` reads through a
    // typed pointer (the `read_volatile` is valid because it is only reachable inside an
    // `#Unsafe` region/fn — sema E3101 — already lowered to a Rust `unsafe` context). The
    // return type is resolved at lowering (see `lower_method_call`), so it is total.
    if module == "core.mem" && matches!(method, "address_of" | "volatile_read") {
        return true;
    }
    // c109 Phase 20: the polymorphic core specials (`math.abs/min/max/clamp`,
    // `random.pick/shuffle`, `io.eprint`). NOT in `core_fixed_sig` — their return
    // type is arg-type dependent, resolved by sema's bespoke `infer_core_call` and
    // written onto the node's `resolved_ret` field (read at lowering, so it's total —
    // I3). The EMITTED form is a fixed per-`(module, method)` string (reproduced in
    // `emit_tir_core_call`), args emitted plainly, byte-for-byte `emit_core_call`.
    // (`io.input` is NOT here — it IS in `core_fixed_sig`, covered by Phase 10.)
    if crate::Sema::is_polymorphic_core_special(module, method) {
        return true;
    }
    // c109 Phase 21: the `tasks.channel()` PRODUCER. NOT in `core_fixed_sig` (its return
    // type `Channel<T>` is inferred from the binding annotation, not the args — sema
    // E0904 requires the annotation). The emit is a plain, arg-free
    // `{root}jet_std::JetChannel::new()` (Source/Codegen/Expression.rs `emit_core_call`),
    // so it's a fixed-string `CoreCall` reproduced in `emit_tir_core_call`. The node's
    // `ty` is not load-bearing (the binding carries `b.ty == Channel<T>`); it lowers to
    // `Unit` (totality fallback). The `Channel`/`Sender`/`Task` METHODS that read T off
    // the binding's annotated slot type then route via their own shape.
    if module == "core.tasks" && method == "channel" {
        return true;
    }
    // c109 Phase 25: the HttpRouter producer + the parse/dispatch core calls (D-ROUTE1=A).
    // NOT in `core_fixed_sig` — their return types are fixed per `(module, method)` but
    // live in sema's bespoke `infer_core_call` (`router` → HttpRouter, `parse` →
    // HttpRequest, `dispatch` → HttpResponse). Each emits a fixed-string `CoreCall`
    // (`{root}jet_http_router_new()` / `{root}jet_http_parse_request(&(raw))` /
    // `{root}jet_http_router_dispatch(&(router), req)`), reproduced in `emit_tir_core_call`.
    // `http.serve` stays out (closure-taking, covered by `CoreClosureCall`); `http.router`
    // is arg-free so it can't collide. The producer's `HttpRouter` value type is covered
    // (`is_covered_handle_ty`) and its binding is forced to `let mut` (D-ROUTE1=A).
    if module == "jet.http" && matches!(method, "router" | "parse" | "dispatch") {
        return true;
    }
    // c109 Phase 29: qualified `io.input(prompt)`. NOT in `core_fixed_sig` — its return
    // type (`Result<String, IOError>`) lives in sema's bespoke `infer_core_call` arm
    // (CheckerCoreLib.rs), carried total by `core_call_return_ty`. It is the DISTINCT
    // qualified MethodCall on a `core.io` alias (the ambient bare `input()`, Phase 25, is
    // a separate `Expr::Call` node → `AmbientInput`). Emits the same fixed-string CoreCall
    // `{root}jet_std_io_input(None|Some(&(prompt)))` (reproduced in `emit_tir_core_call`),
    // byte-for-byte the AST `emit_core_call` arm. Composes with the Phase-8 `?? return`.
    if module == "core.io" && method == "input" {
        return true;
    }
    crate::Sema::core_fixed_sig(module, method).is_some()
}

/// c109 Phase 13: is a closure-taking core call (`tasks.spawn`/`http.serve`/
/// `scope.guard`) inside the subset? These are NOT in `core_fixed_sig` — each has a
/// bespoke emit shape (`emit_core_call`, Source/Codegen/Expression.rs). We cover only
/// the cleanest, byte-reproducible case for each, where the closure arg is a LITERAL
/// in-subset lambda:
///   - `tasks.spawn(<lambda>)` — 1 arg, a literal lambda (the `emit_spawn_lambda`
///     `move |…|` form). A non-lambda spawn arg (a fn-value) takes the AST `arg(0)`
///     path — excluded (its byte shape differs).
///   - `http.serve(addr, <lambda>)` — 2 args; arg0 (addr) any in-subset value, arg1 a
///     literal lambda (the `jet_http_serve(&(addr), <lambda>)` branch). The
///     router-handler branch needs an HttpRouter value, which can only come from
///     `http.router()` (not in `core_fixed_sig`) — so it can't arise in a covered fn.
///   - `scope.guard(<lambda>)` — 1 arg, a literal zero-param lambda.
fn core_closure_call_in_subset(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let lambda_arg = |i: usize| {
        matches!(args.get(i).map(|a| &a.expr), Some(Expr::Lambda(lam)) if lambda_in_subset(lam, cx, locals))
    };
    let no_labels = args.iter().all(|a| a.label.is_none());
    match (module, method) {
        ("core.tasks", "spawn") => args.len() == 1 && no_labels && lambda_arg(0),
        ("jet.http", "serve") => {
            args.len() == 2
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
        }
        ("core.scope", "guard") => args.len() == 1 && no_labels && lambda_arg(0),
        _ => false,
    }
}

/// c109 Phase 13: resolve a handle method `(handle, method, nargs)` into a total
/// `THandleOp`, reproducing the handle arms of `emit_builtin_method`
/// (Source/Codegen/Expression.rs). Returns `None` for anything not covered (so the
/// caller falls through to other shapes). Excluded (with reason): `lines` on
/// FileReader/StdinHandle (dead — E2502, loop-source-only); all HttpRouter `get`/
/// `post`/`put`/`delete` (closure handler → `emit_router_handler`); HttpRequest/
/// HttpResponse accessors (serve-lambda-param slot may be unresolved → AST handle arm
/// wouldn't fire); Arena/Bump/Pool/Fixed (`alloc`/`reset`/`free` — the producer
/// `mem.*.new` isn't a covered call, so an allocator never binds in a covered fn);
/// Channel/Sender/Task (`receive`/`send`/`sender`/`detach` — producers not covered);
/// `Match.group` (the `Option<Match>` unwrap chain isn't cleanly reachable).
/// c109 Phase 19: is this MethodCall the arena allocator constructor `mem.Arena.new(…)`
/// (D-ALLOC1)? Reproduces `emit_method_call`'s constructor branch (Expression.rs ~L1515):
/// the receiver is `Field(Ident(alias), <AllocType>)` where `alias ∈ core_imports` maps to
/// `core.mem` and `<AllocType> ∈ {Arena,Bump,Pool,Fixed}`, and `method == "new"`. Returns
/// the resolved allocator type-name (so the gate can admit it) or `None`.
fn alloc_new_type<'a>(
    receiver: &'a Expr,
    method: &str,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<&'a str> {
    if method != Syntax::MEM_ALLOC_NEW {
        return None;
    }
    let Expr::Field(inner, alloc_type, _) = receiver else {
        return None;
    };
    let Expr::Ident(alias, _) = &**inner else {
        return None;
    };
    if locals.contains(alias) {
        return None;
    }
    if cx.core_imports.get(alias).map(String::as_str) != Some(Syntax::CORE_MEM_MODULE) {
        return None;
    }
    match alloc_type.as_str() {
        "Arena" | "Bump" | "Pool" | "Fixed" => Some(alloc_type.as_str()),
        _ => None,
    }
}

/// c109 Phase 25: is `router.get(path, handler)` (and `.post`/`.put`/`.delete`) inside
/// the subset? Reproduces `emit_router_handler` (Source/Codegen/Expression.rs): the
/// handler (arg 1) must be either a BARE TOP-LEVEL FN name (an `Ident` not in locals —
/// the `env.get(name).is_none()` branch → the `move |__req| user_<fn>(&__req)` wrapper)
/// or an in-subset literal LAMBDA (the `Box::new(<lambda>)` branch). The path (arg 0) is
/// any in-subset value. No labels.
fn router_register_in_subset(
    receiver: &Expr,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if args.iter().any(|a| a.label.is_some()) {
        return false;
    }
    if !expr_in_subset(receiver, cx, locals) {
        return false;
    }
    if !expr_in_subset(&args[0].expr, cx, locals) {
        return false;
    }
    match &args[1].expr {
        // A bare top-level fn name (the `env.get(name).is_none()` named-fn branch). It
        // must NOT be a local (a local handler would take the `Box::new(emit_expr(…))`
        // path, which for a fn-typed local emits its own `Box::new` — still covered, but
        // we keep to the simple named-fn + lambda shapes the live suite uses).
        Expr::Ident(name, _) => !locals.contains(name),
        // A literal in-subset lambda (the `Box::new(<lambda>)` branch).
        Expr::Lambda(lam) => lambda_in_subset(lam, cx, locals),
        _ => false,
    }
}

/// c109 Phase 25: render the router handler (arg 1) exactly as `emit_router_handler`
/// (Source/Codegen/Expression.rs) does, at lowering. A bare top-level fn name (not a
/// local) becomes the `Box::new(move |__req: …| user_<fn>(&__req)) as Box<dyn Fn(…) -> …
/// + Send + Sync>` wrapper; a lambda becomes `Box::new(<lambda>) as Box<…>`.
fn render_router_handler(args: &[crate::AST::CallArg], cx: &Cx, env: &LowerEnv) -> String {
    let root = &cx.root_prefix;
    let boxed_dyn = format!(
        "as Box<dyn Fn({}JetHttpRequest) -> {}JetHttpResponse + Send + Sync>",
        root, root
    );
    match &args[1].expr {
        Expr::Ident(name, _) if !env.locals.contains_key(name) => {
            let rust_name = mangle(name);
            format!(
                "Box::new(move |__req: {}JetHttpRequest| {}(&__req)) {}",
                root, rust_name, boxed_dyn
            )
        }
        Expr::Lambda(lam) => {
            format!("Box::new({}) {}", render_lambda_str(lam, cx, env), boxed_dyn)
        }
        // The gate (`router_register_in_subset`) proved arg 1 is one of the two above.
        _ => unreachable!("router handler gate proved a named-fn or lambda handler"),
    }
}

fn handle_method_op(handle: &str, method: &str, nargs: usize) -> Option<THandleOp> {
    Some(match (handle, method, nargs) {
        ("FileReader", "read_line", 0) => THandleOp::FileReaderReadLine,
        ("FileWriter", "write_line", 1) => THandleOp::FileWriterWriteLine,
        ("FileWriter", "flush", 0) => THandleOp::FileWriterFlush,
        ("StdinHandle", "read_line", 0) => THandleOp::StdinReadLine,
        ("Stopwatch", "elapsed_millis", 0) => THandleOp::StopwatchElapsedMillis,
        ("TcpListener", "accept", 0) => THandleOp::TcpListenerAccept,
        ("TcpListener", "local_addr", 0) => THandleOp::TcpListenerLocalAddr,
        ("TcpStream", "read", 0) => THandleOp::TcpStreamRead,
        ("TcpStream", "write", 1) => THandleOp::TcpStreamWrite,
        ("TcpStream", "peer_addr", 0) => THandleOp::TcpStreamPeerAddr,
        ("TcpStream", "local_addr", 0) => THandleOp::TcpStreamLocalAddr,
        ("TcpStream", "close", 0) => THandleOp::TcpStreamClose,
        // c109 Phase 19: the four arena allocators (`alloc`/`reset`/`free`). Sema sets
        // `recv_type == Some(<allocator>)` via `alloc_method_return`; the AST
        // `emit_builtin_method` arms key on the same `rty`. `Arena`/`Bump`/`Pool`/`Fixed`
        // share identical Rust method names (the engines differ; the surface doesn't).
        ("Arena" | "Bump" | "Pool" | "Fixed", "alloc", 1) => THandleOp::AllocAlloc,
        ("Arena" | "Bump" | "Pool" | "Fixed", "reset", 0) => THandleOp::AllocReset,
        ("Arena" | "Bump" | "Pool" | "Fixed", "free", 0) => THandleOp::AllocFree,
        // c109 Phase 20: HttpRequest/HttpResponse accessors (E2-M10, D-ROUTE1=A).
        // Now reachable because the `http.serve` lambda param type is written back
        // onto `p.ty` (sema), so the slot type is total. The AST `emit_builtin_method`
        // arms key on the same `rty == Some(HttpRequest|HttpResponse)`. Reproduced
        // byte-for-byte in `emit_tir_handle_method`.
        ("HttpRequest", "method", 0) => THandleOp::HttpReqField("method"),
        ("HttpRequest", "path", 0) => THandleOp::HttpReqField("path"),
        ("HttpRequest", "body", 0) => THandleOp::HttpReqField("body"),
        ("HttpRequest", "header", 1) => THandleOp::HttpReqHeader,
        ("HttpRequest", "param", 1) => THandleOp::HttpReqParam,
        ("HttpResponse", "status", 0) => THandleOp::HttpRespField("status"),
        ("HttpResponse", "body", 0) => THandleOp::HttpRespField("body"),
        ("HttpResponse", "header", 1) => THandleOp::HttpRespHeader,
        _ => return None,
    })
}

/// c109 Phase 13: the resolved return type of a covered handle method, read from the
/// authoritative sema handle tables (`file_handle_method_return`/`net_method_return`,
/// Source/Sema/CheckerCoreLib.rs) — a pure `(handle, method)` dispatch, no inference.
/// The return type is rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but kept total per the design principle. A throwaway diags vec absorbs the table's
/// diagnostic side-channel (sema already validated, so none fire here).
fn handle_method_return_ty(handle: &str, method: &str, nargs: usize) -> Type {
    let span = crate::Diagnostics::Span { start: 0, end: 0 };
    let mut sink = Vec::new();
    let ret = crate::Sema::file_handle_method_return(handle, method, nargs, span, &mut sink)
        .or_else(|| crate::Sema::net_method_return(handle, method, nargs, span, &mut sink));
    match ret {
        Some(Some(t)) => t,
        _ => unit_type(),
    }
}

/// c109 Phase 13: the resolved return type of a closure-taking core call, matching
/// `infer_core_call` (Source/Sema/CheckerCoreLib.rs). `spawn` → `Task<elem>` (the
/// closure's body type — total from the lowered lambda's return); `serve` → Unit (runs
/// forever); `guard` → `ScopeGuard`. These types are rarely load-bearing in emit (a
/// binding carries sema's `b.ty`), but kept total per the design principle.
fn core_closure_call_return_ty(module: &str, method: &str, body_ty: Type) -> Type {
    match (module, method) {
        ("core.tasks", "spawn") => Type::Apply {
            name: "Task".to_string(),
            args: vec![body_ty],
        },
        ("core.scope", "guard") => Type::Named("ScopeGuard".to_string()),
        _ => unit_type(),
    }
}

/// c109 Phase 10: the resolved return type of a covered core call, read from the
/// authoritative `Sema::core_fixed_sig` table (totality). A `None` return (a
/// void-effect call like `fs.write`/`env.set`/`process.exit`) lowers to `Unit`.
fn core_call_return_ty(module: &str, method: &str) -> Type {
    // c109 Phase 25: the http producer/parse/dispatch calls aren't in `core_fixed_sig`;
    // their return types are fixed (sema's `infer_core_call`). Carried total per the
    // design principle (the binding's annotation/inference is the load-bearing fact, but
    // this keeps the node's `ty` honest — `dispatch` → HttpResponse composes with the
    // `.status()`/`.body()` accessors that read it).
    match (module, method) {
        ("jet.http", "router") => return Type::Named("HttpRouter".to_string()),
        ("jet.http", "parse") => return Type::Named("HttpRequest".to_string()),
        ("jet.http", "dispatch") => return Type::Named("HttpResponse".to_string()),
        // c109 Phase 29: qualified `io.input(prompt)`. NOT in `core_fixed_sig` — its return
        // type is fixed (`Result<String, IOError>`) but lives in sema's bespoke
        // `infer_core_call` arm (CheckerCoreLib.rs `("core.io", "input")`), NOT the table.
        // Same type the ambient bare `input(...)` (Phase 25 `AmbientInput`) carries, so it
        // composes with the Phase-8 `??`/`?? return <value>` fallback.
        ("core.io", "input") => {
            return Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
            }
        }
        _ => {}
    }
    crate::Sema::core_fixed_sig(module, method)
        .and_then(|(_, ret)| ret)
        .unwrap_or_else(unit_type)
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
    /// c109 Phase 15: the `safe_locals_expr` env replica for the `a ?? panic(…)` form.
    /// `safe_locals_expr` (Source/Codegen/Statement.rs) dumps the FULL codegen `env`
    /// (`HashMap<String, Slot>`), filtered to scalar Int/Float/Bool slots, sorted by
    /// name, at the panic site. The AST codegen `env` LEAKS: a `let` inside a plain
    /// block / loop / mixed-or-range switch arm / comptime-if branch stays in the
    /// shared `&mut env` after the block (sema scopes the *name* so it is never read,
    /// but `safe_locals_expr` dumps the raw env regardless). Only the two
    /// `emit_pattern_match_switch` arm-body boundaries and lambda bodies clone the env
    /// (no leak). To reproduce the dump byte-exact this replica is shared (`Rc<RefCell>`)
    /// across leaky branches via `clone_env`, and DEEP-COPIED via `fork_panic` at the
    /// non-leaky boundaries. It is updated in lock-step with `locals` through `bind`.
    panic_locals: Rc<RefCell<HashMap<String, (String, Option<Type>)>>>,
    /// c109 Phase 17: the enclosing function returns `-> view T` (a borrow). When set,
    /// a `return <e>` lowers via the view-return shape (`emit_view_return`): an `Ident`
    /// becomes `&name`/`name` (deref) / `&<const>`, a field read `&(<place>)`, anything
    /// else a plain expr — resolved at lowering into a `TStmt::ViewReturn`. The AST path
    /// reads this off `view_return` threaded through `emit_stmts`; the TIR carries it on
    /// the env so the `Return` lowering reproduces it byte-for-byte.
    view_return: bool,
}

impl LowerEnv {
    /// A fresh root env for a function/method body (an empty `panic_locals` replica).
    fn new(fn_name: String) -> LowerEnv {
        LowerEnv {
            locals: HashMap::new(),
            fn_name,
            panic_locals: Rc::new(RefCell::new(HashMap::new())),
            view_return: false,
        }
    }
    /// Bind `name` to its resolved Rust place + type, updating BOTH `locals` (used for
    /// place/type resolution) and the `panic_locals` replica (used only for the `??`
    /// panic locals dump). Every covered binding site routes through here so the two
    /// stay in lock-step.
    fn bind(&mut self, name: &str, place: String, ty: Option<Type>) {
        self.locals.insert(name.to_string(), (place.clone(), ty.clone()));
        self.panic_locals
            .borrow_mut()
            .insert(name.to_string(), (place, ty));
    }
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

/// c109 Phase 17: lower a `return <e>` from a `-> view T` function, reproducing
/// `emit_view_return` (Source/Codegen/Statement.rs) byte-for-byte. The view-return
/// subset only admits an `Ident` (a parameter/const borrowed back) or a `Field` read
/// (`field.name` — a borrow into a field of an owned root); sema's E2301/E2304 reject
/// index/slice and a non-owning local, so those never reach here.
///  - an `Ident` resolving to a deref'd slot (`(*name)`) returns the BARE borrow `name`
///    (the deref stripped) — `ViewWrap::Bare` over a `Local(rust_name)`;
///  - an `Ident` resolving to a non-deref slot returns `&name` — `ViewWrap::Addr`;
///  - an `Ident` that is a const returns `&<const>` — `ViewWrap::Addr` over the inlined
///    const value (the same `Local` path the AST takes via `cx.consts`);
///  - an `Ident` not in scope returns `place_of(name)` with no `&` — `ViewWrap::Bare`;
///  - a `Field` read returns `&(<place>)` — `ViewWrap::Addr` over the lowered field;
///  - anything else passes straight to `emit_expr` — `ViewWrap::Bare`.
fn lower_view_return(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match e {
        Expr::Ident(name, _) => {
            // A comptime const inlines at the use site (the AST reads `cx.consts`); take
            // its address. Lower as a normal expr (which inlines the const) wrapped in `&`.
            if cx.consts.contains_key(name) {
                return TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Addr,
                };
            }
            match env.locals.get(name) {
                Some((place, ty)) if place.starts_with("(*") && place.ends_with(')') => {
                    // Deref'd (by-reference) slot: return the bare borrow `name`.
                    let bare = place[2..place.len() - 1].to_string();
                    TStmt::ViewReturn {
                        value: TExpr {
                            ty: ty.clone().unwrap_or(Type::Int),
                            kind: TExprKind::Local(bare),
                        },
                        wrap: ViewWrap::Bare,
                    }
                }
                Some(_) => TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Addr,
                },
                // Not in env: `place_of` returns the mangled name with no `&` (Bare).
                None => TStmt::ViewReturn {
                    value: lower_expr(e, cx, env),
                    wrap: ViewWrap::Bare,
                },
            }
        }
        Expr::Field(..) => TStmt::ViewReturn {
            value: lower_expr(e, cx, env),
            wrap: ViewWrap::Addr,
        },
        _ => TStmt::ViewReturn {
            value: lower_expr(e, cx, env),
            wrap: ViewWrap::Bare,
        },
    }
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        // c109 Phase 17: a param TYPED as a bare type parameter (`item: T`) is forced to
        // the `Move` convention for the slot deref (it is passed by value — `rust_param_type`
        // renders it `T`, no `&`), EXACTLY as `emit_func` forces `conv = Move` for an
        // `is_type_param` param. A param typed `Stack<T>` is NOT a type-var param — it keeps
        // its source convention (`Read` → `&user_Stack<T>`, deref'd place `(*user_s)`).
        let place = param_place_generic(&rust_name, p, &f.type_params);
        env.bind(&p.name, place, Some(p.ty.clone()));
        params.push((rust_name, p.ty.clone(), p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.clone(),
        is_view: f.is_view_return,
        generics: render_generics(&f.type_params),
        is_main: f.name == "main",
        is_unsafe: f.is_unsafe,
        body,
        kind: TFuncKind::TopLevel,
    }
}

/// c109 Phase 17: render the Rust generic clause exactly as `emit_func` does — every type
/// param carries an extra `Clone` bound (`rust_extra_clone_bounds`), so `<T>` → `<T: Clone>`
/// and `<T: Comparable>` → `<T: PartialOrd + Clone>`. Empty for a non-generic function.
fn render_generics(type_params: &[crate::AST::TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }
    let extra = crate::Generics::rust_extra_clone_bounds(type_params);
    crate::Generics::rust_type_param_list(type_params, &extra)
}

/// c109 Phase 17: `param_place` for a (possibly generic) free function. A param whose type
/// is a bare type-parameter name (`Type::Named(T)` where `T` is one of `type_params`) is
/// forced to `Move` for the deref decision (it is by-value), mirroring `emit_func`'s
/// `is_type_param` branch; any other param uses `param_place`'s convention-based deref.
fn param_place_generic(rust_name: &str, p: &Param, type_params: &[crate::AST::TypeParam]) -> String {
    let is_type_param = type_params
        .iter()
        .any(|tp| matches!(&p.ty, Type::Named(n) if n == &tp.name));
    if is_type_param {
        // Forced `Move` → no deref (by-value), exactly `emit_func`.
        rust_name.to_string()
    } else {
        param_place(rust_name, p)
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
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    let mut params = Vec::new();
    let mut self_conv: Option<AccessConvention> = None;
    let mut is_static = true;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            // The self slot, parity with `emit_method`: place `self`, type None. A
            // `mut self` receiver is `&mut Self`, so its place DEREFS (`(*self)`) —
            // `self.field = v` → `((*self)).field = v`, whole-`self` `self = New{}` →
            // `(*self) = New{}` (D-MUTSELF1). `self`/`take self` carry no deref.
            let place = if matches!(p.convention, AccessConvention::Mutate) {
                "(*self)".to_string()
            } else {
                "self".to_string()
            };
            env.bind(Syntax::KW_SELF, place, None);
            self_conv = Some(p.convention);
            is_static = false;
            continue;
        }
        let rust_name = mangle(&p.name);
        let place = param_place(&rust_name, p);
        // A `Self`-typed param resolves to the owning type for totality.
        let pty = resolve_self_ty(&p.ty, type_name);
        env.bind(&p.name, place, Some(pty.clone()));
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
        is_view: f.is_view_return,
        // A method's generic params live on the enclosing `impl<T> user_<T>` block (the
        // caller opened it); `emit_method` renders no per-method clause.
        generics: String::new(),
        is_main: false,
        is_unsafe: f.is_unsafe,
        body,
        kind,
    }
}

/// c109 Phase 12: lower a TRAIT-IMPL method of `type_name` to a `TFunc`. Mirrors
/// `emit_trait_method`'s slot construction (Source/Codegen/Items.rs) EXACTLY — which
/// differs from `emit_method`:
///  - the `self` slot's type is `Some(Type::Named(type_name))` (NOT `None` as in
///    `emit_method`); place `self`, no deref. This is load-bearing for overflow-trap
///    decisions that consult the self slot — though in the covered subset `self` is a
///    struct/enum (never a bare arithmetic operand), so the decision never differs.
///  - non-self params use the same deref logic, but `emit_trait_method` has no
///    `Read if scalar` short-circuit branch — it computes `deref = !p.ty.is_scalar()`
///    for `Read`, which is identical to `param_place` for `Read` (scalar → false).
/// The `TraitMethod` kind drives a bare name, no `pub`, always-`&self` signature.
pub(crate) fn lower_trait_method(f: &Func, type_name: &str, cx: &Cx) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    env.view_return = f.is_view_return;
    let mut params = Vec::new();
    let mut self_conv = AccessConvention::Read;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            self_conv = p.convention;
            // The self slot, EXACTLY `emit_trait_method`'s: type `Some(Named(type_name))`
            // (NOT `None` like `emit_method`). D-MUTSELF1: a `mut self` receiver is
            // `&mut self`, so its place DEREFS (`(*self)`); `self`/`take self` do not.
            let place = if matches!(p.convention, AccessConvention::Mutate) {
                "(*self)".to_string()
            } else {
                "self".to_string()
            };
            env.bind(
                Syntax::KW_SELF,
                place,
                Some(Type::Named(type_name.to_string())),
            );
            continue;
        }
        let rust_name = cx.mangle_name(&p.name);
        let place = param_place(&rust_name, p);
        let pty = resolve_self_ty(&p.ty, type_name);
        env.bind(&p.name, place, Some(pty.clone()));
        params.push((rust_name, pty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        ret: f.return_type.as_ref().map(|t| resolve_self_ty(t, type_name)),
        is_view: f.is_view_return,
        generics: String::new(),
        is_main: false,
        // The trait-method `unsafe` prefix rides on `TFuncKind::TraitMethod.is_unsafe`
        // (the dedicated trait-method emit reads it there); the top-level flag is unused
        // for this kind, but keep it consistent.
        is_unsafe: f.is_unsafe,
        body,
        kind: TFuncKind::TraitMethod {
            is_unsafe: f.is_unsafe,
            self_conv,
        },
    }
}

/// c109 Phase 15: is a DELEGATION trait method (`using field`) coverable? Always — the
/// method is purely structural: a fixed forwarding call `(self).<field>.<method>(args)`
/// with the bare trait method name, and a signature rendered by the SAME
/// `rust_param_type`/`rust_return_type` the AST path uses. There is no body to lower, no
/// type to re-infer; the forward + signature are deterministic. (The `field`/method/
/// args come straight off the `ImplDef`; nothing here can produce code rustc rejects
/// that the AST path wouldn't.) Returns `true` for any delegation method.
pub(crate) fn tir_covers_delegation_method(_f: &Func, _field: &str, _cx: &Cx) -> bool {
    true
}

/// c109 Phase 15: lower a delegation trait method to a `TFunc` with a `Delegation` kind,
/// reproducing `emit_delegation_method` (Source/Codegen/Items.rs) byte-for-byte: the
/// signature line (incl. its quirky two-space `  {`), and the forwarding call. There is
/// no body — the method only forwards to the delegated field with the BARE trait method
/// name (no `user_` mangle, as the trait owns it in Rust).
pub(crate) fn lower_delegation_method(f: &Func, field: &str, cx: &Cx) -> TFunc {
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            if p.name == Syntax::KW_SELF {
                "&self".to_string()
            } else {
                format!("{}: {}", mangle(&p.name), rust_param_type(cx, p.convention, &p.ty))
            }
        })
        .collect();
    // The signature line, EXACTLY `emit_delegation_method`'s format (note the two spaces
    // before `{` and the ` {ret}` only when there is a return).
    let sig = format!(
        "    fn {}({}){}  {{\n",
        f.name,
        params.join(", "),
        if ret_clause.is_empty() {
            String::new()
        } else {
            format!(" {}", ret_clause.trim())
        }
    );
    let fwd_args: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| mangle(&p.name).to_string())
        .collect();
    let field_rust = mangle(field);
    let fwd = format!("(self).{}.{}({})", field_rust, f.name, fwd_args.join(", "));
    TFunc {
        name: f.name.clone(),
        params: Vec::new(),
        ret: f.return_type.clone(),
        // The signature is fully pre-rendered (`sig`); `is_view`/`generics` are unused for delegation.
        is_view: f.is_view_return,
        generics: String::new(),
        is_main: false,
        // A delegation method has no body and never carries `#Unsafe fn` (sema rejects it).
        is_unsafe: false,
        body: Vec::new(),
        kind: TFuncKind::Delegation {
            sig,
            fwd,
            has_return: f.return_type.is_some(),
        },
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
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Struct { .. })) => {
            // c109: a struct-destructuring binding `Type { x, y } @= <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Named`/`Apply` naming a struct
            // (sema guarantees it). The per-field type comes from `cx.struct_fields`,
            // reproducing `emit_stmt`'s `BindPattern::Struct` arm. Each field binds with
            // its resolved type and a non-deref'd slot (the clone owns the value); the
            // pattern's field name is BOTH the bound local and the `.field` read.
            let Some(BindPattern::Struct { fields, span, .. }) = &b.pattern else {
                unreachable!("guard matched a struct pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let field_tys: HashMap<String, Type> = match &init.ty {
                Type::Named(n) | Type::Apply { name: n, .. } => cx
                    .struct_fields
                    .get(n)
                    .map(|fs| fs.iter().cloned().collect())
                    .unwrap_or_default(),
                _ => HashMap::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut field_names = Vec::new();
            for f in fields {
                let m = mangle(&f.name).to_string();
                field_names.push(m.clone());
                env.bind(&f.name, m, field_tys.get(&f.name).cloned());
            }
            return TStmt::StructDestructure {
                tmp,
                init,
                kw,
                fields: field_names,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Tuple { .. })) => {
            // c109 Phase 23: a tuple-destructuring binding `(a, b) @= <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Tuple` (sema guarantees it). Pair the
            // pattern elements to the tuple's CANONICAL fields by position, reproducing
            // `emit_stmt`'s `BindPattern::Tuple` arm. Each element binds with its resolved
            // field type and a non-deref'd slot (the clone owns the value).
            let Some(BindPattern::Tuple { elems, span }) = &b.pattern else {
                unreachable!("guard matched a tuple pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let canonical: Vec<(String, Type)> = match &init.ty {
                Type::Tuple(fs) => fs.iter().map(|(n, t)| (n.clone(), (**t).clone())).collect(),
                _ => Vec::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for (e, (fname, fty)) in elems.iter().zip(canonical.iter()) {
                let elem_rust = mangle(&e.name).to_string();
                let field_rust = mangle(fname).to_string();
                binds.push((elem_rust.clone(), field_rust));
                env.bind(&e.name, elem_rust, Some(fty.clone()));
            }
            return TStmt::TupleDestructure {
                tmp,
                init,
                kw,
                binds,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::List { .. })) => {
            // c109 Phase 26: a list-destructuring binding `[a, b, c] @= <init>`. Lower
            // the init ONCE, then bind each element via `jet_unpack_vec(tmp, want, i,
            // file, line)`, reproducing `emit_stmt`'s `BindPattern::List` arm. The
            // element slot type reproduces `expr_jet_ty(init)`'s `Some(List(inner))`-only
            // match: the LOWERED init's `.ty` is exactly what `expr_jet_ty(&b.init)`
            // resolves (an Ident → its slot type), so a non-`List` init (e.g. a `[T#N]`
            // fan-out result) yields a `None` element type — byte-identical partiality.
            let Some(BindPattern::List { elems, span }) = &b.pattern else {
                unreachable!("guard matched a list pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let elem_ty = match &init.ty {
                Type::List(inner) => Some((**inner).clone()),
                _ => None,
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            let mut elem_names = Vec::new();
            for e in elems {
                let m = mangle(&e.name).to_string();
                elem_names.push(m.clone());
                env.bind(&e.name, m, elem_ty.clone());
            }
            return TStmt::ListDestructure {
                tmp,
                init,
                kw,
                want: elems.len(),
                file: cx.file.clone(),
                line,
                elems: elem_names,
            };
        }
        Stmt::Val(b) => {
            // c109 Phase 19: an arena `view` binding (`x @= arena.alloc(v)`). The AST
            // `emit_let`'s `arena_view` branch emits `let <x> = <init>;` (NO type clause,
            // NEVER `let mut` — a view is a non-reassignable `&mut T`) and binds a DEREF'd
            // slot (reads go through `(*x)`). Reproduce it exactly: a `Let` with `kw: "let"`,
            // empty `ty_clause`, and a deref'd slot place `(*<x>)`.
            if b.arena_view {
                let init = lower_expr(&b.init, cx, env);
                env.bind(&b.name, format!("(*{})", mangle(&b.name)), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause: String::new(),
                    init,
                };
            }
            // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr`. The AST `emit_let`
            // builds `init` from `b.ct.serialize()` (the sema-evaluated value rendered to a
            // Rust literal) — the runtime `init` expr is never emitted. Reproduce it: a
            // verbatim `ConstInline` of the same serialized string, with `kw: "let"` (the
            // `(b.mutable && !b.is_comptime)` guard makes it `let`, never `let mut`) and the
            // type clause from `b.ty` (rendered exactly as the non-comptime path below). All
            // facts are pre-resolved (I3): no inference here.
            if b.is_comptime {
                let serialized = b
                    .ct
                    .as_ref()
                    .map(|v| v.serialize())
                    .unwrap_or_else(|| "Default::default()".to_string());
                // Mirror `emit_let`'s type clause exactly (a Fn type via `rust_fn_trait`,
                // others via `rust_type`). A comptime value is never fn-typed, but match
                // the AST shape verbatim for total byte-parity.
                let ty_clause = b
                    .ty
                    .as_ref()
                    .map(|t| {
                        if let Type::Fn { params, ret } = t {
                            format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), false))
                        } else {
                            format!(": {}", cx.rust_type(t))
                        }
                    })
                    .unwrap_or_default();
                let init = TExpr {
                    ty: b.ty.clone().unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(serialized),
                };
                env.bind(&b.name, mangle(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause,
                    init,
                };
            }
            let mut init = lower_expr(&b.init, cx, env);
            // c109 Phase 13: reproduce `emit_let`'s `mut_fn` form — an escaping FnMut
            // lambda binding gets `let mut` AND an `as <fn-trait(mut)>` init coercion +
            // a `: <fn-trait(mut)>` annotation. Decided here from `Lambda.meta`.
            let mut_fn = matches!(
                &b.init,
                Expr::Lambda(l) if l.meta.escapes && l.meta.needs_fn_mut
            );
            if mut_fn {
                if let Some(Type::Fn { params, ret }) = &b.ty {
                    let coerced = format!(
                        "{} as {}",
                        emit_tir_expr(&init, cx),
                        cx.rust_fn_trait(params, ret.as_deref(), true)
                    );
                    init = TExpr {
                        ty: init.ty.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn { wrapper: coerced },
                        },
                    };
                }
            }
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            let ty = b.ty.clone().unwrap_or_else(|| init.ty.clone());
            // E2-M7/E2-M10/D-ALLOC1/D-ROUTE1: a handle binding forces `let mut` even
            // when bound immutably (its methods take `&mut self`). Mirror
            // `emit_let`'s `is_file_handle` set exactly.
            let is_file_handle = matches!(
                &b.ty,
                Some(Type::Named(n)) if n == "FileReader" || n == "FileWriter"
                    || n == "TcpStream" || n == "HttpRouter"
                    || n == "Arena" || n == "Bump" || n == "Pool" || n == "Fixed"
            );
            let kw = if (b.mutable && !b.is_comptime) || mut_fn || is_file_handle {
                "let mut"
            } else {
                "let"
            };
            // The type annotation clause, rendered exactly as `emit_let`: a Fn type via
            // `rust_fn_trait(params, ret, mut_fn)`, others via `rust_type`. Empty for an
            // inferred binding.
            let ty_clause = b
                .ty
                .as_ref()
                .map(|t| {
                    if let Type::Fn { params, ret } = t {
                        format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), mut_fn))
                    } else {
                        format!(": {}", cx.rust_type(t))
                    }
                })
                .unwrap_or_default();
            env.bind(&b.name, mangle(&b.name), Some(ty));
            TStmt::Let {
                name: b.name.clone(),
                kw,
                ty_clause,
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
            // D-MUTSELF1: a field-assignment `place.field [op]= v`. The place is the
            // field READ lowered to its resolved Rust string (`((*self)).field` once
            // the `mut self` slot derefs), reusing the same `Expr::Field` lowering the
            // read path uses — byte-for-byte the AST `LValue::Field` form. Carried as a
            // plain `TStmt::Assign` so the `op` compound form rides the shared emit.
            LValue::Field { base, field, span } => {
                let field_expr = Expr::Field(base.clone(), field.clone(), *span);
                let place = emit_tir_expr(&lower_expr(&field_expr, cx, env), cx);
                TStmt::Assign {
                    place,
                    op: *op,
                    value: lower_expr(value, cx, env),
                }
            }
        },
        Stmt::Return(Some(e), _) if env.view_return => lower_view_return(e, cx, env),
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
                // The loop var is an `Int` local for the body's scope only. The AST
                // (`Statement.rs`) inserts it into the shared env, emits the body, then
                // RESTORES the prior binding — so a scalar `??` panic dump INSIDE the body
                // sees the var, but one after the loop does not. Reproduce that exactly:
                // bind it on the shared `panic_locals`, lower the body, then restore.
                let mut branch = clone_env(env);
                let prev = branch.panic_locals.borrow().get(var).cloned();
                branch.bind(var, mangle(var), Some(Type::Int));
                let lowered_body = lower_stmts(body, cx, &mut branch);
                match prev {
                    Some(p) => {
                        branch.panic_locals.borrow_mut().insert(var.clone(), p);
                    }
                    None => {
                        branch.panic_locals.borrow_mut().remove(var);
                    }
                }
                TStmt::Range {
                    label: label_name(label),
                    var: var.clone(),
                    start,
                    end,
                    step,
                    body: lowered_body,
                }
            }
            // c109 Phase 5: collection iteration `loop x in coll` / `loop k, v in map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection } => {
                // c109 Phase 22: classify a method-call collection into the matching
                // `emit_for_in` branch (`chars`/`lines`/the `.iter().cloned()` default),
                // resolving the receiver/collection string off the SAME node shape the
                // AST path reads. `method_kind == None` is the plain `.iter()` form.
                let (collection_str, method_kind) = lower_forin_collection(collection, cx, env);
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
                    method_kind,
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
        // c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema chose the
        // branch (`selected_then`); the AST `emit_stmts` emits ONLY that branch's
        // statements INLINE on the SAME `&mut env` at the SAME indent (no `if`, no
        // block — its `let`s leak into the outer scope). Reproduce both: lower the
        // selected branch's statements on the SAME `env` (so their bindings leak, like
        // the AST shared env) and wrap them in a flat `Inline` node.
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => {
            let chosen: &[Stmt] = match selected_then {
                Some(true) => then_body,
                Some(false) => else_body.as_deref().unwrap_or(&[]),
                // Sema didn't resolve (earlier error) — emit nothing (I3), like the AST.
                None => &[],
            };
            TStmt::Inline(lower_stmts(chosen, cx, env))
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region (`Stmt::Unsafe`). The AST
        // `emit_stmts` emits `unsafe { … }` and lowers the body on the SAME `&mut env`
        // (the body's `let`s leak into the outer scope). Reproduce: lower the body on the
        // SAME `env` (so bindings leak) and wrap in `TStmt::Unsafe`. The `#Audit("…")`
        // annotation is dropped (codegen is dumb — it emits nothing, matching the AST).
        // I1: the source `#Unsafe` gate is 1:1 with this node, the only producer of a
        // Rust `unsafe` block.
        Stmt::Unsafe { body, .. } => TStmt::Unsafe(lower_stmts(body, cx, env)),
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1). The AST emits a plain
        // block and lowers the body on the SAME `&mut env` (its `let`s leak into the outer
        // scope). Reproduce: lower the body on the SAME `env`, wrap in `TStmt::Region`.
        Stmt::Region { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1). `emit_stmt`'s
        // `Stmt::Caps` arm is byte-for-byte `Stmt::Region` — a plain block with the body lowered
        // on the SAME `&mut env` (its `let`s leak). Effects erase at codegen (I3); reuse the
        // `TStmt::Region` shape.
        Stmt::Caps { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1). Resolve each
        // field into an `(is_allocator, value)` guard at lowering, then lower the body on
        // the SAME `env` (it leaks like a region). Emit reproduces `emit_stmts`'s
        // `Stmt::ContextBlock` arm byte-for-byte.
        Stmt::ContextBlock { fields, body, .. } => {
            let guards = fields
                .iter()
                .map(|(name, v, _)| {
                    let is_alloc = name == Syntax::CTX_FIELD_ALLOCATOR;
                    (is_alloc, lower_expr(v, cx, env))
                })
                .collect();
            TStmt::ContextBlock {
                guards,
                body: lower_stmts(body, cx, env),
            }
        }
        // Forward-safety default: a Stmt variant not in the subset never reaches
        // lowering (`stmt_in_subset` returns false for it). Kept as a guard against a
        // future variant; currently unreachable because every covered variant is matched.
        #[allow(unreachable_patterns)]
        _ => unreachable!("statement not in TIR subset"),
    }
}

/// Pull the bare label name out of an `@name` loop label, dropping the span. The
/// emitter renders it as `'jet_<name>:` (mirroring `loop_label_prefix`).
fn label_name(label: &Option<(String, Span)>) -> Option<String> {
    label.as_ref().map(|(n, _)| n.clone())
}

/// c109 Phase 22: resolve a `loop x in <coll>` collection into its emitted Rust
/// string + (for a method-call collection) the iteration form, reproducing
/// `emit_for_in`'s branch selection (Source/Codegen/Statement.rs) byte-for-byte.
/// For `chars`/`lines` the returned string is the *receiver* (the form emits
/// `({recv}).chars()` / `BufRead::lines(&mut ({recv}).inner)`); for the plain form
/// (incl. a non-special method call routed to `.iter().cloned()`) it is the whole
/// collection. The FileReader-vs-stdin `lines` split mirrors the AST's
/// `expr_jet_ty(receiver)` / inline-`io.stdin()` test exactly.
fn lower_forin_collection(
    collection: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (String, Option<TForInMethod>) {
    if let Expr::MethodCall {
        receiver, method, ..
    } = collection
    {
        match method.as_str() {
            "chars" => {
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                return (recv, Some(TForInMethod::Chars));
            }
            "lines" => {
                // FileReader streaming vs stdin streaming — the AST tests
                // `expr_jet_ty(receiver)` (reproduced by `tir_recv_jet_ty`) for the
                // FileReader case, then a `StdinHandle` type OR an inline `io.stdin()`
                // receiver for the stdin case. Checked in the SAME order as
                // `emit_for_in` (FileReader first).
                let recv = emit_tir_expr(&lower_expr(receiver, cx, env), cx);
                if matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "FileReader")
                {
                    return (recv, Some(TForInMethod::LinesFile));
                }
                // stdin: a `StdinHandle`-typed receiver OR an inline `io.stdin()` call.
                let is_stdin = matches!(tir_recv_jet_ty(receiver, env), Some(Type::Named(n)) if n == "StdinHandle")
                    || matches!(receiver.as_ref(), Expr::MethodCall { method: m, .. } if m == "stdin");
                if is_stdin {
                    return (recv, Some(TForInMethod::LinesStdin));
                }
                // A `.lines()` on neither (unreachable in valid Jet — sema E2502
                // restricts `.lines()` to a FileReader/StdinHandle loop position) would
                // fall to the AST `else` default; reproduce that for totality.
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
            _ => {
                // The `.iter().cloned()` default: emit the WHOLE method call as the
                // collection value (e.g. a `.split(…)` builtin returning a `[String]`).
                let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
                (coll, None)
            }
        }
    } else {
        let coll = emit_tir_expr(&lower_expr(collection, cx, env), cx);
        (coll, None)
    }
}

/// c109 Phase 22: lower an `if` condition into a `TIfCond`, plus the optional
/// then-branch binding the condition introduces (name, rust place, resolved type).
/// Reproduces `emit_if`'s condition handling (Source/Codegen/Statement.rs):
///  - `x == null` (`Pattern::Absent`) → `IsNone` (no binding);
///  - `value(b)`/`ok(b)`/`err(b)` → `IfLet` with the Rust pattern from
///    `emit_if_let_pattern`, the binding's type resolved off the subject's lowered
///    `Option`/`Result` (mirroring `add_pattern_bindings`);
///  - anything else → `Plain`.
fn lower_if_cond(
    cond: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> (TIfCond, Option<(String, String, Option<Type>)>) {
    if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = cond
    {
        let subj = lower_expr(subject, cx, env);
        return (TIfCond::IsNone { subj }, None);
    }
    // c109 Phase 24: a JSON variant if-let (`if data == Object(entries)`). The Rust
    // if-let pattern is the JSON-aware `emit_if_let_pattern` (`{root}jet_std::Json::Object(
    // user_entries)`); the binding's type comes from `core_json_pattern_types` (the same
    // total fact `variant_binding_types` reads, mirroring `add_pattern_bindings`).
    if let Expr::PatternTest {
        subject,
        pattern: pattern @ Pattern::Variant { variant, bindings, .. },
        ..
    } = cond
    {
        if is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                let ty = crate::Sema::core_json_pattern_types(variant)
                    .and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                );
            }
        }
        // c109 (B4): a USER-enum variant if-let (`if m == Ping(n)`). The Rust if-let
        // pattern is the same `emit_if_let_pattern` (`user_E::user_V(user_b)`), and the
        // binding's type is the variant's first payload type from `variant_binding_types`
        // (the same total fact `add_pattern_bindings` reads on the AST path).
        if !is_json_variant(variant) {
            if let Some(PatSlot::Bind(name)) = bindings.first() {
                let subj = lower_expr(subject, cx, env);
                let pat_str = emit_if_let_pattern(cx, pattern);
                let ty = variant_binding_types(cx, variant)
                    .and_then(|ts| ts.into_iter().next());
                let place = mangle(name);
                return (
                    TIfCond::IfLet { pat_str, subj },
                    Some((name.clone(), place, ty)),
                );
            }
        }
    }
    if let Expr::PatternTest {
        subject, pattern, ..
    } = cond
    {
        if matches!(
            pattern,
            Pattern::Present { .. } | Pattern::Ok { .. } | Pattern::Err { .. }
        ) {
            let subj = lower_expr(subject, cx, env);
            let pat_str = emit_if_let_pattern(cx, pattern);
            // The bound name + its inner type, off the subject's resolved Option/Result
            // (totality — never re-inferred). Mirrors `add_pattern_bindings`.
            let binding = match pattern {
                Pattern::Present { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Option(inner) => Some((**inner).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Ok { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { ok, .. } => Some((**ok).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                Pattern::Err { binding, .. } => {
                    let ty = match &subj.ty {
                        Type::Result { err, .. } => Some((**err).clone()),
                        _ => None,
                    };
                    (binding.clone(), ty)
                }
                _ => unreachable!("checked above"),
            };
            let (name, ty) = binding;
            let place = mangle(&name);
            return (
                TIfCond::IfLet { pat_str, subj },
                Some((name, place, ty)),
            );
        }
    }
    (TIfCond::Plain(lower_expr(cond, cx, env)), None)
}

fn lower_if(ifs: &IfStmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    // c109 Phase 22: classify the condition (plain / if-let / is_none), reproducing
    // `emit_if`'s three head shapes. The if-let form binds its name into the
    // then-branch scope (mirroring `add_pattern_bindings`).
    let (cond, then_binding) = lower_if_cond(&ifs.cond, cx, env);
    // Each branch gets its own `locals` scope (deep-cloned, so a `let` is not visible
    // after the `if`). The panic-dump replica leaks for a plain/`is_none` `if` (the AST
    // `emit_if` passes the SHARED `&mut env` to `emit_stmts` → `clone_env`), but does
    // NOT leak for an if-let condition (the AST clones the env into a fresh `body_env`
    // before `add_pattern_bindings` → `fork_panic`, a deep-copied replica), so a `let`
    // inside an if-let then-body is scoped exactly as the AST's `body_env`.
    let then_body = {
        let mut branch = if then_binding.is_some() {
            fork_panic(env)
        } else {
            clone_env(env)
        };
        if let Some((name, place, ty)) = then_binding {
            branch.bind(&name, place, ty);
        }
        lower_stmts(&ifs.then_body, cx, &mut branch)
    };
    let (else_body, else_is_elseif) = match &ifs.else_branch {
        None => (None, false),
        Some(ElseBranch::Else(body)) => {
            let mut branch = clone_env(env);
            (Some(lower_stmts(body, cx, &mut branch)), false)
        }
        // `else if` nests as an else-body holding a single `If`; the flag marks it so
        // emit renders `} else if …` (an explicit `else { if … }` block does NOT).
        Some(ElseBranch::ElseIf(next)) => {
            let mut branch = clone_env(env);
            (Some(vec![lower_if(next, cx, &mut branch)]), true)
        }
    };
    TStmt::If {
        cond,
        then_body,
        else_body,
        else_is_elseif,
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
    if else_body.is_some() && arms.iter().all(|a| arm_head_range(cx, &a.cond, subject).is_some()) {
        return lower_range_switch(subject, arms, else_body, cx, env);
    }
    // Shape C (c109 Phase 8): all arms are fallible/optional patterns → a Rust match
    // over the subject's Result/Option (`Ok(..)`/`Err(..)`/`Some(..)`/`None`).
    if arms
        .iter()
        .all(|a| arm_fallible_pattern(cx, &a.cond, subject).is_some())
    {
        return lower_fallible_match(subject, arms, else_body, cx, env);
    }
    // Shape D (c109 Phase 15): all arms are plain comparison/Bool conds → the general
    // mixed `if/else if … else` chain (`emit_mixed_switch`).
    if arms.iter().all(|a| arm_is_plain_cond(cx, &a.cond, subject)) {
        return lower_mixed_switch(subject, arms, else_body, cx, env);
    }
    // Shape A: exhaustive enum match (`emit_pattern_match_switch`).
    lower_enum_match(subject, arms, else_body, cx, env)
}

/// c109 Phase 15: lower a MIXED comparison/Bool `when` switch (shape D) to a
/// `TStmt::MixedSwitch`, reproducing `emit_mixed_switch` (Source/Codegen/Statement.rs).
/// The subject is bound once to `_jet_switch_subject = &(subject)` (emitted for parity);
/// each arm's PLAIN condition is resolved to a Rust string at lowering (`emit_expr`); the
/// arm bodies + `else` are lowered on a SHARED env (leaky, like the AST `&mut env`).
fn lower_mixed_switch(
    subject: &Expr,
    arms: &[SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TStmt {
    // The subject's emitted string, used once for the `_jet_switch_subject` borrow —
    // exactly as `emit_mixed_switch` re-emits `emit_expr(subject)`.
    let subject_str = emit_tir_expr(&lower_expr(subject, cx, env), cx);
    let mut tarms = Vec::new();
    for arm in arms {
        // A plain comparison/Bool arm head → `emit_switch_arm_cond`'s `emit_expr(cond)`.
        let cond_str = emit_tir_expr(&lower_expr(&arm.cond, cx, env), cx);
        // The arm body uses the SHARED `&mut env` in `emit_mixed_switch` (leaks).
        let mut branch = clone_env(env);
        let body = lower_stmts(&arm.body, cx, &mut branch);
        tarms.push((cond_str, body));
    }
    let else_lowered = else_body.as_ref().map(|body| {
        let mut branch = clone_env(env);
        lower_stmts(body, cx, &mut branch)
    });
    TStmt::MixedSwitch {
        subject_str,
        arms: tarms,
        else_body: else_lowered,
    }
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
        let pattern = arm_fallible_pattern(cx, &arm.cond, subject).expect("gate proved fallible arm");
        let pat = tir_fallible_pattern(&pattern);
        // An arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_fallible_binding(&pattern, &mut body_env, &subject_ty);
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard: None, body });
    }
    // The `else` arm uses the SHARED `&mut env` in `emit_pattern_match_switch` (leaks).
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
    env.bind(&binding, mangle(&binding), ty);
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
        // The arm body sees the variant's payload bindings, typed from the layout. The
        // arm body is a CLONED env in `emit_pattern_match_switch` (no leak) — fork.
        let mut body_env = fork_panic(env);
        tir_add_pattern_bindings(cx, &pattern, &mut body_env, subject_ty.as_ref());
        let body = lower_stmts(&arm.body, cx, &mut body_env);
        tarms.push(TMatchArm { pattern: pat, guard, body });
    }
    // The `else` arm uses the SHARED `&mut env` in `emit_pattern_match_switch` (leaks).
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
        let (lo, hi) = arm_head_range(cx, &arm.cond, subject).expect("gate proved range arm");
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

/// TIR-local reproduction of codegen's `emit_match_pattern` for the enum-match (shape
/// A) case the subset covers. c109 Phase 24: this now DELEGATES to the AST
/// `emit_match_pattern` (made `pub(crate)`), which is PURE formatting (it takes only
/// `cx` + the pattern + the resolved enum type — no env, no inference), so reusing it is
/// byte-parity-safe and automatically handles the FOREIGN-enum (`{root}{mod}::user_<T>::
/// user_<V>`) and JSON (`{root}jet_std::Json::<Variant>`, non-mangled) variant prefixes
/// the subset now admits — the same reuse Phase 22 made for `emit_if_let_pattern`.
fn tir_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    emit_match_pattern(cx, pattern, enum_type)
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
                    env.bind(b, mangle(b), Some(ty));
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

/// The payload field types a variant binds, from the resolved enum layout. c109
/// Phase 24: DELEGATES to the AST `variant_binding_types` (made `pub(crate)`), which
/// handles the JSON enum (`core_json_pattern_types`) AND user/foreign enums
/// (`cx.variant_owner` → `cx.enum_variants`) — pure table lookups, no env/inference —
/// so the bound payload type is byte-parity-faithful for every covered enum (e.g. a
/// foreign `ParseError.NoFrontmatter(p)` binds `p: String`).
fn variant_payload_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    variant_binding_types(cx, variant)
}

/// c109 Phase 24: the Rust enum-literal head `{prefix}::{mangle(variant)}` for a payload
/// or named enum literal, reproducing `emit_enum_lit`'s `type_prefix` (Expression.rs): a
/// FOREIGN (imported) enum → `{root}{mod}::user_<T>::user_<V>`, a local enum →
/// `user_<T>::user_<V>`. Keyed on the ENUM name in `cx.foreign_types`, byte-for-byte.
fn tir_enum_lit_prefix(cx: &Cx, type_name: &str, variant: &str) -> String {
    let type_prefix = match cx.foreign_types.get(type_name) {
        Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
        None => format!("user_{}", type_name),
    };
    format!("{}::{}", type_prefix, mangle(variant))
}

/// c109 Phase 16: the single-payload type of `(type_name, edge)`, mirroring the AST
/// `enum_variant_payload_type` (Expression.rs). `edge` is the VARIANT name for a
/// positional arg, or `"Variant.label"` for a named arg — the latter never matches a
/// variant name, so it returns `None` (the AST never clones a named-payload arg), as
/// `enum_variant_payload_type` does. Only `Single(t)` / single-field `Named` resolve.
fn enum_variant_payload_type<'a>(cx: &'a Cx, type_name: &str, edge: &str) -> Option<&'a Type> {
    let variants = cx.enum_variants.get(type_name)?;
    let (_, payload) = variants.iter().find(|(v, _)| v == edge)?;
    match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        _ => None,
    }
}

/// c109 Phase 16: lower one enum-literal payload arg, resolving the `clone`/`boxed`
/// decisions as TOTAL facts, reproducing `emit_boxed_enum_arg` (Expression.rs)
/// byte-for-byte. `edge` is the variant name (positional) or `"Variant.label"`
/// (named). A non-scalar single-payload type whose arg is a borrowed-in-env ident
/// gets `(…).clone()`; a recursive (`boxed_edge`) edge gets `Box::new(…)`.
fn lower_enum_arg(
    type_name: &str,
    variant: &str,
    edge: &str,
    e: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TEnumArg {
    let payload_ty = enum_variant_payload_type(cx, type_name, edge);
    let borrowed = matches!(e, Expr::Ident(name, _) if env.is_borrowed(name));
    let clone = payload_ty.is_some_and(|t| !t.is_scalar()) && borrowed;
    let boxed = cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()));
    let _ = variant;
    TEnumArg {
        value: lower_expr(e, cx, env),
        clone,
        boxed,
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

/// Clone the env for a LEAKY child scope (a plain block / loop / mixed-or-range switch
/// arm / comptime-if branch / enum-match else). `locals` is deep-cloned (each branch
/// scopes its own bindings for resolution), but `panic_locals` is SHARED (the Rc is
/// cloned, not its contents) so a `let` inside the child leaks into the parent's panic
/// dump — exactly as the AST codegen `&mut env` does (`safe_locals_expr`).
fn clone_env(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        panic_locals: Rc::clone(&env.panic_locals),
        view_return: env.view_return,
    }
}

/// Clone the env for a NON-LEAKY child scope — the two `emit_pattern_match_switch` arm
/// bodies (an enum or fallible/optional match arm; the AST uses `env.clone()` there) and
/// a lambda body (`emit_lambda` clones the env). Here `panic_locals` is DEEP-COPIED, so
/// bindings inside the arm/lambda do NOT leak into the enclosing function's panic dump,
/// matching the AST's cloned `body_env`/`lam_env`.
fn fork_panic(env: &LowerEnv) -> LowerEnv {
    LowerEnv {
        locals: env.locals.clone(),
        fn_name: env.fn_name.clone(),
        panic_locals: Rc::new(RefCell::new(env.panic_locals.borrow().clone())),
        view_return: env.view_return,
    }
}

/// c109 Phase 15: render the `{ jet_panic_rich(…); }` statement string for a
/// `a ?? panic(msg)` fallback, byte-for-byte `emit_panic_stop`
/// (Source/Codegen/Statement.rs). Every input — the panic message (lowered from the
/// message expression), the source-line text / line / column / caret width (from
/// `cx.src` at the `panic` name span), the escaped file + enclosing function name, and
/// the sorted scalar-locals snapshot — is resolved here so emit reads nothing from
/// `cx.src`/`cx.current_fn` (I3).
fn render_panic_stop(
    name_span: &Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> String {
    let msg = render_panic_message(&args[0].expr, cx, env);
    let (src_line, line, col) = tir_src_line_at(&cx.src, name_span.start);
    let caret_len = (name_span.end - name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }}",
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require(cond[, msg])` (S36), byte-for-byte `emit_require`
/// (Source/Codegen/Statement.rs). The default build emits a guarded `jet_panic_rich`;
/// `cx.test_mode` emits a `return Err(<msg>)` form. The condition + message are lowered
/// via the TIR; every source-position/locals fact is resolved here (I3).
fn render_require(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let cond = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let msg = if call.args.len() == 2 {
        render_panic_message(&call.args[1].expr, cx, env)
    } else {
        "\"condition failed\"".to_string()
    };
    if cx.test_mode {
        let msg_expr = if call.args.len() == 2 {
            msg
        } else {
            "\"condition failed\".to_string()".to_string()
        };
        return format!("{{ if !({}) {{ return Err({}); }} }}", cond, msg_expr);
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    let msg_used = if call.args.len() == 2 {
        msg
    } else {
        "\"condition failed\".to_string()".to_string()
    };
    format!(
        "{{ if !({cond}) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &{msg}, &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        cond = cond,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        msg = msg_used,
        locals = locals_expr,
    )
}

/// c109 Phase 26: render `require_eq(left, right)` (S36), byte-for-byte
/// `emit_require_eq` (Source/Codegen/Statement.rs). Binds the two operands into temps,
/// then compares; on inequality emits the test-mode `return Err(…)` or the default
/// `jet_panic_rich` with a `left: {}, right: {}` message.
fn render_require_eq(call: &crate::AST::Call, cx: &Cx, env: &mut LowerEnv) -> String {
    let left = emit_tir_expr(&lower_expr(&call.args[0].expr, cx, env), cx);
    let right = emit_tir_expr(&lower_expr(&call.args[1].expr, cx, env), cx);
    if cx.test_mode {
        return format!(
            "{{ let _jet_left = ({}); let _jet_right = ({}); if !(_jet_left == _jet_right) {{ return Err(format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show())); }} }}",
            left, right
        );
    }
    let (src_line, line, col) = tir_src_line_at(&cx.src, call.name_span.start);
    let caret_len = (call.name_span.end - call.name_span.start) as u32;
    let fn_name = env.fn_name.clone();
    let locals_expr = render_safe_locals(env);
    format!(
        "{{ let _jet_left = ({left}); let _jet_right = ({right}); if !(_jet_left == _jet_right) {{ jet_panic_rich({file}, {line}, {fn_name_esc}, {src_line_esc}, {col}, {caret}, &format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show()), &if cfg!(debug_assertions) {{ {locals} }} else {{ String::new() }}); }} }}",
        left = left,
        right = right,
        file = escape_rust_str(&cx.file),
        line = line,
        fn_name_esc = escape_rust_str(&fn_name),
        src_line_esc = escape_rust_str(src_line.trim_end()),
        col = col,
        caret = caret_len,
        locals = locals_expr,
    )
}

/// c109 Phase 15: reproduce `emit_panic_message` (Statement.rs): a `Str` literal emits
/// its interpolated form directly; any other expression is `({…}).jet_show()`. The
/// message expression is lowered + emitted via the TIR (= `emit_expr`).
fn render_panic_message(e: &Expr, cx: &Cx, env: &mut LowerEnv) -> String {
    match e {
        Expr::Str(_, _) => emit_tir_expr(&lower_expr(e, cx, env), cx),
        other => format!("({}).jet_show()", emit_tir_expr(&lower_expr(other, cx, env), cx)),
    }
}

/// c109 Phase 15: reproduce `src_line_at` (Statement.rs) — the (line text, 1-based line,
/// 1-based column) for a byte offset.
fn tir_src_line_at(src: &str, offset: usize) -> (&str, u32, u32) {
    let (line, col) = crate::Diagnostics::span_line_col(src, offset);
    let line_start = src[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_end = src[offset..].find('\n').map(|p| offset + p).unwrap_or(src.len());
    (&src[line_start..line_end], line as u32, col as u32)
}

/// c109 Phase 15: reproduce `safe_locals_expr` (Statement.rs) from the `panic_locals`
/// env replica (which mirrors the AST codegen `env` leak semantics — see `LowerEnv`).
/// Dumps the FULL replica filtered to scalar Int/Float/Bool slots, sorted by name, as a
/// `format!("name = {}, …", (place).jet_show(), …)` expression. A deref'd slot uses
/// `(*name).jet_show()` (the place already carries the `(*…)` wrapper, which is the bare
/// `(*name)` form, NOT a double-paren). Empty → `String::new()`.
fn render_safe_locals(env: &LowerEnv) -> String {
    let replica = env.panic_locals.borrow();
    let mut parts: Vec<(String, String)> = replica
        .iter()
        .filter_map(|(name, (place, jet_ty))| {
            let safe = jet_ty
                .as_ref()
                .map_or(false, |t| matches!(t, Type::Int | Type::Float | Type::Bool));
            if !safe {
                return None;
            }
            // `safe_locals_expr` builds `(*rust_name).jet_show()` for a deref'd slot and
            // `(rust_name).jet_show()` otherwise. The replica's `place` is exactly
            // `(*rust_name)` (deref) or `rust_name` — decode it back so the rendered
            // string is byte-identical (NOT `((*rust_name)).jet_show()`).
            let value_expr = if place.starts_with("(*") && place.ends_with(')') {
                let rust_name = &place[2..place.len() - 1];
                format!("(*{}).jet_show()", rust_name)
            } else {
                format!("({}).jet_show()", place)
            };
            Some((name.clone(), value_expr))
        })
        .collect();
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    if parts.is_empty() {
        return "String::new()".to_string();
    }
    let fmt_str = parts
        .iter()
        .map(|(n, _)| format!("{} = {{}}", n))
        .collect::<Vec<_>>()
        .join(", ");
    let args = parts
        .iter()
        .map(|(_, e)| e.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("format!(\"{}\", {})", fmt_str, args)
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
            // c109 Phase 24: a comptime CONST inlines its pre-rendered value FIRST (the
            // AST `emit_expr` Ident arm returns `cx.consts[name]` before any env/fn-value
            // check — so a const takes precedence even over a same-named local, matching
            // byte-for-byte). The `ty` is a placeholder (never read — see `ConstInline`).
            if let Some(val) = cx.consts.get(name) {
                return TExpr {
                    ty: env.ty_of(name).unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(val.clone()),
                };
            }
            // c109 Phase 13: a bare function name used as a VALUE (not a local, not a
            // const) emits `emit_named_fn_value` — `Box::new(move |…| user_<name>(…))
            // as <fn-type>`. Mirrors `emit_expr`'s `Expr::Ident` arm (Expression.rs).
            if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
                if let Some(ft @ Type::Fn { .. }) = cx.fn_types.get(name) {
                    return TExpr {
                        ty: ft.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn {
                                wrapper: emit_named_fn_value(cx, name, ft),
                            },
                        },
                    };
                }
            }
            let ty = env.ty_of(name).unwrap_or(Type::Int);
            TExpr {
                ty,
                kind: TExprKind::Local(env.place_of(name)),
            }
        }
        // c109 Phase 13: a call THROUGH a fn-value `(f)(args)` (`Expr::CallValue`). The
        // AST path (`emit_expr`'s `Expr::CallValue`) emits `({callee})({args})` with the
        // args lowered PLAINLY (it passes `None` to `emit_call_args` → no convention/
        // clone/borrow wrappers). Reproduce exactly: lower the callee, lower each arg
        // with `conv = None`. The result type is the callee fn-type's return (total).
        Expr::CallValue { callee, args, .. } => {
            let callee_t = lower_expr(callee, cx, env);
            let ret_ty = match &callee_t.ty {
                Type::Fn { ret: Some(r), .. } => (**r).clone(),
                _ => unit_type(),
            };
            let targs = args
                .iter()
                .map(|a| lower_one_call_arg(a, None, env, cx))
                .collect();
            TExpr {
                ty: ret_ty,
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Call {
                        callee: Box::new(callee_t),
                        args: targs,
                    },
                },
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
            // c109 Phase 13: `f(args)` where `f` is a LOCAL (a fn-typed binding/param)
            // parses as `Expr::Call`. The AST path (`emit_call`, env-contains-name
            // branch) emits `(place)(args)` with args PLAIN (`emit_call_args(.., None)`).
            // Reproduce as a `FnValue::Call` whose callee is the local's place.
            if env.locals.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
                let callee_ty = env.ty_of(&call.name).unwrap_or_else(unit_type);
                let ret_ty = match &callee_ty {
                    Type::Fn { ret: Some(r), .. } => (**r).clone(),
                    _ => unit_type(),
                };
                let callee_t = TExpr {
                    ty: callee_ty,
                    kind: TExprKind::Local(env.place_of(&call.name)),
                };
                let targs = call
                    .args
                    .iter()
                    .map(|a| lower_one_call_arg(a, None, env, cx))
                    .collect();
                return TExpr {
                    ty: ret_ty,
                    kind: TExprKind::FnValue {
                        kind: TFnValueKind::Call {
                            callee: Box::new(callee_t),
                            args: targs,
                        },
                    },
                };
            }
            // `print` is ambient only when the user has not defined their own
            // `print` function (matches emit_call; sema enforces the shadowing).
            if call.name == Syntax::BUILTIN_PRINT && !cx.sigs.contains_key(&call.name) {
                let arg = lower_expr(&call.args[0].expr, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Print(Box::new(arg)),
                };
            }
            // c109 Phase 26: the rich-runtime-report builtins (S36) — render the whole
            // emit string at lowering, byte-for-byte the AST helper. `require`/`panic`
            // are statement-position calls (a `()` result); the string is the `{ … }`
            // block emit emits as an expr-statement. Disjoint from a user fn of the same
            // name (`cx.sigs.contains_key` would be true then).
            if !cx.sigs.contains_key(&call.name) && !env.locals.contains_key(&call.name) {
                if call.name == Syntax::BUILTIN_REQUIRE {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_require(call, cx, env)),
                    };
                }
                if call.name == Syntax::BUILTIN_REQUIRE_EQ {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_require_eq(call, cx, env)),
                    };
                }
                if call.name == Syntax::BUILTIN_PANIC {
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::RequireStop(render_panic_stop(
                            &call.name_span,
                            &call.args,
                            cx,
                            env,
                        )),
                    };
                }
            }
            // c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). Same
            // lowering as `io.input(...)` (CoreCall would also work, but the bare-call
            // surface has no module alias, so it is its own node). Resolves to
            // `Result<String, IOError>` (matching sema), so it composes with the
            // Phase-8 `??` fallback. The prompt arg (if any) is lowered in-subset.
            if call.name == Syntax::BUILTIN_INPUT
                && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                let prompt = call.args.first().map(|a| Box::new(lower_expr(&a.expr, cx, env)));
                return TExpr {
                    ty: Type::Result {
                        ok: Box::new(Type::String),
                        err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                    },
                    kind: TExprKind::AmbientInput { prompt },
                };
            }
            // c109 Phase 28: the overflow opt-out builtins `wrapping(e)`/`saturating(e)`/
            // `checked(e)` (D-NUMOPS1). The gate proved the name is one of the three (not
            // shadowed) and the sole arg is an integer `Expr::Binary`. Reproduce
            // `emit_call`'s arm (Expression.rs ~L1756): `(lhs).{name}_{op}(rhs)` with PLAIN
            // operands (no trap helper). `checked_*` returns `Option<T>`; the others return
            // `T` — set the result type accordingly so a `checked(...) ?? x` composes.
            if matches!(
                call.name.as_str(),
                Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
            ) && !cx.sigs.contains_key(&call.name)
                && !env.locals.contains_key(&call.name)
            {
                if let Some(Expr::Binary(op, l, r, _)) = call.args.first().map(|a| &a.expr) {
                    let op_suffix = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        BinOp::Div => "div",
                        // Sema validated an arithmetic op; mirror the AST default.
                        _ => "add",
                    };
                    let lhs = lower_expr(l, cx, env);
                    let rhs = lower_expr(r, cx, env);
                    let val_ty = lhs.ty.clone();
                    let result_ty = if call.name == Syntax::BUILTIN_CHECKED {
                        Type::Option(Box::new(val_ty))
                    } else {
                        val_ty
                    };
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::OverflowOpt {
                            prefix: call.name.clone(),
                            op: op_suffix,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                    };
                }
            }
            // c109 Phase 14: an FFI extern call (`emit_call`'s `extern_funcs` arm).
            // Checked BEFORE the unqualified arms, matching `emit_call`'s order. Args
            // use `emit_extern_call_args` (a non-scalar `Read` is `(…).clone()`).
            if !env.locals.contains_key(&call.name) {
                if let Some(wrapper) = cx.extern_funcs.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&call.name).cloned();
                    let eargs = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_extern_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    // The extern fn's return type lives in `cx.fn_types` only if the
                    // function is also a normal sig; extern fns are not in `fn_types`,
                    // so fall back to Unit (the binding carries the real type — the call
                    // result type is rarely load-bearing, like every covered call).
                    return TExpr {
                        ty: call_return_type(cx, &call.name),
                        kind: TExprKind::ExternCall { wrapper, args: eargs },
                    };
                }
                // c109 Phase 14: unqualified inline-module import (`emit_call`'s
                // `unqualified_inline` arm) → `{root}user_{mangled}(args)`.
                if let Some(mangled_key) = cx.unqualified_inline.get(&call.name).cloned() {
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled { mangled: mangled_key },
                            args,
                        },
                    };
                }
                // c109 Phase 14: unqualified file-module import (`emit_call`'s
                // `unqualified_file` arm) → `{root}{rust_mod}::{mangle(fn)}(args)`. The
                // AST looks up the sig under `(call.name, fn_name)`.
                if let Some((rust_mod, fn_name)) = cx.unqualified_file.get(&call.name).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned();
                    let args = call
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                            lower_one_call_arg(a, conv, env, cx)
                        })
                        .collect();
                    let ret = cx
                        .import_rets
                        .get(&(call.name.clone(), fn_name.clone()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod,
                                rust_fn: mangle(&fn_name).to_string(),
                            },
                            args,
                        },
                    };
                }
            }
            // Resolve the callee's signature so each arg's borrow/clone/fn-coercion is
            // decided here, totally — via the shared `lower_one_call_arg` (the single
            // `emit_call_args` reproduction). c109 Phase 13: a callee with a Fn-typed
            // param (now in subset) routes its arg through the Box-coercion form.
            let sig = cx.sigs.get(&call.name).cloned();
            let args = call
                .args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let conv = sig.as_ref().and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
                    lower_one_call_arg(a, conv, env, cx)
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
        Expr::MethodCall { receiver, method, method_span, args, recv_type, resolved_ret } => {
            lower_method_call(receiver, method, *method_span, args, recv_type, resolved_ret.as_ref(), cx, env)
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
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            // c109 Phase 30: a trait-object coercion (`Circle {…}` in a `[Shape]` list). The
            // AST wraps the rendered literal `Box::new(<lit>) as Box<dyn user_<Trait>>`; the
            // value's type is the trait object. Resolved here (totality) — only the plain
            // user-struct branch below carries it (a coerced import_ns/prelude literal is
            // not a construct any covered program produces; the gate keeps those uncoerced).
            let trait_coerce = as_trait
                .as_ref()
                .map(|t| crate::Generics::user_trait_rust(t));
            // c109 Phase 19: a FOREIGN (imported user) struct literal `alias.Type { … }`
            // (`import_ns`). The AST `emit_struct_lit` `import_ns` branch emits
            // `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED fields.
            // Resolve the head here (totality); a missing alias falls to `user_unknown`,
            // exactly as the AST path (the gate already required the alias to resolve).
            if let Some(alias) = import_ns {
                let mod_name = cx
                    .import_mods
                    .get(alias)
                    .map(|s| s.as_str())
                    .unwrap_or("user_unknown");
                let rust_type = if type_args.is_empty() {
                    format!("{}{}::{}", cx.root_prefix, mod_name, mangle(type_name))
                } else {
                    format!(
                        "{}{}::{}::<{}>",
                        cx.root_prefix,
                        mod_name,
                        mangle(type_name),
                        type_args
                            .iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                // A foreign struct's fields are never local boxed edges (boxed_edges
                // hold this module's recursive structs), so no field is boxed here.
                let tfields = fields
                    .iter()
                    .map(|(n, _, fe)| (mangle(n), lower_expr(fe, cx, env), false))
                    .collect();
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type,
                        fields: tfields,
                        extra: None,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 17: a PRELUDE struct literal (HttpRequest/HttpResponse) uses the
            // `is_prelude_struct` branch of `emit_struct_lit`: a `<root>Jet…` Rust head,
            // PLAIN (unmangled) field names, and — for HttpRequest — an injected
            // `params: std::collections::BTreeMap::new()` field. Reproduce it byte-for-byte.
            if let Some(rust) = net_handle_rust_type(type_name) {
                // A prelude struct has no boxed (recursive) edges.
                let mut tfields: Vec<(String, TExpr, bool)> = fields
                    .iter()
                    .map(|(n, _, fe)| (n.clone(), lower_expr(fe, cx, env), false))
                    .collect();
                let extra = if type_name == "HttpRequest" {
                    Some("params: std::collections::BTreeMap::new()".to_string())
                } else {
                    None
                };
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::StructLit {
                        rust_type: format!("{}{}", cx.root_prefix, rust),
                        fields: tfields.drain(..).collect(),
                        extra,
                        as_trait: None,
                    },
                };
            }
            // c109 Phase 19: a GENERIC struct literal carries `type_args` (`Pair<T> {…}`).
            // The Rust head is the turbofish `user_<Name>::<args>` (`user_type_apply_rust`),
            // resolved at lowering; fields mangle. A non-generic literal renders `user_<Name>`.
            // c109: an UNqualified FOREIGN struct (`Note { … }`, no `import_ns`) prefixes its
            // module head (`{root}user_<mod>::user_<Note>`), exactly as `user_type_apply_rust`
            // — or rustc can't find the type (E0422). A local struct keeps the plain head.
            let head = match cx.foreign_types.get(type_name) {
                Some(rust_mod) => format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name),
                None => format!("user_{}", type_name),
            };
            let rust_type = if type_args.is_empty() {
                head
            } else {
                format!(
                    "{}::<{}>",
                    head,
                    type_args
                        .iter()
                        .map(|a| cx.rust_type(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            // c109: a self-referential field (`child: Tree?` on `Tree`) has Rust type
            // `Box<…>` (`cx.boxed_edges`); resolve the `boxed` flag here (a total fact)
            // so emit can wrap the value in `Box::new(…)`, exactly as `emit_struct_lit`.
            let tfields = fields
                .iter()
                .map(|(n, _, fe)| {
                    let boxed = cx
                        .boxed_edges
                        .contains(&(type_name.clone(), n.clone()));
                    (mangle(n), lower_expr(fe, cx, env), boxed)
                })
                .collect();
            // c109 Phase 30: a trait-coerced literal's value type is the trait object (so a
            // list of them types `[Shape]`); an uncoerced literal keeps its struct type.
            let ty = match as_trait {
                Some(t) => Type::TraitObject(t.clone()),
                None => Type::Named(type_name.clone()),
            };
            TExpr {
                ty,
                kind: TExprKind::StructLit {
                    rust_type,
                    fields: tfields,
                    extra: None,
                    as_trait: trait_coerce,
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
                    // c109 Phase 24: a FOREIGN enum's unit literal (`NoteType.User` in
                    // search.jet) qualifies with the module path, exactly as `emit_expr`'s
                    // `Field` arm (Expression.rs ~L232): `{root}{mod}::user_<Enum>::<V>`.
                    // Keyed on the ENUM-name (`enum_name`, the receiver) in `cx.foreign_types`,
                    // NOT the variant — matching the AST byte-for-byte.
                    let prefix = match cx.foreign_types.get(enum_name.as_str()) {
                        Some(rust_mod) => format!(
                            "{}{}::user_{}::{}",
                            cx.root_prefix,
                            rust_mod,
                            enum_name,
                            mangle(member)
                        ),
                        None => format!("user_{}::{}", enum_name, mangle(member)),
                    };
                    return TExpr {
                        ty: Type::Named(enum_name.clone()),
                        kind: TExprKind::EnumLit {
                            prefix,
                            payload: TEnumPayload::Unit,
                        },
                    };
                }
                // c109 Phase 24: `JSON.Null` → `{root}jet_std::Json::Null` (a unit JSON
                // construction reaching codegen as a `Field`, the gate proved it).
                if env.ty_of(enum_name).is_none()
                    && is_json_type_name(enum_name)
                    && member == "Null"
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_JSON.to_string()),
                        kind: TExprKind::JsonLit {
                            variant: "Null".to_string(),
                            arg: None,
                        },
                    };
                }
                // c109 Phase 28: a numeric BOUNDS constant (`U8.MAX`/`I32.MIN`/
                // `Float.INFINITY`/…). The gate proved the receiver is a numeric type
                // name and `member` a bounds-const name. Reproduce the AST `emit_expr`
                // Field arm (Expression.rs ~L224): `{rust_type(nt)}::{member}`. The
                // rendered Rust string is total here; the result type is the numeric
                // type itself (`U8` for `U8.MAX`, `Float` for `Float.INFINITY`).
                if env.ty_of(enum_name).is_none() {
                    if let Some(nt) = crate::AST::numeric_type_from_name(enum_name) {
                        if is_numeric_bounds_const(member) {
                            return TExpr {
                                ty: nt.clone(),
                                kind: TExprKind::ConstInline(format!(
                                    "{}::{}",
                                    cx.rust_type(&nt),
                                    member
                                )),
                            };
                        }
                    }
                }
            }
            let recv = lower_expr(receiver, cx, env);
            let field_ty = struct_field_type(cx, &recv.ty, member).unwrap_or(Type::Int);
            // A field of a CORE struct (`ProcessResult.code`, `JsonError.message`, …) is
            // emitted by its PLAIN Rust name, never `user_<name>` (the core structs in
            // Source/Prelude/Core.rs declare unprefixed fields — B2). Reproduce
            // `core_struct_field_rust_name` (Expression.rs) from the resolved receiver
            // type so the field read is byte-exact for both core and user structs.
            let field_rust =
                core_struct_field_rust_name(&recv.ty, member).unwrap_or_else(|| mangle(member));
            // A self-referential (recursive) edge has Rust type `Box<…>`; the read derefs
            // to the inner type (total fact from `cx.boxed_edges`, keyed on the receiver's
            // resolved struct name — mirrors the AST `boxed_field_read`).
            let boxed = match &recv.ty {
                Type::Named(n) => cx.boxed_edges.contains(&(n.clone(), member.to_string())),
                _ => false,
            };
            TExpr {
                ty: field_ty,
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field_rust,
                    boxed,
                },
            }
        }
        // c109 Phase 4/16: an enum literal. Each payload arg carries its resolved
        // `clone`/`boxed` decisions (`emit_boxed_enum_arg`): a non-scalar payload from
        // a borrowed-in-env ident → `(…).clone()`; a recursive boxed edge →
        // `Box::new(…)`. For a scalar payload from a non-borrowed value both are false
        // (the Phase-4 no-op), so emit is byte-identical. Positional edges key on the
        // variant name; named edges on `"Variant.label"` (never a clone — matches AST).
        Expr::EnumLit { type_name, variant, args, .. } => {
            let prefix = tir_enum_lit_prefix(cx, type_name, variant);
            let payload = if args.is_empty() {
                TEnumPayload::Unit
            } else if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
                let pos = args
                    .iter()
                    .map(|a| match a {
                        EnumLitArg::Positional(e) => {
                            lower_enum_arg(type_name, variant, variant, e, cx, env)
                        }
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
                            let edge = format!("{}.{}", variant, label);
                            (
                                mangle(label),
                                lower_enum_arg(type_name, variant, &edge, expr, cx, env),
                            )
                        }
                        // A positional arg mixed with named is a sema error that
                        // never reaches a covered function; default to a field.
                        EnumLitArg::Positional(e) => (
                            String::new(),
                            lower_enum_arg(type_name, variant, variant, e, cx, env),
                        ),
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
        // c109 Phase 23: a named-tuple literal → a generated `JetTup_<hash>` struct
        // literal. The gate guaranteed `ty` is `Some(Type::Tuple)`. Reproduce
        // `emit_expr`'s `TupleLit` arm: the CANONICAL field order + struct name come
        // from the type; each canonical field's value is taken from the literal (by
        // name) and lowered. Fields are emitted as `user_<f>: <v>` in canonical order.
        Expr::TupleLit(lit_fields, _, ty) => {
            let canonical = match ty {
                Some(Type::Tuple(fs)) => tuple_fields_plain(fs),
                _ => Vec::new(),
            };
            let struct_name = tuple_struct_name(&canonical);
            // Map field-name → its literal value expr (the literal may list fields in
            // any order; the type fixes the canonical order — exactly the AST path).
            let mut value_of: std::collections::HashMap<&str, &Expr> =
                std::collections::HashMap::new();
            for (n, e) in lit_fields {
                value_of.insert(n.as_str(), e);
            }
            let fields: Vec<(String, TExpr)> = canonical
                .iter()
                .map(|(n, fty)| {
                    let v = match value_of.get(n.as_str()) {
                        Some(e) => lower_expr(e, cx, env),
                        // A missing field never occurs in a sema-checked tuple literal;
                        // mirror the AST's `0i64` default defensively (an Int literal).
                        None => TExpr {
                            ty: fty.clone(),
                            kind: TExprKind::IntLit(0, None),
                        },
                    };
                    (mangle(n).to_string(), v)
                })
                .collect();
            TExpr {
                ty: ty.clone().unwrap_or_else(|| Type::Tuple(Vec::new())),
                kind: TExprKind::TupleLit {
                    struct_name,
                    fields,
                },
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
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. The expected-type
        // STRING is the total sema fact (gate guarantees `Some`); the source line is
        // resolved here. The result `ty` is never load-bearing (a `todo!()` diverges and
        // is never an arithmetic operand), so a placeholder suffices — the emitted Rust
        // reads only `expected_type`/`line`/`cx.file`, byte-for-byte Expression.rs.
        Expr::Todo { span, expected_type } => {
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Todo {
                    line,
                    expected_type: expected_type.clone().unwrap_or_else(|| "(unknown)".to_string()),
                },
            }
        }
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
                // c109 Phase 15: the `panic(…)` form — render the whole
                // `{ jet_panic_rich(…); }` statement string at lowering, byte-for-byte
                // `emit_panic_stop`/`safe_locals_expr`, so emit reads nothing from
                // `cx.src`/`cx.current_fn`.
                OrFallback::Panic { name_span, args } => {
                    TOrFallback::Panic(render_panic_stop(name_span, args, cx, env))
                }
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
        // c109 Phase 11: a lambda/closure literal. The gate proved the body is
        // in-subset; lower it via `lower_lambda` (capture/escape facts total from
        // `Lambda.meta`). The result type is the closure's fn type — rarely
        // load-bearing in emit (a closure is consumed in arg position), so carry a
        // placeholder `Fn` type; the binding/arg context supplies the real Rust type.
        Expr::Lambda(lam) => {
            let tl = lower_lambda(lam, cx, env);
            TExpr {
                ty: Type::Fn {
                    params: Vec::new(),
                    ret: None,
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        // c109 Phase 11: fan-out `f.[a, b, c]` (S75/S76). The gate proved the callee
        // is a plain top-level fn ident and every item is in-subset. The AST path
        // routes the Ident callee through `emit_call` with a SYNTHETIC single-arg
        // `Call` (`convention: Read`, default flags) per item; reproduce that exactly
        // as a `TExprKind::Call` per item, then `vec![…]`. The result type is `[T#N]`
        // (S76), erased to a list of the callee's return type.
        Expr::FanOut { callee, items, .. } => {
            let Expr::Ident(name, _) = callee.as_ref() else {
                unreachable!("gate proved fan-out callee is a plain fn ident");
            };
            // The callee's signature drives each synthetic arg's borrow wrapper,
            // exactly as `emit_call_args` does for the synthetic `Read` arg (whose
            // `implicit_clone` is false — the synthetic CallArg carries default flags).
            let sig = cx.sigs.get(name);
            let borrow = matches!(
                sig.and_then(|ps| ps.first()),
                Some((AccessConvention::Read, t)) if !t.is_scalar()
            );
            let calls: Vec<TExpr> = items
                .iter()
                .map(|item| {
                    let value = lower_expr(item, cx, env);
                    TExpr {
                        ty: call_return_type(cx, name),
                        kind: TExprKind::Call {
                            name: name.clone(),
                            args: vec![TCallArg {
                                value,
                                borrow,
                                mut_borrow: false,
                                clone: false,
                                arc_clone: false,
                                fn_coerce: None,
                            }],
                        },
                    }
                })
                .collect();
            // S76: result is `[T#N]` (a fixed-size list), erased to `Vec<T>`.
            let elem_ty = call_return_type(cx, name);
            TExpr {
                ty: Type::List(Box::new(elem_ty)),
                kind: TExprKind::FanOut { calls },
            }
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` (S58). The result type is
        // `Ptr<elem>` (`ptr_type`), total from the node's `elem`. The element's Rust type
        // is resolved here (`cx.rust_type`) so emit makes no decision (I3). The cast is
        // safe Rust (no `unsafe`).
        Expr::PtrFromAddr { elem, addr, .. } => {
            let taddr = lower_expr(addr, cx, env);
            TExpr {
                ty: crate::Sema::ptr_type(elem.clone()),
                kind: TExprKind::PtrFromAddr {
                    elem_rust: cx.rust_type(elem),
                    addr: Box::new(taddr),
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

/// c109 Phase 15: the PLAIN Rust field name for a CORE-struct field read, mirroring
/// `core_struct_field_rust_name` (Source/Codegen/Expression.rs) — but keyed on the
/// RESOLVED receiver type (the TIR's total `recv.ty`) instead of `expr_jet_ty(env)`.
/// Returns `Some(plain_name)` for a known core-struct field (so it is emitted
/// unprefixed, B2), `None` otherwise (the caller falls back to `mangle(member)`).
fn core_struct_field_rust_name(recv_ty: &Type, member: &str) -> Option<String> {
    let Type::Named(type_name) = recv_ty else {
        return None;
    };
    let known = match type_name.as_str() {
        "ProcessResult" => matches!(member, "code" | "output" | "errors"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JsonError" => {
            matches!(member, "line" | "message")
        }
        n if n == Syntax::TYPE_UTF8_ERROR || n == "Utf8Error" => member == "message",
        // E2-M10: HttpRequest / HttpResponse field access.
        "HttpRequest" | "HttpResponse" => {
            matches!(member, "method" | "path" | "body" | "headers" | "status")
        }
        _ => false,
    };
    if known {
        Some(member.to_string())
    } else {
        None
    }
}

/// Look up a field's declared type on a resolved struct receiver type. Returns
/// `None` when the receiver is not a known struct or the field is absent — both
/// impossible for a covered function (sema validated the access).
fn struct_field_type(cx: &Cx, recv_ty: &Type, field: &str) -> Option<Type> {
    // c109 Phase 23: a named-tuple field read (`p.x`) — resolve the field's type off
    // the `Type::Tuple` directly (a tuple has no `cx.struct_fields` entry; its struct
    // is the generated `JetTup_<hash>`). Keeps the field read's result type total.
    if let Type::Tuple(fields) = recv_ty {
        return fields
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| (**t).clone());
    }
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
        // c109 Phase 23: a distinct-type constructor `UserId(x)` yields the distinct
        // type itself (it has no `fn_types` entry). Keeps the call's result type total.
        _ if cx.distinct_types.contains_key(name) => Type::Named(name.to_string()),
        _ => unit_type(),
    }
}

/// c109 Phase 6: lower a method call. The gate proved it is the synthetic `.clone()`
/// or a user instance method on a covered type; resolve every dispatch fact here.
fn lower_method_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
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
    // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. The receiver's resolved
    // type names the distinct; its base type (from `cx.distinct_types`) is the total
    // result type. Mirrors `emit_method_call`'s `METHOD_DISTINCT_RAW` early return.
    if method == Syntax::METHOD_DISTINCT_RAW {
        let recv = lower_expr(receiver, cx, env);
        let base = match &recv.ty {
            Type::Named(n) => cx
                .distinct_types
                .get(n)
                .map(|(b, _)| b.clone())
                .unwrap_or_else(unit_type),
            _ => unit_type(),
        };
        return TExpr {
            ty: base,
            kind: TExprKind::DistinctRaw(Box::new(recv)),
        };
    }
    // c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)`. The gate
    // proved `recv_type == Some(<CoveredStruct>)` and the named field is `Type::Fn`. The
    // AST `emit_method_call` (Expression.rs ~L1573) emits `(({recv}).{user_<field>})({args})`
    // with PLAIN args. Resolve the field's Rust name + the call's result type (the Fn's
    // return) here; emit just splices. (Tried before the JSON/core/user shapes, mirroring
    // the AST dispatch order — a fn-field check fires before user-method dispatch.)
    if let Some(Type::Fn { ret, .. }) = fn_field_call_ty(method, recv_type, cx) {
        let ret_ty = ret.as_deref().cloned().unwrap_or_else(unit_type);
        let recv = lower_expr(receiver, cx, env);
        // Args emit PLAINLY (AST `emit_call_args(.., None, ..)` — no convention), but the
        // arg's own `implicit_clone`/`shared_auto_clone` flags still apply: pass `conv:
        // None` to `lower_one_call_arg`, the single `emit_call_args` reproduction.
        let targs: Vec<TCallArg> =
            args.iter().map(|a| lower_one_call_arg(a, None, env, cx)).collect();
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::FnFieldCall {
                recv: Box::new(recv),
                field_rust: mangle(method),
                args: targs,
            },
        };
    }
    // c109 Phase 24: a JSON construction `JSON.<Variant>(arg)` (the gate proved the
    // receiver is the `Ident("JSON")` type name and `method` is a JSON variant). Lower
    // to `TExprKind::JsonLit`, carrying the payload's `implicit_clone` flag as a total
    // fact (reproducing `emit_core_json_lit`). The result type is `JSON`.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_json_type_name(type_name)
            && is_json_variant(method)
        {
            let arg = args.first().map(|a| {
                Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone))
            });
            return TExpr {
                ty: Type::Named(Syntax::TYPE_JSON.to_string()),
                kind: TExprKind::JsonLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    // c109 Phase 19: the arena allocator constructor `mem.Arena.new(…)` (D-ALLOC1). The
    // gate proved the receiver is `Field(Ident(mem-alias), <AllocType>)` + method `new`.
    // Render the whole ctor call HERE (totality), reproducing `emit_method_call`'s arena
    // branch (Expression.rs ~L1515): `jet_mem::Jet<Alloc>::new()` (no arg) or
    // `::with_capacity|with_slots|with_size((arg) as usize)` (one optional arg). The
    // result type is the allocator handle `Named(<AllocType>)` (`alloc_method_return`'s
    // `new` arm). The allocator's only `unsafe` lives in the vetted `jet_mem` prelude (I1).
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        {
            if let Some(alloc_type) = alloc_new_type(receiver, method, cx, &locals) {
                let rust_type = alloc_handle_rust_type(alloc_type).unwrap_or("jet_mem::JetArena");
                let ctor = if args.is_empty() {
                    format!("{}::new()", rust_type)
                } else {
                    let ctor_fn = match alloc_type {
                        "Pool" => "with_slots",
                        "Fixed" => "with_size",
                        _ => "with_capacity",
                    };
                    let a0 = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                    format!("{}::{}({} as usize)", rust_type, ctor_fn, a0)
                };
                return TExpr {
                    ty: Type::Named(alloc_type.to_string()),
                    kind: TExprKind::AllocNew { ctor },
                };
            }
        }
    }
    // c109 Phase 16: an enum-variant CONSTRUCTION `Enum.Variant(args)` reaching codegen
    // as a `MethodCall` (sema never rewrites a payload variant to `Expr::EnumLit`). The
    // AST `emit_method_call` routes it to `emit_enum_lit` with all-positional args; we
    // reproduce that, resolving each arg's `clone`/`boxed` decisions via `lower_enum_arg`
    // (`emit_boxed_enum_arg` byte-for-byte). This is the construction half of the
    // string/struct/collection-payload + recursive (boxed) enum coverage.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name) {
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        let prefix = tir_enum_lit_prefix(cx, type_name, method);
                        let payload = if args.is_empty() {
                            TEnumPayload::Unit
                        } else {
                            let pos = args
                                .iter()
                                .map(|a| {
                                    lower_enum_arg(type_name, method, method, &a.expr, cx, env)
                                })
                                .collect();
                            TEnumPayload::Positional(pos)
                        };
                        return TExpr {
                            ty: Type::Named(type_name.clone()),
                            kind: TExprKind::EnumLit { prefix, payload },
                        };
                    }
                }
            }
        }
    }
    // c109 Phase 10: a core/stdlib module call `alias.method(args)`. The gate proved
    // `recv_type == None` + receiver is a core-import alias + `core_call_covered`.
    // Mirror `emit_core_call` (Source/Codegen/Expression.rs): resolve the module here
    // (total), lower args PLAINLY (no clone/borrow wrappers — `emit_core_call`'s
    // `arg(i)` is a raw `emit_expr`), and carry the return type from the authoritative
    // `core_fixed_sig` table. Tried BEFORE the builtin shape (a core method named
    // `get`/`split`/… must not be claimed by the receiver-keyed builtin op).
    if recv_type.is_none() {
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias) {
                if let Some(module) = cx.core_imports.get(alias).cloned() {
                    // c109 Phase 13: a closure-taking core call (spawn/serve/guard).
                    // The gate proved a literal-lambda closure arg. Each renders its
                    // bespoke shape at lowering (lambda in subset — Phase 11).
                    if let Some(t) = lower_core_closure_call(&module, method, args, cx, env) {
                        return t;
                    }
                    let targs: Vec<TExpr> =
                        args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                    // c109 Phase 18: the `core.mem` pointer ops carry a non-fixed return
                    // type. `address_of` is always `Int`; `volatile_read(p)` reads through
                    // the typed pointer, so its result is `ptr_elem(p.ty)` — the `T` of the
                    // `Ptr<T>` arg, recovered from the LOWERED arg's total `ty` (no emit-time
                    // inference, I3). A defensive `Unit` fallback (an ill-typed arg sema
                    // would already have rejected) keeps the fact total.
                    let ty = if module == "core.mem" {
                        match method {
                            "address_of" => Type::Int,
                            "volatile_read" => targs
                                .first()
                                .and_then(|a| crate::Sema::ptr_elem(&a.ty))
                                .unwrap_or_else(unit_type),
                            _ => core_call_return_ty(&module, method),
                        }
                    } else if crate::Sema::is_polymorphic_core_special(&module, method) {
                        // c109 Phase 20: the polymorphic special's return type is NOT in
                        // `core_fixed_sig` — sema resolved it (arg-type dependent) and wrote
                        // it onto the node's `resolved_ret`. Read it totally (I3); a unit
                        // fallback (eprint/shuffle return nothing) keeps the fact total.
                        resolved_ret.cloned().unwrap_or_else(unit_type)
                    } else {
                        core_call_return_ty(&module, method)
                    };
                    return TExpr {
                        ty,
                        kind: TExprKind::CoreCall {
                            module,
                            method: method.to_string(),
                            args: targs,
                        },
                    };
                }
                // c109 Phase 14: a qualified cross-module call `alias.method(args)`.
                // The gate proved the alias is a re-export / import_mod / code_module.
                // Mirror `emit_method_call`'s arms IN ORDER (reexport, import_mods,
                // code_modules) — resolving the path pieces here so emit decides nothing.
                if let Some((real_mod, real_fn)) =
                    cx.reexport_calls.get(&(alias.clone(), method.to_string())).cloned()
                {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: real_mod,
                                rust_fn: mangle(&real_fn).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if let Some(mod_name) = cx.import_mods.get(alias).cloned() {
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: mod_name,
                                rust_fn: mangle(method).to_string(),
                            },
                            args: targs,
                        },
                    };
                }
                if cx.code_modules.contains(alias.as_str()) {
                    let mangled_key = format!("{}__{}", alias, method);
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    return TExpr {
                        ty: call_return_type(cx, &mangled_key),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled { mangled: mangled_key },
                            args: targs,
                        },
                    };
                }
            }
        }
    }
    // c109 Phase 9: a built-in collection/string method (`emit_builtin_method`). The
    // gate proved `recv_type == None` + a covered builtin name + an in-subset value
    // receiver. Resolve the Map-vs-List-vs-String emit branch HERE from the
    // receiver's type (reproducing `expr_jet_ty`, incl. its `None` partiality), so
    // emit makes no type decision (I3). The result type comes from the builtin's
    // sema return (`Collections::builtin_method_return`) for totality.
    if recv_type.is_none() {
        if let Some(op) = resolve_builtin_op(receiver, method, method_span, args, env, cx) {
            let recv_t = lower_expr(receiver, cx, env);
            let recv_ast_ty = tir_recv_jet_ty(receiver, env);
            let result_ty = builtin_result_ty(method, args.len(), recv_ast_ty.as_ref());
            // Args are emitted plainly (no clone/borrow wrappers), exactly as
            // `emit_builtin_method`'s `arg(i)` = raw `emit_expr`.
            let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            return TExpr {
                ty: result_ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // c109 Phase 19: `Stopwatch.elapsed_millis()` (gate shape d2). The gate proved
    // `recv_type == None` + the `elapsed_millis` name + an in-subset value receiver.
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`), the same node the Phase-13 handle shape uses — emit is
    // byte-identical to `emit_builtin_method`'s name-keyed `elapsed_millis` arm. The
    // result type is `Int` (`stopwatch_method_return`), kept total per the design.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        let recv_t = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::Int,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::StopwatchElapsedMillis,
                args: Vec::new(),
            },
        };
    }
    // c109 Phase 24: `Match.group(n)` (gate shape d4). The gate proved `recv_type ==
    // Some("Match")` + `group`/1 + an in-subset value receiver. Lower to `BuiltinMethod`/
    // `MatchGroup`, byte-for-byte `emit_builtin_method`'s `("Match", "group")` arm. The
    // result type is `String?`. Placed BEFORE the user-instance shape (also `recv_type ==
    // Some`) — `Match` is never a covered user struct/enum, so the two never collide.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        let recv_t = lower_expr(receiver, cx, env);
        let arg0 = lower_expr(&args[0].expr, cx, env);
        return TExpr {
            ty: Type::Option(Box::new(Type::String)),
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(recv_t),
                op: TBuiltinOp::MatchGroup,
                args: vec![arg0],
            },
        };
    }
    // c109 Phase 21: a Task/Channel/Sender concurrency method (gate shape d3). The gate
    // proved `recv_type == None` + a disjoint concurrency name+arity. Resolve the op +
    // result type HERE (totality). The result type comes from `Collections::
    // builtin_method_return`'s `Type::Apply` arms (Source/Collections.rs), read off the
    // receiver's already-resolved type `Task<T>`/`Channel<T>`/`Sender<T>` (the LOWERED
    // receiver's `.ty`, total from the binding's annotated/inferred slot — never
    // re-inferred in emit, I3): `join` → `T`; `detach`/`send` → Unit; `receive` →
    // `Result<T, Closed>`; `sender` → `Sender<T>`. Args lowered PLAINLY (the AST
    // `emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        // The element type `T` from the receiver's `Apply<T>` (the first type arg).
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let elem = elem.unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "join" => (THandleOp::TaskJoin, elem),
            "detach" => (THandleOp::TaskDetach, unit_type()),
            "receive" => (
                THandleOp::ChannelReceive,
                Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named("Closed".to_string())),
                },
            ),
            "sender" => (
                THandleOp::ChannelSender,
                Type::Apply {
                    name: "Sender".to_string(),
                    args: vec![elem],
                },
            ),
            "send" => (THandleOp::SenderSend, unit_type()),
            _ => unreachable!("is_concurrency_method_name admitted only these names"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/…).
    // The gate proved `recv_type == None` + a closure-method name + a literal lambda
    // arg. Resolve the receiver-type + Fn-vs-FnMut dispatch HERE into a total
    // `TClosureOp` (reproducing `emit_builtin_method`'s closure arms, incl. its
    // `expr_jet_ty(receiver)` Map/trait-object branches), so emit makes no decision.
    if recv_type.is_none() && crate::Collections::is_closure_method(method) {
        let op = resolve_closure_op(receiver, method, args, env, cx);
        let recv_t = lower_expr(receiver, cx, env);
        let recv_ast_ty = tir_recv_jet_ty(receiver, env);
        let result_ty = builtin_result_ty(method, args.len(), recv_ast_ty.as_ref());
        // Args lowered PLAINLY (the lambda + any seed) — `emit_builtin_method`'s
        // `arg(i)` is a raw `emit_expr`, no clone/borrow wrappers.
        let targs = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::ClosureMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method
    // (`is_nan`/`count_ones`/`to_i32`/…). The gate proved `recv_type ==
    // Some(<numeric name>)` + a covered nullary numeric op. Resolve the receiver
    // width source/target + the widening-vs-narrowing branch HERE (reproducing
    // `numeric_conversion`/`conv_rust_target` from Expression.rs) into a total
    // `TNumericOp`, so emit makes no decision (I3). The result type comes from
    // `numeric_method_return` (the sema table), keyed on the receiver type recovered
    // from `recv_type` (the total width source — `src = recv_type.or_else(rty.name())`
    // on the AST side, where `recv_type` is always `Some` for these).
    if let Some(numeric_name) = recv_type {
        if let Some(recv_ty) = crate::AST::numeric_type_from_name(numeric_name) {
            if let Some(op) = resolve_numeric_op(method, numeric_name) {
                let recv_t = lower_expr(receiver, cx, env);
                let result_ty = builtin_result_ty(method, args.len(), Some(&recv_ty));
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(recv_t),
                        op,
                    },
                };
            }
        }
    }
    // c109 Phase 25: HttpRouter route registration `router.get/post/put/delete(path,
    // handler)` (D-ROUTE1=A). The gate (`router_register_in_subset`) proved the receiver
    // + path in-subset and the handler a named-fn/lambda. Render the handler closure HERE
    // (the `emit_router_handler` reproduction); emit assembles the register call. Result
    // is Unit (the registration is a statement effect).
    if recv_type.as_deref() == Some("HttpRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        let verb = match method {
            "get" => "GET",
            "post" => "POST",
            "put" => "PUT",
            "delete" => "DELETE",
            _ => unreachable!(),
        };
        let recv_t = lower_expr(receiver, cx, env);
        let path_t = lower_expr(&args[0].expr, cx, env);
        let handler = render_router_handler(args, cx, env);
        return TExpr {
            ty: unit_type(),
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::HttpRouterRegister { verb, handler },
                args: vec![path_t],
            },
        };
    }
    // c109 Phase 13: a method ON a handle. The gate proved `recv_type ==
    // Some(<handle>)` + a covered handle op. Resolve the handle-receiver branch HERE
    // into a total `THandleOp` (reproducing the handle arms of `emit_builtin_method`),
    // so emit makes no type decision (I3). Args lowered PLAINLY (`arg(i)` = raw
    // `emit_expr`). The return type is the total sema handle-table fact.
    if let Some(handle) = recv_type {
        if let Some(op) = handle_method_op(handle, method, args.len()) {
            let recv_t = lower_expr(receiver, cx, env);
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            // c109 Phase 19: an arena `alloc(v)` returns a `&mut T` view whose VALUE type is
            // the arg's type (sema's `alloc_method_return` returns a `__alloc_infer__`
            // sentinel, resolved from the arg). The result `ty` is rarely load-bearing (an
            // `arena_view` binding emits no type annotation), but kept total per the design —
            // recovered from the LOWERED arg's total `ty`, never re-inferred (I3).
            let ty = match op {
                THandleOp::AllocAlloc => targs
                    .first()
                    .map(|a| a.ty.clone())
                    .unwrap_or_else(unit_type),
                THandleOp::AllocReset | THandleOp::AllocFree => unit_type(),
                _ => handle_method_return_ty(handle, method, args.len()),
            };
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
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
    // c109 Phase 30: DYNAMIC dispatch on a TRAIT-OBJECT receiver (`s.name()`/`s.area()`,
    // `s: Box<dyn user_Shape>`). The gate proved `recv_type == Some(<trait>)` with the
    // trait in `cx.trait_names`. The AST `emit_method_call` (Expression.rs ~L1657) emits
    // `({recv}).{method}({args})` — the BARE (unmangled) method name (vtable dispatch),
    // args lowered PLAINLY (`emit_call_args(.., None, ..)` — no sig). Reuse the `MethodCall`
    // node (`({recv}).{method_rust}({args})`) with the bare method name. The result type is
    // NOT load-bearing here (the AST carries no sig/return for a trait-object call, and a
    // trait-method return isn't registered in `cx.method_rets` by trait name) — it is read
    // only where the call result feeds a type-driven decision, which the sole reachable
    // program (`print("{s.name()}: {s.area()}")` — string interpolation calls `.jet_show()`,
    // type-agnostic) never does. Carry `unit_type`, exactly as the AST has no return fact.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            let recv = lower_expr(receiver, cx, env);
            // Plain args (conv None), reproducing `emit_call_args(cx, None, args, env)`.
            let targs: Vec<TCallArg> =
                args.iter().map(|a| lower_one_call_arg(a, None, env, cx)).collect();
            return TExpr {
                ty: unit_type(),
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv),
                    method_rust: method.to_string(),
                    args: targs,
                },
            };
        }
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

/// c109 Phase 13: lower a closure-taking core call (`tasks.spawn`/`http.serve`/
/// `scope.guard`) into a bespoke `CoreClosureCall` node, reproducing `emit_core_call`
/// (Source/Codegen/Expression.rs) byte-for-byte. Returns `None` when `(module,
/// method)` isn't one of the three (so the caller falls through to the plain
/// `CoreCall`). The gate (`core_closure_call_in_subset`) already proved a literal
/// in-subset lambda in the closure-arg position.
fn lower_core_closure_call(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let lam_at = |i: usize| match args.get(i).map(|a| &a.expr) {
        Some(Expr::Lambda(lam)) => Some(lam),
        _ => None,
    };
    let kind = match (module, method) {
        ("core.tasks", "spawn") => {
            let lam = lam_at(0)?;
            // The spawned body's type (the lambda's return) is the Task's element type.
            let body_ty = lambda_body_ty(lam, cx, env);
            let spawn_closure = render_spawn_lambda(lam, cx, env);
            return Some(TExpr {
                ty: core_closure_call_return_ty(module, method, body_ty),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn { spawn_closure },
                },
            });
        }
        ("jet.http", "serve") => {
            let lam = lam_at(1)?;
            let addr = lower_expr(&args[0].expr, cx, env);
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::Serve {
                addr: Box::new(addr),
                closure,
            }
        }
        ("core.scope", "guard") => {
            let lam = lam_at(0)?;
            let closure = render_lambda_str(lam, cx, env);
            TCoreClosureKind::Guard { closure }
        }
        _ => return None,
    };
    Some(TExpr {
        ty: core_closure_call_return_ty(module, method, unit_type()),
        kind: TExprKind::CoreClosureCall { kind },
    })
}

/// c109 Phase 13: the type of a lambda's body (its return), used for a `spawn`ed
/// closure's `Task<T>` element type. An expression body's type is the lowered expr's
/// `ty`; a block body's type is rarely load-bearing in the subset (the Task type is
/// not read by emit), so a `Unit` placeholder is fine for a block.
fn lambda_body_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    match &lam.body {
        LambdaBody::Expr(e) => {
            let mut lam_env = clone_env(env);
            for p in &lam.params {
                lam_env
                    .locals
                    .insert(p.name.clone(), (mangle(&p.name), p.ty.clone()));
            }
            lower_expr(e, cx, &mut lam_env).ty
        }
        LambdaBody::Block(_) => unit_type(),
    }
}

/// c109 Phase 6/13: lower method-call arguments, mirroring `emit_call_args`
/// (Source/Codegen/Expression.rs). The clone/Arc wrappers, the borrow/mut-borrow
/// wrappers, and the Fn-typed Box-coercion are all decided here from total facts
/// (`CallArg.flags` + the resolved param convention/type), never re-derived in emit.
fn lower_method_args(
    args: &[crate::AST::CallArg],
    sig: &[(AccessConvention, Type)],
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.get(i).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 13: lower ONE call argument, reproducing `emit_call_args`
/// (Source/Codegen/Expression.rs) byte-for-byte — the single source of truth for
/// the clone/Arc, Fn-coercion, and borrow/mut-borrow wrapper order. `conv` is the
/// resolved param `(convention, type)` for this position (`None` when the callee has
/// no known signature, e.g. a `CallValue`). The emit order is exactly the AST path's:
///   1. the implicit-clone / Arc-clone wrapper (`(…).clone()` / `Arc::clone(&…)`);
///   2. the Fn-typed Box-coercion (`Box::new(…) as <fn-type>`, or just ` as <fn-type>`
///      when already boxed);
///   3. the borrow wrapper (`&(…)` for a `Read` non-scalar non-Fn, `&mut (…)` for a
///      `Mutate`).
fn lower_one_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TCallArg {
    let value = lower_expr(&a.expr, cx, env);
    let clone = a.flags.implicit_clone;
    let arc_clone = a.flags.shared_auto_clone;
    // The Fn-typed Box-coercion (`emit_call_args`' `if let Some((_, Type::Fn …))`).
    let fn_coerce = match &conv {
        Some((_, Type::Fn { .. })) => {
            // `already_boxed`: the value already produces a `Box::new(…)`. The AST
            // checks two cases — the emitted string starts with `Box::new(` (only a
            // bare fn-name value does, in subset — `emit_named_fn_value`), OR the
            // value is a fn-typed local ident. Resolve both at lowering.
            let already_boxed = ast_arg_is_named_fn_value(&a.expr, cx, env)
                || matches!(
                    &a.expr,
                    Expr::Ident(name, _)
                        if env.ty_of(name).is_some_and(|t| matches!(t, Type::Fn { .. }))
                );
            let (_, ty) = conv.as_ref().expect("matched Some above");
            Some(TFnCoerce {
                fn_type_rust: cx.rust_type(ty),
                already_boxed,
            })
        }
        _ => None,
    };
    // Borrow wrappers (applied after the clone + fn-coerce wrappers). A `Read`
    // non-scalar (non-Fn) is `&(…)`; a `Mutate` is `&mut (…)`. A Fn-typed `Read` is
    // NOT borrowed (the AST `match conv` skips it), so the fn-coerce form stands alone.
    let (borrow, mut_borrow) = match &conv {
        Some((AccessConvention::Read, t)) if !t.is_scalar() && !matches!(t, Type::Fn { .. }) => {
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
        fn_coerce,
    }
}

/// c109 Phase 14: lower a cross-module call's arguments against the callee's import
/// signature, reproducing `emit_call_args`. Each arg's borrow/clone/fn-coercion is
/// resolved from the sig param convention (the same `lower_one_call_arg` used by the
/// plain-call path).
fn lower_module_args(
    args: &[crate::AST::CallArg],
    sig: Option<&[(AccessConvention, Type)]>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 14: lower one FFI extern-call argument, reproducing
/// `emit_extern_call_args` (Source/Codegen/Expression.rs). The value is wrapped in
/// `(…).clone()` when the arg carries `implicit_clone`, OR when its param is a
/// non-scalar `Read`-convention type and `implicit_clone` is NOT already set (the AST
/// `if a.flags.implicit_clone { … } else if … } if let Some((_, ty)) = sig … if
/// !ty.is_scalar() && !implicit_clone`). The Arc (`shared_auto_clone`) form is excluded
/// from the subset, so it never reaches here.
fn lower_extern_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExternArg {
    let value = lower_expr(&a.expr, cx, env);
    let non_scalar_param = conv
        .as_ref()
        .map(|(_, ty)| !ty.is_scalar())
        .unwrap_or(false);
    // `(…).clone()` is emitted once: either the explicit implicit_clone flag, or the
    // non-scalar-param clone (when implicit_clone is false). The two never stack — the
    // AST applies the param clone only `&& !a.flags.implicit_clone`.
    let clone = a.flags.implicit_clone || (non_scalar_param && !a.flags.implicit_clone);
    TExternArg { value, clone }
}

/// c109 Phase 13: does this AST arg expression emit as a `Box::new(…)` (a bare
/// fn-name value via `emit_named_fn_value`)? That is exactly an `Expr::Ident` which
/// is NOT a local and resolves to a `Type::Fn` in `cx.fn_types` (a top-level fn used
/// as a value). Mirrors `emit_expr`'s `Expr::Ident` arm + `emit_call_args`'
/// `s.starts_with("Box::new(")` check, resolved at lowering.
fn ast_arg_is_named_fn_value(e: &Expr, cx: &Cx, env: &LowerEnv) -> bool {
    if let Expr::Ident(name, _) = e {
        if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
            return matches!(cx.fn_types.get(name), Some(Type::Fn { .. }));
        }
    }
    false
}

/// c109 Phase 9: reproduce codegen's `expr_jet_ty(receiver, env)`
/// (Source/Codegen/Expression.rs) for a built-in method receiver, using the TIR
/// lowering env's slot types. This MUST match `expr_jet_ty` bit-for-bit (incl. its
/// `None` results) because the Map-vs-List-vs-String emit branch in
/// `emit_builtin_method` is keyed on it: a divergence here flips a branch and breaks
/// byte-parity. Only `Ident` (via its slot type), `Str`/`Char`, and chained
/// `chars`/`split`/other method calls resolve; everything else (notably a struct
/// `Field` read) is `None` — exactly as `expr_jet_ty` does, so a `None`-typed
/// receiver lands on the AST's default branch (the list/else arm).
fn tir_recv_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::TupleLit(_, _, Some(ty)) => Some(ty.clone()),
        Expr::MethodCall { receiver, method, .. } => {
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            if method == "split" {
                return Some(Type::List(Box::new(Type::String)));
            }
            tir_recv_jet_ty(receiver, env)
        }
        _ => None,
    }
}

/// c109 Phase 9: resolve the built-in method op from the method name, arg count, and
/// the receiver's resolved type — reproducing `emit_builtin_method`'s name+`rty`
/// dispatch (Source/Codegen/Expression.rs) exactly. The Map-vs-List branch
/// (`insert`/`remove`/`get`) and the String-vs-list branch (`len`) come from
/// `tir_recv_jet_ty` (matching the AST's `rty`); a `None` or non-Map/non-String
/// receiver falls to the list/else branch, byte-for-byte the AST default. Returns
/// `None` for any name/shape the TIR does not lower (the caller stays on the AST
/// path — the gate already excluded these, so this is a defensive belt).
fn resolve_builtin_op(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    env: &LowerEnv,
    cx: &Cx,
) -> Option<TBuiltinOp> {
    if crate::Collections::is_closure_method(method) {
        return None;
    }
    let rty = tir_recv_jet_ty(receiver, env);
    let is_string = matches!(rty, Some(Type::String));
    let is_map = matches!(rty, Some(Type::Map { .. }));
    Some(match (method, args.len()) {
        ("len", 0) => {
            if is_string {
                TBuiltinOp::LenString
            } else {
                TBuiltinOp::LenList
            }
        }
        ("is_empty", 0) => TBuiltinOp::IsEmpty,
        ("push", 1) => TBuiltinOp::Push,
        ("pop", 0) => TBuiltinOp::Pop,
        ("insert", 2) => {
            if is_map {
                TBuiltinOp::InsertMap
            } else {
                TBuiltinOp::InsertList
            }
        }
        ("remove", 1) => {
            if is_map {
                TBuiltinOp::RemoveMap
            } else {
                // The list form embeds the *method-span* line for its bounds panic,
                // exactly as `emit_builtin_method` reads `span_line_col(method_span.start)`.
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                TBuiltinOp::RemoveList { line }
            }
        }
        ("get", 1) => {
            if is_map {
                TBuiltinOp::GetMap
            } else {
                TBuiltinOp::GetList
            }
        }
        ("first", 0) => TBuiltinOp::First,
        ("last", 0) => TBuiltinOp::Last,
        ("contains", 1) => TBuiltinOp::Contains,
        ("index_of", 1) => TBuiltinOp::IndexOf,
        ("reverse", 0) => TBuiltinOp::Reverse,
        ("sort", 0) => TBuiltinOp::Sort,
        ("join", 1) => TBuiltinOp::JoinSep,
        ("clear", 0) => TBuiltinOp::Clear,
        ("chars", 0) => TBuiltinOp::Chars,
        ("bytes", 0) => TBuiltinOp::Bytes,
        ("trim", 0) => TBuiltinOp::Trim,
        ("split", 1) => TBuiltinOp::Split,
        ("starts_with", 1) => TBuiltinOp::StartsWith,
        ("ends_with", 1) => TBuiltinOp::EndsWith,
        ("replace", 2) => TBuiltinOp::Replace,
        ("to_upper", 0) => TBuiltinOp::ToUpper,
        ("to_lower", 0) => TBuiltinOp::ToLower,
        ("repeat", 1) => TBuiltinOp::Repeat,
        ("slice", 2) => {
            // The string-slice form embeds the *receiver-span* line for its bounds panic.
            let line = crate::Diagnostics::span_line_col(&cx.src, receiver.span().start).0;
            TBuiltinOp::Slice { line }
        }
        ("keys", 0) => TBuiltinOp::Keys,
        ("values", 0) => TBuiltinOp::Values,
        ("contains_key", 1) => TBuiltinOp::ContainsKey,
        ("to_string", 0) => TBuiltinOp::ToString,
        _ => return None,
    })
}

/// c109 Phase 9: the resolved return type of a built-in collection/string method,
/// from `Collections::builtin_method_return` (the sema table). Kept total per the
/// design principle; rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but resolved here so the TIR never guesses. Falls back to `Unit` for a void
/// method or an unresolved receiver type (impossible for a covered call — sema
/// validated it).
fn builtin_result_ty(method: &str, nargs: usize, recv_ty: Option<&Type>) -> Type {
    match recv_ty.and_then(|rt| crate::Collections::builtin_method_return(rt, method, nargs, false)) {
        Some(Some(t)) => t,
        _ => unit_type(),
    }
}

/// c109 Phase 11: resolve a closure-taking collection method into a total
/// `TClosureOp`, reproducing `emit_builtin_method`'s closure arms
/// (Source/Codegen/Expression.rs) exactly. The receiver-type branch
/// (`rty = expr_jet_ty(receiver)`) picks Map (`EachMap`) vs trait-object list
/// (`EachRef`) vs plain list; the Fn-vs-FnMut branch reads the lambda arg's
/// `needs_fn_mut` meta. All decisions made HERE, never in emit (I3). The gate
/// proved a literal lambda arg, so `needs_fn_mut` is always readable; a non-lambda
/// arg defaults to the non-mut form, matching the AST `else` branch.
fn resolve_closure_op(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    env: &LowerEnv,
    cx: &Cx,
) -> TClosureOp {
    let rty = tir_recv_jet_ty(receiver, env);
    // The lambda arg's FnMut fact (the AST checks `args[0]` for map/each).
    let fn_mut = matches!(args.first().map(|a| &a.expr), Some(Expr::Lambda(l)) if l.meta.needs_fn_mut);
    match method {
        "map" => {
            if fn_mut {
                TClosureOp::MapMut
            } else {
                TClosureOp::Map
            }
        }
        "filter" => TClosureOp::Filter,
        "each" => {
            // The AST: `match rty { Map => jet_map_each, _ => list_each }`, where
            // `list_each` checks trait-object-list FIRST, then lambda FnMut.
            match &rty {
                Some(Type::Map { .. }) => TClosureOp::EachMap,
                Some(Type::List(inner)) if list_carries_trait(cx, inner) => TClosureOp::EachRef,
                _ if fn_mut => TClosureOp::EachMut,
                _ => TClosureOp::Each,
            }
        }
        "find" => TClosureOp::Find,
        "any" => TClosureOp::Any,
        "all" => TClosureOp::All,
        "sort_by" => TClosureOp::SortBy,
        "reduce" => TClosureOp::Reduce,
        // The gate (`is_closure_method`) admits only the names above.
        _ => unreachable!("non-closure method in resolve_closure_op (gate)"),
    }
}

/// c109 Phase 11: TIR-local reproduction of codegen's `list_carries_trait`
/// (Source/Codegen/Expression.rs) — a list element type that is a trait object or a
/// named trait. Used by the `each`-on-trait-object-list emit branch (`jet_list_each_ref`).
/// In the covered collection subset a trait-object element type is excluded, so this
/// is always false for a covered receiver; reproduced for exactness regardless.
fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
}

/// c109 Phase 11: lower a lambda/closure literal (`Expr::Lambda`) to a `TLambda`,
/// reproducing `emit_lambda` (Source/Codegen/Expression.rs) byte-for-byte. Every
/// capture/escape/Fn-vs-FnMut decision is the TOTAL `Lambda.meta` fact — no capture
/// analysis here. The body is lowered on a CLONED env extended with: the cloned
/// captures (rebound to `_jet_cap_<n>`, place = that name, type `None` — matching the
/// AST slot) and the params (place = mangled name, type from the annotation). The
/// rendered closure body string is produced now so emit is a pure wrapper.
fn lower_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TLambda {
    // `emit_lambda` clones the env (`lam_env = env.clone()`), so a `??` panic inside the
    // lambda body dumps the lambda's env (outer locals + captures + params) and does NOT
    // leak into the enclosing fn — a NON-leaky boundary, so fork the panic replica.
    let mut lam_env = fork_panic(env);
    // The clone-capture prelude: `let _jet_cap_<n> = (<outer place>).clone();`. The
    // outer place comes from the *outer* env (the capture is an outer local). The cap
    // rebinds the name with place `_jet_cap_<n>`, no deref, type `None` (matching the
    // AST slot `{ rust_name: cap, deref: false, jet_ty: None }`).
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        lam_env.bind(name, cap, None);
    }
    // Params bind as `mangle(name)` (no deref), typed from the annotation (or `None`).
    for p in &lam.params {
        lam_env.bind(&p.name, mangle(&p.name), p.ty.clone());
    }
    // The rendered param list: `name[: ty]`, exactly as `emit_lambda`.
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .as_ref()
                .map(|t| format!(": {}", cx.rust_type(t)))
                .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    // The body: an expression body lowers + emits directly; a block body lowers its
    // statements (on the lambda env) and emits a `{ … }` at indent 1 — byte-for-byte
    // `emit_lambda`'s `emit_stmts(…, 1, false)` then `format!("{{ {} }}", inner)`.
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_stmts(&lowered, cx, &mut inner, 1);
            format!("{{ {} }}", inner)
        }
    };
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    TLambda {
        prep,
        params,
        body,
        is_move,
        boxed: lam.meta.escapes,
    }
}

/// c109 Phase 13: render a `tasks.spawn` lambda, reproducing `emit_spawn_lambda`
/// (Source/Codegen/Expression.rs) byte-for-byte. It is `emit_lambda` minus the
/// Fn-vs-FnMut and escape logic: ALWAYS `move`, NEVER `Box::new`. The clone-capture
/// prelude is identical. Returns the full rendered closure string (wrapped in
/// `{ <prep> <closure> }` when there are cloned captures).
fn render_spawn_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut lam_env = fork_panic(env);
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!("let {} = ({}).clone();\n    ", cap, env.place_of(name)));
        lam_env.bind(name, cap, None);
    }
    for p in &lam.params {
        lam_env.bind(&p.name, mangle(&p.name), p.ty.clone());
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .as_ref()
                .map(|t| format!(": {}", cx.rust_type(t)))
                .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_stmts(&lowered, cx, &mut inner, 1);
            format!("{{ {} }}", inner)
        }
    };
    let closure = format!("move |{}| {}", params.join(", "), body);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

/// c109 Phase 13: render a lambda via the plain `emit_lambda` form (used by
/// `http.serve`'s lambda handler and `scope.guard`). Returns the full closure string.
fn render_lambda_str(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let tl = lower_lambda(lam, cx, env);
    let move_kw = if tl.is_move { "move " } else { "" };
    let closure = format!("{}|{}| {}", move_kw, tl.params.join(", "), tl.body);
    let wrapped = if tl.boxed {
        format!("Box::new({})", closure)
    } else {
        closure
    };
    if tl.prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", tl.prep, wrapped)
    }
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
        TFuncKind::TraitMethod { is_unsafe, self_conv } => {
            emit_tir_trait_method(tir, *is_unsafe, *self_conv, cx, out)
        }
        TFuncKind::Delegation { sig, fwd, has_return } => {
            emit_tir_delegation(tir, sig, fwd, *has_return, cx, out)
        }
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }` (or `fn main`).
/// Byte-identical to `emit_func`'s output.
fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let ret_clause = match &tir.ret {
        Some(t) => format!(" -> {}", rust_return_type(cx, t, tir.is_view)),
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
    // c109 Phase 18: an `#Unsafe fn` lowers to `unsafe fn` — the prefix sits right after
    // `vis`, exactly as `emit_func` (`{vis}{unsafe_kw}fn …`). I1: emitted ONLY when the
    // source was `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{vis}{unsafe_kw}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        gen = tir.generics,
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
        Some(t) => format!(" -> {}", rust_return_type(cx, t, tir.is_view)),
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
    // c109 Phase 18: an `#Unsafe fn` inherent method lowers to `pub unsafe fn` — the
    // prefix sits between `pub ` and `fn`, exactly as `emit_method` (`pub {unsafe_kw}fn`).
    // I1: emitted ONLY for a source `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}pub {unsafe_kw}fn {name}({params}){ret} {{\n",
        name = mangle(&tir.name),
        params = params.join(", "),
        ret = ret_clause,
    ));
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 12: a trait-impl method, emitted INSIDE an `impl Trait for user_<T> { … }`
/// block (the caller `emit_trait_impl`/`emit_external_trait_impl` opened it).
/// Byte-identical to `emit_trait_method` (Source/Codegen/Items.rs): a BARE method name
/// (no `user_` mangle — the trait owns it), NO `pub`, an always-`&self` receiver, and
/// an `unsafe ` prefix iff the source was an `@unsafe fn`.
fn emit_tir_trait_method(tir: &TFunc, is_unsafe: bool, self_conv: AccessConvention, cx: &Cx, out: &mut String) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let ret_clause = match &tir.ret {
        // `emit_trait_method` computes `ret = rust_return_type(...)` then, if non-empty,
        // ` -> ret`. A unit return yields the empty clause.
        Some(t) => {
            let ret = rust_return_type(cx, t, tir.is_view);
            if ret.is_empty() {
                String::new()
            } else {
                format!(" -> {}", ret)
            }
        }
        None => String::new(),
    };
    // D-MUTSELF1: the receiver honors the source convention — `&self` / `&mut self` /
    // `self` — matching `emit_trait_method` and the trait declaration (emit_trait_def).
    let self_recv = match self_conv {
        AccessConvention::Read => "&self",
        AccessConvention::Mutate => "&mut self",
        AccessConvention::Move => "self",
    };
    let mut params: Vec<String> = vec![self_recv.to_string()];
    for (rust_name, ty, conv) in &tir.params {
        params.push(format!("{}: {}", rust_name, rust_param_type(cx, *conv, ty)));
    }
    let unsafe_kw = if is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}{unsafe_kw}fn {name}({params}){ret} {{\n",
        name = tir.name,
        params = params.join(", "),
        ret = ret_clause,
    ));
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 15: a DELEGATION trait method (`using field`), emitted INSIDE the
/// `impl Trait for user_<T> { … }` block `emit_external_trait_impl` opened. Byte-for-byte
/// `emit_delegation_method` (Source/Codegen/Items.rs): the pre-rendered signature line,
/// then the single forwarding call (`(self).<field>.<method>(args)`) at 8-space indent —
/// with a trailing `;` for a unit method, none for a returning one — then `    }`.
fn emit_tir_delegation(tir: &TFunc, sig: &str, fwd: &str, has_return: bool, cx: &Cx, out: &mut String) {
    // E2-M12 D-OBS1: track the current function name (parity with the AST path, though a
    // delegation body has no panic site of its own).
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(sig);
    if has_return {
        out.push_str(&format!("        {}\n", fwd));
    } else {
        out.push_str(&format!("        {};\n", fwd));
    }
    out.push_str("    }\n");
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
            kw,
            ty_clause,
            init,
        } => {
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
        // c109 Phase 23: tuple destructure. Mirrors `emit_stmt`'s `BindPattern::Tuple`
        // arm byte-for-byte: borrow the init into a temp, then bind each element from a
        // cloned canonical field of it.
        TStmt::TupleDestructure { tmp, init, kw, binds } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
            for (elem_rust, field_rust) in binds {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, elem_rust, tmp, field_rust
                ));
            }
        }
        // c109: struct destructure. Mirrors `emit_stmt`'s `BindPattern::Struct` arm
        // byte-for-byte: borrow the init into a temp, then bind each field from a
        // cloned field of it (the field name is both the local and the `.field` read).
        TStmt::StructDestructure { tmp, init, kw, fields } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
            for field_rust in fields {
                out.push_str(&format!(
                    "{}{} {} = ({}).{}.clone();\n",
                    pad, kw, field_rust, tmp, field_rust
                ));
            }
        }
        // c109 Phase 26: list destructure. Mirrors `emit_stmt`'s `BindPattern::List`
        // arm byte-for-byte: borrow the init into a temp, then bind each element via
        // the runtime bounds-checked `jet_unpack_vec(tmp, want, i, file, line)` move.
        // `{file:?}` reproduces the AST's debug-formatted path; `kw`/`want`/`line` were
        // all resolved at lowering.
        TStmt::ListDestructure { tmp, init, kw, want, file, line, elems } => {
            out.push_str(&format!("{}let {} = &({});\n", pad, tmp, emit_tir_expr(init, cx)));
            for (i, elem_rust) in elems.iter().enumerate() {
                out.push_str(&format!(
                    "{}{} {} = jet_unpack_vec({}, {}, {}, {:?}, {});\n",
                    pad, kw, elem_rust, tmp, want, i, file, line
                ));
            }
        }
        TStmt::Return(Some(e)) => {
            out.push_str(&format!("{}return {};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::Return(None) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        TStmt::ViewReturn { value, wrap } => {
            let v = emit_tir_expr(value, cx);
            let rendered = match wrap {
                ViewWrap::Addr => format!("&{}", v),
                ViewWrap::Bare => v,
            };
            out.push_str(&format!("{}return {};\n", pad, rendered));
        }
        TStmt::ExprStmt(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_tir_expr(e, cx)));
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
            else_is_elseif,
        } => {
            // c109 Phase 22: render the head per the condition form, byte-for-byte
            // `emit_if` (Source/Codegen/Statement.rs).
            match cond {
                TIfCond::Plain(c) => {
                    out.push_str(&format!("{}if {} {{\n", pad, emit_tir_expr(c, cx)));
                }
                TIfCond::IfLet { pat_str, subj } => {
                    out.push_str(&format!(
                        "{}if let {} = {} {{\n",
                        pad,
                        pat_str,
                        emit_tir_expr(subj, cx)
                    ));
                }
                TIfCond::IsNone { subj } => {
                    out.push_str(&format!(
                        "{}if {}.is_none() {{\n",
                        pad,
                        emit_tir_expr(subj, cx)
                    ));
                }
            }
            emit_tir_stmts(then_body, cx, out, indent + 1);
            match else_body {
                None => out.push_str(&format!("{}}}\n", pad)),
                Some(body) => {
                    // Match the AST path EXACTLY: it renders `} else if …` ONLY for a
                    // real `else if` chain (`ElseBranch::ElseIf` → `else_is_elseif`), and
                    // `} else { … }` for an explicit `else` block — even when the block
                    // holds a single `if` (do NOT flatten that, or parity drifts).
                    if *else_is_elseif {
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
            method_kind,
            body,
        } => {
            let lbl = tir_label_prefix(label);
            // c109 Phase 22: a method-call collection takes a distinct `emit_for_in`
            // branch (`collection_str` holds the RECEIVER for chars/lines). Only the
            // stdin form opens an extra block that needs an extra closing brace.
            let mut needs_extra_close = false;
            match method_kind {
                Some(TForInMethod::Chars) => {
                    out.push_str(&format!(
                        "{}{}for _jet_c in ({recv}).chars() {{\n    {}let {} = _jet_c;\n",
                        pad,
                        lbl,
                        pad,
                        mangle(var),
                        recv = collection_str
                    ));
                }
                Some(TForInMethod::LinesFile) => {
                    out.push_str(&format!(
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut ({}).inner) {{\n",
                        pad, lbl, collection_str
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_raw_line.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &_e.to_string()));\n",
                        pad,
                        mangle(var),
                        cx.root_prefix,
                        cx.file,
                        0
                    ));
                }
                Some(TForInMethod::LinesStdin) => {
                    out.push_str(&format!("{}{{ let mut _jet_stdin_h = {};\n", pad, collection_str));
                    needs_extra_close = true;
                    out.push_str(&format!(
                        "{}{}for _jet_raw_line in std::io::BufRead::lines(&mut _jet_stdin_h.inner) {{\n",
                        pad, lbl
                    ));
                    out.push_str(&format!(
                        "{}    let {} = _jet_raw_line.unwrap_or_else(|_e| {}jet_panic({:?}, {}, &_e.to_string()));\n",
                        pad,
                        mangle(var),
                        cx.root_prefix,
                        cx.file,
                        0
                    ));
                }
                None => match var2 {
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
                },
            }
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
            // D-STDIN1=A: close the outer block holding the JetStdinReader local.
            if needs_extra_close {
                out.push_str(&format!("{}}}\n", pad));
            }
        }
        // c109 Phase 15: a resolved comptime-if — emit ONLY the selected branch's
        // statements INLINE at the SAME indent, with no wrapper (no `if`, no block),
        // exactly as the AST `emit_stmts` does for `Stmt::ComptimeIf`.
        TStmt::Inline(stmts) => {
            emit_tir_stmts(stmts, cx, out, indent);
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region — `unsafe { … }`, byte-for-byte
        // `emit_stmts`'s `Stmt::Unsafe` arm (the `#Audit` annotation emits nothing). I1:
        // emitted ONLY for a source `#Unsafe` gate.
        TStmt::Unsafe(body) => {
            out.push_str(&format!("{}unsafe {{\n", pad));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 19: an explicit `region r { … }` — a plain Rust block, byte-for-byte
        // `emit_stmts`'s `Stmt::Region` arm. The escape/RAII rules are sema's job (I3).
        TStmt::Region(body) => {
            out.push_str(&format!("{}{{\n", pad));
            emit_tir_stmts(body, cx, out, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 19: a `#Context(field: value) { … }` block — a plain block with one
        // RAII guard per field (declaration order) BEFORE the body, byte-for-byte
        // `emit_stmts`'s `Stmt::ContextBlock` arm.
        TStmt::ContextBlock { guards, body } => {
            out.push_str(&format!("{}{{\n", pad));
            let inner = indent + 1;
            let inner_pad = "    ".repeat(inner);
            for (i, (is_alloc, value)) in guards.iter().enumerate() {
                let val = emit_tir_expr(value, cx);
                if *is_alloc {
                    out.push_str(&format!(
                        "{}let _ctx_guard_{} = jet_mem::jet_ctx_push_alloc(&{});\n",
                        inner_pad, i, val
                    ));
                } else {
                    out.push_str(&format!(
                        "{}let _ctx_logger_{} = {};\n",
                        inner_pad, i, val
                    ));
                }
            }
            emit_tir_stmts(body, cx, out, inner);
            out.push_str(&format!("{}}}\n", pad));
        }
        // c109 Phase 15: a mixed comparison/Bool switch — the general `emit_mixed_switch`
        // (Statement.rs) `if/else if … else` chain inside a block that binds
        // `_jet_switch_subject = &(subject)` (emitted for parity even when unused).
        TStmt::MixedSwitch {
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
            for (i, (cond, body)) in arms.iter().enumerate() {
                let kw = if i == 0 { "if" } else { "} else if" };
                out.push_str(&format!("{}{} {} {{\n", inner_pad, kw, cond));
                emit_tir_stmts(body, cx, out, indent + 2);
            }
            // The `else`/fallthrough, byte-for-byte `emit_mixed_switch`: with arms and no
            // else → close the chain (`}`); with an else → `} else { … }`. (An empty
            // arm list is not reachable here — the gate requires at least one arm.)
            match else_body {
                None if !arms.is_empty() => {
                    out.push_str(&format!("{}}}\n", inner_pad));
                }
                None => {}
                Some(body) if arms.is_empty() => {
                    emit_tir_stmts(body, cx, out, indent + 1);
                }
                Some(body) => {
                    out.push_str(&format!("{}}} else {{\n", inner_pad));
                    emit_tir_stmts(body, cx, out, indent + 2);
                    out.push_str(&format!("{}}}\n", inner_pad));
                }
            }
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

/// c109 Phase 16: emit one enum-literal payload arg, applying its resolved
/// `clone`/`boxed` wrappers — `(…).clone()` first, then `Box::new(…)`, exactly as
/// `emit_boxed_enum_arg` (Expression.rs) does.
fn emit_tir_enum_arg(a: &TEnumArg, cx: &Cx) -> String {
    let mut s = emit_tir_expr(&a.value, cx);
    if a.clone {
        s = format!("({}).clone()", s);
    }
    if a.boxed {
        s = format!("Box::new({})", s);
    }
    s
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
        // c109 Phase 24: a comptime const inlined verbatim (the pre-rendered value).
        TExprKind::ConstInline(val) => val.clone(),
        TExprKind::Print(arg) => {
            format!("println!(\"{{}}\", ({}).jet_show())", emit_tir_expr(arg, cx))
        }
        // c109 Phase 25: ambient prelude `input(...)`, byte-for-byte the `emit_call`
        // ambient-input branch (Source/Codegen/Expression.rs): a bare call with NO arg
        // emits `{root}jet_std_io_input(None)`; with a prompt arg `{root}jet_std_io_input(Some(&(arg)))`.
        TExprKind::AmbientInput { prompt } => {
            let helper = format!("{}jet_std_io_input", cx.root_prefix);
            match prompt {
                None => format!("{}(None)", helper),
                Some(p) => format!("{}(Some(&({})))", helper, emit_tir_expr(p, cx)),
            }
        }
        // c109 Phase 26: a `require`/`require_eq`/`panic` rich-report builtin. The whole
        // `{ … }` block emit string was rendered at lowering (byte-for-byte the AST
        // helper); emit reads it verbatim. As a statement-position call it is wrapped
        // `{ … };` by `TStmt::ExprStmt`, matching the AST `Stmt::Expr` `{ … };`.
        TExprKind::RequireStop(rendered) => rendered.clone(),
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
        // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. Mirrors
        // `emit_method_call`'s `METHOD_DISTINCT_RAW` early return byte-for-byte.
        TExprKind::DistinctRaw(recv) => {
            format!("({}).0", emit_tir_expr(recv, cx))
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
        // c109 Phase 27: a call through a fn-typed struct field. Mirrors the AST
        // `emit_method_call` fn-field branch: `(({recv}).{field})({args})`.
        TExprKind::FnFieldCall {
            recv,
            field_rust,
            args,
        } => {
            let arg_str = emit_tir_call_args(args, cx);
            format!("(({}).{})({})", emit_tir_expr(recv, cx), field_rust, arg_str)
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
        // c109 Phase 9: a built-in collection/string method. The Map-vs-List-vs-String
        // branch was resolved into `op` at lowering; emit only formats, reproducing
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (no clone/borrow wrappers — `arg(i)` is a raw `emit_expr`).
        TExprKind::BuiltinMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
            match op {
                TBuiltinOp::LenString => format!("jet_char_len(&({}))", recv),
                TBuiltinOp::LenList => format!("({}).len() as i64", recv),
                TBuiltinOp::IsEmpty => format!("({}).is_empty()", recv),
                TBuiltinOp::Push => format!("({}).push({})", recv, a(0)),
                TBuiltinOp::Pop => format!("({}).pop()", recv),
                TBuiltinOp::InsertMap => {
                    format!("({}).insert(({}).clone(), {})", recv, a(0), a(1))
                }
                TBuiltinOp::InsertList => {
                    format!("({}).insert({} as usize, {})", recv, a(0), a(1))
                }
                TBuiltinOp::RemoveMap => format!("({}).remove(&({}).clone())", recv, a(0)),
                TBuiltinOp::RemoveList { line } => format!(
                    "jet_list_remove(&mut ({}), {}, {:?}, {})",
                    recv, a(0), cx.file, line
                ),
                TBuiltinOp::GetMap => format!("({}).get(&({}).clone()).cloned()", recv, a(0)),
                TBuiltinOp::GetList => format!("({}).get({} as usize).cloned()", recv, a(0)),
                TBuiltinOp::First => format!("({}).first().cloned()", recv),
                TBuiltinOp::Last => format!("({}).last().cloned()", recv),
                TBuiltinOp::Contains => format!("({}).contains(&{})", recv, a(0)),
                TBuiltinOp::IndexOf => format!(
                    "({}).iter().position(|x| *x == {}).map(|i| i as i64)",
                    recv, a(0)
                ),
                TBuiltinOp::Reverse => format!("({}).reverse()", recv),
                TBuiltinOp::Sort => format!("({}).sort()", recv),
                TBuiltinOp::JoinSep => format!(
                    "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
                    recv, a(0)
                ),
                TBuiltinOp::Clear => format!("({}).clear()", recv),
                TBuiltinOp::Chars => format!("({}).chars().collect::<Vec<char>>()", recv),
                TBuiltinOp::Bytes => {
                    format!("{}jet_string_bytes(&({}))", cx.root_prefix, recv)
                }
                TBuiltinOp::Trim => format!("({}).trim().to_string()", recv),
                TBuiltinOp::Split => format!("jet_string_split(&({}), &{})", recv, a(0)),
                TBuiltinOp::StartsWith => format!("({}).starts_with(&{})", recv, a(0)),
                TBuiltinOp::EndsWith => format!("({}).ends_with(&{})", recv, a(0)),
                TBuiltinOp::Replace => format!("({}).replace(&{}, &{})", recv, a(0), a(1)),
                TBuiltinOp::ToUpper => format!("({}).to_uppercase()", recv),
                TBuiltinOp::ToLower => format!("({}).to_lowercase()", recv),
                TBuiltinOp::Repeat => format!("({}).repeat({} as usize)", recv, a(0)),
                TBuiltinOp::Slice { line } => format!(
                    "jet_string_slice(&({}), {}, {}, {:?}, {})",
                    recv, a(0), a(1), cx.file, line
                ),
                TBuiltinOp::Keys => {
                    format!("({}).keys().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::Values => {
                    format!("({}).values().cloned().collect::<Vec<_>>()", recv)
                }
                TBuiltinOp::ContainsKey => format!("({}).contains_key(&{})", recv, a(0)),
                TBuiltinOp::ToString => format!("({}).jet_show()", recv),
                // c109 Phase 24: `Match.group(n)` → indexing into the `Vec<Option<String>>`.
                TBuiltinOp::MatchGroup => {
                    format!("({}).get(({}) as usize).cloned().flatten()", recv, a(0))
                }
            }
        }
        // c109 Phase 12: a numeric predicate / bit-pop / width-conversion method. The
        // width source/target + widening-vs-narrowing branch were resolved into `op` at
        // lowering; emit only formats, reproducing `emit_builtin_method`'s numeric arms
        // + `numeric_conversion` (Source/Codegen/Expression.rs) byte-for-byte.
        TExprKind::NumericMethod { recv, op } => {
            let recv = emit_tir_expr(recv, cx);
            match op {
                TNumericOp::Predicate(m) => format!("({}).{}()", recv, m),
                TNumericOp::BitCount(m) => format!("(({}).{}() as i64)", recv, m),
                TNumericOp::ToShow => format!("({}).jet_show()", recv),
                TNumericOp::CastAs { dst_rust } => format!("(({}) as {})", recv, dst_rust),
                TNumericOp::TryFrom { dst_rust, dst_spelling } => format!(
                    "<{dst_rust}>::try_from(({recv}) as i128).map_err(|_| \
                     \"value doesn't fit in {dst_spelling}\".to_string())"
                ),
            }
        }
        // c109 Phase 28: an overflow opt-out builtin. `prefix`/`op` were resolved at
        // lowering; reproduce `emit_call`'s `(ls).{name}_{suffix}(rs)` byte-for-byte.
        TExprKind::OverflowOpt { prefix, op, lhs, rhs } => {
            let ls = emit_tir_expr(lhs, cx);
            let rs = emit_tir_expr(rhs, cx);
            format!("({}).{}_{}({})", ls, prefix, op, rs)
        }
        // c109 Phase 10: a core/stdlib module call. Reproduces `emit_core_call`
        // (Source/Codegen/Expression.rs) byte-for-byte. `module`/`method` were
        // resolved at lowering; `cx.root_prefix`/`cx.ffi_crate` are program-level
        // (read here, like Phase 9's `cx.file`). Args were lowered PLAINLY — the
        // per-arm `&(…)`/`&mut (…)`/move wrappers are baked into each arm, exactly
        // as `emit_core_call` does (it ignores `CallArg.flags`).
        TExprKind::CoreCall { module, method, args } => {
            emit_tir_core_call(module, method, args, cx)
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
        TExprKind::StructLit { rust_type, fields, extra, as_trait } => {
            let mut parts = fields
                .iter()
                .map(|(field_rust, v, boxed)| {
                    let value = emit_tir_expr(v, cx);
                    // c109: a boxed (self-referential) field is wrapped `Box::new(…)`,
                    // exactly as `emit_struct_lit`. The `boxed` flag is total (resolved
                    // at lowering from `cx.boxed_edges`).
                    let value = if *boxed { format!("Box::new({})", value) } else { value };
                    format!("{}: {}", field_rust, value)
                })
                .collect::<Vec<_>>();
            // c109 Phase 17: a prelude struct's injected field (HttpRequest's `params`),
            // appended verbatim after the user fields, exactly as `emit_struct_lit` does.
            if let Some(extra) = extra {
                parts.push(extra.clone());
            }
            let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
            // c109 Phase 30: a trait-object coercion wraps the whole literal, byte-for-byte
            // `emit_struct_lit`'s `as_trait` branch (Source/Codegen/Expression.rs ~L342).
            match as_trait {
                Some(trait_rust) => format!("Box::new({lit}) as Box<dyn {trait_rust}>"),
                None => lit,
            }
        }
        // c109 Phase 3: `(recv).field`. Mirrors the AST `Expr::Field` emit form
        // exactly (no deref, no clone — owning reads were rewritten to a `.clone()`
        // MethodCall in sema and excluded from the subset).
        TExprKind::Field { recv, field_rust, boxed } => {
            let read = format!("({}).{}", emit_tir_expr(recv, cx), field_rust);
            if *boxed {
                format!("(*{})", read)
            } else {
                read
            }
        }
        // c109 Phase 18: `mem.Ptr<T>.from_addr(addr)` — `(({addr}) as usize as *mut {T})`,
        // byte-for-byte `emit_expr`'s `PtrFromAddr` arm. The cast is safe Rust (no
        // `unsafe`); `elem_rust` was resolved at lowering.
        TExprKind::PtrFromAddr { elem_rust, addr } => {
            format!("(({}) as usize as *mut {})", emit_tir_expr(addr, cx), elem_rust)
        }
        // c109 Phase 19: the arena allocator constructor — the ctor tail was rendered whole
        // at lowering (`jet_mem::Jet<Alloc>::new()` / `::with_capacity(...)`), so emit just
        // splices it. Byte-for-byte `emit_method_call`'s arena constructor branch.
        TExprKind::AllocNew { ctor } => ctor.clone(),
        // c109 Phase 4/16: an enum literal. Prefix + payload were resolved at lowering;
        // emit applies each arg's resolved `clone`/`boxed` wrappers (mirroring
        // `emit_boxed_enum_arg`: `(…).clone()` first, then `Box::new(…)`).
        TExprKind::EnumLit { prefix, payload } => match payload {
            TEnumPayload::Unit => prefix.clone(),
            TEnumPayload::Positional(vals) => {
                let pos = vals
                    .iter()
                    .map(|a| emit_tir_enum_arg(a, cx))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", prefix, pos)
            }
            TEnumPayload::Named(fields) => {
                let parts = fields
                    .iter()
                    .map(|(name, a)| format!("{}: {}", name, emit_tir_enum_arg(a, cx)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", prefix, parts)
            }
        },
        // c109 Phase 24: a JSON construction — `{root}jet_std::Json::<Variant>(<arg>)`.
        // Reproduces `emit_core_json_lit` (Expression.rs): the arg is wrapped in
        // `(…).clone()` iff its `implicit_clone` flag was set; `Null` has no arg.
        TExprKind::JsonLit { variant, arg } => {
            let prefix = format!("{}jet_std::Json", cx.root_prefix);
            match arg {
                None => format!("{}::{}", prefix, variant),
                Some(boxed) => {
                    let (val, implicit_clone) = boxed.as_ref();
                    let s = emit_tir_expr(val, cx);
                    let arg_str = if *implicit_clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    };
                    format!("{}::{}({})", prefix, variant, arg_str)
                }
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
        // c109 Phase 23: a named-tuple literal → `JetTup_<hash> { user_<f>: <v>, … }`.
        // Mirrors `emit_expr`'s `TupleLit` arm byte-for-byte (fields canonical-ordered,
        // resolved at lowering).
        TExprKind::TupleLit { struct_name, fields } => {
            let parts = fields
                .iter()
                .map(|(n, v)| format!("{}: {}", n, emit_tir_expr(v, cx)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", struct_name, parts)
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
        // c109 Phase 23: a `#Todo` typed hole → diverging `todo!(…)`. Byte-for-byte the
        // AST `Expr::Todo` arm (Expression.rs): file/line/expected-type baked into the
        // panic string. `cx.file` is program-level (read here, like every other use).
        TExprKind::Todo { line, expected_type } => format!(
            "todo!(\"#{} at {}:{} — expected {}\")",
            crate::Syntax::KW_TODO,
            cx.file,
            line,
            expected_type
        ),
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
        // c109 Phase 11: a lambda/closure literal. All decisions (prep/move/box) were
        // resolved at lowering off `Lambda.meta`; emit only assembles, byte-for-byte
        // `emit_lambda`: `{move }|params| body`, wrapped `Box::new(…)` when it escapes,
        // and prefixed with the `{ <prep> … }` block when there are cloned captures.
        TExprKind::Lambda(lam) => {
            let move_kw = if lam.is_move { "move " } else { "" };
            let closure = format!("{}|{}| {}", move_kw, lam.params.join(", "), lam.body);
            let wrapped = if lam.boxed {
                format!("Box::new({})", closure)
            } else {
                closure
            };
            if lam.prep.is_empty() {
                wrapped
            } else {
                format!("{{ {} {} }}", lam.prep, wrapped)
            }
        }
        // c109 Phase 11: fan-out `f.[a, b, c]` → `vec![f(a), f(b), f(c)]`. The
        // per-item calls were lowered at lowering; emit only wraps them in `vec![…]`,
        // byte-for-byte the AST `Expr::FanOut` Ident-callee form.
        TExprKind::FanOut { calls } => {
            let elems = calls
                .iter()
                .map(|c| emit_tir_expr(c, cx))
                .collect::<Vec<_>>()
                .join(", ");
            format!("vec![{}]", elems)
        }
        // c109 Phase 11: a closure-taking collection method. The receiver-type +
        // Fn-vs-FnMut dispatch was resolved into `op` at lowering; emit only formats,
        // reproducing `emit_builtin_method`'s closure arms byte-for-byte. Args (the
        // lambda + any seed) are emitted PLAINLY (raw `arg(i)`).
        TExprKind::ClosureMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
            match op {
                TClosureOp::Map => format!("jet_list_map(({}).clone(), {})", recv, a(0)),
                TClosureOp::MapMut => format!("jet_list_map_mut(({}).clone(), {})", recv, a(0)),
                TClosureOp::Filter => format!("jet_list_filter(({}).clone(), {})", recv, a(0)),
                TClosureOp::Each => format!("jet_list_each(({}).clone(), {})", recv, a(0)),
                TClosureOp::EachMut => format!("jet_list_each_mut(({}).clone(), {})", recv, a(0)),
                TClosureOp::EachRef => format!("jet_list_each_ref(&({}), {})", recv, a(0)),
                TClosureOp::EachMap => format!("jet_map_each(({}).clone(), {})", recv, a(0)),
                TClosureOp::Find => format!("jet_list_find(({}).clone(), {})", recv, a(0)),
                TClosureOp::Any => format!("jet_list_any(({}).clone(), {})", recv, a(0)),
                TClosureOp::All => format!("jet_list_all(({}).clone(), {})", recv, a(0)),
                TClosureOp::SortBy => format!("{{ jet_list_sort_by(&mut {}, {}); }}", recv, a(0)),
                TClosureOp::Reduce => {
                    format!("jet_list_reduce(({}).clone(), {}, {})", recv, a(0), a(1))
                }
            }
        }
        // c109 Phase 13: a method ON a handle. The handle-receiver branch was resolved
        // into `op` at lowering; emit only formats, reproducing the handle arms of
        // `emit_builtin_method` (Source/Codegen/Expression.rs) byte-for-byte. Args are
        // emitted PLAINLY (raw `arg(i)`). `cx.root_prefix` is program-level.
        TExprKind::HandleMethod { recv, op, args } => {
            let recv = emit_tir_expr(recv, cx);
            let a = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
            let root = &cx.root_prefix;
            match op {
                THandleOp::FileReaderReadLine => {
                    format!("{}jet_std_file_reader_read_line(&mut ({}))", root, recv)
                }
                THandleOp::FileWriterWriteLine => format!(
                    "{}jet_std_file_writer_write_line(&mut ({}), &({}))",
                    root, recv, a(0)
                ),
                THandleOp::FileWriterFlush => {
                    format!("{}jet_std_file_writer_flush(&mut ({}))", root, recv)
                }
                THandleOp::StdinReadLine => {
                    format!("{}jet_std_io_stdin_read_line(&mut ({}))", root, recv)
                }
                THandleOp::StopwatchElapsedMillis => {
                    format!("{}jet_stopwatch_elapsed_millis(&({}))", root, recv)
                }
                THandleOp::TcpListenerAccept => format!("{}jet_net_tcp_accept(&({}))", root, recv),
                THandleOp::TcpListenerLocalAddr => {
                    format!("{}jet_net_listener_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamRead => format!("{}jet_net_tcp_read(&mut ({}))", root, recv),
                THandleOp::TcpStreamWrite => {
                    format!("{}jet_net_tcp_write(&mut ({}), &({}))", root, recv, a(0))
                }
                THandleOp::TcpStreamPeerAddr => {
                    format!("{}jet_net_tcp_peer_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamLocalAddr => {
                    format!("{}jet_net_tcp_local_addr(&({}))", root, recv)
                }
                THandleOp::TcpStreamClose => format!("{{ drop({}); }}", recv),
                // c109 Phase 19: arena allocator methods (byte-for-byte the AST arms).
                THandleOp::AllocAlloc => {
                    let a0 = emit_tir_expr(&args[0], cx);
                    format!("({}).alloc({})", recv, a0)
                }
                THandleOp::AllocReset => format!("({}).reset()", recv),
                THandleOp::AllocFree => format!("drop({})", recv),
                // c109 Phase 20: HttpRequest/HttpResponse accessors, byte-for-byte the
                // `emit_builtin_method` arms. The plain field accessors clone the field;
                // `header` does a map lookup; `param` calls the prelude helper.
                THandleOp::HttpReqField(field) | THandleOp::HttpRespField(field) => {
                    format!("({}).{}.clone()", recv, field)
                }
                THandleOp::HttpReqHeader | THandleOp::HttpRespHeader => {
                    format!("({}).headers.get(&{}).cloned()", recv, a(0))
                }
                THandleOp::HttpReqParam => {
                    format!("{}jet_http_request_param(&({}), &({}))", root, recv, a(0))
                }
                // c109 Phase 21: Task/Channel/Sender methods, byte-for-byte the
                // `emit_builtin_method` arms (Source/Codegen/Expression.rs). The handle
                // value's prelude methods take `&self`, so the receiver is emitted plainly
                // (Rust autoref); args are plain (raw `emit_expr`). `join` reuses the
                // no-arg `join` arm (`(recv).join()`); `detach` drops the handle (D-DETACH1).
                THandleOp::TaskJoin => format!("({}).join()", recv),
                THandleOp::TaskDetach => format!("{{ let _detach = ({}); }}", recv),
                THandleOp::ChannelReceive => format!("({}).receive()", recv),
                THandleOp::ChannelSender => format!("({}).sender()", recv),
                THandleOp::SenderSend => format!("({}).send({})", recv, a(0)),
                // c109 Phase 25: HttpRouter route registration, byte-for-byte the
                // `emit_builtin_method` router arm (Source/Codegen/Expression.rs ~L937).
                // `recv` is `&mut`-borrowed; the path is plain (args[0]); the handler is
                // the pre-rendered boxed closure.
                THandleOp::HttpRouterRegister { verb, handler } => format!(
                    "{}jet_http_router_register(&mut ({}), \"{}\".to_string(), {}, {})",
                    root, recv, verb, a(0), handler
                ),
            }
        }
        // c109 Phase 13: a closure-taking core call. The closure was rendered at
        // lowering; emit assembles the bespoke shape, byte-for-byte `emit_core_call`
        // (Source/Codegen/Expression.rs).
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { spawn_closure } => {
                format!("{}jet_std::JetTask::spawn({})", cx.root_prefix, spawn_closure)
            }
            TCoreClosureKind::Serve { addr, closure } => format!(
                "{}jet_http_serve(&({}), {})",
                cx.root_prefix,
                emit_tir_expr(addr, cx),
                closure
            ),
            TCoreClosureKind::Guard { closure } => {
                format!("{}jet_scope_guard({})", cx.root_prefix, closure)
            }
        },
        // c109 Phase 13: a fn-typed value. A bare fn-name value echoes the
        // already-rendered `Box::new(move |…| …) as <fn-type>` wrapper; a call through
        // a fn-value emits `({callee})({args})`, byte-for-byte `emit_expr`'s
        // `Expr::CallValue` (Source/Codegen/Expression.rs).
        TExprKind::FnValue { kind } => match kind {
            TFnValueKind::NamedFn { wrapper } => wrapper.clone(),
            TFnValueKind::Call { callee, args } => {
                format!(
                    "({})({})",
                    emit_tir_expr(callee, cx),
                    emit_tir_call_args(args, cx)
                )
            }
        },
        // c109 Phase 14: a cross-module call. The path form was resolved at lowering;
        // emit prepends `cx.root_prefix` exactly where the AST path does (both the
        // qualified `{root}{mod}::{fn}` form and the inline `{root}user_{mangled}` form
        // prefix with root). Args were resolved into `TCallArg`s (`emit_tir_call_args`).
        TExprKind::ModuleCall { form, args } => {
            let arg_str = emit_tir_call_args(args, cx);
            match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{}{}::{}({})", cx.root_prefix, rust_mod, rust_fn, arg_str)
                }
                TModuleCallForm::InlineMangled { mangled } => {
                    format!("{}user_{}({})", cx.root_prefix, mangled, arg_str)
                }
            }
        }
        // c109 Phase 14: an FFI extern call. Reproduces `emit_call`'s `extern_funcs`
        // arm: `{ffi_crate}::{wrapper}(args)`. `cx.ffi_crate` is program-level (read
        // here, like Phase 10's regex form); the AST falls back to "jet_ffi" when it is
        // `None` (always `Some` when an extern call is present, but mirror it exactly).
        // Args use the extern arg form (`(…).clone()` for a non-scalar Read).
        TExprKind::ExternCall { wrapper, args } => {
            let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            let arg_str = args
                .iter()
                .map(|a| {
                    let s = emit_tir_expr(&a.value, cx);
                    if a.clone {
                        format!("({}).clone()", s)
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}::{}({})", crate_name, wrapper, arg_str)
        }
    }
}

/// c109 Phase 8/15: format a `??` fallback right-hand side, mirroring
/// `emit_or_fallback_rhs` (Statement.rs). Value and early-`return` (Phase 8); the
/// `panic(…)` form (Phase 15) carries its fully-rendered statement string from lowering.
fn emit_tir_orfallback_rhs(fallback: &TOrFallback, cx: &Cx) -> String {
    match fallback {
        TOrFallback::Value(e) => emit_tir_expr(e, cx),
        TOrFallback::Return(None) => "return".to_string(),
        TOrFallback::Return(Some(e)) => format!("return {}", emit_tir_expr(e, cx)),
        TOrFallback::Panic(rendered) => rendered.clone(),
    }
}

fn emit_tir_value_block(stmts: &[TStmt], value: &TExpr, cx: &Cx) -> String {
    let mut inner = String::new();
    emit_tir_stmts(stmts, cx, &mut inner, 1);
    format!("{{ {} {} }}", inner, emit_tir_expr(value, cx))
}

/// c109 Phase 10: emit a core/stdlib module call, reproducing `emit_core_call`
/// (Source/Codegen/Expression.rs) byte-for-byte. The `(module, method)` dispatch is
/// a pure syntactic match on the two resolved strings — no type inference (I3). Args
/// were lowered PLAINLY; the per-arm `&(…)`/`&mut (…)`/move wrappers are applied here
/// exactly as the AST path applies them around its `arg(i)` = raw `emit_expr`.
/// `cx.root_prefix`/`cx.ffi_crate` are program-level. The gate only ever admits a
/// `(module, method)` with a matching arm here, so the `/* unknown std call */`
/// fallthrough is unreachable for a covered call (kept for parity with the AST path).
fn emit_tir_core_call(module: &str, method: &str, args: &[TExpr], cx: &Cx) -> String {
    let arg = |i: usize| args.get(i).map(|e| emit_tir_expr(e, cx)).unwrap_or_default();
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    let regex_fn = |name: &str| {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        format!("{}::{}", crate_name, name)
    };
    match (module, method) {
        // c109 Phase 18 (S58, E2-M13): low-level pointer ops, byte-for-byte
        // `emit_core_call`. `address_of` is an inert address cast (no `unsafe`);
        // `volatile_read` reads through a `Ptr<T>` — `read_volatile` is valid because the
        // call only reaches codegen inside an `#Unsafe` region/fn (sema E3101), already
        // lowered to a Rust `unsafe` context.
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        ("core.mem", "volatile_read") => format!("std::ptr::read_volatile({})", arg(0)),
        // c109 Phase 21: the `tasks.channel()` producer, byte-for-byte `emit_core_call`.
        ("core.tasks", "channel") => format!("{}jet_std::JetChannel::new()", cx.root_prefix),
        ("core.fs", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("core.fs", "read_bytes") => format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0)),
        ("core.fs", "write") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "append") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_append"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("core.fs", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("core.fs", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("core.fs", "create_dir") => format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0)),
        ("core.fs", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("core.fs", "copy") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "rename") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_rename"),
            arg(0),
            arg(1)
        ),
        ("core.io", "args") => format!("{}()", helper("jet_std_io_args")),
        // c109 Phase 29: qualified `io.input(prompt)`, byte-for-byte `emit_core_call`
        // (Expression.rs ~L1294): no arg → `jet_std_io_input(None)`; a prompt arg →
        // `jet_std_io_input(Some(&(prompt)))`. Same emitted helper as the ambient bare
        // `input(...)` (Phase 25), the only difference being the source node shape.
        ("core.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        ("core.io", "read_all_input") => format!("{}()", helper("jet_std_io_read_all_input")),
        // D-STDIN1=A: io.stdin() → JetStdinReader handle.
        ("core.io", "stdin") => format!("{}()", helper("jet_std_io_stdin")),
        ("core.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("core.env", "set") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_env_set"),
            arg(0),
            arg(1)
        ),
        ("core.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("core.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("core.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("core.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        ("core.math", "sqrt") => format!("{}({})", helper("jet_std_math_sqrt"), arg(0)),
        ("core.math", "pow") => format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1)),
        ("core.math", "floor") => format!("{}({})", helper("jet_std_math_floor"), arg(0)),
        ("core.math", "ceil") => format!("{}({})", helper("jet_std_math_ceil"), arg(0)),
        ("core.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        ("core.random", "int") => {
            format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1))
        }
        ("core.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("core.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        ("core.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("core.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("core.time", "start") => format!("{}()", helper("jet_std_time_start")),
        ("core.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        ("core.json", "render") => format!("{}(&({}))", helper("jet_std_json_render"), arg(0)),
        ("core.json", "render_pretty") => {
            format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0))
        }
        // E2-M7: streaming file handles (D-IO2).
        ("core.files", "open") => format!("{}(&({}))", helper("jet_std_files_open"), arg(0)),
        ("core.files", "create") => format!("{}(&({}))", helper("jet_std_files_create"), arg(0)),
        ("core.files", "append") => format!("{}(&({}))", helper("jet_std_files_append"), arg(0)),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_path_join"), arg(0), arg(1)
        ),
        ("core.path", "parent") => format!("{}(&({}))", helper("jet_std_path_parent"), arg(0)),
        ("core.path", "extension") => format!("{}(&({}))", helper("jet_std_path_extension"), arg(0)),
        ("core.path", "normalize") => format!("{}(&({}))", helper("jet_std_path_normalize"), arg(0)),
        // E2-M9: first-party ring packages.
        ("jet.csv", "parse") => format!("{}(&({}))", helper("jet_ring_csv_parse"), arg(0)),
        ("jet.csv", "render") => format!("{}(&({}))", helper("jet_ring_csv_render"), arg(0)),
        ("jet.toml", "parse") => format!("{}(&({}))", helper("jet_ring_toml_parse"), arg(0)),
        ("jet.toml", "render") => format!("{}(&({}))", helper("jet_ring_toml_render"), arg(0)),
        ("jet.yaml", "parse") => format!("{}(&({}))", helper("jet_ring_yaml_parse"), arg(0)),
        ("jet.yaml", "render") => format!("{}(&({}))", helper("jet_ring_yaml_render"), arg(0)),
        ("jet.log", "info") => format!("{}(&({}))", helper("jet_ring_log_info"), arg(0)),
        ("jet.log", "warn") => format!("{}(&({}))", helper("jet_ring_log_warn"), arg(0)),
        ("jet.log", "error") => format!("{}(&({}))", helper("jet_ring_log_error"), arg(0)),
        ("jet.log", "debug") => format!("{}(&({}))", helper("jet_ring_log_debug"), arg(0)),
        ("jet.log", "set_level") => format!("{}(&({}))", helper("jet_ring_log_set_level"), arg(0)),
        // E2-M12 D-OBS3: trace context for structured log records.
        ("jet.log", "set_trace_id") => format!("{}(&({}))", helper("jet_ring_log_set_trace_id"), arg(0)),
        // D-LOGFMT1=A: explicit log format override.
        ("jet.log", "setup") => format!("{}(&({}))", helper("jet_ring_log_setup"), arg(0)),
        ("jet.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        // D-JSON3=B: lenient decode emits one log line per coercion; decoded value is plain.
        ("jet.json", "decode") => format!("{}(&({}))", helper("jet_std_json_decode_lenient"), arg(0)),
        ("jet.json", "render") => format!("{}(&({}))", helper("jet_std_json_render"), arg(0)),
        ("jet.json", "render_pretty") => format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0)),
        ("jet.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("jet.time", "format") => format!("{}({}, &({}))", helper("jet_ring_time_format"), arg(0), arg(1)),
        ("jet.crypto", "sha256") => format!("{}(&({}))", helper("jet_ring_crypto_sha256"), arg(0)),
        ("jet.crypto", "sha256_bytes") => format!("{}(&({}))", helper("jet_ring_crypto_sha256_bytes"), arg(0)),
        // E2-M10: core.net — blocking TCP sockets.
        ("core.net", "tcp_listen") => format!("{}(&({}))", helper("jet_net_tcp_listen"), arg(0)),
        ("core.net", "tcp_accept") => format!("{}(&({}))", helper("jet_net_tcp_accept"), arg(0)),
        ("core.net", "tcp_connect") => format!("{}(&({}))", helper("jet_net_tcp_connect"), arg(0)),
        ("core.net", "tcp_read") => format!("{}(&mut ({}))", helper("jet_net_tcp_read"), arg(0)),
        ("core.net", "tcp_write") => {
            format!("{}(&mut ({}), &({}))", helper("jet_net_tcp_write"), arg(0), arg(1))
        }
        ("core.net", "tcp_local_addr") => format!("{}(&({}))", helper("jet_net_tcp_local_addr"), arg(0)),
        ("core.net", "tcp_peer_addr") => format!("{}(&({}))", helper("jet_net_tcp_peer_addr"), arg(0)),
        ("core.net", "set_timeout") => {
            format!("{}(&mut ({}), {})", helper("jet_net_set_timeout"), arg(0), arg(1))
        }
        ("core.net", "tcp_reply") => {
            format!("{}({}, &({}), &({}))", helper("jet_net_tcp_reply"), arg(0), arg(1), arg(2))
        }
        // E2-M10: jet.http — HTTP client.
        ("jet.http", "get") => format!("{}(&({}))", helper("jet_http_get"), arg(0)),
        ("jet.http", "post") => {
            format!("{}(&({}), &({}))", helper("jet_http_post"), arg(0), arg(1))
        }
        // c109 Phase 25: HttpRouter producer + parse/dispatch (D-ROUTE1=A), byte-for-byte
        // `emit_core_call` (Source/Codegen/Expression.rs ~L1411). `router()` is arg-free;
        // `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router
        // and passes the request by value.
        ("jet.http", "router") => format!("{}()", helper("jet_http_router_new")),
        ("jet.http", "parse") => format!("{}(&({}))", helper("jet_http_parse_request"), arg(0)),
        ("jet.http", "dispatch") => format!(
            "{}(&({}), {})",
            helper("jet_http_router_dispatch"), arg(0), arg(1)
        ),
        // D-REGEX1: jet.regex — calls land in the FFI bridge crate.
        ("jet.regex", "is_match") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_is_match"), arg(0), arg(1))
        }
        ("jet.regex", "match") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_match"), arg(0), arg(1))
        }
        ("jet.regex", "find") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_find"), arg(0), arg(1))
        }
        ("jet.regex", "find_all") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_find_all"), arg(0), arg(1))
        }
        ("jet.regex", "split") => {
            format!("{}(&({}), &({}))", regex_fn("jet_regex_split"), arg(0), arg(1))
        }
        ("jet.regex", "replace") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn("jet_regex_replace"), arg(0), arg(1), arg(2)
        ),
        ("jet.regex", "replace_all") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn("jet_regex_replace_all"), arg(0), arg(1), arg(2)
        ),
        // c109 Phase 20: the polymorphic core specials — byte-for-byte `emit_core_call`.
        // Their return type is arg-type dependent (resolved by sema's bespoke
        // `infer_core_call` and written onto the node's `resolved_ret`, read at
        // lowering), but the EMITTED form is a fixed per-`(module, method)` string —
        // no type decision here (I3). Args are emitted PLAINLY, exactly `emit_core_call`.
        ("core.math", "abs") => format!("({}).abs()", arg(0)),
        ("core.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("core.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("core.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        ("core.random", "pick") => format!("{}(&({}))", helper("jet_std_random_pick"), arg(0)),
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        _ => "/* unknown std call */".to_string(),
    }
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
            // c109 Phase 13: the Fn-typed Box-coercion, applied AFTER the clone wrapper
            // and BEFORE the borrow wrapper — exactly `emit_call_args`' order. `Box::new`
            // is added only when the value isn't already boxed (resolved at lowering).
            if let Some(fc) = &a.fn_coerce {
                if !fc.already_boxed {
                    s = format!("Box::new({})", s);
                }
                s = format!("{} as {}", s, fc.fn_type_rust);
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

    /// Like `covers`, but runs the FULL front end (sema) on `src` first, so
    /// sema-filled facts — notably a comptime LOCAL's evaluated `b.ct` value
    /// (S57/M9.5) — are present before gating. Builds a single-module bundle the
    /// same way `lib.rs::check_for_eval` does, asserts sema accepted the program,
    /// then runs `tir_covers` on the sema-enriched function.
    fn covers_after_sema(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let mut prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: std::path::PathBuf::from("."),
            modules: vec![crate::AST::LoadedModule {
                path: std::path::PathBuf::from("test.jet"),
                display: "test.jet".to_string(),
                alias: "main".to_string(),
                imports: std::mem::take(&mut prog.imports),
                items: std::mem::take(&mut prog.items),
                source: src.to_string(),
            }],
            parse_teaching: Vec::new(),
            used_core: std::collections::HashSet::new(),
            cffi: crate::CFFI::CFfi::default(),
        };
        bundle.cffi = crate::CFFI::assemble(&mut bundle).expect("cffi assemble failed");
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
        assert!(
            !diags.iter().any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "sema errors: {diags:?}"
        );
        let module = &bundle.modules[bundle.entry];
        let cx = build_cx_items(&module.items, src, "test.jet", None, &HashMap::new());
        let f = module
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

    /// Coverage of a TRAIT method implemented in a top-level `impl Type: Trait`
    /// block — routes through `tir_covers_trait_method` (the trait-impl gate).
    fn covers_trait_method(src: &str, type_name: &str, method: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let methods: &[Func] = prog
            .items
            .iter()
            .find_map(|i| match i {
                Item::Impl(im) if im.type_name == type_name && im.trait_name.is_some() => {
                    Some(im.methods.as_slice())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no `impl {type_name}: Trait` block"));
        let f = methods
            .iter()
            .find(|m| m.name == method)
            .unwrap_or_else(|| panic!("no trait method {type_name}.{method}"));
        tir_covers_trait_method(f, type_name, &cx)
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
    fn covers_bare_or_return_in_unit_fn() {
        // c109 (bare `?? return` fix): a bare `?? return` in a UNIT fn is in-subset
        // (`orfallback_rhs_in_subset → Return(None) => true`) and emits `None => return`.
        // Sema now accepts it only in a unit fn (rustc accepts `return;`).
        let src = "fn f(xs: [Int]) {\n x := xs.first() ?? return\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_struct_lit_with_string_field_value() {
        // c109 (borrowed struct-lit value clone): a struct with a String field, built
        // from a param value, is in-subset (struct + clone are covered). The borrowed-
        // ident clone is a SEMA rewrite (`(n).clone()`) — the `covers` helper is
        // build_cx-only so it sees the bare ident here, which is also in-subset; the
        // authoritative byte-for-byte proof is tests/tir.rs `borrowed_struct_lit_field_value_cloned`.
        let src = "\
struct Person {
    name: String
}
fn make(n: String) -> Person {
    return Person { name: n }
}
";
        assert!(covers(src, "make"));
    }

    #[test]
    fn covers_is_empty_bool() {
        // c109 (`is_empty` Bool fix): `is_empty` on a list/map/string is now covered
        // (`TBuiltinOp::IsEmpty`, Bool result) — it was excluded while sema mistyped it
        // as `Int`. The `if xs.is_empty()` form must be in-subset.
        let src = "fn f(xs: [Int]) {\n if xs.is_empty() {\n print(\"e\")\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_generic_fn() {
        // c109 Phase 17: a generic free function whose params/return are type vars is
        // covered — the `<T: Clone>` clause renders at lowering; the body uses the
        // type-var value by-value. (The `covers` helper is build_cx-only, so it sees
        // `x: T` as a Read param; sema would require `take x: T`, but the gate shape is
        // identical either way — a type-var param/return is in-subset.)
        assert!(covers("fn id<T>(x: T) -> T {\n return x\n}\n", "id"));
    }

    #[test]
    fn covers_generic_struct_fn() {
        // c109 Phase 19: a GENERIC STRUCT free function — its `Type::Apply` (`Pair<T>`)
        // param/return type and the turbofish construction (`user_Pair::<T> { … }`) are now
        // covered. The struct's type-var fields are admitted by `field_ty_covered`; the
        // turbofish head is resolved at lowering.
        let src = "struct Pair<T> {\n first: T\n second: T\n}\nfn mk<T>(a: T, b: T) -> Pair<T> {\n return Pair<T> {first: a, second: b}\n}\n";
        assert!(covers(src, "mk"));
    }

    /// c109 Phase 18: like `covers`, but injects the `mem` → `core.mem` import (the
    /// `build_cx`-only path leaves `core_imports` empty — it is populated from the bundle
    /// at real codegen; mirror that here so the core-`mem` gate paths are exercised). The
    /// end-to-end build+run + the full-suite byte-parity diff are the authoritative proof
    /// (see `tests/tir.rs::unsafe_fn_block_and_ptr_ops`); this exercises the gate shape.
    fn covers_with_mem(src: &str, fn_name: &str) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut cx = build_cx(&prog, src, "test.jet");
        cx.core_imports.insert("mem".to_string(), "core.mem".to_string());
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

    /// Like `covers`, but injects a foreign type → module mapping (`cx.foreign_types`)
    /// — the `build_cx`-only path leaves it empty (it's populated from the bundle at real
    /// codegen). Mirrors `covers_with_mem`. The end-to-end build+run + the full-suite
    /// byte-parity diff are the authoritative proof (tests/tir.rs); this exercises the gate.
    fn covers_with_foreign(src: &str, fn_name: &str, foreign: &[(&str, &str)]) -> bool {
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let mut cx = build_cx(&prog, src, "test.jet");
        for (ty, module) in foreign {
            cx.foreign_types.insert(ty.to_string(), module.to_string());
        }
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

    #[test]
    fn covers_unqualified_foreign_struct_literal() {
        // c109 (foreign struct literal): an UNqualified cross-module foreign struct literal
        // (`Note { … }`, no `import_ns`) is now covered — the StructLit gate admits a
        // `cx.foreign_types` type and lowering prefixes the module head
        // (`user_notes::user_Note`). The construct miscompiled to a bare `user_Note { … }`
        // (E0422) before; the fix prefixes the foreign module.
        let src = "\
fn mk() {
    n @= Note { text: \"hi\" }
    print(n.text)
}
";
        assert!(covers_with_foreign(src, "mk", &[("Note", "user_notes")]));
    }

    #[test]
    fn covers_unsafe_fn_with_ptr_ops() {
        // c109 Phase 18: a `#Unsafe fn` (S58) is covered — it lowers to `unsafe fn`, and
        // its body's `mem.Ptr<T>.from_addr` / `mem.volatile_read` ops are in-subset.
        let src = "use core.mem\n#Unsafe\nfn read_reg(addr: Int) -> Int {\n p @= mem.Ptr<Int>.from_addr(addr)\n return mem.volatile_read(p)\n}\n";
        assert!(covers_with_mem(src, "read_reg"));
    }

    #[test]
    fn covers_unsafe_block_and_address_of() {
        // c109 Phase 18: a `#Unsafe { … }` audited region + `mem.address_of` (the inert
        // address cast, legal outside unsafe) are covered. The `#Audit("…")` annotation
        // emits nothing.
        let src = "use core.mem\nfn main() {\n cell: Int @= 7\n addr @= mem.address_of(cell)\n #Audit(\"live\")\n #Unsafe {\n p @= mem.Ptr<Int>.from_addr(addr)\n seen @= mem.volatile_read(p)\n print(\"{seen}\")\n }\n}\n";
        assert!(covers_with_mem(src, "main"));
    }

    #[test]
    fn covers_list_param() {
        // c109 Phase 5: a list parameter is now inside the subset (was excluded
        // through Phase 4).
        assert!(covers("fn sum(xs: [Int]) -> Int {\n return 0\n}\n", "sum"));
    }

    #[test]
    fn covers_fixed_list_param_and_field() {
        // c109 (B2): a fixed-size-list type `[E#N]` is covered like a list (`Vec<E>`)
        // as a param/return type and as a struct field, once its element type is
        // covered. (Indexing a `[E#N]` is in-subset only once sema resolves the
        // `IndexKind` — exercised end-to-end by tests/tir.rs; here we gate the
        // sema-independent type-coverage facts the four helpers decide.)
        let mk = |src: &str| {
            let (toks, _) = crate::Lexer::lex(src);
            let prog = crate::Parser::parse(&toks).expect("parse");
            build_cx(&prog, src, "test.jet")
        };
        let fl = Type::FixedList { elem: Box::new(Type::Int), len: 3 };
        // param/return helper coverage:
        assert!(is_subset_param_ty(&fl, &mk("fn f(){}")));
        assert!(is_covered_collection_ty(&fl, &mk("fn f(){}")));
        assert!(collection_elem_covered(&fl, &mk("fn f(){}")));
        // struct-field coverage: a `[Int#3]` field keeps its owning struct covered.
        let src = "struct Grid { row: [Int#3] }\nfn f(){}";
        assert!(is_covered_struct_ty(&Type::Named("Grid".to_string()), &mk(src)));
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
    fn covers_recursive_boxed_struct() {
        // c109 (boxed field read): a self-referential struct is now a covered VALUE type
        // — a boxed field read derefs the `Box` (total `boxed` fact). A fn reading a plain
        // scalar field of a recursive struct routes through the TIR.
        let src = "struct Node { value: Int\n next: Node }\nfn val(n: Node) -> Int {\n return n.value\n}\n";
        assert!(covers(src, "val"));
    }

    #[test]
    fn covers_struct_with_list_field() {
        // c109 Phase 16: a struct with a covered collection field (`[Int]`). The
        // struct-literal emit is plain (`items: vec![…]`), byte-identical to the AST
        // path, so the owning struct is covered as a param/return.
        let src = "struct Bag { items: [Int] }\nfn first_tag(b: Bag) -> Int {\n return 0\n}\n";
        assert!(covers(src, "first_tag"));
    }

    #[test]
    fn covers_generic_struct_literal() {
        // c109 Phase 19: a generic struct literal (`Pair<Int> { … }`) carries non-empty
        // `type_args` (the turbofish `user_Pair::<i64> { … }`) and its field types reference
        // type vars — both now covered. The owning fn routes through the TIR.
        let src = "struct Pair<T> { first: T\n second: T }\nfn mk() -> Pair<Int> {\n return Pair<Int> { first: 1, second: 2 }\n}\n";
        assert!(covers(src, "mk"));
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
    fn covers_mixed_switch_non_ident_subject() {
        // c109 (B1): a pattern switch over a NON-IDENT subject routes through the
        // exhaustive-match / fallible-match path (the subject is matched by source-text
        // equality, not just an ident name). A call subject with unit-variant arms:
        let variant = "enum Light { Red Green Yellow }\nfn pick() -> Light { return Light.Red }\nfn classify() -> Int {\n if pick() {\n Red -> { return 1 }\n Green -> { return 2 }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(variant, "classify"));
        // A field-access subject with a payload-binding (optional) arm:
        let payload = "struct Holder { val: Int? }\nfn f(h: Holder) -> Int {\n if h.val {\n h.val == value(c) -> { return c }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(payload, "f"));
    }

    #[test]
    fn covers_enum_local_and_literal_in_main() {
        // An enum-typed local bound from a literal, passed to a covered helper.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn label(l: Light) -> String {\n if l {\n Red -> { return \"r\" }\n Yellow -> { return \"y\" }\n Green -> { return \"g\" }\n }\n}\nfn main() {\n start @= Light.Red\n print(label(start))\n}\n";
        assert!(covers(src, "main"));
    }

    #[test]
    fn covers_string_payload_enum() {
        // c109 Phase 16: a String-payload enum. The literal's borrowed-payload
        // `.clone()` and pattern bindings are reproduced as total facts
        // (`emit_boxed_enum_arg`), so the match + getter route through the TIR.
        let src = "enum Msg {\n Text(String)\n Ping\n}\nfn show(m: Msg) -> String {\n if m {\n m == Text(s) -> { return s }\n m == Ping -> { return \"ping\" }\n }\n return \"\"\n}\n";
        assert!(covers(src, "show"));
    }

    #[test]
    fn covers_recursive_enum() {
        // c109 Phase 16: a self-referential (boxed) enum. The `Box::new(…)` at
        // construction and the auto-deref at pattern/field sites are total facts
        // (`TEnumArg.boxed`), so a covered traversal routes through the TIR.
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn depth(t: Tree) -> Int {\n if t {\n t == Leaf(n) -> { return n }\n t == Node(inner) -> { return 1 }\n }\n return 0\n}\n";
        assert!(covers(src, "depth"));
    }

    #[test]
    fn covers_recursive_enum_construction_with_clone_box() {
        // c109 Phase 16: constructing a recursive enum from a BORROWED payload —
        // `Tree.Node(inner)` where `inner: Tree` is a `Read` (borrowed) param. The
        // arg gets `Box::new(((*inner)).clone())` (non-scalar payload → borrowed
        // `.clone()`, then the recursive boxed edge → `Box::new`), reproducing
        // `emit_boxed_enum_arg` exactly. The construction reaches codegen as a
        // `MethodCall` (sema never emits an `Expr::EnumLit` for a payload variant).
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn wrap(inner: Tree) -> Tree {\n return Tree.Node(inner)\n}\n";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_struct_payload_enum() {
        // c109 Phase 16: an enum variant carrying a covered struct payload. The
        // struct value flows through the variant construction + pattern binding
        // without a clone/box decision the subset can't make (the value's own move/
        // clone facts live in its sub-expression).
        let src = "struct Point { x: Int\n y: Int }\nenum Shape {\n Dot(Point)\n Line(Int)\n}\nfn area(s: Shape) -> Int {\n if s {\n s == Dot(p) -> { return p.x }\n s == Line(n) -> { return n }\n }\n return 0\n}\n";
        assert!(covers(src, "area"));
    }

    #[test]
    fn covers_collection_payload_enum() {
        // c109 Phase 16: an enum variant carrying a covered collection payload
        // (`[Int]`). Construction (`Data.Nums(xs)`) routes through the variant
        // MethodCall shape; the borrowed-list `.clone()` is total.
        let src = "enum Data {\n Nums([Int])\n One(Int)\n}\nfn mk(xs: [Int]) -> Data {\n return Data.Nums(xs)\n}\n";
        assert!(covers(src, "mk"));
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
    fn covers_method_call_collection_iteration() {
        // c109 Phase 22: `loop c in s.chars()` (char iteration) and `loop x in
        // s.split(…)` (the `.iter().cloned()` default) are now reproduced from
        // `emit_for_in`'s method-call branches.
        let chars = "fn f(s: String) {\n loop c in s.chars() {\n print(c)\n }\n}\n";
        assert!(covers(chars, "f"));
        let split = "fn f(s: String) {\n loop w in s.split(\",\") {\n print(w)\n }\n}\n";
        assert!(covers(split, "f"));
    }

    #[test]
    fn covers_optional_binding_if_condition() {
        // c109 Phase 22: `if x == value(b) { … b … }` lowers to `if let Some(b) = …`.
        let src = "fn f(x: Int?) {\n if x == value(n) {\n print(\"{n}\")\n }\n}\n";
        assert!(covers(src, "f"));
        // `x == null` lowers to `.is_none()`.
        let isnone = "fn f(x: Int?) {\n if x == null {\n print(\"none\")\n }\n}\n";
        assert!(covers(isnone, "f"));
    }

    #[test]
    fn covers_user_enum_variant_if_let_condition() {
        // c109 (B4): `if m == Ping(n) { … } else { … }` over a covered user enum lowers
        // to `if let user_Msg::user_Ping(user_n) = m`. Single-payload variant (one bind).
        let src = "enum Msg { Ping(Int) Pong }\nfn f(m: Msg) -> Int {\n if m == Ping(n) {\n return n\n } else {\n return -1\n }\n}\n";
        assert!(covers(src, "f"));
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
        // `is_intercepted_method_name` flags every collection/string/special builtin
        // name (`len`, `push`, `map`, …). It still guards the STATIC call-site shape
        // (`static_method_call_in_subset`). For an INSTANCE method, the user-method gate
        // now keys on a real `method_sigs` entry instead (the builtin-name-collision fix —
        // see `covers_user_method_shadowing_builtin_name`), so a user instance method
        // SHADOWING a builtin name routes to the user method on both paths. The predicate
        // contents are unchanged; assert them.
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

    #[test]
    fn covers_user_method_shadowing_builtin_name() {
        // c109 (builtin-name collision): a user instance method whose name collides with a
        // builtin (`get`/`len`) now routes through the TIR when a real `method_sigs` entry
        // exists. The AST `emit_method_call` dispatches such a call to `user_<method>` BEFORE
        // `emit_builtin_method` (the fix), so the gate admits it. `recv_type` is a sema fact
        // (`build_cx` alone leaves the call node's `recv_type` empty), so we drive
        // `method_call_in_subset` directly with a synthetic `Some("Bag")` receiver — exactly
        // the node sema produces. (The end-to-end build+run + byte-parity in tests/tir.rs is
        // the authoritative proof; this exercises the gate's user-vs-builtin decision.)
        let src = "struct Bag {\n items: [Int]\n fn get(self) -> Int {\n return 1\n }\n fn len(self) -> Int {\n return 2\n }\n}\n";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        let cx = build_cx(&prog, src, "test.jet");
        let sp = crate::Diagnostics::Span { start: 0, end: 0 };
        let mk = |method: &str| Expr::MethodCall {
            receiver: Box::new(Expr::Ident("b".to_string(), sp)),
            method: method.to_string(),
            method_span: sp,
            args: Vec::new(),
            recv_type: Some("Bag".to_string()),
            resolved_ret: None,
        };
        let mut locals = HashSet::new();
        locals.insert("b".to_string());
        // Both builtin-name user methods are admitted (a real `method_sigs` entry exists).
        for m in ["get", "len"] {
            if let Expr::MethodCall { receiver, method, args, recv_type, .. } = &mk(m) {
                assert!(
                    method_call_in_subset(receiver, method, args, recv_type, &cx, &locals),
                    "user method `{m}` shadowing a builtin name should be covered"
                );
            }
        }
        // A builtin name with NO user method on the type stays excluded (`push` isn't a
        // method on `Bag`), so the builtin/name-keyed path keeps it on the AST side.
        if let Expr::MethodCall { receiver, method, args, recv_type, .. } = &mk("push") {
            assert!(
                !method_call_in_subset(receiver, method, args, recv_type, &cx, &locals),
                "a builtin name with no user method must stay excluded"
            );
        }
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
    fn covers_self_reassignment_method() {
        // D-MUTSELF1: a `mut self` method that reassigns `self` (`self = …`) is NOW
        // covered — the `mut self` slot derefs (`(*self)`), so the LHS lowers to
        // `(*self) = …` (the prior AST-path I2 hole is closed).
        let src = "struct Acc {\n n: Int\n fn reset(mut self) {\n self = Acc { n: 0 }\n }\n}\n";
        assert!(covers_method(src, "Acc", "reset"));
    }

    #[test]
    fn covers_self_field_assign_method() {
        // D-MUTSELF1: a `mut self` method assigning a field (`self.field = v`, S17
        // compound `+=` too) is covered — lowers to `((*self)).field = v`.
        let src = "struct Acc {\n n: Int\n fn bump(mut self) {\n self.n = self.n + 1\n }\n}\n";
        assert!(covers_method(src, "Acc", "bump"));
        let compound = "struct Acc {\n n: Int\n fn bump(mut self) {\n self.n += 1\n }\n}\n";
        assert!(covers_method(compound, "Acc", "bump"));
    }

    #[test]
    fn rejects_generic_method() {
        // c109 Phase 19: a method on a GENERIC struct (`impl<T> user_<T>`) is the deferred
        // "generic-type method" surface — `struct_is_generic` excludes it even though the
        // owning struct is now a covered VALUE type (turbofish construction is covered, but
        // the method's `impl<T>` clause is not yet validated across every shape).
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
    fn covers_or_fallback_panic_form() {
        // c109 Phase 15: the `panic(…)` fallback form is now covered — the
        // `safe_locals_expr` snapshot is reproduced from the `panic_locals` replica.
        let src = "fn p(x: (Int?)) -> Int {\n return x ?? panic(\"missing\")\n}\n";
        assert!(covers(src, "p"));
    }

    #[test]
    fn covers_comptime_if() {
        // c109 Phase 15: a resolved comptime-if routes through the TIR — only the
        // selected branch's statements are emitted inline. (`build_cx`-only gate test:
        // the gate's `stmt_in_subset` admits `Stmt::ComptimeIf` unconditionally; the
        // lowering reads `selected_then`, but the gate does not need sema for routing.)
        let src = "fn f(x: Int) -> Int {\n comptime if true {\n return x\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_mixed_bool_switch() {
        // c109 Phase 15: a mixed comparison/Bool `when` switch (shape D) routes via the
        // TIR's `MixedSwitch` (the general `emit_mixed_switch` if/else chain).
        let src = "fn f(x: Int) -> Int {\n if x {\n x > 10 -> {\n return 2\n }\n x > 0 -> {\n return 1\n }\n else -> {\n return 0\n }\n }\n}\n";
        assert!(covers(src, "f"));
    }

    // c109 Phase 9: built-in collection/string methods. A builtin call has
    // `recv_type == None` (parser default; sema leaves it None for non-numeric
    // builtins), so `build_cx` alone proves the gate's builtin shape.

    #[test]
    fn covers_list_builtin_methods() {
        // push/len/get/sort/reverse/contains on a list-typed param — all covered,
        // so the whole function routes through the TIR.
        let src = "fn f(xs: [Int]) -> Int {\n ys := xs\n ys.push(1)\n ys.reverse()\n ys.sort()\n n := ys.len()\n c := ys.contains(3)\n return n\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_map_builtin_methods() {
        // insert/get/keys/values/contains_key/clear on a map-typed param.
        let src = "fn f(m: [String, Int]) -> Int {\n m2 := m\n m2.insert(\"k\", 1)\n n := m2.len()\n ks := m2.keys()\n vs := m2.values()\n ck := m2.contains_key(\"a\")\n m2.clear()\n return n\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_string_builtin_methods() {
        // to_upper/to_lower/trim/split/starts_with/replace/repeat/slice/chars/bytes.
        let src = "fn f(s: String) -> String {\n up := s.to_upper()\n tr := s.trim()\n sp := s.split(\",\")\n sw := s.starts_with(\"a\")\n rp := s.replace(\"a\", \"b\")\n rep := s.repeat(2)\n sl := s.slice(0, 2)\n ch := s.chars()\n by := s.bytes()\n return up\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_closure_builtin_method() {
        // A closure-taking builtin (`map`/`filter`/…) is deferred to the lambda
        // phase — `is_covered_builtin_name` returns false, and the lambda arg is
        // out-of-subset anyway. The owning function stays on the AST path.
        for name in ["map", "filter", "each", "find", "any", "all", "sort_by", "reduce"] {
            assert!(
                !is_covered_builtin_name(name, 1),
                "{name} (closure method) must NOT be a covered builtin"
            );
        }
    }

    #[test]
    fn covers_is_empty_builtin() {
        // `is_empty` is now Bool-typed (c109 fix) and covered (`TBuiltinOp::IsEmpty`);
        // a function using it routes through the TIR.
        assert!(is_covered_builtin_name("is_empty", 0));
        let src = "fn f(xs: [Int]) -> Int {\n e := xs.is_empty()\n if e {\n return 1\n }\n return 0\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_numeric_conversion_builtin() {
        // Numeric width/predicate/bit methods (`to_i32`/`is_nan`/`count_ones`) are
        // Phase 12 — not covered builtins here, and they carry a `Some(recv_type)`.
        for name in ["to_i32", "to_u8", "is_nan", "count_ones", "to_f64"] {
            assert!(!is_covered_builtin_name(name, 0), "{name} is a Phase-12 numeric method");
        }
    }

    #[test]
    fn covers_string_payload_error_enum() {
        // c109 Phase 16: a `T ? E` whose error enum has a String payload is now
        // covered — the error enum is a covered (String-payload) enum, and its
        // construction (`err(Oops.Msg("bad"))`) reproduces `emit_boxed_enum_arg`
        // (a String literal arg, no borrowed clone) byte-for-byte.
        let src = "enum Oops {\n Msg(String)\n}\nfn f(x: Int) -> Int ? Oops {\n if x == 0 {\n return err(Oops.Msg(\"bad\"))\n }\n return ok(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_fn_typed_param() {
        // c109 Phase 13: a fn-typed parameter is now inside the subset (was excluded
        // through Phase 12, when any callee/param with a `Type::Fn` stayed on the AST
        // path). The body `f(f(x))` is a fn-value call through the local param.
        let src = "fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int {\n return f(f(x))\n}\n";
        assert!(covers(src, "apply_twice"));
    }

    #[test]
    fn covers_fn_name_value_arg() {
        // c109 Phase 13: a bare top-level fn name used as a VALUE (passed to a
        // fn-typed param) is in subset — it emits `emit_named_fn_value`'s
        // `Box::new(move |…| …) as <fn-type>` wrapper.
        let src = "fn callit(f: fn(Int) -> Int) -> Int {\n return f(1)\n}\nfn dbl(x: Int) -> Int {\n return (x * 2)\n}\nfn use_it() -> Int {\n return callit(dbl)\n}\n";
        assert!(covers(src, "use_it"));
    }

    #[test]
    fn handle_method_op_table() {
        // c109 Phase 13: the covered handle-method set, and the excluded ones.
        assert!(handle_method_op("FileReader", "read_line", 0).is_some());
        assert!(handle_method_op("FileWriter", "write_line", 1).is_some());
        assert!(handle_method_op("FileWriter", "flush", 0).is_some());
        assert!(handle_method_op("TcpStream", "read", 0).is_some());
        assert!(handle_method_op("TcpStream", "close", 0).is_some());
        assert!(handle_method_op("TcpListener", "accept", 0).is_some());
        // c109 Phase 19: the arena allocator methods (`alloc`/`reset`/`free`) are now
        // covered (the producer `mem.Arena.new()` is covered too).
        assert!(handle_method_op("Arena", "alloc", 1).is_some());
        assert!(handle_method_op("Bump", "reset", 0).is_some());
        assert!(handle_method_op("Pool", "free", 0).is_some());
        // c109 Phase 20: HttpRequest/HttpResponse accessors are now covered (the
        // `http.serve` lambda-param type is written back onto `p.ty`, so the slot
        // type is total and the AST `rty`-keyed handle arm fires identically).
        assert!(handle_method_op("HttpRequest", "method", 0).is_some());
        assert!(handle_method_op("HttpRequest", "path", 0).is_some());
        assert!(handle_method_op("HttpRequest", "header", 1).is_some());
        assert!(handle_method_op("HttpRequest", "param", 1).is_some());
        assert!(handle_method_op("HttpResponse", "status", 0).is_some());
        assert!(handle_method_op("HttpResponse", "body", 0).is_some());
        // Excluded: dead `lines` (E2502).
        assert!(handle_method_op("FileReader", "lines", 0).is_none());
        // Wrong arity declines.
        assert!(handle_method_op("FileWriter", "write_line", 0).is_none());
    }

    #[test]
    fn polymorphic_core_specials_covered() {
        // c109 Phase 20: the polymorphic core specials route through the core-call
        // shape (`core_call_covered`), their return type read from the node's
        // `resolved_ret` (written by sema). `io.input` is NOT a polymorphic special —
        // its fixed `Result<String, IOError>` return rides `core_call_return_ty`
        // (c109 Phase 29; it is NOT in `core_fixed_sig`).
        assert!(core_call_covered("core.math", "abs"));
        assert!(core_call_covered("core.math", "min"));
        assert!(core_call_covered("core.math", "max"));
        assert!(core_call_covered("core.math", "clamp"));
        assert!(core_call_covered("core.random", "pick"));
        assert!(core_call_covered("core.random", "shuffle"));
        assert!(core_call_covered("core.io", "eprint"));
        // c109 Phase 21: the `tasks.channel()` producer is now covered via the core-call
        // shape (a fixed-string `JetChannel::new()` emit; its `Channel<T>` return type
        // rides on the binding annotation, not the node). `tasks.spawn` stays out of this
        // shape — it has its own bespoke `CoreClosureCall` shape (a `move |…|` closure).
        assert!(core_call_covered("core.tasks", "channel"));
        assert!(!core_call_covered("core.tasks", "spawn"));
        // c109 Phase 25: the HttpRouter producer + parse/dispatch core calls are covered
        // (fixed-string emits; their return types live in sema's `infer_core_call`, not
        // `core_fixed_sig`). `http.serve` stays out (closure-taking → `CoreClosureCall`).
        assert!(core_call_covered("jet.http", "router"));
        assert!(core_call_covered("jet.http", "parse"));
        assert!(core_call_covered("jet.http", "dispatch"));
        assert!(!core_call_covered("jet.http", "serve"));
        // c109 Phase 29: qualified `io.input` is a covered core call. NOT in
        // `core_fixed_sig` (its `Result<String, IOError>` return lives in sema's bespoke
        // `infer_core_call` arm, reproduced in `core_call_return_ty`). Distinct from the
        // ambient bare `input()` (Phase 25), which is its own `Expr::Call` → `AmbientInput`.
        assert!(core_call_covered("core.io", "input"));
        assert!(!crate::Sema::core_fixed_sig("core.io", "input").is_some());
    }

    #[test]
    fn io_input_return_ty() {
        // c109 Phase 29: `core_call_return_ty` carries `io.input`'s fixed
        // `Result<String, IOError>` total (it is NOT in `core_fixed_sig`, so without this
        // arm the node's `ty` would fall back to Unit and break `?? return` composition).
        let ty = core_call_return_ty("core.io", "input");
        match ty {
            Type::Result { ok, err } => {
                assert_eq!(*ok, Type::String);
                assert_eq!(*err, Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()));
            }
            other => panic!("io.input return ty should be Result<String, IOError>, got {other:?}"),
        }
    }

    #[test]
    fn covers_static_new_constructor() {
        // c109 Phase 25: a STATIC constructor `Rect.new(...)` routes (the Phase-7 static
        // shape — `recv_type == None`, type-name receiver, `(Rect, "new") ∈ method_sigs`),
        // even though `new` is in `is_intercepted_method_name` (which guards the INSTANCE
        // shape). `build_cx`-only: the static call carries `recv_type == None` by default.
        let src = "\
struct Rect { width: Int height: Int }
impl Rect {
    fn new(width: Int, height: Int) -> Rect { return Rect{width: width, height: height} }
}
fn build() -> Rect { return Rect.new(4, 3) }
";
        assert!(covers(src, "build"));
        // The instance-method intercept stays whole: a user INSTANCE method named `new`
        // is still excluded (it stays on the AST path).
        assert!(is_intercepted_method_name("new"));
    }

    #[test]
    fn covers_ambient_input() {
        // c109 Phase 25: the ambient prelude `input(...)` routes (bare call, no user
        // `input` fn). It composes with the `??` value fallback (Phase 8).
        let src = "\
fn greet() -> String {
    name @= input() ?? \"world\"
    return \"hi {name}\"
}
";
        assert!(covers(src, "greet"));
        // A user-defined `input` fn shadows the prelude — the gate then treats `input(...)`
        // as a plain fn call (still covered, but via the plain-fn shape, not ambient).
        let shadowed = "\
fn input() -> String { return \"x\" }
fn greet() -> String { return input() }
";
        assert!(covers(shadowed, "greet"));
    }

    #[test]
    fn covers_require_builtins() {
        // c109 Phase 26: the rich-runtime-report builtins `require`/`require_eq`/`panic`
        // (S36) route. Each is a bare `Expr::Call` whose name is the builtin (not a user
        // fn / local) with the right arity; the whole emit string is rendered at lowering.
        assert!(covers("fn f() { require((1 + 1) == 2) }", "f"));
        assert!(covers("fn f() { require(false, \"nope\") }", "f"));
        assert!(covers("fn f() { require_eq(2, 2) }", "f"));
        assert!(covers("fn f() { panic(\"stop\") }", "f"));
        // A user fn / local named `require` shadows the builtin — it then routes via the
        // plain-fn shape, NOT the builtin (still covered, different path).
        assert!(covers(
            "fn require(x: Int) -> Int { return x }\nfn f() -> Int { return require(3) }",
            "f"
        ));
    }

    #[test]
    fn covers_caps_block() {
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region erases to a plain
        // block (byte-for-byte `Stmt::Region`); its body is checked on the SAME locals, so
        // an out-of-subset body keeps the whole fn off the TIR path.
        assert!(covers("fn f() { #Caps(Io) { print(\"x\") } }", "f"));
        // c109: a single-uppercase-letter DECLARED struct name (`P`) is a concrete
        // type, not a type variable — the `is_type_var_name` heuristic is now guarded
        // on non-declaration (`cx.struct_fields` lookup). So `P{x: 1}` and the
        // `P{x} @= p` struct-destructure are both covered; the fn routes through TIR.
        assert!(covers(
            "struct P { x: Int }\nfn f() { p @= P{x: 1}\n#Caps(Io) { P{x} @= p\nprint(x) } }",
            "f"
        ));
    }

    #[test]
    fn covers_free_call_arg_conventions() {
        // c109 Phase 26: ALL three free-call arg conventions route — `Read` (`&(…)`),
        // `Move` (`take`-marked), and `Mutate` (`mut place` → `&mut (…)`).
        assert!(covers(
            "fn bump(mut n: Int) { n += 1 }\nfn f() { s: Int := 1\nbump(mut s) }",
            "f"
        ));
        assert!(covers(
            "fn keep(take s: String) -> String { return s }\nfn f() -> String { return keep(take \"v\") }",
            "f"
        ));
    }

    #[test]
    fn covers_list_destructure() {
        // c109 Phase 26: a list-destructuring binding `[a, b, c] @= <init>` (S74) routes
        // when the init is in-subset — the fan-out result destructure (`41_fan_out`).
        assert!(covers(
            "fn f() { xs @= [1, 2, 3]\n[a, b, c] @= xs\nprint(a) }",
            "f"
        ));
    }

    #[test]
    fn covers_struct_destructure() {
        // c109: a struct-destructuring binding `Type { x, y } @= <init>` (S74) routes
        // when the init is in-subset — the AST `BindPattern::Struct` arm is covered
        // byte-for-byte (per-field type from `cx.struct_fields`).
        assert!(covers(
            "struct Point { x: Int, y: Int }\nfn f() { p @= Point { x: 1, y: 2 }\nPoint { x, y } @= p\nprint(x + y) }",
            "f"
        ));
    }

    #[test]
    fn covers_named_fn_value_binding() {
        // c109 Phase 27: a bare top-level fn name bound to a local as a VALUE
        // (`double_fn @= double`). The init `Ident("double")` resolves to a `Type::Fn`
        // value (`emit_named_fn_value`), in-subset via `ident_is_named_fn_value`. (This
        // binding-site coercion was already wired in lowering; the live-suite `24_callbacks`
        // never routed only because the struct fn-field / fn-field-call were uncovered.)
        assert!(covers(
            "fn dbl(x: Int) -> Int { return (x * 2) }\nfn f() { g @= dbl\nprint(g(3)) }",
            "f"
        ));
    }

    #[test]
    fn covers_fn_field_struct_value_type() {
        // c109 Phase 27: a struct with a FUNCTION-typed field is a covered VALUE type — the
        // fn-typed field renders to `Box<dyn Fn(...)>` and needs no clone/deref decision at
        // the field site (sema-independent — `build_cx` populates `struct_fields`). (The
        // full construction + `w.step(4)` fn-field CALL is sema-dependent — `recv_type ==
        // Some("Worker")` is a sema fact — so it is proven by tests/tir.rs + byte-parity.)
        let src = "struct Worker { step: fn(Int) -> Int }\nfn f() {}";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, src, "test.jet");
        assert!(is_covered_struct_ty(&Type::Named("Worker".to_string()), &cx));
        // The fn-field-call shape resolves the field's Fn type from a covered struct's
        // `struct_fields` (the `recv_type` half is the sema fact tests/tir.rs supplies).
        assert!(fn_field_call_ty("step", &Some("Worker".to_string()), &cx).is_some());
        // A non-existent / non-Fn field is not a fn-field call.
        assert!(fn_field_call_ty("missing", &Some("Worker".to_string()), &cx).is_none());
    }

    #[test]
    fn covers_fn_field_type_covered() {
        // c109 Phase 27: `field_ty_covered` admits a `Type::Fn` field directly.
        let src = "fn f() {}";
        let (toks, _) = crate::Lexer::lex(src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, src, "test.jet");
        let fn_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Some(Box::new(Type::Int)),
        };
        assert!(field_ty_covered(&fn_ty, &cx, &mut HashSet::new()));
    }

    #[test]
    fn concurrency_method_names() {
        // c109 Phase 21: the Task/Channel/Sender method name+arity set. `join` is the
        // 0-arg form (the 1-arg list `join(sep)` is a collection builtin, NOT here);
        // `send` is the 1-arg form.
        assert!(is_concurrency_method_name("join", 0));
        assert!(is_concurrency_method_name("detach", 0));
        assert!(is_concurrency_method_name("receive", 0));
        assert!(is_concurrency_method_name("sender", 0));
        assert!(is_concurrency_method_name("send", 1));
        // Disjoint from the list `join(sep)` (1 arg) and any wrong arity.
        assert!(!is_concurrency_method_name("join", 1));
        assert!(!is_concurrency_method_name("send", 0));
        assert!(!is_concurrency_method_name("receive", 1));
        assert!(!is_concurrency_method_name("len", 0));
    }

    #[test]
    fn concurrency_value_types_covered() {
        // c109 Phase 21: `Task<T>`/`Channel<T>`/`Sender<T>` are covered value types; the
        // `Closed` err type is a covered fallible payload (`Channel.receive()`).
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let apply = |n: &str| Type::Apply {
            name: n.to_string(),
            args: vec![Type::Int],
        };
        assert!(is_covered_concurrency_ty(&apply("Task"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Channel"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Sender"), &cx));
        assert!(is_subset_param_ty(&apply("Task"), &cx));
        // A `[Task<Unit>]` worker list (34_parallel_scan) is a covered collection.
        let tasks = Type::List(Box::new(Type::Apply {
            name: "Task".to_string(),
            args: vec![unit_type()],
        }));
        assert!(is_covered_collection_ty(&tasks, &cx));
        // `Closed` is a covered fallible payload (the `receive()` err type).
        assert!(fallible_payload_covered(&Type::Named("Closed".to_string()), &cx));
        // A non-concurrency `Apply` (e.g. a user generic) is NOT this shape.
        assert!(!is_covered_concurrency_ty(&apply("Pair"), &cx));
    }

    #[test]
    fn covers_concurrency_methods() {
        // c109 Phase 21: a function using the channel `sender`/`send`/`receive` surface +
        // the `tasks.channel()` producer routes. The gate is `build_cx`-only (no sema), so
        // the method calls carry `recv_type == None` (the unannotated AST default), which
        // is exactly what the d3 shape keys on; the `Channel<Int>` annotation supplies the
        // value type. (The `tasks.spawn(take(..) …)`/`Task.join` slice depends on
        // sema-filled `Lambda.meta`, so it's proven end-to-end in tests/tir.rs.)
        let src = "\
use core.tasks as tasks
fn produce(s: Sender<Int>) {
    s.send(7)
}
fn consume(ch: Channel<Int>) -> Int {
    return ch.receive() ?? panic(\"closed\")
}
";
        // The `Sender.send` method + `Sender<Int>` value type (gate shape d3).
        assert!(covers(src, "produce"));
        // The `Channel.receive` method + `Channel<Int>` value type + `Result<Int, Closed>`
        // unwrap via `?? panic`.
        assert!(covers(src, "consume"));
    }

    #[test]
    fn covers_pure_fn() {
        // c109 Phase 23: a `#Pure fn` is covered (purity is sema-only, erased at codegen).
        assert!(covers("#Pure fn double(n: Int) -> Int {\n return (n * 2)\n}\n", "double"));
    }

    #[test]
    fn covers_todo_hole() {
        // c109 Phase 23: a `#Todo` hole is covered (diverging `todo!`). The build_cx-only
        // helper leaves `expected_type` unset (sema fills it), but the gate admits a
        // None-typed hole too (it lowers to the `(unknown)` fallback — never reached here
        // since this is a structural gate test). Reproduce the sema fact: a hole with an
        // expected type. We can't run sema in this helper, so just assert the simpler
        // surrounding fn is covered — the end-to-end `todo_hole` test proves the emit.
        // (A bare `#Todo` body with no sema annotation has `expected_type: None`, which the
        // gate EXCLUDES — so we assert exclusion here, matching the conservative rule.)
        assert!(!covers("fn f(n: Int) -> Int {\n return #Todo\n}\n", "f"));
    }

    #[test]
    fn covers_default_params() {
        // c109 Phase 23: a fn with default param values is covered (defaults are filled at
        // call sites by sema; codegen never reads `p.default`).
        assert!(covers(
            "fn box_dims(w: Int, h: Int = w, d: Int = h) -> String {\n return \"{w}{h}{d}\"\n}\n",
            "box_dims"
        ));
    }

    #[test]
    fn covers_distinct_value_type_and_ctor() {
        // c109 Phase 23: a distinct param type + `.raw()` + the `Name(x)` constructor are
        // covered. The build_cx-only helper registers the distinct in `distinct_types`.
        let src = "UserId @= distinct Int;\nfn greet(id: UserId) -> Int {\n return (id.raw())\n}\n";
        assert!(covers(src, "greet"));
        let src2 = "UserId @= distinct Int;\nfn mk() -> UserId {\n return UserId(42)\n}\n";
        assert!(covers(src2, "mk"));
    }

    #[test]
    fn covers_tuple_value_type() {
        // c109 Phase 23: a tuple PARAM type (`(x: Int, y: Int)`) is a covered value type.
        // A field read on it is the generic `Field` shape. (A tuple LITERAL needs sema's
        // `Expr::TupleLit.ty` to resolve the canonical field order/struct name, which the
        // build_cx-only helper does not fill — so the literal + destructure are proven by
        // the end-to-end `named_tuples` test, not here.)
        let src = "fn first(p: (x: Int, y: Int)) -> Int {\n return p.x\n}\n";
        assert!(covers(src, "first"));
    }

    #[test]
    fn covers_named_args_at_call_site() {
        // c109 Phase 23: a call-site label is allowed (labels never reorder; codegen
        // ignores them). The callee `area` is a plain fn; the labeled call is in-subset.
        let src = "fn area(width: Int, height: Int) -> Int {\n return (width * height)\n}\nfn use_it() -> Int {\n return area(width: 4, height: 3)\n}\n";
        assert!(covers(src, "use_it"));
    }

    #[test]
    fn covers_default_param_method() {
        // c109 Phase 23: a struct-body method with a default param value (`clamp: Bool =
        // false`) is covered (same call-site-fill rule as a free fn; codegen never reads
        // `p.default`).
        let src = "struct Rect {\n w: Int\n fn scale(self, factor: Int, clamp: Bool = false) -> Int {\n return (self.w * factor)\n }\n}\n";
        assert!(covers_method(src, "Rect", "scale"));
    }

    #[test]
    fn core_closure_calls_covered() {
        // c109 Phase 13: the three closure-taking core calls are covered with a
        // literal in-subset lambda; the polymorphic specials stay deferred.
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let locals = HashSet::new();
        let lam = |body: &str| -> Vec<crate::AST::CallArg> {
            let s = format!("fn g() {{ x @= scope.guard({})\n}}\n", body);
            let (t, _) = crate::Lexer::lex(&s);
            let p = crate::Parser::parse(&t).expect("parse lam");
            // Pull the single call arg from the guard call.
            for item in &p.items {
                if let crate::AST::Item::Func(f) = item {
                    for st in &f.body {
                        if let Stmt::Val(b) = st {
                            if let Expr::MethodCall { args, .. } = &b.init {
                                return args.clone();
                            }
                        }
                    }
                }
            }
            Vec::new()
        };
        let guard_args = lam("() => { print(\"x\") }");
        assert!(core_closure_call_in_subset(
            "core.scope", "guard", &guard_args, &cx, &locals
        ));
        // A non-closure core call is not a closure-core-call.
        assert!(!core_closure_call_in_subset(
            "core.fs", "read", &guard_args, &cx, &locals
        ));
    }

    #[test]
    fn covers_json_construction_and_collection() {
        // c109 Phase 24: JSON construction (`JSON.Text`/`JSON.Boolean`/`JSON.Array`/
        // `JSON.Null`) + a `[JSON]` list value type. A fn that builds JSON values and
        // returns a JSON routes (the JSON value type is a covered foreign value type;
        // construction is the `JsonLit` shape). The if-let JSON MATCHING + index-assign
        // need full sema (the JSON pattern / `IndexKind`), proven by `tests/tir.rs` + the
        // whole-suite byte-parity diff; here we gate the sema-independent construction.
        let src = "\
fn build() -> JSON {
    items: [JSON] := []
    items.push(JSON.Text(\"jet\"))
    items.push(JSON.Boolean(true))
    items.push(JSON.Null)
    return JSON.Array(items)
}
";
        assert!(covers(src, "build"));
    }

    #[test]
    fn covers_json_value_param_and_array() {
        // A JSON-typed param + a `[JSON]` list value type + `JSON.Array` construction.
        let src = "\
fn wrap(x: JSON) -> JSON {
    items: [JSON] := []
    items.push(x)
    return JSON.Array(items)
}
";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_enum_field_struct() {
        // c109 Phase 24: a struct with an ENUM field (`note_type: NoteType`) is now a
        // covered struct — `field_ty_covered` admits a covered enum field. So a fn that
        // takes/reads such a struct routes (previously the enum field excluded the struct).
        let src = "\
enum NoteType { User Feedback }
struct Note {
    name: String
    note_type: NoteType
}
fn name_of(n: Note) -> String {
    return n.name
}
";
        assert!(covers(src, "name_of"));
    }

    #[test]
    fn covers_local_enum_with_foreign_payload_value_type() {
        // c109 Phase 24: a local enum whose variant payload is itself a covered enum is
        // covered (`enum_payload_ty_covered` admits a covered enum). (A FOREIGN-enum
        // payload needs the cross-module `foreign_types` table, proven by `tests/tir.rs`;
        // here a LOCAL nested enum exercises the same payload-covered path.)
        let src = "\
enum Kind { A B }
enum Query {
    Tag(String)
    OfKind(Kind)
}
fn mk(k: Kind) -> Query {
    return Query.OfKind(k)
}
";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn covers_comptime_const_in_interpolation() {
        // c109 Phase 24: a comptime const inlines its value at the use site
        // (`cx.consts`), so a fn interpolating a const routes.
        let src = "\
comptime HEADER = \"<html>\"
fn wrap(s: String) -> String {
    return \"{HEADER}: {s}\"
}
";
        assert!(covers(src, "wrap"));
    }

    #[test]
    fn covers_comptime_local_binding() {
        // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr` in a function body
        // routes once sema fills `b.ct`. The runtime `init` (`build()`) is NOT in-subset
        // on its own merits, but the comptime path never emits it — it emits the
        // sema-evaluated literal — so the gate admits it on `b.ct.is_some()`. Needs the
        // full sema pass, hence `covers_after_sema`.
        let src = "\
fn build() -> List<Int> {
    xs: List<Int> := []
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn main() {
    comptime xs = build()
    print(\"{xs}\")
}
";
        assert!(covers_after_sema(src, "main"));
    }

    #[test]
    fn covers_shared_auto_clone_in_free_call_arg() {
        // c109 Phase 6b: a fn with a `Shared<T>` param (`is_covered_shared_ty`) passing
        // that handle to a FREE call inside a loop — sema sets `a.flags.shared_auto_clone`
        // (auto-clone across the loop boundary) — now routes. The gate admits the Arc form
        // on the plain-`Call` path (it lowers via `lower_one_call_arg`'s `arc_clone`). Both
        // `noop` (a `Shared<T>` param) and `loop_user` (the auto-clone call site) are
        // covered. Needs the full sema pass (the flag is sema-resolved), hence
        // `covers_after_sema`.
        let src = "\
fn noop(h: Shared<Int>) {
    print(0)
}
fn loop_user(h: Shared<Int>) {
    loop {
        noop(h)
    }
}
fn main() {
    print(0)
}
";
        assert!(covers_after_sema(src, "noop"));
        assert!(covers_after_sema(src, "loop_user"));
    }

    #[test]
    fn covers_optional_struct_field() {
        // c109 Phase 24: a struct with an OPTIONAL field (`note: String?`) is now covered
        // (`field_ty_covered` admits a covered-payload Option). A fn building it routes.
        let src = "\
struct PR {
    file_path: String
    note: String?
}
fn mk(p: String) -> PR {
    return PR {file_path: p, note: null}
}
";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn covers_numeric_bounds_const() {
        // c109 Phase 28: per-type bounds constants reach codegen as a `Field` whose
        // receiver is a numeric type NAME (`U8.MAX`, `I32.MIN`, `Float.INFINITY`).
        // Gated structurally (numeric type name + a known const member), no sema fact.
        let src = "\
fn bounds() {
    print(U8.MAX)
    print(I32.MIN)
    print(Float.INFINITY)
}
";
        assert!(covers(src, "bounds"));
    }

    #[test]
    fn rejects_unknown_numeric_member() {
        // A numeric type name with a NON-bounds member is NOT a bounds const — it
        // stays excluded (a non-local non-enum ident receiver), so the fn stays on
        // the AST path. (Sema would reject it too; the gate is conservative.)
        let src = "\
fn bad() {
    print(U8.NOPE)
}
";
        assert!(!covers(src, "bad"));
    }

    #[test]
    fn covers_overflow_opt_builtins() {
        // c109 Phase 28: the overflow opt-outs `wrapping(e)`/`saturating(e)`/
        // `checked(e)` over an integer `Expr::Binary`. Gated structurally (the
        // builtin name + a `+`/`-`/`*`/`/` Binary arg), no sema fact required.
        let src = "\
fn ops(a: U8, b: U8) {
    print(wrapping(a + b))
    print(saturating(a * b))
}
";
        assert!(covers(src, "ops"));
    }

    #[test]
    fn rejects_overflow_opt_nonbinary() {
        // `wrapping(x)` whose argument is NOT an integer `Expr::Binary` is not the
        // covered shape — the gate excludes it (sema never produces it, but the gate
        // stays strict).
        let src = "\
fn nope(x: U8) {
    print(wrapping(x))
}
";
        assert!(!covers(src, "nope"));
    }

    #[test]
    fn covers_generic_optional_return() {
        // c109 Phase 30: a generic fn with a `T?` return whose payload is a type var
        // (`largest<T: Comparable>() -> (T?)`). Before Phase 30 the `T?` payload was
        // excluded (`fallible_payload_covered` admitted no type var) — now it routes.
        // Body is a structural `value(x)` (a type-var payload `Some(user_x)`).
        let src = "\
fn opt_id<T: Comparable>(x: T) -> (T?) {
    return value(x)
}
";
        assert!(covers(src, "opt_id"));
    }

    #[test]
    fn rejects_optional_return_uncovered_payload() {
        // A `T?` return whose payload is an UNcovered type (a trait object is not a
        // fallible payload) stays excluded — the type-var admission is narrow.
        let src = "\
trait Shape {
    fn area(self) -> Float
}
fn maybe_shape(s: Shape) -> (Shape?) {
    return value(s)
}
";
        assert!(!covers(src, "maybe_shape"));
    }

    #[test]
    fn covers_trait_object_param() {
        // c109 Phase 30: a TRAIT-OBJECT param (`s: Shape` → `&Box<dyn user_Shape>`). The
        // param type is admitted (`is_covered_trait_object_ty`); a body with no method
        // call is structurally in-subset (the dynamic-dispatch shape needs sema's
        // `recv_type`, proven by tests/tir.rs + parity). An empty body covers.
        let src = "\
trait Shape {
    fn area(self) -> Float
    fn name(self) -> String
}
fn takes_shape(s: Shape) {
}
";
        assert!(covers(src, "takes_shape"));
    }

    #[test]
    fn covers_trait_object_list_param() {
        // c109 Phase 30: a `[Shape]` trait-object list is a covered collection
        // (`collection_elem_covered` admits a trait-object element). A fn taking one,
        // with no body construct beyond the param, routes.
        let src = "\
trait Shape {
    fn area(self) -> Float
}
fn takes_shapes(xs: [Shape]) {
}
";
        assert!(covers(src, "takes_shapes"));
    }

    #[test]
    fn covers_view_returning_trait_method() {
        // c109 (view-trait fix): a `view`-returning trait method `fn label(self) ->
        // view String` is now COVERED. It was excluded while `emit_trait_def` rendered
        // the trait DECLARATION return as `-> String` (ignoring `is_view_return`) against
        // the impl's `-> &String` → rustc E0053; that's now fixed, so the gate admits it.
        // The borrow shape is the existing total `TStmt::ViewReturn { wrap }` (Phase 17).
        let src = "\
trait Named {
    fn label(self) -> view String
}
struct Dog {
    name: String
}
impl Dog: Named {
    fn label(self) -> view String {
        return self.name
    }
}
";
        assert!(covers_trait_method(src, "Dog", "label"));
    }

    #[test]
    fn rejects_non_trait_object_named() {
        // `is_covered_trait_object_ty` admits only a NAME in `cx.trait_names`. A param
        // typed as an unknown name (no such trait/struct) stays excluded — the gate
        // never wrongly treats a plain name as a trait object.
        let src = "\
fn bad(s: Nonexistent) {
}
";
        assert!(!covers(src, "bad"));
    }

    #[test]
    fn covers_recursive_struct_construction() {
        // c109 (recursive struct): constructing a self-referential (boxed) struct is
        // covered — `struct_lit_constructible` admits the boxed edge and lowering wraps the
        // field value `Box::new(…)`. A fn building a nested `Tree { value, child: value(…) }`
        // routes. (The boxed-field READ is also covered now — see covers_recursive_struct_boxed_field_read.)
        let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn build() {
    root @= Tree { value: 1, child: value(Tree { value: 2, child: null }) }
    print(root.value)
}
";
        assert!(covers(src, "build"));
    }

    #[test]
    fn covers_recursive_struct_boxed_field_read() {
        // c109 (recursive struct read): a boxed (recursive) field READ (`t.child`,
        // Rust type `Box<…>`) is now covered — the read derefs the `Box` (`(*(…))`, a
        // total `boxed` fact lowered from `cx.boxed_edges`), so a recursive struct is a
        // covered VALUE type. A fn reading a boxed child routes through the TIR.
        let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn first_child(t: Tree) -> Int {
    kid: Tree? @= t.child
    if kid {
        kid == value(c) -> {
            return c.value
        }
        kid == null -> {
            return 0
        }
    }
    return 0
}
";
        assert!(covers(src, "first_child"));
    }

    #[test]
    fn covers_owning_nonscalar_field_read_clone() {
        // c109: an owning field read of a NON-SCALAR field (`s @= p.name`, `name:
        // String`) — sema rewrites the read to `(p.name).clone()` (a `MethodCall`
        // clone shape). With the single-uppercase-letter struct name `P` now treated
        // as a concrete declared type (not a type var), the whole `main` routes
        // through the TIR. The owning clone emits `((user_p).user_name).clone()`.
        let src = r#"
struct P {
    name: String
}

fn main() {
    p @= P { name: "x" }
    s @= p.name
    t @= p.name
    print(s)
    print(t)
}
"#;
        assert!(covers_after_sema(src, "main"), "owning field-read clone not covered");
    }

    #[test]
    fn covers_indexed_map_assign_through_field() {
        // c109: an indexed map-assign whose base is a FIELD read (`s.scores["a"] = 1`,
        // `scores: Map<String, Int>`). The `LValue::Index` gate already admits a
        // field-read base + sema-resolved `IndexKind`; the only blocker was the
        // single-uppercase-letter struct name `S` (covered by the type-var-heuristic
        // guard). The whole `main` routes through the TIR; the assign emits
        // `{ let __jet_v = 1i64; jet_map_insert(&mut ((user_s).user_scores), …); }`.
        let src = r#"
struct S {
    scores: Map<String, Int>
}

fn main() {
    s := S { scores: [:] }
    s.scores["a"] = 1
    print(s.scores["a"])
}
"#;
        assert!(covers_after_sema(src, "main"), "map-assign through field not covered");
    }
}
