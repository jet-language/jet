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

/// Failure to reconstruct a typed compiler root. The packet remains available
/// for Source cleanup and cannot be silently dropped with the decode error.
pub(crate) struct BootstrapEntryRootDecodeError {
    pub(crate) error: String,
    pub(crate) value: MirRuntimeValue,
}

impl BootstrapEntryRootDecodeError {
    pub(crate) fn into_parts(self) -> (String, MirRuntimeValue) {
        (self.error, self.value)
    }
}

/// NativeAdapter-owned conversion for physical compiler-entry values.
///
/// Generated glue passes the actual typed Source field to its owner. The
/// checked field path identifies a use site, not a runtime instance; adapter
/// implementations resolve that exact instance's physical binding rather
/// than substituting a static configuration-field capability.
pub(crate) trait BootstrapEntrySharedValueMarshaller: Clone + Send + Sync + 'static {
    fn encode_payload<T: 'static>(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        source_value: &T,
    ) -> Result<MirRuntimeValue, String>;

    fn decode_payload<T: 'static>(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        runtime_value: MirRuntimeValue,
    ) -> Result<T, String>;
}

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
    type InterpreterValue: 'static;
    type PreparedInterpreterValue: 'static;
    type InterpreterTransferReceipt: 'static;
    type InterpreterTransferBorrowEntry: 'static;
    type InterpreterTransferActivation: 'static;
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

    /// Validate and reserve the adapter-owned physical part of one checked
    /// Source HostValue leaf without extracting or moving it. The exact
    /// invocation result transfer receipt commits that extraction later.
    fn prepare_interpreter(
        &self,
        field_path: &[String],
        projection_path: &[BootstrapEntryHostProjection],
        checked_type: &MirType,
        shape: &BootstrapEntryHostTypeShape,
        shape_node: usize,
        interpreter_value: &Self::InterpreterValue,
    ) -> Result<Option<Self::PreparedInterpreterValue>, String>;

    /// Consume a previously prepared physical leaf after Source commits the
    /// matching result transfer receipt. Implementations must be infallible:
    /// all validation and reservations happened before Source's commit.
    fn interpreter_transfer_commit_and_extract(
        &self,
        prepared: Self::PreparedInterpreterValue,
        projection_path: &[BootstrapEntryHostProjection],
        receipt: &Self::InterpreterTransferReceipt,
    ) -> (MirRuntimeValue, Option<Self::InterpreterTransferBorrowEntry>);

    /// Activate one invocation-scoped physical borrow context from all
    /// prepared-tree entries after every leaf has been committed and extracted.
    /// The returned guard remains live through the caller's result retirement.
    fn activate_interpreter_transfer_borrows(
        &self,
        receipt: &Self::InterpreterTransferReceipt,
        entries: Vec<Self::InterpreterTransferBorrowEntry>,
    ) -> Option<Self::InterpreterTransferActivation>;


    /// Project a real invocation-scoped native root into the interpreter's
    /// checked HostValue carrier. This is never a serialized handle or MIR ID.
    fn project_interpreter(
        &self,
        field_path: &[String],
        checked_type: &MirType,
        program: &MirProgram,
        runtime_value: MirRuntimeValue,
    ) -> Result<Option<Self::InterpreterValue>, String>;
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
    fn core_owner_rows(&self) -> &[MirCoreOwner];
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

#[derive(Debug)]
pub(crate) enum BootstrapPreparedRuntimeValue<T> {
    Runtime(MirRuntimeValue),
    Physical {
        prepared: T,
        projection_path: Vec<BootstrapEntryHostProjection>,
    },
    Present(Box<Self>),
    Failed(Box<Self>),
    Absent(MirType),
    List(Vec<Self>),
    Map(Vec<(MirConstKey, Self)>),
    Struct {
        type_name: String,
        fields: Vec<(String, Self)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, Self)>,
    },
}

