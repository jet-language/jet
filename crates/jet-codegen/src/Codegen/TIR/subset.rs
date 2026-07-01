//! TIR subset/coverage gate (`tir_covers*` and `is_covered_*`/`*_in_subset` predicates).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::Syntax;
use crate::AST::{
    BinOp, BindPattern, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt, IndexKind, LValue,
    Lambda, LambdaBody, OrFallback, PatSlot, Pattern, Stmt, StrPart, SwitchArm, Type,
    VariantPayload,
};
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

/// c109: is a `#Test` block body fully inside the TIR subset? A test body is a bare
/// statement list (no params, unit context), emitted at indent 1 inside the generated
/// `fn jet_test_N() -> Result<(), String>`. No param/return-type gates apply (the wrapper
/// signature is fixed by `emit_*_tests`); only the body statements must be in-subset.
pub(crate) fn tir_covers_test_body(body: &[Stmt], cx: &Cx) -> bool {
    let mut locals: HashSet<String> = HashSet::new();
    body.iter().all(|s| stmt_in_subset(s, cx, &mut locals))
}

/// c109: is an error-conversion `impl Old -> New { … }` body fully inside the TIR subset?
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
pub(crate) fn resolve_self_ty(ty: &Type, type_name: &str) -> Type {
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
pub(crate) fn is_subset_param_ty(ty: &Type, cx: &Cx) -> bool {
    let ty = cx.expand_type_aliases(ty);
    // D-QUAL4=A: tagged types are transparent — strip the marker and check the inner type.
    if let Type::Tagged { inner, .. } = &ty {
        return is_subset_param_ty(inner, cx);
    }
    // D-TERM1 (ratified 2026-06-22): `Key` is a core enum (prelude, not user-registry).
    // It is always cloneable and has scalar/Char payloads only — fully covered.
    if matches!(&ty, Type::Named(n) if n == crate::Syntax::TYPE_KEY) {
        return true;
    }
    ty.is_scalar()
        || matches!(&ty, Type::Char | Type::String)
        || is_type_var_param_ty(&ty, cx)
        || is_covered_trait_object_ty(&ty, cx)
        || is_covered_distinct_ty(&ty, cx)
        || is_covered_tuple_ty(&ty, cx)
        || is_covered_struct_ty(&ty, cx)
        || is_covered_enum_ty(&ty, cx)
        || is_covered_collection_ty(&ty, cx)
        || is_covered_fallible_ty(&ty, cx)
        || is_covered_fn_ty(&ty, cx)
        || is_covered_foreign_value_ty(&ty, cx)
        || is_covered_generic_struct_ty(&ty, cx)
        || is_covered_concurrency_ty(&ty, cx)
        || is_covered_reactive_ty(&ty, cx)
        || is_covered_shared_ty(&ty, cx)
}

/// c109 Phase 6b: a `Shared<T>` (`Type::Shared`) usable as a param/return/local value
/// type. `cx.rust_type` already renders it to `std::sync::Arc<{T}>` and `rust_param_type`
/// borrows a `Read` non-scalar to `&std::sync::Arc<{T}>` — both shared with the AST path,
/// so the signature is byte-identical. A `Read` param reads as `(*user_h)` (`param_place`'s
/// non-scalar deref). The element `T` must itself be a covered value type. Admitting the
/// type is what lets a fn with a `Shared<T>` param route; passing one to a free call
/// auto-clones the Arc via `lower_one_call_arg`'s `arc_clone` (the gate now admits it).
pub(crate) fn is_covered_shared_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn is_covered_concurrency_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Channel" | "Sender")
        && args.len() == 1
        && concurrency_elem_covered(&args[0], cx)
}

/// D-REACT1=B: a reactive handle type `Signal<T>` / `Derived<T>` (a single-arg
/// `Type::Apply`) usable as a param/return/local *value* type. Structurally the
/// same seam as `is_covered_concurrency_ty`: `cx.rust_type` renders these to
/// `{root}jet_std::Jet{Signal,Derived}<{T}>` (Source/Codegen/Context.rs), so
/// binding/passing/returning one is byte-identical to the AST path. The element `T`
/// must itself be a covered value type. Their methods (`get`/`set`) carry
/// `recv_type == None` and are covered by the reactive-method shape.
pub(crate) fn is_covered_reactive_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Signal" | "Derived" | "Computed")
        && args.len() == 1
        && is_subset_param_ty(&args[0], cx)
}

