//! Semantic checks. Everything here exists so that codegen can stay "dumb"
//! (invariant I3): by the time a Program reaches codegen, it must be
//! impossible for the generated Rust to fail to compile (invariant I2).
//!
//! M1: type inference, mutability, comparison distribution (S25),
//! definite-return analysis. M2: ownership — moves, call-site `mut`/`take`,
//! view returns, use-after-move, and borrow rules that keep generated Rust
//! sound without surfacing Rust concepts to users.

use crate::AST::{
    AccessConvention,
    ExternFn, Func, Type, VariantPayload,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Traits::TraitRegistry;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    /// S50: declared in `extern rust`, implemented by the FFI bridge.
    pub is_extern: bool,
    /// S58 (E2-M13): `@unsafe fn` — calling it requires an enclosing `@unsafe`
    /// block (E3103).
    pub is_unsafe: bool,
    /// S60 (E2-M16): `pure fn` — this function is free of ambient I/O and
    /// non-determinism. Call sites inside a `pure fn` must also be pure (E3401).
    pub is_pure: bool,
    /// D-TAINT1: `#Sanitizer fn` — its return value is untainted by contract.
    /// The taint pass treats a call to such a function as producing a clean
    /// (untainted) value regardless of the taint of its arguments.
    pub is_sanitizer: bool,
    /// S61: parameter names and default-value presence, parallel to `params`.
    /// Empty for extern/built-in functions (no label checking needed there).
    pub param_info: Vec<(String, bool)>,
    /// S61: default expressions for parameters that have them, parallel to `params`.
    /// `None` when no default; only trailing params may have defaults.
    pub defaults: Vec<Option<crate::AST::Expr>>,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodSig {
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    is_view_return: bool,
    is_static: bool,
    self_conv: Option<AccessConvention>,
    /// D-NARG1 (S61): parameter names and default-value presence, parallel to
    /// `params`. Excludes `self` (index 0 of params is self when self_conv is
    /// Some; param_info starts from the first non-self param).
    pub(crate) param_info: Vec<(String, bool)>,
    /// D-NARG1 (S61): default expressions for parameters, parallel to param_info.
    /// `None` when no default; only trailing params may have defaults.
    pub(crate) defaults: Vec<Option<crate::AST::Expr>>,
}

#[derive(Debug, Clone)]
pub(crate) enum TypeDef {
    Struct {
        #[allow(dead_code)] // stored for future duplicate-name diagnostics
        name_span: Span,
        fields: Vec<(String, Span, Type, bool, bool)>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `struct`.
        /// Values of this type must be consumed exactly once (E0140/E0141) and
        /// may not be aliased (E0142).
        single_use: bool,
        /// D-SOA1 / D-SOA2A=C: `#layout(columnar)` was present. A `[S]` of this
        /// struct is stored struct-of-arrays; sema gates the list-op surface to
        /// the v1-supported subset (E1108) and codegen lowers it columnar.
        columnar: bool,
    },
    Enum {
        #[allow(dead_code)] // stored for future duplicate-name diagnostics
        name_span: Span,
        variants: HashMap<String, (Span, VariantPayload)>,
        variant_order: Vec<String>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `enum`.
        single_use: bool,
    },
    /// D-DIST1 (ratified 2026-06-19): a distinct type — a nominal wrapper over
    /// a base type. No implicit coercion either direction (E0128). Arithmetic
    /// only when `is_numeric` (D-DIST3, E0127).
    Distinct {
        #[allow(dead_code)] // stored for future duplicate-name diagnostics
        name_span: Span,
        base: Type,
        is_numeric: bool,
    },
}

pub(crate) struct TypeRegistry {
    types: HashMap<String, TypeDef>,
}

impl TypeRegistry {
    fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    fn struct_fields(&self, name: &str) -> Option<&[(String, Span, Type, bool, bool)]> {
        match self.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => Some(fields.as_slice()),
            _ => None,
        }
    }

    /// D-SOA1: true when `name` is a `#layout(columnar)` struct (its `[name]`
    /// collections are stored struct-of-arrays).
    fn is_columnar(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Struct { columnar: true, .. }))
    }

    fn enum_variants(&self, name: &str) -> Option<&HashMap<String, (Span, VariantPayload)>> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variants, .. }) => Some(variants),
            _ => None,
        }
    }

    fn enum_variant_order(&self, name: &str) -> Option<&[String]> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variant_order, .. }) => Some(variant_order.as_slice()),
            _ => None,
        }
    }

    fn method(&self, type_name: &str, method: &str) -> Option<&MethodSig> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { methods, .. }) | Some(TypeDef::Enum { methods, .. }) => {
                methods.get(method)
            }
            _ => None,
        }
    }

    fn field_names(&self, type_name: &str) -> Vec<String> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { fields, .. }) => {
                fields.iter().map(|(n, ..)| n.clone()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// D-DIST1: true when `name` is a registered distinct type.
    pub(crate) fn is_distinct(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Distinct { .. }))
    }

    /// D-LIN1 (ratified 2026-06-21): true when `name` is a `#SingleUse` struct/enum.
    /// Values of such a type must be consumed exactly once and may not be aliased.
    pub(crate) fn is_single_use(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct { single_use: true, .. })
                | Some(TypeDef::Enum { single_use: true, .. })
        )
    }

    /// D-DIST1: the base type of a distinct type (None if `name` is not distinct).
    pub(crate) fn distinct_base(&self, name: &str) -> Option<&Type> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { base, .. }) => Some(base),
            _ => None,
        }
    }

    /// D-DIST3: true when the distinct type has `#Numeric`.
    pub(crate) fn distinct_is_numeric(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Distinct { is_numeric: true, .. }))
    }
}

