//! Checked request/result transport for the private generated compiler entry.
//!
//! The retained compiler MIR and the emitted binding descriptor are the sole
//! shape authorities. Ordinary aggregates are copied only as required to cross
//! the entry ABI. Shared, trait-object, and callable leaves go through the
//! invocation-local NativeAdapter hook; they are never serialized as IDs or
//! reconstructed from display text. Helper roots are recursively carried as
//! checked MIR aggregates; only physical leaves use owner-issued carriers.

use crate::compiler_bootstrap_host::{
    BootstrapBindingDescriptor, BootstrapCodecSymbols, BootstrapHostCodecError,
};
use jet_foundation::MIR::{
    MirConstKey, MirCoreOwner, MirFieldId, MirFunctionId, MirFunctionKind, MirNominalRef,
    MirProgram, MirRuntimeValue, MirTraitMethod, MirType, MirTypeDef, MirTypeDefKind, MirTypeId,
    MirTypeKind, MirVariantPayload,
};
use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;

const MAX_VALUE_DEPTH: usize = 256;

pub(crate) type BootstrapEntrySharedPayloadEncoder<T, P> = fn(
    &T,
    &MirType,
    &std::sync::Arc<MirProgram>,
    &BootstrapEntryHostTypeShape,
    usize,
    &[String],
    &[BootstrapEntryHostProjection],
    &P,
) -> Result<MirRuntimeValue, String>;

pub(crate) type BootstrapEntrySharedPayloadDecoder<T, P> = fn(
    MirRuntimeValue,
    &MirType,
    &std::sync::Arc<MirProgram>,
    &BootstrapEntryHostTypeShape,
    usize,
    &[String],
    &[BootstrapEntryHostProjection],
    &P,
) -> Result<T, String>;

#[cfg(jet_bootstrap_compiler_artifact)]
pub(crate) trait BootstrapEntrySharedMarshaller<O, T, P>: Clone + Send + Sync + 'static
where
    O: crate::JetSharedPhysicalOwnerApi,
    T: 'static,
    P: BootstrapEntryPhysicalBindings,
{
    fn encode_payload(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        source_value: &T,
    ) -> Result<MirRuntimeValue, String>;

    fn decode_payload(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        runtime_value: MirRuntimeValue,
    ) -> Result<T, String>;
}

pub(crate) trait BootstrapEntryPhysicalBindings: Clone + Send + Sync + 'static {
    // Shared marshalling is bounded on the generated crate's flat Prelude
    // owner trait, which exists only inside the compiler artifact.
    #[cfg(jet_bootstrap_compiler_artifact)]
    type SharedMarshaller<O, T>: BootstrapEntrySharedMarshaller<O, T, Self>
    where
        O: crate::JetSharedPhysicalOwnerApi,
        T: 'static;

    /// Exact Source-produced shape for one Shared payload type.
    fn shared_payload_shape(
        &self,
        checked_type: &MirType,
    ) -> Result<BootstrapEntryHostTypeShape, String>;

    /// Build a root-scoped, cycle-free marshalling service. It may retain
    /// `owner` only as its weak physical capability and must not retain this
    /// invocation binding, an interop root, or a logical alias lease. The
    /// generated converters are static function pointers; an invocation
    /// resolver supplies a short-lived binding whenever they run.
    #[cfg(jet_bootstrap_compiler_artifact)]
    fn shared_marshaller<O: crate::JetSharedPhysicalOwnerApi, T: 'static>(
        &self,
        owner: O,
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        encode: BootstrapEntrySharedPayloadEncoder<T, Self>,
        decode: BootstrapEntrySharedPayloadDecoder<T, Self>,
    ) -> Result<Self::SharedMarshaller<O, T>, String>;

    fn encode<T: 'static>(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        program: &MirProgram,
        source_value: &T,
    ) -> Result<Option<MirRuntimeValue>, String>;

    fn encode_shared(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        program: &MirProgram,
        physical_identity: usize,
        root: jet_jit::SourceSharedInterop::SourceSharedInterop,
        payload_shape: &BootstrapEntryHostTypeShape,
    ) -> Result<Option<MirRuntimeValue>, String>;

    fn decode<T: 'static>(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        program: &MirProgram,
        runtime_value: MirRuntimeValue,
    ) -> Result<Option<T>, String>;

    fn encode_shaped<T: 'static>(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        source_value: &T,
    ) -> Result<Option<MirRuntimeValue>, String>;

    fn decode_shaped<T: 'static>(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        runtime_value: MirRuntimeValue,
    ) -> Result<Option<T>, String>;

    /// Borrow the checked Source machine carrier in-place for a NativeInterface
    /// WRITE call. No Source values are cloned, retained, disposed, or moved.
    fn borrow_interpreter_machine_write<'a>(
        &self,
        value: &'a mut MirRuntimeValue,
        checked_type: &'a MirType,
        shape: &'a BootstrapEntryHostTypeShape,
    ) -> Result<BootstrapEntryMachineWriteBorrow<'a>, String>
    where
        Self: Sized,
    {
        BootstrapEntryMachineWriteBorrow::new(value, checked_type, shape)
    }

}

/// Compiler-independent representation of the exact guest type-shape graph
/// supplied by the running Source evaluator.
#[derive(Clone, Debug)]
pub(crate) struct BootstrapEntryHostTypeShape {
    pub(crate) root: usize,
    pub(crate) nodes: Vec<BootstrapEntryHostTypeNode>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapEntryHostProjection {
    Present,
    Failed,
    List(usize),
    MapKey(usize),
    MapValue(usize),
    StructField(String),
    EnumArg(usize),
    HandlePayload,
    Capture(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct BootstrapEntryHostTypeField {
    pub(crate) name: String,
    pub(crate) node: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct BootstrapEntryHostTypeVariant {
    pub(crate) name: String,
    pub(crate) args: Vec<(Option<String>, usize)>,
}

#[derive(Clone, Debug)]
pub(crate) enum BootstrapEntryHostOwner {
    Cursor,
    Declared(MirType),
    Core { fact: MirCoreOwner, ty: MirType },
    NativeCallable { key: String, callable_type: MirType },
    NativeInterface { receiver_type: MirType },
}

#[derive(Clone, Debug)]
pub(crate) enum BootstrapEntryHostTypeNode {
    Scalar(MirType),
    Option(usize),
    Result { ok: usize, error: usize },
    List(usize),
    Map { key: usize, value: usize },
    Tuple(Vec<BootstrapEntryHostTypeField>),
    Struct {
        type_id: MirTypeId,
        type_name: String,
        args: Vec<usize>,
        fields: Vec<BootstrapEntryHostTypeField>,
    },
    Enum {
        type_id: MirTypeId,
        type_name: String,
        args: Vec<usize>,
        variants: Vec<BootstrapEntryHostTypeVariant>,
    },
    Closure {
        function: MirFunctionId,
        captures: Vec<usize>,
        ty: MirType,
    },
    Handle { ty: MirType, owner: BootstrapEntryHostOwner },
}
pub(crate) trait BootstrapEntryMachineCoreOwnerRows {
    /// The machine program's registered Core owner rows, as host facts.
    fn core_owner_rows(&self) -> Result<Vec<MirCoreOwner>, String>;
}

pub(crate) enum BootstrapEntryMachineAccess<'a, T> {
    Typed(&'a mut T),
    Mir(BootstrapEntryMachineWriteBorrow<'a>),
}

impl<'a, T> BootstrapEntryMachineAccess<'a, T> {
    pub(crate) fn typed_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Typed(machine) => Some(&mut **machine),
            Self::Mir(_) => None,
        }
    }

    pub(crate) fn mir_mut(&mut self) -> Option<&mut BootstrapEntryMachineWriteBorrow<'a>> {
        match self {
            Self::Typed(_) => None,
            Self::Mir(machine) => Some(machine),
        }
    }
}

/// Borrowed, checked view of the live Source machine carried in a native
/// interface call. Every projection refers into the original MIR value.
pub(crate) struct BootstrapEntryMachineWriteBorrow<'a> {
    value: &'a mut MirRuntimeValue,
    checked_type: &'a MirType,
    shape: &'a BootstrapEntryHostTypeShape,
    shape_node: usize,
}

impl<'a> BootstrapEntryMachineWriteBorrow<'a> {
    pub(crate) fn new(
        value: &'a mut MirRuntimeValue,
        checked_type: &'a MirType,
        shape: &'a BootstrapEntryHostTypeShape,
    ) -> Result<Self, String> {
        bootstrap_entry_validate_shape(shape)?;
        let shape_type = bootstrap_entry_shape_node_type(shape, shape.root, 0)?;
        if !bootstrap_entry_shape_type_matches(&shape_type, checked_type) {
            return Err("borrowed Source machine shape differs from its checked MIR type".to_string());
        }
        bootstrap_entry_validate_shaped_runtime(value, checked_type, shape, shape.root, 0)?;
        Ok(Self {
            value,
            checked_type,
            shape,
            shape_node: shape.root,
        })
    }

    pub(crate) fn value(&self) -> &MirRuntimeValue {
        self.value
    }

    pub(crate) fn value_mut(&mut self) -> &mut MirRuntimeValue {
        self.value
    }

    pub(crate) fn checked_type(&self) -> &MirType {
        self.checked_type
    }

    pub(crate) fn shape(&self) -> &BootstrapEntryHostTypeShape {
        self.shape
    }

    pub(crate) fn shape_node(&self) -> usize {
        self.shape_node
    }

    pub(crate) fn field_shape_node(&self, name: &str) -> Result<usize, String> {
        let fields = match self.shape.nodes.get(self.shape_node) {
            Some(BootstrapEntryHostTypeNode::Tuple(fields))
            | Some(BootstrapEntryHostTypeNode::Struct { fields, .. }) => fields,
            _ => return Err("borrowed Source machine is not a checked record".to_string()),
        };
        fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.node)
            .ok_or_else(|| format!("checked Source machine has no field `{name}`"))
    }
    pub(crate) fn field_type_path(&self, path: &[&str]) -> Result<MirType, String> {
        let mut node = self.shape_node;
        for name in path {
            let fields = match self.shape.nodes.get(node) {
                Some(BootstrapEntryHostTypeNode::Tuple(fields))
                | Some(BootstrapEntryHostTypeNode::Struct { fields, .. }) => fields,
                _ => return Err("borrowed Source field path crosses a non-record".to_string()),
            };
            node = fields
                .iter()
                .find(|field| field.name == *name)
                .map(|field| field.node)
                .ok_or_else(|| format!("checked Source machine has no field `{name}`"))?;
        }
        bootstrap_entry_shape_node_type(self.shape, node, 0)
    }

    pub(crate) fn field_mut(&mut self, name: &str) -> Result<&mut MirRuntimeValue, String> {
        let (_, value) =
            bootstrap_entry_project_runtime_field_mut(self.shape, &mut *self.value, self.shape_node, name)?;
        Ok(value)
    }

    pub(crate) fn projection_mut_fields(
        &mut self,
        path: &[&str],
    ) -> Result<(usize, &mut MirRuntimeValue), String> {
        let shape = self.shape;
        let mut shape_node = self.shape_node;
        let mut value = &mut *self.value;
        for name in path {
            let (next_node, next_value) =
                bootstrap_entry_project_runtime_field_mut(shape, value, shape_node, name)?;
            shape_node = next_node;
            value = next_value;
        }
        Ok((shape_node, value))
    }

    pub(crate) fn projection_mut(
        &mut self,
        path: &[BootstrapEntryHostProjection],
    ) -> Result<(usize, &mut MirRuntimeValue), String> {
        bootstrap_entry_project_runtime_mut(
            self.shape,
            &mut *self.value,
            self.shape_node,
            path,
        )
    }
}

fn bootstrap_entry_project_runtime_field_mut<'a>(
    shape: &BootstrapEntryHostTypeShape,
    value: &'a mut MirRuntimeValue,
    shape_node: usize,
    name: &str,
) -> Result<(usize, &'a mut MirRuntimeValue), String> {
    let fields = match shape.nodes.get(shape_node) {
        Some(BootstrapEntryHostTypeNode::Tuple(fields))
        | Some(BootstrapEntryHostTypeNode::Struct { fields, .. }) => fields,
        _ => return Err("borrowed Source projection is not a checked record".to_string()),
    };
    let Some(index) = fields.iter().position(|field| field.name == name) else {
        return Err(format!("checked Source record has no field `{name}`"));
    };
    let expected_type_name = match &shape.nodes[shape_node] {
        BootstrapEntryHostTypeNode::Tuple(_) => "Tuple",
        BootstrapEntryHostTypeNode::Struct { type_name, .. } => type_name,
        _ => unreachable!(),
    };
    let MirRuntimeValue::Struct {
        type_name,
        fields: values,
    } = value
    else {
        return Err("borrowed Source value is not a runtime record".to_string());
    };
    if type_name != expected_type_name || values.len() != fields.len() {
        return Err("borrowed Source record shape differs".to_string());
    }
    for (expected, (actual_name, _)) in fields.iter().zip(values.iter()) {
        if expected.name != *actual_name {
            return Err("borrowed Source record field name/order differs".to_string());
        }
    }
    Ok((fields[index].node, &mut values[index].1))
}

fn bootstrap_entry_project_runtime_mut<'a>(
    shape: &BootstrapEntryHostTypeShape,
    mut value: &'a mut MirRuntimeValue,
    mut shape_node: usize,
    path: &[BootstrapEntryHostProjection],
) -> Result<(usize, &'a mut MirRuntimeValue), String> {
    for projection in path {
        let node = shape
            .nodes
            .get(shape_node)
            .ok_or_else(|| "borrowed Source projection has an invalid shape node".to_string())?;
        let (next_node, next_value) = match (node, projection, &mut *value) {
            (
                BootstrapEntryHostTypeNode::Option(child),
                BootstrapEntryHostProjection::Present,
                MirRuntimeValue::Present(value),
            )
            | (
                BootstrapEntryHostTypeNode::Result { ok: child, .. },
                BootstrapEntryHostProjection::Present,
                MirRuntimeValue::Present(value),
            ) => (*child, value.as_mut()),
            (
                BootstrapEntryHostTypeNode::Result { error: child, .. },
                BootstrapEntryHostProjection::Failed,
                MirRuntimeValue::FailedTold(value),
            ) => (*child, value.as_mut()),
            (
                BootstrapEntryHostTypeNode::List(child),
                BootstrapEntryHostProjection::List(index),
                MirRuntimeValue::List(values),
            ) => (
                *child,
                values.get_mut(*index).ok_or_else(|| {
                    "borrowed Source list projection index is out of range".to_string()
                })?,
            ),
            (
                BootstrapEntryHostTypeNode::Map { value: child, .. },
                BootstrapEntryHostProjection::MapValue(index),
                MirRuntimeValue::Map(values),
            ) => (
                *child,
                &mut values
                    .get_mut(*index)
                    .ok_or_else(|| {
                        "borrowed Source map projection index is out of range".to_string()
                    })?
                    .1,
            ),
            (
                BootstrapEntryHostTypeNode::Map { .. },
                BootstrapEntryHostProjection::MapKey(_),
                MirRuntimeValue::Map(_),
            ) => {
                return Err("MIR map keys are immutable constants, not borrowed runtime values".to_string());
            }
            (
                node @ (BootstrapEntryHostTypeNode::Tuple(_)
                | BootstrapEntryHostTypeNode::Struct { .. }),
                BootstrapEntryHostProjection::StructField(name),
                MirRuntimeValue::Struct {
                    type_name,
                    fields: values,
                },
            ) => {
                let shape_fields = match node {
                    BootstrapEntryHostTypeNode::Tuple(fields) => fields,
                    BootstrapEntryHostTypeNode::Struct {
                        type_name: expected,
                        fields,
                        ..
                    } if expected == type_name => fields,
                    _ => return Err("borrowed Source record type name differs".to_string()),
                };
                if values.len() != shape_fields.len() {
                    return Err("borrowed Source record field count differs".to_string());
                }
                for (expected, (actual, _)) in shape_fields.iter().zip(values.iter()) {
                    if expected.name != *actual {
                        return Err("borrowed Source record field name/order differs".to_string());
                    }
                }
                let index = shape_fields
                    .iter()
                    .position(|field| field.name == *name)
                    .ok_or_else(|| format!("borrowed Source record has no field `{name}`"))?;
                (shape_fields[index].node, &mut values[index].1)
            }
            (
                BootstrapEntryHostTypeNode::Enum {
                    type_name: expected_type,
                    variants,
                    ..
                },
                BootstrapEntryHostProjection::EnumArg(index),
                MirRuntimeValue::Enum {
                    type_name,
                    variant,
                    args,
                },
            ) => {
                if type_name != expected_type {
                    return Err("borrowed Source enum type name differs".to_string());
                }
                let expected = variants
                    .iter()
                    .find(|row| row.name == *variant)
                    .ok_or_else(|| format!("borrowed Source enum variant `{type_name}::{variant}` is absent"))?;
                let (expected_name, child) = expected
                    .args
                    .get(*index)
                    .ok_or_else(|| "borrowed Source enum argument is out of range".to_string())?;
                if args.len() != expected.args.len() || args[*index].0 != *expected_name {
                    return Err("borrowed Source enum payload name/order differs".to_string());
                }
                (*child, &mut args[*index].1)
            }
            (
                BootstrapEntryHostTypeNode::Closure { function, captures, .. },
                BootstrapEntryHostProjection::Capture(index),
                MirRuntimeValue::Closure(closure),
            ) => {
                if closure.function != *function || closure.captures.len() != captures.len() {
                    return Err("borrowed Source closure capture layout differs".to_string());
                }
                (
                    *captures.get(*index).ok_or_else(|| {
                        "borrowed Source closure capture is out of range".to_string()
                    })?,
                    closure.captures.get_mut(*index).ok_or_else(|| {
                        "borrowed Source closure capture is out of range".to_string()
                    })?,
                )
            }
            _ => return Err("borrowed Source projection does not match its checked value shape".to_string()),
        };
        shape_node = next_node;
        value = next_value;
    }
    Ok((shape_node, value))
}


