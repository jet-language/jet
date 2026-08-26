use super::alloc_ptrs::result_ty;
use super::serde_diags::wrong_core_arity;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Effects::{Effect, EffectSet, EffectSummary};
use crate::AST::{Expr, Item, ProgramBundle, StrPart, Type};
use std::collections::{HashMap, HashSet};

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

fn literal_string_list(expr: &Expr) -> Option<Vec<String>> {
    let Expr::ListLit(items, _) = expr else {
        return None;
    };
    items.iter().map(literal_string).collect()
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

fn require_service_name(checker: &mut Checker<'_>, what: &str, expr: &Expr) {
    match literal_string(expr) {
        Some(value) if jet_foundation::ServiceTree::valid_name(&value) => {}
        Some(_) | None => invalid_service_value(
            checker,
            what,
            "use a literal non-empty visible topology name of at most 256 bytes",
            expr.span(),
        ),
    }
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
        let workflow = || Type::Named("ServiceWorkflow".to_string());
        let workflow_id =
            || Type::Union(vec![Type::Int, Type::Named("ServiceWorkflow".to_string())]);
        let task_outcome = || Type::Named("TaskOutcome".to_string());
        let task_status = || Type::Named("TaskStatus".to_string());

        let ret = match method {
            "worker" => {
                if self.service_method_arity("ServiceTree.worker", args, 3, span) {
                    super::net_text_time::require_exact_labels(
                        "ServiceTree.worker",
                        args,
                        &[(2, "capacity")],
                        span,
                        &mut self.diags,
                    );
                    self.expect_core_arg("ServiceTree.worker", 0, &Type::String, &mut args[0]);
                    let handler_ty = Type::Fn {
                        params: Vec::new(),
                        ret: None,
                        effect_bound: None,
                        param_contract: None,
                        call_metadata: None,
                        return_view_provenance: None,
                    };
                    self.expect_core_arg("ServiceTree.worker", 1, &handler_ty, &mut args[1]);
                    if !matches!(args[1].expr, Expr::Ident(..)) {
                        invalid_service_value(
                            self,
                            "worker handler",
                            "pass an ordinary named function value",
                            args[1].expr.span(),
                        );
                    } else if let Expr::Ident(handler, handler_span) = &args[1].expr {
                        // A service owns the handler after this builder call. A
                        // local binding could carry a stack lifetime, so only
                        // a registered top-level function may cross the
                        // topology boundary.
                        if self.lookup(handler).is_some() || !self.funcs.contains_key(handler) {
                            invalid_service_value(
                                self,
                                "worker handler lifetime",
                                "pass a top-level function, not a local or captured function value",
                                *handler_span,
                            );
                        }
                    }
                    self.expect_core_arg("ServiceTree.worker", 2, &Type::Int, &mut args[2]);
                    if let Some(name) = literal_string(&args[0].expr) {
                        if !jet_foundation::ServiceTree::valid_name(&name) {
                            invalid_service_value(
                                self,
                                "worker name",
                                "use a non-empty visible worker name of at most 256 bytes",
                                args[0].expr.span(),
                            );
                        }
                    } else {
                        invalid_service_value(
                            self,
                            "worker name",
                            "use a literal visible worker name of at most 256 bytes",
                            args[0].expr.span(),
                        );
                    }
                    if literal_int(&args[2].expr).is_some_and(|capacity| capacity <= 0) {
                        invalid_service_value(
                            self,
                            "worker capacity",
                            "use a positive bounded integer capacity",
                            args[2].expr.span(),
                        );
                    }
                }
                result_endpoint()
            }
            "group" => {
                if self.service_method_arity("ServiceTree.group", args, 2, span) {
                    self.expect_core_arg("ServiceTree.group", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(
                        "ServiceTree.group",
                        1,
                        &Type::List(Box::new(Type::String)),
                        &mut args[1],
                    );
                    require_service_name(self, "group name", &args[0].expr);
                    match literal_string_list(&args[1].expr) {
                        Some(workers)
                            if !workers.is_empty()
                                && workers
                                    .iter()
                                    .all(|name| jet_foundation::ServiceTree::valid_name(name)) => {}
                        Some(_) | None => invalid_service_value(
                            self,
                            "group worker topology",
                            "use a non-empty literal list of visible worker names",
                            args[1].expr.span(),
                        ),
                    }
                }
                result_unit()
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
                service_result_ty(Type::Named("Delivery".to_string()))
            }
            "receive" => {
                if self.service_method_arity("ServiceTree.receive", args, 1, span) {
                    self.expect_core_arg("ServiceTree.receive", 0, &endpoint, &mut args[0]);
                }
                result_string()
            }
            "mailbox_depth" | "restarts" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        0,
                        &endpoint,
                        &mut args[0],
                    );
                }
                result_int()
            }
            "fail_worker" | "drain_worker" | "partition_worker" | "reconcile_worker" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        0,
                        &endpoint,
                        &mut args[0],
                    );
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
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        2,
                        &Type::Int,
                        &mut args[2],
                    );
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        3,
                        &Type::String,
                        &mut args[3],
                    );
                }
                result_unit()
            }
            "commit_snapshot" | "append_event" => {
                if self.service_method_arity(&format!("ServiceTree.{method}"), args, 1, span) {
                    self.expect_core_arg(
                        &format!("ServiceTree.{method}"),
                        0,
                        &Type::String,
                        &mut args[0],
                    );
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
                    self.expect_core_arg(
                        "ServiceTree.workflow_start",
                        0,
                        &Type::String,
                        &mut args[0],
                    );
                    self.expect_core_arg("ServiceTree.workflow_start", 1, &Type::Int, &mut args[1]);
                    require_service_name(self, "workflow name", &args[0].expr);
                    if literal_int(&args[1].expr).is_some_and(|version| version <= 0) {
                        invalid_service_value(
                            self,
                            "workflow version",
                            "use a positive workflow version",
                            args[1].expr.span(),
                        );
                    }
                }
                service_result_ty(workflow())
            }
            "workflow_step" => {
                if self.service_method_arity("ServiceTree.workflow_step", args, 2, span) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_step",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_step",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                }
                result_unit()
            }
            "workflow_activity" => {
                if self.service_method_arity("ServiceTree.workflow_activity", args, 4, span) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity",
                        2,
                        &Type::String,
                        &mut args[2],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity",
                        3,
                        &Type::Int,
                        &mut args[3],
                    );
                }
                service_result_ty(task_status())
            }
            "workflow_activity_retry" => {
                if self.service_method_arity("ServiceTree.workflow_activity_retry", args, 3, span) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_retry",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_retry",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_retry",
                        2,
                        &task_outcome(),
                        &mut args[2],
                    );
                }
                service_result_ty(task_status())
            }
            "workflow_activity_complete" => {
                if self.service_method_arity(
                    "ServiceTree.workflow_activity_complete",
                    args,
                    3,
                    span,
                ) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_complete",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_complete",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        "ServiceTree.workflow_activity_complete",
                        2,
                        &task_outcome(),
                        &mut args[2],
                    );
                }
                service_result_ty(task_outcome())
            }
            "workflow_history" => {
                if self.service_method_arity("ServiceTree.workflow_history", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_history",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                }
                result_string()
            }
            "workflow_outcome" => {
                if self.service_method_arity("ServiceTree.workflow_outcome", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceTree.workflow_outcome",
                        0,
                        &workflow_id(),
                        &mut args[0],
                    );
                }
                service_result_ty(task_outcome())
            }
            "directory_register" => {
                if self.service_method_arity("ServiceTree.directory_register", args, 2, span) {
                    self.expect_core_arg(
                        "ServiceTree.directory_register",
                        0,
                        &Type::String,
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceTree.directory_register",
                        1,
                        &endpoint,
                        &mut args[1],
                    );
                    require_service_name(self, "directory name", &args[0].expr);
                }
                result_unit()
            }
            "directory_resolve" => {
                if self.service_method_arity("ServiceTree.directory_resolve", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceTree.directory_resolve",
                        0,
                        &Type::String,
                        &mut args[0],
                    );
                    require_service_name(self, "directory name", &args[0].expr);
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

    /// D-SERVICE-WORKFLOW1=D: the workflow handle owns the three recorded
    /// wait points. Their ordinary typed signatures keep the effect boundary
    /// visible to sema; the Prelude owns replay and cancellation behavior.
    pub(crate) fn check_service_workflow_method(
        &mut self,
        method: &str,
        args: &mut Vec<crate::AST::CallArg>,
        span: Span,
    ) -> Option<Option<Type>> {
        let result_string = || service_result_ty(Type::String);
        let result_strings = || service_result_ty(Type::List(Box::new(Type::String)));
        let ret = match method {
            "sleep" => {
                if self.service_method_arity("ServiceWorkflow.sleep", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceWorkflow.sleep",
                        0,
                        &Type::Named("Duration".to_string()),
                        &mut args[0],
                    );
                }
                service_result_ty(Type::Named("Unit".to_string()))
            }
            "activity" => {
                if self.service_method_arity("ServiceWorkflow.activity", args, 2, span) {
                    self.expect_core_arg(
                        "ServiceWorkflow.activity",
                        0,
                        &Type::String,
                        &mut args[0],
                    );
                    self.expect_core_arg(
                        "ServiceWorkflow.activity",
                        1,
                        &Type::String,
                        &mut args[1],
                    );
                }
                result_string()
            }
            "all" => {
                if self.service_method_arity("ServiceWorkflow.all", args, 1, span) {
                    self.expect_core_arg(
                        "ServiceWorkflow.all",
                        0,
                        &Type::List(Box::new(Type::String)),
                        &mut args[0],
                    );
                }
                result_strings()
            }
            _ => return None,
        };
        // D-SERVICE-WORKFLOW1=D: recorded waits are still effects at the
        // function boundary. Keep this fact in sema, beside the typed method
        // contract, so purity and effect-row checks do not inspect workflow
        // source text or duplicate the Prelude's replay policy.
        let effect = match method {
            "sleep" => Effect::Time,
            "activity" | "all" => Effect::IO,
            _ => unreachable!("unknown workflow method reached effect check"),
        };
        self.record_effect(effect.name(), span);
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
                    Type::Named("Delivery".to_string()),
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
                    let expected = Type::Named("Delivery".to_string());
                    self.expect_core_arg(method, 0, &expected, &mut args[0]);
                    args[0].convention = crate::AST::AccessConvention::Move;
                }
                if method == "commit" {
                    Some(Some(result_ty(
                        Type::Named("Unit".to_string()),
                        Type::Named("ServiceError".to_string()),
                    )))
                } else {
                    Some(Some(result_ty(
                        Type::Named("Delivery".to_string()),
                        Type::Named("ServiceError".to_string()),
                    )))
                }
            }
            "retain" => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(
                        "ServiceRuntime.retain",
                        1,
                        args.len(),
                        span,
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                } else {
                    self.expect_core_arg(
                        "ServiceRuntime.retain",
                        0,
                        &Type::Named("Delivery".to_string()),
                        &mut args[0],
                    );
                    args[0].convention = crate::AST::AccessConvention::Move;
                }
                Some(Some(result_ty(
                    Type::Named("Delivery".to_string()),
                    Type::Named("ServiceError".to_string()),
                )))
            }
            _ => None,
        }
    }

