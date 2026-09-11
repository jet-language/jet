//! Native `Library` export projection (D-LIB-EXPORT1=C).
//!
//! The selected checked MIR artifact plan supplies the identity and export
//! rows. This module only renders native wrappers and deterministic foreign
//! publication text.

use std::collections::BTreeSet;

use jet_foundation::AST::{binder_descriptor, ForeignLanguage};
use jet_foundation::MIR::{
    ComponentSignatureDescriptor, MirAccess, MirArtifactId, MirArtifactKind, MirArtifactTarget,
    MirFailureCarrier, MirOwnershipMode, MirProgram, MirSerdeCodec, MirType,
    MirTypeDefKind, MirTypeKind, MirVariantPayload, MirViewSource,
};
#[cfg(test)]
use jet_foundation::MIR::MirOwnership;
use jet_foundation::Names::{mangle, mangle_generated, mangle_path};
use jet_pkg_model::ForeignBridge::{
    ForeignArtifactCoverage, ForeignBoundaryContract, ForeignBoundaryIdentity,
};
use super::Embedding::{component_descriptor, ComponentSignature, ExportOwnership, ExportScalar};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryExport {
    pub name: String,
    pub symbol: String,
    pub callee: String,
    pub scalar: ExportScalar,
    pub params: usize,
    pub conventions: Vec<MirAccess>,
    pub ownership: ExportOwnership,
}

struct RichExport {
    name: String,
    symbol: String,
    callee: String,
    component: ComponentSignature,
    descriptor: ComponentSignatureDescriptor,
    params: Vec<MirAccess>,
    ownership: ExportOwnership,
    failure: MirFailureCarrier,
}

/// A checked rich export retained for the typed owner/view bridge even when
/// its view-bearing signature has no Component Model value descriptor.
#[derive(Debug, Clone)]
struct ResourceExport {
    name: String,
    symbol: String,
    callee: String,
    component: ComponentSignature,
    params: Vec<MirAccess>,
    ownership: ExportOwnership,
    failure: MirFailureCarrier,
}

#[derive(Debug, Clone)]
struct ResourcePlan {
    open: usize,
    bytes: usize,
    at: usize,
    replace: usize,
    close: usize,
}

fn result_ok_type(export: &ResourceExport) -> Option<&MirType> {
    // `MirFunction.return_type` is already the effective success type; the
    // separate failure carrier retains the declared Result error.
    Some(&export.component.result)
}

fn has_result_failure(export: &ResourceExport) -> bool {
    let success = result_ok_type(export);
    matches!(
        &export.failure,
        MirFailureCarrier::Result { success: failure_success, .. }
            if success.is_some_and(|success| failure_success.same_checked_type(success))
    )
}

fn result_error_type(export: &ResourceExport) -> Option<&MirType> {
    match &export.failure {
        MirFailureCarrier::Result { error, .. } => Some(error),
        _ => None,
    }
}

fn nominal_has_encode_impl(program: &MirProgram, ty: &MirType) -> bool {
    let Some(id) = ty.nominal_id() else {
        return false;
    };
    program.impls.iter().any(|implementation| {
        implementation.serde == Some(MirSerdeCodec::Encode)
            && implementation.self_type.nominal_id() == Some(id)
    }) || program
        .types
        .iter()
        .find(|definition| definition.id == id)
        .is_some_and(|definition| {
            definition.derives.iter().any(|trait_id| {
                program
                    .traits
                    .iter()
                    .find(|trait_definition| trait_definition.id == *trait_id)
                    .is_some_and(|trait_definition| {
                        trait_definition
                            .name
                            .rsplit("::")
                            .next()
                            .is_some_and(|name| matches!(name, "Encode" | "Codable"))
                    })
            })
        })
}

/// The resource ABI carries guest failures as the canonical Component/DataTree
/// payload. This gate mirrors the encoders that the generated Rust artifact
/// can actually call; it is deliberately independent from lifecycle statuses.
fn resource_error_is_projectable(
    program: &MirProgram,
    ty: &MirType,
    seen: &mut BTreeSet<jet_foundation::MIR::MirTypeId>,
) -> bool {
    match ty.kind() {
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::Float32 => true,
        MirTypeKind::IntN { bits, .. } => *bits <= 32,
        MirTypeKind::List(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::Option(inner)
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. } => resource_error_is_projectable(program, inner, seen),
        MirTypeKind::Map { key, value } => {
            matches!(key.kind(), MirTypeKind::String)
                && resource_error_is_projectable(program, value, seen)
        }
        MirTypeKind::Apply { .. } => {
            if !nominal_has_encode_impl(program, ty) {
                return false;
            }
            let Some(id) = ty.nominal_id() else {
                return false;
            };
            if !seen.insert(id) {
                return false;
            }
            let supported = program
                .types
                .iter()
                .find(|definition| definition.id == id)
                .is_some_and(|definition| match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. } => fields
                        .iter()
                        .filter(|field| !field.skip && !field.computed)
                        .all(|field| resource_error_is_projectable(program, &field.ty, seen)),
                    MirTypeDefKind::Enum { variants, .. } => variants.iter().all(|variant| {
                        match &variant.payload {
                            MirVariantPayload::Unit => true,
                            MirVariantPayload::Single(ty) => {
                                resource_error_is_projectable(program, ty, seen)
                            }
                            MirVariantPayload::Named(fields) => fields
                                .iter()
                                .filter(|field| !field.skip && !field.computed)
                                .all(|field| resource_error_is_projectable(program, &field.ty, seen)),
                        }
                    }),
                    MirTypeDefKind::Distinct { base, .. }
                    | MirTypeDefKind::Alias { target: base } => {
                        resource_error_is_projectable(program, base, seen)
                    }
                    MirTypeDefKind::UnitFamily { .. } => false,
                });
            seen.remove(&id);
            supported
        }
        MirTypeKind::Result { .. }
        | MirTypeKind::Shared(_)
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Tuple(_)
        | MirTypeKind::Union(_)
        | MirTypeKind::Quantity { .. }
        | MirTypeKind::Measure(_) => false,
    }
}

fn resource_errors_supported(
    program: &MirProgram,
    exports: &[ResourceExport],
    plan: &ResourcePlan,
) -> bool {
    [plan.open, plan.bytes, plan.at, plan.replace, plan.close]
        .into_iter()
        .all(|index| {
            result_error_type(&exports[index]).is_some_and(|error| {
                resource_error_is_projectable(program, error, &mut BTreeSet::new())
            })
        })
}

fn is_integer_list(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::List(inner) if matches!(inner.kind(), MirTypeKind::Int))
}

fn is_integer(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::Int)
}

fn is_nominal(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::Apply { .. })
}


fn is_integer_view(ty: &MirType) -> bool {
    matches!(
        ty.kind(),
        MirTypeKind::Apply { name, args }
            if name.name == "View" && args.len() == 1 && is_integer(&args[0])
    )
}

fn has_checked_read_view_result(export: &ResourceExport) -> bool {
    let Some(view) = result_ok_type(export) else {
        return false;
    };
    is_integer_view(view)
        && export
            .ownership
            .return_views
            .as_ref()
            .is_some_and(|provenance| {
                !provenance.is_empty()
                    && provenance.values().all(|view| {
                        !view.mutable
                            && view.sources.iter().any(|source| {
                                matches!(source.source, MirViewSource::Parameter(0))
                            })
                    })
            })
}


fn has_ownership_mode(export: &ResourceExport, index: usize, mode: MirOwnershipMode) -> bool {
    export
        .ownership
        .parameters
        .get(index)
        .is_some_and(|ownership| ownership.mode == mode)
}

fn unique_resource_operation(
    exports: &[ResourceExport],
    predicate: impl Fn(&ResourceExport) -> bool,
) -> Option<usize> {
    let mut found = None;
    for (index, _) in exports
        .iter()
        .enumerate()
        .filter(|(_, export)| predicate(*export))
    {
        if found.replace(index).is_some() {
            return None;
        }
    }
    found
}

/// returned-view facts.
///
/// The protocol is admitted only when each role is unique and its checked
/// receiver, view, move, and outcome shapes agree. Export names, symbols, and
/// callees remain data carried by the selected rows.
fn classify_resource_exports(exports: &[ResourceExport]) -> Option<ResourcePlan> {
    let open = unique_resource_operation(exports, |export| {
        export.component.params.len() == 1
            && is_integer_list(&export.component.params[0])
            && result_ok_type(export).is_some_and(|ty| is_nominal(ty))
            && export.ownership.parameters.len() == 1
            && has_ownership_mode(export, 0, MirOwnershipMode::ReadBorrow)
            && has_result_failure(export)
    })?;
    let owner_type = result_ok_type(&exports[open])?;

    let bytes = unique_resource_operation(exports, |export| {
        export.component.params.len() == 1
            && has_checked_read_view_result(export)
            && export.component.params[0].same_checked_type(owner_type)
            && has_result_failure(export)
            && has_ownership_mode(export, 0, MirOwnershipMode::ReadBorrow)
    })?;
    let view_type = result_ok_type(&exports[bytes])?;
    if view_type.same_checked_type(owner_type) {
        return None;
    }

    let at = unique_resource_operation(exports, |export| {
        result_ok_type(export).is_some_and(is_integer)
            && has_result_failure(export)
            && export.component.params.len() == 3
            && export.component.params[0].same_checked_type(owner_type)
            && export.component.params[1].same_checked_type(view_type)
            && is_integer(&export.component.params[2])
            && has_ownership_mode(export, 0, MirOwnershipMode::ReadBorrow)
            && has_ownership_mode(export, 1, MirOwnershipMode::ReadBorrow)
    })?;
    let replace = unique_resource_operation(exports, |export| {
        let Some(result) = result_ok_type(export) else {
            return false;
        };
        export.component.params.len() == 2
            && has_result_failure(export)
            && export.component.params[0].same_checked_type(owner_type)
            && is_integer_list(&export.component.params[1])
            && result.same_checked_type(owner_type)
            && has_ownership_mode(export, 0, MirOwnershipMode::Move)
            && has_ownership_mode(export, 1, MirOwnershipMode::ReadBorrow)
    })?;
    let close = unique_resource_operation(exports, |export| {
        result_ok_type(export).is_some_and(MirType::is_unit)
            && has_result_failure(export)
            && export.component.params.len() == 1
            && export.component.params[0].same_checked_type(owner_type)
            && has_ownership_mode(export, 0, MirOwnershipMode::Move)
    })?;

    Some(ResourcePlan {
        open,
        bytes,
        at,
        replace,
        close,
    })
}

/// One host projection backed by the canonical foreign-boundary contract.
///
/// `language` is the generated facade spelling. Zig intentionally records a
/// C contract because the adapter crosses the stable C shim; it does not
/// pretend that Zig has a stable native ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryProjection {
    pub language: String,
    pub direction: String,
    pub artifact_identity: String,
    pub contract: ForeignBoundaryContract,
    pub ownership: String,
    pub integer_ranges: String,
    pub encoding: String,
    pub enums: String,
    pub errors: String,
    pub runtime_roots: String,
    pub resource_lifetime: String,
    pub unsupported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryArtifacts {
    /// Complete generated Rust, including the C ABI wrappers.
    pub rust: String,
    /// The generated C header. It is always emitted for a native Library.
    pub header: String,
    /// Named foreign binding source, in stable language order.
    pub bindings: Vec<(String, String)>,
    pub exports: Vec<LibraryExport>,
    /// One canonical contract row per requested host projection.
    pub projections: Vec<LibraryProjection>,
}
fn collect_exports(
    program: &MirProgram,
    artifact_id: MirArtifactId,
) -> (Vec<LibraryExport>, Vec<RichExport>, Vec<ResourceExport>) {
    let mut scalar = Vec::new();
    let mut rich = Vec::new();
    let mut resources = Vec::new();
    for export in super::Embedding::export_surface(program, artifact_id) {
        if let Some(export_scalar) = export.scalar {
            scalar.push(LibraryExport {
                name: export.name,
                symbol: export.symbol,
                callee: export.callee,
                scalar: export_scalar,
                params: export.params.len(),
                conventions: export.params,
                ownership: export.ownership,
            });
            continue;
        }
        let Some(component) = export.component else {
            continue;
        };
        resources.push(ResourceExport {
            name: export.name.clone(),
            symbol: export.symbol.clone(),
            callee: export.callee.clone(),
            component: component.clone(),
            params: export.params.clone(),
            ownership: export.ownership.clone(),
            failure: export.failure.clone(),
        });
        let Some(descriptor) = component_descriptor(program, &component) else {
            continue;
        };
        rich.push(RichExport {
            name: export.name,
            symbol: export.symbol,
            callee: export.callee,
            component,
            descriptor,
            params: export.params,
            ownership: export.ownership,
            failure: export.failure,
        });
    }
    (scalar, rich, resources)
}

