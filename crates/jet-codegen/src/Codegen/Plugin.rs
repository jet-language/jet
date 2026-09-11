//! D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A (c81): `target: sandbox`
//! guest artifact projection. The checked MIR artifact plan owns the package
//! identity and export rows; this module only renders the guest-side WIT and
//! wrapper text around the already-emitted whole-program Rust.
//!
//! Export rows may use the checked scalar projection or a recursively closed
//! Component Model shape (`record`, `list`, `option`, and `result`). The same
//! compile-time descriptor drives WIT declarations and JIT marshalling; this
//! module does not inspect source AST, rerun semantic lowering, or reconstruct
//! visibility from public functions.

use std::collections::BTreeSet;

use jet_foundation::MIR::{
    MirAccess, MirArtifactId, MirArtifactKind, MirArtifactTarget, MirProgram, MirType,
    MirTypeDef, MirTypeDefKind, MirTypeKind,
};
use jet_foundation::Names::{mangle, mangle_generated, mangle_path};
use super::Embedding::{ComponentSignature, ComponentSignatureDescriptor, ComponentTypeDescriptor, ExportScalar};

/// The guest-side artifacts for a `target: sandbox` build.
#[derive(Debug, Clone)]
pub struct PluginArtifacts {
    /// The full `.wit` world text, e.g. `package jet:mathkit@0.1.0; world
    /// jetplugin { export scale: func(a: f64, b: f64) -> f64; … }`.
    pub wit: String,
    /// The complete guest Rust source (whole-program Rust plus the
    /// `#[export_name]` wrapper functions) — ready for
    /// `rustc --target wasm32-unknown-unknown --crate-type cdylib`.
    pub guest_rust: String,
    /// The `.wit` world name (`jetplugin`, fixed — the package name below is
    /// what varies per plugin).
    pub world_name: String,
    /// The sanitized artifact-plan name used as the package identity.
    pub export_name: String,
    /// The Jet names of every function actually exported (for the ApiFreeze
    /// snapshot / version-handshake diagnostics).
    pub exported_fns: Vec<String>,
    /// The same typed rows consumed by the native Library lowerer.
    pub exports: Vec<super::Embedding::ExportFunction>,
}

/// The one fixed `.wit` world name every plugin uses — only the `package`
/// identity (from the selected artifact-plan name) varies (D-PLUGIN-EXPORT1=A:
/// no new in-source keyword, so there is nothing else to name per-plugin).
const WORLD_NAME: &str = "jetplugin";


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

fn rust_named_type_name(
    program: &MirProgram,
    id: jet_foundation::MIR::MirTypeId,
    fallback: &str,
) -> String {
    program
        .types
        .iter()
        .find(|definition| definition.id == id)
        .map(|definition| mangle_path(&definition.key))
        .unwrap_or_else(|| mangle_path(fallback))
}

fn rust_type(program: &MirProgram, ty: &MirType) -> String {
    match ty.kind() {
        MirTypeKind::Int => "i64".to_string(),
        MirTypeKind::Float => "f64".to_string(),
        MirTypeKind::Bool => "bool".to_string(),
        MirTypeKind::String => "String".to_string(),
        MirTypeKind::List(inner) => format!("Vec<{}>", rust_type(program, inner)),
        MirTypeKind::Option(inner) => {
            format!("JetOutcome<{}, JetAbsent>", rust_type(program, inner))
        }
        MirTypeKind::Result { ok, err } => {
            format!("JetOutcome<{}, {}>", rust_type(program, ok), rust_type(program, err))
        }
        MirTypeKind::FixedList { elem, len } => {
            format!("[{}; {}]", rust_type(program, elem), len.expression())
        }
        MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
            rust_type(program, base)
        }
        MirTypeKind::Apply { name, args } => {
            let head = rust_named_type_name(program, name.id, &name.name);
            if args.is_empty() {
                head
            } else {
                format!(
                    "{head}<{}>",
                    args.iter()
                        .map(|arg| rust_type(program, arg))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        _ => "()".to_string(),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PluginCoreSlot {
    I32,
    I64,
    F64,
}

impl PluginCoreSlot {
    fn rust_ty(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F64 => "f64",
        }
    }
}

fn merge_plugin_slot(left: PluginCoreSlot, right: PluginCoreSlot) -> PluginCoreSlot {
    match (left, right) {
        (PluginCoreSlot::I32, PluginCoreSlot::I32) => PluginCoreSlot::I32,
        (PluginCoreSlot::F64, PluginCoreSlot::F64) => PluginCoreSlot::F64,
        (PluginCoreSlot::I64, PluginCoreSlot::I64)
        | (PluginCoreSlot::I32, PluginCoreSlot::I64)
        | (PluginCoreSlot::I64, PluginCoreSlot::I32)
        | (PluginCoreSlot::I64, PluginCoreSlot::F64)
        | (PluginCoreSlot::F64, PluginCoreSlot::I64)
        | (PluginCoreSlot::I32, PluginCoreSlot::F64)
        | (PluginCoreSlot::F64, PluginCoreSlot::I32) => PluginCoreSlot::I64,
    }
}

fn merge_plugin_slots(
    left: Vec<PluginCoreSlot>,
    right: Vec<PluginCoreSlot>,
) -> Vec<PluginCoreSlot> {
    let length = left.len().max(right.len());
    (0..length)
        .filter_map(|index| match (left.get(index), right.get(index)) {
            (Some(left), Some(right)) => Some(merge_plugin_slot(*left, *right)),
            (Some(slot), None) | (None, Some(slot)) => Some(*slot),
            (None, None) => None,
        })
        .collect()
}

fn plugin_flat_slots(ty: &ComponentTypeDescriptor) -> Vec<PluginCoreSlot> {
    match ty {
        ComponentTypeDescriptor::Int => vec![PluginCoreSlot::I64],
        ComponentTypeDescriptor::Float => vec![PluginCoreSlot::F64],
        ComponentTypeDescriptor::Bool => vec![PluginCoreSlot::I32],
        ComponentTypeDescriptor::String | ComponentTypeDescriptor::List(_) => {
            vec![PluginCoreSlot::I32, PluginCoreSlot::I32]
        }
        ComponentTypeDescriptor::Option(inner) => {
            let mut slots = vec![PluginCoreSlot::I32];
            slots.extend(plugin_flat_slots(inner));
            slots
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let mut slots = vec![PluginCoreSlot::I32];
            slots.extend(merge_plugin_slots(
                plugin_flat_slots(ok),
                plugin_flat_slots(err),
            ));
            slots
        }
        ComponentTypeDescriptor::Record { fields, .. } => fields
            .iter()
            .flat_map(|(_, _, ty)| plugin_flat_slots(ty))
            .collect(),
    }
}

fn plugin_align_to(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

fn plugin_memory_layout(ty: &ComponentTypeDescriptor) -> (usize, usize) {
    match ty {
        ComponentTypeDescriptor::Int | ComponentTypeDescriptor::Float => (8, 8),
        ComponentTypeDescriptor::Bool => (1, 1),
        ComponentTypeDescriptor::String | ComponentTypeDescriptor::List(_) => (8, 4),
        ComponentTypeDescriptor::Option(inner) => {
            let (size, align) = plugin_memory_layout(inner);
            let payload = plugin_align_to(1, align);
            (plugin_align_to(payload + size, align), align)
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let (ok_size, ok_align) = plugin_memory_layout(ok);
            let (err_size, err_align) = plugin_memory_layout(err);
            let align = ok_align.max(err_align);
            let payload = plugin_align_to(1, align);
            (
                plugin_align_to(payload + ok_size.max(err_size), align),
                align,
            )
        }
        ComponentTypeDescriptor::Record { fields, .. } => {
            let mut size = 0;
            let mut align = 1;
            for (_, _, field) in fields {
                let (field_size, field_align) = plugin_memory_layout(field);
                size = plugin_align_to(size, field_align) + field_size;
                align = align.max(field_align);
            }
            (plugin_align_to(size, align), align)
        }
    }
}

fn plugin_sequence_layout(types: &[ComponentTypeDescriptor]) -> (usize, usize) {
    let mut size = 0;
    let mut align = 1;
    for ty in types {
        let (field_size, field_align) = plugin_memory_layout(ty);
        size = plugin_align_to(size, field_align) + field_size;
        align = align.max(field_align);
    }
    (plugin_align_to(size, align), align)
}

fn plugin_record_offsets(
    fields: &[(usize, String, ComponentTypeDescriptor)],
) -> Vec<(usize, &ComponentTypeDescriptor)> {
    let mut offset = 0;
    let mut result = Vec::with_capacity(fields.len());
    for (_, _, field) in fields {
        let (_, align) = plugin_memory_layout(field);
        offset = plugin_align_to(offset, align);
        result.push((offset, field));
        offset += plugin_memory_layout(field).0;
    }
    result
}

fn plugin_needs_post_return(ty: &ComponentTypeDescriptor) -> bool {
    match ty {
        ComponentTypeDescriptor::String | ComponentTypeDescriptor::List(_) => true,
        ComponentTypeDescriptor::Option(inner) => plugin_needs_post_return(inner),
        ComponentTypeDescriptor::Result { ok, err } => {
            plugin_needs_post_return(ok) || plugin_needs_post_return(err)
        }
        ComponentTypeDescriptor::Record { fields, .. } => fields
            .iter()
            .any(|(_, _, ty)| plugin_needs_post_return(ty)),
        ComponentTypeDescriptor::Int
        | ComponentTypeDescriptor::Float
        | ComponentTypeDescriptor::Bool => false,
    }
}

fn plugin_unwrap_transparent<'a>(program: &'a MirProgram, ty: &'a MirType) -> &'a MirType {
    match ty.kind() {
        MirTypeKind::InlineRange { base, .. } | MirTypeKind::Tagged { inner: base, .. } => {
            plugin_unwrap_transparent(program, base)
        }
        MirTypeKind::Apply { name: nominal, .. } => {
            let Some(definition) = program.types.iter().find(|definition| definition.id == nominal.id)
            else {
                return ty;
            };
            match &definition.kind {
                MirTypeDefKind::Alias { target } => plugin_unwrap_transparent(program, target),
                _ => ty,
            }
        }
        _ => ty,
    }
}

fn plugin_record_definition<'a>(
    program: &'a MirProgram,
    ty: &'a MirType,
) -> Option<&'a MirTypeDef> {
    let ty = plugin_unwrap_transparent(program, ty);
    let MirTypeKind::Apply { name: nominal, .. } = ty.kind() else {
        return None;
    };
    let definition = program
        .types
        .iter()
        .find(|definition| definition.id == nominal.id)?;
    matches!(&definition.kind, MirTypeDefKind::Struct { .. }).then_some(definition)
}

fn plugin_record_fields<'a>(
    program: &'a MirProgram,
    ty: &'a MirType,
) -> Option<&'a [jet_foundation::MIR::MirField]> {
    let definition = plugin_record_definition(program, ty)?;
    match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => Some(fields),
        _ => None,
    }
}

