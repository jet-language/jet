//! Resident-JIT marshalling for the canonical local `Cell` runtime.

use crate::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::local_cell::{
    JetCell, JetCellEditGuard, JetCellGetOrSet, JetCellReadGuard,
};
use jet_codegen::{AST::CtReport, AST::CtValue, AST::Type};
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::types_meta::JitMeta;

#[derive(Clone)]
pub(crate) enum CellSchema {
    Unit,
    Int,
    Float,
    Bool,
    Char,
    String,
    Option(Box<CellSchema>, Type),
    List(Box<CellSchema>),
    Struct {
        name: String,
        fields: Vec<(String, CellSchema)>,
    },
    Handle,
}

impl CellSchema {
    fn struct_schema(
        name: &str,
        args: &[Type],
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
        let subst: HashMap<_, _> = params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        Ok(Some(Self::Struct {
            name: name.to_string(),
            fields: field_names
                .iter()
                .zip(field_types)
                .map(|(field, ty)| {
                    let ty = jet_foundation::Generics::substitute_type(ty, &subst);
                    Ok((
                        field.strip_prefix("user_").unwrap_or(field).to_string(),
                        Self::from_type(&ty, meta)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        }))
    }

    pub(crate) fn from_type(ty: &Type, meta: &JitMeta<'_>) -> Result<Self, String> {
        match ty {
            Type::Named(name) if name == "Unit" => {
                Ok(Self::Unit)
            }
            Type::Int | Type::IntN { .. } => Ok(Self::Int),
            Type::Float | Type::Float32 => Ok(Self::Float),
            Type::Bool => Ok(Self::Bool),
            Type::Char => Ok(Self::Char),
            Type::String => Ok(Self::String),
            Type::Option(inner) => Ok(Self::Option(
                Box::new(Self::from_type(inner, meta)?),
                inner.as_ref().clone(),
            )),
            Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                Ok(Self::List(Box::new(Self::from_type(inner, meta)?)))
            }
            Type::Tuple(fields) => Ok(Self::Struct {
                name: "tuple".to_string(),
                fields: fields
                    .iter()
                    .map(|(name, ty)| {
                        Ok((name.clone(), Self::from_type(ty.as_ref(), meta)?))
                    })
                    .collect::<Result<_, String>>()?,
            }),
            Type::Tagged { inner, .. } => Self::from_type(inner, meta),
            Type::Named(name) => {
                Ok(Self::struct_schema(name, &[], meta)?.unwrap_or(Self::Handle))
            }
            Type::Apply { name, args } => {
                Ok(Self::struct_schema(name, args, meta)?.unwrap_or(Self::Handle))
            }
            Type::Map { .. }
            | Type::Result { .. }
            | Type::Shared(_)
            | Type::Fn { .. }
            | Type::TraitObject(_)
            | Type::Union(_) => Ok(Self::Handle),
            // Runtime values carry no dimension metadata (I3): a quantity's
            // cell shape is its erased base numeric type.
            Type::Quantity { base, .. } => Self::from_type(base, meta),
        }
    }
}

#[derive(Clone)]
pub(crate) struct CellProjection {
    pub paths: Vec<Vec<String>>,
}

#[derive(Clone)]
pub(crate) enum CellGuardLayout {
    Read,
    Edit,
    Record(Vec<Option<CellGuardLayout>>),
}

impl CellGuardLayout {
    pub(crate) fn from_type(ty: &Type, _meta: &JitMeta<'_>) -> Result<Option<Self>, String> {
        Ok(match ty {
            Type::Apply { name, .. } if name == "CellReadGuard" => Some(Self::Read),
            Type::Apply { name, .. } if name == "CellEditGuard" => Some(Self::Edit),
            Type::Tagged { inner, .. } => Self::from_type(inner, _meta)?,
            Type::Tuple(fields) => {
                let fields = fields
                    .iter()
                    .map(|(_, ty)| Self::from_type(ty, _meta))
                    .collect::<Result<Vec<_>, _>>()?;
                fields.iter().any(Option::is_some).then_some(Self::Record(fields))
            }
            _ => None,
        })
    }
}

struct GuardSlot<G> {
    guard: Option<G>,
    owner: u64,
}

pub(crate) struct CellState {
    cells: Vec<JetCell<CtValue>>,
    read_guards: Vec<GuardSlot<JetCellReadGuard<CtValue>>>,
    edit_guards: Vec<GuardSlot<JetCellEditGuard<CtValue>>>,
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
        !self.schemas.is_empty()
            || !self.projections.is_empty()
            || !self.guard_layouts.is_empty()
    }

    fn owner(&self) -> u64 {
        self.frames.last().copied().unwrap_or(0)
    }

    fn insert_read(&mut self, guard: JetCellReadGuard<CtValue>, owner: u64) -> i64 {
        self.read_guards.push(GuardSlot {
            guard: Some(guard),
            owner,
        });
        self.read_guards.len() as i64
    }

    fn insert_edit(&mut self, guard: JetCellEditGuard<CtValue>, owner: u64) -> i64 {
        self.edit_guards.push(GuardSlot {
            guard: Some(guard),
            owner,
        });
        self.edit_guards.len() as i64
    }

    fn leave_frame(
        &mut self,
        returned_read: &HashSet<i64>,
        returned_edit: &HashSet<i64>,
    ) {
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
    rt.cells.schemas.get((handle as usize).checked_sub(1)?).cloned()
}

fn decode_value(
    rt: &mut crate::JitRuntime,
    raw: i64,
    schema: &CellSchema,
) -> Option<CtValue> {
    Some(match schema {
        CellSchema::Unit => CtValue::Unit,
        CellSchema::Int | CellSchema::Handle => CtValue::Int(raw),
        CellSchema::Float => CtValue::Float(jet_codegen::AST::CtFloat::f64(f64::from_bits(
            raw as u64,
        ))),
        CellSchema::Bool => CtValue::Bool(raw != 0),
        CellSchema::Char => CtValue::Char(char::from_u32(raw as u32)?),
        CellSchema::String => CtValue::Str(rt.heap.clone_string(raw)?),
        CellSchema::Option(inner, inner_ty) => {
            if raw == 0 {
                CtValue::absent(inner_ty.clone())
            } else {
                CtValue::Present(Box::new(decode_value(rt, raw.wrapping_sub(1), inner)?))
            }
        }
        CellSchema::List(inner) => {
            let len = rt.heap.list_len(raw)?;
            let mut values = Vec::with_capacity(len as usize);
            for index in 0..len {
                let item = match inner.as_ref() {
                    CellSchema::Float => {
                        rt.heap.list_get_float(raw, index)?.to_bits() as i64
                    }
                    _ => rt.heap.list_get_int(raw, index)?,
                };
                values.push(decode_value(rt, item, inner)?);
            }
            CtValue::List(values)
        }
        CellSchema::Struct { name, fields } => {
            let mut values = Vec::with_capacity(fields.len());
            for (index, (field, field_schema)) in fields.iter().enumerate() {
                let raw = match field_schema {
                    CellSchema::Float => rt
                        .heap
                        .record_get_float(raw, index as i64)?
                        .to_bits() as i64,
                    CellSchema::Bool => {
                        i64::from(rt.heap.record_get_bool(raw, index as i64)?)
                    }
                    CellSchema::Char => {
                        rt.heap.record_get_char(raw, index as i64)? as u32 as i64
                    }
                    CellSchema::String => rt.heap.record_get_string(raw, index as i64)?,
                    _ => rt.heap.record_get_int(raw, index as i64)?,
                };
                values.push((field.clone(), decode_value(rt, raw, field_schema)?));
            }
            CtValue::Struct {
                type_name: name.clone(),
                fields: values,
            }
        }
    })
}

fn encode_value(
    rt: &mut crate::JitRuntime,
    value: &CtValue,
    schema: &CellSchema,
) -> Option<i64> {
    match (schema, value) {
        (CellSchema::Unit, CtValue::Unit) => Some(0),
        (CellSchema::Int | CellSchema::Handle, CtValue::Int(value)) => Some(*value),
        (CellSchema::Float, CtValue::Float(value)) => Some(value.as_f64().to_bits() as i64),
        (CellSchema::Bool, CtValue::Bool(value)) => Some(i64::from(*value)),
        (CellSchema::Char, CtValue::Char(value)) => Some(*value as u32 as i64),
        (CellSchema::String, CtValue::Str(value)) => Some(rt.heap.alloc_string(value.clone())),
        (CellSchema::Option(_, _), CtValue::Failed(CtReport::Clean(_))) => Some(0),
        (CellSchema::Option(inner, _), CtValue::Present(value)) => {
            Some(encode_value(rt, value, inner)?.wrapping_add(1))
        }
        (CellSchema::List(inner), CtValue::List(values)) => {
            let list = rt.heap.alloc_empty_list();
            for value in values {
                let raw = encode_value(rt, value, inner)?;
                match inner.as_ref() {
                    CellSchema::Float => {
                        rt.heap
                            .list_push_float(list, f64::from_bits(raw as u64))?;
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
            CtValue::Struct {
                type_name,
                fields,
            },
        ) if expected == type_name || expected == "tuple" => {
            let record = rt.heap.alloc_record(schemas.len());
            for (index, (field, field_schema)) in schemas.iter().enumerate() {
                let value = fields
                    .iter()
                    .find_map(|(name, value)| (name == field).then_some(value))?;
                let raw = encode_value(rt, value, field_schema)?;
                match field_schema {
                    CellSchema::Float => {
                        rt.heap
                            .record_set_float(record, index as i64, f64::from_bits(raw as u64))?;
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
                .or_else(|| error.downcast_ref::<&str>().map(|value| (*value).to_string()))
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

extern "C" fn jet_jit_cell_frame_enter() {
    with_cell(|rt| {
        let frame = rt.cells.next_frame;
        rt.cells.next_frame += 1;
        rt.cells.frames.push(frame);
    });
}

extern "C" fn jet_jit_cell_frame_leave(layout_handle: i64, returned: i64) {
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

extern "C" fn jet_jit_cell_new(raw: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = decode_value(rt, raw, &schema).expect("Cell value ABI");
        rt.cells.cells.push(JetCell::new(value));
        rt.cells.cells.len() as i64
    })
}

extern "C" fn jet_jit_cell_get(cell: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = rt.cells.cells[(cell - 1) as usize].get();
        encode_value(rt, &value, &schema).expect("Cell value ABI")
    })
}

extern "C" fn jet_jit_cell_set(cell: i64, raw: i64, schema_handle: i64) {
    with_cell(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = decode_value(rt, raw, &schema).expect("Cell value ABI");
        rt.cells.cells[(cell - 1) as usize].set(value);
    });
}

extern "C" fn jet_jit_cell_replace(cell: i64, raw: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = decode_value(rt, raw, &schema).expect("Cell value ABI");
        let old = rt.cells.cells[(cell - 1) as usize].replace(value);
        encode_value(rt, &old, &schema).expect("Cell value ABI")
    })
}

extern "C" fn jet_jit_cell_guard_read(cell: i64) -> i64 {
    with_cell_result(|rt| {
        let guard = rt.cells.cells[(cell - 1) as usize].guard_read();
        let owner = rt.cells.owner();
        rt.cells.insert_read(guard, owner)
    })
}

extern "C" fn jet_jit_cell_guard_edit(cell: i64) -> i64 {
    with_cell_result(|rt| {
        let guard = rt.cells.cells[(cell - 1) as usize].guard_edit();
        let owner = rt.cells.owner();
        rt.cells.insert_edit(guard, owner)
    })
}

extern "C" fn jet_jit_cell_guard_get(kind: i64, guard: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
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
        encode_value(rt, &value, &schema).expect("Cell guard value ABI")
    })
}

extern "C" fn jet_jit_cell_guard_set(guard: i64, raw: i64, schema_handle: i64) {
    with_cell(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = decode_value(rt, raw, &schema).expect("Cell guard value ABI");
        rt.cells.edit_guards[(guard - 1) as usize]
            .guard
            .as_ref()
            .expect("live Cell edit guard")
            .set(value);
    });
}

extern "C" fn jet_jit_cell_get_or_set_store(
    guard: i64,
    raw: i64,
    schema_handle: i64,
) {
    with_cell(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let value = decode_value(rt, raw, &schema).expect("Cell optional value ABI");
        rt.cells.edit_guards[(guard - 1) as usize]
            .guard
            .as_ref()
            .expect("live Cell optional edit guard")
            .store_option_value(value);
    });
}

extern "C" fn jet_jit_cell_guard_drop(kind: i64, guard: i64) {
    with_cell(|rt| {
        if kind == 1 {
            rt.cells.read_guards[(guard - 1) as usize].guard = None;
        } else {
            rt.cells.edit_guards[(guard - 1) as usize].guard = None;
        }
    });
}

extern "C" fn jet_jit_cell_guard_project(
    kind: i64,
    guard: i64,
    projection_handle: i64,
) -> i64 {
    with_cell_result(|rt| {
        let projection = rt.cells.projections[(projection_handle - 1) as usize].clone();
        let owner = rt.cells.owner();
        if kind == 2 {
            let guard = rt.cells.edit_guards[(guard - 1) as usize]
                .guard
                .take()
                .expect("live Cell edit guard");
            match projection.paths.as_slice() {
                [path] => {
                    let guard = guard.map(|value| project_mut(value, path).expect("Cell path"));
                    rt.cells.insert_edit(guard, owner)
                }
                [first, second] => {
                    let (first, second) = guard.split(|value| {
                        project_pair_mut(value, first, second).expect("disjoint Cell paths")
                    });
                    let first = rt.cells.insert_edit(first, owner);
                    let second = rt.cells.insert_edit(second, owner);
                    let record = rt.heap.alloc_record(2);
                    rt.heap.record_set_int(record, 0, first).expect("Cell tuple");
                    rt.heap.record_set_int(record, 1, second).expect("Cell tuple");
                    record
                }
                _ => jet_foundation::ice!(None, "Cell projection shape"),
            }
        } else {
            let guard = rt.cells.read_guards[(guard - 1) as usize]
                .guard
                .take()
                .expect("live Cell read guard");
            match projection.paths.as_slice() {
                [path] => {
                    let guard = guard.map(|value| project_ref(value, path).expect("Cell path"));
                    rt.cells.insert_read(guard, owner)
                }
                [first, second] => {
                    let (first, second) = guard.split(|value| {
                        (
                            project_ref(value, first).expect("Cell path"),
                            project_ref(value, second).expect("Cell path"),
                        )
                    });
                    let first = rt.cells.insert_read(first, owner);
                    let second = rt.cells.insert_read(second, owner);
                    let record = rt.heap.alloc_record(2);
                    rt.heap.record_set_int(record, 0, first).expect("Cell tuple");
                    rt.heap.record_set_int(record, 1, second).expect("Cell tuple");
                    record
                }
                _ => jet_foundation::ice!(None, "Cell projection shape"),
            }
        }
    })
}

extern "C" fn jet_jit_cell_get_or_set_begin(cell: i64, schema_handle: i64) -> i64 {
    with_cell_result(|rt| {
        let schema = schema(rt, schema_handle).expect("Cell schema");
        let cell = rt.cells.cells[(cell - 1) as usize].clone();
        let record = rt.heap.alloc_record(2);
        match cell.begin_get_or_set() {
            JetCellGetOrSet::Value(value) => {
                let raw = encode_value(rt, &value, &schema).expect("Cell value ABI");
                rt.heap.record_set_bool(record, 0, true).expect("Cell init");
                rt.heap.record_set_int(record, 1, raw).expect("Cell init");
            }
            JetCellGetOrSet::Empty(guard) => {
                let owner = rt.cells.owner();
                let guard = rt.cells.insert_edit(guard, owner);
                rt.heap.record_set_bool(record, 0, false).expect("Cell init");
                rt.heap.record_set_int(record, 1, guard).expect("Cell init");
            }
        }
        record
    })
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
    get: "jet_jit_cell_get" => jet_jit_cell_get: binary;
    set: "jet_jit_cell_set" => jet_jit_cell_set: ternary_void;
    replace: "jet_jit_cell_replace" => jet_jit_cell_replace: ternary;
    guard_read: "jet_jit_cell_guard_read" => jet_jit_cell_guard_read: unary;
    guard_edit: "jet_jit_cell_guard_edit" => jet_jit_cell_guard_edit: unary;
    guard_get: "jet_jit_cell_guard_get" => jet_jit_cell_guard_get: ternary;
    guard_set: "jet_jit_cell_guard_set" => jet_jit_cell_guard_set: ternary_void;
    get_or_set_store: "jet_jit_cell_get_or_set_store" => jet_jit_cell_get_or_set_store: ternary_void;
    guard_drop: "jet_jit_cell_guard_drop" => jet_jit_cell_guard_drop: binary_void;
    guard_project: "jet_jit_cell_guard_project" => jet_jit_cell_guard_project: ternary;
    get_or_set_begin: "jet_jit_cell_get_or_set_begin" => jet_jit_cell_get_or_set_begin: binary;
}






fn project_ref<'a>(value: &'a CtValue, path: &[String]) -> Option<&'a CtValue> {
    let Some((field, rest)) = path.split_first() else {
        return Some(value);
    };
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    let next = fields
        .iter()
        .find_map(|(name, value)| (name == field).then_some(value))?;
    project_ref(next, rest)
}

fn project_mut<'a>(value: &'a mut CtValue, path: &[String]) -> Option<&'a mut CtValue> {
    let Some((field, rest)) = path.split_first() else {
        return Some(value);
    };
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    let next = fields
        .iter_mut()
        .find_map(|(name, value)| (name == field).then_some(value))?;
    project_mut(next, rest)
}

fn project_pair_mut<'a>(
    value: &'a mut CtValue,
    first: &[String],
    second: &[String],
) -> Option<(&'a mut CtValue, &'a mut CtValue)> {
    let (first_field, first_rest) = first.split_first()?;
    let (second_field, second_rest) = second.split_first()?;
    let CtValue::Struct { fields, .. } = value else {
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
        state.cells.push(JetCell::new(CtValue::Int(1)));
        state.frames.push(1);
        let guard = state.cells[0].guard_edit();
        state.insert_edit(guard, 1);

        let cell = state.cells[0].clone();
        assert!(
            catch_unwind(AssertUnwindSafe(|| cell.guard_read())).is_err(),
            "the hostile borrow must conflict while the edit loan is live"
        );

        state.leave_frame(&HashSet::new(), &HashSet::new());
        assert_eq!(state.cells[0].guard_read().get(), CtValue::Int(1));
    }
}
