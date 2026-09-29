#[test]
fn core_email_address_and_mime_are_bounded_and_deterministic() {
    let scratch = common::Scratch::new("email-mime");
    let dir = &scratch.path;
    let src = r#"
use core.email as email

fn run() {
    sender :: email.address("Mara ☕ <mara@example.com>") ?? panic("unicode address")
    recipient :: email.address("Ada <ada@example.net>") ?? panic("recipient")
    hidden :: email.address("audit@example.org") ?? panic("bcc")
    if email.address("attacker@example.com\nBcc: stolen@example.com") == {
        .Ok(_) -> panic("address injection accepted")
        .Err(_) -> print("address-rejected")
    }
    if email.message(~sender, [~recipient], [Address]{}, "hello\nBcc := stolen@example.com", "text", HTML{""}, [Attachment]{}) == {
        .Ok(_) -> panic("header injection accepted")
        .Err(_) -> print("header-rejected")
    }
    recipients := [~recipient]
    count := 1
    loop count < 101 {
        &recipients.push(~recipient)
        count += 1
    }
    if email.message(~sender, recipients, [Address]{}, "subject", "text", HTML{""}, [Attachment]{}) == {
        .Ok(_) -> panic("recipient bound ignored")
        .Err(_) -> print("recipient-bound")
    }
    too_large := "x".repeat(26214401).bytes()
    if email.attachment("large.bin", "application/octet-stream", too_large) == {
        .Ok(_) -> panic("attachment bound ignored")
        .Err(_) -> print("attachment-bound")
    }
    attachment :: email.attachment("notes.txt", "text/plain", [104, 105]) ?? panic("attachment")
    message :: email.message(sender, [recipient], [hidden], "Welcome ☕", "plain", HTML{"<b>html</b>"}, [attachment]) ?? panic("message")
    first :: email.serialize(~message) ?? panic("serialize")
    second :: email.serialize(message) ?? panic("serialize twice")
    print(first == second)
    print(first.len())
}

"#;
    let (code, stdout, stderr) = build_and_run(&dir, "email_mime", src, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.starts_with("address-rejected\nheader-rejected\nrecipient-bound\nattachment-bound\ntrue\n"), "{stdout}");
    let file = dir.join("email_mime.jet");
    fs::write(&file, src).unwrap();
    for (tier, use_interpreter) in [("default dev", false), ("forced interpreter", true)] {
        match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, use_interpreter) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "email MIME failed in {tier}: {stderr}");
                assert!(
                    stdout.starts_with(
                        "address-rejected\nheader-rejected\nrecipient-bound\nattachment-bound\ntrue\n"
                    ),
                    "{tier}: {stdout}"
                );
            }
            other => panic!("email MIME did not run in {tier}: {other:?}"),
        }
    }
}
