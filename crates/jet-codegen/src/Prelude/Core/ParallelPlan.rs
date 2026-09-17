// D-PARCAPTURE1=D / D-ACCEL1=A: typed facts for explicit `para_*` plans and
// measured release acceleration.
//
// This file proves callback facts, records the D-ACCEL1 measurement row, and
// reuses the indexed chunks consumed by ParallelKernel.  It never creates a
// second scheduler or lets an unproved callback cross the worker boundary.

use std::fmt;
use std::ops::Range;

/// The chunk size shared by the existing indexed parallel collection kernel.
pub const JET_PARALLEL_DEFAULT_CHUNK_ITEMS: usize = 64;

/// A malformed resource policy must not be able to request an unbounded chunk
/// vector.  This is a planning bound, not a workload limit imposed on callers.
pub const JET_PARALLEL_DEFAULT_MAX_CHUNKS: usize = 1 << 20;
/// D-ACCEL1=A: no timing or process-cost measurement is allowed at or below
/// this source-item floor.
pub const JET_ACCEL_STATIC_FLOOR_ITEMS: usize = 1024;
/// D-ACCEL1=A: transient column copies use one private cache block at a time.
pub const JET_ACCEL_CACHE_BLOCK_BYTES: usize = 64 * 1024;
pub const JET_ACCEL_REQUIRED_GAIN_NUMERATOR: u64 = 2;
pub const JET_ACCEL_REQUIRED_GAIN_DENOMINATOR: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetAccelerationPin {
    Scalar,
    Local,
}

impl JetAccelerationPin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "#Scalar",
            Self::Local => "#Local",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetAccelerationTransform {
    TransientColumnCopy,
    PooledParallelChunks,
}

impl JetAccelerationTransform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TransientColumnCopy => "column-copy",
            Self::PooledParallelChunks => "parallel",
        }
    }
}

/// Host costs measured lazily through the existing bounded chunk engine.
/// `LazyLock` makes both values once-per-process facts, not per-loop probes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JetAccelerationRuntimeCosts {
    pub spawn_nanos: u64,
    pub bandwidth_floor_nanos: u64,
    pub spawn_cost_once_per_process: bool,
    pub bandwidth_floor_measured: bool,
}

impl JetAccelerationRuntimeCosts {
    pub const fn unavailable() -> Self {
        Self {
            spawn_nanos: 0,
            bandwidth_floor_nanos: 0,
            spawn_cost_once_per_process: false,
            bandwidth_floor_measured: false,
        }
    }

    pub fn once(items: usize) -> Self {
        if items <= JET_ACCEL_STATIC_FLOOR_ITEMS {
            return Self::unavailable();
        }
        static COSTS: std::sync::LazyLock<JetAccelerationRuntimeCosts> =
            std::sync::LazyLock::new(|| JetAccelerationRuntimeCosts {
                spawn_nanos: jet_acceleration_measure_spawn_nanos(),
                bandwidth_floor_nanos: jet_acceleration_measure_bandwidth_nanos(),
                spawn_cost_once_per_process: true,
                bandwidth_floor_measured: true,
            });
        *COSTS
    }
}

/// Result of the one serial first-chunk probe.  The value is retained so a
/// selected transform never evaluates the first source chunk twice.
#[derive(Debug)]
pub(crate) struct JetAccelerationSample<R> {
    pub value: R,
    pub sample_items: usize,
    pub sample_nanos: u64,
}

/// Inputs to the runtime copy/parallel gate.  Proof and parity are supplied by
/// sema/MIR adapters; this layer only combines them with measured host facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JetAccelerationGateInput {
    pub items: usize,
    pub sample_items: usize,
    pub sample_nanos: u64,
    pub spawn_nanos: u64,
    pub bandwidth_floor_nanos: u64,
    pub first_chunk_serial_timed: bool,
    pub spawn_cost_once_per_process: bool,
    pub bandwidth_floor_measured: bool,
    pub release_build: bool,
    pub cross_mode_parity_proven: bool,
    pub pin: Option<JetAccelerationPin>,
    pub nested_reuse: bool,
    pub single_pass: bool,
    pub proof_proven: bool,
}

impl JetAccelerationGateInput {
    pub const fn deferred(items: usize, release_build: bool) -> Self {
        Self {
            items,
            sample_items: 0,
            sample_nanos: 0,
            spawn_nanos: 0,
            bandwidth_floor_nanos: 0,
            first_chunk_serial_timed: false,
            spawn_cost_once_per_process: false,
            bandwidth_floor_measured: false,
            release_build,
            cross_mode_parity_proven: false,
            pin: None,
            nested_reuse: false,
            single_pass: false,
            proof_proven: false,
        }
    }

    pub fn measured(
        items: usize,
        sample_items: usize,
        sample_nanos: u64,
        costs: JetAccelerationRuntimeCosts,
        release_build: bool,
        cross_mode_parity_proven: bool,
        proof_proven: bool,
        nested_reuse: bool,
        single_pass: bool,
        pin: Option<JetAccelerationPin>,
    ) -> Self {
        Self {
            items,
            sample_items,
            sample_nanos,
            spawn_nanos: costs.spawn_nanos,
            bandwidth_floor_nanos: costs.bandwidth_floor_nanos,
            first_chunk_serial_timed: sample_items != 0 && sample_nanos != 0,
            spawn_cost_once_per_process: costs.spawn_cost_once_per_process,
            bandwidth_floor_measured: costs.bandwidth_floor_measured,
            release_build,
            cross_mode_parity_proven,
            pin,
            nested_reuse,
            single_pass,
            proof_proven,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetAccelerationGateStatus {
    BelowStaticFloor,
    Pinned(JetAccelerationPin),
    NotReleaseBuild,
    CrossModeParityMissing,
    ColumnCopyNotApplicable,
    ProofIncomplete,
    SampleNotTimed,
    SpawnCostNotMeasured,
    BandwidthFloorNotMeasured,
    InvalidMeasurement,
    ProjectedGainBelowThreshold,
    Selected,
}

impl JetAccelerationGateStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BelowStaticFloor => "below-static-floor",
            Self::Pinned(_) => "pinned",
            Self::NotReleaseBuild => "not-release-build",
            Self::CrossModeParityMissing => "cross-mode-parity-missing",
            Self::ColumnCopyNotApplicable => "column-copy-not-applicable",
            Self::ProofIncomplete => "proof-incomplete",
            Self::SampleNotTimed => "sample-not-timed",
            Self::SpawnCostNotMeasured => "spawn-cost-not-measured",
            Self::BandwidthFloorNotMeasured => "bandwidth-floor-not-measured",
            Self::InvalidMeasurement => "invalid-measurement",
            Self::ProjectedGainBelowThreshold => "projected-gain-below-threshold",
            Self::Selected => "selected",
        }
    }

