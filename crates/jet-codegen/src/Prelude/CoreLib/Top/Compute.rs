// ── D-COMPUTE1=D / D-COMPUTE-TYPE1=D / D-COMPUTE-PLACE1=D (#443) ─────────────
// One Core compute family. `Tensor` owns ranked multidimensional storage on the
// explicit CPU-oracle capability; views retain the backing allocation and its
// strides. Mutable access requires the sema-proved exclusive ViewMut path;
// shared writes fail closed instead of copying or pretending to update an alias.
// Engines only marshal into these Prelude symbols (I9).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetComputeDevice {
    Auto,
    Cpu,
}

const MAX_TENSOR_ELEMENTS: usize = 16 * 1024 * 1024;
const CPU_ORACLE_BACKEND: &str = "cpu-oracle";
const CPU_ORACLE_VERSION: &str = "builtin";
const CPU_ORACLE_CACHE: &str = "none";
const CPU_ORACLE_F64_PROFILE: &str = "F64Strict+Reproducible";
const CPU_ORACLE_F32_PROFILE: &str = "F32Strict+Reproducible";
const CPU_ORACLE_F64_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "reproducible-reduction",
    "differential-oracle",
];
const CPU_ORACLE_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "blocked-matmul",
    "differential-oracle",
];

fn jet_compute_registered_capabilities(profile: &str) -> Option<&'static [&'static str]> {
    match profile {
        CPU_ORACLE_F64_PROFILE => Some(CPU_ORACLE_F64_CAPABILITIES),
        CPU_ORACLE_F32_PROFILE => Some(CPU_ORACLE_F32_CAPABILITIES),
        _ => None,
    }
}

fn jet_compute_capabilities_match(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputePlacementReceipt {
    requested: JetComputeDevice,
    selected: JetComputeDevice,
    backend: String,
    version: String,
    profile: String,
    cache: String,
    capabilities: Vec<String>,
    reason: String,
}

#[derive(Clone)]
struct JetComputeTrace {
    // Traces are observations of a live per-call tape.  The transform state
    // owns the strong tape handle; graph values keep only a weak back-link so
    // nested tapes cannot retain one another through recorded values.
    tape: std::sync::Weak<std::sync::Mutex<JetComputeTape>>,
    node: usize,
    parent: Option<Box<JetComputeTrace>>,
}

impl std::fmt::Debug for JetComputeTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetComputeTrace")
            .field("node", &self.node)
            .field("parent", &self.parent)
            .finish()
    }
}

#[derive(Clone, Debug)]
struct JetTensor {
    shape: Vec<i64>,
    strides: Vec<i64>,
    data: std::sync::Arc<Vec<f64>>,
    device: JetComputeDevice,
    last_placement: JetComputePlacementReceipt,
    last_transfer: Option<JetComputeTransferReceipt>,
    trace: Option<JetComputeTrace>,
}

impl PartialEq for JetTensor {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.strides == other.strides
            && self.data == other.data
            && self.device == other.device
            && self.last_placement == other.last_placement
            && self.last_transfer == other.last_transfer
    }
}

#[derive(Clone, Debug, PartialEq)]
enum JetComputeError {
    InvalidShape(String),
    RankMismatch(String),
    OutOfBounds(String),
    Device(String),
    Unsupported(String),
    Arithmetic(String),
    Serialization(String),
}

impl JetShow for JetComputeError {
    fn jet_show(&self) -> String {
        match self {
            JetComputeError::InvalidShape(m)
            | JetComputeError::RankMismatch(m)
            | JetComputeError::OutOfBounds(m)
            | JetComputeError::Device(m)
            | JetComputeError::Unsupported(m)
            | JetComputeError::Arithmetic(m)
            | JetComputeError::Serialization(m) => m.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum JetComputeTapeRule {
    Add,
    Sub,
    Mul,
    Div,
    Maximum,
    Minimum,
    Matmul,
    Unary(String),
    Reshape {
        source_shape: Vec<i64>,
    },
    Broadcast {
        source_shape: Vec<i64>,
    },
    ReduceToShape {
        source_shape: Vec<i64>,
    },
    Transpose,
    SumAxis {
        axis: usize,
        source_shape: Vec<i64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct JetComputeTapeNode {
    parents: Vec<Option<usize>>,
    rule: Option<JetComputeTapeRule>,
    values: Vec<JetTensor>,
    output: JetTensor,
}

#[derive(Clone, Debug, PartialEq)]
struct JetComputeTape {
    nodes: Vec<JetComputeTapeNode>,
    inputs: Vec<JetTensor>,
}

#[derive(Clone, Debug)]
struct JetComputeVjpState {
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    output_node: Option<usize>,
}

enum JetComputeTransformResult {
    Gradient(Vec<JetTensor>),
    ValueAndGradient {
        value: JetTensor,
        gradients: Vec<JetTensor>,
    },
    Vjp {
        value: JetTensor,
        state: JetComputeVjpState,
    },
    Jvp {
        value: JetTensor,
        tangent: JetTensor,
    },
}

struct JetComputeVjpRun<R> {
    pub value: JetTensor,
    pub pull: std::rc::Rc<dyn Fn(JetTensor) -> R>,
    pub grads: std::rc::Rc<dyn Fn() -> R>,
}

impl<R: Clone> Clone for JetComputeVjpRun<R> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            pull: self.pull.clone(),
            grads: self.grads.clone(),
        }
    }
}

impl<R> JetComputeVjpRun<R> {
    fn grads_or_panic(&self) -> R {
        (self.grads)()
    }
}

fn jet_compute_untracked(tensor: &JetTensor) -> JetTensor {
    let mut value = tensor.clone();
    value.trace = None;
    value
}

fn jet_compute_trace_node_for_tape(
    trace: Option<&JetComputeTrace>,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Option<usize> {
    let trace = trace?;
    if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
        return Some(trace.node);
    }
    jet_compute_trace_node_for_tape(trace.parent.as_deref(), tape)
}

fn jet_compute_trace_lanes(
    trace: Option<&JetComputeTrace>,
) -> Vec<(
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    usize,
)> {
    let mut lanes = Vec::new();
    let mut current = trace;
    while let Some(trace) = current {
        if let Some(tape) = trace.tape.upgrade() {
            lanes.push((tape, trace.node));
        }
        current = trace.parent.as_deref();
    }
    lanes
}

fn jet_compute_remove_trace_level(
    tensor: &JetTensor,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetTensor {
    fn remove(
        trace: JetComputeTrace,
        tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    ) -> (Option<JetComputeTrace>, bool) {
        if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
            return (trace.parent.map(|parent| *parent), true);
        }
        let (parent, removed) = match trace.parent {
            Some(parent) => {
                let (parent, removed) = remove(*parent, tape);
                (parent.map(Box::new), removed)
            }
            None => (None, false),
        };
        (
            Some(JetComputeTrace {
                tape: trace.tape,
                node: trace.node,
                parent,
            }),
            removed,
        )
    }

    let Some(trace) = tensor.trace.clone() else {
        return tensor.clone();
    };
    let (trace, _) = remove(trace, tape);
    let mut value = tensor.clone();
    value.trace = trace.and_then(|trace| jet_compute_prune_trace(Some(trace)));
    value
}

fn jet_compute_prune_trace(trace: Option<JetComputeTrace>) -> Option<JetComputeTrace> {
    let trace = trace?;
    let parent = trace
        .parent
        .and_then(|parent| jet_compute_prune_trace(Some(*parent)).map(Box::new));
    if trace.tape.upgrade().is_none() {
        return parent.map(|parent| *parent);
    }
    Some(JetComputeTrace {
        tape: trace.tape,
        node: trace.node,
        parent,
    })
}

fn jet_compute_tape_for_parents(
    parents: &[&JetTensor],
) -> Result<
    Vec<(
        std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
        Vec<Option<usize>>,
    )>,
    JetComputeError,
> {
    let nonempty = parents
        .iter()
        .map(|parent| jet_compute_trace_lanes(parent.trace.as_ref()))
        .filter(|lanes| !lanes.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = nonempty.first() else {
        return Ok(Vec::new());
    };
    if nonempty.iter().skip(1).any(|lanes| {
        !lanes.iter().any(|(tape, _)| {
            first
                .iter()
                .any(|(first_tape, _)| std::sync::Arc::ptr_eq(first_tape, tape))
        })
    }) {
        return Err(JetComputeError::Unsupported(
            "autodiff values belong to different tapes".to_string(),
        ));
    }
    let mut tapes = Vec::new();
    for lanes in &nonempty {
        for (tape, _) in lanes {
            if !tapes
                .iter()
                .any(|existing| std::sync::Arc::ptr_eq(existing, tape))
            {
                tapes.push(tape.clone());
            }
        }
    }
    Ok(tapes
        .into_iter()
        .map(|tape| {
            let ids = parents
                .iter()
                .map(|parent| {
                    jet_compute_trace_node_for_tape(parent.trace.as_ref(), &tape)
                })
                .collect();
            (tape, ids)
        })
        .collect())
}

fn jet_compute_record(
    mut output: JetTensor,
    parents: &[&JetTensor],
    values: Vec<JetTensor>,
    rule: JetComputeTapeRule,
) -> Result<JetTensor, JetComputeError> {
    let tapes = jet_compute_tape_for_parents(parents)?;
    if tapes.is_empty() {
        return Ok(output);
    }
    let mut recorded = Vec::with_capacity(tapes.len());
    for (tape, parent_ids) in tapes {
        let mut tape_guard = tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        let node = tape_guard.nodes.len();
        tape_guard.nodes.push(JetComputeTapeNode {
            parents: parent_ids,
            rule: Some(rule.clone()),
            values: values
                .iter()
                .map(|value| jet_compute_remove_trace_level(value, &tape))
                .collect(),
            output: jet_compute_remove_trace_level(&output, &tape),
        });
        recorded.push((tape.clone(), node));
    }
    let mut trace = None;
    for (tape, node) in recorded.into_iter().rev() {
        trace = Some(Box::new(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: trace,
        }));
    }
    output.trace = trace.map(|trace| *trace);
    Ok(output)
}

fn jet_compute_trace_inputs(
    inputs: Vec<JetTensor>,
) -> (
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    Vec<JetTensor>,
) {
    let values = inputs.clone();
    let tape = std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: values.clone(),
    }));
    let mut tracked = Vec::with_capacity(values.len());
    let mut guard = tape
        .lock()
        .unwrap_or_else(|_| jet_panic("Compute.rs", line!(), "autodiff tape is poisoned"));
    for (index, value) in values.iter().enumerate() {
        let node = guard.nodes.len();
        guard.nodes.push(JetComputeTapeNode {
            parents: Vec::new(),
            rule: None,
            values: vec![value.clone()],
            output: value.clone(),
        });
        let mut input = value.clone();
        input.trace = Some(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: value.trace.clone().map(Box::new),
        });
        tracked.push(input);
        debug_assert_eq!(index, node);
    }
    (tape, tracked)
}

