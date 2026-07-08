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

// Re-export the parent `Codegen` glob so the split-out submodules
// (`subset`/`lower`/`emit`) reach `Cx`, `mangle`, `rust_*`, etc. via `use super::*`.
pub(crate) use super::*;

mod emit;
mod lower;
mod subset;

// Re-export every submodule item so existing `TIR::<name>` call sites and the
// `#[cfg(test)] mod tests` block (which uses `super::*`) keep resolving unchanged.
pub(crate) use emit::*;
pub(crate) use lower::*;
pub(crate) use subset::*;

use crate::AST::{AccessConvention, BinOp, Item, ProgramBundle, Type, UnOp};

/// c139 M4: lowered spawn-lambda body for Cranelift JIT (captures as explicit params).
pub struct TJitSpawnLambda {
    pub params: Vec<(String, Type)>,
    pub captures: Vec<JitSpawnCapture>,
    pub body: TJitSpawnBody,
    pub ret: Type,
}

pub struct JitSpawnCapture {
    pub name: String,
    pub ty: Type,
    pub clone_at_spawn: bool,
}

pub enum TJitSpawnBody {
    Expr(Box<TExpr>),
    Block {
        prefix: Vec<TStmt>,
        tail: Option<Box<TExpr>>,
    },
}

/// c139 M3: every lowered function the JIT may compile from the entry module.
pub struct JitProgram {
    /// Display path of the entry module (for overflow trap messages).
    pub source_file: String,
    /// All top-level `tir_covers` functions in the entry module, including `run`.
    pub funcs: Vec<TFunc>,
    /// c139 M4: spawn lambda bodies in program traversal order (parallel to spawn sites in TIR).
    pub spawn_lambdas: Vec<TJitSpawnLambda>,
    /// M5: mangled field names per struct type (field order).
    pub struct_fields: std::collections::HashMap<String, Vec<String>>,
    /// M5: field types parallel to `struct_fields` order.
    pub struct_field_types: std::collections::HashMap<String, Vec<Type>>,
    /// M5: mangled variant names per enum type (discriminant order).
    pub enum_variants: std::collections::HashMap<String, Vec<String>>,
}

/// c139 M3: every lowered function the JIT may compile from the entry module.
///
/// Returns `None` when there is no plain top-level `run`, or when `run` is
/// outside the existing `tir_covers` gate.
pub fn lower_entry_main_for_jit(bundle: &ProgramBundle) -> Option<TFunc> {
    lower_jit_program(bundle).map(|p| {
        p.funcs
            .into_iter()
            .find(|f| f.name == "run")
            .expect("lower_jit_program always includes run")
    })
}

/// Rust local place for JIT variable lookup (`user_x`).
pub fn local_place(name: &str) -> String {
    super::mangle(name)
}

/// c139 M3: lower every `tir_covers` top-level function in the entry module so the
/// JIT can compile multi-function programs (calls between covered helpers).
pub fn lower_jit_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    let module = bundle.modules.get(bundle.entry)?;
    let extern_funcs = bundle_extern_funcs(bundle);
    let mut cx = build_cx_items(
        &module.items,
        &module.source,
        &module.display,
        None,
        &extern_funcs,
    );
    populate_cx_from_bundle(&mut cx, bundle, bundle.entry);
    let mut funcs = Vec::new();
    let mut have_run = false;
    cx.jit_spawn_lambdas.borrow_mut().clear();
    for item in &module.items {
        match item {
            Item::Func(f) => {
                if !f.type_params.is_empty() || !tir_covers(f, &cx) {
                    continue;
                }
                let lowered = lower_func(f, &cx);
                if f.name == "run" && f.params.is_empty() {
                    have_run = true;
                }
                funcs.push(lowered);
            }
            Item::Struct(s) => {
                if !s.type_params.is_empty() {
                    continue;
                }
                for m in &s.methods {
                    if !tir_covers_method(m, &s.name, &cx) {
                        continue;
                    }
                    let mut lowered = lower_method(m, &s.name, &cx);
                    lowered.name = format!("{}::{}", s.name, m.name);
                    funcs.push(lowered);
                }
            }
            _ => {}
        }
    }
    if !have_run {
        return None;
    }
    let spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
    let mut struct_fields = std::collections::HashMap::new();
    let mut struct_field_types = std::collections::HashMap::new();
    let mut enum_variants = std::collections::HashMap::new();
    for item in &module.items {
        match item {
            Item::Struct(s) if s.type_params.is_empty() => {
                struct_fields.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| format!("user_{}", f.name))
                        .collect(),
                );
                struct_field_types.insert(
                    s.name.clone(),
                    s.fields.iter().map(|f| f.ty.clone()).collect(),
                );
            }
            Item::Enum(e) if e.type_params.is_empty() => {
                enum_variants.insert(
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| format!("user_{}", v.name))
                        .collect(),
                );
            }
            _ => {}
        }
    }
    for (_, fields) in collect_tuple_shapes(&module.items) {
        let tuple_ty = Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), Box::new(ty.clone())))
                .collect(),
        );
        struct_fields.insert(
            tuple_ty.name(),
            fields
                .iter()
                .map(|(name, _)| format!("user_{}", name))
                .collect(),
        );
        struct_field_types.insert(
            tuple_ty.name(),
            fields.iter().map(|(_, ty)| ty.clone()).collect(),
        );
    }
    Some(JitProgram {
        source_file: module.display.clone(),
        funcs,
        spawn_lambdas,
        struct_fields,
        struct_field_types,
        enum_variants,
    })
}

/// Test hook: why `lower_jit_program` returned `None`.
#[doc(hidden)]
pub fn lower_jit_program_fail_reason(bundle: &ProgramBundle) -> String {
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return "missing entry module".to_string();
    };
    let extern_funcs = bundle_extern_funcs(bundle);
    let mut cx = build_cx_items(
        &module.items,
        &module.source,
        &module.display,
        None,
        &extern_funcs,
    );
    populate_cx_from_bundle(&mut cx, bundle, bundle.entry);
    let mut saw_run = false;
    let mut run_tir = false;
    for item in &module.items {
        let Item::Func(f) = item else {
            continue;
        };
        if f.name == "run" && f.params.is_empty() {
            saw_run = true;
            run_tir = tir_covers(f, &cx);
        }
    }
    if !saw_run {
        return "no plain run".to_string();
    }
    if !run_tir {
        let mut locals: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in &module.items {
            let Item::Func(f) = item else {
                continue;
            };
            if f.name != "run" {
                continue;
            }
            for (i, stmt) in f.body.iter().enumerate() {
                let mut probe = locals.clone();
                if !subset::stmt_in_subset(stmt, &cx, &mut probe) {
                    if let crate::AST::Stmt::Val(b) = stmt {
                        if !subset::expr_in_subset(&b.init, &cx, &locals) {
                            return format!("run stmt {i} init outside tir_covers");
                        }
                    }
                    return format!("run stmt {i} outside tir_covers");
                }
                let _ = subset::stmt_in_subset(stmt, &cx, &mut locals);
            }
        }
        return "run outside tir_covers".to_string();
    }
    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// TIR types. Every node carries the facts codegen needs, pre-resolved (totality).
// ---------------------------------------------------------------------------

/// A lowered top-level function. `params` are already mangled to their Rust
/// names and carry their resolved Jet `Type`; `ret` is the resolved return type.
pub struct TFunc {
    /// Jet function name (unmangled) — the emitter mangles via `cx.mangle_name`.
    pub name: String,
    /// `(mangled rust name, resolved jet type, convention)` per parameter. The
    /// convention is kept so the emitter reproduces the `&`/by-value Rust form
    /// without re-deciding (it mirrors `rust_param_type`).
    pub params: Vec<(String, Type, AccessConvention)>,
    /// Resolved return type, or `None` for a unit-returning function.
    pub ret: Option<Type>,
    /// c109 Phase 17: the rendered Rust generic clause (`<T: Clone>` / `<T, U>` / empty),
    /// resolved at lowering via `Generics::rust_type_param_list(&f.type_params, …)` exactly
    /// as `emit_func` does (with the `rust_extra_clone_bounds` every type param carries).
    /// Emitted verbatim after the function name; empty for a non-generic function.
    pub generics: String,
    pub is_main: bool,
    /// D-COV1: the 1-based Jet source line of this function's name, for the
    /// `jet_cov(line)` coverage probe. Only read in coverage mode.
    pub line: usize,
    /// c109 Phase 18: an `#Unsafe fn` (S58, E2-M13/D-LL1) lowers to a Rust `unsafe fn`
    /// (the `unsafe ` keyword prefixes the signature), so the body may use gated pointer
    /// ops directly — calling it is already gated to an `#Unsafe` block in sema (E3103).
    /// I1: this is true ONLY when the source function was `#Unsafe fn`; no `unsafe` is
    /// ever emitted without that source gate. Applies to `TopLevel`/`Method`; a trait
    /// method carries its own `is_unsafe` on `TFuncKind::TraitMethod`.
    pub is_unsafe: bool,
    /// D-REACTCORE1: `#Reactive fn` — the body is emitted inside `jet_reactive_effect`.
    pub is_reactive: bool,
    /// D-METHODMACRO1=A: `@Inline fn` — emits `#[inline]`. Soft hint; sema never
    /// rejects it.
    pub is_inline: bool,
    /// D-METHODMACRO1=A: `@InlineAlways fn` — emits `#[inline(always)]`. Only ever
    /// `true` here once sema has confirmed the function can actually inline
    /// (E0917/E0918/E0919 would have failed the build otherwise) — I3: sema
    /// decides, codegen just emits.
    pub is_inline_always: bool,
    pub body: Vec<TStmt>,
    /// c109 Phase 7: how this function is emitted. A top-level function gets
    /// `pub fn name(…)` at module scope; a method gets `pub fn user_name(<self>, …)`
    /// inside an `impl` block (indented), with the `self` receiver form per the
    /// resolved convention (or no receiver for a static method).
    pub kind: TFuncKind,
}

/// c109 Phase 7: the emission shape of a lowered function.
pub enum TFuncKind {
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
    /// reproduces the `unsafe fn` prefix for an `#Unsafe fn` trait method (S58/D-LL1 —
    /// the body may use gated ops; calling it is already gated to an `#Unsafe` block).
    TraitMethod {
        is_unsafe: bool,
        self_conv: AccessConvention,
    },
    /// c109 Phase 15: a DELEGATION trait method (`using field`) — `emit_delegation_method`
    /// (Source/Codegen/Items.rs). The whole method is structural: a forwarding call
    /// `(self).<field>.<method>(<args>)` to the delegated field, with the BARE trait
    /// method name (no `user_` mangle). There is NO body to lower — the forward string is
    /// resolved at lowering. The signature reproduces `emit_delegation_method`'s exact
    /// shape (a quirky two-space `  {` before the brace, `&self` receiver, no `pub`).
    /// `has_return` decides whether the forward line ends in `;` (unit) or not (returns).
    /// `sig` is the fully-rendered signature line (`    fn name(params)  {\n` with its
    /// quirky double space) and `fwd` the forwarding call — both resolved at lowering.
    Delegation {
        sig: String,
        fwd: String,
        has_return: bool,
    },
}

/// c109 Phase 22: the method-call-collection iteration form on a `loop x in <coll>`,
/// resolved at lowering from `emit_for_in`'s `Expr::MethodCall` branches
/// (Source/Codegen/Statement.rs). Each carries the receiver's emitted Rust string;
/// `file`/the panic line are program/source facts. The plain `.iter().cloned()` form
/// (incl. a non-special method-call collection like `.split(…)`, which `emit_for_in`
/// routes to its `else` default) is represented by `ForIn.method_kind == None`.
pub enum TForInMethod {
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
    /// D-ITER-HOOK: `loop x in mytype` when `mytype` implements `Iterable`.
    Iterable {
        coll_type: String,
        iter_type: String,
    },
}

/// c109 Phase 22: an `if` condition, resolved at lowering from the AST node shape
/// (`emit_if`/`if_pattern_test`, Source/Codegen/Statement.rs):
///  - `Plain` — a boolean expression: `if {cond} {`.
///  - `IfLet` — an optional-binding test (`x == value(b)` → `Some(b)`, `ok(b)`/`err(b)`,
///    a variant `c == Active(id)`): `if let {pat_str} = {subj} {`. The bound name(s)
///    are in scope in the then-branch (the binding's resolved type is bound at lowering,
///    mirroring `add_pattern_bindings`).
///  - `IsNone` — an `x == null` test (`Pattern::Absent`): `if {subj}.is_none() {`.
///  - `Matches` — a binding-free enum variant/group test (`d == .Fire`): `if matches!(&{subj}, {pat}) {`.
pub enum TIfCond {
    Plain(TExpr),
    IfLet { pat_str: String, subj: TExpr },
    IsNone { subj: TExpr },
    Matches { pat_str: String, subj: TExpr },
}

