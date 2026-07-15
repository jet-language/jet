use crate::AST::{AccessConvention, Expr, Func, Param, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::rust_param_type;
use crate::Codegen::rust_return_type;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::SerdeCodec;
use crate::Codegen::TIR::TFunc;
use crate::Codegen::TIR::TFuncKind;
use crate::Codegen::TIR::TWebParamReconstruction;
use crate::Syntax;

/// D-COV1: 1-based line number of a byte offset in the source, for coverage probes.
pub(crate) fn cov_line(cx: &Cx, offset: usize) -> usize {
    line_at_byte_offset(&cx.src, offset)
}

fn line_at_byte_offset(src: &str, offset: usize) -> usize {
    src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::line_at_byte_offset;

    #[test]
    fn coverage_line_accepts_offsets_inside_multibyte_prefixes() {
        let src = "é🚀—λ\nfn run() {}\n";
        for offset in 0..="é🚀—λ".len() {
            assert_eq!(line_at_byte_offset(src, offset), 1, "offset {offset}");
        }
        assert_eq!(line_at_byte_offset(src, "é🚀—λ\n".len()), 2);
        assert_eq!(line_at_byte_offset(src, usize::MAX), 3);
    }
}

pub(crate) fn lower_func(f: &Func, cx: &Cx) -> TFunc {
    lower_func_with_web_boundary(f, cx, false)
}

/// Lower a web function through the same executable TIR as every other target,
/// but retain the one target-boundary fact a flattened `#WasmExport` needs:
/// an all-integer Codable struct parameter is an owned typed local inside the
/// function and scalar fields only at the external ABI. Sema already proved the
/// export type legal; this pass only materializes resolved names/types.
pub(crate) fn lower_web_func(f: &Func, cx: &Cx) -> TFunc {
    lower_func_with_web_boundary(
        f,
        cx,
        f.web_marker == Some(crate::Syntax::WebPartitionMarker::WasmExport),
    )
}

fn lower_func_with_web_boundary(f: &Func, cx: &Cx, reconstruct_web_params: bool) -> TFunc {
    let mut env = LowerEnv::new(f.name.clone());
    // Mirror emit_func's parameter slot construction: a non-scalar `Read` param
    // (String, Char) is a borrow in Rust and reads as `(*name)`.
    let mut params = Vec::new();
    let mut web_param_reconstructions = Vec::new();
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        let param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        // c109 Phase 17: a param TYPED as a bare type parameter (`item: T`) is forced to
        // the `Move` convention for the slot deref (it is passed by value — `rust_param_type`
        // renders it `T`, no `&`), EXACTLY as `emit_func` forces `conv = Move` for an
        // `is_type_param` param. A param typed `Stack<T>` is NOT a type-var param — it keeps
        // its source convention (`Read` → `&user_Stack<T>`, deref'd place `(*user_s)`).
        if reconstruct_web_params {
            if let Type::Named(type_name) = &param_ty {
                if let Some(fields) = cx.struct_fields.get(type_name) {
                    if !fields.is_empty()
                        && fields
                            .iter()
                            .all(|(_, ty)| matches!(ty, Type::Int | Type::IntN { .. }))
                    {
                        let flat_fields = fields
                            .iter()
                            .map(|(field, ty)| {
                                (
                                    cx.mangle_name(field),
                                    cx.mangle_name(&format!("{}_{}", p.name, field)),
                                    ty.clone(),
                                )
                            })
                            .collect();
                        env.bind(&p.name, rust_name.clone(), Some(param_ty.clone()));
                        params.push((rust_name.clone(), param_ty.clone(), p.convention));
                        web_param_reconstructions.push(TWebParamReconstruction {
                            local_rust: rust_name,
                            rust_type: cx.mangle_name(type_name),
                            fields: flat_fields,
                        });
                        continue;
                    }
                }
            }
        }
        let mut slot_param = p.clone();
        slot_param.ty = param_ty.clone();
        let place = param_place_generic(&rust_name, &slot_param, &f.type_params);
        env.bind(&p.name, place, Some(param_ty.clone()));
        params.push((rust_name, param_ty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        web_param_reconstructions,
        ret: f.return_type.clone(),
        generics: render_generics(&f.type_params),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        body,
        kind: TFuncKind::TopLevel,
    }
}

/// D-PREPOST1: lower a `@Pre`/`@Post` condition expression to a Rust bool
/// expression string, in a fresh env with `f`'s params bound exactly as
/// `lower_func` binds them (same place/deref rules) — `@Post` additionally
/// binds `result` to `result_binding` (Rust name, type). Standalone: does not
/// lower/emit the function body itself, so it can run before or independently
/// of the normal body lowering.
pub(crate) fn render_contract_cond(
    f: &Func,
    cond: &Expr,
    result_binding: Option<(&str, &Type)>,
    cx: &Cx,
) -> String {
    let mut env = LowerEnv::new(f.name.clone());
    for p in &f.params {
        let rust_name = cx.mangle_name(&p.name);
        let param_ty = if p.variadic {
            Type::List(Box::new(p.ty.clone()))
        } else {
            p.ty.clone()
        };
        let mut slot_param = p.clone();
        slot_param.ty = param_ty.clone();
        let place = param_place_generic(&rust_name, &slot_param, &f.type_params);
        env.bind(&p.name, place, Some(param_ty));
    }
    if let Some((rust_name, ty)) = result_binding {
        env.bind("result", rust_name.to_string(), Some(ty.clone()));
    }
    emit_tir_expr(&lower_expr(cond, cx, &mut env), cx)
}

/// c109: lower + emit a `#Test` block body through the TIR, reproducing the legacy
/// `emit_stmts(cx, body, &mut env, out, 1, false)` byte-for-byte. The body is a bare
/// statement list with no params and an empty env, emitted at indent 1 inside the
/// `fn jet_test_N() -> Result<(), String>` the caller already opened. The env's
/// `fn_name` is taken LIVE from `cx.current_fn` — exactly the value the legacy `?`/panic
/// emitters read (`emit_*_tests` never resets `cx.current_fn` before the test loop, so
/// both paths embed the same trailing function name in any `?`/panic frame).
pub(crate) fn emit_tir_test_body(body: &[Stmt], cx: &Cx, out: &mut String) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// D-TEST1: lower + emit a property-test body. Identical to `emit_tir_test_body`
/// except each property parameter is bound into the env first (by its mangled
/// name, by value) so references inside the body resolve to the generated input.
/// The caller emits `fn jet_prop_N(p0: T0, …) -> Result<(), String>` so the
/// param names are real Rust locals; this binds them in the lowering env.
pub(crate) fn emit_tir_property_test_body(
    body: &[Stmt],
    params: &[Param],
    cx: &Cx,
    out: &mut String,
) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    for p in params {
        let rust_name = mangle(&p.name);
        env.bind(&p.name, rust_name, Some(p.ty.clone()));
    }
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// c109: lower + emit an error-conversion `impl Old -> New { … }` body through the TIR,
/// reproducing `emit_error_conv`'s `emit_stmts(cx, body, &mut env, out, 1, false)`
/// byte-for-byte. `emit_error_conv` already emitted the signature + opening brace and set
/// `cx.current_fn` to the conversion fn name; it binds `self` to `user_self` (Move, the
/// Old named type — Slot `{rust_name:"user_self", deref:false}`), so the env's `self`
/// place is the bare `user_self`. The body's `return <e>` lowers the expr as-is (sema
/// already inserted any wrapping); emitted at indent 1, the closing brace is the caller's.
pub(crate) fn emit_tir_error_conv_body(body: &[Stmt], from_ty: &str, cx: &Cx, out: &mut String) {
    let mut env = LowerEnv::new(cx.current_fn.borrow().clone());
    env.bind(
        Syntax::KW_SELF,
        "user_self".to_string(),
        Some(Type::Named(from_ty.to_string())),
    );
    let tbody = lower_stmts(body, cx, &mut env);
    emit_tir_stmts(&tbody, cx, out, 1);
}

/// c109 Phase 17: render the Rust generic clause exactly as `emit_func` does — every type
/// param carries an extra `Clone` bound (`rust_extra_clone_bounds`), so `<T>` → `<T: Clone>`
/// and `<T: Comparable>` → `<T: PartialOrd + Clone>`. Empty for a non-generic function.
pub(crate) fn render_generics(type_params: &[crate::AST::TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }
    let extra = crate::Generics::rust_extra_clone_bounds(type_params);
    crate::Generics::rust_type_param_list(type_params, &extra)
}

/// c109 Phase 17: `param_place` for a (possibly generic) free function.
/// Generic parameters preserve their declared access convention exactly like
/// concrete parameters; `&stream: T` therefore dereferences its Rust `&mut T`.
pub(crate) fn param_place_generic(
    rust_name: &str,
    p: &Param,
    _type_params: &[crate::AST::TypeParam],
) -> String {
    param_place(rust_name, p)
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
    env.self_owner = Some(type_name.to_string());
    let mut params = Vec::new();
    let mut self_conv: Option<AccessConvention> = None;
    let mut is_static = true;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            // The self slot, parity with `emit_method`: place `self`, type None. A
            // `mut self` receiver is `&mut Self`, so its place DEREFS (`(*self)`) —
            // `self.field = v` → `((*self)).field = v`, whole-`self` `self = New{}` →
            // `(*self) = New{}` (D-MUTSELF1). `self`/`take self` carry no deref.
            let place = if matches!(p.convention, AccessConvention::Write) {
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
        web_param_reconstructions: Vec::new(),
        ret: f
            .return_type
            .as_ref()
            .map(|t| resolve_self_ty(t, type_name)),
        // A method's generic params live on the enclosing `impl<T> user_<T>` block (the
        // caller opened it); `emit_method` renders no per-method clause.
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
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
///
/// D-SERDE2 (card #131 S1-bridge): `trait_name` selects a codec bridge when it is
/// `Encode`/`Decode` — a hand `impl T.Encode`/`impl T.Decode` whose user-facing
/// `encode`/`decode` verbs + Jet signatures must lower to the Rust trait's
/// `jet_encode`/`jet_decode`. `Encode` is an ordinary instance method (`&self`),
/// only its NAME is bridged. `Decode` is STATIC: the by-value `tree: Data` param
/// binds as an owned local (a clone the emit prepends), so its place is the bare
/// mangled name — no receiver, no `param_place` deref.
pub(crate) fn lower_trait_method(f: &Func, type_name: &str, cx: &Cx, trait_name: &str) -> TFunc {
    let serde = match trait_name {
        crate::Generics::ENCODE => Some(SerdeCodec::Encode),
        crate::Generics::DECODE => Some(SerdeCodec::Decode),
        _ => None,
    };
    let mut env = LowerEnv::new(f.name.clone());
    env.self_owner = Some(type_name.to_string());
    let mut params = Vec::new();
    let mut self_conv = AccessConvention::Read;
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            self_conv = p.convention;
            // The self slot, EXACTLY `emit_trait_method`'s: type `Some(Named(type_name))`
            // (NOT `None` like `emit_method`). D-MUTSELF1: a `mut self` receiver is
            // `&mut self`, so its place DEREFS (`(*self)`); `self`/`take self` do not.
            let place = if matches!(p.convention, AccessConvention::Write) {
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
        // D-SERDE2: a `Decode.decode(tree: Data)` param is emitted as `&jet_std::DataTree`
        // and re-bound to an owned clone at the function head, so the body sees an owned
        // `Data` local — its place is the bare name, NOT `param_place`'s non-scalar deref.
        let place = if serde == Some(SerdeCodec::Decode) {
            rust_name.clone()
        } else {
            param_place(&rust_name, p)
        };
        let pty = resolve_self_ty(&p.ty, type_name);
        env.bind(&p.name, place, Some(pty.clone()));
        params.push((rust_name, pty, p.convention));
    }
    let body = lower_stmts(&f.body, cx, &mut env);
    TFunc {
        name: f.name.clone(),
        params,
        web_param_reconstructions: Vec::new(),
        ret: f
            .return_type
            .as_ref()
            .map(|t| resolve_self_ty(t, type_name)),
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // The trait-method `unsafe` prefix rides on `TFuncKind::TraitMethod.is_unsafe`
        // (the dedicated trait-method emit reads it there); the top-level flag is unused
        // for this kind, but keep it consistent.
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_reactive: f.is_reactive,
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        body,
        kind: TFuncKind::TraitMethod {
            is_unsafe: f.is_unsafe,
            self_conv,
            serde,
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
        .map(|t| rust_return_type(cx, t))
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
                format!(
                    "{}: {}",
                    mangle(&p.name),
                    rust_param_type(cx, p.convention, &p.ty)
                )
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
        web_param_reconstructions: Vec::new(),
        ret: f.return_type.clone(),
        // The signature is fully pre-rendered (`sig`); `is_view`/`generics` are unused for delegation.
        generics: String::new(),
        is_main: false,
        line: cov_line(cx, f.name_span.start),
        // A delegation method has no body and never carries `#Unsafe fn` (sema rejects it).
        // Same for `@Inline`/`@InlineAlways` — a delegation method is pure forwarding,
        // never parsed with an inline marker.
        is_unsafe: false,
        is_pure: false,
        is_reactive: false,
        is_inline: false,
        is_inline_always: false,
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
pub(crate) fn param_place(rust_name: &str, p: &Param) -> String {
    let deref = match p.convention {
        AccessConvention::Read if p.ty.is_scalar() => {
            false
        }
        AccessConvention::Read => true,
        AccessConvention::Write => true,
        AccessConvention::Move => false,
    };
    if deref {
        format!("(*{})", rust_name)
    } else {
        rust_name.to_string()
    }
}