fn jet_compute_empty_tape() -> std::sync::Arc<std::sync::Mutex<JetComputeTape>> {
    std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: Vec::new(),
    }))
}

fn jet_compute_vjp_begin(
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetComputeVjpState {
    JetComputeVjpState {
        output_node: jet_compute_trace_node_for_tape(value.trace.as_ref(), &tape),
        value,
        tape,
    }
}

impl JetShow for JetComputeDevice {
    fn jet_show(&self) -> String {
        match self {
            JetComputeDevice::Auto => "Auto".to_string(),
            JetComputeDevice::Cpu => "CPU".to_string(),
        }
    }
}

impl JetShow for JetComputePlacementReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Placement(requested={}, selected={}, backend={}, version={}, profile={}, cache={}, capabilities={:?}, reason={})",
            self.requested.jet_show(),
            self.selected.jet_show(),
            self.backend,
            self.version,
            self.profile,
            self.cache,
            self.capabilities,
            self.reason
        )
    }
}

impl JetShow for JetTensor {
    fn jet_show(&self) -> String {
        format!(
            "Tensor(shape={:?}, device={}, len={})",
            self.shape,
            self.device.jet_show(),
            jet_compute_tensor_numel(self)
        )
    }
}

fn jet_compute_row_major_strides(shape: &[i64]) -> Result<Vec<i64>, JetComputeError> {
    if shape.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape must have at least one axis".to_string(),
        ));
    }
    if shape.iter().any(|d| *d < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axes must be non-negative".to_string(),
        ));
    }
    if shape
        .iter()
        .any(|d| *d > i64::try_from(MAX_TENSOR_ELEMENTS).unwrap_or(i64::MAX))
    {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axis exceeds the Core storage limit".to_string(),
        ));
    }
    let mut strides = vec![1i64; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        let next = shape[i + 1]
            .checked_mul(strides[i + 1])
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor stride overflow".to_string()))?;
        strides[i] = next;
    }
    Ok(strides)
}

fn jet_compute_numel(shape: &[i64]) -> Result<i64, JetComputeError> {
    let mut n = 1i64;
    for &d in shape {
        if d < 0 {
            return Err(JetComputeError::InvalidShape(
                "Tensor shape axes must be non-negative".to_string(),
            ));
        }
        n = n
            .checked_mul(d)
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor numel overflow".to_string()))?;
    }
    Ok(n)
}

fn jet_compute_storage_len(shape: &[i64]) -> Result<usize, JetComputeError> {
    // Validate strides even when an earlier zero axis makes the element count
    // zero. Otherwise a shape such as `[0, i64::MAX]` could look empty while
    // carrying an unrepresentable axis into later indexing code.
    jet_compute_row_major_strides(shape)?;
    let n = jet_compute_numel(shape)?;
    let len = usize::try_from(n).map_err(|_| {
        JetComputeError::InvalidShape("Tensor storage length is too large".to_string())
    })?;
    if len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(format!(
            "Tensor storage exceeds the {}-element Core limit",
            MAX_TENSOR_ELEMENTS
        )));
    }
    Ok(len)
}

fn jet_compute_view_metadata(
    tensor: &JetTensor,
) -> Result<(&[i64], usize), JetComputeError> {
    let rank = tensor.shape.len();
    if tensor.strides.len() == rank {
        return Ok((&tensor.strides, 0));
    }
    if rank.checked_add(1) != Some(tensor.strides.len()) {
        return Err(JetComputeError::InvalidShape(
            "Tensor stride and view metadata disagree".to_string(),
        ));
    }
    let offset = usize::try_from(tensor.strides[rank]).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view offset must be non-negative".to_string())
    })?;
    Ok((&tensor.strides[..rank], offset))
}

fn jet_compute_view_strides(
    shape: &[i64],
    offset: usize,
) -> Result<Vec<i64>, JetComputeError> {
    let mut strides = jet_compute_row_major_strides(shape)?;
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    Ok(strides)
}

fn jet_compute_tensor_view_bounds(
    tensor: &JetTensor,
    offset: usize,
    expected_len: usize,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, metadata_offset) = jet_compute_view_metadata(tensor)?;
    if strides != expected_strides || metadata_offset != offset {
        return Err(JetComputeError::Unsupported(
            "this operation requires a contiguous Tensor view".to_string(),
        ));
    }
    let end = offset.checked_add(expected_len).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })?;
    if end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    Ok(offset..end)
}

fn jet_compute_view_storage_end(
    tensor: &JetTensor,
    strides: &[i64],
    offset: usize,
) -> Result<usize, JetComputeError> {
    let mut relative_end = 0usize;
    for (&dim, &stride) in tensor.shape.iter().zip(strides.iter()) {
        if dim == 0 {
            continue;
        }
        let dim = usize::try_from(dim).map_err(|_| {
            JetComputeError::InvalidShape("Tensor shape axis is too large".to_string())
        })?;
        let stride = usize::try_from(stride).map_err(|_| {
            JetComputeError::InvalidShape(
                "Tensor view strides must be non-negative and representable".to_string(),
            )
        })?;
        let extent = dim.checked_sub(1).and_then(|last| last.checked_mul(stride)).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
        relative_end = relative_end.checked_add(extent).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
    }
    offset.checked_add(relative_end).and_then(|end| end.checked_add(1)).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })
}

fn jet_compute_tensor_values(tensor: &JetTensor) -> Vec<f64> {
    let Ok(expected_len) = jet_compute_storage_len(&tensor.shape) else {
        return Vec::new();
    };
    let Ok((strides, offset)) = jet_compute_view_metadata(tensor) else {
        return Vec::new();
    };
    let data = tensor.data.as_ref();
    if expected_len == 0 {
        return Vec::new();
    }
    let mut values = Vec::with_capacity(expected_len);
    for flat in 0..expected_len {
        let mut remainder = flat;
        let mut relative_offset = 0usize;
        for axis in (0..tensor.shape.len()).rev() {
            let dim = match usize::try_from(tensor.shape[axis]) {
                Ok(dim) if dim != 0 => dim,
                _ => return Vec::new(),
            };
            let index = remainder % dim;
            remainder /= dim;
            let stride = match usize::try_from(strides[axis]) {
                Ok(stride) => stride,
                Err(_) => return Vec::new(),
            };
            let term = match index.checked_mul(stride) {
                Some(term) => term,
                None => return Vec::new(),
            };
            relative_offset = match relative_offset.checked_add(term) {
                Some(offset) => offset,
                None => return Vec::new(),
            };
        }
        let physical_offset = match offset.checked_add(relative_offset) {
            Some(offset) => offset,
            None => return Vec::new(),
        };
        let Some(value) = data.get(physical_offset).copied() else {
            return Vec::new();
        };
        values.push(value);
    }
    values
}

