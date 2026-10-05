//! Resident-JIT marshalling for the canonical local `Cell` runtime.

use crate::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_codegen::local_cell::{JetCell, JetCellEditGuard, JetCellGetOrSet, JetCellReadGuard};
use jet_foundation::MIR::{MirRuntimeValue, MirType, MirTypeKind};
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::types_meta::JitMeta;

#[derive(Debug, Clone)]
pub(crate) enum CellSchema {
    Unit,
    Int,
    Float,
    Bool,
    Char,
    String,
    Option(Box<CellSchema>, MirType),
    List(Box<CellSchema>),
    Struct {
        name: String,
        fields: Vec<(String, CellSchema)>,
    },
    /// A language aggregate whose resident carrier is owned by the checked
    /// runtime descriptor registry.  `shape` remains available for Cell
    /// projections; it is never a second serialization implementation.
    Descriptor {
        type_id: u64,
        shape: Box<CellSchema>,
    },
    /// Evaluator-private opaque values (Shared, closures, and host handles).
    Handle,
}

impl CellSchema {
    fn struct_schema(
        name: &str,
        args: &[MirType],
        meta: &JitMeta<'_>,
    ) -> Result<Option<Self>, String> {
        let Some((field_names, field_types)) = meta.struct_layout(name) else {
            return Ok(None);
        };
        let params = meta.struct_type_params(name).unwrap_or_default();
        if params.len() != args.len() {
            return Err(format!(
                "jit Cell generic schema arity mismatch for {name}: {} parameters, {} arguments",
                params.len(),
                args.len()
            ));
        }
        let substitutions = params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect::<HashMap<_, _>>();
        Ok(Some(Self::Struct {
            name: name.to_string(),
            fields: field_names
                .iter()
                .zip(field_types)
                .map(|(field, ty)| {
                    let ty = crate::runtime_host::substitute_generic_type(ty, &substitutions);
                    Ok((field.clone(), Self::from_type(&ty, meta)?))
                })
                .collect::<Result<_, String>>()?,
        }))
    }

    fn uses_descriptor(ty: &MirType, meta: &JitMeta<'_>) -> bool {
        match ty.kind() {
            MirTypeKind::List(_)
            | MirTypeKind::FixedList { .. }
            | MirTypeKind::Map { .. }
            | MirTypeKind::Option(_)
            | MirTypeKind::Result { .. } => true,
            MirTypeKind::Apply { name, args } => {
                (name.name == "List" && args.len() == 1)
                    || (name.name == "Result" && args.len() == 2)
                    || meta.user_record_or_enum(&name.name)
            }
            _ => false,
        }
    }

    pub(crate) fn from_type(ty: &MirType, meta: &JitMeta<'_>) -> Result<Self, String> {
        let shape = match ty.kind() {
            MirTypeKind::Apply { name, args } if args.is_empty() && name.name == "Unit" => {
                Self::Unit
            }
            MirTypeKind::Int
            | MirTypeKind::IntN { .. }
            | MirTypeKind::InlineRange { .. } => Self::Int,
            MirTypeKind::Float | MirTypeKind::Float32 => Self::Float,
            MirTypeKind::Bool => Self::Bool,
            MirTypeKind::Char => Self::Char,
            MirTypeKind::String => Self::String,
            MirTypeKind::Option(inner) => Self::Option(
                Box::new(Self::from_type(inner, meta)?),
                (**inner).clone(),
            ),
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                Self::List(Box::new(Self::from_type(inner, meta)?))
            }
            MirTypeKind::Tuple(fields) => Self::Struct {
                name: "tuple".to_string(),
                fields: fields
                    .iter()
                    .map(|(name, ty)| Ok((name.clone(), Self::from_type(ty, meta)?)))
                    .collect::<Result<_, String>>()?,
            },
            MirTypeKind::Tagged { inner, .. } => Self::from_type(inner, meta)?,
            MirTypeKind::Apply { name, args } if args.is_empty() => {
                if let Some(base) = meta.distinct_base(&name.name) {
                    Self::from_type(base, meta)?
                } else {
                    Self::struct_schema(&name.name, &[], meta)?.unwrap_or(Self::Handle)
                }
            }
            MirTypeKind::Apply { name, args } => {
                Self::struct_schema(&name.name, args, meta)?.unwrap_or(Self::Handle)
            }
            MirTypeKind::Map { .. }
            | MirTypeKind::Result { .. }
            | MirTypeKind::Shared(_)
            | MirTypeKind::Fn(_)
            | MirTypeKind::SendFn { .. }
            | MirTypeKind::TraitObject(_)
            | MirTypeKind::Union(_)
            | MirTypeKind::Quantity { .. } => Self::Handle,
            MirTypeKind::Measure(_) => {
                return Err("a type measure is not a JIT cell type".to_string())
            }
        };
        if Self::uses_descriptor(ty, meta) {
            let type_id = crate::runtime_host::runtime_type_id(ty).ok_or_else(|| {
                format!(
                    "jit Cell type `{}` has no runtime descriptor",
                    ty.display_name()
                )
            })?;
            Ok(Self::Descriptor {
                type_id,
                shape: Box::new(shape),
            })
        } else {
            Ok(shape)
        }
    }
}

#[derive(Clone)]
pub(crate) struct CellProjection {
    pub paths: Vec<Vec<String>>,
    pub editable: bool,
}

#[derive(Clone)]
pub(crate) enum CellGuardLayout {
    Read,
    Edit,
    Option(Box<CellGuardLayout>),
    Result(Box<CellGuardLayout>),
    Record(Vec<Option<CellGuardLayout>>),
}

