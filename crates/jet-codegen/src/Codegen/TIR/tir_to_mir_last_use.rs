//! Last uses move (D-MEM-COPYSEM1).
//!
//! Lowering reads an owned local with `ReadPlace`, which every backend
//! materializes as a copy, and a sema-checked `^x` reaches lowering as that
//! same bare read. Once no later code can observe the binding, the copy and
//! the value are indistinguishable, so this pass turns such a read into
//! `MovePlace` and clears the binding's drop flag, exactly as an explicit
//! move lowers. The same liveness fact lets a match arm take the payloads of
//! a subject value nothing reads after the arm binds them: a `Move` of the
//! subject feeds those `EnumPayload`s, which a backend extracts by value.
//!
//! One backward liveness pass over the function's blocks decides both; the
//! pass is linear in instructions times the candidate bitset width.

use std::collections::{HashMap, HashSet};

use jet_foundation::AST::Type;
use jet_foundation::MIR::{
    MirAccess, MirBlockId, MirCaptureOperand, MirConstant, MirInstruction, MirLocalId, MirOpId,
    MirOperation, MirOwnershipMode, MirPlaceBase, MirPlaceId, MirSemanticOp, MirTerminator,
    MirTypeId, MirTypeKind, MirValueId, stable_id,
};

use super::mir::{LowerCtx, LowerError, checked_operation_place_refs, retain_place_access};

#[derive(Clone, Copy)]
enum Candidate {
    /// An owned local: its whole place and its drop flag.
    Local { place: MirPlaceId, flag: MirPlaceId },
    /// An enum subject value whose payloads one block binds.
    Subject { value: MirValueId },
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

#[derive(Clone, Copy)]
enum Event {
    Use(usize),
    Kill(usize),
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
                || matches!(row.ty.kind(), MirTypeKind::Apply { name, .. } if name.name == "View")
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
            rejected.push(false);
        }
        let mut place_candidate: HashMap<MirPlaceId, usize> = HashMap::new();
        for row in &self.places {
            if let Some(candidate) = place_root(&row.base).and_then(|root| root_candidate.get(&root)) {
                place_candidate.insert(row.id, *candidate);
            }
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

        // Copies of a candidate local: the result of a `ReadPlace` of one of
        // its places. A backend may serve a borrowed consumer of a copy by
        // reading the place itself at that consumer, so the local must hold
        // its value up to every use of the copy. Uses in the copy's own block
        // form a read window that no other read of the local may move out of;
        // any other use counts as a use of the local where it occurs.
        let mut copies: HashMap<MirValueId, (usize, usize, usize)> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for (index, instruction) in row.instructions.iter().enumerate() {
                if let (MirOperation::ReadPlace(place), Some(result)) =
                    (&instruction.operation, instruction.result)
                {
                    if let Some(candidate) = place_candidate.get(place) {
                        copies.insert(result, (*candidate, block, index));
                    }
                }
            }
        }
        let mut copy_window_ends: HashMap<MirValueId, usize> = HashMap::new();

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

        // Borrows of a candidate local: a borrowed result of an operation on
        // one of its places. Such a borrow may only feed operations in its
        // own block that do not derive a further borrow; its uses then count
        // as uses of the local.
        let mut borrow_candidate: HashMap<MirValueId, (usize, usize)> = HashMap::new();
        for (block, row) in self.blocks.iter().enumerate() {
            for instruction in &row.instructions {
                let places = checked_operation_place_refs(&instruction.operation);
                for place in &places {
                    let Some(candidate) = place_candidate.get(place).copied() else {
                        continue;
                    };
                    let escapes = match &instruction.operation {
                        MirOperation::RawAddressOf { .. } | MirOperation::Closure { .. } => true,
                        _ => false,
                    };
                    if escapes {
                        rejected[candidate] = true;
                    }
                    if let Some(result) = instruction.result {
                        if mode_of(result).is_some_and(borrowed) {
                            borrow_candidate.insert(result, (candidate, block));
                        }
                    }
                }
                if let MirOperation::Closure { captures, .. } = &instruction.operation {
                    for capture in captures {
                        if let MirCaptureOperand::Place(place) = capture {
                            if let Some(candidate) = place_candidate.get(place) {
                                rejected[*candidate] = true;
                            }
                        }
                    }
                }
            }
        }
        for (block, row) in self.blocks.iter().enumerate() {
            for instruction in &row.instructions {
                for value in instruction.operation.value_uses() {
                    if let Some((candidate, borrow_block)) = borrow_candidate.get(&value) {
                        if *borrow_block != block
                            || instruction.result.and_then(|result| mode_of(result)).is_some_and(borrowed)
                        {
                            rejected[*candidate] = true;
                        }
                    }
                }
            }
            for value in row.terminator.value_uses() {
                if let Some((candidate, borrow_block)) = borrow_candidate.get(&value) {
                    if *borrow_block != block {
                        rejected[*candidate] = true;
                    }
                }
            }
        }

