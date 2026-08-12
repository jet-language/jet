use super::*;
use crate::AST::Type;

/// Card #436: a `String` argument crossing into a C function goes through
/// `CString::new`, which fails on an interior NUL byte (C strings have no
/// length — NUL IS the terminator, so a Jet `String` holding one can never be
/// represented as a C string). A value with a known-at-comptime NUL is
/// rejected in sema (E3211, `Sema/FFI.rs`) before it ever reaches codegen; a
/// value only known at runtime cannot be — so the generated wrapper panics
/// with a real Jet-style message instead of the old
/// `.unwrap_or_default()` (silently sending the C function an EMPTY string,
/// which is a memory-safety-adjacent lie, not an error). Documented in
/// docs/spec/diagnostics.md (E3211) and docs/spec/spec.md (C FFI section).
const NUL_PANIC: &str =
    "a String with an embedded NUL byte can't cross into a C function (C strings are NUL-terminated, not length-prefixed)";
const NULL_RETURN_PANIC: &str =
    "a C function declared to return String returned a null pointer";
const UTF8_RETURN_PANIC: &str =
    "a C function declared to return String returned bytes that are not valid UTF-8";

/// Card #436: `CModule` functions are always emitted in a synthetic per-lib
/// Rust module (`CFFI::assemble` in the jetpack crate folds every
/// `#Extern`/`#Bindgen module` into its own `<c.lib>` "file", separate from
/// wherever the struct/enum/distinct it references was actually declared) —
/// so this module's `cx` never has a local type of its own; every `Named`
/// type it sees belongs to the root program and needs `cx.root_prefix`
/// (`super::` when this is a nested module, empty at the root) to resolve.
/// `cx.rust_type` already prepends that prefix itself for the handful of
/// reserved/builtin type names it special-cases (e.g. `Point` → `JetPoint`);
/// only the generic user-struct/enum/distinct fallback (`Codegen/Context.rs`,
/// `Type::Named(name) => mangle_path(name)`) does not, since ordinarily
/// that fallback fires for a type local to the CURRENT module (no prefix
/// needed there). Detect which case this is by whether the resolved name
/// already carries the prefix.
fn qualify_named_rust_type(cx: &Cx, ty: &Type) -> String {
    let base = cx.rust_type(ty);
    if !cx.root_prefix.is_empty() && !base.starts_with(cx.root_prefix.as_str()) {
        format!("{}{}", cx.root_prefix, base)
    } else {
        base
    }
}

pub(crate) fn emit_c_module(cx: &Cx, cm: &crate::AST::CModule, out: &mut String) {
    if cm.functions.is_empty() {
        return;
    }
    for ef in &cm.functions {
        let params: Vec<String> = ef
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| format!("a{i}: {}", c_abi_rust_type(&p.ty, cx, p.ty_span)))
            .collect();
        let ret = ef
            .return_type
            .as_ref()
            .map(|t| {
                format!(
                    " -> {}",
                    c_abi_rust_type(t, cx, ef.return_type_span.unwrap_or(ef.span))
                )
            })
            .unwrap_or_default();
        let abi = ef.abi.as_ref().map(|(a, _)| a.as_str()).unwrap_or("C");
        out.push_str(&format!(
            "#[allow(non_snake_case)]\nextern \"{}\" {{\n    fn {}({}){};\n}}\n",
            abi,
            ef.rust_path,
            params.join(", "),
            ret
        ));
    }
    out.push('\n');

    for ef in &cm.functions {
        let mut sig_params = Vec::new();
        let mut conv_lines = Vec::new();
        let mut call_args = Vec::new();
        for (i, p) in ef.params.iter().enumerate() {
            sig_params.push(format!(
                "a{i}: {}",
                c_wrapper_param_type(&p.ty, cx, p.ty_span)
            ));
            match &p.ty {
                Type::String => {
                    conv_lines.push(format!(
                        "    let c{i} = std::ffi::CString::new(a{i}.as_str()).unwrap_or_else(|_| jet_panic(file!(), line!(), \"{NUL_PANIC}\"));"
                    ));
                    call_args.push(format!("c{i}.as_ptr()"));
                }
                Type::Char => call_args.push(format!("(*a{i} as u32)")),
                // Card #436: a struct/distinct crosses the ordinary Jet
                // call-site convention as `&T` (D-MEM1 Read, non-scalar —
                // `Context.rs::rust_param_type`), matching `String`/`Char`
                // above; `c_wrapper_param_type` mirrors that with `&T` too.
                // The real `extern "C"` declaration wants it by value, so
                // clone through the reference here. Sound because every
                // C-ABI-accepted struct (`#Layout(c)`, E1104-restricted to
                // fixed-size fields) and distinct is `Clone` (codegen always
                // derives it — `Context.rs::type_is_cloneable_struct` /
                // `Items.rs::emit_distinct`).
                Type::Named(_) => call_args.push(format!("(*a{i}).clone()")),
                _ => call_args.push(format!("a{i}")),
            }
        }
        let ret = ef
            .return_type
            .as_ref()
            .map(|t| {
                format!(
                    " -> {}",
                    c_wrapper_ret_type(t, cx, ef.return_type_span.unwrap_or(ef.span))
                )
            })
            .unwrap_or_default();
        let call = format!("{}({})", ef.rust_path, call_args.join(", "));
        let call_body = match &ef.return_type {
            None => format!("    unsafe {{ {}; }}", call),
            Some(Type::String) => format!(
                "    let p = unsafe {{ {} }};\n    if p.is_null() {{ jet_panic(file!(), line!(), \"{NULL_RETURN_PANIC}\"); }}\n    let bytes = unsafe {{ std::ffi::CStr::from_ptr(p) }};\n    bytes.to_str().unwrap_or_else(|_| jet_panic(file!(), line!(), \"{UTF8_RETURN_PANIC}\")).to_owned()",
                call
            ),
            Some(Type::Char) => format!(
                "    let v = unsafe {{ {} }};\n    char::from_u32(v).unwrap_or('\\u{{0}}')",
                call
            ),
            Some(_) => format!("    unsafe {{ {} }}", call),
        };
        // Emit any argument-conversion lines (e.g. `String` → `CString`) before
        // the call. These bind the `c{i}` temporaries that `call_args` reference;
        // without them the wrapper references an undeclared variable (I2 bug).
        let body = if conv_lines.is_empty() {
            call_body
        } else {
            format!("{}\n{}", conv_lines.join("\n"), call_body)
        };
        out.push_str(&format!(
            "pub fn {}({}){} {{\n{}\n}}\n\n",
            mangle(&ef.name),
            sig_params.join(", "),
            ret,
            body
        ));
    }
}

