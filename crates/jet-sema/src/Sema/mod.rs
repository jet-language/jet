//! Semantic checks. Everything here exists so that codegen can stay "dumb"
//! (invariant I3): by the time a Program reaches codegen, it must be
//! impossible for the generated Rust to fail to compile (invariant I2).
//!
//! M1: type inference, mutability, definite-return analysis.
//! M2: ownership — moves, call-site `&`/`^`,
//! view returns, use-after-move, and borrow rules that keep generated Rust
//! sound without surfacing Rust concepts to users.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Traits::TraitRegistry;
use crate::AST::{AccessConvention, Expr, ExternFn, Func, Stmt, Type, VariantPayload};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

/// Re-export so existing callers (`jet::Sema::FuncSig`) keep working.
pub use crate::AST::FuncSig;

#[derive(Debug, Clone)]
pub(crate) struct MethodSig {
    name_span: Span,
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    is_static: bool,
    self_conv: Option<AccessConvention>,
    /// D-NARG1 (S61): parameter names and default-value presence, parallel to
    /// `params`. Excludes `self` (index 0 of params is self when self_conv is
    /// Some; param_info starts from the first non-self param).
    pub(crate) param_info: Vec<(String, bool)>,
    /// D-NARG1 (S61): default expressions for parameters, parallel to param_info.
    /// `None` when no default; only trailing params may have defaults.
    pub(crate) defaults: Vec<Option<crate::AST::Expr>>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse` method — return cannot be silently ignored (E0419).
    pub(crate) must_use: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum TypeDef {
    Struct {
        fields: Vec<(String, Span, Type, bool)>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `struct`.
        /// Values of this type must be consumed exactly once (E0140/E0141) and
        /// may not be aliased (E0142).
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `@MustUse` was present before `struct`.
        must_use: bool,
        /// D-SOA1 / D-SOA2A=C: `#layout(columnar)` was present. A `[S]` of this
        /// struct is stored struct-of-arrays; sema gates the list-op surface to
        /// the v1-supported subset (E1108) and codegen lowers it columnar.
        columnar: bool,
        /// D-REPRC1: `#Layout(c)` was present — codegen stamps `#[repr(C)]`
        /// on the generated Rust struct, so field order/size/padding match C.
        /// A plain struct (no `#Layout(c)`) has an UNSPECIFIED Rust layout and
        /// must never be accepted at the C FFI boundary (card #436 / E3203) —
        /// only this flag makes `c_named_type_ok` (Sema/FFI.rs) say yes.
        is_c_layout: bool,
    },
    Enum {
        variants: HashMap<String, (Span, VariantPayload)>,
        variant_order: Vec<String>,
        /// D-TAG1: variant groups — group path → (span, ordered leaf paths in
        /// its subtree). A group name matches its whole subtree in patterns.
        groups: HashMap<String, (Span, Vec<String>)>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `enum`.
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `@MustUse` was present before `enum`.
        must_use: bool,
        /// D-REPRC2: present only for `#Layout(c[, tag: Width])`.
        c_layout_tag: Option<crate::AST::CEnumTag>,
    },
    /// D-DIST1 (ratified 2026-06-19): a distinct type — a nominal wrapper over
    /// a base type. No implicit coercion either direction (E0128). Arithmetic
    /// only when `is_numeric` (D-DIST3, E0127).
    Distinct {
        base: Type,
        is_numeric: bool,
        /// D-CAPBUNDLE1: `@Comparable`/`@Printable`/`@CodableAsBase` grants.
        is_comparable: bool,
        is_printable: bool,
        is_codable_as_base: bool,
        /// D-RANGETYPE1: `distinct Int(0..10)` — the inclusive `(lo, hi)`
        /// bounds this type provably holds. `None` for a plain distinct type.
        range: Option<(i64, i64)>,
    },
    /// D-TYPEALIAS1 (ratified 2026-06-28): `alias Name<T> = …` — transparent
    /// generic shortcut; expands in sema, erases at codegen.
    Alias {
        params: Vec<crate::AST::TypeParam>,
        target: Type,
    },
}

