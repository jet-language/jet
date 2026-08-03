//! D-COMPUTE1=D / I9: `core.compute` ambient mirrors AOT Prelude `jet_compute_*`
//! by including the same source (`Prelude/CoreLib/Top/Compute.rs`). Marshalling
//! to `CtValue` lives here; engines must not re-encode tensor law.

use crate::AST::{CtFloat, CtValue, Type};
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
    Ok(JetComputePlacementReceipt {
        requested,
        selected,
        backend: text("backend")?,
        version: text("version")?,
        profile: text("profile")?,
        cache: text("cache")?,
        capabilities,
        reason,
    })
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

fn raw_kernel_to_ct(_contract: &JetRawKernelContract) -> CtValue {
    CtValue::Struct {
        type_name: "RawKernelContract".to_string(),
        fields: Vec::new(),
    }
}

fn ct_to_raw_kernel_contract(
    value: &CtValue,
    span: Span,
) -> Result<JetRawKernelContract, Diagnostic> {
    let _ = value;
    Err(unsupported(
        "provider-issued raw-kernel contract (not forgeable in ambient)",
        span,
    ))
}

fn tensor_to_ct(tensor: &JetTensor) -> CtValue {
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
    CtValue::Struct {
        type_name: "Tensor".to_string(),
        fields: vec![
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
                    .map(|value| CtValue::Some(Box::new(value)))
                    .unwrap_or_else(|| {
                        CtValue::None(Type::Named("ComputeTransfer".to_string()))
                    }),
            ),
        ],
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

fn ct_to_tensor(value: &CtValue, span: Span) -> Result<JetTensor, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Tensor", span));
    };
    if type_name != "Tensor" && type_name != "JetTensor" {
        return Err(unsupported("Tensor", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("Tensor field", span))
    };
    let last_transfer = match field("last_transfer")? {
        CtValue::Some(value) => Some(ct_to_transfer(value, span)?),
        CtValue::None(_) => None,
        _ => return Err(unsupported("Tensor last_transfer", span)),
    };
    let tensor = JetTensor {
        shape: as_i64_list(field("shape")?, span)?,
        strides: as_i64_list(field("strides")?, span)?,
        data: std::sync::Arc::new(as_f64_list(field("data")?, span)?),
        device: ct_to_device(field("device")?, span)?,
        last_placement: ct_to_receipt(field("last_placement")?, span)?,
        last_transfer,
    };
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
    jet_compute_window_bounds(&tensor, start, end, exclusive)
        .map(|bounds| (bounds.start, bounds.end))
        .map_err(|error| unsupported(&format!("Tensor view: {}", error.jet_show()), span))
}

