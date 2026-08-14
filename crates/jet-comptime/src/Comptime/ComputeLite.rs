//! D-COMPUTE1=D / I9: `core.compute` ambient mirrors AOT Prelude `jet_compute_*`
//! by including the same compute and view-access sources
//! (`Prelude/CoreLib/Top/Compute.rs`, `Prelude/Core/ViewAccess.rs`). Marshalling
//! to `CtValue` lives here; engines must not re-encode tensor law.
//! parity: include path=crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs

use crate::AST::{
    ClosureData, CtFloat, CtOpaque, CtReport, CtValue, Lambda, LambdaBody, LambdaMeta, Type,
};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

trait JetShow {
    fn jet_show(&self) -> String;
}

trait JetDisplay {
    fn jet_display(&self) -> String;
}

// The shared compute core uses the same range law as the rest of the
// evaluator.  Comptime has its own value boundary, so it supplies only the
// small range carrier and panic adapter needed to include that core source.
mod compute_range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
}
use compute_range_semantics::jet_range_bounds;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetRange {
    start: i64,
    end: i64,
    exclusive: bool,
}

fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    jet_foundation::ice!(None, "{} (at {}:{})", msg, file, line)
}

#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/Core/ViewAccess.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Compute.rs");

fn device_to_ct(device: JetComputeDevice) -> CtValue {
    CtValue::Enum {
        type_name: "ComputeDevice".to_string(),
        variant: match device {
            JetComputeDevice::Auto => "Auto".to_string(),
            JetComputeDevice::Cpu => "CPU".to_string(),
        },
        args: Vec::new(),
    }
}

fn ct_to_device(value: &CtValue, span: Span) -> Result<JetComputeDevice, Diagnostic> {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            ..
        } if type_name == "ComputeDevice" => match variant.as_str() {
            "Auto" => Ok(JetComputeDevice::Auto),
            "CPU" | "Cpu" => Ok(JetComputeDevice::Cpu),
            _ => Err(unsupported("ComputeDevice variant", span)),
        },
        CtValue::Str(s) if s == "Auto" => Ok(JetComputeDevice::Auto),
        CtValue::Str(s) if s == "CPU" || s == "Cpu" => Ok(JetComputeDevice::Cpu),
        _ => Err(unsupported("ComputeDevice", span)),
    }
}

fn receipt_to_ct(receipt: &JetComputePlacementReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "ComputePlacement".to_string(),
        fields: vec![
            ("requested".to_string(), device_to_ct(receipt.requested)),
            ("selected".to_string(), device_to_ct(receipt.selected)),
            ("backend".to_string(), CtValue::Str(receipt.backend.clone())),
            ("version".to_string(), CtValue::Str(receipt.version.clone())),
            ("profile".to_string(), CtValue::Str(receipt.profile.clone())),
            ("cache".to_string(), CtValue::Str(receipt.cache.clone())),
            (
                "capabilities".to_string(),
                CtValue::List(
                    receipt
                        .capabilities
                        .iter()
                        .cloned()
                        .map(CtValue::Str)
                        .collect(),
                ),
            ),
            ("reason".to_string(), CtValue::Str(receipt.reason.clone())),
        ],
    }
}

fn ct_to_receipt(value: &CtValue, span: Span) -> Result<JetComputePlacementReceipt, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ComputePlacement", span));
    };
    if type_name != "ComputePlacement" && type_name != "JetComputePlacementReceipt" {
        return Err(unsupported("ComputePlacement", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("ComputePlacement field", span))
    };
    let requested = ct_to_device(field("requested")?, span)?;
    let selected = ct_to_device(field("selected")?, span)?;
    let text = |name: &str| match field(name)? {
        CtValue::Str(s) if !s.is_empty() && !s.chars().any(char::is_control) => Ok(s.clone()),
        _ => Err(unsupported("ComputePlacement text field", span)),
    };
    let capabilities = match field("capabilities")? {
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Str(s) if !s.is_empty() && !s.chars().any(char::is_control) => {
                    Ok(s.clone())
                }
                _ => Err(unsupported("ComputePlacement capability", span)),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("ComputePlacement capabilities", span)),
    };
    let reason = match field("reason")? {
        CtValue::Str(s) if !s.is_empty() && !s.chars().any(char::is_control) => s.clone(),
        _ => return Err(unsupported("placement reason", span)),
    };
    let receipt = JetComputePlacementReceipt {
        requested,
        selected,
        backend: text("backend")?,
        version: text("version")?,
        profile: text("profile")?,
        cache: text("cache")?,
        capabilities,
        reason,
    };
    jet_compute_validate_placement(receipt.selected, &receipt)
        .map_err(|error| unsupported(&format!("ComputePlacement metadata: {}", error.jet_show()), span))?;
    Ok(receipt)
}

fn transfer_to_ct(transfer: &JetComputeTransferReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "ComputeTransfer".to_string(),
        fields: vec![
            ("from".to_string(), device_to_ct(transfer.from)),
            ("to".to_string(), device_to_ct(transfer.to)),
            ("bytes".to_string(), CtValue::Int(transfer.bytes)),
            ("fallback".to_string(), CtValue::Str(transfer.fallback.clone())),
        ],
    }
}

fn ct_to_transfer(value: &CtValue, span: Span) -> Result<JetComputeTransferReceipt, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ComputeTransfer", span));
    };
    if type_name != "ComputeTransfer" && type_name != "JetComputeTransferReceipt" {
        return Err(unsupported("ComputeTransfer", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("ComputeTransfer field", span))
    };
    let from = ct_to_device(field("from")?, span)?;
    let to = ct_to_device(field("to")?, span)?;
    let bytes = match field("bytes")? {
        CtValue::Int(n) if *n >= 0 => *n,
        _ => return Err(unsupported("transfer bytes", span)),
    };
    let fallback = match field("fallback")? {
        CtValue::Str(s) if !s.is_empty() && !s.chars().any(char::is_control) => s.clone(),
        _ => return Err(unsupported("transfer fallback", span)),
    };
    Ok(JetComputeTransferReceipt {
        from,
        to,
        bytes,
        fallback,
    })
}

