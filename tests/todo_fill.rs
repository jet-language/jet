use std::path::PathBuf;
use std::process::Command;

fn todo_example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/features/tooling/todo_hole.jet")
}

#[test]
fn check_reports_goal_and_fill_only_offers_checked_candidates() {
    let file = todo_example();
    let check = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["check", file.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(check.status.success(), "check failed: {:?}", check);
    let check_text = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(check_text.contains("expected type: Int"), "{check_text}");
    assert!(
        check_text.contains("required effects: none"),
        "{check_text}"
    );
    assert!(
        check_text.contains("required effects: [IO]"),
        "{check_text}"
    );

    let fill = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["fill", file.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(fill.status.success(), "fill failed: {:?}", fill);
    let fill_text = String::from_utf8_lossy(&fill.stdout);
    assert!(fill_text.contains("candidates:"), "{fill_text}");
    assert!(fill_text.contains("n (checked)"), "{fill_text}");
    assert!(fill_text.contains("0 (checked)"), "{fill_text}");
    for line in fill_text.lines() {
        if line
            .trim_start()
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        {
            assert!(line.contains("(checked)"), "unchecked proposal: {line}");
        }
    }
}