pub(crate) fn bootstrap_entry_shape_node_type(
    shape: &BootstrapEntryHostTypeShape,
    node: usize,
    depth: usize,
) -> Result<MirType, String> {
    if depth >= MAX_VALUE_DEPTH {
        return Err("compiler-entry host type shape exceeds checked nesting bound".to_string());
    }
    let node = shape
        .nodes
        .get(node)
        .ok_or_else(|| "compiler-entry host type shape has an invalid node reference".to_string())?;
    let child = |node| bootstrap_entry_shape_node_type(shape, node, depth + 1);
    Ok(match node {
        BootstrapEntryHostTypeNode::Scalar(ty)
        | BootstrapEntryHostTypeNode::Handle { ty, .. }
        | BootstrapEntryHostTypeNode::Closure { ty, .. } => ty.clone(),
        BootstrapEntryHostTypeNode::Option(inner) => {
            MirType::from_kind(MirTypeKind::Option(Box::new(child(*inner)?)))
        }
        BootstrapEntryHostTypeNode::Result { ok, error } => MirType::from_kind(
            MirTypeKind::Result {
                ok: Box::new(child(*ok)?),
                err: Box::new(child(*error)?),
            },
        ),
        BootstrapEntryHostTypeNode::List(inner) => {
            MirType::from_kind(MirTypeKind::List(Box::new(child(*inner)?)))
        }
        BootstrapEntryHostTypeNode::Map { key, value } => MirType::from_kind(
            MirTypeKind::Map {
                key: Box::new(child(*key)?),
                value: Box::new(child(*value)?),
            },
        ),
        BootstrapEntryHostTypeNode::Tuple(fields) => MirType::from_kind(MirTypeKind::Tuple(
            fields
                .iter()
                .map(|field| Ok((field.name.clone(), child(field.node)?)))
                .collect::<Result<Vec<_>, String>>()?,
        )),
        BootstrapEntryHostTypeNode::Struct {
            type_id,
            type_name,
            args,
            ..
        }
        | BootstrapEntryHostTypeNode::Enum {
            type_id,
            type_name,
            args,
            ..
        } => {
            let id = type_id.clone();
            MirType::from_kind(MirTypeKind::Apply {
                name: jet_foundation::MIR::MirNominalRef {
                    id: id.clone(),
                    name: type_name.clone(),
                },
                args: args
                    .iter()
                    .map(|node| child(*node))
                    .collect::<Result<Vec<_>, String>>()?,
            })
            .with_identity(id)
        }
    })
}

pub(crate) fn bootstrap_entry_shape_type_matches(shape_type: &MirType, checked: &MirType) -> bool {
    match (shape_type.kind(), checked.kind()) {
        (MirTypeKind::List(shape_inner), MirTypeKind::FixedList { elem, .. }) => {
            shape_inner.same_checked_type(elem)
        }
        _ => shape_type.same_checked_type(checked),
    }
}

fn bootstrap_entry_validate_shape_references(
    shape: &BootstrapEntryHostTypeShape,
    node: usize,
    depth: usize,
    visited: &mut HashSet<usize>,
) -> Result<(), String> {
    if depth >= MAX_VALUE_DEPTH {
        return Err("compiler-entry host type shape exceeds checked nesting bound".to_string());
    }
    if node >= shape.nodes.len() {
        return Err("compiler-entry host type shape has an invalid node reference".to_string());
    }
    if !visited.insert(node) {
        return Ok(());
    }
    let mut children = Vec::new();
    match &shape.nodes[node] {
        BootstrapEntryHostTypeNode::Scalar(_)
        | BootstrapEntryHostTypeNode::Handle { .. } => {}
        BootstrapEntryHostTypeNode::Option(inner)
        | BootstrapEntryHostTypeNode::List(inner) => children.push(*inner),
        BootstrapEntryHostTypeNode::Result { ok, error } => {
            children.extend([*ok, *error]);
        }
        BootstrapEntryHostTypeNode::Map { key, value } => children.extend([*key, *value]),
        BootstrapEntryHostTypeNode::Tuple(fields) => {
            children.extend(fields.iter().map(|field| field.node));
        }
        BootstrapEntryHostTypeNode::Struct { args, fields, .. } => {
            children.extend(args.iter().copied());
            children.extend(fields.iter().map(|field| field.node));
        }
        BootstrapEntryHostTypeNode::Enum { args, variants, .. } => {
            children.extend(args.iter().copied());
            children.extend(
                variants
                    .iter()
                    .flat_map(|variant| variant.args.iter().map(|(_, node)| *node)),
            );
        }
        BootstrapEntryHostTypeNode::Closure { captures, .. } => {
            children.extend(captures.iter().copied());
        }
    }
    for child in children {
        bootstrap_entry_validate_shape_references(shape, child, depth + 1, visited)?;
    }
    Ok(())
}

pub(crate) fn bootstrap_entry_validate_shape(
    shape: &BootstrapEntryHostTypeShape,
) -> Result<(), String> {
    if shape.nodes.is_empty() || shape.root >= shape.nodes.len() {
        return Err("compiler-entry host type shape has no valid root node".to_string());
    }
    let mut visited = HashSet::new();
    for node in 0..shape.nodes.len() {
        bootstrap_entry_validate_shape_references(shape, node, 0, &mut visited)?;
    }
    let _ = bootstrap_entry_shape_node_type(shape, shape.root, 0)?;
    Ok(())
}

fn bootstrap_entry_validate_shaped_runtime(
    value: &MirRuntimeValue,
    checked: &MirType,
    shape: &BootstrapEntryHostTypeShape,
    shape_node: usize,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_VALUE_DEPTH {
        return Err("compiler-entry runtime value exceeds checked nesting bound".to_string());
    }
    let shape_type = bootstrap_entry_shape_node_type(shape, shape_node, depth)?;
    if !bootstrap_entry_shape_type_matches(&shape_type, checked) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    let child_type = |node| bootstrap_entry_shape_node_type(shape, node, depth + 1);
    match &shape.nodes[shape_node] {
        BootstrapEntryHostTypeNode::Option(inner) => match value {
            MirRuntimeValue::Absent { element } => {
                if !element.same_checked_type(&child_type(*inner)?) {
                    return Err("absent host value changed its exact checked element type".to_string());
                }
            }
            MirRuntimeValue::Present(value) => bootstrap_entry_validate_shaped_runtime(
                value,
                &child_type(*inner)?,
                shape,
                *inner,
                depth + 1,
            )?,
            _ => return Err("checked Option host value has an invalid runtime carrier".to_string()),
        },
        BootstrapEntryHostTypeNode::Result { ok, error } => match value {
            MirRuntimeValue::Present(value) => bootstrap_entry_validate_shaped_runtime(
                value,
                &child_type(*ok)?,
                shape,
                *ok,
                depth + 1,
            )?,
            MirRuntimeValue::FailedTold(value) => bootstrap_entry_validate_shaped_runtime(
                value,
                &child_type(*error)?,
                shape,
                *error,
                depth + 1,
            )?,
            _ => return Err("checked Result host value has an invalid runtime carrier".to_string()),
        },
        BootstrapEntryHostTypeNode::List(inner) => {
            let MirRuntimeValue::List(values) = value else {
                return Err("checked sequence host value has an invalid runtime carrier".to_string());
            };
            if let MirTypeKind::FixedList { len, .. } = checked.kind() {
                if let jet_foundation::MIR::MirMeasure::Literal { value, .. } = len {
                    if values.len() as u64 != *value {
                        return Err("checked fixed-list host value has the wrong length".to_string());
                    }
                }
            }
            let child_type = child_type(*inner)?;
            for value in values {
                bootstrap_entry_validate_shaped_runtime(
                    value,
                    &child_type,
                    shape,
                    *inner,
                    depth + 1,
                )?;
            }
        }
        BootstrapEntryHostTypeNode::Map { value: child, .. } => {
            let MirRuntimeValue::Map(entries) = value else {
                return Err("checked map host value has an invalid runtime carrier".to_string());
            };
            let child_type = child_type(*child)?;
            for (_, value) in entries {
                bootstrap_entry_validate_shaped_runtime(
                    value,
                    &child_type,
                    shape,
                    *child,
                    depth + 1,
                )?;
            }
        }
        BootstrapEntryHostTypeNode::Tuple(fields) => {
            let MirRuntimeValue::Struct { type_name, fields: values } = value else {
                return Err("checked tuple host value has an invalid runtime carrier".to_string());
            };
            if type_name != "Tuple" || values.len() != fields.len() {
                return Err("checked tuple host value shape differs".to_string());
            }
            for (index, (field, (name, value))) in fields.iter().zip(values).enumerate() {
                if field.name != *name {
                    return Err(format!("checked tuple host field {index} differs"));
                }
                bootstrap_entry_validate_shaped_runtime(
                    value,
                    &child_type(field.node)?,
                    shape,
                    field.node,
                    depth + 1,
                )?;
            }
        }
        BootstrapEntryHostTypeNode::Struct {
            type_name,
            fields,
            ..
        } => {
            let MirRuntimeValue::Struct {
                type_name: actual_name,
                fields: values,
            } = value
            else {
                return Err("checked record host value has an invalid runtime carrier".to_string());
            };
            if actual_name != type_name || values.len() != fields.len() {
                return Err("checked record host value shape differs".to_string());
            }
            for (index, (field, (name, value))) in fields.iter().zip(values).enumerate() {
                if field.name != *name {
                    return Err(format!("checked record host field {index} differs"));
                }
                bootstrap_entry_validate_shaped_runtime(
                    value,
                    &child_type(field.node)?,
                    shape,
                    field.node,
                    depth + 1,
                )?;
            }
        }
        BootstrapEntryHostTypeNode::Enum {
            type_name,
            variants,
            ..
        } => {
            let MirRuntimeValue::Enum {
                type_name: actual_name,
                variant,
                args,
            } = value
            else {
                return Err("checked enum host value has an invalid runtime carrier".to_string());
            };
            let expected = variants
                .iter()
                .find(|candidate| candidate.name == *variant)
                .ok_or_else(|| format!("checked enum variant `{type_name}::{variant}` is absent"))?;
            if actual_name != type_name || args.len() != expected.args.len() {
                return Err("checked enum host value shape differs".to_string());
            }
            for (index, ((expected_name, child), (actual_name, value))) in
                expected.args.iter().zip(args).enumerate()
            {
                if expected_name != actual_name {
                    return Err(format!("checked enum host payload {index} differs"));
                }
                bootstrap_entry_validate_shaped_runtime(
                    value,
                    &child_type(*child)?,
                    shape,
                    *child,
                    depth + 1,
                )?;
            }
        }
        BootstrapEntryHostTypeNode::Scalar(_)
        | BootstrapEntryHostTypeNode::Closure { .. }
        | BootstrapEntryHostTypeNode::Handle { .. } => {}
    }
    Ok(())
}

/// A complete entry result, retained as its checked runtime aggregate until a
/// tier-specific generated Source projection consumes it.
#[derive(Debug)]
pub(crate) struct BootstrapEntryValue {
    pub(crate) ty: MirType,
    value: MirRuntimeValue,
}

impl BootstrapEntryValue {
    pub(crate) fn into_value(self) -> MirRuntimeValue {
        self.value
    }

    pub(crate) fn value(&self) -> &MirRuntimeValue {
        &self.value
    }

    pub(crate) fn field(
        &self,
        program: &MirProgram,
        field: MirFieldId,
    ) -> Option<&MirRuntimeValue> {
        checked_record_field(program, &self.ty, &self.value, field)
            .ok()
            .flatten()
    }
}

/// The exact checked `jet_bootstrap_compile` parameter and result carriers.
pub(crate) struct BootstrapEntryCodec<'a> {
    program: &'a MirProgram,
    entry: MirFunctionId,
    request_type: MirType,
    result_type: MirType,
}

impl<'a> BootstrapEntryCodec<'a> {
    pub(crate) fn new(
        program: &'a MirProgram,
        bindings: &BootstrapBindingDescriptor,
        entry: MirFunctionId,
    ) -> Result<Self, BootstrapHostCodecError> {
        let symbols = BootstrapCodecSymbols::new(bindings, program)?;
        let callable = bindings
            .callables
            .iter()
            .find(|row| row.source_name == "jet_bootstrap_compile" && row.metadata.function == entry)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry("jet_bootstrap_compile".to_string()))?;
        let function = program
            .functions
            .iter()
            .find(|function| function.id == entry)
            .ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(
                    "jet_bootstrap_compile binding is absent from checked compiler MIR".to_string(),
                )
            })?;
        if function.kind != MirFunctionKind::Jet
            || function.name != callable.source_name
            || function.params.len() != 1
            || callable.metadata.parameter_types.len() != 1
            || callable.metadata.parameter_access.as_slice() != [function.params[0].access]
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile must retain its exact one-parameter checked ABI".to_string(),
            ));
        }
        let request_type = function.params[0].ty.clone();
        let result_type = function.return_type.clone();
        let request = checked_nominal_definition(program, &request_type)?;
        let result = checked_nominal_definition(program, &result_type)?;
        if request.name != "JetDriverCompileRequest" || result.name != "JetDriverCompileResult" {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile ABI is not JetDriverCompileRequest -> JetDriverCompileResult"
                    .to_string(),
            ));
        }
        match entry_rust_type(program, &symbols, &result_type) {
            Ok(rust_type) if rust_type == callable.metadata.return_type => {}
            Ok(_) => symbols.record(BootstrapHostCodecError::InvalidMetadata(
                "jet_bootstrap_compile return binding disagrees with its checked MIR return type"
                    .to_string(),
            )),
            Err(error) => symbols.record(error),
        }
        // Both graphs are walked in full and every drift recorded, so one
        // packaging run reports all binding failures.
        validate_binding_graph(program, &symbols, &request_type);
        validate_binding_graph(program, &symbols, &result_type);
        symbols.finish(Ok(()))?;
        Ok(Self {
            program,
            entry,
            request_type,
            result_type,
        })
    }

    pub(crate) fn entry(&self) -> MirFunctionId {
        self.entry
    }

    pub(crate) fn request_type(&self) -> &MirType {
        &self.request_type
    }

    pub(crate) fn result_type(&self) -> &MirType {
        &self.result_type
    }

    /// Validate and transfer one complete aggregate into the checked factory
    /// parameter vector. The factory entry has exactly one source argument.
    pub(crate) fn encode_compile_arguments(
        &self,
        request: MirRuntimeValue,
    ) -> Result<Vec<MirRuntimeValue>, BootstrapHostCodecError> {
        validate_runtime_value(self.program, &self.request_type, &request, 0)
            .map_err(BootstrapHostCodecError::InvalidMetadata)?;
        Ok(vec![request])
    }

    /// Validate and transfer the entire result aggregate without dropping
    /// diagnostics, source MIR, artifact/output metadata, or control results.
    pub(crate) fn decode_compile_result(
        &self,
        value: MirRuntimeValue,
    ) -> Result<BootstrapEntryValue, BootstrapHostCodecError> {
        validate_runtime_value(self.program, &self.result_type, &value, 0)
            .map_err(BootstrapHostCodecError::InvalidMetadata)?;
        Ok(BootstrapEntryValue {
            ty: self.result_type.clone(),
            value,
        })
    }

    /// Build a record from checked field IDs, emitted in checked MIR order.
    pub(crate) fn record(
        &self,
        owner: MirTypeId,
        fields: Vec<(MirFieldId, MirRuntimeValue)>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let definition = checked_definition_by_id(self.program, owner)?;
        let declared = match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => fields,
            _ => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "checked type `{}` is not a struct carrier",
                    definition.name
                )))
            }
        };
        let mut supplied = std::collections::BTreeMap::new();
        for (field, value) in fields {
            if supplied.insert(field, value).is_some() {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "record `{}` repeats checked field ID {}",
                    definition.name, field.0
                )));
            }
        }
        if supplied.len() != declared.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "record `{}` has {} values for {} checked fields",
                definition.name,
                supplied.len(),
                declared.len()
            )));
        }
        let mut output = Vec::with_capacity(declared.len());
        for field in declared {
            let value = supplied.remove(&field.id).ok_or_else(|| {
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "record `{}` is missing checked field `{}`",
                    definition.name, field.name
                ))
            })?;
            validate_runtime_value(self.program, &field.ty, &value, 0)
                .map_err(BootstrapHostCodecError::InvalidMetadata)?;
            output.push((field.name.clone(), value));
        }
        if !supplied.is_empty() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "record `{}` contains an undeclared checked field ID",
                definition.name
            )));
        }
        Ok(MirRuntimeValue::Struct {
            type_name: definition.name.clone(),
            fields: output,
        })
    }

    /// Build an enum from its checked owner, variant, and payload field types.
    pub(crate) fn enumeration(
        &self,
        owner: MirTypeId,
        variant_name: &str,
        values: Vec<MirRuntimeValue>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let definition = checked_definition_by_id(self.program, owner)?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked type `{}` is not an enum carrier",
                definition.name
            )));
        };
        let variant = variants
            .iter()
            .find(|variant| variant.name == variant_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingVariant {
                owner: definition.name.clone(),
                variant: variant_name.to_string(),
            })?;
        let expected = match &variant.payload {
            MirVariantPayload::Unit => Vec::new(),
            MirVariantPayload::Single(ty) => vec![(None, ty)],
            MirVariantPayload::Named(fields) => fields
                .iter()
                .map(|field| (Some(field.name.as_str()), &field.ty))
                .collect(),
        };
        if values.len() != expected.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "enum `{}` variant `{}` has {} values for {} checked fields",
                definition.name,
                variant_name,
                values.len(),
                expected.len()
            )));
        }
        let mut args = Vec::with_capacity(values.len());
        for ((name, ty), value) in expected.into_iter().zip(values) {
            validate_runtime_value(self.program, ty, &value, 0)
                .map_err(BootstrapHostCodecError::InvalidMetadata)?;
            args.push((name.map(str::to_string), value));
        }
        Ok(MirRuntimeValue::Enum {
            type_name: definition.name.clone(),
            variant: variant.name.clone(),
            args,
        })
    }

    /// Build Present/Absent while retaining the checked optional element type.
    pub(crate) fn option(
        &self,
        ty: &MirType,
        value: Option<MirRuntimeValue>,
    ) -> Result<MirRuntimeValue, BootstrapHostCodecError> {
        let MirTypeKind::Option(inner) = ty.kind() else {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "optional carrier requested for a non-optional checked type".to_string(),
            ));
        };
        match value {
            Some(value) => {
                validate_runtime_value(self.program, inner, &value, 0)
                    .map_err(BootstrapHostCodecError::InvalidMetadata)?;
                Ok(MirRuntimeValue::Present(Box::new(value)))
            }
            None => Ok(MirRuntimeValue::Absent {
                element: inner.as_ref().clone(),
            }),
        }
    }
}

