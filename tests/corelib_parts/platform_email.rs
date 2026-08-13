#[test]
fn core_email_address_and_mime_are_bounded_and_deterministic() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_email_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
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
    if email.message(~sender, [~recipient], [], "hello\nBcc := stolen@example.com", "text", "", []) == {
        .Ok(_) -> panic("header injection accepted")
        .Err(_) -> print("header-rejected")
    }
    recipients := [~recipient]
    count := 1
    loop count < 101 {
        recipients.push(~recipient)
        count++
    }
    if email.message(~sender, recipients, [], "subject", "text", "", []) == {
        .Ok(_) -> panic("recipient bound ignored")
        .Err(_) -> print("recipient-bound")
    }
    too_large := "x".repeat(26214401).bytes()
    if email.attachment("large.bin", "application/octet-stream", too_large) == {
        .Ok(_) -> panic("attachment bound ignored")
        .Err(_) -> print("attachment-bound")
    }
    attachment :: email.attachment("notes.txt", "text/plain", [104, 105]) ?? panic("attachment")
    message :: email.message(sender, [recipient], [hidden], "Welcome ☕", "plain", "<b>html</b>", [attachment]) ?? panic("message")
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
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(exit_code, 0, "email MIME failed in default dev: {stderr}");
            assert!(stdout.starts_with("address-rejected\nheader-rejected\nrecipient-bound\nattachment-bound\ntrue\n"), "{stdout}");
        }
        other => panic!("email MIME did not run in default dev: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}
