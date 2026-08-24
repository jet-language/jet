//! D-CONC-SHARE1=A / D-CONC-STM1=A: every shared cell in one statement joins
//! the ordered commit plane.

#[test]
fn shared_multi_cell_statement_registers_every_participant() {
    let source = r#"
struct Counter { value: Int }

fn run() {
    left := shared Counter{ value: 0 }
    right := shared Counter{ value: 1 }
    left.value = right.value
}
"#;
    let output = jet::compile(source).expect("multi-cell shared statement must compile");
    assert!(
        output.rust.contains(".read_txn(&mut __jet_stm"),
        "the RHS shared read must register its participant: {}",
        output.rust
    );
    assert!(
        output.rust.contains(".edit_txn(&mut __jet_stm"),
        "the destination shared write must use the same commit: {}",
        output.rust
    );
}

#[test]
fn shared_plain_access_repl_uses_the_same_value_surface() {
    let output = jet::REPL::run_transcript(
        &[
            "struct Counter { value: Int }",
            "cell := shared Counter{ value: 0 }",
            "cell.value += 1",
            "cell.value",
        ],
        None,
    );
    assert!(
        output.contains("1 : Int"),
        "REPL shared access drifted: {output}"
    );
    assert!(
        !output.contains("E1116"),
        "REPL used the retired closure form: {output}"
    );
}