/// Render the native wrappers and the requested foreign projections.
pub fn emit_library(
    program: &MirProgram,
    artifact_id: MirArtifactId,
    whole_program_rust: &str,
    requested_bindings: &[String],
) -> LibraryArtifacts {
    let artifact = super::Embedding::selected_artifact(program, artifact_id);
    if artifact.kind != MirArtifactKind::NativeLibrary
        || artifact.target != MirArtifactTarget::RustAot
    {
        panic!(
            "MIR Library emission requires a RustAot NativeLibrary artifact, got {:?}/{:?}",
            artifact.target, artifact.kind
        );
    }
    let name = artifact.name.clone();
    let (exports, rich_exports, resource_exports) = collect_exports(program, artifact_id);
    let classified_resource_plan = classify_resource_exports(&resource_exports);
    let resource_error_unsupported = classified_resource_plan
        .as_ref()
        .is_some_and(|plan| !resource_errors_supported(program, &resource_exports, plan));
    let resource_plan = classified_resource_plan.filter(|plan| {
        resource_errors_supported(program, &resource_exports, plan)
    });
    let has_text_export = exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text);
    let mut wrappers = String::new();
    for export in &exports {
        let params = (0..export.params)
            .map(|index| {
                if export.scalar == ExportScalar::Text {
                    format!("p{index}: JetText")
                } else {
                    format!("p{index}: {}", export.scalar.rust_ty())
                }
            })
            .collect::<Vec<_>>();
        let locals = export
            .conventions
            .iter()
            .enumerate()
            .map(|(index, convention)| {
                let mutable = matches!(convention, MirAccess::Write)
                    .then_some("mut ")
                    .unwrap_or_default();
                if export.scalar == ExportScalar::Text {
                    format!(
                        "let {mutable}p{index} = {}(p{index});",
                        mangle_generated("library_read_text")
                    )
                } else if matches!(convention, MirAccess::Write) {
                    format!("let mut p{index} = p{index};")
                } else {
                    String::new()
                }
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let args = (0..export.params)
            .map(|index| match (export.scalar, export.conventions[index]) {
                (ExportScalar::Text, MirAccess::Read) => format!("&p{index}"),
                (_, MirAccess::Read | MirAccess::Move) => format!("p{index}"),
                (_, MirAccess::Write) => format!("&mut p{index}"),
            })
            .collect::<Vec<_>>();
        let wrapper = mangle(&format!("library_export_{}", export.name));
        let symbol = export.symbol.as_str();
        let callee = export.callee.as_str();
        let return_type = if export.scalar == ExportScalar::Text {
            "JetText"
        } else {
            export.scalar.rust_ty()
        };
        let call = format!(
            "match {callee}({}) {{ Ok(value) => value, Err(error) => jet_entry_error_exit_jet(error) }}",
            args.join(", ")
        );
        let body = if export.scalar == ExportScalar::Text {
            format!(
                "{} {}({call})",
                locals.join(" "),
                mangle_generated("library_return_text")
            )
        } else if !locals.is_empty() {
            format!("{} {call}", locals.join(" "))
        } else {
            call
        };
        let body = format!("jet_ffi_callback_boundary(|| {{ {body} }})");
        wrappers.push_str(&format!(
            "#[export_name = \"{symbol}\"]\npub extern \"C\" fn {wrapper}({params}) -> {ret} {{ {body} }}\n",
            params = params.join(", "),
            ret = return_type,
            body = body,
        ));
        wrappers.push_str(&format!("// jet:library-symbol={symbol}\n"));
    }
    for export in &rich_exports {
        wrappers.push_str(&render_component_wrapper(program, export));
    }


    let mut rust = whole_program_rust.to_string();
    rust.push_str("\n// D-LIB-EXPORT1=C: generated native Library wrappers.\n");
    if !rich_exports.is_empty() || resource_plan.is_some() {
        rust.push_str(&component_helpers());
    }
    if has_text_export {
        rust.push_str("// JET_VETTED_UNSAFE_BEGIN: library_text_abi\n");
        rust.push_str(&library_text_helpers());
        rust.push_str("\n// JET_VETTED_UNSAFE_END: library_text_abi\n");
    }
    if let Some(resource_plan) = resource_plan.as_ref() {
        rust.push_str("// JET_VETTED_UNSAFE_BEGIN: library_resource_bridge\n");
        rust.push_str(&resource_helpers(program, &name, &resource_exports, resource_plan));
        rust.push_str("\n// JET_VETTED_UNSAFE_END: library_resource_bridge\n");
    }
    rust.push_str(&wrappers);
    let projections = build_projections(
        &name,
        artifact,
        resource_plan.as_ref(),
        resource_error_unsupported,
    );
    let header =
        render_c_header_with_rich(&name, &exports, &rich_exports, resource_plan.as_ref(), &projections);
    let mut bindings = Vec::new();
    let mut emitted = std::collections::BTreeSet::new();
    for language in requested_bindings {
        let Some(language) = canonical_binding(language) else {
            continue;
        };
        if !emitted.insert(language.to_string()) {
            continue;
        }
        let source = match language {
            "c" => header.clone(),
            "cpp" => render_cpp(&name, &exports, &rich_exports, resource_plan.as_ref()),
            "rust" => render_rust(&name, &exports, &rich_exports, resource_plan.as_ref()),
            "python" => {
                render_python_with_rich(&name, &exports, &rich_exports, resource_plan.as_ref())
            }
            "zig" => render_zig(&name, &exports, &rich_exports, resource_plan.as_ref()),
            "go" => render_go(&name, &exports, &rich_exports, resource_plan.as_ref()),
            "javascript" => {
                render_javascript(&name, &exports, &rich_exports, resource_plan.as_ref())
            }
            "swift" => render_swift(&exports),
            _ => continue,
        };
        bindings.push((language.to_string(), source));
    }

    LibraryArtifacts {
        rust,
        header,
        bindings,
        exports,
        projections,
    }
}

fn rust_component_type(program: &MirProgram, ty: &MirType) -> String {
    match ty.kind() {
        MirTypeKind::Int => "i64".into(),
        MirTypeKind::Float => "f64".into(),
        MirTypeKind::Bool => "bool".into(),
        MirTypeKind::String => "String".into(),
        MirTypeKind::Char => "char".into(),
        MirTypeKind::List(inner) => {
            format!("Vec<{}>", rust_component_type(program, inner))
        }
        MirTypeKind::Option(inner) => format!(
            "JetOutcome<{}, JetAbsent>",
            rust_component_type(program, inner)
        ),
        MirTypeKind::Result { ok, err } => format!(
            "JetOutcome<{}, {}>",
            rust_component_type(program, ok),
            rust_component_type(program, err)
        ),
        MirTypeKind::FixedList { elem, len } => {
            format!("[{}; {}]", rust_component_type(program, elem), len.expression())
        }
        MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
            rust_component_type(program, base)
        }
        MirTypeKind::IntN { signed, bits } => {
            format!("{}{}", if *signed { 'i' } else { 'u' }, bits)
        }
        MirTypeKind::Float32 => "f32".into(),
        MirTypeKind::Apply { name, args } if name.name == "View" && args.len() == 1 => {
            format!("&[{}]", rust_component_type(program, &args[0]))
        }
        MirTypeKind::Apply { name, args } if name.name == "ViewMut" && args.len() == 1 => {
            format!("&mut [{}]", rust_component_type(program, &args[0]))
        }
        MirTypeKind::Apply { name, args } => {
            let head = mangle_path(&name.name);
            if args.is_empty() {
                head
            } else {
                format!(
                    "{head}<{}>",
                    args.iter()
                        .map(|arg| rust_component_type(program, arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        _ => ty.display_name(),
    }
}

fn component_helpers() -> String {
    let read = mangle_generated("library_component_read");
    let return_value = mangle_generated("library_component_return");
    let error = mangle_generated("library_component_error");
    let error_value = mangle_generated("library_component_error_value");
    let free = mangle_generated("library_component_free");
    format!(
        r#"
#[repr(C)]
pub struct JetComponent {{
    pub ptr: *const u8,
    pub len: usize,
}}

fn {read}(value: JetComponent) -> Result<String, String> {{
    if value.len == 0 {{
        return Ok(String::new());
    }}
    if value.ptr.is_null()
        || value.len > isize::MAX as usize
        || (value.ptr as usize).checked_add(value.len).is_none()
    {{
        return Err("invalid component pointer-length pair".to_string());
    }}
    let bytes = unsafe {{ std::slice::from_raw_parts(value.ptr, value.len) }};
    String::from_utf8(bytes.to_vec()).map_err(|_| "component input is not UTF-8".to_string())
}}

fn {return_value}(value: jet_std::DataTree) -> JetComponent {{
    let text = jet_std::render_json(&value, false, 0);
    let bytes = text.into_bytes().into_boxed_slice();
    let len = bytes.len();
    let ptr = Box::into_raw(bytes) as *const u8;
    JetComponent {{ ptr, len }}
}}

fn {error_value}<T: __jet_Encode>(value: &T) -> JetComponent {{
    {return_value}(value.jet_encode())
}}

fn {error}(message: &str) -> JetComponent {{
    {return_value}(jet_std::DataTree::Object(vec![
        ("ok".to_string(), jet_std::DataTree::Bool(false)),
        ("error".to_string(), jet_std::DataTree::Text(message.to_string())),
    ]))
}}

#[export_name = "jet_component_free"]
pub extern "C" fn {free}(value: JetComponent) {{
    if value.ptr.is_null() || value.len == 0 {{
        return;
    }}
    unsafe {{
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            value.ptr as *mut u8,
            value.len,
        )));
    }}
}}
"#,
        read = read,
        return_value = return_value,
        error = error,
        error_value = error_value,
        free = free,
    )
}



fn render_component_wrapper(program: &MirProgram, export: &RichExport) -> String {
    let read = mangle_generated("library_component_read");
    let return_value = mangle_generated("library_component_return");
    let error = mangle_generated("library_component_error");
    let types = export
        .component
        .params
        .iter()
        .map(|ty| rust_component_type(program, ty))
        .collect::<Vec<_>>();
    let mut locals = Vec::new();
    let mut args = Vec::new();
    for (index, (ty, convention)) in export
        .component
        .params
        .iter()
        .zip(&export.params)
        .enumerate()
    {
        let rust_ty = &types[index];
        let mutable = matches!(convention, MirAccess::Write)
            .then_some("mut ")
            .unwrap_or_default();
        locals.push(format!(
            "let {mutable}p{index}: {rust_ty} = <{rust_ty} as __jet_Decode>::jet_decode(__jet_values.get({index}).ok_or_else(|| \"missing component parameter\".to_string())?).map_err(|_| \"component parameter decode failed\".to_string())?;"
        ));
        args.push(match convention {
            MirAccess::Read
                if matches!(
                    ty.kind(),
                    MirTypeKind::Int | MirTypeKind::Float | MirTypeKind::Bool
                ) =>
            {
                format!("p{index}")
            }
            MirAccess::Read => format!("&p{index}"),
            MirAccess::Write => format!("&mut p{index}"),
            MirAccess::Move => format!("p{index}"),
        });
    }
    let parse_values = if export.component.params.is_empty() {
        "let __jet_values = Vec::new();".to_string()
    } else {
        "let __jet_values = match __jet_tree { jet_std::DataTree::Array(values) => values, _ => return Err(\"component input must be a JSON array\".to_string()) };".to_string()
    };
    let call = if matches!(&export.failure, MirFailureCarrier::Result { .. }) {
        format!(
            "match {}({}) {{ Ok(value) => value, Err(error) => return Ok({return_value}(jet_std::DataTree::Object(vec![(\"ok\".to_string(), jet_std::DataTree::Bool(false)), (\"error\".to_string(), error.jet_encode())]))), }}",
            export.callee,
            args.join(", "),
            return_value = return_value,
        )
    } else {
        format!("{}({})", export.callee, args.join(", "))
    };
    let symbol = format!("{}_component", export.symbol);
    let wrapper = mangle(&format!("library_component_{}", export.name));
    format!(
        r#"
#[export_name = "{symbol}"]
pub extern "C" fn {wrapper}(input: JetComponent) -> JetComponent {{
    let run = || -> Result<JetComponent, String> {{
        let text = {read}(input)?;
        let __jet_tree = jet_std::parse_json_typed_datatree(&text)
            .map_err(|_| "component input is not valid JSON".to_string())?;
        {parse_values}
        {locals}
        let __jet_value = {call};
        let __jet_tree = __jet_value.jet_encode();
        Ok({return_value}(jet_std::DataTree::Object(vec![
            ("ok".to_string(), jet_std::DataTree::Bool(true)),
            ("value".to_string(), __jet_tree),
        ])))
    }};
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {{
        Ok(Ok(value)) => value,
        Ok(Err(message)) => {error}(&message),
        Err(_) => {error}("Jet export panicked at the guest boundary"),
    }}
}}
"#,
        symbol = symbol,
        wrapper = wrapper,
        read = read,
        parse_values = parse_values,
        locals = locals.join("\n        "),
        call = call,
        return_value = return_value,
        error = error,
    )
}


const PROJECTION_LANGUAGES: &[&str] =
    &["c", "cpp", "rust", "zig", "go", "python", "javascript"];

fn canonical_binding(language: &str) -> Option<&'static str> {
    match language.trim().to_ascii_lowercase().as_str() {
        "c" => Some("c"),
        "cpp" | "c++" => Some("cpp"),
        "rust" | "rs" => Some("rust"),
        "zig" => Some("zig"),
        "go" => Some("go"),
        "python" | "py" => Some("python"),
        "javascript" | "js" | "node" | "nodejs" => Some("javascript"),
        "swift" => Some("swift"),
        _ => None,
    }
}

fn c_identifier(name: &str) -> String {
    let mut value = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() || value.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        value.insert(0, '_');
    }
    value
}
fn go_export_name(name: &str) -> String {
    c_identifier(name)
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect()
}

