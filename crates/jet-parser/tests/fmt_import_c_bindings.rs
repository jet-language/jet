use jet_parser::Formatter::{format_source_with_options, FormatOptions};

#[test]
fn raw_c_imports_and_module_binding_mutability_are_byte_exact_and_idempotent() {
    let source = include_str!("../../../tests/fixtures/fmt_import_c_bindings.jet");
    for simplify in [false, true] {
        let options = FormatOptions { simplify };
        let once = format_source_with_options(source, options).expect("fixture should format");
        assert_eq!(
            once, source,
            "formatter must preserve imports and binding sigils"
        );
        let twice = format_source_with_options(&once, options).expect("fixture should reformat");
        assert_eq!(twice, once, "formatter must be idempotent");
    }
}
