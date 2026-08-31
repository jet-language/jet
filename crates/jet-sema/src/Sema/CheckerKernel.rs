//! D-COMPUTE-KERNEL-SURFACE1=B: the conservative safe-kernel proof.
//!
//! A marker is not a proof. This pass accepts only a read-only, effect-free
//! expression kernel whose safety obligations are either structural or
//! delegated to the checked Core compute family. More general loops, indexed
//! writes, captures, barriers, and provider calls remain rejected until their
//! proof facts exist.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::SendCrossing;
use crate::AST::{
    AccessConvention, AutoVectorizationFacts, BinOp, Binding, Expr, ForKind, Func, KernelMode,
    KernelProof, LValue, Stmt, Type, UnOp,
};

const SAFE_COMPUTE_CALLS: &[&str] = &[
    "abs",
    "add",
    "broadcast_to",
    "det",
    "div",
    "exp",
    "fft",
    "full",
    "from_list",
    "inv",
    "matmul",
    "maximum",
    "minimum",
    "mul",
    "negate",
    "numel",
    "ones",
    "rank",
    "reshape",
    "shape",
    "solve",
    "sqrt",
    "sub",
    "sum_axis",
    "to_list",
    "to_sparse",
    "transpose",
    "zeros",
];

struct KernelFailure {
    obligation: &'static str,
    span: Span,
}

impl<'a> super::Checker<'a> {
    /// D-SIMD3=B: prove the deliberately small source shape that the native
    /// backend may mark as vectorizable. The proof is conservative: one
    /// half-open, unit-stride range; one or more distinct indexed stores; and
    /// expressions made only from same-lane reads, loop-invariant scalar
    /// reads, and scalar arithmetic. A dynamic list is admitted only for one
    /// in-place root bounded by that root's `len()`, which proves the root is
    /// the only storage participating in the loop. Calls, control flow,
    /// aliases, and cross-lane reads stay scalar until a later proof adds
    /// them.
    pub(crate) fn prove_auto_vectorization_loop(
        &self,
        kind: &ForKind,
        loop_var: &str,
        body: &[Stmt],
        before_direct: &std::collections::BTreeSet<String>,
        before_edges: &std::collections::BTreeSet<String>,
        before_maximal: bool,
    ) -> Option<AutoVectorizationFacts> {
        let ForKind::Range {
            start,
            end,
            step,
            exclusive,
        } = kind
        else {
            return None;
        };
        if !*exclusive || step.is_some() {
            return None;
        }
        if !matches!(start.without_parens(), Expr::Int(0, ..)) {
            return None;
        }
        let end_root = match end.without_parens() {
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } if method == "len" && args.is_empty() => {
                let Expr::Ident(root, _) = receiver.without_parens() else {
                    return None;
                };
                Some(root.as_str())
            }
            _ => None,
        };
        let extent = match end.without_parens() {
            Expr::Int(value, ..) if *value >= 0 => Some(u64::try_from(*value).ok()?),
            _ => {
                let root = end_root?;
                let info = self.lookup(root)?;
                match &info.ty {
                    Type::FixedList { len, .. } => Some(len.literal_value()?),
                    Type::List(_) => None,
                    _ => return None,
                }
            }
        };
        let mut outputs = std::collections::BTreeSet::new();
        let mut inputs = std::collections::BTreeSet::new();
        let mut element_type = None;
        for stmt in body {
            let Stmt::Assign {
                target,
                op: None,
                value,
                ..
            } = stmt
            else {
                return None;
            };
            let LValue::Index { base, index, .. } = target else {
                return None;
            };
            let Expr::Ident(output, _) = base.without_parens() else {
                return None;
            };
            if !matches!(index.without_parens(), Expr::Ident(name, _) if name == loop_var) {
                return None;
            }
            // Repeated stores to one root have an order-sensitive shape that
            // the native loop consumer does not model. Distinct roots remain
            // independent fixed-list destinations.
            if !outputs.insert(output.clone()) {
                return None;
            }

            let output_info = self.lookup(output)?;
            let (output_elem, same_lane_output) = match &output_info.ty {
                Type::FixedList { elem, len } => {
                    if Some(len.literal_value()?) != extent {
                        return None;
                    }
                    // The write target must be a local value, not a borrowed
                    // parameter. Fixed-list scalar values have no interior
                    // references, so distinct local roots are disjoint
                    // storage by construction.
                    if output_info.param_conv.is_some() || !output_info.mutable {
                        return None;
                    }
                    (elem, false)
                }
                Type::List(elem) => {
                    // A dynamic destination is safe only when the range is
                    // its own length. This is the single-root in-place case;
                    // the final root check below rejects a second collection.
                    if end_root != Some(output.as_str()) {
                        return None;
                    }
                    if output_info.param_conv.is_some_and(|conv| {
                        conv != AccessConvention::Write
                    }) || (!output_info.mutable
                        && output_info.param_conv != Some(AccessConvention::Write))
                    {
                        return None;
                    }
                    (elem, true)
                }
                _ => return None,
            };
            if !is_auto_vectorizable_scalar(output_elem) {
                return None;
            }
            if let Some(expected) = &element_type {
                if expected != output_elem.as_ref() {
                    return None;
                }
            } else {
                element_type = Some((**output_elem).clone());
            }

            if !self.prove_auto_element_expr(
                value,
                loop_var,
                output_elem,
                extent,
                output,
                same_lane_output,
                &mut inputs,
            ) {
                return None;
            }
        }
        if outputs.is_empty() {
            return None;
        }
        let collection_roots = outputs
            .iter()
            .chain(inputs.iter())
            .collect::<std::collections::BTreeSet<_>>();
        if collection_roots.iter().any(|root| {
            self.lookup(root.as_str())
                .is_some_and(|info| matches!(&info.ty, Type::List(_)))
        }) && (end_root.is_none()
            || collection_roots.len() != 1
            || !collection_roots
                .iter()
                .any(|root| root.as_str() == end_root.unwrap()))
        {
            return None;
        }
        let no_cross_iteration_deps = !inputs.iter().any(|input| outputs.contains(input));
        if !no_cross_iteration_deps {
            return None;
        }
        // Two shared parameter roots could name the same backing storage. A
        // single input is safe; multiple inputs must be owned local arrays.
        if inputs.len() > 1
            && inputs.iter().any(|name| {
                self.lookup(name)
                    .is_some_and(|info| info.param_conv == Some(AccessConvention::Read))
            })
        {
            return None;
        }

