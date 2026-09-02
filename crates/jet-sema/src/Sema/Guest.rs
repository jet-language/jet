//! D-ADOPT-GUEST1=A: the sema-owned native guest boundary.
//!
//! Imports and exports are the same C-shaped record in opposite directions.
//! Artifact writers consume these rows after sema has checked their types;
//! they do not rediscover the guest surface from source or package metadata.

use super::*;
use crate::AST::{AccessConvention, Expr, ExternFn, Func, Item, Marker, ProgramBundle, Type};
use crate::Syntax;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestDirection {
    Import,
    Export,
}

/// The closed scalar projection used by native Library artifacts. The wider
/// C-safe type law below also admits `#Layout(c)` records and callbacks for
/// ordinary C imports; Library emission has one intentionally smaller ABI
/// surface so every generated language binding has one unambiguous value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestScalar {
    Int,
    Float,
    Bool,
    Text,
}

/// One callable at the C guest boundary. `name` is the Jet-facing name and
/// `symbol` is the native symbol; exports use the same value for both.
#[derive(Debug, Clone)]
pub struct GuestFunction {
    pub direction: GuestDirection,
    pub library: Option<String>,
    pub name: String,
    pub symbol: String,
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    /// `Some` only when every parameter and the return use one of the four
    /// scalar shapes emitted by the native Library projections.
    pub scalar: Option<GuestScalar>,
}

fn c_marker(marker: &Marker, name: &str) -> bool {
    if marker.name != name || marker.negated {
        return false;
    }
    match marker.args.as_slice() {
        [Expr::Ident(argument, _)] => argument == Syntax::C_MODULE_ROOT,
        _ => false,
    }
}

/// Whether a marker is the ratified `#Import(c)` spelling.
pub fn is_guest_import_marker(marker: &Marker) -> bool {
    c_marker(marker, Syntax::MARKER_IMPORT)
}

/// Whether a marker is the ratified `#Export(c)` spelling.
pub fn is_guest_export_marker(marker: &Marker) -> bool {
    c_marker(marker, Syntax::MARKER_EXPORT)
}

/// Whether a function carries the ratified `#Export(c)` spelling.
pub fn is_guest_export(function: &Func) -> bool {
    function
        .markers
        .iter()
        .any(is_guest_export_marker)
}

/// Whether a function carries the per-callable `#Import(c)` declaration.
pub fn is_guest_import(function: &Func) -> bool {
    function
        .markers
        .iter()
        .any(is_guest_import_marker)
}

/// Read the fixed C symbol from a per-callable `#Import(c)` declaration.
/// The parser stores this as a compile-time marker fact; a missing value is an
/// invalid declaration and is deliberately not treated as an import.
pub fn guest_import_symbol(function: &Func) -> Option<&str> {
    function
        .markers
        .iter()
        .find(|marker| is_guest_import_marker(marker))
        .and_then(|marker| match marker.ct.as_ref() {
            Some(crate::AST::CtValue::Str(symbol)) if !symbol.is_empty() => Some(symbol.as_str()),
            _ => None,
        })
}
/// Return the canonical generated Rust wrapper identity for one per-callable
/// `#Import(c)` declaration. The owning module is part of the identity because
/// local Jet names may repeat across modules.
pub fn guest_import_wrapper_name(owner: &str, name: &str) -> String {
    let owner = if owner.is_empty() { "root" } else { owner };
    crate::AST::mangle_path(&format!("jet_ffi_guest.{owner}.{name}"))
}

/// Return the one native spelling used for an exported C symbol. Keep this
/// beside the guest surface so sema, Library output, and bindings cannot drift.
pub fn guest_export_native_symbol(name: &str) -> String {
    let mut symbol = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if symbol.is_empty() || symbol.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        symbol.insert(0, '_');
    }
    symbol
}

/// Whether a per-callable import can cross the resident hidden C bridge.
///
/// The C-safe type law is wider than this value-shaped bridge. Per-callable
/// imports have no C-module direct-wrapper fallback, so sema rejects a valid
/// C signature that the canonical bridge cannot carry.
pub fn guest_import_bridge_compatible(function: &Func) -> bool {
    let Some(symbol) = guest_import_symbol(function) else {
        return false;
    };
    let foreign = ExternFn {
        abi: None,
        name: function.name.clone(),
        name_span: function.name_span,
        params: function.params.clone(),
        return_type: function.return_type.clone(),
        return_type_span: function.return_type_span,
        rust_path: symbol.to_string(),
        rust_path_span: function.name_span,
        effect_root: None,
        undo: function.undo.clone(),
        close: None,
        span: function.span,
    };
    foreign.hidden_c_bridge_compatible()
}


