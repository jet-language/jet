use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Bundle::fn_types_compatible;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, CModule, ExternFn, ExternRustBlock, FuncSig, Item, Param, Type,
    VariantPayload,
};
use jet_foundation::Prelude as CorePrelude;
use std::collections::HashMap;

pub(crate) fn cpp_callback_abi_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Tagged { marker, inner }
            if matches!(
                marker,
                crate::AST::TagMarker::Internal(crate::AST::InternalTag::CppCallbackAbi)
            ) && matches!(inner.as_ref(), Type::Fn { .. }) =>
        {
            Some(inner)
        }
        _ => None,
    }
}

pub(crate) fn is_callback_boundary_param(is_c_abi: bool, ty: &Type) -> bool {
    let c_contract = crate::AST::ForeignLanguage::C.abi_contract();
    (is_c_abi
        && matches!(
            c_contract.callbacks,
            crate::AST::ForeignCallbackModel::ReentrantThreadSafe
        )
        && matches!(ty, Type::Fn { .. }))
        || cpp_callback_abi_type(ty).is_some()
}

pub(crate) fn check_extern_block(
    block: &ExternRustBlock,
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    if crate::Syntax::crate_spec_needs_version(&block.crate_spec) {
        diags.push(Diagnostic::error(
            "E0701",
            format!(
                "the crate `{}` needs a version pin",
                block.crate_spec
            ),
            "every non-`std` `extern rust` crate must pin an exact version so builds stay reproducible"
                .to_string(),
            format!(
                "write: extern rust \"{}@0.1\" {{ ... }}",
                block.crate_spec
            ),
            Some(block.crate_span),
        ));
        ok = false;
    }
    for ef in &block.functions {
        if let Some((_, span)) = &ef.abi {
            diags.push(Diagnostic::error(
                "E3212",
                "`#ABI` only applies to C declarations".to_string(),
                "Rust FFI uses its declared Rust ABI and cannot select a C calling convention"
                    .to_string(),
                "remove `#ABI` from the `extern rust` function".to_string(),
                Some(*span),
            ));
            ok = false;
        }
        if !check_extern_fn(ef, registry, diags) {
            ok = false;
        }
        if !check_close_contract(ef, &block.functions, diags) {
            ok = false;
        }
    }
    ok
}

pub(crate) fn check_extern_fn(
    ef: &ExternFn,
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    for p in &ef.params {
        if !is_ffi_type(&p.ty, registry) {
            diags.push(ffi_type_error(
                &format!("`{}` has type `{}`, which can't cross into Rust", p.name, p.ty.name()),
                "the by-value floor admits checked Jet values; `&` is exclusive access for this call and `^` transfers ownership, but the underlying type must still be supported",
                "use a checked value type, `&T` for one-call exclusive access, or `^T` to give ownership; keep the underlying type in the allowed set",
                p.ty_span,
            ));
            ok = false;
        }
    }
    if let Some(rt) = &ef.return_type {
        if !is_ffi_type(rt, registry) {
            diags.push(ffi_type_error(
                &format!("the return type `{}` can't cross from Rust", rt.name()),
                "foreign functions must return owned values Jet understands",
                "use an allowed return type, or flatten the result into simpler parts",
                ef.return_type_span.unwrap_or(ef.name_span),
            ));
            ok = false;
        }
    }
    ok
}

/// D-FFI-CAP1: make a validated foreign returned handle satisfy the same
/// nominal `Close` protocol as every other resource. The call checker sees
/// this registry fact before it checks the function body; codegen emits the
/// matching bridge-backed implementation.
pub(crate) fn register_foreign_close_impls(functions: &[ExternFn], traits: &mut TraitRegistry) {
    for function in functions {
        let Some(return_type) = function.return_type.as_ref() else {
            continue;
        };
        if function.close.is_none() {
            continue;
        }
        let Some(type_name) = (match return_type {
            Type::Named(name) | Type::Apply { name, .. } => Some(name.clone()),
            Type::String => Some(Syntax::TYPE_STRING.to_string()),
            _ => None,
        }) else {
            continue;
        };
        traits
            .trait_impls
            .insert((type_name, Syntax::TRAIT_CLOSE.to_string()));
    }
}

