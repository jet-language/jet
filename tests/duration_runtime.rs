mod common;
mod tir_support;


#[test]
fn checked_duration_constructors_preserve_units_across_tiers() {
    let source = r#"
fn run() {
    fractional :: Duration.seconds(1.5) ?? panic("fractional duration")
    whole :: Duration.milliseconds(1500) ?? panic("whole duration")
    exact :: Duration.nanoseconds(4611686018427387904) ?? panic("exact duration")
    print(fractional.in(.Milliseconds) ?? panic("fractional read"))
    print(whole.in(.Seconds) ?? panic("whole read"))
    print(exact.in(.Nanoseconds) ?? panic("exact read"))
    _overflow :: Duration.hours(9223372036854775807) ?? {
        print("overflow")
        return
    }
    print("unexpected duration")
}
"#;
    tir_support::assert_tiers_agree(
        "duration_checked_constructors",
        source,
        "1500\n1\n4611686018427387904\noverflow\n",
    );
}

#[test]
fn retired_duration_aliases_are_not_callable() {
    for source in [
        "use core.time as time\nfn run() { _ :: time.seconds(1) }",
        "fn run() { d :: Duration.seconds(1) ?? panic(\"duration\")\n_ :: d.millis() }",
    ] {
        assert!(
            jet::compile(source).is_err(),
            "retired duration alias compiled: {source}"
        );
    }
}

#[test]
fn local_duration_binding_shadows_builtin_without_reaching_codegen() {
    let source = "fn run() {\nDuration :: 1\n_ :: Duration.seconds(1)\n}";
    let diagnostics = jet::compile(source).expect_err("shadowed Duration should fail in sema");
    let rendered = jet::render_diagnostics("shadow.jet", source, &diagnostics);
    assert!(
        rendered.contains("[E0311]"),
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
