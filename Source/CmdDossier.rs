//! D-CONF-MODULE1=A: generic-module member explanation over semantic facts.

use std::process::exit;

use jet::ExitCodes;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

use crate::absolutize;
/// D-CONF-MODULE1=A: explain a generic-module member's specialization input
/// from the semantic index, including the profile/declaration chain for a
/// build-fact value.
pub(crate) fn run_module_explain(subject: &str, file: &str, profile: &str, json: bool) {
    let abs = absolutize(file);
    let module_name = subject.split('.').next().unwrap_or(subject);
    let checked = crate::CmdInspect::check_projection_with_options(
        &abs,
        jet::Policy::GateSet::default(),
        profile,
        &std::collections::BTreeMap::new(),
    )
    .unwrap_or_else(|diagnostics| {
        crate::CmdInspect::render_check_failure(&abs, &diagnostics, json, false);
    });
    let index = checked.index();
    let Some(instance) = index.instances().iter().find(|instance| {
        instance.name == module_name
            || instance
                .applications
                .iter()
                .any(|application| application.name == module_name)
    }) else {
        crate::cli_error!(@fix "E2104", format!("generic module `{module_name}` is not present in `{file}`"), "pass the instantiated member as `module.member` and the source entry file");
        exit(ExitCodes::USER_ERROR);
    };
    if json {
        let arguments = StatusValue::array(
            instance
                .argument_values
                .iter()
                .zip(&instance.argument_provenance)
                .map(|(value, sources)| {
                    StatusValue::object(
                        StatusFields::new()
                            .with("value", value.as_str())
                            .with(
                                "provenance",
                                StatusValue::array(
                                    sources
                                        .iter()
                                        .map(|source| StatusValue::from(source.as_str())),
                                ),
                            ),
                    )
                }),
        );
        let generic_module = StatusValue::object(
            StatusFields::new()
                .with("subject", subject)
                .with("module", instance.name.as_str())
                .with("fingerprint", instance.fingerprint.as_str())
                .with("arguments", arguments)
                .with("check", crate::CmdInspect::check_result_value(&checked.check)),
        );
        println!(
            "{}",
            StatusEnvelope::new("inspect.generic_module", true)
                .with_field("generic_module", generic_module)
                .json()
        );
        return;
    }
    println!("generic module `{subject}`");
    println!("  instance: {}", instance.name);
    println!("  fingerprint: {}", instance.fingerprint);
    for (value, sources) in instance
        .argument_values
        .iter()
        .zip(&instance.argument_provenance)
    {
        println!("  argument: {value}");
        for source in sources {
            println!("    from: {source}");
        }
    }
    print!("{}", crate::CmdInspect::check_result_text(&checked.check));
}
