//! D-COMPUTE1=D / I9: `core.compute` ambient mirrors AOT Prelude `jet_compute_*`
//! by including the same source (`Prelude/CoreLib/Top/Compute.rs`). Marshalling
//! to `CtValue` lives here; engines must not re-encode tensor law.

use crate::AST::{CtFloat, CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

trait JetShow {
    fn jet_show(&self) -> String;
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
    let reason = match field("reason")? {
        CtValue::Str(s) => s.clone(),
        _ => return Err(unsupported("placement reason", span)),
    };
    Ok(JetComputePlacementReceipt {
        requested,
        selected,
        reason,
    })
}

fn tensor_to_ct(tensor: &JetTensor) -> CtValue {
    CtValue::Struct {
        type_name: "Tensor".to_string(),
        fields: vec![
            (
                "shape".to_string(),
                CtValue::List(tensor.shape.iter().map(|d| CtValue::Int(*d)).collect()),
            ),
            (
                "strides".to_string(),
                CtValue::List(tensor.strides.iter().map(|d| CtValue::Int(*d)).collect()),
            ),
            (
                "data".to_string(),
                CtValue::List(
                    tensor
                        .data
                        .iter()
                        .map(|v| CtValue::Float(CtFloat::f64(*v)))
                        .collect(),
                ),
            ),
            ("device".to_string(), device_to_ct(tensor.device)),
            (
                "last_placement".to_string(),
                receipt_to_ct(&tensor.last_placement),
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
    Ok(JetTensor {
        shape: as_i64_list(field("shape")?, span)?,
        strides: as_i64_list(field("strides")?, span)?,
        data: as_f64_list(field("data")?, span)?,
        device: ct_to_device(field("device")?, span)?,
        last_placement: ct_to_receipt(field("last_placement")?, span)?,
    })
}

fn map_err(err: JetComputeError) -> CtValue {
    let (variant, message) = match err {
        JetComputeError::InvalidShape(m) => ("InvalidShape", m),
        JetComputeError::RankMismatch(m) => ("RankMismatch", m),
        JetComputeError::OutOfBounds(m) => ("OutOfBounds", m),
        JetComputeError::Device(m) => ("Device", m),
    };
    CtValue::Enum {
        type_name: "ComputeError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
    }
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