fn projection_contract_language(language: &str) -> ForeignLanguage {
    match language {
        "cpp" => ForeignLanguage::Cpp,
        "rust" => ForeignLanguage::Rust,
        "python" => ForeignLanguage::Py,
        "javascript" => ForeignLanguage::JS,
        "go" => ForeignLanguage::Go,
        // Zig deliberately rides the stable C shim. There is no invented Zig
        // native ABI or second boundary contract.
        "zig" | "c" => ForeignLanguage::C,
        _ => ForeignLanguage::C,
    }
}

fn build_projections(
    name: &str,
    artifact: &jet_foundation::MIR::MirArtifactPlan,
    resource_plan: Option<&ResourcePlan>,
    resource_error_unsupported: bool,
) -> Vec<LibraryProjection> {
    PROJECTION_LANGUAGES
        .iter()
        .filter_map(|language| {
            let language = *language;
            let contract_language = projection_contract_language(language);
            let descriptor = binder_descriptor(contract_language)?.clone();
            let identity = ForeignBoundaryIdentity::new(
                format!("jet-library:{name}"),
                format!("mir-artifact:{}", artifact.id.0),
                "jet-codegen-library-ffi-guest1",
                artifact.artifact_identity.clone(),
                "jet-rust-aot",
                artifact.target.as_str(),
            );
            let coverage = ForeignArtifactCoverage::new(
                artifact.artifact_identity.clone(),
                artifact.target.as_str(),
                "jet-codegen-library-ffi-guest1",
            )
            .with_transitive_dependencies(Vec::<String>::new())
            .with_reachable_callbacks(Vec::<String>::new())
            .with_compiler_flags(Vec::<String>::new());
            let contract = ForeignBoundaryContract::new(
                descriptor,
                format!("{name}:{language}"),
                identity,
            )
            .with_artifact_coverage(coverage)
            .with_assumptions([
                "generated facade uses the selected stable C shim".to_string(),
                "native host implementation remains an independent checked artifact".to_string(),
            ]);
            let mut unsupported = match language {
                "c" => vec![
                    "arbitrary host pointers and unchecked packed fields".to_string(),
                    "variadic or macro-only APIs without a concrete shim".to_string(),
                ],
                "cpp" => vec![
                    "universal C++ ABI and unselected template/class layouts".to_string(),
                    "implicit exception or string_view lifetime conversion".to_string(),
                ],
                "rust" => vec![
                    "direct Rust-native ABI and unproven repr/layout".to_string(),
                ],
                "zig" => vec![
                    "Zig-native ABI; the generated adapter is C-shim-only".to_string(),
                    "implicit allocator or sentinel-pointer conversion".to_string(),
                ],
                "go" => vec![
                    "Go pointer retention across calls; cgo handles are required".to_string(),
                ],
                "python" => vec![
                    "foreign interpreter ownership outside the active ctypes process".to_string(),
                ],
                "javascript" => vec![
                    "synchronous calls from an asynchronous Node facade".to_string(),
                    "browser and embedded runtimes without a Node-API host".to_string(),
                ],
                _ => Vec::new(),
            };
            if resource_error_unsupported {
                unsupported.push(
                    "typed resource protocol omitted: checked outcome error has no lossless JetComponent/DataTree encoding"
                        .to_string(),
                );
            } else if resource_plan.is_none() {
                unsupported.push(
                    "typed resource protocol omitted: no unique checked owner/view/consume operation set"
                        .to_string(),
                );
            }
            Some(LibraryProjection {
                language: language.to_string(),
                direction: "jet-export-to-host + foreign-import-to-jet via stable C shim".into(),
                artifact_identity: artifact.artifact_identity.clone(),
                contract,
                ownership: "owned resource handles; borrowed views carry slot+generation".into(),
                integer_ranges: "Jet Int is exact; fixed-width I64 adapters check bounds before entry and return overflow".into(),
                encoding: "UTF-8 pointer+length; no NUL truncation; invalid pairs are rejected".into(),
                enums: "tagged discriminants use canonical Encode in supported Outcome error carriers; enum value parameters/results without a checked descriptor are explicit unsupported".into(),
                errors: "lifecycle status values remain distinct from lossless guest Outcome payloads in JetComponent/DataTree; panic, exception, and cancellation do not cross the C ABI".into(),
                runtime_roots: match language {
                    "go" => "cgo call-scoped roots + runtime.KeepAlive; retained host pointers require an explicit cgo.Handle".into(),
                    "python" => "active interpreter/ctypes roots; in-process embed is explicit, and context-manager cleanup is available".into(),
                    "javascript" => "Node addon root; signed I64 values use checked BigInt; synchronous calls remain synchronous; Promise scheduling requires an explicit host adapter".into(),
                    _ => "host-owned handle table; no borrowed pointer outlives its owner".into(),
                },
                resource_lifetime: if resource_plan.is_some() {
                    "replace/close increments generation; stale view returns ExpiredView before dereference; close is exactly once".into()
                } else if resource_error_unsupported {
                    "checked owner/view protocol found but omitted because its Outcome error has no lossless JetComponent/DataTree encoding".into()
                } else {
                    "no resource handle protocol emitted because checked operation roles were absent or ambiguous".into()
                },
                unsupported,
            })
        })
        .collect()
}


fn resource_helpers(
    program: &MirProgram,
    name: &str,
    resource_exports: &[ResourceExport],
    plan: &ResourcePlan,
) -> String {
    let prefix = c_identifier(name);
    let owner_type = format!("{prefix}_Document");
    let view_type = format!("{prefix}_View");
    let owner_rust_type = result_ok_type(&resource_exports[plan.open])
        .map(|ty| rust_component_type(program, ty))
        .unwrap_or_else(|| "()".to_string());
    let view_element_rust_type = result_ok_type(&resource_exports[plan.bytes])
        .and_then(|ty| match ty.kind() {
            MirTypeKind::Apply { name, args } if name.name == "View" && args.len() == 1 => {
                Some(rust_component_type(program, &args[0]))
            }
            _ => None,
        })
        .unwrap_or_else(|| "u8".to_string());
    let open_callee = resource_exports[plan.open].callee.as_str();
    let resource_identity = resource_exports
        .iter()
        .map(|export| format!("{}:{}", export.name, export.symbol))
        .collect::<Vec<_>>()
        .join(",");
    let bytes_callee = resource_exports[plan.bytes].callee.as_str();
    let at_callee = resource_exports[plan.at].callee.as_str();
    let replace_callee = resource_exports[plan.replace].callee.as_str();
    let close_callee = resource_exports[plan.close].callee.as_str();
    let open_args = if matches!(resource_exports[plan.open].params[0], MirAccess::Read) {
        "&values"
    } else {
        "values"
    };
    let replace_input = if matches!(resource_exports[plan.replace].params[1], MirAccess::Read) {
        "&values"
    } else {
        "values"
    };
    let document_open = format!("{prefix}_document_open");
    let document_bytes = format!("{prefix}_document_bytes");
    let view_at = format!("{prefix}_view_at");
    let document_replace = format!("{prefix}_document_replace");
    let document_close = format!("{prefix}_document_close");
    let error_payload = mangle_generated("library_component_error_value");
    let open_internal = mangle(&format!("library_resource_{prefix}_open"));
    let bytes_internal = mangle(&format!("library_resource_{prefix}_bytes"));
    let at_internal = mangle(&format!("library_resource_{prefix}_at"));
    let replace_internal = mangle(&format!("library_resource_{prefix}_replace"));
    let close_internal = mangle(&format!("library_resource_{prefix}_close"));
    let source = r#"
// jet-resource-exports: @@RESOURCE_IDENTITY@@
#[repr(C)]
pub struct @@OWNER_TYPE@@ {
    pub slot: u64,
    pub generation: u64,
}

#[repr(C)]
pub struct @@VIEW_TYPE@@ {
    pub slot: u64,
    pub generation: u64,
}

struct @@OWNER_TYPE@@State {
    resource: Option<Box<@@OWNER_RUST_TYPE@@>>,
    generation: u64,
    closed: bool,
}

struct @@VIEW_TYPE@@State {
    owner_slot: u64,
    generation: u64,
    ptr: usize,
    len: usize,
}

static @@PREFIX@@_OWNERS: std::sync::LazyLock<std::sync::Mutex<Vec<Option<@@OWNER_TYPE@@State>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));
static @@PREFIX@@_VIEWS: std::sync::LazyLock<std::sync::Mutex<Vec<Option<@@VIEW_TYPE@@State>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));
const @@PREFIX@@_GUEST_ERROR: i32 = 6;
const @@PREFIX@@_INTERNAL_FAILURE: i32 = 7;
fn @@PREFIX@@_status(run: impl FnOnce() -> Result<i32, i32>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(Ok(code)) => code,
        Ok(Err(code)) => code,
        Err(_) => @@PREFIX@@_INTERNAL_FAILURE,
    }
}

fn @@PREFIX@@_clear_error(error: *mut JetComponent) {
    if !error.is_null() {
        unsafe {
            *error = JetComponent {
                ptr: std::ptr::null(),
                len: 0,
            };
        }
    }
}

fn @@PREFIX@@_guest_error<T: __jet_Encode>(error: &T, out: *mut JetComponent) -> i32 {
    if !out.is_null() {
        unsafe {
            *out = @@ERROR_PAYLOAD@@(error);
        }
    }
    @@PREFIX@@_GUEST_ERROR
}