fn plugin_sequence_element<'a>(
    program: &'a MirProgram,
    ty: &'a MirType,
) -> Option<&'a MirType> {
    match plugin_unwrap_transparent(program, ty).kind() {
        MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => Some(inner),
        _ => None,
    }
}

fn plugin_fixed_list_len(
    program: &MirProgram,
    ty: &MirType,
) -> Option<String> {
    match plugin_unwrap_transparent(program, ty).kind() {
        MirTypeKind::FixedList { len, .. } => Some(len.expression()),
        _ => None,
    }
}

fn plugin_option_element<'a>(
    program: &'a MirProgram,
    ty: &'a MirType,
) -> Option<&'a MirType> {
    match plugin_unwrap_transparent(program, ty).kind() {
        MirTypeKind::Option(inner) => Some(inner),
        _ => None,
    }
}

fn plugin_result_elements<'a>(
    program: &'a MirProgram,
    ty: &'a MirType,
) -> Option<(&'a MirType, &'a MirType)> {
    match plugin_unwrap_transparent(program, ty).kind() {
        MirTypeKind::Result { ok, err } => Some((ok, err)),
        _ => None,
    }
}

fn plugin_fresh(counter: &mut usize, stem: &str) -> String {
    let value = format!("__jet_plugin_{stem}_{counter}");
    *counter += 1;
    value
}

fn plugin_read_memory(ptr: &str, offset: usize, rust_ty: &str) -> String {
    format!(
        "unsafe {{ std::ptr::read_unaligned((({ptr}) as usize + {offset}) as *const {rust_ty}) }}"
    )
}

fn plugin_write_memory(ptr: &str, offset: usize, rust_ty: &str, value: &str) -> String {
    format!(
        "unsafe {{ std::ptr::write_unaligned((({ptr}) as usize + {offset}) as *mut {rust_ty}, {value}); }}"
    )
}

