//! Typed acceleration facts shared by the canonical MIR optimizer and its
//! adapters.  This module records legality and the D-ACCEL1 gate; it never
//! reads a clock, starts workers, or selects a backend representation.

use crate::CanonicalPass;
use crate::PerformanceBudget::CanonicalJson;
use std::collections::BTreeMap;

/// Canonical typed receipt identity for one runtime D-ACCEL1 decision.
pub const ACCELERATION_RECEIPT_TYPE_NAME: &str = "AccelerationDecision";
/// Runtime decision sections use the existing append-only receipt rail.
pub const ACCELERATION_RECEIPT_SECTION_PREFIX: &str = "jet.acceleration.";
/// Canonical `#Receipt` schema identity for the runtime envelope.  It uses
/// the existing `type\0field:type;` framing and the wire-level `DataTree`
/// types, including the complete nested decision and source fields.
pub const ACCELERATION_RECEIPT_SCHEMA: &str =
    "AccelerationDecision\0decision:(measurement: (bandwidth_floor_nanos: Int, floor_nanos: Int, items: Int, projected_serial_nanos: ?Int, required_serial_nanos: ?Int, sample_items: Int, sample_nanos: Int, spawn_nanos: Int), pin: ?String, status: String, transform: String);function:String;loop_header:Int;source:(end: Int, start: Int);";
/// SHA-256 of [`ACCELERATION_RECEIPT_SCHEMA`] under the shared receipt
/// schema-digest mechanism.
pub const ACCELERATION_RECEIPT_SCHEMA_DIGEST: &str =
    "fb216702b04de0611c6ceeb7b92efeeec9b91d3076277f38baac13475962d6d1";

/// D-FRED1=A: the fixed reduction width used by every Float reduction tier.
pub const D_FRED_REDUCTION_LANES: usize = 8;
/// D-ACCEL1=A: no automatic acceleration decision is measured below this
/// source-item floor.
pub const D_ACCEL_STATIC_FLOOR_ITEMS: usize = 1024;
/// D-ACCEL1=A: transient copies are bounded to one cache block at a time.
pub const D_ACCEL_CACHE_BLOCK_BYTES: usize = 64 * 1024;
/// D-ACCEL1=A: a measured candidate must project at least this gain.
pub const D_ACCEL_REQUIRED_GAIN_NUMERATOR: u64 = 2;
pub const D_ACCEL_REQUIRED_GAIN_DENOMINATOR: u64 = 1;

/// The fixed adjacent tree required by D-FRED1=A.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedReductionTree {
    AdjacentPairsThenRoot,
}

impl FixedReductionTree {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdjacentPairsThenRoot => "((0+1)+(2+3))+((4+5)+(6+7))",
        }
    }
}

/// One semantic reduction order.  The code-generation lane kernel implements
/// this record; adapters must not replace it with a backend-local order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedReductionOrder {
    pub lanes: usize,
    pub seed_lane: usize,
    pub tree: FixedReductionTree,
}

impl FixedReductionOrder {
    pub const fn canonical() -> Self {
        Self {
            lanes: D_FRED_REDUCTION_LANES,
            seed_lane: 0,
            tree: FixedReductionTree::AdjacentPairsThenRoot,
        }
    }

    pub const fn is_canonical(self) -> bool {
        self.lanes == D_FRED_REDUCTION_LANES
            && self.seed_lane == 0
            && matches!(self.tree, FixedReductionTree::AdjacentPairsThenRoot)
    }

    pub const fn lane_for(self, index: usize) -> Option<usize> {
        if self.lanes == 0 {
            None
        } else {
            Some(index % self.lanes)
        }
    }

    pub const fn tree_name(self) -> &'static str {
        self.tree.as_str()
    }
}

/// Canonical D-FRED1=A order.  Fold seeds live in lane zero; sum initializes
/// every lane to zero before applying the same tree.
pub const D_FRED1_FIXED_ORDER: FixedReductionOrder = FixedReductionOrder::canonical();

/// The two release-only transforms ratified by D-ACCEL1=A.  The measured
/// decision is consumed by the existing bounded runtime kernel; it does not
/// introduce a second scheduler or the retired provisional `par_*` surface.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccelerationTransform {
    TransientColumnCopy,
    PooledParallelChunks,
}