unsafe fn @@PREFIX@@_input<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], i32> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() || len > isize::MAX as usize || (ptr as usize).checked_add(len).is_none() {
        return Err(2);
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn @@PREFIX@@_values(input: &[u8]) -> Result<Vec<i64>, i32> {
    let mut values = Vec::new();
    values.try_reserve(input.len()).map_err(|_| 4)?;
    values.extend(input.iter().map(|value| i64::from(*value)));
    Ok(values)
}

fn @@PREFIX@@_alloc_owner(resource: Box<@@OWNER_RUST_TYPE@@>) -> Result<u64, i32> {
    let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
    let slot = if let Some(slot) = owners.iter().position(Option::is_none) {
        slot
    } else {
        owners.try_reserve(1).map_err(|_| 4)?;
        owners.push(None);
        owners.len() - 1
    };
    owners[slot] = Some(@@OWNER_TYPE@@State {
        resource: Some(resource),
        generation: 1,
        closed: false,
    });
    Ok(slot as u64)
}

fn @@PREFIX@@_alloc_view(state: @@VIEW_TYPE@@State) -> Result<u64, i32> {
    let mut views = @@PREFIX@@_VIEWS.lock().map_err(|_| 2)?;
    let slot = if let Some(slot) = views.iter().position(Option::is_none) {
        slot
    } else {
        views.try_reserve(1).map_err(|_| 4)?;
        views.push(None);
        views.len() - 1
    };
    views[slot] = Some(state);
    Ok(slot as u64)
}

// Vetted shared-memory reconstruction. The caller must hold a live owner
// generation guard; mutation, move, and close increment that generation first.
unsafe fn @@PREFIX@@_view_from_parts<'a>(
    ptr: usize,
    len: usize,
) -> Result<&'a [@@VIEW_ELEM_RUST_TYPE@@], i32> {
    if len == 0 {
        return Ok(&[]);
    }
    let bytes = len
        .checked_mul(std::mem::size_of::<@@VIEW_ELEM_RUST_TYPE@@>())
        .ok_or(3)?;
    if ptr == 0
        || bytes > isize::MAX as usize
        || ptr.checked_add(bytes).is_none()
        || ptr % std::mem::align_of::<@@VIEW_ELEM_RUST_TYPE@@>() != 0
    {
        return Err(2);
    }
    Ok(unsafe {
        std::slice::from_raw_parts(ptr as *const @@VIEW_ELEM_RUST_TYPE@@, len)
    })
}

#[export_name = "@@DOCUMENT_OPEN@@"]
pub extern "C" fn @@OPEN_INTERNAL@@(
    input: *const u8,
    len: usize,
    out: *mut @@OWNER_TYPE@@,
    error: *mut JetComponent,
) -> i32 {
    @@PREFIX@@_status(|| {
        @@PREFIX@@_clear_error(error);
        if out.is_null() {
            return Err(2);
        }
        let input = unsafe { @@PREFIX@@_input(input, len) }?;
        let values = @@PREFIX@@_values(input)?;
        let resource = match @@OPEN_CALLEE(@@OPEN_ARGS@@) {
            Ok(resource) => resource,
            Err(guest_error) => return Ok(@@PREFIX@@_guest_error(&guest_error, error)),
        };
        let slot = @@PREFIX@@_alloc_owner(Box::new(resource))?;
        unsafe {
            *out = @@OWNER_TYPE@@ {
                slot,
                generation: 1,
            };
        }
        Ok(0)
    })
}

#[export_name = "@@DOCUMENT_BYTES@@"]
pub extern "C" fn @@BYTES_INTERNAL@@(
    owner: *const @@OWNER_TYPE@@,
    out: *mut @@VIEW_TYPE@@,
    error: *mut JetComponent,
) -> i32 {
    @@PREFIX@@_status(|| {
        @@PREFIX@@_clear_error(error);
        if owner.is_null() || out.is_null() {
            return Err(2);
        }
        let token = unsafe { *owner };
        let (ptr, len, generation) = {
            let owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
            let state = owners
                .get(usize::try_from(token.slot).map_err(|_| 2)?)
                .and_then(Option::as_ref)
                .ok_or(2)?;
            if state.closed || state.resource.is_none() {
                return Err(5);
            }
            if state.generation != token.generation {
                return Err(1);
            }
            let resource = state.resource.as_deref().ok_or(5)?;
            let view = match @@BYTES_CALLEE(resource) {
                Ok(view) => view,
                Err(guest_error) => return Ok(@@PREFIX@@_guest_error(&guest_error, error)),
            };
            (view.as_ptr() as usize, view.len(), state.generation)
        };
        let slot = @@PREFIX@@_alloc_view(@@VIEW_TYPE@@State {
            owner_slot: token.slot,
            generation,
            ptr,
            len,
        })?;
        unsafe {
            *out = @@VIEW_TYPE@@ { slot, generation };
        }
        Ok(0)
    })
}

#[export_name = "@@VIEW_AT@@"]
pub extern "C" fn @@AT_INTERNAL@@(
    view: *const @@VIEW_TYPE@@,
    index: usize,
    out: *mut i64,
    error: *mut JetComponent,
) -> i32 {
    @@PREFIX@@_status(|| {
        @@PREFIX@@_clear_error(error);
        if view.is_null() || out.is_null() {
            return Err(2);
        }
        let token = unsafe { *view };
        let (owner_slot, generation, ptr, len) = {
            let views = @@PREFIX@@_VIEWS.lock().map_err(|_| 2)?;
            let view_state = views
                .get(usize::try_from(token.slot).map_err(|_| 2)?)
                .and_then(Option::as_ref)
                .ok_or(2)?;
            if view_state.generation != token.generation {
                return Err(1);
            }
            (
                view_state.owner_slot,
                view_state.generation,
                view_state.ptr,
                view_state.len,
            )
        };
        let owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
        let owner = owners
            .get(usize::try_from(owner_slot).map_err(|_| 2)?)
            .and_then(Option::as_ref)
            .ok_or(1)?;
        if owner.closed || owner.resource.is_none() || owner.generation != generation {
            return Err(1);
        }
        let resource = owner.resource.as_deref().ok_or(5)?;
        let view = unsafe { @@PREFIX@@_view_from_parts(ptr, len) }?;
        let index = i64::try_from(index).map_err(|_| 3)?;
        let value = match @@AT_CALLEE@@(resource, view, index) {
            Ok(value) => value,
            Err(guest_error) => return Ok(@@PREFIX@@_guest_error(&guest_error, error)),
        };
        unsafe {
            *out = value;
        }
        Ok(0)
    })
}

#[export_name = "@@DOCUMENT_REPLACE@@"]
pub extern "C" fn @@REPLACE_INTERNAL@@(
    owner: *mut @@OWNER_TYPE@@,
    input: *const u8,
    len: usize,
    error: *mut JetComponent,
) -> i32 {
    @@PREFIX@@_status(|| {
        @@PREFIX@@_clear_error(error);
        if owner.is_null() {
            return Err(2);
        }
        let input = unsafe { @@PREFIX@@_input(input, len) }?;
        let values = @@PREFIX@@_values(input)?;
        let token = unsafe { *owner };
        let mut state = {
            let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
            let slot = usize::try_from(token.slot).map_err(|_| 2)?;
            let state = owners.get_mut(slot).and_then(Option::take).ok_or(2)?;
            if state.closed || state.resource.is_none() {
                owners[slot] = Some(state);
                return Err(5);
            }
            if state.generation != token.generation {
                owners[slot] = Some(state);
                return Err(1);
            }
            if state.generation == u64::MAX {
                owners[slot] = Some(state);
                return Err(3);
            }
            state
        };
        // Invalidate all views before moving the guest owner.
        state.generation += 1;
        let generation = state.generation;
        let resource = *state.resource.take().ok_or(5)?;
        let next = match @@REPLACE_CALLEE(resource, @@REPLACE_INPUT@@) {
            Ok(next) => next,
            Err(guest_error) => {
                state.closed = true;
                let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
                owners[usize::try_from(token.slot).map_err(|_| 2)?] = Some(state);
                return Ok(@@PREFIX@@_guest_error(&guest_error, error));
            }
        };
        state.resource = Some(Box::new(next));
        let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
        owners[usize::try_from(token.slot).map_err(|_| 2)?] = Some(state);
        unsafe {
            (*owner).generation = generation;
        }
        Ok(0)
    })
}

#[export_name = "@@DOCUMENT_CLOSE@@"]
pub extern "C" fn @@CLOSE_INTERNAL@@(
    owner: *mut @@OWNER_TYPE@@,
    error: *mut JetComponent,
) -> i32 {
    @@PREFIX@@_status(|| {
        @@PREFIX@@_clear_error(error);
        if owner.is_null() {
            return Err(2);
        }
        let token = unsafe { *owner };
        let mut state = {
            let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
            let slot = usize::try_from(token.slot).map_err(|_| 2)?;
            let state = owners.get_mut(slot).and_then(Option::take).ok_or(2)?;
            if state.closed || state.resource.is_none() {
                owners[slot] = Some(state);
                return Err(5);
            }
            if state.generation != token.generation {
                owners[slot] = Some(state);
                return Err(1);
            }
            if state.generation == u64::MAX {
                owners[slot] = Some(state);
                return Err(3);
            }
            state
        };
        // Invalidate all views before moving and dropping the guest owner.
        state.generation += 1;
        let generation = state.generation;
        let resource = *state.resource.take().ok_or(5)?;
        if let Err(guest_error) = @@CLOSE_CALLEE(resource) {
            state.closed = true;
            let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
            owners[usize::try_from(token.slot).map_err(|_| 2)?] = Some(state);
            return Ok(@@PREFIX@@_guest_error(&guest_error, error));
        }
        state.closed = true;
        let mut owners = @@PREFIX@@_OWNERS.lock().map_err(|_| 2)?;
        owners[usize::try_from(token.slot).map_err(|_| 2)?] = Some(state);
        unsafe {
            (*owner).generation = generation;
        }
        Ok(0)
    })
}
"#;
    let mut source = source.to_string();
    for (token, value) in [
        ("@@PREFIX@@", prefix.as_str()),
        ("@@RESOURCE_IDENTITY@@", resource_identity.as_str()),
        ("@@OWNER_TYPE@@", owner_type.as_str()),
        ("@@OWNER_RUST_TYPE@@", owner_rust_type.as_str()),
        ("@@VIEW_TYPE@@", view_type.as_str()),
        ("@@VIEW_ELEM_RUST_TYPE@@", view_element_rust_type.as_str()),
        ("@@OPEN_CALLEE@@", open_callee),
        ("@@AT_CALLEE@@", at_callee),
        ("@@OPEN_ARGS@@", open_args),
        ("@@BYTES_CALLEE@@", bytes_callee),
        ("@@ERROR_PAYLOAD@@", error_payload.as_str()),
        ("@@REPLACE_CALLEE@@", replace_callee),
        ("@@REPLACE_INPUT@@", replace_input),
        ("@@CLOSE_CALLEE@@", close_callee),
        ("@@DOCUMENT_OPEN@@", document_open.as_str()),
        ("@@DOCUMENT_BYTES@@", document_bytes.as_str()),
        ("@@VIEW_AT@@", view_at.as_str()),
        ("@@DOCUMENT_REPLACE@@", document_replace.as_str()),
        ("@@DOCUMENT_CLOSE@@", document_close.as_str()),
        ("@@OPEN_INTERNAL@@", open_internal.as_str()),
        ("@@BYTES_INTERNAL@@", bytes_internal.as_str()),
        ("@@AT_INTERNAL@@", at_internal.as_str()),
        ("@@REPLACE_INTERNAL@@", replace_internal.as_str()),
        ("@@CLOSE_INTERNAL@@", close_internal.as_str()),
    ] {
        source = source.replace(token, value);
    }
    source
}

fn header_guard(name: &str) -> String {
    let mut guard = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    if guard.is_empty() {
        guard.push('_');
    }
    if guard.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        guard.insert(0, '_');
    }
    format!("{guard}_H")
}

fn resource_protocol_header(name: &str) -> String {
    let prefix = c_identifier(name);
    let upper = prefix.to_ascii_uppercase();
    format!(
        "\ntypedef struct {prefix}_Document {{ uint64_t slot; uint64_t generation; }} {prefix}_Document;\ntypedef struct {prefix}_View {{ uint64_t slot; uint64_t generation; }} {prefix}_View;\ntypedef enum {prefix}_Error {{ {upper}_OK = 0, {upper}_EXPIRED_VIEW = 1, {upper}_INVALID_HANDLE = 2, {upper}_BOUNDS_FAILURE = 3, {upper}_ALLOCATION_FAILURE = 4, {upper}_CLOSED = 5, {upper}_GUEST_ERROR = 6, {upper}_INTERNAL_FAILURE = 7 }} {prefix}_Error;\n",
        prefix = prefix,
        upper = upper,
    )
}