fn plugin_flat_lift_expr(
    program: &MirProgram,
    source_ty: &MirType,
    descriptor: &ComponentTypeDescriptor,
    args: &[String],
    arg_slots: &[PluginCoreSlot],
    cursor: &mut usize,
    counter: &mut usize,
    read_text: &str,
) -> String {
    match descriptor {
        ComponentTypeDescriptor::Int => {
            let raw = args[*cursor].clone();
            *cursor += 1;
            format!("({raw} as i64)")
        }
        ComponentTypeDescriptor::Float => {
            let raw = args[*cursor].clone();
            let slot = arg_slots[*cursor];
            *cursor += 1;
            if slot == PluginCoreSlot::F64 {
                raw
            } else {
                format!("f64::from_bits(({raw} as u64))")
            }
        }
        ComponentTypeDescriptor::Bool => {
            let raw = args[*cursor].clone();
            let slot = arg_slots[*cursor];
            *cursor += 1;
            match slot {
                PluginCoreSlot::F64 => {
                    format!("match {raw} {{ 0.0 => false, 1.0 => true, _ => panic!(\"invalid canonical bool\") }}")
                }
                PluginCoreSlot::I32 | PluginCoreSlot::I64 => {
                    format!("match {raw} {{ 0 => false, 1 => true, _ => panic!(\"invalid canonical bool\") }}")
                }
            }
        }
        ComponentTypeDescriptor::String => {
            let ptr = args[*cursor].clone();
            let len = args[*cursor + 1].clone();
            *cursor += 2;
            format!("{read_text}(({ptr}) as i32, ({len}) as i32)")
        }
        ComponentTypeDescriptor::List(inner) => {
            let ptr = args[*cursor].clone();
            let len = args[*cursor + 1].clone();
            *cursor += 2;
            let element_ty = plugin_sequence_element(program, source_ty)
                .expect("component list source type has no element");
            let base = plugin_fresh(counter, "list_base");
            let length = plugin_fresh(counter, "list_len");
            let output = plugin_fresh(counter, "list_out");
            let index = plugin_fresh(counter, "list_index");
            let element_ptr = plugin_fresh(counter, "list_element_ptr");
            let element = plugin_memory_lift_expr(
                program,
                element_ty,
                inner,
                &element_ptr,
                counter,
                read_text,
            );
            let output_type = rust_type(program, source_ty);
            let fixed = plugin_fixed_list_len(program, source_ty)
                .map(|len| {
                    format!(
                        "let {output}: {output_type} = match {output}.try_into() {{ Ok(value) => value, Err(_) => panic!(\"canonical list length mismatch (expected {len})\") }}; {output}"
                    )
                })
                .unwrap_or_else(|| output.clone());
            format!(
                "{{ let {base} = ({ptr}) as usize; let {length} = ({len}) as usize; let mut {output} = Vec::with_capacity({length}); for {index} in 0..{length} {{ let {element_ptr} = ({base} + {index} * {}) as i32; {output}.push({element}); }} {fixed} }}",
                plugin_memory_layout(inner).0
            )
        }
        ComponentTypeDescriptor::Option(inner) => {
            let tag = args[*cursor].clone();
            *cursor += 1;
            let payload_start = *cursor;
            let payload_slots = plugin_flat_slots(inner);
            let child = plugin_flat_lift_expr(
                program,
                plugin_option_element(program, source_ty)
                    .expect("component option source type has no element"),
                inner,
                args,
                arg_slots,
                cursor,
                counter,
                read_text,
            );
            *cursor = payload_start + payload_slots.len();
            format!(
                "{{ match {tag} {{ 0 => Err(JetAbsent), 1 => Ok({child}), _ => panic!(\"invalid canonical option discriminant\") }} }}"
            )
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let tag = args[*cursor].clone();
            *cursor += 1;
            let payload_start = *cursor;
            let payload_slots = merge_plugin_slots(plugin_flat_slots(ok), plugin_flat_slots(err));
            let (source_ok, source_err) = plugin_result_elements(program, source_ty)
                .expect("component result source type has no variants");
            let mut ok_cursor = payload_start;
            let ok_expr = plugin_flat_lift_expr(
                program,
                source_ok,
                ok,
                args,
                arg_slots,
                &mut ok_cursor,
                counter,
                read_text,
            );
            let mut err_cursor = payload_start;
            let err_expr = plugin_flat_lift_expr(
                program,
                source_err,
                err,
                args,
                arg_slots,
                &mut err_cursor,
                counter,
                read_text,
            );
            *cursor = payload_start + payload_slots.len();
            format!(
                "{{ match {tag} {{ 0 => Ok({ok_expr}), 1 => Err({err_expr}), _ => panic!(\"invalid canonical result discriminant\") }} }}"
            )
        }
        ComponentTypeDescriptor::Record { fields: descriptor_fields, .. } => {
            let source_fields = plugin_record_fields(program, source_ty)
                .expect("component record source type has no fields");
            let mut values = std::collections::BTreeMap::new();
            for (index, _, field_descriptor) in descriptor_fields {
                let field = source_fields
                    .get(*index)
                    .expect("component record field index is out of range");
                let value = plugin_flat_lift_expr(
                    program,
                    &field.ty,
                    field_descriptor,
                    args,
                    arg_slots,
                    cursor,
                    counter,
                    read_text,
                );
                values.insert(*index, value);
            }
            let record = plugin_record_definition(program, source_ty)
                .expect("component record source definition disappeared");
            let name = rust_named_type_name(program, record.id, &record.name);
            let fields = match &record.kind {
                MirTypeDefKind::Struct { fields, .. } => fields,
                _ => unreachable!(),
            };
            let assignments = fields
                .iter()
                .filter(|field| !field.computed)
                .map(|field| {
                    let value = if field.skip {
                        "Default::default()".to_string()
                    } else {
                        values
                            .get(&fields.iter().position(|candidate| candidate.id == field.id).unwrap())
                            .cloned()
                            .unwrap_or_else(|| "Default::default()".to_string())
                    };
                    format!("{}: {value}", mangle(&field.name))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name} {{ {assignments} }}")
        }
    }
}

fn plugin_memory_lift_expr(
    program: &MirProgram,
    source_ty: &MirType,
    descriptor: &ComponentTypeDescriptor,
    ptr: &str,
    counter: &mut usize,
    read_text: &str,
) -> String {
    match descriptor {
        ComponentTypeDescriptor::Int => plugin_read_memory(ptr, 0, "i64"),
        ComponentTypeDescriptor::Float => plugin_read_memory(ptr, 0, "f64"),
        ComponentTypeDescriptor::Bool => {
            let raw = plugin_read_memory(ptr, 0, "u8");
            format!("match {raw} {{ 0 => false, 1 => true, _ => panic!(\"invalid canonical bool\") }}")
        }
        ComponentTypeDescriptor::String => {
            let data = plugin_read_memory(ptr, 0, "i32");
            let length = plugin_read_memory(ptr, 4, "i32");
            format!("{read_text}({data}, {length})")
        }
        ComponentTypeDescriptor::List(inner) => {
            let data = plugin_read_memory(ptr, 0, "i32");
            let length = plugin_read_memory(ptr, 4, "i32");
            let element_ty = plugin_sequence_element(program, source_ty)
                .expect("component list source type has no element");
            let base = plugin_fresh(counter, "mem_list_base");
            let len = plugin_fresh(counter, "mem_list_len");
            let output = plugin_fresh(counter, "mem_list_out");
            let index = plugin_fresh(counter, "mem_list_index");
            let element_ptr = plugin_fresh(counter, "mem_list_element_ptr");
            let element = plugin_memory_lift_expr(
                program,
                element_ty,
                inner,
                &element_ptr,
                counter,
                read_text,
            );
            let output_type = rust_type(program, source_ty);
            let fixed = plugin_fixed_list_len(program, source_ty)
                .map(|expected| {
                    format!(
                        "let {output}: {output_type} = match {output}.try_into() {{ Ok(value) => value, Err(_) => panic!(\"canonical list length mismatch (expected {expected})\") }}; {output}"
                    )
                })
                .unwrap_or_else(|| output.clone());
            format!(
                "{{ let {base} = ({data}) as usize; let {len} = ({length}) as usize; let mut {output} = Vec::with_capacity({len}); for {index} in 0..{len} {{ let {element_ptr} = ({base} + {index} * {}) as i32; {output}.push({element}); }} {fixed} }}",
                plugin_memory_layout(inner).0
            )
        }
        ComponentTypeDescriptor::Option(inner) => {
            let raw = plugin_read_memory(ptr, 0, "u8");
            let payload_offset = plugin_align_to(1, plugin_memory_layout(inner).1);
            let child_ty = plugin_option_element(program, source_ty)
                .expect("component option source type has no element");
            let child_ptr = format!("(({ptr}) as usize + {payload_offset}) as i32");
            let child = plugin_memory_lift_expr(
                program,
                child_ty,
                inner,
                &child_ptr,
                counter,
                read_text,
            );
            format!(
                "{{ match {raw} {{ 0 => Err(JetAbsent), 1 => Ok({child}), _ => panic!(\"invalid canonical option discriminant\") }} }}"
            )
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let raw = plugin_read_memory(ptr, 0, "u8");
            let (ok_size, ok_align) = plugin_memory_layout(ok);
            let (err_size, err_align) = plugin_memory_layout(err);
            let payload_offset = plugin_align_to(1, ok_align.max(err_align));
            let (source_ok, source_err) = plugin_result_elements(program, source_ty)
                .expect("component result source type has no variants");
            let ok_ptr = format!("(({ptr}) as usize + {payload_offset}) as i32");
            let err_ptr = ok_ptr.clone();
            let ok_expr = plugin_memory_lift_expr(
                program,
                source_ok,
                ok,
                &ok_ptr,
                counter,
                read_text,
            );
            let err_expr = plugin_memory_lift_expr(
                program,
                source_err,
                err,
                &err_ptr,
                counter,
                read_text,
            );
            let _ = (ok_size, err_size);
            format!(
                "{{ match {raw} {{ 0 => Ok({ok_expr}), 1 => Err({err_expr}), _ => panic!(\"invalid canonical result discriminant\") }} }}"
            )
        }
        ComponentTypeDescriptor::Record { fields, .. } => {
            let source_fields = plugin_record_fields(program, source_ty)
                .expect("component record source type has no fields");
            let offsets = plugin_record_offsets(fields);
            let mut values = std::collections::BTreeMap::new();
            for ((index, _, field_descriptor), (offset, _)) in fields.iter().zip(offsets) {
                let field = source_fields
                    .get(*index)
                    .expect("component record field index is out of range");
                let field_ptr = format!("(({ptr}) as usize + {offset}) as i32");
                let value = plugin_memory_lift_expr(
                    program,
                    &field.ty,
                    field_descriptor,
                    &field_ptr,
                    counter,
                    read_text,
                );
                values.insert(*index, value);
            }
            let record = plugin_record_definition(program, source_ty)
                .expect("component record source definition disappeared");
            let name = rust_named_type_name(program, record.id, &record.name);
            let fields = match &record.kind {
                MirTypeDefKind::Struct { fields, .. } => fields,
                _ => unreachable!(),
            };
            let assignments = fields
                .iter()
                .filter(|field| !field.computed)
                .map(|field| {
                    let value = if field.skip {
                        "Default::default()".to_string()
                    } else {
                        values
                            .get(&fields.iter().position(|candidate| candidate.id == field.id).unwrap())
                            .cloned()
                            .unwrap_or_else(|| "Default::default()".to_string())
                    };
                    format!("{}: {value}", mangle(&field.name))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name} {{ {assignments} }}")
        }
    }
}

fn plugin_flat_lower_expr(
    program: &MirProgram,
    source_ty: &MirType,
    descriptor: &ComponentTypeDescriptor,
    value: &str,
) -> String {
    match descriptor {
        ComponentTypeDescriptor::Int => format!("*({value})"),
        ComponentTypeDescriptor::Float => format!("*({value})"),
        ComponentTypeDescriptor::Bool => format!("if *({value}) {{ 1i32 }} else {{ 0i32 }}"),
        ComponentTypeDescriptor::Record { fields, .. } => {
            let source_fields = plugin_record_fields(program, source_ty)
                .expect("component record source type has no fields");
            let (index, _, field_descriptor) = fields
                .first()
                .expect("zero-slot record cannot be lowered as a scalar");
            let field = source_fields
                .get(*index)
                .expect("component record field index is out of range");
            plugin_flat_lower_expr(
                program,
                &field.ty,
                field_descriptor,
                &format!("&({value}).{}", mangle(&field.name)),
            )
        }
        ComponentTypeDescriptor::Option(inner) => {
            if plugin_flat_slots(inner).is_empty() {
                format!("match {value} {{ Ok(_) => 1i32, Err(_) => 0i32 }}")
            } else {
                panic!("multi-slot option value passed to flat lowering")
            }
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            if plugin_flat_slots(ok).is_empty() && plugin_flat_slots(err).is_empty() {
                format!("match {value} {{ Ok(_) => 0i32, Err(_) => 1i32 }}")
            } else {
                panic!("multi-slot result value passed to flat lowering")
            }
        }
        ComponentTypeDescriptor::String | ComponentTypeDescriptor::List(_) => {
            panic!("multi-slot component value passed to flat lowering")
        }
    }
}

fn plugin_memory_lower(
    program: &MirProgram,
    source_ty: &MirType,
    descriptor: &ComponentTypeDescriptor,
    value: &str,
    ptr: &str,
    counter: &mut usize,
    allocator: &str,
) -> String {
    match descriptor {
        ComponentTypeDescriptor::Int => plugin_write_memory(ptr, 0, "i64", &format!("*({value})")),
        ComponentTypeDescriptor::Float => plugin_write_memory(ptr, 0, "f64", &format!("*({value})")),
        ComponentTypeDescriptor::Bool => plugin_write_memory(
            ptr,
            0,
            "u8",
            &format!("if *({value}) {{ 1u8 }} else {{ 0u8 }}"),
        ),
        ComponentTypeDescriptor::String => {
            let bytes = plugin_fresh(counter, "string_bytes");
            let length = plugin_fresh(counter, "string_len");
            let data = plugin_fresh(counter, "string_data");
            let statements = [
                format!("let {bytes} = ({value}).as_bytes();"),
                format!("let {length} = {bytes}.len();"),
                format!(
                    "let {data} = {allocator}(0, 0, 1, {length} as i32); if {data} == 0 && {length} != 0 {{ panic!(\"canonical string allocation failed\") }}"
                ),
                format!(
                    "if {length} != 0 {{ unsafe {{ std::ptr::copy_nonoverlapping({bytes}.as_ptr(), {data} as *mut u8, {length}); }} }}"
                ),
                plugin_write_memory(ptr, 0, "i32", &format!("{data}")),
                plugin_write_memory(ptr, 4, "i32", &format!("{length} as i32")),
            ];
            format!("{{ {} }}", statements.join(" "))
        }
        ComponentTypeDescriptor::List(inner) => {
            let element_ty = plugin_sequence_element(program, source_ty)
                .expect("component list source type has no element");
            let length = plugin_fresh(counter, "write_list_len");
            let bytes = plugin_fresh(counter, "write_list_bytes");
            let data = plugin_fresh(counter, "write_list_data");
            let index = plugin_fresh(counter, "write_list_index");
            let item = plugin_fresh(counter, "write_list_item");
            let element_ptr = plugin_fresh(counter, "write_list_element_ptr");
            let nested = plugin_memory_lower(
                program,
                element_ty,
                inner,
                &item,
                &element_ptr,
                counter,
                allocator,
            );
            format!(
                "{{ let {length} = ({value}).len(); let {bytes} = {length}.checked_mul({}).expect(\"canonical list size overflow\"); let {data} = {allocator}(0, 0, {}, {bytes} as i32); if {data} == 0 && {bytes} != 0 {{ panic!(\"canonical list allocation failed\") }}; for ({index}, {item}) in ({value}).iter().enumerate() {{ let {element_ptr} = ({data} as usize + {index} * {}) as i32; {nested} }} {} {} }}",
                plugin_memory_layout(inner).0,
                plugin_memory_layout(inner).1,
                plugin_memory_layout(inner).0,
                plugin_write_memory(ptr, 0, "i32", &format!("{data}")),
                plugin_write_memory(ptr, 4, "i32", &format!("{length} as i32")),
            )
        }
        ComponentTypeDescriptor::Option(inner) => {
            let payload_offset = plugin_align_to(1, plugin_memory_layout(inner).1);
            let child_ty = plugin_option_element(program, source_ty)
                .expect("component option source type has no element");
            let child_ptr = format!("(({ptr}) as usize + {payload_offset}) as i32");
            let child = plugin_memory_lower(
                program,
                child_ty,
                inner,
                "inner",
                &child_ptr,
                counter,
                allocator,
            );
            format!(
                "match {value} {{ Ok(inner) => {{ {} {} }}, Err(_) => {{ {} }} }}",
                plugin_write_memory(ptr, 0, "u8", "1u8"),
                child,
                plugin_write_memory(ptr, 0, "u8", "0u8"),
            )
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let (_, ok_align) = plugin_memory_layout(ok);
            let (_, err_align) = plugin_memory_layout(err);
            let payload_offset = plugin_align_to(1, ok_align.max(err_align));
            let (source_ok, source_err) = plugin_result_elements(program, source_ty)
                .expect("component result source type has no variants");
            let ok_ptr = format!("(({ptr}) as usize + {payload_offset}) as i32");
            let err_ptr = ok_ptr.clone();
            let ok_code = plugin_memory_lower(
                program,
                source_ok,
                ok,
                "inner",
                &ok_ptr,
                counter,
                allocator,
            );
            let err_code = plugin_memory_lower(
                program,
                source_err,
                err,
                "inner",
                &err_ptr,
                counter,
                allocator,
            );
            format!(
                "match {value} {{ Ok(inner) => {{ {} {} }}, Err(inner) => {{ {} {} }} }}",
                plugin_write_memory(ptr, 0, "u8", "0u8"),
                ok_code,
                plugin_write_memory(ptr, 0, "u8", "1u8"),
                err_code,
            )
        }
        ComponentTypeDescriptor::Record { fields, .. } => {
            let source_fields = plugin_record_fields(program, source_ty)
                .expect("component record source type has no fields");
            let offsets = plugin_record_offsets(fields);
            let mut statements = Vec::new();
            for ((index, _, field_descriptor), (offset, _)) in fields.iter().zip(offsets) {
                let field = source_fields
                    .get(*index)
                    .expect("component record field index is out of range");
                let field_value = format!("({value}).{}", mangle(&field.name));
                let field_ptr = format!("(({ptr}) as usize + {offset}) as i32");
                statements.push(plugin_memory_lower(
                    program,
                    &field.ty,
                    field_descriptor,
                    &format!("&({field_value})"),
                    &field_ptr,
                    counter,
                    allocator,
                ));
            }
            statements.join(" ")
        }
    }
}

fn plugin_memory_free(
    descriptor: &ComponentTypeDescriptor,
    ptr: &str,
    counter: &mut usize,
    allocator: &str,
) -> String {
    match descriptor {
        ComponentTypeDescriptor::Int
        | ComponentTypeDescriptor::Float
        | ComponentTypeDescriptor::Bool => String::new(),
        ComponentTypeDescriptor::String => {
            let data = plugin_fresh(counter, "free_string_data");
            let length = plugin_fresh(counter, "free_string_len");
            format!(
                "{{ let {data} = {}; let {length} = {}; if {data} != 0 && {length} != 0 {{ {allocator}({data}, {length}, 1, 0); }} }}",
                plugin_read_memory(ptr, 0, "i32"),
                plugin_read_memory(ptr, 4, "i32"),
            )
        }
        ComponentTypeDescriptor::List(inner) => {
            let data = plugin_fresh(counter, "free_list_data");
            let length = plugin_fresh(counter, "free_list_len");
            let index = plugin_fresh(counter, "free_list_index");
            let element_ptr = plugin_fresh(counter, "free_list_element_ptr");
            let child = plugin_memory_free(
                inner,
                &element_ptr,
                counter,
                allocator,
            );
            let (element_size, element_align) = plugin_memory_layout(inner);
            format!(
                "{{ let {data} = {}; let {length} = {}; if {data} != 0 && {length} != 0 {{ for {index} in 0..({length} as usize) {{ let {element_ptr} = ({data} as usize + {index} * {element_size}) as i32; {child} }} {allocator}({data}, ({length} as usize * {element_size}) as i32, {element_align}, 0); }} }}",
                plugin_read_memory(ptr, 0, "i32"),
                plugin_read_memory(ptr, 4, "i32"),
            )
        }
        ComponentTypeDescriptor::Option(inner) => {
            let tag = plugin_fresh(counter, "free_option_tag");
            let child_ptr = format!(
                "(({ptr}) as usize + {}) as i32",
                plugin_align_to(1, plugin_memory_layout(inner).1)
            );
            let child = plugin_memory_free(inner, &child_ptr, counter, allocator);
            format!(
                "{{ let {tag} = {}; match {tag} {{ 0 => {{}}, 1 => {{ {child} }}, _ => panic!(\"invalid canonical option discriminant\") }} }}",
                plugin_read_memory(ptr, 0, "u8"),
            )
        }
        ComponentTypeDescriptor::Result { ok, err } => {
            let tag = plugin_fresh(counter, "free_result_tag");
            let (_, ok_align) = plugin_memory_layout(ok);
            let (_, err_align) = plugin_memory_layout(err);
            let child_ptr = format!(
                "(({ptr}) as usize + {}) as i32",
                plugin_align_to(1, ok_align.max(err_align))
            );
            let ok_code = plugin_memory_free(ok, &child_ptr, counter, allocator);
            let err_code = plugin_memory_free(err, &child_ptr, counter, allocator);
            format!(
                "{{ let {tag} = {}; match {tag} {{ 0 => {{ {ok_code} }}, 1 => {{ {err_code} }}, _ => panic!(\"invalid canonical result discriminant\") }} }}",
                plugin_read_memory(ptr, 0, "u8"),
            )
        }
        ComponentTypeDescriptor::Record { fields, .. } => {
            let mut statements = Vec::new();
            for ((_, _, field), (offset, _)) in fields.iter().zip(plugin_record_offsets(fields)) {
                let field_ptr = format!("(({ptr}) as usize + {offset}) as i32");
                statements.push(plugin_memory_free(field, &field_ptr, counter, allocator));
            }
            statements.join(" ")
        }
    }
}

fn plugin_component_wrapper(
    program: &MirProgram,
    export: &super::Embedding::ExportFunction,
    component: &ComponentSignature,
    descriptor: &ComponentSignatureDescriptor,
    kebab: &str,
    wrapper_name: &str,
) -> String {
    let allocator = mangle_generated("plugin_cabi_realloc_impl");
    let read_text = mangle_generated("plugin_read_component_text");
    let parameter_slots = descriptor
        .params
        .iter()
        .flat_map(plugin_flat_slots)
        .collect::<Vec<_>>();
    let indirect_params = parameter_slots.len() > 16;
    let mut counter = 0;
    let mut locals = Vec::new();
    let mut cleanup = Vec::new();
    let mut call_args = Vec::new();
    if indirect_params {
        let params_ptr = "__jet_plugin_params_ptr";
        let (params_size, params_align) = plugin_sequence_layout(&descriptor.params);
        let mut offset = 0;
        for (index, (source_ty, descriptor_ty)) in component
            .params
            .iter()
            .zip(&descriptor.params)
            .enumerate()
        {
            let (_, field_align) = plugin_memory_layout(descriptor_ty);
            offset = plugin_align_to(offset, field_align);
            let field_ptr = format!("(({params_ptr}) as usize + {offset}) as i32");
            let value = plugin_memory_lift_expr(
                program,
                source_ty,
                descriptor_ty,
                &field_ptr,
                &mut counter,
                &read_text,
            );
            let mutable = matches!(export.params[index], MirAccess::Write)
                .then_some("mut ")
                .unwrap_or_default();
            locals.push(format!("let {mutable}p{index} = {value};"));
            offset += plugin_memory_layout(descriptor_ty).0;
        }
        let mut free_offset = 0;
        for descriptor_ty in &descriptor.params {
            let (_, field_align) = plugin_memory_layout(descriptor_ty);
            free_offset = plugin_align_to(free_offset, field_align);
            let field_ptr = format!("(({params_ptr}) as usize + {free_offset}) as i32");
            let free = plugin_memory_free(descriptor_ty, &field_ptr, &mut counter, &allocator);
            if !free.is_empty() {
                cleanup.push(free);
            }
            free_offset += plugin_memory_layout(descriptor_ty).0;
        }
        cleanup.push(format!("{allocator}({params_ptr}, {params_size}i32, {params_align}, 0);"));
    } else {
        let args = parameter_slots
            .iter()
            .enumerate()
            .map(|(index, _)| format!("a{index}"))
            .collect::<Vec<_>>();
        let mut cursor = 0;
        for (index, (source_ty, descriptor_ty)) in component
            .params
            .iter()
            .zip(&descriptor.params)
            .enumerate()
        {
            let value = plugin_flat_lift_expr(
                program,
                source_ty,
                descriptor_ty,
                &args,
                &parameter_slots,
                &mut cursor,
                &mut counter,
                &read_text,
            );
            let mutable = matches!(export.params[index], MirAccess::Write)
                .then_some("mut ")
                .unwrap_or_default();
            locals.push(format!("let {mutable}p{index} = {value};"));
        }
        assert_eq!(cursor, parameter_slots.len());
    }
    for (index, convention) in export.params.iter().enumerate() {
        call_args.push(match convention {
            MirAccess::Read => format!("&p{index}"),
            MirAccess::Write => format!("&mut p{index}"),
            MirAccess::Move => format!("p{index}"),
        });
    }
    let call = format!(
        "match {}({}) {{ Ok(value) => value, Err(error) => jet_entry_error_exit_jet(error) }}",
        export.callee,
        call_args.join(", ")
    );
    let result_slots = plugin_flat_slots(&descriptor.result);
    let mut wrapper = format!(
        "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}(",
    );
    if indirect_params {
        wrapper.push_str("__jet_plugin_params_ptr: i32");
    } else {
        wrapper.push_str(
            &parameter_slots
                .iter()
                .enumerate()
                .map(|(index, slot)| format!("a{index}: {}", slot.rust_ty()))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    wrapper.push_str(")");
    if result_slots.is_empty() {
        wrapper.push_str(" { ");
    } else if result_slots.len() == 1 {
        wrapper.push_str(&format!(" -> {} {{ ", result_slots[0].rust_ty()));
    } else {
        wrapper.push_str(" -> i32 { ");
    }
    wrapper.push_str(&locals.join(" "));
    wrapper.push_str(&format!(
        " let __jet_plugin_value = jet_ffi_callback_boundary(|| {{ {call} }});"
    ));
    wrapper.push_str(&cleanup.join(" "));
    if result_slots.is_empty() {
        wrapper.push_str(" let _ = __jet_plugin_value; () }");
    } else if result_slots.len() == 1 {
        wrapper.push_str(&format!(
            " {} }}",
            plugin_flat_lower_expr(
                program,
                &component.result,
                &descriptor.result,
                "&__jet_plugin_value",
            )
        ));
    } else {
        let (ret_size, ret_align) = plugin_memory_layout(&descriptor.result);
        let lower = plugin_memory_lower(
            program,
            &component.result,
            &descriptor.result,
            "&__jet_plugin_value",
            "__jet_plugin_ret_ptr",
            &mut counter,
            &allocator,
        );
        wrapper.push_str(&format!(
            " let __jet_plugin_ret_ptr = {allocator}(0, 0, {ret_align}, {ret_size}); if __jet_plugin_ret_ptr == 0 && {ret_size} != 0 {{ panic!(\"canonical component result allocation failed\") }} {lower} __jet_plugin_ret_ptr }}"
        ));
    }
    wrapper.push('\n');
    if result_slots.len() > 1 && plugin_needs_post_return(&descriptor.result) {
        let post_name = mangle(&format!("plugin_post_{}", export.name));
        let free = plugin_memory_free(
            &descriptor.result,
            "ret_ptr",
            &mut counter,
            &allocator,
        );
        let (ret_size, ret_align) = plugin_memory_layout(&descriptor.result);
        wrapper.push_str(&format!(
            "#[export_name = \"cabi_post_{kebab}\"]\npub extern \"C\" fn {post_name}(ret_ptr: i32) {{ if ret_ptr != 0 {{ {free} {allocator}(ret_ptr, {ret_size}i32, {ret_align}, 0); }} }}\n"
        ));
    }
    wrapper
}


/// The host operations named by a sandbox package's checked
/// `authority.needs`.  These are deliberately a closed projection: the
/// generated WIT world cannot grow an ambient import merely because a guest
/// happens to mention a similarly named function.
fn plugin_declared_host_imports(program: &MirProgram) -> Vec<&'static str> {
    let mut imports = BTreeSet::new();
    for need in &program.facts.authority_needs {
        let root = need.split_once(':').map_or(need.as_str(), |(root, _)| root);
        match root {
            "FS.Read" => {
                imports.insert("FS.Read");
            }
            "FS.Write" => {
                imports.insert("FS.Write");
            }
            "Net.Connect" => {
                imports.insert("Net.Connect");
            }
            "Exec" => {
                imports.insert("Exec");
            }
            _ => {}
        }
    }
    imports.into_iter().collect()
}

/// Emit the root WIT imports and the raw standard32 guest-side bridge.  The
/// Component Model owns the actual host adapter; this code only performs the
/// canonical pointer/length and return-area marshalling required by the
/// wasm32 core module.
fn plugin_host_import_bridge(imports: &[&str]) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let allocator = mangle_generated("plugin_cabi_realloc_impl");
    let read_component_text = mangle_generated("plugin_read_component_text");
    let read_io_error = mangle_generated("plugin_read_io_error");
    let free_io_error = mangle_generated("plugin_free_io_error");
    let mut out = String::from(
        "\n// D-PLUGIN1=B: declared host imports use the standard32 canonical ABI.\n\
         #[link(wasm_import_module = \"cm32p2\")]\n\
         extern \"C\" {\n",
    );
    for import in imports {
        match *import {
            "FS.Read" => out.push_str(
                "    #[link_name = \"host-read\"]\n\
                 fn __jet_plugin_import_read(path_ptr: i32, path_len: i32, ret_ptr: i32);\n",
            ),
            "FS.Write" => out.push_str(
                "    #[link_name = \"host-write\"]\n\
                 fn __jet_plugin_import_write(path_ptr: i32, path_len: i32, text_ptr: i32, text_len: i32, ret_ptr: i32);\n",
            ),
            "Net.Connect" => out.push_str(
                "    #[link_name = \"host-connect\"]\n\
                 fn __jet_plugin_import_connect(endpoint_ptr: i32, endpoint_len: i32) -> i32;\n",
            ),
            "Exec" => out.push_str(
                "    #[link_name = \"host-exec\"]\n\
                 fn __jet_plugin_import_exec(program_ptr: i32, program_len: i32, args_ptr: i32, args_len: i32, ret_ptr: i32);\n",
            ),
            _ => {}
        }
    }
    out.push_str("}\n");
    if imports.iter().any(|import| *import == "FS.Read") {
        out.push_str(&format!(
            r#"fn {wrapper}(path: &String) -> Result<String, jet_std::IOError> {{
    // result<string, io-error>: tag @0, payload @8, size 64, align 8.
    let ret_ptr = {allocator}(0, 0, 8, 64);
    if ret_ptr == 0 {{
        panic!("canonical host read return allocation failed")
    }}
    unsafe {{
        __jet_plugin_import_read(path.as_ptr() as i32, path.len() as i32, ret_ptr);
    }}
    let tag = unsafe {{ std::ptr::read_unaligned(ret_ptr as *const u8) }};
    let result = match tag {{
        0 => {{
            let (ptr, len) = unsafe {{
                (
                    std::ptr::read_unaligned((ret_ptr as usize + 8) as *const i32),
                    std::ptr::read_unaligned((ret_ptr as usize + 12) as *const i32),
                )
            }};
            let value = {read_component_text}(ptr, len);
            {allocator}(ptr, len, 1, 0);
            Ok(value)
        }}
        1 => {{
            let value = {read_io_error}((ret_ptr as usize + 8) as i32);
            {free_io_error}((ret_ptr as usize + 8) as i32);
            Err(value)
        }}
        _ => {{
            {allocator}(ret_ptr, 64, 8, 0);
            panic!("invalid canonical host read result discriminant")
        }}
    }};
    {allocator}(ret_ptr, 64, 8, 0);
    result
}}
"#,
            wrapper = mangle_generated("plugin_import_fs_read"),
            allocator = allocator,
            read_component_text = read_component_text,
            read_io_error = read_io_error,
            free_io_error = free_io_error,
        ));
    }
    if imports.iter().any(|import| *import == "FS.Write") {
        out.push_str(&format!(
            r#"fn {wrapper}(path: &String, text: &String) -> Result<(), jet_std::IOError> {{
    // result<_, io-error>: tag @0, payload @8, size 64, align 8.
    let ret_ptr = {allocator}(0, 0, 8, 64);
    if ret_ptr == 0 {{
        panic!("canonical host write return allocation failed")
    }}
    unsafe {{
        __jet_plugin_import_write(
            path.as_ptr() as i32,
            path.len() as i32,
            text.as_ptr() as i32,
            text.len() as i32,
            ret_ptr,
        );
    }}
    let tag = unsafe {{ std::ptr::read_unaligned(ret_ptr as *const u8) }};
    let result = match tag {{
        0 => Ok(()),
        1 => {{
            let value = {read_io_error}((ret_ptr as usize + 8) as i32);
            {free_io_error}((ret_ptr as usize + 8) as i32);
            Err(value)
        }}
        _ => {{
            {allocator}(ret_ptr, 64, 8, 0);
            panic!("invalid canonical host write result discriminant")
        }}
    }};
    {allocator}(ret_ptr, 64, 8, 0);
    result
}}
"#,
            wrapper = mangle_generated("plugin_import_fs_write"),
            allocator = allocator,
            read_io_error = read_io_error,
            free_io_error = free_io_error,
        ));
    }
    if imports.iter().any(|import| *import == "Net.Connect") {
        out.push_str(&format!(
            "fn {}(endpoint: &String) -> Result<bool, jet_std::IOError> {{\n\
             \x20    let connected = unsafe {{ __jet_plugin_import_connect(endpoint.as_ptr() as i32, endpoint.len() as i32) != 0 }};\n\
             \x20    Ok(connected)\n\
             }}\n",
            mangle_generated("plugin_import_net_connect"),
        ));
    }
    if imports.iter().any(|import| *import == "Exec") {
        out.push_str(&format!(
            "fn {}(program: &String, args: &Vec<String>) -> Result<String, jet_std::IOError> {{\n\
             \x20    let ret_ptr = {allocator}(0, 0, 4, 8);\n\
             \x20    if ret_ptr == 0 {{ panic!(\"canonical host exec return allocation failed\") }}\n\
             \x20    let args_bytes = args.len().checked_mul(8).expect(\"canonical host exec argument list is too large\");\n\
             \x20    let args_ptr = {allocator}(0, 0, 4, args_bytes as i32);\n\
             \x20    if args_ptr == 0 && args_bytes != 0 {{ panic!(\"canonical host exec argument allocation failed\") }}\n\
             \x20    for (index, argument) in args.iter().enumerate() {{\n\
             \x20        let bytes = argument.as_bytes();\n\
             \x20        let ptr = {allocator}(0, 0, 1, bytes.len() as i32);\n\
             \x20        if ptr == 0 && !bytes.is_empty() {{ panic!(\"canonical host exec argument allocation failed\") }}\n\
             \x20        if !bytes.is_empty() {{ unsafe {{ std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()); }} }}\n\
             \x20        unsafe {{\n\
             \x20            std::ptr::write_unaligned((args_ptr as usize + index * 8) as *mut i32, ptr);\n\
             \x20            std::ptr::write_unaligned((args_ptr as usize + index * 8 + 4) as *mut i32, bytes.len() as i32);\n\
             \x20        }}\n\
             \x20    }}\n\
             \x20    unsafe {{ __jet_plugin_import_exec(program.as_ptr() as i32, program.len() as i32, args_ptr, args.len() as i32, ret_ptr); }}\n\
             \x20    for index in 0..args.len() {{\n\
             \x20        let (ptr, len) = unsafe {{\n\
             \x20            (std::ptr::read_unaligned((args_ptr as usize + index * 8) as *const i32),\n\
             \x20             std::ptr::read_unaligned((args_ptr as usize + index * 8 + 4) as *const i32))\n\
             \x20        }};\n\
             \x20        {allocator}(ptr, len, 1, 0);\n\
             \x20    }}\n\
             \x20    {allocator}(args_ptr, args_bytes as i32, 4, 0);\n\
             \x20    let (ptr, len) = unsafe {{\n\
             \x20        (std::ptr::read_unaligned(ret_ptr as *const i32),\n\
             \x20         std::ptr::read_unaligned((ret_ptr as usize + 4) as *const i32))\n\
             \x20    }};\n\
             \x20    let value = {read_component_text}(ptr, len);\n\
             \x20    {allocator}(ptr, len, 1, 0);\n\
             \x20    {allocator}(ret_ptr, 8, 4, 0);\n\
             \x20    Ok(value)\n\
             }}\n",
            mangle_generated("plugin_import_exec"),
            allocator = allocator,
            read_component_text = read_component_text,
        ));
    }
    out
}
fn plugin_host_import_wit(import: &str) -> &'static str {
    match import {
        "FS.Read" => "  import host-read: func(path: string) -> result<string, io-error>;\n",
        "FS.Write" => "  import host-write: func(path: string, text: string) -> result<_, io-error>;\n",
        "Net.Connect" => "  import host-connect: func(endpoint: string) -> bool;\n",
        "Exec" => "  import host-exec: func(program: string, args: list<string>) -> string;\n",
        _ => "",
    }
}