pub(crate) struct TypeRegistry {
    types: HashMap<String, TypeDef>,
    /// D-FIELDPOL1: struct name → computed field name → (span, declared
    /// type). A computed field never appears in `TypeDef::Struct::fields`
    /// (it's not a stored field, and is never required/allowed in a struct
    /// literal — E0339); this side table is the only place sema resolves its
    /// type for a *read* (`field_type`).
    computed_fields: HashMap<String, HashMap<String, (Span, Type)>>,
}

impl TypeRegistry {
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    fn struct_fields(&self, name: &str) -> Option<&[(String, Span, Type, bool)]> {
        match self.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => Some(fields.as_slice()),
            _ => None,
        }
    }

    /// D-FIELDPOL1: `name`'s computed fields (field name → span + declared
    /// type), or `None` when `name` isn't a struct / has none.
    pub(crate) fn computed_field_types(
        &self,
        name: &str,
    ) -> Option<&HashMap<String, (Span, Type)>> {
        self.computed_fields.get(name)
    }

    /// D-SOA1: true when `name` is a `#layout(columnar)` struct (its `[name]`
    /// collections are stored struct-of-arrays).
    fn is_columnar(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct { columnar: true, .. })
        )
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

    /// D-TAG1: the enum's variant groups (group path → span + leaf paths).
    fn enum_groups(&self, name: &str) -> Option<&HashMap<String, (Span, Vec<String>)>> {
        match self.types.get(name) {
            Some(TypeDef::Enum { groups, .. }) => Some(groups),
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

    /// D-TYPEALIAS1: true when `name` is a registered transparent type alias.
    pub(crate) fn is_type_alias(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Alias { .. }))
    }

    pub(crate) fn type_alias(&self, name: &str) -> Option<(&[crate::AST::TypeParam], &Type)> {
        match self.types.get(name) {
            Some(TypeDef::Alias { params, target, .. }) => Some((params.as_slice(), target)),
            _ => None,
        }
    }

    /// D-LIN1 (ratified 2026-06-21): true when `name` is a `#SingleUse` struct/enum.
    /// Values of such a type must be consumed exactly once and may not be aliased.
    pub(crate) fn is_single_use(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct {
                single_use: true,
                ..
            }) | Some(TypeDef::Enum {
                single_use: true,
                ..
            })
        )
    }

    /// D-MUSTUSE1 (c18iwxqx): true when `name` is a `@MustUse` struct/enum.
    pub(crate) fn is_must_use(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct { must_use: true, .. })
                | Some(TypeDef::Enum { must_use: true, .. })
        )
    }

    /// D-DIST1: the base type of a distinct type (None if `name` is not distinct).
    pub(crate) fn distinct_base(&self, name: &str) -> Option<&Type> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { base, .. }) => Some(base),
            _ => None,
        }
    }

    /// D-DIST3: true when the distinct type has `@Numeric`.
    pub(crate) fn distinct_is_numeric(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_numeric: true,
                ..
            })
        )
    }

    /// D-RANGETYPE1: the declared `(lo, hi)` inclusive bounds of a range type
    /// (`distinct Int(0..10)`), or `None` for a plain distinct type / a name
    /// that isn't distinct.
    pub(crate) fn distinct_range(&self, name: &str) -> Option<(i64, i64)> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { range, .. }) => *range,
            _ => None,
        }
    }

    /// D-CAPBUNDLE1: true when the distinct type has `@Comparable`.
    pub(crate) fn distinct_is_comparable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_comparable: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `@Printable`.
    pub(crate) fn distinct_is_printable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_printable: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `@CodableAsBase`.
    pub(crate) fn distinct_is_codable_as_base(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_codable_as_base: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: the names of every capability bundle granted to distinct
    /// type `name`, in fixed order — used to compose the E0138 "has" clause.
    pub(crate) fn distinct_granted_bundles(&self, name: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.distinct_is_numeric(name) {
            out.push("@Numeric");
        }
        if self.distinct_is_comparable(name) {
            out.push("@Comparable");
        }
        if self.distinct_is_printable(name) {
            out.push("@Printable");
        }
        if self.distinct_is_codable_as_base(name) {
            out.push("@CodableAsBase");
        }
        out
    }
}