    pub const fn is_selected(self) -> bool {
        matches!(self, Self::Selected)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetAccelerationMeasurement {
    pub items: usize,
    pub sample_items: usize,
    pub sample_nanos: u64,
    pub spawn_nanos: u64,
    pub bandwidth_floor_nanos: u64,
    pub floor_nanos: u64,
    pub projected_serial_nanos: Option<u64>,
    pub required_serial_nanos: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetAccelerationDecision {
    pub transform: JetAccelerationTransform,
    pub status: JetAccelerationGateStatus,
    pub measurement: JetAccelerationMeasurement,
}

impl JetAccelerationDecision {
    pub const fn selected(self) -> bool {
        self.status.is_selected()
    }

    pub fn inspect_line(self) -> String {
        format!(
            "accel transform={} status={} items={} sample_items={} sample_ns={} spawn_ns={} bandwidth_ns={} projected_ns={:?} required_ns={:?}",
            self.transform.as_str(),
            self.status.as_str(),
            self.measurement.items,
            self.measurement.sample_items,
            self.measurement.sample_nanos,
            self.measurement.spawn_nanos,
            self.measurement.bandwidth_floor_nanos,
            self.measurement.projected_serial_nanos,
            self.measurement.required_serial_nanos,
        )
    }
}

const JET_ACCELERATION_RECEIPT_TYPE_NAME: &str = "AccelerationDecision";
const JET_ACCELERATION_RECEIPT_SECTION_PREFIX: &str = "jet.acceleration.";
const JET_ACCELERATION_RECEIPT_SCHEMA_DIGEST: &str =
    "fb216702b04de0611c6ceeb7b92efeeec9b91d3076277f38baac13475962d6d1";
static JET_ACCELERATION_RECEIPT_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Publish one measured decision through the existing typed receipt rail.
///
/// The generated runtime has no profile store of its own.  When a host has
/// enabled receipt capture, this appends a bounded canonical section that the
/// host receipt/perf projections can read; without that environment this path
/// is a no-op and does not allocate or take a clock.
pub fn jet_acceleration_publish(
    function: &str,
    loop_header: u32,
    source_start: usize,
    source_end: usize,
    decision: &JetAccelerationDecision,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (function, loop_header, source_start, source_end, decision);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if std::env::var_os("JET_RECEIPT_DIR").is_none()
            || std::env::var_os("JET_RECEIPT_CLAIM").is_none()
        {
            return;
        }
        let sequence = JET_ACCELERATION_RECEIPT_SEQUENCE
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let section_name = format!("{JET_ACCELERATION_RECEIPT_SECTION_PREFIX}{sequence}");
        let payload = jet_acceleration_receipt_payload(
            function,
            loop_header,
            source_start,
            source_end,
            decision,
        );
        let payload = jet_std::render_datatree_json(&payload, false, 0);
        jet_receipt_attach_encoded(
            &payload,
            &section_name,
            JET_ACCELERATION_RECEIPT_TYPE_NAME,
            JET_ACCELERATION_RECEIPT_SCHEMA_DIGEST,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_acceleration_receipt_payload(
    function: &str,
    loop_header: u32,
    source_start: usize,
    source_end: usize,
    decision: &JetAccelerationDecision,
) -> jet_std::DataTree {
    let measurement = jet_std::DataTree::Object(vec![
        (
            "bandwidth_floor_nanos".into(),
            jet_acceleration_int(decision.measurement.bandwidth_floor_nanos),
        ),
        (
            "floor_nanos".into(),
            jet_acceleration_int(decision.measurement.floor_nanos),
        ),
        ("items".into(), jet_acceleration_int(decision.measurement.items)),
        (
            "projected_serial_nanos".into(),
            jet_acceleration_optional_int(decision.measurement.projected_serial_nanos),
        ),
        (
            "required_serial_nanos".into(),
            jet_acceleration_optional_int(decision.measurement.required_serial_nanos),
        ),
        (
            "sample_items".into(),
            jet_acceleration_int(decision.measurement.sample_items),
        ),
        (
            "sample_nanos".into(),
            jet_acceleration_int(decision.measurement.sample_nanos),
        ),
        (
            "spawn_nanos".into(),
            jet_acceleration_int(decision.measurement.spawn_nanos),
        ),
    ]);
    let pin = match decision.status {
        JetAccelerationGateStatus::Pinned(pin) => {
            jet_std::DataTree::Text(pin.as_str().to_string())
        }
        _ => jet_std::DataTree::Null,
    };
    let decision = jet_std::DataTree::Object(vec![
        ("measurement".into(), measurement),
        ("pin".into(), pin),
        (
            "status".into(),
            jet_std::DataTree::Text(decision.status.as_str().to_string()),
        ),
        (
            "transform".into(),
            jet_std::DataTree::Text(decision.transform.as_str().to_string()),
        ),
    ]);
    jet_std::DataTree::Object(vec![
        ("decision".into(), decision),
        (
            "function".into(),
            jet_std::DataTree::Text(function.to_string()),
        ),
        (
            "loop_header".into(),
            jet_std::DataTree::Int(i64::from(loop_header)),
        ),
        (
            "source".into(),
            jet_std::DataTree::Object(vec![
                ("end".into(), jet_acceleration_int(source_end)),
                ("start".into(), jet_acceleration_int(source_start)),
            ]),
        ),
    ])
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_acceleration_int(value: impl TryInto<i64> + Copy) -> jet_std::DataTree {
    jet_std::DataTree::Int(value.try_into().unwrap_or(i64::MAX))
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_acceleration_optional_int(value: Option<u64>) -> jet_std::DataTree {
    value
        .map(|value| jet_acceleration_int(value))
        .unwrap_or(jet_std::DataTree::Null)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JetAccelerationGate {
    pub static_floor_items: usize,
    pub cache_block_bytes: usize,
    pub required_gain_numerator: u64,
    pub required_gain_denominator: u64,
}

impl JetAccelerationGate {
    pub const fn d_accel1() -> Self {
        Self {
            static_floor_items: JET_ACCEL_STATIC_FLOOR_ITEMS,
            cache_block_bytes: JET_ACCEL_CACHE_BLOCK_BYTES,
            required_gain_numerator: JET_ACCEL_REQUIRED_GAIN_NUMERATOR,
            required_gain_denominator: JET_ACCEL_REQUIRED_GAIN_DENOMINATOR,
        }
    }

    pub const fn should_measure(self, items: usize) -> bool {
        items > self.static_floor_items
    }

    pub fn evaluate(
        self,
        transform: JetAccelerationTransform,
        input: JetAccelerationGateInput,
    ) -> JetAccelerationDecision {
        let measurement = self.measurement(input);
        let status = if input.items <= self.static_floor_items {
            JetAccelerationGateStatus::BelowStaticFloor
        } else if let Some(pin) = input.pin {
            JetAccelerationGateStatus::Pinned(pin)
        } else if !input.release_build {
            JetAccelerationGateStatus::NotReleaseBuild
        } else if !input.cross_mode_parity_proven {
            JetAccelerationGateStatus::CrossModeParityMissing
        } else if matches!(transform, JetAccelerationTransform::TransientColumnCopy)
            && (!input.nested_reuse || input.single_pass)
        {
            JetAccelerationGateStatus::ColumnCopyNotApplicable
        } else if !input.proof_proven {
            JetAccelerationGateStatus::ProofIncomplete
        } else if !input.first_chunk_serial_timed {
            JetAccelerationGateStatus::SampleNotTimed
        } else if !input.spawn_cost_once_per_process {
            JetAccelerationGateStatus::SpawnCostNotMeasured
        } else if !input.bandwidth_floor_measured {
            JetAccelerationGateStatus::BandwidthFloorNotMeasured
        } else if input.sample_items == 0
            || input.sample_items > input.items
            || input.sample_nanos == 0
            || input.spawn_nanos == 0
            || input.bandwidth_floor_nanos == 0
            || self.required_gain_numerator == 0
            || self.required_gain_denominator == 0
        {
            JetAccelerationGateStatus::InvalidMeasurement
        } else if measurement.projected_serial_nanos.is_none()
            || measurement.required_serial_nanos.is_none()
        {
            JetAccelerationGateStatus::InvalidMeasurement
        } else if measurement.projected_serial_nanos < measurement.required_serial_nanos {
            JetAccelerationGateStatus::ProjectedGainBelowThreshold
        } else {
            JetAccelerationGateStatus::Selected
        };
        JetAccelerationDecision {
            transform,
            status,
            measurement,
        }
    }

    fn measurement(self, input: JetAccelerationGateInput) -> JetAccelerationMeasurement {
        let floor_nanos = input.spawn_nanos.max(input.bandwidth_floor_nanos);
        let projected_serial_nanos = match (
            u64::try_from(input.items).ok(),
            u64::try_from(input.sample_items).ok(),
        ) {
            (Some(items), Some(sample_items)) if sample_items != 0 => input
                .sample_nanos
                .checked_mul(items)
                .and_then(|value| value.checked_add(sample_items - 1))
                .map(|value| value / sample_items),
            _ => None,
        };
        let required_serial_nanos = if self.required_gain_denominator == 0 {
            None
        } else {
            floor_nanos
                .checked_mul(self.required_gain_numerator)
                .and_then(|value| value.checked_add(self.required_gain_denominator - 1))
                .map(|value| value / self.required_gain_denominator)
        };
        JetAccelerationMeasurement {
            items: input.items,
            sample_items: input.sample_items,
            sample_nanos: input.sample_nanos,
            spawn_nanos: input.spawn_nanos,
            bandwidth_floor_nanos: input.bandwidth_floor_nanos,
            floor_nanos,
            projected_serial_nanos,
            required_serial_nanos,
        }
    }
}

impl Default for JetAccelerationGate {
    fn default() -> Self {
        Self::d_accel1()
    }
}

fn jet_acceleration_measure_spawn_nanos() -> u64 {
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let items = JET_PARALLEL_DEFAULT_CHUNK_ITEMS
        .saturating_mul(workers.max(2));
    let start = std::time::Instant::now();
    let _ = jet_list_para_chunks_kernel(items, usize::MAX, workers, |_| Ok::<(), ()>(()));
    start.elapsed().as_nanos().max(1) as u64
}

fn jet_acceleration_measure_bandwidth_nanos() -> u64 {
    let mut source = vec![0_u8; JET_ACCEL_CACHE_BLOCK_BYTES];
    let mut target = vec![0_u8; JET_ACCEL_CACHE_BLOCK_BYTES];
    for (index, value) in source.iter_mut().enumerate() {
        *value = index as u8;
    }
    let start = std::time::Instant::now();
    target.copy_from_slice(&source);
    let checksum = target
        .iter()
        .fold(0_u8, |sum, value| sum.wrapping_add(*value));
    std::hint::black_box(checksum);
    start.elapsed().as_nanos().max(1) as u64
}

/// Above the floor this runs exactly the first existing 64-item chunk once.
/// At or below the floor it returns `None` without reading a clock.
pub(crate) fn jet_acceleration_first_chunk<R, F>(
    items: usize,
    f: F,
) -> Option<JetAccelerationSample<R>>
where
    F: FnOnce(Range<usize>) -> R,
{
    if items <= JET_ACCEL_STATIC_FLOOR_ITEMS || items == 0 {
        return None;
    }
    let sample_items = items.min(JET_PARALLEL_DEFAULT_CHUNK_ITEMS);
    let start = std::time::Instant::now();
    let value = f(0..sample_items);
    let sample_nanos = start.elapsed().as_nanos().max(1) as u64;
    Some(JetAccelerationSample {
        value,
        sample_items,
        sample_nanos,
    })
}

/// Complete a column-copy map from an optional retained typed first-chunk
/// probe. The probe is emitted exactly once: a selected decision copies only
/// the remaining range, a rejected measured decision evaluates only that tail
/// serially, and no probe falls back to the original full-range operation.
pub(crate) fn jet_list_accel_column_range_map_with_sample<U, R, P, C, F>(
    range: Range<usize>,
    sample: Option<JetAccelerationSample<Vec<R>>>,
    gate_selected: bool,
    project: P,
    copied_kernel: C,
    original_kernel: F,
) -> Vec<R>
where
    P: Fn(usize) -> U,
    C: Fn(&[U], Range<usize>) -> Vec<R>,
    F: FnOnce(Range<usize>) -> Vec<R>,
{
    let Some(sample) = sample else {
        return original_kernel(range);
    };
    debug_assert_eq!(sample.value.len(), sample.sample_items);
    let sample_end = range
        .start
        .saturating_add(sample.sample_items)
        .min(range.end);
    let mut result = sample.value;
    result.reserve(range.end.saturating_sub(sample_end));
    if sample_end >= range.end {
        return result;
    }
    let tail = if gate_selected {
        jet_list_accel_column_copy_range_map(
            sample_end..range.end,
            project,
            copied_kernel,
        )
    } else {
        original_kernel(sample_end..range.end)
    };
    result.extend(tail);
    result
}

/// Measured D-ACCEL1 map over an index range.  The range is never materialized:
/// the first serial chunk is retained, and only its remaining relative ranges
/// enter the existing bounded `para_*` scheduler.
pub(crate) fn jet_list_accel_range_map<U, F>(
    range: Range<usize>,
    worker_limit: usize,
    release_build: bool,
    cross_mode_parity_proven: bool,
    proof_proven: bool,
    pin: Option<JetAccelerationPin>,
    f: F,
) -> (Vec<U>, JetAccelerationDecision)
where
    U: Send,
    F: Fn(usize) -> U + Sync,
{
    let gate = JetAccelerationGate::d_accel1();
    let start = range.start;
    let end = range.end;
    let items = end.saturating_sub(start);
    // A caller with one worker has no parallel plan to justify a timing probe.
    // Keep the whole operation serial and let the gate record a deferred
    // decision rather than timing a kernel that cannot accelerate the work.
    if items <= JET_ACCEL_STATIC_FLOOR_ITEMS
        || worker_limit <= 1
        || !release_build
        || !cross_mode_parity_proven
        || !proof_proven
    {
        let input = JetAccelerationGateInput::deferred(items, release_build);
        let input = JetAccelerationGateInput {
            cross_mode_parity_proven,
            proof_proven,
            pin,
            ..input
        };
        let decision = gate.evaluate(JetAccelerationTransform::PooledParallelChunks, input);
        let result = (start..end).map(&f).collect();
        return (result, decision);
    }

    let Some(sample) = jet_acceleration_first_chunk(items, |relative| {
        relative
            .map(|index| f(start + index))
            .collect::<Vec<_>>()
    }) else {
        unreachable!("D-ACCEL1 first chunk must exist above the static floor");
    };
    let costs = JetAccelerationRuntimeCosts::once(items);
    let input = JetAccelerationGateInput::measured(
        items,
        sample.sample_items,
        sample.sample_nanos,
        costs,
        release_build,
        cross_mode_parity_proven,
        proof_proven,
        false,
        false,
        pin,
    );
    let decision = gate.evaluate(JetAccelerationTransform::PooledParallelChunks, input);
    let mut result = sample.value;
    if decision.selected() {
        let tail = jet_list_para_chunks(
            items - sample.sample_items,
            worker_limit.max(1),
            |relative_range| {
                let mut output = Vec::with_capacity(relative_range.len());
                for relative in relative_range {
                    let index = start + sample.sample_items + relative;
                    output.push(jet_para_call(index, || f(index))?);
                }
                Ok(output)
            },
        );
        result.extend(tail.into_iter().flatten());
    } else {
        result.extend((start + sample.sample_items..end).map(&f));
    }
    (result, decision)
}

/// Slice convenience wrapper for callers that already own a collection.  The
/// range implementation above is the canonical path, so no index vector is
/// introduced by this adapter.
pub(crate) fn jet_list_accel_map<T, U, F>(
    source: &[T],
    worker_limit: usize,
    release_build: bool,
    cross_mode_parity_proven: bool,
    proof_proven: bool,
    pin: Option<JetAccelerationPin>,
    f: F,
) -> (Vec<U>, JetAccelerationDecision)
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    jet_list_accel_range_map(
        0..source.len(),
        worker_limit,
        release_build,
        cross_mode_parity_proven,
        proof_proven,
        pin,
        |index| f(&source[index]),
    )
}

/// The source operation remains visible at the call site.  Explicit `para_*`
/// calls and the D-ACCEL1 measured path share these operation facts.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelOperation {
    Map,
    Filter,
    Partition,
    Fold,
}

impl JetParallelOperation {
    pub const fn surface_name(self) -> &'static str {
        match self {
            Self::Map => "para_map",
            Self::Filter => "para_filter",
            Self::Partition => "para_partition",
            Self::Fold => "para_fold",
        }
    }

    pub const fn is_reduction(self) -> bool {
        matches!(self, Self::Fold)
    }
}

impl fmt::Display for JetParallelOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.surface_name())
    }
}

/// The loop domain known to sema.  An unknown domain is retained as a typed
/// fact so a rejection can say why chunking was not possible; it is never
/// guessed from a backend carrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetParallelIterationDomain {
    Indexed { start: usize, end: usize },
    Unknown { reason: String },
}

impl JetParallelIterationDomain {
    pub const fn indexed(start: usize, end: usize) -> Self {
        Self::Indexed { start, end }
    }

