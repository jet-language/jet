//! The typed export shapes shared by native Library and sandbox outputs.
//!
//! Artifact emission consumes the selected checked MIR plan and its export
//! rows. Each row references a `MirFunction`; the function row supplies the
//! scalar/component signature and access modes while the plan supplies the
//! external symbol. This module has no source-AST ingress or backend policy
//! selection.

use std::collections::{BTreeMap, BTreeSet};

use jet_foundation::MIR::{
    require_canonical_mir_optimization, MirAccess, MirArtifactId, MirArtifactKind, MirArtifactPlan,
    MirFailureCarrier, MirFunctionForm, MirOwnership, MirProgram, MirType, MirTypeDefKind,
    MirTypeKind, MirTypeId, MirViewProvenance,
};
use jet_foundation::Names::mangle_path;
use jet_foundation::Shape::ShapeProjectionKind;

/// Scalar types admitted at the native Library boundary and as the
/// compatibility projection of a sandbox export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScalar {
    Int,
    Float,
    Bool,
    Text,
}

impl ExportScalar {
    pub(crate) fn rust_ty(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Float => "f64",
            Self::Bool => "bool",
            Self::Text => "String",
        }
    }

    pub(crate) fn wit_ty(self) -> &'static str {
        match self {
            Self::Int => "s64",
            Self::Float => "f64",
            Self::Bool => "bool",
            Self::Text => "string",
        }
    }

    pub(crate) fn c_ty(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
            Self::Bool => "bool",
            Self::Text => "JetText",
        }
    }

    pub(crate) fn python_ctypes_ty(self) -> &'static str {
        match self {
            Self::Int => "ctypes.c_int64",
            Self::Float => "ctypes.c_double",
            Self::Bool => "ctypes.c_bool",
            Self::Text => "JetText",
        }
    }

    pub(crate) fn swift_ty(self) -> &'static str {
        match self {
            Self::Int => "Int64",
            Self::Float => "Double",
            Self::Bool => "Bool",
            Self::Text => "JetText",
        }
    }
}

/// The canonical Component Model descriptors live in foundation MIR so every
/// lowering and execution tier consumes one semantic row.
pub use jet_foundation::MIR::{ComponentSignatureDescriptor, ComponentTypeDescriptor};
/// MIR type shape used while an artifact export is being assembled.  The
/// semantic descriptor below is the canonical cross-tier representation.
#[derive(Debug, Clone)]
pub struct ComponentSignature {
    pub params: Vec<MirType>,
    pub result: MirType,
}

/// Checked ownership and returned-view facts carried with every export row.
/// Library projections consume this fact instead of re-deriving lifetime
/// semantics from names or generated source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOwnership {
    pub parameters: Vec<MirOwnership>,
    pub return_views: Option<BTreeMap<Vec<String>, MirViewProvenance>>,
}

/// One checked export projected for native and component artifact adapters.
#[derive(Debug, Clone)]
pub struct ExportFunction {
    pub name: String,
    pub symbol: String,
    pub callee: String,
    pub scalar: Option<ExportScalar>,
    pub component: Option<ComponentSignature>,
    pub params: Vec<MirAccess>,
    pub ownership: ExportOwnership,
    pub failure: MirFailureCarrier,
}
fn mir_scalar(ty: &MirType) -> Option<ExportScalar> {
    match ty.kind() {
        MirTypeKind::Int => Some(ExportScalar::Int),
        MirTypeKind::Float => Some(ExportScalar::Float),
        MirTypeKind::Bool => Some(ExportScalar::Bool),
        MirTypeKind::String => Some(ExportScalar::Text),
        _ => None,
    }
}

fn component_type_descriptor(
    program: &MirProgram,
    ty: &MirType,
    seen: &mut BTreeSet<MirTypeId>,
) -> Option<ComponentTypeDescriptor> {
    match ty.kind() {
        MirTypeKind::Int => Some(ComponentTypeDescriptor::Int),
        MirTypeKind::Float => Some(ComponentTypeDescriptor::Float),
        MirTypeKind::Bool => Some(ComponentTypeDescriptor::Bool),
        MirTypeKind::String => Some(ComponentTypeDescriptor::String),
        MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
            Some(ComponentTypeDescriptor::List(Box::new(
                component_type_descriptor(program, inner, seen)?,
            )))
        }
        MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. } => component_type_descriptor(program, inner, seen),
        MirTypeKind::Option(inner) => Some(ComponentTypeDescriptor::Option(Box::new(
            component_type_descriptor(program, inner, seen)?,
        ))),
        MirTypeKind::Result { ok, err } => Some(ComponentTypeDescriptor::Result {
            ok: Box::new(component_type_descriptor(program, ok, seen)?),
            err: Box::new(component_type_descriptor(program, err, seen)?),
        }),
        MirTypeKind::Apply { name: nominal, .. } => {
            if !seen.insert(nominal.id) {
                return None;
            }
            let result = program
                .types
                .iter()
                .find(|definition| definition.id == nominal.id)
                .and_then(|definition| match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. } => Some(ComponentTypeDescriptor::Record {
                        name: definition.name.clone(),
                        fields: fields
                            .iter()
                            .enumerate()
                            .filter(|(_, field)| !field.skip && !field.computed)
                            .map(|(index, field)| {
                                component_type_descriptor(program, &field.ty, seen).map(|ty| {
                                    (
                                        index,
                                        field
                                            .shape_names
                                            .name_for(ShapeProjectionKind::Json)
                                            .expect("checked MIR field is missing its JSON shape name")
                                            .to_string(),
                                        ty,
                                    )
                                })
                            })
                            .collect::<Option<Vec<_>>>()?,
                    }),
                    MirTypeDefKind::Alias { target } => {
                        component_type_descriptor(program, target, seen)
                    }
                    _ => None,
                });
            seen.remove(&nominal.id);
            result
        }
        MirTypeKind::Map { .. }
        | MirTypeKind::Shared(_)
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Tuple(_)
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Union(_)
        | MirTypeKind::Quantity { .. }
        | MirTypeKind::Measure(_)
        | MirTypeKind::Char => None,
    }
}