fn jet_compute_validate_placement(
    device: JetComputeDevice,
    receipt: &JetComputePlacementReceipt,
) -> Result<(), JetComputeError> {
    if device == JetComputeDevice::Auto || receipt.selected == JetComputeDevice::Auto {
        return Err(JetComputeError::Unsupported(
            "a Tensor must record the concrete backend selected by Auto placement".to_string(),
        ));
    }
    let Some(expected_capabilities) = jet_compute_registered_capabilities(&receipt.profile) else {
        return Err(JetComputeError::Unsupported(format!(
            "compute profile `{}` is not registered by a backend capability",
            receipt.profile
        )));
    };
    if device != receipt.selected
        || !matches!(
            (receipt.requested, receipt.selected),
            (JetComputeDevice::Cpu, JetComputeDevice::Cpu)
                | (JetComputeDevice::Auto, JetComputeDevice::Cpu)
        )
        || receipt.backend != CPU_ORACLE_BACKEND
        || receipt.version != CPU_ORACLE_VERSION
        || receipt.cache != CPU_ORACLE_CACHE
        || receipt.reason.is_empty()
        || receipt.reason.chars().any(char::is_control)
        || !jet_compute_capabilities_match(&receipt.capabilities, expected_capabilities)
    {
        return Err(JetComputeError::Device(
            "Tensor placement receipt does not match a registered backend capability".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_validate_tensor(tensor: &JetTensor) -> Result<(), JetComputeError> {
    jet_compute_validate_placement(tensor.device, &tensor.last_placement)?;
    let expected_len = jet_compute_storage_len(&tensor.shape)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    if strides.iter().any(|stride| *stride < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor view strides must be non-negative".to_string(),
        ));
    }
    if strides
        .iter()
        .zip(tensor.shape.iter())
        .any(|(stride, dim)| *dim > 1 && *stride == 0)
    {
        return Err(JetComputeError::Unsupported(
            "zero-stride Tensor views are not writable aliases".to_string(),
        ));
    }
    if expected_len == 0 {
        if offset > tensor.data.len() {
            return Err(JetComputeError::InvalidShape(
                "empty Tensor view starts outside backing storage".to_string(),
            ));
        }
        return Ok(());
    }
    let storage_end = jet_compute_view_storage_end(tensor, strides, offset)?;
    if storage_end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    if values.len() != expected_len {
        return Err(JetComputeError::InvalidShape(
            "Tensor view metadata does not address its logical storage".to_string(),
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_place(
    requested: JetComputeDevice,
) -> Result<JetComputePlacementReceipt, JetComputeError> {
    // Epoch 3 registers one CPU capability. Auto selects that capability and
    // records the choice; it never fabricates an accelerator or silently
    // changes precision. Experts can still pin CPU explicitly.
    let capabilities = CPU_ORACLE_F64_CAPABILITIES
        .iter()
        .map(|capability| (*capability).to_string())
        .collect();
    let selected = JetComputeDevice::Cpu;
    Ok(JetComputePlacementReceipt {
        requested,
        selected,
        backend: CPU_ORACLE_BACKEND.to_string(),
        version: CPU_ORACLE_VERSION.to_string(),
        profile: CPU_ORACLE_F64_PROFILE.to_string(),
        cache: CPU_ORACLE_CACHE.to_string(),
        capabilities,
        reason: if requested == JetComputeDevice::Auto {
            "policy=auto; selected=cpu; capability=cpu-oracle.f64".to_string()
        } else {
            "policy=explicit; selected=cpu; capability=cpu-oracle.f64".to_string()
        },
    })
}

fn jet_compute_inherit_placement(mut tensor: JetTensor, source: &JetTensor) -> JetTensor {
    tensor.device = source.device;
    tensor.last_placement = source.last_placement.clone();
    tensor.last_transfer = None;
    tensor
}

fn jet_compute_tensor_from_shape(
    shape: Vec<i64>,
    fill: f64,
    requested: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    if !fill.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let n = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(requested)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(vec![fill; n]),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_zeros(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 0.0, JetComputeDevice::Auto)
}

fn jet_compute_ones(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 1.0, JetComputeDevice::Auto)
}

fn jet_compute_full(shape: &Vec<i64>, value: f64) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), value, JetComputeDevice::Auto)
}

fn jet_compute_from_list(values: &Vec<f64>) -> Result<JetTensor, JetComputeError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let len = i64::try_from(values.len()).map_err(|_| {
        JetComputeError::InvalidShape("Tensor list is too long".to_string())
    })?;
    let shape = vec![len];
    let strides = jet_compute_row_major_strides(&shape)?;
    let storage_len = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(values[..storage_len].to_vec()),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

/// Return the flat storage range selected by a bracket range.  A Tensor range
/// selects rows on its first axis, so a rank-2 matrix window keeps complete
/// rows and a higher-rank window keeps complete first-axis slabs.  This is the
/// one-dimensional bracket surface ratified by D-SHAPE-PLACE1; the inner axes
/// remain part of the same contiguous storage projection.
fn jet_compute_window_bounds(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    if strides != expected_strides {
        return Err(JetComputeError::Unsupported(
            "Tensor window is non-contiguous; use the strided View projection".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    // In row-major storage the first stride is the number of scalar values in
    // one first-axis slab.  For an empty first axis the selected range is also
    // empty, so the stride is still safe to use.
    let slab = strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let flat_start = axis_start.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let flat_end = axis_end.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    let start = usize::try_from(flat_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view start is outside storage".to_string())
    })?;
    let end = usize::try_from(flat_end).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view end is outside storage".to_string())
    })?;
    let start = offset.checked_add(start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let end = offset.checked_add(end).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    if end > tensor.data.len() || start > end {
        return Err(JetComputeError::OutOfBounds(
            "Tensor view is outside storage".to_string(),
        ));
    }
    Ok(start..end)
}

fn jet_compute_slice_checked(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor views are not differentiable; reshape or copy before transforming".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    let mut shape = tensor.shape.clone();
    shape[0] = axis_end.checked_sub(axis_start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor slice has a negative extent".to_string())
    })?;
    let (source_strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let first_stride = source_strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let first_stride = usize::try_from(first_stride).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view stride is not representable".to_string())
    })?;
    let axis_start = usize::try_from(axis_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor slice start is not representable".to_string())
    })?;
    let start_offset = base_offset
        .checked_add(axis_start.checked_mul(first_stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string())
        })?)
        .ok_or_else(|| JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string()))?;
    let mut strides = source_strides.to_vec();
    if start_offset != 0 {
        strides.push(i64::try_from(start_offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    let slice = JetTensor {
        shape,
        strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: tensor.last_transfer.clone(),
        trace: tensor.trace.clone(),
    };
    jet_compute_validate_tensor(&slice)?;
    Ok(slice)
}

fn jet_compute_slice(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> JetTensor {
    match jet_compute_slice_checked(tensor, start, end, exclusive) {
        Ok(slice) => slice,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_slice_range(
    tensor: &JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> JetTensor {
    jet_compute_slice(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view<'a>(
    tensor: &'a JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> &'a [f64] {
    if tensor.trace.is_some() {
        jet_panic(
            file,
            line,
            "Tensor views are not differentiable; reshape or copy before transforming",
        );
    }
    let bounds = match jet_compute_window_bounds(tensor, start, end, exclusive) {
        Ok(bounds) => bounds,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    };
    &tensor.data[bounds]
}

fn jet_compute_view_range<'a>(
    tensor: &'a JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a [f64] {
    jet_compute_view(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view_mut<'a>(
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> &'a mut [f64] {
    if tensor.trace.is_some() {
        jet_panic(
            file,
            line,
            "Tensor mutation is not differentiable; use a pure Tensor function",
        );
    }
    let bounds = match jet_compute_window_bounds(tensor, start, end, exclusive) {
        Ok(bounds) => bounds,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    };
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        jet_panic(
            file,
            line,
            "Tensor mutable view requires exclusive backing storage",
        );
    };
    &mut data[bounds]
}

fn jet_compute_view_mut_range<'a>(
    tensor: &'a mut JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a mut [f64] {
    jet_compute_view_mut(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_tensor_shape(tensor: &JetTensor) -> Vec<i64> {
    tensor.shape.clone()
}

fn jet_compute_tensor_rank(tensor: &JetTensor) -> i64 {
    i64::try_from(tensor.shape.len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_numel(tensor: &JetTensor) -> i64 {
    i64::try_from(jet_compute_tensor_values(tensor).len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_device(tensor: &JetTensor) -> String {
    tensor.device.jet_show()
}

fn jet_compute_tensor_placement(tensor: &JetTensor) -> String {
    tensor.last_placement.jet_show()
}

fn jet_compute_tensor_to_list(tensor: &JetTensor) -> Vec<f64> {
    if tensor.trace.is_some() {
        jet_panic(
            "Compute.rs",
            line!(),
            "Tensor value reads have no registered autodiff rule",
        );
    }
    jet_compute_tensor_values(tensor)
}

fn jet_compute_offset(tensor: &JetTensor, indices: &[i64]) -> Result<usize, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if indices.len() != tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(format!(
            "expected {} indices, got {}",
            tensor.shape.len(),
            indices.len()
        )));
    }
    let (strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let mut relative_offset = 0i64;
    for (i, (&idx, (&dim, &stride))) in indices
        .iter()
        .zip(tensor.shape.iter().zip(strides.iter()))
        .enumerate()
    {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "index {} out of range for axis {} of extent {}",
                idx, i, dim
            )));
        }
        let term = idx.checked_mul(stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
        relative_offset = relative_offset.checked_add(term).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
    }
    usize::try_from(relative_offset)
        .ok()
        .and_then(|relative| base_offset.checked_add(relative))
        .filter(|index| *index < tensor.data.len())
        .ok_or_else(|| JetComputeError::OutOfBounds("tensor index is outside storage".to_string()))
}

fn jet_compute_get_raw(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    let offset = jet_compute_offset(tensor, indices)?;
    tensor.data.get(offset).ok_or_else(|| {
        JetComputeError::OutOfBounds("tensor index is outside storage".to_string())
    }).copied()
}

fn jet_compute_get(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor element reads have no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_get_raw(tensor, indices)
}

fn jet_compute_set(
    tensor: &mut JetTensor,
    indices: &[i64],
    value: f64,
) -> Result<(), JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
        ));
    }
    if !value.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let offset = jet_compute_offset(tensor, indices)?;
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        return Err(JetComputeError::Unsupported(
            "Tensor write requires an exclusive ViewMut borrow".to_string(),
        ));
    };
    let Some(slot) = data.get_mut(offset) else {
        return Err(JetComputeError::OutOfBounds(
            "tensor index is outside storage".to_string(),
        ));
    };
    *slot = value;
    Ok(())
}

fn jet_compute_add(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("add", a, b)
}

fn jet_compute_mul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("mul", a, b)
}

fn jet_compute_reshape(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let tensor_n = jet_compute_storage_len(&tensor.shape)?;
    if n != tensor_n {
        return Err(JetComputeError::InvalidShape(format!(
            "reshape numel {} does not match tensor numel {}",
            n,
            tensor_n
        )));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let source_row_major = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, data) = if source_strides == source_row_major {
        (jet_compute_view_strides(shape, offset)?, tensor.data.clone())
    } else {
        // Reshape is an explicit rank/layout conversion. A non-contiguous view
        // cannot be relabeled as contiguous storage, so materialize its logical
        // order into a new owner.
        (
            jet_compute_row_major_strides(shape)?,
            std::sync::Arc::new(jet_compute_tensor_values(tensor)),
        )
    };
    let output = JetTensor {
        shape: shape.clone(),
        strides,
        data,
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Reshape {
            source_shape: tensor.shape.clone(),
        },
    )
}

/// Matrix alias: rank-2 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_matrix(rows: i64, cols: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if rows < 0 || cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "Matrix rows and cols must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![rows, cols], fill, JetComputeDevice::Cpu)
}