/// D-FFI-CAP1: a foreign close is a normal consuming close protocol, not an
/// unchecked destructor guess. The declaration must name a sibling foreign
/// function with one matching `^Handle` parameter and no result, so the
/// compiler can prove the close call is unique and cannot fabricate a cleanup
/// success value.
fn check_close_contract(
    ef: &ExternFn,
    functions: &[ExternFn],
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let Some((close_name, close_span)) = &ef.close else {
        return true;
    };
    let Some(return_type) = ef.return_type.as_ref() else {
        diags.push(ffi_type_error(
            &format!("`#Close({close_name})` is attached to `{}`", ef.name),
            "a close function belongs to a returned foreign handle",
            "remove `#Close`, or give the foreign function a handle return type",
            *close_span,
        ));
        return false;
    };
    let Some(close_fn) = functions
        .iter()
        .find(|candidate| candidate.name == *close_name)
    else {
        diags.push(ffi_type_error(
            &format!("foreign close function `{close_name}` is not declared"),
            "the close protocol must name a function in the same foreign binding",
            "declare `fn {close_name}(handle: ^Handle);` in this binding, or remove `#Close`",
            *close_span,
        ));
        return false;
    };
    let valid = close_fn.params.len() == 1
        && !close_fn.params[0].variadic
        && close_fn.params[0].convention == AccessConvention::Move
        && close_fn.params[0].ty == *return_type
        && close_fn.return_type.is_none();
    if valid {
        return true;
    }
    diags.push(ffi_type_error(
        &format!("foreign close function `{close_name}` has the wrong signature"),
        "`#Close` runs the named function exactly once by consuming the returned handle",
        &format!(
            "declare `fn {close_name}(handle: ^{})` with no return value",
            return_type.name()
        ),
        close_fn.name_span,
    ));
    false
}

/// S59 (E2-M14): type rules at the **C** boundary. Stricter than Rust FFI: only
/// scalars, `Char`, and `String` (D-CBIND5) cross by value, plus structs/enums
/// whose fields are all C-safe. Aggregates (`[T]`, `[K,V]`, `T?`, `T E!`) have
/// no stable C ABI and are rejected (E3203). Pointers (`Ptr<T>`, M13/S58) belong
/// to the gated tier: a `Ptr<T>` in a C signature fires E3202 unless it is behind
/// `use core.mem` + `#Unsafe`.
pub(crate) fn is_c_abi_type(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::String => true,
        // D-SG9: fixed-width integers/floats have a stable C ABI (`u8` … `i64`, `f32`).
        Type::IntN { .. } | Type::Float32 => true,
        // Generated binding-cache modules are parsed after the entry module's
        // builtin-type normalization pass. Nested callback signatures can
        // therefore retain the canonical scalar spelling as `Named`; those
        // spellings have the same stable ABI as their normalized variants.
        Type::Named(name) => {
            matches!(
                name.as_str(),
                "Int"
                    | "Float"
                    | "Bool"
                    | "Char"
                    | "String"
                    | "I8"
                    | "I16"
                    | "I32"
                    | "I64"
                    | "U8"
                    | "U16"
                    | "U32"
                    | "U64"
                    | "F32"
            ) || c_named_type_ok(name, registry)
        }
        // No stable C ABI for these by value:
        Type::List(_)
        | Type::Map { .. }
        | Type::Option(_)
        | Type::Result { .. }
        | Type::Shared(_)
        | Type::Apply { .. }
        | Type::TraitObject(_)
        | Type::Tuple(_)
        | Type::FixedList { .. } => false,
        Type::Fn {
            params,
            ret,
            effect_bound,
            ..
        } => {
            matches!(effect_bound, Some(b) if b.is_empty())
                && params.iter().all(|p| is_c_abi_type(p, registry))
                && ret.as_deref().is_none_or(|r| is_c_abi_type(r, registry))
        }
        Type::Tagged { inner, .. } => is_c_abi_type(inner, registry),
        Type::InlineRange { base, .. } => is_c_abi_type(base, registry),
        Type::Union(_) => false,
        Type::Quantity { .. } => false,
        Type::Measure(_) => false,
    }
}

