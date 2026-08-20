//! D-CONC-UNIT1 / D-CONC-JOIN1 (Tower #1557): task state and join duty share
//! the existing typestate and D-LIN1 flow facts.

fn diagnostic_codes(source: &str) -> Vec<String> {
    match jet::compile(source) {
        Ok(output) => output.lints.into_iter().map(|diagnostic| diagnostic.code).collect(),
        Err(diagnostics) => diagnostics.into_iter().map(|diagnostic| diagnostic.code).collect(),
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
    let diagnostics = diagnostic_codes(unjoined);
    assert!(
        diagnostics.iter().any(|code| code == "L1101"),
        "missing shared join-duty error: {diagnostics:?}"
    );

    let joined = r#"
fn run() {
    handle :: task 42
    print(handle.join() ?? 0)
}
"#;
    assert!(
        diagnostic_codes(joined).iter().all(|code| code != "L1101"),
        "joining must discharge the D-LIN1 duty: {:?}",
        diagnostic_codes(joined)
    );
}

#[test]
fn a_task_handle_list_is_consumed_by_pop_and_join() {
    let source = r#"
fn run() {
    workers := [Task<()>].{}
    workers.push(task { })
    loop workers.len() > 0 {
        worker :: workers.pop() ?? panic("missing worker")
        worker.join() ?? panic("worker failed")
    }
}
"#;
    jet::compile(source).expect(
        "a [Task<T>] list is not #SingleUse; popping and joining each handle must compile",
    );
}

#[test]
fn task_lifecycle_joins_through_the_shared_flow_fact_walker() {
    let source = r#"
fn run() {
    handle :: task 42
    if {
        true -> handle.join() ?? 0
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

/// Every diagnostic code a source produces, whether it compiled or not.
fn all_codes(source: &str) -> Vec<String> {
    match jet::compile(source) {
        Ok(output) => output
            .lints
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect(),
    }
}

/// Card #2006: a `return` inside a `task { … }` body is a CONDITIONAL return of
/// the enclosing function — the body may run later or not at all — so every
/// statement after the task stays reachable. Before the fix the body's `return`
/// left `flow.reachable == false` in the ENCLOSING block, so `check_stmt`
/// restored the pre-statement facts for each following statement and discarded
/// its declaration: one mistake inside a task body buried its own real error
/// under eleven phantom `nothing named X exists here` reports.
#[test]
fn a_return_inside_a_task_body_keeps_later_declarations() {
    let valueless = r#"
fn run() {
    child :: task { return }
    later :: 7
    print(later)
    child.detach()
}
"#;
    let codes = all_codes(valueless);
    assert!(
        codes.iter().all(|code| code != "E0107" && code != "E0102"),
        "a task-body return must not erase later declarations: {codes:?}"
    );

    // The same program with a real mistake inside the task body: the mistake is
    // reported, and nothing after the task is reported as undeclared.
    let mistaken = r#"
fn run() {
    child :: task { return "child" }
    later :: 7
    print(later)
    child.detach()
}
"#;
    let codes = all_codes(mistaken);
    assert!(
        codes.iter().all(|code| code != "E0107" && code != "E0102"),
        "the real error must not drag a phantom name cascade behind it: {codes:?}"
    );
    assert!(
        !codes.is_empty(),
        "the return handing a value back from a Unit `run` is still an error"
    );
}