fn tape_rule_to_ct(rule: Option<&JetComputeTapeRule>) -> CtValue {
    let Some(rule) = rule else {
        return CtValue::Str("input".to_string());
    };
    let (kind, source_shape, axis) = match rule {
        JetComputeTapeRule::Add => ("add", None, None),
        JetComputeTapeRule::Sub => ("sub", None, None),
        JetComputeTapeRule::Mul => ("mul", None, None),
        JetComputeTapeRule::Div => ("div", None, None),
        JetComputeTapeRule::Maximum => ("maximum", None, None),
        JetComputeTapeRule::Minimum => ("minimum", None, None),
        JetComputeTapeRule::Matmul => ("matmul", None, None),
        JetComputeTapeRule::Unary(op) => (op.as_str(), None, None),
        JetComputeTapeRule::Reshape { source_shape } => ("reshape", Some(source_shape), None),
        JetComputeTapeRule::Broadcast { source_shape } => ("broadcast", Some(source_shape), None),
        JetComputeTapeRule::ReduceToShape { source_shape } => {
            ("reduce_to_shape", Some(source_shape), None)
        }
        JetComputeTapeRule::Transpose => ("transpose", None, None),
        JetComputeTapeRule::SumAxis { axis, source_shape } => {
            ("sum_axis", Some(source_shape), Some(*axis as i64))
        }
    };
    let mut fields = vec![("kind".to_string(), CtValue::Str(kind.to_string()))];
    if let Some(shape) = source_shape {
        fields.push((
            "source_shape".to_string(),
            CtValue::List(shape.iter().map(|value| CtValue::Int(*value)).collect()),
        ));
    }
    if let Some(axis) = axis {
        fields.push(("axis".to_string(), CtValue::Int(axis)));
    }
    CtValue::Struct {
        type_name: "__JetComputeRule".to_string(),
        fields,
    }
}

#[derive(Clone)]
struct JetComputeTapeHandle {
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
}

const TENSOR_HANDLE_FIELD: &str = "__jet_tensor_handle";

#[derive(Clone)]
struct JetComputeTensorHandle {
    tensor: std::sync::Arc<std::sync::Mutex<JetTensor>>,
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone)]
struct JetComputeWindowHandle {
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tensor_valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

fn tape_handle_to_ct(tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>) -> CtValue {
    CtValue::Closure(std::sync::Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(JetComputeTapeHandle { tape })),
    }))
}

fn tensor_handle_to_ct(
    tensor: std::sync::Arc<std::sync::Mutex<JetTensor>>,
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> CtValue {
    CtValue::Closure(std::sync::Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(JetComputeTensorHandle { tensor, valid })),
    }))
}

fn opaque_closure<T: std::any::Any + Send + Sync>(opaque: T) -> CtValue {
    CtValue::Closure(std::sync::Arc::new(ClosureData {
        lambda: Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            body: LambdaBody::Block(Vec::new()),
            span: Span::new(0, 0),
            meta: LambdaMeta::default(),
        },
        captured: std::collections::HashMap::new(),
        return_type: None,
        opaque: Some(CtOpaque::new(opaque)),
    }))
}

const TENSOR_WINDOW_HANDLE_FIELD: &str = "__jet_tensor_window_handle";

pub fn tensor_window_handle(value: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    let tensor = tensor_handle_state(value, span)?;
    Ok(opaque_closure(JetComputeWindowHandle {
        valid: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        tensor_valid: tensor.valid,
    }))
}

pub fn tensor_window_is_live(fields: &[(String, CtValue)]) -> bool {
    let Some(value) = fields
        .iter()
        .find(|(name, _)| name == TENSOR_WINDOW_HANDLE_FIELD)
        .map(|(_, value)| value)
    else {
        return true;
    };
    let CtValue::Closure(data) = value else {
        return false;
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<JetComputeWindowHandle>())
        .is_some_and(|handle| {
            handle.valid.load(std::sync::atomic::Ordering::Acquire)
                && handle
                    .tensor_valid
                    .load(std::sync::atomic::Ordering::Acquire)
        })
}

pub fn tensor_window_drop_value(value: &CtValue, span: Span) -> Result<(), Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("Tensor window", span));
    };
    let Some(value) = fields
        .iter()
        .find(|(name, _)| name == TENSOR_WINDOW_HANDLE_FIELD)
        .map(|(_, value)| value)
    else {
        return Ok(());
    };
    let CtValue::Closure(data) = value else {
        return Err(unsupported("Tensor window handle", span));
    };
    let handle = data
        .opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<JetComputeWindowHandle>())
        .ok_or_else(|| unsupported("Tensor window handle", span))?;
    handle
        .valid
        .store(false, std::sync::atomic::Ordering::Release);
    Ok(())
}

pub fn tensor_window_values_share_handle(left: &CtValue, right: &CtValue) -> bool {
    let handle = |value: &CtValue| {
        let CtValue::Struct { fields, .. } = value else {
            return None;
        };
        let CtValue::Closure(data) = fields
            .iter()
            .find(|(name, _)| name == TENSOR_WINDOW_HANDLE_FIELD)
            .map(|(_, value)| value)?
        else {
            return None;
        };
        data.opaque
            .as_ref()
            .and_then(|opaque| opaque.downcast_ref::<JetComputeWindowHandle>())
            .map(|handle| std::sync::Arc::clone(&handle.valid))
    };
    handle(left).zip(handle(right)).is_some_and(|(left, right)| {
        std::sync::Arc::ptr_eq(&left, &right)
    })
}

fn tensor_handle_from_fields(
    fields: &[(String, CtValue)],
    span: Span,
) -> Result<Option<JetComputeTensorHandle>, Diagnostic> {
    let Some(value) = fields
        .iter()
        .find(|(name, _)| name == TENSOR_HANDLE_FIELD)
        .map(|(_, value)| value)
    else {
        return Ok(None);
    };
    let CtValue::Closure(data) = value else {
        return Err(unsupported("Tensor handle", span));
    };
    data.opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<JetComputeTensorHandle>())
        .filter(|handle| {
            handle
                .valid
                .load(std::sync::atomic::Ordering::Acquire)
        })
        .cloned()
        .map(Some)
        .ok_or_else(|| unsupported("Tensor handle", span))
}

