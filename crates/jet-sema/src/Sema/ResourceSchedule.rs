//! Sema projection for the canonical frame-resource scheduler.
//!
//! This module consumes checked call signatures and the already elaborated
//! argument conventions.  It does not infer a second access list and it does
//! not alter the existing GameScene callback execution path.

use super::Checker;
use crate::AST::{AccessConvention, CallArg, Expr, Lambda, LambdaBody, Stmt};
use crate::Diagnostics::Diagnostic;
use jet_foundation::Facts::{
    DerivationIdentity, DerivationMethod, DerivationPayload, DerivationRecord,
};
use jet_foundation::ResourceSchedule::{
    derive_frame_schedule, derive_frame_schedule_with_options, JetFrameCompletion,
    JetFrameOperation, JetFrameScheduleOptions, JetResourceAccess, JetResourceAccessMode,
    JetResourceIdentity,
};
use jet_foundation::TargetMachine::{TargetHardwareFacts, TargetMachine};
use jet_foundation::Syntax::{
    CoreCallCompletionKind, CoreCallRecord, CoreCallResourceAccessMode,
};
use std::collections::BTreeMap;

#[derive(Clone)]
enum FrameHardwareCall {
    Register {
        profile: String,
        block: String,
        register: String,
        method: String,
        mode: JetResourceAccessMode,
    },
    DmaStart {
        profile: String,
        channel: String,
        buffer: String,
        token: String,
    },
    DmaWait {
        transfer: String,
    },
}

#[derive(Clone)]
struct FrameCall {
    name: String,
    args: Vec<CallArg>,
    core: Option<&'static CoreCallRecord>,
    hardware: Option<FrameHardwareCall>,
}

