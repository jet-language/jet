//! D-COMPUTE1=D / I9: `core.compute` on the canonical TIR evaluator.
//! Semantics come from `ComputeLite` → shared Prelude `Compute.rs`.

use std::collections::HashMap;

use crate::AST::{CtValue, Type};
use crate::Comptime::apply_core_call;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Codegen::TIR::TExpr;

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

fn tuple_value(
    ty: &Type,
    values: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
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

impl<'a> EvalCtx<'a> {
    pub(super) fn eval_compute_transform(
        &mut self,
        method: &str,
        base: CtValue,
        values: Vec<CtValue>,
        targets: Vec<i64>,
        result_ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let (primal_values, tangent_values) = if method == "jvp" {
            let primal_count = values.len() / 2;
            if values.len() != primal_count.saturating_mul(2) {
                return Err(super::unsupported("compute.jvp arguments", span));
            }
            values.split_at(primal_count)
        } else {
            (&values[..], &[][..])
        };
        let tracked = crate::Comptime::ComputeLite::autodiff_trace_inputs(primal_values, span)?;
        let output = self.call_callable(&base, tracked.clone())?;
        let anchor = tracked
            .first()
            .cloned()
            .unwrap_or_else(|| output.clone());
        match method {
            "gradient" => {
                let gradient_ty = compute_gradient_type(method, result_ty)
                    .ok_or_else(|| super::unsupported("compute.gradient result", span))?;
                if let Type::Tuple(gradient_fields) = &gradient_ty {
                    let Some((_, first_inner_ty)) = gradient_fields.first() else {
                        return Err(super::unsupported("compute.gradient result", span));
                    };
                    if let Type::Tuple(component_fields) = first_inner_ty.as_ref() {
                        let component_values = component_fields
                            .iter()
                            .map(|(component, _)| tuple_field(&output, component, span))
                            .collect::<Result<Vec<_>, _>>()?;
                        let component_gradients =
                            crate::Comptime::ComputeLite::autodiff_nested_gradient(
                                &component_values,
                                &anchor,
                                &targets,
                                span,
                            )?;
                        let values = gradient_fields
                            .iter()
                            .enumerate()
                            .map(|(target_index, (_, inner_ty))| {
                                let Type::Tuple(inner_fields) = inner_ty.as_ref() else {
                                    return Err(super::unsupported(
                                        "compute.gradient result",
                                        span,
                                    ));
                                };
                                let inner_values = inner_fields
                                    .iter()
                                    .enumerate()
                                    .map(|(component_index, _)| {
                                        component_gradients
                                            .get(component_index)
                                            .and_then(|values| values.get(target_index))
                                            .cloned()
                                            .ok_or_else(|| {
                                                super::unsupported(
                                                    "compute.gradient result",
                                                    span,
                                                )
                                            })
                                    })
                                    .collect::<Result<Vec<_>, _>>()?;
                                tuple_value(inner_ty, inner_values, span)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        return tuple_value(&gradient_ty, values, span);
                    }
                }
                let values = crate::Comptime::ComputeLite::autodiff_gradient(
                    &output,
                    &anchor,
                    &targets,
                    span,
                )?;
                tuple_value(&gradient_ty, values, span)
            }
            "value_and_gradient" => {
                let gradient_ty = compute_gradient_type(method, result_ty).ok_or_else(|| {
                    super::unsupported("compute.value_and_gradient result", span)
                })?;
                let gradients = crate::Comptime::ComputeLite::autodiff_gradient(
                    &output,
                    &anchor,
                    &targets,
                    span,
                )?;
                let gradients = tuple_value(&gradient_ty, gradients, span)?;
                let value = crate::Comptime::ComputeLite::autodiff_value(
                    &output,
                    &anchor,
                    span,
                )?;
                tuple_value(result_ty, vec![value, gradients], span)
            }
            "vjp" => {
                let gradient_ty = compute_gradient_type(method, result_ty)
                    .ok_or_else(|| super::unsupported("compute.vjp result", span))?;
                let pull = self.store_callable(EvalCallable::ComputePull {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                });
                let grads = self.store_callable(EvalCallable::ComputeGrads {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                });
                let value = crate::Comptime::ComputeLite::autodiff_value(
                    &output,
                    &anchor,
                    span,
                )?;
                Ok(CtValue::Struct {
                    type_name: "VjpRun".to_string(),
                    fields: vec![
                        ("value".to_string(), value),
                        ("pull".to_string(), pull),
                        ("grads".to_string(), grads),
                    ],
                })
            }
            "jvp" => {
                let tangent = crate::Comptime::ComputeLite::autodiff_jvp(
                    &output,
                    &anchor,
                    tangent_values,
                    span,
                )?;
                let value = crate::Comptime::ComputeLite::autodiff_value(
                    &output,
                    &anchor,
                    span,
                )?;
                tuple_value(result_ty, vec![value, tangent], span)
            }
            _ => Err(super::unsupported("core.compute transform", span)),
        }
    }

    pub(super) fn eval_compute_pull(
        &mut self,
        output: CtValue,
        anchor: CtValue,
        args: Vec<CtValue>,
        targets: Vec<i64>,
        gradient_ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        let span = self.span();
        let [seed] = args.as_slice() else {
            return Err(super::unsupported("compute.vjp.pull seed", span));
        };
        let values = crate::Comptime::ComputeLite::autodiff_vjp_pull(
            &output,
            &anchor,
            seed,
            &targets,
            span,
        )?;
        tuple_value(gradient_ty, values, span)
    }

    pub(super) fn eval_compute_grads(
        &mut self,
        output: CtValue,
        anchor: CtValue,
        args: Vec<CtValue>,
        targets: Vec<i64>,
        gradient_ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        if !args.is_empty() {
            return Err(super::unsupported("compute.vjp.grads arguments", self.span()));
        }
        let span = self.span();
        let values = crate::Comptime::ComputeLite::autodiff_unit_grads(
            &output,
            &anchor,
            &targets,
            span,
        )?;
        tuple_value(gradient_ty, values, span)
    }

    pub(super) fn eval_core_compute_call(
        &mut self,
        method: &str,
        args: &'a [TExpr],
        return_ty: &Type,
        source_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if matches!(method, "gradient" | "value_and_gradient" | "vjp" | "jvp")
            && args.len() >= 2
        {
            let base = self.eval_expr(&args[0], scope)?;
            let target = self.eval_expr(args.last().expect("autodiff target arg"), scope)?;
            let targets = target_indexes(&target, source_span)?;
            let mut values = Vec::with_capacity(args.len().saturating_sub(2));
            for arg in &args[1..args.len() - 1] {
                values.push(self.eval_expr(arg, scope)?);
            }
            if args.len() == 2 {
                let result_ty = function_result_type(return_ty, source_span)?;
                return Ok(self.store_callable(EvalCallable::ComputeTransform {
                    base,
                    method: method.to_string(),
                    targets,
                    result_ty,
                }));
            }
            return self.eval_compute_transform(method, base, values, targets, return_ty);
        }

        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_expr(a, scope)?);
        }
        let result = apply_core_call("core.compute", method, argv, source_span, self.repl_mode)?;
        if method == "set" {
            if let Some((tensor, unit)) = crate::Comptime::ComputeLite::take_set_ok(result) {
                if let Some(place) = args.first() {
                    self.write_back_place(place, tensor, scope)?;
                }
                return Ok(unit);
            }
            // take_set_ok always answers a payload for carrier shapes; fall through
            // only if the payload was unexpected.
            return Ok(CtValue::failed(Box::new(CtValue::Str(
                "core.compute.set: unexpected ambient payload".to_string(),
            ))));
        }
        Ok(result)
    }
}
