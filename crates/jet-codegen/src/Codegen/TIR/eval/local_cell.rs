use crate::Comptime::CtValue;
use crate::local_cell::{
    JetCell, JetCellEditGuard, JetCellOptionLike, JetCellReadGuard,
};
use std::collections::HashSet;
use std::rc::Rc;

struct GuardSlot<G> {
    guard: Option<Rc<G>>,
    owner: u64,
}

/// D-LOCALCELL1=A: evaluator-local handles backed by the exact Prelude runtime.
///
/// This registry is intentionally owned by one evaluator context, not the
/// task-shared `EvalRuntime`: sema forbids every Cell handle from crossing into
/// a spawned evaluator.
pub(super) struct EvalLocalCells {
    cells: Vec<JetCell<CtValue>>,
    read_guards: Vec<GuardSlot<JetCellReadGuard<CtValue>>>,
    edit_guards: Vec<GuardSlot<JetCellEditGuard<CtValue>>>,
    frames: Vec<u64>,
    next_frame: u64,
}

impl EvalLocalCells {
    pub(super) fn new() -> Self {
        Self {
            cells: Vec::new(),
            read_guards: Vec::new(),
            edit_guards: Vec::new(),
            frames: Vec::new(),
            next_frame: 1,
        }
    }

    pub(super) fn enter_frame(&mut self) {
        let frame = self.next_frame;
        self.next_frame += 1;
        self.frames.push(frame);
    }

    pub(super) fn leave_frame(&mut self, returned: &CtValue) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let owner = self.frames.last().copied().unwrap_or(0);
        let mut escaped_reads = HashSet::new();
        let mut escaped_edits = HashSet::new();
        collect_guard_handles(returned, &mut escaped_reads, &mut escaped_edits);
        for (index, slot) in self.read_guards.iter_mut().enumerate() {
            if slot.owner == frame {
                if escaped_reads.contains(&index) {
                    slot.owner = owner;
                } else {
                    slot.guard = None;
                }
            }
        }
        for (index, slot) in self.edit_guards.iter_mut().enumerate() {
            if slot.owner == frame {
                if escaped_edits.contains(&index) {
                    slot.owner = owner;
                } else {
                    slot.guard = None;
                }
            }
        }
    }

    pub(super) fn insert_cell(&mut self, value: CtValue) -> usize {
        let index = self.cells.len();
        self.cells.push(JetCell::new(value));
        index
    }

    pub(super) fn cell(&self, index: usize) -> Option<JetCell<CtValue>> {
        self.cells.get(index).cloned()
    }

    pub(super) fn insert_read_guard(&mut self, guard: JetCellReadGuard<CtValue>) -> usize {
        self.insert_read_guard_for(guard, self.frames.last().copied().unwrap_or(0))
    }

    pub(super) fn insert_read_guard_for(
        &mut self,
        guard: JetCellReadGuard<CtValue>,
        owner: u64,
    ) -> usize {
        let index = self.read_guards.len();
        self.read_guards.push(GuardSlot {
            guard: Some(Rc::new(guard)),
            owner,
        });
        index
    }

    pub(super) fn read_guard(
        &self,
        index: usize,
    ) -> Option<Rc<JetCellReadGuard<CtValue>>> {
        self.read_guards.get(index)?.guard.as_ref().cloned()
    }

    pub(super) fn take_read_guard(
        &mut self,
        index: usize,
    ) -> Result<(JetCellReadGuard<CtValue>, u64), String> {
        let slot = self
            .read_guards
            .get_mut(index)
            .ok_or_else(|| "Cell read guard is no longer live".to_string())?;
        let guard = slot
            .guard
            .take()
            .ok_or_else(|| "Cell read guard is no longer live".to_string())?;
        let owner = slot.owner;
        Rc::try_unwrap(guard)
            .map(|guard| (guard, owner))
            .map_err(|_| "Cell read guard is active in another callback".to_string())
    }

    pub(super) fn insert_edit_guard(&mut self, guard: JetCellEditGuard<CtValue>) -> usize {
        self.insert_edit_guard_for(guard, self.frames.last().copied().unwrap_or(0))
    }

    pub(super) fn insert_edit_guard_for(
        &mut self,
        guard: JetCellEditGuard<CtValue>,
        owner: u64,
    ) -> usize {
        let index = self.edit_guards.len();
        self.edit_guards.push(GuardSlot {
            guard: Some(Rc::new(guard)),
            owner,
        });
        index
    }

    pub(super) fn edit_guard(
        &self,
        index: usize,
    ) -> Option<Rc<JetCellEditGuard<CtValue>>> {
        self.edit_guards.get(index)?.guard.as_ref().cloned()
    }

    pub(super) fn take_edit_guard(
        &mut self,
        index: usize,
    ) -> Result<(JetCellEditGuard<CtValue>, u64), String> {
        let slot = self
            .edit_guards
            .get_mut(index)
            .ok_or_else(|| "Cell edit guard is no longer live".to_string())?;
        let guard = slot
            .guard
            .take()
            .ok_or_else(|| "Cell edit guard is no longer live".to_string())?;
        let owner = slot.owner;
        Rc::try_unwrap(guard)
            .map(|guard| (guard, owner))
            .map_err(|_| "Cell edit guard is active in another callback".to_string())
    }
}

