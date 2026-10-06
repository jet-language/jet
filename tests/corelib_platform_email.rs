#![allow(dead_code, unused_imports)]
mod common;
include!("corelib_parts/support.rs");
include!("corelib_parts/platform_email.rs");

#[test]
fn core_email_address_rejects_domain_labels_over_63_bytes() {
    let scratch = common::Scratch::new("email-domain-labels");
    let dir = &scratch.path;
    let src = r#"
use core.email as email

fn run() {
    label_63 := "a".repeat(63)
    valid :: email.address("person@{label_63}.example") ?? panic("valid 63-byte domain label rejected")
    print(valid.mailbox == ("person@{label_63}.example"))
    label_64 := "a".repeat(64)
    if email.address("person@{label_64}.example") == {
        .Ok(_) -> panic("64-byte domain label accepted")
        .Err(_) -> print("overlong-label-rejected")
    }
}

"#;
    let expected = "true\noverlong-label-rejected";
    let (code, stdout, stderr) = build_and_run(&dir, "email_domain_labels", src, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), expected, "{stdout}");
    let file = dir.join("email_domain_labels.jet");
    fs::write(&file, src).unwrap();
    for (tier, use_interpreter) in [("default dev", false), ("forced interpreter", true)] {
        match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, use_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "email address failed in {tier}: {stderr}");
                assert_eq!(stdout.trim(), expected, "{tier}: {stdout}");
            }
            other => panic!("email address did not run in {tier}: {other:?}"),
        }
    }
}