/// D-SERVICE-RECEIPT2=A: one Delivery handle owns every observation and
/// control operation. Observation does not cancel accepted work.
pub(crate) fn check_service_delivery_method(
    &mut self,
    method: &str,
    args: &mut Vec<crate::AST::CallArg>,
    span: Span,
) -> Option<Option<Type>> {
    let result = |ty| {
        Some(Some(result_ty(
            ty,
            Type::Named("ServiceError".to_string()),
        )))
    };
    match method {
        "wait" | "status" => {
            self.service_method_arity(&format!("Delivery.{method}"), args, 0, span);
            result(Type::Named("DeliveryState".to_string()))
        }
        "retry" | "cancel" => {
            self.service_method_arity(&format!("Delivery.{method}"), args, 0, span);
            result(Type::Named("Delivery".to_string()))
        }
        "receipt" => {
            self.service_method_arity("Delivery.receipt", args, 0, span);
            result(Type::Named("DeliveryReceipt".to_string()))
        }
        "events" => {
            self.service_method_arity("Delivery.events", args, 0, span);
            result(Type::List(Box::new(Type::Named("DeliveryEvent".to_string()))))
        }
        _ => None,
    }
}

}

pub fn service_runtime_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "send" | "retry" | "dead_letter" | "retain" => Some(result_ty(
            Type::Named("Delivery".to_string()),
            Type::Named("ServiceError".to_string()),
        )),
        "commit" => Some(result_ty(
            Type::Named("Unit".to_string()),
            Type::Named("ServiceError".to_string()),
        )),
        _ => None,
    }
}

