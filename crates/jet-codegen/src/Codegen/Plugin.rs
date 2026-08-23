//! D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A (c81): `target: sandbox`
//! guest artifacts — the wasm32 Component Model side of the sandboxed plugin
//! loader (the host side lives in `crates/jet-driver/src/Prelude/Plugin.rs`,
//! embedded via the FFI bridge). Reuses the ordinary whole-program `emit_bundle`
//! output verbatim (I3: codegen stays dumb, no second lowering path) and
//! appends one `#[export_name = "…"] pub extern "C" fn` wrapper per exported
//! `pub fn`, calling straight into the already-emitted `__jet_<name>` — the
//! same naming convention every other Jet top-level function gets.
//!
//! v1 scope, both real (working end-to-end) and deliberately narrow — a
//! documented boundary, not a stub:
//!   - an exported `pub fn` must have every parameter and its return type be
//!     one homogeneous Component Model scalar: `Int`, `Float`, `Bool`, or
//!     `Text` (checked before this runs — see
//!     `Jetpack::PluginExport::validate_export_surface` in the driver, I3: the
//!     check lives outside codegen). Non-conforming `pub fn`s are silently
//!     excluded from the export set here — driver-side validation already
//!     turned a non-conforming plugin build into a hard error, so this can
//!     never observe one in practice; this is a defensive skip, not the
//!     enforcement point.
//!   - only the entry file's TOP-LEVEL `pub fn` items are walked — a `pub fn`
//!     nested inside a `module <name> { … }` body isn't (yet) collected. A
//!     top-level function is always emitted as `__jet_<name>` in the whole-
//!     program Rust this module calls straight into; a module-scoped one may
//!     get a different mangled name, which needs verifying (not guessing)
//!     before this recurses into `Item::CodeModule` bodies the way
//!     `ApiFreeze::collect_pub_fns` already does for the analogous library-API
//!     freeze. A real follow-on, not a stub — v1 plugins are single flat
//!     files, which is already a complete, useful shape.

use crate::AST::{AccessConvention, Item, ProgramBundle, Type};
use jet_foundation::Names::mangle;

/// One exported plugin function's homogeneous scalar shape.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginScalar {
    Int,
    Float,
    Bool,
    Text,
}

impl PluginScalar {
    fn wit_ty(self) -> &'static str {
        match self {
            PluginScalar::Int => "s64",
            PluginScalar::Float => "f64",
            PluginScalar::Bool => "bool",
            PluginScalar::Text => "string",
        }
    }
    fn rust_ty(self) -> &'static str {
        match self {
            PluginScalar::Int => "i64",
            PluginScalar::Float => "f64",
            PluginScalar::Bool => "bool",
            PluginScalar::Text => "String",
        }
    }
}

/// The guest-side artifacts for a `target: sandbox` build.
#[derive(Debug, Clone)]
pub struct PluginArtifacts {
    /// The full `.wit` world text, e.g. `package jet:mathkit@0.1.0; world
    /// jetplugin { export scale: func(a: f64, b: f64) -> f64; … }`.
    pub wit: String,
    /// The complete guest Rust source (ordinary `emit_bundle` output plus the
    /// `#[export_name]` wrapper functions) — ready for
    /// `rustc --target wasm32-unknown-unknown --crate-type cdylib`.
    pub guest_rust: String,
    /// The `.wit` world name (`jetplugin`, fixed — the package name below is
    /// what varies per plugin).
    pub world_name: String,
    /// The sanitized export/world identity (from `export:`/package name).
    pub export_name: String,
    /// The Jet names of every function actually exported (for the ApiFreeze
    /// snapshot / version-handshake diagnostics).
    pub exported_fns: Vec<String>,
}

/// The one fixed `.wit` world name every plugin uses — only the `package`
/// identity (from `export_name`) varies (D-PLUGIN-EXPORT1=A: no new in-source
/// keyword, so there is nothing else to name per-plugin).
const WORLD_NAME: &str = "jetplugin";