fn func_to_method_sig(f: &Func) -> MethodSig {
    let self_param = f.self_param();
    // param_info and defaults exclude `self` — they parallel the args a
    // caller provides (no `self` in the call-site arg list).
    let non_self_params = f.params.iter().filter(|p| p.name != "self");
    MethodSig {
        params: f
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        is_view_return: f.is_view_return,
        is_static: self_param.is_none(),
        self_conv: self_param.map(|p| p.convention),
        param_info: non_self_params
            .clone()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        defaults: non_self_params
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
    }
}

fn func_to_sig(f: &Func) -> FuncSig {
    let type_params: HashSet<String> = f.type_params.iter().map(|p| p.name.clone()).collect();
    FuncSig {
        params: f
            .params
            .iter()
            .map(|p| {
                let conv = if matches!(&p.ty, Type::Named(n) if type_params.contains(n)) {
                    AccessConvention::Move
                } else {
                    p.convention
                };
                (conv, p.ty.clone())
            })
            .collect(),
        param_info: f
            .params
            .iter()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        defaults: f
            .params
            .iter()
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        is_view_return: f.is_view_return,
        is_extern: false,
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_sanitizer: f.is_sanitizer,
    }
}

fn extern_to_sig(ef: &ExternFn) -> FuncSig {
    FuncSig {
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        param_info: ef
            .params
            .iter()
            .map(|p| (p.name.clone(), false))
            .collect(),
        defaults: ef.params.iter().map(|_| None).collect(),
        return_type: ef.return_type.clone(),
        is_view_return: ef.is_view_return,
        is_extern: true,
        is_unsafe: false,
        is_pure: false, // extern functions are always considered impure
        is_sanitizer: false, // extern functions can't be sanitizers
    }
}

/// D-NARG-D2: Walk a default expression and substitute any `Ident` that names
/// an earlier parameter with the corresponding supplied argument expression.
/// `param_names` is the slice of earlier param names (index-aligned with `args`).
/// Returns the rewritten expression.
pub(crate) fn substitute_param_refs(
    expr: crate::AST::Expr,
    param_names: &[String],
    args: &[crate::AST::CallArg],
) -> crate::AST::Expr {
    use crate::AST::Expr;
    match expr {
        Expr::Ident(ref name, _) => {
            if let Some(idx) = param_names.iter().position(|n| n == name) {
                if let Some(arg) = args.get(idx) {
                    return arg.expr.clone();
                }
            }
            expr
        }
        Expr::Unary(op, inner, span) => {
            Expr::Unary(op, Box::new(substitute_param_refs(*inner, param_names, args)), span)
        }
        Expr::Binary(op, lhs, rhs, span) => Expr::Binary(
            op,
            Box::new(substitute_param_refs(*lhs, param_names, args)),
            Box::new(substitute_param_refs(*rhs, param_names, args)),
            span,
        ),
        Expr::Field(base, field, span) => {
            Expr::Field(Box::new(substitute_param_refs(*base, param_names, args)), field, span)
        }
        // All other expression forms don't mention parameter names directly
        // (calls, literals, etc.) — leave them as-is. A complex default
        // expression containing a nested call with a param ident would need
        // recursive handling, but the parser only allows simple expressions in
        // defaults (literals, idents, field access, arithmetic).
        other => other,
    }
}