fn scalar_shape(params: &[(AccessConvention, Type)], return_type: Option<&Type>) -> Option<GuestScalar> {
    let scalar = |ty: &Type| match ty {
        Type::Int => Some(GuestScalar::Int),
        Type::Float => Some(GuestScalar::Float),
        Type::Bool => Some(GuestScalar::Bool),
        Type::String => Some(GuestScalar::Text),
        _ => None,
    };
    let mut shape = None;
    for (_, ty) in params {
        let next = scalar(ty)?;
        if shape.is_some_and(|existing| existing != next) {
            return None;
        }
        shape = Some(next);
    }
    let next = scalar(return_type?)?;
    if shape.is_some_and(|existing| existing != next) {
        return None;
    }
    Some(next)
}
fn export_signature(function: &Func) -> GuestFunction {
    let params = function
        .params
        .iter()
        .map(|param| (param.convention, param.ty.clone()))
        .collect::<Vec<_>>();
    let return_type = function.return_type.clone();
    GuestFunction {
        direction: GuestDirection::Export,
        library: None,
        name: function.name.clone(),
        symbol: function.name.clone(),
        scalar: scalar_shape(&params, return_type.as_ref()),
        params,
        return_type,
    }
}


/// Whether a marker is a valid guest-boundary spelling at a callable/module
/// site. The marker vocabulary uses this only to recognize the ratified
/// surface; export collection and C-signature checking remain here.
pub(crate) fn is_guest_marker_at(
    marker: &Marker,
    site: Option<crate::Policy::RuleSite>,
) -> bool {
    match site {
        Some(crate::Policy::RuleSite::Function) => {
            is_guest_import_marker(marker) || is_guest_export_marker(marker)
        }
        Some(crate::Policy::RuleSite::Module) => {
            // C modules use the marker as a bare introducer:
            // `#Import module c.<lib>`.  Per-function guest declarations use
            // the explicit `(c)` argument, so the two forms must stay
            // distinct while both remain outside the Prelude vocabulary.
            marker.name == Syntax::MARKER_IMPORT
                && !marker.negated
                && marker.args.is_empty()
        }
        _ => false,
    }
}

/// Build the sema row for one imported C callable.
pub fn guest_import_signature(library: &str, function: &ExternFn) -> GuestFunction {
    let params = function
        .params
        .iter()
        .map(|param| (param.convention, param.ty.clone()))
        .collect::<Vec<_>>();
    let return_type = function.return_type.clone();
    GuestFunction {
        direction: GuestDirection::Import,
        library: Some(library.to_string()),
        name: function.name.clone(),
        symbol: function.rust_path.clone(),
        scalar: scalar_shape(&params, return_type.as_ref()),
        params,
        return_type,
    }
}

/// Build the sema row for one per-callable `#Import(c)` declaration.
pub fn guest_import_function_signature(function: &Func) -> Option<GuestFunction> {
    let symbol = guest_import_symbol(function)?.to_string();
    let params = function
        .params
        .iter()
        .map(|param| (param.convention, param.ty.clone()))
        .collect::<Vec<_>>();
    let return_type = function.return_type.clone();
    Some(GuestFunction {
        direction: GuestDirection::Import,
        library: None,
        name: function.name.clone(),
        symbol,
        scalar: scalar_shape(&params, return_type.as_ref()),
        params,
        return_type,
    })
}

/// Build the sema row for one `#Export(c)` function.
pub fn guest_export_signature(function: &Func) -> Option<GuestFunction> {
    is_guest_export(function).then(|| export_signature(function))
}

/// Build the sema row for one sandbox export. Unlike the native C guest
/// surface, a sandbox exports every top-level `pub fn` in its entry module;
/// there is no separate export marker (D-PLUGIN-EXPORT1=A).
pub fn sandbox_export_signature(function: &Func) -> Option<GuestFunction> {
    (function.is_pub && !function.is_package_pub).then(|| export_signature(function))
}


/// Collect every guest callable from the post-sema bundle.
///
/// Exports are owned by the entry module. Imports are assembled C modules and
/// can live in any module. Keeping both directions in this one collector
/// ensures archive/shared artifact writers consume the same sema rows.
pub fn guest_surface(bundle: &ProgramBundle) -> Vec<GuestFunction> {
    let mut surface = Vec::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            match item {
                Item::Func(function) if module_idx == bundle.entry => {
                    if let Some(export) = guest_export_signature(function) {
                        surface.push(export);
                    }
                    if let Some(import) = guest_import_function_signature(function) {
                        surface.push(import);
                    }
                }
                Item::Func(function) => {
                    if let Some(import) = guest_import_function_signature(function) {
                        surface.push(import);
                    }
                }
                Item::CModule(c_module) => surface.extend(
                    c_module
                        .functions
                        .iter()
                        .map(|function| guest_import_signature(&c_module.lib, function)),
                ),
                _ => {}
            }
        }
    }
    surface
}