/// Check one `GameScene.on_frame` callback against the canonical schedule
/// engine.  The serial source-order plan is the reference behavior; errors are
/// precise refusals from checked facts, never guessed aliases or devices.
pub(crate) fn check_game_frame_lambda(checker: &mut Checker<'_>, expression: &mut Expr) {
    let Expr::Lambda(lambda) = expression else {
        return;
    };
    let mut async_refusal = None;
    let mut calls = Vec::new();
    let mut transfer_starts = BTreeMap::new();
    collect_lambda_calls(checker, lambda, &mut calls, &mut transfer_starts);
    calls.sort_by_key(|(start, _)| *start);

    let operations = calls
        .into_iter()
        .filter_map(|(start, call)| {
            let signature = call_signature(checker, &call.name);
            let mut hardware_completion = None;
            let mut operation_name = call.name.clone();
            let accesses = if let Some(core) = call.core {
                core.frame_accesses()
                    .iter()
                    .filter_map(|access| {
                        let argument = call.args.get(access.argument)?;
                        Some(JetResourceAccess {
                            resource: resource_identity(&argument.expr)?,
                            mode: core_access_mode(access.mode),
                            region: jet_foundation::ResourceSchedule::JetResourceRegion::Whole,
                            layout: None,
                            temporary: false,
                        })
                    })
                    .collect::<Vec<_>>()
            } else if let Some(hardware) = call.hardware.as_ref() {
                match hardware {
                    FrameHardwareCall::Register {
                        profile,
                        block,
                        register,
                        mode,
                        ..
                    } => {
                        hardware_completion =
                            Some((JetFrameCompletion::Synchronous, None));
                        vec![JetResourceAccess::named(
                            format!("mmio::{profile}::{block}::{register}"),
                            *mode,
                        )]
                    }
                    FrameHardwareCall::DmaStart {
                        profile: _,
                        channel: _,
                        buffer,
                        token,
                    } => {
                        hardware_completion = Some((
                            JetFrameCompletion::Pending {
                                token: token.clone(),
                            },
                            Some("jet_dma_transfer".to_string()),
                        ));
                        vec![JetResourceAccess::named(
                            buffer.clone(),
                            JetResourceAccessMode::Move,
                        )]
                    }
                    FrameHardwareCall::DmaWait { transfer } => {
                        let Some((token, start_profile, channel, buffer)) =
                            transfer_starts.get(transfer)
                        else {
                            async_refusal = Some(format!(
                                "hardware DMA wait `{transfer}` has no checked start binding"
                            ));
                            return None;
                        };
                        operation_name =
                            format!("hardware.dma.wait.{start_profile}.{channel}");
                        hardware_completion = Some((
                            JetFrameCompletion::Event {
                                token: token.clone(),
                            },
                            Some("jet_dma_transfer".to_string()),
                        ));
                        vec![JetResourceAccess::named(
                            buffer.clone(),
                            JetResourceAccessMode::Move,
                        )]
                    }
                }
            } else {
                call.args
                    .iter()
                    .enumerate()
                    .filter_map(|(index, argument)| {
                        let identity = resource_identity(&argument.expr)?;
                        let convention = signature
                            .as_ref()
                            .and_then(|signature| signature.params.get(index))
                            .map(|(convention, _)| *convention)
                            .unwrap_or(argument.convention);
                        Some(JetResourceAccess {
                            resource: identity,
                            mode: access_mode(convention),
                            region: jet_foundation::ResourceSchedule::JetResourceRegion::Whole,
                            layout: None,
                            temporary: false,
                        })
                    })
                    .collect::<Vec<_>>()
            };

            let (completion, completion_provider) =
                if let Some((completion, provider)) = hardware_completion {
                    (completion, provider)
                } else if let Some(core) = call.core {
                    match core.frame_completion() {
                        Some(metadata) => {
                            let completion = match metadata.kind {
                                CoreCallCompletionKind::Synchronous => JetFrameCompletion::Synchronous,
                                CoreCallCompletionKind::Pending => {
                                    let Some(token) = metadata.token else {
                                        async_refusal = Some(format!(
                                            "core frame primitive `{}` has pending completion without a checked token",
                                            call.name
                                        ));
                                        return None;
                                    };
                                    JetFrameCompletion::Pending {
                                        token: token.to_string(),
                                    }
                                }
                                CoreCallCompletionKind::Event => {
                                    let Some(token) = metadata.token else {
                                        async_refusal = Some(format!(
                                            "core frame primitive `{}` has event completion without a checked token",
                                            call.name
                                        ));
                                        return None;
                                    };
                                    JetFrameCompletion::Event {
                                        token: token.to_string(),
                                    }
                                }
                            };
                            (completion, Some(metadata.provider.to_string()))
                        }
                        None => {
                            if !accesses.is_empty() {
                                async_refusal = Some(format!(
                                    "core frame primitive `{}` has resource accesses but no checked completion metadata",
                                    call.name
                                ));
                                return None;
                            }
                            (JetFrameCompletion::Synchronous, None)
                        }
                    }
                } else {
                    if signature
                        .as_ref()
                        .is_none_or(|signature| signature.is_extern)
                        && !accesses.is_empty()
                    {
                        async_refusal = Some(format!(
                            "foreign or unknown frame primitive `{}` has resource accesses but no checked completion token",
                            call.name
                        ));
                        return None;
                    }
                    (JetFrameCompletion::Synchronous, None)
                };

            // Unknown, hardware, or impure calls remain explicit source-order
            // operations. Dropping one would let a visible effect move around
            // a checked resource access merely because its signature is absent.
            let external_effect =
                call.hardware.is_some() || signature.as_ref().is_none_or(|signature| !signature.is_pure);
            (external_effect || !accesses.is_empty()).then(|| JetFrameOperation {
                name: operation_name,
                source_index: start,
                accesses,
                completion,
                completion_provider,
                external_effect,
            })
        })
        .collect::<Vec<_>>();

    if let Some(detail) = async_refusal {
        checker.diags.push(Diagnostic::from_row(
            "E2967",
            &[("detail", detail.as_str())],
            Some(lambda.span),
        ));
        return;
    }
    if operations.is_empty() {
        return;
    }
    match derive_frame_schedule(&operations) {
        Ok(schedule) => {
            let derivation = DerivationRecord::new(
                format!("{}::on_frame@{}", checker.module_path, lambda.span.start),
                "checked frame resource schedule",
                "jet-sema",
                DerivationMethod::StaticDerivation,
                "checked-frame-accesses",
                operations.iter().map(|operation| {
                    format!("{}@{}", operation.name, operation.source_index)
                }),
                DerivationIdentity::new(
                    checker.module_path,
                    String::new(),
                    String::new(),
                    checker.source,
                ),
            )
            .with_payload(DerivationPayload::FrameSchedule(schedule.clone()));
            lambda.meta.frame_schedule_explanation = Some(schedule.explain());
            lambda.meta.frame_schedule = Some(schedule);
            lambda.meta.frame_schedule_derivation = Some(derivation.reference());
        }
        Err(error) => {
            let detail = error.to_string();
            checker.diags.push(Diagnostic::from_row(
                "E2967",
                &[("detail", detail.as_str())],
                Some(lambda.span),
            ));
            return;
        }
    }
    // Each callback is one candidate system. Only cross-callback hazards
    // request the explicit parallel proof; calls inside one callback retain
    // their source-order reference schedule.
    let system = JetFrameOperation::new(
        format!("on_frame@{}", lambda.span.start),
        lambda.span.start,
    )
    .with_accesses(
        operations
            .iter()
            .flat_map(|operation| operation.accesses.iter().cloned()),
    );
    checker.frame_schedule_systems.push(system);
    if checker.frame_schedule_systems.len() > 1 {
        if let Err(error) = derive_frame_schedule_with_options(
            &checker.frame_schedule_systems,
            JetFrameScheduleOptions::parallel(),
        ) {
            let detail = error.to_string();
            checker.diags.push(Diagnostic::from_row(
                "E2967",
                &[("detail", detail.as_str())],
                Some(lambda.span),
            ));
        }
    }
}

