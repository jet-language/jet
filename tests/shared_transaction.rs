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