/// Vec alias: rank-1 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_vec(len: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if len < 0 {
        return Err(JetComputeError::InvalidShape(
            "Vec length must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![len], fill, JetComputeDevice::Cpu)
}

fn jet_compute_matmul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul inner dims {} and {} disagree",
            k, k2
        )));
    }
    let mut out = jet_compute_tensor_from_shape(vec![m, n], 0.0, JetComputeDevice::Cpu)?;
    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for t in 0..k {
                let av = jet_compute_get_raw(a, &vec![i, t])?;
                let bv = jet_compute_get_raw(b, &vec![t, j])?;
                sum += av * bv;
                if !sum.is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "matmul accumulation produced a non-finite value".to_string(),
                    ));
                }
            }
            jet_compute_set(&mut out, &vec![i, j], sum)?;
        }
    }
    jet_compute_record(
        out,
        &[a, b],
        vec![a.clone(), b.clone()],
        JetComputeTapeRule::Matmul,
    )
}

fn jet_compute_device_cpu() -> JetComputeDevice {
    JetComputeDevice::Cpu
}

fn jet_compute_device_auto() -> JetComputeDevice {
    JetComputeDevice::Auto
}

fn jet_compute_on_device(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let receipt = jet_compute_place(device)?;
    Ok(JetTensor {
        shape: tensor.shape.clone(),
        strides: tensor.strides.clone(),
        data: tensor.data.clone(),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: tensor.trace.clone(),
    })
}

// ── D-COMPUTE1=D / #1136: ndarray broadcast, ufuncs, reductions ─────────────

fn jet_compute_broadcast_shape(
    a: &[i64],
    b: &[i64],
) -> Result<Vec<i64>, JetComputeError> {
    if a.is_empty() || b.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "broadcasting requires ranked tensors".to_string(),
        ));
    }
    if a.iter().chain(b.iter()).any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "broadcast shapes cannot contain negative axes".to_string(),
        ));
    }
    let rank = a.len().max(b.len());
    let mut out = vec![1i64; rank];
    for i in 0..rank {
        let da = if i < rank - a.len() {
            1
        } else {
            a[i - (rank - a.len())]
        };
        let db = if i < rank - b.len() {
            1
        } else {
            b[i - (rank - b.len())]
        };
        if da == db {
            out[i] = da;
        } else if da == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = db;
        } else if db == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = da;
        } else {
            return Err(JetComputeError::RankMismatch(format!(
                "cannot broadcast shapes {:?} and {:?}",
                a, b
            )));
        }
    }
    jet_compute_storage_len(&out)?;
    Ok(out)
}

fn jet_compute_materialize_broadcast(
    tensor: &JetTensor,
    shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let strides = jet_compute_row_major_strides(shape)?;
    let src_rank = tensor.shape.len();
    let dst_rank = shape.len();
    if src_rank == 0 || src_rank > dst_rank {
        return Err(JetComputeError::RankMismatch(format!(
            "cannot broadcast rank {} into rank {}",
            src_rank, dst_rank
        )));
    }
    if jet_compute_broadcast_shape(&tensor.shape, shape)? != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    // Empty output has no source element to read.  This also makes shapes such
    // as [0, 3] broadcast-safe instead of indexing an empty backing vector.
    if n == 0 {
        let receipt = jet_compute_place(JetComputeDevice::Cpu)?;
        return Ok(JetTensor {
            shape: shape.to_vec(),
            strides,
            data: std::sync::Arc::new(Vec::new()),
            device: receipt.selected,
            last_placement: receipt,
            last_transfer: None,
            trace: None,
        });
    }
    let mut data = Vec::with_capacity(n);
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut destination_coords = vec![0i64; dst_rank];
        for axis in (0..dst_rank).rev() {
            let dim = shape[axis];
            destination_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let rank_delta = dst_rank - src_rank;
        let source_coords = (0..src_rank)
            .map(|axis| {
                if tensor.shape[axis] == 1 {
                    0
                } else {
                    destination_coords[rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        data.push(jet_compute_get_raw(tensor, &source_coords)?);
    }
    let receipt = jet_compute_place(JetComputeDevice::Cpu)?;
    Ok(JetTensor {
        shape: shape.to_vec(),
        strides,
        data: std::sync::Arc::new(data),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_broadcast_to(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    let out_shape = jet_compute_broadcast_shape(&tensor.shape, shape)?;
    if &out_shape != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    let output = jet_compute_materialize_broadcast(tensor, shape)?;
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Broadcast {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_transpose(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "transpose requires rank-2 tensor".to_string(),
        ));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let mut strides = vec![source_strides[1], source_strides[0]];
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    let out = JetTensor {
        shape: vec![tensor.shape[1], tensor.shape[0]],
        strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_validate_tensor(&out)?;
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Transpose,
    )
}

fn jet_compute_sum_axis(tensor: &JetTensor, axis: i64) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let Some(axis) = usize::try_from(axis)
        .ok()
        .filter(|index| *index < tensor.shape.len())
    else {
        return Err(JetComputeError::OutOfBounds(format!(
            "sum_axis axis {} out of range for rank {}",
            axis,
            tensor.shape.len()
        )));
    };
    let mut out_shape = Vec::new();
    for (i, &d) in tensor.shape.iter().enumerate() {
        if i != axis {
            out_shape.push(d);
        }
    }
    if out_shape.is_empty() {
        out_shape.push(1);
    }
    let mut out = jet_compute_tensor_from_shape(out_shape.clone(), 0.0, JetComputeDevice::Cpu)?;
    let axis_len = tensor.shape[axis];
    let out_n = jet_compute_numel(&out_shape)?;
    for flat in 0..out_n {
        let mut coords = vec![0i64; tensor.shape.len()];
        let mut rem = flat;
        let mut out_coords = vec![0i64; out_shape.len()];
        for i in (0..out_shape.len()).rev() {
            let dim = out_shape[i];
            out_coords[i] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut o = 0usize;
        for i in 0..tensor.shape.len() {
            if i == axis {
                coords[i] = 0;
            } else {
                coords[i] = out_coords[o];
                o += 1;
            }
        }
        let mut sum = 0.0;
        for k in 0..axis_len {
            coords[axis] = k;
            sum += jet_compute_get_raw(tensor, &coords)?;
            if !sum.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sum_axis accumulation produced a non-finite value".to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &out_coords, sum)?;
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::SumAxis {
            axis,
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_unary(op: &str, tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if !matches!(op, "negate" | "abs" | "exp" | "log" | "sqrt") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported unary compute operation `{op}`"
        )));
    }
    let receipt = jet_compute_place(JetComputeDevice::Cpu)?;
    let values = jet_compute_tensor_values(tensor);
    let mut data = Vec::with_capacity(values.len());
    for value in values {
        let output = match op {
            "negate" => -value,
            "abs" => value.abs(),
            "exp" => value.exp(),
            "log" if value > 0.0 => value.ln(),
            "log" => {
                return Err(JetComputeError::Arithmetic(
                    "log requires strictly positive values".to_string(),
                ));
            }
            "sqrt" if value >= 0.0 => value.sqrt(),
            "sqrt" => {
                return Err(JetComputeError::Arithmetic(
                    "sqrt requires non-negative values".to_string(),
                ));
            }
            _ => unreachable!("unvalidated unary operation"),
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(format!(
                "unary operation `{op}` produced a non-finite value"
            )));
        }
        data.push(output);
    }
    let output = JetTensor {
        shape: tensor.shape.clone(),
        strides: jet_compute_row_major_strides(&tensor.shape)?,
        data: std::sync::Arc::new(data),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    };
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Unary(op.to_string()),
    )
}

fn jet_compute_binary(
    op: &str,
    a: &JetTensor,
    b: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let shape = jet_compute_broadcast_shape(&a.shape, &b.shape)?;
    if !matches!(op, "sub" | "div" | "maximum" | "minimum" | "add" | "mul") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported binary compute operation `{op}`"
        )));
    }
    // D-COMPUTE-FUSE1: broadcast indexing and the elementwise operation are
    // one eager Prelude loop. Do not materialize either broadcast operand;
    // this is the shared fusion path for AOT, comptime, and dev evaluation.
    let receipt = jet_compute_place(JetComputeDevice::Cpu)?;
    let n = jet_compute_storage_len(&shape)?;
    let mut data = Vec::with_capacity(n);
    let rank = shape.len();
    let left_rank_delta = rank - a.shape.len();
    let right_rank_delta = rank - b.shape.len();
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut output_coords = vec![0i64; rank];
        for axis in (0..rank).rev() {
            let dim = shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let left_coords = (0..a.shape.len())
            .map(|axis| {
                if a.shape[axis] == 1 {
                    0
                } else {
                    output_coords[left_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let right_coords = (0..b.shape.len())
            .map(|axis| {
                if b.shape[axis] == 1 {
                    0
                } else {
                    output_coords[right_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let x = jet_compute_get_raw(a, &left_coords)?;
        let y = jet_compute_get_raw(b, &right_coords)?;
        if op == "div" && y == 0.0 {
            return Err(JetComputeError::Arithmetic(
                "division by zero in compute operation".to_string(),
            ));
        }
        let output = match op {
            "sub" => x - y,
            "div" => x / y,
            "maximum" => x.max(y),
            "minimum" => x.min(y),
            "add" => x + y,
            "mul" => x * y,
            _ => unreachable!("unvalidated binary operation"),
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "compute operation produced a non-finite value".to_string(),
            ));
        }
        data.push(output);
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let output = JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(data),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    };
    let rule = match op {
        "add" => JetComputeTapeRule::Add,
        "sub" => JetComputeTapeRule::Sub,
        "mul" => JetComputeTapeRule::Mul,
        "div" => JetComputeTapeRule::Div,
        "maximum" => JetComputeTapeRule::Maximum,
        "minimum" => JetComputeTapeRule::Minimum,
        _ => unreachable!("validated binary operation"),
    };
    jet_compute_record(output, &[a, b], vec![a.clone(), b.clone()], rule)
}

// ── #1137 / D-COMPUTE1: dense linalg on the Tensor CPU oracle ───────────────

fn jet_compute_eye(n: i64) -> Result<JetTensor, JetComputeError> {
    if n < 0 {
        return Err(JetComputeError::InvalidShape(
            "eye size must be non-negative".to_string(),
        ));
    }
    let mut out = jet_compute_tensor_from_shape(vec![n, n], 0.0, JetComputeDevice::Cpu)?;
    for i in 0..n {
        jet_compute_set(&mut out, &vec![i, i], 1.0)?;
    }
    Ok(out)
}

fn jet_compute_det(tensor: &JetTensor) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "det has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "det requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("det dimension is too large".to_string())
    })?;
    let matrix_len = n.checked_mul(n).ok_or_else(|| {
        JetComputeError::InvalidShape("det matrix storage length overflow".to_string())
    })?;
    let values = jet_compute_tensor_values(tensor);
    if matrix_len != values.len() {
        return Err(JetComputeError::InvalidShape(
            "det matrix storage is inconsistent".to_string(),
        ));
    }
    let mut a = values.to_vec();
    let mut det = 1.0;
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * n + i].abs() > a[pivot * n + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * n + i].abs() < 1e-15 {
            return Ok(0.0);
        }
        if pivot != i {
            for c in 0..n {
                a.swap(i * n + c, pivot * n + c);
            }
            det = -det;
        }
        let piv = a[i * n + i];
        det *= piv;
        if !det.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "det overflowed to a non-finite value".to_string(),
            ));
        }
        for r in (i + 1)..n {
            let factor = a[r * n + i] / piv;
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "det elimination produced a non-finite factor".to_string(),
                ));
            }
            for c in i..n {
                a[r * n + c] -= factor * a[i * n + c];
                if !a[r * n + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "det elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    Ok(det)
}

fn jet_compute_inv(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "inv has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "inv requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("inv dimension is too large".to_string())
    })?;
    let width = n.checked_mul(2).ok_or_else(|| {
        JetComputeError::InvalidShape("inv augmented width overflow".to_string())
    })?;
    let matrix_len = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("inv matrix storage length overflow".to_string())
    })?;
    if matrix_len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "inv workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut a = vec![0.0; matrix_len];
    for i in 0..n {
        for j in 0..n {
            a[i * width + j] = jet_compute_get_raw(tensor, &vec![i as i64, j as i64])?;
            a[i * width + n + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * width + i].abs() > a[pivot * width + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * width + i].abs() < 1e-15 {
            return Err(JetComputeError::InvalidShape(
                "matrix is singular".to_string(),
            ));
        }
        if pivot != i {
            for c in 0..width {
                a.swap(i * width + c, pivot * width + c);
            }
        }
        let piv = a[i * width + i];
        for c in 0..width {
            a[i * width + c] /= piv;
            if !a[i * width + c].is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "inv normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r * width + i];
            for c in 0..width {
                a[r * width + c] -= factor * a[i * width + c];
                if !a[r * width + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "inv elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(
        vec![tensor.shape[0], tensor.shape[1]],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    for i in 0..n {
        for j in 0..n {
            jet_compute_set(&mut out, &vec![i as i64, j as i64], a[i * width + n + j])?;
        }
    }
    Ok(out)
}

fn jet_compute_solve(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.trace.is_some() || b.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "solve has no registered autodiff rule".to_string(),
        ));
    }
    if a.shape.len() != 2 || a.shape[0] != a.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "solve requires a square rank-2 coefficient tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let n = usize::try_from(a.shape[0])
        .map_err(|_| JetComputeError::InvalidShape("solve dimension is too large".to_string()))?;
    let rhs_cols = match b.shape.as_slice() {
        [rows] if *rows == a.shape[0] => 1,
        [rows, cols] if *rows == a.shape[0] && *cols >= 0 => usize::try_from(*cols).map_err(|_| {
            JetComputeError::InvalidShape("solve right-hand side is too large".to_string())
        })?,
        _ => {
            return Err(JetComputeError::RankMismatch(format!(
                "solve expects a length-{} vector or a matrix with {} rows",
                a.shape[0], a.shape[0]
            )))
        }
    };
    let width = n.checked_add(rhs_cols).ok_or_else(|| {
        JetComputeError::InvalidShape("solve augmented width overflow".to_string())
    })?;
    let workspace = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("solve workspace length overflow".to_string())
    })?;
    if workspace > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "solve workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut augmented = vec![vec![0.0; width]; n];
    for row in 0..n {
        for col in 0..n {
            augmented[row][col] = jet_compute_get_raw(a, &[row as i64, col as i64])?;
        }
        for col in 0..rhs_cols {
            augmented[row][n + col] = if b.shape.len() == 1 {
                jet_compute_get_raw(b, &[row as i64])?
            } else {
                jet_compute_get_raw(b, &[row as i64, col as i64])?
            };
        }
    }
    for pivot in 0..n {
        let mut best = pivot;
        for row in pivot..n {
            if augmented[row][pivot].abs() > augmented[best][pivot].abs() {
                best = row;
            }
        }
        if augmented[best][pivot].abs() < 1e-15 {
            return Err(JetComputeError::Arithmetic(
                "solve coefficient matrix is singular".to_string(),
            ));
        }
        augmented.swap(pivot, best);
        let divisor = augmented[pivot][pivot];
        for value in &mut augmented[pivot][pivot..] {
            *value /= divisor;
            if !value.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for row in 0..n {
            if row == pivot {
                continue;
            }
            let factor = augmented[row][pivot];
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve elimination produced a non-finite factor".to_string(),
                ));
            }
            for col in pivot..width {
                augmented[row][col] -= factor * augmented[pivot][col];
                if !augmented[row][col].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "solve elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let output_shape = if b.shape.len() == 1 {
        vec![a.shape[0]]
    } else {
        vec![a.shape[0], b.shape[1]]
    };
    let mut out = jet_compute_tensor_from_shape(output_shape, 0.0, JetComputeDevice::Cpu)?;
    for row in 0..n {
        for col in 0..rhs_cols {
            let index = if b.shape.len() == 1 {
                vec![row as i64]
            } else {
                vec![row as i64, col as i64]
            };
            jet_compute_set(&mut out, &index, augmented[row][n + col])?;
        }
    }
    Ok(out)
}