impl AccelerationTransform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TransientColumnCopy => "column-copy",
            Self::PooledParallelChunks => "parallel",
        }
    }
}

/// Source facts needed before either D-ACCEL1 transform can be considered.
/// These are semantic premises, not a promise that the transform is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationProof {
    pub source_proven: bool,
    pub independent_iterations: bool,
    pub no_aliasing: bool,
    pub no_cross_iteration_dependencies: bool,
    pub no_early_exit: bool,
    pub effect_free_body: bool,
    pub ownership_safe: bool,
    pub failure_order_preserved: bool,
}

impl AccelerationProof {
    pub const fn unavailable() -> Self {
        Self {
            source_proven: false,
            independent_iterations: false,
            no_aliasing: false,
            no_cross_iteration_dependencies: false,
            no_early_exit: false,
            effect_free_body: false,
            ownership_safe: false,
            failure_order_preserved: false,
        }
    }

    pub const fn proves(self, transform: AccelerationTransform) -> bool {
        let common = self.source_proven
            && self.no_aliasing
            && self.no_cross_iteration_dependencies
            && self.no_early_exit
            && self.effect_free_body
            && self.ownership_safe
            && self.failure_order_preserved;
        match transform {
            AccelerationTransform::TransientColumnCopy
            | AccelerationTransform::PooledParallelChunks => {
                common && self.independent_iterations
            }
        }
    }
}

/// D-ACCEL1 source shape facts.  A copy is legal only for nested reuse; a
/// single-pass AoS loop stays on its existing packed path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationWorkloadFacts {
    pub nested_reuse: bool,
    pub single_pass: bool,
}

impl AccelerationWorkloadFacts {
    pub const fn none() -> Self {
        Self {
            nested_reuse: false,
            single_pass: false,
        }
    }

    pub const fn column_copy_applicable(self) -> bool {
        self.nested_reuse && !self.single_pass
    }
}

/// Existing expert opt-outs.  No new marker is introduced by D-ACCEL1.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccelerationPin {
    Scalar,
    Local,
}

impl AccelerationPin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "#Scalar",
            Self::Local => "#Local",
        }
    }
}

/// Numeric facts supplied by a release host after the first serial chunk has
/// been timed.  The optimizer receives these values; it does not obtain them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationGateInput {
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
    pub pin: Option<AccelerationPin>,
    pub workload: AccelerationWorkloadFacts,
    pub proof: AccelerationProof,
}

impl AccelerationGateInput {
    /// Conservative input used before proof and measurement records arrive.
    pub const fn deferred() -> Self {
        Self {
            items: 0,
            sample_items: 0,
            sample_nanos: 0,
            spawn_nanos: 0,
            bandwidth_floor_nanos: 0,
            first_chunk_serial_timed: false,
            spawn_cost_once_per_process: false,
            bandwidth_floor_measured: false,
            release_build: false,
            cross_mode_parity_proven: false,
            pin: None,
            workload: AccelerationWorkloadFacts::none(),
            proof: AccelerationProof::unavailable(),
        }
    }
}

impl Default for AccelerationGateInput {
    fn default() -> Self {
        Self::deferred()
    }
}

/// Why the release gate did not activate a candidate.  Every variant is
/// inspectable without parsing an adapter-specific string.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccelerationGateStatus {
    BelowStaticFloor,
    Pinned(AccelerationPin),
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

impl AccelerationGateStatus {
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

/// Numeric gate values retained with the decision for `jet inspect accel`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationMeasurement {
    pub items: usize,
    pub sample_items: usize,
    pub sample_nanos: u64,
    pub spawn_nanos: u64,
    pub bandwidth_floor_nanos: u64,
    pub floor_nanos: u64,
    pub projected_serial_nanos: Option<u64>,
    pub required_serial_nanos: Option<u64>,
}

/// One complete D-ACCEL1 decision row.  It is a reportable fact consumed by
/// the selected transform; it is not itself a worker or copy operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationDecision {
    pub transform: AccelerationTransform,
    pub status: AccelerationGateStatus,
    pub measurement: AccelerationMeasurement,
}

impl AccelerationDecision {
    pub const fn selected(self) -> bool {
        self.status.is_selected()
    }

