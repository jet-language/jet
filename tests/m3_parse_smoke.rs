#[test]
fn parse_option_fn() {
    let src = r#"
fn find_even(limit: Int) -> Int? {
    for i in 1..limit {
        if i % 2 == 0 {
            return value(i);
        }
    }
    return null;
}
fn main() {}
"#;
    let (toks, _) = jet::jeter::jet(src);
    let prog = jet::parser::parse(&toks).expect("parse");
    assert_eq!(prog.items.len(), 2);
}