/// The checked `JetEvalHostAdapter` methods, taken from the trait itself. A
/// method with parameters takes the Source machine as its first, WRITE
/// parameter; the parameterless ones (session, clone, physical binding) do not.
fn checked_native_adapter_methods(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<Vec<MirTraitMethod>, BootstrapHostCodecError> {
    let rust_methods = symbols.trait_methods("JetEvalHostAdapter")?;
    let trait_id = rust_methods[0].trait_id;
    let checked_trait = program
        .traits
        .iter()
        .find(|row| row.id == trait_id && row.name == "JetEvalHostAdapter")
        .ok_or_else(|| BootstrapHostCodecError::MissingEntry(
            "checked JetEvalHostAdapter trait".to_string(),
        ))?;
    if checked_trait.methods.len() != rust_methods.len() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "JetEvalHostAdapter Rust metadata and checked MIR list different methods".to_string(),
        ));
    }
    let mut methods = Vec::with_capacity(checked_trait.methods.len());
    for method in &checked_trait.methods {
        let name = &method.name;
        let metadata = rust_methods
            .iter()
            .find(|row| row.method_id == method.id && row.name == *name)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(format!(
                "Rust metadata for checked JetEvalHostAdapter.{name}"
            )))?;
        let arity = method.params.len();
        if metadata.parameter_types.len() != arity
            || metadata.parameter_access.len() != arity
            || metadata.return_type.is_empty()
            || method.self_access != metadata.receiver_access
            || method.params.iter().zip(&metadata.parameter_access)
                .any(|(parameter, access)| parameter.access != *access)
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetEvalHostAdapter.{name} Rust metadata differs from its checked MIR parameters"
            )));
        }
        if let Some(machine) = method.params.first() {
            let definition = checked_nominal_definition(program, &machine.ty)?;
            if machine.access != jet_foundation::MIR::MirAccess::Write
                || definition.name != "JetEvalMachine"
            {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetEvalHostAdapter.{name} does not use the checked JetEvalMachine WRITE parameter"
                )));
            }
        }
        methods.push(method.clone());
    }
    Ok(methods)
}

fn entry_rust_type(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    Ok(match ty.kind() {
        Kind::Int => "jet_foundation::Numeric::JetInt".to_string(),
        Kind::IntN { signed, bits } => format!("{}{bits}", if *signed { 'i' } else { 'u' }),
        Kind::Float => "f64".to_string(),
        Kind::Float32 => "f32".to_string(),
        Kind::Measure(_) => "f64".to_string(),
        Kind::Bool => "bool".to_string(),
        Kind::Char => "char".to_string(),
        Kind::String => "String".to_string(),
        Kind::List(inner) => format!("Vec<{}>", entry_rust_type(program, symbols, inner)?),
        Kind::Map { key, value } => format!(
            "crate::JetMap<{}, {}>",
            entry_rust_type(program, symbols, key)?,
            entry_rust_type(program, symbols, value)?,
        ),
        Kind::Shared(inner) => format!(
            "crate::jet_std::JetShared<{}>",
            entry_rust_type(program, symbols, inner)?,
        ),
        Kind::Option(inner) => format!(
            "crate::JetOutcome<{}, crate::JetAbsent>",
            entry_rust_type(program, symbols, inner)?,
        ),
        Kind::Result { ok, err } => format!(
            "crate::JetOutcome<{}, {}>",
            entry_rust_type(program, symbols, ok)?,
            entry_rust_type(program, symbols, err)?,
        ),
        Kind::Apply { name, args } => {
            if args.is_empty() && name.name == jet_foundation::Syntax::INTERNAL_UNIT_TYPE {
                "()".to_string()
            } else if args.is_empty() && name.name == jet_foundation::Syntax::TYPE_NEVER {
                "std::convert::Infallible".to_string()
            } else if name.name == jet_foundation::Syntax::TYPE_PTR {
                let [inner] = args.as_slice() else {
                    return Err(BootstrapHostCodecError::InvalidMetadata(
                        "checked Ptr type does not have one element argument".to_string(),
                    ));
                };
                format!("*mut {}", entry_rust_type(program, symbols, inner)?)
            } else {
                let definition = checked_definition_by_ref(program, name)?;
                if args.len() != definition.generic_params.len() {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked type `{}` has an unexpected generic argument count",
                        definition.name
                    )));
                }
                let symbol = symbols.type_symbol(&definition.name)?;
                if args.is_empty() {
                    symbol.to_string()
                } else {
                    format!(
                        "{symbol}<{}>",
                        args.iter()
                            .map(|arg| entry_rust_type(program, symbols, arg))
                            .collect::<Result<Vec<_>, _>>()?
                            .join(", ")
                    )
                }
            }
        }
        Kind::TraitObject(bounds) => {
            let traits = bounds
                .iter()
                .map(|bound| symbols.trait_symbol(&bound.name))
                .collect::<Result<Vec<_>, _>>()?;
            if traits.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked trait-object type has no trait bounds".to_string(),
                ));
            }
            format!("Box<dyn {}>", traits.join(" + "))
        }
        Kind::Tuple(fields) => {
            let fields = fields
                .iter()
                .map(|(_, field)| entry_rust_type(program, symbols, field))
                .collect::<Result<Vec<_>, _>>()?;
            match fields.len() {
                0 => "()".to_string(),
                1 => format!("({},)", fields[0]),
                _ => format!("({})", fields.join(", ")),
            }
        }
        Kind::FixedList { elem, len } => format!(
            "[{}; {}]",
            entry_rust_type(program, symbols, elem)?,
            len.expression()
        ),
        Kind::InlineRange { base, .. }
        | Kind::Tagged { inner: base, .. }
        | Kind::Quantity { base, .. } => entry_rust_type(program, symbols, base)?,
        Kind::Union(_) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
                "checked entry union has no nominal identity".to_string(),
            ))?;
            let definition = checked_definition_by_id(program, id).map_err(|_| BootstrapHostCodecError::MissingType(format!("`{}` (MIR type ID {})", ty.canonical_key(), id.0)))?;
            symbols.type_symbol(&definition.name)?.to_string()
        }
        Kind::Fn(_) | Kind::SendFn { .. } => {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetEvalHostAdapter parameter/result unexpectedly uses callable type `{}`",
                ty.canonical_key()
            )));
        }
    })
}

/// Emit the typed argument/result codecs and one dispatch shim per checked
/// `JetEvalHostAdapter` method. The native adapter's interface handler calls
/// `__jet_bootstrap_entry_native_interface_{method}` with the live call and a
/// typed operation; the shim decodes each checked argument (Move arguments are
/// consumed through the call's transfer ledger), borrows the Source machine in
/// place, runs the operation, and encodes its typed result. Methods returning
/// the adapter trait object (sessions and clones) are answered by the adapter
/// itself and get no shim.
fn emit_entry_native_interface_codecs(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    methods: &[MirTraitMethod],
) -> Result<(), BootstrapHostCodecError> {
    out.push_str(ENTRY_NATIVE_INTERFACE_ARGUMENT);
    for method in methods {
        let machine_parameter = !method.params.is_empty();
        let mut argument_types = Vec::with_capacity(method.params.len());
        for (index, parameter) in method.params.iter().enumerate() {
            if machine_parameter && index == 0 {
                continue;
            }
            if parameter.access == jet_foundation::MIR::MirAccess::Write {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetEvalHostAdapter.{} argument {index} is a WRITE borrow; only the machine may be written back",
                    method.name
                )));
            }
            let source_type = entry_rust_type(program, symbols, &parameter.ty)?;
            argument_types.push(source_type.clone());
            let checked_key = format!("{:?}", parameter.ty.canonical_key());
            let path_error = format!(
                "checked JetEvalHostAdapter.{} argument {index} has the wrong MIR type",
                method.name
            );
            let expression = from_runtime_expression(
                program,
                symbols,
                &parameter.ty,
                "value",
                "checked",
                "program",
                "path",
            )?;
            writeln!(
                out,
                r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_native_interface_{method}_arg_{index}_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(
    value: ::jet_foundation::MIR::MirRuntimeValue,
    checked: &::jet_foundation::MIR::MirType,
    compiler_program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    path: &[String],
    physical: &P,
) -> Result<{source_type}, String> {{
    let program = compiler_program.as_ref();
    if checked.canonical_key() != {checked_key} {{
        return Err({path_error:?}.to_string());
    }}
    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program, checked, &value, 0)?;
    Ok({expression})
}}
"#,
                method = method.name,
            )
            .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
        }

        if matches!(method.return_type.kind(), MirTypeKind::TraitObject(_)) {
            continue;
        }
        let source_type = entry_rust_type(program, symbols, &method.return_type)?;
        let checked_key = format!("{:?}", method.return_type.canonical_key());
        let path_error = format!(
            "checked JetEvalHostAdapter.{} result has the wrong MIR type",
            method.name
        );
        let expression = to_runtime_expression(
            program,
            symbols,
            &method.return_type,
            "source_value",
            "checked",
            "compiler_program",
            "path",
            "physical",
        )?;
        writeln!(
            out,
            r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_native_interface_{method}_result_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(
    source_value: &{source_type},
    checked: &::jet_foundation::MIR::MirType,
    compiler_program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    path: &[String],
    physical: &P,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
    let program = compiler_program.as_ref();
    if checked.canonical_key() != {checked_key} {{
        return Err({path_error:?}.to_string());
    }}
    let value = {expression};
    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program, checked, &value, 0)?;
    Ok(value)
}}
"#,
            method = method.name,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
        emit_entry_native_interface_shim(out, program, symbols, method, &argument_types, &source_type)?;
    }
    Ok(())
}

/// Read one checked non-machine argument of a native interface call. A Read
/// argument is decoded from the call row. A Move argument is decoded from its
/// guarded payload first and consumed only after decoding succeeded, so a
/// decode failure leaves the value with the call for Source cleanup.
const ENTRY_NATIVE_INTERFACE_ARGUMENT: &str = r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_native_interface_argument<P, T>(
    call: &mut ::jet_jit::SourceInterfaces::NativeInterfaceCall,
    index: usize,
    compiler_program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    physical: &P,
    decode: fn(
        ::jet_foundation::MIR::MirRuntimeValue,
        &::jet_foundation::MIR::MirType,
        &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
        &[String],
        &P,
    ) -> Result<T, String>,
) -> Result<T, String>
where
    P: crate::BootstrapEntryPhysicalBindings,
{
    let argument = call
        .argument(index)
        .ok_or_else(|| format!("native interface argument {index} is missing"))?;
    let checked = argument.ty.clone();
    let path = vec![format!("argument {index}")];
    if argument.access != ::jet_foundation::MIR::MirAccess::Move {
        let value = argument.value.clone();
        return decode(value, &checked, compiler_program, &path, physical);
    }
    let transfer = call.take_owned_argument(index).map_err(|error| error.to_string())?;
    let guard = transfer.take_value().map_err(|error| error.to_string())?;
    let payload = guard.with_value(|value| value.clone()).map_err(|error| error.to_string())?;
    let decoded = decode(payload, &checked, compiler_program, &path, physical)?;
    call.consume_owned_argument(transfer, guard, |guard, commit| {
        guard.commit_and_extract(::jet_foundation::MIR::MirRuntimeValue::Unit, commit)?;
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(decoded)
}
"#;

/// Emit `__jet_bootstrap_entry_native_interface_{method}`: decode the checked
/// arguments, run `operation` with the in-place Source machine borrow (when the
/// method takes one), and encode the typed result.
fn emit_entry_native_interface_shim(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    method: &MirTraitMethod,
    argument_types: &[String],
    result_type: &str,
) -> Result<(), BootstrapHostCodecError> {
    let name = &method.name;
    let arity = method.params.len();
    let first_argument = usize::from(!method.params.is_empty());
    let argument_names = (first_argument..arity)
        .map(|index| format!("__jet_arg_{index}"))
        .collect::<Vec<_>>();
    let mut argument_lets = String::new();
    for (index, argument) in (first_argument..arity).zip(&argument_names) {
        writeln!(
            argument_lets,
            "    let {argument} = __jet_bootstrap_entry_native_interface_argument(call, {index}, compiler_program, physical, __jet_bootstrap_entry_native_interface_{name}_arg_{index}_from_runtime::<P>)?;"
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    let arguments = argument_names.join(", ");
    let (operation_parameters, invoke) = match method.params.first() {
        Some(machine) => {
            let machine_type = entry_rust_type(program, symbols, &machine.ty)?;
            let mut parameters = vec![format!(
                "crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, {machine_type}>"
            )];
            parameters.extend(argument_types.iter().cloned());
            let separator = if arguments.is_empty() { "" } else { ", " };
            let invoke = format!(
                "    let __jet_machine_type = call.signature().parameters[0].ty.clone();\n\
                 \x20   let __jet_result = call\n\
                 \x20       .with_writeback_argument(0, |__jet_machine_value| {{\n\
                 \x20           let __jet_machine = physical.borrow_interpreter_machine_write(\n\
                 \x20               __jet_machine_value,\n\
                 \x20               &__jet_machine_type,\n\
                 \x20               machine_abi_shape,\n\
                 \x20           )?;\n\
                 \x20           operation(crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Mir(__jet_machine){separator}{arguments})\n\
                 \x20       }})\n\
                 \x20       .map_err(|error| error.to_string())??;\n"
            );
            (parameters.join(", "), invoke)
        }
        None => (
            String::new(),
            "    let _ = machine_abi_shape;\n    let __jet_result = operation()?;\n".to_string(),
        ),
    };
    let arity_error = format!("native JetEvalHostAdapter.{name} call does not have {arity} checked arguments");
    writeln!(
        out,
        r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_native_interface_{name}<P, F>(
    call: &mut ::jet_jit::SourceInterfaces::NativeInterfaceCall,
    compiler_program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    machine_abi_shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    physical: &P,
    operation: F,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String>
where
    P: crate::BootstrapEntryPhysicalBindings,
    F: FnOnce({operation_parameters}) -> Result<{result_type}, String>,
{{
    if call.signature().parameters.len() != {arity} || call.arguments().len() != {arity} {{
        return Err({arity_error:?}.to_string());
    }}
    let __jet_result_type = call.signature().return_type.clone();
{argument_lets}{invoke}    let path = vec![{name:?}.to_string()];
    __jet_bootstrap_entry_native_interface_{name}_result_to_runtime(&__jet_result, &__jet_result_type, compiler_program, &path, physical)
}}
"#
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

/// Append typed request/result conversion code to the private emitted artifact.
/// The generated routines use only types/fields/variants reached from the exact
/// factory signature and resolved through the same binding descriptor.
pub(crate) fn append_bootstrap_entry_codec(
    out: &mut String,
    bindings: &BootstrapBindingDescriptor,
    symbols: &BootstrapCodecSymbols<'_>,
    program: &MirProgram,
    entry: MirFunctionId,
) -> Result<(), BootstrapHostCodecError> {
    let function = program.functions.iter().find(|function| function.id == entry)
        .ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
            "entry codec cannot find the checked compiler factory function".to_string(),
        ))?;
    if function.name != "jet_bootstrap_compile" || function.params.len() != 1 {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "entry codec requires the checked one-request factory function".to_string(),
        ));
    }
    let callable = bindings.callables.iter()
        .find(|row| row.source_name == function.name && row.metadata.function == entry)
        .ok_or_else(|| BootstrapHostCodecError::MissingEntry(function.name.clone()))?;
    if callable.metadata.parameter_types.len() != 1
        || callable.metadata.parameter_access.as_slice() != [function.params[0].access]
        || callable.metadata.return_type.is_empty()
    {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "entry callable binding differs from the checked one-request ABI".to_string(),
        ));
    }
    let request = checked_nominal_definition(program, &function.params[0].ty)?;
    let result = checked_nominal_definition(program, &function.return_type)?;
    if request.name != "JetDriverCompileRequest" || result.name != "JetDriverCompileResult" {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "factory signature is not the checked compile request/result contract".to_string(),
        ));
    }
    validate_binding_graph(program, symbols, &function.params[0].ty);
    validate_binding_graph(program, symbols, &function.return_type);
    let native_methods = checked_native_adapter_methods(program, symbols)?;

    let mut to_types = Vec::new();
    let mut from_types = Vec::new();
    collect_nominal_types(program, &function.params[0].ty, &mut HashSet::new(), &mut to_types)?;
    collect_nominal_types(program, &function.return_type, &mut HashSet::new(), &mut from_types)?;
    let mut to_seen: HashSet<_> = to_types.iter().map(|definition| definition.id).collect();
    let mut from_seen: HashSet<_> = from_types.iter().map(|definition| definition.id).collect();
    for method in &native_methods {
        for (index, parameter) in method.params.iter().enumerate() {
            if index == 0
                && parameter.access == jet_foundation::MIR::MirAccess::Write
                && checked_nominal_definition(program, &parameter.ty)
                    .is_ok_and(|definition| definition.name == "JetEvalMachine")
            {
                continue;
            }
            validate_binding_graph(program, symbols, &parameter.ty);
            let mut graph = Vec::new();
            collect_nominal_types(program, &parameter.ty, &mut HashSet::new(), &mut graph)?;
            for nested in graph {
                if from_seen.insert(nested.id) {
                    from_types.push(nested);
                }
            }
        }
        if !matches!(method.return_type.kind(), MirTypeKind::TraitObject(_)) {
            validate_binding_graph(program, symbols, &method.return_type);
            let mut graph = Vec::new();
            collect_nominal_types(program, &method.return_type, &mut HashSet::new(), &mut graph)?;
            for nested in graph {
                if to_seen.insert(nested.id) {
                    to_types.push(nested);
                }
            }
        }
    }
    to_types.sort_by_key(|definition| definition.id);
    from_types.sort_by_key(|definition| definition.id);
    for definition in &to_types {
        emit_to_runtime_converter(out, program, symbols, definition)?;
    }
    for definition in &from_types {
        emit_from_runtime_converter(out, program, symbols, definition)?;
    }
    emit_entry_native_interface_codecs(out, program, symbols, &native_methods)?;
    emit_entry_machine_access_helpers(out, program, symbols)?;
    emit_entry_host_type_shape_from_source(out, symbols)?;
    emit_entry_conversion_helpers(out)?;
    Ok(())
}