fn func_to_method_sig(f: &Func) -> MethodSig {
    let self_param = f.self_param();
    // param_info and defaults exclude `self` — they parallel the args a
    // caller provides (no `self` in the call-site arg list).
    let non_self_params = f.params.iter().filter(|p| p.name != "self");
    MethodSig {
        name_span: f.name_span,
        params: f
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        is_static: self_param.is_none(),
        self_conv: self_param.map(|p| p.convention),
        param_info: non_self_params
            .clone()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        defaults: non_self_params
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
        must_use: f.is_must_use,
    }
}

fn func_to_sig(f: &Func) -> FuncSig {
    let type_params: HashSet<String> = f.type_params.iter().map(|p| p.name.clone()).collect();
    let param_variadic: Vec<bool> = f.params.iter().map(|p| p.variadic).collect();
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
                let ty = if p.variadic {
                    Type::List(Box::new(p.ty.clone()))
                } else {
                    p.ty.clone()
                };
                (conv, ty)
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
        param_variadic,
        variadic_bounds: f.params.last().and_then(|p| p.variadic_bound_list.clone()),
        return_type: f.return_type.clone(),
        is_extern: false,
        is_c_abi: false,
        c_abi_name: None,
        foreign_effect_root: None,
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_foreign_thread_safe: foreign_thread_safe_func(f),
        is_sanitizer: f.is_sanitizer,
        is_must_use: f.is_must_use,
    }
}

fn extern_to_sig(ef: &ExternFn, is_c_abi: bool) -> FuncSig {
    FuncSig {
        params: ef
            .params
            .iter()
            .map(|p| {
                let ty = if p.variadic {
                    Type::List(Box::new(p.ty.clone()))
                } else {
                    p.ty.clone()
                };
                (p.convention, ty)
            })
            .collect(),
        param_info: ef.params.iter().map(|p| (p.name.clone(), false)).collect(),
        defaults: ef.params.iter().map(|_| None).collect(),
        param_variadic: ef.params.iter().map(|p| p.variadic).collect(),
        variadic_bounds: ef.params.last().and_then(|p| p.variadic_bound_list.clone()),
        return_type: ef.return_type.clone(),
        is_extern: true,
        is_c_abi,
        c_abi_name: ef.abi.as_ref().map(|(name, _)| name.clone()),
        foreign_effect_root: ef.effect_root.clone(),
        // D-CABI-RESULT1=C: any raw out-pointer declaration is callable only
        // from an audited `#Unsafe` region. The declaration remains the exact
        // C status/out shape; no Result adapter is invented.
        is_unsafe: is_c_abi
            && ef.params.iter().any(|p| {
                matches!(&p.ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_PTR)
            }),
        is_pure: false,      // extern functions are always considered impure
        is_foreign_thread_safe: false,
        is_sanitizer: false, // extern functions can't be sanitizers
        is_must_use: false,
    }
}

fn foreign_thread_safe_expr(e: &Expr) -> bool {
        match e {
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) | Expr::Ident(..) => true,
            Expr::Unary(_, a, _) | Expr::Copy(a, _) => foreign_thread_safe_expr(a),
            Expr::Binary(_, a, b, _) => foreign_thread_safe_expr(a) && foreign_thread_safe_expr(b),
            Expr::CompareChain { operands, .. } => operands.iter().all(foreign_thread_safe_expr),
            _ => false,
        }
}

fn foreign_thread_safe_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Return(v, _) => v.as_ref().is_none_or(foreign_thread_safe_expr),
            Stmt::Expr(e) => foreign_thread_safe_expr(e),
            Stmt::Val(b) => foreign_thread_safe_expr(&b.init),
            _ => false,
        }
}

fn foreign_thread_safe_func(f: &Func) -> bool {
    f.is_pure && f.type_params.is_empty() && f.pre.is_empty() && f.post.is_empty()
        && f.body.iter().all(foreign_thread_safe_stmt)
}