    pub const fn from_len(len: usize) -> Self {
        Self::Indexed { start: 0, end: len }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }

    pub const fn start(&self) -> Option<usize> {
        match self {
            Self::Indexed { start, .. } => Some(*start),
            Self::Unknown { .. } => None,
        }
    }

    pub const fn end(&self) -> Option<usize> {
        match self {
            Self::Indexed { end, .. } => Some(*end),
            Self::Unknown { .. } => None,
        }
    }

    pub fn len(&self) -> Option<usize> {
        match self {
            Self::Indexed { start, end } => end.checked_sub(*start),
            Self::Unknown { .. } => None,
        }
    }

    pub fn range(&self) -> Option<Range<usize>> {
        match self {
            Self::Indexed { start, end } if *start <= *end => Some(*start..*end),
            Self::Indexed { .. } | Self::Unknown { .. } => None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.range().is_some()
    }

    fn rejection_reason(&self) -> String {
        match self {
            Self::Indexed { start, end } if start > end => {
                format!("indexed domain has start {start} after end {end}")
            }
            Self::Indexed { .. } => String::new(),
            Self::Unknown { reason } if reason.is_empty() => {
                "iteration bounds were not exposed by sema".to_string()
            }
            Self::Unknown { reason } => reason.clone(),
        }
    }
}

/// Whether an access can observe or mutate a location.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelAccessMode {
    Read,
    Write,
    ReadWrite,
    Reduce,
}

impl JetParallelAccessMode {
    pub const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite | Self::Reduce)
    }

    pub const fn is_reduction(self) -> bool {
        matches!(self, Self::Reduce)
    }
}