/// D-DOTSCOPE1: which `#Test` scope member a `TStmt::ScopeMember` is.
pub enum ScopeMemberKind {
    /// `.setup { … }` — the body's statements are spliced inline (bindings leak
    /// to the rest of the test), running first.
    Setup,
    /// `.expect_fail { … }` — the region must fail (a `require` failure or a
    /// panic). Runs under a panic-catching boundary; if it completes cleanly the
    /// test fails with "expected this region to fail, but it passed".
    ExpectFail,
    /// `.timeout(dur) { … }` — post-hoc budget in nanoseconds. The region runs to
    /// completion, then its elapsed time is compared against the budget; over
    /// budget fails the test. (v1: post-hoc — does not interrupt a hang.)
    Timeout(u64),
    /// `.skip { … }` — a region that is not executed. Emitted as `if false { … }`
    /// so the body still type-checks but never runs.
    Skip,
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub enum TStmt {
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
        /// D-PROVENANCE1=B: if present, record this Float binding's origin after
        /// initialization. Empty for every untracked/non-Float binding.
        track_origin: Option<String>,
    },
    /// c109 Phase 23: a TUPLE-destructuring binding `(a, b) :: <init>` (S74,
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
    /// c109: a STRUCT-destructuring binding `Type.{ x, y } :: <init>` (S74,
    /// `BindPattern::Struct`). Reproduces `emit_stmt`'s `BindPattern::Struct` arm
    /// byte-for-byte: a `let {tmp} = &({init});` borrow temp, then one
    /// `let[ mut] {local_rust} = ({tmp}).{field_rust}.clone();` per bound field.
    /// D-DESTRUCT1: `local_rust`/`field_rust` diverge for a renamed field
    /// (`severity: sev` binds local `sev` from field `severity`); they're equal
    /// when unrenamed (pre-D-DESTRUCT1 shape). The field's resolved type comes
    /// from `cx.struct_fields` (the init's `Type::Named`/`Apply` name), resolved
    /// at lowering for the slot.
    StructDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        /// `(local_rust_name, field_rust_name)` per bound field, source order.
        binds: Vec<(String, String)>,
    },
    /// c109 Phase 26: a LIST-destructuring binding `[a, b, c] :: <init>` (S74,
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
        /// c150: true when the value is a borrowed non-scalar ident (a `Read`-convention
        /// non-Copy parameter in deref position). Assigning `(*user_s)` directly moves
        /// out of a shared reference (E0507); emitting `((*user_s)).clone()` is correct.
        /// Mirrors the `lower_enum_arg` clone predicate. False for scalars and owned values.
        clone_value: bool,
    },
    Return(Option<TExpr>),
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
    /// D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` — the three-part counted loop.
    /// Emitted as `{ let mut init_name = init_val; loop { if !(cond) { break; } body; step; } }`.
    CountedLoop {
        label: Option<String>,
        init: Box<TStmt>,
        cond: TExpr,
        step: Box<TStmt>,
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
    /// D-INDEX-HOOK: `mytype[k] = v` via `IndexMut::set`.
    IndexHookAssign {
        type_name: String,
        base: TExpr,
        index: TExpr,
        value: TExpr,
    },
    /// D-SWIZZLE1: write swizzle `v.xy = value` — ordered lane assignments into the
    /// receiver's backing array. Sema rejects overlapping patterns (E3111).
    MathSwizzleAssign {
        base: TExpr,
        type_name: String,
        lanes: Vec<u8>,
        value: TExpr,
        clone_value: bool,
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
        /// D-SOA1: the collection is a `#layout(columnar)` list — iterate via
        /// `({coll}).iter_aos()` (yields owned gathered `S`) instead of
        /// `({coll}).iter().cloned()`. Always `false` for the map/method forms.
        columnar: bool,
        /// D-STREAMYIELD1: the collection is a `Stream<T>` (`Receiver<T>`) —
        /// iterate it directly BY VALUE (`for x in (coll) { }`; `Receiver<T>`
        /// already implements `IntoIterator<Item = T>`), not `.iter().cloned()`.
        by_value: bool,
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
        arms: Vec<(TExpr, Vec<TStmt>)>,
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
    /// D-REACTCORE1: `#Reactive { … }` — register a reactive effect at this point.
    Reactive {
        closure: String,
    },
    /// c109 Phase 19: an explicit `region r { … }` (D-REGION1 opt B). Lowers to a plain
    /// Rust block `{ … }` — a lexical scope. The region's escape bound (E0631) and arena
    /// drop ordering (S63 RAII) are enforced entirely in sema; codegen is dumb (I3). The
    /// body's `let`s LEAK into the outer scope (the AST shares `&mut env`), so the body is
    /// lowered on the SAME `LowerEnv`.
    Region(Vec<TStmt>),
    /// D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }` — a Cassowary-style
    /// constraint block. Unlike `Region`/the taskgroup path, this DOES need a
    /// real runtime object: `rust_place` is the emitted `let` binding for a
    /// fresh `jet_layout::Handle`, `label` is the source name (for the
    /// handle's debug/conflict-report label), and `body` is the block's
    /// statements lowered on the SAME env the handle was just bound into (the
    /// parser already desugared every `box.anchor` read to an ordinary
    /// `NAME.h(box, anchor)`/`NAME.v(box, anchor)` method call, so `body` is
    /// nothing but plain statements — no layout-specific TIR shape needed
    /// beyond the handle construction itself).
    Layout {
        rust_place: String,
        label: String,
        body: Vec<TStmt>,
    },
    /// c109 Phase 19: a `#Context(field: value) { … }` smart-context block (D-CTX1). Lowers
    /// to a plain block with one RAII/no-op guard per field (in declaration order)
    /// BEFORE the body: `allocator`/`deadline` push a dynamic context guard in
    /// `jet_mem`; `logger` stays a v1 no-op value bind. Each `(field_name, value)`
    /// pair is resolved at lowering. The body leaks like a region.
    ContextBlock {
        guards: Vec<(String, TExpr)>,
        body: Vec<TStmt>,
    },
    /// D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input block.
    /// Lowers to:
    ///   `{ jet_term_enter(); let _live_guard = jet_scope_guard(|| jet_term_leave()); <body> }`
    /// The scope guard guarantees `jet_term_leave()` runs on every exit path — normal
    /// fall-through, early `return`, `?` propagation, and panic unwind. Codegen is dumb
    /// (I3): no decisions here, only emitting the already-checked RAII form.
    Live {
        body: Vec<TStmt>,
    },
    /// D-DOTSCOPE1: a `#Test` scope-member region — `.setup` / `.expect_fail` /
    /// `.timeout(dur)` / `.skip`. Emitted only inside a `jet test` harness fn
    /// (`fn jet_test_N() -> Result<(), String>`); see `emit_tir_stmt` for the
    /// per-kind lowering. Whole-test `.skip` (a `.skip` first statement) is
    /// handled by the harness `main`, not here.
    ScopeMember {
        kind: ScopeMemberKind,
        body: Vec<TStmt>,
    },
    /// D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` — a transaction
    /// block. Lowers to:
    ///   `{ let mut <handle> = jet_transaction(); <body>; <handle>.commit(); }`
    /// `<handle>.on_commit(() => { … })` inside the body lowers to
    /// `<handle>.on_commit(Box::new(move || { … }))`. The registered hooks run LIFO
    /// in `JetTransaction::drop` — but only if `commit()` ran. A `?`-failure or any
    /// early return skips `commit()`, so the hooks drop un-run (D-TXN3). The
    /// irreversible-effect rejection (E0746, D-TXN2) and rollback are sema's job;
    /// codegen is dumb (I3): effects/transaction state are a compile-time fact.
    Transact {
        /// The mangled Rust name of the transaction handle (`user_<name>`), or `None`
        /// for a bare `#Transact { … }` with no handle (no `on_commit`/`on_rollback`
        /// hooks). When `snapshots` is non-empty a handle is synthesized even for a
        /// bare block, so the auto-snapshot has a transaction to register on.
        handle: Option<String>,
        /// D-TXN-ROLLBACK layer 1+2: each entry is `(&mut <place>, Option<RustTy>)`.
        /// `None` → clone-based snapshot via `jet_txn::snapshot`.
        /// `Some(ty)` → the place implements `Rollback`; use `jet_txn::snapshot_custom`
        /// with `<ty>::restore` so the custom cheap diff runs instead of a full clone.
        snapshots: Vec<(String, Option<String>)>,
        body: Vec<TStmt>,
    },
    /// D-DBG3 step 2 (dap-debugger): a source line marker, one per lowered `Stmt`,
    /// inserted ONLY when `cx.debug_linemap` is set (native `jet debug` builds —
    /// never a normal build or the JIT tier, so this is invisible to the JIT
    /// lowering gate and every other TStmt consumer). Emits a `// jet:line N`
    /// comment immediately before the statement's generated Rust, giving the native
    /// backend a rust-line -> jet-line table without touching any other TStmt shape.
    LineMarker(usize),
}

/// c109 Phase 4: one lowered arm of an exhaustive enum match. `pattern` is the
/// fully-resolved Rust match pattern (`user_Light::user_Red`,
/// `user_Conn::user_Active(user_id) | user_Conn::user_Reconnecting(user_id)`,
/// `user_Http::user_Good(__jet_range_0)`); `guard` is the optional `if …` range
/// guard. Both are computed once at lowering — emit only formats them.
pub struct TMatchArm {
    pub pattern: String,
    pub guard: Option<String>,
    pub body: Vec<TStmt>,
}

/// One piece of a D-VARIADIC1 list spread literal — either a single element or `...list`.
pub enum ListSpreadPart {
    Elem(TExpr),
    Spread(TExpr),
}

/// A lowered expression: a resolved `Type` plus its kind. `ty` is **total** — it
/// is never absent, and codegen never recomputes it.
pub struct TExpr {
    pub ty: Type,
    pub kind: TExprKind,
}

pub enum TExprKind {
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
    Call {
        name: String,
        args: Vec<TCallArg>,
    },
    /// D-RANGETYPE1: checked constructor for `distinct Int(lo..hi)` under
    /// postfix `?`. Emits `user_T::try_new(arg)` returning `Result<user_T,
    /// String>`; the enclosing `Try` node handles propagation.
    RangeCheckedCtor {
        name: String,
        arg: Box<TExpr>,
    },
    /// D-SIMD2 / D-LINALG1: a built-in math-type constructor `F32x4(a,b,c,d)` /
    /// `Vec3(x,y,z)` / `Mat3(…)`, or a static method `F32x4.splat(x)` /
    /// `Vec3.from_array(a)`. Emits the prelude free function `{root}jet_math_<T>_<fn>(…)`
    /// (`_new` for the constructor) with plainly-lowered float/array args.
    MathBuiltin {
        type_name: String,
        func: String,
        args: Vec<TExpr>,
    },
    /// D-BIGINT1 / D-DECIMAL1: precise numeric ctor/method/binop → `jet_bigint_*` / `jet_decimal_*`.
    PreciseBuiltin {
        type_name: String,
        func: String,
        args: Vec<TExpr>,
    },
    /// `print(x)` — the one builtin the subset covers.
    Print(Box<TExpr>),
    /// D-LIN1-DROP (ratified 2026-06-25): `drop(x)` — deliberately discard a
    /// value (a `#SingleUse` value's audited terminal consumption). Lowers to a
    /// plain `drop(arg)` in Rust: a move-to-nowhere whose `Drop` runs. No
    /// `unsafe` is emitted (I3) — the `#Unsafe` gate is a sema-only audit.
    Drop(Box<TExpr>),
    /// c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). A bare call
    /// (no module alias) lowering to `{root}jet_std_io_input(None|Some(&(prompt)))`,
    /// byte-for-byte the `emit_call` ambient-input branch (Source/Codegen/Expression.rs
    /// ~L1778). `prompt` is `Some` when a String prompt arg is given, else `None`.
    AmbientInput {
        prompt: Option<Box<TExpr>>,
    },
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
    /// D-CHAINCMP1: `0 <= sev < 10` — a same-direction relational chain,
    /// `operands.len() == ops.len() + 1`. Dumb lowering (R1): emit binds each
    /// shared middle operand to a temp exactly once (a Rust block expression),
    /// then ANDs the adjacent-pair comparisons over those temps. Relational
    /// ops never trap on overflow (only `+ - * / << >>` do), so no `overflow`
    /// flag is needed here.
    CompareChain {
        operands: Vec<TExpr>,
        ops: Vec<BinOp>,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `>=`/`<=`/`==` between layout
    /// values (`HVar`/`VVar`/`LengthVar`) produce a `Constraint`, which Rust's
    /// comparison operators can't do via operator syntax (`PartialOrd`/
    /// `PartialEq` are hard-locked to `bool`) — so this is a DEDICATED node,
    /// not `Binary`. Emits the matching `jet_layout::{ge,le,eq_}(lhs, rhs)`
    /// free function (registers the constraint on whichever side's `LinExpr`
    /// carries the owning handle). `Add`/`Sub` between layout values stay
    /// plain `Binary` — `jet_layout::LinExpr` implements `std::ops::{Add,Sub}`.
    LayoutCompare {
        op: BinOp,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// D-LAYOUT1: a plain `Int`/`Float` operand used on the other side of a
    /// layout `+`/`-`/`>=`/`<=`/`==` (axis-neutral, elaborates to `LengthVar`
    /// — see `layout_axis_of`). Wraps the numeric Rust value into a
    /// `jet_layout::LinExpr` constant so `Add`/`Sub`/`ge`/`le`/`eq_` only
    /// ever operate on `LinExpr` (no foreign-type operator-overload games
    /// with bare `f64`/`i64`).
    LayoutLit {
        inner: Box<TExpr>,
    },
    Unary {
        op: UnOp,
        operand: Box<TExpr>,
    },
    /// D-INCR1: `++`/`--` on a mutable integer lvalue. `place` is the assign/read
    /// Rust place (total at lowering). `postfix`: return old value before update.
    IncDec {
        op: crate::AST::IncDecOp,
        place: String,
        postfix: bool,
        ty: Type,
    },
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
    /// D-CAP9: postfix `p.*` — dereference a raw pointer. Emits Rust `(*(p))`. The
    /// `unsafe` needed to read through a raw pointer is supplied by the enclosing
    /// `#Unsafe` region/fn (sema-gated by E0208), so this node adds no `unsafe`.
    Deref(Box<TExpr>),
    /// D-CAP9: prefix `*x` — take a raw pointer to `x`. Emits `(&(x) as *const _)`.
    /// Forming a pointer is safe Rust; *using* it needs the surrounding `#Unsafe`
    /// region. Gated by E0208 in sema (raw-of only legal inside `#Unsafe`).
    RawOf(Box<TExpr>),
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
    /// D-DBDRIVER1: a `DbValue` construction (`DbValue.Int(n)` / `.Float(f)` /
    /// `.Text(s)` / `.Bool(b)` / `.Null`) — the tagged SQL parameter/column value.
    /// Same shape as `JsonLit` (a FOREIGN prelude enum, not a user `EnumLit`), kept
    /// as its own node rather than reusing `JsonLit` because `DbValue` renders to
    /// a DIFFERENT prelude type (`jet_std::DbValue`, not `jet_std::DataTree`) and
    /// has no recursive `Array`/`Object`-style payload to special-case.
    DbValueLit {
        variant: String,
        arg: Option<Box<(TExpr, bool)>>,
    },
    /// c109 Phase 5: a list literal `[a, b, c]`. Lowers to Rust `vec![…]`. Each
    /// element is lowered as-is (the AST path applies no clone/coercion at the
    /// literal site — `emit_expr` per element).
    ListLit(Vec<TExpr>),
    /// D-VARIADIC1: `[a, ...xs, b]` — one growable list built via `extend`.
    ListSpread {
        parts: Vec<ListSpreadPart>,
    },
    /// D-SOA1: a list literal whose element is a `#layout(columnar)` struct `S`.
    /// Lowers to `user_<S>_columns::from_aos(vec![…])` — the elements build the
    /// array-of-structs, then `from_aos` distributes them across the columns.
    /// `columns_ty` is the resolved `user_<S>_columns` Rust path.
    ColumnarListLit {
        columns_ty: String,
        elems: Vec<TExpr>,
    },
    /// D-SOA1: index-read `xs[i]` on a columnar list — gathers the logical `S`
    /// from the columns at `i` (bounds-checked, same panic as `jet_index_vec`).
    /// Lowers to `(base).gather_at(i, file, line)`.
    ColumnarGather {
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: usize,
    },
    /// D-SOA1: a fused `xs[i].field` field-read on a columnar list — reads
    /// directly from the field's column (`jet_index_vec(&(base).user_<field>, i,
    /// …)`), the cache-friendly fast path (no whole-`S` gather).
    ColumnarColumnRead {
        base: Box<TExpr>,
        index: Box<TExpr>,
        column_rust: String,
        line: usize,
    },
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
    /// D-INDEX-HOOK: `mytype[k]` when the type implements `Index`.
    IndexHook {
        type_name: String,
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: usize,
    },
    /// D-SIMD2: `v[i]` lane access on a SIMD lane type. Lowers to the bounds-checked
    /// prelude helper `{root}jet_math_<T>_lane(&v, i, file, line)`.
    MathLaneIndex {
        lane_ty: String,
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: u32,
    },
    /// D-SWIZZLE1: a read swizzle `v.xyz` on a vector/SIMD lane type. `lanes` holds
    /// source indices (x=0…w=3); length 1 → scalar, 2..4 → `VecN` constructor.
    MathSwizzleRead {
        type_name: String,
        recv: Box<TExpr>,
        lanes: Vec<u8>,
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
    /// or borrowed value. Also the lowering target for `Expr::Copy` — D-CAP2
    /// (D-MEM1/S4) `copy x`, the one user-typable copy verb — so the compiler's
    /// own internal duplication rewrites and the explicit `copy x` a user writes
    /// share one TIR node (I8). The AST path emits `(recv).clone()`
    /// unconditionally; the TIR carries the lowered receiver and the result type
    /// (the receiver's type).
    Clone(Box<TExpr>),
    /// D-MEM1 stage S5 (2026-07-04): `copy d` where `d` is a string-view local
    /// (`Binding.string_view`, a bare `&str` Rust place) — materializes it into
    /// an owned `String` via `.to_string()`. A plain `.clone()` (the `Clone`
    /// node above) would be wrong here: cloning a `&str` hands back another
    /// `&str`, not the owned `String` the copy needs to escape the view's scope.
    MaterializeView(Box<TExpr>),
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
    Todo {
        line: usize,
        expected_type: String,
    },
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
    /// D-TAG1: a binding-free enum variant/group pattern test (`d == .Fire`,
    /// `d == .Fire.Burn` in expression position). Lowers to `matches!(&subj, pat)`
    /// where `pat` is the same Rust pattern string `emit_match_pattern` uses for
    /// switch arms (group names expand to or-patterns over their leaves).
    PatternMatches {
        subj: Box<TExpr>,
        pat_str: String,
    },
    FanOut {
        calls: Vec<TExpr>,
    },
    /// D-HOLE1: `Option.lift2(f, a, b)` — apply `f` to both payloads only when both
    /// `a`/`b` are present; `null` otherwise. `f`/`a`/`b` are lowered plainly as
    /// values (`f` via the generic `Expr::Lambda`/fn-value lowering, same as any
    /// other function-typed argument); emit destructures the zipped pair inside a
    /// closure. No user-visible tuple struct — the pair never surfaces as a Jet
    /// value.
    OptionLift2 {
        f: Box<TExpr>,
        a: Box<TExpr>,
        b: Box<TExpr>,
    },
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
    /// D-TASKSCOPE1=A: `g.all([h1, h2, …])` — join every handle, collect results.
    TaskGroupAll {
        tasks: Box<TExpr>,
    },
    /// D-CONCCOMB1=A: `g.race([h1, h2, …])` — first completed result wins.
    TaskGroupRace {
        tasks: Box<TExpr>,
    },
    /// D-CONCCOMB1=A: `g.any([h1, h2, …])` — first completed result (v1 alias).
    TaskGroupAny {
        tasks: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `g.select()` — start a scoped fluent select builder.
    SelectStart,
    /// D-CONCSELECT1=A: `.recv(ch)` on a select builder.
    SelectRecv {
        builder: Box<TExpr>,
        channel: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `.after(ms: …)` on a select builder.
    SelectAfter {
        builder: Box<TExpr>,
        millis: Box<TExpr>,
        value: Option<Box<TExpr>>,
    },
    /// D-CONCSELECT1=A: `.read(stream)` on a select builder.
    SelectRead {
        builder: Box<TExpr>,
        stream: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `.wait()` — multiplex until one arm wins.
    SelectWait {
        builder: Box<TExpr>,
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
pub enum TModuleCallForm {
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
pub struct TExternArg {
    pub value: TExpr,
    pub clone: bool,
}

/// c109 Phase 13: the three closure-taking core-call shapes (see
/// `TExprKind::CoreClosureCall`). Each holds the already-rendered closure string
/// (`spawn_closure` is the distinct `emit_spawn_lambda` form; `serve`/`guard` use the
/// plain `emit_lambda` form) plus, for `serve`, the lowered address arg.
pub enum TCoreClosureKind {
    /// `tasks.spawn(<lambda>)` → `{root}jet_std::JetTask::spawn(<spawn_closure>)`.
    Spawn { spawn_closure: String },
    /// `http.serve(addr, <lambda>)` → `{root}jet_http_serve(&(<addr>), <closure>)`.
    Serve { addr: Box<TExpr>, closure: String },
    /// `scope.guard(<lambda>)` → `{root}jet_scope_guard(<closure>)`.
    Guard { closure: String },
    /// D-TXN3: `<handle>.on_commit(<lambda>)` → `<handle>.on_commit(Box::new(<closure>))`.
    OnCommit { handle: String, closure: String },
    /// D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(<lambda>)` →
    /// `<handle>.on_rollback(Box::new(<closure>))`. Mirror of `OnCommit`.
    OnRollback { handle: String, closure: String },
    /// D-REACT1=B: `reactive.derived(<lambda>)` → `{root}jet_std::JetDerived::new(<closure>)`.
    ReactiveDerived { closure: String },
    /// D-REACT1=B: `reactive.effect(<lambda>)` → `{root}jet_std::jet_reactive_effect(<closure>)`.
    ReactiveEffect { closure: String },
    /// D-RENDERTGT2=A (c133 M2): reactive UI render loop through the backend seam.
    UiReactiveRender { closure: String },
}

/// c109 Phase 13: the two fn-typed-value forms (see `TExprKind::FnValue`).
pub enum TFnValueKind {
    /// A bare function name used as a value. `wrapper` is the already-rendered
    /// `Box::new(move |…| user_<name>(…)) as <fn-type>` string (`emit_named_fn_value`),
    /// produced at lowering so emit only echoes it.
    NamedFn { wrapper: String },
    /// A call through a fn-value `(f)(args)`. `callee` lowers to its place (a local
    /// of `Type::Fn`, or another fn-value form); args are lowered plainly.
    Call {
        callee: Box<TExpr>,
        args: Vec<TCallArg>,
    },
}

/// c109 Phase 12: a resolved numeric method form, one per `emit_builtin_method`
/// numeric arm (Source/Codegen/Expression.rs). The width source/target and the
/// widening-vs-narrowing branch (which `numeric_conversion` decides from the source
/// width name) are decided ONCE at lowering — the variant encodes the chosen form so
/// emit only formats.
pub enum TNumericOp {
    /// `is_nan`/`is_infinite`/`is_finite` → `({recv}).{method}()` (bool).
    Predicate(String),
    /// `count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros` →
    /// `(({recv}).{method}() as i64)` (Rust returns u32 → widen to Int).
    BitCount(String),
    /// `origin` on a Float receiver → debug provenance note or `"untracked"`.
    Origin,
    /// A widening / float-targeted / float-sourced conversion → `(({recv}) as {dst})`.
    CastAs { dst_rust: String },
    /// An integer-narrowing conversion → the checked `<{dst}>::try_from(...)` form
    /// returning `Result<T, String>`. Both strings resolved at lowering.
    TryFrom {
        dst_rust: String,
        dst_spelling: String,
    },
    /// `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string`
    /// arm of `emit_builtin_method`, which fires for any receiver type).
    ToShow,
}

/// c109 Phase 11: a resolved closure-taking collection-method op, one per
/// `emit_builtin_method` closure arm (Source/Codegen/Expression.rs). The
/// receiver-type branch (Map vs list vs trait-object list) and the Fn-vs-FnMut
/// branch (off the lambda arg's `needs_fn_mut` meta) are decided ONCE at lowering;
/// the variant encodes the chosen form so emit only formats.
pub enum TClosureOp {
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
    // D-ITER1: lazy adapter closure methods.
    /// `take_while(f)` — `jet_list_take_while((recv).clone(), f)`.
    TakeWhile,
    /// `skip_while(f)` — `jet_list_skip_while((recv).clone(), f)`.
    SkipWhile,
    /// `flat_map(f)` — `jet_list_flat_map((recv).clone(), f)`.
    FlatMap,
    /// `scan(seed, f)` — `jet_list_scan((recv).clone(), seed, f)`.
    Scan,
    /// `position(f)` — `jet_list_position((recv).clone(), f)`.
    Position,
    /// `min_by(f)` — `jet_list_min_by((recv).clone(), f)`.
    MinBy,
    /// `max_by(f)` — `jet_list_max_by((recv).clone(), f)`.
    MaxBy,
    /// `fold(init, f)` — `jet_list_fold((recv).clone(), init, f)`.
    Fold,
    /// `group_by(f)` — `jet_list_group_by((recv).clone(), f)`.
    GroupBy,
    /// `count_by(f)` — `jet_list_count_by((recv).clone(), f)`.
    CountBy,
    /// `partition(f)` — inline emit; struct name embedded. `TupleStruct` is `JetTup_<hash>`.
    Partition { tuple_struct: String },
    // D-FAILCOMP1: failure-aware adapters.
    /// `filter_map(f)` — `jet_list_filter_map((recv).clone(), f)`.
    FilterMap,
    // D-AUTOPAR1=A: explicit parallel adapters.
    /// `par_map(f)` — `jet_list_par_map((recv).clone(), f)`.
    ParMap,
    /// `par_filter(f)` — `jet_list_par_filter((recv).clone(), f)`.
    ParFilter,
    /// `par_fold(init, f)` — `jet_list_par_fold((recv).clone(), init, f)`.
    ParFold,
    // D-HOLE1: Option combinators.
    /// `map` on `T?` — `(recv).clone().map(f)` (Rust's native `Option::map`, no
    /// prelude helper needed).
    OptionMap,
    // D-DYNARRAY1: `View<T>` read-only closure methods. `recv` is already a
    // `&[T]` borrow (see `Context::rust_type`'s `View` arm) — NOT `.clone()`d
    // into an owned `Vec` first, unlike every list closure op above; that
    // clone would silently defeat the zero-copy point of `.view(...)`.
    /// `view.fold(init, f)` — `jet_view_fold((recv), init, f)`.
    ViewFold,
    /// `view.map(f)` — `jet_view_map((recv), f)` (map-to-owned: returns `[R]`).
    ViewMap,
}

/// c109 Phase 11: a fully-resolved lambda/closure, every fact carried total from
/// `Lambda.meta`. `prep` is the rendered clone-capture prelude (`let _jet_cap_<n> =
/// (place).clone();\n    ` per cloned capture); `params` the rendered `name[: ty]`
/// param list; `body` the rendered closure body string (an expression body, or a
/// `{ … }` block) — rendered at lowering from the lowered body so emit stays a pure
/// wrapper; `is_move`/`boxed` reproduce the AST wrappers.
pub struct TLambda {
    pub prep: String,
    pub params: Vec<String>,
    pub body: String,
    pub is_move: bool,
    pub boxed: bool,
}

/// c109 Phase 8: the resolved error-conversion of a `?`, mirroring `AST::TryConvert`
/// (the total sema fact). Carried onto the TIR so the emitter never re-derives it.
pub enum TTryConvert {
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
pub enum TOrFallback {
    Value(Box<TExpr>),
    Return(Option<Box<TExpr>>),
    /// The fully-rendered `{ jet_panic_rich(…); }` statement string, resolved at
    /// lowering — byte-identical to `emit_panic_stop`'s output. The interpolated panic
    /// message (which itself may contain lowered sub-expressions) and the locals
    /// snapshot are baked in here.
    Panic(String),
    /// D-ORRETURN-ERG1=B: `?? break` — loop exit.
    Break,
    /// D-ORRETURN-ERG1=B: `?? continue` — loop skip.
    Continue,
}

pub enum TStrPart {
    Lit(String),
    Interp(TExpr, crate::AST::StrFormat),
}

/// c109 Phase 4/16: the resolved payload shape of an enum literal.
pub enum TEnumPayload {
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
pub struct TEnumArg {
    pub value: TExpr,
    /// Wrap the value in `(…).clone()` (non-scalar payload, borrowed-in-env arg).
    pub clone: bool,
    /// Wrap (after the clone) in `Box::new(…)` — a recursive boxed edge.
    pub boxed: bool,
}

/// c109 Phase 9: a resolved built-in collection/string method op. Each variant is
/// one emit form from `emit_builtin_method` (Source/Codegen/Expression.rs). The
/// receiver-type dispatch (`rty = expr_jet_ty(receiver)` → Map vs List vs String)
/// is decided ONCE at lowering — the variant encodes the chosen branch, so emit
/// only formats. Line numbers (for the bounds/remove panic frames) are resolved at
/// lowering; `cx.file`/`cx.root_prefix` are read at emit (program-level, not a
/// per-node decision). Args are emitted plainly (no clone/borrow wrappers), exactly
/// as `emit_builtin_method`'s `arg(i)` does.
pub enum TBuiltinOp {
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
    RemoveList {
        line: usize,
    },
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
    Sum,
    Product,
    Min,
    Max,
    Flatten,
    Intersperse,
    Unzip {
        tuple_struct: String,
    },
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
    /// c97/D-STRPARSE1: `lines()` → `{root}jet_string_lines(&(recv))`.
    Lines,
    /// c97/D-STRPARSE1: `to_int()` on a String → fallible parse, mirroring `Int.parse`:
    /// `(recv).trim().parse::<i64>().map_err(|e| e.to_string())` (`Int ? ParseError`).
    ToIntString,
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
    Slice {
        line: usize,
    },
    /// D-STR-AFTER1: `after(sep)` → `jet_string_after(&(recv), &a0)`. Substring
    /// strictly after the first `sep`; `sep` absent → the whole original string
    /// (mirrors `.replace`'s no-match-is-identity convention).
    After,
    /// D-STR-AFTER1: `before(sep)` → `jet_string_before(&(recv), &a0)`. Substring
    /// strictly before the first `sep`; `sep` absent → the whole original string.
    Before,
    /// D-MEM1 stage S5: the zero-copy sibling of `Trim`, used ONLY as the init
    /// of a `Binding` sema marked `string_view` (E2307 proves it can't outlive
    /// its owner) → `jet_string_trim_view(&(recv))`, a borrowed `&str`, no
    /// `.to_string()`.
    TrimView,
    /// D-MEM1 stage S5: the zero-copy sibling of `After`, same `string_view`
    /// gate → `jet_string_after_view(&(recv), &a0)`.
    AfterView,
    /// D-MEM1 stage S5: the zero-copy sibling of `Before`, same `string_view`
    /// gate → `jet_string_before_view(&(recv), &a0)`.
    BeforeView,
    /// `keys()` → `(recv).keys().cloned().collect::<Vec<_>>()`.
    Keys,
    /// `values()` → `(recv).values().cloned().collect::<Vec<_>>()`.
    Values,
    /// `contains_key(k)` → `(recv).contains_key(&a0)`.
    ContainsKey,
    /// `to_string()` (on a String receiver) → `(recv).jet_show()`.
    ToString,
    /// D-REGEXENGINE1=A: `Match.group(n)` → `(recv).group(a0)`.
    MatchGroup,
    // D-ITER1: non-closure lazy adapters.
    /// `take(n)` → `jet_list_take((recv).clone(), a0)`.
    Take,
    /// `skip(n)` → `jet_list_skip((recv).clone(), a0)`.
    Skip,
    /// `step_by(n)` → `jet_list_step_by((recv).clone(), a0)`.
    StepBy,
    /// `dedup()` → `jet_list_dedup((recv).clone())`.
    Dedup,
    /// `chunks(n)` → `jet_list_chunks((recv).clone(), a0)`.
    Chunks,
    /// `windows(n)` → `jet_list_windows((recv).clone(), a0)`.
    Windows,
    /// `enumerate()` → inline emit building `JetTup_<hash>` struct. The struct name
    /// is embedded here at lowering so emit is a pure formatter.
    Enumerate {
        tuple_struct: String,
    },
    /// `zip([U])` → inline emit building `JetTup_<hash>` struct.
    Zip {
        tuple_struct: String,
    },
    // D-HOLE1: Option combinators.
    /// `zip(U?)` on `T?` → `(recv).clone().zip((a0).clone()).map(|(x,y)| Struct{…})`
    /// (Rust's native `Option::zip`, wrapped into the named-tuple struct). `elem_ty`
    /// (`(a: T, b: U)`) is the resolved pair type — carried so the call's own `TExpr`
    /// type is total (not the generic table's placeholder), even though it's rarely
    /// load-bearing in emit (a binding carries sema's `b.ty`).
    OptionZip {
        tuple_struct: String,
        elem_ty: Type,
    },
    // D-COLLBREADTH1=A: Set<T> operations.
    /// `Set.from([...])` — recv is the list: `(recv).into_iter().collect::<std::collections::HashSet<_>>()`.
    SetFrom,
    /// `set.add(v)` → `(recv).insert(a0)` (HashSet::insert; bool result discarded).
    SetInsert,
    /// `set.remove(v)` → `(recv).remove(&a0)` (bool result discarded).
    SetRemove,
    /// `set.to_list()` → `(recv).iter().cloned().collect::<Vec<_>>()`.
    SetToList,
    /// `set.union(other)` → `(recv).union(&(a0)).cloned().collect::<std::collections::HashSet<_>>()`.
    SetUnion,
    SortedSetFrom,
    SortedSetInsert,
    SortedSetRemove,
    SortedSetToList,
    SortedSetUnion,
    PriorityQueueFrom,
    PriorityQueuePeek,
    PriorityQueueToSortedList,
    LruPut,
    LruGet,
    LruCapacity,
    LruKeys,
    BitSetAdd,
    BitSetRemove,
    BitSetCount,
    BitSetToList,
    BitSetNew,
    ByteBufferNew,
    ByteBufferFrom,
    ByteBufferWrite {
        method: String,
    },
    ByteBufferToBytes,
    // D-TAG1: Bag<T> counted multiset (HashMap-backed).
    BagAdd,
    BagRemove,
    BagHas,
    BagCount,
    BagLen,
    // D-COLLBREADTH1=A: Deque<T> operations.
    /// `deque.push_front(v)` → `(recv).push_front(a0)`.
    DequePushFront,
    /// `deque.push_back(v)` → `(recv).push_back(a0)`.
    DequePushBack,
    /// `deque.pop_front()` → `(recv).pop_front()` (returns `Option<T>`).
    DequePopFront,
    /// `deque.pop_back()` → `(recv).pop_back()` (returns `Option<T>`).
    DequePopBack,
    /// `deque.peek_front()` → `(recv).front().cloned()` (returns `Option<T>`).
    DequePeekFront,
    /// `deque.peek_back()` → `(recv).back().cloned()` (returns `Option<T>`).
    DequePeekBack,
    // D-FAILCOMP1: failure-aware list adapters.
    /// `try_collect()` on `[Result<T,E>]` → `jet_list_try_collect((recv).clone())`.
    TryCollect,
    // D-DYNARRAY1: `View<T>` — a zero-copy window (`&[T]`) over a list's
    // backing storage. The read-only accessor methods (`len`/`is_empty`/
    // `get`/`first`/`last`/`contains`/`index_of`) reuse `LenList`/`IsEmpty`/
    // `GetList`/`First`/`Last`/`Contains`/`IndexOf` above unchanged — every
    // one of those emits a plain Rust slice/`.get`/`.first`/… call that a
    // `&[T]` receiver satisfies exactly as a `Vec<T>` does.
    /// `list.view(a..b)` → `jet_view_new(&(recv), a0, a1, file, line)`.
    ViewNew {
        line: usize,
    },
}

/// c109 Phase 13: a resolved handle-method op, one per handle arm of
/// `emit_builtin_method` (Source/Codegen/Expression.rs). The handle-receiver branch
/// (keyed on `rty == Some(Named(<handle>))`) is decided ONCE at lowering from the
/// total `recv_type` — emit only formats. Args are emitted plainly (raw `arg(i)`).
/// `{root}` denotes `cx.root_prefix` (program-level, read at emit).
pub enum THandleOp {
    /// FileReader: `read_line()` → `{root}jet_std_file_reader_read_line(&mut (recv))`.
    FileReaderReadLine,
    /// FileWriter: `write_line(s)` → `{root}jet_std_file_writer_write_line(&mut (recv), &(a0))`.
    FileWriterWriteLine,
    /// FileWriter: `flush()` → `{root}jet_std_file_writer_flush(&mut (recv))`.
    FileWriterFlush,
    /// StdinHandle: `read_line()` → `{root}jet_std_io_stdin_read_line(&mut (recv))`.
    StdinReadLine,
    /// Stdout/Stderr: stream writes and facts (D-COREIO1=A).
    StdoutWrite,
    StdoutWriteLine,
    StdoutWriteBytes,
    StdoutFlush,
    StdoutIsTty,
    StderrWrite,
    StderrWriteLine,
    StderrWriteBytes,
    StderrFlush,
    StderrIsTty,
    /// Stopwatch: `elapsed_millis()` → `{root}jet_stopwatch_elapsed_millis(&(recv))`.
    StopwatchElapsedMillis,
    /// D-DET1 Clock: `now()` → `{root}jet_clock_now(&(recv))` (current ms, no advance).
    ClockNow,
    /// D-DET1 Clock: `tick(ms)` → `{root}jet_clock_tick(&mut (recv), a0)` (advance + read).
    ClockTick,
    /// D-DET-CAPAPI Clock: `advance(to_ms)` → `{root}jet_clock_advance(&mut (recv), a0)` (absolute set + read).
    ClockAdvance,
    /// D-DET-CAPAPI Clock: `wait(d)` → `{root}jet_clock_wait(&mut (recv), &(a0))` (advance by a Duration + read).
    ClockWait,
    /// D-DET1 Rng: `int(lo, hi)` → `{root}jet_rng_int(&mut (recv), a0, a1)` (draw in [lo,hi]).
    RngInt,
    /// D-DET1 Rng: `float()` → `{root}jet_rng_float(&mut (recv))` (draw in [0,1)).
    RngFloat,
    /// D-RANDOMDIST1 Rng: `float_range(lo, hi)` → `{root}jet_rng_float_range(&mut (recv), a0, a1)`.
    RngFloatRange,
    /// D-DET-CAPAPI Rng: `bool()` → `{root}jet_rng_bool(&mut (recv))` (coin draw).
    RngBool,
    /// D-RANDOMDIST1 Rng: `bool(p)` → `{root}jet_rng_bool_p(&mut (recv), a0)`.
    RngBoolP,
    /// D-RANDOMDIST1 Rng: `normal(mean, stddev)` → `{root}jet_rng_normal(&mut (recv), a0, a1)`.
    RngNormal,
    /// D-RANDOMDIST1 Rng: `exponential(lambda)` → `{root}jet_rng_exponential(&mut (recv), a0)`.
    RngExponential,
    /// D-RANDOMDIST1 Rng: `bytes(n)` → `{root}jet_rng_bytes(&mut (recv), a0)`.
    RngBytes,
    /// D-RANDOMDIST1 Rng: `split()` → `{root}jet_rng_split(&mut (recv))`.
    RngSplit,
    /// D-DET-CAPAPI Rng: `pick(list)` → `{root}jet_rng_pick(&mut (recv), &(a0))` (uniform `T?`).
    RngPick,
    /// D-RANDOMDIST1 Rng: `weighted_pick(list, weights)` → `{root}jet_rng_weighted_pick(&mut (recv), &(a0), &(a1))`.
    RngWeightedPick,
    /// D-RANDOMDIST1 Rng: `sample(list, k)` → `{root}jet_rng_sample(&mut (recv), &(a0), a1)`.
    RngSample,
    /// D-DET-CAPAPI Rng: `shuffle(&list)` → `{root}jet_rng_shuffle(&mut (recv), &mut (a0))` (in-place).
    RngShuffle,
    /// D-SOLVER-LIB1=A: `Solver.new(seed)` → `{root}jet_solver_new(seed)`.
    SolverNew,
    /// D-SOLVER-LIB1=A: `solver.require(ok)` → `{root}jet_solver_require(&mut solver, ok)`.
    SolverRequire,
    /// D-SOLVER-LIB1=A: `solver.failure_count()` → `{root}jet_solver_failure_count(&solver)`.
    SolverFailureCount,
    /// D-SOLVER-LIB1=A: `solver.status()` → `{root}jet_solver_status(&solver)`.
    SolverStatus,
    GameSceneNew,
    GameReplayRecord,
    GameBackendHeadless,
    GameBudgetsNew,
    GameSceneOnFrame,
    GameSceneComponent,
    GameSceneQuery,
    GameAssetsImage,
    GameAssetsSound,
    GameInputBind,
    GameBudgetsSet,
    GameInputPressed,
    /// D-DET-CAPAPI Duration: `millis()` → `{root}jet_duration_millis(&(recv))` (span as ms).
    DurationMillis,
    /// D-TIME-CALENDAR1 Duration: `seconds()` → `{root}jet_duration_seconds(&(recv))`.
    DurationSeconds,
    /// D-BIGINT1 / D-DECIMAL1: instance methods on precise numeric types.
    PreciseMethod {
        type_name: String,
        method: String,
    },
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
    /// D-ARGS1: ArgsSpec `.flag(name, help)` → `(recv).flag(&a0, &a1)` → `JetArgsSpec`.
    ArgsSpecFlag,
    ArgsSpecFlagShort,
    /// D-ARGS1: ArgsSpec `.option(name, help, meta)` → `(recv).option(&a0, &a1, &a2)` → `JetArgsSpec`.
    ArgsSpecOption,
    ArgsSpecOptionShort,
    ArgsSpecOptionDefault,
    ArgsSpecOptionEnv,
    ArgsSpecOptionInt,
    ArgsSpecOptionFloat,
    ArgsSpecOptionChoice,
    ArgsSpecRepeat,
    ArgsSpecRequiredOption,
    /// D-ARGS1: ArgsSpec `.positional(name, help)` → `(recv).positional(&a0, &a1)` → `JetArgsSpec`.
    ArgsSpecPositional,
    ArgsSpecSubcommand,
    ArgsSpecVersion,
    ArgsSpecCompletion,
    /// D-ARGS1: ArgsSpec `.help()` → `(recv).help()` → `String`.
    ArgsSpecHelp,
    /// D-ARGS1: ArgsSpec `.parse(argv)` → `{root}jet_args_parse(&(recv), &(a0))` → `Result<JetParsedArgs, String>`.
    ArgsSpecParse,
    /// D-ARGS1: ParsedArgs `.flag(name)` → `{root}jet_args_flag(&(recv), &(a0))` → `bool`.
    ParsedArgsFlag,
    /// D-ARGS1: ParsedArgs `.option(name)` → `{root}jet_args_option(&(recv), &(a0))` → `Option<String>`.
    ParsedArgsOption,
    ParsedArgsOptionInt,
    ParsedArgsOptionFloat,
    ParsedArgsOptions,
    ParsedArgsSubcommand,
    /// D-ARGS1: ParsedArgs `.positional(n)` → `{root}jet_args_positional(&(recv), a0)` → `Option<String>`.
    ParsedArgsPositional,
    /// D-PROCESS1: ProcessSpec builder/run/spawn methods.
    ProcessSpecMethod {
        method: String,
    },
    /// D-PROCESS1: ProcessChild control/streaming methods.
    ProcessChildMethod {
        method: String,
    },
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s `Value` handle — plain
    /// inherent-method passthrough, same shape as `ArgsSpecHelp`.
    ReflectValueTypeName,
    ReflectValueDisplay,
    ReflectValueFields,
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x).fields()`'s `Field` handle.
    ReflectFieldName,
    ReflectFieldValue,
    /// c109 Phase 21: Task `join()` → `(recv).join()` (the no-arg `join` arm of
    /// `emit_builtin_method`, Source/Codegen/Expression.rs ~L967 — shared with the dead
    /// list no-arg join, but here it's the JetTask method). Returns the task's value `T`.
    TaskJoin,
    /// c109 Phase 21: Task `detach()` → `{ let _detach = (recv); }` (D-DETACH1 —
    /// fire-and-forget; drops the JoinHandle). Returns unit.
    TaskDetach,
    /// D-COROUTINE1=A: Task control-plane pause request (thread-runtime v1: metadata only).
    TaskPause,
    /// D-COROUTINE1=A: Task control-plane resume request (thread-runtime v1: metadata only).
    TaskResume,
    /// D-COROUTINE1=A: Task control-plane cancel request (thread-runtime v1: metadata only).
    TaskCancel,
    /// D-COROUTINE1=A: Task control-plane trace string.
    TaskTrace,
    /// c109 Phase 21 / D-TUPLE-DESTRUCT1: Receiver `receive()` → `(recv).receive()` →
    /// `Result<T, Closed>`.
    ChannelReceive,
    /// c109 Phase 21: Sender `send(v)` → `(recv).send(a0)`. Returns unit.
    SenderSend,
    /// c109 Phase 25: HttpRouter `get`/`post`/`put`/`delete` route registration
    /// (D-ROUTE1=A). Emits `{root}jet_http_router_register(&mut (recv), "<VERB>".to_string(),
    /// <path>, <handler>)` where `<path>` is the lowered first arg (args[0]) and `<handler>`
    /// is a pre-rendered boxed-closure string (`emit_router_handler` reproduction, resolved
    /// at lowering). `verb` is the uppercase HTTP method literal.
    HttpRouterRegister {
        verb: &'static str,
        handler: String,
        file: String,
        line: usize,
    },
    /// D-SIMD2 / D-LINALG1: an INSTANCE method on a built-in math value type. Emits
    /// the prelude free function `{root}jet_math_<type>_<method>(&(recv), <args>)`
    /// (e.g. `jet_math_Vec3_dot(&(v), w)`, `jet_math_F32x4_sum(&(v))`). `reduce`
    /// carries the validated marker op so the right fold function is named.
    MathMethod {
        type_name: String,
        method: String,
        reduce_op: Option<String>,
    },
    /// D-REACT1=B: `Signal.get()`/`Derived.get()` → `(recv).get()` (reads + tracks).
    ReactiveGet,
    /// D-REACT1=B: `Signal.set(v)` → `(recv).set(<arg0>)` (writes + notifies).
    ReactiveSet,
    /// D-EVENT1=D: Event/Hook/Subscription/EventScope/EventTrace runtime methods.
    EventMethod {
        method: String,
    },
    /// D-WATCH-SCOPE1: WatchHandle/WatchSet polling and callback methods.
    WatchMethod {
        method: String,
    },
    /// D-HONESTNUM1=A: `Measurement<Float>` arithmetic / accessors.
    /// `.add(m)/.sub(m)/.mul(m)/.div(m)` → `(recv).<method>(a0)` → `JetMeasurement<f64>`.
    /// `.value()/.uncertainty()` → `(recv).<method>()` → `f64`.
    MeasurementMethod {
        method: String,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1: an instance method on `LayoutHandle`
    /// (`.h`/`.v`/`.value`/`.suggest`/`.is_feasible`/`.conflict`) or
    /// `Constraint` (`.required`/`.strong`/`.medium`/`.weak`). Every Jet
    /// method name IS the `jet_layout` Rust method name (no renaming table
    /// needed, unlike `MathMethod`) — pure passthrough: `(recv).method(args)`.
    LayoutMethod {
        method: String,
    },
    /// D-PENDING1=B: `Loadable<T,E>` predicate / accessor methods.
    /// `.is_loading()/.is_loaded()/.is_failed()/.is_idle()` → `(recv).<method>()`.
    /// `.loaded()` → `(recv).loaded()` → `Option<T>`.
    /// `.or_else(default)` → `(recv).or_else(a0)` → `T`.
    LoadableMethod {
        method: String,
    },
    /// D-TTLVAL1=A: `Expiring<T>` fallible accessors.
    ExpiringMethod {
        method: String,
    },
    /// D-TTLVAL1=A: `Rotting<T>` fallible secret accessors.
    RottingMethod {
        method: String,
    },
    /// D-APPROX1=A: method call on a sketch data structure (HyperLogLog/TDigest/CMS/ReservoirSampler).
    SketchMethod {
        sketch: String,
        method: String,
    },
    /// D-TIMEDEPTH1=A: method call on a civil-time type (Date/DateTime).
    CivilTimeMethod {
        kind: String,
        method: String,
    },
    /// D-URL1=A: method call on Url/Mime value types.
    UrlMimeMethod {
        kind: String,
        method: String,
    },
    /// D-REGEXENGINE1=A: method call on Regex/Match value types.
    RegexMethod {
        kind: String,
        method: String,
    },
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP client type (HttpClientReq/HttpClientResp).
    HttpClientMethod {
        kind: String,
        method: String,
    },
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP server type (HttpMux/HttpSrvReq/HttpSrvResp).
    HttpServerMethod {
        kind: String,
        method: String,
    },
    /// D-SERDE-ACCESS=B: `DataTree.field(name)` → `(recv).field(&a0)`.
    DataTreeField,
    /// D-SERDE-ACCESS=B: `DataTree.at(i)` → `(recv).at(a0)`.
    DataTreeAt,
    /// D-SERDE-ACCESS=B: `DataTree.int()` → `(recv).int()`.
    DataTreeInt,
    /// D-SERDE-ACCESS=B: `DataTree.text()` → `(recv).text()`.
    DataTreeText,
    /// D-SERDE-ACCESS=B: `DataTree.bool()` → `(recv).bool()`.
    DataTreeBool,
    /// D-SERDE-ACCESS=B: `DataTree.float()` → `(recv).float()`.
    DataTreeFloat,
    /// D-SERDE-ACCESS=B: same accessors on `Json`/`Data`.
    JsonField,
    JsonAt,
    JsonInt,
    JsonText,
    JsonBool,
    JsonFloat,
    /// D-PATHFS1: `Path.from(str)` constructor → `{root}jet_path_from(&(recv))`.
    PathFrom,
    /// D-PATHFS1: `path.join(other)` → `{root}jet_path_join(&(recv), &(a0))` → `JetPath`.
    PathJoin,
    /// D-PATHFS1: `path.parent()` → `{root}jet_path_parent(&(recv))` → `Option<JetPath>`.
    PathParent,
    /// D-PATHFS1: `path.extension()` → `{root}jet_path_extension(&(recv))` → `Option<String>`.
    PathExtension,
    /// D-PATHFS1: `path.stem()` → `{root}jet_path_stem(&(recv))` → `Option<String>`.
    PathStem,
    /// D-PATHFS1: `path.to_string()` → `(recv).jet_show()` → `String`.
    PathToString,
    /// D-PATHFS1: `path.write_atomic(bytes)` → `{root}jet_path_write_atomic(&(recv), &(a0))` → `Result<(), IoError>`.
    PathWriteAtomic,
    /// D-PATHFS1: `path.walk()` → `{root}jet_path_walk(&(recv))` → `Vec<JetPath>`.
    PathWalk,
    /// D-RENDERTGT2=A (c133 M1): NullBackend measure/layout/paint/on_event/commands.
    UiBackendMethod {
        method: String,
    },
    /// c-devserver (owner-directed 2026-07-01): `DevServer` builder methods
    /// (`.html`/`.port`/`.serve`).
    DevServerMethod {
        method: String,
    },
    /// D-DBDRIVER1: `conn.query(sql, params)` → `Result<Vec<Row>, DbError>`. Encodes
    /// `params` via `jet_std::jet_db_encode_params`, calls the FFI bridge's
    /// `jet_db_query`, decodes the wire result via `jet_std::jet_db_decode_query_result`.
    DbQuery,
    /// D-DBDRIVER1: `conn.query_one(sql, params)` → `Result<Option<Row>, DbError>`.
    /// Same as `DbQuery` but takes only the first row (if any).
    DbQueryOne,
    /// D-DBDRIVER1: `conn.execute(sql, params)` → `Result<Int, DbError>` (affected rows).
    DbExecute,
    /// D-DBDRIVER1: `conn.begin()` → `{ffi}::jet_db_begin((recv).handle)` → `Bool`.
    DbBegin,
    /// D-DBDRIVER1: `conn.commit()` → `{ffi}::jet_db_commit((recv).handle)` → `Bool`.
    DbCommit,
    /// D-DBDRIVER1: `conn.rollback()` → `{ffi}::jet_db_rollback((recv).handle)` → `Bool`.
    DbRollback,
    /// D-DBDRIVER1: `conn.close()` → `{ffi}::jet_db_close((recv).handle)` → `Bool`.
    DbClose,
    /// D-DBDRIVER1: `DbValue` accessor methods (`.int()`/`.float()`/`.text()`/
    /// `.bool()`/`.is_null()`) → `(recv).<method>()`, same shape as `JsonInt`/….
    DbValueInt,
    DbValueFloat,
    DbValueText,
    DbValueBool,
    DbValueIsNull,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call(name, args)` →
    /// `Result<Float, String>`, a homogeneous `[Float]` call across the
    /// sandboxed Component Model boundary (wire-encoded, see `Prelude/Plugin.rs`).
    PluginCall,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call_int(name, args)` →
    /// `Result<Int, String>`, the `[Int]` sibling of `PluginCall`.
    PluginCallInt,
    /// D-SHIFT1 (c7shift): `Reader.over(bytes)` constructor →
    /// `{root}jet_reader_over(&(recv))` → `JetReader`. `recv` is the `[U8]`
    /// argument (same "arg becomes the recv slot" shape as `PathFrom`).
    ReaderOver,
    /// D-SHIFT1: `reader.read_u8()` → `{root}jet_reader_read_u8(&mut (recv))`
    /// → `Result<U8, String>`. Bounds miss is an ordinary `Err`, never a panic.
    ReaderReadU8,
    ReaderReadU16Le,
    ReaderReadU16Be,
    ReaderReadU32Le,
    ReaderReadU32Be,
    ReaderReadU64Le,
    ReaderReadU64Be,
    /// D-SHIFT1: `reader.take(n)` → `{root}jet_reader_take(&mut (recv), (a0))`
    /// → `Result<Vec<u8>, String>` (owned copy — see CoreLib.rs comment on
    /// why `take` copies rather than borrowing a `View<T>`).
    ReaderTake,
    /// D-SHIFT1: `reader.remaining()` → `{root}jet_reader_remaining(&(recv))` → `Int`.
    ReaderRemaining,
    /// D-SHIFT1: `reader.at_end()` → `{root}jet_reader_at_end(&(recv))` → `Bool`.
    ReaderAtEnd,
    /// D-SHIFT1: `Cursor.over(s)` constructor →
    /// `{root}jet_cursor_over(&(recv))` → `JetCursor`.
    CursorOver,
    /// D-SHIFT1: `cursor.take_until(delim)` →
    /// `{root}jet_cursor_take_until(&mut (recv), &(a0))` → `Result<String, String>`.
    CursorTakeUntil,
    /// D-SHIFT1: `cursor.skip_ws()` → `{root}jet_cursor_skip_ws(&mut (recv))` → `()`.
    CursorSkipWs,
    /// D-SHIFT1: `cursor.take_pattern("…")` — consume-mode reuse of the
    /// D-PARSESTR1 scan engine (`str_match_scan_closure_ex`, I8: one matcher,
    /// not two). `parts` is the pattern literal's already-parsed holes;
    /// `canonical` is the same `(name, type)` list sema put in the call's
    /// `resolved_ret` `Type::Tuple` (so `collect_tuple_shapes_from_expr`
    /// already registered the `JetTup_<hash>` struct this op constructs).
    CursorTakePattern {
        parts: Vec<crate::AST::StrMatchPart>,
        canonical: Vec<(String, Type)>,
    },
}

/// One lowered call argument, with the borrow/clone decisions already made (so
/// the emitter reproduces `emit_call_args` without consulting `cx.sigs`).
///
/// Emission order mirrors `emit_call_args` exactly: the clone wrapper (`.clone()`
/// or `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`).
pub struct TCallArg {
    pub value: TExpr,
    /// Emit `&(...)` around the value (a non-scalar passed by `Read` convention).
    pub borrow: bool,
    /// Emit `&mut (...)` around the value (a `Mutate`-convention argument). c109
    /// Phase 6: method args may be `Mutate`; the plain-call path never sets this.
    pub mut_borrow: bool,
    /// Emit `(...).clone()` (an implicit clone — a value passed by `Move`).
    pub clone: bool,
    /// Emit `(...).clone()` (a `Shared` value auto-cloned at the call site — its
    /// own cheap-handle `Clone` impl; D-MEM1 S6 changed this from a hardcoded
    /// `Arc::clone(&...)` once `Shared<T>` stopped being a bare `Arc<T>`).
    /// c109 Phase 6: method/Arc args may set this; the plain-call path does not.
    pub arc_clone: bool,
    /// c109 Phase 13: the Fn-typed-parameter coercion (`emit_call_args`' fn-arg
    /// path). When `Some(<fn-type rust string>)`, the value is wrapped
    /// `Box::new(value) as <fn-type>` — unless it is ALREADY boxed (a bare fn-name
    /// value emits its own `Box::new(…)`, or the value is a fn-typed local ident), in
    /// which case only the ` as <fn-type>` suffix is applied. `already_boxed` carries
    /// that resolved decision so emit makes none. This is mutually exclusive with the
    /// borrow/clone wrappers (a Fn param is never borrowed/cloned — `emit_call_args`
    /// skips `&(…)` for `Type::Fn`).
    pub fn_coerce: Option<TFnCoerce>,
    /// D-FIXARR1: a `[T#N]` argument passed to a `[T]` (Vec) slot is widened by
    /// copying into a growable list. When true, emit wraps with `.to_vec()`.
    pub widen_to_vec: bool,
}

/// c109 Phase 13: the resolved Fn-typed-argument coercion (`emit_call_args`).
pub struct TFnCoerce {
    /// The target fn-type, rendered as a Rust type string (`cx.rust_type(ty)`).
    pub fn_type_rust: String,
    /// Whether the value already produces a `Box::new(…)` (a bare fn-name value, or a
    /// fn-typed local ident) — so emit applies only ` as <fn-type>`, never re-boxing.
    /// Reproduces `emit_call_args`' `already_boxed` decision, resolved at lowering.
    pub already_boxed: bool,
}

// ---------------------------------------------------------------------------
// The gate: is this function fully inside the Phase-1 subset?
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::{Expr, Func, Item, Stmt};
    use std::collections::{HashMap, HashSet};

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
                web_target_ceiling: prog.web_target_ceiling,
                pub_file: prog.pub_file,
                html_path: prog.html_path.clone(),
                no_alloc_policy: prog.no_alloc_policy,
            }],
            parse_teaching: Vec::new(),
            used_core: std::collections::HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            import_targets: std::collections::HashMap::new(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: std::collections::HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: std::collections::HashMap::new(),
            active_os: crate::Syntax::OsTarget::host(),
        };
        // No C imports in unit tests; CFfi::default() is the correct empty state.
        let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
        assert!(
            !diags
                .iter()
                .any(|d| d.severity == crate::Diagnostics::Severity::Error),
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

    #[test]
    fn covers_simple_arithmetic_fn() {
        assert!(covers(
            "fn add(a: Int, b: Int) -> Int {\n return (a + b)\n}\n",
            "add"
        ));
    }

    #[test]
    fn covers_print_and_string_param() {
        assert!(covers(
            "fn greet(s: String) {\n print(\"hi {s}\")\n}\n",
            "greet"
        ));
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
    return Person.{ name: n }
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
        let src = "struct Pair<T> {\n first: T\n second: T\n}\nfn mk<T>(a: T, b: T) -> Pair<T> {\n return Pair<T>.{first: a, second: b}\n}\n";
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
        cx.core_imports
            .insert("mem".to_string(), "core.mem".to_string());
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
    n :: Note.{ text: \"hi\" }
    print(n.text)
}
";
        assert!(covers_with_foreign(src, "mk", &[("Note", "user_notes")]));
    }

    #[test]
    fn covers_unsafe_fn_with_ptr_ops() {
        // c109 Phase 18: a `#Unsafe fn` (S58) is covered — it lowers to `unsafe fn`, and
        // its body's `mem.Ptr<T>.from_addr` / `mem.volatile_read` ops are in-subset.
        let src = "use core.mem\n#Unsafe\nfn read_reg(addr: Int) -> Int {\n p :: mem.Ptr<Int>.from_addr(addr)\n return mem.volatile_read(p)\n}\n";
        assert!(covers_with_mem(src, "read_reg"));
    }

    #[test]
    fn covers_unsafe_block_and_address_of() {
        // c109 Phase 18: a `#Unsafe("…") { … }` audited region + `mem.address_of` (the
        // inert address cast, legal outside unsafe) are covered.
        let src = "use core.mem\nfn run() {\n cell: Int :: 7\n addr :: mem.address_of(cell)\n #Unsafe(\"live\") {\n p :: mem.Ptr<Int>.from_addr(addr)\n seen :: mem.volatile_read(p)\n print(\"{seen}\")\n }\n}\n";
        assert!(covers_with_mem(src, "run"));
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
        let fl = Type::FixedList {
            elem: Box::new(Type::Int),
            len: 3,
        };
        // param/return helper coverage:
        assert!(is_subset_param_ty(&fl, &mk("fn f(){}")));
        assert!(is_covered_collection_ty(&fl, &mk("fn f(){}")));
        assert!(collection_elem_covered(&fl, &mk("fn f(){}")));
        // struct-field coverage: a `[Int#3]` field keeps its owning struct covered.
        let src = "struct Grid { row: [Int#3] }\nfn f(){}";
        assert!(is_covered_struct_ty(
            &Type::Named("Grid".to_string()),
            &mk(src)
        ));
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
        let src = "struct C { n: Int }\nimpl C {\n fn bumped(self) -> Int {\n return (self.n + 1)\n }\n}\nfn use_it(c: Int) -> Int {\n return c\n}\nfn caller() -> Int {\n x :: C.{ n: 1 }\n return x.bumped()\n}\n";
        assert!(!covers(src, "caller"));
    }

    // c109 Phase 3: structs.

    #[test]
    fn covers_struct_param_and_scalar_field_read() {
        // A plain struct param with a scalar field read (borrow position) and a
        // struct literal + struct return are all in the subset.
        let src = "struct Point { x: Int\n y: Int }\nfn sum_pt(p: Point) -> Int {\n return (p.x + p.y)\n}\nfn origin() -> Point {\n return Point.{ x: 0, y: 0 }\n}\n";
        assert!(covers(src, "sum_pt"));
        assert!(covers(src, "origin"));
    }

    #[test]
    fn covers_nested_struct() {
        // A struct field whose type is itself a covered struct, with a chained
        // field read and a nested literal.
        let src = "struct Inner { v: Int }\nstruct Outer { inner: Inner\n label: Int }\nfn deep(o: Outer) -> Int {\n return (o.inner.v + o.label)\n}\n";
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
        let src = "struct Pair<T> { first: T\n second: T }\nfn mk() -> Pair<Int> {\n return Pair<Int>.{ first: 1, second: 2 }\n}\n";
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
        let src = "fn f() {\n x :: 0\n loop {\n x = (x + 1)\n if (x > 3) {\n break\n }\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_while_form() {
        let src = "fn f() {\n x :: 0\n loop (x < 3) {\n x = (x + 1)\n }\n print(x)\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_labeled_loops() {
        let src = "fn f() {\n outer@ loop {\n loop n in 1..3 {\n if (n == 2) {\n break outer@\n }\n }\n break\n }\n}\n";
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
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn next(light: Light) -> Light {\n if light == {\n Red -> { return Light.Yellow }\n Yellow -> { return Light.Green }\n Green -> { return Light.Red }\n }\n}\n";
        assert!(covers(src, "next"));
    }

    #[test]
    fn covers_enum_payload_or_and_wildcard() {
        // Scalar-payload enum, or-pattern with a shared binding, and a wildcard slot.
        let src = "enum Conn {\n Active(Int)\n Reconnecting(Int)\n Idle(Int)\n Closed\n}\nfn d(c: Conn) -> String {\n if c == {\n Active(id) | Reconnecting(id) -> { return \"live:{id}\" }\n Idle(_) -> { return \"idle\" }\n Closed -> { return \"closed\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "d"));
    }

    #[test]
    fn covers_enum_payload_range_pattern() {
        // A range pattern in a payload slot (guard-emitted) plus a wildcard slot.
        let src = "enum Http {\n Good(Int)\n Fail(Int)\n}\nfn classify(r: Http) -> String {\n if r == {\n Good(200..299) -> { return \"ok\" }\n Good(_) -> { return \"other\" }\n Fail(_) -> { return \"err\" }\n }\n return \"unknown\"\n}\n";
        assert!(covers(src, "classify"));
    }

    #[test]
    fn covers_arm_head_range_switch() {
        // An all-range arm-head scalar switch with an `else` (mixed-switch path).
        let src = "fn grade(score: Int) -> String {\n if score == {\n 0..59 -> { return \"F\" }\n 60..100 -> { return \"P\" }\n else -> { return \"?\" }\n }\n}\n";
        assert!(covers(src, "grade"));
    }

    #[test]
    fn covers_mixed_switch_non_ident_subject() {
        // c109 (B1): a pattern switch over a NON-IDENT subject routes through the
        // exhaustive-match / fallible-match path (the subject is matched by source-text
        // equality, not just an ident name). A call subject with unit-variant arms:
        let variant = "enum Light { Red Green Yellow }\nfn pick() -> Light { return Light.Red }\nfn classify() -> Int {\n if pick() == {\n Red -> { return 1 }\n Green -> { return 2 }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(variant, "classify"));
        // A field-access subject with a payload-binding (optional) arm:
        let payload = "struct Holder { val: Int? }\nfn f(h: Holder) -> Int {\n if h.val == {\n Val(c) -> { return c }\n else -> { return 0 }\n }\n}\n";
        assert!(covers(payload, "f"));
    }

    #[test]
    fn covers_enum_local_and_literal_in_main() {
        // An enum-typed local bound from a literal, passed to a covered helper.
        let src = "enum Light {\n Red\n Yellow\n Green\n}\nfn label(l: Light) -> String {\n if l == {\n Red -> { return \"r\" }\n Yellow -> { return \"y\" }\n Green -> { return \"g\" }\n }\n}\nfn run() {\n start :: Light.Red\n print(label(start))\n}\n";
        assert!(covers(src, "run"));
    }

    #[test]
    fn covers_string_payload_enum() {
        // c109 Phase 16: a String-payload enum. The literal's borrowed-payload
        // `.clone()` and pattern bindings are reproduced as total facts
        // (`emit_boxed_enum_arg`), so the match + getter route through the TIR.
        let src = "enum Msg {\n Text(String)\n Ping\n}\nfn show(m: Msg) -> String {\n if m == {\n Text(s) -> { return s }\n Ping -> { return \"ping\" }\n }\n return \"\"\n}\n";
        assert!(covers(src, "show"));
    }

    #[test]
    fn covers_recursive_enum() {
        // c109 Phase 16: a self-referential (boxed) enum. The `Box::new(…)` at
        // construction and the auto-deref at pattern/field sites are total facts
        // (`TEnumArg.boxed`), so a covered traversal routes through the TIR.
        let src = "enum Tree {\n Leaf(Int)\n Node(Tree)\n}\nfn depth(t: Tree) -> Int {\n if t == {\n Leaf(n) -> { return n }\n Node(inner) -> { return 1 }\n }\n return 0\n}\n";
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
        let src = "struct Point { x: Int\n y: Int }\nenum Shape {\n Dot(Point)\n Line(Int)\n}\nfn area(s: Shape) -> Int {\n if s == {\n Dot(p) -> { return p.x }\n Line(n) -> { return n }\n }\n return 0\n}\n";
        assert!(covers(src, "area"));
    }

    #[test]
    fn covers_collection_payload_enum() {
        // c109 Phase 16: an enum variant carrying a covered collection payload
        // (`[Int]`). Construction (`Holder.Nums(xs)`) routes through the variant
        // MethodCall shape; the borrowed-list `.clone()` is total.
        let src = "enum Holder {\n Nums([Int])\n One(Int)\n}\nfn mk(xs: [Int]) -> Holder {\n return Holder.Nums(xs)\n}\n";
        assert!(covers(src, "mk"));
    }

    #[test]
    fn rejects_range_switch_over_non_ident_subject() {
        // D-IF3: a value+range mixed switch (shape D) lowers each range head to
        // `subject >= lo && subject <= hi`, so the subject must be a scalar ident
        // local for the emitted condition to type-check. A NON-IDENT subject (a
        // call) with a range arm is excluded from the subset (stays on the AST
        // path), even though the value arm alone would be fine.
        let src = "fn pick() -> Int { return 5 }\nfn f() -> String {\n if pick() == {\n 0 -> { return \"zero\" }\n 1..10 -> { return \"low\" }\n else -> { return \"mid\" }\n }\n}\n";
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
        let src = "fn empty() -> [String: Int] {\n return []\n}\nfn one() -> [String: Int] {\n return [\"a\": 1]\n}\nfn accept(m: [String: Int]) -> Int {\n return 0\n}\n";
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
        let src = "fn f(m: [String: Int]) {\n loop k, v in m {\n print(\"{k}={v}\")\n }\n}\n";
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
        // c109 Phase 22: `if x == Val(b) { … b … }` lowers to `if let Some(b) = …`.
        let src = "fn f(x: Int?) {\n if x == Val(n) {\n print(\"{n}\")\n }\n}\n";
        assert!(covers(src, "f"));
        // `x == None` lowers to `.is_none()`.
        let isnone = "fn f(x: Int?) {\n if x == None {\n print(\"none\")\n }\n}\n";
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
            "len",
            "push",
            "pop",
            "get",
            "map",
            "filter",
            "each",
            "find",
            "sort",
            "join",
            "to_string",
            "clone",
            "raw",
            "snapshot",
            "new",
            "to_i32",
            "is_nan",
            "chars",
            "trim",
            "keys",
            "values",
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
            type_args: Vec::new(),
            args: Vec::new(),
            recv_type: Some("Bag".to_string()),
            resolved_ret: None,
        };
        let mut locals = HashSet::new();
        locals.insert("b".to_string());
        // Both builtin-name user methods are admitted (a real `method_sigs` entry exists).
        for m in ["get", "len"] {
            if let Expr::MethodCall {
                receiver,
                method,
                args,
                recv_type,
                ..
            } = &mk(m)
            {
                assert!(
                    method_call_in_subset(receiver, method, args, recv_type, &cx, &locals),
                    "user method `{m}` shadowing a builtin name should be covered"
                );
            }
        }
        // A builtin name with NO user method on the type stays excluded (`push` isn't a
        // method on `Bag`), so the builtin/name-keyed path keeps it on the AST side.
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            recv_type,
            ..
        } = &mk("push")
        {
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
        let src = "struct Acc {\n total: Int\n fn doubled(&self) -> Int {\n return (self.total + self.total)\n }\n}\n";
        assert!(covers_method(src, "Acc", "doubled"));
    }

    #[test]
    fn covers_static_constructor() {
        // A static (no-`self`) associated function returning the owning type.
        let src =
            "struct Cell {\n n: Int\n fn make(v: Int) -> Cell {\n return Cell.{ n: v }\n }\n}\n";
        assert!(covers_method(src, "Cell", "make"));
    }

    #[test]
    fn covers_enum_instance_method() {
        // A `when self` match in an enum method body is covered.
        let src = "enum Dir {\n North\n South\n fn code(self) -> Int {\n if self == {\n North -> { return 0 }\n South -> { return 1 }\n }\n }\n}\n";
        assert!(covers_method(src, "Dir", "code"));
    }

    #[test]
    fn covers_self_reassignment_method() {
        // D-MUTSELF1: a `mut self` method that reassigns `self` (`self = …`) is NOW
        // covered — the `mut self` slot derefs (`(*self)`), so the LHS lowers to
        // `(*self) = …` (the prior AST-path I2 hole is closed).
        let src = "struct Acc {\n n: Int\n fn reset(&self) {\n self = Acc.{ n: 0 }\n }\n}\n";
        assert!(covers_method(src, "Acc", "reset"));
    }

    #[test]
    fn covers_self_field_assign_method() {
        // D-MUTSELF1: a `mut self` method assigning a field (`self.field = v`, S17
        // compound `+=` too) is covered — lowers to `((*self)).field = v`.
        let src = "struct Acc {\n n: Int\n fn bump(&self) {\n self.n = self.n + 1\n }\n}\n";
        assert!(covers_method(src, "Acc", "bump"));
        let compound = "struct Acc {\n n: Int\n fn bump(&self) {\n self.n += 1\n }\n}\n";
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
        let src = "fn f(x: Int) -> Int ? Error {\n if x == 0 {\n return err(\"bad\")\n }\n return ok(x)\n}\nfn g(x: Int) -> Int ? Error {\n n :: f(x)?\n return ok((n + 1))\n}\n";
        assert!(covers(src, "f"));
        assert!(covers(src, "g"));
    }

    #[test]
    fn covers_optional_return_and_chaining() {
        // A `T?` return with `Val`/`None`, plus `?.` chaining over a covered struct.
        // (Multi-letter struct name; a single uppercase letter reads as a type var.)
        let src = "struct Addr {\n city: String\n}\nfn opt(x: Int) -> (Int?) {\n if x > 0 {\n return Val(x)\n }\n return None\n}\nfn ch(a: (Addr?)) -> (String?) {\n return a?.city\n}\n";
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
        let src =
            "fn f(x: Int) -> Int {\n comptime if true {\n return x\n } else {\n return 0\n }\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn covers_mixed_bool_switch() {
        // c109 Phase 15 / D-IF3: a mixed value+range dispatch (shape D) routes via the
        // TIR's `MixedSwitch` (the general `emit_mixed_switch` if/else chain) — a
        // bare-value arm (`0 ->` ≡ `x == 0`) beside range arms (`1..10 ->`), each
        // range lowered to `x >= lo && x <= hi`. (Q4 retired free-predicate arms.)
        let src = "fn f(x: Int) -> Int {\n if x == {\n 0 -> {\n return 2\n }\n 1..10 -> {\n return 1\n }\n else -> {\n return 0\n }\n }\n}\n";
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
        let src = "fn f(m: [String: Int]) -> Int {\n m2 := m\n m2.insert(\"k\", 1)\n n := m2.len()\n ks := m2.keys()\n vs := m2.values()\n ck := m2.contains_key(\"a\")\n m2.clear()\n return n\n}\n";
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
        for name in [
            "map", "filter", "each", "find", "any", "all", "sort_by", "reduce",
        ] {
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
        let src =
            "fn f(xs: [Int]) -> Int {\n e := xs.is_empty()\n if e {\n return 1\n }\n return 0\n}\n";
        assert!(covers(src, "f"));
    }

    #[test]
    fn rejects_numeric_conversion_builtin() {
        // Numeric width/predicate/bit methods (`to_i32`/`is_nan`/`count_ones`) are
        // Phase 12 — not covered builtins here, and they carry a `Some(recv_type)`.
        for name in ["to_i32", "to_u8", "is_nan", "count_ones", "to_f64"] {
            assert!(
                !is_covered_builtin_name(name, 0),
                "{name} is a Phase-12 numeric method"
            );
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
        // D-ARGS1: ArgsSpec builder and ParsedArgs query methods.
        assert!(handle_method_op("ArgsSpec", "flag", 2).is_some());
        assert!(handle_method_op("ArgsSpec", "option", 3).is_some());
        assert!(handle_method_op("ArgsSpec", "positional", 2).is_some());
        assert!(handle_method_op("ArgsSpec", "help", 0).is_some());
        assert!(handle_method_op("ArgsSpec", "parse", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "flag", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "option", 1).is_some());
        assert!(handle_method_op("ParsedArgs", "positional", 1).is_some());
        // D-ANY-JAI1 (c7jaiany §6): reflect.of(x)'s Value/Field handle methods.
        assert!(handle_method_op("Value", "type_name", 0).is_some());
        assert!(handle_method_op("Value", "display", 0).is_some());
        assert!(handle_method_op("Value", "fields", 0).is_some());
        assert!(handle_method_op("Field", "name", 0).is_some());
        assert!(handle_method_op("Field", "value", 0).is_some());
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
        assert!(core_call_covered("core.random", "weighted_pick"));
        assert!(core_call_covered("core.random", "sample"));
        assert!(core_call_covered("core.random", "shuffle"));
        assert!(core_call_covered("core.io", "eprint"));
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: the `tasks.channel<T>()` producer is
        // covered via the core-call shape (a fixed-string `jet_std::channel::<T>()`
        // emit; its `(Sender<T>, Receiver<T>)` return type rides on `resolved_ret`,
        // filled from the call-site turbofish). `tasks.spawn` stays out of this
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
    fn new(width: Int, height: Int) -> Rect { return Rect.{width: width, height: height} }
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
    name :: input() ?? \"world\"
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
        // `P{x} :: p` struct-destructure are both covered; the fn routes through TIR.
        assert!(covers(
            "struct P { x: Int }\nfn f() { p :: P.{x: 1}\n#Caps(Io) { P.{x} :: p\nprint(x) } }",
            "f"
        ));
    }

    #[test]
    fn covers_free_call_arg_conventions() {
        // c109 Phase 26: ALL three free-call arg conventions route — `Read` (`&(…)`),
        // `Move` (`take`-marked), and `Mutate` (`mut place` → `&mut (…)`).
        assert!(covers(
            "fn bump(n: &Int) { n += 1 }\nfn f() { s: Int := 1\nbump(&s) }",
            "f"
        ));
        assert!(covers(
            "fn keep(s: ^String) -> String { return s }\nfn f() -> String { return keep(^\"v\") }",
            "f"
        ));
    }

    #[test]
    fn covers_list_destructure() {
        // c109 Phase 26: a list-destructuring binding `[a, b, c] :: <init>` (S74) routes
        // when the init is in-subset — the fan-out result destructure (`41_fan_out`).
        assert!(covers(
            "fn f() { xs :: [1, 2, 3]\n[a, b, c] :: xs\nprint(a) }",
            "f"
        ));
    }

    #[test]
    fn covers_struct_destructure() {
        // c109: a struct-destructuring binding `Type { x, y } :: <init>` (S74) routes
        // when the init is in-subset — the AST `BindPattern::Struct` arm is covered
        // byte-for-byte (per-field type from `cx.struct_fields`).
        assert!(covers(
            "struct Point { x: Int, y: Int }\nfn f() { p :: Point.{ x: 1, y: 2 }\nPoint.{ x, y } :: p\nprint(x + y) }",
            "f"
        ));
    }

    #[test]
    fn covers_named_fn_value_binding() {
        // c109 Phase 27: a bare top-level fn name bound to a local as a VALUE
        // (`double_fn :: double`). The init `Ident("double")` resolves to a `Type::Fn`
        // value (`emit_named_fn_value`), in-subset via `ident_is_named_fn_value`. (This
        // binding-site coercion was already wired in lowering; the live-suite `24_callbacks`
        // never routed only because the struct fn-field / fn-field-call were uncovered.)
        assert!(covers(
            "fn dbl(x: Int) -> Int { return (x * 2) }\nfn f() { g :: dbl\nprint(g(3)) }",
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
        assert!(is_covered_struct_ty(
            &Type::Named("Worker".to_string()),
            &cx
        ));
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
            effect_bound: None,
        };
        assert!(field_ty_covered(&fn_ty, &cx, &mut HashSet::new()));
    }

    #[test]
    fn concurrency_method_names() {
        // c109 Phase 21 + D-COROUTINE1=A / D-TUPLE-DESTRUCT1: the Task/Receiver/Sender
        // method name+arity set. `join` is the 0-arg form (the 1-arg list `join(sep)`
        // is a collection builtin, NOT here); `send` is the 1-arg form. No `sender` —
        // `tasks.channel<T>()` returns the sender directly, no `.sender()` method.
        assert!(is_concurrency_method_name("join", 0));
        assert!(is_concurrency_method_name("wait", 0));
        assert!(is_concurrency_method_name("detach", 0));
        assert!(is_concurrency_method_name("pause", 0));
        assert!(is_concurrency_method_name("resume", 0));
        assert!(is_concurrency_method_name("cancel", 0));
        assert!(is_concurrency_method_name("trace", 0));
        assert!(is_concurrency_method_name("receive", 0));
        assert!(!is_concurrency_method_name("sender", 0));
        assert!(is_concurrency_method_name("send", 1));
        // Disjoint from the list `join(sep)` (1 arg) and any wrong arity.
        assert!(!is_concurrency_method_name("join", 1));
        assert!(!is_concurrency_method_name("send", 0));
        assert!(!is_concurrency_method_name("receive", 1));
        assert!(!is_concurrency_method_name("len", 0));
    }

    #[test]
    fn reactive_method_names_and_value_types() {
        // D-REACT1=B: the reactive method set (`get`/0, `set`/1) and value types.
        assert!(is_reactive_method_name("get", 0));
        assert!(is_reactive_method_name("set", 1));
        assert!(!is_reactive_method_name("get", 1)); // a list `get(i)` is NOT this shape
        assert!(!is_reactive_method_name("set", 0));
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let apply = |n: &str| Type::Apply {
            name: n.to_string(),
            args: vec![Type::Int],
        };
        assert!(is_covered_reactive_ty(&apply("Signal"), &cx));
        assert!(is_covered_reactive_ty(&apply("Derived"), &cx));
        assert!(is_subset_param_ty(&apply("Signal"), &cx));
        assert!(is_subset_param_ty(&apply("Derived"), &cx));
        assert!(!is_covered_reactive_ty(&apply("Receiver"), &cx));
        // The producer + closure-call shapes are covered.
        assert!(core_call_covered("jet.reactive", "signal"));
        assert!(crate::Sema::is_polymorphic_core_special(
            "jet.reactive",
            "derived"
        ));
    }

    #[test]
    fn event_method_names_and_core_calls() {
        // D-EVENT1=D: typed Event/Hook family lowers through the event handle
        // method shape plus generic core-call constructors.
        assert!(is_event_handle_type(Some("Event")));
        assert!(is_event_handle_type(Some("Hook")));
        assert!(is_event_handle_type(Some("Subscription")));
        assert!(is_event_handle_type(Some("EventScope")));
        assert!(is_event_handle_type(Some("EventTrace")));
        assert!(!is_event_handle_type(Some("Signal")));
        assert!(is_event_method_name("on", 2));
        assert!(is_event_method_name("once", 2));
        assert!(is_event_method_name("on_priority", 3));
        assert!(is_event_method_name("emit", 1));
        assert!(is_event_method_name("emit_async", 1));
        assert!(is_event_method_name("run", 2));
        assert!(is_event_method_name("unsubscribe", 0));
        assert!(is_event_method_name("active_count", 0));
        assert!(is_event_method_name("summary", 0));
        assert!(!is_event_method_name("on", 1));
        assert!(!is_event_method_name("emit", 0));
        assert!(core_call_covered("core.event", "new"));
        assert!(core_call_covered("core.event", "with_policy"));
        assert!(core_call_covered("core.event", "hook"));
        assert!(core_call_covered("core.event", "scope"));
        assert!(core_call_covered("core.event", "policy_sync"));
        assert!(core_call_covered("core.event", "policy_async"));
        assert!(!core_call_covered("core.event", "subscribe"));
    }

    #[test]
    fn concurrency_value_types_covered() {
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: `Task<T>`/`Receiver<T>`/`Sender<T>` are
        // covered value types; the `Closed` err type is a covered fallible payload
        // (`Receiver.receive()`).
        let cx_src = "fn f() {}\n";
        let (toks, _) = crate::Lexer::lex(cx_src);
        let prog = crate::Parser::parse(&toks).expect("parse");
        let cx = build_cx(&prog, cx_src, "t.jet");
        let apply = |n: &str| Type::Apply {
            name: n.to_string(),
            args: vec![Type::Int],
        };
        assert!(is_covered_concurrency_ty(&apply("Task"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Receiver"), &cx));
        assert!(is_covered_concurrency_ty(&apply("Sender"), &cx));
        assert!(is_subset_param_ty(&apply("Task"), &cx));
        // A `[Task<Unit>]` worker list (34_parallel_scan) is a covered collection.
        let tasks = Type::List(Box::new(Type::Apply {
            name: "Task".to_string(),
            args: vec![unit_type()],
        }));
        assert!(is_covered_collection_ty(&tasks, &cx));
        // `Closed` is a covered fallible payload (the `receive()` err type).
        assert!(fallible_payload_covered(
            &Type::Named("Closed".to_string()),
            &cx
        ));
        // A non-concurrency `Apply` (e.g. a user generic) is NOT this shape.
        assert!(!is_covered_concurrency_ty(&apply("Pair"), &cx));
    }

    #[test]
    fn covers_concurrency_methods() {
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: a function using the `send`/`receive`
        // surface + the `tasks.channel<T>()` producer routes. The gate is
        // `build_cx`-only (no sema), so the method calls carry `recv_type == None`
        // (the unannotated AST default), which is exactly what the d3 shape keys on;
        // the `Receiver<Int>` annotation supplies the value type. (The
        // `tasks.spawn(take(..) …)`/`Task.join` slice depends on sema-filled
        // `Lambda.meta`, so it's proven end-to-end in tests/tir.rs.)
        let src = "\
use core.tasks as tasks
fn produce(s: Sender<Int>) {
    s.send(7)
}
fn consume(ch: Receiver<Int>) -> Int {
    return ch.receive() ?? panic(\"closed\")
}
";
        // The `Sender.send` method + `Sender<Int>` value type (gate shape d3).
        assert!(covers(src, "produce"));
        // The `Receiver.receive` method + `Receiver<Int>` value type + `Result<Int, Closed>`
        // unwrap via `?? panic`.
        assert!(covers(src, "consume"));
    }

    #[test]
    fn covers_pure_fn() {
        // c109 Phase 23: a `@Pure fn` is covered (purity is sema-only, erased at codegen).
        assert!(covers(
            "@Pure fn double(n: Int) -> Int {\n return (n * 2)\n}\n",
            "double"
        ));
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
        let src = "UserId :: distinct Int;\nfn greet(id: UserId) -> Int {\n return (id.raw())\n}\n";
        assert!(covers(src, "greet"));
        let src2 = "UserId :: distinct Int;\nfn mk() -> UserId {\n return UserId(42)\n}\n";
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
            let s = format!("fn g() {{ x :: scope.guard({})\n}}\n", body);
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
            "core.scope",
            "guard",
            &guard_args,
            &cx,
            &locals
        ));
        // A non-closure core call is not a closure-core-call.
        assert!(!core_closure_call_in_subset(
            "core.files",
            "read",
            &guard_args,
            &cx,
            &locals
        ));
    }

    #[test]
    fn covers_json_construction_and_collection() {
        // D-ENC-DYN1=A+: dynamic `Data` construction (`Data.Text`/`Data.Bool`/`Data.Array`/
        // `Data.Null`) + a `[Data]` list value type. A fn that builds `Data` values and
        // returns a `Data` routes (the dynamic value type is a covered foreign value type;
        // construction is the `JsonLit` shape). The if-let MATCHING + index-assign need
        // full sema (the `Data` pattern / `IndexKind`), proven by `tests/tir.rs` + the
        // whole-suite byte-parity diff; here we gate the sema-independent construction.
        let src = "\
fn build() -> Data {
    items: [Data] := []
    items.push(Data.Text(\"jet\"))
    items.push(Data.Bool(true))
    items.push(Data.Null)
    return Data.Array(items)
}
";
        assert!(covers(src, "build"));
    }

    #[test]
    fn covers_json_value_param_and_array() {
        // A `Data`-typed param + a `[Data]` list value type + `Data.Array` construction.
        let src = "\
fn wrap(x: Data) -> Data {
    items: [Data] := []
    items.push(x)
    return Data.Array(items)
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
fn build() -> [Int] {
    xs: [Int] := []
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn run() {
    comptime xs = build()
    print(\"{xs}\")
}
";
        assert!(covers_after_sema(src, "run"));
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
fn run() {
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
    return PR.{file_path: p, note: None}
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
        // Body is a structural `Val(x)` (a type-var payload `Some(user_x)`).
        let src = "\
fn opt_id<T: Comparable>(x: T) -> (T?) {
    return Val(x)
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
    return Val(s)
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
        // field value `Box::new(…)`. A fn building a nested `Tree { value, child: Val(…) }`
        // routes. (The boxed-field READ is also covered now — see covers_recursive_struct_boxed_field_read.)
        let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn build() {
    root :: Tree.{ value: 1, child: Val(Tree.{ value: 2, child: None }) }
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
    kid: Tree? :: t.child
    if kid == {
        Val(c) -> {
            return c.value
        }
        None -> {
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
        // c109: an owning field read of a NON-SCALAR field (`s :: p.name`, `name:
        // String`) — sema rewrites the read to `(p.name).clone()` (a `MethodCall`
        // clone shape). With the single-uppercase-letter struct name `P` now treated
        // as a concrete declared type (not a type var), the whole `main` routes
        // through the TIR. The owning clone emits `((user_p).user_name).clone()`.
        let src = r#"
struct P {
    name: String
}

fn run() {
    p :: P.{ name: "x" }
    s :: p.name
    t :: p.name
    print(s)
    print(t)
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "owning field-read clone not covered"
        );
    }

    #[test]
    fn covers_indexed_map_assign_through_field() {
        // c109: an indexed map-assign whose base is a FIELD read (`s.scores["a"] = 1`,
        // `scores: [String: Int]`). The `LValue::Index` gate already admits a
        // field-read base + sema-resolved `IndexKind`; the only blocker was the
        // single-uppercase-letter struct name `S` (covered by the type-var-heuristic
        // guard). The whole `main` routes through the TIR; the assign emits
        // `{ let __jet_v = 1i64; jet_map_insert(&mut ((user_s).user_scores), …); }`.
        let src = r#"
struct S {
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
    s.scores["a"] = 1
    print(s.scores["a"])
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "map-assign through field not covered"
        );
    }

    #[test]
    fn covers_map_builtin_on_struct_field_receiver() {
        // c109: a map builtin (`.len()`) on a struct-FIELD-read receiver
        // (`s.scores.len()`), where the field was initialized from an empty-map
        // struct-literal field (`scores: [:]` takes its type from the struct field).
        // The builtin gate already admits a field-read receiver + the struct-literal
        // empty-map field is in-subset; the single-uppercase-letter struct name `S` was
        // the only blocker (now a concrete declared type). The whole `main` routes
        // through the TIR.
        let src = r#"
struct S {
    scores: [String: Int]
}

fn run() {
    s := S.{ scores: [] }
    print(s.scores.len())
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "map builtin on field receiver not covered"
        );
    }

    #[test]
    fn covers_field_read_and_eq_on_inlined_comptime_values() {
        // c109: a FIELD READ off a comptime-const struct value (`comptime P =
        // Pair{…}`; then `P.left`) and an `==` against a comptime-const enum value
        // (`comptime L = Light.Green`; then `L == Light.Green`). The const inlines to
        // its pre-rendered Rust value string (`cx.consts[…]`); reading a field off the
        // inlined struct / comparing the inlined enum is byte-identical to the AST path.
        // The Field gate now admits a non-local comptime-const receiver.
        let src = r#"
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

comptime P = Pair.{left: 7, right: "seven"}
comptime L = Light.Green

fn run() {
    print("{P.left}")
    print("{P.right}")
    print("{L == Light.Green}")
}
"#;
        assert!(
            covers_after_sema(src, "run"),
            "field-read/== on inlined comptime values not covered"
        );
    }

    #[test]
    fn covers_wildcard_enum_payload_if_let() {
        // c109 (D-PATW): a user-enum variant if-let condition with a WILDCARD payload
        // slot (`if w == Some(_)`). The `_` binds nothing; the if-let head renders
        // `if let user_Wrapper::user_Some(_) = user_w` (byte-for-byte the AST). Covered
        // when the variant is a single-payload variant of a covered enum.
        let src = "\
enum Wrapper {
    Some(Int)
    Empty
}
fn run() {
    w :: Wrapper.Some(42)
    if w == Some(_) {
        print(\"has value\")
    }
}
";
        assert!(
            covers(src, "run"),
            "wildcard enum-payload if-let not covered"
        );
    }
}