pub(crate) fn c_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    match registry.types.get(name) {
        // Card #436 / D-REPRC1: only a `#Layout(c)` struct has a defined,
        // C-matching Rust layout (codegen stamps `#[repr(C)]` on it, and
        // `#Layout(c)` already bans growable fields via E1104). A plain
        // struct's Rust field order/size/padding is unspecified — accepting
        // it here would let sema wave through a shape CModule codegen (and
        // rustc) cannot lower correctly (I2/I3).
        Some(TypeDef::Struct {
            fields,
            is_c_layout,
            ..
        }) => *is_c_layout && fields.iter().all(|(_, _, ty)| is_c_abi_type(ty, registry)),
        // No ratified C-safe enum representation exists yet (tag placement,
        // discriminant width, payload union layout are all undecided — see
        // card #436 report). Reject every enum at the C boundary rather than
        // let sema accept a shape CModule codegen can't lower (I2/I3); a
        // future `#Layout(c) enum` design needs an owner ballot first.
        Some(TypeDef::Enum {
            variants,
            c_layout_tag,
            ..
        }) => {
            c_layout_tag.is_some()
                && variants.values().all(|(_, payload)| match payload {
                    VariantPayload::Unit => true,
                    VariantPayload::Single(ty, _) => is_c_abi_type(ty, registry),
                    VariantPayload::Named(fields) => {
                        fields.iter().all(|f| is_c_abi_type(&f.ty, registry))
                    }
                })
        }
        // D-DIST1: distinct types are repr(transparent) over their base, so
        // they share the base's C ABI exactly — treat as C-compatible iff
        // the base is.
        Some(TypeDef::Distinct { base, .. }) => is_c_abi_type(base, registry),
        Some(TypeDef::Alias { target, .. }) => is_c_abi_type(target, registry),
        None => false,
    }
}

/// E3211 (card #436) — a string literal with a known, comptime interior NUL
/// byte is passed to a C-boundary function's `String` parameter. C strings
/// are NUL-terminated, not length-prefixed, so the byte can never make the
/// trip — `CString::new` would fail at runtime. Caught here for any literal
/// (an all-`Lit` `Expr::Str`, no interpolation) because the value — and the
/// bug — are already fully known at compile time; a value built at runtime
/// panics instead (see `Codegen/CModule.rs`'s `NUL_PANIC`, documented in
/// docs/spec/diagnostics.md).
pub(crate) fn e3211(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3211",
        "This string literal has an embedded NUL byte, so it can't cross into a C function."
            .to_string(),
        "C strings are NUL-terminated, not length-prefixed — an embedded `\\0` would truncate the string on the C side, silently losing everything after it.".to_string(),
        "Remove the embedded NUL, or split the call so the C function only sees the part before it.".to_string(),
        Some(span),
    )
}

/// E3203 — a non-C-ABI type appears by value in a C FFI signature.
pub(crate) fn e3203(ty: &Type, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3203",
        format!(
            "`{}` is not a C-compatible type for a foreign function parameter or return.",
            ty.name()
        ),
        format!(
            "`#{}` / `#{}` functions must use types with a stable C ABI at the edge.",
            Syntax::MARKER_EXTERN_MODULE,
            Syntax::MARKER_BINDGEN,
        ),
        "Use scalars, `String`, or a struct with C layout; pointers only through the gated tier."
            .to_string(),
        Some(span),
    )
}

/// E3202 — a pointer type (`Ptr<T>`, S58) appears by value in a C FFI signature
/// outside an `#Unsafe` / `core.mem` region. Ordinary C-FFI code passes by-value
/// scalars and `String`; pointers must stay behind `use core.mem` + `#Unsafe`.
/// Reachable since the M13 pointer tier shipped (commit cd4713d).
pub fn e3202(ty: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3202",
        format!("Type `{}` cannot cross the C boundary here.", ty),
        "C FFI allows by-value scalars and `String` in ordinary code; pointers and other gated types need `use core.mem` and an `#Unsafe { … }` region (S58)."
            .to_string(),
        "Move the call inside `#Unsafe`, or change the type to a C-safe value type.".to_string(),
        Some(span),
    )
}