fn tensor_handle_state(
    value: &CtValue,
    span: Span,
) -> Result<JetComputeTensorHandle, Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("Tensor", span));
    };
    tensor_handle_from_fields(fields, span)?.ok_or_else(|| unsupported("Tensor handle", span))
}

fn trace_to_ct(trace: &JetComputeTrace) -> CtValue {
    let tape_handle = trace.tape.upgrade().unwrap_or_else(|| {
        jet_panic(
            "ComputeLite::trace_to_ct",
            line!(),
            "autodiff tape ended before its trace was marshalled",
        )
    });
    let tape = tape_handle
        .lock()
        .unwrap_or_else(|_| jet_panic("ComputeLite::trace_to_ct", line!(), "autodiff tape is poisoned"));
    let nodes = tape
        .nodes
        .iter()
        .map(|node| CtValue::Struct {
            type_name: "__JetComputeNode".to_string(),
            fields: vec![
                (
                    "parents".to_string(),
                    CtValue::List(
                        node.parents
                            .iter()
                            .map(|parent| CtValue::Int(parent.map_or(-1, |value| value as i64)))
                            .collect(),
                    ),
                ),
                ("rule".to_string(), tape_rule_to_ct(node.rule.as_ref())),
                (
                    "values".to_string(),
                    CtValue::List(
                        node.values
                            .iter()
                            .map(|value| tensor_to_ct_inner(value, false))
                            .collect(),
                    ),
                ),
                ("output".to_string(), tensor_to_ct_inner(&node.output, false)),
            ],
        })
        .collect();
    let inputs = CtValue::List(
        tape.inputs
            .iter()
            .map(|value| tensor_to_ct_inner(value, false))
            .collect(),
    );
    drop(tape);
    let mut fields = vec![
        ("identity".to_string(), tape_handle_to_ct(tape_handle)),
        ("node".to_string(), CtValue::Int(trace.node as i64)),
        ("inputs".to_string(), inputs),
        ("nodes".to_string(), CtValue::List(nodes)),
    ];
    if let Some(parent) = &trace.parent {
        fields.push((
            "parent".to_string(),
            CtValue::Present(Box::new(trace_to_ct(parent))),
        ));
    }
    CtValue::Struct {
        type_name: "__JetComputeTrace".to_string(),
        fields,
    }
}

fn tensor_to_ct(tensor: &JetTensor) -> CtValue {
    let handle = std::sync::Arc::new(std::sync::Mutex::new(tensor.clone()));
    let valid = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    tensor_to_ct_with_handle(tensor, handle, valid, true)
}

fn tensor_to_ct_inner(tensor: &JetTensor, include_trace: bool) -> CtValue {
    let handle = std::sync::Arc::new(std::sync::Mutex::new(tensor.clone()));
    let valid = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    tensor_to_ct_with_handle(tensor, handle, valid, include_trace)
}

fn tensor_to_ct_with_handle(
    tensor: &JetTensor,
    handle: std::sync::Arc<std::sync::Mutex<JetTensor>>,
    valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    include_trace: bool,
) -> CtValue {
    // CtValue owns lists; Prelude remains authority for validation and logical
    // element selection at this engine boundary.
    if let Err(error) = jet_compute_validate_tensor(tensor) {
        jet_panic(
            "ComputeLite::tensor_to_ct",
            line!(),
            &format!("invalid Tensor result: {}", error.jet_show()),
        );
    }
    // CtValue owns lists, not borrowed strided allocations. Marshal the
    // logical projection as a fresh contiguous value at this engine boundary.
    let strides = match jet_compute_row_major_strides(&tensor.shape) {
        Ok(strides) => strides,
        Err(error) => jet_panic(
            "ComputeLite::tensor_to_ct",
            line!(),
            &format!("invalid Tensor view metadata: {}", error.jet_show()),
        ),
    };
    let data = jet_compute_tensor_values(tensor);
    let mut fields = vec![
        (
            "shape".to_string(),
            CtValue::List(tensor.shape.iter().map(|d| CtValue::Int(*d)).collect()),
        ),
        (
            "strides".to_string(),
            CtValue::List(strides.iter().map(|d| CtValue::Int(*d)).collect()),
        ),
        (
            "data".to_string(),
            CtValue::List(data.iter().map(|v| CtValue::Float(CtFloat::f64(*v))).collect()),
        ),
        ("device".to_string(), device_to_ct(tensor.device)),
        (
            "last_placement".to_string(),
            receipt_to_ct(&tensor.last_placement),
        ),
        (
            "last_transfer".to_string(),
            tensor
                .last_transfer
                .as_ref()
                .map(transfer_to_ct)
                .map(|value| CtValue::Present(Box::new(value)))
                .unwrap_or_else(|| {
                    CtValue::absent(Type::Named("ComputeTransfer".to_string()))
                }),
        ),
    ];
    if include_trace {
        if let Some(trace) = &tensor.trace {
            fields.push((
                "autodiff".to_string(),
                CtValue::Present(Box::new(trace_to_ct(trace))),
            ));
        }
    }
    fields.push((
        TENSOR_HANDLE_FIELD.to_string(),
        tensor_handle_to_ct(handle, valid),
    ));
    CtValue::Struct {
        type_name: "Tensor".to_string(),
        fields,
    }
}

fn as_i64_list(value: &CtValue, span: Span) -> Result<Vec<i64>, Diagnostic> {
    match value {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Int(n) => Ok(*n),
                _ => Err(unsupported("Int list element", span)),
            })
            .collect(),
        _ => Err(unsupported("[Int]", span)),
    }
}

fn as_f64_list(value: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match value {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Float(f) => Ok(f.as_f64()),
                CtValue::Int(n) => Ok(*n as f64),
                _ => Err(unsupported("Float list element", span)),
            })
            .collect(),
        _ => Err(unsupported("[Float]", span)),
    }
}