impl CellGuardLayout {
    pub(crate) fn from_type(ty: &MirType, meta: &JitMeta<'_>) -> Result<Option<Self>, String> {
        Ok(match ty.kind() {
            MirTypeKind::Apply { name, .. } if name.name == "CellReadGuard" => Some(Self::Read),
            MirTypeKind::Apply { name, .. } if name.name == "CellEditGuard" => Some(Self::Edit),
            MirTypeKind::Tagged { inner, .. } => Self::from_type(inner, meta)?,
            MirTypeKind::Option(inner) => Self::from_type(inner, meta)?
                .map(|layout| Self::Option(Box::new(layout))),
            MirTypeKind::Result { ok, .. } => Self::from_type(ok, meta)?
                .map(|layout| Self::Result(Box::new(layout))),
            MirTypeKind::Tuple(fields) => {
                let fields = fields
                    .iter()
                    .map(|(_, ty)| Self::from_type(ty, meta))
                    .collect::<Result<Vec<_>, _>>()?;
                fields
                    .iter()
                    .any(Option::is_some)
                    .then_some(Self::Record(fields))
            }
            MirTypeKind::Apply { name, .. } => {
                let identity = ty.identity;
                let identity_key = ty.identity_key();
                let fields = meta
                    .program
                    .type_instances
                    .iter()
                    .find(|instance| {
                        instance.identity == identity
                            || instance.identity_key() == identity_key
                            || instance.identity == Some(name.id)
                    })
                    .and_then(|instance| instance.tuple_fields());
                let Some(fields) = fields else {
                    return Ok(None);
                };
                let fields = fields
                    .iter()
                    .map(|(_, ty)| Self::from_type(ty, meta))
                    .collect::<Result<Vec<_>, _>>()?;
                fields
                    .iter()
                    .any(Option::is_some)
                    .then_some(Self::Record(fields))
            }
            _ => None,
        })
    }
}

struct GuardSlot<G> {
    guard: Option<G>,
    owner: u64,
    schema_handle: i64,
}

pub(crate) struct CellState {
    cells: Vec<JetCell<MirRuntimeValue>>,
    cell_schemas: Vec<i64>,
    read_guards: Vec<GuardSlot<JetCellReadGuard<MirRuntimeValue>>>,
    edit_guards: Vec<GuardSlot<JetCellEditGuard<MirRuntimeValue>>>,
    schemas: Vec<CellSchema>,
    projections: Vec<CellProjection>,
    guard_layouts: Vec<CellGuardLayout>,
    frames: Vec<u64>,
    next_frame: u64,
}

impl CellState {
    pub(crate) fn new() -> Self {
        Self {
            cells: Vec::new(),
            cell_schemas: Vec::new(),
            read_guards: Vec::new(),
            edit_guards: Vec::new(),
            schemas: Vec::new(),
            projections: Vec::new(),
            guard_layouts: Vec::new(),
            frames: Vec::new(),
            next_frame: 1,
        }
    }

    pub(crate) fn register_schema(&mut self, schema: CellSchema) -> i64 {
        self.schemas.push(schema);
        self.schemas.len() as i64
    }

    pub(crate) fn register_projection(&mut self, projection: CellProjection) -> i64 {
        self.projections.push(projection);
        self.projections.len() as i64
    }

    pub(crate) fn register_guard_layout(&mut self, layout: CellGuardLayout) -> i64 {
        self.guard_layouts.push(layout);
        self.guard_layouts.len() as i64
    }

    /// True when compile baked schema/projection/layout handles into machine code.
    /// Tier-cache restore starts from a fresh `CellState`, so those programs must
    /// not publish a disk artifact (handles would dangle).
    pub(crate) fn has_compile_handles(&self) -> bool {
        !self.schemas.is_empty() || !self.projections.is_empty() || !self.guard_layouts.is_empty()
    }

    fn owner(&self) -> u64 {
        self.frames.last().copied().unwrap_or(0)
    }

    fn insert_read(
        &mut self,
        guard: JetCellReadGuard<MirRuntimeValue>,
        owner: u64,
        schema_handle: i64,
    ) -> i64 {
        self.read_guards.push(GuardSlot {
            guard: Some(guard),
            owner,
            schema_handle,
        });
        self.read_guards.len() as i64
    }

    fn insert_edit(
        &mut self,
        guard: JetCellEditGuard<MirRuntimeValue>,
        owner: u64,
        schema_handle: i64,
    ) -> i64 {
        self.edit_guards.push(GuardSlot {
            guard: Some(guard),
            owner,
            schema_handle,
        });
        self.edit_guards.len() as i64
    }

    fn leave_frame(&mut self, returned_read: &HashSet<i64>, returned_edit: &HashSet<i64>) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let parent = self.owner();
        for (index, slot) in self.read_guards.iter_mut().enumerate() {
            if slot.owner == frame {
                if returned_read.contains(&(index as i64 + 1)) {
                    slot.owner = parent;
                } else {
                    slot.guard = None;
                }
            }
        }
        for (index, slot) in self.edit_guards.iter_mut().enumerate() {
            if slot.owner == frame {
                if returned_edit.contains(&(index as i64 + 1)) {
                    slot.owner = parent;
                } else {
                    slot.guard = None;
                }
            }
        }
    }
}

fn collect_returned_guards(
    rt: &crate::JitRuntime,
    raw: i64,
    layout: &CellGuardLayout,
    read: &mut HashSet<i64>,
    edit: &mut HashSet<i64>,
) -> Option<()> {
    match layout {
        CellGuardLayout::Read => {
            read.insert(raw);
        }
        CellGuardLayout::Edit => {
            edit.insert(raw);
        }
        CellGuardLayout::Option(inner) | CellGuardLayout::Result(inner) => {
            let (present, payload) = crate::runtime_host::jit_result_parts(rt, raw)?;
            if present {
                collect_returned_guards(rt, payload as i64, inner, read, edit)?;
            }
        }
        CellGuardLayout::Record(fields) => {
            for (index, field) in fields.iter().enumerate() {
                if let Some(field) = field {
                    let value = rt.heap.record_get_int(raw, index as i64)?;
                    collect_returned_guards(rt, value, field, read, edit)?;
                }
            }
        }
    }
    Some(())
}

fn schema(rt: &crate::JitRuntime, handle: i64) -> Option<CellSchema> {
    rt.cells
        .schemas
        .get((handle as usize).checked_sub(1)?)
        .cloned()
}
fn cell_schema_handle(rt: &crate::JitRuntime, cell: i64) -> Option<i64> {
    rt.cells
        .cell_schemas
        .get((cell as usize).checked_sub(1)?)
        .copied()
}
fn guard_index(handle: i64) -> Option<usize> {
    usize::try_from(handle).ok()?.checked_sub(1)
}

