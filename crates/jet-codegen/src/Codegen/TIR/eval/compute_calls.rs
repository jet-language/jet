//! D-COMPUTE1=D / I9: `core.compute` on the canonical TIR evaluator.
//! Semantics come from `ComputeLite` → shared Prelude `Compute.rs`.

use std::collections::HashMap;

use crate::Codegen::TIR::TExpr;
use crate::Comptime::apply_core_call;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtValue, Type};

use super::{EvalCallable, EvalCtx};

fn target_indexes(value: &CtValue, span: Span) -> Result<Vec<i64>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(super::unsupported("autodiff target list", span));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Int(index) => Ok(*index),
            _ => Err(super::unsupported("autodiff target index", span)),
        })
        .collect()
}

fn compute_gradient_type(method: &str, result_ty: &Type) -> Option<Type> {
    match method {
        "gradient" => Some(result_ty.clone()),
        "value_and_gradient" => match result_ty {
            Type::Tuple(fields) => fields
                .iter()
                .find(|(name, _)| name == "gradients")
                .map(|(_, ty)| (**ty).clone()),
            _ => None,
        },
        "vjp" => match result_ty {
            Type::Apply { name, args } if name == "VjpRun" && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        },
        _ => None,
    }
}

fn tuple_value(ty: &Type, values: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let Type::Tuple(fields) = ty else {
        return Err(super::unsupported("autodiff tuple result", span));
    };
    if fields.len() != values.len() {
        return Err(super::unsupported("autodiff tuple arity", span));
    }
    Ok(CtValue::Struct {
        type_name: "tuple".to_string(),
        fields: fields
            .iter()
            .map(|(name, _)| name.clone())
            .zip(values)
            .collect(),
    })
}

fn tuple_field(value: &CtValue, name: &str, span: Span) -> Result<CtValue, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(super::unsupported("autodiff named tuple", span));
    };
    if type_name != "tuple" {
        return Err(super::unsupported("autodiff named tuple", span));
    }
    fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| super::unsupported("autodiff tuple field", span))
}

fn function_result_type(ty: &Type, span: Span) -> Result<Type, Diagnostic> {
    match ty {
        Type::Fn { ret: Some(ret), .. } => Ok((**ret).clone()),
        _ => Err(super::unsupported("autodiff transform result", span)),
    }
}

fn compute_transform_kind(
    method: &str,
    span: Span,
) -> Result<crate::Comptime::ComputeLite::JetComputeTransformKind, Diagnostic> {
    match method {
        "gradient" => Ok(crate::Comptime::ComputeLite::JetComputeTransformKind::Gradient),
        "value_and_gradient" => {
            Ok(crate::Comptime::ComputeLite::JetComputeTransformKind::ValueAndGradient)
        }
        "vjp" => Ok(crate::Comptime::ComputeLite::JetComputeTransformKind::Vjp),
        "jvp" => Ok(crate::Comptime::ComputeLite::JetComputeTransformKind::Jvp),
        _ => Err(super::unsupported("core.compute transform", span)),
    }
}

fn compute_result_shape(ty: &Type) -> crate::Comptime::ComputeLite::JetComputeResultShape {
    match ty {
        Type::Tuple(fields)
            if fields
                .iter()
                .all(|(_, field)| field.is_compute_tensor_family()) =>
        {
            crate::Comptime::ComputeLite::JetComputeResultShape::TensorTuple(fields.len())
        }
        _ => crate::Comptime::ComputeLite::JetComputeResultShape::Tensor,
    }
}

