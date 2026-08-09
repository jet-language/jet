use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{AccessConvention, CModule, ExternFn, ExternRustBlock, Type, VariantPayload};
use jet_foundation::Prelude as CorePrelude;
use std::collections::HashMap;

pub(crate) fn cpp_callback_abi_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::CppCallbackAbi))
                && matches!(inner.as_ref(), Type::Fn { .. }) =>
        {
            Some(inner)
        }
        _ => None,
    }
}

pub(crate) fn is_callback_boundary_param(is_c_abi: bool, ty: &Type) -> bool {
    (is_c_abi && matches!(ty, Type::Fn { .. })) || cpp_callback_abi_type(ty).is_some()
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
            diags.push(Diagnostic::error("E3212", "`#ABI` only applies to C declarations".to_string(), "Rust FFI uses its declared Rust ABI and cannot select a C calling convention".to_string(), "remove `#ABI` from the `extern rust` function".to_string(), Some(*span)));
            ok = false;
        }
        if !check_extern_fn(ef, registry, diags) {
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
        // D-MEM1: unmarked (`Read`) is by-value and fine at the boundary; only
        // an explicit `&`/`^` marker is rejected.
        if !matches!(p.convention, AccessConvention::Read) {
            diags.push(ffi_type_error(
                &format!(
                    "`{}` can't use `{}` at the FFI boundary",
                    p.name,
                    access_keyword(p.convention)
                ),
                "foreign functions take owned copies — the write-capability marker `&` and move-capability marker `^` aren't allowed here",
                "remove the capability sigil and pass by value",
                p.name_span,
            ));
            ok = false;
        }
        if !is_ffi_type(&p.ty, registry) {
            diags.push(ffi_type_error(
                &format!("`{}` has type `{}`, which can't cross into Rust", p.name, p.ty.name()),
                "only plain value types can cross the `extern rust` boundary — no references, callbacks, or trait objects",
                "use `Int`, `Float`, `Bool`, `String`, `Char`, collections of those, or a struct whose fields are allowed",
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

/// S59 (E2-M14): type rules at the **C** boundary. Stricter than Rust FFI: only
/// scalars, `Char`, and `String` (D-CBIND5) cross by value, plus structs/enums
/// whose fields are all C-safe. Aggregates (`[T]`, `[K,V]`, `T?`, `T ? E`) have
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
                "Int" | "Float" | "Bool" | "Char" | "String"
                    | "I8" | "I16" | "I32" | "I64"
                    | "U8" | "U16" | "U32" | "U64" | "F32"
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
        Type::Fn { params, ret, effect_bound, .. } => {
            matches!(effect_bound, Some(b) if b.is_empty())
                && params.iter().all(|p| is_c_abi_type(p, registry))
                && ret.as_deref().is_none_or(|r| is_c_abi_type(r, registry))
        }
        Type::Tagged { inner, .. } => is_c_abi_type(inner, registry),
        Type::Union(_) => false,
        Type::Quantity { .. } => false,
        Type::ComputeDim(_) => false,
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
        }) => {
            *is_c_layout
                && fields
                    .iter()
                    .all(|(_, _, ty, _)| is_c_abi_type(ty, registry))
        }
        // No ratified C-safe enum representation exists yet (tag placement,
        // discriminant width, payload union layout are all undecided — see
        // card #436 report). Reject every enum at the C boundary rather than
        // let sema accept a shape CModule codegen can't lower (I2/I3); a
        // future `#Layout(c) enum` design needs an owner ballot first.
        Some(TypeDef::Enum { variants, c_layout_tag, .. }) => {
            c_layout_tag.is_some() && variants.values().all(|(_, payload)| match payload {
                VariantPayload::Unit => true,
                VariantPayload::Single(ty, _) => is_c_abi_type(ty, registry),
                VariantPayload::Named(fields) => fields.iter().all(|f| is_c_abi_type(&f.ty, registry)),
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
            let known = matches!(abi.as_str(), "system" | "cdecl" | "stdcall" | "fastcall" | "win64" | "sysv64");
            if !known {
                diags.push(Diagnostic::error("E3212", format!("`{abi}` is not a known C calling convention"), "`#ABI` accepts only the ratified native ABI names".to_string(), "use `system`, `cdecl`, `stdcall`, `fastcall`, `win64`, or `sysv64`".to_string(), Some(*span)));
                ok = false;
            } else {
                let available = match abi.as_str() {
                    "system" => true,
                    "cdecl" | "stdcall" | "fastcall" => cfg!(all(target_os = "windows", target_arch = "x86")),
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
                    diags.push(Diagnostic::error("E3214", format!("variadic C function `{}` cannot use `{abi}`", ef.name), "variadics allow only the default C ABI, or cdecl on Windows x86".to_string(), "remove `#ABI`, or use `#ABI(cdecl)` on Windows x86".to_string(), Some(*span)));
                    ok = false;
                }
            }
        }
        for p in &ef.params {
            // D-MEM1: unmarked (`Read`) is by-value; only explicit markers reject.
            if !matches!(p.convention, AccessConvention::Read) {
                diags.push(ffi_type_error(
                    &format!(
                        "`{}` can't use `{}` at the C boundary",
                        p.name,
                        access_keyword(p.convention)
                    ),
                    "C functions take values by copy — the write-capability marker `&` and move-capability marker `^` aren't allowed here",
                    "remove the capability sigil and pass by value",
                    p.name_span,
                ));
                ok = false;
            }
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
                diags.push(e3202(&rt.name(), ef.return_type_span.unwrap_or(ef.name_span)));
                ok = false;
            } else if !is_c_abi_type(rt, registry) {
                diags.push(e3203(rt, ef.return_type_span.unwrap_or(ef.name_span)));
                ok = false;
            }
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
    Diagnostic::error(
        "E0702",
        what.to_string(),
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
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
        Type::Union(_) => false,
        Type::Quantity { .. } => false,
        Type::ComputeDim(_) => false,
    }
}

pub(crate) fn ffi_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    match registry.types.get(name) {
        Some(TypeDef::Struct { fields, .. }) => {
            fields.iter().all(|(_, _, ty, _)| is_ffi_type(ty, registry))
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
    if ef.name == Syntax::BUILTIN_REQUIRE_EQ || ef.name == Syntax::BUILTIN_EXPECT {
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