/// Classify a `pub fn`'s signature for export: `Some(scalar)` when every
/// parameter and the return type are the same Component Model scalar, else
/// `None` (not exportable in v1 — see module doc).
pub fn plugin_export_shape(f: &crate::AST::Func) -> Option<PluginScalar> {
    let mut shape: Option<PluginScalar> = None;
    let note = |t: &Type, shape: &mut Option<PluginScalar>| -> bool {
        let this = match t {
            Type::Int => PluginScalar::Int,
            Type::Float => PluginScalar::Float,
            Type::Bool => PluginScalar::Bool,
            Type::String => PluginScalar::Text,
            _ => return false,
        };
        match shape {
            Some(s) if *s != this => false,
            Some(_) => true,
            None => {
                *shape = Some(this);
                true
            }
        }
    };
    for p in &f.params {
        if !note(&p.ty, &mut shape) {
            return None;
        }
    }
    match &f.return_type {
        Some(t) if note(t, &mut shape) => {}
        _ => return None,
    }
    shape
}

/// `snake_case` (or anything) -> `kebab-case`, the Component Model's required
/// identifier shape. Jet function names are ASCII identifiers, so a plain
/// `_` -> `-` swap is exact and total.
fn to_kebab(name: &str) -> String {
    name.replace('_', "-")
}

/// Sanitize an export/world identity into a valid `.wit` package name segment
/// (lowercase ASCII alphanumeric + `-`, must start with a letter).
fn sanitize_package_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
        format!(
            "plugin-{}",
            if out.is_empty() {
                "export".to_string()
            } else {
                out
            }
        )
    } else {
        out
    }
}