        let effect_free_body = self.fx_direct.difference(before_direct).next().is_none()
            && self.fx_edges.difference(before_edges).next().is_none()
            && self.fx_maximal == before_maximal;
        if !effect_free_body {
            return None;
        }

        Some(AutoVectorizationFacts {
            element_type: element_type?,
            no_aliasing: true,
            no_early_exit: true,
            effect_free_body,
            no_cross_iteration_deps,
        })
    }

    fn prove_auto_element_expr(
        &self,
        expr: &Expr,
        loop_var: &str,
        element_type: &Type,
        extent: Option<u64>,
        output: &str,
        same_lane_output: bool,
        inputs: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        match expr.without_parens() {
            Expr::Int(..) | Expr::Float(..) => true,
            Expr::Ident(name, _) if name == loop_var => true,
            Expr::Ident(name, _) => self
                .lookup(name)
                .is_some_and(|info| is_auto_vectorizable_scalar(&info.ty)),
            Expr::Unary(UnOp::Neg, inner, ..) => self.prove_auto_element_expr(
                inner,
                loop_var,
                element_type,
                extent,
                output,
                same_lane_output,
                inputs,
            ),
            Expr::Binary(op, left, right, ..)
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) =>
            {
                self.prove_auto_element_expr(
                    left,
                    loop_var,
                    element_type,
                    extent,
                    output,
                    same_lane_output,
                    inputs,
                ) && self.prove_auto_element_expr(
                    right,
                    loop_var,
                    element_type,
                    extent,
                    output,
                    same_lane_output,
                    inputs,
                )
            }
            Expr::Index { base, index, .. } => {
                let Expr::Ident(root, _) = base.without_parens() else {
                    return false;
                };
                if !matches!(index.without_parens(), Expr::Ident(name, _) if name == loop_var) {
                    return false;
                }
                let Some(info) = self.lookup(root) else {
                    return false;
                };
                match &info.ty {
                    Type::FixedList { elem, len } => {
                        let Some(length) = len.literal_value() else {
                            return false;
                        };
                        if elem.as_ref() != element_type || Some(length) != extent {
                            return false;
                        }
                    }
                    Type::List(elem) if elem.as_ref() == element_type => {}
                    _ => return false,
                }
                if !(same_lane_output && root == output) {
                    inputs.insert(root.clone());
                }
                true
            }
            _ => false,
        }
    }

    /// Attach a proof only after the ordinary function body has been checked.
    /// The TIR and every execution tier then receive the same proof record.
    pub(crate) fn check_kernel_marker(&mut self, f: &mut Func, owner_type: Option<&str>) {
        let Some(marker) = f.kernel.as_mut() else {
            return;
        };

        let failure = if owner_type.is_some() {
            Some(KernelFailure {
                obligation: "top-level declaration",
                span: marker.span,
            })
        } else if marker.mode != KernelMode::Parallel {
            Some(KernelFailure {
                obligation: "parallel execution mode",
                span: marker.span,
            })
        } else if f.is_unsafe {
            Some(KernelFailure {
                obligation: "safe memory boundary",
                span: f.unsafe_span.unwrap_or(marker.span),
            })
        } else if f.is_reactive || f.is_job || f.inline_foreign.is_some() {
            Some(KernelFailure {
                obligation: "effect-free function body",
                span: marker.span,
            })
        } else if !f.type_params.is_empty()
            || f.params
                .iter()
                .any(|param| param.variadic || !matches!(param.convention, AccessConvention::Read))
        {
            Some(KernelFailure {
                obligation: "read-only, monomorphic parameters",
                span: marker.span,
            })
        } else if f.params.iter().any(|param| {
            self.crossing_problem(&param.ty, SendCrossing::Kernel, true)
                .is_some()
        }) || f.return_type.as_ref().is_some_and(|ty| {
            self.crossing_problem(ty, SendCrossing::Kernel, true)
                .is_some()
        }) {
            Some(KernelFailure {
                obligation: "sendable kernel boundary values",
                span: marker.span,
            })
        } else if f
            .declared_effects
            .as_ref()
            .is_some_and(|effects| !effects.is_empty())
            || !self.fx_direct.is_empty()
            || !self.fx_edges.is_empty()
            || self.fx_maximal
        {
            Some(KernelFailure {
                obligation: "no reachable effects or opaque calls",
                span: marker.span,
            })
        } else {
            let mut names = f
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect::<std::collections::HashSet<_>>();
            let mut failure = None;
            for statement in &f.body {
                if failure.is_some() {
                    break;
                }
                if let Err(found) = self.prove_kernel_statement(statement, &mut names) {
                    failure = Some(found);
                }
            }
            failure
        };

        if let Some(failure) = failure {
            marker.proof = None;
            if failure.obligation == "sendable kernel boundary values" {
                if let Some((name, ty, problem)) = f.params.iter().find_map(|param| {
                    self.crossing_problem(&param.ty, SendCrossing::Kernel, true)
                        .map(|problem| (param.name.clone(), param.ty.clone(), problem))
                }) {
                    self.report_unsendable(&name, &ty, problem, SendCrossing::Kernel, marker.span);
                    return;
                }
                if let Some(ty) = f.return_type.as_ref() {
                    if let Some(problem) = self.crossing_problem(ty, SendCrossing::Kernel, true) {
                        self.report_unsendable(
                            "kernel result",
                            ty,
                            problem,
                            SendCrossing::Kernel,
                            marker.span,
                        );
                        return;
                    }
                }
            }
            self.diags
                .push(kernel_failure(failure.obligation, failure.span));
        } else {
            marker.proof = Some(KernelProof::parallel());
        }
    }

    fn prove_kernel_statement(
        &self,
        statement: &Stmt,
        names: &mut std::collections::HashSet<String>,
    ) -> Result<(), KernelFailure> {
        match statement {
            Stmt::Expr(expression) => self.prove_kernel_expr(expression, names),
            Stmt::Return(Some(expression), _) => self.prove_kernel_expr(expression, names),
            Stmt::Return(None, _) => Ok(()),
            Stmt::Val(binding) if !binding.mutable && binding.pattern.is_none() => {
                self.prove_kernel_binding(binding, names)
            }
            Stmt::Val(binding) => Err(KernelFailure {
                obligation: "immutable scalar locals",
                span: binding.name_span,
            }),
            _ => Err(KernelFailure {
                obligation: "uniform straight-line control flow",
                span: statement.span(),
            }),
        }
    }

    fn prove_kernel_binding(
        &self,
        binding: &Binding,
        names: &mut std::collections::HashSet<String>,
    ) -> Result<(), KernelFailure> {
        self.prove_kernel_expr(&binding.init, names)?;
        names.insert(binding.name.clone());
        Ok(())
    }

    fn prove_kernel_expr(
        &self,
        expression: &Expr,
        names: &std::collections::HashSet<String>,
    ) -> Result<(), KernelFailure> {
        match expression {
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) => Ok(()),
            Expr::Ident(name, _) if names.contains(name) => Ok(()),
            Expr::Ident(_, span) => Err(KernelFailure {
                obligation: "closed captures",
                span: *span,
            }),
            Expr::Unary(_, inner, _) | Expr::Paren(inner, _) => {
                self.prove_kernel_expr(inner, names)
            }
            Expr::Binary(_, left, right, _) => {
                self.prove_kernel_expr(left, names)?;
                self.prove_kernel_expr(right, names)
            }
            Expr::Field(base, _, _) => self.prove_kernel_expr(base, names),
            Expr::ListLit(items, _) => {
                for item in items {
                    self.prove_kernel_expr(item, names)?;
                }
                Ok(())
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } if matches!(receiver.as_ref(), Expr::Ident(alias, _) if self
                .core_imports
                .get(alias)
                .is_some_and(|module| module == "core.compute"))
                && SAFE_COMPUTE_CALLS.contains(&method.as_str()) =>
            {
                for argument in args {
                    self.prove_kernel_expr(&argument.expr, names)?;
                }
                Ok(())
            }
            Expr::MethodCall {
                method: _,
                method_span,
                ..
            } => Err(KernelFailure {
                obligation: "audited Core compute calls only",
                span: *method_span,
            }),
            Expr::Call(call) => Err(KernelFailure {
                obligation: "closed captures and provider calls",
                span: call.name_span,
            }),
            Expr::Index { span, .. } | Expr::Slice { span, .. } => Err(KernelFailure {
                obligation: "statically proved bounds",
                span: *span,
            }),
            Expr::Lambda(lambda) => Err(KernelFailure {
                obligation: "no captures",
                span: lambda.span,
            }),
            Expr::CallValue { span, .. } => Err(KernelFailure {
                obligation: "closed calls",
                span: *span,
            }),
            _ => Err(KernelFailure {
                obligation: "race-free straight-line body",
                span: expression.span(),
            }),
        }
    }
}

fn is_auto_vectorizable_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Float | Type::Float32)
}

fn kernel_failure(obligation: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1102",
        format!("`#Kernel(.parallel)` cannot cross its worker boundary: sema cannot prove {obligation}"),
        "a safe kernel crosses a worker boundary only after sema proves sendability, bounds, aliasing, captures, races, barriers, and control flow before TIR".to_string(),
        "use a read-only effect-free expression over checked Core compute operations, or move provider/raw code behind its typed `#Unsafe` boundary".to_string(),
        Some(span),
    )
}
