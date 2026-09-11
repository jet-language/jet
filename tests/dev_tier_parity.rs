//! Curated per-topic interpreter/JIT/AOT tier batteries (#2020).
//!
//! parity: guard tests/dev_tier_parity.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot
#![allow(dead_code, unused_imports)]

mod common;
include!("dev_parts/support.rs");
include!("dev_parts/tier_parity.rs");
#[test]
fn target_selected_prelude_parity() {
    // The ordinary hosted target must keep one meaning through the three
    // applicable execution tiers. The helper compares interpreter, resident
    // JIT, and compiled AOT stdout, stderr, and exit status.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hosted = root.join("tests/target_witness/hosted_default.jet");
    assert_cranelift_three_way(
        hosted.to_string_lossy().as_ref(),
        "target_selected_prelude_parity_hosted",
    );

    use jet::TargetMachine::{
        ExecutionTier, TargetMachine, TargetMachineError,
    };

    // A no-OS target has no resident host to adapt to: Dev and JIT are
    // explicitly inapplicable, while AOT remains the selected execution tier.
    let no_os = TargetMachine::wasm_no_os();
    assert!(no_os.supports_execution_tier(ExecutionTier::Aot).is_ok());
    for tier in [ExecutionTier::Dev, ExecutionTier::Jit] {
        assert!(matches!(
            no_os.supports_execution_tier(tier),
            Err(TargetMachineError::ExecutionTierUnsupported { .. })
        ));
    }

    // The same heap-free source is admitted by each typed target. The AOT
    // emitter and the web adapter carry the selected provider and Prelude
    // closure from the checked dossier; neither adapter invents a profile
    // from its runtime environment.
    let source = root.join("tests/target_witness/heap_free_core.jet");
    let source = source.to_string_lossy().into_owned();
    for machine in [
        no_os,
        TargetMachine::wasm_wasi(),
        TargetMachine::wasm_browser(),
    ] {
        let provider = machine.provider_identity();
        let layer = machine.max_runtime_layer().as_str();
        let environment = machine.environment_identity();
        let output = jet::Driver::compile_bundle_path_with_target_machine(
            &source,
            jet::Sema::CompileMode::Run,
            &machine,
        )
        .unwrap_or_else(|error| {
            panic!(
                "target-selected heap-free witness should compile for {}: {error:?}",
                machine.name
            )
        });

        if machine.is_browser_target() {
            let web = output
                .web
                .as_ref()
                .expect("browser target must emit web artifacts");
            assert!(
                web.manifest_json
                    .contains(&format!("\"providerIdentity\": \"{provider}\"")),
                "browser manifest lost provider identity: {}",
                web.manifest_json
            );
            assert!(
                web.manifest_json
                    .contains(&format!("\"targetEnvironment\": \"{environment}\"")),
                "browser manifest lost target environment: {}",
                web.manifest_json
            );
            assert!(
                web.manifest_json
                    .contains("\"preludeClosureIdentity\": \"prelude-closure-v1:"),
                "browser manifest lost Prelude closure identity: {}",
                web.manifest_json
            );
        } else {
            let marker =
                format!("// jet:target-dossier layer={layer} provider={provider}");
            assert!(
                output.rust.contains(&marker),
                "{} artifact lost target dossier marker {marker:?}",
                machine.name
            );
            assert!(
                output.rust.contains("closure=prelude-closure-v1:"),
                "{} artifact lost Prelude closure identity",
                machine.name
            );
        }
    }
}
