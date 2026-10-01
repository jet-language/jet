use super::{BootstrapCodecSymbols, BootstrapHostCodecError};
use crate::Codegen::MIRRust::MirRustTraitMethodMetadata;
use std::fmt::Write as _;

/// Adapter methods the host answers with a native operation. Each takes the
/// Source machine first and is served by `__jet_bootstrap_native_{name}`; any
/// other machine method of the trait is answered as unavailable (`None`), which
/// the evaluator reports as an explicit unsupported-host diagnostic.
const HOST_OPERATIONS: &[&str] = &["host_call", "foreign_call", "handle_call"];

pub(crate) fn emit_bootstrap_native_adapter_impl(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let trait_symbol = symbols.trait_symbol("JetEvalHostAdapter")?;
    // The Jet trait is the only method list; every method it declares gets a
    // trait-impl body and an interface binding below.
    let methods = symbols.trait_methods("JetEvalHostAdapter")?;
    for metadata in &methods {
        let name = &metadata.name;
        if metadata.receiver_access.is_none()
            || metadata.parameter_types.len() != metadata.parameter_access.len()
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetEvalHostAdapter.{name} has an incompatible checked Rust signature"
            )));
        }
        if !metadata.parameter_access.is_empty()
            && metadata.parameter_access[0] != jet_foundation::MIR::MirAccess::Write
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetEvalHostAdapter.{name} does not expose the required checked Machine WRITE borrow"
            )));
        }
    }
    let first_method = methods[0];

    let host_adapter = symbols.field_binding("JetEvalConfig", "host_adapter")?;
    let receiver_type = host_adapter.ty.option_inner().ok_or_else(|| {
        BootstrapHostCodecError::InvalidMetadata(
            "JetEvalConfig.host_adapter is not the checked optional adapter field".to_string(),
        )
    })?;
    let adapter_trait = symbols
        .metadata
        .traits
        .iter()
        .find(|row| row.trait_id == first_method.trait_id)
        .ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(
                "JetEvalHostAdapter method metadata names no checked trait".to_string(),
            )
        })?;
    // Trait-object bounds are nominal refs; match the checked trait by name.
    if receiver_type.trait_bounds().is_none_or(|bounds| {
        !bounds
            .iter()
            .any(|bound| bound.name == adapter_trait.name || bound.name == adapter_trait.key)
    }) {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "JetEvalConfig.host_adapter does not carry the exact JetEvalHostAdapter trait bound"
                .to_string(),
        ));
    }

    let source_program = symbols.type_symbol("MIRProgram")?;
    let source_binding_type = symbols.type_symbol("JetEvalNativeBindingIdentity")?;
    let source_binding_identity = "::jet_jit::SourceResources::SourceNativeBindingIdentity";
    let resource_handle = "::jet_jit::SourceResources::SourceResourceHandle";
    let expected_receiver_key = format!("{:?}", receiver_type.canonical_key());
    emit_native_callback_jobs(out)?;
    writeln!(
        out,
        "#[doc(hidden)]\n\
         struct __JetBootstrapNativeBindingRegistration {{\n\
             field_path: Vec<String>,\n\
             binding: {source_binding_type},\n\
             identity: {source_binding_identity},\n\
             capability: {resource_handle},\n\
             dynamic: bool,\n\
         }}\n\
         type __JetBootstrapNativePacket = (::jet_foundation::MIR::MirType, ::jet_foundation::MIR::MirRuntimeValue);\n\
         #[doc(hidden)]\n\
         struct __JetBootstrapNativeAdapterRoot {{
             bindings: ::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,
             program: ::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
             source_program: ::std::sync::Arc<{source_program}>,
             machine_abi_shape: crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,\n\
             shared_payload_shapes: ::std::collections::BTreeMap<String, crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape>,\n\
             completion_scope: ::jet_jit::SourceExecutionCompletionScopeWeak,\n\
             callback_jobs: ::std::sync::Weak<dyn ::jet_jit::SourceCallbacks::SourceCallbackJobOwner>,\n\
             resources_weak: ::jet_jit::SourceResources::SourceResourceLeaseWeak,\n\
             artifact: ::jet_foundation::MIR::MirArtifactId,
             receiver_type: ::jet_foundation::MIR::MirType,
             execution: ::jet_foundation::MIR::MirExecutionIdentity,
             execution_policy: crate::BootstrapFactoryTier,
             helper_roots: Vec<::jet_foundation::MIR::MirFunctionId>,
             task_callback_invoke: ::jet_foundation::MIR::MirFunctionId,
             task_root_release: ::jet_foundation::MIR::MirFunctionId,
             owned_root_drop: ::jet_foundation::MIR::MirFunctionId,
             owned_callback_result_drop: ::jet_foundation::MIR::MirFunctionId,\n\
             callback_transfer: ::jet_foundation::MIR::MirFunctionId,\n\
             callback_release: ::jet_foundation::MIR::MirFunctionId,\n\
             task_root_take: ::jet_foundation::MIR::MirFunctionId,\n\
             task_root_cleanup_root: ::jet_foundation::MIR::MirFunctionId,\n\
             owned_root_cleanup_clone: ::jet_foundation::MIR::MirFunctionId,\n\
            owned_root_drop_values: ::jet_foundation::MIR::MirFunctionId,\n\
            shared_payload_finalize: ::jet_foundation::MIR::MirFunctionId,\n\
             template: ::jet_jit::SourceInterfaces::NativeInterfaceObject,
             native_bindings: ::std::sync::Mutex<Vec<__JetBootstrapNativeBindingRegistration>>,
         }}
         #[doc(hidden)]\n\
         #[derive(Clone)]\n\
         struct __JetBootstrapNativeAdapterInstance {{
             root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,
             owner: ::std::sync::Weak<__JetBootstrapNativeAdapterOwner>,
         }}\n\
         #[doc(hidden)]\n\
         struct __JetBootstrapNativeAdapterOwner {{\n\
             root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,\n\
             callbacks: __JetBootstrapNativeCallbackSession,\n\
             resources: ::jet_jit::SourceResources::SourceResourceLease,\n\
             cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,\n\
             instance: ::std::sync::OnceLock<::jet_jit::SourceInterfaces::NativeInterfaceObject>,\n\
             capability: ::std::sync::OnceLock<::jet_jit::SourceResources::SourceResourceHandle>,\n\
             callback_failed: ::std::sync::Arc<::std::sync::atomic::AtomicBool>,\n\
             context: ::std::sync::Arc<__JetBootstrapNativeCallbackContext>,\n\
             borrowed_session: bool,\n\
         }}\n\
         impl Drop for __JetBootstrapNativeAdapterOwner {{
             fn drop(&mut self) {{
                 if let Some(capability) = self.capability.take() {{
                     if let Err(error) = self.resources.arena().release_capability(&capability) {{
                         self.context.record_failure(::jet_jit::SourceDeoptError::InvalidRequest(error));
                     }}
                 }}
                 let _ = self.instance.take();
             }}
         }}
         #[doc(hidden)]\n\
         #[derive(Clone)]\n\
         struct __JetBootstrapNativeAdapter {{\n\
             root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,\n\
             resources: ::jet_jit::SourceResources::SourceResourceLease,\n\
             cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,\n\
             callbacks: __JetBootstrapNativeCallbackSession,\n\
             instance: ::jet_jit::SourceInterfaces::NativeInterfaceObject,\n\
             capability: ::jet_jit::SourceResources::SourceResourceHandle,\n\
             owner: ::std::sync::Arc<__JetBootstrapNativeAdapterOwner>,\n\
             borrowed_session: bool,\n\
         }}\n\
         fn __jet_bootstrap_native_adapter_template(\n\
             bindings: &::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,\n\
             program: &::jet_foundation::MIR::MirProgram,\n\
             artifact: ::jet_foundation::MIR::MirArtifactId,\n\
             receiver_type: ::jet_foundation::MIR::MirType,\n\
         ) -> Result<::jet_jit::SourceInterfaces::NativeInterfaceObject, String> {{\n\
             if receiver_type.canonical_key() != {expected_receiver_key} {{\n\
                 return Err(\"native adapter receiver type differs from the exact checked JetEvalConfig.host_adapter leaf\".to_string());\n\
             }}\n\
             let template = bindings.create_object(()).map_err(|error| error.to_string())?;\n"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    for metadata in &methods {
        let method_name = &metadata.name;
        let method_id = metadata.method_id.0;
        let trait_id = metadata.trait_id.0;
        let receiver_access = mir_access_expression(
            metadata.receiver_access.expect("validated checked receiver access"),
        );
        let parameter_accesses = metadata
            .parameter_access
            .iter()
            .map(|access| mir_access_expression(*access))
            .collect::<Vec<_>>()
            .join(", ");
        let expected_len = metadata.parameter_types.len();
        writeln!(
            out,
            "    {{\n\
             let descriptor = ::jet_jit::SourceInterfaces::NativeInterfaceMethod::checked(\n\
                 program,\n\
                 artifact,\n\
                 ::jet_foundation::MIR::MirTraitId({trait_id}),\n\
                 ::jet_foundation::MIR::MirTraitMethodId({method_id}),\n\
                 receiver_type.clone(),\n\
             ).map_err(|error| error.to_string())?;\n\
             let expected_parameter_access = [{parameter_accesses}];\n\
             let checked_trait = program.traits.iter().find(|row| row.id == ::jet_foundation::MIR::MirTraitId({trait_id}))\n\
                 .ok_or_else(|| \"checked JetEvalHostAdapter trait disappeared from MIR\".to_string())?;\n\
             let checked_method = checked_trait.methods.iter().find(|row| row.id == ::jet_foundation::MIR::MirTraitMethodId({method_id}))\n\
                 .ok_or_else(|| \"checked JetEvalHostAdapter method disappeared from MIR\".to_string())?;\n\
             if descriptor.identity.method_name != {method_name:?}\n\
                 || descriptor.signature.receiver_access != {receiver_access}\n\
                 || descriptor.signature.parameters.len() != {expected_len}\n\
                 || checked_method.params.len() != {expected_len}\n\
                 || descriptor.signature.parameters.iter().zip(checked_method.params.iter()).any(|(actual, checked)| actual.index != checked.index || actual.name != checked.name || !actual.ty.same_checked_type(&checked.ty) || actual.access != checked.access || actual.ownership != checked.ownership || actual.public_label != checked.public_label || actual.variadic != checked.variadic || actual.default_present != checked.default_present)\n\
                 || descriptor.signature.parameters.iter().zip(expected_parameter_access.iter()).any(|(actual, expected)| actual.access != *expected)\n\
                 || checked_method.params.iter().zip(expected_parameter_access.iter()).any(|(actual, expected)| actual.access != *expected)\n\
                 || !descriptor.signature.return_type.same_checked_type(&checked_method.return_type)\n\
             {{\n\
                 return Err(format!(\"JetEvalHostAdapter.{method_name} Rust metadata disagrees with its checked MIR interface signature\"));\n\
             }}\n\
             let handler: ::jet_jit::SourceInterfaces::NativeInterfaceHandler = ::std::sync::Arc::new(|object, call| {{\n\
                 let instance = object.root::<__JetBootstrapNativeAdapterInstance>()\n\
                     .ok_or_else(|| \"native adapter carrier has no callback-scope context\".to_string())?;\n\
                 __jet_bootstrap_native_interface_{method_name}(instance, call)\n\
             }});\n\
             bindings.bind(template.clone(), descriptor.identity, descriptor.signature, handler)\n\
                 .map_err(|error| error.to_string())?;\n\
             }}\n"
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }

    writeln!(
        out,
        "    Ok(template)\n\
         }}\n\
         impl __JetBootstrapNativeAdapter {{\n\
             fn new_with_bindings(\n\
                 resources: ::jet_jit::SourceResources::SourceResourceLease,\n\
                 bindings: ::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,\n\
                 program: ::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
                 source_program: ::std::sync::Arc<{source_program}>,\n\
                 machine_abi_shape: crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,\n\
                 shared_payload_shapes: ::std::collections::BTreeMap<String, crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape>,\n\
                 completion_scope: ::jet_jit::SourceExecutionCompletionScope,\n\
                 callback_jobs: ::std::sync::Arc<dyn ::jet_jit::SourceCallbacks::SourceCallbackJobOwner>,\n\
                 artifact: ::jet_foundation::MIR::MirArtifactId,\n\
                 receiver_type: ::jet_foundation::MIR::MirType,\n\
                 template: ::jet_jit::SourceInterfaces::NativeInterfaceObject,\n\
                 native_bindings: Vec<__JetBootstrapNativeBindingRegistration>,\n\
                 helper_roots: Vec<::jet_foundation::MIR::MirFunctionId>,\n\
                 task_callback_invoke: ::jet_foundation::MIR::MirFunctionId,\n\
                 task_root_release: ::jet_foundation::MIR::MirFunctionId,\n\
                 owned_root_drop: ::jet_foundation::MIR::MirFunctionId,\n\
                 owned_callback_result_drop: ::jet_foundation::MIR::MirFunctionId,\n\
                 callback_transfer: ::jet_foundation::MIR::MirFunctionId,\n\
                 callback_release: ::jet_foundation::MIR::MirFunctionId,\n\
                 task_root_take: ::jet_foundation::MIR::MirFunctionId,\n\
                 task_root_cleanup_root: ::jet_foundation::MIR::MirFunctionId,\n\
                 owned_root_cleanup_clone: ::jet_foundation::MIR::MirFunctionId,\n\
                owned_root_drop_values: ::jet_foundation::MIR::MirFunctionId,\n\
                shared_payload_finalize: ::jet_foundation::MIR::MirFunctionId,\n\
                 execution_policy: crate::BootstrapFactoryTier,\n\
             ) -> Result<Self, String> {{\n\
                 if receiver_type.canonical_key() != {expected_receiver_key} {{\n\
                     return Err(\"native adapter constructor receiver type differs from the checked host-adapter field\".to_string());\n\
                 }}\n\
                 let execution = program.execution_identity(Some(artifact)).map_err(|error| error.to_string())?;\n\
                 let root = ::std::sync::Arc::new(__JetBootstrapNativeAdapterRoot {{\n\
                     bindings,\n\
                     program,\n\
                     source_program,\n\
                     machine_abi_shape,\n\
                     shared_payload_shapes,\n\
                     completion_scope: completion_scope.downgrade(),\n\
                     callback_jobs: ::std::sync::Arc::downgrade(&callback_jobs),\n\
                     resources_weak: resources.downgrade(),\n\
                     artifact,\n\
                     execution,\n\
                     execution_policy,\n\
                     helper_roots,\n\
                     task_callback_invoke,\n\
                     task_root_release,\n\
                     owned_root_drop,\n\
                     owned_callback_result_drop,\n\
                     callback_transfer,\n\
                     callback_release,\n\
                     task_root_take,\n\
                     task_root_cleanup_root,\n\
                     owned_root_cleanup_clone,\n\
                     owned_root_drop_values,\n\
                    shared_payload_finalize,\n\
                     receiver_type,\n\
                     template,\n\
                     native_bindings: ::std::sync::Mutex::new(native_bindings),\n\
                 }});\n\
                __jet_bootstrap_native_adapter_new_scope(root, resources, None)\n\
             }}\n\
\n\
         }}\n\
         impl {trait_symbol} for __JetBootstrapNativeAdapter {{"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    for metadata in &methods {
        let receiver = match metadata.receiver_access {
            Some(jet_foundation::MIR::MirAccess::Read) => "&self",
            Some(jet_foundation::MIR::MirAccess::Write) => "&mut self",
            Some(jet_foundation::MIR::MirAccess::Move) | None => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetEvalHostAdapter.{} has no borrowed (read or write) checked receiver",
                    metadata.name
                )))
            }
        };
        let parameters = metadata
            .parameter_types
            .iter()
            .enumerate()
            .map(|(index, ty)| format!("__jet_arg_{index}: {ty}"))
            .collect::<Vec<_>>()
            .join(", ");
        let separator = if parameters.is_empty() { "" } else { ", " };
        let result = match metadata.name.as_str() {
            "new_session" => "if self.borrowed_session { Box::new(self.clone()) } else { Box::new(__jet_bootstrap_native_adapter_new_scope(self.root.clone(), self.resources.clone(), self.cleanup.clone()).unwrap_or_else(|error| panic!(\"Source callback-scope adapter construction failed: {error}\"))) }".to_string(),
            "clone_adapter" => "Box::new(self.clone())".to_string(),
            "poll_callbacks" => "__jet_bootstrap_native_poll_callbacks(&self.root, &self.callbacks, __jet_arg_0)".to_string(),
            "drain_callbacks" => "__jet_bootstrap_native_drain_callbacks(&self.root, &self.callbacks, __jet_arg_0)".to_string(),
            "physical_binding" => "__jet_bootstrap_native_adapter_physical_binding(self)".to_string(),
            _ if metadata.parameter_types.is_empty() => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetEvalHostAdapter.{} has no native adapter implementation",
                    metadata.name
                )))
            }
            _ => {
                let arguments = native_operation_arguments(metadata);
                let (physical_binding, operation) = native_operation(metadata);
                let physical_binding = if physical_binding {
                    "let compiler_program = self.root.program.as_ref(); let physical = __JetBootstrapNativePhysicalBindings::new(self); "
                } else {
                    ""
                };
                format!(
                    "{{ let _jet_resource_scope = self.resources.activate(); {physical_binding}let mut machine = crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess::Typed(__jet_arg_0); {operation}(&mut machine{arguments}) }}"
                )
            }
        };
        let return_type = &metadata.return_type;
        writeln!(
            out,
            "    fn {}({receiver}{separator}{parameters}) -> {return_type} {{ {result} }}",
            metadata.symbol,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    writeln!(out, "}}")
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    emit_native_unavailable_operations(out, &methods)?;
    emit_native_interface_helpers(out, &methods)?;
    emit_native_callback_scope_helpers(out)?;
    emit_native_callback_helpers(out, symbols)?;
    emit_native_binding_helpers(out, symbols)
}

/// The Rust helper serving a machine method, and whether it also takes the
/// compiler program and physical bindings (`host_call` resolves Source
/// resources through them).
fn native_operation(metadata: &MirRustTraitMethodMetadata) -> (bool, String) {
    let name = metadata.name.as_str();
    if HOST_OPERATIONS.contains(&name) {
        (name == "host_call", format!("__jet_bootstrap_native_{name}"))
    } else {
        (false, format!("__jet_bootstrap_native_unavailable_{name}"))
    }
}

/// `, __jet_arg_1, ...` for the operation arguments after the machine, plus
/// the program and physical bindings when the helper needs them.
fn native_operation_arguments(metadata: &MirRustTraitMethodMetadata) -> String {
    let mut arguments = (1..metadata.parameter_types.len())
        .map(|index| format!(", __jet_arg_{index}"))
        .collect::<String>();
    if native_operation(metadata).0 {
        arguments.push_str(", compiler_program, &physical");
    }
    arguments
}

/// Machine methods without a host implementation answer `None`: the Source
/// evaluator reports the operation as unavailable to the typed host adapter.
fn emit_native_unavailable_operations(
    out: &mut String,
    methods: &[&MirRustTraitMethodMetadata],
) -> Result<(), BootstrapHostCodecError> {
    for metadata in methods {
        let name = metadata.name.as_str();
        if metadata.parameter_types.is_empty()
            || matches!(name, "poll_callbacks" | "drain_callbacks")
            || HOST_OPERATIONS.contains(&name)
        {
            continue;
        }
        let parameters = metadata.parameter_types[1..]
            .iter()
            .map(|ty| format!(", _: {ty}"))
            .collect::<String>();
        let return_type = &metadata.return_type;
        writeln!(
            out,
            "fn __jet_bootstrap_native_unavailable_{name}<T>(\n\
             \x20   _machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>{parameters},\n\
             ) -> {return_type} {{\n\
             \x20   Err(Default::default())\n\
             }}"
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    Ok(())
}

fn mir_access_expression(access: jet_foundation::MIR::MirAccess) -> &'static str {
    match access {
        jet_foundation::MIR::MirAccess::Read => "::jet_foundation::MIR::MirAccess::Read",
        jet_foundation::MIR::MirAccess::Write => "::jet_foundation::MIR::MirAccess::Write",
        jet_foundation::MIR::MirAccess::Move => "::jet_foundation::MIR::MirAccess::Move",
    }
}
fn emit_native_callback_jobs(out: &mut String) -> Result<(), BootstrapHostCodecError> {
    out.push_str(
        r#"#[doc(hidden)]
struct __JetBootstrapNativeCallbackJobState {
    admission_closed: bool,
    open_sessions: usize,
    active: Vec<::jet_jit::JetSchedulerJoin<Result<(), ::jet_jit::JetTaskFailure>>>,
    failures: Vec<::jet_jit::JetTaskFailure>,
    draining: bool,
    transitions: usize,
}

#[doc(hidden)]
struct __JetBootstrapNativeCallbackJobs {
    state: ::std::sync::Mutex<__JetBootstrapNativeCallbackJobState>,
}

impl __JetBootstrapNativeCallbackJobs {
    pub(crate) fn new() -> ::std::sync::Arc<Self> {
        ::std::sync::Arc::new(Self {
            state: ::std::sync::Mutex::new(__JetBootstrapNativeCallbackJobState {
                admission_closed: false,
                open_sessions: 0,
                active: Vec::new(),
                failures: Vec::new(),
                draining: false,
                transitions: 0,
            }),
        })
    }

    fn lock_state(&self) -> ::std::sync::MutexGuard<'_, __JetBootstrapNativeCallbackJobState> {
        self.state
            .lock()
            .unwrap_or_else(::std::sync::PoisonError::into_inner)
    }

    fn schedule(
        &self,
        job: ::jet_jit::SourceCallbacks::SourceCallbackJob,
    ) -> Result<(), ::jet_jit::SourceCallbacks::SourceCallbackJob> {
        let job_slot = ::std::sync::Arc::new(::std::sync::Mutex::new(Some(job)));
        {
            let mut state = self.lock_state();
            if state.admission_closed && state.open_sessions == 0 {
                return Err(job_slot
                    .lock()
                    .unwrap_or_else(::std::sync::PoisonError::into_inner)
                    .take()
                    .expect("rejected callback job missing"));
            }
            let Some(reservations) = state.transitions.checked_add(1) else {
                return Err(job_slot
                    .lock()
                    .unwrap_or_else(::std::sync::PoisonError::into_inner)
                    .take()
                    .expect("rejected callback job missing"));
            };
            if state.active.try_reserve(reservations).is_err() {
                return Err(job_slot
                    .lock()
                    .unwrap_or_else(::std::sync::PoisonError::into_inner)
                    .take()
                    .expect("rejected callback job missing"));
            }
            state.transitions += 1;
        }

        let task_slot = ::std::sync::Arc::clone(&job_slot);
        let spawned = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(move || {
            ::jet_jit::jet_scheduler_spawn(move || {
                let job = task_slot
                    .lock()
                    .unwrap_or_else(::std::sync::PoisonError::into_inner)
                    .take()
                    .expect("callback job was already taken");
                job()
            })
        }));
        match spawned {
            Ok(join) => {
                let mut state = self.lock_state();
                state.active.push(join);
                state.transitions -= 1;
                Ok(())
            }
            Err(payload) => {
                let job = job_slot
                    .lock()
                    .unwrap_or_else(::std::sync::PoisonError::into_inner)
                    .take();
                if let Some(job) = job {
                    self.lock_state().transitions -= 1;
                    Err(job)
                } else {
                    // The scheduler accepted the task before unwinding; keep its
                    // completion debt so no later drain can claim it was joined.
                    ::std::panic::resume_unwind(payload);
                }
            }
        }
    }

    fn drain_jobs(&self) -> ::jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome {
        loop {
            let handle = {
                let mut state = self.lock_state();
                if state.active.is_empty() {
                    if state.admission_closed
                        && state.open_sessions == 0
                        && state.transitions == 0
                    {
                        return ::jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome::Complete {
                            failures: state.failures.clone(),
                        };
                    }
                    return ::jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome::Pending {
                        open_callback_sessions: state.open_sessions,
                        pending_jobs: state.active.len() + state.transitions,
                    };
                }
                state.failures.reserve(1);
                state.transitions += 1;
                state.active.swap_remove(0)
            };
            let mut handle = Some(handle);
            let joined = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                handle.as_mut().expect("callback job handle missing").join()
            }));
            match joined {
                Ok(joined) => {
                    let mut state = self.lock_state();
                    state.transitions -= 1;
                    if let Ok(Err(failure)) | Err(failure) = joined {
                        state.failures.push(failure);
                    }
                    drop(state);
                    drop(handle.take());
                }
                Err(payload) => {
                    let handle = handle.take().expect("callback job handle missing");
                    let mut state = self.lock_state();
                    state.active.push(handle);
                    state.transitions -= 1;
                    drop(state);
                    ::std::panic::resume_unwind(payload);
                }
            }
        }
    }
}

impl ::jet_jit::SourceCallbacks::SourceCallbackJobOwner for __JetBootstrapNativeCallbackJobs {
    fn session_started(&self) -> Result<(), String> {
        let mut state = self.lock_state();
        if state.admission_closed {
            return Err("native callback-job session admission is closed".to_string());
        }
        let Some(open_sessions) = state.open_sessions.checked_add(1) else {
            return Err("native callback-job session count overflow".to_string());
        };
        state.open_sessions = open_sessions;
        Ok(())
    }

    fn session_finished(&self) {
        let mut state = self.lock_state();
        if state.open_sessions == 0 {
            panic!("native callback-job session registration underflow");
        }
        state.open_sessions -= 1;
    }

    fn close_admission(&self) {
        self.lock_state().admission_closed = true;
    }

    fn submit(
        &self,
        job: ::jet_jit::SourceCallbacks::SourceCallbackJob,
    ) -> Result<(), ::jet_jit::SourceCallbacks::SourceCallbackJob> {
        self.schedule(job)
    }

    fn drain(&self) -> ::jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome {
        {
            let mut state = self.lock_state();
            if state.draining {
                return ::jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome::Pending {
                    open_callback_sessions: state.open_sessions,
                    pending_jobs: state.active.len() + state.transitions,
                };
            }
            state.draining = true;
        }
        let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
            self.drain_jobs()
        }));
        self.lock_state().draining = false;
        match result {
            Ok(outcome) => outcome,
            Err(payload) => ::std::panic::resume_unwind(payload),
        }
    }
}
"#,
    );
    Ok(())
}