/// Collect the exact entry-module guest export list in source order.
pub fn guest_export_surface(bundle: &ProgramBundle) -> Vec<GuestFunction> {
    guest_surface(bundle)
        .into_iter()
        .filter(|function| function.direction == GuestDirection::Export)
        .collect()
}
/// Collect the exact top-level public entry-module functions exported by a
/// `target: sandbox` build, in source order. This intentionally stays separate
/// from `guest_export_surface`, whose explicit `#Export(c)` marker is the
/// native C/Library boundary.
pub fn sandbox_export_surface(bundle: &ProgramBundle) -> Vec<GuestFunction> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(function) => sandbox_export_signature(function),
            _ => None,
        })
        .collect()
}

/// Collect every C import row after CFFI assembly. Both user overlays and
/// generated bindgen modules use the same record and C type law.
pub fn guest_import_surface(bundle: &ProgramBundle) -> Vec<GuestFunction> {
    guest_surface(bundle)
        .into_iter()
        .filter(|function| function.direction == GuestDirection::Import)
        .collect()
}

/// Check all entry-module `#Export(c)` signatures with the same C-safe type
/// law used by `#Import`/`#Bindgen` declarations.
pub(crate) fn check_guest_export_surface(
    items: &[Item],
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    for function in items.iter().filter_map(|item| match item {
        Item::Func(function) if is_guest_export(function) => Some(function),
        _ => None,
    }) {
        if !check_c_signature(
            &function.params,
            function.return_type.as_ref(),
            function
                .return_type_span
                .unwrap_or(function.name_span),
            registry,
            diags,
        ) {
            ok = false;
        }
    }
    ok
}

/// Record one native symbol in the bundle-wide guest namespace.
fn record_guest_symbol(
    symbols: &mut BTreeMap<String, String>,
    symbol: String,
    owner: &str,
    kind: &str,
    name: &str,
    span: Option<crate::Diagnostics::Span>,
    diags: &mut Vec<Diagnostic>,
) {
    let description = format!("{kind} `{name}` in module `{owner}`");
    if let Some(previous) = symbols.insert(symbol.clone(), description.clone()) {
        diags.push(Diagnostic::error(
            "E1341",
            format!(
                "{previous} and {description} use the same C symbol `{symbol}`"
            ),
            "the native guest call surface must map every imported, exported, and generated C symbol unambiguously before codegen"
                .to_string(),
            "rename one declaration or its native symbol string".to_string(),
            span,
        ));
    }
}

/// Reject duplicate native symbols across every bundle-defined or referenced
/// guest row before codegen. This includes per-callable imports, C-module
/// imports, exported library symbols, and the generated text release symbol.
pub(crate) fn check_guest_symbol_collisions(
    bundle: &ProgramBundle,
    diags: &mut Vec<Diagnostic>,
) {
    let mut symbols = BTreeMap::<String, String>::new();
    let mut has_text_export = false;
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            match item {
                Item::Func(function) => {
                    if let Some(import) = guest_import_function_signature(function) {
                        record_guest_symbol(
                            &mut symbols,
                            import.symbol,
                            &module.alias,
                            "guest imports",
                            &function.name,
                            Some(function.name_span),
                            diags,
                        );
                    }
                    if module_idx == bundle.entry {
                        if let Some(export) = guest_export_signature(function) {
                            has_text_export |= export.scalar == Some(GuestScalar::Text);
                            record_guest_symbol(
                                &mut symbols,
                                guest_export_native_symbol(&export.name),
                                &module.alias,
                                "Library exports",
                                &function.name,
                                Some(function.name_span),
                                diags,
                            );
                        }
                    }
                }
                Item::CModule(c_module) => {
                    for function in &c_module.functions {
                        record_guest_symbol(
                            &mut symbols,
                            function.rust_path.clone(),
                            &module.alias,
                            "C module imports",
                            &function.name,
                            Some(function.name_span),
                            diags,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if has_text_export {
        record_guest_symbol(
            &mut symbols,
            "jet_text_free".to_string(),
            "<generated>",
            "generated Library symbols",
            "jet_text_free",
            None,
            diags,
        );
    }
}


/// Check all per-callable `#Import(c)` signatures with the same C-safe law as
/// C modules and exports. A declaration is registered only after this pass.
pub(crate) fn check_guest_import_surface(
    items: &[Item],
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    for function in items.iter().filter_map(|item| match item {
        Item::Func(function) if guest_import_function_signature(function).is_some() => Some(function),
        _ => None,
    }) {
        let c_safe = check_c_signature(
            &function.params,
            function.return_type.as_ref(),
            function
                .return_type_span
                .unwrap_or(function.name_span),
            registry,
            diags,
        );
        if !c_safe {
            ok = false;
        } else if !guest_import_bridge_compatible(function) {
            diags.push(Diagnostic::error(
                "E1341",
                format!(
                    "guest import `{}` cannot use the resident hidden C bridge",
                    function.name
                ),
                "per-callable imports have no direct C-module wrapper fallback".to_string(),
                "use a read-only scalar or String bridge signature".to_string(),
                Some(function.name_span),
            ));
            ok = false;
        }
    }
    ok
}