fn call_signature(checker: &Checker<'_>, name: &str) -> Option<crate::AST::FuncSig> {
    checker.funcs.get(name).cloned().or_else(|| {
        checker
            .modules
            .and_then(|modules| modules.iter().find_map(|module| module.funcs.get(name)))
            .cloned()
    })
}

fn record_dma_start(
    checker: &Checker<'_>,
    name: &str,
    init: &Expr,
    transfer_starts: &mut BTreeMap<String, (String, String, String, String)>,
) {
    if let Some(FrameHardwareCall::DmaStart {
        profile,
        channel,
        buffer,
        token,
    }) = hardware_frame_call(checker, init)
    {
        transfer_starts.insert(
            name.to_string(),
            (token, profile, channel, buffer),
        );
    }
}

fn collect_dma_starts(
    checker: &Checker<'_>,
    statements: &[Stmt],
    transfer_starts: &mut BTreeMap<String, (String, String, String, String)>,
) {
    for statement in statements {
        match statement {
            Stmt::Val(binding) => {
                record_dma_start(checker, &binding.name, &binding.init, transfer_starts);
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::AuthorityScope { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => {
                collect_dma_starts(checker, body, transfer_starts);
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    collect_dma_starts(checker, &arm.body, transfer_starts);
                }
                if let Some(body) = else_body {
                    collect_dma_starts(checker, body, transfer_starts);
                }
            }
            Stmt::CountedLoop {
                init, step, body, ..
            } => {
                record_dma_start(checker, &init.name, &init.init, transfer_starts);
                if let Some(step) = step.as_deref() {
                    collect_dma_starts(checker, std::slice::from_ref(step), transfer_starts);
                }
                collect_dma_starts(checker, body, transfer_starts);
            }
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                collect_dma_starts(checker, then_body, transfer_starts);
                if let Some(body) = else_body {
                    collect_dma_starts(checker, body, transfer_starts);
                }
            }
            Stmt::Expr(_)
            | Stmt::Assign { .. }
            | Stmt::Return(..)
            | Stmt::Break(..)
            | Stmt::BreakValue(..)
            | Stmt::Continue(..)
            | Stmt::BreakLabel(..)
            | Stmt::BreakLabelValue(..)
            | Stmt::ContinueLabel(..)
            | Stmt::Yield(..)
            | Stmt::DeferClose { .. } => {}
        }
    }
}

