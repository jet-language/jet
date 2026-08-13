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
//!     all-`Int` or all-`Float` (checked before this runs — see
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

use crate::AST::{Item, ProgramBundle, Type};
use jet_foundation::Names::mangle;

/// One exported plugin function's homogeneous scalar shape.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PluginScalar {
    Int,
    Float,
}

impl PluginScalar {
    fn wit_ty(self) -> &'static str {
        match self {
            PluginScalar::Int => "s64",
            PluginScalar::Float => "f64",
        }
    }
    fn rust_ty(self) -> &'static str {
        match self {
            PluginScalar::Int => "i64",
            PluginScalar::Float => "f64",
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
/// parameter and the return type are the same of `Int`/`Float`, else `None`
/// (not exportable in v1 — see module doc).
pub fn plugin_export_shape(f: &crate::AST::Func) -> Option<PluginScalar> {
    let mut shape: Option<PluginScalar> = None;
    let note = |t: &Type, shape: &mut Option<PluginScalar>| -> bool {
        let this = match t {
            Type::Int => PluginScalar::Int,
            Type::Float => PluginScalar::Float,
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
        let rust_params: Vec<String> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, _)| format!("p{i}: {}", scalar.rust_ty()))
            .collect();
        let call_args: Vec<String> = (0..f.params.len()).map(|i| format!("p{i}")).collect();
        let wrapper_name = mangle(&format!("plugin_export_{}", f.name));
        wrapper_fns.push_str(&format!(
            "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}({rust_params}) -> {ret} {{ {callee}({call_args}) }}\n",
            wrapper_name = wrapper_name,
            rust_params = rust_params.join(", "),
            ret = scalar.rust_ty(),
            callee = mangle(&f.name),
            call_args = call_args.join(", "),
        ));
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
    guest_rust.push_str(&wrapper_fns);

    PluginArtifacts {
        wit,
        guest_rust,
        world_name: WORLD_NAME.to_string(),
        export_name: sanitized,
        exported_fns: exported,
    }
}