impl JetCellOptionLike for CtValue {
    type Value = CtValue;

    fn value(&self) -> Option<&CtValue> {
        match self {
            CtValue::Some(value) => Some(value),
            CtValue::None(_) => None,
            _ => None,
        }
    }

    fn store(&mut self, value: CtValue) {
        *self = CtValue::Some(Box::new(value));
    }
}

fn collect_guard_handles(
    value: &CtValue,
    reads: &mut HashSet<usize>,
    edits: &mut HashSet<usize>,
) {
    match value {
        CtValue::Struct { type_name, fields } => {
            if type_name == "__JetTirCellReadGuard" {
                if let Some(index) = internal_index(fields) {
                    reads.insert(index);
                }
            } else if type_name == "__JetTirCellEditGuard" {
                if let Some(index) = internal_index(fields) {
                    edits.insert(index);
                }
            } else {
                for (_, field) in fields {
                    collect_guard_handles(field, reads, edits);
                }
            }
        }
        CtValue::List(values) => {
            for value in values {
                collect_guard_handles(value, reads, edits);
            }
        }
        CtValue::Map(values) => {
            for value in values.values() {
                collect_guard_handles(value, reads, edits);
            }
        }
        CtValue::Enum { args, .. } => {
            for (_, value) in args {
                collect_guard_handles(value, reads, edits);
            }
        }
        CtValue::Some(value) | CtValue::ResOk(value) | CtValue::ResErr(value) => {
            collect_guard_handles(value, reads, edits);
        }
        CtValue::Closure(closure) => {
            for value in closure.captured.values() {
                collect_guard_handles(value, reads, edits);
            }
        }
        CtValue::Int(_)
        | CtValue::Float(_)
        | CtValue::Bool(_)
        | CtValue::Char(_)
        | CtValue::Str(_)
        | CtValue::BigInt(_)
        | CtValue::Bytes(_)
        | CtValue::None(_)
        | CtValue::Unit => {}
    }
}

pub(super) fn internal_index(fields: &[(String, CtValue)]) -> Option<usize> {
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
        _ => None,
    })
}

pub(super) fn project_ref<'a>(
    value: &'a CtValue,
    path: &[String],
) -> Option<&'a CtValue> {
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

pub(super) fn project_mut<'a>(
    value: &'a mut CtValue,
    path: &[String],
) -> Option<&'a mut CtValue> {
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

/// Borrow two sema-proved disjoint field paths without raw pointers. Equal or
/// prefix-overlapping paths return `None`; sema has already rejected them for
/// edit guards, so that result indicates corrupted TIR rather than user input.
pub(super) fn project_pair_mut<'a>(
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

    let first_index = fields
        .iter()
        .position(|(name, _)| name == first_field)?;
    let second_index = fields
        .iter()
        .position(|(name, _)| name == second_field)?;
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