/// c109 Phase 21: a `Task<T>`/`Channel<T>`/`Sender<T>` element type. Any covered value
/// type, PLUS `Unit` (`Type::Named("Unit")`) — the result type of a `() => { … }` spawn
/// closure that returns nothing (`tasks.spawn(take(s) () => { s.send(…) })` →
/// `Task<Unit>`, the `[Task<Unit>]` worker list in 34_parallel_scan). `Unit` renders via
/// `cx.rust_type` to `()` (Source/Codegen/Context.rs), so `JetTask<()>` is byte-identical
/// to the AST path. (`Unit` is not a covered value type generally — it has no binding/
/// param surface of its own — so it's admitted only here, where it can only appear as the
/// erased result of a unit-returning task.)
pub(crate) fn concurrency_elem_covered(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn is_covered_generic_struct_ty(ty: &Type, cx: &Cx) -> bool {
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
    // c148: pass cx so multi-char type params are recognized.
    args.iter()
        .all(|a| is_type_var_param_ty(a, cx) || is_subset_param_ty(a, cx))
}

/// c109 Phase 23: a DISTINCT type (`UserId #= distinct Int`, D-DIST1) usable as a
/// param/return/local *value* type. A distinct type renders via `cx.rust_type` to its
/// newtype `user_<Name>` (the `Type::Named` fallthrough in Context.rs), and the emitted
/// `#[repr(transparent)]` newtype is `Copy` iff its base is (sema/codegen derive set) —
/// but the param convention (`Read`→deref for a non-scalar Named) is decided exactly as
/// for a struct, so passing/binding/returning one is byte-identical to the AST path with
/// no new emit. Construction is the `is_distinct_ctor` `Call` shape; `.raw()` is the
/// dedicated DistinctRaw method shape; `+`/`==` on a `#Numeric` distinct emit the native
/// operator (`ast_operand_is_integer` returns `None` for a distinct-typed operand, so the
/// overflow trap is never claimed — matching the AST path's plain `+`).
pub(crate) fn is_covered_distinct_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn is_covered_tuple_ty(ty: &Type, cx: &Cx) -> bool {
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
///
/// c148: also checks `cx.current_type_params` so multi-char params (`Kind`, `Elem`)
/// are treated identically to single-char ones.
pub(crate) fn is_type_var_param_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(n)
        if crate::Generics::is_type_var_name(n)
            || cx.current_type_params.borrow().contains(n.as_str()))
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
pub(crate) fn is_covered_foreign_value_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn is_prelude_struct_name(name: &str) -> bool {
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
pub(crate) fn foreign_struct_lit_in_subset(
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
        .all(|a| is_type_var_param_ty(a, cx) || is_subset_param_ty(a, cx))
}

/// c109 Phase 13: a `fn(…) -> …` parameter/return type the subset lowers. The fn-type
/// renders via `cx.rust_type` (`Box<dyn Fn(…) -> … [+ Send + Sync]>`) exactly as the
/// AST `rust_param_type`/`rust_return_type` do — passed/returned by value (no `&`,
/// `param_place`'s deref matches `emit_func`'s slot). The param/return + arg types must
/// themselves be covered value types so the rendered fn-trait is well-formed and the
/// arg lowering can wrap it. A higher-order fn param (a fn taking/returning a fn) is
/// admitted recursively.
pub(crate) fn is_covered_fn_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Fn { params, ret, .. } => {
            params.iter().all(|p| is_subset_param_ty(p, cx))
                && ret
                    .as_ref()
                    .map(|r| is_subset_param_ty(r, cx))
                    .unwrap_or(true)
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
pub(crate) fn is_covered_trait_object_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn is_covered_fallible_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn fallible_payload_covered(ty: &Type, cx: &Cx) -> bool {
    // c109 Phase 30: a type-variable payload (`T` in a generic fn's `T?` return —
    // `largest<T: Comparable>() -> (T?)`). A type var renders via `cx.rust_type` to the
    // bare letter (`Option<T>`), and `value(best)`/`null` lower to `Some(user_best)`/`None`
    // byte-identically (no clone/box decision). A type var only appears where a type param
    // is in scope (sema guarantees), so an `Option<T>` payload is total.
    if is_type_var_param_ty(ty, cx) {
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
/// decision the subset can't make from total facts. A `FixedList` (`[E#N]`, D-FIXARR1) is
/// covered exactly like a `List`: indexing reads the element type off the base, and a fan-out
/// expression already produces a `[T#N]` value (Rust `[E; N]`). Widening to `[T]` (Vec)
/// when passed to a List slot is handled by `TCallArg.widen_to_vec` — so a `[E#N]`
/// param/return/element is covered once its element type is covered.
pub(crate) fn is_covered_collection_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn collection_elem_covered(ty: &Type, cx: &Cx) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        // c109 Phase 17: a type-variable element (`[T]` in a generic fn). A type var only
        // appears where a type param is in scope (sema guarantees), and renders by value
        // via `cx.rust_type` (`Vec<T>`), so a `[T]` list param/return/local is covered.
        // c148: pass cx so multi-char params are recognized.
        || is_type_var_param_ty(ty, cx)
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
pub(crate) fn is_covered_enum_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    enum_is_covered(name, cx)
}

pub(crate) fn enum_is_covered(name: &str, cx: &Cx) -> bool {
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
pub(crate) fn enum_is_covered_inner(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if crate::Generics::is_type_var_name(name)
        || is_json_type_name(name)
        || is_db_value_type_name(name)
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
        payload_tys
            .iter()
            .all(|t| enum_payload_ty_covered(t, cx, seen))
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
pub(crate) fn enum_payload_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
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
pub(crate) fn core_enum_or_prelude(name: &str) -> bool {
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
pub(crate) fn is_covered_struct_ty(ty: &Type, cx: &Cx) -> bool {
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
pub(crate) fn struct_lit_constructible(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A name that is a genuinely-declared user struct (in `cx.struct_fields`) is a
    // concrete type, never a type variable — even a single-uppercase-letter name like
    // `P`. The `is_type_var_name` heuristic only excludes *undeclared* single-letter
    // names (true generic type vars `T`/`U`), so guard it on non-declaration.
    let is_type_var =
        crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
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
pub(crate) fn boxed_field_payload_constructible(
    ty: &Type,
    cx: &Cx,
    seen: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_constructible(inner, cx, seen),
        Type::Named(n) => struct_lit_constructible(n, cx, seen),
        _ => false,
    }
}

/// c109 Phase 19: is `name` a GENERIC user struct (one with declared type params)? A generic
/// struct's fields reference type vars (`first: T`); `struct_is_covered` now admits those
/// (so a generic struct is a covered VALUE type — Phase 19 covers turbofish construction +
/// `Type::Apply` params). But a METHOD on a generic struct (`impl<T> user_<T>`) is a
/// SEPARATE deferred surface (the inventory's "generic-type method"), so the method gates
/// exclude an owning type that is generic. (Free generic functions are covered by Phase 17;
/// generic STRUCT free functions by Phase 19; generic METHODS stay on the AST path.)
///
/// c148: uses `cx.struct_type_params` (populated from `StructDef.type_params`) rather
/// than `ty_mentions_type_var`, so multi-char type params (`Kind`, `Elem`) are recognized.
pub(crate) fn struct_is_generic(name: &str, cx: &Cx) -> bool {
    cx.struct_type_params
        .get(name)
        .map(|params| !params.is_empty())
        .unwrap_or(false)
}

pub(crate) fn struct_is_covered(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A struct that is a trait/enum or a non-user (foreign/core/prelude) type is
    // out. `cx.struct_fields` only holds user structs declared in this module.
    // A declared user struct is a concrete type, never a type var (see
    // `struct_lit_constructible`): a single-uppercase-letter struct name (`P`) is real.
    let is_type_var =
        crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
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
pub(crate) fn boxed_field_payload_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_covered(inner, cx, seen),
        Type::Named(n) => struct_is_covered(n, cx, seen),
        _ => false,
    }
}

/// A struct *field* type the subset can lower: scalar/String/Char, or another
/// covered struct. Compound/optional/enum/fn field types exclude the struct.
pub(crate) fn field_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    // c109 Phase 19: a generic struct's field may be a bare type VARIABLE (`first: T`
    // in `Pair<T>`). It renders to the bare `T` via `cx.rust_type` and a struct-lit
    // field value is the type-var value itself (by value), so a type-var field needs no
    // clone/deref decision — admit it. (A struct with a type-var field is only ever
    // *used* as a `Type::Apply` — `Pair<Int>` — which `is_covered_generic_struct_ty`
    // gates; a bare `Pair` never type-checks in sema.)
    // c148: pass cx for multi-char type param recognition.
    if is_type_var_param_ty(ty, cx) {
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
        Type::Tagged { inner, .. } => field_ty_covered(inner, cx, seen),
        _ => false,
    }
}

/// `locals` is the set of names bound as params/locals so far in this scope.
/// It is threaded so an `Expr::Ident` can be classified: a name that is not a
/// local must not be a const/fn-value (excluded). Bindings extend it in order.
pub(crate) fn stmt_in_subset(s: &Stmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
    match s {
        Stmt::Val(b) => {
            // c109 Phase 19: an `arena_view` binding (`x #= arena.alloc(v)` / `x #=
            // arena.alloc(v)`) IS covered — it lowers to a plain `let <x> = <init>;` (no
            // type, no mut) with a deref'd slot, exactly as `emit_let`'s `arena_view`
            // branch (the init is a covered `arena.alloc(v)` handle call). The escape/
            // use-after-reset rules (E0631/E0632) are enforced entirely in sema.
            match &b.pattern {
                // c109 Phase 23: a TUPLE-destructuring binding `(a, b) #= <init>` (S74,
                // `BindPattern::Tuple`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name from `(tmp).user_<canonical-field>.clone()` (pairing
                // elems to the type's canonical fields BY POSITION). Covered when the init
                // is in-subset (its lowered `.ty` is a `Type::Tuple` — sema guarantees a
                // tuple pattern destructures a tuple value, so the canonical field names
                // are total at lowering). The Struct/List destructure forms stay on the
                // AST path (no live-suite use; can be a later slice).
                Some(BindPattern::Tuple { elems, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109 Phase 26: a LIST-destructuring binding `[a, b, c] #= <init>` (S74,
                // `BindPattern::List`). The AST `emit_stmt` borrows the init into a temp,
                // then binds each name via `jet_unpack_vec(tmp, want, i, file, line)`
                // (a runtime bounds-checked element move). Covered when the init is
                // in-subset; the element type partiality (`expr_jet_ty`'s
                // `Some(List(inner))`-only match) is reproduced at lowering. The
                // fan-out result-list destructure (`41_fan_out` `main`) is exactly this.
                Some(BindPattern::List { elems, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
                    for e in elems {
                        locals.insert(e.name.clone());
                    }
                    ok
                }
                // c109: a STRUCT-destructuring binding `Type { x, y } #= <init>`
                // (S74, `BindPattern::Struct`). The AST `emit_stmt` borrows the init
                // into a temp, then binds each field via `(tmp).user_<field>.clone()`
                // (the pattern's field name is both the bound local and the read).
                // Covered when the init is in-subset; the per-field type comes from
                // `cx.struct_fields` at lowering (total — sema proved the pattern
                // destructures a struct value).
                Some(BindPattern::Struct { fields, .. }) => {
                    let ok = !b.is_comptime && !b.uninit && expr_in_subset(&b.init, cx, locals);
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
                    // D-UNINIT1 (ratified 2026-06-21, opt C): an `#Uninit` binding needs no
                    // init expression to be in-subset — lower.rs emits
                    // `MaybeUninit::uninit().assume_init()` verbatim (the placeholder
                    // `Expr::Int(0, …)` init is never evaluated or lowered).
                    //
                    // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr`. Sema
                    // evaluates the value into `b.ct` and the AST `emit_let` emits it as
                    // literal data (`let <name>[: <ty>] = <ct.serialize()>;`) — the runtime
                    // `init` expr is NEVER emitted, so it need not be in-subset. Covered
                    // whenever the resolved value is present (`b.ct.is_some()`).
                    // `b.uninit` cannot co-occur with comptime or pattern.
                    let ok = if b.uninit {
                        true
                    } else if b.is_comptime {
                        b.ct.is_some()
                    } else {
                        expr_in_subset(&b.init, cx, locals)
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
            LValue::Index {
                base, index, kind, ..
            } => {
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
        // D-IGNORERET2=A: `.drop("reason")` lowers to an ExprStmt of the receiver;
        // the method call itself is erased. Covered iff the receiver is in-subset.
        Stmt::Expr(Expr::MethodCall {
            receiver, method, ..
        }) if method == Syntax::METHOD_DROP => expr_in_subset(receiver, cx, locals),
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
        // D-LOOP-SEMICOLON1=A: init var is in scope for cond, step, and body.
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            if !expr_in_subset(&init.init, cx, locals) {
                return false;
            }
            let mut inner = locals.clone();
            inner.insert(init.name.clone());
            if !expr_in_subset(cond, cx, &inner) {
                return false;
            }
            if !stmt_in_subset(step.as_ref(), cx, &mut inner) {
                return false;
            }
            body.iter().all(|s| stmt_in_subset(s, cx, &mut inner))
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            ..
        } => match kind {
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
        // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` erases entirely.
        // Always "in subset" since it emits nothing in Rust (I3).
        Stmt::ComptimeBlock { .. } => true,
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
        // D-CTEFFECT1: `#Impure` erases to a plain block at codegen (I3).
        Stmt::Impure { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        Stmt::Reactive { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // D-IGNORERET2=A: `#Suppress(MustUse)` erases to a plain block at codegen (I3).
        Stmt::SuppressMustUse { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1) lowers to a plain Rust
        // block; the body's `let`s LEAK into the outer scope (the AST shares `&mut env`),
        // so the gate checks the body on the SAME `locals`.
        Stmt::Region { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        Stmt::TaskGroup { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
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
        // D-SCAP1: a `#grant(Fs) { caps -> … }` scoped-capability grant erases to a
        // plain Rust block (the grant/revoke is a compile-time capability fact, I3).
        // The capability handle is sema-only — it is NOT emitted, so the body lowers
        // exactly like `Stmt::Region`. Check the body on the SAME `locals`.
        Stmt::Grant { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // D-TERM1 (ratified 2026-06-22): `live { … }` lowers to a guarded Rust block.
        // The body leaks into the outer scope like a region; check it on the SAME `locals`.
        Stmt::Live { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // D-DET1: `assume_deterministic { … }` erases to a plain Rust block (the
        // determinism suspension is a compile-time fact, I3). Body leaks like a
        // region; check it on the SAME `locals`.
        Stmt::AssumeDet { body, .. } => body.iter().all(|s| stmt_in_subset(s, cx, locals)),
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` lowers to a
        // transaction-guarded Rust block. The handle `name` is a covered local
        // inside the body (so `name.on_commit(…)` resolves); check the body with it
        // in scope.
        Stmt::Transact { name, body, .. } => {
            let mut inner = locals.clone();
            if let Some(name) = name {
                inner.insert(name.clone());
            }
            body.iter().all(|s| stmt_in_subset(s, cx, &mut inner))
        }
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
pub(crate) fn forin_method_collection_in_subset(
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
pub(crate) fn if_cond_in_subset(
    cond: &Expr,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<Vec<String>> {
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
        if let Pattern::Variant {
            variant,
            bindings,
            span: _,
        } = pattern
        {
            if is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Bind(_))
            {
                if let PatSlot::Bind(b) = &bindings[0] {
                    return Some(vec![b.clone()]);
                }
            }
            // D-TERM1 (ratified 2026-06-22): a `Key` variant if-let.
            // `if k == Key.Char(c)` → `if let JetKey::Char(user_c) = (k).clone() { … }`.
            // Unit variants (`if k == Key.Enter`) → `if let JetKey::Enter = (k).clone()`.
            if is_key_variant(variant) {
                if bindings.is_empty()
                    || (bindings.len() == 1 && matches!(bindings[0], PatSlot::Bind(_)))
                {
                    let names: Vec<String> = bindings
                        .iter()
                        .filter_map(|s| {
                            if let PatSlot::Bind(n) = s {
                                Some(n.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    return Some(names);
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
            // c109 (D-PATW): a USER-enum variant if-let with a WILDCARD payload slot
            // (`if w == Some(_)`). The `_` binds nothing, so the then-branch gains no
            // local; `emit_if_let_pattern` already renders the slot as `_`, producing
            // `if let user_E::user_V(_) = <subj>` (byte-for-byte the AST `emit_if`). A
            // single-payload covered-enum variant whose one slot is a wildcard is in
            // subset, introducing NO binding (empty bindings vec). (The recently-covered
            // user-variant if-let bound a NAME; this binds `_`.)
            if !is_json_variant(variant)
                && bindings.len() == 1
                && matches!(bindings[0], PatSlot::Wildcard)
            {
                if let Some(owner) = cx.variant_owner.get(variant) {
                    if enum_is_covered(owner, cx) {
                        return Some(Vec::new());
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

pub(crate) fn if_in_subset(ifs: &IfStmt, cx: &Cx, locals: &mut HashSet<String>) -> bool {
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
    if !ifs
        .then_body
        .iter()
        .all(|s| stmt_in_subset(s, cx, &mut then_locals))
    {
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
pub(crate) fn switch_in_subset(
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
        && arms
            .iter()
            .all(|a| arm_head_range(cx, &a.cond, subject).is_some())
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
    // pattern, which would route through `emit_pattern_matches`).
    //
    // D-IF3: a range head (`400..499 ->`) is admitted into this chain too, lowered to
    // `subject >= lo && subject <= hi`, so a value+range mix (`200 -> …` next to
    // `400..499 -> …`) is covered — provided the subject is a scalar ident local so the
    // emitted range condition type-checks (the same constraint shape B imposes).
    // Conservative: a variant/fallible pattern-test arm in the chain excludes the whole
    // switch (stays on the AST path). The `else` is optional.
    let has_range = arms
        .iter()
        .any(|a| arm_head_range(cx, &a.cond, subject).is_some());
    let subject_is_scalar_ident = matches!(subject, Expr::Ident(name, _) if locals.contains(name));
    if arms.iter().all(|a| {
        arm_is_plain_cond(cx, &a.cond, subject) || arm_head_range(cx, &a.cond, subject).is_some()
    }) && (!has_range || subject_is_scalar_ident)
    {
        for a in arms {
            // A range head lowers to a comparison string from `subject_str`; only the
            // PLAIN-cond arms carry a sub-expression that must itself be in-subset.
            if arm_head_range(cx, &a.cond, subject).is_none()
                && !expr_in_subset(&a.cond, cx, locals)
            {
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
pub(crate) fn arm_is_plain_cond(cx: &Cx, cond: &Expr, subject: &Expr) -> bool {
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
pub(crate) fn arm_fallible_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(cx, s, subject) => match pattern {
            Pattern::Ok { .. }
            | Pattern::Err { .. }
            | Pattern::Present { .. }
            | Pattern::Absent(_) => Some(pattern.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The single name an `ok(b)`/`err(b)`/`value(b)` pattern binds (`null` binds none).
pub(crate) fn fallible_pattern_binding(pattern: &Pattern) -> Option<String> {
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
pub(crate) fn arm_variant_pattern(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(cx, s, subject) => {
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
pub(crate) fn pattern_is_variant_or_orvariant(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Variant { bindings, .. } => bindings
            .iter()
            // Only plain name-binds, wildcards, and ranges in payload slots are
            // covered (those are the slot kinds the TIR reproduces).
            .all(|s| {
                matches!(
                    s,
                    PatSlot::Bind(_) | PatSlot::Wildcard | PatSlot::Range { .. }
                )
            }),
        Pattern::Or(alts, _) => {
            !alts.is_empty() && alts.iter().all(pattern_is_variant_or_orvariant)
        }
        _ => false,
    }
}

/// The owning enum of a variant (or or-of-variant) pattern, via `cx.variant_owner`.
pub(crate) fn variant_pattern_enum(cx: &Cx, pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { variant, .. } => cx.variant_owner.get(variant).cloned(),
        Pattern::Or(alts, _) => alts.iter().find_map(|a| variant_pattern_enum(cx, a)),
        _ => None,
    }
}

/// An arm-head range pattern (`lo..hi -> …`), as `(lo, hi)`. Mirrors the parser's
/// arm-head range lowering: a `PatternTest` whose pattern is `Pattern::Range`.
pub(crate) fn arm_head_range(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<(i64, i64)> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern: Pattern::Range { lo, hi, .. },
            ..
        } if pattern_subjects_match(cx, s, subject) => Some((*lo, *hi)),
        _ => None,
    }
}

/// Mirror codegen's `pattern_subjects_match` (Statement.rs): an arm subject names
/// the same ident as the switch subject, is the implicit `it`, or (B1) reads
/// identically in source — a NON-IDENT subject (`h.val`, `pick()`, `xs[0]`) compared
/// spanlessly via its source slice, matching the AST so a non-ident pattern switch
/// routes through the SAME `lower_enum_match` / `lower_fallible_match` the AST's
/// `emit_pattern_match_switch` uses.
pub(crate) fn pattern_subjects_match(cx: &Cx, a: &Expr, b: &Expr) -> bool {
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
pub(crate) fn add_pattern_binding_names(pattern: &Pattern, locals: &mut HashSet<String>) {
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

pub(crate) fn expr_in_subset(e: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) => true,
        Expr::ComptimeSplice { value, .. } => value.is_some(),
        Expr::Str(parts, _) => parts.iter().all(|p| match p {
            StrPart::Lit(_) => true,
            StrPart::Interp(e, _) => expr_in_subset(e, cx, locals),
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
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => expr_in_subset(inner, cx, locals),
        Expr::Binary(_, l, r, _) => expr_in_subset(l, cx, locals) && expr_in_subset(r, cx, locals),
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
            // D-LIN1-DROP: `drop(x)` — the discard builtin (exactly one arg, not
            // shadowed by a user `drop` fn or local). Lowers to `TExprKind::Drop`.
            let is_drop = c.name == Syntax::BUILTIN_DROP
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
                    && c.args
                        .iter()
                        .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
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
            let is_distinct_ctor =
                !locals.contains(&c.name) && cx.distinct_types.contains_key(&c.name);
            // D-SIMD2 / D-LINALG1: a built-in math-type constructor `F32x4(a,b,c,d)` /
            // `Vec3(x,y,z)` / `Mat3(…)`. Lowers to the prelude `jet_math_<T>_new(…)`.
            let is_math_ctor = !locals.contains(&c.name)
                && crate::Sema::is_math_type(&c.name)
                && !cx.type_names.contains(&c.name);
            let is_precise_ctor = !locals.contains(&c.name)
                && (c.name == crate::Syntax::TYPE_BIGINT || c.name == crate::Syntax::TYPE_DECIMAL)
                && !cx.type_names.contains(&c.name);
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
            (is_print
                || is_drop
                || is_ambient_input
                || is_require
                || is_require_eq
                || is_panic
                || is_plain_fn
                || is_distinct_ctor
                || is_math_ctor
                || is_precise_ctor
                || is_extern
                || is_unqual_inline
                || is_unqual_file)
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
            if !then_body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut then_locals))
            {
                return false;
            }
            if !expr_in_subset(then_value, cx, &then_locals) {
                return false;
            }
            let mut else_locals = locals.clone();
            else_body
                .iter()
                .all(|s| stmt_in_subset(s, cx, &mut else_locals))
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
                    && cx.variant_owner.get(member).map(String::as_str) == Some(enum_name.as_str())
                {
                    return true;
                }
                // c109 Phase 24: the `JSON.Null` unit construction reaches codegen as a
                // `Field` (the AST `emit_expr` Field arm emits `{root}jet_std::Json::Null`,
                // Expression.rs ~L222). Cover it (the only no-arg JSON variant).
                if !locals.contains(enum_name) && is_json_type_name(enum_name) && member == "Null" {
                    return true;
                }
                // D-DBDRIVER1: `DbValue.Null` — the same no-arg-`Field` shape as
                // `JSON.Null` above, for the tagged SQL parameter/column value.
                if !locals.contains(enum_name)
                    && is_db_value_type_name(enum_name)
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
                // c109: a comptime-CONST receiver (`comptime P = Pair{…}`; then `P.left`).
                // The const inlines to its pre-rendered Rust value string (`cx.consts[P]`
                // = `user_Pair { … }`) at the use site, and reading a field off it is a
                // plain place read — the AST `emit_expr` Field arm routes the const-ident
                // `inner` through `boxed_field_read`, which calls `emit_expr(Ident)` →
                // `cx.consts[P]`, yielding `((user_Pair { … }).user_<field>)`. The TIR
                // reproduces this exactly (`lower_expr`'s Ident arm already inlines the
                // const string; the Field arm wraps it). A comptime const can hold a
                // struct or enum value; either way the field read is byte-identical.
                if !locals.contains(enum_name) && cx.consts.contains_key(enum_name) {
                    return true;
                }
                // A non-local ident receiver that is NOT a covered enum / comptime const
                // (a core/json/numeric path, an imported namespace, a module alias) is
                // excluded — those use Rust heads/spellings the subset does not emit.
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
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => {
            // D-TERM1 (ratified 2026-06-22): `Key` is a core prelude enum, not in
            // the user registry, but is always covered — all payloads are scalar/Char.
            let key_type = crate::Syntax::TYPE_KEY;
            if type_name == key_type {
                if !is_key_variant(variant) {
                    return false;
                }
                return args.iter().all(|a| match a {
                    EnumLitArg::Positional(e) => expr_in_subset(e, cx, locals),
                    EnumLitArg::Named { expr, .. } => expr_in_subset(expr, cx, locals),
                });
            }
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
        // D-VARIADIC1: list/call spread — covered when the spread operand is in-subset.
        Expr::Spread(inner, _) => expr_in_subset(inner, cx, locals),
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
        Expr::Index {
            base, index, kind, ..
        } => {
            !matches!(kind, IndexKind::Unknown)
                && expr_in_subset(base, cx, locals)
                && expr_in_subset(index, cx, locals)
        }
        // c109 Phase 5: an inclusive copy slice `coll[a..b]` (lists only — the AST
        // path's `jet_slice_vec` is list-specific). Base/start/end must be in-subset.
        Expr::Slice {
            base, start, end, ..
        } => {
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
        Expr::MethodCall {
            receiver,
            method,
            args,
            recv_type,
            ..
        } => method_call_in_subset(receiver, method, args, recv_type, cx, locals),
        // D-TAINT1: `#Tainted expr` — the tag is erased; in-subset iff the inner is.
        Expr::Tainted(inner, _) => expr_in_subset(inner, cx, locals),
        // c109 Phase 8: optional constructors `value(x)` / `null`. Covered when the
        // inner value (if any) is in-subset — they lower to `Some(x)` / `None`.
        Expr::Present(inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Absent(_) => true,
        // D-SIMD2: a reduce-op marker `#Op`. Only appears inside `v.reduce(#Op)`; the
        // method lowering consumes it (it never emits on its own), so it is in-subset.
        Expr::ReduceMarker(_, _) => true,
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
        Expr::OrFallback {
            value, fallback, ..
        } => expr_in_subset(value, cx, locals) && orfallback_rhs_in_subset(fallback, cx, locals),
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
        // D-CAP9: postfix `p.*` deref and prefix `*x` raw-of. Both only appear
        // inside `use core.mem` + an `#Unsafe` region (sema-gated by E0208); the
        // deref/cast forms are byte-for-byte the AST path (no convention facts).
        Expr::Deref(inner, _) | Expr::RawOf(inner, _) => expr_in_subset(inner, cx, locals),
        Expr::Paren(inner, _) => expr_in_subset(inner, cx, locals),
        // Everything else (tuples, …) is out.
        _ => false,
    }
}

/// c109 Phase 13: is `name` a bare top-level function used as a VALUE? It must be a
/// non-local, non-const name in `cx.fn_types` whose type is a `Type::Fn`. Such a name
/// emits `emit_named_fn_value`'s `Box::new(move |…| user_<name>(…)) as <fn-type>`
/// (Source/Codegen/Statement.rs). A const (inlined value) or an unqualified module
/// import is NOT a fn-value, so this stays narrow.
pub(crate) fn ident_is_named_fn_value(name: &str, cx: &Cx, locals: &HashSet<String>) -> bool {
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
pub(crate) fn orfallback_rhs_in_subset(
    fallback: &OrFallback,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    match fallback {
        OrFallback::Value(e) => expr_in_subset(e, cx, locals),
        OrFallback::Return(None, _) => true,
        OrFallback::Return(Some(e), _) => expr_in_subset(e, cx, locals),
        OrFallback::Panic { args, .. } => {
            args.len() == 1 && args[0].label.is_none() && expr_in_subset(&args[0].expr, cx, locals)
        }
        OrFallback::Break(_) | OrFallback::Continue(_) => true,
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
pub(crate) fn lambda_in_subset(lam: &Lambda, cx: &Cx, locals: &HashSet<String>) -> bool {
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
pub(crate) fn fan_out_callee_in_subset(callee: &Expr, cx: &Cx, locals: &HashSet<String>) -> bool {
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
pub(crate) fn method_call_in_subset(
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
    // D-OPTGC1: `handle.with` / `handle.with_mut` on a traced `Gc<T>` value.
    if matches!(method, "with" | "with_mut") {
        return args.len() == 1
            && matches!(&args[0].expr, Expr::Lambda(l) if lambda_in_subset(l, cx, locals))
            && expr_in_subset(receiver, cx, locals);
    }
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact` handle.
    // Sema types the handle `Transaction` (`recv_type == Some("Transaction")`).
    // It lowers to a Drop-backed commit guard (a `scope.guard` cousin), so the
    // single arg must be an in-subset literal zero-param lambda.
    if method == Syntax::TXN_ON_COMMIT && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` — the mirror of
    // `on_commit`, same in-subset shape (a literal zero-param lambda on a handle sema
    // typed `Transaction`).
    if method == Syntax::TXN_ON_ROLLBACK && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-TASKSCOPE1=A: `g.task { … }` on a taskgroup handle (scoped spawn).
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SPAWN_METHOD
    {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-NURSERY1=A: `g.all([…])` — join a list of task handles.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_ALL_METHOD
    {
        return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-CONCCOMB1=A: `g.race([…])` / `g.any([…])` — first completed task wins.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && (method == Syntax::TASKGROUP_RACE_METHOD || method == Syntax::TASKGROUP_ANY_METHOD)
    {
        return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-CONCSELECT1=A: fluent scoped select on taskgroups.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SELECT_METHOD
    {
        return args.is_empty();
    }
    if recv_type
        .as_deref()
        .is_some_and(|rt| rt == Syntax::TYPE_SELECT_BUILDER || rt.starts_with("SelectBuilder<"))
    {
        match method {
            Syntax::SELECT_RECV_METHOD | Syntax::SELECT_READ_METHOD => {
                return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
            }
            Syntax::SELECT_AFTER_METHOD => {
                return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
            }
            Syntax::SELECT_WAIT_METHOD => return args.is_empty(),
            _ => {}
        }
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
        if !locals.contains(type_name) && is_json_type_name(type_name) && is_json_variant(method) {
            return args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // D-DBDRIVER1: a `DbValue` construction `DbValue.Int(n)` / `.Float(f)` /
    // `.Text(s)` / `.Bool(b)` — same shape as the JSON construction just above.
    if let Expr::Ident(type_name, _) = receiver {
        if !locals.contains(type_name)
            && is_db_value_type_name(type_name)
            && is_db_value_variant(method)
        {
            return args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
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
        // D-ENC1: nested-namespace core call `<alias>.<leaf>.method(args)` (e.g.
        // `encoding.json.to_string(x)`). Mirrors the Ident-alias arm below for the
        // `Field(Ident(alias), leaf)` receiver shape; covered iff `<ns>.<leaf>` is a real
        // submodule with a covered method.
        if let Expr::Field(base, leaf, _) = receiver {
            if let Expr::Ident(alias, _) = &**base {
                if !locals.contains(alias) {
                    if let Some(ns) = cx.core_imports.get(alias) {
                        let submodule = format!("{}.{}", ns, leaf);
                        if crate::Syntax::is_known_core_module(&submodule) {
                            return core_call_covered(&submodule, method)
                                && args.iter().all(|a| {
                                    a.label.is_none() && expr_in_subset(&a.expr, cx, locals)
                                });
                        }
                    }
                }
            }
        }
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
                        && args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
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
        return args.len() <= 1 && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    // D-OPTGC1: `gc.Gc.new<T>(value)` traced-handle constructor.
    if recv_type.as_deref() == Some(Syntax::GC_TYPE) && method == Syntax::GC_NEW {
        return args.len() == 1 && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
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
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d-coll-ctor) [D-COLLBREADTH1=A]: a collection static constructor —
    // `Set.from([...])` or `Deque.new()`. The receiver is a bare type-name ident
    // (`"Set"` / `"Deque"`), NOT a local. Sema types the call and leaves
    // `recv_type == None`. The method is `"from"` (for Set) or `"new"` (for Deque).
    // Both are `is_intercepted_method_name` names, so they never reach the static
    // user-type shape (line ~2843). This shape claims them BEFORE that check. Every arg
    // must be in-subset (for `Set.from`, the list literal is always in-subset).
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !locals.contains(type_name.as_str()) {
                match (type_name.as_str(), method, args.len()) {
                    ("Set", "from", 1) | ("Deque", "new", 0) => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    // D-PATHFS1: `Path.from(str)` — static constructor for typed paths.
                    // Like `Set.from`, admitted before `static_method_call_in_subset`
                    // blocks `from` (an intercepted name). Path is not a user type.
                    ("Path", "from", 1) if !cx.type_names.contains("Path") => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    _ => {}
                }
            }
        }
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
    // Shape (d5) [D-REACT1=B]: a reactive `Signal`/`Derived` method (`.get()`/`.set(v)`).
    // Sema sets `recv_type == Some("Signal"|"Derived")` (CheckerInfer's reactive arm), so
    // these are keyed on recv_type — NOT the bare name (`get`/0 would alias a list `get`).
    // `Signal.get()`/`Derived.get()` → `(recv).get()`; `Signal.set(v)` → `(recv).set(v)`.
    if matches!(recv_type.as_deref(), Some("Signal") | Some("Derived") | Some("Computed"))
        && is_reactive_method_name(method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d6) [D-HONESTNUM1=A]: a `Measurement<Float>` method.
    // Sema sets `recv_type == Some("Measurement")`.
    if recv_type.as_deref() == Some("Measurement") && is_measurement_method_name(method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d7) [D-PENDING1=B]: a `Loadable<T,E>` method.
    // Sema sets `recv_type == Some("Loadable")`.
    if recv_type.as_deref() == Some("Loadable") && is_loadable_method_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // D-TTLVAL1=A: Expiring<T> / Rotting<T> methods.
    if matches!(recv_type.as_deref(), Some("Expiring" | "Rotting"))
        && matches!((method, args.len()), ("get", 1) | ("is_valid", 1) | ("force", 1))
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d7b) [D-RENDERTGT2=A]: a UI backend method.
    if matches!(recv_type.as_deref(), Some("NullBackend" | "TuiBackend"))
        && is_ui_backend_method_name(recv_type.as_deref(), method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d8) [D-APPROX1=A]: a sketch method (HyperLogLog/TDigest/CMS/ReservoirSampler).
    if is_sketch_type(recv_type.as_deref()) && is_sketch_method_name(recv_type.as_deref(), method) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d10) [D-NETDEP1=A / D-HTTPLIB1=A]: an HTTP type method call.
    if is_http_type(recv_type.as_deref()) && is_http_method_name(recv_type.as_deref(), method) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d9) [D-TIMEDEPTH1=A]: a civil-time method (Date/DateTime).
    if matches!(recv_type.as_deref(), Some("Date" | "DateTime"))
        && is_civil_time_method_name(recv_type.as_deref(), method)
    {
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
    // D-SIMD2 / D-LINALG1: a method on a built-in math value type (and NOT a user
    // type sharing the name). Admitted when the receiver + args are in-subset.
    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                return expr_in_subset(receiver, cx, locals)
                    && args
                        .iter()
                        .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
            }
        }
    }
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
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; handle it specially before the user-enum path.
                if type_name == crate::Syntax::TYPE_KEY {
                    return is_key_variant(method)
                        && args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                }
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        return enum_is_covered(type_name, cx)
                            && args
                                .iter()
                                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
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
        if let Some(type_name) = static_call_type_name(receiver, locals) {
            return static_method_call_in_subset(&type_name, method, args, cx, locals);
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
    args.iter()
        .zip(sig.iter())
        .all(|(a, (_, _pty))| expr_in_subset(&a.expr, cx, locals))
}

/// c109 Phase 27: is `recv.method(args)` a call THROUGH a fn-typed struct FIELD (not a
/// user method)? Returns the field's `Type::Fn` when so. `recv_type` is the total sema
/// fact (`Some(<StructType>)`, set by CheckerInfer's fn-field arm); the field must exist
/// on a COVERED struct with a `Type::Fn` type and the same name as `method`. Mirrors the
/// AST `emit_method_call` fn-field branch's guard (`struct_fields` lookup + `Type::Fn`).
pub(crate) fn fn_field_call_ty<'a>(
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
pub(crate) fn fn_field_call_in_subset(
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
            a.label.is_none() && !a.flags.shared_auto_clone && expr_in_subset(&a.expr, cx, locals)
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
/// D-PROTO1/D-PROTO2: resolve a static-method receiver to a user type name.
/// `Payment.Client.client()` is `MethodCall(Field(Ident(Payment), Client), …)`.
pub(crate) fn static_call_type_name_unchecked(receiver: &Expr) -> Option<String> {
    match receiver {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, leaf, _) => {
            if let Expr::Ident(prefix, _) = base.as_ref() {
                if prefix
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
                {
                    return Some(format!("{prefix}.{leaf}"));
                }
            }
            None
        }
        _ => None,
    }
}

pub(crate) fn static_call_type_name(receiver: &Expr, locals: &HashSet<String>) -> Option<String> {
    let name = static_call_type_name_unchecked(receiver)?;
    match receiver {
        Expr::Ident(n, _) if locals.contains(n) => None,
        Expr::Field(base, _, _) => {
            if let Expr::Ident(prefix, _) = base.as_ref() {
                if locals.contains(prefix) {
                    return None;
                }
            }
            Some(name)
        }
        _ => Some(name),
    }
}

pub(crate) fn static_method_call_in_subset(
    type_name: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if locals.contains(type_name) {
        return false;
    }
    // D-SIMD2 / D-LINALG1: a static method on a built-in math type (`F32x4.splat(x)`,
    // `Vec3.from_array([…])`). Admitted when the (type, method, nargs) names a covered
    // static and every arg is in-subset.
    if crate::Sema::is_math_type(type_name)
        && !cx.type_names.contains(type_name)
        && crate::Sema::math_static_return(type_name, method, args.len()).is_some()
    {
        return args
            .iter()
            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
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
    let Some(sig) = cx
        .method_sigs
        .get(&(type_name.to_string(), method.to_string()))
    else {
        return false;
    };
    if args.len() != sig.len() {
        return false;
    }
    // c109 Phase 13: a Fn-typed static-method param routes through the Box-coercion
    // (`lower_one_call_arg`). c109 Phase 23: a call-site LABEL (`Rect.new(width: 4.0)`,
    // D-NARG1) is allowed — labels never reorder (D-NARG-D4) and codegen ignores them.
    args.iter()
        .zip(sig.iter())
        .all(|(a, (_, _pty))| expr_in_subset(&a.expr, cx, locals))
}

/// Method names a core/stdlib/builtin/special lowering intercepts *before* the
/// user-method dispatch (`emit_method_call` → `emit_builtin_method` and the
/// `.raw()`/`.snapshot()`/`mem.*.new` special cases, Source/Codegen/Expression.rs).
/// A user method sharing one of these names is emitted by that bespoke lowering on
/// the AST path, not by `method_sigs`, so the TIR must NOT claim it — exclude.
/// The list is intentionally a superset (every name those paths mention, guarded
/// or not): an extra exclusion only keeps a function on the AST path (always safe).
pub(crate) fn is_intercepted_method_name(method: &str) -> bool {
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
        // D-ITER1: lazy iterator adapters.
        | "take" | "skip" | "step_by" | "dedup" | "chunks" | "windows"
        | "enumerate" | "zip"
        | "take_while" | "skip_while" | "flat_map" | "scan"
        | "position" | "min_by" | "max_by" | "fold" | "group_by" | "partition"
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
        // D-COLLBREADTH1=A: Set<T> and Deque<T> methods.
        | "add" | "union" | "to_list"
        | "push_front" | "push_back" | "pop_front" | "pop_back" | "peek_front" | "peek_back"
        // `from` is the static constructor for Set — admitted here so the static-call
        // shape below can claim it before `is_intercepted_method_name` blocks it.
        | "from"
    )
}

/// A call argument is in-subset only if its convention is one the emitter
/// reproduces: a `Read` borrow or a by-`Move` value (with an optional implicit
/// clone). `Mutate` args would need `&mut place` handling we don't yet emit.
pub(crate) fn arg_conv_in_subset(_a: &crate::AST::CallArg) -> bool {
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
pub(crate) fn closure_method_in_subset(
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
        "reduce" | "scan" | "fold" | "par_fold" => {
            // (seed, lambda). The seed is any in-subset value; the lambda must be a
            // literal in-subset closure.
            args.len() == 2
                && expr_in_subset(&args[0].expr, cx, locals)
                && matches!(&args[1].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals))
        }
        // (lambda). map/filter/each/find/any/all/sort_by + D-ITER1 + D-AUTOPAR1 closure adapters.
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
pub(crate) fn is_covered_builtin_name(method: &str, nargs: usize) -> bool {
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
        // c97/D-STRPARSE1. `to_int`/0 on a String reaches here only with `recv_type ==
        // None` (the numeric `to_int` sets a numeric `recv_type`, so it never does);
        // `resolve_builtin_op`'s `is_string` guard is the final filter.
        | ("lines", 0) | ("to_int", 0)
        // `to_string` (String/Bool/Char receiver — those carry `recv_type == None`;
        // a numeric `to_string` sets `recv_type` and so is excluded by the guard).
        | ("to_string", 0)
        // D-ITER1: non-closure lazy adapters.
        | ("take", 1) | ("skip", 1) | ("step_by", 1)
        | ("dedup", 0) | ("chunks", 1) | ("windows", 1)
        | ("enumerate", 0) | ("zip", 1)
        // D-COLLBREADTH1=A: Set<T> instance methods.
        | ("add", 1) | ("union", 1) | ("to_list", 0)
        // D-COLLBREADTH1=A: Deque<T> instance methods.
        | ("push_front", 1) | ("push_back", 1)
        | ("pop_front", 0) | ("pop_back", 0)
        | ("peek_front", 0) | ("peek_back", 0)
        // D-FAILCOMP1: failure-aware list adapter.
        | ("try_collect", 0)
    )
    // NOTE: `is_empty` (now Bool-typed in `Collections::*_method_return` after the
    // c109 fix; lowered to `TBuiltinOp::IsEmpty`) is covered above. `join()` (no
    // separator) stays excluded: sema requires `join(sep)` (E0311 on no-arg), so the
    // no-arg form never reaches codegen — its AST arm is dead.
}

/// c109 Phase 21 + D-COROUTINE1=A: is `(method, nargs)` a Task/Channel/Sender
/// concurrency method (`emit_builtin_method`'s `Type::Apply`-receiver arms)?
/// `Task.join()/wait()/detach()/pause()/resume()/cancel()/trace()`,
/// `Channel.receive()/sender()`, `Sender.send(v)`. The arg count disambiguates
/// `Task.join()` (0 args) from the list `join(sep)` (1 arg, shape d) and `Sender.send(v)`
/// (1 arg) — every name+arity here is disjoint from every other covered shape.
pub(crate) fn is_concurrency_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("join", 0)
            | ("wait", 0)
            | ("detach", 0)
            | ("pause", 0)
            | ("resume", 0)
            | ("cancel", 0)
            | ("trace", 0)
            | ("receive", 0)
            | ("sender", 0)
            | ("send", 1)
    )
}

/// D-REACT1=B: is `(method, nargs)` a reactive `Signal`/`Derived` method?
/// `Signal.get()`/`Derived.get()` (0 args), `Signal.set(v)` (1 arg). Always keyed
/// together with `recv_type == Some("Signal"|"Derived")`, never on the name alone.
pub(crate) fn is_reactive_method_name(method: &str, nargs: usize) -> bool {
    matches!((method, nargs), ("get", 0) | ("set", 1))
}

/// D-HONESTNUM1=A: is `(method, nargs)` a `Measurement<Float>` method?
/// `.add/sub/mul/div(m)` (1 arg), `.value()/.uncertainty()` (0 args).
/// Always keyed with `recv_type == Some("Measurement")`.
pub(crate) fn is_measurement_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("add" | "sub" | "mul" | "div", 1) | ("value" | "uncertainty", 0)
    )
}

/// D-PENDING1=B: is `(method, nargs)` a `Loadable<T,E>` method?
/// `.is_loading()/.is_loaded()/.is_failed()/.is_idle()/.loaded()` (0 args),
/// `.or_else(default)` (1 arg).
/// Always keyed with `recv_type == Some("Loadable")`.
pub(crate) fn is_loadable_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        (
            "is_loading" | "is_loaded" | "is_failed" | "is_idle" | "loaded",
            0
        ) | ("or_else", 1)
    )
}

/// D-RENDERTGT2=A (c133 M1/M2): is `(backend, method, nargs)` a UI backend method?
pub(crate) fn is_ui_backend_method_name(
    backend: Option<&str>,
    method: &str,
    nargs: usize,
) -> bool {
    match (backend, method, nargs) {
        (_, "measure", 2) | (_, "layout", 2) | (_, "paint", 1) | (_, "on_event", 1) => true,
        (Some("NullBackend"), "commands", 0) => true,
        (Some("TuiBackend"), "frame_lines" | "render_count", 0) => true,
        _ => false,
    }
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is this an HTTP type?
pub(crate) fn is_http_type(recv_type: Option<&str>) -> bool {
    matches!(
        recv_type,
        Some("HttpClientReq" | "HttpClientResp" | "HttpMux" | "HttpSrvReq" | "HttpSrvResp")
    )
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is `method` valid for this HTTP type?
pub(crate) fn is_http_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("HttpClientReq") => matches!(method, "header" | "body" | "timeout" | "send"),
        Some("HttpClientResp") => matches!(method, "status" | "body" | "header"),
        Some("HttpMux") => matches!(method, "get" | "post" | "put" | "delete" | "patch"),
        Some("HttpSrvReq") => matches!(method, "method" | "path" | "body" | "param" | "header"),
        Some("HttpSrvResp") => matches!(method, "header"),
        _ => false,
    }
}

/// D-TIMEDEPTH1=A: is `method` valid for this civil-time type?
pub(crate) fn is_civil_time_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("Date") => matches!(
            method,
            "year"
                | "month"
                | "day"
                | "add_days"
                | "add_months"
                | "diff_days"
                | "weekday"
                | "day_of_year"
                | "to_string"
        ),
        Some("DateTime") => matches!(
            method,
            "hour" | "minute" | "second" | "to_timestamp" | "date" | "to_string"
        ),
        _ => false,
    }
}

/// D-APPROX1=A: is this a sketch receiver type?
pub(crate) fn is_sketch_type(recv_type: Option<&str>) -> bool {
    matches!(
        recv_type,
        Some("HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler")
    )
}

/// D-APPROX1=A: is `method` a valid method for this sketch type?
pub(crate) fn is_sketch_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("HyperLogLog") => matches!(method, "add" | "count"),
        Some("TDigest") => matches!(method, "add" | "quantile"),
        Some("CountMinSketch") => matches!(method, "add" | "count"),
        Some("ReservoirSampler") => matches!(method, "add" | "sample"),
        _ => false,
    }
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
pub(crate) fn resolve_numeric_op(method: &str, src_name: &str) -> Option<TNumericOp> {
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
pub(crate) fn conv_rust_target_tir(
    method: &str,
) -> Option<(&'static str, &'static str, Option<(bool, u8)>)> {
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
pub(crate) fn parse_int_name_tir(name: &str) -> Option<(bool, u8)> {
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
pub(crate) fn is_covered_numeric_method(method: &str, nargs: usize) -> bool {
    nargs == 0
        && matches!(
            method,
            "is_nan"
                | "is_infinite"
                | "is_finite"
                | "count_ones"
                | "count_zeros"
                | "leading_zeros"
                | "trailing_zeros"
                | "to_i8"
                | "to_i16"
                | "to_i32"
                | "to_i64"
                | "to_int"
                | "to_u8"
                | "to_u16"
                | "to_u32"
                | "to_u64"
                | "to_f32"
                | "to_f64"
                | "to_float"
                | "to_string"
        )
}

/// c109 Phase 28: is `member` a per-type numeric bounds constant (`U8.MAX`,
/// `I32.MIN`, `Float.INFINITY`, …)? Mirrors the AST `emit_expr` Field arm's filter
/// (Source/Codegen/Expression.rs ~L226) exactly.
pub(crate) fn is_numeric_bounds_const(member: &str) -> bool {
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
pub(crate) fn core_call_covered(module: &str, method: &str) -> bool {
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
    // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `JetMeasurement<f64>`. NOT in
    // `core_fixed_sig` — the return type is `Measurement<Float>` (generic Apply).
    if module == "core.science.measurement" && method == "from" {
        return true;
    }
    // D-PENDING1=B: `L.idle/loading/loaded/failed` → `JetLoadable`. NOT in `core_fixed_sig`.
    if module == "core.async.loadable" && matches!(method, "idle" | "loading" | "loaded" | "failed")
    {
        return true;
    }
    // D-APPROX1=A: `HLL.new()`, `TD.new()`, `CMS.new()`, `RS.new(capacity)`. NOT in `core_fixed_sig`.
    if matches!(
        module,
        "core.sketch.hll" | "core.sketch.tdigest" | "core.sketch.cms" | "core.sketch.reservoir"
    ) && method == "new"
    {
        return true;
    }
    // D-TIMEDEPTH1=A: civil-time constructors. NOT in `core_fixed_sig`.
    if matches!(module, "core.time.date" | "core.time.datetime")
        && matches!(method, "new" | "today" | "parse" | "from_timestamp" | "now")
    {
        return true;
    }
    // D-TTLVAL1=A: Expiring<T> / Rotting<T> constructors. NOT in `core_fixed_sig` (generic T).
    if module == "core.time.expiring" && method == "new" {
        return true;
    }
    if module == "core.secrets" && method == "rotting_new" {
        return true;
    }
    // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors. NOT in `core_fixed_sig`.
    if matches!(module, "core.http.client" | "core.http.server")
        && matches!(
            method,
            "get" | "post" | "request" | "mux" | "serve" | "response"
        )
    {
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
pub(crate) fn core_closure_call_in_subset(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let lambda_arg = |i: usize| matches!(args.get(i).map(|a| &a.expr), Some(Expr::Lambda(lam)) if lambda_in_subset(lam, cx, locals));
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
        // D-REACT1=B / D-SIGNAL1: `reactive.derived/computed/effect(<lambda>)` —
        // 1 arg, a literal zero-param in-subset lambda (rendered by `render_lambda_str`).
        ("jet.reactive", "derived" | "computed" | "effect") => {
            args.len() == 1 && no_labels && lambda_arg(0)
        }
        // D-RENDERTGT2=A (c133 M2): `ui.reactive_render(<lambda>)`.
        ("core.ui", "reactive_render") => args.len() == 1 && no_labels && lambda_arg(0),
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
pub(crate) fn alloc_new_type<'a>(
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
pub(crate) fn router_register_in_subset(
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

pub(crate) fn handle_method_op(handle: &str, method: &str, nargs: usize) -> Option<THandleOp> {
    Some(match (handle, method, nargs) {
        ("FileReader", "read_line", 0) => THandleOp::FileReaderReadLine,
        ("FileWriter", "write_line", 1) => THandleOp::FileWriterWriteLine,
        ("FileWriter", "flush", 0) => THandleOp::FileWriterFlush,
        ("StdinHandle", "read_line", 0) => THandleOp::StdinReadLine,
        ("Stopwatch", "elapsed_millis", 0) => THandleOp::StopwatchElapsedMillis,
        // D-DET1: deterministic injected Clock/Rng capability methods.
        ("Clock", "now", 0) => THandleOp::ClockNow,
        ("Clock", "tick", 1) => THandleOp::ClockTick,
        // D-DET-CAPAPI: absolute set + Duration advance; the widened Rng draws; Duration read.
        ("Clock", "advance", 1) => THandleOp::ClockAdvance,
        ("Clock", "wait", 1) => THandleOp::ClockWait,
        ("Rng", "int", 2) => THandleOp::RngInt,
        ("Rng", "float", 0) => THandleOp::RngFloat,
        ("Rng", "bool", 0) => THandleOp::RngBool,
        ("Rng", "pick", 1) => THandleOp::RngPick,
        ("Rng", "shuffle", 1) => THandleOp::RngShuffle,
        ("Duration", "millis", 0) => THandleOp::DurationMillis,
        ("BigInt", "add" | "sub" | "mul", 1) => THandleOp::PreciseMethod {
            type_name: "BigInt".to_string(),
            method: method.to_string(),
        },
        ("BigInt", "neg" | "to_string", 0) => THandleOp::PreciseMethod {
            type_name: "BigInt".to_string(),
            method: method.to_string(),
        },
        ("Decimal", "add" | "sub" | "mul", 1) => THandleOp::PreciseMethod {
            type_name: "Decimal".to_string(),
            method: method.to_string(),
        },
        ("Decimal", "to_string", 0) => THandleOp::PreciseMethod {
            type_name: "Decimal".to_string(),
            method: method.to_string(),
        },
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
        // D-ARGS1: ArgsSpec builder methods.
        ("ArgsSpec", "flag", 2) => THandleOp::ArgsSpecFlag,
        ("ArgsSpec", "option", 3) => THandleOp::ArgsSpecOption,
        ("ArgsSpec", "positional", 2) => THandleOp::ArgsSpecPositional,
        ("ArgsSpec", "help", 0) => THandleOp::ArgsSpecHelp,
        ("ArgsSpec", "parse", 1) => THandleOp::ArgsSpecParse,
        // D-ARGS1: ParsedArgs query methods.
        ("ParsedArgs", "flag", 1) => THandleOp::ParsedArgsFlag,
        ("ParsedArgs", "option", 1) => THandleOp::ParsedArgsOption,
        ("ParsedArgs", "positional", 1) => THandleOp::ParsedArgsPositional,
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
        // D-SERDE-ACCESS=B: DataTree accessor methods.
        ("DataTree", "field", 1) => THandleOp::DataTreeField,
        ("DataTree", "at", 1) => THandleOp::DataTreeAt,
        ("DataTree", "int", 0) => THandleOp::DataTreeInt,
        ("DataTree", "text", 0) => THandleOp::DataTreeText,
        ("DataTree", "bool", 0) => THandleOp::DataTreeBool,
        ("DataTree", "float", 0) => THandleOp::DataTreeFloat,
        // D-SERDE-ACCESS=B: same accessors on Json/Data (the dynamic parse result).
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "field", 1) => THandleOp::JsonField,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "at", 1) => THandleOp::JsonAt,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "int", 0) => THandleOp::JsonInt,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "text", 0) => THandleOp::JsonText,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "bool", 0) => THandleOp::JsonBool,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "float", 0) => THandleOp::JsonFloat,
        // D-PATHFS1: typed Path instance methods.
        ("Path", "join", 1) => THandleOp::PathJoin,
        ("Path", "parent", 0) => THandleOp::PathParent,
        ("Path", "extension", 0) => THandleOp::PathExtension,
        ("Path", "stem", 0) => THandleOp::PathStem,
        ("Path", "to_string", 0) => THandleOp::PathToString,
        ("Path", "write_atomic", 1) => THandleOp::PathWriteAtomic,
        ("Path", "walk", 0) => THandleOp::PathWalk,
        // D-DBDRIVER1: `DbConnection` instance methods.
        ("DbConnection", "query", 2) => THandleOp::DbQuery,
        ("DbConnection", "query_one", 2) => THandleOp::DbQueryOne,
        ("DbConnection", "execute", 2) => THandleOp::DbExecute,
        ("DbConnection", "begin", 0) => THandleOp::DbBegin,
        ("DbConnection", "commit", 0) => THandleOp::DbCommit,
        ("DbConnection", "rollback", 0) => THandleOp::DbRollback,
        ("DbConnection", "close", 0) => THandleOp::DbClose,
        // D-DBDRIVER1: `DbValue` accessor methods.
        ("DbValue", "int", 0) => THandleOp::DbValueInt,
        ("DbValue", "float", 0) => THandleOp::DbValueFloat,
        ("DbValue", "text", 0) => THandleOp::DbValueText,
        ("DbValue", "bool", 0) => THandleOp::DbValueBool,
        ("DbValue", "is_null", 0) => THandleOp::DbValueIsNull,
        // D-SIMD2 / D-LINALG1 math methods are handled by a dedicated gate + lowering
        // block (user-type-aware via `cx.type_names`), NOT here — `handle_method_op`
        // has no `cx`, and a user struct may share a math name.
        _ => return None,
    })
}

/// c109 Phase 13: the resolved return type of a covered handle method, read from the
/// authoritative sema handle tables (`file_handle_method_return`/`net_method_return`,
/// Source/Sema/CheckerCoreLib.rs) — a pure `(handle, method)` dispatch, no inference.
/// The return type is rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but kept total per the design principle. A throwaway diags vec absorbs the table's
/// diagnostic side-channel (sema already validated, so none fire here).
pub(crate) fn handle_method_return_ty(handle: &str, method: &str, nargs: usize) -> Type {
    let span = crate::Diagnostics::Span { start: 0, end: 0 };
    let mut sink = Vec::new();
    let ret = crate::Sema::file_handle_method_return(handle, method, nargs, span, &mut sink)
        .or_else(|| crate::Sema::net_method_return(handle, method, nargs, span, &mut sink))
        .or_else(|| crate::Sema::path_method_return(handle, method, nargs, span, &mut sink))
        .or_else(|| {
            if handle == "DbConnection" {
                Some(crate::Sema::db_connection_method_return_ty(method))
            } else {
                None
            }
        })
        .or_else(|| {
            if is_db_value_type_name(handle) {
                Some(crate::Sema::db_value_method_return(method, nargs))
            } else {
                None
            }
        })
        .or_else(|| {
            if handle == crate::Syntax::TYPE_BIGINT || handle == crate::Syntax::TYPE_DECIMAL {
                crate::Collections::builtin_method_return(
                    &Type::Named(handle.to_string()),
                    method,
                    nargs,
                    false,
                )
            } else {
                None
            }
        });
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
pub(crate) fn core_closure_call_return_ty(module: &str, method: &str, body_ty: Type) -> Type {
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
pub(crate) fn core_call_return_ty(module: &str, method: &str) -> Type {
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
        // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `Measurement<Float>`.
        ("core.science.measurement", "from") => {
            return Type::Apply {
                name: Syntax::TYPE_MEASUREMENT.to_string(),
                args: vec![Type::Float],
            }
        }
        // D-PENDING1=B: Loadable constructors — type carries T from the loaded(val) arg.
        ("core.async.loadable", "idle") | ("core.async.loadable", "loading") => {
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![
                    Type::Named("Unknown".to_string()),
                    Type::Named("Unknown".to_string()),
                ],
            }
        }
        ("core.async.loadable", "loaded") => {
            // Type is Loadable<T, Unknown> — T comes from the arg; Unknown for E.
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![Type::Int, Type::Named("Unknown".to_string())], // sema refines T
            };
        }
        ("core.async.loadable", "failed") => {
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![Type::Named("Unknown".to_string()), Type::String], // sema refines E
            };
        }
        // D-APPROX1=A: sketch constructors → opaque named types.
        ("core.sketch.hll", "new") => return Type::Named("HyperLogLog".to_string()),
        ("core.sketch.tdigest", "new") => return Type::Named("TDigest".to_string()),
        ("core.sketch.cms", "new") => return Type::Named("CountMinSketch".to_string()),
        ("core.sketch.reservoir", "new") => return Type::Named("ReservoirSampler".to_string()),
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") | ("core.time.date", "today") => {
            return Type::Named("Date".to_string())
        }
        ("core.time.date", "parse") => {
            return Type::Result {
                ok: Box::new(Type::Named("Date".to_string())),
                err: Box::new(Type::String),
            }
        }
        ("core.time.datetime", "from_timestamp") | ("core.time.datetime", "now") => {
            return Type::Named("DateTime".to_string())
        }
        // D-TTLVAL1=A: Expiring<T> / Rotting<T> constructors — T from arg 0.
        ("core.time.expiring", "new") => {
            return Type::Apply {
                name: "Expiring".to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }
        }
        ("core.secrets", "rotting_new") => {
            return Type::Apply {
                name: "Rotting".to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }
        }
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors.
        ("core.http.client", "get") | ("core.http.client", "post") => {
            return Type::Result {
                ok: Box::new(Type::Named("HttpClientResp".to_string())),
                err: Box::new(Type::String),
            }
        }
        ("core.http.client", "request") => return Type::Named("HttpClientReq".to_string()),
        ("core.http.server", "mux") => return Type::Named("HttpMux".to_string()),
        ("core.http.server", "serve") => {
            return Type::Result {
                ok: Box::new(Type::Tuple(vec![])),
                err: Box::new(Type::String),
            }
        }
        ("core.http.server", "response") => return Type::Named("HttpSrvResp".to_string()),
        _ => {}
    }
    crate::Sema::core_fixed_sig(module, method)
        .and_then(|(_, ret)| ret)
        .unwrap_or_else(unit_type)
}

// ---------------------------------------------------------------------------
// Lowering: AST -> TIR. This is where every fact is resolved ONCE.
// ---------------------------------------------------------------------------