fn plugin_host_io_wit_definitions() -> &'static str {
    "enum io-operation {\n\
     \x20 read,\n\
     \x20 write,\n\
     \x20 flush,\n\
     \x20 connect,\n\
     \x20 accept,\n\
     \x20 close,\n\
     \x20 resolve,\n\
     \x20 codec,\n\
     }\n\
     record io-context {\n\
     \x20 operation: io-operation,\n\
     \x20 resource: option<string>,\n\
     \x20 os-code: option<s64>,\n\
     \x20 cause: option<string>,\n\
     }\n\
     enum process-resource-limit {\n\
     \x20 wall-time,\n\
     \x20 cpu-time,\n\
     \x20 memory,\n\
     \x20 open-files,\n\
     \x20 output,\n\
     }\n\
     variant io-error {\n\
     \x20 invalid-input(io-context),\n\
     \x20 not-found(io-context),\n\
     \x20 permission-denied(io-context),\n\
     \x20 timed-out(io-context),\n\
     \x20 cancelled(io-context),\n\
     \x20 closed(io-context),\n\
     \x20 protocol(io-context),\n\
     \x20 other(io-context),\n\
     \x20 resource-limit(process-resource-limit),\n\
     }\n"
}

/// Build the guest artifacts from a checked `SandboxPlugin` MIR artifact.
pub fn emit_plugin(
    program: &MirProgram,
    artifact_id: MirArtifactId,
    whole_program_rust: &str,
) -> PluginArtifacts {
    if let Err(error) = program.cffi.validate_boundaries() {
        panic!("MIR Plugin emission received invalid foreign boundary facts: {error}");
    }
    let artifact = super::Embedding::selected_artifact(program, artifact_id);
    if artifact.kind != MirArtifactKind::SandboxPlugin
        || artifact.target != MirArtifactTarget::RustAot
    {
        panic!(
            "MIR Plugin emission requires a RustAot SandboxPlugin artifact, got {:?}/{:?}",
            artifact.target, artifact.kind
        );
    }
    let sanitized = sanitize_package_name(&artifact.name);
    let exports = super::Embedding::export_surface(program, artifact_id);

    let mut wit_lines = Vec::new();
    let mut wit_definitions = Vec::new();
    let mut wit_defined = BTreeSet::new();
    let mut wrapper_fns = String::new();
    let mut has_text_export = false;
    let mut has_component_export = false;
    let plugin_read_text = mangle_generated("plugin_read_text");
    let plugin_return_text = mangle_generated("plugin_return_text");
    let plugin_post_text = mangle_generated("plugin_post_text");
    let host_imports = plugin_declared_host_imports(program);
    if host_imports
        .iter()
        .any(|import| matches!(*import, "FS.Read" | "FS.Write"))
    {
        wit_definitions.push(plugin_host_io_wit_definitions().to_string());
    }
    for import in &host_imports {
        wit_lines.push(plugin_host_import_wit(import).trim_end_matches('\n').to_string());
    }

    for export in &exports {
        let kebab = to_kebab(&export.symbol);
        let wrapper_name = mangle(&format!("plugin_export_{}", export.name));
        let callee = export.callee.as_str();
        if let Some(scalar) = export.scalar {
            let wit_params: Vec<String> = export
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
            if scalar == ExportScalar::Text {
                has_text_export = true;
                let rust_params: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .flat_map(|(i, _)| [format!("p{i}_ptr: i32"), format!("p{i}_len: i32")])
                    .collect();
                let locals: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, convention)| {
                        let mutable = matches!(convention, MirAccess::Write)
                            .then_some("mut ")
                            .unwrap_or_default();
                        format!("let {mutable}p{i} = {plugin_read_text}(p{i}_ptr, p{i}_len);")
                    })
                    .collect();
                let call_args: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, convention)| match convention {
                        MirAccess::Read => format!("&p{i}"),
                        MirAccess::Write => format!("&mut p{i}"),
                        MirAccess::Move => format!("p{i}"),
                    })
                    .collect();
                let call = format!(
                    "match {}({}) {{ Ok(value) => value, Err(error) => jet_entry_error_exit_jet(error) }}",
                    callee,
                    call_args.join(", ")
                );
                wrapper_fns.push_str(&format!(
                    "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}({rust_params}) -> i32 {{ {locals} jet_ffi_callback_boundary(|| {{ {plugin_return_text}({call}) }}) }}\n#[export_name = \"cabi_post_{kebab}\"]\npub extern \"C\" fn {post_name}(ret_ptr: i32) {{ {plugin_post_text}(ret_ptr) }}\n",
                    wrapper_name = wrapper_name,
                    rust_params = rust_params.join(", "),
                    locals = locals.join(" "),
                    call = call,
                    post_name = mangle(&format!("plugin_post_{}", export.name)),
                    plugin_return_text = plugin_return_text,
                    plugin_post_text = plugin_post_text,
                ));
            } else {
                let rust_params: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("p{i}: {}", scalar.rust_ty()))
                    .collect();
                let locals: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(_, convention)| matches!(convention, MirAccess::Write))
                    .map(|(i, _)| format!("let mut p{i} = p{i};"))
                    .collect();
                let call_args: Vec<String> = export
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, convention)| match convention {
                        MirAccess::Write => format!("&mut p{i}"),
                        MirAccess::Read | MirAccess::Move => format!("p{i}"),
                    })
                    .collect();
                let call = format!(
                    "match {}({}) {{ Ok(value) => value, Err(error) => jet_entry_error_exit_jet(error) }}",
                    callee,
                    call_args.join(", ")
                );
                wrapper_fns.push_str(&format!(
                    "#[export_name = \"{kebab}\"]\npub extern \"C\" fn {wrapper_name}({rust_params}) -> {ret} {{ {locals} jet_ffi_callback_boundary(|| {{ {call} }}) }}\n",
                    wrapper_name = wrapper_name,
                    rust_params = rust_params.join(", "),
                    ret = scalar.rust_ty(),
                    locals = locals.join(" "),
                    call = call,
                ));
            }
            continue;
        }

        let component = export
            .component
            .as_ref()
            .expect("component export row has no checked signature");
        let descriptor = super::Embedding::component_descriptor(program, component)
            .expect("component export row has no checked descriptor");
        has_component_export = true;
        for definition in descriptor.wit_definitions() {
            let name = definition
                .strip_prefix("record ")
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or_default()
                .to_string();
            if wit_defined.insert(name) {
                wit_definitions.push(definition);
            }
        }
        let wit_params: Vec<String> = descriptor
            .params
            .iter()
            .enumerate()
            .map(|(i, ty)| format!("p{i}: {}", ty.wit_type()))
            .collect();
        let wit_result = descriptor.result.wit_type();
        wit_lines.push(format!(
            "  export {kebab}: func({}) -> {wit_result};",
            wit_params.join(", ")
        ));
        wrapper_fns.push_str(&plugin_component_wrapper(
            program,
            export,
            component,
            &descriptor,
            &kebab,
            &wrapper_name,
        ));
    }

    let wit = format!(
        "package jet:{sanitized}@0.1.0;\n\n{}\nworld {WORLD_NAME} {{\n{}\n}}\n",
        wit_definitions.join("\n"),
        wit_lines.join("\n")
    );

    let mut guest_rust = String::from(whole_program_rust);
    // Sandbox guests use the ordinary Prelude's Time implementation too; add
    // the complete wasm TZif closure that browser emission already embeds.
    guest_rust.push_str(
        "\n// c81 / D-PLUGIN-EXPORT1=A: generated export wrappers (jet-codegen/Plugin.rs).\n",
    );
    if has_text_export || has_component_export || !host_imports.is_empty() {
        guest_rust.push_str(&plugin_text_helpers());
    }
    if host_imports
        .iter()
        .any(|import| matches!(*import, "FS.Read" | "FS.Write"))
    {
        guest_rust.push_str(&plugin_io_error_helpers());
    }
    guest_rust.push_str(&plugin_host_import_bridge(&host_imports));
    guest_rust.push_str(&wrapper_fns);

    PluginArtifacts {
        wit,
        guest_rust,
        world_name: WORLD_NAME.to_string(),
        export_name: sanitized,
        exported_fns: exports.iter().map(|export| export.name.clone()).collect(),
        exports,
    }
}