fn collect_lambda_calls(
    checker: &Checker<'_>,
    lambda: &Lambda,
    calls: &mut Vec<(usize, FrameCall)>,
    transfer_starts: &mut BTreeMap<String, (String, String, String, String)>,
) {
    if let LambdaBody::Block(statements) = &lambda.body {
        collect_dma_starts(checker, statements, transfer_starts);
    }
    let mut collect = |expression: &Expr| match expression {
        Expr::Call(call) => {
            let core = call
                .name
                .rsplit_once('.')
                .and_then(|(module, member)| jet_foundation::Syntax::core_call(module, member));
            calls.push((
                call.name_span.start,
                FrameCall {
                    name: call.name.clone(),
                    args: call.args.clone(),
                    core,
                    hardware: None,
                },
            ));
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            method_span,
            ..
        } => {
            if let Some(module) = core_module_path_from_receiver(checker, receiver) {
                if let Some(core) = jet_foundation::Syntax::core_call(&module, method) {
                    calls.push((
                        method_span.start,
                        FrameCall {
                            name: format!("{module}.{method}"),
                            args: args.clone(),
                            core: Some(core),
                            hardware: None,
                        },
                    ));
                    return;
                }
            }
            let Some(hardware) = hardware_frame_call(checker, expression) else {
                return;
            };
            if matches!(
                &hardware,
                FrameHardwareCall::DmaWait { transfer }
                    if !transfer_starts.contains_key(transfer)
            ) {
                return;
            }
            calls.push((
                method_span.start,
                FrameCall {
                    name: hardware_call_name(&hardware),
                    args: args.clone(),
                    core: None,
                    hardware: Some(hardware),
                },
            ));
        }
        _ => {}
    };
    match &lambda.body {
        LambdaBody::Expr(expression) => expression.for_each_expr(&mut collect),
        LambdaBody::Block(statements) => {
            for statement in statements {
                statement.for_each_expr(&mut collect);
            }
        }
    }
}
fn hardware_profile_for_alias(
    checker: &Checker<'_>,
    alias: &str,
) -> Option<(String, TargetHardwareFacts)> {
    let profile = checker.core_imports.get(alias)?.clone();
    let facts = match profile.as_str() {
        "board.sensor_v1" => TargetMachine::board_sensor_v1().hardware,
        "board.virt_aarch64" => TargetMachine::board_virt_aarch64().hardware,
        _ => return None,
    };
    Some((profile, facts))
}

fn hardware_expr_path(expression: &Expr) -> Vec<String> {
    match expression {
        Expr::Ident(name, _) => vec![name.clone()],
        Expr::Field(base, member, _) => {
            let mut path = hardware_expr_path(base);
            path.push(member.clone());
            path
        }
        Expr::Paren(inner, _) => hardware_expr_path(inner),
        _ => Vec::new(),
    }
}

fn hardware_buffer_name(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Paren(inner, _) => hardware_buffer_name(inner),
        _ => None,
    }
}

