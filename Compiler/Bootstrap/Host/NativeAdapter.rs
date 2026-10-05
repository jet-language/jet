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
             machine_abi_shape: crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,\n\
             shared_payload_shapes: ::std::collections::BTreeMap<String, crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape>,\n\
             completion_scope: ::jet_jit::SourceExecutionCompletionScopeWeak,\n\
             callback_jobs: ::std::sync::Weak<dyn ::jet_jit::SourceCallbacks::SourceCallbackJobOwner>,\n\
             resources_weak: ::jet_jit::SourceResources::SourceResourceLeaseWeak,\n\
             artifact: ::jet_foundation::MIR::MirArtifactId,
             receiver_type: ::jet_foundation::MIR::MirType,
             execution: ::jet_foundation::MIR::MirExecutionIdentity,
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
             let template = bindings.create_object(()).map_err(|error| error.to_string())?;\n\
             // The image's sealed identity: restore proved it equals the\n\
             // archived execution identity, so no method re-digests the program.\n\
             let execution = program.sealed_execution_identity(Some(artifact)).map_err(|error| error.to_string())?;\n"
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
             let descriptor = ::jet_jit::SourceInterfaces::NativeInterfaceMethod::checked_for_execution(\n\
                 program,\n\
                 execution.clone(),\n\
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
                 machine_abi_shape: crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape,\n\
                 shared_payload_shapes: ::std::collections::BTreeMap<String, crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape>,\n\
                 completion_scope: ::jet_jit::SourceExecutionCompletionScope,\n\
                 callback_jobs: ::std::sync::Arc<dyn ::jet_jit::SourceCallbacks::SourceCallbackJobOwner>,\n\
                 artifact: ::jet_foundation::MIR::MirArtifactId,\n\
                 receiver_type: ::jet_foundation::MIR::MirType,\n\
                 template: ::jet_jit::SourceInterfaces::NativeInterfaceObject,\n\
                 native_bindings: Vec<__JetBootstrapNativeBindingRegistration>,\n\
             ) -> Result<Self, String> {{\n\
                 if receiver_type.canonical_key() != {expected_receiver_key} {{\n\
                     return Err(\"native adapter constructor receiver type differs from the checked host-adapter field\".to_string());\n\
                 }}\n\
                 let execution = program.sealed_execution_identity(Some(artifact)).map_err(|error| error.to_string())?;\n\
                 let root = ::std::sync::Arc::new(__JetBootstrapNativeAdapterRoot {{\n\
                     bindings,\n\
                     program,\n\
                     machine_abi_shape,\n\
                     shared_payload_shapes,\n\
                     completion_scope: completion_scope.downgrade(),\n\
                     callback_jobs: ::std::sync::Arc::downgrade(&callback_jobs),\n\
                     resources_weak: resources.downgrade(),\n\
                     artifact,\n\
                     execution,\n\
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
                let arguments = native_operation_arguments(metadata, false);
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
        // Checked non-optional methods are fallible (`JetOutcome<T, JetErr>`);
        // the native implementation itself never fails them.
        let result = if return_type.ends_with(", JetErr>") { format!("Ok({result})") } else { result };
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
/// the program and physical bindings when the helper needs them. The helper
/// takes each argument as the checked trait method spells it; `decoded` names
/// owned values decoded from a native interface call, which are borrowed
/// where the trait method takes a reference.
fn native_operation_arguments(metadata: &MirRustTraitMethodMetadata, decoded: bool) -> String {
    let mut arguments = (1..metadata.parameter_types.len())
        .map(|index| {
            let borrow = if decoded && metadata.parameter_types[index].starts_with('&') { "&" } else { "" };
            format!(", {borrow}__jet_arg_{index}")
        })
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
            let checked = ::jet_jit::SourceInterfaces::NativeInterfaceMethod::checked_for_execution(
                owner.root.program.as_ref(),
                owner.root.execution.clone(),
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
                    // These checked returns are emitted fallible
                    // (`JetOutcome<T, JetErr>`); the native side never fails them.
                    "poll_callbacks" | "drain_callbacks" => format!(
                        "|mut machine| match machine.typed_mut() {{ Some(machine) => Ok(Ok(__jet_bootstrap_native_{name}(&adapter.root, &adapter.callbacks, machine))), None => Err(\"native adapter {name} needs the typed Source machine\".to_string()) }}"
                    ),
                    "physical_binding" => {
                        "|| Ok(Ok(__jet_bootstrap_native_adapter_physical_binding(&adapter)))".to_string()
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
                        let arguments = native_operation_arguments(metadata, true);
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
    cleanup: Option<::jet_jit::SourceResources::SourceResourceCleanupLease>,
    // Keeps this callback session admitted by the invocation's job owner.
    _registration: ::jet_jit::SourceCallbacks::SourceCallbackJobSessionRegistration,
    failures: __JetBootstrapNativeCallbackFailures,
}

/// The sendable part of a callback scope that reports transport failures.
/// Callbacks run only on the machine thread (the `poll_callbacks` /
/// `drain_callbacks` pump), so the transport hooks never run Source code.
#[derive(Clone)]
struct __JetBootstrapNativeCallbackFailures {
    execution: ::jet_foundation::MIR::MirExecutionIdentity,
    resources: ::jet_jit::SourceResources::SourceResourceLeaseWeak,
    completion_scope: ::jet_jit::SourceExecutionCompletionScope,
    failed: ::std::sync::Arc<::std::sync::atomic::AtomicBool>,
}

impl __JetBootstrapNativeCallbackFailures {
    fn record(&self, cause: ::jet_jit::SourceDeoptError) {
        self.failed.store(true, ::std::sync::atomic::Ordering::Release);
        self.completion_scope.record(::jet_jit::SourceExecutionCompletion {
            execution: Some(self.execution.clone()),
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
}

impl __JetBootstrapNativeCallbackContext {
    fn record_failure(&self, cause: ::jet_jit::SourceDeoptError) {
        self.failures.record(cause);
    }
}

fn __jet_bootstrap_native_callback_transport(
    failures: __JetBootstrapNativeCallbackFailures,
) -> __JetBootstrapNativeCallbackSession {
    __JetBootstrapNativeCallbackSession::new(
        move |abandonment| {
            // Payloads are machine callback leases; without the machine they
            // can only be reported, never released.
            if !abandonment.is_empty() {
                failures.record(::jet_jit::SourceDeoptError::Callback(
                    "Source callback session was abandoned with unreleased callbacks or undelivered events".to_string(),
                ));
            }
        },
        // The machine-thread pump drains ready events; no job is scheduled.
        |_ready| {},
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
        callback_failed: context.failures.failed.clone(),
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
    let failures = __JetBootstrapNativeCallbackFailures {
        execution: root.execution.clone(),
        resources: resources.downgrade(),
        completion_scope,
        failed: ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false)),
    };
    let callbacks = __jet_bootstrap_native_callback_transport(failures.clone());
    let context = ::std::sync::Arc::new(__JetBootstrapNativeCallbackContext {
        root,
        cleanup,
        _registration: registration,
        failures,
    });
    Ok(__jet_bootstrap_native_adapter_owner_from_context(context, callbacks, resources, false))
}
"#);
    Ok(())
}

/// The machine-thread callback pump. Callback payloads are Source callback
/// leases (`jet_eval_callback_transfer`) that stay in the machine's callback
/// table, so only the owner of the `&mut` machine (`poll_callbacks` /
/// `drain_callbacks`) invokes, cleans up or releases them.
fn emit_native_callback_helpers(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let eval_value = symbols.type_symbol("JetEvalRuntimeValue")?;
    let eval_machine = symbols.type_symbol("JetEvalMachine")?;
    let callback_result = symbols.type_symbol("JetEvalCallbackResult")?;
    let span = symbols.type_symbol("Span")?;
    let callback_session = "::jet_jit::SourceCallbacks::SourceCallbackSession";
    let callback_event = "::jet_jit::SourceCallbacks::SourceCallbackEvent";
    let callback_error = "::jet_jit::SourceCallbacks::SourceCallbackError";
    let callback_reply_error = "::jet_jit::SourceCallbacks::SourceCallbackReplyError";
    let invoke = symbols.call(
        "jet_eval_callback_invoke",
        &[("*machine", ""), ("lease", "lease"), ("args", "args"), ("span", "span.clone()")],
    )?;
    let merge = symbols.call(
        "jet_eval_merge_callback_output",
        &[
            ("*machine", ""),
            ("result.@f.JetEvalCallbackResult.stdout@", "result.@f.JetEvalCallbackResult.stdout@.clone()"),
            ("result.@f.JetEvalCallbackResult.stderr@", "result.@f.JetEvalCallbackResult.stderr@.clone()"),
        ],
    )?;
    let release = symbols.call(
        "jet_eval_callback_release",
        &[("*machine", ""), ("lease", "lease"), ("span", "span.clone()")],
    )?;
    let drop_argument = symbols.call(
        "jet_eval_drop_value",
        &[("*machine", ""), ("value", "value"), ("span", "span.clone()")],
    )?;
    let drop_callback_value = symbols.call(
        "jet_eval_drop_value",
        &[
            ("*machine", ""),
            ("@deref.JetEvalCallbackOutcome.Value.value@value", "@deref.JetEvalCallbackOutcome.Value.value@value"),
            ("span", "span.clone()"),
        ],
    )?;
    writeln!(
        out,
        "#[doc(hidden)]\n\
         struct __JetBootstrapNativeCallbackPayload {{\n\
             lease: i64,\n\
             span: {span},\n\
         }}\n\
         type __JetBootstrapNativeCallbackSession = {callback_session}<__JetBootstrapNativeCallbackPayload, Vec<{eval_value}>, {span}, ({callback_result}, {span})>;\n\
         fn __jet_bootstrap_native_callback_drop_result(\n\
             machine: &mut {eval_machine},\n\
             result: {callback_result},\n\
             span: {span},\n\
         ) -> bool {{\n\
             match result.@f.JetEvalCallbackResult.outcome@ {{\n\
                 @p.JetEvalCallbackOutcome.Value@{{ value }} => {drop_callback_value},\n\
                 _ => true,\n\
             }}\n\
         }}\n\
         fn __jet_bootstrap_native_poll_callbacks(\n\
             root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &mut {eval_machine},\n\
         ) -> bool {{\n\
             __jet_bootstrap_native_pump_callbacks(root, callbacks, machine, false)\n\
         }}\n\
         fn __jet_bootstrap_native_drain_callbacks(\n\
             root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &mut {eval_machine},\n\
         ) -> bool {{\n\
             __jet_bootstrap_native_pump_callbacks(root, callbacks, machine, true)\n\
         }}\n\
         fn __jet_bootstrap_native_pump_callbacks(\n\
             _root: &__JetBootstrapNativeAdapterRoot,\n\
             callbacks: &__JetBootstrapNativeCallbackSession,\n\
             machine: &mut {eval_machine},\n\
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
                         let callback_lease = callbacks.with_payload(invocation.callback_id(), |payload| payload.lease);\n\
                         let args = invocation.take_command();\n\
                         let span = invocation.take_context();\n\
                         match callback_lease {{\n\
                             Ok(lease) if lease > 0 => {{\n\
                                 let lease = jet_foundation::Numeric::JetInt::from_i64(lease);\n\
                                 let result = {invoke};\n\
                                 {merge};\n\
                                 match invocation.respond(Ok((result, span))) {{\n\
                                     Ok(()) => true,\n\
                                     Err({callback_reply_error}::Disconnected) => true,\n\
                                     Err(_) => false,\n\
                                 }}\n\
                             }}\n\
                             _ => {{\n\
                                 let mut cleanup_ok = true;\n\
                                 for value in args {{ cleanup_ok &= {drop_argument}; }}\n\
                                 let replied = invocation.respond(Err({callback_error}::UnknownCallback)).is_ok();\n\
                                 cleanup_ok && replied\n\
                             }}\n\
                         }}\n\
                     }}\n\
                     {callback_event}::Cleanup(mut cleanup) => {{\n\
                         let span = cleanup.take_context().or_else(|| callbacks.with_payload(cleanup.callback_id(), |payload| payload.span.clone()).ok());\n\
                         let Some(span) = span else {{ return false; }};\n\
                         let mut ok = true;\n\
                         if let Some(args) = cleanup.take_command() {{\n\
                             for value in args {{ ok &= {drop_argument}; }}\n\
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
                         let lease = jet_foundation::Numeric::JetInt::from_i64(release.payload().lease);\n\
                         let span = release.payload().span.clone();\n\
                         let released = {release};\n\
                         let completed = release.complete().is_ok();\n\
                         released && completed\n\
                     }}\n\
                 }};\n\
                 if !event_ok {{ return false; }}\n\
                 if drain {{ let _ = callbacks.retire(); }}\n\
             }}\n\
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_native_binding_helpers(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
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
            dyn FnMut(f64, &String, &String, &String, &String)
                -> Result<f64, jet_foundation::Outcome::JetAbsent>;
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
            let execution = program.sealed_execution_identity(Some(artifact))
                .map_err(|error| error.to_string())?;
            let identity = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked_for_execution(
                execution,
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
            let execution = program.sealed_execution_identity(Some(artifact))
                .map_err(|error| error.to_string())?;
            let expected = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked_for_execution(
                execution.clone(),
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
            let active_execution = execution;
            let callback_identity = identity.clone();
            let wrapper = bindings.create_native_callable_rc_wrapper(
                identity,
                carrier,
                move |registration| {
                    ::std::rc::Rc::new(::std::cell::RefCell::new(Some(
                        Box::new(move |value: f64, source_unit: &String, destination_unit: &String, scale_num: &String, scale_den: &String| {
                            let _registration = &registration;
                            let _scope = active_bindings.activate_for_execution(active_program.as_ref(), artifact, active_execution.clone())
                                .unwrap_or_else(|error| panic!("numeric native callable activation failed: {error}"));
                            let values = vec![
                                ::jet_foundation::MIR::MirRuntimeValue::Float { value, f32: false },
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
                                    Err(jet_foundation::Outcome::JetAbsent)
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
            let @p.JetEvalNativeBindingIdentity.Callable@(owner) = &row.binding else {
                return Err("numeric callable registration has a non-callable Source identity".to_string());
            };
            if identity.key != crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY
                || owner.@f.JetEvalNativeCallableBinding.key@ != identity.key
                || !__jet_bootstrap_type_to_host(&owner.@f.JetEvalNativeCallableBinding.callable_type@)?
                    .same_checked_type(&identity.callable_type)
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
            ).map_err(|error| format!("{error:?}"))?;
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

        // `expected_execution` is `program`'s sealed execution identity for
        // `artifact`, computed once by the owner of `program` (the image never
        // changes), so a per-session registration never re-digests the program.
        fn __jet_bootstrap_native_binding_registration(
            resources: &::jet_jit::SourceResources::SourceResourceLease,
            cleanup: Option<&::jet_jit::SourceResources::SourceResourceCleanupLease>,
            program: &::jet_foundation::MIR::MirProgram,
            artifact: ::jet_foundation::MIR::MirArtifactId,
            expected_execution: &::jet_foundation::MIR::MirExecutionIdentity,
            field_path: Vec<String>,
            binding: __JET_SOURCE_BINDING_IDENTITY__,
            identity: __JET_SOURCE_RESOURCE_IDENTITY__,
            object: ::jet_jit::SourceInterfaces::NativeInterfaceObject,
            dynamic: bool,
        ) -> Result<__JetBootstrapNativeBindingRegistration, String> {
            let host_field = __jet_bootstrap_native_binding_field_is(&field_path, "host_adapter");
            let numeric_field = __jet_bootstrap_native_binding_field_is(&field_path, "numeric_unit_conversion_exact");
            if (host_field && matches!(&binding, @p.JetEvalNativeBindingIdentity.Interface@(..)))
                || (numeric_field && !dynamic && matches!(&binding, @p.JetEvalNativeBindingIdentity.Callable@(..)))
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
            // The Source owner identity, as its key (callables only) and checked host type.
            let (owner_key, owner_type) = match &binding {
                @p.JetEvalNativeBindingIdentity.Callable@(owner) => (
                    Some(owner.@f.JetEvalNativeCallableBinding.key@.clone()),
                    __jet_bootstrap_type_to_host(&owner.@f.JetEvalNativeCallableBinding.callable_type@)?,
                ),
                @p.JetEvalNativeBindingIdentity.Interface@(receiver_type) => {
                    (None, __jet_bootstrap_type_to_host(receiver_type)?)
                }
            };
            match &identity {
                __JET_SOURCE_RESOURCE_IDENTITY__::Interface { execution, artifact: owner_artifact, receiver_type }
                    if host_field
                        && owner_key.is_none()
                        && *execution == *expected_execution
                        && *owner_artifact == artifact
                        && receiver_type.same_checked_type(expected_host)
                        && owner_type.same_checked_type(expected_host) => {}
                __JET_SOURCE_RESOURCE_IDENTITY__::Callable(source)
                    if numeric_field
                        && source.execution == *expected_execution
                        && source.artifact == artifact
                        && owner_key.as_ref().is_some_and(|key| source.key == *key)
                        && source.key == __JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY
                        && source.callable_type.same_checked_type(expected_numeric)
                        && owner_type.same_checked_type(expected_numeric) => {}
                _ => return Err("native binding identity disagrees with its exact checked Source field".to_string()),
            }
            if !identity.value_type().same_checked_type(&owner_type) {
                return Err("native binding owner type disagrees with its physical capability identity".to_string());
            }
            let physical = match &identity {
                __JET_SOURCE_RESOURCE_IDENTITY__::Callable(callable) =>
                    __JET_SOURCE_NATIVE_BINDING__::callable(&object, callable.clone())?,
                __JET_SOURCE_RESOURCE_IDENTITY__::Interface { execution, artifact, receiver_type } =>
                    __JET_SOURCE_NATIVE_BINDING__::interface(&object, execution.clone(), *artifact, receiver_type.clone())?,
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
                &root.execution,
                vec!["JetEvalConfig".to_string(), "host_adapter".to_string()],
                @new.JetEvalNativeBindingIdentity.Interface@(__jet_bootstrap_type_from_host(&root.receiver_type)?),
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
        ) -> @t.JetEvalHostValue@ {
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
            ).unwrap_or_else(|error| panic!("native adapter capability identity changed: {error:?}"));
            if binding.carrier() != &adapter.instance.clone_root()
                || !adapter.instance.matches_root(binding.carrier())
            {
                panic!("native adapter capability no longer retains this callback-scope instance");
            }
            let receiver_type = __jet_bootstrap_type_from_host(&adapter.root.receiver_type)
                .unwrap_or_else(|error| panic!("native adapter receiver type has no Source spelling: {error}"));
            __jet_bootstrap_resource_host_handle(
                &adapter.capability,
                @new.JetEvalHostOwner.Native@(@new.JetEvalNativeBindingIdentity.Interface@(receiver_type)),
            ).unwrap_or_else(|error| panic!("native adapter capability has no Source handle: {error}"))
        }
        fn __jet_bootstrap_native_adapter_host_runtime(
            physical: &__JetBootstrapNativePhysicalBindings,
            checked_type: &::jet_foundation::MIR::MirType,
            value: &@t.JetEvalHostValue@,
        ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
            let not_interface = || "host_adapter did not return its checked native interface capability".to_string();
            let @p.JetEvalHostValue.Handle@(handle, raw, owner, payload) = value else { return Err(not_interface()) };
            let @p.JetEvalHostOwner.Native@(binding) = &@deref.JetEvalHostValue.Handle.owner@*owner else { return Err(not_interface()) };
            let @p.JetEvalNativeBindingIdentity.Interface@(receiver_type) = &@deref.JetEvalHostOwner.Native.binding@*binding else {
                return Err(not_interface());
            };
            if !matches!(&@deref.JetEvalHostValue.Handle.payload@*payload, @p.JetEvalHostValue.Data@(data) if matches!(&@deref.JetEvalHostValue.Data.value@*data, @p.TComptimeValue.Unit@)) {
                return Err(not_interface());
            }
            let receiver_type = __jet_bootstrap_type_to_host(receiver_type)?;
            let handle = ::jet_foundation::MIR::MirHandleId(handle.@f.MIRHandleID.value@);
            if !receiver_type.same_checked_type(checked_type)
                || !receiver_type.same_checked_type(&physical.adapter.root.receiver_type)
                || handle != physical.adapter.capability.handle
                || raw.to_i64() != Some(physical.adapter.capability.raw)
            {
                return Err("host_adapter capability is not the exact current callback-scope instance".to_string());
            }
            let current = physical.adapter.resources.arena()
                .lookup_capability(handle, physical.adapter.capability.raw)?;
            if current != physical.adapter.capability {
                return Err("host_adapter capability generation changed before encoding".to_string());
            }
            let identity = __JET_SOURCE_RESOURCE_IDENTITY__::Interface {
                execution: physical.adapter.root.execution.clone(),
                artifact: physical.adapter.root.artifact,
                receiver_type,
            };
            let binding = physical.adapter.resources.arena().borrow_native_binding(
                handle,
                physical.adapter.capability.raw,
                &identity,
            ).map_err(|error| format!("{error:?}"))?;
            if binding.carrier() != &physical.adapter.instance.clone_root()
                || !physical.adapter.instance.matches_root(binding.carrier())
            {
                return Err("host_adapter capability does not retain the exact callback-scope root".to_string());
            }
            Ok(::jet_foundation::MIR::MirRuntimeValue::NativeOwned(
                physical.adapter.instance.clone_root(),
            ))
        }


        /// Check that a native Source field's runtime carrier is the physical
        /// root of exactly one registration of that field.
        fn __jet_bootstrap_native_binding_project(
            physical: &__JetBootstrapNativePhysicalBindings,
            field_path: &[String],
            checked_type: &::jet_foundation::MIR::MirType,
            value: ::jet_foundation::MIR::MirRuntimeValue,
        ) -> Result<(), String> {
            let Some(field) = field_path.last().map(String::as_str) else {
                return Err("native Source field has no checked field path".to_string());
            };
            let ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(carrier) = value else {
                return Err(format!("native Source field `{}` has no retained NativeOwned carrier", field_path.join(".")));
            };
            let rows = physical.adapter.root.native_bindings.lock().unwrap_or_else(|error| error.into_inner());
            let mut matched = false;
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
                ).map_err(|error| format!("{error:?}"))?;
                if binding.carrier() != &carrier {
                    continue;
                }
                if row.dynamic && (field != "host_adapter"
                    || !physical.adapter.instance.matches_root(binding.carrier()))
                {
                    continue;
                }
                let leaf_matches = match &row.binding {
                    @p.JetEvalNativeBindingIdentity.Callable@(..) => field == "numeric_unit_conversion_exact",
                    @p.JetEvalNativeBindingIdentity.Interface@(..) => field == "host_adapter",
                };
                if !leaf_matches {
                    return Err("native Source field binding variant disagrees with its checked leaf".to_string());
                }
                if std::mem::replace(&mut matched, true) {
                    return Err(format!("native Source field `{}` has ambiguous physical binding registrations", field_path.join(".")));
                }
            }
            if matched {
                Ok(())
            } else {
                Err(format!("native Source field `{}` does not retain the supplied exact carrier", field_path.join(".")))
            }
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
            type SharedMarshaller<O, T> = __JetBootstrapNativeSharedMarshaller<O, T>
            where
                O: crate::JetSharedPhysicalOwnerApi,
                T: 'static;

            fn encode<T: 'static>(
                &self,
                field_path: &[String],
                checked_type: &::jet_foundation::MIR::MirType,
                _program: &::jet_foundation::MIR::MirProgram,
                source_value: &T,
            ) -> Result<Option<::jet_foundation::MIR::MirRuntimeValue>, String> {
                if __jet_bootstrap_native_binding_field_is(field_path, "host_adapter") {
                    let source = (source_value as &dyn std::any::Any)
                        .downcast_ref::<Box<dyn __JET_TRAIT__>>()
                        .ok_or_else(|| "checked host_adapter field is not its exact Source trait object".to_string())?;
                    let value = source.__JET_PHYSICAL_BINDING_METHOD__()
                        .map_err(|_| "host_adapter physical binding failed".to_string())?;
                    return __jet_bootstrap_native_adapter_host_runtime(self, checked_type, &value).map(Some);
                }
                if __jet_bootstrap_native_binding_field_is(field_path, "numeric_unit_conversion_exact") {
                    let wrapper = (source_value as &dyn std::any::Any)
                        .downcast_ref::<__JetBootstrapNativeNumericWrapper>()
                        .ok_or_else(|| "checked numeric callback field is not its exact typed wrapper".to_string())?;
                    let (identity, carrier) = __jet_bootstrap_native_numeric_association(wrapper)?;
                    let expected = ::jet_jit::SourceInterfaces::NativeCallableIdentity::checked_for_execution(
                        self.adapter.root.execution.clone(),
                        self.adapter.root.artifact,
                        crate::__JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY,
                        checked_type.clone(),
                    ).map_err(|error| error.to_string())?;
                    if identity != expected {
                        return Err("numeric callback wrapper association changed checked identity".to_string());
                    }
                    __jet_bootstrap_native_binding_project(
                        self,
                        field_path,
                        checked_type,
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
                 root: ::jet_jit::SourceSharedInterop::SourceSharedInterop,
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
                _program: &::jet_foundation::MIR::MirProgram,
                runtime_value: ::jet_foundation::MIR::MirRuntimeValue,
            ) -> Result<Option<T>, String> {
                if __jet_bootstrap_native_binding_field_is(field_path, "host_adapter") {
                    __jet_bootstrap_native_binding_project(self, field_path, checked_type, runtime_value)?;
                    let source: Box<dyn __JET_TRAIT__> = Box::new(self.adapter.clone());
                    let erased: Box<dyn std::any::Any> = Box::new(source);
                    return erased.downcast::<T>()
                        .map(|value| Some(*value))
                        .map_err(|_| "checked host_adapter field has an incompatible Rust leaf type".to_string());
                }
                if __jet_bootstrap_native_binding_field_is(field_path, "numeric_unit_conversion_exact") {
                    __jet_bootstrap_native_binding_project(self, field_path, checked_type, runtime_value)?;
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
        }
    "#;
    let generated = generated
        .replace("__JET_SOURCE_BINDING_IDENTITY__", "@t.JetEvalNativeBindingIdentity@")
        .replace("__JET_SOURCE_RESOURCE_IDENTITY__", &resource_identity)
        .replace("__JET_SOURCE_NATIVE_BINDING__", &source_native_binding)
        .replace("__JET_TRAIT__", symbols.trait_symbol("JetEvalHostAdapter")?)
        .replace(
            "__JET_PHYSICAL_BINDING_METHOD__",
            &symbols.trait_method_metadata("JetEvalHostAdapter", "physical_binding")?.symbol,
        )
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
    symbols.callable_symbol(source_name).map(str::to_string)
}