fn projection_index(handle: i64) -> Option<usize> {
    usize::try_from(handle.checked_abs()?).ok()?.checked_sub(1)
}

fn projection_kind(handle: i64) -> Option<i64> {
    (handle != 0).then_some(if handle < 0 { 2 } else { 1 })
}


fn project_schema(schema: &CellSchema, path: &[String]) -> Option<CellSchema> {
    let Some((field, rest)) = path.split_first() else {
        return Some(schema.clone());
    };
    let schema = match schema {
        CellSchema::Descriptor { shape, .. } => shape.as_ref(),
        schema => schema,
    };
    let CellSchema::Struct { fields, .. } = schema else {
        return None;
    };
    let field = fields.iter().find_map(|(name, schema)| {
        (name == field).then_some(schema)
    })?;
    project_schema(field, rest)
}

fn cell_callback_slot(callback: i64) -> Option<crate::runtime_host::JitCallableSlot> {
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::jit_callable_parts(rt, callback))
}

fn cell_callback_fault(message: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_host_fault(message);
        0
    })
}

fn optional_value_schema(schema: &CellSchema) -> Option<&CellSchema> {
    match schema {
        CellSchema::Option(inner, _) => Some(inner.as_ref()),
        CellSchema::Descriptor { shape, .. } => optional_value_schema(shape),
        _ => None,
    }
}


fn decode_value(rt: &mut crate::JitRuntime, raw: i64, schema: &CellSchema) -> Option<MirRuntimeValue> {
    Some(match schema {
        CellSchema::Unit => MirRuntimeValue::Unit,
        CellSchema::Int | CellSchema::Handle => MirRuntimeValue::Int(raw),
        CellSchema::Float => {
            MirRuntimeValue::Float {
                value: f64::from_bits(raw as u64),
                f32: false,
            }
        }
        CellSchema::Bool => MirRuntimeValue::Bool(raw != 0),
        CellSchema::Char => MirRuntimeValue::Char(char::from_u32(raw as u32)?),
        CellSchema::Descriptor { type_id, .. } => {
            crate::runtime_host::decode_jit_cell_value(rt, raw, *type_id).ok()?
        }
        CellSchema::String => MirRuntimeValue::String(rt.heap.clone_string(raw)?),
        CellSchema::Option(inner, inner_ty) => {
            let (present, payload) = crate::runtime_host::jit_result_parts(rt, raw)?;
            if present {
                MirRuntimeValue::Present(Box::new(decode_value(
                    rt,
                    payload as i64,
                    inner,
                )?))
            } else {
                MirRuntimeValue::Absent {
                    element: inner_ty.clone(),
                }
            }
        }
        CellSchema::List(inner) => {
            let len = rt.heap.list_len(raw)?;
            let mut values = Vec::with_capacity(len as usize);
            for index in 0..len {
                let item = match inner.as_ref() {
                    CellSchema::Float => rt.heap.list_get_float(raw, index)?.to_bits() as i64,
                    _ => rt.heap.list_get_int(raw, index)?,
                };
                values.push(decode_value(rt, item, inner)?);
            }
            MirRuntimeValue::List(values)
        }
        CellSchema::Struct { name, fields } => {
            let mut values = Vec::with_capacity(fields.len());
            for (index, (field, field_schema)) in fields.iter().enumerate() {
                let raw = match field_schema {
                    CellSchema::Float => {
                        rt.heap.record_get_float(raw, index as i64)?.to_bits() as i64
                    }
                    CellSchema::Bool => i64::from(rt.heap.record_get_bool(raw, index as i64)?),
                    CellSchema::Char => rt.heap.record_get_char(raw, index as i64)? as u32 as i64,
                    CellSchema::String => rt.heap.record_get_string(raw, index as i64)?,
                    _ => rt.heap.record_get_int(raw, index as i64)?,
                };
                values.push((field.clone(), decode_value(rt, raw, field_schema)?));
            }
            MirRuntimeValue::Struct {
                type_name: name.clone(),
                fields: values,
            }
        }
    })
}

fn encode_value(rt: &mut crate::JitRuntime, value: &MirRuntimeValue, schema: &CellSchema) -> Option<i64> {
    if !matches!(
        schema,
        CellSchema::Option(_, _) | CellSchema::Descriptor { .. }
    ) {
        if let MirRuntimeValue::Present(inner) = value {
            return encode_value(rt, inner, schema);
        }
    }
    match (schema, value) {
        (CellSchema::Unit, MirRuntimeValue::Unit) => Some(0),
        (CellSchema::Int | CellSchema::Handle, MirRuntimeValue::Int(value)) => Some(*value),
        (CellSchema::Float, MirRuntimeValue::Float { value, .. }) => Some(value.to_bits() as i64),
        (CellSchema::Bool, MirRuntimeValue::Bool(value)) => Some(i64::from(*value)),
        (CellSchema::Char, MirRuntimeValue::Char(value)) => Some(*value as u32 as i64),
        (CellSchema::String, MirRuntimeValue::String(value)) => Some(rt.heap.alloc_string(value.clone())),
        (CellSchema::Descriptor { type_id, .. }, value) => {
            crate::runtime_host::encode_jit_cell_value(rt, value, *type_id).ok()
        }
        (CellSchema::Option(_, _), MirRuntimeValue::Absent { .. }) => {
            Some(crate::runtime_host::alloc_jit_result(rt, false, 0))
        }
        (CellSchema::Option(inner, _), MirRuntimeValue::Present(value)) => {
            let payload = encode_value(rt, value, inner)?;
            Some(crate::runtime_host::alloc_jit_result(
                rt,
                true,
                payload as u64,
            ))
        }
        (CellSchema::Option(inner, _), value) => {
            let payload = encode_value(rt, value, inner)?;
            Some(crate::runtime_host::alloc_jit_result(
                rt,
                true,
                payload as u64,
            ))
        }
        (CellSchema::List(inner), MirRuntimeValue::List(values)) => {
            let list = rt.heap.alloc_empty_list();
            for value in values {
                let raw = encode_value(rt, value, inner)?;
                match inner.as_ref() {
                    CellSchema::Float => {
                        rt.heap.list_push_float(list, f64::from_bits(raw as u64))?;
                    }
                    _ => rt.heap.list_push_int(list, raw)?,
                }
            }
            Some(list)
        }
        (
            CellSchema::Struct {
                name: expected,
                fields: schemas,
            },
            MirRuntimeValue::Struct { type_name, fields },
        ) if expected == type_name || expected == "tuple" => {
            let record = rt.heap.alloc_record(schemas.len());
            for (index, (field, field_schema)) in schemas.iter().enumerate() {
                let value = fields
                    .iter()
                    .find_map(|(name, value)| (name == field).then_some(value))?;
                let raw = encode_value(rt, value, field_schema)?;
                match field_schema {
                    CellSchema::Float => {
                        rt.heap.record_set_float(
                            record,
                            index as i64,
                            f64::from_bits(raw as u64),
                        )?;
                    }
                    CellSchema::Bool => {
                        rt.heap.record_set_bool(record, index as i64, raw != 0)?;
                    }
                    CellSchema::Char => {
                        rt.heap.record_set_char(
                            record,
                            index as i64,
                            char::from_u32(raw as u32)?,
                        )?;
                    }
                    CellSchema::String => {
                        rt.heap.record_set_string(record, index as i64, raw)?;
                    }
                    _ => rt.heap.record_set_int(record, index as i64, raw)?,
                }
            }
            Some(record)
        }
        _ => None,
    }
}