impl<T> BootstrapPreparedRuntimeValue<T> {
    fn into_runtime<P>(
        self,
        physical: &P,
        receipt: &P::InterpreterTransferReceipt,
        borrow_entries: &mut Vec<P::InterpreterTransferBorrowEntry>,
    ) -> MirRuntimeValue
    where
        P: BootstrapEntryPhysicalBindings<PreparedInterpreterValue = T>,
    {
        match self {
            Self::Runtime(value) => value,
            Self::Physical {
                prepared,
                projection_path,
            } => {
                let (runtime_value, entry) = physical.interpreter_transfer_commit_and_extract(
                    prepared,
                    &projection_path,
                    receipt,
                );
                if let Some(entry) = entry {
                    borrow_entries.push(entry);
                }
                runtime_value
            }
            Self::Present(value) => MirRuntimeValue::Present(Box::new(
                value.into_runtime(physical, receipt, borrow_entries),
            )),
            Self::Failed(value) => MirRuntimeValue::FailedTold(Box::new(
                value.into_runtime(physical, receipt, borrow_entries),
            )),
            Self::Absent(element) => MirRuntimeValue::Absent { element },
            Self::List(values) => MirRuntimeValue::List(
                values
                    .into_iter()
                    .map(|value| value.into_runtime(physical, receipt, borrow_entries))
                    .collect(),
            ),
            Self::Map(values) => MirRuntimeValue::Map(
                values
                    .into_iter()
                    .map(|(key, value)| {
                        (key, value.into_runtime(physical, receipt, borrow_entries))
                    })
                    .collect(),
            ),
            Self::Struct { type_name, fields } => MirRuntimeValue::Struct {
                type_name,
                fields: fields
                    .into_iter()
                    .map(|(name, value)| {
                        (name, value.into_runtime(physical, receipt, borrow_entries))
                    })
                    .collect(),
            },
            Self::Enum {
                type_name,
                variant,
                args,
            } => MirRuntimeValue::Enum {
                type_name,
                variant,
                args: args
                    .into_iter()
                    .map(|(name, value)| {
                        (name, value.into_runtime(physical, receipt, borrow_entries))
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct BootstrapEntryPreparedInterpreterTransfer<T> {
    pub(crate) ty: MirType,
    value: BootstrapPreparedRuntimeValue<T>,
}
impl<T> BootstrapEntryPreparedInterpreterTransfer<T> {
    pub(crate) fn new(ty: MirType, value: BootstrapPreparedRuntimeValue<T>) -> Self {
        Self { ty, value }
    }
}

pub(crate) fn __jet_bootstrap_entry_interpreter_transfer_commit_and_extract<P>(
    prepared: BootstrapEntryPreparedInterpreterTransfer<P::PreparedInterpreterValue>,
    receipt: &P::InterpreterTransferReceipt,
    physical: &P,
) -> (
    MirRuntimeValue,
    Option<P::InterpreterTransferActivation>,
)
where
    P: BootstrapEntryPhysicalBindings,
{
    let mut borrow_entries = Vec::new();
    let value = prepared.value.into_runtime(physical, receipt, &mut borrow_entries);
    let activation = physical.activate_interpreter_transfer_borrows(receipt, borrow_entries);
    (value, activation)
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

fn bootstrap_entry_validate_prepared<T>(
    value: &BootstrapPreparedRuntimeValue<T>,
    checked: &MirType,
    shape: &BootstrapEntryHostTypeShape,
    shape_node: usize,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_VALUE_DEPTH {
        return Err("compiler-entry prepared value exceeds checked nesting bound".to_string());
    }
    let shape_type = bootstrap_entry_shape_node_type(shape, shape_node, depth)?;
    if !bootstrap_entry_shape_type_matches(&shape_type, checked) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    match value {
        BootstrapPreparedRuntimeValue::Runtime(value) => {
            bootstrap_entry_validate_shaped_runtime(value, checked, shape, shape_node, depth)
        }
        BootstrapPreparedRuntimeValue::Physical { .. } => Ok(()),
        BootstrapPreparedRuntimeValue::Present(inner) => {
            let child = match &shape.nodes[shape_node] {
                BootstrapEntryHostTypeNode::Option(child) => *child,
                BootstrapEntryHostTypeNode::Result { ok, .. } => *ok,
                _ => return Err("checked Present carrier has no Option/Result shape".to_string()),
            };
            bootstrap_entry_validate_prepared(
                inner,
                &bootstrap_entry_shape_node_type(shape, child, depth + 1)?,
                shape,
                child,
                depth + 1,
            )
        }
        BootstrapPreparedRuntimeValue::Failed(inner) => {
            let BootstrapEntryHostTypeNode::Result { error, .. } = &shape.nodes[shape_node] else {
                return Err("checked Failed carrier has no Result shape".to_string());
            };
            let child = *error;
            bootstrap_entry_validate_prepared(
                inner,
                &bootstrap_entry_shape_node_type(shape, child, depth + 1)?,
                shape,
                child,
                depth + 1,
            )
        }
        BootstrapPreparedRuntimeValue::Absent(element) => {
            let BootstrapEntryHostTypeNode::Option(child) = &shape.nodes[shape_node] else {
                return Err("checked Absent carrier has no Option shape".to_string());
            };
            if element.same_checked_type(&bootstrap_entry_shape_node_type(
                shape,
                *child,
                depth + 1,
            )?) {
                Ok(())
            } else {
                Err("absent HostValue changed its exact checked element type".to_string())
            }
        }
        BootstrapPreparedRuntimeValue::List(values) => {
            let BootstrapEntryHostTypeNode::List(child) = &shape.nodes[shape_node] else {
                return Err("checked List carrier has no sequence shape".to_string());
            };
            let child = *child;
            if let MirTypeKind::FixedList { len, .. } = checked.kind() {
                if let jet_foundation::MIR::MirMeasure::Literal { value, .. } = len {
                    if values.len() as u64 != *value {
                        return Err("checked fixed-list host value has the wrong length".to_string());
                    }
                }
            }
            let child_type = bootstrap_entry_shape_node_type(shape, child, depth + 1)?;
            for value in values {
                bootstrap_entry_validate_prepared(value, &child_type, shape, child, depth + 1)?;
            }
            Ok(())
        }
        BootstrapPreparedRuntimeValue::Map(values) => {
            let BootstrapEntryHostTypeNode::Map { value: child, .. } = &shape.nodes[shape_node] else {
                return Err("checked Map carrier has no map shape".to_string());
            };
            let child = *child;
            let child_type = bootstrap_entry_shape_node_type(shape, child, depth + 1)?;
            for (_, value) in values {
                bootstrap_entry_validate_prepared(value, &child_type, shape, child, depth + 1)?;
            }
            Ok(())
        }
        BootstrapPreparedRuntimeValue::Struct { type_name, fields } => {
            let shape_fields = match &shape.nodes[shape_node] {
                BootstrapEntryHostTypeNode::Tuple(fields) => fields,
                BootstrapEntryHostTypeNode::Struct {
                    type_name: expected,
                    fields,
                    ..
                } if expected == type_name => fields,
                _ => return Err("checked Struct carrier has no matching record shape".to_string()),
            };
            if fields.len() != shape_fields.len() {
                return Err("checked Struct carrier has a mismatched field count".to_string());
            }
            for ((name, value), expected) in fields.iter().zip(shape_fields) {
                if name != &expected.name {
                    return Err("checked Struct carrier field name/order differs".to_string());
                }
                bootstrap_entry_validate_prepared(
                    value,
                    &bootstrap_entry_shape_node_type(shape, expected.node, depth + 1)?,
                    shape,
                    expected.node,
                    depth + 1,
                )?;
            }
            Ok(())
        }
        BootstrapPreparedRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } => {
            let BootstrapEntryHostTypeNode::Enum {
                type_name: expected_type,
                variants,
                ..
            } = &shape.nodes[shape_node]
            else {
                return Err("checked Enum carrier has no enum shape".to_string());
            };
            if type_name != expected_type {
                return Err("checked Enum carrier has a mismatched type name".to_string());
            }
            let expected = variants
                .iter()
                .find(|expected| expected.name == *variant)
                .ok_or_else(|| format!("checked enum variant `{type_name}::{variant}` is absent"))?;
            if args.len() != expected.args.len() {
                return Err("checked Enum carrier payload count differs".to_string());
            }
            for ((name, value), (expected_name, child)) in args.iter().zip(&expected.args) {
                if name != expected_name {
                    return Err("checked Enum carrier payload name/order differs".to_string());
                }
                bootstrap_entry_validate_prepared(
                    value,
                    &bootstrap_entry_shape_node_type(shape, *child, depth + 1)?,
                    shape,
                    *child,
                    depth + 1,
                )?;
            }
            Ok(())
        }
    }
}
pub(crate) fn bootstrap_entry_check_prepared<T>(
    value: &BootstrapPreparedRuntimeValue<T>,
    checked: &MirType,
    shape: &BootstrapEntryHostTypeShape,
    shape_node: usize,
    depth: usize,
) -> Result<(), String> {
    bootstrap_entry_validate_prepared(value, checked, shape, shape_node, depth)
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
        let symbols = BootstrapCodecSymbols::new(bindings)?;
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
    for root_name in [
        "JetEvalTaskRoot",
        "JetEvalOwnedRoot",
        "JetEvalRuntimeValue",
        "JetEvalCallbackResult",
        "JetEvalTaskCallbackInvokeResult",
        "JetEvalSharedHostCarrier",
        "MIRProgram",
        "MIRType",
        "JetEvalHostTypeShape",
        "JetEvalConfig",
        "Span",
        "JetEvalOwnedRootDropResult",
    ] {
        let Some(definition) = program.types.iter().find(|definition| definition.name == root_name) else {
            symbols.record(BootstrapHostCodecError::MissingType(root_name.to_string()));
            continue;
        };
        let root_type = match helper_root_type(program, definition) {
            Ok(root_type) => root_type,
            Err(error) => {
                symbols.record(error);
                continue;
            }
        };
        let root_type = &root_type;
        validate_binding_graph(program, symbols, root_type);
        let mut root_graph = Vec::new();
        collect_nominal_types(program, root_type, &mut HashSet::new(), &mut root_graph)?;
        for nested in root_graph {
            if to_seen.insert(nested.id) {
                to_types.push(nested);
            }
            if from_seen.insert(nested.id) {
                from_types.push(nested);
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
    emit_interpreter_host_projection(out, symbols)?;
    emit_entry_interpreter_to_runtime(out, symbols)?;
    emit_entry_conversion_helpers(out)?;
    emit_entry_helper_root_transports(out, program, symbols)?;

    let request_source = symbols.type_symbol("JetDriverCompileRequest")?;
    let result_source = symbols.type_symbol("JetDriverCompileResult")?;
    let host_value = symbols.type_symbol("JetEvalHostValue")?;
    let request_expression = to_runtime_expression(
        program, symbols, &function.params[0].ty, "request", "checked",
        "&program", "path", "physical",
    )?;
    let result_expression = from_runtime_expression(
        program, symbols, &function.return_type, "value", "checked",
        "program", "path",
    )?;
    writeln!(
        out,
        "\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_request_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(request: {request_source}, program: ::std::sync::Arc<::jet_foundation::MIR::MirProgram>, physical: &P) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{\n    let __jet_request_type = __jet_bootstrap_entry_type_for(program.as_ref(), ::jet_foundation::MIR::MirFunctionId({entry_id}), false)?;\n    let checked = &__jet_request_type;\n    let path = vec![\"JetDriverCompileRequest\".to_string()];\n    let value = {request_expression};\n    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program.as_ref(), &__jet_request_type, &value, 0)?;\n    Ok(value)\n}}\n\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_request_to_host_value<P: crate::BootstrapEntryPhysicalBindings<InterpreterValue = {host_value}>>(request: {request_source}, program: ::std::sync::Arc<::jet_foundation::MIR::MirProgram>, physical: &P) -> Result<Vec<{host_value}>, String> {{\n    let value = __jet_bootstrap_entry_request_to_runtime(request, ::std::sync::Arc::clone(&program), physical)?;\n    let checked = __jet_bootstrap_entry_type_for(program.as_ref(), ::jet_foundation::MIR::MirFunctionId({entry_id}), false)?;\n    let path = vec![\"JetDriverCompileRequest\".to_string()];\n    Ok(vec![__jet_bootstrap_entry_host_value_from_runtime(value, &checked, program.as_ref(), &path, physical, 0)?])\n}}\n\n#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_result_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(value: ::jet_foundation::MIR::MirRuntimeValue, program: &::jet_foundation::MIR::MirProgram, physical: &P) -> Result<{result_source}, String> {{\n    let __jet_result_type = __jet_bootstrap_entry_type_for(program, ::jet_foundation::MIR::MirFunctionId({entry_id}), true)?;\n    let checked = &__jet_result_type;\n    crate::compiler_bootstrap_entry_codec::validate_runtime_value(program, &__jet_result_type, &value, 0)?;\n    let path = vec![\"JetDriverCompileResult\".to_string()];\n    {result_expression}\n}}\n",
        entry_id = entry.0,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn emit_interpreter_host_projection(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let host = symbols.type_symbol("JetEvalHostValue")?;
    let host_field = symbols.type_symbol("JetEvalHostField")?;
    let host_map_entry = symbols.type_symbol("JetEvalHostMapEntry")?;
    let host_enum_arg = symbols.type_symbol("JetEvalHostEnumArg")?;
    let replacements = [
        ("__JET_HOST_VALUE_TYPE__", host),
        ("__JET_HOST_FIELD_TYPE__", host_field),
        ("__JET_HOST_MAP_ENTRY_TYPE__", host_map_entry),
        ("__JET_HOST_ENUM_ARG_TYPE__", host_enum_arg),
        ("__JET_HOST_DATA__", symbols.variant_symbol("JetEvalHostValue", "Data")?),
        ("__JET_HOST_ABSENT__", symbols.variant_symbol("JetEvalHostValue", "Absent")?),
        ("__JET_HOST_PRESENT__", symbols.variant_symbol("JetEvalHostValue", "Present")?),
        ("__JET_HOST_FAILED__", symbols.variant_symbol("JetEvalHostValue", "Failed")?),
        ("__JET_HOST_LIST__", symbols.variant_symbol("JetEvalHostValue", "List")?),
        ("__JET_HOST_MAP__", symbols.variant_symbol("JetEvalHostValue", "Map")?),
        ("__JET_HOST_STRUCT__", symbols.variant_symbol("JetEvalHostValue", "Struct")?),
        ("__JET_HOST_ENUM__", symbols.variant_symbol("JetEvalHostValue", "Enum")?),
        ("__JET_HOST_CLOSURE__", symbols.variant_symbol("JetEvalHostValue", "Closure")?),
        ("__JET_HOST_MAP_KEY__", symbols.field_symbol("JetEvalHostMapEntry", "key")?),
        ("__JET_HOST_MAP_VALUE__", symbols.field_symbol("JetEvalHostMapEntry", "value")?),
        ("__JET_HOST_FIELD_NAME__", symbols.field_symbol("JetEvalHostField", "name")?),
        ("__JET_HOST_FIELD_VALUE__", symbols.field_symbol("JetEvalHostField", "value")?),
        ("__JET_HOST_ENUM_NAME__", symbols.field_symbol("JetEvalHostEnumArg", "name")?),
        ("__JET_HOST_ENUM_VALUE__", symbols.field_symbol("JetEvalHostEnumArg", "value")?),
    ];
    let mut generated = String::from(
        r#"
fn __jet_bootstrap_entry_projection_type<'a>(
    program: &'a ::jet_foundation::MIR::MirProgram,
    ty: &'a ::jet_foundation::MIR::MirType,
) -> Result<&'a ::jet_foundation::MIR::MirType, String> {
    use ::jet_foundation::MIR::{MirTypeDefKind as D, MirTypeKind as K};
    let K::Apply { name, .. } = ty.kind() else {
        return Ok(ty);
    };
    let definition = program.types.iter().find(|definition| definition.id == name.id)
        .ok_or_else(|| format!("checked entry projection type `{}` is absent", name.name))?;
    match &definition.kind {
        D::Alias { target } => __jet_bootstrap_entry_projection_type(program, target),
        D::Distinct { base, .. } => __jet_bootstrap_entry_projection_type(program, base),
        _ => Ok(ty),
    }
}

fn __jet_bootstrap_entry_projection_child_type(
    program: &::jet_foundation::MIR::MirProgram,
    ty: &::jet_foundation::MIR::MirType,
    kind: &str,
    index: usize,
) -> Result<::jet_foundation::MIR::MirType, String> {
    use ::jet_foundation::MIR::MirTypeKind as K;
    let ty = __jet_bootstrap_entry_projection_type(program, ty)?;
    let child = match (kind, ty.kind()) {
        ("list", K::List(inner)) | ("fixed-list", K::FixedList { elem: inner, .. })
        | ("option", K::Option(inner)) | ("shared", K::Shared(inner))
        | ("result", K::Result { ok: inner, .. }) if index == 0 => inner,
        ("result", K::Result { err, .. }) if index == 1 => err,
        ("map", K::Map { key, .. }) if index == 0 => key,
        ("map", K::Map { value, .. }) if index == 1 => value,
        ("tuple", K::Tuple(fields)) => fields.get(index)
            .map(|(_, ty)| ty)
            .ok_or_else(|| "checked tuple projection field is absent".to_string())?,
        _ => return Err(format!("checked MIR projection type `{}` has no {kind} child {index}", ty.canonical_key())),
    };
    Ok(child.clone())
}

fn __jet_bootstrap_entry_projection_struct_name(
    program: &::jet_foundation::MIR::MirProgram,
    ty: &::jet_foundation::MIR::MirType,
) -> Result<String, String> {
    use ::jet_foundation::MIR::{MirTypeDefKind as D, MirTypeKind as K};
    let ty = __jet_bootstrap_entry_projection_type(program, ty)?;
    match ty.kind() {
        K::Tuple(_) => Ok("Tuple".to_string()),
        K::Apply { name, .. } => {
            let definition = program.types.iter().find(|definition| definition.id == name.id)
                .ok_or_else(|| format!("checked entry projection type `{}` is absent", name.name))?;
            if matches!(&definition.kind, D::Struct { .. }) {
                Ok(definition.name.clone())
            } else {
                Err(format!("checked entry projection type `{}` is not a record", definition.name))
            }
        }
        _ => Err(format!("checked MIR projection type `{}` is not a record", ty.canonical_key())),
    }
}

fn __jet_bootstrap_entry_projection_struct_field_type(
    program: &::jet_foundation::MIR::MirProgram,
    ty: &::jet_foundation::MIR::MirType,
    runtime_type: &str,
    field_name: &str,
    index: usize,
) -> Result<::jet_foundation::MIR::MirType, String> {
    use ::jet_foundation::MIR::{MirTypeDefKind as D, MirTypeKind as K};
    let ty = __jet_bootstrap_entry_projection_type(program, ty)?;
    match ty.kind() {
        K::Tuple(fields) => {
            if runtime_type != "Tuple" {
                return Err("checked tuple projection has a non-tuple runtime name".to_string());
            }
            fields.get(index)
                .filter(|(name, _)| name == field_name)
                .map(|(_, ty)| ty.clone())
                .ok_or_else(|| "checked tuple projection field name/order differs".to_string())
        }
        K::Apply { name, .. } => {
            let definition = program.types.iter().find(|definition| definition.id == name.id)
                .ok_or_else(|| format!("checked entry projection type `{}` is absent", name.name))?;
            if definition.name != runtime_type {
                return Err("checked record projection has a mismatched runtime type name".to_string());
            }
            let D::Struct { fields, .. } = &definition.kind else {
                return Err(format!("checked entry projection type `{}` is not a record", definition.name));
            };
            fields.get(index)
                .filter(|field| field.name == field_name)
                .map(|field| field.ty.clone())
                .ok_or_else(|| "checked record projection field name/order differs".to_string())
        }
        _ => Err(format!("checked MIR projection type `{}` is not a record", ty.canonical_key())),
    }
}

fn __jet_bootstrap_entry_projection_enum_field_type(
    program: &::jet_foundation::MIR::MirProgram,
    ty: &::jet_foundation::MIR::MirType,
    runtime_type: &str,
    variant: &str,
    index: usize,
) -> Result<(Option<String>, ::jet_foundation::MIR::MirType), String> {
    use ::jet_foundation::MIR::{MirTypeDefKind as D, MirTypeKind as K, MirVariantPayload as P};
    let ty = __jet_bootstrap_entry_projection_type(program, ty)?;
    let K::Apply { name, .. } = ty.kind() else {
        return Err(format!("checked MIR projection type `{}` is not an enum", ty.canonical_key()));
    };
    let definition = program.types.iter().find(|definition| definition.id == name.id)
        .ok_or_else(|| format!("checked entry projection type `{}` is absent", name.name))?;
    if definition.name != runtime_type {
        return Err("checked enum projection has a mismatched runtime type name".to_string());
    }
    match &definition.kind {
        D::Enum { variants, .. } => {
            let row = variants.iter().find(|row| row.name == variant)
                .ok_or_else(|| format!("checked entry enum variant `{}::{variant}` is absent", definition.name))?;
            match &row.payload {
                P::Unit => Err("unit enum projection contains unexpected payload".to_string()),
                P::Single(ty) if index == 0 => Ok((None, ty.clone())),
                P::Named(fields) => fields.get(index)
                    .map(|field| (Some(field.name.clone()), field.ty.clone()))
                    .ok_or_else(|| "checked named enum projection field is absent".to_string()),
                _ => Err("checked enum projection payload index is invalid".to_string()),
            }
        }
        D::UnitFamily { members } if members.iter().any(|member| member == variant) => {
            Err("unit-family projection contains unexpected payload".to_string())
        }
        _ => Err(format!("checked entry projection type `{}` is not an enum", definition.name)),
    }
}
fn __jet_bootstrap_entry_host_value_from_runtime<P>(
    value: ::jet_foundation::MIR::MirRuntimeValue,
    checked: &::jet_foundation::MIR::MirType,
    program: &::jet_foundation::MIR::MirProgram,
    path: &[String],
    physical: &P,
    depth: usize,
) -> Result<__JET_HOST_VALUE_TYPE__, String>
where
    P: crate::BootstrapEntryPhysicalBindings<InterpreterValue = __JET_HOST_VALUE_TYPE__>,
{
    use ::jet_foundation::MIR::{MirRuntimeValue as V, MirTypeKind as K};
    if depth >= 256 {
        return Err("compiler-entry HostValue projection exceeds checked nesting bound".to_string());
    }
    match value {
        V::Present(inner) => {
            let shape = __jet_bootstrap_entry_projection_type(program, checked)?;
            let (kind, index) = match shape.kind() {
                K::Option(_) => ("option", 0),
                K::Result { .. } => ("result", 0),
                _ => return Err("checked HostValue Present carrier has no Option/Result type".to_string()),
            };
            let child_type = __jet_bootstrap_entry_projection_child_type(program, checked, kind, index)?;
            let mut child_path = path.to_vec();
            child_path.push("present".to_string());
            Ok(__JET_HOST_PRESENT__(__jet_bootstrap_entry_host_value_from_runtime(
                *inner, &child_type, program, &child_path, physical, depth + 1,
            )?))
        }
        V::FailedTold(inner) => {
            let child_type = __jet_bootstrap_entry_projection_child_type(program, checked, "result", 1)?;
            let mut child_path = path.to_vec();
            child_path.push("failed".to_string());
            Ok(__JET_HOST_FAILED__(__jet_bootstrap_entry_host_value_from_runtime(
                *inner, &child_type, program, &child_path, physical, depth + 1,
            )?))
        }
        V::Absent { element } => {
            let expected = __jet_bootstrap_entry_projection_child_type(program, checked, "option", 0)?;
            if !element.same_checked_type(&expected) {
                return Err("absent HostValue projection changed its checked element type".to_string());
            }
            Ok(__JET_HOST_ABSENT__(element))
        }
        V::List(values) => {
            let shape = __jet_bootstrap_entry_projection_type(program, checked)?;
            let kind = match shape.kind() {
                K::List(_) => "list",
                K::FixedList { .. } => "fixed-list",
                _ => return Err("checked HostValue List carrier has no sequence type".to_string()),
            };
            let item_type = __jet_bootstrap_entry_projection_child_type(program, checked, kind, 0)?;
            let values = values.into_iter().enumerate().map(|(index, value)| {
                let mut child_path = path.to_vec();
                child_path.push(format!("[{index}]"));
                __jet_bootstrap_entry_host_value_from_runtime(
                    value, &item_type, program, &child_path, physical, depth + 1,
                )
            }).collect::<Result<Vec<_>, String>>()?;
            Ok(__JET_HOST_LIST__(values))
        }
        V::Map(entries) => {
            let item_type = __jet_bootstrap_entry_projection_child_type(program, checked, "map", 1)?;
            let entries = entries.into_iter().enumerate().map(|(index, (key, value))| {
                let mut child_path = path.to_vec();
                child_path.push(format!("[{index}]"));
                Ok(__JET_HOST_MAP_ENTRY_TYPE__ {
                    __JET_HOST_MAP_KEY__: __JET_HOST_DATA__(__jet_bootstrap_ct_key_value_from_host(&key)?),
                    __JET_HOST_MAP_VALUE__: __jet_bootstrap_entry_host_value_from_runtime(
                        value, &item_type, program, &child_path, physical, depth + 1,
                    )?,
                })
            }).collect::<Result<Vec<_>, String>>()?;
            Ok(__JET_HOST_MAP__(entries))
        }
        V::Struct { type_name, fields } => {
            let expected_name = __jet_bootstrap_entry_projection_struct_name(program, checked)?;
            if type_name != expected_name {
                return Err("checked HostValue record projection has a mismatched type name".to_string());
            }
            let fields = fields.into_iter().enumerate().map(|(index, (name, value))| {
                let field_type = __jet_bootstrap_entry_projection_struct_field_type(
                    program, checked, &type_name, &name, index,
                )?;
                let mut child_path = path.to_vec();
                child_path.push(name.clone());
                Ok(__JET_HOST_FIELD_TYPE__ {
                    __JET_HOST_FIELD_NAME__: name,
                    __JET_HOST_FIELD_VALUE__: __jet_bootstrap_entry_host_value_from_runtime(
                        value, &field_type, program, &child_path, physical, depth + 1,
                    )?,
                })
            }).collect::<Result<Vec<_>, String>>()?;
            Ok(__JET_HOST_STRUCT__(type_name, fields))
        }
        V::Enum { type_name, variant, args } => {
            let args = args.into_iter().enumerate().map(|(index, (name, value))| {
                let (expected_name, field_type) = __jet_bootstrap_entry_projection_enum_field_type(
                    program, checked, &type_name, &variant, index,
                )?;
                if name != expected_name {
                    return Err("checked HostValue enum payload name/order differs".to_string());
                }
                let mut child_path = path.to_vec();
                child_path.push(format!("{type_name}::{variant}[{index}]"));
                Ok(__JET_HOST_ENUM_ARG_TYPE__ {
                    __JET_HOST_ENUM_NAME__: name.ok_or(::jet_foundation::Outcome::JetAbsent),
                    __JET_HOST_ENUM_VALUE__: __jet_bootstrap_entry_host_value_from_runtime(
                        value, &field_type, program, &child_path, physical, depth + 1,
                    )?,
                })
            }).collect::<Result<Vec<_>, String>>()?;
            Ok(__JET_HOST_ENUM__(type_name, variant, args))
        }
        V::Closure(closure) => {
            let function = program.functions.iter().find(|function| function.id == closure.function)
                .ok_or_else(|| format!("checked HostValue closure {:?} is absent", closure.function))?;
            if closure.captures.len() != function.capture_params.len() {
                return Err("checked HostValue closure capture count differs".to_string());
            }
            let captures = closure.captures.into_iter().zip(&function.capture_params).enumerate()
                .map(|(index, (value, capture))| {
                    let mut child_path = path.to_vec();
                    child_path.push(format!("capture[{index}]"));
                    __jet_bootstrap_entry_host_value_from_runtime(
                        value, &capture.ty, program, &child_path, physical, depth + 1,
                    )
                }).collect::<Result<Vec<_>, String>>()?;
            Ok(__JET_HOST_CLOSURE__(closure.function, captures))
        }
        V::NativeOwned(_) | V::NativeCursor(_) => {
            physical.project_interpreter(path, checked, program, value)?
                .ok_or_else(|| format!("NativeAdapter has no checked HostValue owner for `{}`", path.join(".")))
        }
        V::Moved => Err("moved runtime value cannot be projected to HostValue".to_string()),
        value => Ok(__JET_HOST_DATA__(__jet_bootstrap_ct_from_host(&value)?)),
    }
}
"#,
    );
    for (token, replacement) in replacements {
        generated = generated.replace(token, &replacement);
    }
    out.push_str(&generated);
    Ok(())
}
fn emit_entry_interpreter_to_runtime(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let host = symbols.type_symbol("JetEvalHostValue")?;
    writeln!(
        out,
        r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_runtime_to_interpreter<P>(
    value: ::jet_foundation::MIR::MirRuntimeValue,
    checked: &::jet_foundation::MIR::MirType,
    program: &::jet_foundation::MIR::MirProgram,
    physical: &P,
) -> Result<P::InterpreterValue, String>
where
    P: crate::BootstrapEntryPhysicalBindings<InterpreterValue = {host}>,
{{
    crate::compiler_bootstrap_entry_codec::validate_runtime_value(
        program, checked, &value, 0,
    )?;
    let path = vec!["$interpreter_value".to_string()];
    __jet_bootstrap_entry_host_value_from_runtime(
        value, checked, program, &path, physical, 0,
    )
}}
"#,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    emit_prepared_interpreter_transfer(out, symbols)?;
    emit_shaped_runtime_value_codec(out, symbols)?;
    Ok(())
}
fn emit_prepared_interpreter_transfer(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let mut replacements = vec![
        ("__SHAPE_TYPE__", symbols.type_symbol("JetEvalHostTypeShape")?),
        ("__SHAPE_NODE__", symbols.type_symbol("JetEvalHostTypeNode")?),
        (
            "__SHAPE_FIELD__",
            symbols.type_symbol("JetEvalHostTypeFieldShape")?,
        ),
        (
            "__SHAPE_VARIANT__",
            symbols.type_symbol("JetEvalHostTypeVariantShape")?,
        ),
        ("__SHAPE_ARG__", symbols.type_symbol("JetEvalHostTypeArgShape")?),
        ("__SHAPE_OWNER__", symbols.type_symbol("JetEvalHostOwner")?),
        (
            "__SHAPE_CORE_OWNER__",
            symbols.type_symbol("JetEvalHostCoreOwner")?,
        ),
        (
            "__SHAPE_NATIVE_BINDING__",
            symbols.type_symbol("JetEvalNativeBindingIdentity")?,
        ),
        (
            "__SHAPE_CALLABLE_BINDING__",
            symbols.type_symbol("JetEvalNativeCallableBinding")?,
        ),
        (
            "__SHAPE_OWNER_CURSOR__",
            symbols.variant_symbol("JetEvalHostOwner", "Cursor")?,
        ),
        (
            "__SHAPE_OWNER_DECLARED__",
            symbols.variant_symbol("JetEvalHostOwner", "Declared")?,
        ),
        (
            "__SHAPE_OWNER_CORE__",
            symbols.variant_symbol("JetEvalHostOwner", "Core")?,
        ),
        (
            "__SHAPE_OWNER_NATIVE__",
            symbols.variant_symbol("JetEvalHostOwner", "Native")?,
        ),
        (
            "__SHAPE_BINDING_CALLABLE__",
            symbols.variant_symbol("JetEvalNativeBindingIdentity", "Callable")?,
        ),
        (
            "__SHAPE_BINDING_INTERFACE__",
            symbols.variant_symbol("JetEvalNativeBindingIdentity", "Interface")?,
        ),
        (
            "__SHAPE_CORE_FACT__",
            symbols.field_symbol("JetEvalHostCoreOwner", "fact")?,
        ),
        (
            "__SHAPE_CORE_TYPE__",
            symbols.field_symbol("JetEvalHostCoreOwner", "ty")?,
        ),
        (
            "__SHAPE_CALLABLE_KEY__",
            symbols.field_symbol("JetEvalNativeCallableBinding", "key")?,
        ),
        (
            "__SHAPE_CALLABLE_TYPE__",
            symbols.field_symbol("JetEvalNativeCallableBinding", "callable_type")?,
        ),
        (
            "__TYPE_ID_VALUE__",
            symbols.field_symbol("MIRTypeID", "value")?,
        ),
        (
            "__FUNCTION_ID_VALUE__",
            symbols.field_symbol("MIRFunctionID", "value")?,
        ),
        (
            "__SHAPE_ROOT__",
            symbols.field_symbol("JetEvalHostTypeShape", "root")?,
        ),
        (
            "__SHAPE_NODES__",
            symbols.field_symbol("JetEvalHostTypeShape", "nodes")?,
        ),
        (
            "__FIELD_NAME__",
            symbols.field_symbol("JetEvalHostTypeFieldShape", "name")?,
        ),
        (
            "__FIELD_NODE__",
            symbols.field_symbol("JetEvalHostTypeFieldShape", "node")?,
        ),
        (
            "__VARIANT_NAME__",
            symbols.field_symbol("JetEvalHostTypeVariantShape", "name")?,
        ),
        (
            "__VARIANT_ARGS__",
            symbols.field_symbol("JetEvalHostTypeVariantShape", "args")?,
        ),
        (
            "__ARG_NAME__",
            symbols.field_symbol("JetEvalHostTypeArgShape", "name")?,
        ),
        (
            "__ARG_NODE__",
            symbols.field_symbol("JetEvalHostTypeArgShape", "node")?,
        ),
        ("__HOST_TYPE__", symbols.type_symbol("JetEvalHostValue")?),
        (
            "__HOST_MAP_ENTRY__",
            symbols.type_symbol("JetEvalHostMapEntry")?,
        ),
        ("__HOST_FIELD__", symbols.type_symbol("JetEvalHostField")?),
        ("__HOST_ENUM_ARG__", symbols.type_symbol("JetEvalHostEnumArg")?),
        (
            "__HOST_MAP_KEY__",
            symbols.field_symbol("JetEvalHostMapEntry", "key")?,
        ),
        (
            "__HOST_MAP_VALUE__",
            symbols.field_symbol("JetEvalHostMapEntry", "value")?,
        ),
        (
            "__HOST_FIELD_NAME__",
            symbols.field_symbol("JetEvalHostField", "name")?,
        ),
        (
            "__HOST_FIELD_VALUE__",
            symbols.field_symbol("JetEvalHostField", "value")?,
        ),
        (
            "__HOST_ENUM_NAME__",
            symbols.field_symbol("JetEvalHostEnumArg", "name")?,
        ),
        (
            "__HOST_ENUM_VALUE__",
            symbols.field_symbol("JetEvalHostEnumArg", "value")?,
        ),
    ];
    for variant in [
        "Scalar",
        "Option",
        "Result",
        "List",
        "Map",
        "Tuple",
        "Struct",
        "Enum",
        "Closure",
        "Handle",
    ] {
        replacements.push((
            match variant {
                "Scalar" => "__SHAPE_SCALAR__",
                "Option" => "__SHAPE_OPTION__",
                "Result" => "__SHAPE_RESULT__",
                "List" => "__SHAPE_LIST__",
                "Map" => "__SHAPE_MAP__",
                "Tuple" => "__SHAPE_TUPLE__",
                "Struct" => "__SHAPE_STRUCT__",
                "Enum" => "__SHAPE_ENUM__",
                "Closure" => "__SHAPE_CLOSURE__",
                _ => "__SHAPE_HANDLE__",
            },
            symbols.variant_symbol("JetEvalHostTypeNode", variant)?,
        ));
    }
    for variant in [
        "Data",
        "Handle",
        "Absent",
        "Present",
        "Failed",
        "List",
        "Map",
        "Struct",
        "Enum",
        "Closure",
        "SharedCarrier",
    ] {
        replacements.push((
            match variant {
                "Data" => "__HOST_DATA__",
                "Handle" => "__HOST_HANDLE__",
                "Absent" => "__HOST_ABSENT__",
                "Present" => "__HOST_PRESENT__",
                "Failed" => "__HOST_FAILED__",
                "List" => "__HOST_LIST__",
                "Map" => "__HOST_MAP__",
                "Struct" => "__HOST_STRUCT__",
                "Enum" => "__HOST_ENUM__",
                "Closure" => "__HOST_CLOSURE__",
                _ => "__HOST_SHARED_CARRIER__",
            },
            symbols.variant_symbol("JetEvalHostValue", variant)?,
        ));
    }
    let mut generated = String::from(
        r#"
pub(crate) fn __jet_bootstrap_entry_host_type_shape_from_source(
    source: &__SHAPE_TYPE__,
) -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostOwner as Owner,
        BootstrapEntryHostTypeField as Field,
        BootstrapEntryHostTypeNode as Node,
        BootstrapEntryHostTypeShape as Shape,
        BootstrapEntryHostTypeVariant as Variant,
    };
    let root = __jet_bootstrap_source_index(&source.__SHAPE_ROOT__, "host result type root")?;
    let nodes = source.__SHAPE_NODES__.iter().enumerate().map(|(index, node)| {
        Ok(match node {
            __SHAPE_SCALAR__(ty) => Node::Scalar(__jet_bootstrap_type_to_host(ty)?),
            __SHAPE_OPTION__(inner) => Node::Option(
                __jet_bootstrap_source_index(inner, "option type node")?,
            ),
            __SHAPE_RESULT__(ok, error) => Node::Result {
                ok: __jet_bootstrap_source_index(ok, "result success type node")?,
                error: __jet_bootstrap_source_index(error, "result error type node")?,
            },
            __SHAPE_LIST__(inner) => Node::List(
                __jet_bootstrap_source_index(inner, "list element type node")?,
            ),
            __SHAPE_MAP__(key, value) => Node::Map {
                key: __jet_bootstrap_source_index(key, "map key type node")?,
                value: __jet_bootstrap_source_index(value, "map value type node")?,
            },
            __SHAPE_TUPLE__(fields) => Node::Tuple(fields.iter().map(|field| {
                Ok(Field {
                    name: field.__FIELD_NAME__.clone(),
                    node: __jet_bootstrap_source_index(
                        &field.__FIELD_NODE__,
                        "tuple field type node",
                    )?,
                })
            }).collect::<Result<Vec<_>, String>>()?),
            __SHAPE_STRUCT__(type_id, type_name, args, fields) => Node::Struct {
                type_id: ::jet_foundation::MIR::MirTypeId(
                    __jet_bootstrap_source_u64(
                        &type_id.__TYPE_ID_VALUE__,
                        "host nominal type ID",
                    )?,
                ),
                type_name: type_name.clone(),
                args: args.iter().map(|arg| {
                    __jet_bootstrap_source_index(arg, "nominal argument type node")
                }).collect::<Result<Vec<_>, String>>()?,
                fields: fields.iter().map(|field| {
                    Ok(Field {
                        name: field.__FIELD_NAME__.clone(),
                        node: __jet_bootstrap_source_index(
                            &field.__FIELD_NODE__,
                            "record field type node",
                        )?,
                    })
                }).collect::<Result<Vec<_>, String>>()?,
            },
            __SHAPE_ENUM__(type_id, type_name, args, variants) => Node::Enum {
                type_id: ::jet_foundation::MIR::MirTypeId(
                    __jet_bootstrap_source_u64(
                        &type_id.__TYPE_ID_VALUE__,
                        "host nominal type ID",
                    )?,
                ),
                type_name: type_name.clone(),
                args: args.iter().map(|arg| {
                    __jet_bootstrap_source_index(arg, "nominal argument type node")
                }).collect::<Result<Vec<_>, String>>()?,
                variants: variants.iter().map(|variant| {
                    Ok(Variant {
                        name: variant.__VARIANT_NAME__.clone(),
                        args: variant.__VARIANT_ARGS__.iter().map(|arg| {
                            Ok((
                                arg.__ARG_NAME__.as_ref().ok().cloned(),
                                __jet_bootstrap_source_index(
                                    &arg.__ARG_NODE__,
                                    "enum payload type node",
                                )?,
                            ))
                        }).collect::<Result<Vec<_>, String>>()?,
                    })
                }).collect::<Result<Vec<_>, String>>()?,
            },
            __SHAPE_CLOSURE__(ty, function, captures) => Node::Closure {
                function: ::jet_foundation::MIR::MirFunctionId(
                    __jet_bootstrap_source_u64(
                        &function.__FUNCTION_ID_VALUE__,
                        "host closure function ID",
                    )?,
                ),
                captures: captures.iter().map(|capture| {
                    __jet_bootstrap_source_index(capture, "closure capture type node")
                }).collect::<Result<Vec<_>, String>>()?,
                ty: __jet_bootstrap_type_to_host(ty)?,
            },
            __SHAPE_HANDLE__(ty, owner) => Node::Handle {
                ty: __jet_bootstrap_type_to_host(ty)?,
                owner: match owner {
                    __SHAPE_OWNER_CURSOR__ => Owner::Cursor,
                    __SHAPE_OWNER_DECLARED__(ty) => Owner::Declared(
                        __jet_bootstrap_type_to_host(ty)?,
                    ),
                    __SHAPE_OWNER_CORE__(owner) => Owner::Core {
                        fact: __jet_bootstrap_mir_MIRCoreOwner_to_host(
                            &owner.__SHAPE_CORE_FACT__,
                        )?,
                        ty: __jet_bootstrap_type_to_host(&owner.__SHAPE_CORE_TYPE__)?,
                    },
                    __SHAPE_OWNER_NATIVE__(binding) => match binding {
                        __SHAPE_BINDING_CALLABLE__(binding) => Owner::NativeCallable {
                            key: binding.__SHAPE_CALLABLE_KEY__.clone(),
                            callable_type: __jet_bootstrap_type_to_host(
                                &binding.__SHAPE_CALLABLE_TYPE__,
                            )?,
                        },
                        __SHAPE_BINDING_INTERFACE__(receiver_type) => {
                            Owner::NativeInterface {
                                receiver_type: __jet_bootstrap_type_to_host(receiver_type)?,
                            }
                        }
                    },
                },
            },
        })
    }).collect::<Result<Vec<_>, String>>()?;
    let shape = Shape { root, nodes };
    crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(&shape)?;
    Ok(shape)
}

pub(crate) fn __jet_bootstrap_entry_host_type_shape_to_source(
    shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
) -> Result<__SHAPE_TYPE__, String> {
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostOwner as Owner,
        BootstrapEntryHostTypeNode as Node,
    };
    crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(shape)?;
    let source_index = |index: usize| -> Result<_, String> {
        Ok(jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(index as u64)))
    };
    let owner_to_source = |owner: &Owner| -> Result<__SHAPE_OWNER__, String> {
        Ok(match owner {
            Owner::Cursor => __SHAPE_OWNER_CURSOR__,
            Owner::Declared(ty) => {
                __SHAPE_OWNER_DECLARED__(__jet_bootstrap_type_from_host(ty)?)
            }
            Owner::Core { fact, ty } => __SHAPE_OWNER_CORE__(__SHAPE_CORE_OWNER__ {
                __SHAPE_CORE_FACT__: __jet_bootstrap_mir_MIRCoreOwner_from_host(fact)?,
                __SHAPE_CORE_TYPE__: __jet_bootstrap_type_from_host(ty)?,
            }),
            Owner::NativeCallable {
                key,
                callable_type,
            } => __SHAPE_OWNER_NATIVE__(__SHAPE_BINDING_CALLABLE__(
                __SHAPE_CALLABLE_BINDING__ {
                    __SHAPE_CALLABLE_KEY__: key.clone(),
                    __SHAPE_CALLABLE_TYPE__: __jet_bootstrap_type_from_host(callable_type)?,
                },
            )),
            Owner::NativeInterface { receiver_type } => __SHAPE_OWNER_NATIVE__(
                __SHAPE_BINDING_INTERFACE__(__jet_bootstrap_type_from_host(receiver_type)?),
            ),
        })
    };
    let nodes = shape.nodes.iter().map(|node| {
        Ok(match node {
            Node::Scalar(ty) => __SHAPE_SCALAR__(__jet_bootstrap_type_from_host(ty)?),
            Node::Option(inner) => __SHAPE_OPTION__(source_index(*inner)?),
            Node::Result { ok, error } => {
                __SHAPE_RESULT__(source_index(*ok)?, source_index(*error)?)
            }
            Node::List(inner) => __SHAPE_LIST__(source_index(*inner)?),
            Node::Map { key, value } => {
                __SHAPE_MAP__(source_index(*key)?, source_index(*value)?)
            }
            Node::Tuple(fields) => __SHAPE_TUPLE__(fields.iter().map(|field| {
                Ok(__SHAPE_FIELD__ {
                    __FIELD_NAME__: field.name.clone(),
                    __FIELD_NODE__: source_index(field.node)?,
                })
            }).collect::<Result<Vec<_>, String>>()?),
            Node::Struct {
                type_id,
                type_name,
                args,
                fields,
            } => __SHAPE_STRUCT__(
                __jet_bootstrap_mir_MIRTypeID_from_host(type_id)?,
                type_name.clone(),
                args.iter().map(|node| source_index(*node))
                    .collect::<Result<Vec<_>, String>>()?,
                fields.iter().map(|field| {
                    Ok(__SHAPE_FIELD__ {
                        __FIELD_NAME__: field.name.clone(),
                        __FIELD_NODE__: source_index(field.node)?,
                    })
                }).collect::<Result<Vec<_>, String>>()?,
            ),
            Node::Enum {
                type_id,
                type_name,
                args,
                variants,
            } => __SHAPE_ENUM__(
                __jet_bootstrap_mir_MIRTypeID_from_host(type_id)?,
                type_name.clone(),
                args.iter().map(|node| source_index(*node))
                    .collect::<Result<Vec<_>, String>>()?,
                variants.iter().map(|variant| {
                    Ok(__SHAPE_VARIANT__ {
                        __VARIANT_NAME__: variant.name.clone(),
                        __VARIANT_ARGS__: variant.args.iter().map(|(name, node)| {
                            Ok(__SHAPE_ARG__ {
                                __ARG_NAME__: match name {
                                    Some(name) => Ok(name.clone()),
                                    None => Err(::jet_foundation::Outcome::JetAbsent),
                                },
                                __ARG_NODE__: source_index(*node)?,
                            })
                        }).collect::<Result<Vec<_>, String>>()?,
                    })
                }).collect::<Result<Vec<_>, String>>()?,
            ),
            Node::Closure {
                function,
                captures,
                ty,
            } => __SHAPE_CLOSURE__(
                __jet_bootstrap_type_from_host(ty)?,
                __jet_bootstrap_mir_MIRFunctionID_from_host(function)?,
                captures.iter().map(|node| source_index(*node))
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            Node::Handle { ty, owner } => __SHAPE_HANDLE__(
                __jet_bootstrap_type_from_host(ty)?,
                owner_to_source(owner)?,
            ),
        })
    }).collect::<Result<Vec<_>, String>>()?;
    Ok(__SHAPE_TYPE__ {
        __SHAPE_ROOT__: source_index(shape.root)?,
        __SHAPE_NODES__: nodes,
    })
}

fn __jet_bootstrap_entry_host_value_to_prepared_runtime<P>(
    value: &__HOST_TYPE__,
    checked: &::jet_foundation::MIR::MirType,
    shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    shape_node: usize,
    field_path: &mut Vec<String>,
    projection_path: &mut Vec<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection>,
    physical: &P,
    depth: usize,
) -> Result<
    crate::compiler_bootstrap_entry_codec::BootstrapPreparedRuntimeValue<P::PreparedInterpreterValue>,
    String,
>
where
    P: crate::BootstrapEntryPhysicalBindings<InterpreterValue = __HOST_TYPE__>,
{
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostProjection as Path,
        BootstrapEntryHostTypeNode as Node,
        BootstrapPreparedRuntimeValue as Prepared,
    };
    use ::jet_foundation::MIR::{MirRuntimeValue as V, MirTypeKind as K};
    if depth >= 256 {
        return Err("compiler-entry interpreter value exceeds checked nesting bound".to_string());
    }
    let shape_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
        shape,
        shape_node,
        depth,
    )?;
    if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
        &shape_type,
        checked,
    ) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    match value {
        value @ (__HOST_HANDLE__(..) | __HOST_CLOSURE__(..) | __HOST_SHARED_CARRIER__(..)) => {
            let prepared = physical
                .prepare_interpreter(field_path, projection_path, checked, shape, shape_node, value)?
                .ok_or_else(|| format!("NativeAdapter has no checked physical owner for `{}`", field_path.join(".")))?;
            Ok(Prepared::Physical {
                prepared,
                projection_path: projection_path.clone(),
            })
        }
        __HOST_DATA__(data) => Ok(Prepared::Runtime(__jet_bootstrap_ct_to_host(data)?)),
        __HOST_ABSENT__(element) => {
            let Node::Option(inner_node) = &shape.nodes[shape_node] else {
                return Err("checked Absent HostValue has no Option shape".to_string());
            };
            let expected = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                *inner_node,
                depth + 1,
            )?;
            if !element.same_checked_type(&expected) || !matches!(checked.kind(), K::Option(_)) {
                return Err("absent HostValue changed its exact checked element type".to_string());
            }
            Ok(Prepared::Absent(element.clone()))
        }
        __HOST_PRESENT__(inner) => {
            let child_node = match (&shape.nodes[shape_node], checked.kind()) {
                (Node::Option(child), K::Option(_)) => *child,
                (Node::Result { ok, .. }, K::Result { .. }) => *ok,
                _ => return Err("checked HostValue Present carrier has no Option/Result type".to_string()),
            };
            field_path.push("present".to_string());
            projection_path.push(Path::Present);
            let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                child_node,
                depth + 1,
            )?;
            let prepared = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                inner,
                &child_type,
                shape,
                child_node,
                field_path,
                projection_path,
                physical,
                depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            Ok(Prepared::Present(Box::new(prepared?)))
        }
        __HOST_FAILED__(inner) => {
            let Node::Result { error, .. } = &shape.nodes[shape_node] else {
                return Err("checked HostValue Failed carrier has no Result type".to_string());
            };
            let child_node = *error;
            field_path.push("failed".to_string());
            projection_path.push(Path::Failed);
            let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                child_node,
                depth + 1,
            )?;
            let prepared = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                inner,
                &child_type,
                shape,
                child_node,
                field_path,
                projection_path,
                physical,
                depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            Ok(Prepared::Failed(Box::new(prepared?)))
        }
        __HOST_LIST__(values) => {
            let Node::List(inner_node) = &shape.nodes[shape_node] else {
                return Err("checked HostValue List carrier has no sequence shape".to_string());
            };
            if !matches!(checked.kind(), K::List(_) | K::FixedList { .. }) {
                return Err("checked HostValue List carrier has no sequence type".to_string());
            }
            if let K::FixedList { len, .. } = checked.kind() {
                if let ::jet_foundation::MIR::MirMeasure::Literal { value: expected, .. } = len {
                    if values.len() as u64 != *expected {
                        return Err("checked HostValue fixed-list length differs".to_string());
                    }
                }
            }
            let child_node = *inner_node;
            let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                child_node,
                depth + 1,
            )?;
            let mut prepared = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                field_path.push(format!("[{index}]"));
                projection_path.push(Path::List(index));
                let child = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                    value,
                    &child_type,
                    shape,
                    child_node,
                    field_path,
                    projection_path,
                    physical,
                    depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                prepared.push(child?);
            }
            Ok(Prepared::List(prepared))
        }
        __HOST_MAP__(entries) => {
            let Node::Map { key: key_node, value: value_node } = &shape.nodes[shape_node] else {
                return Err("checked HostValue Map carrier has no map shape".to_string());
            };
            if !matches!(checked.kind(), K::Map { .. }) {
                return Err("checked HostValue Map carrier has no map type".to_string());
            }
            let key_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                *key_node,
                depth + 1,
            )?;
            let value_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape,
                *value_node,
                depth + 1,
            )?;
            let mut prepared = Vec::with_capacity(entries.len());
            for (index, entry) in entries.iter().enumerate() {
                field_path.push(format!("[{index}]"));
                projection_path.push(Path::MapKey(index));
                let key_value = match &entry.__HOST_MAP_KEY__ {
                    __HOST_DATA__(data) => __jet_bootstrap_ct_to_host(data)?,
                    _ => return Err("checked map key is not immutable data".to_string()),
                };
                let key = __jet_bootstrap_runtime_key_from_host(&key_value)?;
                projection_path.pop();
                projection_path.push(Path::MapValue(index));
                let value = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                    &entry.__HOST_MAP_VALUE__,
                    &value_type,
                    shape,
                    *value_node,
                    field_path,
                    projection_path,
                    physical,
                    depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                prepared.push((key, value?));
            }
            let _ = key_type;
            Ok(Prepared::Map(prepared))
        }
        __HOST_STRUCT__(type_name, fields) => {
            let shape_fields = match &shape.nodes[shape_node] {
                Node::Tuple(fields) if type_name == "Tuple" => fields,
                Node::Struct { type_name: expected, fields, .. } if type_name == expected => fields,
                _ => return Err("checked HostValue record has a mismatched type name".to_string()),
            };
            if shape_fields.len() != fields.len() {
                return Err("checked HostValue record has a mismatched field count".to_string());
            }
            let mut prepared = Vec::with_capacity(fields.len());
            for (index, (field, expected)) in fields.iter().zip(shape_fields).enumerate() {
                if field.__HOST_FIELD_NAME__ != expected.name {
                    return Err(format!("checked HostValue record field {index} name/order differs"));
                }
                let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape,
                    expected.node,
                    depth + 1,
                )?;
                field_path.push(field.__HOST_FIELD_NAME__.clone());
                projection_path.push(Path::StructField(field.__HOST_FIELD_NAME__.clone()));
                let value = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                    &field.__HOST_FIELD_VALUE__,
                    &child_type,
                    shape,
                    expected.node,
                    field_path,
                    projection_path,
                    physical,
                    depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                prepared.push((field.__HOST_FIELD_NAME__.clone(), value?));
            }
            Ok(Prepared::Struct {
                type_name: type_name.clone(),
                fields: prepared,
            })
        }
        __HOST_ENUM__(type_name, variant, args) => {
            let Node::Enum {
                type_name: expected_type,
                variants,
                ..
            } = &shape.nodes[shape_node]
            else {
                return Err("checked HostValue enum has no enum shape".to_string());
            };
            if type_name != expected_type {
                return Err("checked HostValue enum has a mismatched type name".to_string());
            }
            let expected = variants
                .iter()
                .find(|row| row.name == *variant)
                .ok_or_else(|| format!("checked enum variant `{type_name}::{variant}` is absent"))?;
            if expected.args.len() != args.len() {
                return Err("checked HostValue enum payload count differs".to_string());
            }
            let mut prepared = Vec::with_capacity(args.len());
            for (index, (arg, (expected_name, child_node))) in
                args.iter().zip(&expected.args).enumerate()
            {
                let actual_name = arg.__HOST_ENUM_NAME__.as_ref().ok().cloned();
                if actual_name != *expected_name {
                    return Err(format!("checked HostValue enum payload {index} name/order differs"));
                }
                let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape,
                    *child_node,
                    depth + 1,
                )?;
                field_path.push(format!("{type_name}::{variant}[{index}]"));
                projection_path.push(Path::EnumArg(index));
                let value = __jet_bootstrap_entry_host_value_to_prepared_runtime(
                    &arg.__HOST_ENUM_VALUE__,
                    &child_type,
                    shape,
                    *child_node,
                    field_path,
                    projection_path,
                    physical,
                    depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                prepared.push((actual_name, value?));
            }
            Ok(Prepared::Enum {
                type_name: type_name.clone(),
                variant: variant.clone(),
                args: prepared,
            })
        }
    }
}

