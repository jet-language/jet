//! D-FACT-READ1: one typed, compile-time reader for every registered plane.

fn diagnostics(source: &str) -> Vec<jet::Diagnostics::Diagnostic> {
    jet::compile(source).expect_err("the fixture must be rejected")
}

#[test]
fn runtime_fact_reads_are_refused_before_codegen() {
    let diags = diagnostics(
        "#Numeric Severity :: distinct Int(0..10)\n\nfn run() {\n    print(Severity.@range.start)\n}\n",
    );
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0302")
        .expect("runtime fact read should have a registered diagnostic");
    assert!(diagnostic.what.contains("compile-time only"), "{diagnostic:?}");
    assert!(diagnostic.why.contains("never selects runtime behavior"), "{diagnostic:?}");
    assert!(diagnostic.fix.contains("binding"), "{diagnostic:?}");
}

#[test]
fn fact_reads_do_not_enter_type_position() {
    let diags = diagnostics(
        "#Numeric Severity :: distinct Int(0..10)\n\nfn takes(value: Severity.@range) {}\nfn run() {}\n",
    );
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0119")
        .expect("a fact in type position should have a registered diagnostic");
    assert!(diagnostic.what.contains("fact value"), "{diagnostic:?}");
    assert!(diagnostic.why.contains("do not mint or select types"), "{diagnostic:?}");
}

#[test]
fn folded_fact_reads_emit_values_without_runtime_dispatch() {
    let output = jet::compile(
        "#Numeric Severity :: distinct Int(0..10)\n\n@range :: Severity.@range\n\nfn run() {\n    print(@range.start)\n}\n",
    )
    .expect("a comptime fact read should compile");
    assert!(output.rust.contains("JetRange"), "the typed fact carrier is missing");
    assert!(
        !output.rust.contains("fact_read") && !output.rust.contains("jet.fact"),
        "a folded fact must not emit a runtime reader or dispatch path"
    );
}

#[test]
fn derive_bodies_read_the_same_typed_fact() {
    let output = jet::compile(
        "derive T.Debug {\n    states :: T.@states\n    emit(\"fn derived_fact_read() => String {{ return \\\"ok\\\" }}\")\n}\n\nstate Report { Draft, Published }\n\n#Debug\nstruct Report {\n    value: Int\n}\n\nfn run() {}\n",
    )
    .expect("derive fact read should compile");
    assert!(output.rust.contains("derived_fact_read"));
}