fn plugin_io_error_helpers() -> String {
    let allocator = mangle_generated("plugin_cabi_realloc_impl");
    let read_component_text = mangle_generated("plugin_read_component_text");
    let read_io_error = mangle_generated("plugin_read_io_error");
    let free_io_error = mangle_generated("plugin_free_io_error");
    PLUGIN_IO_ERROR_HELPERS
        .replace("JET_PLUGIN_CABI_REALLOC_IMPL", &allocator)
        .replace("JET_PLUGIN_READ_COMPONENT_TEXT", &read_component_text)
        .replace("JET_PLUGIN_READ_IO_ERROR", &read_io_error)
        .replace("JET_PLUGIN_FREE_IO_ERROR", &free_io_error)
}

const PLUGIN_IO_ERROR_HELPERS: &str = r#"
// result<string, io-error> and result<_, io-error> use the same canonical
// return area: one u8 discriminant at byte 0, payload at byte 8, size 64,
// alignment 8. The io-error payload is a variant of size 56/alignment 8.
fn JET_PLUGIN_READ_IO_ERROR(ptr: i32) -> jet_std::IOError {
    let tag = unsafe { std::ptr::read_unaligned(ptr as *const u8) };
    if tag <= 7 {
        // io-context: operation @0; option<string> resource @4; option<s64>
        // os-code @16; option<string> cause @32; total size 48/alignment 8.
        let context_ptr = (ptr as usize + 8) as i32;
        let operation = match unsafe {
            std::ptr::read_unaligned(context_ptr as *const u8)
        } {
            0 => jet_std::IOOperation::Read,
            1 => jet_std::IOOperation::Write,
            2 => jet_std::IOOperation::Flush,
            3 => jet_std::IOOperation::Connect,
            4 => jet_std::IOOperation::Accept,
            5 => jet_std::IOOperation::Close,
            6 => jet_std::IOOperation::Resolve,
            7 => jet_std::IOOperation::Codec,
            _ => panic!("invalid canonical io-operation discriminant"),
        };
        let resource = match unsafe {
            std::ptr::read_unaligned((context_ptr as usize + 4) as *const u8)
        } {
            0 => Err(JetAbsent),
            1 => {
                let ptr = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 8) as *const i32)
                };
                let len = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 12) as *const i32)
                };
                Ok(JET_PLUGIN_READ_COMPONENT_TEXT(ptr, len))
            }
            _ => panic!("invalid canonical io-context resource discriminant"),
        };
        let os_code = match unsafe {
            std::ptr::read_unaligned((context_ptr as usize + 16) as *const u8)
        } {
            0 => Err(JetAbsent),
            1 => {
                let value = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 24) as *const i64)
                };
                Ok(value)
            }
            _ => panic!("invalid canonical io-context os-code discriminant"),
        };
        let cause = match unsafe {
            std::ptr::read_unaligned((context_ptr as usize + 32) as *const u8)
        } {
            0 => Err(JetAbsent),
            1 => {
                let ptr = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 36) as *const i32)
                };
                let len = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 40) as *const i32)
                };
                Ok(JET_PLUGIN_READ_COMPONENT_TEXT(ptr, len))
            }
            _ => panic!("invalid canonical io-context cause discriminant"),
        };
        let context = jet_std::IOContext {
            operation,
            resource,
            os_code,
            cause,
        };
        match tag {
            0 => jet_std::IOError::InvalidInput(context),
            1 => jet_std::IOError::NotFound(context),
            2 => jet_std::IOError::PermissionDenied(context),
            3 => jet_std::IOError::TimedOut(context),
            4 => jet_std::IOError::Cancelled(context),
            5 => jet_std::IOError::Closed(context),
            6 => jet_std::IOError::Protocol(context),
            7 => jet_std::IOError::Other(context),
            _ => unreachable!(),
        }
    } else if tag == 8 {
        let limit = match unsafe {
            std::ptr::read_unaligned((ptr as usize + 8) as *const u8)
        } {
            0 => jet_std::ProcessResourceLimit::WallTime,
            1 => jet_std::ProcessResourceLimit::CpuTime,
            2 => jet_std::ProcessResourceLimit::Memory,
            3 => jet_std::ProcessResourceLimit::OpenFiles,
            4 => jet_std::ProcessResourceLimit::Output,
            _ => panic!("invalid canonical process-resource-limit discriminant"),
        };
        jet_std::IOError::ResourceLimit(limit)
    } else {
        panic!("invalid canonical io-error discriminant")
    }
}