/// E3301 — an OS-dependent std API was called in a `--freestanding` build.
pub fn e3301(api: &str, hint: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3301",
        format!("`{}` is not available in a freestanding build.", api),
        "`--freestanding` targets have no OS; only `core`-level APIs are available.".to_string(),
        hint.to_string(),
        Some(span),
    )
}

/// E3302 — the target triple is unknown or its toolchain component is missing.
pub fn e3302(triple: &str) -> Diagnostic {
    Diagnostic::error(
        "E3302",
        format!("Target `{}` is not available.", triple),
        "rustc doesn't have the standard library for this target compiled in, \
         or the target triple is not recognised."
            .to_string(),
        "Run `jet self doctor --target <triple>` to see what's missing, \
         or `rustup target add <triple>` to install it."
            .to_string(),
        None,
    )
}

/// E3303 — freestanding build needs an allocator but none is configured.
pub fn e3303(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3303",
        "This freestanding program allocates memory but has no global allocator configured."
            .to_string(),
        "`--freestanding` builds cannot use the OS heap; a custom allocator is required."
            .to_string(),
        "Add `use core.mem;` and configure an arena or fixed allocator with `mem.set_allocator(…)`."
            .to_string(),
        Some(span),
    )
}

/// Validate one C FFI module's signatures (E3203/E3202). Registers nothing; the
/// caller registers the functions after a clean check.
pub(crate) fn check_c_module(
    cm: &CModule,
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    for ef in &cm.functions {
        if let Some((abi, span)) = &ef.abi {
            let known = matches!(
                abi.as_str(),
                "system" | "cdecl" | "stdcall" | "fastcall" | "win64" | "sysv64"
            );
            if !known {
                diags.push(Diagnostic::error(
                    "E3212",
                    format!("`{abi}` is not a known C calling convention"),
                    "`#ABI` accepts only the ratified native ABI names".to_string(),
                    "use `system`, `cdecl`, `stdcall`, `fastcall`, `win64`, or `sysv64`"
                        .to_string(),
                    Some(*span),
                ));
                ok = false;
            } else {
                let available = match abi.as_str() {
                    "system" => true,
                    "cdecl" | "stdcall" | "fastcall" => {
                        cfg!(all(target_os = "windows", target_arch = "x86"))
                    }
                    "win64" => cfg!(all(target_os = "windows", target_arch = "x86_64")),
                    "sysv64" => cfg!(all(not(target_os = "windows"), target_arch = "x86_64")),
                    _ => false,
                };
                if !available {
                    diags.push(Diagnostic::error("E3213", format!("`{abi}` is not available on this target"), "native calling conventions are restricted by operating system and architecture".to_string(), "use the default C ABI or `system` for portable declarations".to_string(), Some(*span)));
                    ok = false;
                }
                if ef.params.iter().any(|p| p.variadic)
                    && !(abi == "cdecl" && cfg!(all(target_os = "windows", target_arch = "x86")))
                {
                    diags.push(Diagnostic::error(
                        "E3214",
                        format!("variadic C function `{}` cannot use `{abi}`", ef.name),
                        "variadics allow only the default C ABI, or cdecl on Windows x86"
                            .to_string(),
                        "remove `#ABI`, or use `#ABI(cdecl)` on Windows x86".to_string(),
                        Some(*span),
                    ));
                    ok = false;
                }
            }
        }
        for p in &ef.params {
            if let Type::Apply { name, args } = &p.ty {
                if name == Syntax::TYPE_PTR {
                    // D-CABI-RESULT1=C: raw, non-null out pointers preserve the C
                    // header exactly. The call is marked unsafe by `extern_to_sig`;
                    // only a single C-safe pointee is admitted.
                    if args.len() != 1 || !is_c_abi_type(&args[0], registry) {
                        diags.push(e3203(&p.ty, p.ty_span));
                        ok = false;
                    }
                } else if !is_c_abi_type(&p.ty, registry) {
                    diags.push(e3203(&p.ty, p.ty_span));
                    ok = false;
                }
            } else if !is_c_abi_type(&p.ty, registry) {
                diags.push(e3203(&p.ty, p.ty_span));
                ok = false;
            }
        }
        if let Some(rt) = &ef.return_type {
            if matches!(rt, Type::Apply { name, .. } if name == Syntax::TYPE_PTR) {
                diags.push(e3202(
                    &rt.name(),
                    ef.return_type_span.unwrap_or(ef.name_span),
                ));
                ok = false;
            } else if !is_c_abi_type(rt, registry) {
                diags.push(e3203(rt, ef.return_type_span.unwrap_or(ef.name_span)));
                ok = false;
            }
        }
        if !check_close_contract(ef, &cm.functions, diags) {
            ok = false;
        }
    }
    ok
}

