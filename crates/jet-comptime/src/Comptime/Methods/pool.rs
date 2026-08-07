//! D-POOLID-API1=A: resident tier-0 `Pool<T>` generational-arena state.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};

use super::super::Diagnostics::unsupported;

pub(super) struct PoolOutcome {
    pub(super) value: CtValue,
    pub(super) updated: Option<CtValue>,
}

pub(super) fn new_value() -> CtValue {
    pool_value(Vec::new(), Vec::new())
}

pub(super) fn is_method(recv: &CtValue, method: &str) -> bool {
    matches!(
        (recv, method),
        (
            CtValue::Struct { type_name, .. },
            "add" | "remove" | "ids"
        ) if type_name == crate::Syntax::MEM_POOL
    )
}

pub(super) fn apply(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    resolved_ret: Option<&Type>,
    span: Span,
) -> Result<PoolOutcome, Diagnostic> {
    if !is_method(recv, method) {
        return Err(unsupported("this Pool method", span));
    }
    let CtValue::Struct { type_name, fields } = recv else {
        unreachable!("is_method accepted only Pool structs")
    };
    debug_assert_eq!(type_name, crate::Syntax::MEM_POOL);
    let mut slots = list_field(fields, "slots");
    let mut free = list_field(fields, "free");
    let mut changed = false;
    let value = match method {
        "add" => {
            let value = args.first()
                .cloned()
                .ok_or_else(|| unsupported("Pool.add without a value", span))?;
            let (index, generation) = match free.pop() {
                Some(CtValue::Int(index)) => {
                    let index = usize::try_from(index)
                        .map_err(|_| unsupported("this Pool state", span))?;
                    let generation = slots.get(index)
                        .and_then(|slot| slot_generation(slot, "Vacant"))
                        .ok_or_else(|| unsupported("this Pool state", span))?;
                    slots[index] = occupied(generation, value);
                    (index, generation)
                }
                Some(_) => return Err(unsupported("this Pool state", span)),
                None => {
                    let index = slots.len();
                    slots.push(occupied(0, value));
                    (index, 0)
                }
            };
            changed = true;
            id(index, generation)
        }
        "remove" => {
            let removed = args.first().and_then(id_parts).and_then(|(index, generation)| {
                let current = slots.get(index)?;
                (slot_generation(current, "Occupied") == Some(generation))
                    .then_some((index, generation))
            });
            match removed {
                Some((index, generation)) => {
                    let old = std::mem::replace(
                        &mut slots[index],
                        vacant(generation.wrapping_add(1)),
                    );
                    free.push(CtValue::Int(index as i64));
                    changed = true;
                    let CtValue::Enum { mut args, .. } = old else {
                        unreachable!("validated occupied Pool slot")
                    };
                    CtValue::Present(Box::new(args.swap_remove(1).1))
                }
                None => CtValue::absent(match resolved_ret {
                    Some(Type::Option(inner)) => (**inner).clone(),
                    _ => Type::Int,
                }),
            }
        }
        "ids" => CtValue::List(slots.iter().enumerate().filter_map(|(index, slot)| {
            slot_generation(slot, "Occupied").map(|generation| id(index, generation))
        }).collect()),
        _ => return Err(unsupported("this Pool method", span)),
    };
    Ok(PoolOutcome {
        value,
        updated: changed.then(|| pool_value(slots, free)),
    })
}

fn pool_value(slots: Vec<CtValue>, free: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: crate::Syntax::MEM_POOL.to_string(),
        fields: vec![
            ("slots".to_string(), CtValue::List(slots)),
            ("free".to_string(), CtValue::List(free)),
        ],
    }
}

fn list_field(fields: &[(String, CtValue)], wanted: &str) -> Vec<CtValue> {
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        (name, CtValue::List(values)) if name == wanted => Some(values.clone()),
        _ => None,
    }).unwrap_or_default()
}

fn id(index: usize, generation: u32) -> CtValue {
    CtValue::Struct {
        type_name: "Id".to_string(),
        fields: vec![
            ("index".to_string(), CtValue::Int(index as i64)),
            ("generation".to_string(), CtValue::Int(i64::from(generation))),
        ],
    }
}

fn id_parts(value: &CtValue) -> Option<(usize, u32)> {
    let CtValue::Struct { type_name, fields } = value else { return None };
    if type_name != "Id" { return None }
    let int_field = |wanted: &str| fields.iter().find_map(|(name, value)| match value {
        CtValue::Int(value) if name == wanted => Some(*value),
        _ => None,
    });
    Some((
        usize::try_from(int_field("index")?).ok()?,
        u32::try_from(int_field("generation")?).ok()?,
    ))
}

fn occupied(generation: u32, value: CtValue) -> CtValue {
    CtValue::Enum {
        type_name: "__PoolSlot".to_string(),
        variant: "Occupied".to_string(),
        args: vec![(None, CtValue::Int(i64::from(generation))), (None, value)],
    }
}

fn vacant(generation: u32) -> CtValue {
    CtValue::Enum {
        type_name: "__PoolSlot".to_string(),
        variant: "Vacant".to_string(),
        args: vec![(None, CtValue::Int(i64::from(generation)))],
    }
}

fn slot_generation(slot: &CtValue, wanted: &str) -> Option<u32> {
    let CtValue::Enum { variant, args, .. } = slot else { return None };
    if variant != wanted { return None }
    match args.first().map(|(_, value)| value) {
        Some(CtValue::Int(generation)) => u32::try_from(*generation).ok(),
        _ => None,
    }
}
