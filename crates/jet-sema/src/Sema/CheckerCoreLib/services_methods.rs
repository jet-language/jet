use super::alloc_ptrs::result_ty;
use crate::AST::{Expr, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use super::serde_diags::wrong_core_arity;

fn service_error_ty() -> Type {
    Type::Named("ServiceError".to_string())
}

fn service_result_ty(ok: Type) -> Type {
    result_ty(ok, service_error_ty())
}

fn literal_string(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    parts
        .iter()
        .map(|part| match part {
            StrPart::Lit(value) => Some(value.as_str()),
            StrPart::Interp(..) => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.concat())
}

fn literal_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value, ..) => Some(*value),
        Expr::Paren(inner, _) => literal_int(inner),
        Expr::Unary(crate::AST::UnOp::Neg, inner, _) => literal_int(inner)?.checked_neg(),
        _ => None,
    }
}

fn invalid_service_value(checker: &mut Checker<'_>, what: &str, fix: &str, span: Span) {
    checker.diags.push(Diagnostic::error(
        "E0112",
        format!("service {what} is outside the checked builder shape"),
        "service topology values must stay visible to sema before runtime construction".to_string(),
        fix.to_string(),
        Some(span),
    ));
}

impl<'a> Checker<'a> {
    fn service_method_arity(
        &mut self,
        api: &str,
        args: &mut Vec<crate::AST::CallArg>,
        expected: usize,
        span: Span,
    ) -> bool {
        if args.len() == expected {
            return true;
        }
        self.diags
            .push(wrong_core_arity(api, expected, args.len(), span));
        for arg in args.iter_mut() {
            self.infer(&mut arg.expr);
        }
        false
    }

