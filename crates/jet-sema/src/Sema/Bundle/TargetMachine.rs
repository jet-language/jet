use crate::Diagnostics::Diagnostic;
use crate::AST::ProgramBundle;
use crate::Sema::{effect_covers, effect_root};
use jet_foundation::RingLayer::classify_prelude_closure;
use jet_foundation::TargetMachine::{
    RegisterWidth, TargetDmaOperation, TargetDmaOperationFact, TargetDmaOwner, TargetHardwareFacts,
    TargetHardwareUnresolvedReference, TargetHardwareUse, TargetInterruptHandlerFact,
    TargetMachine, TargetMachineError, TargetRegisterAccessFact, TargetMachineUse,
};
use std::collections::{BTreeMap, BTreeSet};

/// Project the complete sema-owned Prelude closure into target requirements.
///
/// `used_core` contains direct calls and sema-generated roots. The target pass
/// must see the transitive closure, not only imports visible to one checker, so
/// it expands the same RingLayer graph before validating target facts.
pub fn target_machine_use(bundle: &ProgramBundle) -> TargetMachineUse {
    let mut used_core = bundle.used_core.clone();
    let hardware = target_hardware_use(bundle);
    if !hardware.register_accesses.is_empty()
        || !hardware.interrupt_handlers.is_empty()
        || !hardware.dma_operations.is_empty()
        || !hardware.unresolved_references.is_empty()
    {
        used_core.insert("core.hardware".to_string());
    }
    let closure = classify_prelude_closure(used_core.iter());
    let mut usage = TargetMachineUse::from_core_apis(closure.keys());
    usage.core_apis = closure.into_keys().collect();
    usage
}
/// Return the selected target profile's immutable hardware facts.
///
/// A board import is the source-level profile selector.  The profile is
/// resolved here, before any backend sees the package, so register widths,
/// interrupt vectors, and DMA ownership cannot be reconstructed per tier.
pub fn target_hardware_profile(bundle: &ProgramBundle) -> Option<TargetHardwareFacts> {
    selected_hardware_profile(bundle).map(|(_, facts)| facts)
}

/// Return the exact profile identity selected by a board import or file target.
pub fn target_hardware_profile_id(bundle: &ProgramBundle) -> Option<String> {
    selected_hardware_profile(bundle).map(|(id, _)| id)
}

/// Return generated capability identifiers for the selected immutable profile.
pub fn target_hardware_capabilities(bundle: &ProgramBundle) -> Vec<String> {
    let Some((_, facts)) = selected_hardware_profile(bundle) else {
        return Vec::new();
    };
    let mut capabilities = Vec::new();
    for block in &facts.register_blocks {
        for register in &block.registers {
            capabilities.push(format!("register.{}.{}", block.name, register.name));
        }
    }
    for interrupt in &facts.interrupts {
        capabilities.push(format!("interrupt.{}", interrupt.name));
    }
    for channel in &facts.dma_channels {
        capabilities.push(format!("dma.{}", channel.name));
    }
    capabilities
}

/// Project all source-level board operations into the shared hardware fact
/// plane.  This is deliberately a sema projection: no target pointer, host
/// callback, or tier-specific operation is created here.
pub fn target_hardware_use(bundle: &ProgramBundle) -> TargetHardwareUse {
    project_target_hardware(bundle, None)
}

/// Project hardware operations with the effect solver's transitive handler
/// facts.  The plain API remains useful to tooling that has only an AST;
/// the driver uses this form so an interrupt calling an allocating helper is
/// rejected by the same target boundary as a direct allocation.
pub fn target_hardware_use_with_effect_facts(
    bundle: &ProgramBundle,
    effects: &crate::Sema::SemIndexEffectFacts,
) -> TargetHardwareUse {
    project_target_hardware(bundle, Some(effects))
}

fn selected_hardware_profile(bundle: &ProgramBundle) -> Option<(String, TargetHardwareFacts)> {
    let entry = bundle.modules.get(bundle.entry)?;
    for (_, import) in crate::AST::walk_imports(entry) {
        if let Some(profile) = crate::AST::target_profile_path(import) {
            if let Some(facts) = hardware_facts_for_profile(&profile) {
                return Some((profile, facts));
            }
        }
    }
    if let Some(profile) = entry.default_target.as_deref() {
        if let Some(facts) = hardware_facts_for_profile(profile) {
            return Some((profile.to_string(), facts));
        }
    }
    None
}

fn hardware_facts_for_profile(profile: &str) -> Option<TargetHardwareFacts> {
    match profile {
        "board.sensor_v1" => Some(TargetMachine::board_sensor_v1().hardware),
        "board.virt_aarch64" => Some(TargetMachine::board_virt_aarch64().hardware),
        _ => None,
    }
}