fn JET_PLUGIN_FREE_IO_ERROR(ptr: i32) {
    let tag = unsafe { std::ptr::read_unaligned(ptr as *const u8) };
    if tag <= 7 {
        let context_ptr = (ptr as usize + 8) as i32;
        let resource_tag = unsafe {
            std::ptr::read_unaligned((context_ptr as usize + 4) as *const u8)
        };
        match resource_tag {
            0 => {}
            1 => {
                let value_ptr = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 8) as *const i32)
                };
                let value_len = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 12) as *const i32)
                };
                JET_PLUGIN_CABI_REALLOC_IMPL(value_ptr, value_len, 1, 0);
            }
            _ => panic!("invalid canonical io-context resource discriminant"),
        }
        let cause_tag = unsafe {
            std::ptr::read_unaligned((context_ptr as usize + 32) as *const u8)
        };
        match cause_tag {
            0 => {}
            1 => {
                let value_ptr = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 36) as *const i32)
                };
                let value_len = unsafe {
                    std::ptr::read_unaligned((context_ptr as usize + 40) as *const i32)
                };
                JET_PLUGIN_CABI_REALLOC_IMPL(value_ptr, value_len, 1, 0);
            }
            _ => panic!("invalid canonical io-context cause discriminant"),
        }
    } else if tag == 8 {
        let limit = unsafe {
            std::ptr::read_unaligned((ptr as usize + 8) as *const u8)
        };
        if limit > 4 {
            panic!("invalid canonical process-resource-limit discriminant")
        }
    } else {
        panic!("invalid canonical io-error discriminant")
    }
}
"#;