#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_prepare_interpreter_transfer<P>(
    value: &__HOST_TYPE__,
    checked: &::jet_foundation::MIR::MirType,
    shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    physical: &P,
) -> Result<
    crate::compiler_bootstrap_entry_codec::BootstrapEntryPreparedInterpreterTransfer<P::PreparedInterpreterValue>,
    String,
>
where
    P: crate::BootstrapEntryPhysicalBindings<InterpreterValue = __HOST_TYPE__>,
{
    crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(shape)?;
    let shape_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
        shape,
        shape.root,
        0,
    )?;
    if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
        &shape_type,
        checked,
    ) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    let mut field_path = vec!["$interpreter_value".to_string()];
    let mut projection_path = Vec::new();
    let value = __jet_bootstrap_entry_host_value_to_prepared_runtime(
        value,
        checked,
        shape,
        shape.root,
        &mut field_path,
        &mut projection_path,
        physical,
        0,
    )?;
    crate::compiler_bootstrap_entry_codec::bootstrap_entry_check_prepared(
        &value,
        checked,
        shape,
        shape.root,
        0,
    )?;
    Ok(crate::compiler_bootstrap_entry_codec::BootstrapEntryPreparedInterpreterTransfer::new(
        checked.clone(),
        value,
    ))
}
"#,
    );
    for (token, replacement) in replacements {
        generated = generated.replace(token, &replacement);
    }
    out.push_str(&generated);
    Ok(())
}
fn emit_shaped_runtime_value_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let mut replacements = vec![
        ("__EVAL_TYPE__", symbols.type_symbol("JetEvalRuntimeValue")?),
        (
            "__EVAL_AGGREGATE__",
            symbols.type_symbol("JetEvalRuntimeAggregate")?,
        ),
        (
            "__EVAL_MAP_ENTRY__",
            symbols.type_symbol("JetEvalRuntimeMapEntry")?,
        ),
        (
            "__EVAL_FIELD__",
            symbols.type_symbol("JetEvalRuntimeField")?,
        ),
        (
            "__EVAL_ENUM_ARG__",
            symbols.type_symbol("JetEvalRuntimeEnumArg")?,
        ),
        (
            "__EVAL_MAP_KEY__",
            symbols.field_symbol("JetEvalRuntimeMapEntry", "key")?,
        ),
        (
            "__EVAL_MAP_VALUE__",
            symbols.field_symbol("JetEvalRuntimeMapEntry", "value")?,
        ),
        (
            "__EVAL_FIELD_NAME__",
            symbols.field_symbol("JetEvalRuntimeField", "name")?,
        ),
        (
            "__EVAL_FIELD_VALUE__",
            symbols.field_symbol("JetEvalRuntimeField", "value")?,
        ),
        (
            "__EVAL_ENUM_NAME__",
            symbols.field_symbol("JetEvalRuntimeEnumArg", "name")?,
        ),
        (
            "__EVAL_ENUM_VALUE__",
            symbols.field_symbol("JetEvalRuntimeEnumArg", "value")?,
        ),
    ];
    for variant in ["List", "Map", "Struct", "Enum"] {
        replacements.push((
            match variant {
                "List" => "__AGGREGATE_LIST__",
                "Map" => "__AGGREGATE_MAP__",
                "Struct" => "__AGGREGATE_STRUCT__",
                _ => "__AGGREGATE_ENUM__",
            },
            symbols.variant_symbol("JetEvalRuntimeAggregate", variant)?,
        ));
    }
    for variant in [
        "Moved",
        "Data",
        "Absent",
        "Result",
        "Closure",
        "Address",
        "SharedCell",
        "Shared",
        "SharedWeak",
        "SharedGuard",
        "SharedSnapshot",
        "Condition",
        "RangeCursor",
        "ListCursor",
        "ForeignHandle",
        "Aggregate",
        "RuntimeFailure",
        "HostCursor",
    ] {
        replacements.push((
            match variant {
                "Moved" => "__EVAL_MOVED__",
                "Data" => "__EVAL_DATA__",
                "Absent" => "__EVAL_ABSENT__",
                "Result" => "__EVAL_RESULT__",
                "Closure" => "__EVAL_CLOSURE__",
                "Address" => "__EVAL_ADDRESS__",
                "SharedCell" => "__EVAL_SHARED_CELL__",
                "Shared" => "__EVAL_SHARED__",
                "SharedWeak" => "__EVAL_SHARED_WEAK__",
                "SharedGuard" => "__EVAL_SHARED_GUARD__",
                "SharedSnapshot" => "__EVAL_SHARED_SNAPSHOT__",
                "Condition" => "__EVAL_CONDITION__",
                "RangeCursor" => "__EVAL_RANGE_CURSOR__",
                "ListCursor" => "__EVAL_LIST_CURSOR__",
                "ForeignHandle" => "__EVAL_FOREIGN_HANDLE__",
                "Aggregate" => "__EVAL_RUNTIME_AGGREGATE__",
                "RuntimeFailure" => "__EVAL_RUNTIME_FAILURE__",
                _ => "__EVAL_HOST_CURSOR__",
            },
            symbols.variant_symbol("JetEvalRuntimeValue", variant)?,
        ));
    }
    for variant in ["Option", "Result", "List", "Map", "Tuple", "Struct", "Enum", "Closure", "Handle"] {
        replacements.push((
            match variant {
                "Option" => "__SHAPE_OPTION__",
                "Result" => "__SHAPE_RESULT__",
                "List" => "__SHAPE_LIST__",
                "Map" => "__SHAPE_MAP__",
                "Tuple" => "__SHAPE_TUPLE__",
                "Struct" => "__SHAPE_STRUCT__",
                "Enum" => "__SHAPE_ENUM__",
                "Closure" => "__SHAPE_CLOSURE__",
                _ => "__SHAPE_HANDLE__",
            },
            symbols.variant_symbol("JetEvalHostTypeNode", variant)?,
        ));
    }
    let mut generated = String::from(
        r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_runtime_value_to_mir<M>(
    value: &__EVAL_TYPE__,
    checked: &::jet_foundation::MIR::MirType,
    shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    shape_node: usize,
    field_path: &mut Vec<String>,
    projection_path: &mut Vec<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection>,
    physical: &M,
    depth: usize,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String>
where
    M: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedValueMarshaller,
{
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostProjection as Path,
        BootstrapEntryHostTypeNode as Node,
    };
    use ::jet_foundation::MIR::{MirRuntimeValue as V, MirTypeKind as K};
    if depth >= 256 {
        return Err("compiler-entry Source runtime value exceeds checked nesting bound".to_string());
    }
    let shape_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
        shape,
        shape_node,
        depth,
    )?;
    if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
        &shape_type,
        checked,
    ) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    match value {
        __EVAL_MOVED__ => Err("moved Source runtime values cannot cross a compiler boundary".to_string()),
        __EVAL_DATA__(data) => __jet_bootstrap_ct_to_host(data),
        __EVAL_ABSENT__(element) => {
            let Node::Option(inner) = &shape.nodes[shape_node] else {
                return Err("checked absent Source value has no Option shape".to_string());
            };
            let expected = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape, *inner, depth + 1,
            )?;
            let element = __jet_bootstrap_type_to_host(element)?;
            if !element.same_checked_type(&expected) {
                return Err("absent Source value changed its exact checked element type".to_string());
            }
            Ok(V::Absent { element })
        }
        __EVAL_RESULT__(ok, value) => {
            let (child, absent) = match (&shape.nodes[shape_node], checked.kind()) {
                (Node::Option(child), K::Option(_)) => (*child, !*ok),
                (Node::Result { ok: child, .. }, K::Result { .. }) if *ok => (*child, false),
                (Node::Result { error: child, .. }, K::Result { .. }) => (*child, false),
                _ => return Err("checked Source Result carrier has no Option/Result type".to_string()),
            };
            let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                shape, child, depth + 1,
            )?;
            if absent {
                return Ok(V::Absent { element: child_type });
            }
            field_path.push(if *ok { "present" } else { "failed" }.to_string());
            projection_path.push(if *ok { Path::Present } else { Path::Failed });
            let result = __jet_bootstrap_entry_runtime_value_to_mir(
                value,
                &child_type,
                shape,
                child,
                field_path,
                projection_path,
                physical,
                depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            result.map(|value| if *ok { V::Present(Box::new(value)) } else { V::FailedTold(Box::new(value)) })
        }
        __EVAL_AGGREGATE__(aggregate) => match aggregate {
            __AGGREGATE_LIST__(values) => {
                let Node::List(child) = &shape.nodes[shape_node] else {
                    return Err("checked Source list has no sequence shape".to_string());
                };
                if !matches!(checked.kind(), K::List(_) | K::FixedList { .. }) {
                    return Err("checked Source list has no sequence type".to_string());
                }
                if let K::FixedList { len, .. } = checked.kind() {
                    if let ::jet_foundation::MIR::MirMeasure::Literal { value: expected, .. } = len {
                        if values.len() as u64 != *expected {
                            return Err("checked Source fixed-list length differs".to_string());
                        }
                    }
                }
                let child = *child;
                let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape, child, depth + 1,
                )?;
                let mut output = Vec::with_capacity(values.len());
                for (index, value) in values.iter().enumerate() {
                    field_path.push(format!("[{index}]"));
                    projection_path.push(Path::List(index));
                    let converted = __jet_bootstrap_entry_runtime_value_to_mir(
                        value,
                        &child_type,
                        shape,
                        child,
                        field_path,
                        projection_path,
                        physical,
                        depth + 1,
                    );
                    field_path.pop();
                    projection_path.pop();
                    output.push(converted?);
                }
                Ok(V::List(output))
            }
            __AGGREGATE_MAP__(entries) => {
                let Node::Map { key, value: child } = &shape.nodes[shape_node] else {
                    return Err("checked Source map has no map shape".to_string());
                };
                if !matches!(checked.kind(), K::Map { .. }) {
                    return Err("checked Source map has no map type".to_string());
                }
                let _key_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape, *key, depth + 1,
                )?;
                let child = *child;
                let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape, child, depth + 1,
                )?;
                let mut output = Vec::with_capacity(entries.len());
                for (index, entry) in entries.iter().enumerate() {
                    field_path.push(format!("[{index}]"));
                    projection_path.push(Path::MapKey(index));
                    let key = __jet_bootstrap_eval_key_to_host(&entry.__EVAL_MAP_KEY__);
                    projection_path.pop();
                    projection_path.push(Path::MapValue(index));
                    let value = __jet_bootstrap_entry_runtime_value_to_mir(
                        &entry.__EVAL_MAP_VALUE__,
                        &child_type,
                        shape,
                        child,
                        field_path,
                        projection_path,
                        physical,
                        depth + 1,
                    );
                    field_path.pop();
                    projection_path.pop();
                    output.push((key?, value?));
                }
                Ok(V::Map(output))
            }
            __AGGREGATE_STRUCT__(type_name, fields) => {
                let shape_fields = match &shape.nodes[shape_node] {
                    Node::Tuple(fields) if type_name == "Tuple" => fields,
                    Node::Struct { type_name: expected, fields, .. } if type_name == expected => fields,
                    _ => return Err("checked Source record type name differs from its shape".to_string()),
                };
                if shape_fields.len() != fields.len() {
                    return Err("checked Source record field count differs".to_string());
                }
                let mut output = Vec::with_capacity(fields.len());
                for (index, (field, expected)) in fields.iter().zip(shape_fields).enumerate() {
                    if field.__EVAL_FIELD_NAME__ != expected.name {
                        return Err(format!("checked Source record field {index} name/order differs"));
                    }
                    let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                        shape, expected.node, depth + 1,
                    )?;
                    field_path.push(field.__EVAL_FIELD_NAME__.clone());
                    projection_path.push(Path::StructField(field.__EVAL_FIELD_NAME__.clone()));
                    let value = __jet_bootstrap_entry_runtime_value_to_mir(
                        &field.__EVAL_FIELD_VALUE__,
                        &child_type,
                        shape,
                        expected.node,
                        field_path,
                        projection_path,
                        physical,
                        depth + 1,
                    );
                    field_path.pop();
                    projection_path.pop();
                    output.push((field.__EVAL_FIELD_NAME__.clone(), value?));
                }
                Ok(V::Struct { type_name: type_name.clone(), fields: output })
            }
            __AGGREGATE_ENUM__(type_name, variant, args) => {
                let Node::Enum { type_name: expected_type, variants, .. } = &shape.nodes[shape_node] else {
                    return Err("checked Source enum has no enum shape".to_string());
                };
                if type_name != expected_type {
                    return Err("checked Source enum type name differs".to_string());
                }
                let expected = variants.iter().find(|row| row.name == *variant)
                    .ok_or_else(|| format!("checked enum variant `{type_name}::{variant}` is absent"))?;
                if expected.args.len() != args.len() {
                    return Err("checked Source enum payload count differs".to_string());
                }
                let mut output = Vec::with_capacity(args.len());
                for (index, (arg, (expected_name, child))) in args.iter().zip(&expected.args).enumerate() {
                    let actual_name = arg.__EVAL_ENUM_NAME__.as_ref().ok().cloned();
                    if actual_name != *expected_name {
                        return Err(format!("checked Source enum payload {index} name/order differs"));
                    }
                    let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                        shape, *child, depth + 1,
                    )?;
                    field_path.push(format!("{type_name}::{variant}[{index}]"));
                    projection_path.push(Path::EnumArg(index));
                    let value = __jet_bootstrap_entry_runtime_value_to_mir(
                        &arg.__EVAL_ENUM_VALUE__,
                        &child_type,
                        shape,
                        *child,
                        field_path,
                        projection_path,
                        physical,
                        depth + 1,
                    );
                    field_path.pop();
                    projection_path.pop();
                    output.push((actual_name, value?));
                }
                Ok(V::Enum { type_name: type_name.clone(), variant: variant.clone(), args: output })
            }
        },
        __EVAL_CLOSURE__(function, captures) => {
            let Node::Closure { function: expected_function, captures: capture_nodes, .. } = &shape.nodes[shape_node] else {
                return Err("checked Source closure has no closure shape".to_string());
            };
            if function.0 != expected_function.0 || captures.len() != capture_nodes.len() {
                return Err("checked Source closure identity/capture layout differs".to_string());
            }
            let mut output = Vec::with_capacity(captures.len());
            for (index, (value, child)) in captures.iter().zip(capture_nodes).enumerate() {
                let child_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                    shape, *child, depth + 1,
                )?;
                field_path.push(format!("capture[{index}]"));
                projection_path.push(Path::Capture(index));
                let converted = __jet_bootstrap_entry_runtime_value_to_mir(
                    value,
                    &child_type,
                    shape,
                    *child,
                    field_path,
                    projection_path,
                    physical,
                    depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(converted?);
            }
            Ok(V::Closure(::jet_foundation::MIR::MirRuntimeClosure {
                function: *function,
                captures: output,
            }))
        }
        __EVAL_RUNTIME_FAILURE__(..) => Err("terminal runtime failures cannot cross a compiler Shared boundary".to_string()),
        value => physical.encode_payload(
            field_path,
            projection_path,
            checked,
            shape,
            shape_node,
            value,
        ),
    }
}