fn tape_rule_from_ct(value: &CtValue, span: Span) -> Result<Option<JetComputeTapeRule>, Diagnostic> {
    let kind = match value {
        CtValue::Str(kind) => kind.as_str(),
        CtValue::Struct { type_name, fields } if type_name == "__JetComputeRule" => fields
            .iter()
            .find_map(|(name, value)| (name == "kind").then_some(value))
            .and_then(|value| match value {
                CtValue::Str(kind) => Some(kind.as_str()),
                _ => None,
            })
            .ok_or_else(|| unsupported("autodiff tape rule", span))?,
        _ => return Err(unsupported("autodiff tape rule", span)),
    };
    if kind == "input" {
        return Ok(None);
    }
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("autodiff tape rule", span));
    };
    let source_shape = || {
        fields
            .iter()
            .find_map(|(name, value)| (name == "source_shape").then_some(value))
            .ok_or_else(|| unsupported("autodiff source shape", span))
            .and_then(|value| as_i64_list(value, span))
    };
    let axis = || {
        fields
            .iter()
            .find_map(|(name, value)| (name == "axis").then_some(value))
            .and_then(|value| match value {
                CtValue::Int(axis) => Some(*axis),
                _ => None,
            })
            .ok_or_else(|| unsupported("autodiff reduction axis", span))
    };
    Ok(Some(match kind {
        "add" => JetComputeTapeRule::Add,
        "sub" => JetComputeTapeRule::Sub,
        "mul" => JetComputeTapeRule::Mul,
        "div" => JetComputeTapeRule::Div,
        "maximum" => JetComputeTapeRule::Maximum,
        "minimum" => JetComputeTapeRule::Minimum,
        "matmul" => JetComputeTapeRule::Matmul,
        "negate" | "abs" | "exp" | "log" | "sqrt" => {
            JetComputeTapeRule::Unary(kind.to_string())
        }
        "reshape" => JetComputeTapeRule::Reshape {
            source_shape: source_shape()?,
        },
        "broadcast" => JetComputeTapeRule::Broadcast {
            source_shape: source_shape()?,
        },
        "reduce_to_shape" => JetComputeTapeRule::ReduceToShape {
            source_shape: source_shape()?,
        },
        "transpose" => JetComputeTapeRule::Transpose,
        "sum_axis" => JetComputeTapeRule::SumAxis {
            axis: usize::try_from(axis()?)
                .map_err(|_| unsupported("autodiff reduction axis", span))?,
            source_shape: source_shape()?,
        },
        _ => return Err(unsupported("autodiff tape operation", span)),
    }))
}

