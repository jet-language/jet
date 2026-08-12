use crate::AST::{Func, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::TIR::is_covered_enum_ty;
use crate::Codegen::TIR::is_covered_struct_ty;
use crate::Codegen::TIR::is_subset_param_ty;
use crate::Codegen::TIR::is_subset_return_ty;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::stmt_in_subset;
use crate::Codegen::TIR::struct_is_generic;
use crate::Syntax;
use std::collections::HashSet;

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
    // c109 Phase 17/19: GENERIC free functions are covered when every type parameter is a
    // plain `<T>` / bounded `<T: Trait>` form (the clause renders via `render_generics`),
    // the body uses only covered values, and any generic struct application is admitted by
    // the same `is_subset_param_ty`/`is_covered_generic_struct_ty` shape used by lowering.
    if f.type_params.is_empty() {
        // Non-generic: no type-var should appear (defensive — sema wouldn't allow it).
    }
    // A method always has a `self` first parameter; the subset is top-level
    // functions only. (Top-level funcs never have `self`, but check anyway.)
    if f.params.iter().any(|p| p.name == Syntax::KW_SELF) {
        return false;
    }
    // Params must be scalars, String, or a covered value type. c109 Phase 23: a
    // DEFAULT parameter value (`h: Int = w`, S61/D-NARG-D2) is covered — sema fills
    // omitted trailing args at every CALL SITE (`CheckerItems::default-value filling`,
    // substituting earlier-param refs with the supplied arg), so by codegen the call's
    // args are complete and the default expr is GONE from the AST. Codegen never reads
    // `p.default` (the signature emits the same `name: ty` regardless), so a defaulted
    // param lowers byte-identically — the only thing the default touches is the call,
    // already handled. No `p.default` exclusion needed.
    for p in &f.params {
        let param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        if !is_subset_param_ty(&param_ty, cx) {
            return false;
        }
    }
    // Return type, if present, must be a scalar, String, or a covered struct type.
    if let Some(rt) = &f.return_type {
        if !is_subset_return_ty(rt, cx) {
            return false;
        }
    }
    // Track parameter names so identifier references can be classified: a name
    // that is neither a local/param binding nor a builtin is a program-level
    // reference (const or fn-value), which the subset excludes.
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109: is a `#Test` block body fully inside the TIR subset? A test body is a bare
/// statement list (no params, unit context), emitted at indent 1 inside the generated
/// `fn jet_test_N() -> Result<(), String>`. No param/return-type gates apply (the wrapper
/// signature is fixed by `emit_*_tests`); only the body statements must be in-subset.
pub(crate) fn tir_covers_test_body(body: &[Stmt], cx: &Cx) -> bool {
    let mut locals: HashSet<String> = HashSet::new();
    body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109: is an error-conversion `impl Old => New { … }` body fully inside the TIR subset?
/// The body has the Old value bound as `self`, returns the New type, and is emitted at
/// indent 1 inside the `pub fn <conv>(user_self: Old) -> New` `emit_error_conv` opens.
/// The signature/braces are fixed by `emit_error_conv`; only the body statements gate.
pub(crate) fn tir_covers_error_conv_body(body: &[Stmt], cx: &Cx) -> bool {
    let mut locals: HashSet<String> = HashSet::new();
    locals.insert(Syntax::KW_SELF.to_string());
    body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109 Phase 7: is this method (an inherent method of `type_name`) fully inside
/// the TIR subset? Covers two method classes:
///   - **instance methods** — a `self` first parameter (`self`/`mut self`/`view
///     self`/… via any convention), where `self.field` reads and covered-subset
///     constructs (Phases 1–6) make up the body. The `self` slot lowers to the
///     correct Rust receiver (`&self`/`&mut self`/`self`).
///   - **static methods** — no `self` parameter; an associated function on the
///     type (`Type.make(x) -> Type`). The body + every static call site
///     (`Type.make(x)` → `__jet_Type::__jet_make(x)`) are covered.
///
/// The owning `type_name` must itself be a covered struct or enum (so the receiver
/// place + field reads emit exactly as `emit_method` does). The rule is the same
/// **exclude on any doubt**: a false negative just keeps the method on the AST
/// path, a false positive risks a silent miscompile (a wrong `self` receiver).
pub(crate) fn tir_covers_method(f: &Func, type_name: &str, cx: &Cx) -> bool {
    // Method-owned type parameters are in scope while the structural gate walks
    // the signature and body. This keeps generic methods on the same TIR path as
    // generic free functions without leaking their names into the enclosing item.
    let previous_type_params = cx.current_type_params.borrow().clone();
    let mut method_type_params = previous_type_params.clone();
    method_type_params.extend(f.type_params.iter().map(|param| param.name.clone()));
    cx.current_type_params.replace(method_type_params);
    let covered = tir_covers_method_inner(f, type_name, cx);
    cx.current_type_params.replace(previous_type_params);
    covered
}

fn tir_covers_method_inner(f: &Func, type_name: &str, cx: &Cx) -> bool {
    // c109 Phase 18: an `#Unsafe fn` method IS covered
    // (it lowers to an `unsafe fn`, the `is_unsafe` flag driving the prefix). c109
    // Phase 23: a `#Pure fn` method IS covered (purity is sema-only; codegen erases it).
    // The owning type must be a covered struct or enum (the receiver place and
    // every `self.field` read then emit exactly as `emit_method` produces them).
    let generic_owner = struct_is_generic(type_name, cx);
    let owner_ty = Type::Named(type_name.to_string());
    if !generic_owner
        && !is_covered_struct_ty(&owner_ty, cx)
        && !is_covered_enum_ty(&owner_ty, cx)
    {
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
        if !is_subset_return_ty(&resolve_self_ty(rt, type_name), cx) {
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
///  - `is_unsafe` (`#Unsafe fn`) is excluded — its body may use gated pointer ops the
///    subset does not lower, and the `unsafe fn` prefix is a separate emit concern.
///  - a trait method ALWAYS has a `self` receiver (a trait method without `self` is a
///    static trait method, rare; exclude it — the emit hook always renders a receiver).
pub(crate) fn tir_covers_trait_method(
    f: &Func,
    type_name: &str,
    cx: &Cx,
    trait_name: &str,
) -> bool {
    let serde_generic_owner = matches!(trait_name, crate::Generics::ENCODE | crate::Generics::DECODE)
        && struct_is_generic(type_name, cx);
    // Signature shape: source-generated serde methods carry the owner's generic
    // bounds on the parsed method so the whole fragment remains source-native.
    // Other generic trait methods remain outside this subset.
    // c109 Phase 18: an `#Unsafe fn` trait method IS
    // covered (`TFuncKind::TraitMethod.is_unsafe` already drives the `unsafe ` prefix
    // in `emit_tir_trait_method`).
    // c109 Phase 23: a `#Pure` trait method is covered (purity is sema-only; erased).
    if !f.type_params.is_empty() && !serde_generic_owner {
        return false;
    }
    // The owning type must be a covered struct, enum, or distinct type.
    let owner_ty = Type::Named(type_name.to_string());
    if !serde_generic_owner
        && !is_covered_struct_ty(&owner_ty, cx)
        && !is_covered_enum_ty(&owner_ty, cx)
        && !cx.distinct_types.contains_key(type_name)
    {
        return false;
    }
    // c109 Phase 19: a trait method on a GENERIC struct is the deferred generic-type
    // method surface — exclude (conservative, as in `tir_covers_method`).
    if struct_is_generic(type_name, cx)
        && trait_name != crate::Generics::ENCODE
        && trait_name != crate::Generics::DECODE
    {
        return false;
    }
    // D-SERDE2 (card #131 S1-bridge): a hand `impl T.Decode` `decode` is a STATIC trait
    // method (no `self`) — the codec bridge in `emit_tir_trait_method` renders it as
    // `jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<FieldError>>` with no
    // receiver. Admit it (the general "static trait fn" exclusion below does not apply).
    let is_decode = trait_name == crate::Generics::DECODE;
    if !is_decode {
        // A trait method must have `self` as its FIRST parameter (the receiver `&self`/
        // `&mut self`/`self` per convention). A trait method with no `self` (static trait
        // fn) emits no receiver — exclude it (the emit hook always renders a receiver).
        let Some(first) = f.params.first() else {
            return false;
        };
        if first.name != Syntax::KW_SELF {
            return false;
        }
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
        if !is_subset_return_ty(&resolve_self_ty(rt, type_name), cx) {
            return false;
        }
    }
    let mut locals: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    // D-MUTSELF1: self-mutation is fully lowered (the `mut self` slot derefs), so a
    // trait method that assigns `self` / `self.field` is now covered like any other.
    f.body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}
