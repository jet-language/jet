use super::*;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{AccessConvention, CModule, ExternFn, ExternRustBlock, Type, VariantPayload};
use std::collections::HashMap;

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
    if ef.is_view_return {
        diags.push(ffi_type_error(
            "a `&` return can't cross into Rust",
            "foreign functions must return owned values — nothing borrowed across the boundary",
            "return the value directly, or wrap it in a `List` or `String`",
            ef.name_span,
        ));
        ok = false;
    }
    for p in &ef.params {
        // D-CAP8: `Infer` (unmarked) is by-value, like `Read` — both are fine at the
        // boundary; only an explicit `~`/`^`/`&` marker is rejected.
        if !matches!(
            p.convention,
            AccessConvention::Read | AccessConvention::Infer
        ) {
            diags.push(ffi_type_error(
                &format!(
                    "`{}` can't use `{}` at the FFI boundary",
                    p.name,
                    access_keyword(p.convention)
                ),
                "foreign functions take owned copies — `~`, `^`, and `&` aren't allowed here",
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
                ef.name_span,
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
/// `use core.mem` + `@unsafe`.
pub(crate) fn is_c_abi_type(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char | Type::String => true,
        // D-SG9: fixed-width integers/floats have a stable C ABI (`u8` … `i64`, `f32`).
        Type::IntN { .. } | Type::Float32 => true,
        Type::Named(name) => c_named_type_ok(name, registry),
        // No stable C ABI for these by value:
        Type::List(_)
        | Type::Map { .. }
        | Type::Option(_)
        | Type::Result { .. }
        | Type::Shared(_)
        | Type::Apply { .. }
        | Type::TraitObject(_)
        | Type::Fn { .. }
        | Type::Tuple(_)
        | Type::FixedList { .. } => false,
        Type::Tagged { inner, .. } => is_c_abi_type(inner, registry),
    }
}

pub(crate) fn c_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    match registry.types.get(name) {
        Some(TypeDef::Struct { fields, .. }) => fields
            .iter()
            .all(|(_, _, ty, _, _)| is_c_abi_type(ty, registry)),
        Some(TypeDef::Enum { variants, .. }) => {
            variants.values().all(|(_, payload)| match payload {
                VariantPayload::Unit => true,
                VariantPayload::Single(ty, _) => is_c_abi_type(ty, registry),
                VariantPayload::Named(fs) => fs.iter().all(|f| is_c_abi_type(&f.ty, registry)),
            })
        }
        // D-DIST1: distinct types are repr(transparent) over a scalar; treat as C-compatible.
        Some(TypeDef::Distinct { base, .. }) => is_c_abi_type(base, registry),
        Some(TypeDef::Alias { target, .. }) => is_c_abi_type(target, registry),
        None => false,
    }
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
            Syntax::ATTR_EXTERN_MODULE,
            Syntax::ATTR_BINDGEN,
        ),
        "Use scalars, `String`, or a struct with C layout; pointers only through the gated tier."
            .to_string(),
        Some(span),
    )
}

/// E3202 — a pointer type (`Ptr<T>`, S58) appears by value in a C FFI signature
/// outside an `@unsafe` / `core.mem` region. Ordinary C-FFI code passes by-value
/// scalars and `String`; pointers must stay behind `use core.mem` + `@unsafe`.
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
        "Run `jet doctor --target <triple>` to see what's missing, \
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
        if ef.is_view_return {
            diags.push(e3203(&Type::Named("view".to_string()), ef.name_span));
            ok = false;
        }
        for p in &ef.params {
            // D-CAP8: `Infer` (unmarked) is by-value like `Read`; only explicit markers reject.
            if !matches!(
                p.convention,
                AccessConvention::Read | AccessConvention::Infer
            ) {
                diags.push(ffi_type_error(
                    &format!(
                        "`{}` can't use `{}` at the C boundary",
                        p.name,
                        access_keyword(p.convention)
                    ),
                    "C functions take values by copy — `~`, `^`, and `&` aren't allowed here",
                    "remove the capability sigil and pass by value",
                    p.name_span,
                ));
                ok = false;
            }
            if matches!(&p.ty, Type::Apply { name, .. } if name == Syntax::TYPE_PTR) {
                diags.push(e3202(&p.ty.name(), p.ty_span));
                ok = false;
            } else if !is_c_abi_type(&p.ty, registry) {
                diags.push(e3203(&p.ty, p.ty_span));
                ok = false;
            }
        }
        if let Some(rt) = &ef.return_type {
            if matches!(rt, Type::Apply { name, .. } if name == Syntax::TYPE_PTR) {
                diags.push(e3202(&rt.name(), ef.name_span));
                ok = false;
            } else if !is_c_abi_type(rt, registry) {
                diags.push(e3203(rt, ef.name_span));
                ok = false;
            }
        }
    }
    ok
}

pub(crate) fn access_keyword(c: AccessConvention) -> &'static str {
    match c {
        AccessConvention::Read => "read",
        AccessConvention::Infer => "read", // unmarked defaults to read pre-resolution
        AccessConvention::Write => Syntax::SIGIL_MUTATE,
        AccessConvention::Move => Syntax::SIGIL_MOVE,
        AccessConvention::Share => Syntax::SIGIL_VIEW,
        AccessConvention::Raw => "raw",
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
        Type::Map { key, value } => is_ffi_type(key, registry) && is_ffi_type(value, registry),
        Type::Result { ok, err } => is_ffi_type(ok, registry) && is_ffi_type(err, registry),
        Type::Named(name) => ffi_named_type_ok(name, registry),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Fn { .. } | Type::Tuple(_) => false,
        Type::FixedList { elem, .. } => is_ffi_type(elem, registry),
        Type::Tagged { inner, .. } => is_ffi_type(inner, registry),
    }
}

pub(crate) fn ffi_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    if name == Syntax::TYPE_ERROR {
        return true;
    }
    match registry.types.get(name) {
        Some(TypeDef::Struct { fields, .. }) => fields
            .iter()
            .all(|(_, _, ty, _, _)| is_ffi_type(ty, registry)),
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
) {
    if ef.name == Syntax::BUILTIN_PRINT
        || ef.name == Syntax::BUILTIN_PANIC
        || ef.name == Syntax::BUILTIN_REQUIRE
        || ef.name == Syntax::BUILTIN_REQUIRE_EQ
        || ef.name == Syntax::BUILTIN_EXPECT
    {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", ef.name),
            format!("`{}` is provided by the language itself", ef.name),
            "choose a different name for this foreign function".to_string(),
            Some(ef.name_span),
        ));
        return;
    }
    if name_defined(&ef.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &ef.name,
            "every function needs a unique name so calls aren't ambiguous",
            ef.name_span,
        ));
        return;
    }
    funcs.insert(ef.name.clone(), extern_to_sig(ef));
}