    /// Render the canonical nested decision object carried by a runtime
    /// acceleration receipt section.
    ///
    /// Source identity and the append-only sequence live in the surrounding
    /// receipt envelope; this value remains the shared Foundation projection.
    pub fn to_canonical_json(self) -> CanonicalJson {
        let pin = match self.status {
            AccelerationGateStatus::Pinned(pin) => {
                CanonicalJson::String(pin.as_str().to_string())
            }
            _ => CanonicalJson::Null,
        };
        let measurement = CanonicalJson::object([
            (
                "bandwidth_floor_nanos".into(),
                acceleration_integer(self.measurement.bandwidth_floor_nanos),
            ),
            (
                "floor_nanos".into(),
                acceleration_integer(self.measurement.floor_nanos),
            ),
            ("items".into(), acceleration_integer(self.measurement.items)),
            (
                "projected_serial_nanos".into(),
                acceleration_optional_integer(self.measurement.projected_serial_nanos),
            ),
            (
                "required_serial_nanos".into(),
                acceleration_optional_integer(self.measurement.required_serial_nanos),
            ),
            (
                "sample_items".into(),
                acceleration_integer(self.measurement.sample_items),
            ),
            (
                "sample_nanos".into(),
                acceleration_integer(self.measurement.sample_nanos),
            ),
            (
                "spawn_nanos".into(),
                acceleration_integer(self.measurement.spawn_nanos),
            ),
        ])
        .expect("acceleration measurement keys are unique");
        CanonicalJson::object([
            ("measurement".into(), measurement),
            ("pin".into(), pin),
            (
                "status".into(),
                CanonicalJson::String(self.status.as_str().to_string()),
            ),
            (
                "transform".into(),
                CanonicalJson::String(self.transform.as_str().to_string()),
            ),
        ])
        .expect("acceleration decision keys are unique")
    }

    /// Decode one canonical runtime receipt value into the Foundation type.
    pub fn from_canonical_json(value: &CanonicalJson) -> Result<Self, String> {
        let fields = acceleration_object(value, "acceleration decision")?;
        acceleration_require_fields(
            fields,
            "acceleration decision",
            &["measurement", "pin", "status", "transform"],
        )?;
        let transform = match acceleration_string(fields, "transform")? {
            "column-copy" => AccelerationTransform::TransientColumnCopy,
            "parallel" => AccelerationTransform::PooledParallelChunks,
            actual => return Err(format!("unknown acceleration transform `{actual}`")),
        };
        let pin = match fields.get("pin") {
            None | Some(CanonicalJson::Null) => None,
            Some(value) => Some(acceleration_pin(value)?),
        };
        let status_name = acceleration_string(fields, "status")?;
        let status = match status_name {
            "below-static-floor" => AccelerationGateStatus::BelowStaticFloor,
            "pinned" => AccelerationGateStatus::Pinned(
                pin.ok_or("pinned acceleration decision is missing pin")?,
            ),
            "not-release-build" => AccelerationGateStatus::NotReleaseBuild,
            "cross-mode-parity-missing" => AccelerationGateStatus::CrossModeParityMissing,
            "column-copy-not-applicable" => AccelerationGateStatus::ColumnCopyNotApplicable,
            "proof-incomplete" => AccelerationGateStatus::ProofIncomplete,
            "sample-not-timed" => AccelerationGateStatus::SampleNotTimed,
            "spawn-cost-not-measured" => AccelerationGateStatus::SpawnCostNotMeasured,
            "bandwidth-floor-not-measured" => AccelerationGateStatus::BandwidthFloorNotMeasured,
            "invalid-measurement" => AccelerationGateStatus::InvalidMeasurement,
            "projected-gain-below-threshold" => {
                AccelerationGateStatus::ProjectedGainBelowThreshold
            }
            "selected" => AccelerationGateStatus::Selected,
            actual => return Err(format!("unknown acceleration gate status `{actual}`")),
        };
        if pin.is_some() && !matches!(status, AccelerationGateStatus::Pinned(_)) {
            return Err("un-pinned acceleration decision carries a pin".into());
        }
        let measurement = acceleration_object(
            fields
                .get("measurement")
                .ok_or("acceleration decision is missing measurement")?,
            "acceleration measurement",
        )?;
        acceleration_require_fields(
            measurement,
            "acceleration measurement",
            &[
                "bandwidth_floor_nanos",
                "floor_nanos",
                "items",
                "projected_serial_nanos",
                "required_serial_nanos",
                "sample_items",
                "sample_nanos",
                "spawn_nanos",
            ],
        )?;
        Ok(Self {
            transform,
            status,
            measurement: AccelerationMeasurement {
                bandwidth_floor_nanos: acceleration_u64(measurement, "bandwidth_floor_nanos")?,
                floor_nanos: acceleration_u64(measurement, "floor_nanos")?,
                items: acceleration_usize(measurement, "items")?,
                projected_serial_nanos: acceleration_optional_u64(
                    measurement,
                    "projected_serial_nanos",
                )?,
                required_serial_nanos: acceleration_optional_u64(
                    measurement,
                    "required_serial_nanos",
                )?,
                sample_items: acceleration_usize(measurement, "sample_items")?,
                sample_nanos: acceleration_u64(measurement, "sample_nanos")?,
                spawn_nanos: acceleration_u64(measurement, "spawn_nanos")?,
            },
        })
    }
}

