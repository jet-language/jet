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
        assert!(!rust.contains("jet_std::DataTree"),
            "retained native helpers and Encode producers must use the canonical Dyn carrier");
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

#[test]
fn checked_core_fraction_display_reaches_implicit_trait_target() {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-core-fraction-display-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
use core.math.[gcd]
fn format_bounds(span: Int) -> String Never! { "{0 - span / 2} to {span / 2 - 1}" }
fn timestamp_text(ticks: Int) -> String Never! { "{ticks / 1000000000}" }
fn run() Never! {
    assert_eq(gcd(2, 4), 2)
    quotient :: 1 / 3
    assert_eq("{quotient}", "1/3")
    eighth :: 1 / 8
    assert_eq("{eighth}", "0.125")
    assert_eq(format_bounds(8), "-4 to 3")
    assert_eq(timestamp_text(2000000000), "2")
    assert_eq(timestamp_text(125000000), "0.125")
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap())
            .expect("source-owned Fraction must load with its checked Display implementation");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "Fraction formatting must pass the real checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("implicit Fraction Display must name a checked MIR target");
        mir.validate().expect("Fraction formatting MIR is valid");
        assert!(mir.functions.iter().any(|function| {
            matches!(&function.form,
                jet_foundation::MIR::MirFunctionForm::TraitMethod { owner, trait_ref, .. }
                    if trait_ref.name == "Display"
                        && owner.nominal_name().is_some_and(|name| {
                            name.ends_with("::Core/math/math.jet::Fraction")
                        }))
        }), "Core Fraction Display must remain materialized");
        for name in ["__fraction_sub", "__fraction_from_parts"] {
            assert!(mir.functions.iter().any(|function| function.name.ends_with(name)),
                "synthesized precise operation must retain its exact source provider: {name}");
        }
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("fraction_display.rs");
        let executable = directory.join(format!("fraction_display{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=fraction_display", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("Fraction Display proof requires rustc");
        assert!(compiled.status.success(),
            "source-owned Fraction formatting must compile:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable)
            .output().expect("Fraction Display proof must execute");
        assert!(executed.status.success(),
            "fraction and compiler-range formatting must preserve exact text:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn checked_never_comparators_sort_named_and_literal_callbacks() {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-never-comparator-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
struct SortRow { key: Int }
fn row_order(left: SortRow, right: SortRow) -> Ordering Never! { left.key <=> right.key }
fn run() Never! {
    rows := [SortRow]{SortRow{key: 3}, SortRow{key: 1}, SortRow{key: 2}}
    &rows.sort_by(row_order)
    assert_eq(rows[0].key, 1)
    assert_eq(rows[2].key, 3)
    &rows.sort_by((left: SortRow, right: SortRow) -> right.key <=> left.key)
    assert_eq(rows[0].key, 3)
    assert_eq(rows[2].key, 1)
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap())
            .expect("comparator fixture must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "Never comparators must pass the real checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("Never is not a fallible comparator operation");
        mir.validate().expect("comparator MIR must remain valid");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("never_comparator.rs");
        let executable = directory.join(format!("never_comparator{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=never_comparator", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("comparator proof requires rustc");
        assert!(compiled.status.success(), "Never comparators must emit native code:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable)
            .output().expect("comparator proof must execute");
        assert!(executed.status.success(), "both checked comparator forms must sort correctly:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn loop_payload_views_end_before_owner_reassignment() {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-loop-payload-view-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
struct Handle { name: String }
fn lookup(rows: [Handle], name: String) -> Bool {
    loop row in rows { if row.name == name -> return true }
    false
}
fn route_sum() -> Int {
    route := Int?{None}
    total := 0
    loop row in ([Int]{1, 2, 3}) {
        route = Val(row)
        loop operand in ([Int]{10, 20}) {
            value :: if route == .Val(position) -> position + position + operand else -> 0
            total = total + value
        }
        route = None
    }
    total
}
fn run() Never! {
    assert_eq(route_sum(), 114)
    rows :: [Handle]{Handle{name: "Float"}}
    loop name in ([String]{"U8", "U64", "F32"}) -> assert(!lookup(rows, name))
    assert(!lookup(rows, "Ordinary") && lookup(rows, "Float"))
    assert_eq(rows.len(), 1)
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap())
            .expect("nested payload views and read metadata must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "payload-view source must pass the checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("nested payload views must lower");
        mir.validate().expect("payload-view MIR is valid");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("loop_payload_view.rs");
        let executable = directory.join(format!("loop_payload_view{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=loop_payload_view", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("payload-view regression requires rustc");
        assert!(compiled.status.success(), "payload references must not outlive their uses:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable)
            .output().expect("payload-view regression must execute");
        assert!(executed.status.success(), "rebound payloads and metadata reads must retain their values:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn indexed_loop_handle_receivers_keep_exact_accessors() {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-indexed-reader-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("input.txt"), "AB").unwrap();
        std::fs::write(directory.join("package.jet"),
            "name: \"indexed_reader_proof\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read] } }\n").unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
use core.files.[FileReader, open]
struct Row { reader: FileReader }
fn run() -[FS]> {
    groups := [[Row]]{[Row]{Row{reader: open("input.txt") ?? panic("open")}}}
    loop group in groups {
        loop row in group {
            first :: row.reader.read(1) ?? panic("read")
            second :: row.reader.read(1) ?? panic("read")
            assert_eq(first[0], U8{65})
            assert_eq(second[0], U8{66})
            assert_eq((row.reader.read(1) ?? panic("read")).len(), 0)
        }
    }
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap()).expect("indexed reader source must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "indexed handle reads must pass the checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("indexed handle read windows must keep legal accessor metadata");
        mir.validate().expect("indexed handle MIR is valid");
        assert!(mir.functions.iter().flat_map(|function| &function.places).any(|place| {
            place.access == jet_foundation::MIR::MirAccess::Write
                && place.ty.nominal_name().is_some_and(|name| name.ends_with("::FileReader"))
                && place.projections.iter().filter(|projection| {
                    matches!(projection, jet_foundation::MIR::MirProjection::Index { write_call: Some(_), .. })
                }).count() >= 2
        }), "nested reader receiver must retain both exact index setters");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("indexed_reader.rs");
        let executable = directory.join(format!("indexed_reader{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=indexed_reader", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("indexed reader regression requires rustc");
        assert!(compiled.status.success(), "indexed handle receivers must compile:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable).current_dir(&directory)
            .output().expect("indexed reader regression must execute");
        assert!(executed.status.success(), "all reads must advance the same retained handle:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        std::fs::remove_dir_all(directory).unwrap();
    });
}

/// Check, lower, emit, rustc-compile and run one regression program through
/// the hosted Rust seed path (the r16 collect-all emission sweep).
fn hosted_seed_program_runs(label: &str, package: Option<&str>, program: &str) {
    hosted_seed_program_runs_as(label, package, &[], program, false);
}

/// The same regression with sibling module files (`name.jet` loaded by the
/// entry's `module name`), so an item crosses a module boundary.
fn hosted_seed_modules_run(label: &str, modules: &[(&str, &str)], program: &str) {
    hosted_seed_program_runs_as(label, None, modules, program, false);
}

/// The same regression, then also built the way the bootstrap builds it: the
/// runtime/Core block as its own `jet_runtime` rlib and the program as a
/// dependent crate (`jet_store::runtime::split_runtime_crate`), so an item
/// emitted in the wrong crate (an orphan impl, a crate-private helper) fails.
fn hosted_seed_program_runs_split(label: &str, package: Option<&str>, program: &str) {
    hosted_seed_program_runs_as(label, package, &[], program, true);
}

fn hosted_seed_program_runs_as(
    label: &str,
    package: Option<&str>,
    modules: &[(&str, &str)],
    program: &str,
    split: bool,
) {
    jet_driver::run_compiler_work(|| {
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        if let Some(package) = package {
            std::fs::write(directory.join("package.jet"), package).unwrap();
        }
        for (name, source) in modules {
            std::fs::write(directory.join(format!("{name}.jet")), source).unwrap();
        }
        let entry = directory.join("run.jet");
        std::fs::write(&entry, program).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap()).expect("regression source must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        let errors: Vec<_> = diagnostics.iter()
            .filter(|diagnostic| diagnostic.severity == Diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{label} must pass the checker: {errors:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(&bundle, &nominals, request)
            .expect("regression program must lower to checked MIR");
        mir.validate().expect("regression MIR is valid");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let run = |executable: &std::path::Path, mode: &str| {
            let executed = std::process::Command::new(executable).current_dir(&directory)
                .output().expect("regression must execute");
            assert!(executed.status.success(), "{label} ({mode}) must run:\n{}",
                String::from_utf8_lossy(&executed.stderr));
        };
        let source = directory.join(format!("{label}.rs"));
        let executable = directory.join(format!("{label}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, &rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "-Awarnings"])
            .arg(format!("--crate-name={label}"))
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("regression requires rustc");
        assert!(compiled.status.success(), "{label} must compile:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        run(&executable, "single file");
        if split {
            let split = jet_store::runtime::split_runtime_crate(&rust)
                .expect("generated runtime markers are valid")
                .expect("a native program has a runtime block");
            let runtime_source = directory.join("jet_runtime.rs");
            let runtime_rlib = directory.join("libjet_runtime.rlib");
            std::fs::write(&runtime_source, &split.runtime_lib).unwrap();
            let runtime = std::process::Command::new("rustc")
                .args(["--edition=2021", "-Awarnings", "--crate-type=rlib", "--crate-name=jet_runtime"])
                .arg(&runtime_source).arg("-o").arg(&runtime_rlib)
                .output().expect("regression requires rustc");
            assert!(runtime.status.success(), "{label} split runtime crate must compile:\n{}",
                String::from_utf8_lossy(&runtime.stderr));
            let program_source = directory.join(format!("{label}_program.rs"));
            let program_executable =
                directory.join(format!("{label}_program{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&program_source, &split.program).unwrap();
            let program = std::process::Command::new("rustc")
                .args(["--edition=2021", "-Awarnings"])
                .arg(format!("--crate-name={label}_program"))
                .arg("--extern").arg(format!("jet_runtime={}", runtime_rlib.display()))
                .arg(&program_source).arg("-o").arg(&program_executable)
                .output().expect("regression requires rustc");
            assert!(program.status.success(), "{label} split program crate must compile:\n{}",
                String::from_utf8_lossy(&program.stderr));
            run(&program_executable, "split runtime/program");
        }
        std::fs::remove_dir_all(directory).unwrap();
    });
}

/// r16 root 1: an audited `#Unsafe` value tail is the callable's value, and a
/// covered else-less nested-pattern table never falls off the function end.
#[test]
fn value_tails_inside_unsafe_and_covered_nested_tables_return_values() {
    hosted_seed_program_runs("value_tails", None, r#"
enum Prepared {
    Checked(Int)
    Replay(String)
}
fn unsafe_tail(value: Int) -> Int IOError! {
    #Unsafe("regression: the audited region's last line is the callable value") {
        Ok(value + 1)
    }
}
fn prepared(owned: Prepared?, allowed: Bool) -> Int? {
    if owned == {
        .Val(.Checked(value)) -> return Val(value)
        .Val(.Replay(_)) -> {
            if !allowed -> return None
            return Val(-1)
        }
        .None -> {
            return None
        }
    }
}
fn run() {
    assert_eq(unsafe_tail(41) ?? panic("tail"), 42)
    assert_eq(prepared(Val(Prepared.Checked(7)), false), Val(7))
    assert_eq(prepared(Val(Prepared.Replay("r")), true), Val(-1))
    assert_eq(prepared(Val(Prepared.Replay("r")), false), None)
    assert_eq(prepared(None, true), None)
}
"#);
}

/// r16 root 3: a checked scalar head converts by its source/target pair:
/// F64 -> F32 narrows, and fixed-width -> fixed-width keeps the carrier cast.
#[test]
fn checked_scalar_heads_select_the_source_target_conversion() {
    hosted_seed_program_runs("scalar_heads", None, r#"
fn shown(value: Float, is_f32: Bool) -> String? {
    if is_f32 -> Val("{F32{value}}") else -> Val("{value}")
}
fn byte(word: Int) -> U8 { U8{U64{word}.bit_and(U64{255})} }
fn run() {
    assert_eq(shown(1.5, true), Val("1.5"))
    assert_eq(shown(0.25, false), Val("0.25"))
    assert_eq(Int{byte(513)}, 1)
}
"#);
}

/// r16 root 4: `EnvError` is a cataloged Core enum leaf, so its checked MIR
/// owner row exists wherever `core.sys` names it (`entry_names`).
#[test]
fn env_error_has_a_checked_core_enum_owner_row() {
    hosted_seed_program_runs(
        "env_error_owner",
        Some("name: \"env_error_owner\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Env] } }\n"),
        r#"
use core.sys as sys
fn run() -[Env]> {
    names :: sys.vars() ?? panic("environment snapshot")
    assert(names.len() >= 0)
}
"#,
    );
}

/// A record holding a resource handle cannot be copied. `loop x in ^xs`
/// takes the list and hands each element out by value, so the body moves it
/// into another list without a Rust `.clone()` the record lacks; a read-only
/// loop before the take still walks the list in place.
#[test]
fn taken_lists_move_noncloneable_records() {
    hosted_seed_program_runs(
        "noncloneable_takes",
        Some("name: \"noncloneable_takes\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read, FS.Write] } }\n"),
        r#"
use core.files.[FileReader, open, create]
struct Held {
    name: String
    handle: FileReader
}
fn opened(name: String) -[FS]> Held {
    writer :: create(name) ?? panic("create")
    writer.write_all([U8]{65}) ?? panic("write")
    writer.fsync() ?? panic("fsync")
    Held{name: name, handle: open(name) ?? panic("open")}
}
fn short_ones(listed: ^[Held]) -> [Held] {
    names := [String]{}
    loop held in listed -> &names.push(held.name)
    assert_eq(names.len(), 4)
    kept := [Held]{}
    loop held in ^listed {
        if held.name.len() < 6 -> &kept.push(^held)
    }
    kept
}
fn run() -[FS]> {
    held := [Held]{}
    &held.push(opened("a.txt"))
    &held.push(opened("bb.txt"))
    &held.push(opened("c.txt"))
    &held.push(opened("long-name.txt"))
    kept := short_ones(^held)
    assert_eq(kept.len(), 2)
}
"#,
    );
}

/// The original Prelude `Err` keeps its checked identity
/// (`jet:canonical-prelude::…::Err`) and is carried by the one native
/// `JetErr`: a typed `IOError` converted into the default error domain flows
/// through `jet_err_apply_conversion` without a second Rust error type.
#[test]
fn default_err_identity_is_the_native_err_carrier() {
    hosted_seed_program_runs(
        "default_err_identity",
        Some("name: \"default_err_identity\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read] } }\n"),
        r#"
use core.files as files
fn length(path: String) -[FS]> Int {
    reader :: files.open(path)
    reader.path.len()
}
fn run() -[FS]> {
    if length("missing-default-err-input.txt") == {
        .Ok(_) -> panic("the input must be missing")
        .Err(_) -> {}
    }
}
"#,
    );
}

/// The default `Err`'s declared fields keep their declared types under the
/// original Prelude identity: `message` is a `String` for a method call, a
/// comparison and interpolation. Its derived `Equatable` is the runtime's one
/// `JetErr` impl, so the program also builds as a split runtime/program pair
/// (r17g2 E0117: an orphan program impl).
#[test]
fn default_err_fields_keep_their_declared_types() {
    hosted_seed_program_runs_split(
        "default_err_fields",
        Some("name: \"default_err_fields\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read] } }\n"),
        r#"
use core.files as files
fn length(path: String) -[FS]> Int {
    reader :: files.open(path)
    reader.path.len()
}
fn run() -[FS]> {
    if length("missing-default-err-fields.txt") == {
        .Ok(_) -> panic("the input must be missing")
        .Err(error) -> {
            assert(error.message.len() > 0)
            assert(error.message != "")
            shown :: "failed: {error.message}"
            assert(shown.len() > error.message.len())
            assert(error == error)
        }
    }
}
"#,
    );
}

/// A record holding a Core file owner moves out at its last use: the wrap
/// and the push take the binding and clear its leaf cleanup flags, rather
/// than cloning a record whose handle has no Rust `Clone`.
#[test]
fn file_owner_records_move_at_their_last_use() {
    hosted_seed_program_runs(
        "file_owner_last_use",
        Some("name: \"file_owner_last_use\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read] } }\n"),
        r#"
use core.files.[FileReader, open]
struct Checked { path: String, reader: FileReader }
fn checked(path: String) -[FS]> Checked IOError! {
    reader :: open(path) ?? return Err(err)
    file :: Checked{path: path, reader: reader}
    Ok(file)
}
fn gather(paths: [String]) -[FS]> [Checked] IOError! {
    found := [Checked]{}
    loop path in paths {
        file :: checked(path) ?? return Err(err)
        &found.push(file)
    }
    Ok(found)
}
fn run() -[FS]> {
    files :: gather([String]{"run.jet"}) ?? panic("open")
    first :: files[0].reader.read(1) ?? panic("read")
    assert_eq(first[0], U8{10})
    assert_eq(files[0].path, "run.jet")
}
"#,
    );
}

/// r17g5: `shared ParentAttachment?{None}` whose payload record lives in
/// another module keeps its `Shared<ParentAttachment?>` payload type, not the
/// bare `None` placeholder `Int?` (CoreLibrary's `CoreSharedParentAttachment`).
/// The `None` reaches `Shared.new` from the worklist cache, so the argument
/// takes its parameter's option type after the cache, not only on a fresh lower.
#[test]
fn shared_typed_none_of_an_imported_record_keeps_its_payload_type() {
    hosted_seed_modules_run(
        "shared_imported_none",
        &[("backend", "struct ParentAttachment { manifest_digest: String }\n")],
        r#"
module backend
use backend.[ParentAttachment]
struct Host {
    rows: [String]
    attachment: Shared<ParentAttachment?>
}
fn host_new(rows: ^[String]) -> Host Never! {
    original := ^rows
    Host{
        rows: ^original,
        attachment: shared ParentAttachment?{None},
    }
}
fn attach(host: &Host, digest: String) Never! {
    guard := host.attachment.guard_edit()
    guard.value = Val(ParentAttachment{manifest_digest: digest})
}
fn digest(host: Host) -> String Never! {
    guard :: host.attachment.guard_read()
    attachment :: guard.value ?? return "none"
    attachment.manifest_digest
}
fn run() {
    host := host_new([String]{"a"})
    assert_eq(digest(host), "none")
    attach(&host, "m")
    assert_eq(digest(host), "m")
}
"#,
    );
}

/// Core's secret-bearing Close impls wipe a buffer taken from the consumed
/// receiver. Rebinding `^self` made a second owner whose scope-end Close
/// re-entered the same impl forever, so a natively closed secret never
/// returned.
#[test]
fn core_secret_closes_wipe_once_and_return() {
    hosted_seed_program_runs(
        "secret_closes_return",
        Some("name: \"secret_closes_return\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [Rand] } }\n"),
        r#"
use core.crypto as crypto
fn run() -[Rand]> {
    if true {
        secret :: crypto.Secret.from_text("credential")
    }
    if true {
        signing :: crypto.SigningKey.new_random() ?? panic("signing key")
        exchange :: crypto.X25519SecretKey.new_random() ?? panic("exchange key")
    }
}
"#,
    );
}

/// Core's Directory and AdvisoryLock owners close by reading the consumed
/// receiver in place. Rebinding `^self` re-entered the same Close until the
/// stack overflowed; reacquiring the advisory lock proves the first owner's
/// descriptor was released, and returning proves no Close re-entered itself.
#[test]
fn core_directory_and_lock_owners_close_once_and_return() {
    hosted_seed_program_runs(
        "directory_lock_closes",
        Some("name: \"directory_lock_closes\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS, Env] } }\n"),
        r#"
use core.files as files
fn run() -[FS, Env]> {
    files.create_dir("held") ?? panic("create held")
    if true {
        directory :: files.open_directory("held") ?? panic("open held")
        advisory :: directory.lock() ?? panic("advisory lock")
    }
    if true {
        directory :: files.open_directory("held") ?? panic("reopen held")
        advisory :: directory.lock() ?? panic("advisory lock after close")
    }
}
"#,
    );
}

/// A held FileLock's descriptor has exactly one owner: `lock_owner` consumes
/// the private `__LockFd` guard into the native FileLock, so the guard does not
/// close the descriptor the FileLock owns. Closing it twice aborted natively
/// ("owned file descriptor already closed"); reacquiring proves the release.
#[test]
fn core_file_lock_descriptor_closes_once() {
    hosted_seed_program_runs(
        "file_lock_closes_once",
        Some("name: \"file_lock_closes_once\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS, Env] } }\n"),
        r#"
use core.files as files
fn run() -[FS, Env]> {
    if true {
        held :: files.lock("lockfile") ?? panic("file lock")
    }
    if files.try_lock("lockfile") == {
        .Val(_) -> {}
        else -> panic("file lock still held after close")
    }
}
"#,
    );
}

/// A trait-object option payload binds by reference while its owner is only
/// changed after the binding's last read: the binding's own scope-end
/// cleanup reads no referent, so a later write of a local root (`sink`) or a
/// later write-borrow of a `&` parameter on another path (`machine`) leaves
/// the binding a borrow instead of cloning a `Box<dyn …>`.
#[test]
fn trait_object_payload_bindings_borrow_past_later_owner_writes() {
    hosted_seed_program_runs(
        "trait_payload_binding",
        None,
        r#"
trait Sink { fn note(self) -> Int }
struct Counter { base: Int }
impl Counter.Sink { fn note(self) -> Int { self.base + 1 } }
struct Session { sink: Sink?, level: Int }
struct Machine { session: Session }
fn sink_level(config: ^Session) -> Int {
    session := config
    total := 0
    if session.sink == {
        None -> {}
        Val(sink) -> {
            total = sink.note()
            session.sink = None
        }
    }
    if session.sink == None -> total = total + session.level
    total
}
fn bump(machine: &Machine) { machine.session.level = machine.session.level + 1 }
fn machine_level(machine: &Machine) -> Int {
    if machine.session.sink == .Val(sink) -> return sink.note()
    bump(&machine)
    machine.session.level
}
fn run() {
    assert_eq(sink_level(Session{sink: Val(Counter{base: 41}), level: 1}), 43)
    held := Machine{session: Session{sink: Val(Counter{base: 6}), level: 0}}
    assert_eq(machine_level(&held), 7)
    bare := Machine{session: Session{sink: None, level: 4}}
    assert_eq(machine_level(&bare), 5)
}
"#,
    );
}
/// `core.encoding`'s `EncodingError` is a source-owned Core record: an
/// `alias.EncodingError{…}` literal builds the source record its signature
/// names, not the native `jet_std::EncodingError` carrier.
#[test]
fn source_owned_encoding_error_literal_keeps_its_identity() {
    hosted_seed_program_runs_split(
        "source_owned_encoding_error",
        None,
        r#"
use core.encoding as encoding
fn unsupported(reason: String) -> encoding.EncodingError Never! {
    encoding.EncodingError{
        format: encoding.EncodingFormat.JSON,
        kind: encoding.EncodingErrorKind.Unsupported,
        byte_offset: 0,
        line: None,
        column: None,
        path: "$",
        reason: reason,
        cause: None,
    }
}
fn run() {
    error :: unsupported("probe")
    assert(error.byte_offset == 0)
    assert(error.kind == encoding.EncodingErrorKind.Unsupported)
}
"#,
    );
}

/// `core.term`'s source-owned `Stdout`/`Stderr` dispatch to their own Jet
/// methods, not the native stdio handle table: writes take a write receiver
/// (`h := stdout(); &h.write(…)`), `is_tty` is a read query a temporary can
/// answer, and `print`/`eprint` reach the same source handles.
#[test]
fn source_owned_stdio_handles_dispatch_to_their_methods() {
    hosted_seed_program_runs_split(
        "source_owned_stdio",
        None,
        r#"
use core.term as term
fn run() -[IO, Time.Wait]> {
    out := term.stdout()
    &out.write_line("stdout line").drop("stdout")
    &out.write_bytes([U8]{104, 105, 10}).drop("stdout bytes")
    &out.flush().drop("stdout flush")
    err := term.stderr()
    &err.write("stderr line\n").drop("stderr")
    &err.flush() ?? panic("stderr flush")
    tty :: term.stdout().is_tty() || term.stderr().is_tty()
    print("printed line {tty || !tty}")
    term.eprint("eprinted line")
}
"#,
    );
}


/// A record whose fields were taken and refilled (`^r.f` then `r.f = …`)
/// still moves whole at its last use, rather than cloning a record that
/// holds a `Box<dyn …>`.
#[test]
fn refilled_records_move_whole_at_their_last_use() {
    hosted_seed_program_runs(
        "refilled_record_move",
        None,
        r#"
trait Sink { fn note(self) -> Int }
struct Counter { base: Int }
impl Counter.Sink { fn note(self) -> Int { self.base + 1 } }
struct Report { text: String }
struct Compiled { sink: Sink?, report: Report?, gaps: [String] }
struct Query { compile: Compiled, report: Report?, gaps: [String] }
fn compiled() -> Compiled {
    Compiled{sink: Val(Counter{base: 1}), report: Val(Report{text: "r"}), gaps: [String]{"g"}}
}
fn query() -> Query {
    compile := compiled()
    report :: ^compile.report
    compile.report = None
    gaps :: ^compile.gaps
    compile.gaps = [String]{}
    Query{compile: ^compile, report: ^report, gaps: ^gaps}
}
fn run() {
    result :: query()
    assert_eq(result.gaps.len(), 1)
    assert(result.compile.report == None)
    assert_eq(result.compile.gaps.len(), 0)
    if result.compile.sink == .Val(sink) -> assert_eq(sink.note(), 2) else -> panic("sink")
}
"#,
    );
}

/// A non-cloneable success payload binds by move: each match arm is entered
/// only on its own test, so a later arm's probe of the result is no later
/// use of the payload (straight-line and loop matches, `? found -> found`
/// in a loop, and an option payload pushed from a loop). The arm's binding
/// then moves whole at its last use.
#[test]
fn noncloneable_result_payloads_move_into_their_arm() {
    hosted_seed_program_runs(
        "noncloneable_payload_arms",
        Some("name: \"noncloneable_payload_arms\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read] } }\n"),
        r#"
use core.files.[FileReader, open]
struct Checked { path: String, reader: FileReader }
#Error
enum Missing { Gone(String) }
#Error
enum Wrapped { Failed(String) }
fn checked(path: String) -[FS]> Checked Missing! {
    reader :: open(path) ?? return Err(Missing.Gone(path))
    Ok(Checked{path: path, reader: reader})
}
fn probe(path: String) -[FS]> Checked? Missing! {
    if path == "absent" -> return Ok(None)
    file :: checked(path) ?? return Err(err)
    Ok(Val(file))
}
fn note(file: Checked) -> Int { file.path.len() }
fn listed(paths: [String]) -[FS]> [Checked] Missing! {
    found := [Checked]{}
    loop path in paths {
        if checked(path) == {
            .Ok(file) -> {
                note(file)
                &found.push(file)
            }
            .Err(error) -> return Err(error)
        }
    }
    Ok(found)
}
fn wrapped(paths: [String]) -[FS]> [Checked] Wrapped! {
    if listed(paths) == {
        .Ok(found) -> Ok(found)
        .Err(_) -> Err(Wrapped.Failed("missing"))
    }
}
fn first_path(path: String) -[FS]> String? Wrapped! {
    if checked(path) == {
        .Ok(file) -> {
            note(file)
            Ok(Val(file.path))
        }
        .Err(_) -> {
            if path == "" -> return Ok(None)
            Err(Wrapped.Failed("missing"))
        }
    }
}
fn present(paths: [String]) -[FS]> Bool Missing! {
    loop path in paths {
        probed :: probe(path) ? found -> found ! error -> return Err(error)
        if probed != None -> return Ok(true)
    }
    Ok(false)
}
fn kept(paths: [String]) -[FS]> [Checked] {
    out := [Checked]{}
    loop path in paths {
        maybe :: probe(path) ? found -> found ! error -> panic("probe")
        if maybe == {
            .Val(found) -> &out.push(found)
            .None -> {}
        }
    }
    out
}
fn run() -[FS]> {
    assert_eq((wrapped([String]{"run.jet"}) ?? panic("wrapped")).len(), 1)
    assert_eq(first_path("run.jet") ?? panic("first"), "run.jet")
    assert(present([String]{"absent", "run.jet"}) ?? panic("present"))
    assert_eq(kept([String]{"absent", "run.jet"}).len(), 1)
}
"#,
    );
}

/// `&xs.push(^owner.field)` moves the field into the list (the s49
/// `discover_function_entries_from_files` shape): the builtin argument keeps
/// its move mark, so a non-Clone field (a file handle inside) is moved out
/// of its matched owner rather than copied.
#[test]
fn builtin_push_of_a_moved_field_moves_it() {
    hosted_seed_program_runs(
        "builtin_push_moved_field",
        None,
        r#"
use core.files as files
struct Held {
    path: String
    handle: files.FileReader
}
struct Entry {
    path: String
    file: Held
}
fn entry(path: String) -[FS]> Entry IOError! {
    handle :: files.open(path) ?? return Err(err)
    Ok(Entry{path: path, file: Held{path: path, handle: handle}})
}
fn collect(seed: ^[Held], path: String) -[FS]> [Held] {
    held := ^seed
    found :: entry(path) ?? return held
    &held.push(^found.file)
    held
}
fn run() -[FS, IO]> {
    files.write("builtin_push_moved_field.txt", "x") ?? panic("write")
    held :: collect(^[Held]{}, "builtin_push_moved_field.txt")
    print("held {held.len()} {held[0].path}")
    files.remove("builtin_push_moved_field.txt") ?? panic("remove")
}
"#,
    );
}

/// The full s49 shape: the owner is a match arm's payload of a temporary
/// result (`if f() == { .Ok(m) -> &xs.push(^m.file) .Err(_) -> {} }`). The
/// arm binds the non-Clone payload by move (the fall-through re-test after
/// the arms only reads the discriminant) and pushes its moved field.
#[test]
fn builtin_push_of_a_matched_payload_field_moves_it() {
    hosted_seed_program_runs(
        "builtin_push_matched_field",
        None,
        r#"
use core.files as files
struct Held {
    path: String
    handle: files.FileReader
}
struct Entry {
    path: String
    file: Held
}
fn entry(path: String) -[FS]> Entry IOError! {
    handle :: files.open(path) ?? return Err(err)
    Ok(Entry{path: path, file: Held{path: path, handle: handle}})
}
fn collect(seed: ^[Held], path: String, wanted: Bool) -[FS]> [Held] {
    held := ^seed
    if wanted {
        if entry(path) == {
            .Ok(found) -> {
                &held.push(^found.file)
            }
            .Err(error) -> {
                if path == "" -> panic("{error}")
            }
        }
    }
    held
}
fn run() -[FS, IO]> {
    files.write("builtin_push_matched_field.txt", "x") ?? panic("write")
    held :: collect(^[Held]{}, "builtin_push_matched_field.txt", true)
    missing :: collect(^[Held]{}, "builtin_push_matched_field.absent", true)
    print("held {held.len()} {held[0].path} {missing.len()}")
    files.remove("builtin_push_matched_field.txt") ?? panic("remove")
}
"#,
    );
}

/// Root 2: Core handle ops write through a FileReader/FileWriter reached from
/// a read binding (a Read parameter, a field of a read `self`). The emitted
/// Rust lends the interior-mutable carrier by `&`.
#[test]
fn read_bound_handles_write_through_interior_mutable_carriers() {
    jet_driver::run_compiler_work(|| {
        use jet_foundation::MIR::{MirAccess, MirPlace, MirPlaceBase, MirProjection};
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let directory = std::env::var_os("JET_TEST_SCRATCH_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| deps.join("core-value-hook-proof"))
            .join(format!("jet-read-bound-handles-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("input.txt"), "AB").unwrap();
        std::fs::write(directory.join("package.jet"),
            "name: \"read_bound_handles_proof\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read, FS.Write] } }\n").unwrap();
        let entry = directory.join("run.jet");
        std::fs::write(&entry, r#"
use core.files.[FileReader, FileWriter, open, create]
struct Held {
    handle: FileReader
    fn next_byte(self) -[FS]> U8 {
        chunk :: self.handle.read(1) ?? panic("held read")
        chunk[0]
    }
}
fn read_byte(reader: FileReader) -[FS]> U8 {
    chunk :: reader.read(1) ?? panic("parameter read")
    chunk[0]
}
fn write_bytes(writer: FileWriter, bytes: [U8]) -[FS]> {
    writer.write_all(bytes) ?? panic("parameter write")
    writer.fsync() ?? panic("parameter fsync")
}
fn expect_ab(first: [U8], second: [U8]) {
    assert_eq(first, [U8]{65})
    assert_eq(second, [U8]{66})
}
fn run() -[FS]> {
    reader :: open("input.txt") ?? panic("open")
    first :: read_byte(reader)
    second :: read_byte(reader)
    expect_ab([U8]{first}, [U8]{second})
    held :: Held{handle: open("input.txt") ?? panic("open held")}
    held_first :: held.next_byte()
    held_second :: held.next_byte()
    expect_ab([U8]{held_first}, [U8]{held_second})
    writer :: create("output.txt") ?? panic("create")
    write_bytes(writer, [U8]{67, 68})
}
"#).unwrap();
        let mut bundle = Loader::load_entry(entry.to_str().unwrap()).expect("read-bound handle source must load");
        let (diagnostics, _, nominals) =
            Sema::check_bundle_with_effect_facts(&mut bundle, Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| {
            diagnostic.severity != Diagnostics::Severity::Error
        }), "handle ops through read bindings must pass the checker: {diagnostics:?}");
        let request = MirArtifactRequest::new(
            MirArtifactTarget::RustAot,
            MirArtifactKind::NativeExecutable,
            MirArtifactBuildMode::Dev,
        );
        let (mir, artifact) = Codegen::TIR::lower_checked_mir_program_for(
            &bundle, &nominals, request,
        ).expect("read-bound handle ops must lower");
        mir.validate().expect("read-bound handle MIR is valid");
        // Each shape is a Write place rooted at a binding the function holds
        // only for reading.
        let read_rooted_write = |handle: &str, shape: &dyn Fn(&MirPlace) -> bool| {
            mir.functions.iter().any(|function| {
                function.params.iter().all(|param| param.access != MirAccess::Write)
                    && function.capture_params.iter().all(|capture| capture.access != MirAccess::Write)
                    && function.places.iter().any(|place| {
                        place.access == MirAccess::Write
                            && place.ty.nominal_name().is_some_and(|name| name.ends_with(handle))
                            && shape(place)
                    })
            })
        };
        let parameter = |place: &MirPlace| {
            matches!(place.base, MirPlaceBase::Parameter(_)) && place.projections.is_empty()
        };
        assert!(read_rooted_write("::FileReader", &parameter), "a read FileReader parameter is written");
        assert!(read_rooted_write("::FileWriter", &parameter), "a read FileWriter parameter is written");
        assert!(read_rooted_write("::FileReader", &|place: &MirPlace| {
            matches!(place.base, MirPlaceBase::Parameter(_))
                && matches!(place.projections.as_slice(), [MirProjection::Field { .. }])
        }), "a FileReader field of a read self is written");
        let rust = Codegen::MIRRust::emit_mir_program(
            &mir,
            &Codegen::MIRRust::MirRustConfig {
                target: jet_foundation::Layout::TargetLayout::host(),
                target_kind: Codegen::MIRRust::MirRustTarget::Native,
                root_prefix: String::new(),
                execution: Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact),
            },
        );
        let source = directory.join("read_bound_handles.rs");
        let executable = directory.join(format!("read_bound_handles{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "--crate-name=read_bound_handles", "-Awarnings"])
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("read-bound handle regression requires rustc");
        assert!(compiled.status.success(), "read-bound handle ops must compile:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable).current_dir(&directory)
            .output().expect("read-bound handle regression must execute");
        assert!(executed.status.success(), "every read-bound op must advance its one handle:\n{}",
            String::from_utf8_lossy(&executed.stderr));
        assert_eq!(std::fs::read(directory.join("output.txt")).unwrap(), b"CD",
            "the read-bound writer must write and sync through the held handle");
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn single_file_list_formatter_has_one_kernel_definition() {
    hosted_seed_program_runs("list_formatter_flat", None, r#"
fn run() Never! {
    values := [String]{"alpha", "beta"}
    assert("{values}" == "[alpha, beta]")
    assert("{values:Debug}" == "[\"alpha\", \"beta\"]")
}
"#);
}

#[test]
fn payload_and_cursor_loans_end_before_metadata_owner_updates() {
    hosted_seed_program_runs(
        "payload_loan_retirement",
        None,
        include_str!("../../../tests/ui/compiler_payload_loans_before_owner_updates.jet"),
    );
}