impl fmt::Display for JetParallelAccessMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read/write",
            Self::Reduce => "reduction",
        })
    }
}

/// The relation between an access and the current iteration.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelAccessScope {
    /// The same location is touched by every iteration.
    Invariant,
    /// Each iteration owns a location derived from its index.
    IterationLocal,
    /// Sema proved two iteration projections cannot overlap.
    Disjoint,
    /// The callback did not expose enough place information.
    Unknown,
}

impl fmt::Display for JetParallelAccessScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invariant => "invariant",
            Self::IterationLocal => "iteration-local",
            Self::Disjoint => "disjoint",
            Self::Unknown => "unknown",
        })
    }
}

/// One typed place/access fact.  `place` is the stable sema identity used in a
/// diagnostic; it is not a backend field map or a runtime address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelAccessFact {
    pub place: String,
    pub mode: JetParallelAccessMode,
    pub scope: JetParallelAccessScope,
}

impl JetParallelAccessFact {
    pub fn new(
        place: impl Into<String>,
        mode: JetParallelAccessMode,
        scope: JetParallelAccessScope,
    ) -> Self {
        Self {
            place: place.into(),
            mode,
            scope,
        }
    }

    pub fn read(place: impl Into<String>) -> Self {
        Self::new(place, JetParallelAccessMode::Read, JetParallelAccessScope::Invariant)
    }

    pub fn read_local(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelAccessMode::Read,
            JetParallelAccessScope::IterationLocal,
        )
    }

    pub fn write(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelAccessMode::Write,
            JetParallelAccessScope::Invariant,
        )
    }

    pub fn write_local(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelAccessMode::Write,
            JetParallelAccessScope::IterationLocal,
        )
    }

    pub fn read_write(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelAccessMode::ReadWrite,
            JetParallelAccessScope::Invariant,
        )
    }

    pub fn reduce(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelAccessMode::Reduce,
            JetParallelAccessScope::Invariant,
        )
    }

    pub fn disjoint(
        place: impl Into<String>,
        mode: JetParallelAccessMode,
    ) -> Self {
        Self::new(place, mode, JetParallelAccessScope::Disjoint)
    }

    pub fn is_proven_disjoint(&self) -> bool {
        matches!(
            self.scope,
            JetParallelAccessScope::IterationLocal | JetParallelAccessScope::Disjoint
        )
    }

    pub fn description(&self) -> String {
        format!(
            "{} `{}` ({})",
            self.mode, self.place, self.scope
        )
    }
}

/// An effect fact is separate from place access: a callback may be read-only
/// with respect to its input and still perform an external effect.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelEffectKind {
    Pure,
    ReadOnly,
    SharedRead,
    Reduction,
    MayFail,
    CancellationPoint,
    SharedWrite,
    External,
    Unknown,
}

impl fmt::Display for JetParallelEffectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pure => "pure",
            Self::ReadOnly => "read-only",
            Self::SharedRead => "synchronized read",
            Self::Reduction => "reduction",
            Self::MayFail => "fallible",
            Self::CancellationPoint => "cancellation point",
            Self::SharedWrite => "shared write",
            Self::External => "external effect",
            Self::Unknown => "unknown effect",
        })
    }
}

/// One typed callback effect.  `MayFail` and `CancellationPoint` are allowed:
/// the receipt rail makes their outcomes deterministic without pretending that
/// work already performed can be rolled back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelEffectFact {
    pub name: String,
    pub kind: JetParallelEffectKind,
}

impl JetParallelEffectFact {
    pub fn new(name: impl Into<String>, kind: JetParallelEffectKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }

    pub fn pure() -> Self {
        Self::new("callback", JetParallelEffectKind::Pure)
    }