pub(crate) fn foreign_thread_safe_lambda(lam: &crate::AST::Lambda) -> bool {
    lam.take_names.is_empty()
        && lam.meta.mut_captures.is_empty()
        && lam.meta.cloned_captures.is_empty()
        && match &lam.body {
            crate::AST::LambdaBody::Expr(e) => foreign_thread_safe_expr(e),
            crate::AST::LambdaBody::Block(stmts) => stmts.iter().all(foreign_thread_safe_stmt),
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
        Expr::Unary(op, inner, span) => Expr::Unary(
            op,
            Box::new(substitute_param_refs(*inner, param_names, args)),
            span,
        ),
        Expr::Binary(op, lhs, rhs, span) => Expr::Binary(
            op,
            Box::new(substitute_param_refs(*lhs, param_names, args)),
            Box::new(substitute_param_refs(*rhs, param_names, args)),
            span,
        ),
        Expr::Field(base, field, span) => Expr::Field(
            Box::new(substitute_param_refs(*base, param_names, args)),
            field,
            span,
        ),
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
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
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
    def_span: Span,
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

/// D-DYNARRAY1 (ratified 2026-07-01): bookkeeping for a `View<T>` binding —
/// what `x :: list.view(a..b)` produced. The view points into `list`'s backing
/// storage and is valid only inside the lexical scope that owns `list`; the
/// checker forbids it escaping (returned, stored in another binding, stored in
/// a struct field) via E2305. Crossing a task/channel boundary is covered
/// separately by the general sendability check (`SendProblemKind::ViewBorrow`),
/// not this map.
#[derive(Debug, Clone)]
pub(crate) struct ListViewInfo {
    /// The list this view points into.
    owner: String,
    /// `scopes.len()` at the view's declaration — cleared when that scope pops.
    scope_len: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum SendProblemKind {
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

/// What the driver is compiling — affects `run` / test requirements (M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    /// `jet run` / `jet build` — needs `run`, ignores test blocks in codegen.
    Run,
    /// `jet test` — needs at least one test; `run` is optional.
    Test,
    /// `jet bench` (D-BENCH1) — type-check `#Bench` block bodies and emit the
    /// timing harness; `run` is optional, like `Test`.
    Bench,
    /// `jet check` / LSP — type-check only; imported modules and library files
    /// need not define `run`.
    Check,
    /// `jet eval` — full sema type-checking, but `run` may return a non-`()`
    /// type (E0122 is relaxed). The entry still requires a `run` function.
    Eval,
}

pub(crate) struct ModuleState {
    module_path: String,
    module_alias: String,
    func_spans: HashMap<String, Span>,
    const_spans: HashMap<String, Span>,
    import_spans: HashMap<String, Span>,
    /// D-PUBPKG1=A: modules under the same project package/workspace root may
    /// see `pub(package)` items. Dependency/hangar modules get their own root.
    package_scope: PathBuf,
    funcs: HashMap<String, FuncSig>,
    func_pub: HashMap<String, bool>,
    func_pkg_pub: HashMap<String, bool>,
    type_pub: HashMap<String, bool>,
    type_pkg_pub: HashMap<String, bool>,
    method_pub: HashMap<(String, String), bool>,
    method_pkg_pub: HashMap<(String, String), bool>,
    field_pub: HashMap<(String, String), bool>,
    field_pkg_pub: HashMap<(String, String), bool>,
    registry: TypeRegistry,
    consts: HashMap<String, Type>,
    imports: HashMap<String, usize>,
    core_imports: HashMap<String, String>,
    tests: HashMap<String, Span>,
    trait_reg: TraitRegistry,
    /// D-MOD2: inline code module aliases present in this file (alias → module name).
    /// `math.double(x)` resolves to `user_math__double(x)` when `math` is in here.
    code_modules: HashMap<String, String>,
    /// Inline module spelling -> compiler semantic identity. Ordinary modules
    /// use their module identity; generic instances use `instance:<digest>`.
    code_module_identities: HashMap<String, String>,
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
    consts: &'a HashMap<String, Type>,
    modules: Option<&'a [ModuleState]>,
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    core_imports: &'a HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    code_modules: &'a HashMap<String, String>,
    code_module_identities: &'a HashMap<String, String>,
    /// D-MOD3: unqualified inline-module items in scope (name → mangled name).
    unqualified: &'a HashMap<String, String>,
    /// D-MOD3: unqualified file-module items in scope (name → (fn_name, module_idx)).
    unqualified_file: &'a HashMap<String, (String, usize)>,
    /// D-MOD2: pub flags for this module's functions, including inline-module
    /// items mangled as `M__item`. Used to reject `M.private()` from outside.
    func_pub: &'a HashMap<String, bool>,
    /// D-PUBPKG1=A: package-scoped function visibility flags.
    func_pkg_pub: &'a HashMap<String, bool>,
    module_path: &'a str,
    reference_anchors: &'a mut HashMap<(String, usize, usize), Effects::DefinitionAnchorFact>,
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
    /// where the function-typed parameter carries a `@Pure`/`#(…)` bound. Rolled
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
    /// being checked. While `> 0`, the determinism rejections inside a `@Pure fn`
    /// (E3403 non-deterministic Core call, E3401 impure Core call) are suspended —
    /// the expert "I know this is deterministic" escape. A semantic footgun
    /// (v1-legal per the card); does not relax memory/type safety, only the
    /// determinism check. Zeroed/restored around lambda bodies like `txn_depth`.
    det_suppress: usize,
    /// D-CTX1 / c26: nesting depth of `#Context { … }` blocks (for L0506).
    context_depth: usize,
    /// True while inside a `#Context` block that set an `allocator` field.
    context_allocator_active: bool,
    in_unsafe: bool,
    /// D-IGNORERET2=A: true while inside a `#Suppress(MustUse) { … }` block.
    /// Suppresses E0402 / E0419 for fallible / `@MustUse` results dropped as statements.
    suppress_must_use: bool,
    /// True while checking a `pure fn` body, so E3403 can fire on a
    /// non-deterministic std call (time/random) reached from pure code.
    in_pure: bool,
    /// D-MEM1/S7 (D-NOALLOC-SEM1=A): true when the enclosing module declared
    /// `policy no_alloc`. Local-only: set once per function-body check from
    /// that module's own `no_alloc_policy`, never toggled by a call into
    /// another function — a callee's own allocations are its own module's
    /// concern (E0921 only fires on shapes written directly in THIS body).
    no_alloc: bool,
    /// D-PRELUDEX1=A: true when the enclosing file declared `#NoPrelude`.
    /// Disables ambient `print`/`input` resolution for this body.
    no_prelude: bool,
    /// D-PREPOST1: true while type-checking a `@Pre` clause's condition —
    /// `result` isn't bound yet at function entry, so a reference to it here
    /// is E0144 instead of the normal "undefined name" error.
    in_pre_clause: bool,
    /// True while inferring a comptime binding's RHS or inside a comptime
    /// context — suppresses E2712 for `$name` comptime splice expressions.
    in_comptime: bool,
    ret: Option<Type>,
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
    /// D-DYNARRAY1 (ratified 2026-07-01): `View<T>` bindings in scope — a binding
    /// `x :: list.view(a..b)` holds a scope-bound window into `list`'s backing
    /// storage. Maps the view name → which list it points into (for E2305
    /// escape). Mirrors `arena_views`'s shape; kept separate so the arena
    /// mechanism (E0631/E0632, its own wording and drop-tracking) stays untouched.
    list_views: HashMap<String, ListViewInfo>,
    /// D-MEM1 stage S5 (2026-07-04): string-`view` bindings in scope — a binding
    /// `x :: s.trim()` / `x :: s.after(sep)` / `x :: s.before(sep)` holds a
    /// scope-bound `&str` window into `s`'s backing storage (codegen: `b.string_view`).
    /// Maps the view name → which `String` it points into (for E2307 escape).
    /// Mirrors `list_views`'s shape exactly; kept separate so `ListViewInfo`'s
    /// wording stays owner-type-agnostic in the shared struct but the two kinds
    /// of view (list window vs string window) never alias in one map (I8: one
    /// owner-tracking shape per kind, not one shared namespace).
    string_views: HashMap<String, ListViewInfo>,
    /// D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL1: `:= uninit`
    /// bindings not yet definitely written — maps name → the decl span. A read
    /// while still in this map is E0420 (write-before-read proof); a write
    /// clears it. Branch-merged in `check_if` (intersection of "initialized").
    uninit: HashMap<String, Span>,
    /// True while inferring an expression that the generated Rust will only
    /// borrow (method receivers, field/index bases, lvalues). Field reads in
    /// borrow position must NOT be rewritten to `.clone()`.
    borrow_ctx: bool,
    /// D-MEM1 stage S5: true only while inferring a string-view name (see
    /// `string_views`) in one of the TWO positions its bare `&str` Rust place
    /// actually supports — the receiver of a chained `.trim()`/`.after()`/
    /// `.before()`, or the operand of `copy`. The general `Expr::Ident` arm
    /// reports E2307 whenever it reads a string-view name and this is false —
    /// a single, general choke point instead of hunting down every possible
    /// consuming context (list/tuple literal element, call argument, plain
    /// assignment, …) one at a time.
    allow_string_view_read: bool,
    /// M8: when false, a lambda is consumed inline (collection methods / borrow).
    lambda_escapes: bool,
    /// M11: when true, lambda is being passed to tasks.spawn — stricter capture rules (E1101).
    is_task_spawn: bool,
    /// D-MEM1 S6 (D-SHARED-API1=A): true only while binding `Shared<T>.edit(f)`'s
    /// closure parameter — grants it write access with no `&` sigil (the API
    /// contract IS the exclusive lock; `check_lambda` reads this once, at bind
    /// time, then it's irrelevant for the rest of the closure body).
    lambda_param_mutable: bool,
    /// D-DETACH1: task names whose spawn lambda had a non-view sendability error (E1102 fired).
    /// At `.detach()`, if the task is in this set and NOT in view_borrow_escape_tasks, E1103 fires.
    view_capture_tasks: HashSet<String>,
    /// D-DETACH1: task names whose spawn lambda captured a `view` borrow specifically (E1102/ViewBorrow).
    /// At `.detach()`, if the task is in this set, E1106 fires instead of E1103.
    view_borrow_escape_tasks: HashSet<String>,
    /// D-DETACH1: the binding name currently being elaborated (set at check_binding
    /// entry, cleared after). Used to record view-capturing task names.
    current_binding_name: Option<String>,
    /// M8: binding name when checking `f :: (…) => …` (E0804 self-call).
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
    /// D-CTEFFECT1: `--allow-impure` was passed — `#Impure` blocks may execute
    /// Tier-2 ambient comptime effects (Fs/Env/Exec/Io) at compile time.
    allow_impure: bool,
    /// D-CTEFFECT1: nesting depth of `#Impure` blocks currently being checked.
    /// Passed as `initial_impure_depth` to comptime evaluation of bindings
    /// inside, so the interpreter starts with the gate already open.
    ct_impure_depth: usize,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated while
    /// checking this function body. Drained into `CompileOutput.comptime_inputs`
    /// by Bundle.rs after the full bundle is checked.
    pub(super) ct_embed_inputs: Vec<crate::AST::ComptimeInput>,
    /// D-WHEN2 (ratified 2026-06-19): when true, we are inside a dropped
    /// `comptime if` arm — name-resolution runs normally (so unknown-name
    /// typos are caught) but all other diagnostics are suppressed and the arm
    /// is never lowered to codegen.
    in_dropped_comptime_arm: bool,
    /// E0209 liveness gate (was D-L0201) — pointer to the statements that follow the
    /// currently-executing statement in the innermost block, plus the count.
    /// Set by `check_block` before each call to `check_stmt`.
    /// Safety: valid for the duration of `check_stmt` — the slice lives in
    /// the `Program` AST which outlives the checker.
    stmt_tail_ptr: *const crate::AST::Stmt,
    stmt_tail_len: usize,
    /// E0209 liveness gate (was D-L0201): stack of enclosing block tails, from outermost to innermost.
    /// Each entry is the (ptr, len) of the tail saved before entering a nested
    /// block, so `is_name_live_after` can walk up through all enclosing scopes.
    liveness_frames: Vec<(*const crate::AST::Stmt, usize)>,
    /// D-TASKSCOPE1=A: stack of active `taskgroup` scopes (innermost last).
    taskgroup_stack: Vec<TaskGroupCtx>,
    /// True while inferring the body passed to `g.task { … }` — suppresses L1101
    /// (the taskgroup owns the handle until scope exit or an explicit join).
    in_taskgroup_spawn: bool,
    /// D-METHODMACRO1=A: top-level function names whose bare identifier was
    /// read as a VALUE (not called directly) while checking this one function
    /// body — see `CheckerInfer/expr.rs`'s `Expr::Ident` arm, the single spot
    /// that resolves a bare name to a global function's signature. Rolled up
    /// into a whole-program accumulator (`check_func_body`'s
    /// `global_addr_taken` parameter) so `@InlineAlways` (E0918) can be
    /// checked once every function has run through here.
    inline_addr_taken: HashSet<String>,
}

pub mod ApiFreeze;
mod Bundle;
mod Captures;
mod CheckerCli;
mod CheckerCore;
mod CheckerCoreLib;
mod CheckerFieldPolicy;
mod CheckerInfer;
mod CheckerInline;
mod CheckerItems;
mod CheckerMarkers;
mod CheckerOwnership;
mod CheckerPatchable;
mod CheckerSchedule;
mod CheckerTaskGroup;
use CheckerTaskGroup::TaskGroupCtx;
mod CheckerValidate;
mod Diagnostics;
mod Effects;
mod FFI;
pub mod HotSwap;
mod OsTarget;
mod Protocol;
mod Purity;
mod Registration;
pub mod Schema;
mod SchemaMigration;
mod ScopeMembers;
mod PolicyFacts;
mod BudgetSpecs;
pub use BudgetSpecs::{collect_budget_specs, collect_budget_specs_bundle, collect_located_budget_specs_bundle, BudgetApplicability, BudgetAxis, BudgetComparisonFact, BudgetLimitFact, BudgetQuantity, BudgetRawQuantity, BudgetSpec, LocatedBudgetSpec};
mod CheckerReferences;
mod State;
mod Taint;
mod WebPartition;

pub(crate) use Bundle::*;
pub(crate) use Captures::*;
pub(crate) use CheckerCli::*;
pub use CheckerCoreLib::*;
pub(crate) use CheckerFieldPolicy::*;
pub(crate) use CheckerPatchable::*;
pub(crate) use CheckerValidate::*;
pub(crate) use Diagnostics::*;
pub(crate) use Effects::*;
pub(crate) use Purity::*;
pub use Registration::*;
pub(crate) use Taint::{check_func_taint, collect_sanitizers};
pub(crate) use FFI::*;
// D-STATE1: typestate pass — wrong-state operation (E0150).
pub(crate) use State::{check_items_state, StateTable};
// D-LIN1: single-use (must-consume) diagnostics live in CheckerOwnership.
pub(crate) use WebPartition::check_web_partition;
// D-OSTARGET1=A: native OS platform gating (mixed-axis + unmatched-call).
pub(crate) use OsTarget::{check_os_target, desugar_os_switches};

// Public entry points (preserve `jet::Sema::<item>` paths).
pub use Bundle::{
    check_bundle, check_bundle_allow_impure, check_bundle_freestanding,
    check_bundle_with_effect_facts,
};
pub use Effects::{DefinitionAnchorFact, EffectSummary, SemIndexEffectFacts};
pub use PolicyFacts::{
    collect_policy_facts, collect_policy_facts_from_program, PolicyDomain, PolicyFact,
    PolicyFactGraph,
};
// D-EFFBUDGET1: the closed effect vocabulary, exposed so jet-driver can
// validate `pkg.jet` `effects:`/`grants:` manifest keys against it.
// D-EFFTREE1: also export the tree helpers — jet-driver's EffectBudget and
// manifest parsing need root validation and ancestor-subsumption coverage
// too, not just the bare enum.
pub(crate) use CheckerInline::{check_inline_always_fn, e0918_address_taken};
pub(crate) use CheckerMarkers::check_marker_vocabulary;
pub(crate) use CheckerSchedule::check_every_marker;
pub use Effects::{effect_covers, effect_root, parse_effect_name, show_set, Effect, EffectSet};
pub use Purity::{check_pure_fn, check_pure_program_root, e3401, e3402, e3403};
pub use Registration::{check, check_with_mode, effect_key};
pub use FFI::{e3202, e3301, e3302, e3303};
// D-MIGRATE2C: `jet inspect schema status` reuses the schema-migration diff.
pub use SchemaMigration::{check_schema_migrations, desugar_migrations};

/// D-REACTCORE1: free variable reads in a statement block (for reactive-scope capture cloning).
pub fn block_free_var_reads(stmts: &[crate::AST::Stmt]) -> HashSet<String> {
    let mut bound = HashSet::new();
    let mut read = HashSet::new();
    let mut mut_cap = HashSet::new();
    Captures::block_collect_captures(stmts, &mut bound, &mut read, &mut mut_cap);
    read
}
