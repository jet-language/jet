use super::alloc_ptrs::result_ty;
use crate::AST::Type;
use crate::Diagnostics::Span;
use crate::Sema::Checker;
use super::serde_diags::wrong_core_arity;

impl<'a> Checker<'a> {
    /// D-SERVICE-AUTHORITY1: durable delivery is a method on the authority
    /// scope. The key is explicit so retries and duplicate sends cannot hide
    /// behind a process-local mailbox.
    pub(crate) fn check_service_runtime_method(
        &mut self,
        method: &str,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Option<Type>> {
        match method {
            "send" => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("ServiceRuntime.send", 3, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                } else {
                    super::net_text_time::require_exact_labels(
                        "ServiceRuntime.send",
                        args,
                        &[(2, "key")],
                        span,
                        &mut self.diags,
                    );
                    self.expect_core_arg(
                        "ServiceRuntime.send",
                        0,
                        &Type::Named("ServiceEndpoint".to_string()),
                        &mut args[0],
                    );
                    self.expect_core_arg("ServiceRuntime.send", 1, &Type::String, &mut args[1]);
                    self.expect_core_arg("ServiceRuntime.send", 2, &Type::String, &mut args[2]);
                }
                Some(Some(result_ty(
                    Type::Named("ServiceReceipt".to_string()),
                    Type::Named("ServiceError".to_string()),
                )))
            }
            "retry" | "dead_letter" | "commit" => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity(method, 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                } else {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                }
                Some(Some(result_ty(
                    Type::Named("ServiceReceipt".to_string()),
                    Type::Named("ServiceError".to_string()),
                )))
            }
            "retain" => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("ServiceRuntime.retain", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                } else {
                    self.expect_core_arg("ServiceRuntime.retain", 0, &Type::String, &mut args[0]);
                }
                Some(Some(result_ty(
                    Type::Named("ServiceReceipt".to_string()),
                    Type::Named("ServiceError".to_string()),
                )))
            }
            _ => None,
        }
    }
}

pub fn service_runtime_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "send" | "retry" | "dead_letter" | "retain" => Some(result_ty(
            Type::Named("ServiceReceipt".to_string()),
            Type::Named("ServiceError".to_string()),
        )),
        "commit" => Some(result_ty(
            Type::Named("Unit".to_string()),
            Type::Named("ServiceError".to_string()),
        )),
        _ => None,
    }
}