/// Build the guest artifacts for a `target: sandbox` package. `export_name` is
/// the manifest `export:` field value, or the package/payload name when
/// omitted (resolved by the caller — driver-layer manifest lookup, not
/// codegen's concern, I3).
pub fn emit_plugin(
    bundle: &ProgramBundle,
    whole_program_rust: &str,
    export_name: &str,
) -> PluginArtifacts {
    let sanitized = sanitize_package_name(export_name);
    let entry_items = &bundle.modules[bundle.entry].items;

    let mut wit_lines = Vec::new();
    let mut wrapper_fns = String::new();
    let mut exported = Vec::new();
    let mut has_text_export = false;

    for item in entry_items {
        let Item::Func(f) = item else { continue };
        if !bundle.name_ledger.public(bundle.entry, &f.name) {
            continue;
        }
        let Some(scalar) = plugin_export_shape(f) else {
            continue;
        };
        let kebab = to_kebab(&f.name);
        let wit_params: Vec<String> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, _)| format!("p{i}: {}", scalar.wit_ty()))
            .collect();
        wit_lines.push(format!(
            "  export {kebab}: func({}) -> {};",
            wit_params.join(", "),
            scalar.wit_ty()
        ));
        let wrapper_name = mangle(&format!("plugin_export_{}", f.name));
        if scalar == PluginScalar::Text {
            has_text_export = true;
            let rust_params: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .flat_map(|(i, _)| [format!("p{i}_ptr: i32"), format!("p{i}_len: i32")])
                .collect();
            let locals: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mutable = matches!(p.convention, AccessConvention::Write)
                        .then_some("mut ")
                        .unwrap_or_default();
                    format!("let {mutable}p{i} = __jet_plugin_read_text(p{i}_ptr, p{i}_len);")
                })
                .collect();
            let call_args: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .map(|(i, p)| match p.convention {
                    AccessConvention::Read => format!("&p{i}"),
                    AccessConvention::Write => format!("&mut p{i}"),
                    AccessConvention::Move => format!("p{i}"),
                })
                .collect();
            wrapper_fns.push_str(&format!(
                "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}({rust_params}) -> i32 {{ {locals} __jet_plugin_return_text({callee}({call_args})) }}\n#[export_name = \"cabi_post_{kebab}\"]\npub extern \"C\" fn {post_name}(ret_ptr: i32) {{ __jet_plugin_post_text(ret_ptr) }}\n",
                wrapper_name = wrapper_name,
                rust_params = rust_params.join(", "),
                locals = locals.join(" "),
                callee = mangle(&f.name),
                call_args = call_args.join(", "),
                post_name = mangle(&format!("plugin_post_{}", f.name)),
            ));
        } else {
            let rust_params: Vec<String> = f
                .params
                .iter()
                .enumerate()
                .map(|(i, _)| format!("p{i}: {}", scalar.rust_ty()))
                .collect();
            let call_args: Vec<String> = (0..f.params.len()).map(|i| format!("p{i}")).collect();
            wrapper_fns.push_str(&format!(
                "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}({rust_params}) -> {ret} {{ {callee}({call_args}) }}\n",
                wrapper_name = wrapper_name,
                rust_params = rust_params.join(", "),
                ret = scalar.rust_ty(),
                callee = mangle(&f.name),
                call_args = call_args.join(", "),
            ));
        }
        exported.push(f.name.clone());
    }

    let wit = format!(
        "package jet:{sanitized}@0.1.0;\n\nworld {WORLD_NAME} {{\n{}\n}}\n",
        wit_lines.join("\n")
    );

    let mut guest_rust = String::from(whole_program_rust);
    guest_rust.push_str(
        "\n// c81 / D-PLUGIN-EXPORT1=A: generated export wrappers (jet-codegen/Plugin.rs).\n",
    );
    if has_text_export {
        guest_rust.push_str(
            r#"
fn __jet_plugin_cabi_realloc_impl(
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
) -> i32 {
    let old_len = old_len as usize;
    let new_len = new_len as usize;
    let align = align as usize;
    if new_len == 0 {
        if old_ptr != 0 && old_len != 0 {
            if let Ok(layout) = std::alloc::Layout::from_size_align(old_len, align) {
                unsafe { std::alloc::dealloc(old_ptr as *mut u8, layout) };
            }
        }
        return 0;
    }
    let Ok(new_layout) = std::alloc::Layout::from_size_align(new_len, align) else {
        return 0;
    };
    let ptr = if old_ptr == 0 {
        unsafe { std::alloc::alloc(new_layout) }
    } else {
        let Ok(old_layout) = std::alloc::Layout::from_size_align(old_len, align) else {
            return 0;
        };
        unsafe { std::alloc::realloc(old_ptr as *mut u8, old_layout, new_len) }
    };
    ptr as i32
}

#[export_name = "cabi_realloc"]
pub extern "C" fn __jet_plugin_cabi_realloc(
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
) -> i32 {
    __jet_plugin_cabi_realloc_impl(old_ptr, old_len, align, new_len)
}

fn __jet_plugin_read_text(ptr: i32, len: i32) -> String {
    if ptr == 0 || len == 0 {
        return String::new();
    }
    unsafe {
        String::from_utf8_lossy(std::slice::from_raw_parts(ptr as *const u8, len as usize))
            .into_owned()
    }
}

fn __jet_plugin_return_text(value: String) -> i32 {
    let bytes = value.into_bytes();
    let len = bytes.len();
    let ptr = __jet_plugin_cabi_realloc_impl(0, 0, 1, len as i32) as *mut u8;
    if ptr.is_null() && len != 0 {
        return 0;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len) };
    std::mem::forget(bytes);
    let result = __jet_plugin_cabi_realloc_impl(0, 0, 4, 8) as *mut i32;
    if result.is_null() {
        if !ptr.is_null() && len != 0 {
            if let Ok(layout) = std::alloc::Layout::from_size_align(len, 1) {
                unsafe { std::alloc::dealloc(ptr, layout) };
            }
        }
        return 0;
    }
    unsafe {
        result.write(ptr as i32);
        result.add(1).write(len as i32);
    }
    result as i32
}

fn __jet_plugin_post_text(ret_ptr: i32) {
    if ret_ptr == 0 {
        return;
    }
    let (ptr, len) = unsafe {
        let pair = std::slice::from_raw_parts(ret_ptr as *const i32, 2);
        (pair[0], pair[1])
    };
    if ptr != 0 && len != 0 {
        if let Ok(layout) = std::alloc::Layout::from_size_align(len as usize, 1) {
            unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
        }
    }
    if let Ok(layout) = std::alloc::Layout::from_size_align(8, 4) {
        unsafe { std::alloc::dealloc(ret_ptr as *mut u8, layout) };
    }
}
"#,
        );
    }
    guest_rust.push_str(&wrapper_fns);

    PluginArtifacts {
        wit,
        guest_rust,
        world_name: WORLD_NAME.to_string(),
        export_name: sanitized,
        exported_fns: exported,
    }
}