fn curried_gradient_value(
    gradient_ty: &Type,
    values: &[Vec<crate::Comptime::ComputeLite::JetTensor>],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let Type::Tuple(fields) = gradient_ty else {
        return Err(super::unsupported("autodiff gradient result", span));
    };
    if fields.len() != values.len() {
        return Err(super::unsupported("autodiff gradient arity", span));
    }
    let values = fields
        .iter()
        .zip(values)
        .map(|((_, ty), gradients)| match ty.as_ref() {
            Type::Tuple(inner_fields) if inner_fields.len() == gradients.len() => tuple_value(
                ty,
                gradients
                    .iter()
                    .map(crate::Comptime::ComputeLite::autodiff_tensor_to_ct)
                    .collect(),
                span,
            ),
            _ if gradients.len() == 1 => Ok(crate::Comptime::ComputeLite::autodiff_tensor_to_ct(
                &gradients[0],
            )),
            _ => Err(super::unsupported("autodiff gradient result", span)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    tuple_value(gradient_ty, values, span)
}

impl<'a> EvalCtx<'a> {
    fn make_compute_handle(
        &mut self,
        method: &str,
        base: CtValue,
        targets: Vec<i64>,
        base_ty: &Type,
    ) -> Result<
        (
            crate::Comptime::ComputeLite::JetComputeHandle,
            crate::Comptime::ComputeLite::JetComputeTransformKind,
        ),
        Diagnostic,
    > {
        let span = self.span();
        let kind = compute_transform_kind(method, span)?;
        let Type::Fn {
            ret: Some(base_ret),
            ..
        } = base_ty
        else {
            return Err(super::unsupported("autodiff transform function", span));
        };
        let base_arity = match base_ty {
            Type::Fn { params, .. } => params.len(),
            _ => 0,
        };
        let result_shape = compute_result_shape(base_ret);
        let tuple_fields = match base_ret.as_ref() {
            Type::Tuple(fields)
                if matches!(
                    result_shape,
                    crate::Comptime::ComputeLite::JetComputeResultShape::TensorTuple(_)
                ) =>
            {
                fields.iter().map(|(name, _)| name.clone()).collect()
            }
            _ => Vec::new(),
        };
        let base_ptr = self as *mut EvalCtx<'a> as *mut ();
        let base_value = base.clone();
        let base_span = span;
        let plan_base = crate::Comptime::ComputeLite::JetComputeBase::new(
            base_arity,
            move |inputs: &[crate::Comptime::ComputeLite::JetTensor]| {
                let input_values = inputs
                    .iter()
                    .map(crate::Comptime::ComputeLite::autodiff_tensor_to_ct)
                    .collect::<Vec<_>>();
                // SAFETY: the handle is stored only in this evaluator's runtime
                // and is called while that EvalCtx owns the runtime lock.
                let ctx = unsafe { &mut *(base_ptr as *mut EvalCtx<'a>) };
                let output = ctx
                    .call_callable(&base_value, input_values)
                    .map_err(|error| {
                        crate::Comptime::ComputeLite::JetComputeError::Unsupported(
                            error.what.clone(),
                        )
                    })?;
                match &result_shape {
                    crate::Comptime::ComputeLite::JetComputeResultShape::Tensor => {
                        let tensor = crate::Comptime::ComputeLite::autodiff_tensor_from_ct(
                            &output, base_span,
                        )
                        .map_err(|error| {
                            crate::Comptime::ComputeLite::JetComputeError::Unsupported(
                                error.what.clone(),
                            )
                        })?;
                        Ok(crate::Comptime::ComputeLite::JetComputeBaseResult::Tensor(
                            tensor,
                        ))
                    }
                    crate::Comptime::ComputeLite::JetComputeResultShape::TensorTuple(_) => {
                        let values = tuple_fields
                            .iter()
                            .map(|field| {
                                tuple_field(&output, field, base_span).map_err(|error| {
                                    crate::Comptime::ComputeLite::JetComputeError::Unsupported(
                                        error.what.clone(),
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                            .iter()
                            .map(|value| {
                                crate::Comptime::ComputeLite::autodiff_tensor_from_ct(
                                    value, base_span,
                                )
                                .map_err(|error| {
                                    crate::Comptime::ComputeLite::JetComputeError::Unsupported(
                                        error.what.clone(),
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(crate::Comptime::ComputeLite::JetComputeBaseResult::TensorTuple(values))
                    }
                }
            },
        );
        let raw = crate::Comptime::ComputeLite::jet_compute_curried_new(
            plan_base,
            kind,
            &targets,
            result_shape,
        );
        Ok((
            crate::Comptime::ComputeLite::JetComputeHandle::new(raw),
            kind,
        ))
    }

    fn store_compute_handle(
        &mut self,
        raw: i64,
        kind: crate::Comptime::ComputeLite::JetComputeTransformKind,
        result_ty: Type,
    ) -> CtValue {
        self.store_callable(EvalCallable::ComputeHandle {
            handle: crate::Comptime::ComputeLite::JetComputeHandle::new(raw),
            kind,
            result_ty,
        })
    }

    pub(super) fn eval_compute_handle(
        &mut self,
        handle: &crate::Comptime::ComputeLite::JetComputeHandle,
        kind: crate::Comptime::ComputeLite::JetComputeTransformKind,
        args: Vec<CtValue>,
        result_ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let tensors = args
            .iter()
            .map(|value| crate::Comptime::ComputeLite::autodiff_tensor_from_ct(value, span))
            .collect::<Result<Vec<_>, _>>()?;
        let (primals, tangents) = if kind.is_jvp() {
            if tensors.len() % 2 != 0 {
                return Err(super::unsupported("compute.jvp arguments", span));
            }
            let split = tensors.len() / 2;
            (tensors[..split].to_vec(), tensors[split..].to_vec())
        } else {
            (tensors, Vec::new())
        };
        let result = crate::Comptime::ComputeLite::jet_compute_call_curried(
            handle.raw(),
            crate::Comptime::ComputeLite::JetComputeInputPack::new(primals, tangents),
        )
        .map_err(|error| {
            super::unsupported(
                &crate::Comptime::ComputeLite::autodiff_error_message(&error),
                span,
            )
        })?;
        match result {
            crate::Comptime::ComputeLite::JetComputeCurriedResult::Gradient(values) => {
                curried_gradient_value(result_ty, &values, span)
            }
            crate::Comptime::ComputeLite::JetComputeCurriedResult::ValueAndGradient {
                value,
                gradients,
            } => {
                let gradient_ty = compute_gradient_type("value_and_gradient", result_ty)
                    .ok_or_else(|| super::unsupported("compute.value_and_gradient result", span))?;
                tuple_value(
                    result_ty,
                    vec![
                        crate::Comptime::ComputeLite::autodiff_tensor_to_ct(&value),
                        curried_gradient_value(&gradient_ty, &gradients, span)?,
                    ],
                    span,
                )
            }
            crate::Comptime::ComputeLite::JetComputeCurriedResult::Vjp { value, pull, grads } => {
                let gradient_ty = compute_gradient_type("vjp", result_ty)
                    .ok_or_else(|| super::unsupported("compute.vjp result", span))?;
                let pull = self.store_compute_handle(
                    pull,
                    crate::Comptime::ComputeLite::JetComputeTransformKind::Gradient,
                    gradient_ty.clone(),
                );
                let grads = self.store_compute_handle(
                    grads,
                    crate::Comptime::ComputeLite::JetComputeTransformKind::Gradient,
                    gradient_ty,
                );
                Ok(CtValue::Struct {
                    type_name: "VjpRun".to_string(),
                    fields: vec![
                        (
                            "value".to_string(),
                            crate::Comptime::ComputeLite::autodiff_tensor_to_ct(&value),
                        ),
                        ("pull".to_string(), pull),
                        ("grads".to_string(), grads),
                    ],
                })
            }
            crate::Comptime::ComputeLite::JetComputeCurriedResult::Jvp { value, tangent } => {
                tuple_value(
                    result_ty,
                    vec![
                        crate::Comptime::ComputeLite::autodiff_tensor_to_ct(&value),
                        crate::Comptime::ComputeLite::autodiff_tensor_to_ct(&tangent),
                    ],
                    span,
                )
            }
        }
    }

    pub(super) fn eval_core_compute_call(
        &mut self,
        method: &str,
        args: &'a [TExpr],
        return_ty: &Type,
        source_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if matches!(method, "gradient" | "value_and_gradient" | "vjp" | "jvp") && args.len() >= 2 {
            let base = self.eval_expr(&args[0], scope)?;
            let target = self.eval_expr(args.last().expect("autodiff target arg"), scope)?;
            let targets = target_indexes(&target, source_span)?;
            let mut values = Vec::with_capacity(args.len().saturating_sub(2));
            for arg in &args[1..args.len() - 1] {
                values.push(self.eval_expr(arg, scope)?);
            }
            let (handle, kind) = self.make_compute_handle(method, base, targets, &args[0].ty)?;
            if args.len() == 2 {
                let result_ty = function_result_type(return_ty, source_span)?;
                return Ok(self.store_callable(EvalCallable::ComputeHandle {
                    handle,
                    kind,
                    result_ty,
                }));
            }
            return self.eval_compute_handle(&handle, kind, values, return_ty);
        }

        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_expr(a, scope)?);
        }
        let result = apply_core_call("core.compute", method, argv, source_span, self.repl_mode)?;
        if method == "set" {
            if let Some((tensor, unit)) = crate::Comptime::ComputeLite::take_set_ok(&result) {
                if let Some(place) = args.first() {
                    self.write_back_place(place, tensor, scope)?;
                }
                return Ok(unit);
            }
            return Ok(result);
        }
        Ok(result)
    }
}