fn project_target_hardware(
    bundle: &ProgramBundle,
    effect_facts: Option<&crate::Sema::SemIndexEffectFacts>,
) -> TargetHardwareUse {
    let mut hardware = TargetHardwareUse::default();
    for module in &bundle.modules {
        let aliases = profile_aliases(module);
        let profile_facts = aliases
            .values()
            .find_map(|profile| hardware_facts_for_profile(profile));
        for item in &module.items {
            match item {
                crate::AST::Item::Func(function) => project_function(
                    function,
                    &module.alias,
                    &aliases,
                    profile_facts.as_ref(),
                    effect_facts,
                    &mut hardware,
                ),
                crate::AST::Item::Struct(definition) => {
                    for function in &definition.methods {
                        project_function(
                            function,
                            &module.alias,
                            &aliases,
                            profile_facts.as_ref(),
                            effect_facts,
                            &mut hardware,
                        );
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            project_function(
                                function,
                                &module.alias,
                                &aliases,
                                profile_facts.as_ref(),
                                effect_facts,
                                &mut hardware,
                            );
                        }
                    }
                }
                crate::AST::Item::Enum(definition) => {
                    for function in &definition.methods {
                        project_function(
                            function,
                            &module.alias,
                            &aliases,
                            profile_facts.as_ref(),
                            effect_facts,
                            &mut hardware,
                        );
                    }
                    for implementation in &definition.trait_impls {
                        for function in &implementation.methods {
                            project_function(
                                function,
                                &module.alias,
                                &aliases,
                                profile_facts.as_ref(),
                                effect_facts,
                                &mut hardware,
                            );
                        }
                    }
                }
                crate::AST::Item::Impl(implementation) => {
                    for function in &implementation.methods {
                        project_function(
                            function,
                            &module.alias,
                            &aliases,
                            profile_facts.as_ref(),
                            effect_facts,
                            &mut hardware,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    hardware.unresolved_references.sort();
    hardware.unresolved_references.dedup();
    hardware
}

fn profile_aliases(module: &crate::AST::LoadedModule) -> BTreeMap<String, String> {
    crate::AST::walk_imports(module)
        .into_iter()
        .filter_map(|(_, import)| {
            crate::AST::target_profile_path(import).map(|profile| (import.import_alias(), profile))
        })
        .collect()
}

fn project_function(
    function: &crate::AST::Func,
    module_alias: &str,
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    effect_facts: Option<&crate::Sema::SemIndexEffectFacts>,
    hardware: &mut TargetHardwareUse,
) {
    for marker in &function.markers {
        if marker.name != crate::Syntax::MARKER_INTERRUPT {
            continue;
        }
        let Some(interrupt) = marker
            .expr_arg(0)
            .and_then(|expr| {
                interrupt_name(
                    expr,
                    aliases,
                    profile_facts,
                    &mut hardware.unresolved_references,
                )
            })
        else {
            continue;
        };
        let mut effects = declared_or_direct_effects(
            function,
            module_alias,
            effect_facts,
        );
        let forbidden_effects = marker_effect_row(marker);
        let mut handler = TargetInterruptHandlerFact::new(
            interrupt,
            function.name.clone(),
            std::mem::take(&mut effects),
            true,
        );
        handler.forbidden_effects = forbidden_effects;
        hardware.interrupt_handlers.push(handler);
    }

    let mut transfers = BTreeMap::<String, (String, String)>::new();
    let mut in_flight = BTreeMap::<String, String>::new();
    for statement in &function.body {
        let mut moved = BTreeSet::new();
        statement.for_each_expr(|expr| {
            if let Some((_, buffer, convention)) = dma_start(
                expr,
                aliases,
                profile_facts,
                &mut hardware.unresolved_references,
            ) {
                if convention == crate::AST::AccessConvention::Move {
                    moved.insert(buffer);
                }
            }
        });
        if let crate::AST::Stmt::Val(binding) = statement {
            if let Some((channel, buffer, _)) = dma_start(
                &binding.init,
                aliases,
                profile_facts,
                &mut hardware.unresolved_references,
            ) {
                if !binding.name.is_empty() {
                    transfers.insert(binding.name.clone(), (channel, buffer));
                }
            }
        }
        statement.for_each_expr(|expr| {
            project_hardware_expr(
                expr,
                aliases,
                profile_facts,
                &moved,
                &mut transfers,
                &mut in_flight,
                hardware,
            );
        });
    }

}
fn declared_or_direct_effects(
    function: &crate::AST::Func,
    module_alias: &str,
    effect_facts: Option<&crate::Sema::SemIndexEffectFacts>,
) -> jet_foundation::Effects::EffectSet {
    let mut effects = jet_foundation::Effects::EffectSet::new();
    if let Some(declared) = &function.declared_effects {
        effects.extend(declared.iter().map(|(effect, _)| effect.clone()));
    }
    for statement in &function.body {
        statement.for_each_expr(|expr| collect_direct_effect(expr, &mut effects));
    }
    if let Some(facts) = effect_facts {
        let qualified = format!("{module_alias}::{}", function.name);
        if let Some(solved) = facts.solved.get(&qualified).or_else(|| facts.solved.get(&function.name)) {
            effects.extend(solved.iter().cloned());
        }
    }
    effects
}

fn collect_direct_effect(
    expr: &crate::AST::Expr,
    effects: &mut jet_foundation::Effects::EffectSet,
) {
    match expr {
        crate::AST::Expr::Call(call) => match call.name.as_str() {
            "alloc" | "try_alloc" => {
                effects.insert("Mem.Alloc".to_string());
            }
            "wait" => {
                effects.insert("Time.Wait".to_string());
            }
            "sleep" => {
                effects.insert("Time.Sleep".to_string());
            }
            _ => {}
        },
        crate::AST::Expr::MethodCall { method, .. } => {
            if method == "alloc" || method == "try_alloc" {
                effects.insert("Mem.Alloc".to_string());
            } else if method == "wait" {
                effects.insert("Time.Wait".to_string());
            } else if method == "sleep" {
                effects.insert("Time.Sleep".to_string());
            }
        }
        _ => {}
    }
}

fn marker_effect_row(marker: &crate::AST::Marker) -> jet_foundation::Effects::EffectSet {
    marker
        .args
        .get(1)
        .and_then(|arg| match arg {
            crate::AST::MarkerCallArg::EffectRow { effects, .. } => Some(effects),
            crate::AST::MarkerCallArg::Expr(_) => None,
        })
        .into_iter()
        .flatten()
        .map(|(effect, _)| effect.trim_start_matches('!').to_string())
        .collect()
}

fn project_hardware_expr(
    expr: &crate::AST::Expr,
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    moved: &BTreeSet<String>,
    transfers: &mut BTreeMap<String, (String, String)>,
    in_flight: &mut BTreeMap<String, String>,
    hardware: &mut TargetHardwareUse,
) {
    match expr {
        crate::AST::Expr::Ident(name, _) => {
            if in_flight.contains_key(name) && !moved.contains(name) {
                hardware
                    .dma_operations
                    .push(TargetDmaOperationFact::use_buffer(name.clone()));
            }
        }
        crate::AST::Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if let Some((channel, buffer, convention)) = dma_start(
                expr,
                aliases,
                profile_facts,
                &mut hardware.unresolved_references,
            ) {
                let mut operation = TargetDmaOperationFact::start(channel.clone(), buffer.clone());
                if convention != crate::AST::AccessConvention::Move {
                    operation.owner = TargetDmaOwner::Device;
                }
                hardware.dma_operations.push(operation);
                in_flight.insert(buffer, channel);
                return;
            }
            if method == "wait" {
                if let crate::AST::Expr::Ident(transfer, _) = receiver.as_ref() {
                    if let Some((channel, buffer)) = transfers.get(transfer).cloned() {
                        hardware
                            .dma_operations
                            .push(TargetDmaOperationFact::wait(channel, buffer.clone()));
                        in_flight.remove(&buffer);
                        return;
                    }
                }
            }
            if let Some(path) = expr_path(receiver) {
                if let Some((block, register, width)) = register_for_path(
                    &path,
                    aliases,
                    profile_facts,
                    &mut hardware.unresolved_references,
                ) {
                    let operation = match method.as_str() {
                        "read" => Some(TargetRegisterAccessFact::read(block, register, width)),
                        "write" | "set" | "clear" => {
                            Some(TargetRegisterAccessFact::write(block, register, width))
                        }
                        _ => None,
                    };
                    if let Some(operation) = operation {
                        hardware.register_accesses.push(operation);
                    }
                }
            }
            // Keep the ordinary recursive visitor in control of nested
            // arguments; only a move argument is exempted at the statement
            // level above.
            let _ = args;
        }
        _ => {}
    }
}

fn dma_start(
    expr: &crate::AST::Expr,
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    unresolved: &mut Vec<TargetHardwareUnresolvedReference>,
) -> Option<(String, String, crate::AST::AccessConvention)> {
    let crate::AST::Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr
    else {
        return None;
    };
    if method != "start"
        || !matches!(receiver.as_ref(), crate::AST::Expr::Ident(name, _) if name == "dma")
        || args.len() < 2
    {
        return None;
    }
    let channel_path = expr_path(&args[0].expr)?;
    let channel = channel_for_path(&channel_path, aliases, profile_facts, unresolved)?;
    let buffer = expr_name(&args[1].expr)?;
    Some((channel, buffer, args[1].convention))
}

fn channel_for_path(
    path: &[String],
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    unresolved: &mut Vec<TargetHardwareUnresolvedReference>,
) -> Option<String> {
    if path.len() < 3 || !aliases.contains_key(&path[0]) {
        return None;
    }
    let requested = format!("{}_{}", path[1].to_ascii_uppercase(), path[2].to_ascii_uppercase());
    let Some(facts) = profile_facts else {
        unresolved.push(TargetHardwareUnresolvedReference::DmaChannel {
            channel: requested,
        });
        return None;
    };
    let Some(channel) = facts
        .dma_channels
        .iter()
        .find(|channel| channel.name.eq_ignore_ascii_case(&requested))
    else {
        unresolved.push(TargetHardwareUnresolvedReference::DmaChannel {
            channel: requested,
        });
        return None;
    };
    Some(channel.name.clone())
}

fn interrupt_name(
    expr: &crate::AST::Expr,
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    unresolved: &mut Vec<TargetHardwareUnresolvedReference>,
) -> Option<String> {
    let path = expr_path(expr)?;
    if path.len() < 2 || !aliases.contains_key(&path[0]) {
        return None;
    }
    let requested = path.last()?.clone();
    let Some(facts) = profile_facts else {
        unresolved.push(TargetHardwareUnresolvedReference::Interrupt {
            interrupt: requested,
        });
        return None;
    };
    let Some(interrupt) = facts
        .interrupts
        .iter()
        .find(|interrupt| interrupt.name.eq_ignore_ascii_case(&requested))
    else {
        unresolved.push(TargetHardwareUnresolvedReference::Interrupt {
            interrupt: requested,
        });
        return None;
    };
    Some(interrupt.name.clone())
}

fn register_for_path(
    path: &[String],
    aliases: &BTreeMap<String, String>,
    profile_facts: Option<&TargetHardwareFacts>,
    unresolved: &mut Vec<TargetHardwareUnresolvedReference>,
) -> Option<(String, String, RegisterWidth)> {
    if path.len() < 3 || !aliases.contains_key(&path[0]) {
        return None;
    }
    let requested_block = path[1].to_ascii_uppercase();
    let requested_register = path[2].clone();
    let Some(facts) = profile_facts else {
        unresolved.push(TargetHardwareUnresolvedReference::RegisterBlock {
            block: requested_block,
        });
        return None;
    };
    let Some(block) = facts
        .register_blocks
        .iter()
        .find(|block| block.name.eq_ignore_ascii_case(&requested_block))
    else {
        unresolved.push(TargetHardwareUnresolvedReference::RegisterBlock {
            block: requested_block,
        });
        return None;
    };
    let Some(register) = block
        .register(&requested_register)
        .or_else(|| {
            block
                .registers
                .iter()
                .find(|register| register.name.eq_ignore_ascii_case(&requested_register))
        })
    else {
        unresolved.push(TargetHardwareUnresolvedReference::Register {
            block: block.name.clone(),
            register: requested_register,
        });
        return None;
    };
    Some((block.name.clone(), register.name.clone(), register.width))
}

fn expr_path(expr: &crate::AST::Expr) -> Option<Vec<String>> {
    match expr {
        crate::AST::Expr::Ident(name, _) => Some(vec![name.clone()]),
        crate::AST::Expr::Field(base, member, _) => {
            let mut path = expr_path(base)?;
            path.push(member.clone());
            Some(path)
        }
        crate::AST::Expr::Paren(inner, _) => expr_path(inner),
        _ => None,
    }
}

fn expr_name(expr: &crate::AST::Expr) -> Option<String> {
    match expr {
        crate::AST::Expr::Ident(name, _) => Some(name.clone()),
        crate::AST::Expr::Paren(inner, _) => expr_name(inner),
        _ => None,
    }
}

/// Admit a selected target before codegen. TargetMachine remains the typed fact
/// validator; this seam projects its data errors through registered sema rows
/// so human and machine output share one diagnostic contract.
pub fn check_target_machine(machine: &TargetMachine, usage: &TargetMachineUse) -> Vec<Diagnostic> {
    machine
        .validate(usage)
        .iter()
        .map(|error| diagnostic_for_error(machine, error))
        .collect()
}

/// Validate the target-profile hardware fact plane at the sema boundary.
///
/// Register operations are checked against generated width/access/volatile
/// facts by `TargetMachine`. Handler effects come from the existing effect
/// solver, and DMA operations consume the existing ownership projection. The
/// result is intentionally a normal target diagnostic; no parser, Prelude, or
/// backend adapter is implied by this boundary.
#[allow(dead_code)]
pub fn validate_target_hardware(
    machine: &TargetMachine,
    hardware: &TargetHardwareUse,
) -> Vec<Diagnostic> {
    let mut errors = machine.validate_hardware(hardware);
    validate_interrupt_handlers(machine, hardware, &mut errors);
    validate_dma_operations(machine, hardware, &mut errors);
    errors
        .iter()
        .map(|error| diagnostic_for_error(machine, error))
        .collect()
}

fn validate_interrupt_handlers(
    machine: &TargetMachine,
    hardware: &TargetHardwareUse,
    errors: &mut Vec<TargetMachineError>,
) {
    for handler in &hardware.interrupt_handlers {
        let Some(interrupt) = machine.hardware.interrupt(&handler.interrupt) else {
            errors.push(TargetMachineError::UnknownInterrupt {
                interrupt: handler.interrupt.clone(),
            });
            continue;
        };
        if interrupt.bounded && !handler.bounded {
            errors.push(TargetMachineError::InterruptHandlerUnbounded {
                interrupt: interrupt.name.clone(),
                handler: handler.handler.clone(),
            });
        }

        for effect in &handler.effects {
            let baseline_forbidden = effect_is_allocation(effect) || effect_is_wait(effect);
            let profile_forbidden = interrupt
                .forbidden_effects
                .iter()
                .chain(handler.forbidden_effects.iter())
                .any(|bound| effect_covers(bound, effect));
            if baseline_forbidden || profile_forbidden {
                errors.push(TargetMachineError::InterruptEffectForbidden {
                    interrupt: interrupt.name.clone(),
                    handler: handler.handler.clone(),
                    effect: effect.clone(),
                });
            }
        }
    }
}

fn effect_is_allocation(effect: &str) -> bool {
    effect == "Mem" || effect == "Mem.Alloc" || effect.starts_with("Mem.Alloc(")
}

fn effect_is_wait(effect: &str) -> bool {
    if effect == "Time" {
        return true;
    }
    if effect == "Time.Wait" || effect.starts_with("Time.Wait.") {
        return true;
    }
    if effect == "Time.Sleep" || effect.starts_with("Time.Sleep.") {
        return true;
    }
    effect_root(effect) == "Time" && effect.ends_with(".Wait")
}

fn validate_dma_operations(
    machine: &TargetMachine,
    hardware: &TargetHardwareUse,
    errors: &mut Vec<TargetMachineError>,
) {
    let mut in_flight = BTreeMap::<String, String>::new();
    for operation in &hardware.dma_operations {
        match operation.operation {
            TargetDmaOperation::UseBuffer => {
                if operation.owner != TargetDmaOwner::Cpu {
                    errors.push(TargetMachineError::DmaOwnershipMismatch {
                        channel: operation.channel.clone(),
                        buffer: operation.buffer.clone(),
                        expected: TargetDmaOwner::Cpu,
                        actual: operation.owner,
                    });
                }
                if let Some(channel) = in_flight.get(&operation.buffer) {
                    errors.push(TargetMachineError::DmaBufferUnavailable {
                        channel: channel.clone(),
                        buffer: operation.buffer.clone(),
                    });
                }
            }
            TargetDmaOperation::Start | TargetDmaOperation::Wait => {
                let Some(channel) = machine.hardware.dma_channel(&operation.channel) else {
                    errors.push(TargetMachineError::UnknownDmaChannel {
                        channel: operation.channel.clone(),
                    });
                    continue;
                };
                if channel.ownership != jet_foundation::TargetMachine::TargetDmaOwnership::Transfer
                {
                    continue;
                }
                match operation.operation {
                    TargetDmaOperation::Start => {
                        if operation.owner != TargetDmaOwner::Cpu {
                            errors.push(TargetMachineError::DmaOwnershipMismatch {
                                channel: channel.name.clone(),
                                buffer: operation.buffer.clone(),
                                expected: TargetDmaOwner::Cpu,
                                actual: operation.owner,
                            });
                        }
                        if in_flight
                            .insert(operation.buffer.clone(), channel.name.clone())
                            .is_some()
                        {
                            errors.push(TargetMachineError::DmaStartWhileInFlight {
                                channel: channel.name.clone(),
                                buffer: operation.buffer.clone(),
                            });
                        }
                    }
                    TargetDmaOperation::Wait => {
                        if operation.owner != TargetDmaOwner::Device {
                            errors.push(TargetMachineError::DmaOwnershipMismatch {
                                channel: channel.name.clone(),
                                buffer: operation.buffer.clone(),
                                expected: TargetDmaOwner::Device,
                                actual: operation.owner,
                            });
                        }
                        match in_flight.get(&operation.buffer) {
                            None => errors.push(TargetMachineError::DmaWaitWithoutTransfer {
                                channel: channel.name.clone(),
                                buffer: operation.buffer.clone(),
                            }),
                            Some(active) if active != &channel.name => {
                                errors.push(TargetMachineError::DmaWaitChannelMismatch {
                                    channel: channel.name.clone(),
                                    buffer: operation.buffer.clone(),
                                });
                            }
                            Some(_) => {
                                in_flight.remove(&operation.buffer);
                            }
                        }
                    }
                    TargetDmaOperation::UseBuffer => unreachable!(),
                }
            }
        }
    }
}

fn diagnostic_for_error(machine: &TargetMachine, error: &TargetMachineError) -> Diagnostic {
    let target = target_name(machine);
    match error {
        TargetMachineError::CoreApiUnavailable {
            api,
            required,
            available,
        } => {
            let required = required.as_str();
            let available = available.as_str();
            Diagnostic::from_row(
                "E3310",
                &[
                    ("api", api),
                    ("required", required),
                    ("available", available),
                    ("target", target),
                ],
                None,
            )
        }
        TargetMachineError::HeapRequiresAllocator | TargetMachineError::MissingAllocatorPolicy => {
            Diagnostic::from_row("E3303", &[], None)
        }
        TargetMachineError::MissingTargetCapability { capability }
        | TargetMachineError::HostedCapabilityRequiresOs { capability } => Diagnostic::from_row(
            "E3311",
            &[("target", target), ("capability", capability)],
            None,
        ),
        TargetMachineError::InvalidProviderContract {
            capability,
            provider,
            sha256,
        } => Diagnostic::from_row(
            "E3312",
            &[
                ("target", target),
                ("capability", capability),
                ("provider", provider),
                ("sha256", sha256),
            ],
            None,
        ),
        TargetMachineError::MmioOutsideRegion {
            address,
            size_bytes,
        } => {
            let detail = format!(
                "address 0x{address:X} ({size_bytes} bytes) is outside a declared MMIO region"
            );
            mmio_diagnostic(target, &detail)
        }
        TargetMachineError::MmioMissingUnsafeGate { address } => {
            let detail = format!("address 0x{address:X} has no #Unsafe gate");
            mmio_diagnostic(target, &detail)
        }
        TargetMachineError::MmioEmptyUnsafeReason { address } => {
            let detail = format!("address 0x{address:X} has an empty #Unsafe reason");
            mmio_diagnostic(target, &detail)
        }
        _ => {
            let (fact, detail) = generic_detail(error);
            Diagnostic::from_row(
                "E3314",
                &[
                    ("target", target),
                    ("fact", fact),
                    ("detail", detail.as_str()),
                ],
                None,
            )
        }
    }
}

fn target_name(machine: &TargetMachine) -> &str {
    if machine.name.trim().is_empty() {
        &machine.triple
    } else {
        &machine.name
    }
}

fn mmio_diagnostic(target: &str, detail: &str) -> Diagnostic {
    Diagnostic::from_row("E3313", &[("target", target), ("detail", detail)], None)
}

fn generic_detail(error: &TargetMachineError) -> (&'static str, String) {
    match error {
        TargetMachineError::MissingTargetTriple => {
            ("target triple", "the selected target triple is empty".to_string())
        }
        TargetMachineError::MissingMemoryKind { kind } => (
            "memory",
            format!("the target is missing a {} memory region", memory_kind_name(*kind)),
        ),
        TargetMachineError::DuplicateMemoryRegion { name } => {
            ("memory", format!("memory region `{name}` is declared more than once"))
        }
        TargetMachineError::EmptyMemoryRegion { name } => {
            ("memory", format!("memory region `{name}` has zero size"))
        }
        TargetMachineError::MemoryAddressOverflow { name } => {
            ("memory", format!("memory region `{name}` overflows the address space"))
        }
        TargetMachineError::OverlappingMemoryRegions { first, second } => (
            "memory",
            format!("memory regions `{first}` and `{second}` overlap"),
        ),
        TargetMachineError::MissingLinkerInput => {
            ("linker", "the no-OS target has no linker input".to_string())
        }
        TargetMachineError::LinkerFileMissingPath => {
            ("linker", "the linker file path is empty".to_string())
        }
        TargetMachineError::LinkerFileMissingHash { path } => (
            "linker",
            format!("linker file `{path}` has no valid sha256 provenance"),
        ),
        TargetMachineError::AllocatorRegionUnknown { region } => (
            "allocator",
            format!("allocator region `{region}` does not exist"),
        ),
        TargetMachineError::AllocatorRegionNotRam { region } => (
            "allocator",
            format!("allocator region `{region}` is not RAM"),
        ),
        TargetMachineError::AllocatorRegionTooSmall {
            region,
            requested_bytes,
            available_bytes,
        } => (
            "allocator",
            format!(
                "allocator region `{region}` has {available_bytes} bytes but needs {requested_bytes}"
            ),
        ),
        TargetMachineError::HostedAllocatorRequiresOs => (
            "allocator",
            "a hosted system allocator cannot be selected on a no-OS target".to_string(),
        ),
        TargetMachineError::MissingPanicPolicy => (
            "panic",
            "the no-OS target has no panic policy".to_string(),
        ),
        TargetMachineError::RamOverflow {
            used_bytes,
            ram_bytes,
        } => (
            "memory",
            format!("RAM use ({used_bytes} bytes) exceeds {ram_bytes} bytes"),
        ),
        TargetMachineError::HostedHasNoLinkerScript => (
            "linker",
            "hosted targets do not have a generated linker script".to_string(),
        ),
        TargetMachineError::HostedHasNoStartup => (
            "startup",
            "hosted targets do not have generated startup glue".to_string(),
        ),
        TargetMachineError::UnsupportedStartupTriple { triple } => (
            "startup",
            format!("target triple `{triple}` has no startup generator"),
        ),
        TargetMachineError::ExecutionTierUnsupported { tier, machine } => (
            "execution",
            format!("target machine `{machine}` does not support the `{tier}` tier"),
        ),
        TargetMachineError::FirmwareToolchainMissing { tool } => (
            "toolchain",
            format!("required firmware tool `{tool}` is unavailable"),
        ),
        TargetMachineError::FirmwareBuildFailed { detail } => ("firmware", detail.clone()),
        TargetMachineError::SizeBudgetExceeded { report } => (
            "size budget",
            format!(
                "artifact={} flash={} ram={} used_ram={}",
                report.artifact_bytes, report.flash_bytes, report.ram_bytes, report.ram_used_bytes
            ),
        ),
        TargetMachineError::HardwareFactsMissingSvd => (
            "hardware profile",
            "register, interrupt, or DMA facts require SVD provenance".to_string(),
        ),
        TargetMachineError::InvalidHardwareSvd { source, sha256 } => (
            "hardware profile",
            format!("SVD `{source}` has invalid sha256 provenance `{sha256}`"),
        ),
        TargetMachineError::HardwareFactEmptyName { kind } => (
            "hardware profile",
            format!("{kind} fact has an empty name"),
        ),
        TargetMachineError::DuplicateRegisterBlock { name } => (
            "registers",
            format!("register block `{name}` is declared more than once"),
        ),
        TargetMachineError::RegisterBlockEmpty { name } => (
            "registers",
            format!("register block `{name}` has zero size"),
        ),
        TargetMachineError::RegisterBlockAddressOverflow { name } => (
            "registers",
            format!("register block `{name}` overflows the address space"),
        ),
        TargetMachineError::RegisterBlockOutsideRegion {
            name,
            address,
            size_bytes,
        } => (
            "registers",
            format!(
                "register block `{name}` at 0x{address:X} ({size_bytes} bytes) is outside a declared MMIO region"
            ),
        ),
        TargetMachineError::DuplicateRegister { block, register } => (
            "registers",
            format!("register `{register}` is declared more than once in `{block}`"),
        ),
        TargetMachineError::RegisterAddressOverflow { block, register } => (
            "registers",
            format!("register `{block}.{register}` overflows the address space"),
        ),
        TargetMachineError::RegisterOutsideBlock { block, register } => (
            "registers",
            format!("register `{block}.{register}` lies outside its block"),
        ),
        TargetMachineError::UnknownRegisterBlock { block } => (
            "registers",
            format!("register block `{block}` is not in the target SVD"),
        ),
        TargetMachineError::UnknownRegister { block, register } => (
            "registers",
            format!("register `{block}.{register}` is not in the target SVD"),
        ),
        TargetMachineError::RegisterWidthMismatch {
            block,
            register,
            expected,
            actual,
        } => (
            "registers",
            format!(
                "`{block}.{register}` requires {} but operation requested {}",
                expected.as_str(),
                actual.as_str()
            ),
        ),
        TargetMachineError::RegisterReadDenied { block, register } => (
            "registers",
            format!("register `{block}.{register}` is not readable"),
        ),
        TargetMachineError::RegisterWriteDenied { block, register } => (
            "registers",
            format!("register `{block}.{register}` is not writable"),
        ),
        TargetMachineError::DuplicateInterrupt { name } => (
            "interrupts",
            format!("interrupt `{name}` is declared more than once"),
        ),
        TargetMachineError::DuplicateInterruptVector { vector } => (
            "interrupts",
            format!("interrupt vector {vector} is declared more than once"),
        ),
        TargetMachineError::InvalidInterruptEffect { interrupt, effect } => (
            "interrupts",
            format!("interrupt `{interrupt}` has invalid effect `{effect}`"),
        ),
        TargetMachineError::UnknownInterrupt { interrupt } => (
            "interrupts",
            format!("interrupt `{interrupt}` is not in the target vector table"),
        ),
        TargetMachineError::InterruptEffectForbidden {
            interrupt,
            handler,
            effect,
        } => (
            "interrupts",
            format!("handler `{handler}` for `{interrupt}` uses forbidden effect `{effect}`"),
        ),
        TargetMachineError::InterruptHandlerUnbounded { interrupt, handler } => (
            "interrupts",
            format!("handler `{handler}` for `{interrupt}` is not bounded"),
        ),
        TargetMachineError::DuplicateDmaChannel { name } => (
            "DMA",
            format!("DMA channel `{name}` is declared more than once"),
        ),
        TargetMachineError::DuplicateDmaChannelNumber { channel } => (
            "DMA",
            format!("DMA channel number {channel} is declared more than once"),
        ),
        TargetMachineError::DmaTransferSizeZero { channel } => (
            "DMA",
            format!("DMA channel `{channel}` has a zero transfer limit"),
        ),
        TargetMachineError::UnknownDmaChannel { channel } => (
            "DMA",
            format!("DMA channel `{channel}` is not in the target profile"),
        ),
        TargetMachineError::DmaOwnershipMismatch {
            channel,
            buffer,
            expected,
            actual,
        } => (
            "DMA",
            format!(
                "DMA `{channel}` buffer `{buffer}` expects {} ownership, got {}",
                dma_owner_name(*expected),
                dma_owner_name(*actual)
            ),
        ),
        TargetMachineError::DmaStartWhileInFlight { channel, buffer } => (
            "DMA",
            format!("DMA `{channel}` cannot start while buffer `{buffer}` is in flight"),
        ),
        TargetMachineError::DmaWaitWithoutTransfer { channel, buffer } => (
            "DMA",
            format!("DMA `{channel}` cannot wait for buffer `{buffer}` before start"),
        ),
        TargetMachineError::DmaBufferUnavailable { channel, buffer } => (
            "DMA",
            format!("buffer `{buffer}` is unavailable while DMA `{channel}` owns it"),
        ),
        TargetMachineError::DmaWaitChannelMismatch { channel, buffer } => (
            "DMA",
            format!("DMA `{channel}` does not own in-flight buffer `{buffer}`"),
        ),
        _ => ("target", "the selected target fact is invalid".to_string()),
    }
}

fn memory_kind_name(kind: jet_foundation::TargetMachine::MemoryKind) -> &'static str {
    match kind {
        jet_foundation::TargetMachine::MemoryKind::Flash => "flash",
        jet_foundation::TargetMachine::MemoryKind::Ram => "RAM",
        jet_foundation::TargetMachine::MemoryKind::Mmio => "MMIO",
        jet_foundation::TargetMachine::MemoryKind::Reserved => "reserved",
    }
}

fn dma_owner_name(owner: jet_foundation::TargetMachine::TargetDmaOwner) -> &'static str {
    match owner {
        jet_foundation::TargetMachine::TargetDmaOwner::Cpu => "CPU",
        jet_foundation::TargetMachine::TargetDmaOwner::Device => "device",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::TargetMachine::{
        ByteSize, ClockPolicy, MmioAccess, ProviderContract, TargetCapability, UnsafeGate,
    };

    fn row<'a>(diagnostics: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
        diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap_or_else(|| panic!("missing {code} diagnostic: {diagnostics:?}"))
    }

    fn assert_row(diagnostics: &[Diagnostic], code: &str, what: &str, why: &str, fix: &str) {
        let diagnostic = row(diagnostics, code);
        assert_eq!(diagnostic.what, what);
        assert_eq!(diagnostic.why, why);
        assert_eq!(diagnostic.fix, fix);
    }

    #[test]
    fn target_machine_rows_are_registered_and_stable() {
        let machine = TargetMachine::wasm_no_os();
        let diagnostics =
            check_target_machine(&machine, &TargetMachineUse::from_core_apis(["core.files"]));
        assert_row(
            &diagnostics,
            "E3310",
            "Prelude part `core.files` needs the `hosted` runtime layer, but target `wasm.no-os` provides `core`.",
            "The complete semantic Prelude closure includes `core.files`; a target cannot emit a higher layer or substitute a second Prelude.",
            "Select a typed target with `hosted` support, or remove `core.files` from the reachable closure.",
        );

        let usage = TargetMachineUse {
            required_capabilities: vec![TargetCapability::TimeWall],
            ..TargetMachineUse::default()
        };
        assert_row(
            &check_target_machine(&machine, &usage),
            "E3311",
            "Target `wasm.no-os` lacks the required `Time.Wall` capability.",
            "The selected target profile must state each machine service separately; a target triple cannot imply a UART, clock, entropy source, scheduler, allocator, startup provider, or MMIO device.",
            "Select a typed target profile with a `Time.Wall` provider, or remove the reachable operation.",
        );

        let mut invalid_provider = machine.clone();
        invalid_provider.monotonic_clock = ClockPolicy::Provider {
            provider: ProviderContract::new("", "not-a-digest"),
        };
        assert_row(
            &check_target_machine(
                &invalid_provider,
                &TargetMachineUse {
                    required_capabilities: vec![TargetCapability::TimeMonotonic],
                    ..TargetMachineUse::default()
                },
            ),
            "E3312",
            "Provider `` for `Time.Monotonic` on target `wasm.no-os` has invalid digest `not-a-digest`.",
            "Every selected provider needs a stable identity and sha256 provenance so artifacts cannot reuse changed target behavior.",
            "Declare a nonempty provider identity and a `sha256:` digest for `Time.Monotonic`.",
        );

        assert_row(
            &check_target_machine(
                &machine,
                &TargetMachineUse {
                    mmio: vec![MmioAccess {
                        address: 0x10,
                        size: ByteSize::bytes(4),
                        unsafe_gate: Some(UnsafeGate {
                            reason: "probe".to_string(),
                        }),
                    }],
                    ..TargetMachineUse::default()
                },
            ),
            "E3313",
            "Target `wasm.no-os` has an MMIO fact failure: address 0x10 (4 bytes) is outside a declared MMIO region.",
            "MMIO addresses must lie inside a declared MMIO region and every access must carry a nonempty `#Unsafe` reason.",
            "Declare a matching MMIO provider and region, then place the access inside `#Unsafe(\"reason\")`.",
        );

        let broken = TargetMachine::bare_metal("broken", "");
        assert_row(
            &check_target_machine(&broken, &TargetMachineUse::default()),
            "E3314",
            "Target `broken` has an invalid `target triple` fact: the selected target triple is empty.",
            "Target memory, linker, allocator, panic, startup, and execution facts are explicit inputs to sema and cannot be recovered safely from a triple.",
            "Select a complete typed target profile and correct the `target triple` declaration.",
        );
    }
}