/// `__jet_bootstrap_entry_host_type_shape_from_source`: the Source
/// accessor's checked `JetEvalHostTypeShape` as the host shape graph the
/// native adapter validates machine and Shared payload ABI against.
fn emit_entry_host_type_shape_from_source(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let _ = symbols.type_symbol("JetEvalHostTypeShape")?;
    out.push_str(
        r#"
pub(crate) fn __jet_bootstrap_entry_host_type_shape_from_source(
    source: &@t.JetEvalHostTypeShape@,
) -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostOwner as Owner,
        BootstrapEntryHostTypeField as Field,
        BootstrapEntryHostTypeNode as Node,
        BootstrapEntryHostTypeShape as Shape,
        BootstrapEntryHostTypeVariant as Variant,
    };
    let field_shapes = |fields: &[@t.JetEvalHostTypeFieldShape@], label: &str| {
        fields.iter().map(|field| {
            Ok(Field {
                name: field.@f.JetEvalHostTypeFieldShape.name@.clone(),
                node: __jet_bootstrap_source_index(&field.@f.JetEvalHostTypeFieldShape.node@, label)?,
            })
        }).collect::<Result<Vec<_>, String>>()
    };
    let indices = |indices: &[jet_foundation::Numeric::JetInt], label: &str| {
        indices.iter()
            .map(|index| __jet_bootstrap_source_index(index, label))
            .collect::<Result<Vec<_>, String>>()
    };
    let root = __jet_bootstrap_source_index(&source.@f.JetEvalHostTypeShape.root@, "host result type root")?;
    let nodes = source.@f.JetEvalHostTypeShape.nodes@.iter().map(|node| {
        Ok(match node {
            @p.JetEvalHostTypeNode.Scalar@(ty) => Node::Scalar(__jet_bootstrap_type_to_host(ty)?),
            @p.JetEvalHostTypeNode.Option@(inner) => Node::Option(
                __jet_bootstrap_source_index(inner, "option type node")?,
            ),
            @p.JetEvalHostTypeNode.Result@(ok, error) => Node::Result {
                ok: __jet_bootstrap_source_index(ok, "result success type node")?,
                error: __jet_bootstrap_source_index(error, "result error type node")?,
            },
            @p.JetEvalHostTypeNode.List@(inner) => Node::List(
                __jet_bootstrap_source_index(inner, "list element type node")?,
            ),
            @p.JetEvalHostTypeNode.Map@(key, value) => Node::Map {
                key: __jet_bootstrap_source_index(key, "map key type node")?,
                value: __jet_bootstrap_source_index(value, "map value type node")?,
            },
            @p.JetEvalHostTypeNode.Tuple@(fields) => Node::Tuple(field_shapes(fields, "tuple field type node")?),
            @p.JetEvalHostTypeNode.Struct@(type_id, type_name, args, fields) => Node::Struct {
                type_id: ::jet_foundation::MIR::MirTypeId(type_id.@f.MIRTypeID.value@),
                type_name: type_name.clone(),
                args: indices(args, "nominal argument type node")?,
                fields: field_shapes(fields, "record field type node")?,
            },
            @p.JetEvalHostTypeNode.Enum@(type_id, type_name, args, variants) => Node::Enum {
                type_id: ::jet_foundation::MIR::MirTypeId(type_id.@f.MIRTypeID.value@),
                type_name: type_name.clone(),
                args: indices(args, "nominal argument type node")?,
                variants: variants.iter().map(|variant| {
                    Ok(Variant {
                        name: variant.@f.JetEvalHostTypeVariantShape.name@.clone(),
                        args: variant.@f.JetEvalHostTypeVariantShape.args@.iter().map(|arg| {
                            Ok((
                                arg.@f.JetEvalHostTypeArgShape.name@.as_ref().ok().cloned(),
                                __jet_bootstrap_source_index(&arg.@f.JetEvalHostTypeArgShape.node@, "enum payload type node")?,
                            ))
                        }).collect::<Result<Vec<_>, String>>()?,
                    })
                }).collect::<Result<Vec<_>, String>>()?,
            },
            @p.JetEvalHostTypeNode.Closure@(ty, function, captures) => Node::Closure {
                function: ::jet_foundation::MIR::MirFunctionId(function.@f.MIRFunctionID.value@),
                captures: indices(captures, "closure capture type node")?,
                ty: __jet_bootstrap_type_to_host(ty)?,
            },
            @p.JetEvalHostTypeNode.Handle@(ty, owner) => Node::Handle {
                ty: __jet_bootstrap_type_to_host(ty)?,
                owner: match &@deref.JetEvalHostTypeNode.Handle.owner@*owner {
                    @p.JetEvalHostOwner.Cursor@ => Owner::Cursor,
                    @p.JetEvalHostOwner.Declared@(ty) => Owner::Declared(__jet_bootstrap_type_to_host(ty)?),
                    @p.JetEvalHostOwner.Core@(owner) => Owner::Core {
                        fact: __jet_bootstrap_mir_MIRCoreOwner_to_host(&owner.@f.JetEvalHostCoreOwner.fact@)?,
                        ty: __jet_bootstrap_type_to_host(&owner.@f.JetEvalHostCoreOwner.ty@)?,
                    },
                    @p.JetEvalHostOwner.Native@(binding) => match &@deref.JetEvalHostOwner.Native.binding@*binding {
                        @p.JetEvalNativeBindingIdentity.Callable@(binding) => Owner::NativeCallable {
                            key: binding.@f.JetEvalNativeCallableBinding.key@.clone(),
                            callable_type: __jet_bootstrap_type_to_host(&binding.@f.JetEvalNativeCallableBinding.callable_type@)?,
                        },
                        @p.JetEvalNativeBindingIdentity.Interface@(receiver_type) => Owner::NativeInterface {
                            receiver_type: __jet_bootstrap_type_to_host(receiver_type)?,
                        },
                    },
                    @p.JetEvalHostOwner.FixedBackingRoot@(..) => {
                        return Err("checked host type shape names a fixed-backing root, which has no host owner".to_string());
                    }
                },
            },
        })
    }).collect::<Result<Vec<_>, String>>()?;
    let shape = Shape { root, nodes };
    crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(&shape)?;
    Ok(shape)
}
"#,
    );
    Ok(())
}

fn emit_entry_conversion_helpers(out: &mut String) -> Result<(), BootstrapHostCodecError> {
    out.push_str(
        r#"
/// The Source runtime's Shared owner operations report through its own copy
/// of the physical-operation outcome; the JIT interop surface takes its own.
/// Both carry the same result and `Box<dyn Any + Send>` completion.
fn __jet_bootstrap_shared_outcome<T>(
    outcome: crate::JetSharedPhysicalOperationOutcome<T>,
) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<T> {
    ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(outcome.result, outcome.completion)
}

/// A runtime owner-alias lease behind the JIT interop alias surface.
struct __JetBootstrapSharedOwnerAlias<T: Send + Sync + 'static> {
    alias: Option<crate::jet_std::JetSharedPhysicalOwnerAlias<T>>,
}

impl<T: Send + Sync + 'static> ::jet_jit::SourceSharedInterop::SourceSharedInteropOwnerAliasLease
    for __JetBootstrapSharedOwnerAlias<T>
{
    fn token_id(&self) -> i64 {
        self.alias.as_ref().map_or(0, |alias| alias.token_id())
    }

    fn release(
        mut self: Box<Self>,
    ) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<()> {
        match self.alias.take() {
            Some(alias) => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Ok(()), alias.release()),
            None => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(
                Err("Source Shared owner alias was already released".to_string()),
                None,
            ),
        }
    }
}

/// Weak upgrade path of one Source Shared owner for the JIT interop root.
struct __JetBootstrapSharedOwnerWeak<T: Send + Sync + 'static> {
    owner: crate::jet_std::JetSharedPhysicalOwner<T>,
    root_builder: ::std::sync::Arc<
        dyn Fn() -> Result<::jet_jit::SourceSharedInterop::SourceSharedInterop, String> + Send + Sync,
    >,
    type_id: u64,
}

impl<T: Send + Sync + 'static> ::jet_jit::SourceSharedInterop::SourceSharedInteropWeakOwner
    for __JetBootstrapSharedOwnerWeak<T>
{
    fn upgrade(
        &self,
    ) -> Result<
        Option<(
            ::jet_jit::SourceSharedInterop::SourceSharedInterop,
            Box<dyn ::jet_jit::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>,
        )>,
        String,
    > {
        let Some(alias) = self.owner.try_retain_alias()? else {
            return Ok(None);
        };
        let root = __jet_bootstrap_shared_source_interop_root(
            &self.owner,
            self.type_id,
            self.root_builder.clone(),
        )?;
        if root.type_id() != self.type_id
            || root.identity() != self.owner.physical_identity()
            || root.protocol_order_key() != Some(self.owner.protocol_order_key())
        {
            return Err("Source Shared weak upgrade resolved an inconsistent physical root".to_string());
        }
        Ok(Some((root, Box::new(__JetBootstrapSharedOwnerAlias { alias: Some(alias) }))))
    }
}

/// The JIT interop root of one Source Shared owner: the checked callback
/// root plus the owner's identity, weak-upgrade and alias lifecycle. This is
/// the host side of the runtime's JIT-only `source_interop_root`, which the
/// stage-zero runtime crate does not link.
fn __jet_bootstrap_shared_source_interop_root<T: Send + Sync + 'static>(
    owner: &crate::jet_std::JetSharedPhysicalOwner<T>,
    type_id: u64,
    root_builder: ::std::sync::Arc<
        dyn Fn() -> Result<::jet_jit::SourceSharedInterop::SourceSharedInterop, String> + Send + Sync,
    >,
) -> Result<::jet_jit::SourceSharedInterop::SourceSharedInterop, String> {
    let root = root_builder()?;
    if root.type_id() != type_id {
        return Err("Source Shared callback root has a different checked type".to_string());
    }
    let typed_owner = owner.clone();
    let weak_owner = owner.clone();
    let count_owner = owner.clone();
    let retain_owner = owner.clone();
    Ok(root
        .with_owner_identity(owner.physical_identity())
        .with_protocol_order_key(owner.protocol_order_key())
        .with_owner_lifecycle(move || {
            Box::new(__JetBootstrapSharedOwnerWeak {
                owner: weak_owner.clone(),
                root_builder: root_builder.clone(),
                type_id,
            })
        })
        .with_owner_alias_lifecycle(
            move || Ok(count_owner.strong_count()),
            move || {
                retain_owner.retain_alias().map(|alias| {
                    Box::new(__JetBootstrapSharedOwnerAlias { alias: Some(alias) })
                        as Box<dyn ::jet_jit::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>
                })
            },
        )
        .with_typed_owner(typed_owner))
}

struct __JetBootstrapSharedGuard<
    G,
    Read,
    Stage,
    Suspend,
    Resume,
    Held,
    Abort,
    RootPtr,
    MarkDirty,
    Revision,
    Release,
> {
    guard: Option<G>,
    read: Read,
    stage: Stage,
    suspend: Suspend,
    resume: Resume,
    held: Held,
    abort: Abort,
    root_ptr: RootPtr,
    mark_dirty: MarkDirty,
    revision: Revision,
    release: Release,
}

impl<G, Read, Stage, Suspend, Resume, Held, Abort, RootPtr, MarkDirty, Revision, Release>
    ::jet_jit::SourceSharedInterop::SourceSharedInteropGuard
    for __JetBootstrapSharedGuard<
        G,
        Read,
        Stage,
        Suspend,
        Resume,
        Held,
        Abort,
        RootPtr,
        MarkDirty,
        Revision,
        Release,
    >
where
    G: 'static,
    Read: Fn(&G) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> + 'static,
    Stage: Fn(&mut G, ::jet_foundation::MIR::MirRuntimeValue) -> Result<(), String> + 'static,
    Suspend: Fn(
            &mut G,
        ) -> crate::JetSharedPhysicalOperationOutcome<()>
        + 'static,
    Resume: Fn(&mut G, &mut dyn FnMut() -> bool) -> Result<bool, String> + 'static,
    Held: Fn(&G) -> bool + 'static,
    Abort: Fn(
            &mut G,
        ) -> crate::JetSharedPhysicalOperationOutcome<()>
        + 'static,
    RootPtr: Fn(&G) -> Result<*mut (), String> + 'static,
    MarkDirty: Fn(&G) -> Result<(), String> + 'static,
    Revision: Fn(&G) -> Result<u64, String> + 'static,
    Release: Fn(G) -> crate::JetSharedPhysicalOperationOutcome<()> + 'static,
{
    fn read_value(&mut self) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
        let guard = self
            .guard
            .as_ref()
            .ok_or_else(|| "Shared guard was already released".to_string())?;
        (self.read)(guard)
    }

    fn stage_value(
        &mut self,
        value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<(), String> {
        let guard = self
            .guard
            .as_mut()
            .ok_or_else(|| "Shared guard was already released".to_string())?;
        (self.stage)(guard, value)
    }

    fn revision(&self) -> Result<u64, String> {
        let guard = self
            .guard
            .as_ref()
            .ok_or_else(|| "Shared guard was already released".to_string())?;
        (self.revision)(guard)
    }

    fn held(&self) -> bool {
        self.guard.as_ref().is_some_and(|guard| (self.held)(guard))
    }

    fn wait_suspend(
        &mut self,
        value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<()> {
        let Some(guard) = self.guard.as_mut() else {
            return ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(
                Err("Shared guard was already released".to_string()),
                None,
            );
        };
        match (self.read)(guard) {
            Ok(current) if current == value => __jet_bootstrap_shared_outcome((self.suspend)(guard)),
            Ok(_) => match (self.stage)(guard, value) {
                Ok(()) => __jet_bootstrap_shared_outcome((self.suspend)(guard)),
                Err(error) => {
                    ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Err(error), None)
                }
            },
            Err(error) => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Err(error), None),
        }
    }

    fn wait_resume(
        &mut self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<
        Option<::jet_foundation::MIR::MirRuntimeValue>,
    > {
        let Some(guard) = self.guard.as_mut() else {
            return ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(
                Err("Shared guard was already released".to_string()),
                None,
            );
        };
        match (self.resume)(guard, cancelled) {
            Ok(true) => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(
                (self.read)(guard).map(Some),
                None,
            ),
            Ok(false) => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Ok(None), None),
            Err(error) => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Err(error), None),
        }
    }

    fn finish_value(
        &mut self,
        value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<(), String> {
        if !self.held() {
            return Ok(());
        }
        if self.read_value()? != value {
            self.stage_value(value)?;
        }
        Ok(())
    }

    fn wait_abort(&mut self) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<()> {
        match self.guard.as_mut() {
            Some(guard) => __jet_bootstrap_shared_outcome((self.abort)(guard)),
            None => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Ok(()), None),
        }
    }

    fn release(&mut self) -> ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome<()> {
        match self.guard.take() {
            Some(guard) => __jet_bootstrap_shared_outcome((self.release)(guard)),
            None => ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(Ok(()), None),
        }
    }

    fn release_during_drop(&mut self) {
        drop(self.guard.take());
    }

    fn root_ptr(&mut self) -> Result<*mut (), String> {
        let guard = self
            .guard
            .as_ref()
            .ok_or_else(|| "Shared guard was already released".to_string())?;
        (self.root_ptr)(guard)
    }

    fn mark_dirty(&mut self) -> Result<(), String> {
        let guard = self
            .guard
            .as_ref()
            .ok_or_else(|| "Shared guard was already released".to_string())?;
        (self.mark_dirty)(guard)
    }
}
fn __jet_bootstrap_entry_type_for(
    program: &::jet_foundation::MIR::MirProgram,
    entry: ::jet_foundation::MIR::MirFunctionId,
    result: bool,
) -> Result<::jet_foundation::MIR::MirType, String> {
    let function = program.functions.iter().find(|function| function.id == entry)
        .ok_or_else(|| format!("checked compiler entry {:?} is absent", entry))?;
    if result {
        Ok(function.return_type.clone())
    } else {
        function.params.first().map(|parameter| parameter.ty.clone())
            .ok_or_else(|| "checked compiler entry has no request parameter".to_string())
    }
}

fn __jet_bootstrap_entry_field_type(
    program: &::jet_foundation::MIR::MirProgram,
    owner: ::jet_foundation::MIR::MirTypeId,
    field: ::jet_foundation::MIR::MirFieldId,
) -> Result<::jet_foundation::MIR::MirType, String> {
    let definition = program.types.iter().find(|definition| definition.id == owner)
        .ok_or_else(|| format!("checked entry type {:?} is absent", owner))?;
    let ::jet_foundation::MIR::MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
        return Err(format!("checked entry type `{}` is not a struct", definition.name));
    };
    fields.iter().find(|row| row.id == field).map(|row| row.ty.clone())
        .ok_or_else(|| format!("checked entry field {:?} is absent from `{}`", field, definition.name))
}

fn __jet_bootstrap_entry_variant_field_type(
    program: &::jet_foundation::MIR::MirProgram,
    owner: ::jet_foundation::MIR::MirTypeId,
    variant: &str,
    field: usize,
) -> Result<::jet_foundation::MIR::MirType, String> {
    let definition = program.types.iter().find(|definition| definition.id == owner)
        .ok_or_else(|| format!("checked entry type {:?} is absent", owner))?;
    let ::jet_foundation::MIR::MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
        return Err(format!("checked entry type `{}` is not an enum", definition.name));
    };
    let row = variants.iter().find(|row| row.name == variant)
        .ok_or_else(|| format!("checked entry variant `{}::{}` is absent", definition.name, variant))?;
    match &row.payload {
        ::jet_foundation::MIR::MirVariantPayload::Single(ty) if field == 0 => Ok(ty.clone()),
        ::jet_foundation::MIR::MirVariantPayload::Named(fields) => fields.get(field)
            .map(|row| row.ty.clone())
            .ok_or_else(|| format!("checked entry enum field index {field} is absent")),
        _ => Err(format!("checked entry enum variant `{}::{}` has no field at index {field}", definition.name, variant)),
    }
}

fn __jet_bootstrap_entry_distinct_base(
    program: &::jet_foundation::MIR::MirProgram,
    owner: ::jet_foundation::MIR::MirTypeId,
) -> Result<::jet_foundation::MIR::MirType, String> {
    let definition = program.types.iter().find(|definition| definition.id == owner)
        .ok_or_else(|| format!("checked entry type {:?} is absent", owner))?;
    match &definition.kind {
        ::jet_foundation::MIR::MirTypeDefKind::Distinct { base, .. } => Ok(base.clone()),
        _ => Err(format!("checked entry type `{}` is not distinct", definition.name)),
    }
}