fn trace_from_ct(value: &CtValue, span: Span) -> Result<JetComputeTrace, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("autodiff trace", span));
    };
    if type_name != "__JetComputeTrace" {
        return Err(unsupported("autodiff trace", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
            .ok_or_else(|| unsupported("autodiff trace field", span))
    };
    let tape_handle = match field("identity")? {
        CtValue::Closure(data) => data
            .opaque
            .as_ref()
            .and_then(|opaque| opaque.downcast_ref::<JetComputeTapeHandle>())
            .map(|handle| handle.tape.clone())
            .ok_or_else(|| unsupported("autodiff trace identity", span))?,
        _ => return Err(unsupported("autodiff trace identity", span)),
    };
    let node = match field("node")? {
        CtValue::Int(node) if *node >= 0 => usize::try_from(*node).map_err(|_| unsupported("autodiff trace node", span))?,
        _ => return Err(unsupported("autodiff trace node", span)),
    };
    let _inputs = match field("inputs")? {
        CtValue::List(values) => values
            .iter()
            .map(|value| ct_to_tensor_inner(value, span, false))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("autodiff trace inputs", span)),
    };
    let nodes = match field("nodes")? {
        CtValue::List(values) => values
            .iter()
            .map(|value| {
                let CtValue::Struct { type_name, fields } = value else {
                    return Err(unsupported("autodiff tape node", span));
                };
                if type_name != "__JetComputeNode" {
                    return Err(unsupported("autodiff tape node", span));
                }
                let node_field = |name: &str| {
                    fields
                        .iter()
                        .find(|(field, _)| field == name)
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported("autodiff tape node field", span))
                };
                let parents = match node_field("parents")? {
                    CtValue::List(values) => values
                        .iter()
                        .map(|value| match value {
                            CtValue::Int(value) if *value < 0 => Ok(None),
                            CtValue::Int(value) => usize::try_from(*value)
                                .map(Some)
                                .map_err(|_| unsupported("autodiff parent", span)),
                            _ => Err(unsupported("autodiff parent", span)),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(unsupported("autodiff parents", span)),
                };
                let rule = tape_rule_from_ct(node_field("rule")?, span)?;
                let values = match node_field("values")? {
                    CtValue::List(values) => values
                        .iter()
                        .map(|value| ct_to_tensor_inner(value, span, false))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(unsupported("autodiff node values", span)),
                };
                let output = ct_to_tensor_inner(node_field("output")?, span, false)?;
                Ok(JetComputeTapeNode {
                    parents,
                    rule,
                    values,
                    output,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        _ => return Err(unsupported("autodiff trace nodes", span)),
    };
    if node >= nodes.len() {
        return Err(unsupported("autodiff trace node", span));
    }
    let parent = match fields.iter().find(|(name, _)| name == "parent").map(|(_, value)| value) {
        Some(CtValue::Present(value)) => Some(Box::new(trace_from_ct(value, span)?)),
        Some(CtValue::Failed(CtReport::Clean(_))) | None => None,
        Some(_) => return Err(unsupported("autodiff trace parent", span)),
    };
    Ok(JetComputeTrace {
        tape: std::sync::Arc::downgrade(&tape_handle),
        node,
        parent,
    })
}

fn ct_to_tensor(value: &CtValue, span: Span) -> Result<JetTensor, Diagnostic> {
    ct_to_tensor_inner(value, span, true)
}

fn ct_to_tensor_inner(
    value: &CtValue,
    span: Span,
    include_trace: bool,
) -> Result<JetTensor, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Tensor", span));
    };
    if type_name != "Tensor" && type_name != "JetTensor" {
        return Err(unsupported("Tensor", span));
    }
    let handle = tensor_handle_from_fields(fields, span)?
        .ok_or_else(|| unsupported("Tensor handle", span))?;
    let mut tensor = handle
        .tensor
        .lock()
        .map_err(|_| unsupported("Tensor handle", span))?
        .clone();
    if !include_trace {
        tensor.trace = None;
    }
    jet_compute_validate_tensor(&tensor)
        .map_err(|error| unsupported(&format!("Tensor metadata: {}", error.jet_show()), span))?;
    if let Some(receipt) = &tensor.last_transfer {
        jet_compute_validate_transfer_receipt(&tensor, receipt)
            .map_err(|error| unsupported(&format!("Tensor transfer metadata: {}", error.jet_show()), span))?;
    }
    Ok(tensor)
}

fn tensor_window_args(
    args: &[CtValue],
    span: Span,
) -> Result<(i64, i64, bool), Diagnostic> {
    match args {
        [CtValue::Struct { type_name, fields }] if type_name == crate::Syntax::TYPE_RANGE => {
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let start = match field("start") {
                Some(CtValue::Int(value)) => *value,
                _ => return Err(unsupported("Range.start", span)),
            };
            let end = match field("end") {
                Some(CtValue::Int(value)) => *value,
                _ => return Err(unsupported("Range.end", span)),
            };
            Ok((start, end, matches!(field("exclusive"), Some(CtValue::Bool(true)))))
        }
        [CtValue::Int(start), CtValue::Int(end)] => Ok((*start, *end, false)),
        _ => Err(unsupported("Tensor view range", span)),
    }
}

/// Ambient marshalling for a read-only Tensor view.  The CtValue boundary has
/// no borrowed slice carrier, so the view is materialized only at this engine
/// boundary; bounds and first-axis slab selection still come from Prelude.
pub fn tensor_view_window(
    value: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<(usize, usize), Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    jet_compute_view_checked(&tensor, start, end, exclusive)
        .map(|view| (0, view.len()))
        .map_err(|error| unsupported(&format!("Tensor view: {}", error.jet_show()), span))
}

/// Ambient marshalling for a mutable Tensor view. Construction uses the same
/// trace, bounds, and exclusivity checker as AOT and resident JIT; the returned
/// range is only a CtValue carrier fact.
pub fn tensor_view_mut_window(
    value: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<(usize, usize), Diagnostic> {
    let handle = tensor_handle_state(value, span)?;
    let mut tensor = handle
        .tensor
        .lock()
        .map_err(|_| unsupported("Tensor handle", span))?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    let view = jet_compute_view_mut_checked(&mut tensor, start, end, exclusive)
        .map_err(|error| unsupported(&format!("Tensor mutable view: {}", error.jet_show()), span))?;
    let len = usize::try_from(view.len())
        .map_err(|_| unsupported("Tensor mutable view length", span))?;
    Ok((0, len))
}

/// Mutable/read-only Tensor-window indexing is an adapter concern at the
/// CtValue boundary. Window addressing and element access come from the
/// shared Prelude helpers used by AOT and the resident JIT.
pub fn tensor_view_get_value(
    value: &CtValue,
    args: &[CtValue],
    index: i64,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    jet_compute_window_get(&tensor, start, end, exclusive, index)
        .map(|value| CtValue::Float(CtFloat::f64(value)))
        .map_err(|error| unsupported(&format!("Tensor view: {error}"), span))
}

pub fn tensor_view_set_value(
    value: &CtValue,
    args: &[CtValue],
    index: i64,
    replacement: &CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let handle = tensor_handle_state(value, span)?;
    let mut tensor = handle
        .tensor
        .lock()
        .map_err(|_| unsupported("Tensor handle", span))?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    let replacement = as_float(replacement, span)?;
    jet_compute_window_set(
        &mut tensor,
        start,
        end,
        exclusive,
        index,
        replacement,
    )
    .map_err(|error| unsupported(&format!("Tensor view: {error}"), span))?;
    let snapshot = (*tensor).clone();
    drop(tensor);
    Ok(tensor_to_ct_with_handle(
        &snapshot,
        std::sync::Arc::clone(&handle.tensor),
        handle.valid,
        true,
    ))
}

pub fn tensor_view_list(
    value: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    let view = jet_compute_view_checked(&tensor, start, end, exclusive)
        .map_err(|error| unsupported(&format!("Tensor view: {}", error.jet_show()), span))?;
    Ok(CtValue::List(
        view
            .iter()
            .map(|value| CtValue::Float(CtFloat::f64(*value)))
            .collect(),
    ))
}

pub fn tensor_to_list_value(value: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    Ok(CtValue::List(
        jet_compute_tensor_to_list(&tensor)
            .into_iter()
            .map(|value| CtValue::Float(CtFloat::f64(value)))
            .collect(),
    ))
}

pub fn tensor_slice_value(
    value: &CtValue,
    start: i64,
    end: i64,
    exclusive: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    jet_compute_slice_checked(&tensor, start, end, exclusive)
        .map(|slice| tensor_to_ct(&slice))
        .map_err(|error| unsupported(&format!("Tensor slice: {}", error.jet_show()), span))
}

pub fn tensor_copy_value(value: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    jet_compute_copy_checked(&tensor)
        .map(|copy| tensor_to_ct(&copy))
        .map_err(|error| unsupported(&format!("Tensor copy: {}", error.jet_show()), span))
}

/// The TIR clone node is the implicit Tensor sharing operation. Keep its
/// storage Arc shared while giving the clone its own canonical handle, exactly
/// as a Prelude `JetTensor::clone` does at the AOT/JIT boundary.
pub fn tensor_clone_value(value: &CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    let cloned = tensor.clone();
    Ok(tensor_to_ct(&cloned))
}

pub fn tensor_drop_value(value: &CtValue, span: Span) -> Result<(), Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("Tensor", span));
    };
    let value = fields
        .iter()
        .find(|(name, _)| name == TENSOR_HANDLE_FIELD)
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported("Tensor handle", span))?;
    let CtValue::Closure(data) = value else {
        return Err(unsupported("Tensor handle", span));
    };
    let handle = data
        .opaque
        .as_ref()
        .and_then(|opaque| opaque.downcast_ref::<JetComputeTensorHandle>())
        .ok_or_else(|| unsupported("Tensor handle", span))?;
    handle
        .valid
        .store(false, std::sync::atomic::Ordering::Release);
    Ok(())
}