    /// D-SERVICE1=D: the tree is one typed topology handle. Every operation
    /// below keeps its endpoint, delivery, state, and authority types visible
    /// to sema before lowering to the private Prelude adapter.
    pub(crate) fn check_service_tree_method(
        &mut self,
        method: &str,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Option<Type>> {
        let unit = Type::Named("Unit".to_string());
        let endpoint = Type::Named("ServiceEndpoint".to_string());
        let result_unit = || service_result_ty(unit.clone());
        let result_endpoint = || service_result_ty(endpoint.clone());
        let result_string = || service_result_ty(Type::String);
        let result_int = || service_result_ty(Type::Int);

        let ret = match method {
            "worker" => {
                if self.service_method_arity("ServiceTree.worker", args, 2, span) {
                    super::net_text_time::require_exact_labels(
                        "ServiceTree.worker",
                        args,
                        &[(1, "capacity")],
                        span,
                        &mut self.diags,
                    );
                    self.expect_core_arg("ServiceTree.worker", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg("ServiceTree.worker", 1, &Type::Int, &mut args[1]);
                    if let Some(name) = literal_string(&args[0].expr) {
                        if !jet_foundation::ServiceTree::valid_name(&name) {
                            invalid_service_value(
                                self,
                                "worker name",
                                "use a non-empty visible worker name of at most 256 bytes",
                                args[0].expr.span(),
                            );
                        }
                    }
                    if literal_int(&args[1].expr).is_some_and(|capacity| capacity <= 0) {
                        invalid_service_value(
                            self,
                            "worker capacity",
                            "use a positive bounded integer capacity",
                            args[1].expr.span(),
                        );
                    }
                }
                result_endpoint()
            }
            "set_restart" => {
                if self.service_method_arity("ServiceTree.set_restart", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceTree.set_restart",
                        0,
                        &Type::Named("ServiceRestart".to_string()),
                        &mut args[0],
                    );
                }
                result_unit()
            }
            "set_delivery" => {
                if self.service_method_arity("ServiceTree.set_delivery", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceTree.set_delivery",
                        0,
                        &Type::Named("ServiceDelivery".to_string()),
                        &mut args[0],
                    );
                }
                result_unit()
            }
            "start" | "stop" => {
                self.service_method_arity(&format!("ServiceTree.{method}"), args, 0, span);
                result_unit()
            }
            "send" => {
                if self.service_method_arity("ServiceTree.send", args, 2, span) {
                    self.expect_core_arg("ServiceTree.send", 0, &endpoint, &mut args[0]);
                    self.expect_core_arg("ServiceTree.send", 1, &Type::String, &mut args[1]);
                }
                result_unit()
            }
            "send_durable" => {
                if self.service_method_arity("ServiceTree.send_durable", args, 3, span) {
                    super::net_text_time::require_exact_labels(
                        "ServiceTree.send_durable",
                        args,
                        &[(2, "key")],
                        span,
                        &mut self.diags,
                    );
                    self.expect_core_arg("ServiceTree.send_durable", 0, &endpoint, &mut args[0]);
                    self.expect_core_arg(
                        "ServiceTree.send_durable",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        "ServiceTree.send_durable",
                        2,
                        &Type::String,
                        &mut args[2],
                    );
                }
                service_result_ty(Type::Named("ServiceReceipt".to_string()))
            }
            "receive" => {
                if self.service_method_arity("ServiceTree.receive", args, 1, span) {
                    self.expect_core_arg("ServiceTree.receive", 0, &endpoint, &mut args[0]);
                }
                result_string()
            }
            "mailbox_depth" | "restarts" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 0, &endpoint, &mut args[0]);
                }
                result_int()
            }
            "fail_worker" | "drain_worker" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 0, &endpoint, &mut args[0]);
                }
                result_unit()
            }
            "dead_letter_count" | "event_count" | "directory_generation" => {
                self.service_method_arity(&format!("ServiceTree.{method}"), args, 0, span);
                Type::Int
            }
            "drain_dead_letters" => {
                self.service_method_arity("ServiceTree.drain_dead_letters", args, 0, span);
                result_int()
            }
            "set_state_empty" => {
                self.service_method_arity("ServiceTree.set_state_empty", args, 0, span);
                result_unit()
            }
            "set_state_snapshot" | "set_state_event_log" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 4, span) {
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        0,
                        &Type::Named("ServiceStateStore".to_string()),
                        &mut args[0],
                    );
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 1, &Type::String, &mut args[1]);
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 2, &Type::Int, &mut args[2]);
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 3, &Type::String, &mut args[3]);
                }
                result_unit()
            }
            "commit_snapshot" | "append_event" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(&format!("ServiceTree.{method}"), 0, &Type::String, &mut args[0]);
                }
                result_unit()
            }
            "restore_snapshot" => {
                self.service_method_arity("ServiceTree.restore_snapshot", args, 0, span);
                result_string()
            }
            "replay_events" | "observe" | "show" => {
                self.service_method_arity(&format!("ServiceTree.{method}"), args, 0, span);
                Type::String
            }
            "workflow_start" => {
                if self.service_method_arity("ServiceTree.workflow_start", args, 2, span) {
                    self.expect_core_arg("ServiceTree.workflow_start", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg("ServiceTree.workflow_start", 1, &Type::Int, &mut args[1]);
                }
                result_int()
            }
            "workflow_step" => {
                if self.service_method_arity("ServiceTree.workflow_step", args, 2, span) {
                    self.expect_core_arg("ServiceTree.workflow_step", 0, &Type::Int, &mut args[0]);
                    self.expect_core_arg("ServiceTree.workflow_step", 1, &Type::String, &mut args[1]);
                }
                result_unit()
            }
            "workflow_history" => {
                if self.service_method_arity("ServiceTree.workflow_history", args, 1, span) {
                    self.expect_core_arg("ServiceTree.workflow_history", 0, &Type::Int, &mut args[0]);
                }
                result_string()
            }
            "directory_register" => {
                if self.service_method_arity("ServiceTree.directory_register", args, 2, span) {
                    self.expect_core_arg("ServiceTree.directory_register", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg("ServiceTree.directory_register", 1, &endpoint, &mut args[1]);
                }
                result_unit()
            }
            "directory_resolve" => {
                if self.service_method_arity("ServiceTree.directory_resolve", args, 1, span) {
                    self.expect_core_arg("ServiceTree.directory_resolve", 0, &Type::String, &mut args[0]);
                }
                result_endpoint()
            }
            "handoff_generation" | "rollback_generation" | "chaos_fail" => {
                self.service_method_arity(&format!("ServiceTree.{method}"), args, 0, span);
                result_int()
            }
            "upgrade_receipt" => {
                self.service_method_arity("ServiceTree.upgrade_receipt", args, 0, span);
                service_result_ty(Type::Named("ServiceUpgradeReceipt".to_string()))
            }
            _ => return None,
        };
        Some(Some(ret))
    }

    pub(crate) fn check_service_endpoint_method(
        &mut self,
        method: &str,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Option<Type>> {
        let ret = match method {
            "send" => {
                if self.service_method_arity("ServiceEndpoint.send", args, 1, span) {
                    self.expect_core_arg("ServiceEndpoint.send", 0, &Type::String, &mut args[0]);
                }
                service_result_ty(Type::Named("Unit".to_string()))
            }
            "receive" => {
                self.service_method_arity("ServiceEndpoint.receive", args, 0, span);
                service_result_ty(Type::String)
            }
            "show" => {
                self.service_method_arity("ServiceEndpoint.show", args, 0, span);
                Type::String
            }
            _ => return None,
        };
        Some(Some(ret))
    }

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
                if method == "commit" {
                    Some(Some(result_ty(
                        Type::Named("Unit".to_string()),
                        Type::Named("ServiceError".to_string()),
                    )))
                } else {
                    Some(Some(result_ty(
                        Type::Named("ServiceReceipt".to_string()),
                        Type::Named("ServiceError".to_string()),
                    )))
                }
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
