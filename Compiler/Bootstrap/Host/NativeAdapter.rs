use super::{BootstrapCodecSymbols, BootstrapHostCodecError};
use std::fmt::Write as _;

pub(crate) fn emit_bootstrap_native_adapter_impl(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    const METHODS: &[(&str, usize)] = &[
        ("new_session", 0),
        ("clone_adapter", 0),
        ("poll_callbacks", 1),
        ("drain_callbacks", 1),
        ("host_call", 6),
        ("foreign_call", 4),
        ("handle_call", 5),
        ("task_group", 6),
        ("native_call", 5),
        ("channel_select", 6),
        ("shared_call", 10),
    ];
    let trait_symbol = symbols.trait_symbol("JetEvalHostAdapter")?;
    let mut methods = Vec::with_capacity(METHODS.len());
    for (name, arity) in METHODS {
        let metadata = symbols.trait_method_metadata("JetEvalHostAdapter", name)?;
        if metadata.name != *name
            || metadata.receiver_access.is_none()
            || metadata.parameter_types.len() != *arity
            || metadata.parameter_access.len() != *arity
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetEvalHostAdapter.{name} has an incompatible checked Rust signature"
            )));
        }
        methods.push((metadata, *arity));
    }

    writeln!(
        out,
        "#[doc(hidden)]\n\
         struct __JetBootstrapNativeAdapterRoot {{\n\
             resources: ::jet_jit::SourceResources::SourceResourceLease,\n\
             bindings: ::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>,\n\
         }}\n\
         #[doc(hidden)]\n\
         #[derive(Clone)]\n\
         struct __JetBootstrapNativeAdapter {{\n\
             root: ::std::sync::Arc<__JetBootstrapNativeAdapterRoot>,\n\
         }}\n\
         impl __JetBootstrapNativeAdapter {{\n\
             fn new(resources: ::jet_jit::SourceResources::SourceResourceLease) -> Self {{\n\
                 Self::new_with_bindings(resources, ::std::sync::Arc::new(::jet_jit::SourceInterfaces::NativeInterfaceBindings::new()))\n\
             }}\n\
             fn new_with_bindings(resources: ::jet_jit::SourceResources::SourceResourceLease, bindings: ::std::sync::Arc<::jet_jit::SourceInterfaces::NativeInterfaceBindings>) -> Self {{\n\
                 Self {{ root: ::std::sync::Arc::new(__JetBootstrapNativeAdapterRoot {{ resources, bindings }}) }}\n\
             }}\n\
         }}\n\
         impl {trait_symbol} for __JetBootstrapNativeAdapter {{"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    for (metadata, arity) in methods {
        let receiver = match metadata.receiver_access {
            Some(jet_foundation::MIR::MirAccess::Read) => "&self",
            Some(jet_foundation::MIR::MirAccess::Write) => "&mut self",
            Some(jet_foundation::MIR::MirAccess::Move) => "self",
            None => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetEvalHostAdapter.{} has no checked receiver access",
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
            "new_session" | "clone_adapter" => {
                "Box::new(Self { root: self.root.clone() })".to_string()
            }
            "poll_callbacks" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_poll_callbacks(&self.root, __jet_arg_0) }".to_string(),
            "drain_callbacks" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_drain_callbacks(&self.root, __jet_arg_0) }".to_string(),
            "host_call" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_host_call(__jet_arg_0, __jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4, __jet_arg_5) }".to_string(),
            "foreign_call" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_foreign_call(__jet_arg_1, __jet_arg_2, __jet_arg_3) }".to_string(),
            "handle_call" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_handle_call(__jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4) }".to_string(),
            "task_group" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_task_group(&self.root, __jet_arg_0, __jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4, __jet_arg_5) }".to_string(),
            "native_call" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_native_call(&self.root.bindings, __jet_arg_0, __jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4) }".to_string(),
            "channel_select" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_channel_select(&self.root, __jet_arg_0, __jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4, __jet_arg_5) }".to_string(),
            "shared_call" => "{ let _jet_resource_scope = self.root.resources.activate(); __jet_bootstrap_native_shared_call(&self.root.resources, __jet_arg_0, __jet_arg_1, __jet_arg_2, __jet_arg_3, __jet_arg_4, __jet_arg_5, __jet_arg_6, __jet_arg_7, __jet_arg_8, __jet_arg_9) }".to_string(),
            _ => unreachable!("adapter method list and implementation map are synchronized"),
        };
        let return_type = &metadata.return_type;
        writeln!(
            out,
            "    fn {}({receiver}{separator}{parameters}) -> {return_type} {{ {result} }}",
            metadata.symbol,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
        let _ = arity;
    }
    writeln!(out, "}}")
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}