    pub fn read_only(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::ReadOnly)
    }

    pub fn shared_read(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::SharedRead)
    }

    pub fn reduction(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::Reduction)
    }

    pub fn may_fail(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::MayFail)
    }

    pub fn cancellation_point(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::CancellationPoint)
    }

    pub fn shared_write(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::SharedWrite)
    }

    pub fn external(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::External)
    }

    pub fn unknown(name: impl Into<String>) -> Self {
        Self::new(name, JetParallelEffectKind::Unknown)
    }

    fn rejection_reason(&self, operation: JetParallelOperation, has_reduction: bool) -> Option<String> {
        let reason = match self.kind {
            JetParallelEffectKind::Pure
            | JetParallelEffectKind::ReadOnly
            | JetParallelEffectKind::SharedRead
            | JetParallelEffectKind::MayFail
            | JetParallelEffectKind::CancellationPoint => return None,
            JetParallelEffectKind::Reduction if operation.is_reduction() && has_reduction => {
                return None
            }
            JetParallelEffectKind::Reduction => {
                "reduction effect needs a proven identity and associative merge law"
            }
            JetParallelEffectKind::SharedWrite => {
                "shared writes can race; use explicit synchronized state outside the callback"
            }
            JetParallelEffectKind::External => {
                "external effects observe callback order and are not rollback-safe"
            }
            JetParallelEffectKind::Unknown => {
                "effect facts were not classified by sema"
            }
        };
        Some(format!("effect `{}` is {kind}: {reason}", self.name, kind = self.kind))
    }
}

/// A dependency relation between two iterations.  `Independent` is a positive
/// witness; the other variants are retained so rejection text names the exact
/// relation instead of collapsing all failures into "not parallel".
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelDependencyKind {
    Independent,
    ReadAfterWrite,
    WriteAfterRead,
    WriteAfterWrite,
    LoopCarried,
    Unknown,
}

impl JetParallelDependencyKind {
    pub const fn conflicts(self) -> bool {
        !matches!(self, Self::Independent)
    }
}

impl fmt::Display for JetParallelDependencyKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Independent => "independent",
            Self::ReadAfterWrite => "read-after-write",
            Self::WriteAfterRead => "write-after-read",
            Self::WriteAfterWrite => "write-after-write",
            Self::LoopCarried => "loop-carried",
            Self::Unknown => "unknown",
        })
    }
}

/// One source-level dependency fact.  Iteration numbers are optional because a
/// loop-carried dependency may be known symbolically rather than at one pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelDependencyFact {
    pub place: String,
    pub kind: JetParallelDependencyKind,
    pub source_iteration: Option<usize>,
    pub target_iteration: Option<usize>,
    pub detail: String,
}

impl JetParallelDependencyFact {
    pub fn new(
        place: impl Into<String>,
        kind: JetParallelDependencyKind,
        source_iteration: Option<usize>,
        target_iteration: Option<usize>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            place: place.into(),
            kind,
            source_iteration,
            target_iteration,
            detail: detail.into(),
        }
    }

    pub fn independent(place: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::Independent,
            None,
            None,
            "sema proved disjoint iteration projections",
        )
    }

    pub fn read_after_write(
        place: impl Into<String>,
        source_iteration: Option<usize>,
        target_iteration: Option<usize>,
    ) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::ReadAfterWrite,
            source_iteration,
            target_iteration,
            "a later read observes an earlier iteration write",
        )
    }

    pub fn write_after_read(
        place: impl Into<String>,
        source_iteration: Option<usize>,
        target_iteration: Option<usize>,
    ) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::WriteAfterRead,
            source_iteration,
            target_iteration,
            "a later write follows an earlier iteration read",
        )
    }

    pub fn write_after_write(
        place: impl Into<String>,
        source_iteration: Option<usize>,
        target_iteration: Option<usize>,
    ) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::WriteAfterWrite,
            source_iteration,
            target_iteration,
            "iterations write the same location",
        )
    }

    pub fn loop_carried(place: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::LoopCarried,
            None,
            None,
            detail,
        )
    }

    pub fn unknown(place: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(
            place,
            JetParallelDependencyKind::Unknown,
            None,
            None,
            detail,
        )
    }

    pub fn description(&self) -> String {
        let pair = match (self.source_iteration, self.target_iteration) {
            (Some(source), Some(target)) => format!(" from iteration {source} to {target}"),
            _ => String::new(),
        };
        if self.detail.is_empty() {
            format!("{} on `{}`{}", self.kind, self.place, pair)
        } else {
            format!("{} on `{}`{}: {}", self.kind, self.place, pair, self.detail)
        }
    }
}

/// The order used to combine fold partials.  The stable adjacent tree does not
/// require commutativity; associativity and a two-sided identity are the laws
/// that make the parallel fold equivalent to its serial definition.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelReductionOrder {
    SourceOrder,
    StableAdjacentPair,
}

impl fmt::Display for JetParallelReductionOrder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceOrder => "source-order",
            Self::StableAdjacentPair => "stable-adjacent-pair",
        })
    }
}

/// Proof facts for a `para_fold` merge.  A deterministic tree alone is not a
/// portable reduction law: identity and associativity must also be proved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelReductionFact {
    pub operator: String,
    pub identity_proved: bool,
    pub associative: bool,
    pub commutative: bool,
    pub order: JetParallelReductionOrder,
}

impl JetParallelReductionFact {
    pub fn new(
        operator: impl Into<String>,
        identity_proved: bool,
        associative: bool,
    ) -> Self {
        Self {
            operator: operator.into(),
            identity_proved,
            associative,
            commutative: false,
            order: JetParallelReductionOrder::StableAdjacentPair,
        }
    }

    pub fn proven(operator: impl Into<String>) -> Self {
        Self::new(operator, true, true)
    }

    pub fn with_commutativity(mut self, commutative: bool) -> Self {
        self.commutative = commutative;
        self
    }

    pub fn with_order(mut self, order: JetParallelReductionOrder) -> Self {
        self.order = order;
        self
    }

    pub fn is_proven(&self) -> bool {
        !self.operator.trim().is_empty()
            && self.identity_proved
            && self.associative
            && matches!(self.order, JetParallelReductionOrder::StableAdjacentPair)
    }

    pub fn rejection_reason(&self) -> Option<String> {
        if self.operator.trim().is_empty() {
            return Some("reduction operator is unnamed".to_string());
        }
        if !self.identity_proved {
            return Some("merge identity is not proven on both sides".to_string());
        }
        if !self.associative {
            return Some("merge is not proven associative".to_string());
        }
        if !matches!(self.order, JetParallelReductionOrder::StableAdjacentPair) {
            return Some(format!(
                "reduction order `{}` is not the stable adjacent-pair tree",
                self.order
            ));
        }
        None
    }
}

/// Resource inputs are explicit facts supplied by the caller/host.  The
/// planner never queries `available_parallelism`; the later kernel may use the
/// same bounds when it runs the already selected plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetParallelResourceBounds {
    pub requested_workers: usize,
    pub available_workers: usize,
    pub chunk_items: usize,
    pub max_chunks: usize,
}

impl Default for JetParallelResourceBounds {
    fn default() -> Self {
        Self {
            requested_workers: 1,
            available_workers: 1,
            chunk_items: JET_PARALLEL_DEFAULT_CHUNK_ITEMS,
            max_chunks: JET_PARALLEL_DEFAULT_MAX_CHUNKS,
        }
    }
}

impl JetParallelResourceBounds {
    pub const fn new(
        requested_workers: usize,
        available_workers: usize,
        chunk_items: usize,
        max_chunks: usize,
    ) -> Self {
        Self {
            requested_workers,
            available_workers,
            chunk_items,
            max_chunks,
        }
    }

    pub const fn explicit(workers: usize, available_workers: usize) -> Self {
        Self {
            requested_workers: workers,
            available_workers,
            chunk_items: JET_PARALLEL_DEFAULT_CHUNK_ITEMS,
            max_chunks: JET_PARALLEL_DEFAULT_MAX_CHUNKS,
        }
    }

