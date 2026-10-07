//! Static drop-flag resolution (#4155).
//!
//! Lowering guards each owned local's scope-end cleanup with a live flag
//! (`owned_live_N`): written `false` in the entry block, set where the
//! binding is initialized, cleared by every whole move, and read by the
//! `Branch` in front of the cleanup's move and drop. On most paths the flag
//! holds one constant wherever a guard reads it, which is what drop
//! elaboration finds statically for Rust. One forward pass over the constant
//! flag values resolves each such guard to a `Jump`: `true` keeps the
//! cleanup unconditionally, `false` leaves it unreachable for
//! `UnreachableBlockElimination`. A flag with no read left loses its writes,
//! the constants only they consumed, and its local, so no backend carries
//! dead flag traffic. The pass only removes branches whose outcome is the
//! same on every path, so execution order and drop timing are unchanged.

use std::collections::{HashMap, HashSet};

use jet_foundation::MIR::{
    MirBlockId, MirConstant, MirDropEdge, MirLocalId, MirOperation, MirPlaceBase, MirPlaceId,
    MirProjection, MirSemanticOp, MirTerminator, MirValueId,
};

use super::mir::{LowerCtx, checked_operation_place_refs};

/// The value a flag holds at a program point.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Live {
    Yes,
    No,
    Unknown,
}

fn join(left: Live, right: Live) -> Live {
    if left == right { left } else { Live::Unknown }
}

/// One `ReadPlace` of a flag that feeds only its block's `Branch`.
struct Guard {
    flag: usize,
    block: usize,
    index: usize,
    result: MirValueId,
}

/// Locals an operation names by identity rather than through a place.
fn identity_locals(operation: &MirOperation) -> Vec<MirLocalId> {
    match operation {
        MirOperation::Semantic(MirSemanticOp::DataEntriesToMap { local, .. }) => vec![*local],
        MirOperation::Semantic(MirSemanticOp::RequireStop { context, .. }) => {
            context.locals.iter().map(|(_, local)| *local).collect()
        }
        _ => Vec::new(),
    }
}

