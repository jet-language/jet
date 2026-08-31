//! D-ADOPT-GUEST1=A: the sema-owned native guest boundary.
//!
//! Imports and exports are the same C-shaped record in opposite directions.
//! Artifact writers consume these rows after sema has checked their types;
//! they do not rediscover the guest surface from source or package metadata.

use super::*;
use crate::AST::{AccessConvention, Expr, ExternFn, Func, Item, Marker, ProgramBundle, Type};
use crate::Syntax;

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
        Some(crate::Policy::RuleSite::Module) => is_guest_import_marker(marker),
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
    is_guest_export(function).then(|| GuestFunction {
        direction: GuestDirection::Export,
        library: None,
        name: function.name.clone(),
        symbol: function.name.clone(),
        params: function
            .params
            .iter()
            .map(|param| (param.convention, param.ty.clone()))
            .collect(),
        scalar: scalar_shape(
            &function
                .params
                .iter()
                .map(|param| (param.convention, param.ty.clone()))
                .collect::<Vec<_>>(),
            function.return_type.as_ref(),
        ),
        return_type: function.return_type.clone(),
    })
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