    pub const fn with_chunk_items(self, chunk_items: usize) -> Self {
        Self { chunk_items, ..self }
    }

    pub const fn with_max_chunks(self, max_chunks: usize) -> Self {
        Self { max_chunks, ..self }
    }

    pub const fn worker_limit(self) -> usize {
        if self.requested_workers == 0 {
            1
        } else {
            self.requested_workers
        }
    }

    pub fn selected_workers(self, chunk_count: usize) -> usize {
        if chunk_count == 0 || self.available_workers == 0 {
            return 0;
        }
        self.worker_limit()
            .min(self.available_workers)
            .min(chunk_count)
    }

    pub fn deterministic_chunks(
        self,
        domain: &JetParallelIterationDomain,
    ) -> Result<Vec<JetParallelChunk>, JetParallelRejection> {
        let range = domain.range().ok_or_else(|| JetParallelRejection::Domain {
            reason: domain.rejection_reason(),
        })?;
        if self.chunk_items == 0 {
            return Err(JetParallelRejection::Resource {
                reason: "chunk_items must be greater than zero (got 0)".to_string(),
            });
        }
        let len = range.end - range.start;
        if len == 0 {
            return Ok(Vec::new());
        }
        if self.available_workers == 0 {
            return Err(JetParallelRejection::Resource {
                reason: "available_workers must be greater than zero for a non-empty domain (got 0)"
                    .to_string(),
            });
        }
        let chunk_count = len
            .checked_add(self.chunk_items - 1)
            .map(|value| value / self.chunk_items)
            .ok_or_else(|| JetParallelRejection::Resource {
                reason: format!(
                    "chunk count overflows usize for {} items and chunk_items={}",
                    len, self.chunk_items
                ),
            })?;
        if chunk_count > self.max_chunks {
            return Err(JetParallelRejection::Resource {
                reason: format!(
                    "chunk count {chunk_count} exceeds max_chunks {}",
                    self.max_chunks
                ),
            });
        }
        let workers = self.selected_workers(chunk_count);
        let mut chunks = Vec::with_capacity(chunk_count);
        for ordinal in 0..chunk_count {
            let start = range.start + ordinal * self.chunk_items;
            let end = start
                .saturating_add(self.chunk_items)
                .min(range.end);
            chunks.push(JetParallelChunk {
                ordinal,
                start,
                end,
                worker: ordinal % workers,
            });
        }
        Ok(chunks)
    }
}

/// A deterministic source-ordered chunk.  `worker` is only a stable assignment
/// fact for the later kernel; it does not start or identify a live thread.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetParallelChunk {
    pub ordinal: usize,
    pub start: usize,
    pub end: usize,
    pub worker: usize,
}

impl JetParallelChunk {
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

/// The reason a plan remains on the caller thread.  A measured D-ACCEL1
/// rejection is explicit in the plan instead of being mistaken for a proof.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelSequentialReason {
    EmptyDomain,
    SingleChunk,
    WorkerLimitOne,
    AccelerationGateRejected,
    ExplicitSerial,
}

impl fmt::Display for JetParallelSequentialReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyDomain => "empty iteration domain",
            Self::SingleChunk => "one deterministic chunk does not justify worker setup",
            Self::WorkerLimitOne => "worker limit is one",
            Self::AccelerationGateRejected => "D-ACCEL1 measured gain did not clear its threshold",
            Self::ExplicitSerial => "caller selected the serial execution form",
        })
    }
}

/// How a selected plan is executed by the later shared kernel.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelExecution {
    Sequential { reason: JetParallelSequentialReason },
    Parallel { workers: usize },
}

impl JetParallelExecution {
    pub const fn is_parallel(self) -> bool {
        matches!(self, Self::Parallel { .. })
    }

    pub const fn workers(self) -> usize {
        match self {
            Self::Sequential { .. } => 1,
            Self::Parallel { workers } => workers,
        }
    }
}

/// The complete backend-neutral fact selected for one explicit `para_*` call
/// or one D-ACCEL1 measured transform.  A plan only exists after
/// `JetParallelProof` accepts the operation and the resource bounds produce
/// stable chunks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelPlan {
    pub operation: JetParallelOperation,
    pub domain: JetParallelIterationDomain,
    pub proof: JetParallelProof,
    pub resources: JetParallelResourceBounds,
    pub chunks: Vec<JetParallelChunk>,
    pub execution: JetParallelExecution,
    pub reduction: Option<JetParallelReductionFact>,
    pub acceleration: Option<JetAccelerationDecision>,
}

impl JetParallelPlan {
    /// Select an explicit `para_*` operation without consulting host timing.
    pub fn select_explicit(
        operation: JetParallelOperation,
        proof: JetParallelProof,
        resources: JetParallelResourceBounds,
    ) -> Result<Self, JetParallelRejection> {
        Self::select_with_acceleration(operation, proof, resources, None)
    }

    /// Select a proven operation after the D-ACCEL1 measurement has cleared
    /// its release gate.  Execution still goes through the existing bounded
    /// indexed kernel; this method adds no worker population.
    pub(crate) fn select_accelerated(
        operation: JetParallelOperation,
        proof: JetParallelProof,
        resources: JetParallelResourceBounds,
        acceleration: JetAccelerationDecision,
    ) -> Result<Self, JetParallelRejection> {
        Self::select_with_acceleration(operation, proof, resources, Some(acceleration))
    }

    fn select_with_acceleration(
        operation: JetParallelOperation,
        proof: JetParallelProof,
        resources: JetParallelResourceBounds,
        acceleration: Option<JetAccelerationDecision>,
    ) -> Result<Self, JetParallelRejection> {
        proof.validate(operation)?;
        let chunks = resources.deterministic_chunks(&proof.domain)?;
        let execution = if chunks.is_empty() {
            JetParallelExecution::Sequential {
                reason: JetParallelSequentialReason::EmptyDomain,
            }
        } else {
            let workers = resources.selected_workers(chunks.len());
            if chunks.len() == 1 {
                JetParallelExecution::Sequential {
                    reason: JetParallelSequentialReason::SingleChunk,
                }
            } else if resources.worker_limit() == 1 || workers <= 1 {
                JetParallelExecution::Sequential {
                    reason: JetParallelSequentialReason::WorkerLimitOne,
                }
            } else if acceleration.is_some_and(|decision| !decision.selected()) {
                JetParallelExecution::Sequential {
                    reason: JetParallelSequentialReason::AccelerationGateRejected,
                }
            } else {
                JetParallelExecution::Parallel { workers }
            }
        };
        Ok(Self {
            operation,
            domain: proof.domain.clone(),
            reduction: proof.reduction.clone(),
            proof,
            resources,
            chunks,
            execution,
            acceleration,
        })
    }

    pub fn is_parallel(&self) -> bool {
        self.execution.is_parallel()
    }

