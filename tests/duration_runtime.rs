fn checked_bundle(source: &str) -> jet::AST::ProgramBundle {
    let dir = std::env::temp_dir().join(format!(
        "jet_duration_runtime_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    bundle
}

#[test]
fn checked_duration_surface_reaches_tir_and_jit() {
    let source = r#"
fn run() {
    d :: Duration.seconds(1.5) ?? panic("duration")
    print(d.in(.Milliseconds) ?? panic("read"))
}
"#;
    let compiled = jet::compile(source).expect("duration surface should compile");
    assert!(compiled.rust.contains("jet_duration_from_float"));
    assert!(compiled.rust.contains("jet_duration_in"));

    if jet_jit::cranelift_host_supported() {
        let bundle = checked_bundle(r#"
fn make() => Duration ? RangeError {
    return Duration.seconds(1.5)
}
fn read(d: Duration) => Int ? RangeError {
    return d.in(.Milliseconds)
}
fn run() {}
"#);
        jet_jit::try_compile_bundle(&bundle).expect("duration surface should lower to resident JIT");
    }
}

#[test]
fn retired_duration_aliases_are_not_callable() {
    for source in [
        "use core.time as time\nfn run() { _ :: time.seconds(1) }",
        "fn run() { d :: Duration.seconds(1) ?? panic(\"duration\")\n_ :: d.millis() }",
    ] {
        assert!(jet::compile(source).is_err(), "retired duration alias compiled: {source}");
    }
}

#[test]
fn local_duration_binding_shadows_builtin_without_reaching_codegen() {
    let source = "fn run() {\nDuration :: 1\n_ :: Duration.seconds(1)\n}";
    let diagnostics = jet::compile(source).expect_err("shadowed Duration should fail in sema");
    let rendered = jet::render_diagnostics("shadow.jet", source, &diagnostics);
    assert!(
        rendered.contains("[E0311]") && rendered.contains("`seconds` isn't a method"),
        "{rendered}"
    );
}

#[test]
fn formatter_preserves_duration_unit_calls() {
    let source = "fn run(){d::Duration.seconds(1)??panic(\"duration\")\nprint(d.in(.Milliseconds)??panic(\"read\"))}";
    let once = jet::format_source(source).expect("duration source should format");
    let twice = jet::format_source(&once).expect("formatted duration source should parse");
    assert_eq!(once, twice);
    assert!(once.contains("Duration.seconds(1)"));
    assert!(once.contains("d.in(.Milliseconds)"));
}