pub(crate) fn access_keyword(c: AccessConvention) -> &'static str {
    match c {
        AccessConvention::Read => "read",
        AccessConvention::Write => Syntax::SIGIL_WRITE,
        AccessConvention::Move => Syntax::SIGIL_MOVE,
    }
}

pub(crate) fn ffi_type_error(what: &str, why: &str, fix: &str, span: Span) -> Diagnostic {
    Diagnostic::from_row(
        "E0702",
        &[("subject", what), ("reason", why), ("fix", fix)],
        Some(span),
    )
}

/// D-FFI-CAP1: raw foreign capabilities are typed in the declaration, but a
/// call must name an audited boundary unless a generated binding owns the
/// adapter. The generated binding distinction is carried by later binder
/// work; this common seam keeps the safe/#Unsafe diagnostic identical today.
pub(crate) fn ffi_capability_error(name: &str, capability: &str, span: Span) -> Diagnostic {
    ffi_type_error(
        &format!("call `{name}` uses capability `{capability}` outside `#Unsafe`"),
        "a foreign capability can write to or transfer ownership of Jet storage, so a raw call needs an audited boundary",
        &format!(
            "call `{name}` inside `#Unsafe(\"…\") {{ … }}`, or expose it through a generated typed binding"
        ),
        span,
    )
}

/// D-FFI-CAP1: every call path uses the same safe/#Unsafe boundary rule. Keep
/// the ordinary `#Unsafe` diagnostic for contracts without an explicit
/// capability, and use E0702 when a raw foreign signature names `&` or `^`.
pub(crate) fn foreign_call_boundary_error(
    sig: &FuncSig,
    name: &str,
    in_unsafe: bool,
    span: Span,
) -> Option<Diagnostic> {
    if !sig.is_unsafe || in_unsafe {
        return None;
    }
    if sig.is_extern {
        if let Some(capability) = ffi_capability(sig) {
            return Some(ffi_capability_error(name, capability, span));
        }
    }
    Some(Diagnostic::error(
        "E3103",
        format!("`{name}` is an `#Unsafe` function"),
        "its contract can't be checked by the compiler, so the caller must vouch for it"
            .to_string(),
        format!("call it inside `#{}(\"…\") {{ … }}`", Syntax::KW_UNSAFE),
        Some(span),
    ))
}

/// Return the first explicit capability in a foreign signature. The signature
/// retains declaration order, so this is stable even when a later descriptor
/// grows more capability metadata.
pub(crate) fn ffi_capability(sig: &FuncSig) -> Option<&'static str> {
    sig.params
        .iter()
        .find_map(|(convention, _)| match convention {
            AccessConvention::Read => None,
            AccessConvention::Write => Some(Syntax::SIGIL_WRITE),
            AccessConvention::Move => Some(Syntax::SIGIL_MOVE),
        })
}

pub(crate) fn is_ffi_type(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Shared(_) => false,
        Type::List(inner) | Type::Option(inner) => is_ffi_type(inner, registry),
        Type::Map { key, value, .. } => is_ffi_type(key, registry) && is_ffi_type(value, registry),
        Type::Result { ok, err } => is_ffi_type(ok, registry) && is_ffi_type(err, registry),
        Type::Named(name) => ffi_named_type_ok(name, registry),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Fn { .. } | Type::Tuple(_) => false,
        Type::FixedList { elem, .. } => is_ffi_type(elem, registry),
        Type::Tagged { inner, .. } => is_ffi_type(inner, registry),
        Type::InlineRange { base, .. } => is_ffi_type(base, registry),
        Type::Union(_) => false,
        Type::Quantity { .. } => false,
        Type::Measure(_) => false,
    }
}