#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_runtime_value_from_mir<M>(
    value: ::jet_foundation::MIR::MirRuntimeValue,
    checked: &::jet_foundation::MIR::MirType,
    shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    shape_node: usize,
    field_path: &mut Vec<String>,
    projection_path: &mut Vec<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection>,
    physical: &M,
    depth: usize,
) -> Result<__EVAL_TYPE__, String>
where
    M: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedValueMarshaller,
{
    use crate::compiler_bootstrap_entry_codec::{
        BootstrapEntryHostProjection as Path,
        BootstrapEntryHostTypeNode as Node,
    };
    use ::jet_foundation::MIR::{MirRuntimeValue as V, MirTypeKind as K};
    if depth >= 256 {
        return Err("compiler-entry MIR runtime value exceeds checked nesting bound".to_string());
    }
    let shape_type = crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
        shape, shape_node, depth,
    )?;
    if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
        &shape_type, checked,
    ) {
        return Err("compiler-entry guest type shape differs from checked MIR type".to_string());
    }
    let child_type = |node| crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
        shape, node, depth + 1,
    );
    match (&shape.nodes[shape_node], value) {
        (Node::Option(child), V::Absent { element }) => {
            if !element.same_checked_type(&child_type(*child)?) {
                return Err("absent MIR value changed its exact checked element type".to_string());
            }
            Ok(__EVAL_ABSENT__(__jet_bootstrap_type_from_host(&element)?))
        }
        (Node::Option(child), V::Present(value)) => {
            if !matches!(checked.kind(), K::Option(_)) {
                return Err("checked MIR Present carrier has no Option type".to_string());
            }
            let child = *child;
            let ty = child_type(child)?;
            field_path.push("present".to_string());
            projection_path.push(Path::Present);
            let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                *value, &ty, shape, child, field_path, projection_path, physical, depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            converted.map(|value| __EVAL_RESULT__(true, value))
        }
        (Node::Result { ok, .. }, V::Present(value)) => {
            if !matches!(checked.kind(), K::Result { .. }) {
                return Err("checked MIR Present carrier has no Result type".to_string());
            }
            let child = *ok;
            let ty = child_type(child)?;
            field_path.push("present".to_string());
            projection_path.push(Path::Present);
            let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                *value, &ty, shape, child, field_path, projection_path, physical, depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            converted.map(|value| __EVAL_RESULT__(true, value))
        }
        (Node::Result { error, .. }, V::FailedTold(value)) => {
            if !matches!(checked.kind(), K::Result { .. }) {
                return Err("checked MIR Failed carrier has no Result type".to_string());
            }
            let child = *error;
            let ty = child_type(child)?;
            field_path.push("failed".to_string());
            projection_path.push(Path::Failed);
            let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                *value, &ty, shape, child, field_path, projection_path, physical, depth + 1,
            );
            field_path.pop();
            projection_path.pop();
            converted.map(|value| __EVAL_RESULT__(false, value))
        }
        (Node::List(child), V::List(values)) => {
            if !matches!(checked.kind(), K::List(_) | K::FixedList { .. }) {
                return Err("checked MIR sequence has no List type".to_string());
            }
            if let K::FixedList { len, .. } = checked.kind() {
                if let ::jet_foundation::MIR::MirMeasure::Literal { value: expected, .. } = len {
                    if values.len() as u64 != *expected {
                        return Err("checked MIR fixed-list length differs".to_string());
                    }
                }
            }
            let child = *child;
            let ty = child_type(child)?;
            let mut output = Vec::with_capacity(values.len());
            for (index, value) in values.into_iter().enumerate() {
                field_path.push(format!("[{index}]"));
                projection_path.push(Path::List(index));
                let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                    value, &ty, shape, child, field_path, projection_path, physical, depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(converted?);
            }
            Ok(__EVAL_RUNTIME_AGGREGATE__(__AGGREGATE_LIST__(output)))
        }
        (Node::Map { key: key_node, value: child }, V::Map(entries)) => {
            if !matches!(checked.kind(), K::Map { .. }) {
                return Err("checked MIR map has no Map type".to_string());
            }
            let _key_type = child_type(*key_node)?;
            let child = *child;
            let ty = child_type(child)?;
            let mut output = Vec::with_capacity(entries.len());
            for (index, (key, value)) in entries.into_iter().enumerate() {
                field_path.push(format!("[{index}]"));
                projection_path.push(Path::MapKey(index));
                let key = __jet_bootstrap_ct_key_value_from_host(&key)?;
                projection_path.pop();
                projection_path.push(Path::MapValue(index));
                let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                    value, &ty, shape, child, field_path, projection_path, physical, depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(__EVAL_MAP_ENTRY__ {
                    __EVAL_MAP_KEY__: __EVAL_DATA__(key),
                    __EVAL_MAP_VALUE__: converted?,
                });
            }
            Ok(__EVAL_RUNTIME_AGGREGATE__(__AGGREGATE_MAP__(output)))
        }
        (Node::Tuple(fields), V::Struct { type_name, fields: values })
        | (Node::Struct { type_name: _, fields }, V::Struct { type_name, fields: values }) => {
            if values.len() != fields.len() {
                return Err("checked MIR record field count differs".to_string());
            }
            let mut output = Vec::with_capacity(values.len());
            for (index, (field, (name, value))) in fields.iter().zip(values).enumerate() {
                if field.name != name {
                    return Err(format!("checked MIR record field {index} name/order differs"));
                }
                let ty = child_type(field.node)?;
                field_path.push(field.name.clone());
                projection_path.push(Path::StructField(field.name.clone()));
                let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                    value, &ty, shape, field.node, field_path, projection_path, physical, depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(__EVAL_FIELD__ {
                    __EVAL_FIELD_NAME__: field.name.clone(),
                    __EVAL_FIELD_VALUE__: converted?,
                });
            }
            if type_name != "Tuple" && !matches!(&shape.nodes[shape_node], Node::Struct { type_name: expected, .. } if expected == &type_name) {
                return Err("checked MIR record type name differs".to_string());
            }
            Ok(__EVAL_RUNTIME_AGGREGATE__(__AGGREGATE_STRUCT__(type_name, output)))
        }
        (Node::Enum { type_name: expected_type, variants, .. }, V::Enum { type_name, variant, args }) => {
            if type_name != *expected_type {
                return Err("checked MIR enum type name differs".to_string());
            }
            let expected = variants.iter().find(|row| row.name == variant)
                .ok_or_else(|| format!("checked enum variant `{type_name}::{variant}` is absent"))?;
            if args.len() != expected.args.len() {
                return Err("checked MIR enum payload count differs".to_string());
            }
            let mut output = Vec::with_capacity(args.len());
            for (index, ((expected_name, child), (name, value))) in
                expected.args.iter().zip(args).enumerate()
            {
                if expected_name != &name {
                    return Err(format!("checked MIR enum payload {index} name/order differs"));
                }
                let ty = child_type(*child)?;
                field_path.push(format!("{type_name}::{variant}[{index}]"));
                projection_path.push(Path::EnumArg(index));
                let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                    value, &ty, shape, *child, field_path, projection_path, physical, depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(__EVAL_ENUM_ARG__ {
                    __EVAL_ENUM_NAME__: match name {
                        Some(name) => Ok(Some(name)),
                        None => Ok(None),
                    },
                    __EVAL_ENUM_VALUE__: converted?,
                });
            }
            Ok(__EVAL_RUNTIME_AGGREGATE__(__AGGREGATE_ENUM__(type_name, variant, output)))
        }
        (Node::Closure { function: expected, captures, .. }, V::Closure(closure)) => {
            if closure.function != *expected || closure.captures.len() != captures.len() {
                return Err("checked MIR closure identity/capture layout differs".to_string());
            }
            let mut output = Vec::with_capacity(captures.len());
            for (index, (value, child)) in closure.captures.into_iter().zip(captures).enumerate() {
                let ty = child_type(*child)?;
                field_path.push(format!("capture[{index}]"));
                projection_path.push(Path::Capture(index));
                let converted = __jet_bootstrap_entry_runtime_value_from_mir(
                    value, &ty, shape, *child, field_path, projection_path, physical, depth + 1,
                );
                field_path.pop();
                projection_path.pop();
                output.push(converted?);
            }
            Ok(__EVAL_CLOSURE__(closure.function, output))
        }
        (Node::Scalar(_), value) => {
            Ok(__EVAL_DATA__(__jet_bootstrap_ct_from_host(&value)?))
        }
        (_, value) => physical.decode_payload(
            field_path,
            projection_path,
            checked,
            shape,
            shape_node,
            value,
        ),
    }
}
"#,
    );
    for (token, replacement) in replacements {
        generated = generated.replace(token, &replacement);
    }
    out.push_str(&generated);
    Ok(())
}
fn emit_entry_conversion_helpers(out: &mut String) -> Result<(), BootstrapHostCodecError> {
    out.push_str(
        r#"
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
    ::jet_jit::SourceSharedInteropGuard
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
    ) -> crate::JetSharedPhysicalOperationOutcome<()> {
        let Some(guard) = self.guard.as_mut() else {
            return crate::JetSharedPhysicalOperationOutcome::new(
                Err("Shared guard was already released".to_string()),
                None,
            );
        };
        match (self.read)(guard) {
            Ok(current) if current == value => (self.suspend)(guard),
            Ok(_) => match (self.stage)(guard, value) {
                Ok(()) => (self.suspend)(guard),
                Err(error) => {
                    crate::JetSharedPhysicalOperationOutcome::new(Err(error), None)
                }
            },
            Err(error) => crate::JetSharedPhysicalOperationOutcome::new(Err(error), None),
        }
    }

    fn wait_resume(
        &mut self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> crate::JetSharedPhysicalOperationOutcome<
        Option<::jet_foundation::MIR::MirRuntimeValue>,
    > {
        let Some(guard) = self.guard.as_mut() else {
            return crate::JetSharedPhysicalOperationOutcome::new(
                Err("Shared guard was already released".to_string()),
                None,
            );
        };
        match (self.resume)(guard, cancelled) {
            Ok(true) => crate::JetSharedPhysicalOperationOutcome::new(
                (self.read)(guard).map(Some),
                None,
            ),
            Ok(false) => crate::JetSharedPhysicalOperationOutcome::new(Ok(None), None),
            Err(error) => crate::JetSharedPhysicalOperationOutcome::new(Err(error), None),
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

    fn wait_abort(&mut self) -> crate::JetSharedPhysicalOperationOutcome<()> {
        match self.guard.as_mut() {
            Some(guard) => (self.abort)(guard),
            None => crate::JetSharedPhysicalOperationOutcome::new(Ok(()), None),
        }
    }

    fn release(&mut self) -> crate::JetSharedPhysicalOperationOutcome<()> {
        match self.guard.take() {
            Some(guard) => (self.release)(guard),
            None => crate::JetSharedPhysicalOperationOutcome::new(Ok(()), None),
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

fn __jet_bootstrap_entry_child_type(
    ty: &::jet_foundation::MIR::MirType,
    kind: &str,
    index: usize,
) -> Result<&::jet_foundation::MIR::MirType, String> {
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
fn __jet_bootstrap_entry_checked_root_type(
    program: &::jet_foundation::MIR::MirProgram,
    type_id: ::jet_foundation::MIR::MirTypeId,
    expected_name: &str,
) -> Result<::jet_foundation::MIR::MirType, String> {
    let definition = program.types.iter().find(|definition| definition.id == type_id)
        .ok_or_else(|| format!("checked helper root type ID {} is absent", type_id.0))?;
    if definition.name != expected_name {
        return Err(format!("checked helper root ID {} names `{}` instead of `{expected_name}`", type_id.0, definition.name));
    }
    // A nongeneric nominal root needs no registered instance: lowering only
    // registers the instances its own uses demand, so the exact `Apply` is
    // rebuilt from the definition (identity = the definition ID) when absent.
    if !definition.generic_params.is_empty() {
        return Err(format!("checked helper root type `{expected_name}` is generic"));
    }
    Ok(program.type_instances.iter().find(|ty| {
        matches!(ty.kind(), ::jet_foundation::MIR::MirTypeKind::Apply { name, args }
            if name.id == type_id && name.name.as_str() == expected_name && args.is_empty())
    }).cloned().unwrap_or_else(|| {
        ::jet_foundation::MIR::MirType::from_kind(::jet_foundation::MIR::MirTypeKind::Apply {
            name: ::jet_foundation::MIR::MirNominalRef { id: type_id, name: expected_name.to_string() },
            args: Vec::new(),
        })
        .with_identity(type_id)
    }))
}

fn __jet_bootstrap_entry_checked_root_type_matches(
    checked: &::jet_foundation::MIR::MirType,
    expected: &::jet_foundation::MIR::MirType,
    type_id: ::jet_foundation::MIR::MirTypeId,
    expected_name: &str,
) -> bool {
    matches!(checked.kind(), ::jet_foundation::MIR::MirTypeKind::Apply { name, args }
        if name.id == type_id && name.name.as_str() == expected_name && args.is_empty())
        && checked.nominal_id() == Some(type_id)
        && checked.same_checked_type(expected)
}
"#,
    );
    Ok(())
}

/// The exact checked type of a nongeneric nominal helper root: its registered
/// `Apply` instance, or, when lowering registered none (it registers only the
/// instances the program's own uses demand), the same `Apply` rebuilt from the
/// definition with the definition ID as identity. The emitted runtime twin is
/// `__jet_bootstrap_entry_checked_root_type`.
fn helper_root_type(program: &MirProgram, definition: &MirTypeDef) -> Result<MirType, BootstrapHostCodecError> {
    if !definition.generic_params.is_empty() {
        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "checked helper root `{}` is generic",
            definition.name
        )));
    }
    Ok(program
        .type_instances
        .iter()
        .find(|ty| {
            matches!(ty.kind(), MirTypeKind::Apply { name, args }
                if name.id == definition.id && name.name == definition.name && args.is_empty())
        })
        .cloned()
        .unwrap_or_else(|| {
            MirType::from_kind(MirTypeKind::Apply {
                name: MirNominalRef { id: definition.id, name: definition.name.clone() },
                args: Vec::new(),
            })
            .with_identity(definition.id)
        }))
}

fn emit_entry_helper_root_transports(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    emit_entry_type_only_physical_bindings(out, symbols)?;
    for (root_name, stem) in [
        ("JetEvalTaskRoot", "jet_eval_task_root"),
        ("JetEvalOwnedRoot", "jet_eval_owned_root"),
        ("JetEvalRuntimeValue", "jet_eval_runtime_value"),
        ("JetEvalCallbackResult", "jet_eval_callback_result"),
        (
            "JetEvalTaskCallbackInvokeResult",
            "jet_eval_task_callback_invoke_result",
        ),
        (
            "JetEvalSharedHostCarrier",
            "jet_eval_shared_host_carrier",
        ),
        ("JetEvalSharedPayloadFinalizer", "jet_eval_shared_payload_finalizer"),
        ("JetEvalSharedValueData", "jet_eval_shared_value_data"),
        ("MIRProgram", "mir_program"),
        ("MIRType", "mir_type"),
        ("JetEvalHostTypeShape", "jet_eval_host_type_shape"),
        ("JetEvalConfig", "jet_eval_config"),
        ("Span", "span"),
        (
            "JetEvalOwnedRootDropResult",
            "jet_eval_owned_root_drop_result",
        ),
    ] {
        let definition = program.types.iter().find(|definition| definition.name == root_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingType(root_name.to_string()))?;
        if !definition.generic_params.is_empty()
            || !matches!(&definition.kind, MirTypeDefKind::Struct { .. } | MirTypeDefKind::Enum { .. })
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked transport type `{root_name}` is not a nongeneric aggregate"
            )));
        }
        let type_instance = helper_root_type(program, definition)?;
        if type_instance.nominal_id() != Some(definition.id) {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked helper root `{root_name}` lost its nominal type identity"
            )));
        }
        let struct_fields = match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => Some(fields),
            MirTypeDefKind::Enum { .. } => None,
            _ => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "checked transport type `{root_name}` is not a record or enum"
                )));
            }
        };
        if let Some(fields) = struct_fields {
            for field in fields {
                let row = symbols.field_binding(root_name, &field.name)?;
                if row.field != field.id
                    || row.owner != definition.id
                    || !row.ty.same_checked_type(&field.ty)
                {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked transport field binding drift for `{root_name}.{}`",
                        field.name
                    )));
                }
            }
        }
        if root_name == "JetEvalTaskCallbackInvokeResult" {
            let fields = struct_fields.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
                "checked JetEvalTaskCallbackInvokeResult is not a struct".to_string(),
            ))?;
            let expected = [
                ("root", "JetEvalTaskRoot"),
                ("result", "JetEvalCallbackResult"),
                ("cleanup_root", "JetEvalOwnedRoot"),
            ];
            if fields.len() != expected.len() + 1 {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalTaskCallbackInvokeResult field set changed".to_string(),
                ));
            }
            for (field_name, type_name) in expected {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                let field_type = checked_nominal_definition(program, &field.ty)?;
                if field_type.name != type_name {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalTaskCallbackInvokeResult.{field_name} is not `{type_name}`"
                    )));
                }
            }
            let reusable = fields.iter().find(|field| field.name == "reusable")
                .ok_or_else(|| BootstrapHostCodecError::MissingField {
                    owner: root_name.to_string(),
                    field: "reusable".to_string(),
                })?;
            if !matches!(reusable.ty.kind(), MirTypeKind::Bool) {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalTaskCallbackInvokeResult.reusable is not Bool".to_string(),
                ));
            }
        }
        if root_name == "JetEvalSharedHostCarrier" {
            let fields = struct_fields.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
                "checked JetEvalSharedHostCarrier is not a struct".to_string(),
            ))?;
            let expected = [
                ("kind", "JetEvalSharedHostKind"),
                ("registry", "JetEvalSharedRuntime"),
                ("origin", "JetEvalSharedHostOrigin"),
                ("physical", "JetEvalSharedHostPhysicalRoot"),
                ("payload_shape", "JetEvalHostTypeShape"),
            ];
            if fields.len() != expected.len() + 5 {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalSharedHostCarrier field set changed".to_string(),
                ));
            }
            for (field_name, type_name) in expected {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                let field_type = checked_nominal_definition(program, &field.ty)?;
                if field_type.name != type_name {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalSharedHostCarrier.{field_name} is not `{type_name}`"
                    )));
                }
            }
            for field_name in ["index", "owner_alias_token"] {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                if !matches!(field.ty.kind(), MirTypeKind::Option(inner) if matches!(inner.kind(), MirTypeKind::Int)) {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalSharedHostCarrier.{field_name} is not an optional Int"
                    )));
                }
            }
            for field_name in ["owner_alias_token_pre_adopted", "owns_root_capability"] {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                if !matches!(field.ty.kind(), MirTypeKind::Bool) {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalSharedHostCarrier.{field_name} is not Bool"
                    )));
                }
            }
            let finalizer = fields.iter().find(|field| field.name == "payload_finalizer")
                .ok_or_else(|| BootstrapHostCodecError::MissingField {
                    owner: root_name.to_string(),
                    field: "payload_finalizer".to_string(),
                })?;
            let MirTypeKind::Option(finalizer_type) = finalizer.ty.kind() else {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked Shared payload finalizer is not optional".to_string(),
                ));
            };
            if checked_nominal_definition(program, finalizer_type)?.name != "JetEvalSharedPayloadFinalizer" {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked Shared payload finalizer lost its Source ticket type".to_string(),
                ));
            }
            let physical = fields.iter().find(|field| field.name == "physical")
                .ok_or_else(|| BootstrapHostCodecError::MissingField {
                    owner: root_name.to_string(),
                    field: "physical".to_string(),
                })?;
            let physical_definition = checked_nominal_definition(program, &physical.ty)?;
            let MirTypeDefKind::Enum { variants, .. } = &physical_definition.kind else {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalSharedHostPhysicalRoot is not an enum".to_string(),
                ));
            };
            let expected_variants = [
                ("Shared", "JetEvalSharedValue"),
                ("Snapshot", "JetEvalSharedSnapshot"),
                ("Condition", "JetEvalSharedCondition"),
                ("Weak", "JetEvalSharedWeak"),
                ("Guard", "JetEvalSharedGuard"),
            ];
            if variants.len() != expected_variants.len() {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalSharedHostPhysicalRoot variant set changed".to_string(),
                ));
            }
            for (variant_name, type_name) in expected_variants {
                let variant = variants.iter().find(|variant| variant.name == variant_name)
                    .ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalSharedHostPhysicalRoot is missing `{variant_name}`"
                    )))?;
                let payload_type = match &variant.payload {
                    MirVariantPayload::Single(ty) => Some(ty),
                    MirVariantPayload::Named(fields) if fields.len() == 1 => Some(&fields[0].ty),
                    MirVariantPayload::Unit | MirVariantPayload::Named(_) => None,
                }.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(format!(
                    "checked JetEvalSharedHostPhysicalRoot::{variant_name} must carry one physical root"
                )))?;
                let payload_definition = checked_nominal_definition(program, payload_type)?;
                if payload_definition.name != type_name {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked JetEvalSharedHostPhysicalRoot::{variant_name} is not `{type_name}`"
                    )));
                }
            }
        }
        if root_name == "JetEvalOwnedRootDropResult" {
            let fields = struct_fields.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
                "checked JetEvalOwnedRootDropResult is not a struct".to_string(),
            ))?;
            if fields.len() != 5 {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked JetEvalOwnedRootDropResult field set changed".to_string(),
                ));
            }
            let disposition = fields.iter().find(|field| field.name == "disposition")
                .ok_or_else(|| BootstrapHostCodecError::MissingField {
                    owner: root_name.to_string(),
                    field: "disposition".to_string(),
                })?;
            let disposition_definition = checked_nominal_definition(program, &disposition.ty)?;
            if disposition_definition.name != "JetEvalOwnedRootDropDisposition" {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "checked drop result disposition type changed".to_string(),
                ));
            }
            match &disposition_definition.kind {
                MirTypeDefKind::UnitFamily { members }
                    if members.len() == 2
                        && members.iter().any(|member| member == "Cleaned")
                        && members.iter().any(|member| member == "ConsumedFailure") => {}
                MirTypeDefKind::Enum { variants, .. }
                    if variants.len() == 2
                        && variants.iter().any(|variant| variant.name == "Cleaned" && matches!(variant.payload, MirVariantPayload::Unit))
                        && variants.iter().any(|variant| variant.name == "ConsumedFailure" && matches!(variant.payload, MirVariantPayload::Unit)) => {}
                _ => {
                    return Err(BootstrapHostCodecError::InvalidMetadata(
                        "checked drop result disposition is not exactly Cleaned/ConsumedFailure".to_string(),
                    ));
                }
            }
            for (field_name, type_name) in [
                ("failure", "JetEvalError"),
                ("internal_problem", "JetEvalInternalProblem"),
            ] {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                let MirTypeKind::Option(inner) = field.ty.kind() else {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked drop result `{field_name}` is not optional"
                    )));
                };
                let inner_definition = checked_nominal_definition(program, inner)?;
                if inner_definition.name != type_name {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked drop result `{field_name}` is not `?{type_name}`"
                    )));
                }
            }
            for field_name in ["stdout", "stderr"] {
                let field = fields.iter().find(|field| field.name == field_name)
                    .ok_or_else(|| BootstrapHostCodecError::MissingField {
                        owner: root_name.to_string(),
                        field: field_name.to_string(),
                    })?;
                if !matches!(field.ty.kind(), MirTypeKind::String) {
                    return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                        "checked drop result `{field_name}` is not String"
                    )));
                }
            }
        }
        let source_type = symbols.type_symbol(root_name)?;
        writeln!(
            out,
            r#"
#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_{stem}_to_runtime<P: crate::BootstrapEntryPhysicalBindings>(
    source: &{source_type},
    program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    physical: &P,
) -> Result<
    (::jet_foundation::MIR::MirType, ::jet_foundation::MIR::MirRuntimeValue),
    String,