    pub const fn worker_count(&self) -> usize {
        self.execution.workers()
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn acceleration_decision(&self) -> Option<JetAccelerationDecision> {
        self.acceleration
    }

    pub fn merge_tree(&self) -> Vec<(usize, usize)> {
        if !self.operation.is_reduction() {
            return Vec::new();
        }
        let mut active: Vec<usize> = self.chunks.iter().map(|chunk| chunk.ordinal).collect();
        let mut pairs = Vec::with_capacity(active.len().saturating_sub(1));
        while active.len() > 1 {
            let mut next = Vec::with_capacity(active.len().div_ceil(2));
            let mut cursor = 0;
            let left = active[cursor];
            if let Some(&right) = active.get(cursor + 1) {
                pairs.push((left, right));
                next.push(left);
                cursor += 2;
            } else {
                next.push(left);
                cursor += 1;
            }
            active = next;
        }
        pairs
    }

    pub fn receipt(&self) -> JetParallelReceipt {
        JetParallelReceipt::new(self)
    }
}


/// Why a proof or resource policy cannot produce an explicit plan.  Each
/// variant stores the typed fact that caused the rejection; `reason()` is
/// intentionally specific enough for a sema diagnostic or inspect ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetParallelRejection {
    Domain { reason: String },
    MissingEffectFacts,
    Access { fact: JetParallelAccessFact, reason: String },
    Dependence { fact: JetParallelDependencyFact },
    Effect { fact: JetParallelEffectFact, reason: String },
    Reduction { reason: String },
    Resource { reason: String },
}

impl JetParallelRejection {
    pub fn reason(&self) -> String {
        match self {
            Self::Domain { reason } => format!("iteration domain rejected: {reason}"),
            Self::MissingEffectFacts => {
                "effect rejected: callback effect facts are unavailable".to_string()
            }
            Self::Access { fact, reason } => {
                format!("access rejected: {}: {reason}", fact.description())
            }
            Self::Dependence { fact } => {
                format!("dependence rejected: {}", fact.description())
            }
            Self::Effect { fact, reason } => {
                format!(
                    "{reason} (`{}` is {}); operation cannot reorder it",
                    fact.name, fact.kind
                )
            }
            Self::Reduction { reason } => format!("reduction law rejected: {reason}"),
            Self::Resource { reason } => format!("resource bound rejected: {reason}"),
        }
    }
}

impl fmt::Display for JetParallelRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason())
    }
}

impl std::error::Error for JetParallelRejection {}

/// The proof assembled by sema for one callback and one iteration domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelProof {
    pub domain: JetParallelIterationDomain,
    pub accesses: Vec<JetParallelAccessFact>,
    pub effects: Vec<JetParallelEffectFact>,
    pub dependencies: Vec<JetParallelDependencyFact>,
    pub reduction: Option<JetParallelReductionFact>,
}
impl JetParallelProof {


    pub fn new(
        domain: JetParallelIterationDomain,
        accesses: Vec<JetParallelAccessFact>,
        effects: Vec<JetParallelEffectFact>,
        dependencies: Vec<JetParallelDependencyFact>,
        reduction: Option<JetParallelReductionFact>,
    ) -> Self {
        Self {
            domain,
            accesses,
            effects,
            dependencies,
            reduction,
        }
    }

    /// Construct the smallest complete witness for a pure, access-free body.
    /// Real sema callers should add each source access and dependency fact.
    pub fn pure(domain: JetParallelIterationDomain) -> Self {
        Self::new(domain, Vec::new(), vec![JetParallelEffectFact::pure()], Vec::new(), None)
    }

    pub fn validate(&self, operation: JetParallelOperation) -> Result<(), JetParallelRejection> {
        if !self.domain.is_valid() {
            return Err(JetParallelRejection::Domain {
                reason: self.domain.rejection_reason(),
            });
        }
        if self.effects.is_empty() {
            return Err(JetParallelRejection::MissingEffectFacts);
        }
        for access in &self.accesses {
            if matches!(access.scope, JetParallelAccessScope::Unknown) {
                return Err(JetParallelRejection::Access {
                    fact: access.clone(),
                    reason: "scope is unknown, so iteration disjointness is not proven".to_string(),
                });
            }
            if access.mode.is_reduction() && !operation.is_reduction() {
                return Err(JetParallelRejection::Access {
                    fact: access.clone(),
                    reason: format!(
                        "reduction access is only valid for {}, not {}",
                        JetParallelOperation::Fold,
                        operation
                    ),
                });
            }
            if access.mode.writes() && !access.is_proven_disjoint() && !access.mode.is_reduction() {
                let dependency = match access.mode {
                    JetParallelAccessMode::Write => JetParallelDependencyFact::write_after_write(
                        access.place.clone(),
                        None,
                        None,
                    ),
                    JetParallelAccessMode::ReadWrite => {
                        JetParallelDependencyFact::write_after_write(access.place.clone(), None, None)
                    }
                    JetParallelAccessMode::Read | JetParallelAccessMode::Reduce => unreachable!(),
                };
                return Err(JetParallelRejection::Dependence { fact: dependency });
            }
        }
        for effect in &self.effects {
            if let Some(reason) = effect.rejection_reason(operation, self.reduction.is_some()) {
                return Err(JetParallelRejection::Effect {
                    fact: effect.clone(),
                    reason,
                });
            }
        }
        for dependency in &self.dependencies {
            if dependency.kind.conflicts() {
                return Err(JetParallelRejection::Dependence {
                    fact: dependency.clone(),
                });
            }
        }
        if operation.is_reduction() {
            let Some(reduction) = self.reduction.as_ref() else {
                return Err(JetParallelRejection::Reduction {
                    reason: "para_fold requires a proven identity and associative merge law"
                        .to_string(),
                });
            };
            if let Some(reason) = reduction.rejection_reason() {
                return Err(JetParallelRejection::Reduction { reason });
            }
        }
        Ok(())
    }

    pub fn is_proven_independent(&self, operation: JetParallelOperation) -> bool {
        self.validate(operation).is_ok()
    }

    pub fn rejection(&self, operation: JetParallelOperation) -> Option<JetParallelRejection> {
        self.validate(operation).err()
    }
}

/// A callback failure identified by its source item.  The lowest source index
/// wins even if a later-index chunk reports first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelFailure {
    pub source_index: usize,
    pub message: String,
}

/// Terminal state for a plan receipt.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetParallelReceiptStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for JetParallelReceiptStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        })
    }
}

/// Bounded outcome facts from a plan execution.  It intentionally carries no
/// partial collection or accumulator: a failed/cancelled plan has no result,
/// while effects already performed by the callback remain outside this rail.
/// The optional D-ACCEL1 row keeps its measured decision available to inspect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetParallelReceipt {
    pub operation: JetParallelOperation,
    pub status: JetParallelReceiptStatus,
    pub chunks_total: usize,
    pub chunks_started: usize,
    pub chunks_completed: usize,
    pub items_completed: usize,
    pub first_failure: Option<JetParallelFailure>,
    pub cancellation_index: Option<usize>,
    pub partial_result_discarded: bool,
    pub external_effects_rolled_back: bool,
    pub acceleration: Option<JetAccelerationDecision>,
}

impl JetParallelReceipt {
    pub fn new(plan: &JetParallelPlan) -> Self {
        Self {
            operation: plan.operation,
            status: JetParallelReceiptStatus::Running,
            chunks_total: plan.chunks.len(),
            chunks_started: 0,
            chunks_completed: 0,
            items_completed: 0,
            first_failure: None,
            cancellation_index: None,
            partial_result_discarded: false,
            external_effects_rolled_back: false,
            acceleration: plan.acceleration,
        }
    }
    pub fn acceleration_decision(&self) -> Option<JetAccelerationDecision> {
        self.acceleration
    }

    pub fn record_chunk_started(&mut self) {
        self.chunks_started = self.chunks_started.saturating_add(1).min(self.chunks_total);
    }