/// Project checked `THardwareCall` syntax into the canonical frame schedule.
/// Register and channel names come from the same immutable target facts used by
/// TIR lowering; this seam never invents a second hardware fact table.
fn hardware_frame_call(checker: &Checker<'_>, expression: &Expr) -> Option<FrameHardwareCall> {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        method_span,
        ..
    } = expression
    else {
        return None;
    };
    let receiver_path = hardware_expr_path(receiver);
    if receiver_path.len() >= 3 {
        if let Some((profile, facts)) =
            receiver_path
                .first()
                .and_then(|alias| hardware_profile_for_alias(checker, alias))
        {
            let block = facts
                .register_blocks
                .iter()
                .find(|block| block.name.eq_ignore_ascii_case(&receiver_path[1]))?;
            let register = block
                .registers
                .iter()
                .find(|register| register.name.eq_ignore_ascii_case(&receiver_path[2]))?;
            let mode = match method.as_str() {
                "read" => JetResourceAccessMode::Read,
                "write" | "set" | "clear" => JetResourceAccessMode::Write,
                _ => return None,
            };
            return Some(FrameHardwareCall::Register {
                profile,
                block: block.name.clone(),
                register: register.name.clone(),
                method: method.clone(),
                mode,
            });
        }
    }
    if method == "start"
        && matches!(receiver.as_ref(), Expr::Ident(name, _) if name == "dma")
        && args.len() >= 2
    {
        let channel_path = hardware_expr_path(&args[0].expr);
        let (profile, facts) = channel_path
            .first()
            .and_then(|alias| hardware_profile_for_alias(checker, alias))?;
        if channel_path.len() < 3 {
            return None;
        }
        let channel_key = format!(
            "{}_{}",
            channel_path[1].to_ascii_uppercase(),
            channel_path[2].to_ascii_uppercase()
        );
        let channel = facts
            .dma_channels
            .iter()
            .find(|channel| channel.name.eq_ignore_ascii_case(&channel_key))?;
        let buffer = hardware_buffer_name(&args[1].expr)?;
        return Some(FrameHardwareCall::DmaStart {
            profile,
            channel: channel.name.clone(),
            buffer,
            token: format!("jet_dma_transfer@{}", method_span.start),
        });
    }
    if method == "wait" {
        if let Expr::Ident(transfer, _) = receiver.as_ref() {
            return Some(FrameHardwareCall::DmaWait {
                transfer: transfer.clone(),
            });
        }
    }
    None
}

fn hardware_call_name(call: &FrameHardwareCall) -> String {
    match call {
        FrameHardwareCall::Register {
            profile,
            block,
            register,
            method,
            ..
        } => format!("hardware.register.{profile}.{block}.{register}.{method}"),
        FrameHardwareCall::DmaStart {
            profile, channel, ..
        } => format!("hardware.dma.start.{profile}.{channel}"),
        FrameHardwareCall::DmaWait { transfer } => {
            format!("hardware.dma.wait.{transfer}")
        }
    }
}


fn core_module_path_from_receiver(checker: &Checker<'_>, receiver: &Expr) -> Option<String> {
    match receiver {
        Expr::Ident(alias, _) if checker.lookup(alias).is_none() => {
            checker.core_imports.get(alias).cloned()
        }
        Expr::Field(base, member, _) => {
            let prefix = core_module_path_from_receiver(checker, base)?;
            Some(format!("{prefix}.{member}"))
        }
        _ => None,
    }
}

fn core_access_mode(mode: CoreCallResourceAccessMode) -> JetResourceAccessMode {
    match mode {
        CoreCallResourceAccessMode::Read => JetResourceAccessMode::Read,
        CoreCallResourceAccessMode::Write => JetResourceAccessMode::Write,
        CoreCallResourceAccessMode::Move => JetResourceAccessMode::Move,
    }
}

fn access_mode(convention: AccessConvention) -> JetResourceAccessMode {
    match convention {
        AccessConvention::Read => JetResourceAccessMode::Read,
        AccessConvention::Write => JetResourceAccessMode::Write,
        AccessConvention::Move => JetResourceAccessMode::Move,
    }
}


fn resource_identity(expression: &Expr) -> Option<JetResourceIdentity> {
    match expression {
        Expr::Paren(inner, _) | Expr::Place(inner, _, _) | Expr::Copy(inner, _) => {
            resource_identity(inner)
        }
        Expr::Ident(name, _) => Some(JetResourceIdentity::named(name.clone())),
        Expr::Field(base, _member, _) => {
            // No checked byte/field-region fact exists at this seam.  Keep the
            // containing resource identity so a field access cannot be
            // mistaken for a disjoint resource and reordered unsafely.
            resource_identity(base)
        }
        Expr::Index { base, .. } | Expr::Slice { base, .. } => {
            resource_identity(base).or(Some(JetResourceIdentity::unknown()))
        }
        Expr::Call(_) | Expr::MethodCall { .. } | Expr::CallValue { .. } => {
            Some(JetResourceIdentity::unknown())
        }
        _ => None,
    }
}