fn __jet_bootstrap_entry_child_type<'a>(
    ty: &'a ::jet_foundation::MIR::MirType,
    kind: &str,
    index: usize,
) -> Result<&'a ::jet_foundation::MIR::MirType, String> {
    use ::jet_foundation::MIR::MirTypeKind as K;
    match (kind, ty.kind()) {
        ("list", K::List(inner)) | ("fixed-list", K::FixedList { elem: inner, .. })
        | ("option", K::Option(inner)) | ("shared", K::Shared(inner))
        | ("range", K::InlineRange { base: inner, .. })
        | ("tagged", K::Tagged { inner, .. })
        | ("quantity", K::Quantity { base: inner, .. }) => Ok(inner),
        ("map", K::Map { key, .. }) if index == 0 => Ok(key),
        ("map", K::Map { value, .. }) if index == 1 => Ok(value),
        ("result", K::Result { ok, .. }) if index == 0 => Ok(ok),
        ("result", K::Result { err, .. }) if index == 1 => Ok(err),
        ("tuple", K::Tuple(fields)) => fields.get(index).map(|(_, ty)| ty)
            .ok_or_else(|| "checked tuple field is absent".to_string()),
        _ => Err(format!("checked MIR type `{}` has no {kind} child {index}", ty.canonical_key())),
    }
}

fn __jet_bootstrap_entry_next_record_field(
    fields: &mut impl Iterator<Item = (String, ::jet_foundation::MIR::MirRuntimeValue)>,
    expected: &str,
) -> Result<(String, ::jet_foundation::MIR::MirRuntimeValue), String> {
    let (name, value) = fields.next().ok_or_else(|| format!("record is missing checked field `{expected}`"))?;
    if name != expected {
        return Err(format!("record field `{name}` is out of checked order; expected `{expected}`"));
    }
    Ok((name, value))
}

fn __jet_bootstrap_entry_next_enum_arg(
    args: &mut impl Iterator<Item = (Option<String>, ::jet_foundation::MIR::MirRuntimeValue)>,
    expected: Option<&str>,
) -> Result<(Option<String>, ::jet_foundation::MIR::MirRuntimeValue), String> {
    let (name, value) = args.next().ok_or_else(|| "enum payload is missing a checked field".to_string())?;
    if name.as_deref() != expected {
        return Err("enum payload field name/order differs from checked MIR".to_string());
    }
    Ok((name, value))
}

fn __jet_bootstrap_entry_take_tuple_field(
    fields: &mut Vec<(String, ::jet_foundation::MIR::MirRuntimeValue)>,
    index: usize,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    let _ = index;
    if fields.is_empty() {
        return Err("tuple result is missing a checked field".to_string());
    }
    Ok(fields.remove(0).1)
}

fn __jet_bootstrap_entry_integer_to_runtime(
    value: &jet_foundation::Numeric::JetInt,
) -> ::jet_foundation::MIR::MirRuntimeValue {
    value.to_i64()
        .map(::jet_foundation::MIR::MirRuntimeValue::Int)
        .unwrap_or_else(|| ::jet_foundation::MIR::MirRuntimeValue::BigInt(value.to_string_rep()))
}

fn __jet_bootstrap_entry_integer_from_runtime(
    value: ::jet_foundation::MIR::MirRuntimeValue,
) -> Result<jet_foundation::Numeric::JetInt, String> {
    match value {
        ::jet_foundation::MIR::MirRuntimeValue::Int(value) =>
            Ok(jet_foundation::Numeric::JetInt::from_i64(value)),
        ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) =>
            Ok(jet_foundation::Numeric::JetInt::from_big(
                jet_foundation::Numeric::CtBigInt::from_str(&value).map_err(|error| error.to_string())?,
            )),
        _ => Err("entry integer does not have an exact integer carrier".to_string()),
    }
}

fn __jet_bootstrap_entry_integer_text(
    value: ::jet_foundation::MIR::MirRuntimeValue,
) -> Result<String, String> {
    match value {
        ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(value.to_string()),
        ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) => Ok(value),
        _ => Err("entry fixed-width integer does not have an exact integer carrier".to_string()),
    }
}
"#,
    );
    Ok(())
}

fn emit_entry_machine_access_helpers(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let machine_type = symbols.type_symbol("JetEvalMachine")?;
    let machine_program_field = symbols.field_symbol("JetEvalMachine", "program")?;
    let program_core_owners_field = symbols.field_symbol("MIRProgram", "core_owners")?;
    let program_definition = program
        .types
        .iter()
        .find(|definition| definition.name == "MIRProgram")
        .ok_or_else(|| BootstrapHostCodecError::MissingType("MIRProgram".to_string()))?;
    let MirTypeDefKind::Struct {
        fields: program_fields,
        ..
    } = &program_definition.kind
    else {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "checked MirProgram is not a struct".to_string(),
        ));
    };
    let core_owners_type = &program_fields
        .iter()
        .find(|field| field.name == "core_owners")
        .ok_or_else(|| BootstrapHostCodecError::MissingField {
            owner: "MIRProgram".to_string(),
            field: "core_owners".to_string(),
        })?
        .ty;
    if !matches!(core_owners_type.kind(), MirTypeKind::List(_)) {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "checked MirProgram.core_owners is not a List".to_string(),
        ));
    }
    let core_owners_source = entry_rust_type(program, symbols, core_owners_type)?;
    let core_owner_definition = checked_nominal_definition(
        program,
        match core_owners_type.kind() {
            MirTypeKind::List(inner) => inner,
            _ => unreachable!(),
        },
    )?;
    if core_owner_definition.name != "MIRCoreOwner" {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "checked MirProgram.core_owners does not contain MirCoreOwner rows".to_string(),
        ));
    }
    let decoded_rows = from_runtime_expression(
        program,
        symbols,
        core_owners_type,
        "(*__jet_core_owner_runtime).clone()",
        "&__jet_core_owner_rows_type",
        "compiler_program",
        "&__jet_core_owner_path",
    )?;
    let mut generated = String::from(
        r#"
impl crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows for __MACHINE_TYPE__ {
    fn core_owner_rows(&self) -> Result<Vec<::jet_foundation::MIR::MirCoreOwner>, String> {
        self.__MACHINE_PROGRAM_FIELD__.__PROGRAM_CORE_OWNERS_FIELD__
            .iter()
            .map(__jet_bootstrap_mir_MIRCoreOwner_to_host)
            .collect()
    }
}

#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_verify_core_owner<T, P>(
    machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
    owner: &::jet_foundation::MIR::MirCoreOwner,
    compiler_program: &::jet_foundation::MIR::MirProgram,
    physical: &P,
) -> Result<bool, String>
where
    T: crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows,
    P: crate::BootstrapEntryPhysicalBindings,
{
    match machine {
        crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Typed(machine) => {
            let mut found = false;
            for row in crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows::core_owner_rows(&**machine)?
                .iter()
                .filter(|row| row.id == owner.id)
            {
                if found {
                    return Err("Core owner registration ID is not unique".to_string());
                }
                if row != owner {
                    return Err("Core owner fact disagrees with its registered MIR row".to_string());
                }
                found = true;
            }
            Ok(found)
        }
        crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Mir(machine) => {
            let __jet_core_owner_rows_type =
                machine.field_type_path(&["program", "core_owners"])?;
            let (_, __jet_core_owner_runtime) =
                machine.projection_mut_fields(&["program", "core_owners"])?;
            crate::compiler_bootstrap_entry_codec::validate_runtime_value(
                compiler_program,
                &__jet_core_owner_rows_type,
                __jet_core_owner_runtime,
                0,
            )?;
            let __jet_core_owner_path = vec![
                "JetEvalMachine".to_string(),
                "program".to_string(),
                "core_owners".to_string(),
            ];
            let __jet_core_owner_source: __CORE_OWNERS_SOURCE__ = {__DECODED_ROWS__};
            let __jet_core_owner_rows: Vec<::jet_foundation::MIR::MirCoreOwner> = __jet_core_owner_source
                .iter()
                .map(__jet_bootstrap_mir_MIRCoreOwner_to_host)
                .collect::<Result<Vec<_>, String>>()?;
            let mut found = false;
            for row in __jet_core_owner_rows.iter().filter(|row| row.id == owner.id) {
                if found {
                    return Err("Core owner registration ID is not unique".to_string());
                }
                if row != owner {
                    return Err("Core owner fact disagrees with its registered MIR row".to_string());
                }
                found = true;
            }
            let _ = physical;
            Ok(found)
        }
    }
}
"#,
    );
    for (token, replacement) in [
        ("__MACHINE_TYPE__", machine_type),
        ("__MACHINE_PROGRAM_FIELD__", machine_program_field),
        ("__PROGRAM_CORE_OWNERS_FIELD__", program_core_owners_field),
        ("__DECODED_ROWS__", decoded_rows.as_str()),
        ("__CORE_OWNERS_SOURCE__", core_owners_source.as_str()),
    ] {
        generated = generated.replace(token, replacement);
    }
    out.push_str(&generated);
    Ok(())
}