fn with_cell_result(f: impl FnOnce(&mut crate::JitRuntime) -> i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match catch_unwind(AssertUnwindSafe(|| f(rt))) {
        Ok(value) => value,
        Err(error) => {
            let message = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    error
                        .downcast_ref::<&str>()
                        .map(|value| (*value).to_string())
                })
                .unwrap_or_else(|| "Cell runtime panic".to_string());
            rt.set_trap(&message);
            0
        }
    })
}

fn with_cell(f: impl FnOnce(&mut crate::JitRuntime)) {
    let _ = with_cell_result(|rt| {
        f(rt);
        0
    });
}

fn jet_jit_cell_frame_enter() {
    with_cell(|rt| {
        let frame = rt.cells.next_frame;
        rt.cells.next_frame += 1;
        rt.cells.frames.push(frame);
    });
}

fn jet_jit_cell_frame_leave(layout_handle: i64, returned: i64) {
    with_cell(|rt| {
        let mut read = HashSet::new();
        let mut edit = HashSet::new();
        if layout_handle != 0 {
            let layout = rt.cells.guard_layouts[(layout_handle - 1) as usize].clone();
            collect_returned_guards(rt, returned, &layout, &mut read, &mut edit)
                .expect("Cell guard return ABI");
        }
        rt.cells.leave_frame(&read, &edit);
    });
}

fn jet_jit_cell_new(raw: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let Some(value) = decode_value(rt, raw, &schema) else {
            rt.set_host_fault("Cell value descriptor ABI mismatch");
            return 0;
        };
        rt.cells.cells.push(JetCell::new(value));
        rt.cells.cell_schemas.push(schema_handle);
        rt.cells.cells.len() as i64
    })
}

fn cell_get_value(rt: &mut crate::JitRuntime, cell: i64, schema_handle: i64) -> i64 {
    let schema = schema(rt, schema_handle).expect("Cell schema");
    let value = rt.cells.cells[(cell - 1) as usize].get();
    let Some(raw) = encode_value(rt, &value, &schema) else {
        rt.set_host_fault("Cell value descriptor ABI mismatch");
        return 0;
    };
    raw
}

fn jet_jit_cell_get(cell: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| cell_get_value(rt, cell, schema_handle))
}

fn jet_jit_cell_get_host(cell: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = cell_schema_handle(rt, cell).expect("Cell schema");
        cell_get_value(rt, cell, schema_handle)
    })
}

fn cell_set_value(rt: &mut crate::JitRuntime, cell: i64, raw: i64, schema_handle: i64) {
    let schema = schema(rt, schema_handle).expect("Cell schema");
    let Some(value) = decode_value(rt, raw, &schema) else {
        rt.set_host_fault("Cell value descriptor ABI mismatch");
        return;
    };
    rt.cells.cells[(cell - 1) as usize].set(value);
}

fn jet_jit_cell_set(cell: i64, raw: i64, schema_handle: i64) {
    with_cell(|rt| cell_set_value(rt, cell, raw, schema_handle));
}

fn jet_jit_cell_set_host(cell: i64, raw: i64) {
    with_cell(|rt| {
        let schema_handle = cell_schema_handle(rt, cell).expect("Cell schema");
        cell_set_value(rt, cell, raw, schema_handle);
    });
}

fn cell_replace_value(
    rt: &mut crate::JitRuntime,
    cell: i64,
    raw: i64,
    schema_handle: i64,
) -> i64 {
    let schema = schema(rt, schema_handle).expect("Cell schema");
    let Some(value) = decode_value(rt, raw, &schema) else {
        rt.set_host_fault("Cell value descriptor ABI mismatch");
        return 0;
    };
    let old = rt.cells.cells[(cell - 1) as usize].replace(value);
    let Some(raw) = encode_value(rt, &old, &schema) else {
        rt.set_host_fault("Cell value descriptor ABI mismatch");
        return 0;
    };
    raw
}

fn jet_jit_cell_replace(cell: i64, raw: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| cell_replace_value(rt, cell, raw, schema_handle))
}

fn jet_jit_cell_replace_host(cell: i64, raw: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = cell_schema_handle(rt, cell).expect("Cell schema");
        cell_replace_value(rt, cell, raw, schema_handle)
    })
}

fn jet_jit_cell_guard_read(cell: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = cell_schema_handle(rt, cell).expect("Cell schema");
        let guard = rt.cells.cells[(cell - 1) as usize].guard_read();
        let owner = rt.cells.owner();
        rt.cells.insert_read(guard, owner, schema_handle)
    })
}

fn jet_jit_cell_guard_edit(cell: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = cell_schema_handle(rt, cell).expect("Cell schema");
        let guard = rt.cells.cells[(cell - 1) as usize].guard_edit();
        let owner = rt.cells.owner();
        rt.cells.insert_edit(guard, owner, schema_handle)
    })
}