impl LowerCtx<'_> {
    pub(super) fn resolve_drop_flags(&mut self) {
        if self.drop_live_places.is_empty() {
            return;
        }
        let block_count = self.blocks.len();
        let block_index: HashMap<MirBlockId, usize> = self
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id, index))
            .collect();
        // Candidate flags: a whole-local place whose local has no other place.
        let mut flag_places: Vec<MirPlaceId> = self.drop_live_places.values().copied().collect();
        flag_places.sort_unstable_by_key(|place| place.0);
        flag_places.dedup();
        let place_rows: HashMap<MirPlaceId, usize> =
            self.places.iter().enumerate().map(|(index, row)| (row.id, index)).collect();
        let mut flag_of_place: HashMap<MirPlaceId, usize> = HashMap::new();
        let mut flag_locals: Vec<MirLocalId> = Vec::new();
        let mut flag_of_local: HashMap<MirLocalId, usize> = HashMap::new();
        for place in &flag_places {
            let Some(row) = place_rows.get(place).map(|index| &self.places[*index]) else {
                continue;
            };
            let MirPlaceBase::Local(local) = row.base else {
                continue;
            };
            if !row.projections.is_empty() || row.persist_key.is_some() {
                continue;
            }
            flag_of_place.insert(*place, flag_locals.len());
            flag_of_local.insert(local, flag_locals.len());
            flag_locals.push(local);
        }
        let count = flag_locals.len();
        if count == 0 {
            return;
        }
        let mut rejected = vec![false; count];
        for row in &self.places {
            if let MirPlaceBase::Local(local) = &row.base {
                if let Some(flag) = flag_of_local.get(local) {
                    if flag_of_place.get(&row.id) != Some(flag) {
                        rejected[*flag] = true;
                    }
                }
            }
        }

        // Every mention of a flag is a constant-or-unknown write or a read.
        let mut constants: HashMap<MirValueId, bool> = HashMap::new();
        let mut reads: HashMap<MirValueId, (usize, usize, usize)> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                if let (Some(result), MirOperation::Constant(MirConstant::Bool(value))) =
                    (instruction.result, &instruction.operation)
                {
                    constants.insert(result, *value);
                }
                for local in identity_locals(&instruction.operation) {
                    if let Some(flag) = flag_of_local.get(&local) {
                        rejected[*flag] = true;
                    }
                }
                for place in checked_operation_place_refs(&instruction.operation) {
                    let Some(flag) = flag_of_place.get(&place).copied() else {
                        continue;
                    };
                    match (&instruction.operation, instruction.result) {
                        (MirOperation::WritePlace { .. }, _) => {}
                        (MirOperation::ReadPlace(_), Some(result)) => {
                            reads.insert(result, (flag, block, index));
                        }
                        _ => rejected[flag] = true,
                    }
                }
            }
        }

        // A read must feed exactly one use: its own block's two-way Branch.
        let mut uses: HashMap<MirValueId, usize> = HashMap::new();
        for row in &self.blocks {
            for instruction in &row.instructions {
                for value in instruction.operation.value_uses() {
                    *uses.entry(value).or_default() += 1;
                }
            }
            for value in row.terminator.value_uses() {
                *uses.entry(value).or_default() += 1;
            }
        }
        for row in &self.places {
            if let MirPlaceBase::Temporary(value) = &row.base {
                *uses.entry(*value).or_default() += 1;
            }
            for projection in &row.projections {
                match projection {
                    MirProjection::Index { index, .. } => *uses.entry(*index).or_default() += 1,
                    MirProjection::Range { range, .. } => *uses.entry(*range).or_default() += 1,
                    _ => {}
                }
            }
        }
        let mut guards: HashMap<usize, Guard> = HashMap::new();
        for (result, (flag, block, index)) in &reads {
            let branch = match &self.blocks[*block].terminator {
                MirTerminator::Branch { condition, then_target, else_target } => {
                    condition == result && then_target != else_target
                }
                _ => false,
            };
            if !branch || uses.get(result).copied() != Some(1) {
                rejected[*flag] = true;
                continue;
            }
            guards.insert(
                *block,
                Guard { flag: *flag, block: *block, index: *index, result: *result },
            );
        }
        if rejected.iter().all(|rejected| *rejected) {
            return;
        }

        // Forward constant propagation of every flag to a fixed point. A
        // resolved guard only passes its state along the edge it takes.
        // Blocks entered through cleanup edges (failure or unwind drop
        // rows) start unknown.
        let mut entry_state: Vec<Option<Vec<Live>>> = vec![None; block_count];
        let unknown = vec![Live::Unknown; count];
        let mut pending: Vec<usize> = Vec::new();
        let mut queued = vec![false; block_count];
        let mut seed = |block: usize, entry_state: &mut Vec<Option<Vec<Live>>>| {
            entry_state[block] = Some(unknown.clone());
            if !queued[block] {
                queued[block] = true;
                pending.push(block);
            }
        };
        if let Some(entry) = block_index.get(&self.entry).copied() {
            seed(entry, &mut entry_state);
        }
        for row in &self.drops {
            if let MirDropEdge::Failure(target) | MirDropEdge::Unwind(target) = &row.edge
            {
                if let Some(block) = block_index.get(target).copied() {
                    seed(block, &mut entry_state);
                }
            }
        }
        let mut read_state: HashMap<MirValueId, Live> = HashMap::new();
        while let Some(block) = pending.pop() {
            queued[block] = false;
            let mut state = entry_state[block].clone().expect("queued block has a state");
            for instruction in &self.blocks[block].instructions {
                match &instruction.operation {
                    MirOperation::WritePlace { place, value } => {
                        if let Some(flag) = flag_of_place.get(place) {
                            state[*flag] = match constants.get(value) {
                                Some(true) => Live::Yes,
                                Some(false) => Live::No,
                                None => Live::Unknown,
                            };
                        }
                    }
                    MirOperation::ReadPlace(place) => {
                        if let (Some(flag), Some(result)) =
                            (flag_of_place.get(place), instruction.result)
                        {
                            read_state.insert(result, state[*flag]);
                        }
                    }
                    _ => {}
                }
            }
            let terminator = &self.blocks[block].terminator;
            let targets = match (guards.get(&block), terminator) {
                (Some(guard), MirTerminator::Branch { then_target, else_target, .. })
                    if !rejected[guard.flag] =>
                {
                    match read_state.get(&guard.result) {
                        Some(Live::Yes) => vec![*then_target],
                        Some(Live::No) => vec![*else_target],
                        _ => terminator.targets(),
                    }
                }
                _ => terminator.targets(),
            };
            for target in targets {
                let Some(target) = block_index.get(&target).copied() else {
                    continue;
                };
                let merged = match &entry_state[target] {
                    None => state.clone(),
                    Some(previous) => {
                        previous.iter().zip(&state).map(|(left, right)| join(*left, *right)).collect()
                    }
                };
                if entry_state[target].as_ref() != Some(&merged) {
                    entry_state[target] = Some(merged);
                    if !queued[target] {
                        queued[target] = true;
                        pending.push(target);
                    }
                }
            }
        }

        // Resolve the guards with a known flag; a flag keeps its writes while
        // any read of it remains.
        let mut live_reads = vec![0usize; count];
        let mut resolved: Vec<(usize, usize, bool)> = Vec::new();
        for guard in guards.values() {
            if rejected[guard.flag] {
                continue;
            }
            match read_state.get(&guard.result) {
                Some(Live::Yes) => resolved.push((guard.block, guard.index, true)),
                Some(Live::No) => resolved.push((guard.block, guard.index, false)),
                _ => live_reads[guard.flag] += 1,
            }
        }
        if resolved.is_empty() && !live_reads.iter().zip(&rejected).any(|(reads, rejected)| *reads == 0 && !rejected) {
            return;
        }
        for (block, _, live) in &resolved {
            let MirTerminator::Branch { then_target, else_target, .. } = self.blocks[*block].terminator.clone()
            else {
                unreachable!("resolved drop guard ends in a branch");
            };
            let (taken, dropped) = if *live { (then_target, else_target) } else { (else_target, then_target) };
            self.blocks[*block].terminator = MirTerminator::Jump { target: taken };
            let source = self.blocks[*block].id;
            if let Some(position) = block_index.get(&dropped).copied() {
                for instruction in &mut self.blocks[position].instructions {
                    if let MirOperation::Phi { incoming } = &mut instruction.operation {
                        incoming.retain(|(predecessor, _)| *predecessor != source);
                    }
                }
            }
        }

        // Delete the traffic of every flag nothing reads any more: its reads
        // (each fed only a resolved branch), its writes, the constants only
        // those writes used, and its local and place.
        let dead: HashSet<usize> = (0..count).filter(|flag| !rejected[*flag] && live_reads[*flag] == 0).collect();
        if dead.is_empty() {
            return;
        }
        let mut removed: HashSet<MirValueId> = HashSet::new();
        let mut released: Vec<MirValueId> = Vec::new();
        for row in &mut self.blocks {
            row.instructions.retain(|instruction| {
                let flag = match &instruction.operation {
                    MirOperation::WritePlace { place, value } => {
                        let flag = flag_of_place.get(place).copied();
                        if flag.is_some_and(|flag| dead.contains(&flag)) {
                            released.push(*value);
                        }
                        flag
                    }
                    MirOperation::ReadPlace(place) => flag_of_place.get(place).copied(),
                    _ => None,
                };
                if flag.is_some_and(|flag| dead.contains(&flag)) {
                    removed.extend(instruction.result);
                    return false;
                }
                true
            });
        }
        let mut orphaned: HashSet<MirValueId> = HashSet::new();
        for value in released {
            if let Some(count) = uses.get_mut(&value) {
                *count -= 1;
                if *count == 0 && constants.contains_key(&value) {
                    orphaned.insert(value);
                }
            }
        }
        for row in &mut self.blocks {
            row.instructions.retain(|instruction| {
                !instruction.result.is_some_and(|result| orphaned.contains(&result))
            });
        }
        removed.extend(orphaned);
        self.values.retain(|(value, _, _, _)| !removed.contains(value));
        for value in &removed {
            self.value_blocks.remove(value);
        }
        let dead_locals: HashSet<MirLocalId> = dead.iter().map(|flag| flag_locals[*flag]).collect();
        let dead_places: HashSet<MirPlaceId> = flag_of_place
            .iter()
            .filter(|(_, flag)| dead.contains(flag))
            .map(|(place, _)| *place)
            .collect();
        self.locals.retain(|local| !dead_locals.contains(&local.id));
        self.places.retain(|place| !dead_places.contains(&place.id));
        self.drop_live_places.retain(|_, flag| !dead_places.contains(flag));
    }
}