fn emit_to_runtime_converter(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    definition: &MirTypeDef,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let id = definition.id.0;
    let body = match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            let mut rows = Vec::with_capacity(fields.len());
            for field in fields {
                let symbol = symbols.field_symbol(&definition.name, &field.name)?;
                let field_source = if definition.name == "JetEvalOwnedRoot" && field.name == "config" {
                    format!("&{}", symbols.call("jet_eval_owned_root_config", &[("*value", "value.clone()")])?)
                } else if BootstrapCodecSymbols::boxed_edge(definition, &field.name) {
                    format!("&*value.{symbol}")
                } else {
                    format!("&value.{symbol}")
                };
                let expr = to_runtime_expression(
                    program,
                    symbols,
                    &field.ty,
                    &field_source,
                    &format!("&__jet_field_type_{}", field.id.0),
                    "program",
                    &format!("&__jet_field_path_{}", field.id.0),
                    "physical",
                )?;
                rows.push(format!(
                    "let __jet_field_type_{field_id} = __jet_bootstrap_entry_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), ::jet_foundation::MIR::MirFieldId({field_id}))?; let mut __jet_field_path_{field_id} = path.to_vec(); __jet_field_path_{field_id}.push({field_name:?}.to_string()); fields.push(({field_name:?}.to_string(), {expr}));",
                    owner = id,
                    field_id = field.id.0,
                    field_name = field.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry record type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let mut fields = Vec::with_capacity({field_count}); {rows} Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name: {name:?}.to_string(), fields }})",
                id = id,
                field_count = fields.len(),
                rows = rows.join(" "),
                name = definition.name,
            )
        }
        MirTypeDefKind::Enum { variants, .. } => {
            let mut arms = Vec::with_capacity(variants.len());
            for variant in variants {
                let path = symbols.variant_path(&definition.name, &variant.name)?;
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![(None, None, ty)],
                    MirVariantPayload::Named(fields) => fields
                        .iter()
                        .map(|field| {
                            Ok((
                                Some(field.name.as_str()),
                                Some(symbols.field_binding_by_id(definition.id, &definition.name, field.id, &field.name)?.symbol.as_str()),
                                &field.ty,
                            ))
                        })
                        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?,
                };
                let bindings = (0..fields.len())
                    .map(|index| format!("__payload_{index}"))
                    .collect::<Vec<_>>();
                let pattern = if fields.is_empty() {
                    path.clone()
                } else if fields.iter().all(|(_, symbol, _)| symbol.is_some()) {
                    format!(
                        "{path} {{ {} }}",
                        fields
                            .iter()
                            .zip(&bindings)
                            .map(|((_, symbol, _), binding)| format!("{}: {binding}", symbol.unwrap()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    format!("{path}({})", bindings.join(", "))
                };
                let mut encode = Vec::with_capacity(fields.len());
                for (index, ((name, _, field_ty), binding)) in fields.iter().zip(&bindings).enumerate() {
                    let ty_var = format!("__jet_variant_type_{index}");
                    let path_var = format!("__jet_variant_path_{index}");
                    let edge = match name {
                        Some(name) => format!("{}.{name}", variant.name),
                        None => variant.name.clone(),
                    };
                    let source = if BootstrapCodecSymbols::boxed_edge(definition, &edge) {
                        format!("&**{binding}")
                    } else {
                        binding.to_string()
                    };
                    let value = to_runtime_expression(
                        program,
                        symbols,
                        field_ty,
                        &source,
                        &format!("&{ty_var}"),
                        "program",
                        &format!("&{path_var}"),
                        "physical",
                    )?;
                    let name_expr = name.map_or_else(|| "None".to_string(), |name| format!("Some({name:?}.to_string())"));
                    encode.push(format!(
                        "let {ty_var} = __jet_bootstrap_entry_variant_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), {variant_name:?}, {index})?; let mut {path_var} = path.to_vec(); {path_var}.push(format!(\"{{}}::{{}}[{index}]\", {owner_name:?}, {variant_name:?})); args.push(({name_expr}, {value}));",
                        owner = id,
                        variant_name = variant.name,
                        owner_name = definition.name,
                    ));
                }
                arms.push(format!(
                    "{pattern} => {{ let mut args = Vec::with_capacity({payload_len}); {encode} Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name: {owner_name:?}.to_string(), variant: {variant_name:?}.to_string(), args }}) }},",
                    payload_len = fields.len(),
                    encode = encode.join(" "),
                    owner_name = definition.name,
                    variant_name = variant.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry enum type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} match value {{ {} }}",
                arms.join(" "),
                id = id,
            )
        }
        MirTypeDefKind::Distinct { base, .. } => {
            let expr = to_runtime_expression(
                program,
                symbols,
                base,
                "&value.0",
                "&__jet_distinct_base",
                "program",
                "path",
                "physical",
            )?;
            format!(
                "let __jet_distinct_base = __jet_bootstrap_entry_distinct_base(program, ::jet_foundation::MIR::MirTypeId({id}))?; {expr}",
                id = id,
            )
        }
        MirTypeDefKind::Alias { target } => to_runtime_expression(
            program,
            symbols,
            target,
            "value",
            "ty",
            "program",
            "path",
            "physical",
        )?,
        MirTypeDefKind::UnitFamily { members } => {
            let mut arms = Vec::with_capacity(members.len());
            for member in members {
                let path = unit_family_variant_path(symbols, &definition.name, member)?;
                arms.push(format!(
                    "{path} => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name: {name:?}.to_string(), variant: {member:?}.to_string(), args: Vec::new() }}),",
                    name = definition.name,
                ));
            }
            format!("match value {{ {} }}", arms.join(" "))
        }
    };
writeln!(
    out,
    "fn __jet_bootstrap_entry_type_{id}_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: &{source}, ty: &::jet_foundation::MIR::MirType, program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>, path: &[String], physical: &P) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{ let checked = ty; {body} }}\n",
    id = id,
)
.map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_from_runtime_converter(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    definition: &MirTypeDef,
) -> Result<(), BootstrapHostCodecError> {
    let source = symbols.type_symbol(&definition.name)?;
    let id = definition.id.0;
    let body = match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            let mut binds = Vec::with_capacity(fields.len());
            let mut initializers = Vec::with_capacity(fields.len());
            let mut decoded_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let symbol = symbols.field_symbol(&definition.name, &field.name)?;
                let value_name = format!("__jet_field_value_{}", field.id.0);
                let type_name = format!("__jet_field_type_{}", field.id.0);
                let decoded_name = format!("__jet_decoded_field_{}", field.id.0);
                let expr = from_runtime_expression(
                    program,
                    symbols,
                    &field.ty,
                    &value_name,
                    &format!("&{type_name}"),
                    "program",
                    &format!("&{{ let mut p = path.to_vec(); p.push({:?}.to_string()); p }}", field.name),
                )?;
                binds.push(format!(
                    "let {type_name} = __jet_bootstrap_entry_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), ::jet_foundation::MIR::MirFieldId({field_id}))?; let (_, {value_name}) = __jet_bootstrap_entry_next_record_field(&mut fields, {field_name:?})?; let {decoded_name} = {expr};",
                    owner = id,
                    field_id = field.id.0,
                    field_name = field.name,
                ));
                if BootstrapCodecSymbols::boxed_edge(definition, &field.name) {
                    initializers.push(format!("{symbol}: Box::new({decoded_name})"));
                } else {
                    initializers.push(format!("{symbol}: {decoded_name}"));
                }
                decoded_fields.push((field.name.as_str(), decoded_name));
            }
            let construction = if definition.name == "JetEvalOwnedRoot" {
                let component = |name: &str| {
                    decoded_fields.iter().find(|(field, _)| *field == name)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(format!(
                            "checked JetEvalOwnedRoot is missing `{name}`"
                        )))
                };
                let program_value = component("program")?;
                let registry_value = component("registry")?;
                let config_value = component("config")?;
                symbols.call(
                    "jet_eval_owned_root_from_parts",
                    &[
                        (program_value.as_str(), program_value.as_str()),
                        (registry_value.as_str(), registry_value.as_str()),
                        (config_value.as_str(), config_value.as_str()),
                    ],
                )?
            } else {
                format!("{source} {{ {} }}", initializers.join(", "))
            };
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry record type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields }} = value else {{ return Err(format!(\"expected record carrier for `{{}}`\", {name:?})); }}; if type_name != {name:?} {{ return Err(format!(\"record carrier type mismatch for `{{}}`\", {name:?})); }} let mut fields = fields.into_iter(); {} if fields.next().is_some() {{ return Err(format!(\"record `{{}}` contains undeclared fields\", {name:?})); }} Ok({construction})",
                binds.join(" "),
                id = id,
                name = definition.name,
                construction = construction,
            )
        }
        MirTypeDefKind::Enum { variants, .. } => {
            let mut arms = Vec::with_capacity(variants.len());
            for variant in variants {
                let path = symbols.variant_path(&definition.name, &variant.name)?;
                let fields = match &variant.payload {
                    MirVariantPayload::Unit => Vec::new(),
                    MirVariantPayload::Single(ty) => vec![(None, None, ty)],
                    MirVariantPayload::Named(fields) => fields
                        .iter()
                        .map(|field| {
                            Ok((
                                Some(field.name.as_str()),
                                Some(symbols.field_binding_by_id(definition.id, &definition.name, field.id, &field.name)?.symbol.as_str()),
                                &field.ty,
                            ))
                        })
                        .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?,
                };
                let mut decode = Vec::with_capacity(fields.len());
                let mut payload_values = Vec::with_capacity(fields.len());
                for (index, (name, symbol, field_ty)) in fields.iter().enumerate() {
                    let arg = format!("__jet_arg_{index}");
                    let ty_var = format!("__jet_arg_type_{index}");
                    let expr = from_runtime_expression(
                        program,
                        symbols,
                        field_ty,
                        &format!("{arg}.1"),
                        &format!("&{ty_var}"),
                        "program",
                        &format!(
                            "&{{ let mut p = path.to_vec(); p.push(format!(\"{{}}::{{}}[{index}]\", {owner_name:?}, {variant_name:?})); p }}",
                            owner_name = definition.name,
                            variant_name = variant.name,
                        ),
                    )?;
                    let expected_name = name.map_or_else(|| "None".to_string(), |name| format!("Some({name:?})"));
                    decode.push(format!(
                        "let {ty_var} = __jet_bootstrap_entry_variant_field_type(program, ::jet_foundation::MIR::MirTypeId({owner}), {variant_name:?}, {index})?; let {arg} = __jet_bootstrap_entry_next_enum_arg(&mut args, {expected_name})?; let {arg}_value = {expr};",
                        owner = id,
                        variant_name = variant.name,
                    ));
                    let edge = match name {
                        Some(name) => format!("{}.{name}", variant.name),
                        None => variant.name.clone(),
                    };
                    let value = if BootstrapCodecSymbols::boxed_edge(definition, &edge) {
                        format!("Box::new({arg}_value)")
                    } else {
                        format!("{arg}_value")
                    };
                    payload_values.push(match symbol {
                        Some(symbol) if matches!(variant.payload, MirVariantPayload::Named(_)) => {
                            format!("{symbol}: {value}")
                        }
                        _ => value,
                    });
                }
                let construction = if payload_values.is_empty() {
                    path
                } else if matches!(variant.payload, MirVariantPayload::Named(_)) {
                    format!("{path} {{ {} }}", payload_values.join(", "))
                } else {
                    format!("{path}({})", payload_values.join(", "))
                };
                arms.push(format!(
                    "{variant_name:?} => {{ let mut args = args.into_iter(); {} if args.next().is_some() {{ return Err(\"enum contains excess payload values\".to_string()); }} Ok({construction}) }},",
                    decode.join(" "),
                    variant_name = variant.name,
                ));
            }
            format!(
                "let definition = program.types.iter().find(|definition| definition.id == ::jet_foundation::MIR::MirTypeId({id})).ok_or_else(|| \"checked entry enum type is absent\".to_string())?; if ty.nominal_id() != Some(definition.id) {{ return Err(format!(\"entry type mismatch for `{{}}`\", definition.name)); }} let ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} = value else {{ return Err(format!(\"expected enum carrier for `{{}}`\", {name:?})); }}; if type_name != {name:?} {{ return Err(format!(\"enum carrier type mismatch for `{{}}`\", {name:?})); }} match variant.as_str() {{ {} _ => Err(format!(\"unknown checked enum variant `{{}}::{{}}`\", {name:?}, variant)), }}",
                arms.join(" "),
                id = id,
                name = definition.name,
            )
        }
        MirTypeDefKind::Distinct { base, .. } => {
            let expr = from_runtime_expression(
                program,
                symbols,
                base,
                "value",
                "&__jet_distinct_base",
                "program",
                "path",
            )?;
            format!(
                "let __jet_distinct_base = __jet_bootstrap_entry_distinct_base(program, ::jet_foundation::MIR::MirTypeId({id}))?; Ok({source}({expr}))",
                id = id,
                source = source,
            )
        }
        MirTypeDefKind::Alias { target } => from_runtime_expression(
            program,
            symbols,
            target,
            "value",
            "ty",
            "program",
            "path",
        )?,
        MirTypeDefKind::UnitFamily { members } => {
            let mut arms = Vec::with_capacity(members.len());
            for member in members {
                let path = unit_family_variant_path(symbols, &definition.name, member)?;
                arms.push(format!("{member:?} => Ok({path}),"));
            }
            format!(
                "let ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} = value else {{ return Err(\"expected unit-family enum carrier\".to_string()); }}; if type_name != {name:?} || !args.is_empty() {{ return Err(\"unit-family carrier shape mismatch\".to_string()); }} match variant.as_str() {{ {} _ => Err(format!(\"unknown checked unit-family member `{{}}`\", variant)), }}",
                arms.join(" "),
                name = definition.name,
            )
        }
    };
    writeln!(
        out,
        "fn __jet_bootstrap_entry_type_{id}_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: ::jet_foundation::MIR::MirRuntimeValue, ty: &::jet_foundation::MIR::MirType, program: &::jet_foundation::MIR::MirProgram, path: &[String], physical: &P) -> Result<{source}, String> {{ let checked = ty; {body} }}\n",
        id = id,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn unit_family_variant_path(
    symbols: &BootstrapCodecSymbols<'_>,
    owner: &str,
    member: &str,
) -> Result<String, BootstrapHostCodecError> {
    Ok(format!(
        "{}::{}",
        symbols.type_symbol(owner)?,
        jet_foundation::Names::mangle(member),
    ))
}

fn shared_to_runtime_expression(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    inner: &MirType,
    source: &str,
    checked: &str,
    program_expr: &str,
    path: &str,
    physical: &str,
) -> Result<String, BootstrapHostCodecError> {
    let payload_type = entry_rust_type(program, symbols, inner)?;
    let encoded = to_runtime_expression(
        program,
        symbols,
        inner,
        "payload",
        "&__jet_shared_inner_type",
        "program",
        "&__jet_shared_payload_path",
        "physical",
    )?;
    let decoded = from_runtime_expression(
        program,
        symbols,
        inner,
        "__jet_shared_runtime",
        "&__jet_shared_inner_type",
        "program",
        "&__jet_shared_payload_path",
    )?;
    // The guard is the runtime's own carrier (the emitter spells `Shared` guards
    // `jet_std::JetSharedGuard<T>`), not a checked Source definition, so it has
    // no binding-metadata symbol.
    let guard_type = format!("crate::jet_std::JetSharedGuard<{payload_type}>");
    Ok(format!(
        r#"{{
            use crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedMarshaller as _;
            fn __jet_bootstrap_shared_payload_encode<P: crate::BootstrapEntryPhysicalBindings>(
                payload: &{payload_type},
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                physical: &P,
            ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
                let __jet_shared_inner_type = checked_type.clone();
                let checked = &__jet_shared_inner_type;
                let mut __jet_shared_payload_path = field_path.to_vec();
                __jet_shared_payload_path.push("$shared_payload".to_string());
                let __jet_runtime = {encoded};
                Ok(__jet_runtime)
            }}

            fn __jet_bootstrap_shared_payload_decode<P: crate::BootstrapEntryPhysicalBindings>(
                __jet_shared_runtime: ::jet_foundation::MIR::MirRuntimeValue,
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                physical: &P,
            ) -> Result<{payload_type}, String> {{
                let __jet_shared_inner_type = checked_type.clone();
                let checked = &__jet_shared_inner_type;
                let program = program.as_ref();
                let mut __jet_shared_payload_path = field_path.to_vec();
                __jet_shared_payload_path.push("$shared_payload".to_string());
                Ok({decoded})
            }}

            let __jet_shared_inner_type =
                __jet_bootstrap_entry_child_type({checked}, "shared", 0)?.clone();
            let __jet_shared_identity = ({source}).physical_identity();
            let __jet_shared_owner = ({source}).physical_owner();
            let __jet_shared_type_id =
                ::jet_jit::SourceSharedInterop::checked_type_id_for_program(
                    {program_expr}.as_ref(),
                    checked,
                );
            let __jet_shared_shape =
                {physical}.shared_payload_shape(&__jet_shared_inner_type)?;
            let __jet_shared_marshaller = ::std::sync::Arc::new(
                {physical}.shared_marshaller(
                    __jet_shared_owner.clone(),
                    &__jet_shared_inner_type,
                    &__jet_shared_shape,
                    __jet_bootstrap_shared_payload_encode::<P>,
                    __jet_bootstrap_shared_payload_decode::<P>,
                )?,
            );
            let __jet_shared_base_path: ::std::sync::Arc<[String]> =
                ::std::sync::Arc::from(({path}).to_vec());
            let __jet_shared_owner_build = __jet_shared_owner.clone();
            let __jet_shared_marshaller_build =
                ::std::sync::Arc::clone(&__jet_shared_marshaller);
            let __jet_shared_inner_type_build =
                ::std::sync::Arc::new(__jet_shared_inner_type);
            let __jet_shared_shape = ::std::sync::Arc::new(__jet_shared_shape);
            let __jet_shared_shape_build =
                ::std::sync::Arc::clone(&__jet_shared_shape);
            let __jet_shared_build: ::std::sync::Arc<
                dyn Fn() -> Result<::jet_jit::SourceSharedInterop::SourceSharedInterop, String>
                    + Send
                    + Sync,
            > = ::std::sync::Arc::new(move || {{
                let __jet_shared_read_owner = __jet_shared_owner_build.clone();
                let __jet_shared_edit_owner = __jet_shared_owner_build.clone();
                let __jet_shared_guard_owner = __jet_shared_owner_build.clone();
                let __jet_shared_capture_owner = __jet_shared_owner_build.clone();
                let __jet_shared_replace_owner = __jet_shared_owner_build.clone();
                let __jet_shared_read_marshaller =
                    ::std::sync::Arc::clone(&__jet_shared_marshaller_build);
                let __jet_shared_edit_marshaller =
                    ::std::sync::Arc::clone(&__jet_shared_marshaller_build);
                let __jet_shared_guard_marshaller =
                    ::std::sync::Arc::clone(&__jet_shared_marshaller_build);
                let __jet_shared_capture_marshaller =
                    ::std::sync::Arc::clone(&__jet_shared_marshaller_build);
                let __jet_shared_replace_marshaller =
                    ::std::sync::Arc::clone(&__jet_shared_marshaller_build);
                let __jet_shared_read_type =
                    ::std::sync::Arc::clone(&__jet_shared_inner_type_build);
                let __jet_shared_edit_type =
                    ::std::sync::Arc::clone(&__jet_shared_inner_type_build);
                let __jet_shared_guard_type =
                    ::std::sync::Arc::clone(&__jet_shared_inner_type_build);
                let __jet_shared_capture_type =
                    ::std::sync::Arc::clone(&__jet_shared_inner_type_build);
                let __jet_shared_replace_type =
                    ::std::sync::Arc::clone(&__jet_shared_inner_type_build);
                let __jet_shared_read_shape =
                    ::std::sync::Arc::clone(&__jet_shared_shape_build);
                let __jet_shared_edit_shape =
                    ::std::sync::Arc::clone(&__jet_shared_shape_build);
                let __jet_shared_guard_shape =
                    ::std::sync::Arc::clone(&__jet_shared_shape_build);
                let __jet_shared_capture_shape =
                    ::std::sync::Arc::clone(&__jet_shared_shape_build);
                let __jet_shared_replace_shape =
                    ::std::sync::Arc::clone(&__jet_shared_shape_build);
                let __jet_shared_read_path =
                    ::std::sync::Arc::clone(&__jet_shared_base_path);
                let __jet_shared_edit_path =
                    ::std::sync::Arc::clone(&__jet_shared_base_path);
                let __jet_shared_guard_path =
                    ::std::sync::Arc::clone(&__jet_shared_base_path);
                let __jet_shared_capture_path =
                    ::std::sync::Arc::clone(&__jet_shared_base_path);
                let __jet_shared_replace_path =
                    ::std::sync::Arc::clone(&__jet_shared_base_path);
                Ok(::jet_jit::SourceSharedInterop::SourceSharedInterop::from_callbacks_with_guard(
                    __jet_shared_type_id,
                    move |callback: &mut dyn FnMut(
                        &::jet_foundation::MIR::MirRuntimeValue,
                    ) -> Result<(), String>| {{
                        let operation = __jet_shared_read_owner.with_live_root(|payload, _revision| {{
                            let runtime_value = __jet_shared_read_marshaller.encode_payload(
                                &__jet_shared_read_path,
                                &[],
                                &__jet_shared_read_type,
                                &__jet_shared_read_shape,
                                __jet_shared_read_shape.root,
                                payload,
                            )?;
                            callback(&runtime_value)
                        }});
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(result)) => result,
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(result, completion)
                    }},
                    move |callback: &mut dyn FnMut(
                        &mut ::jet_foundation::MIR::MirRuntimeValue,
                    ) -> Result<(), String>| {{
                        let operation = __jet_shared_edit_owner.with_live_edit(|payload| {{
                            let mut runtime_value = __jet_shared_edit_marshaller.encode_payload(
                                &__jet_shared_edit_path,
                                &[],
                                &__jet_shared_edit_type,
                                &__jet_shared_edit_shape,
                                __jet_shared_edit_shape.root,
                                payload,
                            )?;
                            callback(&mut runtime_value)?;
                            let replacement = __jet_shared_edit_marshaller.decode_payload(
                                &__jet_shared_edit_path,
                                &[],
                                &__jet_shared_edit_type,
                                &__jet_shared_edit_shape,
                                __jet_shared_edit_shape.root,
                                runtime_value,
                            )?;
                            *payload = replacement;
                            Ok(())
                        }});
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(result)) => result,
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(result, completion)
                    }},
                    move |editable: bool| {{
                        let operation =
                            __jet_shared_guard_owner.acquire_physical_guard(editable);
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(guard)) => {{
                                let read_marshaller = __jet_shared_guard_marshaller.clone();
                                let stage_marshaller = __jet_shared_guard_marshaller.clone();
                                let read_type = __jet_shared_guard_type.clone();
                                let stage_type = __jet_shared_guard_type.clone();
                                let read_shape = __jet_shared_guard_shape.clone();
                                let stage_shape = __jet_shared_guard_shape.clone();
                                let read_path = __jet_shared_guard_path.clone();
                                let stage_path = __jet_shared_guard_path.clone();
                                let read = move |guard: &{guard_type}| {{
                                    read_marshaller.encode_payload(
                                        &read_path,
                                        &[],
                                        &read_type,
                                        &read_shape,
                                        read_shape.root,
                                        &**guard,
                                    )
                                }};
                                let stage = move |
                                    guard: &mut {guard_type},
                                    runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
                                | {{
                                    let replacement = stage_marshaller.decode_payload(
                                        &stage_path,
                                        &[],
                                        &stage_type,
                                        &stage_shape,
                                        stage_shape.root,
                                        runtime_value,
                                    )?;
                                    **guard = replacement;
                                    Ok(())
                                }};
                                let suspend =
                                    move |guard: &mut {guard_type}| guard.physical_wait_suspend();
                                let resume = move |
                                    guard: &mut {guard_type},
                                    cancelled: &mut dyn FnMut() -> bool,
                                | guard.physical_wait_resume(cancelled);
                                let held =
                                    move |guard: &{guard_type}| guard.physical_permit_held();
                                let abort =
                                    move |guard: &mut {guard_type}| guard.physical_wait_abort();
                                let root_ptr =
                                    move |guard: &{guard_type}| guard.physical_root_ptr();
                                let mark_dirty =
                                    move |guard: &{guard_type}| guard.physical_mark_dirty();
                                let revision =
                                    move |guard: &{guard_type}| guard.physical_revision();
                                let release =
                                    move |guard: {guard_type}| guard.release();
                                Ok(Box::new(__JetBootstrapSharedGuard {{
                                    guard: Some(guard),
                                    read,
                                    stage,
                                    suspend,
                                    resume,
                                    held,
                                    abort,
                                    root_ptr,
                                    mark_dirty,
                                    revision,
                                    release,
                                }}) as Box<dyn ::jet_jit::SourceSharedInterop::SourceSharedInteropGuard>)
                            }},
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(result, completion)
                    }},
                    move |callback: &mut dyn FnMut(
                        &::jet_foundation::MIR::MirRuntimeValue,
                        u64,
                    ) -> Result<(), String>| {{
                        let operation =
                            __jet_shared_capture_owner.with_live_root(|payload, revision| {{
                                let runtime_value =
                                    __jet_shared_capture_marshaller.encode_payload(
                                        &__jet_shared_capture_path,
                                        &[],
                                        &__jet_shared_capture_type,
                                        &__jet_shared_capture_shape,
                                        __jet_shared_capture_shape.root,
                                        payload,
                                    )?;
                                callback(&runtime_value, revision)
                            }});
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(result)) => result,
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(result, completion)
                    }},
                    move |expected_revision: u64,
                          runtime_value: ::jet_foundation::MIR::MirRuntimeValue| {{
                        let replacement = match __jet_shared_replace_marshaller.decode_payload(
                            &__jet_shared_replace_path,
                            &[],
                            &__jet_shared_replace_type,
                            &__jet_shared_replace_shape,
                            __jet_shared_replace_shape.root,
                            runtime_value,
                        ) {{
                            Ok(replacement) => replacement,
                            Err(error) => {{
                                return ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(
                                    Err(error),
                                    None,
                                );
                            }}
                        }};
                        let operation = __jet_shared_replace_owner
                            .replace_if_revision(expected_revision, replacement);
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(result)) => Ok(result),
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        ::jet_jit::SourceSharedInterop::SourceSharedInteropOperationOutcome::new(result, completion)
                    }},
                ))
            }});
            let __jet_shared_root =
                __jet_bootstrap_shared_source_interop_root(&__jet_shared_owner, __jet_shared_type_id, __jet_shared_build)?;
            {physical}.encode_shared(
                {path},
                checked,
                {program_expr}.as_ref(),
                __jet_shared_identity,
                __jet_shared_root,
                &__jet_shared_shape,
            )?
            .ok_or_else(|| format!(
                "NativeAdapter has no carrier for checked Shared field `{{}}`",
                {path}.join("."),
            ))?
        }}"#
    ))
}
fn to_runtime_expression(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    source: &str,
    checked: &str,
    program_expr: &str,
    path: &str,
    physical: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let mir = "::jet_foundation::MIR::MirRuntimeValue";
    Ok(match ty.kind() {
        Kind::Int => format!("__jet_bootstrap_entry_integer_to_runtime({source})"),
        Kind::IntN { .. } => format!("{mir}::BigInt(({source}).to_string())"),
        Kind::Float => format!("{mir}::Float {{ value: *({source}), f32: false }}"),
        Kind::Float32 => format!("{mir}::Float {{ value: f64::from(*({source})), f32: true }}"),
        Kind::Measure(_) => format!("{mir}::Float {{ value: *({source}), f32: false }}"),
        Kind::Bool => format!("{mir}::Bool(*({source}))"),
        Kind::Char => format!("{mir}::Char(*({source}))"),
        Kind::String => format!("{mir}::String(({source}).clone())"),
        Kind::List(inner) | Kind::FixedList { elem: inner, .. } => {
            let inner_ty = format!("__jet_bootstrap_entry_child_type({checked}, {}, 0)?", if matches!(ty.kind(), Kind::List(_)) { "\"list\"" } else { "\"fixed-list\"" });
            if is_u8_type(inner) {
                format!("{mir}::Bytes(({source}).iter().copied().collect())")
            } else {
                let child = to_runtime_expression(
                    program,
                    symbols,
                    inner,
                    "item",
                    "&__jet_item_type",
                    program_expr,
                    path,
                    physical,
                )?;
                format!("{{ let __jet_item_type = {inner_ty}; {mir}::List(({source}).iter().map(|item| Ok({child})).collect::<Result<Vec<_>, String>>()?) }}")
            }
        }
        Kind::Map { key, value } => {
            let key_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"map\", 0)?");
            let value_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"map\", 1)?");
            let encoded_key = to_const_key_expression(program, symbols, key, "key", "__jet_key_type", program_expr, path)?;
            let encoded_value = to_runtime_expression(program, symbols, value, "item", "__jet_value_type", program_expr, path, physical)?;
            format!("{{ let __jet_key_type = {key_ty}; let __jet_value_type = {value_ty}; {mir}::Map(({source}).iter().map(|(key, item)| Ok(({encoded_key}, {encoded_value}))).collect::<Result<Vec<_>, String>>()?) }}")
        }
        Kind::Shared(inner) if entry_type_is_thread_bound(program, inner, &mut HashSet::new()) => format!(
            "return Err(format!(\"checked Shared `{{}}` holds thread-bound Source values and has no physical interop root\", {path}.join(\".\")))"
        ),
        Kind::Shared(inner) => shared_to_runtime_expression(
            program, symbols, inner, source, checked, program_expr, path, physical,
        )?,
        Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("{physical}.encode({path}, {checked}, {program_expr}.as_ref(), {source})?.ok_or_else(|| format!(\"NativeAdapter has no carrier for checked physical field `{{}}`\", {path}.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"option\", 0)?");
            let child = to_runtime_expression(program, symbols, inner, "inner", "&__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {inner_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({child})), Err(_) => {mir}::Absent {{ element: __jet_inner_type.clone() }} }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"result\", 0)?");
            let err_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"result\", 1)?");
            let ok_value = to_runtime_expression(program, symbols, ok, "inner", "&__jet_ok_type", program_expr, path, physical)?;
            let err_value = to_runtime_expression(program, symbols, err, "inner", "&__jet_err_type", program_expr, path, physical)?;
            format!("{{ let __jet_ok_type = {ok_ty}; let __jet_err_type = {err_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({ok_value})), Err(inner) => {mir}::FailedTold(Box::new({err_value})) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_ref(program, name)?;
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, {checked}, {program_expr}, {path}, {physical})?", name.id.0)
        }
        Kind::Tuple(fields) => {
            let mut items = Vec::with_capacity(fields.len());
            for (index, (name, field_ty)) in fields.iter().enumerate() {
                let ty_expr = format!("__jet_bootstrap_entry_child_type({checked}, \"tuple\", {index})?");
                let expr = to_runtime_expression(program, symbols, field_ty, &format!("&({source}).{index}"), &format!("__jet_tuple_type_{index}"), program_expr, path, physical)?;
                items.push(format!("{{ let __jet_tuple_type_{index} = {ty_expr}; ({name:?}.to_string(), {expr}) }}"));
            }
            if fields.is_empty() {
                format!("{mir}::Unit")
            } else {
                format!("{mir}::Struct {{ type_name: \"Tuple\".to_string(), fields: vec![{}] }}", items.join(", "))
            }
        }
        Kind::InlineRange { base, .. } | Kind::Tagged { inner: base, .. } | Kind::Quantity { base, .. } => {
            let kind = match ty.kind() {
                Kind::InlineRange { .. } => "range",
                Kind::Tagged { .. } => "tagged",
                _ => "quantity",
            };
            let child_ty = format!("__jet_bootstrap_entry_child_type({checked}, \"{kind}\", 0)?");
            let child = to_runtime_expression(program, symbols, base, source, "__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {child_ty}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, {checked}, {program_expr}, {path}, {physical})?", id.0)
        }
    })
}