/// Build the canonical checked Component Model shape. Every consumer uses
/// this result rather than independently walking MIR definitions.
pub fn component_descriptor(
    program: &MirProgram,
    signature: &ComponentSignature,
) -> Option<ComponentSignatureDescriptor> {
    let mut seen = BTreeSet::new();
    Some(ComponentSignatureDescriptor {
        params: signature
            .params
            .iter()
            .map(|ty| component_type_descriptor(program, ty, &mut seen))
            .collect::<Option<Vec<_>>>()?,
        result: component_type_descriptor(program, &signature.result, &mut seen)?,
    })
}

fn mir_type_contains_view(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Apply { name, .. } if matches!(name.name.as_str(), "View" | "ViewMut") => true,
        MirTypeKind::List(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. } => mir_type_contains_view(inner),
        MirTypeKind::Result { ok, err } => {
            mir_type_contains_view(ok) || mir_type_contains_view(err)
        }
        _ => false,
    }
}

fn component_signature(
    program: &MirProgram,
    function: &jet_foundation::MIR::MirFunction,
    allow_library_views: bool,
) -> Option<ComponentSignature> {
    let signature = ComponentSignature {
        params: function
            .params
            .iter()
            .map(|param| param.ty.clone())
            .collect(),
        result: function.return_type.clone(),
    };
    if component_descriptor(program, &signature).is_some()
        || (allow_library_views
            && (matches!(
                &function.failure,
                MirFailureCarrier::Result { .. }
            )
                || signature.params.iter().any(mir_type_contains_view)
                || mir_type_contains_view(&signature.result)))
    {
        Some(signature)
    } else {
        None
    }
}

fn scalar_signature(
    params: &[jet_foundation::MIR::MirParam],
    return_type: &MirType,
) -> Option<ExportScalar> {
    let scalar = mir_scalar(return_type)?;
    params
        .iter()
        .all(|param| mir_scalar(&param.ty) == Some(scalar))
        .then_some(scalar)
}

fn export_function(
    program: &MirProgram,
    export: &jet_foundation::MIR::MirExport,
    allow_component: bool,
    allow_library_views: bool,
) -> Option<ExportFunction> {
    let function = program
        .functions
        .iter()
        .find(|function| function.id == export.function)?;
    if !matches!(function.form, MirFunctionForm::TopLevel) || !function.capture_params.is_empty() {
        return None;
    }
    let scalar = scalar_signature(&function.params, &function.return_type);
    let component = allow_component
        .then(|| component_signature(program, function, allow_library_views))
        .flatten();
    if scalar.is_none() && component.is_none() {
        return None;
    }
    Some(ExportFunction {
        name: function.name.clone(),
        symbol: export.symbol.clone(),
        callee: mangle_path(&function.key),
        scalar,
        component,
        params: function.params.iter().map(|param| param.access).collect(),
        ownership: ExportOwnership {
            parameters: function.params.iter().map(|param| param.ownership).collect(),
            return_views: function.return_view_provenance.clone(),
        },
        failure: function.failure.clone(),
    })
}

pub(crate) fn selected_artifact(
    program: &MirProgram,
    artifact_id: MirArtifactId,
) -> &MirArtifactPlan {
    require_canonical_mir_optimization(program)
        .unwrap_or_else(|error| panic!("MIR artifact emission requires canonical optimization: {error}"));
    program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .unwrap_or_else(|| panic!("MIR artifact ID {:?} has no plan row", artifact_id))
}

/// Collect the exact export list from the selected MIR artifact plan.
///
/// Export selection is a checked MIR fact. This function never reads source
/// AST or reconstructs visibility/target policy from the program.
pub fn export_surface(program: &MirProgram, artifact_id: MirArtifactId) -> Vec<ExportFunction> {
    let artifact = selected_artifact(program, artifact_id);
    let allow_component = matches!(
        artifact.kind,
        MirArtifactKind::NativeLibrary | MirArtifactKind::SandboxPlugin
    );
    let allow_library_views = artifact.kind == MirArtifactKind::NativeLibrary;
    artifact
        .exports
        .iter()
        .filter_map(|export| {
            export_function(
                program,
                export,
                allow_component,
                allow_library_views,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ExportScalar;

    #[test]
    fn scalar_lowering_table_covers_both_boundaries() {
        let rows = [
            (ExportScalar::Int, "i64", "s64", "int64_t"),
            (ExportScalar::Float, "f64", "f64", "double"),
            (ExportScalar::Bool, "bool", "bool", "bool"),
            (ExportScalar::Text, "String", "string", "JetText"),
        ];
        for (scalar, rust, wit, c) in rows {
            assert_eq!(scalar.rust_ty(), rust);
            assert_eq!(scalar.wit_ty(), wit);
            assert_eq!(scalar.c_ty(), c);
        }
    }
}