fn acceleration_integer(value: impl ToString) -> CanonicalJson {
    CanonicalJson::integer(value.to_string()).expect("acceleration integer is canonical")
}

fn acceleration_optional_integer(value: Option<u64>) -> CanonicalJson {
    value
        .map(|value| acceleration_integer(value))
        .unwrap_or(CanonicalJson::Null)
}

fn acceleration_object<'a>(
    value: &'a CanonicalJson,
    label: &str,
) -> Result<&'a BTreeMap<String, CanonicalJson>, String> {
    match value {
        CanonicalJson::Object(fields) => Ok(fields),
        _ => Err(format!("{label} must be a canonical object")),
    }
}

fn acceleration_require_fields(
    fields: &BTreeMap<String, CanonicalJson>,
    label: &str,
    keys: &[&str],
) -> Result<(), String> {
    if fields.len() != keys.len() || !keys.iter().all(|key| fields.contains_key(*key)) {
        return Err(format!("{label} has missing or unknown fields"));
    }
    Ok(())
}

fn acceleration_string<'a>(
    fields: &'a BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<&'a str, String> {
    match fields.get(key) {
        Some(CanonicalJson::String(value)) => Ok(value),
        Some(_) => Err(format!("acceleration field `{key}` must be a string")),
        None => Err(format!("acceleration field `{key}` is missing")),
    }
}

fn acceleration_pin(value: &CanonicalJson) -> Result<AccelerationPin, String> {
    match value {
        CanonicalJson::Null => Err("pinned acceleration decision is missing pin".into()),
        CanonicalJson::String(value) => match value.as_str() {
            "#Scalar" => Ok(AccelerationPin::Scalar),
            "#Local" => Ok(AccelerationPin::Local),
            actual => Err(format!("unknown acceleration pin `{actual}`")),
        },
        _ => Err("acceleration field `pin` must be a string or null".into()),
    }
}