pub fn tensor_values_share_handle(left: &CtValue, right: &CtValue) -> bool {
    let handle = |value: &CtValue| {
        let CtValue::Struct { fields, .. } = value else {
            return None;
        };
        let CtValue::Closure(data) = fields
            .iter()
            .find(|(name, _)| name == TENSOR_HANDLE_FIELD)
            .map(|(_, value)| value)?
        else {
            return None;
        };
        data.opaque
            .as_ref()
            .and_then(|opaque| opaque.downcast_ref::<JetComputeTensorHandle>())
            .map(|handle| std::sync::Arc::clone(&handle.valid))
    };
    handle(left).zip(handle(right)).is_some_and(|(left, right)| {
        std::sync::Arc::ptr_eq(&left, &right)
    })
}

pub fn tensor_replace_data(
    value: &CtValue,
    items: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let handle = tensor_handle_state(value, span)?;
    let mut tensor = handle
        .tensor
        .lock()
        .map_err(|_| unsupported("Tensor handle", span))?;
    let values = as_f64_list(&CtValue::List(items), span)?;
    if values.len() != jet_compute_tensor_values(&tensor).len() {
        return Err(unsupported("Tensor write-back length", span));
    }
    jet_compute_replace_data_checked(&mut tensor, values)
        .map_err(|error| unsupported(&format!("Tensor write-back: {}", error.jet_show()), span))?;
    let snapshot = (*tensor).clone();
    drop(tensor);
    Ok(tensor_to_ct_with_handle(
        &snapshot,
        std::sync::Arc::clone(&handle.tensor),
        handle.valid,
        true,
    ))
}

fn map_err(err: JetComputeError) -> CtValue {
    let (variant, message) = match err {
        JetComputeError::InvalidShape(m) => ("InvalidShape", m),
        JetComputeError::RankMismatch(m) => ("RankMismatch", m),
        JetComputeError::OutOfBounds(m) => ("OutOfBounds", m),
        JetComputeError::Device(m) => ("Device", m),
        JetComputeError::Unsupported(m) => ("Unsupported", m),
        JetComputeError::Arithmetic(m) => ("Arithmetic", m),
        JetComputeError::Serialization(m) => ("Serialization", m),
    };
    CtValue::Enum {
        type_name: "ComputeError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
    }
}

fn stream_to_ct(s: &JetComputeStream) -> CtValue {
    CtValue::Struct {
        type_name: "ComputeStream".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Int(s.id)),
            ("device".to_string(), device_to_ct(s.device)),
        ],
    }
}

fn ct_to_stream(value: &CtValue, span: Span) -> Result<JetComputeStream, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("ComputeStream", span));
    };
    if type_name != "ComputeStream" && type_name != "JetComputeStream" {
        return Err(unsupported("ComputeStream", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("ComputeStream field", span))
    };
    Ok(JetComputeStream {
        id: match field("id")? {
            CtValue::Int(n) if *n > 0 => *n,
            _ => return Err(unsupported("compute stream id", span)),
        },
        device: ct_to_device(field("device")?, span)?,
    })
}

fn sparse_to_ct(s: &JetSparseCsr) -> CtValue {
    CtValue::Struct {
        type_name: "SparseTensor".to_string(),
        fields: vec![
            ("rows".to_string(), CtValue::Int(s.rows)),
            ("cols".to_string(), CtValue::Int(s.cols)),
            (
                "row_ptr".to_string(),
                CtValue::List(s.row_ptr.iter().copied().map(CtValue::Int).collect()),
            ),
            (
                "col_idx".to_string(),
                CtValue::List(s.col_idx.iter().copied().map(CtValue::Int).collect()),
            ),
            (
                "values".to_string(),
                CtValue::List(
                    s.values
                        .iter()
                        .map(|v| CtValue::Float(CtFloat::f64(*v)))
                        .collect(),
                ),
            ),
        ],
    }
}

fn ct_to_sparse(value: &CtValue, span: Span) -> Result<JetSparseCsr, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("SparseTensor", span));
    };
    if type_name != "SparseTensor" && type_name != "JetSparseCsr" {
        return Err(unsupported("SparseTensor", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("SparseTensor field", span))
    };
    let sparse = JetSparseCsr {
        rows: match field("rows")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("sparse rows", span)),
        },
        cols: match field("cols")? {
            CtValue::Int(n) => *n,
            _ => return Err(unsupported("sparse cols", span)),
        },
        row_ptr: as_i64_list(field("row_ptr")?, span)?,
        col_idx: as_i64_list(field("col_idx")?, span)?,
        values: as_f64_list(field("values")?, span)?,
    };
    jet_compute_validate_sparse(&sparse)
        .map_err(|error| unsupported(&format!("SparseTensor metadata: {}", error.jet_show()), span))?;
    Ok(sparse)
}

fn ok_sparse(s: JetSparseCsr) -> CtValue {
    CtValue::Present(Box::new(sparse_to_ct(&s)))
}

fn ok_tensor(tensor: JetTensor) -> CtValue {
    CtValue::Present(Box::new(tensor_to_ct(&tensor)))
}

fn err_compute(err: JetComputeError) -> CtValue {
    CtValue::failed(Box::new(map_err(err)))
}

fn autodiff_state(
    output: &CtValue,
    anchor: &CtValue,
    span: Span,
) -> Result<JetComputeVjpState, Diagnostic> {
    let output = ct_to_tensor(output, span)?;
    let anchor = ct_to_tensor(anchor, span)?;
    let tape = anchor
        .trace
        .as_ref()
        .and_then(|trace| trace.tape.upgrade())
        .or_else(|| output.trace.as_ref().and_then(|trace| trace.tape.upgrade()))
        .unwrap_or_else(jet_compute_empty_tape);
    Ok(jet_compute_vjp_begin(output, tape))
}