pub(crate) fn ffi_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    match registry.types.get(name) {
        Some(TypeDef::Struct { fields, .. }) => {
            fields.iter().all(|(_, _, ty)| is_ffi_type(ty, registry))
        }
        Some(TypeDef::Enum { variants, .. }) => {
            variants.values().all(|(_, payload)| match payload {
                VariantPayload::Unit => true,
                VariantPayload::Single(ty, _) => is_ffi_type(ty, registry),
                VariantPayload::Named(fs) => fs.iter().all(|f| is_ffi_type(&f.ty, registry)),
            })
        }
        // D-DIST1: distinct types are repr(transparent) over a scalar; treat as FFI-compatible.
        Some(TypeDef::Distinct { base, .. }) => is_ffi_type(base, registry),
        Some(TypeDef::Alias { target, .. }) => is_ffi_type(target, registry),
        None => false,
    }
}

pub(crate) fn register_extern_fn(
    ef: &ExternFn,
    funcs: &mut HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
    diags: &mut Vec<Diagnostic>,
    is_c_abi: bool,
    prelude_enabled: bool,
) {
    if ef.name == Syntax::BUILTIN_ASSERT_EQ || ef.name == Syntax::BUILTIN_EXPECT {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", ef.name),
            format!("`{}` is provided by the language itself", ef.name),
            "choose a different name for this foreign function".to_string(),
            Some(ef.name_span),
        ));
        return;
    }
    if prelude_enabled && CorePrelude::entry(&ef.name).is_some() {
        diags.push(crate::Sema::Prelude::shadow_warning(&ef.name, ef.name_span));
    }
    if name_defined(&ef.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &ef.name,
            "every function needs a unique name so calls aren't ambiguous",
            ef.name_span,
        ));
        return;
    }
    funcs.insert(ef.name.clone(), extern_to_sig(ef, is_c_abi));
}

pub(crate) struct ForeignUndoContract<'a> {
    pub forward_name: &'a str,
    pub forward_params: &'a [Param],
    pub inverse: &'a str,
    pub inverse_span: Span,
}