/// Naive DFT on a rank-1 real tensor → interleaved [re, im, re, im, …] length 2n.
fn jet_compute_fft(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "fft has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if tensor.shape.len() != 1 {
        return Err(JetComputeError::RankMismatch(
            "fft requires a rank-1 tensor".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    let n = values.len();
    let output_len = n
        .checked_mul(2)
        .and_then(|length| i64::try_from(length).ok())
        .ok_or_else(|| JetComputeError::InvalidShape("fft output length overflow".to_string()))?;
    let mut out = jet_compute_tensor_from_shape(
        vec![output_len],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    if n == 0 {
        return Ok(out);
    }
    for k in 0..n {
        let mut re = 0.0;
        let mut im = 0.0;
        for t in 0..n {
            let angle = -2.0 * std::f64::consts::PI * (k as f64) * (t as f64) / (n as f64);
            re += values[t] * angle.cos();
            im += values[t] * angle.sin();
        }
        jet_compute_set(&mut out, &vec![(2 * k) as i64], re)?;
        jet_compute_set(&mut out, &vec![(2 * k + 1) as i64], im)?;
    }
    Ok(out)
}

// ── #1138: stream + transfer receipts (CPU oracle) ──────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeStream {
    id: i64,
    device: JetComputeDevice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeTransferReceipt {
    from: JetComputeDevice,
    to: JetComputeDevice,
    bytes: i64,
    fallback: String,
}

fn jet_compute_validate_transfer_receipt(
    tensor: &JetTensor,
    receipt: &JetComputeTransferReceipt,
) -> Result<(), JetComputeError> {
    if receipt.from == JetComputeDevice::Auto || receipt.to == JetComputeDevice::Auto {
        return Err(JetComputeError::Device(
            "transfer receipt must name concrete source and destination devices".to_string(),
        ));
    }
    if receipt.to != tensor.device {
        return Err(JetComputeError::Device(
            "transfer receipt destination does not match Tensor placement".to_string(),
        ));
    }
    let logical_bytes = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(std::mem::size_of::<f64>())
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let expected_bytes = if receipt.from == receipt.to {
        0
    } else {
        logical_bytes
    };
    if receipt.bytes != expected_bytes
        || (receipt.from == receipt.to && receipt.fallback != "none")
        || (receipt.from != receipt.to && receipt.fallback == "none")
    {
        return Err(JetComputeError::Device(
            "transfer receipt does not match the selected backend operation".to_string(),
        ));
    }
    Ok(())
}

impl JetShow for JetComputeStream {
    fn jet_show(&self) -> String {
        // Stream identity is runtime-local; exposing it would make AOT/JIT
        // output differ despite identical compute semantics.
        format!("ComputeStream(device={})", self.device.jet_show())
    }
}

impl JetShow for JetComputeTransferReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Transfer(from={}, to={}, bytes={}, fallback={})",
            self.from.jet_show(),
            self.to.jet_show(),
            self.bytes,
            self.fallback
        )
    }
}

fn jet_compute_stream_new() -> JetComputeStream {
    static NEXT_STREAM_ID: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(1);
    JetComputeStream {
        id: NEXT_STREAM_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        device: JetComputeDevice::Cpu,
    }
}