fn autodiff_transform_state(
    method: &str,
    output: &CtValue,
    anchor: &CtValue,
    tangents: &[CtValue],
    targets: &[i64],
    span: Span,
) -> Result<JetComputeTransformResult, Diagnostic> {
    let state = autodiff_state(output, anchor, span)?;
    let tangents = tangents
        .iter()
        .map(|value| ct_to_tensor(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_transform(method, &state, &tangents, targets).map_err(|error| {
        unsupported(
            &format!("compute.{method} autodiff: {}", error.jet_show()),
            span,
        )
    })
}

pub fn autodiff_value(
    output: &CtValue,
    anchor: &CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let state = autodiff_state(output, anchor, span)?;
    Ok(tensor_to_ct(&jet_compute_remove_trace_level(
        &state.value,
        &state.tape,
    )))
}

/// Start one per-call autodiff tape for the Tensor arguments. The returned
/// values retain the tape through the CtValue boundary; no ambient/global tape
/// is used.
pub fn autodiff_trace_inputs(
    values: &[CtValue],
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let inputs = values
        .iter()
        .map(|value| ct_to_tensor(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    let (tape, tracked) = jet_compute_trace_inputs(inputs);
    // `tensor_to_ct` places the strong handle in each trace identity.  Keep
    // this local alive until all tracked arguments have crossed the boundary.
    let _tape_owner = tape;
    Ok(tracked.iter().map(tensor_to_ct).collect())
}

pub fn autodiff_gradient(
    output: &CtValue,
    anchor: &CtValue,
    targets: &[i64],
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let JetComputeTransformResult::Gradient(values) = autodiff_transform_state(
        "gradient",
        output,
        anchor,
        &[],
        targets,
        span,
    )? else {
        return Err(unsupported("compute.gradient result", span));
    };
    Ok(values.iter().map(tensor_to_ct).collect())
}

pub fn autodiff_unit_grads(
    output: &CtValue,
    anchor: &CtValue,
    targets: &[i64],
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    autodiff_gradient(output, anchor, targets, span)
}

pub fn autodiff_nested_gradient(
    outputs: &[CtValue],
    anchor: &CtValue,
    targets: &[i64],
    span: Span,
) -> Result<Vec<Vec<CtValue>>, Diagnostic> {
    let states = outputs
        .iter()
        .map(|output| autodiff_state(output, anchor, span))
        .collect::<Result<Vec<_>, _>>()?;
    let gradients = jet_compute_nested_gradient(&states, targets).map_err(|error| {
        unsupported(
            &format!("compute.gradient autodiff: {}", error.jet_show()),
            span,
        )
    })?;
    Ok(gradients
        .iter()
        .map(|values| values.iter().map(tensor_to_ct).collect())
        .collect())
}

pub fn autodiff_vjp_pull(
    output: &CtValue,
    anchor: &CtValue,
    seed: &CtValue,
    targets: &[i64],
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let seed = ct_to_tensor(seed, span)?;
    let state = match autodiff_transform_state("vjp", output, anchor, &[], targets, span)? {
        JetComputeTransformResult::Vjp { state, .. } => state,
        _ => return Err(unsupported("compute.vjp result", span)),
    };
    let values = jet_compute_vjp_pull_or_panic(&state, &seed, targets, "compute.vjp.pull");
    Ok(values.iter().map(tensor_to_ct).collect())
}

pub fn autodiff_jvp(
    output: &CtValue,
    anchor: &CtValue,
    tangents: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let JetComputeTransformResult::Jvp { tangent, .. } = autodiff_transform_state(
        "jvp",
        output,
        anchor,
        tangents,
        &[],
        span,
    )? else {
        return Err(unsupported("compute.jvp result", span));
    };
    Ok(tensor_to_ct(&tangent))
}

fn as_float(value: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match value {
        CtValue::Float(f) => Ok(f.as_f64()),
        CtValue::Int(n) => Ok(*n as f64),
        _ => Err(unsupported("Float", span)),
    }
}

fn as_int(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("Int", span)),
    }
}

/// Evaluate a `core.compute` call against the shared Prelude (I9).
pub fn apply(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.compute.{method} arg {i}"), span))
    };
    match method {
        "zeros" => Ok(match jet_compute_zeros(&as_i64_list(one(0)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "ones" => Ok(match jet_compute_ones(&as_i64_list(one(0)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "full" => Ok(
            match jet_compute_full(&as_i64_list(one(0)?, span)?, as_float(one(1)?, span)?) {
                Ok(t) => ok_tensor(t),
                Err(e) => err_compute(e),
            },
        ),
        "from_list" => Ok(match jet_compute_from_list(&as_f64_list(one(0)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "matrix" => Ok(match jet_compute_matrix(
            as_int(one(0)?, span)?,
            as_int(one(1)?, span)?,
            as_float(one(2)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "vec" => Ok(match jet_compute_vec(as_int(one(0)?, span)?, as_float(one(1)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "add" => Ok(match jet_compute_add(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "mul" => Ok(match jet_compute_mul(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "matmul" => Ok(match jet_compute_matmul(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "reshape" => Ok(match jet_compute_reshape(
            &ct_to_tensor(one(0)?, span)?,
            &as_i64_list(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "get" => Ok(match jet_compute_get(
            &ct_to_tensor(one(0)?, span)?,
            &as_i64_list(one(1)?, span)?,
        ) {
            Ok(v) => CtValue::Present(Box::new(CtValue::Float(CtFloat::f64(v)))),
            Err(e) => err_compute(e),
        }),
        "set" => {
            let handle = tensor_handle_state(one(0)?, span)?;
            let mut tensor = handle
                .tensor
                .lock()
                .map_err(|_| unsupported("Tensor handle", span))?;
            match jet_compute_set(
                &mut *tensor,
                &as_i64_list(one(1)?, span)?,
                as_float(one(2)?, span)?,
            ) {
                Ok(()) => {
                    let snapshot = (*tensor).clone();
                    drop(tensor);
                    Ok(CtValue::Present(Box::new(CtValue::Struct {
                        type_name: "__JetComputeSet".to_string(),
                        fields: vec![
                            (
                                "tensor".to_string(),
                                tensor_to_ct_with_handle(
                                    &snapshot,
                                    std::sync::Arc::clone(&handle.tensor),
                                    handle.valid,
                                    true,
                                ),
                            ),
                            ("unit".to_string(), CtValue::Unit),
                        ],
                    })))
                }
                Err(e) => Ok(err_compute(e)),
            }
        }
        "shape" => Ok(CtValue::List(
            jet_compute_tensor_shape(&ct_to_tensor(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Int)
                .collect(),
        )),
        "rank" => Ok(CtValue::Int(jet_compute_tensor_rank(&ct_to_tensor(
            one(0)?,
            span,
        )?))),
        "numel" => Ok(CtValue::Int(jet_compute_tensor_numel(&ct_to_tensor(
            one(0)?,
            span,
        )?))),
        "to_list" => Ok(CtValue::List(
            jet_compute_tensor_to_list(&ct_to_tensor(one(0)?, span)?)
                .into_iter()
                .map(|v| CtValue::Float(CtFloat::f64(v)))
                .collect(),
        )),
        "device" => Ok(CtValue::Str(jet_compute_tensor_device(&ct_to_tensor(
            one(0)?,
            span,
        )?))),
        "placement" => Ok(CtValue::Str(jet_compute_tensor_placement(
            &ct_to_tensor(one(0)?, span)?,
        ))),
        "device_cpu" => Ok(device_to_ct(jet_compute_device_cpu())),
        "device_auto" => Ok(device_to_ct(jet_compute_device_auto())),
        "on_device" => Ok(match jet_compute_on_device(
            &ct_to_tensor(one(0)?, span)?,
            ct_to_device(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        // #1136 ndarray slice — broadcast / ufunc / transpose / sum_axis
        "broadcast_to" => Ok(match jet_compute_broadcast_to(
            &ct_to_tensor(one(0)?, span)?,
            &as_i64_list(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "transpose" => Ok(match jet_compute_transpose(&ct_to_tensor(one(0)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "sum_axis" => Ok(match jet_compute_sum_axis(
            &ct_to_tensor(one(0)?, span)?,
            as_int(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "negate" | "abs" | "exp" | "log" | "sqrt" => {
            Ok(match jet_compute_unary(method, &ct_to_tensor(one(0)?, span)?) {
                Ok(t) => ok_tensor(t),
                Err(e) => err_compute(e),
            })
        }
        "sub" | "div" | "maximum" | "minimum" => Ok(match jet_compute_binary(
            method,
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "eye" => Ok(match jet_compute_eye(as_int(one(0)?, span)?) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "det" => Ok(match jet_compute_det(&ct_to_tensor(one(0)?, span)?) {
            Ok(v) => CtValue::Present(Box::new(CtValue::Float(CtFloat::f64(v)))),
            Err(e) => err_compute(e),
        }),
        "inv" | "fft" => {
            let tensor = ct_to_tensor(one(0)?, span)?;
            Ok(match if method == "inv" {
                jet_compute_inv(&tensor)
            } else {
                jet_compute_fft(&tensor)
            } {
                Ok(t) => ok_tensor(t),
                Err(e) => err_compute(e),
            })
        }
        "solve" => Ok(match jet_compute_solve(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "stream_new" => Ok(stream_to_ct(&jet_compute_stream_new())),
        "stream_sync" => Ok(
            match jet_compute_stream_sync(&ct_to_stream(one(0)?, span)?) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(e) => err_compute(e),
            },
        ),
        "stream_show" => Ok(CtValue::Str(jet_compute_stream_show(&ct_to_stream(
            one(0)?,
            span,
        )?))),
        "transfer" => Ok(match jet_compute_transfer(
            &ct_to_tensor(one(0)?, span)?,
            ct_to_device(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "transfer_show" => Ok(CtValue::Str(jet_compute_transfer_show(&ct_to_tensor(
            one(0)?,
            span,
        )?))),
        "kernel_bounds_ok" => Ok(match jet_compute_kernel_bounds_ok(
            &as_i64_list(one(0)?, span)?,
            &as_i64_list(one(1)?, span)?,
        ) {
            Ok(b) => CtValue::Present(Box::new(CtValue::Bool(b))),
            Err(e) => err_compute(e),
        }),
        "mse_loss" => Ok(match jet_compute_mse_loss(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "sgd_step" => Ok(match jet_compute_sgd_step(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
            as_float(one(2)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "serialize" => Ok(match jet_compute_serialize(&ct_to_tensor(one(0)?, span)?) {
            Ok(payload) => CtValue::Present(Box::new(CtValue::Str(payload))),
            Err(e) => err_compute(e),
        }),
        "deserialize" => {
            let payload = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("serialize payload", span)),
            };
            Ok(match jet_compute_deserialize(&payload) {
                Ok(t) => ok_tensor(t),
                Err(e) => err_compute(e),
            })
        }
        "to_sparse" => Ok(match jet_compute_to_sparse(&ct_to_tensor(one(0)?, span)?) {
            Ok(s) => ok_sparse(s),
            Err(e) => err_compute(e),
        }),
        "sparse_nnz" => Ok(CtValue::Int(jet_compute_sparse_nnz(&ct_to_sparse(
            one(0)?,
            span,
        )?))),
        "sparse_mv" => Ok(match jet_compute_sparse_mv(
            &ct_to_sparse(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "sparse_show" => Ok(CtValue::Str(jet_compute_sparse_show(&ct_to_sparse(
            one(0)?,
            span,
        )?))),
        "matmul_f32_tile" => Ok(match jet_compute_matmul_f32_tile(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "profile_f32_strict" | "profile_show" => Ok(CtValue::Str(jet_compute_profile_show())),
        _ => Err(unsupported(
            &format!("`core.compute.{method}()`"),
            span,
        )),
    }
}

/// Unpack only `set`'s success payload. Failed results stay untouched so the
/// caller can return the canonical Prelude error without mutating its place.
pub fn take_set_ok(value: &CtValue) -> Option<(CtValue, CtValue)> {
    let CtValue::Present(inner) = value else {
        return None;
    };
    let CtValue::Struct { type_name, fields } = inner.as_ref() else {
        return None;
    };
    if type_name != "__JetComputeSet" {
        return None;
    }
    let tensor = fields.iter().find(|(n, _)| n == "tensor")?.1.clone();
    Some((tensor, CtValue::Present(Box::new(CtValue::Unit))))
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("Tensor".to_string())
}