fn acceleration_u64(
    fields: &BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<u64, String> {
    match fields.get(key) {
        Some(CanonicalJson::Integer(value)) => value
            .parse()
            .map_err(|_| format!("acceleration field `{key}` is out of range")),
        Some(_) => Err(format!("acceleration field `{key}` must be an integer")),
        None => Err(format!("acceleration field `{key}` is missing")),
    }
}

fn acceleration_usize(
    fields: &BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<usize, String> {
    match fields.get(key) {
        Some(CanonicalJson::Integer(value)) => value
            .parse()
            .map_err(|_| format!("acceleration field `{key}` is out of range")),
        Some(_) => Err(format!("acceleration field `{key}` must be an integer")),
        None => Err(format!("acceleration field `{key}` is missing")),
    }
}

fn acceleration_optional_u64(
    fields: &BTreeMap<String, CanonicalJson>,
    key: &str,
) -> Result<Option<u64>, String> {
    match fields.get(key) {
        Some(CanonicalJson::Null) => Ok(None),
        Some(CanonicalJson::Integer(value)) => value
            .parse()
            .map(Some)
            .map_err(|_| format!("acceleration field `{key}` is out of range")),
        Some(_) => Err(format!("acceleration field `{key}` must be an integer or null")),
        None => Err(format!("acceleration field `{key}` is missing")),
    }
}

/// Backend-neutral D-ACCEL1 gate.  All values are supplied by the caller, and
/// runtime adapters use `should_measure` before reading a clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccelerationGate {
    pub static_floor_items: usize,
    pub cache_block_bytes: usize,
    pub required_gain_numerator: u64,
    pub required_gain_denominator: u64,
}

impl AccelerationGate {
    pub const fn d_accel1() -> Self {
        Self {
            static_floor_items: D_ACCEL_STATIC_FLOOR_ITEMS,
            cache_block_bytes: D_ACCEL_CACHE_BLOCK_BYTES,
            required_gain_numerator: D_ACCEL_REQUIRED_GAIN_NUMERATOR,
            required_gain_denominator: D_ACCEL_REQUIRED_GAIN_DENOMINATOR,
        }
    }
    /// D-ACCEL1 forbids even the first-chunk clock read at or below the floor.
    pub const fn should_measure(self, items: usize) -> bool {
        items > self.static_floor_items
    }

    pub fn evaluate(
        self,
        transform: AccelerationTransform,
        input: AccelerationGateInput,
    ) -> AccelerationDecision {
        let measurement = self.measurement(input);
        let status = if input.items <= self.static_floor_items {
            AccelerationGateStatus::BelowStaticFloor
        } else if let Some(pin) = input.pin {
            AccelerationGateStatus::Pinned(pin)
        } else if !input.release_build {
            AccelerationGateStatus::NotReleaseBuild
        } else if !input.cross_mode_parity_proven {
            AccelerationGateStatus::CrossModeParityMissing
        } else if matches!(transform, AccelerationTransform::TransientColumnCopy)
            && !input.workload.column_copy_applicable()
        {
            AccelerationGateStatus::ColumnCopyNotApplicable
        } else if !input.proof.proves(transform) {
            AccelerationGateStatus::ProofIncomplete
        } else if !input.first_chunk_serial_timed {
            AccelerationGateStatus::SampleNotTimed
        } else if !input.spawn_cost_once_per_process {
            AccelerationGateStatus::SpawnCostNotMeasured
        } else if !input.bandwidth_floor_measured {
            AccelerationGateStatus::BandwidthFloorNotMeasured
        } else if input.sample_items == 0
            || input.sample_items > input.items
            || input.sample_nanos == 0
            || input.spawn_nanos == 0
            || input.bandwidth_floor_nanos == 0
            || self.required_gain_numerator == 0
            || self.required_gain_denominator == 0
        {
            AccelerationGateStatus::InvalidMeasurement
        } else if measurement.projected_serial_nanos.is_none()
            || measurement.required_serial_nanos.is_none()
        {
            AccelerationGateStatus::InvalidMeasurement
        } else if measurement.projected_serial_nanos < measurement.required_serial_nanos {
            AccelerationGateStatus::ProjectedGainBelowThreshold
        } else {
            AccelerationGateStatus::Selected
        };
        let decision = AccelerationDecision {
            transform,
            status,
            measurement,
        };
        if CanonicalPass::enabled() {
            CanonicalPass::record(
                "optimization",
                "mir.acceleration-gate",
                "crates/jet-foundation/src/MIROptimization/Acceleration.rs",
                "mir",
                CanonicalPass::debug_payload("mir", "mir.acceleration-gate", &input),
                CanonicalPass::debug_identity("mir", "mir.acceleration-gate", &input),
                "mir",
                CanonicalPass::debug_payload("mir", "mir.acceleration-gate", &decision),
                CanonicalPass::debug_identity("mir", "mir.acceleration-gate", &decision),
                "checked",
            );
        }
        decision
    }

    fn measurement(self, input: AccelerationGateInput) -> AccelerationMeasurement {
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
                .and_then(|value| {
                    value.checked_add(self.required_gain_denominator - 1)
                })
                .map(|value| value / self.required_gain_denominator)
        };
        AccelerationMeasurement {
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

impl Default for AccelerationGate {
    fn default() -> Self {
        Self::d_accel1()
    }
}