fn jet_compute_stream_sync(stream: &JetComputeStream) -> Result<(), JetComputeError> {
    if stream.id <= 0 {
        return Err(JetComputeError::Device(
            "cannot synchronize an invalid compute stream".to_string(),
        ));
    }
    if stream.device != JetComputeDevice::Cpu {
        return Err(JetComputeError::Unsupported(
            "only CPU compute streams are available in this profile".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_stream_show(stream: &JetComputeStream) -> String {
    stream.jet_show()
}

fn jet_compute_transfer(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let logical_byte_count = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(std::mem::size_of::<f64>())
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let from = tensor.device;
    let mut out = jet_compute_on_device(tensor, device)?;
    let (bytes, transfer_kind) = if from == out.device {
        (0, "no-op; same backend and allocation".to_string())
    } else {
        (logical_byte_count, "copy; selected backend".to_string())
    };
    out.last_placement.reason = format!(
        "transfer kind={transfer_kind} bytes={bytes} from={} to={}",
        from.jet_show(),
        out.device.jet_show()
    );
    out.last_transfer = Some(JetComputeTransferReceipt {
        from,
        to: out.device,
        bytes,
        fallback: if from == out.device {
            "none".to_string()
        } else {
            "not-applicable".to_string()
        },
    });
    Ok(out)
}

fn jet_compute_transfer_show(tensor: &JetTensor) -> String {
    tensor
        .last_transfer
        .as_ref()
        .map_or_else(|| tensor.last_placement.jet_show(), |receipt| receipt.jet_show())
}

// ── #1139 / #1140: safe kernel bounds + typed raw-kernel boundary ────────────

fn jet_compute_kernel_bounds_ok(
    shape: &[i64],
    indices: &[i64],
) -> Result<bool, JetComputeError> {
    if shape.len() != indices.len() {
        return Err(JetComputeError::RankMismatch(
            "kernel index rank must match tensor shape".to_string(),
        ));
    }
    jet_compute_storage_len(shape)?;
    if shape.iter().any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "kernel shape axes must be non-negative".to_string(),
        ));
    }
    for (i, (&idx, &dim)) in indices.iter().zip(shape.iter()).enumerate() {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "kernel index {idx} out of bounds for axis {i} (extent {dim})"
            )));
        }
    }
    Ok(true)
}

// ── #1141 / D-COMPUTE-AUTODIFF1: reverse-mode VJP + JVP for dense ops ────────

/// Reverse-mode broadcast rule: axes introduced by broadcasting and axes with
/// extent one are summed back into the operand's original shape.
fn jet_compute_reduce_to_shape(
    tensor: &JetTensor,
    target_shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if target_shape.is_empty() || target_shape.len() > tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(
            "gradient target must be a ranked tensor with no greater rank".to_string(),
        ));
    }
    if jet_compute_broadcast_shape(target_shape, &tensor.shape)? != tensor.shape {
        return Err(JetComputeError::RankMismatch(format!(
            "gradient shape {:?} is not broadcast-compatible with {:?}",
            target_shape, tensor.shape
        )));
    }
    let mut out = jet_compute_tensor_from_shape(
        target_shape.to_vec(),
        0.0,
        JetComputeDevice::Cpu,
    )?;
    let rank_delta = tensor.shape.len() - target_shape.len();
    let values = jet_compute_tensor_values(tensor);
    for flat in 0..values.len() {
        let mut rem = flat as i64;
        let mut output_coords = vec![0i64; tensor.shape.len()];
        for axis in (0..tensor.shape.len()).rev() {
            let dim = tensor.shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut target_coords = vec![0i64; target_shape.len()];
        for axis in 0..target_shape.len() {
            let source_axis = axis + rank_delta;
            target_coords[axis] = if target_shape[axis] == 1 {
                0
            } else {
                output_coords[source_axis]
            };
        }
        let target_offset = jet_compute_offset(&out, &target_coords)?;
        let Some(data) = std::sync::Arc::get_mut(&mut out.data) else {
            return Err(JetComputeError::Unsupported(
                "gradient accumulation requires exclusive output storage".to_string(),
            ));
        };
        let Some(slot) = data.get_mut(target_offset) else {
            return Err(JetComputeError::OutOfBounds(
                "gradient accumulation is outside storage".to_string(),
            ));
        };
        *slot += values[flat];
        if !slot.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "gradient accumulation produced a non-finite value".to_string(),
            ));
        }
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::ReduceToShape {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_zero_like(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    Ok(jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(tensor.shape.clone(), 0.0, JetComputeDevice::Cpu)?,
        tensor,
    ))
}

fn jet_compute_tensor_from_values_like(
    template: &JetTensor,
    values: &[f64],
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&template.shape)?;
    if values.len() != expected || values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "autodiff values do not match the Tensor shape".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape(
        template.shape.clone(),
        0.0,
        JetComputeDevice::Cpu,
    )?;
    let Some(storage) = std::sync::Arc::get_mut(&mut output.data) else {
        return Err(JetComputeError::Unsupported(
            "autodiff output requires exclusive storage".to_string(),
        ));
    };
    storage.clone_from_slice(values);
    Ok(jet_compute_inherit_placement(output, template))
}