/// D-NARG-D2: Check whether a default expression references any parameter
/// that appears *after* the current parameter index. Collects the names of
/// any forward-referenced parameters found. `all_param_names` is the full
/// list of non-self parameter names for this function (in order).
/// `default_param_idx` is the index of the parameter whose default we're checking.
pub(crate) fn find_forward_refs(
    expr: &crate::AST::Expr,
    all_param_names: &[String],
    default_param_idx: usize,
) -> Vec<(String, crate::Diagnostics::Span)> {
    let mut found = Vec::new();
    find_forward_refs_inner(expr, all_param_names, default_param_idx, &mut found);
    found
}

fn find_forward_refs_inner(
    expr: &crate::AST::Expr,
    all_param_names: &[String],
    default_param_idx: usize,
    found: &mut Vec<(String, crate::Diagnostics::Span)>,
) {
    use crate::AST::Expr;
    match expr {
        Expr::Ident(name, span) => {
            // A forward ref: name is in param list at an index >= default_param_idx
            if let Some(idx) = all_param_names.iter().position(|n| n == name) {
                if idx >= default_param_idx {
                    found.push((name.clone(), *span));
                }
            }
        }
        Expr::Unary(_, inner, _) => {
            find_forward_refs_inner(inner, all_param_names, default_param_idx, found);
        }
        Expr::Binary(_, lhs, rhs, _) => {
            find_forward_refs_inner(lhs, all_param_names, default_param_idx, found);
            find_forward_refs_inner(rhs, all_param_names, default_param_idx, found);
        }
        Expr::Field(base, _, _) => {
            find_forward_refs_inner(base, all_param_names, default_param_idx, found);
        }
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    ty: Type,
    mutable: bool,
    /// Set when the name is a parameter (with its access convention).
    param_conv: Option<AccessConvention>,
    /// Loop nesting depth where the name was declared (for move-in-loop).
    decl_loop_depth: usize,
    /// Whether this local can cross a task/channel boundary. For ordinary
    /// values this follows the type; for lambdas it also includes captures.
    sendable: bool,
    /// Binding span for a Task value that must be consumed with `.join()`.
    task_lint_span: Option<Span>,
    /// D-LIN1 (ratified 2026-06-21): set (to the binding name's span) when this
    /// local owns a `#SingleUse` value that must be consumed exactly once. `None`
    /// for ordinary values, for parameters (the caller owns the consume duty), and
    /// for `view`/`&` borrows (which never own). When still in scope and not in
    /// `moved` at scope end, E0140 fires.
    single_use_span: Option<Span>,
    /// D-DETACH1: set when the task's spawn lambda captured a view borrow (E1102
    /// fired at spawn time). Used by the `detach()` handler to emit E1103.
    #[allow(dead_code)] // D-DETACH1 reader (E1103 path) not yet implemented
    task_has_view_capture: bool,
}

/// D-ALLOC2 (ratified 2026-06-21): bookkeeping for an arena-`view` binding —
/// what `x :: arena.alloc(v)` produced. The view points into `arena`'s storage
/// and is valid only inside the region (the lexical scope of the `arena`
/// binding, or an explicit `region`); the checker forbids it escaping (E0631)
/// or being used after `arena` is `reset`/`free`d (E0632).
#[derive(Debug, Clone)]
pub(crate) struct ArenaViewInfo {
    /// The arena this view points into.
    arena: String,
    /// `scopes.len()` at the view's declaration — the region floor a use must
    /// stay within. Cleared when that scope is popped.
    scope_len: usize,
    /// Set when the backing arena was `reset`/`free`d after this view was made:
    /// `(verb, span_of_reset_or_free)`. Any later *read* of the view is E0632.
    dead: Option<(String, Span)>,
}

#[derive(Debug, Clone)]
pub(crate) enum SendProblemKind {
    RefField,
    ClosureNeedsTake,
    ClosureCaptures,
    TraitValue(String),
    ViewBorrow,
}

#[derive(Debug, Clone)]
pub(crate) struct SendabilityProblem {
    root: Option<String>,
    path: Vec<String>,
    kind: SendProblemKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SendCrossing {
    TaskCapture,
    TaskResult,
    ChannelSend,
}

/// What the driver is compiling — affects `main` / test requirements (M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    /// `jet run` / `jet build` — needs `main`, ignores test blocks in codegen.
    Run,
    /// `jet test` — needs at least one test; `main` is optional.
    Test,
    /// `jet bench` (D-BENCH1) — type-check `#Bench` block bodies and emit the
    /// timing harness; `main` is optional, like `Test`.
    Bench,
    /// `jet check` / LSP — type-check only; imported modules and library files
    /// need not define `main`.
    Check,
    /// `jet eval` — full sema type-checking, but `main` may return a non-`()`
    /// type (E0122 is relaxed). The entry still requires a `main` function.
    Eval,
}

pub(crate) struct ModuleState {
    funcs: HashMap<String, FuncSig>,
    func_pub: HashMap<String, bool>,
    type_pub: HashMap<String, bool>,
    method_pub: HashMap<(String, String), bool>,
    field_pub: HashMap<(String, String), bool>,
    registry: TypeRegistry,
    structs: HashMap<String, Vec<(Option<String>, Type)>>,
    consts: HashMap<String, Type>,
    imports: HashMap<String, usize>,
    core_imports: HashMap<String, String>,
    tests: HashMap<String, Span>,
    trait_reg: TraitRegistry,
    /// D-MOD2: inline code module aliases present in this file (alias → module name).
    /// `math.double(x)` resolves to `user_math__double(x)` when `math` is in here.
    code_modules: HashMap<String, String>,
    /// D-MOD3: unqualified items imported via `use alias.Item` (inline modules).
    /// Maps unqualified name → mangled name (e.g. "clamp" → "math__clamp").
    unqualified: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items imported via `use alias.Item`.
    /// Maps name → (function_name, module_idx).
    unqualified_file: HashMap<String, (String, usize)>,
    /// D-MOD4: `pub use alias.Item` re-exports — items this module exposes on its
    /// own public surface even though they're defined elsewhere. Maps the
    /// exported name → (target_function_name, target_module_idx). A caller doing
    /// `thismod.Item` resolves through here to the real definition.
    reexports: HashMap<String, (String, usize)>,
}

pub(crate) struct Checker<'a> {
    funcs: &'a HashMap<String, FuncSig>,
    registry: &'a TypeRegistry,
    structs: &'a HashMap<String, Vec<(Option<String>, Type)>>,
    consts: &'a HashMap<String, Type>,
    modules: Option<&'a [ModuleState]>,
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    core_imports: &'a HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    code_modules: &'a HashMap<String, String>,
    /// D-MOD3: unqualified inline-module items in scope (name → mangled name).
    unqualified: &'a HashMap<String, String>,
    /// D-MOD3: unqualified file-module items in scope (name → (fn_name, module_idx)).
    unqualified_file: &'a HashMap<String, (String, usize)>,
    /// D-MOD2: pub flags for this module's functions, including inline-module
    /// items mangled as `M__item`. Used to reject `M.private()` from outside.
    func_pub: &'a HashMap<String, bool>,
    diags: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    /// name -> span of the use that gave the value away.
    moved: HashMap<String, Span>,
    loop_depth: usize,
    /// D-LABEL1: stack of `@name` loop labels in scope, innermost last.
    loop_labels: Vec<String>,
    /// D-EFF1: effects this function body reaches directly (Core calls, impure
    /// builtins). Accumulated during the walk; rolled into the per-function
    /// `EffectSummary` after the body is checked.
    fx_direct: EffectSet,
    /// D-EFF1: user functions called in this body (call-graph edges for the
    /// whole-program transitive fixpoint).
    fx_edges: BTreeSet<String>,
    /// D-EFF1: a foreign (`extern`) call was reached — the body's effects are
    /// the maximal set (an un-inspectable body may do anything).
    fx_maximal: bool,
    /// D-EFF1: stack of active `#Caps(…)` regions, innermost last. Every effect
    /// or edge recorded while one is open is also added to it (and all enclosing
    /// regions) so the region's own effect set can be checked against its caps.
    region_stack: Vec<RegionAccum>,
    /// D-EFF1: completed `#Caps(…)` regions in this body, rolled into the
    /// `EffectSummary` for the post-pass E0741 check.
    fx_regions: Vec<RegionSummary>,
    /// D-EFF2: callback-bound obligations recorded at higher-order call sites
    /// where the function-typed parameter carries a `#Pure`/`#(…)` bound. Rolled
    /// into the `EffectSummary` for the post-pass E0747 check.
    fx_callback_obligations: Vec<CallbackObligation>,
    /// D-TXN2: nesting depth of `#Transact(name) { … }` blocks whose body is
    /// being checked **directly** (not inside a deferred lambda). While `> 0`, an
    /// irreversible Core effect (Net/Fs/Exec) reached directly in the block is
    /// E0746 at the call site — the fix is to move it after the block or register
    /// it via `name.on_commit(…)`. Zeroed and restored around every lambda body
    /// (effects inside an `on_commit`/other lambda are deferred, not rejected).
    txn_depth: usize,
    /// D-DET1: nesting depth of `assume_deterministic { … }` blocks currently
    /// being checked. While `> 0`, the determinism rejections inside a `#Pure fn`
    /// (E3403 non-deterministic Core call, E3401 impure Core call) are suspended —
    /// the expert "I know this is deterministic" escape. A semantic footgun
    /// (v1-legal per the card); does not relax memory/type safety, only the
    /// determinism check. Zeroed/restored around lambda bodies like `txn_depth`.
    det_suppress: usize,
    in_unsafe: bool,
    /// True while checking a `pure fn` body, so E3403 can fire on a
    /// non-deterministic std call (time/random) reached from pure code.
    in_pure: bool,
    ret: Option<Type>,
    /// `-> view T` on this function (borrowed return).
    view_return: bool,
    fn_name: String,
    /// Context type for bare `null` (E0308).
    expected_type: Option<Type>,
    /// Collections currently read by an active `for x in xs` loop (E0507).
    iter_borrowed: HashSet<String>,
    /// D-ALLOC-D (ratified 2026-06-19): allocator names that have been freed
    /// or reset in this scope — maps name → verb ("free"/"reset").
    /// E3104 fires if `.alloc()` is called on a freed/reset allocator.
    freed_allocators: HashMap<String, String>,
    /// D-ALLOC2 (ratified 2026-06-21): arena-`view` bindings in scope — a binding
    /// `x :: arena.alloc(v)` holds a scope-bound view into `arena`. Maps the view
    /// name → which arena it points into (for E0631 escape / E0632 use-after-reset).
    arena_views: HashMap<String, ArenaViewInfo>,
    /// D-UNINIT1 (ratified 2026-06-21): `#Uninit` bindings not yet definitely
    /// written — maps name → the `#Uninit` decl span. A read while still in this
    /// map is E0420 (write-before-read proof); a write clears it. Branch-merged
    /// in `check_if` (intersection of "initialized").
    uninit: HashMap<String, Span>,
    /// True while inferring an expression that the generated Rust will only
    /// borrow (method receivers, field/index bases, lvalues). Field reads in
    /// borrow position must NOT be rewritten to `.clone()`.
    borrow_ctx: bool,
    /// M8: when false, a lambda is consumed inline (collection methods / borrow).
    lambda_escapes: bool,
    /// M11: when true, lambda is being passed to tasks.spawn — stricter capture rules (E1101).
    is_task_spawn: bool,
    /// D-DETACH1: task names whose spawn lambda had a non-view sendability error (E1102 fired).
    /// At `.detach()`, if the task is in this set and NOT in view_borrow_escape_tasks, E1103 fires.
    view_capture_tasks: HashSet<String>,
    /// D-DETACH1: task names whose spawn lambda captured a `view` borrow specifically (E1102/ViewBorrow).
    /// At `.detach()`, if the task is in this set, E1106 fires instead of E1103.
    view_borrow_escape_tasks: HashSet<String>,
    /// D-DETACH1: the binding name currently being elaborated (set at check_binding
    /// entry, cleared after). Used to record view-capturing task names.
    current_binding_name: Option<String>,
    /// M8: binding name when checking `val f = (…) => …` (E0804 self-call).
    lambda_binding: Option<String>,
    /// Names mutably captured by an escaping lambda still in scope (E0204).
    lambda_mut_borrow_stack: Vec<HashSet<String>>,
    /// M9: generic/trait metadata for this program.
    trait_reg: &'a TraitRegistry,
    /// M9.5: local comptime evaluation context.
    ct_funcs: &'a HashMap<String, Func>,
    ct_externs: &'a HashSet<String>,
    ct_base_dir: &'a std::path::Path,
    ct_globals: &'a HashMap<String, crate::Comptime::CtValue>,
    ct_scopes: Vec<HashMap<String, crate::Comptime::CtValue>>,
    /// Active generic type parameters while checking a generic item.
    type_param_scope: Vec<crate::AST::TypeParam>,
    /// E2-M15: reject OS-dependent std APIs in `--freestanding` builds (E3301).
    freestanding: bool,
    /// D-WHEN2 (ratified 2026-06-19): when true, we are inside a dropped
    /// `comptime if` arm — name-resolution runs normally (so unknown-name
    /// typos are caught) but all other diagnostics are suppressed and the arm
    /// is never lowered to codegen.
    in_dropped_comptime_arm: bool,
    /// D-L0201: liveness gate — pointer to the statements that follow the
    /// currently-executing statement in the innermost block, plus the count.
    /// Set by `check_block` before each call to `check_stmt`.
    /// Safety: valid for the duration of `check_stmt` — the slice lives in
    /// the `Program` AST which outlives the checker.
    stmt_tail_ptr: *const crate::AST::Stmt,
    stmt_tail_len: usize,
    /// D-L0201: stack of enclosing block tails, from outermost to innermost.
    /// Each entry is the (ptr, len) of the tail saved before entering a nested
    /// block, so `is_name_live_after` can walk up through all enclosing scopes.
    liveness_frames: Vec<(*const crate::AST::Stmt, usize)>,
}