pub fn tensor_view_list(
    value: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let tensor = ct_to_tensor(value, span)?;
    let (start, end, exclusive) = tensor_window_args(args, span)?;
    let view = jet_compute_slice_checked(&tensor, start, end, exclusive)
        .map_err(|error| unsupported(&format!("Tensor view: {}", error.jet_show()), span))?;
    Ok(CtValue::List(
        jet_compute_tensor_to_list(&view)
            .iter()
            .map(|value| CtValue::Float(CtFloat::f64(*value)))
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

pub fn tensor_replace_data(
    value: &CtValue,
    items: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut tensor = ct_to_tensor(value, span)?;
    let values = as_f64_list(&CtValue::List(items), span)?;
    if values.len() != jet_compute_tensor_values(&tensor).len() {
        return Err(unsupported("Tensor write-back length", span));
    }
    tensor.strides = jet_compute_row_major_strides(&tensor.shape)
        .map_err(|error| unsupported(&format!("Tensor write-back: {}", error.jet_show()), span))?;
    tensor.data = std::sync::Arc::new(values);
    jet_compute_validate_tensor(&tensor)
        .map_err(|error| unsupported(&format!("Tensor write-back: {}", error.jet_show()), span))?;
    Ok(tensor_to_ct(&tensor))
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

fn grad_to_ct(g: &JetComputeGradTriple) -> CtValue {
    CtValue::Struct {
        type_name: "GradTriple".to_string(),
        fields: vec![
            ("value".to_string(), tensor_to_ct(&g.value)),
            ("grad_a".to_string(), tensor_to_ct(&g.grad_a)),
            ("grad_b".to_string(), tensor_to_ct(&g.grad_b)),
        ],
    }
}

fn ct_to_grad(value: &CtValue, span: Span) -> Result<JetComputeGradTriple, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("GradTriple", span));
    };
    if type_name != "GradTriple" && type_name != "JetComputeGradTriple" {
        return Err(unsupported("GradTriple", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .ok_or_else(|| unsupported("GradTriple field", span))
    };
    Ok(JetComputeGradTriple {
        value: ct_to_tensor(field("value")?, span)?,
        grad_a: ct_to_tensor(field("grad_a")?, span)?,
        grad_b: ct_to_tensor(field("grad_b")?, span)?,
    })
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

fn ok_grad(g: JetComputeGradTriple) -> CtValue {
    CtValue::ResOk(Box::new(grad_to_ct(&g)))
}

fn ok_sparse(s: JetSparseCsr) -> CtValue {
    CtValue::ResOk(Box::new(sparse_to_ct(&s)))
}

fn ok_tensor(tensor: JetTensor) -> CtValue {
    CtValue::ResOk(Box::new(tensor_to_ct(&tensor)))
}

fn err_compute(err: JetComputeError) -> CtValue {
    CtValue::ResErr(Box::new(map_err(err)))
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
            Ok(v) => CtValue::ResOk(Box::new(CtValue::Float(CtFloat::f64(v)))),
            Err(e) => err_compute(e),
        }),
        "set" => {
            let mut tensor = ct_to_tensor(one(0)?, span)?;
            match jet_compute_set(
                &mut tensor,
                &as_i64_list(one(1)?, span)?,
                as_float(one(2)?, span)?,
            ) {
                Ok(()) => Ok(CtValue::ResOk(Box::new(CtValue::Struct {
                    type_name: "__JetComputeSet".to_string(),
                    fields: vec![
                        ("tensor".to_string(), tensor_to_ct(&tensor)),
                        ("unit".to_string(), CtValue::Unit),
                    ],
                }))),
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
            Ok(v) => CtValue::ResOk(Box::new(CtValue::Float(CtFloat::f64(v)))),
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
                Ok(()) => CtValue::ResOk(Box::new(CtValue::Unit)),
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
            Ok(b) => CtValue::ResOk(Box::new(CtValue::Bool(b))),
            Err(e) => err_compute(e),
        }),
        "raw_kernel_contract" => {
            let reason = match one(0)? {
                CtValue::Str(s) => s.clone(),
                _ => return Err(unsupported("raw kernel reason", span)),
            };
            Ok(
                match jet_compute_raw_kernel_contract(reason, as_int(one(1)?, span)?) {
                    Ok(contract) => CtValue::ResOk(Box::new(raw_kernel_to_ct(&contract))),
                    Err(e) => err_compute(e),
                },
            )
        }
        "raw_kernel_contract_show" => Ok(CtValue::Str(
            jet_compute_raw_kernel_contract_show(&ct_to_raw_kernel_contract(one(0)?, span)?),
        )),
        "jvp_add" | "jvp_mul" | "jvp_matmul" => Ok(match if method == "jvp_add" {
            jet_compute_jvp_add(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
                &ct_to_tensor(one(3)?, span)?,
            )
        } else if method == "jvp_matmul" {
            jet_compute_jvp_matmul(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
                &ct_to_tensor(one(3)?, span)?,
            )
        } else {
            jet_compute_jvp_mul(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
                &ct_to_tensor(one(3)?, span)?,
            )
        } {
            Ok(t) => ok_tensor(t),
            Err(e) => err_compute(e),
        }),
        "vjp_add" | "vjp_mul" | "vjp_matmul" => Ok(match if method == "vjp_add" {
            jet_compute_vjp_add_value(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
            )
        } else if method == "vjp_matmul" {
            jet_compute_vjp_matmul_value(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
            )
        } else {
            jet_compute_vjp_mul_value(
                &ct_to_tensor(one(0)?, span)?,
                &ct_to_tensor(one(1)?, span)?,
                &ct_to_tensor(one(2)?, span)?,
            )
        } {
            Ok(g) => ok_grad(g),
            Err(e) => err_compute(e),
        }),
        "value_and_grad_mul" => Ok(match jet_compute_value_and_grad_mul(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(g) => ok_grad(g),
            Err(e) => err_compute(e),
        }),
        "grad_value" | "grad_a" | "grad_b" => {
            let g = ct_to_grad(one(0)?, span)?;
            Ok(tensor_to_ct(&match method {
                "grad_value" => jet_compute_grad_value(&g),
                "grad_a" => jet_compute_grad_a(&g),
                _ => jet_compute_grad_b(&g),
            }))
        }
        "grad_show" => Ok(CtValue::Str(jet_compute_grad_show(&ct_to_grad(
            one(0)?,
            span,
        )?))),
        "mse_loss" => Ok(match jet_compute_mse_loss(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
        ) {
            Ok(v) => CtValue::ResOk(Box::new(CtValue::Float(CtFloat::f64(v)))),
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
        "serialize" => Ok(CtValue::Str(jet_compute_serialize(&ct_to_tensor(
            one(0)?,
            span,
        )?))),
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

/// Unpack `set`'s success payload into (updated tensor, unit Result).
pub fn take_set_ok(value: CtValue) -> Option<(CtValue, CtValue)> {
    match value {
        CtValue::ResOk(inner) => match *inner {
            CtValue::Struct { type_name, fields } if type_name == "__JetComputeSet" => {
                let tensor = fields.iter().find(|(n, _)| n == "tensor")?.1.clone();
                Some((tensor, CtValue::ResOk(Box::new(CtValue::Unit))))
            }
            other => Some((
                CtValue::Unit,
                CtValue::ResOk(Box::new(other)),
            )),
        },
        other => Some((CtValue::Unit, other)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("Tensor".to_string())
}