fn cell_guard_get_value(
    rt: &mut crate::JitRuntime,
    kind: i64,
    guard: i64,
    schema_handle: i64,
) -> i64 {
    let schema = schema(rt, schema_handle).expect("Cell schema");
    let value = if kind == 1 {
        rt.cells.read_guards[(guard - 1) as usize]
            .guard
            .as_ref()
            .expect("live Cell read guard")
            .get()
    } else {
        rt.cells.edit_guards[(guard - 1) as usize]
            .guard
            .as_ref()
            .expect("live Cell edit guard")
            .get()
    };
    let Some(raw) = encode_value(rt, &value, &schema) else {
        rt.set_host_fault("Cell guard descriptor ABI mismatch");
        return 0;
    };
    raw
}

fn jet_jit_cell_guard_get(kind: i64, guard: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| cell_guard_get_value(rt, kind, guard, schema_handle))
}

fn jet_jit_cell_read_guard_get(guard: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = rt.cells.read_guards[(guard - 1) as usize].schema_handle;
        cell_guard_get_value(rt, 1, guard, schema_handle)
    })
}

fn jet_jit_cell_edit_guard_get(guard: i64) -> i64 {
    with_cell_result(|rt| {
        let schema_handle = rt.cells.edit_guards[(guard - 1) as usize].schema_handle;
        cell_guard_get_value(rt, 2, guard, schema_handle)
    })
}

fn cell_guard_set_value(
    rt: &mut crate::JitRuntime,
    guard: i64,
    raw: i64,
    schema_handle: i64,
) {
    let schema = schema(rt, schema_handle).expect("Cell schema");
    let Some(value) = decode_value(rt, raw, &schema) else {
        rt.set_host_fault("Cell guard descriptor ABI mismatch");
        return;
    };
    rt.cells.edit_guards[(guard - 1) as usize]
        .guard
        .as_ref()
        .expect("live Cell edit guard")
        .set(value);
}

fn jet_jit_cell_guard_set(guard: i64, raw: i64, schema_handle: i64) {
    with_cell(|rt| cell_guard_set_value(rt, guard, raw, schema_handle));
}

fn jet_jit_cell_edit_guard_set(guard: i64, raw: i64) {
    with_cell(|rt| {
        let schema_handle = rt.cells.edit_guards[(guard - 1) as usize].schema_handle;
        cell_guard_set_value(rt, guard, raw, schema_handle);
    });
}

fn jet_jit_cell_get_or_set_store(guard: i64, raw: i64, schema_handle: i64) {
    with_cell(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let Some(value_schema) = optional_value_schema(&schema) else {
            rt.set_host_fault("Cell.get_or_set requires an optional descriptor");
            return;
        };
        let Some(value) = decode_value(rt, raw, value_schema) else {
            rt.set_host_fault("Cell optional descriptor ABI mismatch");
            return;
        };
        rt.cells.edit_guards[(guard - 1) as usize]
            .guard
            .as_ref()
            .expect("live Cell optional edit guard")
            .store_option_value(value);
    });
}

fn jet_jit_cell_guard_drop(kind: i64, guard: i64) {
    with_cell(|rt| {
        if kind == 1 {
            rt.cells.read_guards[(guard - 1) as usize].guard = None;
        } else {
            rt.cells.edit_guards[(guard - 1) as usize].guard = None;
        }
    });
}

fn cell_guard_project_value(
    rt: &mut crate::JitRuntime,
    kind: i64,
    guard: i64,
    projection: CellProjection,
) -> i64 {
    let owner = rt.cells.owner();
    let source_schema_handle = if kind == 2 {
        rt.cells.edit_guards[(guard - 1) as usize].schema_handle
    } else {
        rt.cells.read_guards[(guard - 1) as usize].schema_handle
    };
    let source_schema = schema(rt, source_schema_handle).expect("Cell schema");
    let projected_schema_handles = projection
        .paths
        .iter()
        .map(|path| {
            let projected = project_schema(&source_schema, path).expect("Cell schema path");
            rt.cells.register_schema(projected)
        })
        .collect::<Vec<_>>();
    if kind == 2 {
        let guard = rt.cells.edit_guards[(guard - 1) as usize]
            .guard
            .take()
            .expect("live Cell edit guard");
        match (projection.paths.as_slice(), projected_schema_handles.as_slice()) {
            ([path], [schema_handle]) => {
                let guard = guard.map(|value| project_mut(value, path).expect("Cell path"));
                rt.cells.insert_edit(guard, owner, *schema_handle)
            }
            ([first, second], [first_schema, second_schema]) => {
                let (first, second) = guard.split(|value| {
                    project_pair_mut(value, first, second).expect("disjoint Cell paths")
                });
                let first = rt.cells.insert_edit(first, owner, *first_schema);
                let second = rt.cells.insert_edit(second, owner, *second_schema);
                let record = rt.heap.alloc_record(2);
                rt.heap
                    .record_set_int(record, 0, first)
                    .expect("Cell tuple");
                rt.heap
                    .record_set_int(record, 1, second)
                    .expect("Cell tuple");
                record
            }
            _ => jet_foundation::ice!(None, "Cell projection shape"),
        }
    } else {
        let guard = rt.cells.read_guards[(guard - 1) as usize]
            .guard
            .take()
            .expect("live Cell read guard");
        match (projection.paths.as_slice(), projected_schema_handles.as_slice()) {
            ([path], [schema_handle]) => {
                let guard = guard.map(|value| project_ref(value, path).expect("Cell path"));
                rt.cells.insert_read(guard, owner, *schema_handle)
            }
            ([first, second], [first_schema, second_schema]) => {
                let (first, second) = guard.split(|value| {
                    (
                        project_ref(value, first).expect("Cell path"),
                        project_ref(value, second).expect("Cell path"),
                    )
                });
                let first = rt.cells.insert_read(first, owner, *first_schema);
                let second = rt.cells.insert_read(second, owner, *second_schema);
                let record = rt.heap.alloc_record(2);
                rt.heap
                    .record_set_int(record, 0, first)
                    .expect("Cell tuple");
                rt.heap
                    .record_set_int(record, 1, second)
                    .expect("Cell tuple");
                record
            }
            _ => jet_foundation::ice!(None, "Cell projection shape"),
        }
    }
}

