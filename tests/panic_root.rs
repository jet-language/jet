#[test]
fn panic_is_not_required_by_positive_capability_bounds() {
    for source in [
        r#"
fn run() {
    #Abilities(FS) {
        panic("stop")
    }
}
"#,
        r#"
fn run() {
    #Abilities(caps: FS) {
        panic("stop")
    }
}
"#,
    ] {
        jet::compile(source).expect("Panic must not need a positive capability");
    }
}