fn from_runtime_expression(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    runtime: &str,
    checked: &str,
    program_expr: &str,
    path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let value = runtime;
    Ok(match ty.kind() {
        Kind::Int => format!("__jet_bootstrap_entry_integer_from_runtime({value})?"),
        Kind::IntN { signed, bits } => {
            let target = format!("{}{bits}", if *signed { 'i' } else { 'u' });
            format!("__jet_bootstrap_entry_integer_text({value})?.parse::<{target}>().map_err(|_| format!(\"checked fixed-width integer does not fit {target}\"))?")
        }
        Kind::Float => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: false }} => value, _ => return Err(\"checked Float result has invalid carrier\".to_string()) }}"),
        Kind::Float32 => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: true }} => value as f32, _ => return Err(\"checked Float32 result has invalid carrier\".to_string()) }}"),
        Kind::Measure(_) => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32: false }} => value, _ => return Err(\"checked Measure result has invalid carrier\".to_string()) }}"),
        Kind::Bool => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => value, _ => return Err(\"checked Bool result has invalid carrier\".to_string()) }}"),
        Kind::Char => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Char(value) => value, _ => return Err(\"checked Char result has invalid carrier\".to_string()) }}"),
        Kind::String => format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::String(value) => value, _ => return Err(\"checked String result has invalid carrier\".to_string()) }}"),
        Kind::List(inner) | Kind::FixedList { elem: inner, .. } => {
            if is_u8_type(inner) {
                format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Bytes(bytes) => bytes, ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| match item {{ ::jet_foundation::MIR::MirRuntimeValue::Int(value) => u8::try_from(value).map_err(|_| \"byte value is out of range\".to_string()), _ => Err(\"byte list contains a non-integer carrier\".to_string()) }}).collect::<Result<Vec<_>, String>>()?, _ => return Err(\"checked byte list result has invalid carrier\".to_string()) }}")
            } else {
                let kind = if matches!(ty.kind(), Kind::List(_)) { "list" } else { "fixed-list" };
                let child_type = format!("__jet_bootstrap_entry_child_type({checked}, \"{kind}\", 0)?");
                let child = from_runtime_expression(program, symbols, inner, "item", "&__jet_item_type", program_expr, path)?;
                if matches!(ty.kind(), Kind::List(_)) {
                    format!("{{ let __jet_item_type = {child_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| Ok({child})).collect::<Result<_, String>>()?, _ => return Err(\"checked List result has invalid carrier\".to_string()) }} }}")
                } else {
                    format!("{{ let __jet_item_type = {child_type}; let items = match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::List(items) => items.into_iter().map(|item| Ok({child})).collect::<Result<Vec<_>, String>>()?, _ => return Err(\"checked FixedList result has invalid carrier\".to_string()) }}; items.try_into().map_err(|_| \"checked FixedList result has the wrong length\".to_string())? }}")
                }
            }
        }
        Kind::Map { key, value: item_ty } => {
            let key_type = format!("__jet_bootstrap_entry_child_type({checked}, \"map\", 0)?");
            let item_type = format!("__jet_bootstrap_entry_child_type({checked}, \"map\", 1)?");
            let decoded_key = from_const_key_expression(program, symbols, key, "key", "&__jet_key_type", program_expr, path)?;
            let decoded_item = from_runtime_expression(program, symbols, item_ty, "item", "&__jet_item_type", program_expr, path)?;
            format!("{{ let __jet_key_type = {key_type}; let __jet_item_type = {item_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => entries.into_iter().map(|(key, item)| Ok(({decoded_key}, {decoded_item}))).collect::<Result<_, String>>()?, _ => return Err(\"checked Map result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Shared(_) | Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("physical.decode(path, {checked}, program, {value})?.ok_or_else(|| format!(\"NativeAdapter has no typed result projection for physical field `{{}}`\", path.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_type = format!("__jet_bootstrap_entry_child_type({checked}, \"option\", 0)?");
            let child = from_runtime_expression(program, symbols, inner, "*inner", "&__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({child}), ::jet_foundation::MIR::MirRuntimeValue::Absent {{ element }} if element.same_checked_type(__jet_inner_type) => Err(Default::default()), _ => return Err(\"checked Option result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_type = format!("__jet_bootstrap_entry_child_type({checked}, \"result\", 0)?");
            let err_type = format!("__jet_bootstrap_entry_child_type({checked}, \"result\", 1)?");
            let ok_value = from_runtime_expression(program, symbols, ok, "*inner", "&__jet_ok_type", program_expr, path)?;
            let err_value = from_runtime_expression(program, symbols, err, "*inner", "&__jet_err_type", program_expr, path)?;
            format!("{{ let __jet_ok_type = {ok_type}; let __jet_err_type = {err_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({ok_value}), ::jet_foundation::MIR::MirRuntimeValue::FailedTold(inner) => Err({err_value}), _ => return Err(\"checked Result result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_ref(program, name)?;
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, {checked}, {program_expr}, {path}, physical)?", name.id.0)
        }
        Kind::Tuple(fields) => {
            if fields.is_empty() {
                format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Unit => (), _ => return Err(\"checked unit result has invalid carrier\".to_string()) }}")
            } else {
                let mut decoded = Vec::with_capacity(fields.len());
                for (index, (_, field_ty)) in fields.iter().enumerate() {
                    let field_type = format!("__jet_bootstrap_entry_child_type({checked}, \"tuple\", {index})?");
                    let child = from_runtime_expression(program, symbols, field_ty, "field_value", &format!("&__jet_tuple_type_{index}"), program_expr, path)?;
                    decoded.push(format!("{{ let __jet_tuple_type_{index} = {field_type}; let field_value = __jet_bootstrap_entry_take_tuple_field(&mut fields, {index})?; {child} }}"));
                }
                format!("{{ let ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields: mut fields }} = {value} else {{ return Err(\"checked tuple result has invalid carrier\".to_string()) }}; if type_name != \"Tuple\" {{ return Err(\"checked tuple result has wrong nominal name\".to_string()) }} ({}) }}", decoded.join(", "))
            }
        }
        Kind::InlineRange { base, .. } | Kind::Tagged { inner: base, .. } | Kind::Quantity { base, .. } => {
            let kind = match ty.kind() { Kind::InlineRange { .. } => "range", Kind::Tagged { .. } => "tagged", _ => "quantity" };
            let inner_type = format!("__jet_bootstrap_entry_child_type({checked}, \"{kind}\", 0)?");
            let child = from_runtime_expression(program, symbols, base, value, "__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, {checked}, {program_expr}, {path}, physical)?", id.0)
        }
    })
}

fn to_const_key_expression(
    program: &MirProgram,
    _symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    source: &str,
    _checked: &str,
    _program_expr: &str,
    _path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    let key = "::jet_foundation::MIR::MirConstKey";
    Ok(match ty.kind() {
        Kind::Int => format!("{key}::Int(({source}).to_i64().ok_or_else(|| \"integer map key exceeds i64\".to_string())?)"),
        Kind::IntN { .. } => format!("{key}::Int(({source}).to_string().parse::<i64>().map_err(|_| \"fixed-width integer map key exceeds i64\".to_string())?)"),
        Kind::String => format!("{key}::String(({source}).clone())"),
        Kind::Bool => format!("{key}::Bool(*({source}))"),
        Kind::Char => format!("{key}::Char(*({source}))"),
        Kind::Tagged { inner, .. } | Kind::Quantity { base: inner, .. } => {
            return to_const_key_expression(program, _symbols, inner, source, _checked, _program_expr, _path)
        }
        Kind::Apply { name, .. } => {
            let definition = checked_definition_by_ref(program, name)?;
            match &definition.kind {
                MirTypeDefKind::Enum { variants, .. } if variants.iter().all(|variant| matches!(variant.payload, MirVariantPayload::Unit)) => {
                    let mut arms = Vec::new();
                    for variant in variants {
                        let path = _symbols.variant_path(&definition.name, &variant.name)?;
                        arms.push(format!("{path} => Ok({key}::Enum {{ type_name: {name:?}.to_string(), variant: {variant:?}.to_string() }}),", name=definition.name, variant=variant.name));
                    }
                    return Ok(format!("match {source} {{ {} }}", arms.join(" ")))
                }
                MirTypeDefKind::UnitFamily { members } => {
                    let mut arms = Vec::new();
                    for member in members {
                        let path = unit_family_variant_path(_symbols, &definition.name, member)?;
                        arms.push(format!("{path} => Ok({key}::Enum {{ type_name: {name:?}.to_string(), variant: {member:?}.to_string() }}),", name=definition.name, member=member));
                    }
                    return Ok(format!("match {source} {{ {} }}", arms.join(" ")))
                }
                _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked constant-key ABI", definition.name))),
            }
        }
        _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked constant-key ABI", ty.canonical_key()))),
    })
}

fn from_const_key_expression(
    _program: &MirProgram,
    _symbols: &BootstrapCodecSymbols<'_>,
    ty: &MirType,
    key: &str,
    _checked: &str,
    _program_expr: &str,
    _path: &str,
) -> Result<String, BootstrapHostCodecError> {
    use MirTypeKind as Kind;
    Ok(match ty.kind() {
        Kind::Int => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Int(value) => jet_foundation::Numeric::JetInt::from_i64(value), _ => return Err(\"map key does not match checked Int\".to_string()) }}"),
        Kind::IntN { signed, bits } => {
            let target=format!("{}{bits}",if *signed {'i'} else {'u'});
            format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Int(value) => {target}::try_from(value).map_err(|_| \"map key does not fit checked fixed width\".to_string())?, _ => return Err(\"map key does not match checked fixed width\".to_string()) }}")
        }
        Kind::String => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::String(value) => value, _ => return Err(\"map key does not match checked String\".to_string()) }}"),
        Kind::Bool => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Bool(value) => value, _ => return Err(\"map key does not match checked Bool\".to_string()) }}"),
        Kind::Char => format!("match {key} {{ ::jet_foundation::MIR::MirConstKey::Char(value) => value, _ => return Err(\"map key does not match checked Char\".to_string()) }}"),
        Kind::Tagged { inner, .. } | Kind::Quantity { base: inner, .. } => return from_const_key_expression(_program, _symbols, inner, key, _checked, _program_expr, _path),
        _ => return Err(BootstrapHostCodecError::InvalidMetadata(format!("entry map key type `{}` has no checked source reconstruction",ty.canonical_key()))),
    })
}

fn is_u8_type(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::IntN { signed: false, bits: 8 })
}

/// Whether the emitted Rust carrier of `ty` is bound to one thread: plain
/// `Fn` values are `Rc` closures and trait objects are unbounded `Box<dyn _>`,
/// so no value reaching them can back a cross-thread Shared owner.
fn entry_type_is_thread_bound(program: &MirProgram, ty: &MirType, visited: &mut HashSet<MirTypeId>) -> bool {
    use MirTypeKind as Kind;
    match ty.kind() {
        Kind::Fn(_) | Kind::TraitObject(_) => true,
        Kind::List(inner) | Kind::Option(inner) | Kind::Shared(inner) => entry_type_is_thread_bound(program, inner, visited),
        Kind::FixedList { elem: inner, .. }
        | Kind::InlineRange { base: inner, .. }
        | Kind::Tagged { inner, .. }
        | Kind::Quantity { base: inner, .. } => entry_type_is_thread_bound(program, inner, visited),
        Kind::Map { key, value } => {
            entry_type_is_thread_bound(program, key, visited) || entry_type_is_thread_bound(program, value, visited)
        }
        Kind::Result { ok, err } => {
            entry_type_is_thread_bound(program, ok, visited) || entry_type_is_thread_bound(program, err, visited)
        }
        Kind::Tuple(fields) => fields.iter().any(|(_, field)| entry_type_is_thread_bound(program, field, visited)),
        Kind::Union(members) => members.iter().any(|member| entry_type_is_thread_bound(program, member, visited)),
        Kind::Apply { name, args } => {
            if args.iter().any(|arg| entry_type_is_thread_bound(program, arg, visited)) {
                return true;
            }
            if !visited.insert(name.id) {
                return false;
            }
            let Some(definition) = program.types.iter().find(|definition| definition.id == name.id) else {
                return false;
            };
            match &definition.kind {
                MirTypeDefKind::Struct { fields, .. } => {
                    fields.iter().any(|field| entry_type_is_thread_bound(program, &field.ty, visited))
                }
                MirTypeDefKind::Enum { variants, .. } => variants.iter().any(|variant| match &variant.payload {
                    MirVariantPayload::Unit => false,
                    MirVariantPayload::Single(ty) => entry_type_is_thread_bound(program, ty, visited),
                    MirVariantPayload::Named(fields) => {
                        fields.iter().any(|field| entry_type_is_thread_bound(program, &field.ty, visited))
                    }
                }),
                MirTypeDefKind::Distinct { base, .. } => entry_type_is_thread_bound(program, base, visited),
                MirTypeDefKind::Alias { target } => entry_type_is_thread_bound(program, target, visited),
                MirTypeDefKind::UnitFamily { .. } => false,
            }
        }
        _ => false,
    }
}


/// Walk every nominal type reachable from `root` and record each missing or
/// drifted binding on `symbols`, continuing past failures so the caller's
/// `finish` reports the complete list.
fn validate_binding_graph(
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    root: &MirType,
) {
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(ty) = stack.pop() {
        match ty.kind() {
            MirTypeKind::List(inner) | MirTypeKind::Shared(inner) | MirTypeKind::Option(inner)
            | MirTypeKind::FixedList { elem: inner, .. } | MirTypeKind::InlineRange { base: inner, .. }
            | MirTypeKind::Tagged { inner, .. } | MirTypeKind::Quantity { base: inner, .. } => stack.push(inner),
            MirTypeKind::Map { key, value } | MirTypeKind::Result { ok: key, err: value } => { stack.push(key); stack.push(value); }
            MirTypeKind::Fn(signature) => { stack.extend(signature.params.iter()); if let Some(ret)=signature.ret.as_deref(){stack.push(ret);} }
            MirTypeKind::SendFn { params, ret, .. } => { stack.extend(params.iter()); if let Some(ret)=ret.as_deref(){stack.push(ret);} }
            MirTypeKind::Tuple(fields) => stack.extend(fields.iter().map(|(_,ty)|ty)),
            MirTypeKind::Union(members) => stack.extend(members.iter()),
            MirTypeKind::Apply { name, args } => {
                stack.extend(args.iter());
                if !visited.insert(name.id) { continue; }
                let definition = match checked_definition_by_ref(program, name) {
                    Ok(definition) => definition,
                    Err(error) => { symbols.record(error); continue; }
                };
                let _ = symbols.type_symbol(&definition.name);
                match &definition.kind {
                    MirTypeDefKind::Struct { fields, .. } => for field in fields {
                        match symbols.field_binding(&definition.name,&field.name) {
                            Ok(row) if row.field!=field.id || row.owner!=definition.id || !row.ty.same_checked_type(&field.ty) => {
                                symbols.record(BootstrapHostCodecError::InvalidMetadata(format!("checked field binding drift for `{}.{}`",definition.name,field.name)));
                            }
                            Ok(_) => {}
                            Err(error) => symbols.record(error),
                        }
                        stack.push(&field.ty);
                    },
                    MirTypeDefKind::Enum { variants, .. } => for variant in variants {
                        let _ = symbols.variant_path(&definition.name,&variant.name);
                        match &variant.payload {
                            MirVariantPayload::Unit=>{},
                            MirVariantPayload::Single(ty)=>stack.push(ty),
                            MirVariantPayload::Named(fields)=>for field in fields {
                                match symbols.field_binding_by_id(definition.id,&definition.name,field.id,&field.name) {
                                    Ok(row) if row.field!=field.id || row.owner!=definition.id || !row.ty.same_checked_type(&field.ty) => {
                                        symbols.record(BootstrapHostCodecError::InvalidMetadata(format!("checked enum payload binding drift for `{}::{}.{}`",definition.name,variant.name,field.name)));
                                    }
                                    Ok(_) => {}
                                    Err(error) => symbols.record(error),
                                }
                                stack.push(&field.ty);
                            },
                        }
                    },
                    MirTypeDefKind::Distinct { base, .. } | MirTypeDefKind::Alias { target: base }=>stack.push(base),
                    MirTypeDefKind::UnitFamily { .. }=>{},
                }
            }
            _=>{}
        }
    }
}

fn collect_nominal_types<'a>(
    program:&'a MirProgram,
    root:&MirType,
    visited:&mut HashSet<MirTypeId>,
    output:&mut Vec<&'a MirTypeDef>,
)->Result<(),BootstrapHostCodecError>{
    let mut stack=vec![root];
    while let Some(ty)=stack.pop(){
        match ty.kind(){
            MirTypeKind::List(inner)|MirTypeKind::Shared(inner)|MirTypeKind::Option(inner)|MirTypeKind::FixedList{elem:inner,..}|MirTypeKind::InlineRange{base:inner,..}|MirTypeKind::Tagged{inner,..}|MirTypeKind::Quantity{base:inner,..}=>stack.push(inner),
            MirTypeKind::Map{key,value}|MirTypeKind::Result{ok:key,err:value}=>{stack.push(key);stack.push(value);},
            MirTypeKind::Fn(signature)=>{stack.extend(signature.params.iter());if let Some(ret)=signature.ret.as_deref(){stack.push(ret);}},
            MirTypeKind::SendFn{params,ret,..}=>{stack.extend(params.iter());if let Some(ret)=ret.as_deref(){stack.push(ret);}},
            MirTypeKind::Tuple(fields)=>stack.extend(fields.iter().map(|(_,ty)|ty)),
            MirTypeKind::Union(members)=>stack.extend(members.iter()),
            MirTypeKind::Apply{name,args}=>{
                stack.extend(args.iter());
                if visited.insert(name.id){
                    let def=checked_definition_by_ref(program,name)?;output.push(def);
                    match &def.kind{
                        MirTypeDefKind::Struct{fields,..}=>stack.extend(fields.iter().map(|field|&field.ty)),
                        MirTypeDefKind::Enum{variants,..}=>for variant in variants{match &variant.payload{MirVariantPayload::Unit=>{},MirVariantPayload::Single(ty)=>stack.push(ty),MirVariantPayload::Named(fields)=>stack.extend(fields.iter().map(|field|&field.ty)),}},
                        MirTypeDefKind::Distinct{base,..}|MirTypeDefKind::Alias{target:base}=>stack.push(base),
                        MirTypeDefKind::UnitFamily{..}=>{},
                    }
                }
            },
            _=>{},
        }
    }
    Ok(())
}

/// The declaration row of a nominal entry type: its checked identity, or for an
/// application whose identity is the instance key, its nominal reference.
fn checked_nominal_definition<'a>(program:&'a MirProgram,ty:&MirType)->Result<&'a MirTypeDef,BootstrapHostCodecError>{
    let id=ty.nominal_id().ok_or_else(||BootstrapHostCodecError::InvalidMetadata(format!("entry type `{}` is not checked nominal",ty.canonical_key())))?;
    if let Ok(definition)=checked_definition_by_id(program,id){
        return Ok(definition);
    }
    match ty.kind(){
        MirTypeKind::Apply{name,..}=>checked_definition_by_ref(program,name),
        _=>Err(BootstrapHostCodecError::MissingType(format!("`{}` (MIR type ID {})",ty.canonical_key(),id.0))),
    }
}
fn checked_definition_by_id<'a>(program:&'a MirProgram,id:MirTypeId)->Result<&'a MirTypeDef,BootstrapHostCodecError>{
    program.types.iter().find(|definition|definition.id==id).ok_or_else(||BootstrapHostCodecError::MissingType(format!("MIR type ID {}",id.0)))
}
/// A nominal reference whose ID has no checked row names its source spelling,
/// so a lowering identity drift is diagnosable from the error alone.
fn checked_definition_by_ref<'a>(program:&'a MirProgram,name:&MirNominalRef)->Result<&'a MirTypeDef,BootstrapHostCodecError>{
    program.types.iter().find(|definition|definition.id==name.id).ok_or_else(||BootstrapHostCodecError::MissingType(format!("`{}` (MIR type ID {})",name.name,name.id.0)))
}