fn resource_protocol_declarations(name: &str) -> String {
    let prefix = c_identifier(name);
    format!(
        "int {prefix}_document_open(const uint8_t *input, size_t len, {prefix}_Document *out, JetComponent *error);\nint {prefix}_document_bytes(const {prefix}_Document *owner, {prefix}_View *out, JetComponent *error);\nint {prefix}_view_at(const {prefix}_View *view, size_t index, int64_t *out, JetComponent *error);\nint {prefix}_document_replace({prefix}_Document *owner, const uint8_t *input, size_t len, JetComponent *error);\nint {prefix}_document_close({prefix}_Document *owner, JetComponent *error);\n",
        prefix = prefix,
    )
}

#[cfg(test)]
fn render_c_header(name: &str, exports: &[LibraryExport]) -> String {
    render_c_header_with_rich(name, exports, &[], None, &[])
}

fn append_c_matrix_field(out: &mut String, key: &str, value: &str) {
    for line in value.split(|ch| ch == '\r' || ch == '\n') {
        let mut safe = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '*' && chars.peek() == Some(&'/') {
                safe.push_str("* /");
                chars.next();
            } else if ch == '\0' || ch.is_control() {
                safe.push('?');
            } else {
                safe.push(ch);
            }
        }
        out.push_str("/* jet-ffi-projection: ");
        out.push_str(key);
        out.push('=');
        out.push_str(&safe);
        out.push_str(" */\n");
    }
}

fn append_projection_matrix_comments(out: &mut String, projections: &[LibraryProjection]) {
    if projections.is_empty() {
        return;
    }
    out.push_str("\n/* jet-ffi-projection-matrix: schema=v1; proof=declared;unexecuted */\n");
    for projection in projections {
        append_c_matrix_field(out, "language", &projection.language);
        append_c_matrix_field(out, "direction", &projection.direction);
        append_c_matrix_field(out, "artifact-identity", &projection.artifact_identity);
        append_c_matrix_field(out, "supported-ownership", &projection.ownership);
        append_c_matrix_field(out, "supported-integer-ranges", &projection.integer_ranges);
        append_c_matrix_field(out, "supported-encoding", &projection.encoding);
        append_c_matrix_field(out, "supported-enums", &projection.enums);
        append_c_matrix_field(out, "supported-errors", &projection.errors);
        append_c_matrix_field(out, "runtime-roots", &projection.runtime_roots);
        append_c_matrix_field(out, "resource-lifetime", &projection.resource_lifetime);
        append_c_matrix_field(
            out,
            "target-runtime-restrictions",
            &format!(
                "target={}; availability={}; task-thread={}",
                projection.contract.identity.target,
                projection.contract.target_availability,
                projection.contract.task_thread
            ),
        );
        append_c_matrix_field(
            out,
            "proof-status",
            "declared;unexecuted;runtime/golden/cost/human/qualification acceptance remains #2919",
        );
        for unsupported in &projection.unsupported {
            append_c_matrix_field(out, "unsupported", unsupported);
        }
        for (key, value) in projection.contract.provenance_fields() {
            append_c_matrix_field(out, &format!("contract-{key}"), &value);
        }
    }
}