> {{
    let checked = __jet_bootstrap_entry_checked_root_type(
        program.as_ref(),
        ::jet_foundation::MIR::MirTypeId({type_id}),
        {root_name:?},
    )?;
    let path = vec![{root_name:?}.to_string()];
    let value = __jet_bootstrap_entry_type_{type_id}_to_runtime(
        source, &checked, program, &path, physical,
    )?;
    crate::compiler_bootstrap_entry_codec::validate_runtime_value(
        program.as_ref(), &checked, &value, 0,
    )?;
    Ok((checked, value))
}}

#[doc(hidden)]
pub(crate) fn __jet_bootstrap_entry_{stem}_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(
    checked_type: &::jet_foundation::MIR::MirType,
    runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
    physical: &P,
) -> Result<
    {source_type},
    crate::compiler_bootstrap_entry_codec::BootstrapEntryRootDecodeError,
> {{
    let checked = __jet_bootstrap_entry_checked_root_type(
        program.as_ref(),
        ::jet_foundation::MIR::MirTypeId({type_id}),
        {root_name:?},
    ).map_err(|error| crate::compiler_bootstrap_entry_codec::BootstrapEntryRootDecodeError {{
        error,
        value: runtime_value.clone(),
    }})?;
    if !__jet_bootstrap_entry_checked_root_type_matches(
        checked_type,
        &checked,
        ::jet_foundation::MIR::MirTypeId({type_id}),
        {root_name:?},
    ) {{
        return Err(crate::compiler_bootstrap_entry_codec::BootstrapEntryRootDecodeError {{
            error: format!("helper root carrier type is not checked `{root_name}`"),
            value: runtime_value,
        }});
    }}
    if let Err(error) = crate::compiler_bootstrap_entry_codec::validate_runtime_value(
        program.as_ref(), &checked, &runtime_value, 0,
    ) {{
        return Err(crate::compiler_bootstrap_entry_codec::BootstrapEntryRootDecodeError {{
            error,
            value: runtime_value,
        }});
    }}
    let cleanup_packet = runtime_value.clone();
    let path = vec![{root_name:?}.to_string()];
    __jet_bootstrap_entry_type_{type_id}_from_runtime(
        runtime_value, &checked, program.as_ref(), &path, physical,
    ).map_err(|error| crate::compiler_bootstrap_entry_codec::BootstrapEntryRootDecodeError {{
        error,
        value: cleanup_packet,
    }})
}}
"#,
            type_id = definition.id.0,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    emit_entry_no_physical_data_helpers(out, program, symbols)?;
    emit_entry_root_config_projection(
        out,
        program,
        symbols,
        "JetEvalTaskRoot",
        "jet_eval_task_root",
        &[
            ("JetEvalTaskRoot", "origin", "JetEvalOwnedRoot"),
            ("JetEvalOwnedRoot", "config", "JetEvalConfig"),
        ],
    )?;
    emit_entry_root_config_projection(
        out,
        program,
        symbols,
        "JetEvalOwnedRoot",
        "jet_eval_owned_root",
        &[("JetEvalOwnedRoot", "config", "JetEvalConfig")],
    )?;
    Ok(())
}
fn emit_entry_type_only_physical_bindings(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let host_value = symbols.type_symbol("JetEvalHostValue")?;
    let generated = r#"
#[derive(Clone, Copy)]
struct __JetBootstrapEntryTypeOnlyPhysicalBindings;

#[derive(Clone, Copy)]
struct __JetBootstrapEntryTypeOnlySharedMarshaller;

impl crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedValueMarshaller
    for __JetBootstrapEntryTypeOnlySharedMarshaller
{
    fn encode_payload<T: 'static>(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _source_value: &T,
    ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
        Err("type-only compiler-data codec cannot marshal a Shared payload".to_string())
    }

    fn decode_payload<T: 'static>(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<T, String> {
        Err("type-only compiler-data codec cannot reconstruct a Shared payload".to_string())
    }
}

