//! #29 compiler-seam seed witnesses; these die at compiler cutover.
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

fn check_core(source: &str) -> Vec<jet::Diagnostics::Diagnostic> {
    let scratch = common::Scratch::new("core_shared_site");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("seam.jet");
    std::fs::write(&entry, source).unwrap();
    jet::run_compiler_work(|| {
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        // Only this executable compiler witness supplies a trusted Core identity.
        // No user path gains access to the reserved intrinsics.
        let index = bundle.entry;
        bundle.modules[index].display = "<corelib>/shared_site_witness.jet".into();
        jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter().filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
            .collect()
    })
}

#[test]
fn mem_only_direct_initializers_are_admitted_for_each_lifetime() {
    for lifetime in ["process", "world", "thread", "attempt"] {
        let source = format!("use core.mem as mem\nfn init() -> Shared<Int> Never! {{ shared 7 }}\nfn run() {{ mem.__{lifetime}_shared(init).drop(\"site witness\") }}\n");
        assert!(check_core(&source).is_empty(), "{lifetime}");
    }
}

#[test]
fn malformed_core_initializer_operands_are_refused() {
    let cases = [
        ("fn init() -> Shared<Int> Never! { shared 7 }", "init()"),
        ("fn init(value: Int) -> Shared<Int> Never! { shared value }", "init"),
        ("fn init<T>() -> Shared<Int> Never! { shared 7 }", "init"),
        ("fn init() -> Int Never! { 7 }", "init"),
        ("fn init() -> Shared<Int>? { Val(shared 7) }", "init"),
        ("fn init() -> Shared<Int> Never! { shared 7 }", "() -> shared 7"),
    ];
    for (declaration, operand) in cases {
        let source = format!("use core.mem as mem\n{declaration}\nfn run() {{ mem.__process_shared({operand}).drop(\"invalid site\") }}\n");
        let diagnostics = check_core(&source);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0109"), "{source}\n{diagnostics:#?}");
    }
}

#[test]
fn initializer_transitive_io_is_not_hidden_by_shared_allocation() {
    let source = "use core.mem as mem\nfn helper() { print(\"not pure\") }\nfn init() -> Shared<Int> Never! { helper(); shared 7 }\nfn run() { mem.__process_shared(init).drop(\"impure site\") }\n";
    let diagnostics = check_core(source);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0109"), "{diagnostics:#?}");
}

#[test]
fn every_lifetime_conflict_is_reported_against_first_source_site() {
    let source = "use core.mem as mem\nfn init() -> Shared<Int> Never! { shared 7 }\nfn z_first() { mem.__process_shared(init).drop(\"first\") }\nfn a_second() { mem.__world_shared(init).drop(\"second\") }\nfn b_third() { mem.__thread_shared(init).drop(\"third\") }\nfn run() { mem.__attempt_shared(init).drop(\"fourth\") }\n";
    let diagnostics = check_core(source);
    let conflicts = diagnostics.iter().filter(|diagnostic| diagnostic.code == "E0109").collect::<Vec<_>>();
    assert_eq!(conflicts.len(), 3, "{diagnostics:#?}");
    let starts = conflicts.iter().map(|diagnostic| diagnostic.span.unwrap().start).collect::<Vec<_>>();
    assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "{starts:?}");
}

#[test]
fn user_cannot_borrow_core_trust_with_a_core_looking_module_name() {
    for lifetime in ["process", "world", "thread", "attempt"] {
        let source = format!("use core.mem as mem\nfn init() -> Shared<Int> Never! {{ shared 7 }}\nfn run() {{ mem.__{lifetime}_shared(init).drop(\"reserved site\") }}\n");
        let diagnostics = tir_support::compile_source("core.mem", &source).err().expect("user seam must refuse");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"), "{diagnostics:#?}");
    }
}

fn lower_core(source: &str) -> jet_foundation::MIR::MirProgram {
    let scratch = common::Scratch::new("core_shared_site_mir");
    tir_support::write_test_package(&scratch.path, tir_support::TIR_TEST_PACKAGE);
    let entry = scratch.path.join("seam.jet");
    std::fs::write(&entry, source).unwrap();
    jet::run_compiler_work(|| {
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let index = bundle.entry;
        bundle.modules[index].display = "<corelib>/shared_site_witness.jet".into();
        let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
        assert!(!diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
        let request = jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            jet_foundation::MIR::MirArtifactBuildMode::Dev,
        );
        let tir = jet::Codegen::TIR::lower_checked_tir_program_for(&bundle, request).unwrap();
        jet::Codegen::TIR::lower_tir_to_mir(tir).unwrap()
    })
}

const TWO_CALLERS: &str = "use core.mem as mem\nfn init() -> Shared<Int> Never! { shared 7 }\nfn first() { mem.__process_shared(init).drop(\"first\") }\nfn run() { first(); mem.__process_shared(init).drop(\"second\") }\n";

#[test]
fn both_callers_lower_to_one_typed_initializer_identity_site() {
    use jet_foundation::MIR::{MirConstant, MirOperation, MirSharedLifetime, MirTypeKind};
    let program = lower_core(TWO_CALLERS);
    jet_foundation::MIROptimization::verify_mir_legality(&program).unwrap();
    let sites = program.constants.iter().filter(|row| matches!(row.value, MirConstant::SharedSite(_))).collect::<Vec<_>>();
    assert_eq!(sites.len(), 1);
    let site = sites[0];
    let MirConstant::SharedSite(identity) = &site.value else { unreachable!() };
    assert_eq!(identity.lifetime, MirSharedLifetime::Process);
    assert_eq!(site.key, format!("shared-site:{}", identity.initializer.0));
    assert!(matches!(site.ty.kind, MirTypeKind::Shared(_)));
    let readers = program.functions.iter().flat_map(|function| &function.blocks)
        .flat_map(|block| &block.instructions)
        .filter(|instruction| matches!(&instruction.operation, MirOperation::Global { name } if name == &site.key)).count();
    assert_eq!(readers, 2);
    let mut other_lifetime = program.clone();
    let row = other_lifetime.constants.iter_mut().find(|row| row.id == site.id).unwrap();
    let MirConstant::SharedSite(identity) = &mut row.value else { unreachable!() };
    identity.lifetime = MirSharedLifetime::World;
    assert_ne!(program.deterministic_digest(), other_lifetime.deterministic_digest());
}

#[test]
fn malformed_typed_site_rows_are_rejected_before_emission() {
    use jet_foundation::MIR::{MirConstant, MirType, MirTypeKind};
    let program = lower_core(TWO_CALLERS);
    let index = program.constants.iter().position(|row| matches!(row.value, MirConstant::SharedSite(_))).unwrap();
    for defect in 0..5 {
        let mut malformed = program.clone();
        match defect {
            0 => malformed.constants[index].key = "not-an-init-identity".into(),
            1 => malformed.constants[index].is_storage = true,
            2 => malformed.constants[index].ty = MirType::from_kind(MirTypeKind::String),
            3 => malformed.constants[index].value = MirConstant::List(vec![malformed.constants[index].value.clone()]),
            4 => {
                let MirConstant::SharedSite(site) = &malformed.constants[index].value else { unreachable!() };
                let initializer = malformed.functions.iter_mut().find(|function| function.id == site.initializer).unwrap();
                initializer.effects.solved.insert("IO".into());
            }
            _ => unreachable!(),
        }
        assert!(jet_foundation::MIROptimization::verify_mir_legality(&malformed).is_err(), "malformed site {defect} admitted");
    }
}

fn quoted_api_list<'a>(source: &'a str, begin: &str, end: &str) -> std::collections::BTreeSet<&'a str> {
    let body = source.split_once(begin).expect("API list declaration").1
        .split_once(end).expect("API list end").0;
    let mut names = std::collections::BTreeSet::new();
    for name in body.split('"').skip(1).step_by(2) {
        assert!(name.starts_with("core.") && name.contains("::"), "not a canonical API: {name}");
        assert!(names.insert(name), "duplicate Web refusal: {name}");
    }
    names
}