fn emit_native_interface_helpers(
    out: &mut String,
    methods: &[&MirRustTraitMethodMetadata],
) -> Result<(), BootstrapHostCodecError> {
    out.push_str(
        r#"fn __jet_bootstrap_native_adapter_for_interface_call(
            instance: &__JetBootstrapNativeAdapterInstance,
            call: &::jet_jit::SourceInterfaces::NativeInterfaceCall,
            trait_id: ::jet_foundation::MIR::MirTraitId,
            method_id: ::jet_foundation::MIR::MirTraitMethodId,
            method_name: &str,
        ) -> Result<__JetBootstrapNativeAdapter, String> {
            let owner = instance.owner.upgrade()
                .ok_or_else(|| "native adapter callback scope has retired".to_string())?;
            if !::std::sync::Arc::ptr_eq(&instance.root, &owner.root) {
                return Err("native adapter instance and owner have different immutable roots".to_string());
            }
            let checked = ::jet_jit::SourceInterfaces::NativeInterfaceMethod::checked(
                owner.root.program.as_ref(),
                owner.root.artifact,
                trait_id,
                method_id,
                owner.root.receiver_type.clone(),
            ).map_err(|error| error.to_string())?;
            if checked.identity.method_name != method_name
                || checked.identity != *call.identity()
                || checked.signature != *call.signature()
            {
                return Err(format!("native adapter `{method_name}` call differs from its checked compiler-image method"));
            }
            let instance = owner.instance.get()
                .ok_or_else(|| "native adapter owner has no callback-scope interface instance".to_string())?
                .clone();
            let capability = owner.capability.get()
                .ok_or_else(|| "native adapter owner has no callback-scope resource capability".to_string())?
                .clone();
            Ok(__JetBootstrapNativeAdapter {
                root: owner.root.clone(),
                resources: owner.resources.clone(),
                cleanup: owner.cleanup.clone(),
                callbacks: owner.callbacks.clone(),
                instance,
                capability,
                borrowed_session: owner.borrowed_session,
                owner,
            })
        }
"#,
    );

    for metadata in methods {
        let name = metadata.name.as_str();
        let trait_id = metadata.trait_id.0;
        let method_id = metadata.method_id.0;
        let body = match name {
            "new_session" => r#"if !call.arguments().is_empty() {
                return Err("checked new_session call unexpectedly has arguments".to_string());
            }
            let child = __jet_bootstrap_native_adapter_new_scope(
                adapter.root.clone(),
                adapter.resources.clone(),
                adapter.cleanup.clone(),
            )?;
            Ok(child.instance.as_runtime_value())"#
                .to_string(),
            "clone_adapter" => r#"if !call.arguments().is_empty() {
                return Err("checked clone_adapter call unexpectedly has arguments".to_string());
            }
            Ok(adapter.instance.as_runtime_value())"#
                .to_string(),
            _ => {
                let closure = match name {
                    // Callback pumping runs Source callbacks against the typed
                    // machine; a MIR-borrowed machine cannot host them.
                    "poll_callbacks" | "drain_callbacks" => format!(
                        "|mut machine| match machine.typed_mut() {{ Some(machine) => Ok(__jet_bootstrap_native_{name}(&adapter.root, &adapter.callbacks, machine)), None => Err(\"native adapter {name} needs the typed Source machine\".to_string()) }}"
                    ),
                    "physical_binding" => {
                        "|| Ok(__jet_bootstrap_native_adapter_physical_binding(&adapter))".to_string()
                    }
                    _ if metadata.parameter_types.is_empty() => {
                        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                            "JetEvalHostAdapter.{name} has no native interface handler"
                        )))
                    }
                    _ => {
                        let parameters = (1..metadata.parameter_types.len())
                            .map(|index| format!(", __jet_arg_{index}"))
                            .collect::<String>();
                        let arguments = native_operation_arguments(metadata);
                        let (_, operation) = native_operation(metadata);
                        format!(
                            "|machine{parameters}| {{ let mut machine = machine; Ok({operation}(&mut machine{arguments})) }}"
                        )
                    }
                };
                let compiler_program_binding = if native_operation(metadata).0 {
                    "let compiler_program = adapter.root.program.as_ref(); "
                } else {
                    ""
                };
                format!(
                    "let _jet_resource_scope = adapter.resources.activate(); {compiler_program_binding}let physical = __JetBootstrapNativePhysicalBindings::new(&adapter);\n\
                     crate::__jet_bootstrap_entry_native_interface_{name}(\n\
                         call,\n\
                         &adapter.root.program,\n\
                         &adapter.root.machine_abi_shape,\n\
                         &physical,\n\
                         {closure},\n\
                     )"
                )
            }
        };
        let generated = format!(
            r#"fn __jet_bootstrap_native_interface_{name}(
                instance: &__JetBootstrapNativeAdapterInstance,
                call: &mut ::jet_jit::SourceInterfaces::NativeInterfaceCall,
            ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
                let adapter = __jet_bootstrap_native_adapter_for_interface_call(
                    instance,
                    call,
                    ::jet_foundation::MIR::MirTraitId({trait_id}),
                    ::jet_foundation::MIR::MirTraitMethodId({method_id}),
                    {name:?},
                )?;
                {body}
            }}