fn jet_compute_unary_vjp(
    op: &str,
    input: &JetTensor,
    output: &JetTensor,
    cot: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(input)?;
    jet_compute_validate_tensor(output)?;
    jet_compute_validate_tensor(cot)?;
    if input.shape != output.shape || output.shape != cot.shape {
        return Err(JetComputeError::RankMismatch(
            "unary cotangent shape must equal the unary output".to_string(),
        ));
    }
    let input_values = jet_compute_tensor_values(input);
    match op {
        "negate" => jet_compute_unary("negate", cot),
        "abs" => {
            let signs = input_values
                .iter()
                .map(|value| {
                    if *value > 0.0 {
                        1.0
                    } else if *value < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>();
            let signs = jet_compute_tensor_from_values_like(input, &signs)?;
            jet_compute_binary("mul", &signs, cot)
        }
        "exp" => jet_compute_binary("mul", output, cot),
        "log" => jet_compute_binary("div", cot, input),
        "sqrt" => {
            let two = jet_compute_full(&output.shape, 2.0)?;
            let denominator = jet_compute_binary("mul", &two, output)?;
            jet_compute_binary("div", cot, &denominator)
        }
        _ => Err(JetComputeError::Unsupported(format!(
            "unsupported unary derivative `{op}`"
        ))),
    }
}

fn jet_compute_rule_gradients(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    cot: &JetTensor,
    active_tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Result<Vec<JetTensor>, JetComputeError> {
    let cot = jet_compute_remove_trace_level(cot, active_tape);
    match rule {
        JetComputeTapeRule::Add => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&cot, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Sub => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let negative = jet_compute_unary("negate", &cot)?;
            let b = jet_compute_reduce_to_shape(&negative, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Mul => {
            let a_full = jet_compute_binary("mul", &values[1], &cot)?;
            let b_full = jet_compute_binary("mul", &values[0], &cot)?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Div => {
            let a_full = jet_compute_binary("div", &cot, &values[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &cot)?;
            let b_full = jet_compute_unary(
                "negate",
                &jet_compute_binary("div", &numerator, &denominator)?,
            )?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for ((output, a), b) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no derivative at a tie".to_string(),
                    ));
                }
                let left_slot = if (maximum && *a == *output) || (!maximum && *a == *output) {
                    1.0
                } else {
                    0.0
                };
                let right_slot = if (maximum && *b == *output) || (!maximum && *b == *output) {
                    1.0
                } else {
                    0.0
                };
                left_mask.push(left_slot);
                right_mask.push(right_slot);
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &cot)?;
            let right = jet_compute_binary("mul", &right_mask, &cot)?;
            Ok(vec![
                jet_compute_reduce_to_shape(&left, &values[0].shape)?,
                jet_compute_reduce_to_shape(&right, &values[1].shape)?,
            ])
        }
        JetComputeTapeRule::Matmul => {
            let (a, b) = jet_compute_vjp_matmul(&values[0], &values[1], &cot)?;
            Ok(vec![a, b])
        }
        JetComputeTapeRule::Unary(op) => Ok(vec![jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &cot,
        )?]),
        JetComputeTapeRule::Reshape { source_shape } => Ok(vec![jet_compute_reshape(
            &cot,
            &source_shape.clone(),
        )?]),
        JetComputeTapeRule::Broadcast { source_shape } => Ok(vec![
            jet_compute_reduce_to_shape(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::ReduceToShape { source_shape } => Ok(vec![
            jet_compute_broadcast_to(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::Transpose => Ok(vec![jet_compute_transpose(&cot)?]),
        JetComputeTapeRule::SumAxis { axis, source_shape } => {
            let mut reduced_shape = source_shape.clone();
            reduced_shape[*axis] = 1;
            let cot = jet_compute_reshape(&cot, &reduced_shape)?;
            Ok(vec![jet_compute_broadcast_to(&cot, source_shape)?])
        }
    }
}

fn jet_compute_reverse(
    state: &JetComputeVjpState,
    seed: &JetTensor,
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_validate_tensor(&state.value)?;
    jet_compute_validate_tensor(seed)?;
    if state.value.shape != seed.shape {
        return Err(JetComputeError::RankMismatch(
            "VJP seed shape must equal the function output shape".to_string(),
        ));
    }
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    let mut cotangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    if let Some(output_node) = state.output_node {
        let Some(slot) = cotangents.get_mut(output_node) else {
            return Err(JetComputeError::Unsupported(
                "VJP output node is outside its tape".to_string(),
            ));
        };
        *slot = Some(jet_compute_untracked(seed));
    }
    for index in (0..nodes.len()).rev() {
        let Some(cot) = cotangents[index].take() else {
            continue;
        };
        let node = &nodes[index];
        let Some(rule) = &node.rule else {
            continue;
        };
        let gradients = jet_compute_rule_gradients(
            rule,
            &node.values,
            &node.output,
            &cot,
            &state.tape,
        )?;
        for (parent, gradient) in node.parents.iter().zip(gradients) {
            let Some(parent) = parent else {
                continue;
            };
            let gradient = jet_compute_remove_trace_level(&gradient, &state.tape);
            let Some(slot) = cotangents.get_mut(*parent) else {
                return Err(JetComputeError::Unsupported(
                    "VJP parent node is outside its tape".to_string(),
                ));
            };
            *slot = Some(match slot.take() {
                Some(previous) => jet_compute_binary("add", &previous, &gradient)?,
                None => gradient,
            });
        }
    }
    let mut result = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let gradient = cotangents
            .get(index)
            .and_then(Option::as_ref)
            .cloned()
            .unwrap_or(jet_compute_zero_like(input)?);
        result.push(jet_compute_inherit_placement(gradient, input));
    }
    Ok(result)
}

fn jet_compute_select_gradients(
    all: Vec<JetTensor>,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    targets
        .iter()
        .map(|target| {
            let index = usize::try_from(*target).map_err(|_| {
                JetComputeError::Unsupported("negative autodiff target index".to_string())
            })?;
            all.get(index).cloned().ok_or_else(|| {
                JetComputeError::Unsupported("autodiff target index is outside the function signature".to_string())
            })
        })
        .collect()
}

fn jet_compute_gradient_seed(state: &JetComputeVjpState) -> Result<JetTensor, JetComputeError> {
    if jet_compute_tensor_numel(&state.value) != 1 {
        return Err(JetComputeError::RankMismatch(
            "compute.gradient requires a scalar Tensor output".to_string(),
        ));
    }
    jet_compute_ones(&state.value.shape)
}

fn jet_compute_vjp_pull(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_select_gradients(jet_compute_reverse(state, seed)?, targets)
}

fn jet_compute_vjp_pull_or_panic(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    match jet_compute_vjp_pull(state, seed, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_gradient_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    let seed = match jet_compute_gradient_seed(state) {
        Ok(seed) => seed,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    };
    jet_compute_vjp_pull_or_panic(state, &seed, targets, context)
}

fn jet_compute_vjp_unit_grads_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    jet_compute_gradient_or_panic(state, targets, context)
}

/// The one transform dispatcher used by AOT and the interpreter.  Engines
/// marshal callable arguments and package the typed result; this function
/// owns transform selection, scalar seeding, value detachment, and lazy VJP
/// state creation.
fn jet_compute_transform(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
) -> Result<JetComputeTransformResult, JetComputeError> {
    let value = jet_compute_remove_trace_level(&state.value, &state.tape);
    match method {
        "gradient" => Ok(JetComputeTransformResult::Gradient(
            jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        )),
        "value_and_gradient" => Ok(JetComputeTransformResult::ValueAndGradient {
            value,
            gradients: jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        }),
        "vjp" => Ok(JetComputeTransformResult::Vjp {
            value,
            state: state.clone(),
        }),
        "jvp" => Ok(JetComputeTransformResult::Jvp {
            value,
            tangent: jet_compute_jvp(state, tangents.to_vec())?,
        }),
        _ => Err(JetComputeError::Unsupported(format!(
            "unknown autodiff transform `{method}`"
        ))),
    }
}

fn jet_compute_transform_or_panic(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
    context: &str,
) -> JetComputeTransformResult {
    match jet_compute_transform(method, state, tangents, targets) {
        Ok(result) => result,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_nested_gradient(
    states: &[JetComputeVjpState],
    targets: &[i64],
) -> Result<Vec<Vec<JetTensor>>, JetComputeError> {
    states
        .iter()
        .map(|state| {
            let result = jet_compute_transform("gradient", state, &[], targets)?;
            let JetComputeTransformResult::Gradient(values) = result else {
                return Err(JetComputeError::Unsupported(
                    "nested gradient did not return gradients".to_string(),
                ));
            };
            Ok(values)
        })
        .collect()
}

fn jet_compute_nested_gradient_or_panic(
    states: &[JetComputeVjpState],
    targets: &[i64],
    context: &str,
) -> Vec<Vec<JetTensor>> {
    match jet_compute_nested_gradient(states, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_jvp_rule(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    tangents: &[JetTensor],
) -> Result<JetTensor, JetComputeError> {
    match rule {
        JetComputeTapeRule::Add => jet_compute_binary("add", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Sub => jet_compute_binary("sub", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Mul => {
            let left = jet_compute_binary("mul", &tangents[0], &values[1])?;
            let right = jet_compute_binary("mul", &values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Div => {
            let left = jet_compute_binary("div", &tangents[0], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &tangents[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let right = jet_compute_binary("div", &numerator, &denominator)?;
            jet_compute_binary("sub", &left, &right)
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_tangent = if tangents[0].shape == output.shape {
                tangents[0].clone()
            } else {
                jet_compute_broadcast_to(&tangents[0], &output.shape.to_vec())?
            };
            let right_tangent = if tangents[1].shape == output.shape {
                tangents[1].clone()
            } else {
                jet_compute_broadcast_to(&tangents[1], &output.shape.to_vec())?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let left_tangents = jet_compute_tensor_values(&left_tangent);
            let right_tangents = jet_compute_tensor_values(&right_tangent);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for (((output, a), b), (left, right)) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
                .zip(left_tangents.iter().zip(right_tangents.iter()))
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no JVP at a tie".to_string(),
                    ));
                }
                if (maximum && *a == *output) || (!maximum && *a == *output) {
                    left_mask.push(1.0);
                    right_mask.push(0.0);
                } else {
                    left_mask.push(0.0);
                    right_mask.push(1.0);
                }
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &left_tangent)?;
            let right = jet_compute_binary("mul", &right_mask, &right_tangent)?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Matmul => {
            let left = jet_compute_matmul(&tangents[0], &values[1])?;
            let right = jet_compute_matmul(&values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Unary(op) => jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &tangents[0],
        ),
        JetComputeTapeRule::Reshape { .. } => {
            jet_compute_reshape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Broadcast { .. } => {
            jet_compute_broadcast_to(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::ReduceToShape { .. } => {
            jet_compute_reduce_to_shape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Transpose => jet_compute_transpose(&tangents[0]),
        JetComputeTapeRule::SumAxis { axis, .. } => jet_compute_sum_axis(&tangents[0], *axis as i64),
    }
}

fn jet_compute_jvp(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
) -> Result<JetTensor, JetComputeError> {
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    if input_tangents.len() != inputs.len() {
        return Err(JetComputeError::RankMismatch(
            "JVP tangent count must equal the function input count".to_string(),
        ));
    }
    for (input, tangent) in inputs.iter().zip(input_tangents.iter()) {
        if input.shape != tangent.shape {
            return Err(JetComputeError::RankMismatch(
                "JVP tangent shapes must match their primal inputs".to_string(),
            ));
        }
    }
    let mut tangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    for (index, tangent) in input_tangents.into_iter().enumerate() {
        if let Some(slot) = tangents.get_mut(index) {
            *slot = Some(jet_compute_remove_trace_level(&tangent, &state.tape));
        }
    }
    for (index, node) in nodes.iter().enumerate().skip(inputs.len()) {
        let Some(rule) = &node.rule else {
            continue;
        };
        let mut node_tangents = Vec::with_capacity(node.parents.len());
        for (parent, value) in node.parents.iter().zip(node.values.iter()) {
            node_tangents.push(
                parent
                    .and_then(|parent| tangents.get(parent).and_then(Option::as_ref).cloned())
                    .unwrap_or(jet_compute_zero_like(value)?),
            );
        }
        tangents[index] = Some(jet_compute_jvp_rule(
            rule,
            &node.values,
            &node.output,
            &node_tangents,
        )?);
    }
    match state.output_node {
        Some(node) => tangents
            .get(node)
            .and_then(Option::clone)
            .ok_or_else(|| JetComputeError::Unsupported("JVP output tangent is unavailable".to_string())),
        None => jet_compute_zero_like(&state.value),
    }
}

fn jet_compute_jvp_or_panic(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
    context: &str,
) -> JetTensor {
    match jet_compute_jvp(state, input_tangents) {
        Ok(value) => value,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_vjp_matmul(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(cot)?;
    if a.shape.len() != 2
        || b.shape.len() != 2
        || cot.shape.len() != 2
        || a.shape[1] != b.shape[0]
        || cot.shape[0] != a.shape[0]
        || cot.shape[1] != b.shape[1]
    {
        return Err(JetComputeError::RankMismatch(
            "matmul cotangent shape must equal the matmul output".to_string(),
        ));
    }
    let b_t = jet_compute_transpose(b)?;
    let a_t = jet_compute_transpose(a)?;
    Ok((
        jet_compute_inherit_placement(jet_compute_matmul(cot, &b_t)?, a),
        jet_compute_inherit_placement(jet_compute_matmul(&a_t, cot)?, b),
    ))
}

// ── #1142: ML step + serialization over the Tensor oracle ───────────────────

fn jet_compute_mse_loss(pred: &JetTensor, target: &JetTensor) -> Result<f64, JetComputeError> {
    if pred.trace.is_some() || target.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "mse_loss has no registered autodiff rule".to_string(),
        ));
    }
    let diff = jet_compute_binary("sub", pred, target)?;
    let sq = jet_compute_mul(&diff, &diff)?;
    let n = jet_compute_numel(&sq.shape)? as f64;
    if n == 0.0 {
        return Err(JetComputeError::InvalidShape(
            "mse_loss requires a non-empty tensor".to_string(),
        ));
    }
    let sum = jet_compute_tensor_values(&sq)
        .iter()
        .try_fold(0.0, |sum, value| {
            let next = sum + *value;
            next.is_finite().then_some(next)
        })
        .ok_or_else(|| {
            JetComputeError::Arithmetic(
                "mse_loss accumulated a non-finite value".to_string(),
            )
        })?;
    let loss = sum / n;
    if !loss.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "mse_loss produced a non-finite value".to_string(),
        ));
    }
    Ok(loss)
}

fn jet_compute_sgd_step(
    param: &JetTensor,
    grad: &JetTensor,
    lr: f64,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(param)?;
    jet_compute_validate_tensor(grad)?;
    if param.shape != grad.shape {
        return Err(JetComputeError::RankMismatch(
            "sgd parameter and gradient shapes must match".to_string(),
        ));
    }
    if !lr.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "sgd learning rate must be finite".to_string(),
        ));
    }
    let scaled = jet_compute_full(&grad.shape, lr)?;
    let delta = jet_compute_mul(grad, &scaled)?;
    jet_compute_binary("sub", param, &delta)
}

fn jet_compute_serialize(tensor: &JetTensor) -> String {
    if tensor.trace.is_some() {
        jet_panic(
            "Compute.rs",
            line!(),
            "Tensor serialization has no registered autodiff rule",
        );
    }
    if let Err(error) = jet_compute_validate_tensor(tensor) {
        jet_panic(
            "core.compute.serialize",
            line!(),
            &format!("cannot serialize invalid Tensor: {}", error.jet_show()),
        );
    }
    let shape = tensor
        .shape
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let data = jet_compute_tensor_values(tensor)
        .iter()
        // Debug formatting is Rust's shortest round-tripping f64 spelling.
        // Keep it stable across the AOT/JIT/interpreter Prelude boundary.
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("shape={shape};data={data}")
}

fn jet_compute_deserialize(payload: &String) -> Result<JetTensor, JetComputeError> {
    let mut fields = payload.split(';');
    let Some(shape_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…".to_string(),
        ));
    };
    let Some(data_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…".to_string(),
        ));
    };
    if fields.next().is_some() || !data_part.starts_with("data=") {
        return Err(JetComputeError::Serialization(
            "deserialize contains duplicate or unknown fields".to_string(),
        ));
    }
    let shape_str = shape_part
        .strip_prefix("shape=")
        .ok_or_else(|| JetComputeError::Serialization("missing shape=".to_string()))?;
    if shape_str.is_empty() {
        return Err(JetComputeError::Serialization(
            "serialized Tensor shape cannot be empty".to_string(),
        ));
    }
    let shape: Vec<i64> = shape_str
        .split(',')
        .map(|p| {
            if p.is_empty() {
                return Err(JetComputeError::Serialization(
                    "serialized Tensor shape contains an empty axis".to_string(),
                ));
            }
            let axis = p.parse::<i64>().map_err(|_| {
                JetComputeError::Serialization(format!("bad shape axis `{p}`"))
            })?;
            if axis.to_string() != p {
                return Err(JetComputeError::Serialization(format!(
                    "non-canonical shape axis `{p}`"
                )));
            }
            Ok(axis)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let data_str = data_part.strip_prefix("data=").unwrap_or("");
    let data: Vec<f64> = if data_str.is_empty() {
        Vec::new()
    } else {
        data_str
            .split(',')
            .map(|p| {
                if p.is_empty() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor data contains an empty value".to_string(),
                    ));
                }
                let value = p.parse::<f64>().map_err(|_| {
                    JetComputeError::Serialization(format!("bad data value `{p}`"))
                })?;
                if !value.is_finite() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor contains a non-finite value".to_string(),
                    ));
                }
                if format!("{value:?}") != p {
                    return Err(JetComputeError::Serialization(format!(
                        "non-canonical data value `{p}`"
                    )));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let expected = jet_compute_storage_len(&shape)?;
    if expected != data.len() {
        return Err(JetComputeError::Serialization(format!(
            "deserialize storage length mismatch: shape wants {expected}, got {}",
            data.len()
        )));
    }
    let mut tensor = jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu)?;
    tensor.data = std::sync::Arc::new(data);
    Ok(tensor)
}

// ── #1137 sparse CSR + #1143 CPU SIMD tile + #1147 profile ──────────────────

#[derive(Clone, Debug, PartialEq)]
struct JetSparseCsr {
    rows: i64,
    cols: i64,
    row_ptr: Vec<i64>,
    col_idx: Vec<i64>,
    values: Vec<f64>,
}

impl JetShow for JetSparseCsr {
    fn jet_show(&self) -> String {
        format!(
            "SparseCsr({}x{}, nnz={})",
            self.rows,
            self.cols,
            self.values.len()
        )
    }
}

fn jet_compute_to_sparse(tensor: &JetTensor) -> Result<JetSparseCsr, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "to_sparse has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "to_sparse requires a rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    let rows = tensor.shape[0];
    let cols = tensor.shape[1];
    let mut row_ptr = vec![0i64];
    let mut col_idx = Vec::new();
    let mut values = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let v = jet_compute_get_raw(tensor, &vec![r, c])?;
            if v != 0.0 {
                col_idx.push(c);
                values.push(v);
            }
        }
        row_ptr.push(i64::try_from(values.len()).map_err(|_| {
            JetComputeError::InvalidShape("sparse nnz is too large".to_string())
        })?);
    }
    Ok(JetSparseCsr {
        rows,
        cols,
        row_ptr,
        col_idx,
        values,
    })
}

fn jet_compute_sparse_nnz(sparse: &JetSparseCsr) -> i64 {
    i64::try_from(sparse.values.len()).unwrap_or(i64::MAX)
}

fn jet_compute_sparse_mv(
    sparse: &JetSparseCsr,
    vector: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    if vector.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "sparse_mv has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_sparse(sparse)?;
    jet_compute_validate_tensor(vector)?;
    if vector.shape.len() != 1 || vector.shape[0] != sparse.cols {
        return Err(JetComputeError::RankMismatch(format!(
            "sparse_mv expects a length-{} vector",
            sparse.cols
        )));
    }
    let mut out = jet_compute_zeros(&vec![sparse.rows])?;
    for r in 0..sparse.rows {
        let row = usize::try_from(r).map_err(|_| {
            JetComputeError::InvalidShape("sparse row index is too large".to_string())
        })?;
        let start = usize::try_from(sparse.row_ptr[row]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let end = usize::try_from(sparse.row_ptr[row + 1]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let mut acc = 0.0;
        for k in start..end {
            let c = sparse.col_idx[k];
            acc += sparse.values[k] * jet_compute_get_raw(vector, &[c])?;
            if !acc.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sparse matrix-vector multiplication produced a non-finite value"
                        .to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &vec![r], acc)?;
    }
    Ok(out)
}

fn jet_compute_validate_sparse(sparse: &JetSparseCsr) -> Result<(), JetComputeError> {
    if sparse.rows < 0 || sparse.cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "sparse dimensions must be non-negative".to_string(),
        ));
    }
    let rows = usize::try_from(sparse.rows).map_err(|_| {
        JetComputeError::InvalidShape("sparse row count is too large".to_string())
    })?;
    if sparse.row_ptr.len() != rows.saturating_add(1)
        || sparse.col_idx.len() != sparse.values.len()
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR arrays have inconsistent lengths".to_string(),
        ));
    }
    let nnz = i64::try_from(sparse.values.len()).map_err(|_| {
        JetComputeError::InvalidShape("sparse nnz is too large".to_string())
    })?;
    if sparse.row_ptr.first().copied() != Some(0)
        || sparse.row_ptr.last().copied() != Some(nnz)
        || sparse
            .row_ptr
            .windows(2)
            .any(|pair| pair[0] < 0 || pair[1] < pair[0] || pair[1] > nnz)
        || sparse
            .col_idx
            .iter()
            .any(|col| *col < 0 || *col >= sparse.cols)
        || sparse.values.iter().any(|value| !value.is_finite())
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR invariants are invalid".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_sparse_show(sparse: &JetSparseCsr) -> String {
    sparse.jet_show()
}