mod FFI;
mod Registration;
mod Bundle;
mod CheckerCore;
mod CheckerInfer;
mod CheckerCoreLib;
mod CheckerOwnership;
mod CheckerItems;
mod Diagnostics;
mod Captures;
mod Capability;
mod Purity;
mod Effects;
mod Taint;
mod SchemaMigration;
pub mod HotSwap;

pub(crate) use FFI::*;
pub(crate) use Registration::*;
pub(crate) use Bundle::*;
pub(crate) use CheckerCoreLib::*;
pub(crate) use Diagnostics::*;
pub(crate) use Captures::*;
pub(crate) use Purity::*;
pub(crate) use Effects::*;
pub(crate) use Taint::{check_func_taint, collect_sanitizers};
// D-LIN1: single-use (must-consume) diagnostics live in CheckerOwnership.
// `e0140_unconsumed` is referenced only within that module; the other two fire
// from CheckerCore (E0141) and CheckerInfer (E0142).
pub(crate) use CheckerOwnership::{e0141_unconsumed_branch, e0142_aliased};

// Public entry points (preserve `jet::Sema::<item>` paths).
pub use Registration::{check, check_with_mode};
pub use Bundle::{check_bundle, check_bundle_freestanding};
pub use FFI::{e3202, e3301, e3302, e3303};
pub use Purity::{check_pure_fn, check_pure_program_root, e3401, e3402, e3403};
// D-MIGRATE2C: `jet schema status` reuses the schema-migration diff.
pub use SchemaMigration::check_schema_migrations;
