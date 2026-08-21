#[test]
fn qualified_variant_heads_round_trip_through_protocol_expansion() {
    let literal_source = "fn make() :> Payment.Client { return Payment.Client.{_token: 0} }\n";
    let literal = jet::format_source(literal_source).expect("qualified literal should format");
    assert!(literal.contains("Payment.Client{_token: 0}"), "{literal}");
    assert!(!literal.contains("Payment.Client.{"), "{literal}");
    assert_eq!(
        literal,
        jet::format_source(&literal).expect("qualified literal should reformat")
    );

    let source = include_str!("../examples/features/concurrency/protocol.jet");
    let formatted = jet::format_source(source).expect("protocol source should format");
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted protocol source should reformat")
    );
    jet::compile(&formatted).expect("protocol expansion must emit canonical variant literals");
}

#[test]
fn generic_module_template_struct_heads_round_trip() {
    let source = r#"module cache<K>(capacity: Int) {
    struct Entry { key: K }
    fn entry(k: K) :> Entry { return Entry.{key: k} }
}

"#;
    let formatted = jet::format_source(source).expect("generic module source should format");
    assert!(
        formatted.contains("Entry{"),
        "struct head was not canonicalized:\n{formatted}"
    );
    assert!(
        !formatted.contains("Entry.{"),
        "retired struct head survived formatting:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted generic module should reformat")
    );
}

#[test]
fn derive_template_bodies_round_trip_retired_signatures() {
    let source = include_str!("../examples/features/reflection/derive_loop.jet");
    let formatted = jet::format_source(source).expect("derive template should format");
    assert!(
        formatted.contains("fn @method(self) String :> field.@name"),
        "derive callable was not canonicalized:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted derive template should reformat")
    );
}

#[test]
fn marker_template_bodies_round_trip_retired_signatures() {
    let source = include_str!("../examples/features/reflection/user_rule_body.jet");
    let formatted = jet::format_source(source).expect("marker template should format");
    assert!(
        formatted.contains("fn greeting(self) String :> \"hello\""),
        "marker callable was not canonicalized:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted marker template should reformat")
    );
}

#[test]
fn generic_module_and_library_bodies_round_trip_retired_signatures() {
    let generic = include_str!("../examples/features/modules/generic_modules.jet");
    let formatted = jet::format_source(generic).expect("generic module should format");
    assert!(
        formatted.contains("pub fn slot(k: K) Slot :> Slot.Value(k)"),
        "generic module callable was not canonicalized:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted generic module should reformat")
    );

    let library = include_str!("../examples/features/modules/library.jet");
    let formatted = jet::format_source(library).expect("library should format");
    assert!(
        formatted.contains("fn greeting(self) String\n"),
        "library signature was not canonicalized:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted library should reformat")
    );
}

#[test]
fn callback_view_provenance_round_trips_retired_function_type_arrows() {
    let source = include_str!("../examples/features/memory/view_from_callback.jet");
    let formatted = jet::format_source(source).expect("callback view source should format");
    assert!(
        formatted.contains("pick: fn(String, String) View<str> from _0"),
        "function-type result arrow survived formatting:\n{formatted}"
    );
    assert!(
        formatted.contains(") View<str> from input {"),
        "callable result arrow survived formatting:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted callback view source should reformat")
    );
}

#[test]
fn generated_source_template_bodies_round_trip_retired_signatures() {
    let source = include_str!(
        "../examples/features/tooling/build_entry_discovery/packages/foundation/tools/build.jet"
    );
    let formatted = jet::format_source(source).expect("generated source template should format");
    assert!(
        formatted.contains("fn foundation_marker() String :> \"foundation\""),
        "generated callable was not canonicalized:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted)
            .expect("formatted generated source template should reformat")
    );
}

#[test]
fn inline_module_struct_heads_round_trip() {
    let source = r#"module arith {
    struct Tally { total: Int }
    fn tally(value: Int) :> Tally { return Tally.{total: value} }
}
"#;
    let formatted = jet::format_source(source).expect("inline module source should format");
    assert!(
        formatted.contains("Tally{"),
        "struct head was not canonicalized:\n{formatted}"
    );
    assert!(
        !formatted.contains("Tally.{"),
        "retired struct head survived formatting:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted inline module should reformat")
    );
}

#[test]
fn effect_arrows_round_trip_beside_foreign_bodies() {
    let source = r#"#Extern module c.demo {
    fn probe(value: Int) => Int = "probe";
}

fn run() =[IO]=> Int {
    return 1
}
"#;
    let formatted = jet::format_source(source).expect("foreign source should format");
    assert!(
        formatted.contains("fn probe(value: Int) Int = \"probe\""),
        "foreign declaration arrow was not canonicalized:\n{formatted}"
    );
    assert!(
        formatted.contains("fn run() Int :[IO]>"),
        "effect arrow was not canonicalized:\n{formatted}"
    );
    assert!(
        !formatted.contains("=[IO]=>") && !formatted.contains("Int) => Int"),
        "retired arrow survived formatting:\n{formatted}"
    );
    assert_eq!(
        formatted,
        jet::format_source(&formatted).expect("formatted foreign source should reformat")
    );
}