"#
        );
        out.push_str(&generated);
    }
    Ok(())
}


fn emit_native_callback_scope_helpers(out: &mut String) -> Result<(), BootstrapHostCodecError> {
    out.push_str(r#"
struct __JetBootstrapNativeCallbackContext {
    root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,
    resources: ::jet_jit::SourceResources::SourceResourceLeaseWeak,
    cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,
    registration: ::std::sync::Arc<::jet_jit::SourceCallbacks::SourceCallbackJobSessionRegistration>,
    completion_scope: ::jet_jit::SourceExecutionCompletionScope,
    failed: ::std::sync::Arc<::std::sync::atomic::AtomicBool>,
}

impl __JetBootstrapNativeCallbackContext {
    fn record_failure(&self, cause: ::jet_jit::SourceDeoptError) {
        self.failed.store(true, ::std::sync::atomic::Ordering::Release);
        self.completion_scope.record(::jet_jit::SourceExecutionCompletion {
            execution: Some(self.root.execution.clone()),
            function: None,
            lease: self.resources.upgrade(),
            disposition: ::jet_jit::SourceExecutionCompletionDisposition::NotInvoked {
                cause,
                entry_values: Vec::new(),
                write_borrow_indices: Vec::new(),
                completions: Vec::new(),
            },
        });
    }

    fn submit(&self, job: ::jet_jit::SourceCallbacks::SourceCallbackJob) {
        if let Err(job) = self.registration.submit(job) {
            // Only the exact unstarted job is returned by the shared owner ABI.
            // A mandatory transport hook cannot discard its owned packets.
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(job));
            let failure = match result {
                Ok(Ok(())) => return,
                Ok(Err(failure)) => failure,
                Err(payload) => {
                    let message = payload.downcast_ref::<String>().cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_string()))
                        .unwrap_or_else(|| "Source callback cleanup job panicked".to_string());
                    ::jet_jit::JetTaskFailure::Panicked(message)
                }
            };
            self.record_failure(::jet_jit::SourceDeoptError::CallbackJob(failure));
        }
    }
}

fn __jet_bootstrap_native_callback_transport(
    context: ::std::sync::Arc<__JetBootstrapNativeCallbackContext>,
) -> __JetBootstrapNativeCallbackSession {
    let abandoned_context = context.clone();
    __JetBootstrapNativeCallbackSession::new(
        move |abandonment| {
            if abandonment.is_empty() {
                return;
            }
            let job_context = abandoned_context.clone();
            abandoned_context.submit(Box::new(move || {
                let scope = job_context.completion_scope.clone();
                scope.with_current(|| __jet_bootstrap_native_callback_abandoned(job_context, abandonment))
            }));
        },
        move |ready| {
            let Some(callbacks) = ready.session() else {
                return;
            };
            let job_context = context.clone();
            context.submit(Box::new(move || {
                let scope = job_context.completion_scope.clone();
                scope.with_current(|| __jet_bootstrap_native_callback_ready(job_context, callbacks))
            }));
        },
    )
}

fn __jet_bootstrap_native_adapter_owner_from_context(
    context: ::std::sync::Arc<__JetBootstrapNativeCallbackContext>,
    callbacks: __JetBootstrapNativeCallbackSession,
    resources: ::jet_jit::SourceResources::SourceResourceLease,
    borrowed_session: bool,
) -> ::std::sync::Arc<__JetBootstrapNativeAdapterOwner> {
    ::std::sync::Arc::new(__JetBootstrapNativeAdapterOwner {
        root: context.root.clone(),
        callbacks,
        resources,
        cleanup: context.cleanup.clone(),
        instance: ::std::sync::OnceLock::new(),
        capability: ::std::sync::OnceLock::new(),
        callback_failed: context.failed.clone(),
        context,
        borrowed_session,
    })
}

fn __jet_bootstrap_native_adapter_owner_new(
    root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,
    resources: ::jet_jit::SourceResources::SourceResourceLease,
    cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,
) -> Result<::std::sync::Arc<__JetBootstrapNativeAdapterOwner>, String> {
    let completion_scope = root.completion_scope.upgrade()
        .ok_or_else(|| "Source callback origin has no live completion receiver".to_string())?;
    let jobs = root.callback_jobs.upgrade()
        .ok_or_else(|| "Source callback origin has no live job owner".to_string())?;
    let registration = ::jet_jit::SourceCallbacks::SourceCallbackJobSessionRegistration::start(jobs)?;
    let context = ::std::sync::Arc::new(__JetBootstrapNativeCallbackContext {
        root,
        resources: resources.downgrade(),
        cleanup,
        registration: ::std::sync::Arc::new(registration),
        completion_scope,
        failed: ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false)),
    });
    let callbacks = __jet_bootstrap_native_callback_transport(context.clone());
    Ok(__jet_bootstrap_native_adapter_owner_from_context(context, callbacks, resources, false))
}
"#);
    Ok(())
}