#[test]
fn browser_host_refusal_lists_match_and_every_member_is_registered() {
    let rust = quoted_api_list(
        include_str!("../crates/jet-sema/src/Sema/TargetSurface.rs"),
        "const WEB_HOST_UNSUPPORTED_CORE_APIS: &[&str] = &[", "\n];");
    let jet = quoted_api_list(
        include_str!("../Compiler/JetSema/Source/Sema/CheckProgram.jet"),
        "fn sema_web_host_unsupported_core_apis() -> [String] Never! {", "\n}");
    assert_eq!(rust, jet);
    assert_eq!(rust.len(), 16);
    // Fault names join this list only with their real registered source rows.
    assert!(rust.iter().all(|name| !name.starts_with("core.testing::")));
    for name in rust {
        let (module, member) = name.split_once("::").unwrap();
        assert!(jet_foundation::CoreModuleExports::core_leaf_kind(module, member).is_some(),
            "unknown Web refusal API {name}");
    }
}

#[test]
fn browser_refuses_direct_environment_api_with_spanless_diagnostic() {
    let source = "#Target(Web)\nuse core.sys as sys\nfn run() { print(sys.get(\"JET_SHARED_SITE_TEST\")) }\n";
    let diagnostics = tir_support::compile_source("web_host_surface", source).err().expect("browser env read must refuse");
    let refusal = diagnostics.iter().find(|diagnostic| diagnostic.code == "E-WEB-TIR-UNSUPPORTED").expect("registered browser refusal");
    assert!(refusal.what.contains("core.sys.get"));
    assert!(refusal.span.is_none());
}

#[test]
fn browser_admits_non_host_core_api_and_ordinary_shared() {
    let source = "#Target(Web)\nuse core.prelude as pure\nfn run() { owner :: shared pure.identity_int(7); guard :: owner.guard_read(); print(guard.value) }\n";
    let result = tir_support::compile_source("web_pure_shared", source);
    assert!(result.is_ok(), "ordinary Shared or pure Core API refused: {:#?}", result.err());
}