fn jet_jit_cell_guard_project(kind: i64, guard: i64, projection_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let projection = projection_index(projection_handle)
            .and_then(|index| rt.cells.projections.get(index))
            .cloned()
            .expect("Cell projection");
        cell_guard_project_value(rt, kind, guard, projection)
    })
}

fn jet_jit_cell_guard_map(guard: i64, projection_handle: i64) -> i64 {
    let kind = projection_kind(projection_handle).expect("Cell projection kind");
    with_cell_result(|rt| {
        let projection = projection_index(projection_handle)
            .and_then(|index| rt.cells.projections.get(index))
            .cloned()
            .expect("Cell projection");
        cell_guard_project_value(rt, kind, guard, projection)
    })
}

fn jet_jit_cell_guard_split(guard: i64, first_handle: i64, second_handle: i64) -> i64 {
    let kind = projection_kind(first_handle).expect("Cell projection kind");
    assert_eq!(
        projection_kind(second_handle),
        Some(kind),
        "Cell split projection kinds must match"
    );
    with_cell_result(|rt| {
        let first = projection_index(first_handle)
            .and_then(|index| rt.cells.projections.get(index))
            .cloned()
            .expect("Cell split projection");
        let second = projection_index(second_handle)
            .and_then(|index| rt.cells.projections.get(index))
            .cloned()
            .expect("Cell split projection");
        if first.paths.len() != 1 || second.paths.len() != 1 {
            jet_foundation::ice!(None, "Cell split projection shape");
        }
        let projection = CellProjection {
            paths: vec![first.paths[0].clone(), second.paths[0].clone()],
            editable: kind == 2,
        };
        cell_guard_project_value(rt, kind, guard, projection)
    })
}

fn cell_callback_stopped() -> bool {
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::runtime_stop_pending(rt))
}

fn jet_jit_cell_read(cell: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("Cell.read callback handle is invalid");
    };
    let Some((cell, schema)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = cell_schema_handle(rt, cell)?;
        let cell = rt
            .cells
            .cells
            .get((cell as usize).checked_sub(1)?)?
            .clone();
        Some((cell, schema(rt, schema_handle)?))
    }) else {
        return cell_callback_fault("Cell.read received an invalid cell handle");
    };
    let guard = cell.guard_read();
    let Some(raw) = Concurrency::with_runtime_mut(|rt| {
        guard.read(|value| encode_value(rt, value, &schema))
    }) else {
        drop(guard);
        return cell_callback_fault("Cell.read value ABI is invalid");
    };
    let Some(result) = crate::runtime_host::invoke_universal_unary(slot, raw) else {
        drop(guard);
        return cell_callback_fault("Cell.read callback has no unary universal thunk");
    };
    let stopped = cell_callback_stopped();
    drop(guard);
    if stopped {
        0
    } else {
        result
    }
}

fn jet_jit_cell_edit(cell: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("Cell.edit callback handle is invalid");
    };
    let Some((cell, schema)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = cell_schema_handle(rt, cell)?;
        let cell = rt
            .cells
            .cells
            .get((cell as usize).checked_sub(1)?)?
            .clone();
        Some((cell, schema(rt, schema_handle)?))
    }) else {
        return cell_callback_fault("Cell.edit received an invalid cell handle");
    };
    let guard = cell.guard_edit();
    let Some(mut raw) = Concurrency::with_runtime_mut(|rt| {
        guard.read(|value| encode_value(rt, value, &schema))
    }) else {
        drop(guard);
        return cell_callback_fault("Cell.edit value ABI is invalid");
    };
    let address = (&mut raw as *mut i64) as i64;
    let Some(result) = crate::runtime_host::invoke_universal_unary(slot, address) else {
        drop(guard);
        return cell_callback_fault("Cell.edit callback has no unary universal thunk");
    };
    if cell_callback_stopped() {
        drop(guard);
        return 0;
    }
    let Some(value) = Concurrency::with_runtime_mut(|rt| decode_value(rt, raw, &schema)) else {
        drop(guard);
        return cell_callback_fault("Cell.edit callback produced an invalid value");
    };
    guard.set(value);
    result
}

fn jet_jit_cell_read_guard_read(guard: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("CellReadGuard.read callback handle is invalid");
    };
    let Some(index) = guard_index(guard) else {
        return cell_callback_fault("CellReadGuard.read received an invalid guard");
    };
    let Some((guard, schema)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = rt.cells.read_guards.get(index)?.schema_handle;
        let schema = schema(rt, schema_handle)?;
        let guard = rt.cells.read_guards.get_mut(index)?.guard.take()?;
        Some((guard, schema))
    }) else {
        return cell_callback_fault("CellReadGuard.read received an invalid guard");
    };
    let Some(raw) = Concurrency::with_runtime_mut(|rt| {
        guard.read(|value| encode_value(rt, value, &schema))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            if let Some(slot) = rt.cells.read_guards.get_mut(index) {
                slot.guard = Some(guard);
            }
        });
        return cell_callback_fault("CellReadGuard.read value ABI is invalid");
    };
    let result = crate::runtime_host::invoke_universal_unary(slot, raw);
    let stopped = cell_callback_stopped();
    Concurrency::with_runtime_mut(|rt| {
        if let Some(slot) = rt.cells.read_guards.get_mut(index) {
            slot.guard = Some(guard);
        }
    });
    if stopped {
        0
    } else {
        result.unwrap_or_else(|| {
            cell_callback_fault("CellReadGuard.read callback has no unary universal thunk")
        })
    }
}