fn render_c_header_with_rich(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
    projections: &[LibraryProjection],
) -> String {
    let guard = header_guard(name);
    let mut out = format!(
        "/* Generated by Jet — D-LIB-EXPORT1=C / D-EMBED1=E / D-EMBED2=C. */\n#ifndef {guard}\n#define {guard}\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n",
    );
    append_projection_matrix_comments(&mut out, projections);
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out.push_str("\ntypedef struct JetText { const uint8_t *ptr; size_t len; } JetText;\n");
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str(
            "\ntypedef struct JetComponent { const uint8_t *ptr; size_t len; } JetComponent;\n",
        );
    }
    if resource_plan.is_some() {
        out.push_str(&resource_protocol_header(name));
    }
    out.push_str("\n#ifdef __cplusplus\nextern \"C\" {\n#endif\n");
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out.push_str("void jet_text_free(JetText value);\n");
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("void jet_component_free(JetComponent value);\n");
    }
    if resource_plan.is_some() {
        out.push_str(&resource_protocol_declarations(name));
    }
    for export in exports {
        let access = export
            .conventions
            .iter()
            .map(|convention| match convention {
                MirAccess::Read => "read",
                MirAccess::Write => "write",
                MirAccess::Move => "move",
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!("/* jet-access: {access} */\n"));
        let params = (0..export.params)
            .map(|index| format!("{} p{index}", export.scalar.c_ty()))
            .collect::<Vec<_>>();
        out.push_str(&format!(
            "{} {}({});\n",
            export.scalar.c_ty(),
            export.symbol,
            if params.is_empty() {
                "void".to_string()
            } else {
                params.join(", ")
            },
        ));
    }
    for export in rich_exports {
        let ownership = export
            .ownership
            .parameters
            .iter()
            .map(|ownership| match ownership.mode {
                MirOwnershipMode::Copy => "copy",
                MirOwnershipMode::Owned => "owned",
                MirOwnershipMode::Shared => "shared",
                MirOwnershipMode::ReadBorrow => "read",
                MirOwnershipMode::WriteBorrow => "write",
                MirOwnershipMode::Move => "move",
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(
            "/* jet-component-wire: {} */\n/* jet-ownership: {ownership} */\nJetComponent {}_component(JetComponent input);\n",
            export.descriptor.wire(),
            export.symbol,
        ));
    }
    out.push_str("\n#ifdef __cplusplus\n}\n#endif\n#endif\n");
    out
}


fn render_cpp(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = format!(
        "// Generated Jet C++ facade; all calls cross the stable C shim.\n#include \"{name}.h\"\n#include <cstdint>\n\nnamespace jet {{\n",
        name = name,
    );
    for export in exports {
        out.push_str(&format!(
            "inline auto {name}({args}) -> {ret} {{ return ::{symbol}({call}); }}\n",
            name = export.name,
            args = (0..export.params)
                .map(|i| format!("{} p{i}", export.scalar.c_ty()))
                .collect::<Vec<_>>()
                .join(", "),
            ret = export.scalar.c_ty(),
            symbol = export.symbol,
            call = (0..export.params)
                .map(|i| format!("p{i}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    for export in rich_exports {
        out.push_str(&format!(
            "using {name}Component = JetComponent;\ninline {name}Component {name}_component_call({name}Component input) {{ return ::{symbol}_component(input); }}\n",
            name = export.name,
            symbol = export.symbol,
        ));
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        let upper = prefix.to_ascii_uppercase();
        out.push_str(&format!(
            "using ResourceDocumentAbi = {prefix}_Document;\nusing ResourceView = {prefix}_View;\nusing ResourceStatus = {prefix}_Error;\nusing ResourceError = JetComponent;\nclass ResourceDocument {{\npublic:\n    ResourceDocument() : raw_{{0, 0}}, active_(false) {{}}\n    ResourceDocument(const ResourceDocument&) = delete;\n    ResourceDocument& operator=(const ResourceDocument&) = delete;\n    ~ResourceDocument() {{ close(); }}\n    int open(const uint8_t* input, size_t len, JetComponent* error = nullptr) {{\n        if (active_) {{ int status = close(error); if (status != 0) return status; }}\n        int status = ::{prefix}_document_open(input, len, &raw_, error);\n        active_ = status == 0;\n        return status;\n    }}\n    int bytes(ResourceView* out, JetComponent* error = nullptr) const {{\n        return active_ ? ::{prefix}_document_bytes(&raw_, out, error) : {upper}_CLOSED;\n    }}\n    int at(const ResourceView& view, size_t index, int64_t* out, JetComponent* error = nullptr) const {{\n        return active_ ? ::{prefix}_view_at(&view, index, out, error) : {upper}_CLOSED;\n    }}\n    int replace(const uint8_t* input, size_t len, JetComponent* error = nullptr) {{\n        if (!active_) return {upper}_CLOSED;\n        int status = ::{prefix}_document_replace(&raw_, input, len, error);\n        if (status == {upper}_GUEST_ERROR) active_ = false;\n        return status;\n    }}\n    int close(JetComponent* error = nullptr) {{\n        if (!active_) return {upper}_CLOSED;\n        int status = ::{prefix}_document_close(&raw_, error);\n        if (status == 0 || status == {upper}_GUEST_ERROR) active_ = false;\n        return status;\n    }}\n    const ResourceDocumentAbi& raw() const {{ return raw_; }}\nprivate:\n    ResourceDocumentAbi raw_;\n    bool active_;\n}};\n",
            prefix = prefix,
            upper = upper,
        ));
    }
    out.push_str("}\n");
    out
}
fn render_rust(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = format!(
        "// Generated Jet Rust host facade; ABI is the stable C header for {name}.\n",
        name = name,
    );
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out.push_str("#[repr(C)] pub struct JetText { pub ptr: *const u8, pub len: usize }\n");
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("#[repr(C)] pub struct JetComponent { pub ptr: *const u8, pub len: usize }\n");
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        let upper = prefix.to_ascii_uppercase();
        out.push_str(&format!(
            "#[repr(C)] #[derive(Clone, Copy)] pub struct {prefix}_Document {{ pub slot: u64, pub generation: u64 }}\n#[repr(C)] #[derive(Clone, Copy)] pub struct {prefix}_View {{ pub slot: u64, pub generation: u64 }}\npub const {upper}_OK: i32 = 0;\npub const {upper}_EXPIRED_VIEW: i32 = 1;\npub const {upper}_INVALID_HANDLE: i32 = 2;\npub const {upper}_BOUNDS_FAILURE: i32 = 3;\npub const {upper}_ALLOCATION_FAILURE: i32 = 4;\npub const {upper}_CLOSED: i32 = 5;\npub const {upper}_GUEST_ERROR: i32 = 6;\npub const {upper}_INTERNAL_FAILURE: i32 = 7;\n",
            prefix = prefix,
            upper = upper,
        ));
    }
    out.push_str("extern \"C\" {\n");
    for export in exports {
        let ffi = mangle(&format!("library_host_{}", export.name));
        let ty = if export.scalar == ExportScalar::Text {
            "JetText"
        } else {
            export.scalar.rust_ty()
        };
        out.push_str(&format!(
            "    #[link_name = \"{symbol}\"] fn {ffi}({args}) -> {ty};\n",
            symbol = export.symbol,
            ffi = ffi,
            args = (0..export.params)
                .map(|i| format!("p{i}: {ty}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    for export in rich_exports {
        let ffi = mangle(&format!("library_component_host_{}", export.name));
        out.push_str(&format!(
            "    #[link_name = \"{symbol}_component\"] fn {ffi}(input: JetComponent) -> JetComponent;\n",
            symbol = export.symbol,
            ffi = ffi,
        ));
    }
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out.push_str("    #[link_name = \"jet_text_free\"] fn jet_text_free(value: JetText);\n");
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("    #[link_name = \"jet_component_free\"] fn jet_component_free(value: JetComponent);\n");
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        out.push_str(&format!(
            "    #[link_name = \"{prefix}_document_open\"] fn {prefix}_document_open(input: *const u8, len: usize, out: *mut {prefix}_Document, error: *mut JetComponent) -> i32;\n    #[link_name = \"{prefix}_document_bytes\"] fn {prefix}_document_bytes(owner: *const {prefix}_Document, out: *mut {prefix}_View, error: *mut JetComponent) -> i32;\n    #[link_name = \"{prefix}_view_at\"] fn {prefix}_view_at(view: *const {prefix}_View, index: usize, out: *mut i64, error: *mut JetComponent) -> i32;\n    #[link_name = \"{prefix}_document_replace\"] fn {prefix}_document_replace(owner: *mut {prefix}_Document, input: *const u8, len: usize, error: *mut JetComponent) -> i32;\n    #[link_name = \"{prefix}_document_close\"] fn {prefix}_document_close(owner: *mut {prefix}_Document, error: *mut JetComponent) -> i32;\n",
            prefix = prefix,
        ));
    }
    out.push_str("}\n");
    for export in exports {
        let ffi = mangle(&format!("library_host_{}", export.name));
        let ty = if export.scalar == ExportScalar::Text {
            "JetText"
        } else {
            export.scalar.rust_ty()
        };
        out.push_str(&format!(
            "pub unsafe fn {name}({args}) -> {ty} {{ {ffi}({call}) }}\n",
            name = export.name,
            args = (0..export.params)
                .map(|i| format!("p{i}: {ty}"))
                .collect::<Vec<_>>()
                .join(", "),
            ty = ty,
            ffi = ffi,
            call = (0..export.params)
                .map(|i| format!("p{i}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        let upper = prefix.to_ascii_uppercase();
        out.push_str(&format!(
            "fn resource_empty_error() -> JetComponent {{ JetComponent {{ ptr: std::ptr::null(), len: 0 }} }}\nunsafe fn resource_release_error(error: &mut JetComponent) {{ if !error.ptr.is_null() {{ jet_component_free(*error); }} *error = resource_empty_error(); }}\npub struct ResourceDocument {{ raw: {prefix}_Document, active: bool }}\npub type ResourceView = {prefix}_View;\npub type ResourceErrorPayload = JetComponent;\nimpl ResourceDocument {{\n    pub fn open(input: &[u8]) -> Result<Self, i32> {{\n        let mut error = resource_empty_error();\n        let result = Self::open_with_error(input, &mut error);\n        if result.is_err() {{ unsafe {{ resource_release_error(&mut error); }} }}\n        result\n    }}\n    pub fn open_with_error(input: &[u8], error: &mut JetComponent) -> Result<Self, i32> {{\n        let mut raw = {prefix}_Document {{ slot: 0, generation: 0 }};\n        let status = unsafe {{ {prefix}_document_open(input.as_ptr(), input.len(), &mut raw, error as *mut JetComponent) }};\n        if status == {upper}_OK {{ Ok(Self {{ raw, active: true }}) }} else {{ Err(status) }}\n    }}\n    pub fn bytes(&self) -> Result<ResourceView, i32> {{\n        let mut error = resource_empty_error();\n        let result = self.bytes_with_error(&mut error);\n        if result.is_err() {{ unsafe {{ resource_release_error(&mut error); }} }}\n        result\n    }}\n    pub fn bytes_with_error(&self, error: &mut JetComponent) -> Result<ResourceView, i32> {{\n        if !self.active {{ return Err({upper}_CLOSED); }}\n        let mut view = {prefix}_View {{ slot: 0, generation: 0 }};\n        let status = unsafe {{ {prefix}_document_bytes(&self.raw, &mut view, error as *mut JetComponent) }};\n        if status == {upper}_OK {{ Ok(view) }} else {{ Err(status) }}\n    }}\n    pub fn at(&self, view: &ResourceView, index: usize) -> Result<i64, i32> {{\n        let mut error = resource_empty_error();\n        let result = self.at_with_error(view, index, &mut error);\n        if result.is_err() {{ unsafe {{ resource_release_error(&mut error); }} }}\n        result\n    }}\n    pub fn at_with_error(&self, view: &ResourceView, index: usize, error: &mut JetComponent) -> Result<i64, i32> {{\n        if !self.active {{ return Err({upper}_CLOSED); }}\n        let mut value = 0i64;\n        let status = unsafe {{ {prefix}_view_at(view, index, &mut value, error as *mut JetComponent) }};\n        if status == {upper}_OK {{ Ok(value) }} else {{ Err(status) }}\n    }}\n    pub fn replace(&mut self, input: &[u8]) -> Result<(), i32> {{\n        let mut error = resource_empty_error();\n        let result = self.replace_with_error(input, &mut error);\n        if result.is_err() {{ unsafe {{ resource_release_error(&mut error); }} }}\n        result\n    }}\n    pub fn replace_with_error(&mut self, input: &[u8], error: &mut JetComponent) -> Result<(), i32> {{\n        if !self.active {{ return Err({upper}_CLOSED); }}\n        let status = unsafe {{ {prefix}_document_replace(&mut self.raw, input.as_ptr(), input.len(), error as *mut JetComponent) }};\n        if status == {upper}_GUEST_ERROR {{ self.active = false; }}\n        if status == {upper}_OK {{ Ok(()) }} else {{ Err(status) }}\n    }}\n    pub fn close(&mut self) -> Result<(), i32> {{\n        let mut error = resource_empty_error();\n        let result = self.close_with_error(&mut error);\n        if result.is_err() {{ unsafe {{ resource_release_error(&mut error); }} }}\n        result\n    }}\n    pub fn close_with_error(&mut self, error: &mut JetComponent) -> Result<(), i32> {{\n        if !self.active {{ return Err({upper}_CLOSED); }}\n        let status = unsafe {{ {prefix}_document_close(&mut self.raw, error as *mut JetComponent) }};\n        if status == {upper}_OK || status == {upper}_GUEST_ERROR {{ self.active = false; }}\n        if status == {upper}_OK {{ Ok(()) }} else {{ Err(status) }}\n    }}\n}}\nimpl Drop for ResourceDocument {{\n    fn drop(&mut self) {{ let _ = self.close(); }}\n}}\n",
            prefix = prefix,
            upper = upper,
        ));
    }
    for export in rich_exports {
        let ffi = mangle(&format!("library_component_host_{}", export.name));
        out.push_str(&format!(
            "pub type {name}Component = JetComponent;\npub unsafe fn {name}_component_call(input: JetComponent) -> JetComponent {{ {ffi}(input) }}\n",
            name = export.name,
            ffi = ffi,
        ));
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("pub unsafe fn free_component(value: JetComponent) { jet_component_free(value); }\n");
    }
    out
}

fn render_zig(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = format!(
        "// Generated Jet Zig adapter; all calls use the stable C shim from {name}.h.\nconst c = @cImport({{ @cInclude(\"{name}.h\"); }});\n\n",
        name = name,
    );
    for export in exports {
        let ty = match export.scalar {
            ExportScalar::Int => "i64",
            ExportScalar::Float => "f64",
            ExportScalar::Bool => "bool",
            ExportScalar::Text => "c.JetText",
        };
        out.push_str(&format!(
            "pub fn {name}({args}) {ty} {{ return c.{symbol}({call}); }}\n",
            name = export.name,
            args = (0..export.params)
                .map(|i| format!("p{i}: {ty}"))
                .collect::<Vec<_>>()
                .join(", "),
            ty = ty,
            symbol = export.symbol,
            call = (0..export.params)
                .map(|i| format!("p{i}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    for export in rich_exports {
        out.push_str(&format!(
            "pub const {name}Component = c.JetComponent;\npub fn {name}_component_call(input: c.JetComponent) c.JetComponent {{ return c.{symbol}_component(input); }}\n",
            name = export.name,
            symbol = export.symbol,
        ));
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("pub fn component_free(value: c.JetComponent) void { c.jet_component_free(value); }\n");
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        out.push_str(&format!(
            "pub const RESOURCE_OK: c_int = 0;\npub const RESOURCE_EXPIRED_VIEW: c_int = 1;\npub const RESOURCE_INVALID_HANDLE: c_int = 2;\npub const RESOURCE_BOUNDS_FAILURE: c_int = 3;\npub const RESOURCE_ALLOCATION_FAILURE: c_int = 4;\npub const RESOURCE_CLOSED: c_int = 5;\npub const RESOURCE_GUEST_ERROR: c_int = 6;\npub const RESOURCE_INTERNAL_FAILURE: c_int = 7;\n",
        ));
        out.push_str(&format!(
            "pub const ResourceView = c.{prefix}_View;\npub const ResourceError = c.JetComponent;\npub const ResourceDocument = struct {{\n    raw: c.{prefix}_Document = .{{ .slot = 0, .generation = 0 }},\n    active: bool = false,\n    pub fn open(self: *ResourceDocument, input: []const u8) c_int {{\n        return self.open_with_error(input, null);\n    }}\n    pub fn open_with_error(self: *ResourceDocument, input: []const u8, error_out: ?*ResourceError) c_int {{\n        const status = c.{prefix}_document_open(input.ptr, input.len, &self.raw, error_out);\n        self.active = status == 0;\n        return status;\n    }}\n    pub fn bytes(self: *const ResourceDocument, out: *ResourceView) c_int {{\n        return self.bytes_with_error(out, null);\n    }}\n    pub fn bytes_with_error(self: *const ResourceDocument, out: *ResourceView, error_out: ?*ResourceError) c_int {{\n        if (!self.active) return c.{upper}_CLOSED;\n        return c.{prefix}_document_bytes(&self.raw, out, error_out);\n    }}\n    pub fn at(self: *const ResourceDocument, view: *const ResourceView, index: usize, out: *i64) c_int {{\n        return self.at_with_error(view, index, out, null);\n    }}\n    pub fn at_with_error(self: *const ResourceDocument, view: *const ResourceView, index: usize, out: *i64, error_out: ?*ResourceError) c_int {{\n        if (!self.active) return c.{upper}_CLOSED;\n        return c.{prefix}_view_at(view, index, out, error_out);\n    }}\n    pub fn replace(self: *ResourceDocument, input: []const u8) c_int {{\n        return self.replace_with_error(input, null);\n    }}\n    pub fn replace_with_error(self: *ResourceDocument, input: []const u8, error_out: ?*ResourceError) c_int {{\n        if (!self.active) return c.{upper}_CLOSED;\n        const status = c.{prefix}_document_replace(&self.raw, input.ptr, input.len, error_out);\n        if (status == c.{upper}_GUEST_ERROR) self.active = false;\n        return status;\n    }}\n    pub fn close(self: *ResourceDocument) c_int {{\n        return self.close_with_error(null);\n    }}\n    pub fn close_with_error(self: *ResourceDocument, error_out: ?*ResourceError) c_int {{\n        if (!self.active) return c.{upper}_CLOSED;\n        const status = c.{prefix}_document_close(&self.raw, error_out);\n        if (status == 0 or status == c.{upper}_GUEST_ERROR) self.active = false;\n        return status;\n    }}\n}};\n",
            prefix = prefix,
            upper = prefix.to_ascii_uppercase(),
        ));
    }
    out
}

fn render_go(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = format!(
        "// Code generated by Jet. Stable C shim through cgo for {name}.\npackage {pkg}\n\n/* #include \"{name}.h\" */\nimport \"C\"\n\n",
        name = name,
        pkg = c_identifier(name),
    );
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        let upper = prefix.to_ascii_uppercase();
        out = out.replace("import \"C\"\n", "import \"C\"\nimport \"runtime\"\nimport \"unsafe\"\n");
        out.push_str(
            "const (\n    ResourceOK C.int = 0\n    ResourceExpiredView C.int = 1\n    ResourceInvalidHandle C.int = 2\n    ResourceBoundsFailure C.int = 3\n    ResourceAllocationFailure C.int = 4\n    ResourceClosed C.int = 5\n    ResourceGuestError C.int = 6\n    ResourceInternalFailure C.int = 7\n)\n",
        );
        out.push_str(&format!(
            "type ResourceView = C.{prefix}_View\ntype ResourceError = C.JetComponent\ntype ResourceDocument struct {{ raw C.{prefix}_Document; active bool }}\nfunc resourcePtr(input []byte) unsafe.Pointer {{ if len(input) == 0 {{ return nil }}; return unsafe.Pointer(&input[0]) }}\nfunc (d *ResourceDocument) Open(input []byte) C.int {{ return d.OpenWithError(input, nil) }}\nfunc (d *ResourceDocument) OpenWithError(input []byte, errorOut *ResourceError) C.int {{ status := C.{prefix}_document_open((*C.uint8_t)(resourcePtr(input)), C.size_t(len(input)), &d.raw, errorOut); runtime.KeepAlive(input); d.active = status == 0; return status }}\nfunc (d *ResourceDocument) Bytes(out *ResourceView) C.int {{ return d.BytesWithError(out, nil) }}\nfunc (d *ResourceDocument) BytesWithError(out *ResourceView, errorOut *ResourceError) C.int {{ if !d.active {{ return C.{upper}_CLOSED }}; return C.{prefix}_document_bytes(&d.raw, out, errorOut) }}\nfunc (d *ResourceDocument) At(view *ResourceView, index int) (C.int64_t, C.int) {{ return d.AtWithError(view, index, nil) }}\nfunc (d *ResourceDocument) AtWithError(view *ResourceView, index int, errorOut *ResourceError) (C.int64_t, C.int) {{ if !d.active {{ return 0, C.{upper}_CLOSED }}; var value C.int64_t; status := C.{prefix}_view_at(view, C.size_t(index), &value, errorOut); return value, status }}\nfunc (d *ResourceDocument) Replace(input []byte) C.int {{ return d.ReplaceWithError(input, nil) }}\nfunc (d *ResourceDocument) ReplaceWithError(input []byte, errorOut *ResourceError) C.int {{ if !d.active {{ return C.{upper}_CLOSED }}; status := C.{prefix}_document_replace(&d.raw, (*C.uint8_t)(resourcePtr(input)), C.size_t(len(input)), errorOut); runtime.KeepAlive(input); if status == C.{upper}_GUEST_ERROR {{ d.active = false }}; return status }}\nfunc (d *ResourceDocument) Close() C.int {{ return d.CloseWithError(nil) }}\nfunc (d *ResourceDocument) CloseWithError(errorOut *ResourceError) C.int {{ if !d.active {{ return C.{upper}_CLOSED }}; status := C.{prefix}_document_close(&d.raw, errorOut); if status == 0 || status == C.{upper}_GUEST_ERROR {{ d.active = false }}; return status }}\n",
            prefix = prefix,
            upper = upper,
        ));
    }
    for export in exports {
        let ty = match export.scalar {
            ExportScalar::Int => "C.int64_t",
            ExportScalar::Float => "C.double",
            ExportScalar::Bool => "C.bool",
            ExportScalar::Text => "C.JetText",
        };
        out.push_str(&format!(
            "func {name}({args}) {ty} {{ return C.{symbol}({call}) }}\n",
            name = go_export_name(&export.name),
            args = (0..export.params)
                .map(|i| format!("p{i} {ty}"))
                .collect::<Vec<_>>()
                .join(", "),
            ty = ty,
            symbol = export.symbol,
            call = (0..export.params)
                .map(|i| format!("p{i}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    for export in rich_exports {
        let name = go_export_name(&export.name);
        out.push_str(&format!(
            "type {name}Component = C.JetComponent\nfunc {name}ComponentCall(input C.JetComponent) C.JetComponent {{ return C.{symbol}_component(input) }}\n",
            name = name,
            symbol = export.symbol,
        ));
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out.push_str("func FreeComponent(value C.JetComponent) { C.jet_component_free(value) }\n");
    }
    out
}

fn render_javascript(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = String::from(
        r#"// Generated Node.js ESM facade. `load` requires a registered Node-API `.node` addon.
import { Buffer } from "node:buffer";
const I64_MIN = -(1n << 63n);
const I64_MAX = (1n << 63n) - 1n;

function checkedI64(value) {
    if (typeof value !== "bigint") {
        throw new TypeError("Jet Int projection expects a BigInt");
    }
    if (value < I64_MIN || value > I64_MAX) {
        throw new RangeError("Jet Int projection exceeds signed I64");
    }
    return value;
}

export function load(path) {
    const module = { exports: {} };
    process.dlopen(module, path);
    const addon = module.exports;
    if (addon === null || typeof addon !== "object") {
        throw new TypeError("Jet library did not load a registered Node-API addon");
    }
    return new Library(addon);
}

function componentResponse(addon, symbol, value) {
    const call = addon[symbol];
    if (typeof call !== "function") {
        throw new TypeError(`Node-API addon has no ${symbol} export`);
    }
    const result = call(Buffer.from(JSON.stringify(value), "utf8"));
    if (!Buffer.isBuffer(result) && !(result instanceof Uint8Array)) {
        throw new TypeError(`Node-API ${symbol} must return a byte buffer`);
    }
    const envelope = JSON.parse(Buffer.from(result).toString("utf8"));
    if (envelope && envelope.ok === false) {
        const error = new Error("Jet component call failed");
        error.payload = envelope.error ?? null;
        throw error;
    }
    return envelope && Object.prototype.hasOwnProperty.call(envelope, "value")
        ? envelope.value
        : envelope;
}

export class Library {
    constructor(addon) {
        this.addon = addon;
    }
}

"#,
    );
    for export in exports {
        let body = if export.scalar == ExportScalar::Text {
            format!(
                "const encoded = args.map(value => typeof value === \"string\" ? Buffer.from(value, \"utf8\") : value); return library.addon.{symbol}(...encoded);",
                symbol = export.symbol,
            )
        } else if export.scalar == ExportScalar::Int {
            format!(
                "const encoded = args.map(checkedI64); return library.addon.{symbol}(...encoded);",
                symbol = export.symbol,
            )
        } else {
            format!("return library.addon.{}(...args);", export.symbol)
        };
        out.push_str(&format!(
            "export function {name}(library, ...args) {{ {body} }}\n",
            name = export.name,
            body = body,
        ));
    }
    for export in rich_exports {
        out.push_str(&format!(
            "export function {name}_component(library, ...args) {{ return componentResponse(library.addon, \"{symbol}_component\", args); }}\n",
            name = export.name,
            symbol = export.symbol,
        ));
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        out.push_str(
            "export const RESOURCE_OK = 0;\nexport const RESOURCE_EXPIRED_VIEW = 1;\nexport const RESOURCE_INVALID_HANDLE = 2;\nexport const RESOURCE_BOUNDS_FAILURE = 3;\nexport const RESOURCE_ALLOCATION_FAILURE = 4;\nexport const RESOURCE_CLOSED = 5;\nexport const RESOURCE_GUEST_ERROR = 6;\nexport const RESOURCE_INTERNAL_FAILURE = 7;\n",
        );
        out.push_str(&format!(
            "function resourceCall(library, operation, ...args) {{\n    const call = library.addon[operation];\n    if (typeof call !== \"function\") throw new TypeError(\"Node-API addon has no \" + operation + \" resource export\");\n    const result = call(...args);\n    if (result === null || typeof result !== \"object\") throw new TypeError(\"Node-API \" + operation + \" returned no resource status\");\n    return result;\n}}\n\nexport class ResourceDocument {{\n    constructor(library) {{ this.library = library; this.raw = {{ slot: 0n, generation: 0n }}; this.active = false; this.error = null; }}\n    open(input) {{\n        const result = resourceCall(this.library, \"{prefix}_document_open\", Buffer.from(input));\n        this.error = result.error ?? null;\n        if (result.status === 0) {{ this.raw = {{ slot: BigInt(result.slot), generation: BigInt(result.generation) }}; this.active = true; }}\n        return result.status;\n    }}\n    bytes() {{\n        if (!this.active) return {{ status: 5, error: null }};\n        const result = resourceCall(this.library, \"{prefix}_document_bytes\", this.raw);\n        this.error = result.error ?? null;\n        return result.status === 0 ? {{ status: 0, view: {{ slot: BigInt(result.slot), generation: BigInt(result.generation) }}, error: null }} : result;\n    }}\n    at(view, index) {{\n        if (!this.active) return {{ value: 0, status: 5, error: null }};\n        const result = resourceCall(this.library, \"{prefix}_view_at\", view, Number(index));\n        this.error = result.error ?? null;\n        return {{ value: result.value ?? 0, status: result.status, error: this.error }};\n    }}\n    replace(input) {{\n        if (!this.active) return 5;\n        const result = resourceCall(this.library, \"{prefix}_document_replace\", this.raw, Buffer.from(input));\n        this.error = result.error ?? null;\n        if (result.status === 0) this.raw.generation = BigInt(result.generation ?? (this.raw.generation + 1n));\n        if (result.status === 6) this.active = false;\n        return result.status;\n    }}\n    close() {{\n        if (!this.active) return 5;\n        const result = resourceCall(this.library, \"{prefix}_document_close\", this.raw);\n        this.error = result.error ?? null;\n        if (result.status === 0 || result.status === 6) this.active = false;\n        return result.status;\n    }}\n}}\n",
            prefix = prefix,
        ));
    }
    out.push_str(&format!("\n// library: {name}\n", name = name));
    out
}

#[cfg(test)]
fn render_python(name: &str, exports: &[LibraryExport]) -> String {
    render_python_with_rich(name, exports, &[], None)
}

fn render_python_with_rich(
    name: &str,
    exports: &[LibraryExport],
    rich_exports: &[RichExport],
    resource_plan: Option<&ResourcePlan>,
) -> String {
    let mut out = String::from(
        "# Generated by Jet — D-LIB-EXPORT1=C.\nimport ctypes\nimport sys\n\nclass Library:\n    def __init__(self, path):\n        self._lib = ctypes.CDLL(path)\n",
    );
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out = out.replace(
            "class Library:",
            "class JetText(ctypes.Structure):\n    _fields_ = [(\"ptr\", ctypes.POINTER(ctypes.c_uint8)), (\"len\", ctypes.c_size_t)]\n\nclass Library:",
        );
        out.push_str("        self._lib.jet_text_free.argtypes = [JetText]\n");
        out.push_str("        self._lib.jet_text_free.restype = None\n");
    }
    for export in exports {
        out.push_str(&format!(
            "        self._lib.{symbol}.argtypes = [{types}]\n        self._lib.{symbol}.restype = {ret}\n",
            symbol = export.symbol,
            types = (0..export.params)
                .map(|_| export.scalar.python_ctypes_ty())
                .collect::<Vec<_>>()
                .join(", "),
            ret = export.scalar.python_ctypes_ty(),
        ));
        if export.scalar == ExportScalar::Text {
            out.push_str(&format!(
                "    def {name}(self, *args):\n        encoded = [arg.encode(\"utf-8\") for arg in args]\n        buffers = [ctypes.create_string_buffer(arg) for arg in encoded]\n        values = [JetText(ctypes.cast(arg, ctypes.POINTER(ctypes.c_uint8)), len(raw)) for arg, raw in zip(buffers, encoded)]\n        result = self._lib.{symbol}(*values)\n        if result.len == 0:\n            return \"\"\n        address = ctypes.cast(result.ptr, ctypes.c_void_p).value\n        if address is None or result.len > sys.maxsize or result.len > sys.maxsize - address:\n            raise ValueError(\"invalid JetText pointer-length pair\")\n        try:\n            return ctypes.string_at(result.ptr, result.len).decode(\"utf-8\")\n        finally:\n            self._lib.jet_text_free(result)\n",
                name = export.name,
                symbol = export.symbol,
            ));
        } else {
            out.push_str(&format!(
                "    def {name}(self, *args):\n        return self._lib.{symbol}(*args)\n",
                name = export.name,
                symbol = export.symbol,
            ));
        }
    }
    if !rich_exports.is_empty() || resource_plan.is_some() {
        out = out.replace(
            "import ctypes\n",
            "import ctypes\nimport json\n",
        );
        out = out.replace(
            "class Library:",
            "class JetComponent(ctypes.Structure):\n    _fields_ = [(\"ptr\", ctypes.POINTER(ctypes.c_uint8)), (\"len\", ctypes.c_size_t)]\n\nclass Library:",
        );
        let mut component_config =
            String::from("        self._lib.jet_component_free.argtypes = [JetComponent]\n        self._lib.jet_component_free.restype = None\n");
        for export in rich_exports {
            component_config.push_str(&format!(
                "        self._lib.{symbol}_component.argtypes = [JetComponent]\n        self._lib.{symbol}_component.restype = JetComponent\n",
                symbol = export.symbol,
            ));
        }
        out = out.replace(
            "        self._lib = ctypes.CDLL(path)\n",
            &format!(
                "        self._lib = ctypes.CDLL(path)\n{component_config}",
                component_config = component_config,
            ),
        );
        for export in rich_exports {
            out.push_str(&format!(
                "\ndef {name}_component(library, *values):\n    raw = json.dumps(values).encode(\"utf-8\")\n    buf = ctypes.create_string_buffer(raw)\n    result = library._lib.{symbol}_component(JetComponent(ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)), len(raw)))\n    try:\n        payload = json.loads(ctypes.string_at(result.ptr, result.len).decode(\"utf-8\"))\n    finally:\n        library._lib.jet_component_free(result)\n    if isinstance(payload, dict) and payload.get(\"ok\") is False:\n        error = RuntimeError(\"Jet component call failed\")\n        error.payload = payload.get(\"error\")\n        raise error\n    return payload[\"value\"] if isinstance(payload, dict) and \"value\" in payload else payload\n",
                name = export.name,
                symbol = export.symbol,
            ));
        }
    }
    if resource_plan.is_some() {
        let prefix = c_identifier(name);
        let upper = prefix.to_ascii_uppercase();
        out.push_str(&format!(
            "\n{upper}_OK = 0\n{upper}_EXPIRED_VIEW = 1\n{upper}_INVALID_HANDLE = 2\n{upper}_BOUNDS_FAILURE = 3\n{upper}_ALLOCATION_FAILURE = 4\n{upper}_CLOSED = 5\n{upper}_GUEST_ERROR = 6\n{upper}_INTERNAL_FAILURE = 7\n",
            upper = upper,
        ));
        let mut resource = r#"
class JetResourceDocument(ctypes.Structure):
    _fields_ = [("slot", ctypes.c_uint64), ("generation", ctypes.c_uint64)]

class JetResourceView(ctypes.Structure):
    _fields_ = [("slot", ctypes.c_uint64), ("generation", ctypes.c_uint64)]

class ResourceDocument:
    def __init__(self, library):
        self.library = library
        self.raw = JetResourceDocument()
        self.active = False
        self.error = JetComponent()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.close()
        return False

    def _clear_error(self):
        if bool(self.error.ptr):
            self.library._lib.jet_component_free(self.error)
        self.error = JetComponent()

    def error_json(self):
        if not bool(self.error.ptr):
            return None
        address = ctypes.cast(self.error.ptr, ctypes.c_void_p).value
        if address is None or self.error.len > sys.maxsize or self.error.len > sys.maxsize - address:
            raise ValueError("invalid guest error pointer-length pair")
        return json.loads(ctypes.string_at(self.error.ptr, self.error.len).decode("utf-8"))

    def open(self, input):
        self._clear_error()
        data = bytes(input)
        buffer = ctypes.create_string_buffer(data) if data else None
        pointer = ctypes.cast(buffer, ctypes.POINTER(ctypes.c_uint8)) if buffer is not None else None
        error = JetComponent()
        status = self.library._lib.@@PREFIX@@_document_open(pointer, len(data), ctypes.byref(self.raw), ctypes.byref(error))
        self.active = status == 0
        if status != 0:
            self.error = error
        return status

    def bytes(self):
        if not self.active:
            self._clear_error()
            return @@UPPER@@_CLOSED, None
        self._clear_error()
        view = JetResourceView()
        error = JetComponent()
        status = self.library._lib.@@PREFIX@@_document_bytes(ctypes.byref(self.raw), ctypes.byref(view), ctypes.byref(error))
        if status != 0:
            self.error = error
        return status, view if status == 0 else None

    def at(self, view, index):
        if not self.active:
            self._clear_error()
            return 0, @@UPPER@@_CLOSED
        self._clear_error()
        if not isinstance(index, int) or index < 0 or index > sys.maxsize:
            return 0, @@UPPER@@_BOUNDS_FAILURE
        value = ctypes.c_int64()
        error = JetComponent()
        status = self.library._lib.@@PREFIX@@_view_at(ctypes.byref(view), index, ctypes.byref(value), ctypes.byref(error))
        if status != 0:
            self.error = error
        return value.value, status

    def replace(self, input):
        if not self.active:
            self._clear_error()
            return @@UPPER@@_CLOSED
        self._clear_error()
        data = bytes(input)
        buffer = ctypes.create_string_buffer(data) if data else None
        pointer = ctypes.cast(buffer, ctypes.POINTER(ctypes.c_uint8)) if buffer is not None else None
        error = JetComponent()
        status = self.library._lib.@@PREFIX@@_document_replace(ctypes.byref(self.raw), pointer, len(data), ctypes.byref(error))
        if status == @@UPPER@@_GUEST_ERROR:
            self.active = False
        if status != 0:
            self.error = error
        return status

    def close(self):
        if not self.active:
            self._clear_error()
            return @@UPPER@@_CLOSED
        self._clear_error()
        error = JetComponent()
        status = self.library._lib.@@PREFIX@@_document_close(ctypes.byref(self.raw), ctypes.byref(error))
        if status in (0, @@UPPER@@_GUEST_ERROR):
            self.active = False
        if status != 0:
            self.error = error
        return status

"#.to_string();
        resource = resource.replace("@@PREFIX@@", &prefix);
        resource = resource.replace("@@UPPER@@", &upper);
        let mut config = r#"        self._lib.@@PREFIX@@_document_open.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t, ctypes.POINTER(JetResourceDocument), ctypes.POINTER(JetComponent)]
        self._lib.@@PREFIX@@_document_open.restype = ctypes.c_int
        self._lib.@@PREFIX@@_document_bytes.argtypes = [ctypes.POINTER(JetResourceDocument), ctypes.POINTER(JetResourceView), ctypes.POINTER(JetComponent)]
        self._lib.@@PREFIX@@_document_bytes.restype = ctypes.c_int
        self._lib.@@PREFIX@@_view_at.argtypes = [ctypes.POINTER(JetResourceView), ctypes.c_size_t, ctypes.POINTER(ctypes.c_int64), ctypes.POINTER(JetComponent)]
        self._lib.@@PREFIX@@_view_at.restype = ctypes.c_int
        self._lib.@@PREFIX@@_document_replace.argtypes = [ctypes.POINTER(JetResourceDocument), ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t, ctypes.POINTER(JetComponent)]
        self._lib.@@PREFIX@@_document_replace.restype = ctypes.c_int
        self._lib.@@PREFIX@@_document_close.argtypes = [ctypes.POINTER(JetResourceDocument), ctypes.POINTER(JetComponent)]
        self._lib.@@PREFIX@@_document_close.restype = ctypes.c_int
"#.to_string();
        config = config.replace("@@PREFIX@@", &prefix);
        out = out.replace(
            "        self._lib = ctypes.CDLL(path)\n",
            &format!(
                "        self._lib = ctypes.CDLL(path)\n{config}",
                config = config,
            ),
        );
        out.push_str(&resource);
    }
    out.push_str(&format!(
        "\ndef load(path):\n    return Library(path)\n\n# library: {name}\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_foreign_projections_use_the_native_symbol() {
        let exports = vec![LibraryExport {
            name: "on_tick".to_string(),
            symbol: "on_tick".to_string(),
            callee: "on_tick".to_string(),
            scalar: ExportScalar::Int,
            params: 1,
            conventions: vec![MirAccess::Read],
            ownership: ExportOwnership {
                parameters: vec![MirOwnership::from_access(MirAccess::Read)],
                return_views: None,
            },
        }];
        let header = render_c_header("flightlog", &exports);
        let python = render_python("flightlog", &exports);
        assert!(header.contains("int64_t on_tick(int64_t p0);"));
        assert!(python.contains("self._lib.on_tick"));
        assert_eq!(exports[0].symbol, "on_tick");
    }

    #[test]
    fn generated_text_projections_check_pointer_length_and_utf8() {
        let exports = vec![LibraryExport {
            name: "greet".to_string(),
            symbol: "greet".to_string(),
            callee: "greet".to_string(),
            scalar: ExportScalar::Text,
            params: 1,
            conventions: vec![MirAccess::Read],
            ownership: ExportOwnership {
                parameters: vec![MirOwnership::from_access(MirAccess::Read)],
                return_views: None,
            },
        }];
        let python = render_python("flightlog", &exports);
        let swift = render_swift(&exports);
        assert!(python.contains("address = ctypes.cast(result.ptr, ctypes.c_void_p).value"));
        assert!(python.contains("result.len > sys.maxsize - address"));
        assert!(python.contains(".decode(\"utf-8\")"));
        assert!(swift.contains("case invalidPointerLength"));
        assert!(swift.contains("case invalidUTF8"));
        assert!(swift.contains("String(bytes: bytes, encoding: .utf8)"));
        assert!(swift.contains("mutating func release()"));
        assert!(swift.contains("jet_text_free(self)"));
        assert!(swift.contains("self.ptr = nil"));
        assert!(swift.contains("self.len = 0"));
    }
}

fn render_swift(exports: &[LibraryExport]) -> String {
    let mut out = String::from("// Generated by Jet — D-LIB-EXPORT1=C.\nimport Foundation\n\n");
    if exports
        .iter()
        .any(|export| export.scalar == ExportScalar::Text)
    {
        out.push_str(
            "public enum JetTextError: Error { case invalidPointerLength; case invalidUTF8 }\n\npublic struct JetText {\n    public var ptr: UnsafePointer<UInt8>?\n    public var len: Int\n\n    private func checkedBuffer() throws -> UnsafeBufferPointer<UInt8> {\n        guard len >= 0 else { throw JetTextError.invalidPointerLength }\n        if len == 0 { return UnsafeBufferPointer(start: nil, count: 0) }\n        guard let ptr else { throw JetTextError.invalidPointerLength }\n        let address = UInt(bitPattern: UnsafeRawPointer(ptr))\n        guard UInt(len) <= UInt.max - address else { throw JetTextError.invalidPointerLength }\n        return UnsafeBufferPointer(start: ptr, count: len)\n    }\n\n    public func decode() throws -> String {\n        let bytes = try checkedBuffer()\n        guard let value = String(bytes: bytes, encoding: .utf8) else { throw JetTextError.invalidUTF8 }\n        return value\n    }\n\n    public mutating func release() throws {\n        _ = try checkedBuffer()\n        jet_text_free(self)\n        self.ptr = nil\n        self.len = 0\n    }\n}\n\n@_silgen_name(\"jet_text_free\") private func jet_text_free(_ value: JetText)\n\n",
        );
    }
    for export in exports {
        let params = (0..export.params)
            .map(|index| format!("_ p{index}: {}", export.scalar.swift_ty()))
            .collect::<Vec<_>>();
        let args = (0..export.params)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>();
        out.push_str(&jet_name_format!(
            "@_silgen_name(\"{symbol}\") private func {name_prefix}{name}({params}) -> {ret}\npublic func {name}({params}) -> {ret} {{ {name_prefix}{name}({args}) }}\n\n",
            symbol = export.symbol,
            name = export.name,
            params = params.join(", "),
            ret = export.scalar.swift_ty(),
            args = args.join(", "),
        ));
    }
    out
}

fn library_text_helpers() -> String {
    let read_text = mangle_generated("library_read_text");
    let return_text = mangle_generated("library_return_text");
    let text_free = mangle_generated("library_text_free");
    LIBRARY_TEXT_HELPERS
        .replace("JET_LIBRARY_READ_TEXT", &read_text)
        .replace("JET_LIBRARY_RETURN_TEXT", &return_text)
        .replace("JET_LIBRARY_TEXT_FREE", &text_free)
}

const LIBRARY_TEXT_HELPERS: &str = r#"
#[repr(C)]
pub struct JetText {
    pub ptr: *const u8,
    pub len: usize,
}

fn JET_LIBRARY_READ_TEXT(value: JetText) -> String {
    if value.len == 0 {
        return String::new();
    }
    if value.ptr.is_null()
        || value.len > isize::MAX as usize
        || (value.ptr as usize).checked_add(value.len).is_none()
    {
        panic!("invalid JetText pointer-length pair");
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| panic!("JetText contains invalid UTF-8"))
}

fn JET_LIBRARY_RETURN_TEXT(value: String) -> JetText {
    let bytes = value.into_bytes();
    if bytes.is_empty() {
        return JetText { ptr: std::ptr::null(), len: 0 };
    }
    let len = bytes.len();
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *const u8;
    JetText { ptr, len }
}

#[export_name = "jet_text_free"]
pub extern "C" fn JET_LIBRARY_TEXT_FREE(value: JetText) {
    jet_ffi_callback_boundary(|| {
        if value.len == 0 {
            return;
        }
        if value.ptr.is_null()
            || value.len > isize::MAX as usize
            || (value.ptr as usize).checked_add(value.len).is_none()
        {
            panic!("invalid JetText pointer-length pair");
        }
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                value.ptr as *mut u8,
                value.len,
            )));
        }
    });
}
"#;
