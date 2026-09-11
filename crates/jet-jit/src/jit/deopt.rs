//! MIR frame snapshots for a Cranelift-to-interpreter handoff.
//!
//! The interpreter owns execution semantics. This module only carries the
//! versioned foundation MIR identities and the packed ABI frame values across
//! a host boundary.

use jet_foundation::MIR::{
    MirArtifactId, MirBlockId, MirExecutionIdentity, MirFrameIdentity, MirFunctionId, MirPlaceId,
    MirProgram, MirValueId,
};
use std::cell::RefCell;

#[derive(Debug, Clone)]
pub struct MirFrameValue {
    pub value: MirValueId,
    pub bits: u64,
}

#[derive(Debug, Clone)]
pub struct MirFramePlace {
    pub place: MirPlaceId,
    pub bits: u64,
}

#[derive(Debug, Clone)]
pub struct MirFrameSnapshot {
    pub identity: MirFrameIdentity,
    pub values: Vec<MirFrameValue>,
    pub places: Vec<MirFramePlace>,
}

#[derive(Debug, Clone)]
pub struct MirFrameSchema {
    pub execution: MirExecutionIdentity,
    pub function: MirFunctionId,
    pub values: Vec<MirValueId>,
    pub places: Vec<MirPlaceId>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DeoptState {
    schemas: Vec<MirFrameSchema>,
    last_snapshot: Option<MirFrameSnapshot>,
    sequence: u64,
}

pub(crate) struct DeoptStateGuard {
    previous: Option<DeoptState>,
}

thread_local! {
    static DEOPT_STATE: RefCell<DeoptState> = const { RefCell::new(DeoptState {
        schemas: Vec::new(),
        last_snapshot: None,
        sequence: 0,
    }) };
}

pub(crate) fn install_frame_schemas(
    program: &MirProgram,
    artifact: Option<MirArtifactId>,
) -> Result<(), String> {
    let execution = program
        .execution_identity(artifact)
        .map_err(|error| format!("MIR deopt execution identity unavailable: {error}"))?;
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.schemas.clear();
        state.last_snapshot = None;
        state.sequence = 0;
        state.schemas.extend(program.functions.iter().map(|function| MirFrameSchema {
            execution: execution.clone(),
            function: function.id,
            values: function.values.iter().map(|(id, _, _, _)| *id).collect(),
            places: function.places.iter().map(|place| place.id).collect(),
        }));
    });
    Ok(())
}

pub(crate) fn capture_deopt_state() -> DeoptState {
    DEOPT_STATE.with(|slot| slot.borrow().clone())
}

pub(crate) fn install_deopt_state(state: DeoptState) -> DeoptStateGuard {
    let previous = DEOPT_STATE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), state));
    DeoptStateGuard { previous: Some(previous) }
}

impl Drop for DeoptStateGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            DEOPT_STATE.with(|slot| *slot.borrow_mut() = previous);
        }
    }
}

pub fn frame_schema(function: MirFunctionId) -> Option<MirFrameSchema> {
    DEOPT_STATE.with(|slot| {
        slot.borrow()
            .schemas
            .iter()
            .find(|schema| schema.function == function)
            .cloned()
    })
}

pub fn last_snapshot() -> Option<MirFrameSnapshot> {
    DEOPT_STATE.with(|slot| slot.borrow().last_snapshot.clone())
}

pub(crate) fn record_frame_snapshot(
    program: &MirProgram,
    artifact: MirArtifactId,
    function: MirFunctionId,
    block: MirBlockId,
    values: Vec<MirFrameValue>,
    places: Vec<MirFramePlace>,
) -> Result<(), String> {
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let sequence = state.sequence;
        state.sequence = state.sequence.wrapping_add(1);
        let identity = program
            .frame_identity(Some(artifact), function, Some(block), sequence)
            .map_err(|error| format!("MIR deopt frame identity unavailable: {error}"))?;
        state.last_snapshot = Some(MirFrameSnapshot { identity, values, places });
        Ok(())
    })
}

fn record_abi_frame(function: i64, argc: i64, args: &[i64; 8]) {
    let Ok(function) = u64::try_from(function) else {
        return;
    };
    let Ok(argc) = usize::try_from(argc) else {
        return;
    };
    if argc > args.len() {
        return;
    }
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let Some((execution, schema_function, schema_values)) = state
            .schemas
            .iter()
            .find(|schema| schema.function == MirFunctionId(function))
            .map(|schema| {
                (
                    schema.execution.clone(),
                    schema.function,
                    schema.values.iter().copied().collect::<Vec<_>>(),
                )
            })
        else {
            return;
        };
        if argc > schema_values.len() {
            return;
        }
        let sequence = state.sequence;
        state.sequence = state.sequence.wrapping_add(1);
        let identity = MirFrameIdentity {
            schema_version: execution.schema_version,
            execution,
            function: schema_function,
            block: None,
            sequence,
        };
        let values = schema_values
            .into_iter()
            .zip(args.iter().copied())
            .take(argc)
            .map(|(value, bits)| MirFrameValue { value, bits: bits as u64 })
            .collect();
        state.last_snapshot = Some(MirFrameSnapshot {
            identity,
            values,
            places: Vec::new(),
        });
    });
}

pub(crate) fn clear_deopt_state() {
    DEOPT_STATE.with(|slot| *slot.borrow_mut() = DeoptState::default());
}

/// Host ABI entry used by every deopt-capable Cranelift module. A native
/// frame cannot resume without the interpreter's typed MIR frame object; the
/// explicit trap is therefore safer than guessing from packed words.
pub(crate) fn jet_deopt_call(
    function: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    crate::trace::note_deopt_invoked_for_test();
    record_abi_frame(function, argc, &[a0, a1, a2, a3, a4, a5, a6, a7]);
    if let Ok(function) = u64::try_from(function) {
        super::resident::publish_runtime_deopt(
            MirFunctionId(function),
            "native frame requested interpreter deopt",
        );
    }
    crate::Concurrency::with_runtime_mut(|runtime| {
        runtime.set_host_fault("MIR deopt requested without an interpreter frame");
    });
    0
}