fn emit_native_callback_helpers(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let eval_value = symbols.type_symbol("JetEvalRuntimeValue")?;
    let eval_machine = symbols.type_symbol("JetEvalMachine")?;
    let callback_result = symbols.type_symbol("JetEvalCallbackResult")?;

    let span = symbols.type_symbol("Span")?;
    let callback_transfer = callable_symbol(symbols, "jet_eval_callback_transfer")?;
    let callback_invoke = callable_symbol(symbols, "jet_eval_callback_invoke")?;
    let callback_release = callable_symbol(symbols, "jet_eval_callback_release")?;
    let task_root = symbols.type_symbol("JetEvalTaskRoot")?;
    let task_root_take = callable_symbol(symbols, "jet_eval_task_root_take")?;
    let _task_callback_invoke = callable_symbol(symbols, "jet_eval_task_callback_invoke")?;
    let task_root_release = callable_symbol(symbols, "jet_eval_task_root_release")?;
    let callback_value = symbols.variant_path("JetEvalCallbackOutcome", "Value")?;
    let merge_callback_output = callable_symbol(symbols, "jet_eval_merge_callback_output")?;
    let callback_session = "::jet_jit::SourceCallbacks::SourceCallbackSession";
    let callback_lease = "::jet_jit::SourceCallbacks::SourceCallbackLease";
    let callback_event = "::jet_jit::SourceCallbacks::SourceCallbackEvent";
    let callback_error = "::jet_jit::SourceCallbacks::SourceCallbackError";
    let callback_reply_error = "::jet_jit::SourceCallbacks::SourceCallbackReplyError";
    let drop_value = callable_symbol(symbols, "jet_eval_drop_value")?;
    out.push_str(
        r#"
fn __jet_bootstrap_native_record_completion_box(
    root: &__JetBootstrapNativeAdapterRoot,
    completion: ::jet_jit::SourceExecutionCompletionBox,
) -> Result<(), ::jet_jit::SourceExecutionCompletionBox> {
    match ::jet_jit::SourceExecutionCompletionScope::record_current_box(completion) {
        Ok(()) => Ok(()),
        Err(completion) => root.completion_scope.record_box(completion),
    }
}
"#,
    );


    writeln!(
        out,
        "#[doc(hidden)]\n\
         struct __JetBootstrapNativeCallbackPayload {{\n\
             callback_root: {task_root},\n\
             adapter_root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,\n\
             span: {span},\n\
         }}\n\
         type __JetBootstrapNativeCallbackSession = {callback_session}<__JetBootstrapNativeCallbackPayload, Vec<{eval_value}>, {span}, ({callback_result}, {span})>;\n\
         type __JetBootstrapNativeCallbackLease = {callback_lease}<__JetBootstrapNativeCallbackPayload, Vec<{eval_value}>, {span}, ({callback_result}, {span})>;\n\
         fn __jet_bootstrap_native_callback_register(\n\
             adapter: &__JetBootstrapNativeAdapter,\n\
             machine: &{eval_machine},\n\
             closure: {eval_value},\n\
             span: {span},\n\
         ) -> Result<__JetBootstrapNativeCallbackLease, String> {{\n\
             let source_lease = {callback_transfer}(machine, closure, span)\n\
                 .ok_or_else(|| \"Source callback transfer failed\".to_string())?;\n\
             let cleanup_lease = source_lease.clone();\n\
             let callback_root = match {task_root_take}(machine, source_lease, span) {{\n\
                 Some(root) => root,\n\
                 None => {{\n\
                     let _ = {callback_release}(machine, cleanup_lease, span);\n\
                     return Err(\"Source callback root transfer failed\".to_string());\n\
                 }}\n\
             }};\n\
             let payload = __JetBootstrapNativeCallbackPayload {{\n\
                 callback_root,\n\
                 adapter_root: adapter.root.clone(),\n\
                 span,\n\
             }};\n\
             match adapter.callbacks.register(payload) {{\n\
                 Ok(callback) => Ok(callback),\n\
                 Err(error) => {{\n\
                     let (reason, payload) = error.into_parts();\n\
                     let cleanup_adapter = Box::new(__JetBootstrapNativeAdapter {{\n\
                         root: payload.adapter_root.clone(),\n\
                         callbacks: __JetBootstrapNativeCallbackSession::new(),\n\
                     }});\n\
                     let cleanup = {task_root_release}(&payload.callback_root, cleanup_adapter, payload.span);\n\
                     {merge_callback_output}(machine, cleanup.stdout.clone(), cleanup.stderr.clone());\n\
                     if !__jet_bootstrap_native_callback_drop_result(machine, cleanup, payload.span) {{\n\
                         return Err(format!(\"{{reason}}; Source callback root cleanup failed\"));\n\
                     }}\n\
                     Err(reason.to_string())\n\
                 }}\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_callback_enqueue(\n\
             callback: &__JetBootstrapNativeCallbackLease,\n\
             args: Vec<{eval_value}>,\n\
             span: {span},\n\
         ) -> Result<({callback_result}, {span}), ({callback_error}, Vec<{eval_value}>, {span})> {{\n\
             match callback.enqueue(args, span) {{\n\
                 Ok(reply) => reply.recv().map_err(|error| (error, Vec::new(), span)),\n\
                 Err(error) => {{\n\
                     let (reason, args, span) = error.into_parts();\n\
                     match callback.queue_rejected_cleanup(args, span) {{\n\
                         Ok(()) => Err((reason, Vec::new(), span)),\n\
                         Err(error) => {{\n\
                             let (cleanup_error, args, span) = error.into_parts();\n\
                             Err((cleanup_error, args, span))\n\
                         }}\n\
                     }}\n\
                 }}\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_callback_drop_result(\n\
             machine: &{eval_machine},\n\
             result: {callback_result},\n\
             span: {span},\n\
         ) -> bool {{\n\
             match result.outcome {{\n\
                 {callback_value}(value) => {drop_value}(machine, value, span),\n\
                 _ => false,\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_callback_enqueue(\n\
             callback: &__JetBootstrapNativeCallbackLease,\n\
             args: Vec<{eval_value}>,\n\
             span: {span},\n\
         ) -> Result<({callback_result}, {span}), ({callback_error}, Vec<{eval_value}>, {span})> {{\n\
             match callback.enqueue(args, span) {{\n\
                 Ok(reply) => reply.recv().map_err(|error| (error, Vec::new(), span)),\n\
                 Err(error) => {{\n\
                     let (reason, args, span) = error.into_parts();\n\
                     match callback.queue_rejected_cleanup(args, span) {{\n\
                         Ok(()) => Err((reason, Vec::new(), span)),\n\
                         Err(error) => {{\n\
                             let (cleanup_error, args, span) = error.into_parts();\n\
                             Err((cleanup_error, args, span))\n\
                         }}\n\
                     }}\n\
                 }}\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_callback_drop_result(\n\
             machine: &{eval_machine},\n\
             result: {callback_result},\n\
             span: {span},\n\
         ) -> bool {{\n\
             match result.outcome {{\n\
                 {callback_value}(value) => {drop_value}(machine, value, span),\n\
                 _ => true,\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_poll_callbacks(\n\
             root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &{eval_machine},\n\
         ) -> bool {{\n\
             __jet_bootstrap_native_pump_callbacks(root, callbacks, machine, false)\n\
         }}\n\
         fn __jet_bootstrap_native_drain_callbacks(\n\
             root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &{eval_machine},\n\
         ) -> bool {{\n\
             __jet_bootstrap_native_pump_callbacks(root, callbacks, machine, true)\n\
         }}\n\
         fn __jet_bootstrap_native_pump_callbacks(\n\
             root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &{eval_machine},\n\
             drain: bool,\n\
         ) -> bool {{\n\
             if drain {{ let _ = callbacks.retire(); }}\n\
             loop {{\n\
                 let event = if drain {{ callbacks.next() }} else {{ callbacks.try_next() }};\n\
                 let Some(event) = event else {{\n\
                     if !drain {{ return true; }}\n\
                     let status = callbacks.drain_status();\n\
                     let retired = callbacks.retire().is_ok();\n\
                     return retired && status.producers == 0 && status.inflight == 0\n\
                         && status.pending == 0 && status.borrows == 0 && status.releases == 0;\n\
                 }};\n\
                 let event_ok = match event {{\n\
                     {callback_event}::Invoke(mut invocation) => {{\n\
                         let callback_id = invocation.callback_id();\n\
                         let callback_lease = callbacks.with_payload(callback_id, |payload| payload.lease);\n\
                         let mut args = invocation.take_command();\n\
                         let span = invocation.take_context();\n\
                         match callback_lease {{\n\
                             Ok(lease) if lease > 0 => {{\n\
                                 let result = {callback_invoke}(machine, jet_foundation::Numeric::JetInt::from_i64(lease), args, span);\n\
                                 {merge_callback_output}(machine, result.stdout.clone(), result.stderr.clone());\n\
                                 match invocation.respond(Ok((result, span))) {{\n\
                                     Ok(()) => true,\n\
                                     Err({callback_reply_error}::Disconnected(Ok((result, reply_span)))) => __jet_bootstrap_native_callback_drop_result(machine, result, reply_span),\n\
                                     Err({callback_reply_error}::Disconnected(Err(_))) => true,\n\
                                     Err(_) => false,\n\
                                 }}\n\
                             }}\n\
                             _ => {{\n\
                                 let mut cleanup_ok = true;\n\
                                 for value in args.drain(..) {{ cleanup_ok &= {drop_value}(machine, value, span); }}\n\
                                 let replied = invocation.respond(Err({callback_error}::UnknownCallback)).is_ok();\n\
                                 cleanup_ok && replied\n\
                             }}\n\
                         }}\n\
                     }}\n\
                     {callback_event}::Cleanup(mut cleanup) => {{\n\
                         let span = cleanup.take_context().or_else(|| callbacks.with_payload(cleanup.callback_id(), |payload| payload.span).ok());\n\
                         let Some(span) = span else {{ return false; }};\n\
                         let mut ok = true;\n\
                         if let Some(mut args) = cleanup.take_command() {{\n\
                             for value in args.drain(..) {{ ok &= {drop_value}(machine, value, span); }}\n\
                         }}\n\
                         cleanup.complete();\n\
                         ok\n\
                     }}\n\
                     {callback_event}::ReplyCleanup(mut cleanup) => {{\n\
                         let result = match cleanup.take_result() {{\n\
                             Ok((result, span)) => __jet_bootstrap_native_callback_drop_result(machine, result, span),\n\
                             Err(_) => true,\n\
                         }};\n\
                         result && cleanup.complete().is_ok()\n\
                     }}\n\
                     {callback_event}::Release(release) => {{\n\
                         let payload = release.payload();\n\
                         let released = {callback_release}(machine, jet_foundation::Numeric::JetInt::from_i64(payload.lease), payload.span);\n\
                         let completed = release.complete().is_ok();\n\
                         released && completed\n\
                     }}\n\
                 }};\n\
                 if !event_ok {{ return false; }}\n\
                 if drain {{ let _ = callbacks.retire(); }}\n\
             }}\n\
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    Ok(())
}

fn emit_native_binding_helpers(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let eval_machine = symbols.type_symbol("JetEvalMachine")?;
    let eval_result = symbols.type_symbol("JetEvalResult")?;
    let host_type_shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let result_transfer_receipt = symbols.type_symbol("JetEvalResultHostTransferReceipt")?;
    let result_host_type_shape = callable_symbol(symbols, "jet_eval_result_host_type_shape")?;
    let result_transfer_prepare =
        callable_symbol(symbols, "jet_eval_result_host_transfer_prepare")?;
    let result_transfer_commit =
        callable_symbol(symbols, "jet_eval_result_host_transfer_commit")?;
    let result_transfer_dispose =
        callable_symbol(symbols, "jet_eval_result_host_transfer_dispose")?;
    let result_transfer_disposition =
        symbols.type_symbol("JetEvalResultHostTransferCommitDisposition")?;
    let transfer_committed =
        symbols.variant_path("JetEvalResultHostTransferCommitDisposition", "Committed")?;
    let transfer_rejected =
        symbols.variant_path("JetEvalResultHostTransferCommitDisposition", "Rejected")?;
    let transfer_consumed_failure =
        symbols.variant_path("JetEvalResultHostTransferCommitDisposition", "ConsumedFailure")?;
    let source_binding_identity = symbols.type_symbol("JetEvalNativeBindingIdentity")?;
    let host_adapter_field_symbol = symbols.field_symbol("JetEvalConfig", "host_adapter")?;
    let host_value = symbols.type_symbol("JetEvalHostValue")?;
    let owner_native = symbols.variant_path("JetEvalHostOwner", "Native")?;
    let owner_cursor = symbols.variant_path("JetEvalHostOwner", "Cursor")?;
    let owner_core = symbols.variant_path("JetEvalHostOwner", "Core")?;
    let owner_declared = symbols.variant_path("JetEvalHostOwner", "Declared")?;
    let owner_callable = symbols.variant_path("JetEvalNativeBindingIdentity", "Callable")?;
    let owner_interface = symbols.variant_path("JetEvalNativeBindingIdentity", "Interface")?;
    let host_handle = symbols.variant_path("JetEvalHostValue", "Handle")?;
    let host_data = symbols.variant_path("JetEvalHostValue", "Data")?;
    let host_closure = symbols.variant_path("JetEvalHostValue", "Closure")?;
    let host_shared_carrier = symbols.variant_path("JetEvalHostValue", "SharedCarrier")?;
    let host_kind_shared = symbols.variant_path("JetEvalSharedHostKind", "Shared")?;
    let physical_shared = symbols.variant_path("JetEvalSharedHostPhysicalRoot", "Shared")?;
    let physical_weak = symbols.variant_path("JetEvalSharedHostPhysicalRoot", "Weak")?;
    let carrier_physical = symbols.field_symbol("JetEvalSharedHostCarrier", "physical")?;
    let carrier_kind = symbols.field_symbol("JetEvalSharedHostCarrier", "kind")?;
    let value_metadata = symbols.field_symbol("JetEvalSharedValue", "metadata")?;
    let value_physical_root = symbols.field_symbol("JetEvalSharedValueMeta", "physical_root")?;
    let weak_metadata = symbols.field_symbol("JetEvalSharedWeak", "metadata")?;
    let weak_physical_root = symbols.field_symbol("JetEvalSharedWeakMeta", "physical_root")?;
    let cap_handle = symbols.field_symbol("JetEvalSharedHostRootCapability", "handle")?;
    let cap_raw = symbols.field_symbol("JetEvalSharedHostRootCapability", "raw")?;
    let cap_generation = symbols.field_symbol("JetEvalSharedHostRootCapability", "generation")?;
    let cap_type_identity = symbols.field_symbol("JetEvalSharedHostRootCapability", "type_identity")?;
    let cap_physical_identity = symbols.field_symbol("JetEvalSharedHostRootCapability", "physical_identity")?;
    let weak_cap_handle = symbols.field_symbol("JetEvalSharedHostWeakCapability", "handle")?;
    let weak_cap_raw = symbols.field_symbol("JetEvalSharedHostWeakCapability", "raw")?;
    let weak_cap_generation = symbols.field_symbol("JetEvalSharedHostWeakCapability", "generation")?;
    let weak_cap_type_identity = symbols.field_symbol("JetEvalSharedHostWeakCapability", "type_identity")?;
    let weak_cap_physical_identity = symbols.field_symbol("JetEvalSharedHostWeakCapability", "physical_identity")?;
    let ct_unit = symbols.variant_path("TComptimeValue", "Unit")?;
    let host_field = symbols.field_binding("JetEvalConfig", "host_adapter")?;
    let numeric_field = symbols.field_binding("SemaRegistrationHostHooks", "numeric_unit_conversion_exact")?;
    let numeric_callable_type = numeric_field.ty.option_inner().ok_or_else(|| {
        BootstrapHostCodecError::InvalidMetadata(
            "SemaRegistrationHostHooks.numeric_unit_conversion_exact is not the checked optional callable field".to_string(),
        )
    })?;
    let jet_foundation::MIR::MirTypeKind::Fn(numeric_signature) = numeric_callable_type.kind() else {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "SemaRegistrationHostHooks.numeric_unit_conversion_exact is not a checked Fn".to_string(),
        ));
    };
    let numeric_parameters_match = numeric_signature.params.len() == 5
        && matches!(numeric_signature.params[0].kind(), jet_foundation::MIR::MirTypeKind::Float)
        && numeric_signature.params[1..]
            .iter()
            .all(|ty| matches!(ty.kind(), jet_foundation::MIR::MirTypeKind::String))
        && numeric_signature.call_metadata.as_ref().is_none_or(|metadata| {
            metadata.conventions.len() == numeric_signature.params.len()
                && metadata
                    .conventions
                    .iter()
                    .all(|access| *access == jet_foundation::MIR::MirAccess::Read)
        })
        && numeric_signature.ret.as_deref().is_some_and(|ret| {
            ret.option_inner().is_some_and(|inner| {
                matches!(inner.kind(), jet_foundation::MIR::MirTypeKind::Float)
            })
        });
    if !numeric_parameters_match {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "numeric_unit_conversion_exact differs from the checked Float/String×4 -> ?Float ABI".to_string(),
        ));
    }
    let source_resources = "::jet_jit::SourceResources";
    let resource_identity = format!("{source_resources}::SourceNativeBindingIdentity");
    let source_native_binding = format!("{source_resources}::SourceNativeBinding");

    let generated = r#"
        type __JetBootstrapNativeNumericFn =
            dyn FnMut(&f64, &String, &String, &String, &String)
                -> Result<f64, ::jet_foundation::Outcome::JetAbsent>;
        type __JetBootstrapNativeNumericInner =
            ::std::cell::RefCell<Option<Box<__JetBootstrapNativeNumericFn>>>;
        type __JetBootstrapNativeNumericWrapper =
            ::std::rc::Rc<__JetBootstrapNativeNumericInner>;


        fn __jet_bootstrap_native_numeric_callable_template(
            bindings: &::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,
            program: &::jet_foundation::MIR::MirProgram,
            artifact: ::jet_foundation::MIR::MirArtifactId,
        ) -> Result<(
            ::jet_jit::SourceInterfaces::NativeInterfaceObject,
            ::jet_jit::SourceInterfaces::NativeCallableIdentity,
        ), String> {
            let checked = crate::__jet_bootstrap_entry_field_type(
                program,
                ::jet_foundation::MIR::MirTypeId(__JET_NUMERIC_OWNER__),
                ::jet_foundation::MIR::MirFieldId(__JET_NUMERIC_FIELD__),
            )?;
            let callable_type = checked.option_inner()
                .ok_or_else(|| "checked numeric callback field lost its Option leaf".to_string())?
                .clone();
            let identity = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked(
                program,
                artifact,
                crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY,
                callable_type.clone(),
            ).map_err(|error| error.to_string())?;
            let signature = ::jet_jit::SourceInterfaces::NativeCallableSignature::checked(&callable_type)
                .map_err(|error| error.to_string())?;
            let object = bindings.create_object(()).map_err(|error| error.to_string())?;
            let handler: ::jet_jit::SourceInterfaces::NativeCallableHandler =
                ::std::sync::Arc::new(|_, call| crate::__jet_bootstrap_native_numeric_unit_conversion(call));
            bindings.bind_callable(object.clone(), identity.clone(), signature, handler)
                .map_err(|error| error.to_string())?;
            Ok((object, identity))
        }


        fn __jet_bootstrap_native_numeric_wrapper(
            bindings: &::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,
            program: &::std::sync::Arc<::jet_foundation::MIR::MirProgram>,
            artifact: ::jet_foundation::MIR::MirArtifactId,
            identity: ::jet_jit::SourceInterfaces::NativeCallableIdentity,
            carrier: ::jet_foundation::MIR::MirNativeOwned,
        ) -> Result<__JetBootstrapNativeNumericWrapper, String> {
            let checked = crate::__jet_bootstrap_entry_field_type(
                program.as_ref(),
                ::jet_foundation::MIR::MirTypeId(__JET_NUMERIC_OWNER__),
                ::jet_foundation::MIR::MirFieldId(__JET_NUMERIC_FIELD__),
            )?;
            let callable_type = checked.option_inner()
                .ok_or_else(|| "checked numeric callback field lost its Option leaf".to_string())?;
            let expected = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked(
                program.as_ref(),
                artifact,
                crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY,
                callable_type.clone(),
            ).map_err(|error| error.to_string())?;
            if identity != expected {
                return Err("numeric callback identity is not the exact checked config callable".to_string());
            }
            let signature = ::jet_jit::SourceInterfaces::NativeCallableSignature::checked(callable_type)
                .map_err(|error| error.to_string())?;
            let active_bindings = bindings.clone();
            let active_program = program.clone();
            let callback_identity = identity.clone();
            let wrapper = bindings.create_native_callable_rc_wrapper(
                identity,
                carrier,
                move |registration| {
                    ::std::rc::Rc::new(::std::cell::RefCell::new(Some(
                        Box::new(move |value, source_unit, destination_unit, scale_num, scale_den| {
                            let _registration = &registration;
                            let _scope = active_bindings.activate(active_program.as_ref(), artifact)
                                .unwrap_or_else(|error| panic!("numeric native callable activation failed: {error}"));
                            let values = vec![
                                ::jet_foundation::MIR::MirRuntimeValue::Float { value: *value, f32: false },
                                ::jet_foundation::MIR::MirRuntimeValue::String(source_unit.clone()),
                                ::jet_foundation::MIR::MirRuntimeValue::String(destination_unit.clone()),
                                ::jet_foundation::MIR::MirRuntimeValue::String(scale_num.clone()),
                                ::jet_foundation::MIR::MirRuntimeValue::String(scale_den.clone()),
                            ];
                            let arguments = signature.parameters.iter().zip(values).enumerate()
                                .map(|(index, (parameter, value))| ::jet_jit::SourceInterfaces::NativeInterfaceArgument {
                                    index,
                                    ty: parameter.ty.clone(),
                                    access: parameter.access,
                                    value,
                                    writeback: false,
                                }).collect();
                            let completion = active_bindings.dispatch_callable_for_scope(
                                callback_identity.clone(),
                                signature.clone(),
                                arguments,
                                ::jet_foundation::Diagnostics::Span::new(0, 0),
                            );
                            let (outcome, call) = completion.into_parts();
                            if !call.transfers().is_empty() || call.pending_transfers().next().is_some() {
                                panic!("read-only numeric callable unexpectedly transferred an argument");
                            }
                            match outcome {
                                Ok(::jet_foundation::MIR::MirRuntimeValue::Present(value)) => {
                                    match *value {
                                        ::jet_foundation::MIR::MirRuntimeValue::Float { value, f32: false } => Ok(value),
                                        _ => panic!("numeric native callable returned a non-Float Present value"),
                                    }
                                }
                                Ok(::jet_foundation::MIR::MirRuntimeValue::Absent { element })
                                    if signature.return_type.as_ref()
                                        .and_then(::jet_foundation::MIR::MirType::option_inner)
                                        .is_some_and(|expected| element.same_checked_type(expected)) =>
                                {
                                    Err(::jet_foundation::Outcome::JetAbsent)
                                }
                                Ok(_) => panic!("numeric native callable returned an invalid checked Option value"),
                                Err(error) => panic!("numeric native callable dispatch failed: {error}"),
                            }
                        }) as Box<__JetBootstrapNativeNumericFn>,
                    )))
                },
            ).map_err(|error| error.to_string())?;
            Ok(wrapper)
        }

        fn __jet_bootstrap_native_numeric_association(
            wrapper: &__JetBootstrapNativeNumericWrapper,
        ) -> Result<(
            ::jet_jit::SourceInterfaces::NativeCallableIdentity,
            ::jet_foundation::MIR::MirNativeOwned,
        ), String> {
            ::jet_jit::SourceInterfaces::native_callable_rc_association(wrapper)
                .map_err(|error| error.to_string())
        }

        fn __jet_bootstrap_native_numeric_registration(
            adapter: &__JetBootstrapNativeAdapter,
        ) -> Result<(
            ::jet_jit::SourceInterfaces::NativeCallableIdentity,
            ::jet_foundation::MIR::MirNativeOwned,
        ), String> {
            let rows = adapter.root.native_bindings.lock().unwrap_or_else(|error| error.into_inner());
            let row = rows.iter()
                .find(|row| !row.dynamic && __jet_bootstrap_native_binding_field_is(&row.field_path, "numeric_unit_conversion_exact"))
                .ok_or_else(|| "checked numeric callable has no registered physical root".to_string())?;
            let identity = match &row.identity {
                __JET_SOURCE_RESOURCE_IDENTITY__::Callable(identity) => identity,
                _ => return Err("numeric callable registration has an interface identity".to_string()),
            };
            let __JET_OWNER_CALLABLE__(owner) = &row.binding else {
                return Err("numeric callable registration has a non-callable Source identity".to_string());
            };
            if identity.key != crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY
                || owner.key != identity.key
                || !owner.callable_type.same_checked_type(&identity.callable_type)
            {
                return Err("numeric callable registration disagrees with its exact checked identity".to_string());
            }
            let current = adapter.resources.arena().lookup_capability(row.capability.handle, row.capability.raw)?;
            if current != row.capability {
                return Err("numeric callable capability generation changed".to_string());
            }
            let binding = adapter.resources.arena().borrow_native_binding(
                row.capability.handle,
                row.capability.raw,
                &row.identity,
            ).map_err(|error| error.to_string())?;
            Ok((identity.clone(), binding.carrier().clone()))
        }

        fn __jet_bootstrap_native_numeric_wrapper_for_adapter(
            adapter: &__JetBootstrapNativeAdapter,
        ) -> Result<__JetBootstrapNativeNumericWrapper, String> {
            let (identity, carrier) = __jet_bootstrap_native_numeric_registration(adapter)?;
            __jet_bootstrap_native_numeric_wrapper(
                &adapter.root.bindings,
                &adapter.root.program,
                adapter.root.artifact,
                identity,
                carrier,
            )
        }


        fn __jet_bootstrap_native_binding_field_is(field_path: &[String], field: &str) -> bool {
            field_path.last().is_some_and(|name| name == field)
        }

        fn __jet_bootstrap_native_binding_registration(
            resources: &::jet_jit::SourceResources::SourceResourceLease,
            cleanup: Option<&::jet_jit::SourceResources::SourceResourceCleanupLease>,
            program: &::jet_foundation::MIR::MirProgram,
            artifact: ::jet_foundation::MIR::MirArtifactId,
            field_path: Vec<String>,
            binding: __JET_SOURCE_BINDING_IDENTITY__,
            identity: __JET_SOURCE_RESOURCE_IDENTITY__,
            object: ::jet_jit::SourceInterfaces::NativeInterfaceObject,
            dynamic: bool,
        ) -> Result<__JetBootstrapNativeBindingRegistration, String> {
            let host_field = __jet_bootstrap_native_binding_field_is(&field_path, "host_adapter");
            let numeric_field = __jet_bootstrap_native_binding_field_is(&field_path, "numeric_unit_conversion_exact");
            if (host_field && matches!(&binding, __JET_OWNER_INTERFACE__(_)))
                || (numeric_field && !dynamic && matches!(&binding, __JET_OWNER_CALLABLE__(_)))
            {
                // These are the only native Source fields owned by this adapter.
            } else {
                return Err("native binding is not attached to an approved checked JetEvalConfig/SemaRegistrationHostHooks leaf".to_string());
            }
            let checked_host = crate::__jet_bootstrap_entry_field_type(
                program,
                ::jet_foundation::MIR::MirTypeId(__JET_HOST_OWNER__),
                ::jet_foundation::MIR::MirFieldId(__JET_HOST_FIELD__),
            )?;
            let expected_host = checked_host.option_inner()
                .ok_or_else(|| "checked JetEvalConfig.host_adapter lost its Option leaf".to_string())?;
            let checked_numeric = crate::__jet_bootstrap_entry_field_type(
                program,
                ::jet_foundation::MIR::MirTypeId(__JET_NUMERIC_OWNER__),
                ::jet_foundation::MIR::MirFieldId(__JET_NUMERIC_FIELD__),
            )?;
            let expected_numeric = checked_numeric.option_inner()
                .ok_or_else(|| "checked SemaRegistrationHostHooks.numeric_unit_conversion_exact lost its Option leaf".to_string())?;
            let expected_execution = program.execution_identity(Some(artifact))
                .map_err(|error| error.to_string())?;
            match (&identity, &binding) {
                (__JET_SOURCE_RESOURCE_IDENTITY__::Interface { execution, artifact: owner_artifact, receiver_type },
                 __JET_OWNER_INTERFACE__(owner_type))
                    if host_field
                        && *execution == expected_execution
                        && *owner_artifact == artifact
                        && receiver_type.same_checked_type(expected_host)
                        && owner_type.same_checked_type(expected_host) => {}
                (__JET_SOURCE_RESOURCE_IDENTITY__::Callable(source), __JET_OWNER_CALLABLE__(owner))
                    if numeric_field
                        && source.execution == expected_execution
                        && source.artifact == artifact
                        && source.key == owner.key
                        && source.key == __JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY
                        && source.callable_type.same_checked_type(expected_numeric)
                        && owner.callable_type.same_checked_type(expected_numeric) => {}
                _ => return Err("native binding identity disagrees with its exact checked Source field".to_string()),
            }
            if identity.value_type().same_checked_type(match &binding {
                __JET_OWNER_CALLABLE__(owner) => &owner.callable_type,
                __JET_OWNER_INTERFACE__(receiver_type) => receiver_type,
            }) == false {
                return Err("native binding owner type disagrees with its physical capability identity".to_string());
            }
            let physical = match &identity {
                __JET_SOURCE_RESOURCE_IDENTITY__::Callable(callable) =>
                    __JET_SOURCE_NATIVE_BINDING__::callable(&object, callable.clone())?,
                __JET_SOURCE_RESOURCE_IDENTITY__::Interface { execution, artifact, receiver_type } =>
                    __JET_SOURCE_NATIVE_BINDING__::interface(&object, *execution, *artifact, receiver_type.clone())?,
            };
            let capability = match cleanup {
                Some(cleanup) => cleanup.register_native_binding_root(physical)?,
                None => resources.arena().register_native_binding_root(physical)?,
            };
            if capability.handle != ::jet_jit::SourceResources::native_binding_handle_id()
                || capability.kind != ::jet_jit::SourceResources::SourceResourceKind::NativeBinding
            {
                let _ = resources.arena().release_capability(&capability);
                return Err("Source arena returned a mismatched NativeBinding capability".to_string());
            }
            Ok(__JetBootstrapNativeBindingRegistration {
                field_path,
                binding,
                identity,
                capability,
                dynamic,
            })
        }

        fn __jet_bootstrap_native_adapter_new_scope(
            root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,
            resources: ::jet_jit::SourceResources::SourceResourceLease,
            cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,
        ) -> Result<__JetBootstrapNativeAdapter, String> {
            let owner = __jet_bootstrap_native_adapter_owner_new(root, resources, cleanup)?;
            __jet_bootstrap_native_adapter_bind_owner(owner)
        }

        fn __jet_bootstrap_native_adapter_borrow_scope(
            adapter: &__JetBootstrapNativeAdapter,
            resources: ::jet_jit::SourceResources::SourceResourceLease,
        ) -> Result<__JetBootstrapNativeAdapter, String> {
            let owner = __jet_bootstrap_native_adapter_owner_from_context(
                adapter.owner.context.clone(),
                adapter.callbacks.clone(),
                resources,
                true,
            );
            __jet_bootstrap_native_adapter_bind_owner(owner)
        }

        fn __jet_bootstrap_native_adapter_bind_owner(
            owner: ::std::sync::Arc<__JetBootstrapNativeAdapterOwner>,
        ) -> Result<__JetBootstrapNativeAdapter, String> {
            let root = owner.root.clone();
            let resources = owner.resources.clone();
            let cleanup = owner.cleanup.clone();
            let _activation = resources.activate();
            let context = __JetBootstrapNativeAdapterInstance {
                root: root.clone(),
                owner: ::std::sync::Arc::downgrade(&owner),
            };
            let instance = root.bindings.instantiate_interface_for_carrier(
                &root.template.clone_root(),
                &root.receiver_type,
                context,
            ).map_err(|error| error.to_string())?;
            let registration = __jet_bootstrap_native_binding_registration(
                &resources,
                cleanup.as_ref(),
                root.program.as_ref(),
                root.artifact,
                vec!["JetEvalConfig".to_string(), "host_adapter".to_string()],
                __JET_OWNER_INTERFACE__(root.receiver_type.clone()),
                __JET_SOURCE_RESOURCE_IDENTITY__::Interface {
                    execution: root.execution.clone(),
                    artifact: root.artifact,
                    receiver_type: root.receiver_type.clone(),
                },
                instance.clone(),
                true,
            )?;
            let capability = registration.capability.clone();
            owner.instance.set(instance.clone())
                .map_err(|_| "callback-scope owner already has a physical interface instance".to_string())?;
            owner.capability.set(capability.clone())
                .map_err(|_| "callback-scope owner already has a NativeBinding capability".to_string())?;
            Ok(__JetBootstrapNativeAdapter {
                root,
                resources,
                cleanup,
                callbacks: owner.callbacks.clone(),
                instance,
                capability,
                borrowed_session: owner.borrowed_session,
                owner,
            })
        }

        fn __jet_bootstrap_native_adapter_physical_binding(
            adapter: &__JetBootstrapNativeAdapter,
        ) -> __JET_HOST_VALUE__ {
            let arena = adapter.resources.arena();
            let current = arena.lookup_capability(adapter.capability.handle, adapter.capability.raw)
                .unwrap_or_else(|error| panic!("native adapter capability lookup failed: {error}"));
            if current != adapter.capability {
                panic!("native adapter capability generation changed");
            }
            let binding = arena.borrow_native_binding(
                adapter.capability.handle,
                adapter.capability.raw,
                &__JET_SOURCE_RESOURCE_IDENTITY__::Interface {
                    execution: adapter.root.execution.clone(),
                    artifact: adapter.root.artifact,
                    receiver_type: adapter.root.receiver_type.clone(),
                },
            ).unwrap_or_else(|error| panic!("native adapter capability identity changed: {error}"));
            if binding.carrier() != &adapter.instance.clone_root()
                || !adapter.instance.matches_root(binding.carrier())
            {
                panic!("native adapter capability no longer retains this callback-scope instance");
            }
            __JET_HOST_HANDLE__(
                adapter.capability.handle,
                jet_foundation::Numeric::JetInt::from_i64(adapter.capability.raw),
                __JET_OWNER_NATIVE__(__JET_OWNER_INTERFACE__(adapter.root.receiver_type.clone())),
                __JET_HOST_DATA__(__JET_CT_UNIT__),
            )
        }
        fn __jet_bootstrap_native_adapter_host_runtime(
            physical: &__JetBootstrapNativePhysicalBindings,
            checked_type: &::jet_foundation::MIR::MirType,
            value: &__JET_HOST_VALUE__,
        ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
            let (handle, raw, receiver_type) = match value {
                __JET_HOST_HANDLE__(
                    handle,
                    raw,
                    __JET_OWNER_NATIVE__(__JET_OWNER_INTERFACE__(receiver_type)),
                    __JET_HOST_DATA__(__JET_CT_UNIT__),
                ) => (handle, raw, receiver_type),
                _ => return Err("host_adapter did not return its checked native interface capability".to_string()),
            };
            if !receiver_type.same_checked_type(checked_type)
                || !receiver_type.same_checked_type(&physical.adapter.root.receiver_type)
                || *handle != physical.adapter.capability.handle
                || raw.to_i64() != Some(physical.adapter.capability.raw)
            {
                return Err("host_adapter capability is not the exact current callback-scope instance".to_string());
            }
            let current = physical.adapter.resources.arena()
                .lookup_capability(*handle, physical.adapter.capability.raw)?;
            if current != physical.adapter.capability {
                return Err("host_adapter capability generation changed before encoding".to_string());
            }
            let identity = __JET_SOURCE_RESOURCE_IDENTITY__::Interface {
                execution: physical.adapter.root.execution.clone(),
                artifact: physical.adapter.root.artifact,
                receiver_type: receiver_type.clone(),
            };
            let binding = physical.adapter.resources.arena().borrow_native_binding(
                *handle,
                physical.adapter.capability.raw,
                &identity,
            ).map_err(|error| error.to_string())?;
            if binding.carrier() != &physical.adapter.instance.clone_root()
                || !physical.adapter.instance.matches_root(binding.carrier())
            {
                return Err("host_adapter capability does not retain the exact callback-scope root".to_string());
            }
            Ok(::jet_foundation::MIR::MirRuntimeValue::NativeOwned(
                physical.adapter.instance.clone_root(),
            ))
        }


        fn __jet_bootstrap_native_binding_project(
            physical: &__JetBootstrapNativePhysicalBindings,
            field_path: &[String],
            checked_type: &::jet_foundation::MIR::MirType,
            _program: &::jet_foundation::MIR::MirProgram,
            value: ::jet_foundation::MIR::MirRuntimeValue,
        ) -> Result<Option<__JET_HOST_VALUE__>, String> {
            let Some(field) = field_path.last().map(String::as_str) else { return Ok(None); };
            if field != "host_adapter" && field != "numeric_unit_conversion_exact" {
                return Ok(None);
            }
            let ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(carrier) = value else {
                return Err(format!("native Source field `{}` has no retained NativeOwned carrier", field_path.join(".")));
            };
            let rows = physical.adapter.root.native_bindings.lock().unwrap_or_else(|error| error.into_inner());
            let mut projected = None;
            for row in rows.iter().filter(|row| row.field_path.last().is_some_and(|name| name == field)) {
                let value_type = row.identity.value_type();
                if !value_type.same_checked_type(checked_type) {
                    continue;
                }
                if row.capability.handle != ::jet_jit::SourceResources::native_binding_handle_id()
                    || row.capability.kind != ::jet_jit::SourceResources::SourceResourceKind::NativeBinding
                {
                    return Err("native Source field carries a non-NativeBinding capability".to_string());
                }
                let arena = physical.adapter.resources.arena();
                let current = arena.lookup_capability(row.capability.handle, row.capability.raw)?;
                if current != row.capability {
                    return Err("native Source field capability generation changed before projection".to_string());
                }
                let binding = arena.borrow_native_binding(
                    row.capability.handle,
                    row.capability.raw,
                    &row.identity,
                ).map_err(|error| error.to_string())?;
                if binding.carrier() != &carrier {
                    continue;
                }
                if row.dynamic && (field != "host_adapter"
                    || !physical.adapter.instance.matches_root(binding.carrier()))
                {
                    continue;
                }
                let value = match &row.binding {
                    __JET_OWNER_CALLABLE__(_) if field == "numeric_unit_conversion_exact" =>
                        __JET_HOST_HANDLE__(
                            row.capability.handle,
                            jet_foundation::Numeric::JetInt::from_i64(row.capability.raw),
                            __JET_OWNER_NATIVE__(row.binding.clone()),
                            __JET_HOST_DATA__(__JET_CT_UNIT__),
                        ),
                    __JET_OWNER_INTERFACE__(receiver_type) if field == "host_adapter" =>
                        __JET_HOST_HANDLE__(
                            row.capability.handle,
                            jet_foundation::Numeric::JetInt::from_i64(row.capability.raw),
                            __JET_OWNER_NATIVE__(__JET_OWNER_INTERFACE__(receiver_type.clone())),
                            __JET_HOST_DATA__(__JET_CT_UNIT__),
                        ),
                    _ => return Err("native Source field binding variant disagrees with its checked leaf".to_string()),
                };
                if projected.replace(value).is_some() {
                    return Err(format!("native Source field `{}` has ambiguous physical binding registrations", field_path.join(".")));
                }
            }
            projected.map(Some).ok_or_else(|| format!("native Source field `{}` does not retain the supplied exact carrier", field_path.join(".")))
        }
        fn __jet_bootstrap_native_host_value_to_runtime(
            adapter: &__JetBootstrapNativeAdapter,
            result: &mut __JET_EVAL_RESULT__,
            mut host_value: __JET_HOST_VALUE__,
            checked_type: &::jet_foundation::MIR::MirType,
            span: ::jet_foundation::Diagnostics::Span,
        ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
            let source_shape: __JET_HOST_TYPE_SHAPE__ = __JET_RESULT_HOST_TYPE_SHAPE__(
                result,
                checked_type.clone(),
                span,
            ).map_err(|_| "Source result has no checked guest HostTypeShape".to_string())?;
            let shape = __jet_bootstrap_entry_host_type_shape_from_source(&source_shape)?;
            let mut receipt: __JET_RESULT_TRANSFER_RECEIPT__ = __JET_RESULT_TRANSFER_PREPARE__(
                result,
                &host_value,
                checked_type.clone(),
                span,
            ).map_err(|_| "Source result HostValue transfer could not be prepared".to_string())?;
            let physical = __JetBootstrapNativePhysicalBindings::new(adapter);
            let prepared = match __jet_bootstrap_entry_prepare_interpreter_transfer(
                &host_value,
                checked_type,
                &shape,
                &physical,
            ) {
                Ok(prepared) => prepared,
                Err(error) => {
                    if !__JET_RESULT_TRANSFER_DISPOSE__(result, &mut receipt, span) {
                        return Err(format!("{error}; Source result transfer preparation cleanup failed"));
                    }
                    return Err(error);
                }
            };
            match __JET_RESULT_TRANSFER_COMMIT__(
                result,
                &mut host_value,
                &mut receipt,
                span,
            ) {
                __JET_TRANSFER_COMMITTED__ => {}
                __JET_TRANSFER_REJECTED__ => {
                    drop(prepared);
                    if !__JET_RESULT_TRANSFER_DISPOSE__(result, &mut receipt, span) {
                        return Err("Source rejected result transfer and receipt disposal failed".to_string());
                    }
                    return Err("Source rejected result HostValue transfer".to_string());
                }
                __JET_TRANSFER_CONSUMED_FAILURE__ => {
                    drop(prepared);
                    if !__JET_RESULT_TRANSFER_DISPOSE__(result, &mut receipt, span) {
                        return Err("Source consumed failed result transfer and receipt disposal failed".to_string());
                    }
                    return Err("Source consumed result HostValue transfer with failure".to_string());
                }
            }
            Ok(__jet_bootstrap_entry_interpreter_transfer_commit_and_extract(
                prepared,
                &receipt,
                &physical,
            ))
        }


        fn __jet_bootstrap_native_adapter_callback_config(
            adapter: &__JetBootstrapNativeAdapter,
            packet: &__JetBootstrapNativePacket,
            live_resources: &::jet_jit::SourceResources::SourceResourceLease,
        ) -> Result<__JET_EVAL_CONFIG__, String> {
            let callback_helper = adapter.root.program.functions.iter()
                .find(|helper| helper.id == adapter.root.task_callback_invoke)
                .ok_or_else(|| "checked task callback helper disappeared".to_string())?;
            let callback_root_type = &callback_helper.params.first()
                .ok_or_else(|| "checked task callback helper has no TaskRoot parameter".to_string())?.ty;
            let drop_helper = adapter.root.program.functions.iter()
                .find(|helper| helper.id == adapter.root.owned_root_drop)
                .ok_or_else(|| "checked owned-root drop helper disappeared".to_string())?;
            let owned_root_type = &drop_helper.params.first()
                .ok_or_else(|| "checked owned-root drop helper has no OwnedRoot parameter".to_string())?.ty;
            let (root_type, root_value) = packet;
            let physical = __JetBootstrapNativePhysicalBindings::new(adapter);
            let mut config = if root_type.same_checked_type(callback_root_type) {
                let config = crate::__jet_bootstrap_entry_jet_eval_task_root_config_from_runtime(
                    root_type,
                    root_value,
                    &adapter.root.program,
                    &physical,
                )?;
                config
            } else if root_type.same_checked_type(owned_root_type) {
                let config = crate::__jet_bootstrap_entry_jet_eval_owned_root_config_from_runtime(
                    root_type,
                    root_value,
                    &adapter.root.program,
                    &physical,
                )?;
                config
            } else {
                return Err("callback helper root is neither the checked TaskRoot nor OwnedRoot carrier".to_string());
            };
            let callback_adapter = __jet_bootstrap_native_adapter_borrow_scope(
                adapter,
                live_resources.clone(),
            )?;
            config.__JET_HOST_ADAPTER_FIELD__ = Ok(Box::new(callback_adapter));
            Ok(config)
        }


        #[derive(Clone)]
        struct __JetBootstrapNativePhysicalBindings {
            adapter: __JetBootstrapNativeAdapter,
        }

        impl __JetBootstrapNativePhysicalBindings {
            fn new(adapter: &__JetBootstrapNativeAdapter) -> Self {
                Self { adapter: adapter.clone() }
            }
        }
        fn __jet_bootstrap_native_shared_shapes_equal(
            left: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
            right: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
        ) -> bool {
            use crate::compiler_bootstrap_entry_codec::{
                BootstrapEntryHostTypeField as Field,
                BootstrapEntryHostTypeNode as Node,
                BootstrapEntryHostTypeVariant as Variant,
            };
            fn fields_equal(left: &[Field], right: &[Field]) -> bool {
                left.len() == right.len()
                    && left.iter().zip(right).all(|(left, right)| {
                        left.name == right.name && left.node == right.node
                    })
            }
            fn variants_equal(left: &[Variant], right: &[Variant]) -> bool {
                left.len() == right.len()
                    && left.iter().zip(right).all(|(left, right)| {
                        left.name == right.name && left.args == right.args
                    })
            }
            fn owners_equal(
                left: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostOwner,
                right: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostOwner,
            ) -> bool {
                use crate::compiler_bootstrap_entry_codec::BootstrapEntryHostOwner as Owner;
                match (left, right) {
                    (Owner::Cursor, Owner::Cursor) => true,
                    (Owner::Declared(left), Owner::Declared(right)) => {
                        left.same_checked_type(right)
                    }
                    (
                        Owner::Core { fact: left_fact, ty: left_ty },
                        Owner::Core { fact: right_fact, ty: right_ty },
                    ) => left_fact == right_fact && left_ty.same_checked_type(right_ty),
                    (
                        Owner::NativeCallable { key: left_key, callable_type: left_ty },
                        Owner::NativeCallable { key: right_key, callable_type: right_ty },
                    ) => left_key == right_key && left_ty.same_checked_type(right_ty),
                    (
                        Owner::NativeInterface { receiver_type: left },
                        Owner::NativeInterface { receiver_type: right },
                    ) => left.same_checked_type(right),
                    _ => false,
                }
            }
            if left.root != right.root || left.nodes.len() != right.nodes.len() {
                return false;
            }
            left.nodes.iter().zip(&right.nodes).all(|(left, right)| {
                match (left, right) {
                    (Node::Scalar(left), Node::Scalar(right)) => {
                        left.same_checked_type(right)
                    }
                    (
                        Node::Handle { ty: left_ty, owner: left_owner },
                        Node::Handle { ty: right_ty, owner: right_owner },
                    ) => {
                        left_ty.same_checked_type(right_ty) && owners_equal(left_owner, right_owner)
                    }
                    (Node::Option(left), Node::Option(right))
                    | (Node::List(left), Node::List(right)) => left == right,
                    (
                        Node::Result { ok: left_ok, error: left_error },
                        Node::Result { ok: right_ok, error: right_error },
                    ) => left_ok == right_ok && left_error == right_error,
                    (
                        Node::Map { key: left_key, value: left_value },
                        Node::Map { key: right_key, value: right_value },
                    ) => left_key == right_key && left_value == right_value,
                    (Node::Tuple(left), Node::Tuple(right)) => fields_equal(left, right),
                    (
                        Node::Struct {
                            type_id: left_id,
                            type_name: left_name,
                            args: left_args,
                            fields: left_fields,
                        },
                        Node::Struct {
                            type_id: right_id,
                            type_name: right_name,
                            args: right_args,
                            fields: right_fields,
                        },
                    ) => {
                        left_id == right_id
                            && left_name == right_name
                            && left_args == right_args
                            && fields_equal(left_fields, right_fields)
                    }
                    (
                        Node::Enum {
                            type_id: left_id,
                            type_name: left_name,
                            args: left_args,
                            variants: left_variants,
                        },
                        Node::Enum {
                            type_id: right_id,
                            type_name: right_name,
                            args: right_args,
                            variants: right_variants,
                        },
                    ) => {
                        left_id == right_id
                            && left_name == right_name
                            && left_args == right_args
                            && variants_equal(left_variants, right_variants)
                    }
                    (
                        Node::Closure {
                            function: left_function,
                            captures: left_captures,
                            ty: left_ty,
                        },
                        Node::Closure {
                            function: right_function,
                            captures: right_captures,
                            ty: right_ty,
                        },
                    ) => {
                        left_function == right_function
                            && left_captures == right_captures
                            && left_ty.same_checked_type(right_ty)
                    }
                    _ => false,
                }
            })
        }

        struct __JetBootstrapNativeSharedMarshaller<O, T> {
            owner: O,
            owner_identity: usize,
            root: ::std::sync::Weak<__JetBootstrapNativeAdapterRoot>,
            payload_type: ::jet_foundation::MIR::MirType,
            shape: crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
            encode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadEncoder<
                T,
                __JetBootstrapNativePhysicalBindings,
            >,
            decode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadDecoder<
                T,
                __JetBootstrapNativePhysicalBindings,
            >,
        }

        impl<O: Clone, T> Clone for __JetBootstrapNativeSharedMarshaller<O, T> {
            fn clone(&self) -> Self {
                Self {
                    owner: self.owner.clone(),
                    owner_identity: self.owner_identity,
                    root: self.root.clone(),
                    payload_type: self.payload_type.clone(),
                    shape: self.shape.clone(),
                    encode: self.encode,
                    decode: self.decode,
                }
            }
        }

        impl<O, T> __JetBootstrapNativeSharedMarshaller<O, T>
        where
            O: crate::JetSharedPhysicalOwnerApi,
            T: 'static,
        {
            fn invocation_bindings(
                &self,
            ) -> Result<(
                __JetBootstrapNativeAdapter,
                __JetBootstrapNativePhysicalBindings,
            ), String> {
                if self.owner.owner_identity() != self.owner_identity {
                    return Err("Shared marshaller physical owner identity changed".to_string());
                }
                let root = self.root.upgrade()
                    .ok_or_else(|| "Shared marshaller adapter root has retired".to_string())?;
                let shape = root.shared_payload_shapes
                    .get(&self.payload_type.canonical_key())
                    .ok_or_else(|| "Shared marshaller checked payload shape has retired".to_string())?;
                if !__jet_bootstrap_native_shared_shapes_equal(shape, &self.shape) {
                    return Err("Shared marshaller checked payload shape changed".to_string());
                }
                let resources = root.resources_weak.upgrade()
                    .ok_or_else(|| "Shared marshaller Source resource lease has retired".to_string())?;
                let adapter = __jet_bootstrap_native_adapter_new_scope(root, resources, None)?;
                let physical = __JetBootstrapNativePhysicalBindings::new(&adapter);
                Ok((adapter, physical))
            }

            fn validate_payload_shape(
                &self,
                checked_type: &::jet_foundation::MIR::MirType,
                shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                shape_node: usize,
            ) -> Result<(), String> {
                crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(shape)?;
                if !__jet_bootstrap_native_shared_shapes_equal(&self.shape, shape) {
                    return Err("Shared payload converter received a different checked HostTypeShape".to_string());
                }
                let shape_type =
                    crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                        shape,
                        shape_node,
                        0,
                    )?;
                if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
                    &shape_type,
                    checked_type,
                ) {
                    return Err("Shared payload converter shape differs from its checked MIR type".to_string());
                }
                Ok(())
            }
        }

        impl<O, T>
            crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedMarshaller<
                O,
                T,
                __JetBootstrapNativePhysicalBindings,
            > for __JetBootstrapNativeSharedMarshaller<O, T>
        where
            O: crate::JetSharedPhysicalOwnerApi,
            T: 'static,
        {
            fn encode_payload(
                &self,
                field_path: &[String],
                projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                shape_node: usize,
                source_value: &T,
            ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
                self.validate_payload_shape(checked_type, shape, shape_node)?;
                let (adapter, physical) = self.invocation_bindings()?;
                (self.encode)(
                    source_value,
                    checked_type,
                    &adapter.root.program,
                    shape,
                    shape_node,
                    field_path,
                    projection_path,
                    &physical,
                )
            }

            fn decode_payload(
                &self,
                field_path: &[String],
                projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                shape_node: usize,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<T, String> {
                self.validate_payload_shape(checked_type, shape, shape_node)?;
                let (adapter, physical) = self.invocation_bindings()?;
                (self.decode)(
                    runtime_value,
                    checked_type,
                    &adapter.root.program,
                    shape,
                    shape_node,
                    field_path,
                    projection_path,
                    &physical,
                )
            }
        }

        impl __JetBootstrapNativePhysicalBindings {

            fn new_shared_marshaller<O: crate::JetSharedPhysicalOwnerApi, T: 'static>(
                &self,
                owner: O,
                checked_type: &::jet_foundation::MIR::MirType,
                shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                encode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadEncoder<
                    T,
                    Self,
                >,
                decode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadDecoder<
                    T,
                    Self,
                >,
            ) -> Result<__JetBootstrapNativeSharedMarshaller<O, T>, String> {
                crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(shape)?;
                let checked_shape =
                    crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                        shape,
                        shape.root,
                        0,
                    )?;
                if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
                    &checked_shape,
                    checked_type,
                ) {
                    return Err("Shared marshaller HostTypeShape differs from its checked payload type".to_string());
                }
                let expected_shape = self.shared_payload_shape(checked_type)?;
                if !__jet_bootstrap_native_shared_shapes_equal(&expected_shape, shape) {
                    return Err("Shared marshaller HostTypeShape differs from the retained compiler-image shape".to_string());
                }
                let owner_identity = owner.owner_identity();
                if owner_identity == 0 {
                    return Err("Shared marshaller has no stable physical owner identity".to_string());
                }
                Ok(__JetBootstrapNativeSharedMarshaller {
                    owner,
                    owner_identity,
                    root: ::std::sync::Arc::downgrade(&self.adapter.root),
                    payload_type: checked_type.clone(),
                    shape: shape.clone(),
                    encode,
                    decode,
                })
            }

            fn encode_shaped<T: 'static>(
                &self,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                source_value: &T,
            ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                self.encode(
                    field_path,
                    checked_type,
                    self.adapter.root.program.as_ref(),
                    source_value,
                )
            }

            fn decode_shaped<T: 'static>(
                &self,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<Option<T>, String> {
                self.decode(
                    field_path,
                    checked_type,
                    self.adapter.root.program.as_ref(),
                    runtime_value,
                )
            }
        }


        impl crate::BootstrapEntryPhysicalBindings for __JetBootstrapNativePhysicalBindings {
            type InterpreterValue = __JET_HOST_VALUE__;
            type SharedMarshaller<O, T> = __JetBootstrapNativeSharedMarshaller<O, T>
            where
                O: crate::JetSharedPhysicalOwnerApi,
                T: 'static;

            fn encode<T: 'static>(
                &self,
                field_path: &[String],
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::jet_foundation::MIR::MirProgram,
                source_value: &T,
            ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                if __jet_bootstrap_native_binding_field_is(field_path, "host_adapter") {
                    let source = (source_value as &dyn std::any::Any)
                        .downcast_ref::<Box<dyn __JET_TRAIT__>>()
                        .ok_or_else(|| "checked host_adapter field is not its exact Source trait object".to_string())?;
                    let value = source.physical_binding();
                    return __jet_bootstrap_native_adapter_host_runtime(self, checked_type, &value).map(Some);
                }
                if __jet_bootstrap_native_binding_field_is(field_path, "numeric_unit_conversion_exact") {
                    let wrapper = (source_value as &dyn std::any::Any)
                        .downcast_ref::<__JetBootstrapNativeNumericWrapper>()
                        .ok_or_else(|| "checked numeric callback field is not its exact typed wrapper".to_string())?;
                    let (identity, carrier) = __jet_bootstrap_native_numeric_association(wrapper)?;
                    let expected = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked(
                        program,
                        self.adapter.root.artifact,
                        crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY,
                        checked_type.clone(),
                    ).map_err(|error| error.to_string())?;
                    if identity != expected {
                        return Err("numeric callback wrapper association changed checked identity".to_string());
                    }
                    let _ = __jet_bootstrap_native_binding_project(
                        self,
                        field_path,
                        checked_type,
                        program,
                        ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(carrier.clone()),
                    )?;
                    return Ok(Some(::jet_foundation::MIR::MirRuntimeValue::NativeOwned(carrier)));
                }
                Ok(None)
            }

             fn shared_payload_shape(
                 &self,
                 checked_type: &::jet_foundation::MIR::MirType,
             ) -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {
                 self.adapter.root.shared_payload_shapes
                     .get(&checked_type.canonical_key())
                     .cloned()
                     .ok_or_else(|| format!(
                         "checked Source Shared payload type `{}` has no compiler-image HostTypeShape",
                         checked_type.canonical_key(),
                     ))
             }
            fn shared_marshaller<O: crate::JetSharedPhysicalOwnerApi, T: 'static>(
                &self,
                owner: O,
                checked_type: &::jet_foundation::MIR::MirType,
                shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                encode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadEncoder<
                    T,
                    Self,
                >,
                decode: crate::compiler_bootstrap_entry_codec::BootstrapEntrySharedPayloadDecoder<
                    T,
                    Self,
                >,
            ) -> Result<Self::SharedMarshaller<O, T>, String> {
                self.new_shared_marshaller(owner, checked_type, shape, encode, decode)
            }

             fn encode_shared(
                 &self,
                 _field_path: &[String],
                 checked_type: &::jet_foundation::MIR::MirType,
                 program: &::jet_foundation::MIR::MirProgram,
                 physical_identity: usize,
                 root: ::jet_jit::SourceSharedInterop,
                 payload_shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
             ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                 let ::jet_foundation::MIR::MirTypeKind::Shared(payload_type) = checked_type.kind() else {
                     return Err("physical Shared encoder received a non-Shared checked type".to_string());
                 };
                 let expected_type_id = ::jet_jit::SourceSharedInterop::checked_type_id_for_program(
                     program,
                     checked_type,
                 );
                 if root.type_id() != expected_type_id
                     || root.identity() != physical_identity
                     || physical_identity == 0
                 {
                     return Err("Source Shared compiler root disagrees with its checked type or physical owner".to_string());
                 }
                 crate::compiler_bootstrap_entry_codec::bootstrap_entry_validate_shape(payload_shape)?;
                 let shape_type =
                     crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_node_type(
                         payload_shape,
                         payload_shape.root,
                         0,
                     )?;
                 if !crate::compiler_bootstrap_entry_codec::bootstrap_entry_shape_type_matches(
                     &shape_type,
                     payload_type,
                 ) {
                     return Err("Source Shared HostTypeShape differs from its checked payload type".to_string());
                 }
                 let expected_shape = self.shared_payload_shape(payload_type)?;
                 if !__jet_bootstrap_native_shared_shapes_equal(&expected_shape, payload_shape) {
                     return Err("Source Shared HostTypeShape differs from the retained compiler-image shape".to_string());
                 }
                 let owner_alias = root.retain_owner_alias()?;
                 let root = root.with_owner_alias_lease(owner_alias)?;
                 Ok(Some(root.as_native_owned()))
             }

            fn decode<T: 'static>(
                &self,
                field_path: &[String],
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::jet_foundation::MIR::MirProgram,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<Option<T>, String> {
                if __jet_bootstrap_native_binding_field_is(field_path, "host_adapter") {
                    let _ = __jet_bootstrap_native_binding_project(self, field_path, checked_type, program, runtime_value)?;
                    let source: Box<dyn __JET_TRAIT__> = Box::new(self.adapter.clone());
                    let erased: Box<dyn std::any::Any> = Box::new(source);
                    return erased.downcast::<T>()
                        .map(|value| Some(*value))
                        .map_err(|_| "checked host_adapter field has an incompatible Rust leaf type".to_string());
                }
                if __jet_bootstrap_native_binding_field_is(field_path, "numeric_unit_conversion_exact") {
                    let _ = __jet_bootstrap_native_binding_project(
                        self,
                        field_path,
                        checked_type,
                        program,
                        runtime_value,
                    )?;
                    let wrapper = __jet_bootstrap_native_numeric_wrapper_for_adapter(&self.adapter)?;
                    let erased: Box<dyn std::any::Any> = Box::new(wrapper);
                    return erased.downcast::<T>()
                        .map(|value| Some(*value))
                        .map_err(|_| "checked numeric callback field has an incompatible Rust leaf type".to_string());
                }
                Ok(None)
            }
            fn encode_shaped<T: 'static>(
                &self,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                source_value: &T,
            ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                self.encode(
                    field_path,
                    checked_type,
                    self.adapter.root.program.as_ref(),
                    source_value,
                )
            }

            fn decode_shaped<T: 'static>(
                &self,
                field_path: &[String],
                _projection_path: &[crate::compiler_bootstrap_entry_codec::BootstrapEntryHostProjection],
                checked_type: &::jet_foundation::MIR::MirType,
                _shape: &crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,
                _shape_node: usize,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<Option<T>, String> {
                self.decode(
                    field_path,
                    checked_type,
                    self.adapter.root.program.as_ref(),
                    runtime_value,
                )
            }

            fn adopt_interpreter(
                &self,
                field_path: &[String],
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::jet_foundation::MIR::MirProgram,
                interpreter_value: Self::InterpreterValue,
            ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                use ::jet_foundation::MIR::{MirRuntimeClosure, MirRuntimeValue as V, MirTypeKind as K};
                match interpreter_value {
                    __JET_HOST_CLOSURE__(function, captures) => {
                        let checked_params = match checked_type.kind() {
                            K::Fn(signature) => &signature.params,
                            K::SendFn { params, .. } => params,
                            _ => return Err("checked Source closure has no Fn/SendFn type".to_string()),
                        };
                        let source_function = program.functions.iter()
                            .find(|row| row.id == function)
                            .ok_or_else(|| "Source closure function is absent from its checked program".to_string())?;
                        if source_function.params.len() != checked_params.len()
                            || source_function.params.iter().zip(checked_params).any(|(actual, expected)| {
                                !actual.ty.same_checked_type(expected)
                            })
                            || captures.len() != source_function.capture_params.len()
                        {
                            return Err("Source closure captures or callable signature disagree with checked MIR".to_string());
                        }
                        let mut runtime_captures = Vec::with_capacity(captures.len());
                        for (capture, parameter) in captures.into_iter().zip(&source_function.capture_params) {
                            runtime_captures.push(__jet_bootstrap_entry_interpreter_to_runtime(
                                capture,
                                &parameter.ty,
                                program,
                                self,
                            )?);
                        }
                        Ok(Some(V::Closure(MirRuntimeClosure {
                            function,
                            captures: runtime_captures,
                        })))
                    }
                    __JET_HOST_HANDLE__(handle, raw, owner, payload) => {
                        let raw = raw.to_string_rep().parse::<i64>()
                            .map_err(|_| "Source HostValue handle capability is outside i64".to_string())?;
                        let arena = self.adapter.resources.arena();
                        match owner {
                            __JET_OWNER_CURSOR__ => {
                                if handle != ::jet_jit::SourceResources::loop_cursor_handle_id() {
                                    return Err("Source Cursor HostValue disagrees with its checked handle type".to_string());
                                }
                                if payload != __JET_HOST_DATA__(__JET_CT_UNIT__) {
                                    return Err("Source Cursor HostValue carries a non-unit payload".to_string());
                                }
                                let capability = arena.lookup_capability(handle, raw)?;
                                if capability.handle != handle
                                    || capability.kind != ::jet_jit::SourceResources::SourceResourceKind::NativeCursor
                                {
                                    return Err("Source Cursor capability has the wrong checked kind".to_string());
                                }
                                let cursor = arena.lookup_cursor(handle, raw)?;
                                Ok(Some(V::NativeCursor(cursor)))
                            }
                            __JET_OWNER_NATIVE__(binding) => {
                                let rows = self.adapter.root.native_bindings.lock()
                                    .unwrap_or_else(|error| error.into_inner());
                                let mut matching = rows.iter().filter(|row| row.binding == binding);
                                let registration = matching.next()
                                    .ok_or_else(|| "Source native binding capability has no exact adapter registration".to_string())?;
                                if matching.next().is_some()
                                    || registration.capability.handle != handle
                                    || registration.capability.raw != raw
                                    || !registration.identity.value_type().same_checked_type(checked_type)
                                {
                                    return Err("Source native binding capability disagrees with its checked identity".to_string());
                                }
                                if payload != __JET_HOST_DATA__(__JET_CT_UNIT__) {
                                    return Err("Source native binding HostValue carries a non-unit payload".to_string());
                                }
                                let current = arena.lookup_capability(handle, raw)?;
                                if current != registration.capability
                                    || current.kind != ::jet_jit::SourceResources::SourceResourceKind::NativeBinding
                                {
                                    return Err("Source native binding capability generation or kind changed".to_string());
                                }
                                let native = arena.borrow_native_binding(handle, raw, &registration.identity)
                                    .map_err(|error| error.to_string())?;
                                if registration.dynamic
                                    && !self.adapter.instance.matches_root(native.carrier())
                                {
                                    return Err("Source native binding does not retain this callback-scope instance".to_string());
                                }
                                Ok(Some(V::NativeOwned(native.carrier().clone_root())))
                            }
                            __JET_OWNER_CORE__(_) => Err(
                                "Core owner HostValue must cross the typed Source transfer-receipt seam".to_string(),
                            ),
                            __JET_OWNER_DECLARED__(_) => Err(
                                "declared owner HostValue has no registered native physical binding".to_string(),
                            ),
                        }
                    }
                    __JET_HOST_SHARED_CARRIER__(carrier) => {
                        if carrier.__JET_CARRIER_KIND__ != __JET_HOST_KIND_SHARED__
                            || !matches!(checked_type.kind(), K::Shared(_))
                        {
                            return Err("Source Shared carrier does not match its checked Shared<T> type".to_string());
                        }
                        let capability = match &carrier.__JET_CARRIER_PHYSICAL__ {
                            __JET_PHYSICAL_SHARED__(entry) => entry.__JET_VALUE_METADATA__()
                                .guard_read().value.__JET_VALUE_PHYSICAL_ROOT__.
                                clone().ok_or_else(|| "Source Shared carrier has no physical root capability".to_string())?,
                            _ => return Err("Source Shared carrier has a mismatched physical root variant".to_string()),
                        };
                        let handle = capability.__JET_CAP_HANDLE__;
                        let raw = capability.__JET_CAP_RAW__.to_string_rep().parse::<i64>()
                            .map_err(|_| "Source Shared capability is outside i64".to_string())?;
                        let generation = capability.__JET_CAP_GENERATION__.to_string_rep().parse::<u32>()
                            .map_err(|_| "Source Shared generation is outside u32".to_string())?;
                        let type_identity = u64::try_from(capability.__JET_CAP_TYPE_IDENTITY__.to_string_rep().parse::<i64>()
                            .map_err(|_| "Source Shared type identity is outside i64".to_string())?)
                            .map_err(|_| "Source Shared type identity is negative".to_string())?;
                        let physical_identity = usize::try_from(capability.__JET_CAP_PHYSICAL_IDENTITY__.to_string_rep().parse::<i64>()
                            .map_err(|_| "Source Shared physical identity is outside i64".to_string())?)
                            .map_err(|_| "Source Shared physical identity is negative".to_string())?;
                        if physical_identity == 0
                            || type_identity != ::jet_jit::SourceSharedInterop::checked_type_id_for_program(program, checked_type)
                        {
                            return Err("Source Shared capability has a mismatched checked owner identity".to_string());
                        }
                        let arena = self.adapter.resources.arena();
                        let current = arena.lookup_capability(handle, raw)?;
                        if current.handle != ::jet_jit::SourceResources::shared_interop_root_handle_id()
                            || current.generation != generation
                            || current.kind != (::jet_jit::SourceResources::SourceResourceKind::SharedInteropRoot { type_identity })
                        {
                            return Err("Source Shared capability generation or checked root kind changed".to_string());
                        }
                        let (root, lease) = arena.borrow_shared_interop_root(&current, type_identity)
                            .map_err(|error| error.to_string())?;
                        if root.type_id() != type_identity || root.identity() != physical_identity {
                            return Err("Source Shared capability resolves to a different physical owner".to_string());
                        }
                        Ok(Some(root.with_resource_lease_owner(lease).as_native_owned()))
                    }
                    other => {
                        let _ = (field_path, other);
                        Ok(None)
                    }
                }
            }

            fn project_interpreter(
                &self,
                field_path: &[String],
                checked_type: &::jet_foundation::MIR::MirType,
                program: &::jet_foundation::MIR::MirProgram,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<Option<Self::InterpreterValue>, String> {
                __jet_bootstrap_native_binding_project(self, field_path, checked_type, program, runtime_value)
            }
        }
    "#;
    let generated = generated
        .replace("__JET_SOURCE_BINDING_IDENTITY__", &source_binding_identity)
        .replace("__JET_SOURCE_RESOURCE_IDENTITY__", &resource_identity)
        .replace("__JET_SOURCE_NATIVE_BINDING__", &source_native_binding)
        .replace("__JET_OWNER_CALLABLE__", &owner_callable)
        .replace("__JET_OWNER_INTERFACE__", &owner_interface)
        .replace("__JET_OWNER_NATIVE__", &owner_native)
        .replace("__JET_OWNER_CURSOR__", &owner_cursor)
        .replace("__JET_OWNER_CORE__", &owner_core)
        .replace("__JET_OWNER_DECLARED__", &owner_declared)
        .replace("__JET_HOST_VALUE__", &host_value)
        .replace("__JET_HOST_HANDLE__", &host_handle)
        .replace("__JET_HOST_DATA__", &host_data)
        .replace("__JET_HOST_CLOSURE__", &host_closure)
        .replace("__JET_HOST_SHARED_CARRIER__", &host_shared_carrier)
        .replace("__JET_HOST_KIND_SHARED__", &host_kind_shared)
        .replace("__JET_PHYSICAL_SHARED__", &physical_shared)
        .replace("__JET_PHYSICAL_WEAK__", &physical_weak)
        .replace("__JET_CARRIER_PHYSICAL__", &carrier_physical)
        .replace("__JET_CARRIER_KIND__", &carrier_kind)
        .replace("__JET_VALUE_METADATA__", &value_metadata)
        .replace("__JET_VALUE_PHYSICAL_ROOT__", &value_physical_root)
        .replace("__JET_WEAK_METADATA__", &weak_metadata)
        .replace("__JET_WEAK_PHYSICAL_ROOT__", &weak_physical_root)
        .replace("__JET_CAP_HANDLE__", &cap_handle)
        .replace("__JET_CAP_RAW__", &cap_raw)
        .replace("__JET_CAP_GENERATION__", &cap_generation)
        .replace("__JET_CAP_TYPE_IDENTITY__", &cap_type_identity)
        .replace("__JET_CAP_PHYSICAL_IDENTITY__", &cap_physical_identity)
        .replace("__JET_WEAK_CAP_HANDLE__", &weak_cap_handle)
        .replace("__JET_WEAK_CAP_RAW__", &weak_cap_raw)
        .replace("__JET_WEAK_CAP_GENERATION__", &weak_cap_generation)
        .replace("__JET_WEAK_CAP_TYPE_IDENTITY__", &weak_cap_type_identity)
        .replace("__JET_WEAK_CAP_PHYSICAL_IDENTITY__", &weak_cap_physical_identity)
        .replace("__JET_EVAL_RESULT__", &eval_result)
        .replace("__JET_HOST_TYPE_SHAPE__", &host_type_shape)
        .replace("__JET_RESULT_TRANSFER_RECEIPT__", &result_transfer_receipt)
        .replace("__JET_RESULT_HOST_TYPE_SHAPE__", &result_host_type_shape)
        .replace("__JET_RESULT_TRANSFER_PREPARE__", &result_transfer_prepare)
        .replace("__JET_RESULT_TRANSFER_COMMIT__", &result_transfer_commit)
        .replace("__JET_RESULT_TRANSFER_DISPOSE__", &result_transfer_dispose)
        .replace("__JET_EVAL_MACHINE__", &eval_machine)
        .replace("__JET_EVAL_CONFIG__", &symbols.type_symbol("JetEvalConfig")?)
        .replace("__JET_RESULT_TRANSFER_DISPOSITION__", &result_transfer_disposition)
        .replace("__JET_TRANSFER_COMMITTED__", &transfer_committed)
        .replace("__JET_TRANSFER_REJECTED__", &transfer_rejected)
        .replace("__JET_TRANSFER_CONSUMED_FAILURE__", &transfer_consumed_failure)
        .replace("__JET_HOST_ADAPTER_FIELD__", &host_adapter_field_symbol)
        .replace("__JET_CT_UNIT__", &ct_unit)
        .replace("__JET_TRAIT__", &symbols.trait_symbol("JetEvalHostAdapter")?)
        .replace("__JET_HOST_OWNER__", &host_field.owner.0.to_string())
        .replace("__JET_HOST_FIELD__", &host_field.field.0.to_string())
        .replace("__JET_NUMERIC_OWNER__", &numeric_field.owner.0.to_string())
        .replace("__JET_NUMERIC_FIELD__", &numeric_field.field.0.to_string());
    out.push_str(&generated);
    Ok(())
}

fn callable_symbol(
    symbols: &BootstrapCodecSymbols<'_>,
    source_name: &str,
) -> Result<String, BootstrapHostCodecError> {
    let mut rows = symbols
        .metadata
        .callables
        .iter()
        .filter(|row| row.source_name == source_name);
    let row = rows
        .next()
        .ok_or_else(|| BootstrapHostCodecError::MissingEntry(source_name.to_string()))?;
    if rows.next().is_some() {
        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "Source helper `{source_name}` has an ambiguous emitted symbol"
        )));
    }
    Ok(row.metadata.symbol.clone())
}