fn plugin_text_helpers() -> String {
    let cabi_realloc_impl = mangle_generated("plugin_cabi_realloc_impl");
    let cabi_realloc = mangle_generated("plugin_cabi_realloc");
    let read_text = mangle_generated("plugin_read_text");
    let read_component_text = mangle_generated("plugin_read_component_text");
    let return_text = mangle_generated("plugin_return_text");
    let post_text = mangle_generated("plugin_post_text");
    PLUGIN_TEXT_HELPERS
        .replace("JET_PLUGIN_CABI_REALLOC_IMPL", &cabi_realloc_impl)
        .replace("JET_PLUGIN_CABI_REALLOC", &cabi_realloc)
        .replace("JET_PLUGIN_READ_TEXT", &read_text)
        .replace("JET_PLUGIN_READ_COMPONENT_TEXT", &read_component_text)
        .replace("JET_PLUGIN_RETURN_TEXT", &return_text)
        .replace("JET_PLUGIN_POST_TEXT", &post_text)
}

const PLUGIN_TEXT_HELPERS: &str = r#"
fn JET_PLUGIN_CABI_REALLOC_IMPL(
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
pub extern "C" fn JET_PLUGIN_CABI_REALLOC(
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
) -> i32 {
    JET_PLUGIN_CABI_REALLOC_IMPL(old_ptr, old_len, align, new_len)
}

fn JET_PLUGIN_READ_TEXT(ptr: i32, len: i32) -> String {
    if ptr == 0 || len == 0 {
        return String::new();
    }
    unsafe {
        String::from_utf8_lossy(std::slice::from_raw_parts(ptr as *const u8, len as usize))
            .into_owned()
    }
}

fn JET_PLUGIN_READ_COMPONENT_TEXT(ptr: i32, len: i32) -> String {
    if ptr == 0 || len == 0 {
        return String::new();
    }
    unsafe {
        String::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len as usize).to_vec())
            .unwrap_or_else(|_| panic!("invalid canonical UTF-8 string"))
    }
}