/// The C-ABI Rust type used in the `extern "C"` declaration.
///
/// Card #436: `Sema::FFI::is_c_abi_type` is the source of truth for what may
/// reach here — every arm it accepts MUST have a matching arm below, or an
/// accepted-but-unlowered shape becomes an I2/I3 bug (rustc ICE or silent
/// `()`  placeholder). `IntN`/`Float32` (D-SG9, fixed-width) and a `Named`
/// struct/distinct (D-REPRC1/D-DIST1 — sema already required `#Layout(c)` /
/// a C-abi base) all get their ordinary generated Rust type via `cx.rust_type`,
/// which is ABI-identical to the C shape sema verified.
fn c_abi_rust_type(ty: &Type, cx: &Cx, span: crate::Diagnostics::Span) -> String {
    match ty {
        Type::Int => "std::os::raw::c_longlong".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "u32".to_string(),
        Type::String => "*const std::os::raw::c_char".to_string(),
        Type::IntN { .. } | Type::Float32 => cx.rust_type(ty),
        Type::Named(_) => qualify_named_rust_type(cx, ty),
        Type::Fn { params, ret, .. } => {
            let ps = params
                .iter()
                .map(|p| c_abi_rust_type(p, cx, span))
                .collect::<Vec<_>>()
                .join(", ");
            let r = ret
                .as_deref()
                .map(|t| format!(" -> {}", c_abi_rust_type(t, cx, span)))
                .unwrap_or_default();
            format!("extern \"C\" fn({ps}){r}")
        }
        Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
            format!("*mut {}", qualify_named_rust_type(cx, &args[0]))
        }
        Type::Tagged { inner, .. } => c_abi_rust_type(inner, cx, span),
        // Sema owns this closed set (I3). Reaching here is a compiler invariant
        // violation, never generated placeholder Rust.
        other => jet_foundation::ice!(
            Some(span),
            "I3: sema admitted unsupported C ABI type {}",
            other.name()
        ),
    }
}

/// The Rust type the safe wrapper accepts, matching cross-module call sites
/// (Read convention: scalars by value, `String`/`Char` by shared reference).
/// See `c_abi_rust_type` — same acceptance contract with `is_c_abi_type`.
fn c_wrapper_param_type(ty: &Type, cx: &Cx, span: crate::Diagnostics::Span) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "&char".to_string(),
        Type::String => "&String".to_string(),
        Type::IntN { .. } | Type::Float32 => cx.rust_type(ty),
        // Card #436: by-reference, matching the ordinary Jet call-site
        // convention for a non-scalar `Read` param (see the call-arg match
        // in `emit_c_module`, which clones through this reference before
        // the real `extern "C"` call).
        Type::Named(_) => format!("&{}", qualify_named_rust_type(cx, ty)),
        Type::Fn { .. } => c_abi_rust_type(ty, cx, span),
        Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
            format!("*mut {}", qualify_named_rust_type(cx, &args[0]))
        }
        Type::Tagged { inner, .. } => c_wrapper_param_type(inner, cx, span),
        other => jet_foundation::ice!(
            Some(span),
            "I3: sema admitted unsupported C ABI parameter {}",
            other.name()
        ),
    }
}

fn c_wrapper_ret_type(ty: &Type, cx: &Cx, span: crate::Diagnostics::Span) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "char".to_string(),
        Type::String => "String".to_string(),
        Type::IntN { .. } | Type::Float32 => cx.rust_type(ty),
        Type::Named(_) => qualify_named_rust_type(cx, ty),
        Type::Fn { .. } => c_abi_rust_type(ty, cx, span),
        Type::Tagged { inner, .. } => c_wrapper_ret_type(inner, cx, span),
        other => jet_foundation::ice!(
            Some(span),
            "I3: sema admitted unsupported C ABI return {}",
            other.name()
        ),
    }
}
