#[test]
fn bare_run_stays_valid() {
    jet::compile("fn run() {}\n").expect("bare beginner entrypoint must compile");
}

#[test]
fn fallible_void_run_is_the_only_fallible_entrypoint() {
    let src = r#"
fn run() -> Void ? {
    return err("boom")
}
"#;
    let out = jet::compile(src).expect("fallible Void run should compile");
    assert!(
        out.rust.contains("pub fn user_run() -> Result<(), String>"),
        "Void ? run should lower to Result<(), String>:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("if let Err(__jet_err) = user_run()"),
        "fallible run wrapper must handle returned errors:\n{}",
        out.rust
    );
    assert!(
        out.rust.contains("std::process::exit(1);"),
        "fallible run wrapper must exit nonzero on error:\n{}",
        out.rust
    );
}

#[test]
fn fallible_void_run_can_finish_normally_after_try() {
    let src = r#"
fn step() -> Int ? {
    return ok(1)
}

fn run() -> Void ? {
    n :: step()?
    print(n)
}
"#;
    let out = jet::compile(src).expect("fallible Void run should allow normal completion");
    assert!(
        out.rust.contains("Ok(())"),
        "fallible Void run should synthesize success at the end:\n{}",
        out.rust
    );
}

#[test]
fn unit_fallible_run_stays_rejected() {
    let src = r#"
fn run() -> Unit ? {
    return err("boom")
}
"#;
    let diags = jet::compile(src).expect_err("Unit ? run should not be accepted");
    assert!(
        diags.iter().any(|d| d.code == "E0122"),
        "expected E0122, got: {diags:?}"
    );
}

#[test]
fn fallible_void_fallthrough_is_entrypoint_only() {
    let src = r#"
fn helper() -> Void ? {
}

fn run() {
    print("hi")
}
"#;
    let diags = jet::compile(src).expect_err("non-run Void ? fallthrough needs return");
    assert!(
        diags.iter().any(|d| d.code == "E0114"),
        "expected E0114, got: {diags:?}"
    );
}