        // Per-block events in execution order. Index `instructions.len()`
        // is the terminator.
        let mut events: Vec<Vec<(usize, Event)>> = vec![Vec::new(); block_count];
        for (block, row) in self.blocks.iter().enumerate() {
            let out = &mut events[block];
            for (index, instruction) in row.instructions.iter().enumerate() {
                let mut kills = Vec::new();
                for place in checked_operation_place_refs(&instruction.operation) {
                    let Some(candidate) = place_candidate.get(&place).copied() else {
                        continue;
                    };
                    let Candidate::Local { place: whole, .. } = candidates[candidate] else {
                        continue;
                    };
                    match &instruction.operation {
                        MirOperation::WritePlace { .. }
                        | MirOperation::ReplacePlace { .. }
                        | MirOperation::InitializeUninit { .. }
                            if place == whole =>
                        {
                            kills.push(candidate);
                        }
                        MirOperation::MovePlace { .. }
                            if place == whole && guarded[block] == Some(candidate) => {}
                        _ => out.push((index, Event::Use(candidate))),
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
                    }
                }
                for value in instruction.operation.value_uses() {
                    if let Some((candidate, _)) = borrow_candidate.get(&value) {
                        out.push((index, Event::Use(*candidate)));
                    }
                    if let Some(candidate) = subject_candidate.get(&value) {
                        out.push((index, Event::Use(*candidate)));
                    }
                    if let Some(&(candidate, copy_block, copy_index)) = copies.get(&value) {
                        if copy_block == block && copy_index < index {
                            let end = copy_window_ends.entry(value).or_insert(index);
                            *end = (*end).max(index);
                        } else {
                            out.push((index, Event::Use(candidate)));
                        }
                    }
                }
                if let Some(candidate) = instruction.result.and_then(|result| subject_candidate.get(&result)) {
                    kills.push(*candidate);
                }
                out.extend(kills.into_iter().map(|candidate| (index, Event::Kill(candidate))));
            }
            let end = row.instructions.len();
            for value in row.terminator.value_uses() {
                if let Some((candidate, _)) = borrow_candidate.get(&value) {
                    out.push((end, Event::Use(*candidate)));
                }
                if let Some(&(candidate, copy_block, _)) = copies.get(&value) {
                    if copy_block == block {
                        let window_end = copy_window_ends.entry(value).or_insert(end);
                        *window_end = (*window_end).max(end);
                    } else {
                        out.push((end, Event::Use(candidate)));
                    }
                }
            }
            if !reachable[block] {
                for (_, event) in out.iter() {
                    if let Event::Use(candidate) = event {
                        rejected[*candidate] = true;
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

        // Decide each read against the liveness right after it.
        let mut group_end: HashMap<(usize, usize), usize> = HashMap::new();
        for (group, row) in groups.iter().enumerate() {
            group_end.insert((row.block, *row.payloads.last().expect("payload group")), group);
        }
        let mut read_windows: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for (value, end) in &copy_window_ends {
            let (candidate, block, start) = copies[value];
            read_windows.entry((block, candidate)).or_default().push((start, *end));
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
                if let MirOperation::ReadPlace(place) = &instructions[index].operation {
                    if let Some(candidate) = place_candidate.get(place).copied() {
                        if let Candidate::Local { place: whole, .. } = candidates[candidate] {
                            if whole == *place
                                && !rejected[candidate]
                                && !bit_has(&live, candidate)
                                && !inside_read_window(block, index, candidate)
                            {
                                conversions.push((block, index, candidate));
                            }
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
            let Candidate::Local { place, flag } = candidates[candidate] else {
                unreachable!("read conversion targets a local candidate");
            };
            let (span, source_line, result) = {
                let instruction = &self.blocks[block].instructions[index];
                (instruction.span, instruction.source_line, instruction.result)
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