/// Named CPU-SIMD profile path; math matches scalar matmul (D-COMPUTE-BACKEND1).
/// CPU-SIMD profile path (#1143): blocked matmul in f32 arithmetic with a fixed
/// tile size. Same numeric contract as `matmul` for modest shapes; distinct
/// algorithm (tiled accumulation, f32 cast) so the SIMD profile is not a
/// facade over the f64 triple loop.
fn jet_compute_matmul_f32_tile(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.trace.is_some() || b.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "matmul_f32_tile has no registered autodiff rule".to_string(),
        ));
    }
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul_f32_tile dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul_f32_tile inner dims {} and {} disagree",
            k, k2
        )));
    }
    const TILE: i64 = 8;
    let mut out = jet_compute_tensor_from_shape(vec![m, n], 0.0, JetComputeDevice::Cpu)?;
    let mut i0 = 0i64;
    while i0 < m {
        let i1 = (i0 + TILE).min(m);
        let mut j0 = 0i64;
        while j0 < n {
            let j1 = (j0 + TILE).min(n);
            let mut t0 = 0i64;
            while t0 < k {
                let t1 = (t0 + TILE).min(k);
                for i in i0..i1 {
                    for j in j0..j1 {
                        let mut acc = jet_compute_get_raw(&out, &vec![i, j])? as f32;
                        if !acc.is_finite() {
                            return Err(JetComputeError::Arithmetic(
                                "f32 tile accumulator is non-finite".to_string(),
                            ));
                        }
                        for t in t0..t1 {
                            let av = jet_compute_get_raw(a, &vec![i, t])? as f32;
                            let bv = jet_compute_get_raw(b, &vec![t, j])? as f32;
                            if !av.is_finite() || !bv.is_finite() {
                                return Err(JetComputeError::Arithmetic(
                                    "f32 tile input is outside the finite f32 range"
                                        .to_string(),
                                ));
                            }
                            acc += av * bv;
                            if !acc.is_finite() {
                                return Err(JetComputeError::Arithmetic(
                                    "f32 tile accumulation overflowed".to_string(),
                                ));
                            }
                        }
                        jet_compute_set(&mut out, &vec![i, j], acc as f64)?;
                    }
                }
                t0 = t1;
            }
            j0 = j1;
        }
        i0 = i1;
    }
    out.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
    out.last_placement.capabilities = CPU_ORACLE_F32_CAPABILITIES
        .iter()
        .map(|capability| (*capability).to_string())
        .collect();
    out.last_placement.reason = format!(
        "algorithm=blocked-matmul; tile={TILE}; arithmetic=f32; reduction=ordered"
    );
    Ok(out)
}

fn jet_compute_profile_f32_strict() -> String {
    format!(
        "backend={};version={};profile={};algorithm=blocked-matmul;tile=8;cache={}",
        CPU_ORACLE_BACKEND,
        CPU_ORACLE_VERSION,
        CPU_ORACLE_F32_PROFILE,
        CPU_ORACLE_CACHE,
    )
}

fn jet_compute_profile_show() -> String {
    jet_compute_profile_f32_strict()
}