pub(crate) fn foreign_undo_contracts(items: &[Item]) -> Vec<ForeignUndoContract<'_>> {
    fn collect<'a>(items: &'a [Item], contracts: &mut Vec<ForeignUndoContract<'a>>) {
        for item in items {
            match item {
                Item::Func(function) if function.inline_foreign.is_some() => {
                    if let Some((inverse, inverse_span)) = &function.undo {
                        contracts.push(ForeignUndoContract {
                            forward_name: &function.name,
                            forward_params: &function.params,
                            inverse,
                            inverse_span: *inverse_span,
                        });
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        collect(body, contracts);
                    }
                }
                Item::ExternRust(block) => {
                    for foreign in &block.functions {
                        if let Some((inverse, inverse_span)) = &foreign.undo {
                            contracts.push(ForeignUndoContract {
                                forward_name: &foreign.name,
                                forward_params: &foreign.params,
                                inverse,
                                inverse_span: *inverse_span,
                            });
                        }
                    }
                }
                Item::CModule(module) => {
                    for foreign in &module.functions {
                        if let Some((inverse, inverse_span)) = &foreign.undo {
                            contracts.push(ForeignUndoContract {
                                forward_name: &foreign.name,
                                forward_params: &foreign.params,
                                inverse,
                                inverse_span: *inverse_span,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut contracts = Vec::new();
    collect(items, &mut contracts);
    contracts
}

pub(crate) fn validate_foreign_undo_contract(
    contract: &ForeignUndoContract<'_>,
    funcs: &HashMap<String, FuncSig>,
    diags: &mut Vec<Diagnostic>,
) {
    validate_undo_target_parts(
        contract.forward_name,
        contract.forward_params,
        contract.inverse,
        contract.inverse_span,
        funcs,
        diags,
    );
}

/// D-BOUND-UNDO1=A: validate foreign undo targets after the complete module
/// registration pass. A target may be declared later in the file, so checking
/// while the foreign item is registered would make source order observable.
pub(crate) fn validate_foreign_undo_contracts(
    items: &[Item],
    funcs: &HashMap<String, FuncSig>,
    diags: &mut Vec<Diagnostic>,
) {
    for contract in foreign_undo_contracts(items) {
        validate_foreign_undo_contract(&contract, funcs, diags);
    }
}

fn validate_undo_target_parts(
    forward_name: &str,
    forward_params: &[Param],
    inverse: &str,
    span: Span,
    funcs: &HashMap<String, FuncSig>,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(inverse_sig) = funcs.get(inverse) else {
        diags.push(Diagnostic::error(
            "E0102",
            format!("undo function `{inverse}` is not defined"),
            "rollback must call a function that is registered in the same program".to_string(),
            format!("define `fn {inverse}(…) {{ … }}` or correct the `#Undo` name"),
            Some(span),
        ));
        return;
    };
    let forward_params = forward_params
        .iter()
        .map(|param| {
            let ty = if param.variadic {
                Type::List(Box::new(param.ty.clone()))
            } else {
                param.ty.clone()
            };
            (param.convention, ty)
        })
        .collect::<Vec<_>>();
    if inverse_sig.params.len() != forward_params.len() {
        diags.push(Diagnostic::error(
            "E0104",
            format!(
                "undo function `{inverse}` expects {} argument{}, but `{forward_name}` has {}",
                inverse_sig.params.len(),
                if inverse_sig.params.len() == 1 {
                    ""
                } else {
                    "s"
                },
                forward_params.len(),
            ),
            "the compensating call receives the same captured arguments as the foreign call"
                .to_string(),
            format!("give `{inverse}` the same parameter count as `{forward_name}`"),
            Some(span),
        ));
        return;
    }

    for (index, ((forward_convention, forward_type), (inverse_convention, inverse_type))) in
        forward_params.iter().zip(&inverse_sig.params).enumerate()
    {
        let same_type = fn_types_compatible(
            &undo_callable_type(std::slice::from_ref(forward_type), None),
            &undo_callable_type(std::slice::from_ref(inverse_type), None),
        );
        if *forward_convention != *inverse_convention || !same_type {
            let reason = if forward_convention != inverse_convention {
                format!(
                    "access convention `{}` does not match `{}`",
                    access_keyword(*inverse_convention),
                    access_keyword(*forward_convention),
                )
            } else {
                format!(
                    "type `{}` does not match `{}`",
                    inverse_type.name(),
                    forward_type.name(),
                )
            };
            diags.push(Diagnostic::error(
                "E0112",
                format!(
                    "undo function `{inverse}` parameter {} is incompatible with `{forward_name}`: {reason}",
                    index + 1,
                ),
                "rollback passes the foreign call's captured arguments to the inverse with the same Jet signature"
                    .to_string(),
                format!(
                    "make parameter {} of `{inverse}` use the same type and access convention as `{forward_name}`",
                    index + 1,
                ),
                Some(span),
            ));
        }
    }

    if let Some(return_type) = inverse_sig
        .return_type
        .as_ref()
        .filter(|ty| !is_unit_return(ty))
    {
        diags.push(Diagnostic::error(
            "E0113",
            format!(
                "undo function `{inverse}` returns `{}`, but rollback functions must return Unit",
                return_type.name(),
            ),
            "rollback invokes the inverse for its side effect and cannot use a returned value"
                .to_string(),
            format!(
                "remove `{}` from `{inverse}` so it returns Unit",
                return_type.name()
            ),
            Some(span),
        ));
    }
}

fn undo_callable_type(params: &[Type], return_type: Option<Type>) -> Type {
    Type::Fn {
        params: params.to_vec(),
        ret: return_type.map(Box::new),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: None,
    }
}

fn is_unit_return(ty: &Type) -> bool {
    matches!(ty, Type::Named(name) if name == Syntax::INTERNAL_UNIT_TYPE)
}
