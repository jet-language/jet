//! Last uses move (D-MEM-COPYSEM1, #3714).
//!
//! Lowering reads an owned local or one of its fields with `ReadPlace`,
//! which every backend materializes as a copy, and a sema-checked `^x` or
//! `^x.field` reaches lowering as that same bare read. When the read's value
//! is consumed (stored, returned, wrapped, or passed to an owning parameter)
//! and no later code can observe the place, the copy and the value are
//! indistinguishable, so this pass turns the read into `MovePlace`. A whole
//! local's drop flag clears, exactly as an explicit move lowers; a field
//! moves only when a write later in the same block refills it (`^x.f`
//! followed by `x.f = ...`), so its owner is whole again before anything,
//! its own cleanup included, reads it. A read that only inspects its value
//! (`list.len()`) stays a read.
//! The same liveness fact lets a match arm take the payloads of a subject
//! value nothing reads after the arm binds them: a `Move` of the subject
//! feeds those `EnumPayload`s, which a backend extracts by value.
//!
//! Borrows, views and copies keep a place alive. Every use of a value that
//! borrows or views a place, directly or through further borrows, counts as
//! a use of the place, and a borrow or view stored anywhere keeps every read
//! of the place a copy. A backend may serve a borrowed consumer of a copy by
//! reading the place itself at that consumer, so the place holds its value
//! up to every use of every other copy of it.
//!
//! One backward liveness pass over the function's blocks decides all of it;
//! the pass is linear in instructions times the candidate bitset width.

use std::collections::{HashMap, HashSet};

use jet_foundation::AST::Type;
use jet_foundation::MIR::{
    MirAccess, MirBlockId, MirCallArg, MirCaptureOperand, MirConstant, MirCopyFact, MirFieldId,
    MirInstruction, MirLocalId, MirOpId, MirOperation, MirOwnershipMode, MirPlaceBase, MirPlaceId,
    MirPreludeCall, MirPreludeCallId, MirProjection, MirSemanticOp, MirTerminator, MirType,
    MirTypeId, MirTypeKind, MirValueId, stable_id,
};

use super::mir::{LowerCtx, LowerError, checked_operation_place_refs, retain_place_access};

#[derive(Clone, Copy)]
enum Candidate {
    /// An owned local: its whole place and its drop flag.
    Local { place: MirPlaceId, flag: MirPlaceId },
    /// A field path (`paths`) of the owned local candidate `root`.
    Field { root: usize },
    /// An enum subject value whose payloads one block binds.
    Subject { value: MirValueId },
    /// A copy (`ReadPlace` result) used outside its own block: it keeps the
    /// candidates its place observes alive up to each such use.
    Copy { value: MirValueId },
}

/// The storage a whole candidate place names: an owned local, or an owned
/// (`^`) parameter, whose scope-end drop is guarded by a live flag the same
/// way.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Root {
    Local(MirLocalId),
    Parameter(MirValueId),
}

fn place_root(base: &MirPlaceBase) -> Option<Root> {
    match base {
        MirPlaceBase::Local(local) => Some(Root::Local(*local)),
        MirPlaceBase::Parameter(value) => Some(Root::Parameter(*value)),
        _ => None,
    }
}

/// The field path of a place whose projections are all record fields.
fn field_path(projections: &[MirProjection]) -> Option<Vec<MirFieldId>> {
    projections
        .iter()
        .map(|projection| match projection {
            MirProjection::Field { field, .. } => Some(*field),
            _ => None,
        })
        .collect()
}

/// Whether a place of one root, given by its projections, may share storage
/// with the field `path` of that root: only a different field at the same
/// depth keeps them apart.
fn overlaps(projections: &[MirProjection], path: &[MirFieldId]) -> bool {
    for (projection, field) in projections.iter().zip(path) {
        match projection {
            MirProjection::Field { field: own, .. } if own != field => return false,
            MirProjection::Field { .. } => {}
            _ => return true,
        }
    }
    true
}

/// Whether writing a place of one root replaces all of the field `path`:
/// the place is the path itself or a record field prefix of it.
fn covers(projections: &[MirProjection], path: &[MirFieldId]) -> bool {
    projections.len() <= path.len()
        && projections.iter().zip(path).all(|(projection, field)| {
            matches!(projection, MirProjection::Field { field: own, .. } if own == field)
        })
}

fn is_view(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::Apply { name, .. } if name.name == "View" || name.name == "ViewMut")
}

#[derive(Clone, Copy)]
enum Event {
    Use(usize),
    Kill(usize),
}

/// How one operation treats one of its operand values.
enum Consume {
    /// The operation takes the value (stores, wraps or passes it owned).
    Yes,
    /// The operation's result carries the value onward.
    Through,
    /// The operation only inspects the value.
    No,
}

