//! D-CONC-UNIT1 / D-CONC-JOIN1 (Tower #1557): task state and join duty share
//! the existing typestate and D-LIN1 flow facts.

fn lint_codes(source: &str) -> Vec<String> {
    match jet::compile(source) {
        Ok(output) => output.lints.into_iter().map(|diagnostic| diagnostic.code).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn task_join_duty_uses_the_dlin1_obligation_pass() {
    let unjoined = r#"
fn run() {
    handle :: task 42
    print(0)
}
"#;
    let lints = lint_codes(unjoined);
    assert!(lints.iter().any(|code| code == "L1101"), "missing shared join-duty lint: {lints:?}");

    let joined = r#"
fn run() {
    handle :: task 42
    print(handle.join() ?? 0)
}
"#;
    assert!(
        lint_codes(joined).iter().all(|code| code != "L1101"),
        "joining must discharge the D-LIN1 duty: {:?}",
        lint_codes(joined)
    );
}

#[test]
fn task_lifecycle_joins_through_the_shared_flow_fact_walker() {
    let source = r#"
fn run() {
    handle :: task 42
    if {
        true -> handle.join()
        else -> handle.detach()
    }
}
"#;
    let output = jet::compile(source).expect("both branches must discharge the task duty");
    assert!(
        output.lints.iter().any(|diagnostic| diagnostic.code == "L0152"),
        "joined task states must report the shared typestate divergence: {:?}",
        output.lints
    );
    assert!(
        output.lints.iter().all(|diagnostic| diagnostic.code != "L1101"),
        "the shared D-LIN1 pass must not report a task consumed on both branches: {:?}",
        output.lints
    );
}