/// D-SERVICE1=D: validate the facts that need the complete checked function
/// graph. The method checker owns local shape and type checks; this pass owns
/// the service-wide effect, lifetime, and cycle proof before lowering.
pub(crate) fn validate_service_handlers(
    bundle: &mut ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    solved: &HashMap<String, EffectSet>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut workers = Vec::new();
    for module in &mut bundle.modules {
        let module_alias = module.alias.clone();
        for item in &mut module.items {
            let Item::Func(function) = item else { continue };
            for statement in &mut function.body {
                statement.for_each_expr_mut(|expression| {
                    let Expr::MethodCall { method, args, .. } = expression else {
                        return;
                    };
                    if method != "worker" || args.len() < 2 {
                        return;
                    }
                    let Expr::Ident(handler, _) = &args[1].expr else {
                        return;
                    };
                    let Some(summary_key) =
                        service_summary_key(&module_alias, handler, summaries)
                    else {
                        // The local method check reports non-top-level values;
                        // this catches extern/unknown names whose body cannot
                        // supply a closed effect graph to the supervisor.
                        diags.push(Diagnostic::error(
                            "E0112",
                            format!("service worker `{handler}` has no checked handler body"),
                            "a service worker must have a sema-known function body for effects and lifetime checking"
                                .to_string(),
                            "pass a top-level Jet function with a checked body".to_string(),
                            Some(args[1].expr.span()),
                        ));
                        return;
                    };
                    workers.push(ServiceWorker {
                        key: summary_key,
                        handler: handler.clone(),
                        span: expression.span(),
                    });
                });
            }
        }
    }

    let canonical_summaries = summaries
        .iter()
        .filter(|(key, _)| key.contains("::"))
        .map(|(key, summary)| (key.clone(), summary))
        .collect::<HashMap<_, _>>();

    let mut graph = HashMap::<String, Vec<String>>::new();
    for (key, summary) in &canonical_summaries {
        let mut edges = Vec::new();
        for edge in &summary.edges {
            let Some(target) = service_edge_key(key, edge, &canonical_summaries) else {
                continue;
            };
            if !edges.contains(&target) {
                edges.push(target);
            }
        }
        graph.insert(key.clone(), edges);
    }

    let mut reported_effects = HashSet::new();
    for worker in &workers {
        let Some(summary) = canonical_summaries.get(&worker.key) else {
            continue;
        };
        let effect_projection_missing =
            !summary.direct.is_empty() && !solved.contains_key(&worker.key);
        if (summary.maximal || effect_projection_missing)
            && reported_effects.insert(worker.key.clone())
        {
            diags.push(Diagnostic::error(
                "E0112",
                format!("service worker `{}` has an open effect row", worker.handler),
                "a supervisor must know every effect a promoted worker can reach".to_string(),
                "close the function's effects with a checked body or keep it out of the service tree"
                    .to_string(),
                Some(worker.span),
            ));
        }
    }

    let mut reported_cycles = HashSet::new();
    for worker in &workers {
        if reported_cycles.contains(&worker.key) {
            continue;
        }
        let Some(summary) = canonical_summaries.get(&worker.key) else {
            continue;
        };
        if worker_reaches_worker(&worker.key, &worker.key, summary, &graph) {
            reported_cycles.insert(worker.key.clone());
            diags.push(Diagnostic::error(
                "E0112",
                format!("service worker `{}` is part of a handler cycle", worker.handler),
                "a promoted worker graph must have an acyclic handler topology".to_string(),
                "remove the recursive worker dependency or keep the functions outside this service tree"
                    .to_string(),
                Some(worker.span),
            ));
        }
    }
}