fn consumes(
    operation: &MirOperation,
    value: MirValueId,
    prelude: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Consume {
    let owned_arg = |args: &[MirCallArg]| {
        if args.iter().any(|arg| arg.value == value && arg.access == MirAccess::Move) {
            Consume::Yes
        } else {
            Consume::No
        }
    };
    match operation {
        MirOperation::WritePlace { value: stored, .. }
        | MirOperation::ReplacePlace { value: stored, .. } => {
            if *stored == value {
                Consume::Yes
            } else {
                Consume::No
            }
        }
        MirOperation::Move { .. }
        | MirOperation::TraitBox { .. }
        | MirOperation::Present { .. }
        | MirOperation::ResultOk { .. }
        | MirOperation::ResultErr { .. }
        | MirOperation::BuildList { .. }
        | MirOperation::BuildMap { .. }
        | MirOperation::Struct { .. }
        | MirOperation::Tuple { .. }
        | MirOperation::Enum { .. }
        | MirOperation::Semantic(MirSemanticOp::StructLiteral { .. }) => Consume::Yes,
        MirOperation::Copy { fact, .. } => {
            if matches!(fact, MirCopyFact::Explicit | MirCopyFact::ViewMaterialize) {
                Consume::No
            } else {
                Consume::Through
            }
        }
        MirOperation::Phi { .. }
        | MirOperation::OptionValue { .. }
        | MirOperation::ResultValue { .. }
        | MirOperation::EnumPayload { .. } => Consume::Through,
        MirOperation::Call { args, .. }
        | MirOperation::IndirectCall { args, .. }
        | MirOperation::CoreCall { args, .. }
        | MirOperation::Semantic(
            MirSemanticOp::StaticPreludeCall { args, .. }
            | MirSemanticOp::HostCall { args, .. }
            | MirSemanticOp::ClosureMethod { args, .. },
        ) => owned_arg(args),
        // A builtin method borrows its receiver; an argument is passed owned
        // exactly when the route's borrow mask (receiver first) says so.
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod { call, receiver, args, .. }) => {
            let owned = *receiver != value
                && args.iter().enumerate().any(|(index, arg)| {
                    *arg == value
                        && prelude.get(call).is_some_and(|row| {
                            row.signature.borrow_mask.get(index + 1) == Some(&false)
                        })
                });
            if owned { Consume::Yes } else { Consume::No }
        }
        MirOperation::LoopIterInit { by_value, .. } => {
            if *by_value {
                Consume::Yes
            } else {
                Consume::No
            }
        }
        MirOperation::Closure { captures, .. } => {
            if captures
                .iter()
                .any(|capture| matches!(capture, MirCaptureOperand::Value(captured) if *captured == value))
            {
                Consume::Yes
            } else {
                Consume::No
            }
        }
        _ => Consume::No,
    }
}

/// The payload bindings of one subject value: their block and instruction
/// indices, in order.
struct PayloadGroup {
    candidate: usize,
    block: usize,
    payloads: Vec<usize>,
}

fn bit_set(bits: &mut [u64], index: usize) {
    bits[index / 64] |= 1 << (index % 64);
}

fn bit_clear(bits: &mut [u64], index: usize) {
    bits[index / 64] &= !(1 << (index % 64));
}

fn bit_has(bits: &[u64], index: usize) -> bool {
    bits[index / 64] >> (index % 64) & 1 == 1
}

fn borrowed(mode: MirOwnershipMode) -> bool {
    matches!(mode, MirOwnershipMode::ReadBorrow | MirOwnershipMode::WriteBorrow)
}