impl<O, T>
    crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedMarshaller<
        O,
        T,
        __JetBootstrapEntryTypeOnlyPhysicalBindings,
    > for __JetBootstrapEntryTypeOnlySharedMarshaller
where
    O: crate::JetSharedPhysicalOwnerApi,
    T: 'static,
{
    fn encode_payload(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _source_value: &T,
    ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
        Err("type-only compiler-data codec cannot marshal a Shared payload".to_string())
    }

    fn decode_payload(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<T, String> {
        Err("type-only compiler-data codec cannot reconstruct a Shared payload".to_string())
    }
}

impl crate::BootstrapEntryPhysicalBindings for __JetBootstrapEntryTypeOnlyPhysicalBindings {
    type InterpreterValue = __HOST_VALUE_TYPE__;
    type PreparedInterpreterValue = ();
    type InterpreterTransferReceipt = ();
    type InterpreterTransferActivation = ();
    type InterpreterTransferBorrowEntry = ();
    type SharedMarshaller<O, T> = __JetBootstrapEntryTypeOnlySharedMarshaller
    where
        O: crate::JetSharedPhysicalOwnerApi,
        T: 'static;

    fn shared_payload_shape(
        &self,
        _checked_type: &::jet_foundation::MIR::MirType,
    ) -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {
        Err("type-only compiler-data codec has no Source Shared payload shape".to_string())
    }

    fn shared_marshaller<O: crate::JetSharedPhysicalOwnerApi, T: 'static>(
        &self,
        _owner: O,
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _encode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadEncoder<T, Self>,
        _decode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadDecoder<T, Self>,
    ) -> Result<Self::SharedMarshaller<O, T>, String> {
        Err("type-only compiler-data codec cannot create a Shared marshaller".to_string())
    }
    fn encode<T: 'static>(
        &self,
        _field_path: &[String],
        _checked_type: &::jet_foundation::MIR::MirType,
        _program: &::jet_foundation::MIR::MirProgram,
        _source_value: &T,
    ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
        Err("type-only compiler-data codec cannot encode physical values".to_string())
    }

    fn encode_shared(
        &self,
        _field_path: &[String],
        _checked_type: &::jet_foundation::MIR::MirType,
        _program: &::jet_foundation::MIR::MirProgram,
        _physical_identity: usize,
        _root: ::jet_jit::SourceSharedInterop,
        _payload_shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
    ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
        Err("type-only compiler-data codec cannot encode physical Shared roots".to_string())
    }

    fn decode<T: 'static>(
        &self,
        _field_path: &[String],
        _checked_type: &::jet_foundation::MIR::MirType,
        _program: &::jet_foundation::MIR::MirProgram,
        _runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<Option<T>, String> {
        Err("type-only compiler-data codec cannot decode physical values".to_string())
    }

    fn encode_shaped<T: 'static>(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _source_value: &T,
    ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
        Err("type-only compiler-data codec cannot encode shaped physical values".to_string())
    }

    fn decode_shaped<T: 'static>(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<Option<T>, String> {
        Err("type-only compiler-data codec cannot decode shaped physical values".to_string())
    }

    fn prepare_interpreter(
        &self,
        _field_path: &[String],
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _checked_type: &::jet_foundation::MIR::MirType,
        _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        _shape_node: usize,
        _interpreter_value: &Self::InterpreterValue,
    ) -> Result<Option<Self::PreparedInterpreterValue>, String> {
        Err("type-only compiler-data codec cannot prepare physical interpreter values".to_string())
    }

    fn interpreter_transfer_commit_and_extract(
        &self,
        _prepared: Self::PreparedInterpreterValue,
        _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
        _receipt: &Self::InterpreterTransferReceipt,
    ) -> (
        ::jet_foundation::MIR::MirRuntimeValue,
        Option<Self::InterpreterTransferBorrowEntry>,
    ) {
        panic!("type-only compiler-data codec received a physical transfer")
    }

    fn activate_interpreter_transfer_borrows(
        &self,
        _receipt: &Self::InterpreterTransferReceipt,
        _entries: Vec<Self::InterpreterTransferBorrowEntry>,
    ) -> Option<Self::InterpreterTransferActivation> {
        None
    }

    fn project_interpreter(
        &self,
        _field_path: &[String],
        _checked_type: &::jet_foundation::MIR::MirType,
        _program: &::jet_foundation::MIR::MirProgram,
        _runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
    ) -> Result<Option<Self::InterpreterValue>, String> {
        Err("type-only compiler-data codec cannot project physical interpreter values".to_string())
    }
}
"#
    .replace("__HOST_VALUE_TYPE__", host_value);
    out.push_str(&generated);
    Ok(())
}