pub(crate) fn checked_record_field<'a>(program:&MirProgram,ty:&MirType,value:&'a MirRuntimeValue,id:MirFieldId)->Result<Option<&'a MirRuntimeValue>,String>{
    let definition=ty.nominal_id().and_then(|id|program.types.iter().find(|d|d.id==id)).ok_or_else(||"checked record type is absent".to_string())?;
    let MirTypeDefKind::Struct{fields,..}=&definition.kind else{return Err(format!("checked type `{}` is not a record",definition.name));};
    let field=fields.iter().find(|field|field.id==id).ok_or_else(||format!("checked field ID {} is absent",id.0))?;
    let MirRuntimeValue::Struct{type_name,fields:values}=value else{return Err(format!("result `{}` is not a record",definition.name));};
    if type_name!=&definition.name || values.len()!=fields.len(){return Err(format!("result record `{}` is incomplete or mistyped",definition.name));}
    for (actual,expected) in values.iter().zip(fields){if actual.0!=expected.name{return Err(format!("result record `{}` has field order mismatch",definition.name));}}
    Ok(values.iter().find(|(name,_)|name==&field.name).map(|(_,value)|value))
}

pub(crate) fn validate_runtime_value(program:&MirProgram,ty:&MirType,value:&MirRuntimeValue,depth:usize)->Result<(),String>{
    use MirRuntimeValue as V;use MirTypeKind as K;
    if depth>=MAX_VALUE_DEPTH{return Err("compiler-entry value exceeds checked nesting bound".to_string());}
    match ty.kind(){
        K::Int|K::IntN{..}|K::Measure(_)=>validate_integer(ty,value),
        K::Float=>if matches!(value,V::Float{f32:false,..}){Ok(())}else{Err(mismatch(ty,"f64"))},
        K::Float32=>if matches!(value,V::Float{f32:true,..}){Ok(())}else{Err(mismatch(ty,"f32"))},
        K::Bool=>if matches!(value,V::Bool(_)){Ok(())}else{Err(mismatch(ty,"Bool"))},
        K::Char=>if matches!(value,V::Char(_)){Ok(())}else{Err(mismatch(ty,"Char"))},
        K::String=>if matches!(value,V::String(_)){Ok(())}else{Err(mismatch(ty,"String"))},
        K::List(inner)=>validate_sequence(program,ty,inner,value,None,depth),
        K::FixedList{elem,len}=>validate_sequence(program,ty,elem,value,len.literal_value(),depth),
        K::Map{key,value:item_ty}=>{
            let V::Map(entries)=value else{return Err(mismatch(ty,"Map"));};let mut seen=BTreeSet::new();
            for (key_value,item) in entries{validate_const_key(program,key,key_value,depth+1)?;if !seen.insert(key_value){return Err("compiler-entry map has duplicate keys".to_string());}validate_runtime_value(program,item_ty,item,depth+1)?;}Ok(())
        },
        K::Shared(_) => {
            let root = jet_jit::SourceSharedInterop::SourceSharedInterop::from_native_owned(value)?;
            if root.type_id() == jet_jit::SourceSharedInterop::checked_type_id_for_program(program, ty) {
                Ok(())
            } else {
                Err(mismatch(ty, "exact checked Shared physical root"))
            }
        }
        K::TraitObject(_) | K::Fn(_) | K::SendFn { .. } => {
            if matches!(value, V::NativeOwned(_)) {
                Ok(())
            } else {
                Err(mismatch(ty, "registered native-owned physical carrier"))
            }
        }
        K::Option(inner)=>match value{V::Present(value)=>validate_runtime_value(program,inner,value,depth+1),V::Absent{element} if element.same_checked_type(inner)=>Ok(()),V::Absent{..}=>Err("absent carrier changed its exact checked element type".to_string()),_=>Err(mismatch(ty,"Option"))},
        K::Result{ok,err}=>match value{V::Present(value)=>validate_runtime_value(program,ok,value,depth+1),V::FailedTold(value)=>validate_runtime_value(program,err,value,depth+1),_=>Err(mismatch(ty,"Result"))},
        K::Apply{name,..}=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||format!("checked type `{}` is absent",name.name))?;validate_nominal(program,def,ty,value,depth+1)},
        K::Tuple(fields)=>{
            if fields.is_empty(){return if matches!(value,V::Unit){Ok(())}else{Err(mismatch(ty,"Unit"))};}
            let V::Struct{type_name,fields:values}=value else{return Err(mismatch(ty,"Tuple"));};
            if type_name!="Tuple"{return Err(mismatch(ty,"Tuple"));}
            validate_named_values(program,ty,&fields.iter().map(|(name,ty)|(name.as_str(),ty)).collect::<Vec<_>>(),values,depth+1)
        },
        K::InlineRange{base,lo,hi}=>{validate_runtime_value(program,base,value,depth+1)?;let number=integer_as_i128(value).ok_or_else(||mismatch(ty,"integer range"))?;if number<i128::from(*lo)||number>i128::from(*hi){return Err(format!("value is outside checked range `{}`",ty.canonical_key()));}Ok(())},
        K::Tagged{inner,..}|K::Quantity{base:inner,..}=>validate_runtime_value(program,inner,value,depth+1),
        K::Union(members)=>{let mut last=None;for member in members{match validate_runtime_value(program,member,value,depth+1){Ok(())=>return Ok(()),Err(err)=>last=Some(err)}}Err(last.unwrap_or_else(||mismatch(ty,"union member")))},
    }
}
fn validate_sequence(program:&MirProgram,ty:&MirType,elem:&MirType,value:&MirRuntimeValue,expected:Option<u64>,depth:usize)->Result<(),String>{
    let count=match value{MirRuntimeValue::Bytes(bytes) if is_u8_type(elem)=>bytes.len(),MirRuntimeValue::List(values)=>{for item in values{validate_runtime_value(program,elem,item,depth+1)?;}values.len()},_=>return Err(mismatch(ty,"sequence"))};
    if expected.is_some_and(|expected|usize::try_from(expected).ok()!=Some(count)){return Err(format!("sequence `{}` has incorrect fixed length",ty.canonical_key()));}Ok(())
}
fn validate_nominal(program:&MirProgram,def:&MirTypeDef,ty:&MirType,value:&MirRuntimeValue,depth:usize)->Result<(),String>{
    use MirRuntimeValue as V;
    match &def.kind{
        MirTypeDefKind::Struct{fields,..}=>{let V::Struct{type_name,fields:values}=value else{return Err(mismatch(ty,"record"));};if type_name!=&def.name{return Err(mismatch(ty,"exact record name"));}validate_named_values(program,ty,&fields.iter().map(|field|(field.name.as_str(),&field.ty)).collect::<Vec<_>>(),values,depth+1)},
        MirTypeDefKind::Enum{variants,..}=>{let V::Enum{type_name,variant,args}=value else{return Err(mismatch(ty,"enum"));};if type_name!=&def.name{return Err(mismatch(ty,"exact enum name"));}let variant=variants.iter().find(|v|v.name==*variant).ok_or_else(||format!("unknown checked variant `{}`",variant))?;let expected=match &variant.payload{MirVariantPayload::Unit=>Vec::new(),MirVariantPayload::Single(ty)=>vec![(None,ty)],MirVariantPayload::Named(fields)=>fields.iter().map(|f|(Some(f.name.as_str()),&f.ty)).collect()};if args.len()!=expected.len(){return Err("checked enum payload arity mismatch".to_string());}for ((name,ty),(actual,value)) in expected.into_iter().zip(args){if name!=actual.as_deref(){return Err("checked enum payload name/order mismatch".to_string());}validate_runtime_value(program,ty,value,depth+1)?;}Ok(())},
        MirTypeDefKind::Distinct{base,..}|MirTypeDefKind::Alias{target:base}=>validate_runtime_value(program,base,value,depth+1),
        MirTypeDefKind::UnitFamily{members}=>match value{V::Enum{type_name,variant,args} if type_name==&def.name&&members.iter().any(|m|m==variant)&&args.is_empty()=>Ok(()),_=>Err(mismatch(ty,"unit-family variant"))},
    }
}
fn validate_named_values(program:&MirProgram,ty:&MirType,declared:&[(&str,&MirType)],values:&[(String,MirRuntimeValue)],depth:usize)->Result<(),String>{
    if declared.len()!=values.len(){return Err(format!("aggregate `{}` has missing/extra fields",ty.canonical_key()));}let mut seen=HashSet::with_capacity(values.len());
    for ((expected,expected_ty),(actual,value)) in declared.iter().zip(values){if !seen.insert(actual.as_str())||actual!=expected{return Err(format!("aggregate `{}` field names/order are invalid",ty.canonical_key()));}validate_runtime_value(program,expected_ty,value,depth+1)?;}Ok(())
}
fn validate_const_key(program:&MirProgram,ty:&MirType,key_ty:&MirConstKey,depth:usize)->Result<(),String>{
    if depth>=MAX_VALUE_DEPTH{return Err("map key exceeds checked nesting bound".to_string());}
    match (ty.kind(),key_ty){
        (MirTypeKind::Int|MirTypeKind::IntN{..}|MirTypeKind::Measure(_),MirConstKey::Int(_))|(MirTypeKind::String,MirConstKey::String(_))|(MirTypeKind::Bool,MirConstKey::Bool(_))|(MirTypeKind::Char,MirConstKey::Char(_))=>Ok(()),
        (MirTypeKind::Tuple(types),MirConstKey::Tuple(values)) if types.len()==values.len()=>{for ((name,ty),(actual,key)) in types.iter().zip(values){if name!=actual{return Err("tuple map key names do not match checked type".to_string());}validate_const_key(program,ty,key,depth+1)?;}Ok(())},
        (MirTypeKind::Apply{name,..},MirConstKey::Struct{type_name,fields})=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||"checked map key type is missing".to_string())?;let MirTypeDefKind::Struct{fields:declared,..}=&def.kind else{return Err("map key is not a checked struct".to_string());};if type_name!=&def.name||declared.len()!=fields.len(){return Err("struct key differs from checked nominal type".to_string());}for (field,(name,key)) in declared.iter().zip(fields){if field.name!=*name{return Err("struct key field order/name mismatch".to_string());}validate_const_key(program,&field.ty,key,depth+1)?;}Ok(())},
        (MirTypeKind::Apply{name,..},MirConstKey::Enum{type_name,variant})=>{let def=program.types.iter().find(|d|d.id==name.id).ok_or_else(||"checked map key type is missing".to_string())?;if type_name!=&def.name{return Err("enum key name mismatch".to_string());}match &def.kind{MirTypeDefKind::Enum{variants,..} if variants.iter().any(|row|row.name==*variant&&matches!(row.payload,MirVariantPayload::Unit))=>Ok(()),MirTypeDefKind::UnitFamily{members} if members.iter().any(|member|member==variant)=>Ok(()),_=>Err("map enum key is not a checked unit variant".to_string())}},
        (MirTypeKind::Tagged{inner,..}|MirTypeKind::Quantity{base:inner,..},_)=>validate_const_key(program,inner,key_ty,depth+1),
        _=>Err(format!("map key does not match checked type `{}`",ty.canonical_key())),
    }
}
fn validate_integer(ty:&MirType,value:&MirRuntimeValue)->Result<(),String>{
    let text=match value{MirRuntimeValue::Int(v)=>v.to_string(),MirRuntimeValue::BigInt(v)=>v.clone(),_=>return Err(mismatch(ty,"exact integer"))};
    let exact=jet_foundation::Numeric::CtBigInt::from_str(&text).map_err(|_|"invalid exact integer carrier".to_string())?;
    if exact.to_string_rep()!=text{return Err("integer carrier is not canonical".to_string());}
    if let MirTypeKind::IntN{signed,bits}=ty.kind(){if *bits==0{return Err("fixed-width integer has zero bits".to_string());}let binary=exact.abs().to_radix(2).map_err(|e|e.to_string())?;let width=binary.len();let fits=if *signed{if exact.is_zero(){true}else if text.starts_with('-'){width<usize::from(bits.saturating_sub(1))||(width==usize::from(*bits)&&binary==format!("1{}","0".repeat(usize::from(bits.saturating_sub(1)))))}else{width<=usize::from(bits.saturating_sub(1))}}else{!text.starts_with('-')&&width<=usize::from(*bits)};if !fits{return Err(format!("integer is outside checked {}{} range",if *signed{'I'}else{'U'},bits));}}Ok(())
}
fn integer_as_i128(value:&MirRuntimeValue)->Option<i128>{match value{MirRuntimeValue::Int(v)=>Some(i128::from(*v)),MirRuntimeValue::BigInt(v)=>jet_foundation::Numeric::CtBigInt::from_str(v).ok()?.try_i128(),_=>None}}
fn mismatch(ty:&MirType,expected:&str)->String{format!("runtime value does not match checked `{}` ({expected})",ty.canonical_key())}