impl LowerCtx<'_> {
    pub(super) fn move_last_uses(&mut self) -> Result<(), LowerError> {
        let block_index: HashMap<MirBlockId, usize> = self
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id, index))
            .collect();
        let value_rows: HashMap<MirValueId, usize> = self
            .values
            .iter()
            .enumerate()
            .map(|(index, (value, _, _, _))| (*value, index))
            .collect();
        let mode_of = |value: MirValueId| {
            value_rows
                .get(&value)
                .map(|row| self.values[*row].3.mode)
        };
        // A value that may hold a borrow of the storage it came from.
        let holds_borrow = |value: MirValueId| {
            value_rows.get(&value).is_some_and(|row| {
                borrowed(self.values[*row].3.mode) || is_view(&self.values[*row].1)
            })
        };
        let mut definitions: HashMap<MirValueId, (usize, usize)> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                if let Some(result) = instruction.result {
                    definitions.insert(result, (block, index));
                }
            }
        }

        // Owned locals: a whole local place with a drop flag. Places whose
        // release is observable (a scope-end `close`, a file owner, a
        // function-level drop row) keep their lexical lifetime.
        let place_rows: HashMap<MirPlaceId, usize> = self
            .places
            .iter()
            .enumerate()
            .map(|(index, row)| (row.id, index))
            .collect();
        let mut excluded_roots: HashSet<Root> = HashSet::new();
        for place in self.file_owner_places.iter().chain(self.drops.iter().map(|drop| &drop.place)) {
            if let Some(root) = place_rows
                .get(place)
                .and_then(|row| place_root(&self.places[*row].base))
            {
                excluded_roots.insert(root);
            }
        }
        let mut owned: Vec<(MirPlaceId, MirPlaceId)> = self
            .drop_live_places
            .iter()
            .map(|(place, flag)| (*place, *flag))
            .collect();
        owned.sort_unstable_by_key(|(place, _)| place.0);
        let mut candidates: Vec<Candidate> = Vec::new();
        // The field path of each `Candidate::Field`, empty for the others.
        let mut paths: Vec<Vec<MirFieldId>> = Vec::new();
        let mut root_candidate: HashMap<Root, usize> = HashMap::new();
        let mut local_candidate: HashMap<MirLocalId, usize> = HashMap::new();
        let mut flag_candidate: HashMap<MirPlaceId, usize> = HashMap::new();
        let mut rejected: Vec<bool> = Vec::new();
        for (place, flag) in owned {
            let Some(row) = place_rows.get(&place).map(|row| &self.places[*row]) else {
                continue;
            };
            let Some(root) = place_root(&row.base) else {
                continue;
            };
            if !row.projections.is_empty()
                || row.persist_key.is_some()
                || excluded_roots.contains(&root)
                || self.scope_end_close(&row.ty).is_some()
                || is_view(&row.ty)
            {
                continue;
            }
            if let Some(existing) = root_candidate.get(&root) {
                rejected[*existing] = true;
                continue;
            }
            root_candidate.insert(root, candidates.len());
            flag_candidate.insert(flag, candidates.len());
            candidates.push(Candidate::Local { place, flag });
            paths.push(Vec::new());
            rejected.push(false);
        }
        let mut place_candidate: HashMap<MirPlaceId, usize> = HashMap::new();
        for row in &self.places {
            if let Some(candidate) = place_root(&row.base).and_then(|root| root_candidate.get(&root)) {
                place_candidate.insert(row.id, *candidate);
            }
        }

        // Field paths of owned locals that some `ReadPlace` reads: one
        // candidate per (local, path), whichever place rows name it. A field
        // has no scope-end `close` of its own (its owner drops it), so moving
        // it out changes no observable release.
        let read_places: HashSet<MirPlaceId> = self
            .blocks
            .iter()
            .flat_map(|row| &row.instructions)
            .filter_map(|instruction| match &instruction.operation {
                MirOperation::ReadPlace(place) => Some(*place),
                _ => None,
            })
            .collect();
        let mut field_candidate: HashMap<MirPlaceId, usize> = HashMap::new();
        let mut root_fields: HashMap<usize, Vec<usize>> = HashMap::new();
        {
            let mut field_rows: Vec<usize> = (0..self.places.len())
                .filter(|row| read_places.contains(&self.places[*row].id))
                .collect();
            field_rows.sort_unstable_by_key(|row| self.places[*row].id.0);
            let mut by_path: HashMap<(usize, Vec<MirFieldId>), usize> = HashMap::new();
            for row in field_rows {
                let row = &self.places[row];
                let Some(root @ Root::Local(_)) = place_root(&row.base) else {
                    continue;
                };
                let Some(&owner) = root_candidate.get(&root) else {
                    continue;
                };
                if row.projections.is_empty() || row.persist_key.is_some() || is_view(&row.ty) {
                    continue;
                }
                let Some(path) = field_path(&row.projections) else {
                    continue;
                };
                let candidate = match by_path.get(&(owner, path.clone())) {
                    Some(candidate) => *candidate,
                    None => {
                        let candidate = candidates.len();
                        by_path.insert((owner, path.clone()), candidate);
                        candidates.push(Candidate::Field { root: owner });
                        paths.push(path);
                        rejected.push(false);
                        root_fields.entry(owner).or_default().push(candidate);
                        candidate
                    }
                };
                field_candidate.insert(row.id, candidate);
            }
        }
        // Every candidate a place observes: its root and the field
        // candidates of that root it overlaps.
        let mut touches: HashMap<MirPlaceId, Vec<usize>> = HashMap::new();
        for row in &self.places {
            let Some(&root) = place_candidate.get(&row.id) else {
                continue;
            };
            let mut observed = vec![root];
            if let Some(fields) = root_fields.get(&root) {
                observed.extend(
                    fields
                        .iter()
                        .copied()
                        .filter(|field| overlaps(&row.projections, &paths[*field])),
                );
            }
            touches.insert(row.id, observed);
        }
        // Locals an operation names by identity (a parameter's local row
        // names the parameter's own place).
        for local in &self.locals {
            if let Some(candidate) = place_candidate.get(&local.place) {
                local_candidate.entry(local.id).or_insert(*candidate);
            }
        }
        // A parameter read as a bare value rather than through its place is
        // outside this place-based liveness; such a parameter keeps its
        // lexical lifetime.
        let parameter_candidate: HashMap<MirValueId, usize> = root_candidate
            .iter()
            .filter_map(|(root, candidate)| match root {
                Root::Parameter(value) => Some((*value, *candidate)),
                Root::Local(_) => None,
            })
            .collect();
        if !parameter_candidate.is_empty() {
            for row in &self.blocks {
                let uses = row
                    .instructions
                    .iter()
                    .flat_map(|instruction| instruction.operation.value_uses())
                    .chain(row.terminator.value_uses());
                for value in uses {
                    if let Some(candidate) = parameter_candidate.get(&value) {
                        rejected[*candidate] = true;
                    }
                }
            }
        }

        // Copies: the result of a `ReadPlace` of a candidate place, with that
        // place, its block and its index.
        let mut copies: HashMap<MirValueId, (MirPlaceId, usize, usize)> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                if let (MirOperation::ReadPlace(place), Some(result)) =
                    (&instruction.operation, instruction.result)
                {
                    if touches.contains_key(place) {
                        copies.insert(result, (*place, block, index));
                    }
                }
            }
        }

        // Enum subjects: every use is an EnumIs or an EnumPayload of one
        // variant, and the payloads (each index once) sit in one block after
        // every EnumIs of that block.
        type EnumUse = (usize, usize, Option<(MirTypeId, String, usize)>);
        let mut enum_uses: HashMap<MirValueId, Vec<EnumUse>> = HashMap::new();
        let mut other_uses: HashSet<MirValueId> = HashSet::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                match &instruction.operation {
                    MirOperation::EnumIs { subject, .. } => {
                        enum_uses.entry(*subject).or_default().push((block, index, None));
                    }
                    MirOperation::EnumPayload { subject, owner, variant, index: payload } => {
                        enum_uses.entry(*subject).or_default().push((
                            block,
                            index,
                            Some((*owner, variant.clone(), *payload)),
                        ));
                    }
                    operation => other_uses.extend(operation.value_uses()),
                }
            }
            other_uses.extend(row.terminator.value_uses());
        }
        for row in &self.places {
            if let MirPlaceBase::Temporary(value) = &row.base {
                other_uses.insert(*value);
            }
        }
        let mut subjects: Vec<(MirValueId, Vec<EnumUse>)> = enum_uses
            .into_iter()
            .filter(|(value, _)| !other_uses.contains(value))
            .collect();
        subjects.sort_unstable_by_key(|(value, _)| value.0);
        let mut subject_candidate: HashMap<MirValueId, usize> = HashMap::new();
        let mut groups: Vec<PayloadGroup> = Vec::new();
        for (value, uses) in subjects {
            let payloads: Vec<&EnumUse> = uses.iter().filter(|(_, _, payload)| payload.is_some()).collect();
            let Some(first) = payloads.first() else {
                continue;
            };
            let (block, first_index) = (first.0, first.1);
            let shape = first.2.as_ref().map(|(owner, variant, _)| (*owner, variant.clone()));
            let mut indices = HashSet::new();
            let one_block = payloads.iter().all(|(payload_block, _, payload)| {
                let (owner, variant, index) = payload.as_ref().expect("payload use");
                *payload_block == block
                    && shape.as_ref() == Some(&(*owner, variant.clone()))
                    && indices.insert(*index)
            });
            let tests_first = uses.iter().all(|(use_block, index, payload)| {
                payload.is_some() || *use_block != block || *index < first_index
            });
            let defined = match definitions.get(&value) {
                Some((def_block, def_index)) => !matches!(
                    self.blocks[*def_block].instructions[*def_index].operation,
                    MirOperation::Parameter { .. }
                        | MirOperation::Capture { .. }
                        | MirOperation::Global { .. }
                ),
                None => false,
            };
            let owned_value = matches!(
                mode_of(value),
                Some(MirOwnershipMode::Owned | MirOwnershipMode::Move)
            );
            if !one_block || !tests_first || !defined || !owned_value
                || self.read_window_of(value).is_some()
            {
                continue;
            }
            subject_candidate.insert(value, candidates.len());
            groups.push(PayloadGroup {
                candidate: candidates.len(),
                block,
                payloads: payloads.iter().map(|(_, index, _)| *index).collect(),
            });
            candidates.push(Candidate::Subject { value });
            paths.push(Vec::new());
            rejected.push(false);
        }
        if candidates.is_empty() {
            return Ok(());
        }

        // CFG: predecessors, blocks reachable from the entry along explicit
        // edges, and cleanup blocks guarded by one candidate's drop flag.
        let block_count = self.blocks.len();
        let successors: Vec<Vec<usize>> = self
            .blocks
            .iter()
            .map(|row| {
                row.terminator
                    .targets()
                    .iter()
                    .filter_map(|target| block_index.get(target).copied())
                    .collect()
            })
            .collect();
        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); block_count];
        for (block, targets) in successors.iter().enumerate() {
            for target in targets {
                predecessors[*target].push(block);
            }
        }
        let mut reachable = vec![false; block_count];
        if let Some(entry) = block_index.get(&self.entry).copied() {
            let mut pending = vec![entry];
            reachable[entry] = true;
            while let Some(block) = pending.pop() {
                for target in &successors[block] {
                    if !reachable[*target] {
                        reachable[*target] = true;
                        pending.push(*target);
                    }
                }
            }
        }
        let flag_guard = |block: usize| -> Option<usize> {
            let MirTerminator::Branch { condition, then_target, else_target } =
                &self.blocks[block].terminator
            else {
                return None;
            };
            if then_target == else_target {
                return None;
            }
            let (def_block, def_index) = definitions.get(condition)?;
            let MirOperation::ReadPlace(flag) = &self.blocks[*def_block].instructions[*def_index].operation
            else {
                return None;
            };
            flag_candidate.get(flag).copied()
        };
        let mut guarded: Vec<Option<usize>> = vec![None; block_count];
        for (block, preds) in predecessors.iter().enumerate() {
            let Some(first) = preds.first() else {
                continue;
            };
            let then_target = |pred: usize| match &self.blocks[pred].terminator {
                MirTerminator::Branch { then_target, .. } => block_index.get(then_target).copied(),
                _ => None,
            };
            let guard = flag_guard(*first);
            if guard.is_some()
                && preds.iter().all(|pred| flag_guard(*pred) == guard && then_target(*pred) == Some(block))
            {
                guarded[block] = guard;
            }
        }

        // Borrows and views of candidate places, by origin. An origin is a
        // copy (its own `ReadPlace` value, observing its place's candidates)
        // or a borrowed or view result of an operation on candidate places
        // (`borrow_roots`, with the candidates those places observe). A
        // borrowed or view result of an operand that carries origins carries
        // them too, transitively; every use of such a value is a use of its
        // origins. An operation that lets a place escape (raw address,
        // closure capture) keeps all of that place's reads copies.
        let mut borrow_roots: HashMap<MirValueId, Vec<usize>> = HashMap::new();
        for row in &self.blocks {
            for instruction in &row.instructions {
                let escapes = matches!(
                    instruction.operation,
                    MirOperation::RawAddressOf { .. } | MirOperation::Closure { .. }
                );
                let mut observed: Vec<usize> = Vec::new();
                for place in checked_operation_place_refs(&instruction.operation) {
                    let Some(list) = touches.get(&place) else {
                        continue;
                    };
                    if escapes {
                        for candidate in list {
                            rejected[*candidate] = true;
                        }
                    }
                    observed.extend(list.iter().copied());
                }
                if observed.is_empty() || matches!(instruction.operation, MirOperation::ReadPlace(_)) {
                    continue;
                }
                if let Some(result) = instruction.result.filter(|result| holds_borrow(*result)) {
                    observed.sort_unstable();
                    observed.dedup();
                    borrow_roots.insert(result, observed);
                }
            }
        }
        let mut origins: HashMap<MirValueId, Vec<MirValueId>> = HashMap::new();
        for value in copies.keys().chain(borrow_roots.keys()) {
            origins.insert(*value, vec![*value]);
        }
        let mut changed = true;
        while changed {
            changed = false;
            for row in &self.blocks {
                for instruction in &row.instructions {
                    let Some(result) = instruction.result else {
                        continue;
                    };
                    if copies.contains_key(&result)
                        || borrow_roots.contains_key(&result)
                        || !holds_borrow(result)
                    {
                        continue;
                    }
                    let mut carried: Vec<MirValueId> = instruction
                        .operation
                        .value_uses()
                        .iter()
                        .filter_map(|value| origins.get(value))
                        .flatten()
                        .copied()
                        .collect();
                    if carried.is_empty() {
                        continue;
                    }
                    if let Some(existing) = origins.get(&result) {
                        carried.extend(existing.iter().copied());
                    }
                    carried.sort_unstable_by_key(|value| value.0);
                    carried.dedup();
                    if origins.get(&result).is_none_or(|existing| existing.len() != carried.len()) {
                        origins.insert(result, carried);
                        changed = true;
                    }
                }
            }
        }
        // A borrow or view stored into a place or captured outlives every
        // read this pass decides.
        for row in &self.blocks {
            for instruction in &row.instructions {
                let stored: Vec<MirValueId> = match &instruction.operation {
                    MirOperation::WritePlace { value, .. }
                    | MirOperation::ReplacePlace { value, .. } => vec![*value],
                    MirOperation::Closure { captures, .. } => captures
                        .iter()
                        .filter_map(|capture| match capture {
                            MirCaptureOperand::Value(value) => Some(*value),
                            MirCaptureOperand::Place(_) => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                for value in stored {
                    if copies.contains_key(&value) {
                        continue;
                    }
                    for origin in origins.get(&value).into_iter().flatten() {
                        let observed = match copies.get(origin) {
                            Some((place, _, _)) => &touches[place],
                            None => &borrow_roots[origin],
                        };
                        for candidate in observed {
                            rejected[*candidate] = true;
                        }
                    }
                }
            }
        }

        // A use of a copy's value after the copy in its own block lies in the
        // copy's read window; a use anywhere else is tracked by the copy's
        // own liveness bit.
        let inside_copy_block = |origin: &MirValueId, block: usize, index: usize| {
            copies
                .get(origin)
                .is_some_and(|(_, copy_block, copy_index)| *copy_block == block && *copy_index < index)
        };
        let mut cross_copies: Vec<MirValueId> = Vec::new();
        {
            let mut seen: HashSet<MirValueId> = HashSet::new();
            for (block, row) in self.blocks.iter().enumerate() {
                let uses = row
                    .instructions
                    .iter()
                    .enumerate()
                    .flat_map(|(index, instruction)| {
                        instruction.operation.value_uses().into_iter().map(move |value| (index, value))
                    })
                    .chain(
                        row.terminator
                            .value_uses()
                            .into_iter()
                            .map(|value| (row.instructions.len(), value)),
                    );
                for (index, value) in uses {
                    for origin in origins.get(&value).into_iter().flatten() {
                        if copies.contains_key(origin)
                            && !inside_copy_block(origin, block, index)
                            && seen.insert(*origin)
                        {
                            cross_copies.push(*origin);
                        }
                    }
                }
            }
        }
        cross_copies.sort_unstable_by_key(|value| value.0);
        let mut copy_bit: HashMap<MirValueId, usize> = HashMap::new();
        for value in cross_copies {
            copy_bit.insert(value, candidates.len());
            candidates.push(Candidate::Copy { value });
            paths.push(Vec::new());
            rejected.push(false);
        }
        // The copy bits that observe each candidate.
        let mut observing_copies: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];
        for (value, bit) in &copy_bit {
            for candidate in &touches[&copies[value].0] {
                observing_copies[*candidate].push(*bit);
            }
        }

        // Per-block events in execution order. Index `instructions.len()`
        // is the terminator.
        let mut copy_window_ends: HashMap<MirValueId, usize> = HashMap::new();
        let mut events: Vec<Vec<(usize, Event)>> = vec![Vec::new(); block_count];
        for (block, row) in self.blocks.iter().enumerate() {
            let out = &mut events[block];
            let value_events = |out: &mut Vec<(usize, Event)>,
                                    window_ends: &mut HashMap<MirValueId, usize>,
                                    index: usize,
                                    value: MirValueId| {
                if let Some(candidate) = subject_candidate.get(&value) {
                    out.push((index, Event::Use(*candidate)));
                }
                for origin in origins.get(&value).into_iter().flatten() {
                    if copies.contains_key(origin) {
                        if inside_copy_block(origin, block, index) {
                            let end = window_ends.entry(*origin).or_insert(index);
                            *end = (*end).max(index);
                        } else if let Some(bit) = copy_bit.get(origin) {
                            out.push((index, Event::Use(*bit)));
                        }
                    } else {
                        for candidate in &borrow_roots[origin] {
                            out.push((index, Event::Use(*candidate)));
                        }
                    }
                }
            };
            for (index, instruction) in row.instructions.iter().enumerate() {
                let mut kills = Vec::new();
                for place in checked_operation_place_refs(&instruction.operation) {
                    let Some(list) = touches.get(&place) else {
                        continue;
                    };
                    let projections = &self.places[place_rows[&place]].projections;
                    for &candidate in list {
                        let kill = match candidates[candidate] {
                            Candidate::Local { place: whole, .. } => match &instruction.operation {
                                MirOperation::WritePlace { .. }
                                | MirOperation::ReplacePlace { .. }
                                | MirOperation::InitializeUninit { .. }
                                    if place == whole =>
                                {
                                    Some(true)
                                }
                                MirOperation::MovePlace { .. }
                                    if place == whole && guarded[block] == Some(candidate) =>
                                {
                                    None
                                }
                                _ => Some(false),
                            },
                            // A write of the field or of a record prefix
                            // replaces it; a replace that releases a larger
                            // owner, and the owner's own cleanup, read it.
                            Candidate::Field { .. } => match &instruction.operation {
                                MirOperation::WritePlace { .. } | MirOperation::InitializeUninit { .. }
                                    if covers(projections, &paths[candidate]) =>
                                {
                                    Some(true)
                                }
                                MirOperation::ReplacePlace { .. }
                                    if projections.len() == paths[candidate].len()
                                        && covers(projections, &paths[candidate]) =>
                                {
                                    Some(true)
                                }
                                _ => Some(false),
                            },
                            Candidate::Subject { .. } | Candidate::Copy { .. } => None,
                        };
                        match kill {
                            Some(true) => kills.push(candidate),
                            Some(false) => out.push((index, Event::Use(candidate))),
                            None => {}
                        }
                    }
                }
                let locals: Vec<MirLocalId> = match &instruction.operation {
                    MirOperation::Semantic(MirSemanticOp::DataEntriesToMap { local, .. }) => {
                        vec![*local]
                    }
                    MirOperation::Semantic(MirSemanticOp::RequireStop { context, .. }) => {
                        context.locals.iter().map(|(_, local)| *local).collect()
                    }
                    _ => Vec::new(),
                };
                for local in locals {
                    if let Some(candidate) = local_candidate.get(&local) {
                        out.push((index, Event::Use(*candidate)));
                        for field in root_fields.get(candidate).into_iter().flatten() {
                            out.push((index, Event::Use(*field)));
                        }
                    }
                }
                for value in instruction.operation.value_uses() {
                    value_events(&mut *out, &mut copy_window_ends, index, value);
                }
                if let Some(result) = instruction.result {
                    if let Some(candidate) = subject_candidate.get(&result) {
                        kills.push(*candidate);
                    }
                    if let Some(bit) = copy_bit.get(&result) {
                        kills.push(*bit);
                    }
                }
                out.extend(kills.into_iter().map(|candidate| (index, Event::Kill(candidate))));
            }
            let end = row.instructions.len();
            for value in row.terminator.value_uses() {
                value_events(&mut *out, &mut copy_window_ends, end, value);
            }
            if !reachable[block] {
                for (_, event) in out.iter() {
                    if let Event::Use(candidate) = event {
                        match candidates[*candidate] {
                            Candidate::Copy { value } => {
                                for observed in &touches[&copies[&value].0] {
                                    rejected[*observed] = true;
                                }
                            }
                            _ => rejected[*candidate] = true,
                        }
                    }
                }
            }
        }

        // Backward liveness to a fixed point.
        let words = candidates.len().div_ceil(64);
        let mut generated = vec![0u64; block_count * words];
        let mut killed = vec![0u64; block_count * words];
        for (block, block_events) in events.iter().enumerate() {
            let range = block * words..(block + 1) * words;
            for (_, event) in block_events {
                match *event {
                    Event::Use(candidate) => {
                        if !bit_has(&killed[range.clone()], candidate) {
                            bit_set(&mut generated[range.clone()], candidate);
                        }
                    }
                    Event::Kill(candidate) => bit_set(&mut killed[range.clone()], candidate),
                }
            }
        }
        let mut live_in = vec![0u64; block_count * words];
        let mut live_out = vec![0u64; block_count * words];
        let mut changed = true;
        while changed {
            changed = false;
            for block in (0..block_count).rev() {
                let range = block * words..(block + 1) * words;
                for word in 0..words {
                    let out = successors[block]
                        .iter()
                        .fold(0u64, |acc, target| acc | live_in[target * words + word]);
                    live_out[range.start + word] = out;
                    let input = generated[range.start + word] | (out & !killed[range.start + word]);
                    if input != live_in[range.start + word] {
                        live_in[range.start + word] = input;
                        changed = true;
                    }
                }
            }
        }

        // Each value's uses, as (block, index) with the terminator at the
        // block's instruction count, to decide whether a read is consumed.
        let mut users: HashMap<MirValueId, Vec<(usize, usize)>> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                for value in instruction.operation.value_uses() {
                    users.entry(value).or_default().push((block, index));
                }
            }
            for value in row.terminator.value_uses() {
                users.entry(value).or_default().push((block, row.instructions.len()));
            }
        }
        let prelude: HashMap<MirPreludeCallId, &MirPreludeCall> =
            self.prelude_calls.iter().map(|row| (row.id, row)).collect();
        let consumed = |value: MirValueId| -> bool {
            let mut pending = vec![value];
            let mut seen: HashSet<MirValueId> = HashSet::new();
            while let Some(value) = pending.pop() {
                if !seen.insert(value) {
                    continue;
                }
                for &(block, index) in users.get(&value).into_iter().flatten() {
                    let row = &self.blocks[block];
                    let Some(instruction) = row.instructions.get(index) else {
                        if matches!(
                            &row.terminator,
                            MirTerminator::Return { value: Some(returned) }
                            | MirTerminator::Break { value: Some(returned), .. }
                            | MirTerminator::Yield { value: returned, .. }
                                if *returned == value
                        ) {
                            return true;
                        }
                        continue;
                    };
                    match consumes(&instruction.operation, value, &prelude) {
                        Consume::Yes => return true,
                        Consume::Through => pending.extend(instruction.result),
                        Consume::No => {}
                    }
                }
            }
            false
        };

        // Decide each read against the liveness right after it.
        let mut group_end: HashMap<(usize, usize), usize> = HashMap::new();
        for (group, row) in groups.iter().enumerate() {
            group_end.insert((row.block, *row.payloads.last().expect("payload group")), group);
        }
        let mut read_windows: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for (value, end) in &copy_window_ends {
            let (place, block, start) = copies[value];
            for candidate in &touches[&place] {
                read_windows.entry((block, *candidate)).or_default().push((start, *end));
            }
        }
        let inside_read_window = |block: usize, index: usize, candidate: usize| {
            read_windows
                .get(&(block, candidate))
                .is_some_and(|windows| windows.iter().any(|(start, end)| *start < index && index < *end))
        };
        let mut conversions: Vec<(usize, usize, usize)> = Vec::new();
        let mut moved_groups: Vec<usize> = Vec::new();
        for block in 0..block_count {
            if !reachable[block] {
                continue;
            }
            let mut live = live_out[block * words..(block + 1) * words].to_vec();
            let block_events = &events[block];
            let mut cursor = block_events.len();
            let apply = |live: &mut Vec<u64>, cursor: &mut usize, index: usize| {
                while *cursor > 0 && block_events[*cursor - 1].0 >= index {
                    *cursor -= 1;
                    match block_events[*cursor].1 {
                        Event::Kill(candidate) => bit_clear(live, candidate),
                        Event::Use(candidate) => bit_set(live, candidate),
                    }
                }
            };
            let instructions = &self.blocks[block].instructions;
            apply(&mut live, &mut cursor, instructions.len());
            for index in (0..instructions.len()).rev() {
                if let (MirOperation::ReadPlace(place), Some(result)) =
                    (&instructions[index].operation, instructions[index].result)
                {
                    let target = match place_candidate.get(place).map(|candidate| (*candidate, candidates[*candidate])) {
                        Some((candidate, Candidate::Local { place: whole, .. })) if whole == *place => Some(candidate),
                        _ => field_candidate.get(place).copied(),
                    };
                    if let Some(candidate) = target {
                        // A field move pays for splitting its owner only for
                        // an owned (heap) value; a Copy field stays a read.
                        // MIR legality admits a field move only when a write
                        // that refills the field dominates every later use of
                        // its owner: a refill later in the move's own block
                        // does, since every path out of the move runs it.
                        let (root, worth) = match candidates[candidate] {
                            Candidate::Field { root } => (
                                root,
                                matches!(
                                    mode_of(result),
                                    Some(MirOwnershipMode::Owned | MirOwnershipMode::Move)
                                ) && block_events.iter().any(|(at, event)| {
                                    *at > index && matches!(event, Event::Kill(killed) if *killed == candidate)
                                }),
                            ),
                            _ => (candidate, true),
                        };
                        let own = copy_bit.get(&result).copied();
                        if worth
                            && !rejected[candidate]
                            && !rejected[root]
                            && !bit_has(&live, candidate)
                            && !inside_read_window(block, index, candidate)
                            && !observing_copies[candidate]
                                .iter()
                                .any(|bit| Some(*bit) != own && bit_has(&live, *bit))
                            && self.read_window_of(result).is_none()
                            && consumed(result)
                        {
                            conversions.push((block, index, candidate));
                        }
                    }
                }
                if let Some(group) = group_end.get(&(block, index)).copied() {
                    let candidate = groups[group].candidate;
                    if !rejected[candidate] && !bit_has(&live, candidate) {
                        moved_groups.push(group);
                    }
                }
                apply(&mut live, &mut cursor, index);
            }
        }

        // Rewrite in place first (no index moves), then insert the new
        // instructions last position first so earlier indices stay valid.
        let bool_type = self.mir_type(&Type::Bool)?;
        let mut inserts: Vec<(usize, usize, Vec<MirInstruction>)> = Vec::new();
        for group in moved_groups {
            let block = groups[group].block;
            let first = groups[group].payloads[0];
            let Candidate::Subject { value } = candidates[groups[group].candidate] else {
                unreachable!("payload group targets a subject candidate");
            };
            let (span, source_line) = {
                let instruction = &self.blocks[block].instructions[first];
                (instruction.span, instruction.source_line)
            };
            let ty = self.values[value_rows[&value]].1.clone();
            let take = self.last_use_instruction(
                span,
                source_line,
                "match.payload.take",
                Some(ty),
                MirOperation::Move { value },
            )?;
            let taken = take.result.expect("move result");
            for payload in &groups[group].payloads {
                if let MirOperation::EnumPayload { subject, .. } =
                    &mut self.blocks[block].instructions[*payload].operation
                {
                    *subject = taken;
                }
            }
            inserts.push((block, first, vec![take]));
        }
        for (block, index, candidate) in conversions {
            let (span, source_line, result, place) = {
                let instruction = &self.blocks[block].instructions[index];
                let MirOperation::ReadPlace(place) = instruction.operation else {
                    unreachable!("read conversion targets a ReadPlace");
                };
                (instruction.span, instruction.source_line, instruction.result, place)
            };
            retain_place_access(&mut self.places[place_rows[&place]], MirAccess::Move);
            let operation = MirOperation::MovePlace { place };
            if let Some(result) = result {
                let row = value_rows[&result];
                let ty = self.values[row].1.clone();
                let ownership = self.operation_ownership(&operation, &ty)?;
                self.values[row].3 = ownership;
            }
            self.blocks[block].instructions[index].operation = operation;
            // A field move leaves its owner live: the refill that follows
            // makes it whole again.
            let Candidate::Local { flag, .. } = candidates[candidate] else {
                continue;
            };
            let off = self.last_use_instruction(
                span,
                source_line,
                "owned.local.last-use.state",
                Some(bool_type.clone()),
                MirOperation::Constant(MirConstant::Bool(false)),
            )?;
            let off_value = off.result.expect("constant result");
            let write = self.last_use_instruction(
                span,
                source_line,
                "owned.local.last-use.update",
                None,
                MirOperation::WritePlace { place: flag, value: off_value },
            )?;
            inserts.push((block, index + 1, vec![off, write]));
        }
        inserts.sort_unstable_by(|left, right| (right.0, right.1).cmp(&(left.0, left.1)));
        for (block, position, instructions) in inserts {
            self.blocks[block].instructions.splice(position..position, instructions);
        }
        Ok(())
    }

    fn last_use_instruction(
        &mut self,
        span: jet_foundation::Diagnostics::Span,
        source_line: Option<u32>,
        role: &str,
        ty: Option<jet_foundation::MIR::MirType>,
        operation: MirOperation,
    ) -> Result<MirInstruction, LowerError> {
        let detail = format!("{operation:?}");
        let identity = self.reserve_identity("operation", span, role, &detail)?;
        let value = MirValueId(stable_id("mir-value", &identity));
        if let Some(ty) = &ty {
            let ownership = self.operation_ownership(&operation, ty)?;
            self.values.push((value, ty.clone(), span, ownership));
        }
        Ok(MirInstruction {
            id: MirOpId(stable_id("mir-op", &identity)),
            span,
            source_line,
            result: ty.as_ref().map(|_| value),
            ty,
            operation,
        })
    }
}