fn JET_PLUGIN_RETURN_TEXT(value: String) -> i32 {
    let bytes = value.into_bytes();
    let len = bytes.len();
    let ptr = JET_PLUGIN_CABI_REALLOC_IMPL(0, 0, 1, len as i32);
    if ptr == 0 && len != 0 {
        panic!("canonical string allocation failed");
    }
    if len != 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, len);
        }
    }
    let result = JET_PLUGIN_CABI_REALLOC_IMPL(0, 0, 4, 8);
    if result == 0 {
        if ptr != 0 && len != 0 {
            if let Ok(layout) = std::alloc::Layout::from_size_align(len, 1) {
                unsafe { std::alloc::dealloc(ptr as *mut u8, layout) };
            }
        }
        panic!("canonical string return area allocation failed");
    }
    unsafe {
        std::ptr::write_unaligned(result as *mut i32, ptr);
        std::ptr::write_unaligned((result as usize + 4) as *mut i32, len as i32);
    }
    result
}

fn JET_PLUGIN_POST_TEXT(ret_ptr: i32) {
    if ret_ptr == 0 {
        return;
    }
    let (ptr, len) = unsafe {
        (
            std::ptr::read_unaligned(ret_ptr as *const i32),
            std::ptr::read_unaligned((ret_ptr as usize + 4) as *const i32),
        )
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
"#;
