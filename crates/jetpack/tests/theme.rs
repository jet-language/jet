use std::process::Command;

fn jetpack() -> &'static str {
    env!("CARGO_BIN_EXE_jetpack")
}

#[test]
fn redirected_help_auto_is_ansi_free() {
    let output = Command::new(jetpack())
        .arg("help")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("jetpack —"));
    assert!(!output.stdout.contains(&0x1b), "redirected help contained ANSI");
}

#[test]
fn json_mode_disables_presentation_even_for_forced_error_output() {
    let output = Command::new(jetpack())
        .args(["not-a-command", "--json", "--color=always"])
        .env("FORCE_COLOR", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty(), "expected an error diagnostic");
    assert!(!output.stdout.contains(&0x1b), "JSON stdout contained ANSI");
    assert!(!output.stderr.contains(&0x1b), "JSON error contained ANSI");
}