fn jet_jit_cell_edit_guard_read(guard: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("CellEditGuard.read callback handle is invalid");
    };
    let Some(index) = guard_index(guard) else {
        return cell_callback_fault("CellEditGuard.read received an invalid guard");
    };
    let Some((guard, schema)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = rt.cells.edit_guards.get(index)?.schema_handle;
        let schema = schema(rt, schema_handle)?;
        let guard = rt.cells.edit_guards.get_mut(index)?.guard.take()?;
        Some((guard, schema))
    }) else {
        return cell_callback_fault("CellEditGuard.read received an invalid guard");
    };
    let Some(raw) = Concurrency::with_runtime_mut(|rt| {
        guard.read(|value| encode_value(rt, value, &schema))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
                slot.guard = Some(guard);
            }
        });
        return cell_callback_fault("CellEditGuard.read value ABI is invalid");
    };
    let result = crate::runtime_host::invoke_universal_unary(slot, raw);
    let stopped = cell_callback_stopped();
    Concurrency::with_runtime_mut(|rt| {
        if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
            slot.guard = Some(guard);
        }
    });
    if stopped {
        0
    } else {
        result.unwrap_or_else(|| {
            cell_callback_fault("CellEditGuard.read callback has no unary universal thunk")
        })
    }
}

fn jet_jit_cell_edit_guard_edit(guard: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("CellEditGuard.edit callback handle is invalid");
    };
    let Some(index) = guard_index(guard) else {
        return cell_callback_fault("CellEditGuard.edit received an invalid guard");
    };
    let Some((guard, schema)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = rt.cells.edit_guards.get(index)?.schema_handle;
        let schema = schema(rt, schema_handle)?;
        let guard = rt.cells.edit_guards.get_mut(index)?.guard.take()?;
        Some((guard, schema))
    }) else {
        return cell_callback_fault("CellEditGuard.edit received an invalid guard");
    };
    let Some(mut raw) = Concurrency::with_runtime_mut(|rt| {
        guard.read(|value| encode_value(rt, value, &schema))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
                slot.guard = Some(guard);
            }
        });
        return cell_callback_fault("CellEditGuard.edit value ABI is invalid");
    };
    let address = (&mut raw as *mut i64) as i64;
    let result = crate::runtime_host::invoke_universal_unary(slot, address);
    if cell_callback_stopped() {
        Concurrency::with_runtime_mut(|rt| {
            if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
                slot.guard = Some(guard);
            }
        });
        return 0;
    }
    let Some(value) = Concurrency::with_runtime_mut(|rt| decode_value(rt, raw, &schema)) else {
        Concurrency::with_runtime_mut(|rt| {
            if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
                slot.guard = Some(guard);
            }
        });
        return cell_callback_fault("CellEditGuard.edit callback produced an invalid value");
    };
    guard.set(value);
    Concurrency::with_runtime_mut(|rt| {
        if let Some(slot) = rt.cells.edit_guards.get_mut(index) {
            slot.guard = Some(guard);
        }
    });
    result.unwrap_or_else(|| {
        cell_callback_fault("CellEditGuard.edit callback has no unary universal thunk")
    })
}

fn jet_jit_cell_get_or_set_begin(cell: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let Some(value_schema) = optional_value_schema(&schema) else {
            rt.set_host_fault("Cell.get_or_set requires an optional descriptor");
            return 0;
        };
        let cell = rt.cells.cells[(cell - 1) as usize].clone();
        let record = rt.heap.alloc_record(2);
        match cell.begin_get_or_set() {
            JetCellGetOrSet::Value(value) => {
                let Some(raw) = encode_value(rt, &value, value_schema) else {
                    rt.set_host_fault("Cell optional descriptor ABI mismatch");
                    return 0;
                };
                rt.heap.record_set_bool(record, 0, true).expect("Cell init");
                rt.heap.record_set_int(record, 1, raw).expect("Cell init");
            }
            JetCellGetOrSet::Empty(guard) => {
                let owner = rt.cells.owner();
                let guard = rt.cells.insert_edit(guard, owner, schema_handle);
                rt.heap
                    .record_set_bool(record, 0, false)
                    .expect("Cell init");
                rt.heap.record_set_int(record, 1, guard).expect("Cell init");
            }
        }
        record
    })
}

fn jet_jit_cell_get_or_set_callback(cell: i64, callback: i64) -> i64 {
    let Some(slot) = cell_callback_slot(callback) else {
        return cell_callback_fault("Cell.get_or_set callback handle is invalid");
    };
    let Some((value_schema, operation)) = Concurrency::with_runtime_mut(|rt| {
        let schema_handle = cell_schema_handle(rt, cell)?;
        let schema = schema(rt, schema_handle)?;
        let value_schema = optional_value_schema(&schema)?.clone();
        let cell = rt.cells.cells.get((cell as usize).checked_sub(1)?)?.clone();
        Some((value_schema, cell.begin_get_or_set()))
    }) else {
        return cell_callback_fault("Cell.get_or_set received an invalid Cell");
    };
    match operation {
        JetCellGetOrSet::Value(value) => {
            Concurrency::with_runtime_mut(|rt| encode_value(rt, &value, &value_schema))
                .unwrap_or_else(|| cell_callback_fault("Cell.get_or_set value ABI is invalid"))
        }
        JetCellGetOrSet::Empty(guard) => {
            let Some(raw) = crate::runtime_host::invoke_universal_many(slot, &[]) else {
                drop(guard);
                return cell_callback_fault(
                    "Cell.get_or_set callback has no zero-argument universal thunk",
                );
            };
            if cell_callback_stopped() {
                drop(guard);
                return 0;
            }
            let Some(value) =
                Concurrency::with_runtime_mut(|rt| decode_value(rt, raw, &value_schema))
            else {
                drop(guard);
                return cell_callback_fault("Cell.get_or_set callback produced an invalid value");
            };
            let Some(encoded) =
                Concurrency::with_runtime_mut(|rt| encode_value(rt, &value, &value_schema))
            else {
                drop(guard);
                return cell_callback_fault("Cell.get_or_set value ABI is invalid");
            };
            guard.store_option_value(value);
            encoded
        }
    }
}