    pub fn record_chunk_completed(&mut self, items: usize) {
        self.chunks_completed = self.chunks_completed.saturating_add(1).min(self.chunks_total);
        self.items_completed = self.items_completed.saturating_add(items);
    }

    pub fn record_failure(&mut self, source_index: usize, message: impl Into<String>) {
        let candidate = JetParallelFailure {
            source_index,
            message: message.into(),
        };
        let replace = self.first_failure.as_ref().is_none_or(|current| {
            (candidate.source_index, &candidate.message)
                < (current.source_index, &current.message)
        });
        if replace {
            self.first_failure = Some(candidate);
        }
        self.partial_result_discarded = true;
        self.external_effects_rolled_back = false;
        self.status = JetParallelReceiptStatus::Running;
    }

    pub fn record_cancellation(&mut self, source_index: usize) {
        if self
            .cancellation_index
            .is_none_or(|current| source_index < current)
        {
            self.cancellation_index = Some(source_index);
        }
        self.partial_result_discarded = true;
        self.external_effects_rolled_back = false;
        self.status = JetParallelReceiptStatus::Running;
    }

    pub fn finish(&mut self) -> JetParallelReceiptStatus {
        self.status = if self.first_failure.is_some() {
            JetParallelReceiptStatus::Failed
        } else if self.cancellation_index.is_some() {
            JetParallelReceiptStatus::Cancelled
        } else {
            JetParallelReceiptStatus::Completed
        };
        if self.status != JetParallelReceiptStatus::Completed {
            self.partial_result_discarded = true;
        }
        self.status
    }

    pub fn has_result(&self) -> bool {
        self.status == JetParallelReceiptStatus::Completed && !self.partial_result_discarded
    }

    pub fn selected_failure(&self) -> Option<&JetParallelFailure> {
        self.first_failure.as_ref()
    }
}

#[cfg(test)]
mod parallel_plan_tests {
    use super::*;

    fn pure_map(len: usize) -> JetParallelProof {
        JetParallelProof::new(
            JetParallelIterationDomain::from_len(len),
            vec![JetParallelAccessFact::read("input")],
            vec![JetParallelEffectFact::pure()],
            vec![JetParallelDependencyFact::independent("input")],
            None,
        )
    }

    #[test]
    fn chunks_are_source_ordered_and_worker_bounded() {
        let domain = JetParallelIterationDomain::from_len(130);
        let bounds = JetParallelResourceBounds::explicit(99, 3).with_chunk_items(64);
        let chunks = bounds.deterministic_chunks(&domain).unwrap();
        assert_eq!(
            chunks,
            vec![
                JetParallelChunk {
                    ordinal: 0,
                    start: 0,
                    end: 64,
                    worker: 0,
                },
                JetParallelChunk {
                    ordinal: 1,
                    start: 64,
                    end: 128,
                    worker: 1,
                },
                JetParallelChunk {
                    ordinal: 2,
                    start: 128,
                    end: 130,
                    worker: 2,
                },
            ]
        );
    }

    #[test]
    fn proof_rejects_exact_dependence_and_external_effects() {
        let dependent = JetParallelProof::new(
            JetParallelIterationDomain::from_len(8),
            vec![JetParallelAccessFact::write("total")],
            vec![JetParallelEffectFact::pure()],
            Vec::new(),
            None,
        );
        assert!(dependent
            .rejection(JetParallelOperation::Map)
            .unwrap()
            .reason()
            .contains("write-after-write on `total`"));

        let external = JetParallelProof::new(
            JetParallelIterationDomain::from_len(8),
            vec![JetParallelAccessFact::read("input")],
            vec![JetParallelEffectFact::external("stdout")],
            Vec::new(),
            None,
        );
        assert!(external
            .rejection(JetParallelOperation::Map)
            .unwrap()
            .reason()
            .contains("effect `stdout`"));
    }

    #[test]
    fn explicit_plan_uses_sequential_fallback_for_one_chunk() {
        let plan = JetParallelPlan::select_explicit(
            JetParallelOperation::Map,
            pure_map(4),
            JetParallelResourceBounds::explicit(8, 8),
        )
        .unwrap();
        assert_eq!(plan.execution, JetParallelExecution::Sequential {
            reason: JetParallelSequentialReason::SingleChunk,
        });
        assert_eq!(plan.chunks[0].range(), 0..4);
    }

    #[test]
    fn measured_range_does_not_probe_when_worker_limit_is_one() {
        let (values, decision) = jet_list_accel_range_map(
            0..(JET_ACCEL_STATIC_FLOOR_ITEMS + 1),
            1,
            true,
            true,
            true,
            None,
            |index| index,
        );
        assert_eq!(values.len(), JET_ACCEL_STATIC_FLOOR_ITEMS + 1);
        assert_eq!(
            decision.status,
            JetAccelerationGateStatus::SampleNotTimed
        );
    }

    #[test]
    fn measured_gate_uses_two_times_the_retained_chunk_projection() {
        let costs = JetAccelerationRuntimeCosts {
            spawn_nanos: 100,
            bandwidth_floor_nanos: 100,
            spawn_cost_once_per_process: true,
            bandwidth_floor_measured: true,
        };
        let selected = JetAccelerationGate::d_accel1().evaluate(
            JetAccelerationTransform::PooledParallelChunks,
            JetAccelerationGateInput::measured(
                2048, 64, 100, costs, true, true, true, false, false, None,
            ),
        );
        assert_eq!(selected.status, JetAccelerationGateStatus::Selected);
        assert_eq!(selected.measurement.projected_serial_nanos, Some(3200));
        assert_eq!(selected.measurement.required_serial_nanos, Some(200));

        let rejected = JetAccelerationGate::d_accel1().evaluate(
            JetAccelerationTransform::PooledParallelChunks,
            JetAccelerationGateInput::measured(
                2048, 64, 6, costs, true, true, true, false, false, None,
            ),
        );
        assert_eq!(
            rejected.status,
            JetAccelerationGateStatus::ProjectedGainBelowThreshold
        );
    }

    #[test]
    fn fold_requires_identity_and_associative_merge() {
        let missing = pure_map(128);
        assert!(matches!(
            missing.rejection(JetParallelOperation::Fold),
            Some(JetParallelRejection::Reduction { .. })
        ));
        let proof = JetParallelProof::new(
            JetParallelIterationDomain::from_len(128),
            vec![JetParallelAccessFact::read("input")],
            vec![JetParallelEffectFact::reduction("sum")],
            Vec::new(),
            Some(JetParallelReductionFact::proven("sum")),
        );
        assert!(proof.is_proven_independent(JetParallelOperation::Fold));
        let plan = JetParallelPlan::select_explicit(
            JetParallelOperation::Fold,
            proof,
            JetParallelResourceBounds::explicit(2, 2),
        )
        .unwrap();
        assert_eq!(plan.merge_tree(), vec![(0, 1)]);
    }

    #[test]
    fn receipt_keeps_lowest_failure_and_discards_result_on_cancel() {
        let plan = JetParallelPlan::select_explicit(
            JetParallelOperation::Map,
            pure_map(128),
            JetParallelResourceBounds::explicit(2, 2),
        )
        .unwrap();
        let mut receipt = plan.receipt();
        receipt.record_failure(90, "late");
        receipt.record_failure(4, "original");
        receipt.record_cancellation(3);
        assert_eq!(receipt.finish(), JetParallelReceiptStatus::Failed);
        assert_eq!(receipt.selected_failure().unwrap().source_index, 4);
        assert!(!receipt.has_result());
        assert!(!receipt.external_effects_rolled_back);
    }
}
