use std::process::Command;

fn jet() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jet"))
}

fn effects_example() -> String {
    format!(
        "{}/examples/features/effects/effects.jet",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn find_searches_by_signature_effect_and_example() {
    let target = effects_example();

    let signature = jet()
        .args(["find", "Int -> Int", target.as_str()])
        .output()
        .unwrap();
    assert!(
        signature.status.success(),
        "signature stderr: {}",
        String::from_utf8_lossy(&signature.stderr)
    );
    let signature_stdout = String::from_utf8_lossy(&signature.stdout);
    assert!(
        signature_stdout.contains("square"),
        "signature: {signature_stdout}"
    );

    let effect = jet()
        .args(["find", "--effect", "IO", "report", target.as_str()])
        .output()
        .unwrap();
    assert!(
        effect.status.success(),
        "effect stderr: {}",
        String::from_utf8_lossy(&effect.stderr)
    );
    let effect_stdout = String::from_utf8_lossy(&effect.stdout);
    assert!(effect_stdout.contains("report"), "effect: {effect_stdout}");
    assert!(
        effect_stdout.contains("holds IO"),
        "effect reason: {effect_stdout}"
    );
    assert!(
        effect_stdout.contains("why:"),
        "effect explanation: {effect_stdout}"
    );

    let example = jet()
        .args(["find", "--example", "4 -> 16", target.as_str()])
        .output()
        .unwrap();
    assert!(
        example.status.success(),
        "example stderr: {}",
        String::from_utf8_lossy(&example.stderr)
    );
    let example_stdout = String::from_utf8_lossy(&example.stdout);
    assert!(
        example_stdout.contains("square"),
        "example: {example_stdout}"
    );
    assert!(
        example_stdout.contains("why:"),
        "example explanation: {example_stdout}"
    );
}

#[test]
fn find_is_a_flat_top_level_verb() {
    assert!(jet::CLI::is_canonical_top_level("find"));
    let out = jet().args(["find", "--help"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("jet find"), "stdout: {stdout}");
    assert!(!stdout.contains("jet inspect find"), "stdout: {stdout}");
}