fn emit_entry_no_physical_data_helpers(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    for (root_name, stem) in [
        ("MIRProgram", "mir_program"),
        ("MIRType", "mir_type"),
        ("Span", "span"),
        ("JetEvalHostTypeShape", "jet_eval_host_type_shape"),
    ] {
        let _definition = program
            .types
            .iter()
            .find(|definition| definition.name == root_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingType(root_name.to_string()))?;
        let source_type = symbols.type_symbol(root_name)?;
        writeln!(
            out,
            "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_{stem}_to_runtime_data(\n\
                 source: &{source_type},\n\
                 program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
             ) -> Result<(::jet_foundation::MIR::MirType, ::jet_foundation::MIR::MirRuntimeValue), String> {{\n\
                 let physical = __JetBootstrapEntryTypeOnlyPhysicalBindings;\n\
                 __jet_bootstrap_entry_{stem}_to_runtime(source, program, &physical)\n\
             }}\n\
             #[doc(hidden)]\n\
             pub(crate) fn __jet_bootstrap_entry_{stem}_from_runtime_data(\n\
                 checked_type: &::jet_foundation::MIR::MirType,\n\
                 runtime_value: ::jet_foundation::MIR::MirRuntimeValue,\n\
                 program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
             ) -> Result<{source_type}, String> {{\n\
                 let physical = __JetBootstrapEntryTypeOnlyPhysicalBindings;\n\
                 __jet_bootstrap_entry_{stem}_from_runtime(checked_type, runtime_value, program, &physical)\n\
                     .map_err(|error| error.error)\n\
             }}\n",
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_host_type_shape_from_runtime(\n\
             checked_type: &::jet_foundation::MIR::MirType,\n\
             runtime_value: ::jet_foundation::MIR::MirRuntimeValue,\n\
             program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
         ) -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {{\n\
             let physical = __JetBootstrapEntryTypeOnlyPhysicalBindings;\n\
             let source: {shape} = __jet_bootstrap_entry_jet_eval_host_type_shape_from_runtime(\n\
                 checked_type, runtime_value, program, &physical,\n\
             ).map_err(|error| error.error)?;\n\
             __jet_bootstrap_entry_host_type_shape_from_source(&source)\n\
         }}\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}


fn emit_entry_root_config_projection(
    out: &mut String,
    program: &MirProgram,
    symbols: &BootstrapCodecSymbols<'_>,
    root_name: &str,
    root_stem: &str,
    fields: &[(&str, &str, &str)],
) -> Result<(), BootstrapHostCodecError> {
    let root = program
        .types
        .iter()
        .find(|definition| definition.name == root_name)
        .ok_or_else(|| BootstrapHostCodecError::MissingType(root_name.to_string()))?;
    let config = program
        .types
        .iter()
        .find(|definition| definition.name == "JetEvalConfig")
        .ok_or_else(|| BootstrapHostCodecError::MissingType("JetEvalConfig".to_string()))?;
    let config_source = symbols.type_symbol("JetEvalConfig")?;
    let mut current_type = root.id;
    let mut current_name = root_name;
    let mut statements = String::new();
    let mut value_expression = "runtime_value".to_string();
    let mut type_expression = "__root_type".to_string();
    let mut path = vec![root_name.to_string()];

    for (index, (owner_name, field_name, child_name)) in fields.iter().enumerate() {
        if *owner_name != current_name {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked `{root_name}` configuration projection is not a contiguous field path"
            )));
        }
        let owner = program
            .types
            .iter()
            .find(|definition| definition.name == *owner_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingType((*owner_name).to_string()))?;
        let MirTypeDefKind::Struct { fields: owner_fields, .. } = &owner.kind else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked config projection owner `{owner_name}` is not a record"
            )));
        };
        let field = owner_fields
            .iter()
            .find(|field| field.name == *field_name)
            .ok_or_else(|| BootstrapHostCodecError::MissingField {
                owner: (*owner_name).to_string(),
                field: (*field_name).to_string(),
            })?;
        let child = checked_nominal_definition(program, &field.ty)?;
        if child.name != *child_name {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked `{owner_name}.{field_name}` is not `{child_name}`"
            )));
        }
        let field_type = format!("__field_{index}_type");
        let field_value = format!("__field_{index}_value");
        writeln!(
            statements,
            "let {field_type} = __jet_bootstrap_entry_field_type(program.as_ref(), ::jet_foundation::MIR::MirTypeId({owner_id}), ::jet_foundation::MIR::MirFieldId({field_id}))?;\n\
             let __expected_{index}_type = __jet_bootstrap_entry_checked_root_type(program.as_ref(), ::jet_foundation::MIR::MirTypeId({child_id}), {child_name:?})?;\n\
             if !{field_type}.same_checked_type(&__expected_{index}_type) {{ return Err(\"checked config projection field type changed\".to_string()); }}\n\
             let {field_value} = crate::compiler_bootstrap_entry_codec::checked_record_field(program.as_ref(), &{type_expression}, {value_expression}, ::jet_foundation::MIR::MirFieldId({field_id}))?\n\
                 .ok_or_else(|| format!(\"checked config projection field `{owner_name}.{field_name}` is absent\"))?;",
            owner_id = owner.id.0,
            field_id = field.id.0,
            child_id = child.id.0,
            type_expression = type_expression,
            value_expression = value_expression,
            owner_name = owner_name,
            field_name = field_name,
            child_name = child_name,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
        current_type = child.id;
        current_name = child_name;
        type_expression = field_type;
        value_expression = field_value;
        path.push((*field_name).to_string());
    }
    if current_type != config.id {
        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "checked `{root_name}` config projection does not end at JetEvalConfig"
        )));
    }
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_entry_{root_stem}_config_from_runtime<P: crate::BootstrapEntryPhysicalBindings>(\n\
             checked_type: &::jet_foundation::MIR::MirType,\n\
             runtime_value: &::jet_foundation::MIR::MirRuntimeValue,\n\
             program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
             physical: &P,\n\
         ) -> Result<{config_source}, String> {{\n\
             let __root_type = __jet_bootstrap_entry_checked_root_type(program.as_ref(), ::jet_foundation::MIR::MirTypeId({root_id}), {root_name:?})?;\n\
             if !__jet_bootstrap_entry_checked_root_type_matches(checked_type, &__root_type, ::jet_foundation::MIR::MirTypeId({root_id}), {root_name:?}) {{\n\
                 return Err(\"callback root carrier has the wrong checked type\".to_string());\n\
             }}\n\
             {statements}\n\
             crate::compiler_bootstrap_entry_codec::validate_runtime_value(program.as_ref(), &{config_type}, {config_value}, 0)?;\n\
             let path = vec![{path}];\n\
             __jet_bootstrap_entry_type_{config_id}_from_runtime({config_value}.clone(), &{config_type}, program.as_ref(), &path, physical)\n\
         }}\n",
        root_id = root.id.0,
        root_name = root_name,
        statements = statements,
        config_type = type_expression,
        config_value = value_expression,
        config_id = config.id.0,
        path = path
            .iter()
            .map(|component| format!("{component:?}.to_string()"))
            .collect::<Vec<_>>()
            .join(", "),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
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
    fn core_owner_rows(&self) -> &[::jet_foundation::MIR::MirCoreOwner] {
        &self.__MACHINE_PROGRAM_FIELD__.__PROGRAM_CORE_OWNERS_FIELD__
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
            for row in crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows::core_owner_rows(&**machine)
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
            let __jet_core_owner_rows: Vec<::jet_foundation::MIR::MirCoreOwner> =
                {__DECODED_ROWS__};
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
                    let accessor = symbols.callable_symbol("jet_eval_owned_root_config")?;
                    format!("&{accessor}(value)")
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
                let bindings = fields
                    .iter()
                    .enumerate()
                    .map(|(index, (_, symbol, _))| {
                        symbol.map_or_else(|| format!("__payload_{index}"), |symbol| symbol.to_string())
                    })
                    .collect::<Vec<_>>();
                let pattern = if fields.is_empty() {
                    path.clone()
                } else if fields.iter().all(|(name, _, _)| name.is_some()) {
                    format!(
                        "{path} {{ {} }}",
                        fields
                            .iter()
                            .zip(&bindings)
                            .map(|((name, _, _), binding)| format!("{}: {binding}", name.unwrap()))
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
                    let value = to_runtime_expression(
                        program,
                        symbols,
                        field_ty,
                        &format!("{binding}"),
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
                initializers.push(format!("{symbol}: {decoded_name}"));
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
                let constructor = symbols.callable_symbol("jet_eval_owned_root_from_parts")?;
                format!("{constructor}({program_value}, {registry_value}, {config_value})")
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
                    payload_values.push(match symbol {
                        Some(symbol) if matches!(variant.payload, MirVariantPayload::Named(_)) => {
                            format!("{symbol}: {arg}_value")
                        }
                        _ => format!("{arg}_value"),
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
    let guard_type = "crate::jet_std::JetSharedGuard";
    Ok(format!(
        r#"{{
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
                let program = program.as_ref();
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
                {decoded}
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
                ::std::sync::Arc::from({path}.to_vec());
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
                        crate::JetSharedPhysicalOperationOutcome::new(result, completion)
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
                        crate::JetSharedPhysicalOperationOutcome::new(result, completion)
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
                                let read = move |guard: &{guard_type}<_>| {{
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
                                    guard: &mut {guard_type}<_>,
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
                                    move |guard: &mut {guard_type}<_>| guard.physical_wait_suspend();
                                let resume = move |
                                    guard: &mut {guard_type}<_>,
                                    cancelled: &mut dyn FnMut() -> bool,
                                | guard.physical_wait_resume(cancelled);
                                let held =
                                    move |guard: &{guard_type}<_>| guard.physical_permit_held();
                                let abort =
                                    move |guard: &mut {guard_type}<_>| guard.physical_wait_abort();
                                let root_ptr =
                                    move |guard: &{guard_type}<_>| guard.physical_root_ptr();
                                let mark_dirty =
                                    move |guard: &{guard_type}<_>| guard.physical_mark_dirty();
                                let revision =
                                    move |guard: &{guard_type}<_>| guard.physical_revision();
                                let release =
                                    move |guard: {guard_type}<_>| guard.release();
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
                        crate::JetSharedPhysicalOperationOutcome::new(result, completion)
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
                        crate::JetSharedPhysicalOperationOutcome::new(result, completion)
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
                                return crate::JetSharedPhysicalOperationOutcome::new(
                                    Err(error),
                                    None,
                                );
                            }}
                        }};
                        let operation = __jet_shared_replace_owner
                            .replace_if_revision(expected_revision, replacement);
                        let completion = operation.completion;
                        let result = match operation.result {{
                            Ok(Some(result)) => result,
                            Ok(None) => Err("Shared owner was dropped".to_string()),
                            Err(error) => Err(error),
                        }};
                        crate::JetSharedPhysicalOperationOutcome::new(result, completion)
                    }},
                ))
            }});
            let __jet_shared_root =
                __jet_shared_owner.source_interop_root(__jet_shared_type_id, __jet_shared_build)?;
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
            let key_ty = "__jet_bootstrap_entry_child_type(checked, \"map\", 0)?";
            let value_ty = "__jet_bootstrap_entry_child_type(checked, \"map\", 1)?";
            let encoded_key = to_const_key_expression(program, symbols, key, "key", "__jet_key_type", program_expr, path)?;
            let encoded_value = to_runtime_expression(program, symbols, value, "item", "__jet_value_type", program_expr, path, physical)?;
            format!("{{ let __jet_key_type = {key_ty}; let __jet_value_type = {value_ty}; {mir}::Map(({source}).iter().map(|(key, item)| Ok(({encoded_key}, {encoded_value}))).collect::<Result<Vec<_>, String>>()?) }}")
        }
        Kind::Shared(inner) => shared_to_runtime_expression(
            program, symbols, inner, source, checked, program_expr, path, physical,
        )?,
        Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("{physical}.encode({path}, checked, {program_expr}.as_ref(), {source})?.ok_or_else(|| format!(\"NativeAdapter has no carrier for checked physical field `{{}}`\", {path}.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_ty = "__jet_bootstrap_entry_child_type(checked, \"option\", 0)?";
            let child = to_runtime_expression(program, symbols, inner, "inner", "&__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {inner_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({child})), Err(_) => {mir}::Absent {{ element: __jet_inner_type.clone() }} }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_ty = "__jet_bootstrap_entry_child_type(checked, \"result\", 0)?";
            let err_ty = "__jet_bootstrap_entry_child_type(checked, \"result\", 1)?";
            let ok_value = to_runtime_expression(program, symbols, ok, "inner", "&__jet_ok_type", program_expr, path, physical)?;
            let err_value = to_runtime_expression(program, symbols, err, "inner", "&__jet_err_type", program_expr, path, physical)?;
            format!("{{ let __jet_ok_type = {ok_ty}; let __jet_err_type = {err_ty}; match {source} {{ Ok(inner) => {mir}::Present(Box::new({ok_value})), Err(inner) => {mir}::FailedTold(Box::new({err_value})) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_ref(program, name)?;
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, checked, {program_expr}, {path}, {physical})?", name.id.0)
        }
        Kind::Tuple(fields) => {
            let mut items = Vec::with_capacity(fields.len());
            for (index, (name, field_ty)) in fields.iter().enumerate() {
                let ty_expr = format!("__jet_bootstrap_entry_child_type(checked, \"tuple\", {index})?");
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
            let child_ty = format!("__jet_bootstrap_entry_child_type(checked, \"{kind}\", 0)?");
            let child = to_runtime_expression(program, symbols, base, source, "__jet_inner_type", program_expr, path, physical)?;
            format!("{{ let __jet_inner_type = {child_ty}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_to_runtime({source}, checked, {program_expr}, {path}, {physical})?", id.0)
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
            let key_type = "__jet_bootstrap_entry_child_type(checked, \"map\", 0)?";
            let item_type = "__jet_bootstrap_entry_child_type(checked, \"map\", 1)?";
            let decoded_key = from_const_key_expression(program, symbols, key, "key", "&__jet_key_type", program_expr, path)?;
            let decoded_item = from_runtime_expression(program, symbols, item_ty, "item", "&__jet_item_type", program_expr, path)?;
            format!("{{ let __jet_key_type = {key_type}; let __jet_item_type = {item_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => entries.into_iter().map(|(key, item)| Ok(({decoded_key}, {decoded_item}))).collect::<Result<_, String>>()?, _ => return Err(\"checked Map result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Shared(_) | Kind::TraitObject(_) | Kind::Fn(_) | Kind::SendFn { .. } => {
            format!("physical.decode(path, checked, program, {value})?.ok_or_else(|| format!(\"NativeAdapter has no typed result projection for physical field `{{}}`\", path.join(\".\")))?")
        }
        Kind::Option(inner) => {
            let inner_type = "__jet_bootstrap_entry_child_type(checked, \"option\", 0)?";
            let child = from_runtime_expression(program, symbols, inner, "*inner", "&__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({child}), ::jet_foundation::MIR::MirRuntimeValue::Absent {{ element }} if element.same_checked_type(__jet_inner_type) => Err(Default::default()), _ => return Err(\"checked Option result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Result { ok, err } => {
            let ok_type = "__jet_bootstrap_entry_child_type(checked, \"result\", 0)?";
            let err_type = "__jet_bootstrap_entry_child_type(checked, \"result\", 1)?";
            let ok_value = from_runtime_expression(program, symbols, ok, "*inner", "&__jet_ok_type", program_expr, path)?;
            let err_value = from_runtime_expression(program, symbols, err, "*inner", "&__jet_err_type", program_expr, path)?;
            format!("{{ let __jet_ok_type = {ok_type}; let __jet_err_type = {err_type}; match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Present(inner) => Ok({ok_value}), ::jet_foundation::MIR::MirRuntimeValue::FailedTold(inner) => Err({err_value}), _ => return Err(\"checked Result result has invalid carrier\".to_string()) }} }}")
        }
        Kind::Apply { name, .. } => {
            checked_definition_by_ref(program, name)?;
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, checked, {program_expr}, {path}, physical)?", name.id.0)
        }
        Kind::Tuple(fields) => {
            if fields.is_empty() {
                format!("match {value} {{ ::jet_foundation::MIR::MirRuntimeValue::Unit => (), _ => return Err(\"checked unit result has invalid carrier\".to_string()) }}")
            } else {
                let mut decoded = Vec::with_capacity(fields.len());
                for (index, (_, field_ty)) in fields.iter().enumerate() {
                    let field_type = format!("__jet_bootstrap_entry_child_type(checked, \"tuple\", {index})?");
                    let child = from_runtime_expression(program, symbols, field_ty, "field_value", &format!("&__jet_tuple_type_{index}"), program_expr, path)?;
                    decoded.push(format!("{{ let __jet_tuple_type_{index} = {field_type}; let field_value = __jet_bootstrap_entry_take_tuple_field(&mut fields, {index})?; {child} }}"));
                }
                format!("{{ let ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields: mut fields }} = {value} else {{ return Err(\"checked tuple result has invalid carrier\".to_string()) }}; if type_name != \"Tuple\" {{ return Err(\"checked tuple result has wrong nominal name\".to_string()) }} ({}) }}", decoded.join(", "))
            }
        }
        Kind::InlineRange { base, .. } | Kind::Tagged { inner: base, .. } | Kind::Quantity { base, .. } => {
            let kind = match ty.kind() { Kind::InlineRange { .. } => "range", Kind::Tagged { .. } => "tagged", _ => "quantity" };
            let inner_type = format!("__jet_bootstrap_entry_child_type(checked, \"{kind}\", 0)?");
            let child = from_runtime_expression(program, symbols, base, value, "__jet_inner_type", program_expr, path)?;
            format!("{{ let __jet_inner_type = {inner_type}; {child} }}")
        }
        Kind::Union(members) => {
            let id = ty.identity.ok_or_else(|| BootstrapHostCodecError::InvalidMetadata("checked entry union has no nominal identity".to_string()))?;
            if members.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata("checked entry union has no members".to_string()));
            }
            format!("__jet_bootstrap_entry_type_{}_from_runtime({value}, checked, {program_expr}, {path}, physical)?", id.0)
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
