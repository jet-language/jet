//! D-COMPUTE-KERNEL-SURFACE1=B: the conservative safe-kernel proof.
//!
//! A marker is not a proof. This pass accepts only a read-only, effect-free
//! expression kernel whose safety obligations are either structural or
//! delegated to the checked Core compute family. More general loops, indexed
//! writes, captures, barriers, and provider calls remain rejected until their
//! proof facts exist.

use crate::AST::{AccessConvention, Binding, Expr, Func, KernelMode, KernelProof, Stmt};
use crate::Diagnostics::{Diagnostic, Span};

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
        } else if f.is_reactive || f.is_task || f.inline_foreign.is_some() {
            Some(KernelFailure {
                obligation: "effect-free function body",
                span: marker.span,
            })
        } else if !f.type_params.is_empty()
            || f.params.iter().any(|param| {
                param.variadic || !matches!(param.convention, AccessConvention::Read)
            })
        {
            Some(KernelFailure {
                obligation: "read-only, monomorphic parameters",
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
            self.diags.push(kernel_failure(failure.obligation, failure.span));
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
            Expr::MethodCall { method: _, method_span, .. } => Err(KernelFailure {
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

fn kernel_failure(obligation: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1130",
        format!("safe `#Kernel(.parallel)` cannot prove {obligation}"),
        "a safe kernel needs a sema proof for bounds, aliasing, captures, races, barriers, and control flow before TIR".to_string(),
        "use a read-only effect-free expression over checked Core compute operations, or move provider/raw code behind its typed `#Unsafe` boundary".to_string(),
        Some(span),
    )
}
