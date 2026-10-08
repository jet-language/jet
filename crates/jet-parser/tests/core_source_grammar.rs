use jet_parser::{Lexer, Parser};

fn parse_trusted_core(name: &str, source: &str) {
    // Use the module loader's offset-preserving package-header mask, not an
    // ad hoc time-specific prefix strip. The body still uses trusted lexing.
    let source = jet_pkg_model::Package::mask_inline_package_source(source)
        .unwrap_or_else(|error| panic!("{name} package header: {error:?}"))
        .0;
    let (tokens, diagnostics) = Lexer::lex_generated(&source);
    assert!(diagnostics.is_empty(), "{name} trusted lexing: {diagnostics:#?}");
    Parser::parse_for_check_with_source(&tokens, &source)
        .unwrap_or_else(|diagnostics| panic!("{name} trusted parsing: {diagnostics:#?}"));
}

#[test]
fn civil_core_internals_use_the_trusted_loader_lexing_contract() {
    let source = include_str!("../../../Core/time/time.jet");
    // Embedded Core uses generated lexing; checking this file as a user entry
    // must not grant access to the reserved namespace.
    parse_trusted_core("time", source);
    let (_, diagnostics) = Lexer::lex("fn __wall_sample() -> Int { 0 }");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"));
}

#[test]
fn system_process_and_files_internals_parse_as_trusted_core() {
    for (name, source) in [
        ("sys", include_str!("../../../Core/sys/sys.jet")),
        ("process", include_str!("../../../Core/process/process.jet")),
        ("files", include_str!("../../../Core/files/files.jet")),
    ] {
        parse_trusted_core(name, source);
    }
}

#[test]
fn watcher_event_carriers_use_the_trusted_loader_lexing_contract() {
    parse_trusted_core("watcher", include_str!("../../../Core/watcher/watcher.jet"));
}

#[test]
fn source_value_hooks_parse_as_trusted_compiler() {
    for (name, source) in [
        ("eval-values", include_str!("../../../Compiler/JetEval/Source/EvalValues.jet")),
        ("eval-operations", include_str!("../../../Compiler/JetEval/Source/EvalOperations.jet")),
        ("eval", include_str!("../../../Compiler/JetEval/Source/Evaluator.jet")),
        ("eval-format", include_str!("../../../Compiler/JetEval/Source/Formatting/Values.jet")),
        ("emit-types", include_str!("../../../Compiler/JetCodegen/Source/Emit/Types.jet")),
        ("source-hook-registry", include_str!("../../../Compiler/JetFoundation/Source/Registry/CoreModules.jet")),
        ("mir-hooks", include_str!("../../../Compiler/JetFoundation/Source/MIR/MIR.jet")),
        ("native-values", include_str!("../../../Compiler/JetBackend/Source/Lower/Values.jet")),
        ("native-reach", include_str!("../../../Compiler/JetBackend/Source/Lower/Reach.jet")),
        ("native-display", include_str!("../../../Compiler/JetBackend/Source/Lower/Display.jet")),
    ] {
        parse_trusted_core(name, source);
    }
}