struct ServiceWorker {
    key: String,
    handler: String,
    span: Span,
}

fn service_summary_key(
    module_alias: &str,
    handler: &str,
    summaries: &HashMap<String, EffectSummary>,
) -> Option<String> {
    let local = format!("{module_alias}::{handler}");
    if summaries.contains_key(&local) {
        return Some(local);
    }
    let suffix = format!("::{handler}");
    let matches = summaries
        .keys()
        .filter(|key| key.ends_with(&suffix))
        .filter(|key| key.contains("::"))
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.into_iter().next().unwrap())
}

fn service_edge_key(
    current: &str,
    edge: &str,
    summaries: &HashMap<String, &EffectSummary>,
) -> Option<String> {
    if edge == "__jet_panic__" {
        return None;
    }
    if summaries.contains_key(edge) {
        return Some(edge.to_string());
    }
    let module = current.split_once("::")?.0;
    let local = format!("{module}::{edge}");
    if summaries.contains_key(&local) {
        return Some(local);
    }
    let suffix = format!("::{edge}");
    let matches = summaries
        .keys()
        .filter(|key| key.ends_with(&suffix))
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.into_iter().next().unwrap())
}

fn worker_reaches_worker(
    start: &str,
    target: &str,
    start_summary: &EffectSummary,
    graph: &HashMap<String, Vec<String>>,
) -> bool {
    let mut pending = graph
        .get(start)
        .cloned()
        .unwrap_or_else(|| start_summary.edges.iter().cloned().collect());
    let mut visited = HashSet::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.insert(node.clone()) {
            continue;
        }
        if let Some(edges) = graph.get(&node) {
            pending.extend(edges.iter().cloned());
        }
    }
    false
}
