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
            CtValue::Int(n) => *n,
            _ => 1,
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
    Ok(JetSparseCsr {
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
    })
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
                    Ok(s) => CtValue::ResOk(Box::new(CtValue::Str(s))),
                    Err(e) => err_compute(e),
                },
            )
        }
        "jvp_mul" => Ok(match jet_compute_jvp_mul(
            &ct_to_tensor(one(0)?, span)?,
            &ct_to_tensor(one(1)?, span)?,
            &ct_to_tensor(one(2)?, span)?,
            &ct_to_tensor(one(3)?, span)?,
        ) {
            Ok(t) => ok_tensor(t),
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
