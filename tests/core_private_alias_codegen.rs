//! Core's reserved part spelling and its local namespace alias share one checked target.
mod common;

use std::process::Command;

#[test]
fn core_private_grouped_import_keeps_its_checked_target_through_emit_and_run() {
    let scratch = common::Scratch::new("core_private_alias_codegen");
    let entry = scratch.join("run.jet");
    std::fs::write(&entry, "use core.files as files\nfn run() { print(7) }\n").unwrap();

    let mut bundle = jet::Loader::load_entry_with_overlay(entry.to_str().unwrap(), None, false)
        .expect("load Core-private import closure");
    let time_source = jet_foundation::CoreModuleExports::core_source_module("core.time").unwrap();
    let time_modules = bundle.modules.iter()
        .filter(|module| module.path.ends_with(time_source.path))
        .collect::<Vec<_>>();
    assert_eq!(time_modules.len(), 1, "Stat's implicit DateTime field must load its source owner once");
    assert!(time_modules[0].items.iter().any(|item| matches!(
        item, jet::AST::Item::Struct(row) if row.name == "DateTime"
    )), "the loaded owner must supply the real DateTime declaration");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(
        !diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
        "Core-private import checking failed: {diagnostics:#?}"
    );
    let (module_index, module) = bundle.modules.iter().enumerate().find(|(_, module)| {
        module.imports.iter().any(|import| matches!(
            &import.kind,
            jet::AST::ImportKind::Unqualified { module_alias, .. }
                if module_alias == "__core_files_path_components"
        ))
    }).expect("Core closure contains the private grouped PathPlatform import");
    let grouped = module.imports.iter().find(|import| matches!(
        &import.kind,
        jet::AST::ImportKind::Unqualified { module_alias, .. }
            if module_alias == "__core_files_path_components"
    )).unwrap();
    let namespace = module.imports.iter().find(|import| import.import_alias() == "host_path")
        .expect("the same part is imported under a different local namespace alias");
    let target = bundle.name_ledger.import_target(module_index, grouped.span)
        .expect("sema records the grouped declaration's canonical target");
    assert_eq!(bundle.name_ledger.import_target(module_index, namespace.span), Some(target));
    assert_eq!(bundle.modules[target].alias, "__core_files_path_components");
    assert_eq!(bundle.name_ledger.alias(module_index, "PathPlatform").unwrap().target_module, Some(target));

    for arguments in [vec!["emit", "--rust", "run.jet"], vec!["run", "run.jet"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(&arguments)
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", scratch.join("cache"))
            .env("JETPACK_ROOT", scratch.join("jetpack"))
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .output().unwrap();
        assert!(output.status.success(), "{arguments:?} failed:\n{}", String::from_utf8_lossy(&output.stderr));
        if arguments[0] == "run" {
            assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
        } else {
            assert!(!output.stdout.is_empty(), "Rust emission must produce source");
        }
    }
}
