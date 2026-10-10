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
        let source = directory.join(format!("{label}.rs"));
        let executable = directory.join(format!("{label}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source, rust).unwrap();
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2021", "-Awarnings"])
            .arg(format!("--crate-name={label}"))
            .arg(&source).arg("-o").arg(&executable)
            .output().expect("regression requires rustc");
        assert!(compiled.status.success(), "{label} must compile:\n{}",
            String::from_utf8_lossy(&compiled.stderr));
        let executed = std::process::Command::new(&executable).current_dir(&directory)
            .output().expect("regression must execute");
        assert!(executed.status.success(), "{label} must run:\n{}",
            String::from_utf8_lossy(&executed.stderr));
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