host_fns! {
    struct CellHostFns;
    register: register_symbols;
    declare: declare_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let noarg = Signature::new(cc);
        let mut noarg_i64 = Signature::new(cc);
        noarg_i64.returns.push(AbiParam::new(types::I64));
        let mut unary = noarg_i64.clone();
        unary.params.push(AbiParam::new(types::I64));
        let mut binary = unary.clone();
        binary.params.push(AbiParam::new(types::I64));
        let mut ternary = binary.clone();
        ternary.params.push(AbiParam::new(types::I64));
        let mut binary_void = noarg.clone();
        binary_void.params.extend([AbiParam::new(types::I64); 2]);
        let mut ternary_void = noarg.clone();
        ternary_void.params.extend([AbiParam::new(types::I64); 3]);


    }
    frame_enter: "jet_jit_cell_frame_enter" => jet_jit_cell_frame_enter: noarg;
    frame_leave: "jet_jit_cell_frame_leave" => jet_jit_cell_frame_leave: binary_void;
    new: "jet_jit_cell_new" => jet_jit_cell_new: binary;
    new_prelude: "jet_std::JetCell::new" => jet_jit_cell_new: binary;
    get: "jet_jit_cell_get" => jet_jit_cell_get: binary;
    get_host: "jet_cell_get" => jet_jit_cell_get_host: unary;
    set: "jet_jit_cell_set" => jet_jit_cell_set: ternary_void;
    set_host: "jet_cell_set" => jet_jit_cell_set_host: binary_void;
    replace: "jet_jit_cell_replace" => jet_jit_cell_replace: ternary;
    replace_host: "jet_cell_replace" => jet_jit_cell_replace_host: binary;
    guard_read: "jet_jit_cell_guard_read" => jet_jit_cell_guard_read: unary;
    guard_read_host: "jet_cell_guard_read" => jet_jit_cell_guard_read: unary;
    guard_edit: "jet_jit_cell_guard_edit" => jet_jit_cell_guard_edit: unary;
    guard_edit_host: "jet_cell_guard_edit" => jet_jit_cell_guard_edit: unary;
    guard_get: "jet_jit_cell_guard_get" => jet_jit_cell_guard_get: ternary;
    guard_set: "jet_jit_cell_guard_set" => jet_jit_cell_guard_set: ternary_void;
    get_or_set_store: "jet_jit_cell_get_or_set_store" => jet_jit_cell_get_or_set_store: ternary_void;
    guard_drop: "jet_jit_cell_guard_drop" => jet_jit_cell_guard_drop: binary_void;
    guard_project: "jet_jit_cell_guard_project" => jet_jit_cell_guard_project: ternary;
    guard_map: "jet_cell_guard_map" => jet_jit_cell_guard_map: binary;
    guard_split: "jet_cell_guard_split" => jet_jit_cell_guard_split: ternary;
    get_or_set_begin: "jet_jit_cell_get_or_set_begin" => jet_jit_cell_get_or_set_begin: binary;
    get_or_set_host: "jet_cell_get_or_set" => jet_jit_cell_get_or_set_callback: binary;
    read: "jet_cell_read" => jet_jit_cell_read: binary;
    edit: "jet_cell_edit" => jet_jit_cell_edit: binary;
    read_guard_get: "jet_cell_read_guard_get" => jet_jit_cell_read_guard_get: unary;
    read_guard_read: "jet_cell_read_guard_read" => jet_jit_cell_read_guard_read: binary;
    edit_guard_get: "jet_cell_edit_guard_get" => jet_jit_cell_edit_guard_get: unary;
    edit_guard_set: "jet_cell_edit_guard_set" => jet_jit_cell_edit_guard_set: binary_void;
    edit_guard_read: "jet_cell_edit_guard_read" => jet_jit_cell_edit_guard_read: binary;
    edit_guard_edit: "jet_cell_edit_guard_edit" => jet_jit_cell_edit_guard_edit: binary;
}

fn project_ref<'a>(value: &'a MirRuntimeValue, path: &[String]) -> Option<&'a MirRuntimeValue> {
    let Some((field, rest)) = path.split_first() else {
        return Some(value);
    };
    let MirRuntimeValue::Struct { fields, .. } = value else {
        return None;
    };
    let next = fields
        .iter()
        .find_map(|(name, value)| (name == field).then_some(value))?;
    project_ref(next, rest)
}

fn project_mut<'a>(value: &'a mut MirRuntimeValue, path: &[String]) -> Option<&'a mut MirRuntimeValue> {
    let Some((field, rest)) = path.split_first() else {
        return Some(value);
    };
    let MirRuntimeValue::Struct { fields, .. } = value else {
        return None;
    };
    let next = fields
        .iter_mut()
        .find_map(|(name, value)| (name == field).then_some(value))?;
    project_mut(next, rest)
}

fn project_pair_mut<'a>(
    value: &'a mut MirRuntimeValue,
    first: &[String],
    second: &[String],
) -> Option<(&'a mut MirRuntimeValue, &'a mut MirRuntimeValue)> {
    let (first_field, first_rest) = first.split_first()?;
    let (second_field, second_rest) = second.split_first()?;
    let MirRuntimeValue::Struct { fields, .. } = value else {
        return None;
    };
    if first_field == second_field {
        let next = fields
            .iter_mut()
            .find_map(|(name, value)| (name == first_field).then_some(value))?;
        return project_pair_mut(next, first_rest, second_rest);
    }
    let first_index = fields.iter().position(|(name, _)| name == first_field)?;
    let second_index = fields.iter().position(|(name, _)| name == second_field)?;
    let (first_value, second_value) = if first_index < second_index {
        let (left, right) = fields.split_at_mut(second_index);
        (&mut left[first_index].1, &mut right[0].1)
    } else {
        let (left, right) = fields.split_at_mut(first_index);
        (&mut right[0].1, &mut left[second_index].1)
    };
    Some((
        project_mut(first_value, first_rest)?,
        project_mut(second_value, second_rest)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trap_frame_cleanup_releases_loan_before_next_borrow() {
        let mut state = CellState::new();
        state.cells.push(JetCell::new(MirRuntimeValue::Int(1)));
        state.frames.push(1);
        let guard = state.cells[0].guard_edit();
        state.insert_edit(guard, 1, 0);

        let cell = state.cells[0].clone();
        assert!(
            catch_unwind(AssertUnwindSafe(|| cell.guard_read())).is_err(),
            "the hostile borrow must conflict while the edit loan is live"
        );

        state.leave_frame(&HashSet::new(), &HashSet::new());
        assert_eq!(state.cells[0].guard_read().get(), MirRuntimeValue::Int(1));
    }
}
