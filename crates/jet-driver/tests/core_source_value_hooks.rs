use jet_driver::{Codegen, Diagnostics, Loader, Sema};
use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactKind, MirArtifactRequest, MirArtifactTarget,
};

#[test]
fn checked_core_value_hooks_render_and_fork_in_native_rust() {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-core-clock-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
use core.files as files
use core.time.[Clock]
use core.watcher.[WatchEvent]
#Debug
struct ClockRow { clock: Clock }
fn run() {
    clock :: Clock.new(5)
    fork := clock.copy()
    &fork.tick(7)
    assert(clock.now() == 5)
    assert(fork.now() == 12)
    assert("{clock}" == "Clock {{ now: 5 }}")
    assert("{clock:Debug}" == "Clock {{ now: 5 }}")
    row :: ClockRow{clock: clock.copy()}
    assert("{row:Debug}".contains("Clock {{ now: 5 }}"))
    copied_row := row.copy()
    &copied_row.clock.tick(2)
    assert(row.clock.now() == 5)
    assert(copied_row.clock.now() == 7)
    observed :: WatchEvent{domain: .File, kind: .Created, path: "watched", detail: "ok", pid: 7, port: 9}
    assert(!"{observed}".contains("pid:"))
    assert("{observed:Debug}".contains("pid: 7"))
    assert("{observed:Debug}".contains("\"watched\""))
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap())
            .expect("canonical Core sources and their checked private hooks must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "Core hook regression must pass the real checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("checked Core value hooks must survive lowering");
        mir.validate().expect("Core hooks have valid checked MIR");
        let clock = mir.types.iter().find(|definition| {
            definition.key.ends_with("::Core/time/time.jet::Clock")
        }).expect("the regression must use the source-owned, Shared-backed Clock");
        let clock_type = mir.type_instances.iter().find(|ty| {
            ty.identity == Some(clock.id)
        }).expect("Clock has a canonical checked type instance");
        assert!(jet_foundation::MIR::mir_source_value_hook(&mir, clock_type, "debug").is_some(),
            "checked Clock debug hook: type={clock_type:?}; hooks={:?}",
            mir.functions.iter().filter(|function| {
                function.name.contains("__show") || function.name.contains("__fork")
            }).map(|function| (&function.name, &function.form)).collect::<Vec<_>>());
        assert!(jet_foundation::MIR::mir_source_value_hook(&mir, clock_type, "clone").is_some());
        let row = mir.types.iter().find(|definition| definition.name == "ClockRow")
            .expect("the enclosing source row must be emitted");
        let jet_foundation::MIR::MirTypeDefKind::Struct { fields, .. } = &row.kind else {
            panic!("ClockRow must retain its checked struct shape");
        };
        assert_eq!(fields[0].ty.nominal_name(), Some(clock.key.as_str()),
            "a selective imported field must keep its checked Core owner: {:?}; alias={:?}; imports={:?}",
            fields[0].ty, bundle.name_ledger.effective_alias(bundle.entry, "Clock"),
            bundle.modules[bundle.entry].imports);
        let expiring = mir.types.iter().find(|definition| {
            definition.key.ends_with("::Core/time/time.jet::ExpiringValue")
        }).expect("imported Core retains the generic private Show provider");
        assert_eq!(expiring.generic_params.len(), 1);
        let expiring_type = mir.type_instances.iter().find(|ty| {
            ty.identity == Some(expiring.id)
        }).expect("ExpiringValue has a checked generic type instance");
        assert!(jet_foundation::MIR::mir_source_value_hook(&mir, expiring_type, "show").is_some());
        assert!(jet_foundation::MIR::mir_source_value_hook(&mir, expiring_type, "debug").is_none(),
            "a Show-only private provider must not invent a Debug hook");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("clock_hooks.rs");
        let executable = directory.join(format!("clock_hooks{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=clock_hooks", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("Core source hook proof requires rustc");
        assert!(compiled.status.success(),
            "checked hooks must emit one native Clone/Debug implementation, including enclosing rows:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable)
            .output().expect("Core source hook proof must run");
        assert!(executed.status.success(),
            "Clock Show/Debug must use its source hook and semantic copies must fork the timeline:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        std::fs::remove_dir_all(directory).unwrap();
    });
}
