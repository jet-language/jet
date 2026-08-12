mod email_native {
    fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (index, byte) in data.iter().enumerate() {
            out[index % 32] = out[index % 32].wrapping_mul(31).wrapping_add(*byte);
        }
        out
    }
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../crates/jet-codegen/src/Prelude/CoreLib/Email.rs");
}

#[test]
fn core_email_mime_bytes_use_crlf_and_keep_bcc_envelope_only() {
    use email_native::jet_email;
    let from = jet_email::address(&"Mara ☕ <mara@example.com>".to_string()).unwrap();
    let to = jet_email::address(&"Ada <ada@example.net>".to_string()).unwrap();
    let bcc = jet_email::address(&"secret@example.org".to_string()).unwrap();
    let attachment = jet_email::attachment(
        &"notes.txt".to_string(),
        &"text/plain".to_string(),
        &b"hi".to_vec(),
    ).unwrap();
    let message = jet_email::message(
        &from, &vec![to], &vec![bcc], &"Welcome ☕".to_string(),
        &"plain".to_string(), &"<b>html</b>".to_string(), &vec![attachment],
    ).unwrap();
    let first = jet_email::serialize(&message).unwrap();
    let second = jet_email::serialize(&message).unwrap();
    assert_eq!(first, second);
    let wire = String::from_utf8(first).unwrap();
    assert!(wire.contains("Content-Type: multipart/mixed"));
    assert!(wire.contains("Content-Type: multipart/alternative"));
    assert!(wire.contains("aGk="));
    assert!(!wire.contains("Bcc:"));
    assert!(!wire.contains("secret@example.org"));
    assert!(!wire.replace("\r\n", "").contains('\n'));
    assert!(wire.split_inclusive("\r\n").all(|line| line.len() <= jet_email::MAX_HEADER_BYTES || !line.contains(':')));
}

#[test]
fn core_email_headers_and_wire_bounds_are_prospective() {
    use email_native::jet_email;
    let quoted = jet_email::address(&"\"Doe, \\\"Ada\\\"\" <ada@example.net>".to_string()).unwrap();
    assert!(jet_email::address(&"\"a@b\"@example.net".to_string()).is_ok());
    let unicode_name = format!("{} <mara@example.com>", "界".repeat(120));
    let from = jet_email::address(&unicode_name).unwrap();
    let message = jet_email::message(
        &from, &vec![quoted], &vec![], &"界".repeat(120),
        &"plain".to_string(), &String::new(), &vec![],
    ).unwrap();
    let wire = String::from_utf8(jet_email::serialize(&message).unwrap()).unwrap();
    assert!(wire.contains("\"Doe, \\\"Ada\\\"\" <ada@example.net>"));
    for word in wire.split_whitespace().filter(|word| word.starts_with("=?UTF-8?B?")) {
        assert!(word.len() <= 75, "encoded word too long: {}", word.len());
    }
    for line in wire.split_inclusive("\r\n") {
        if line.contains(':') || line.starts_with(' ') {
            assert!(line.len() <= jet_email::MAX_HEADER_BYTES, "header line too long: {}", line.len());
        }
    }

    let sender = jet_email::address(&"sender@example.com".to_string()).unwrap();
    let recipient = jet_email::address(&"recipient@example.com".to_string()).unwrap();
    let at_body_limit = "x".repeat(jet_email::MAX_BODY_BYTES);
    assert!(jet_email::message(
        &sender, &vec![recipient.clone()], &vec![], &"subject".to_string(),
        &at_body_limit, &String::new(), &vec![],
    ).is_ok());
    let over_body_limit = "x".repeat(jet_email::MAX_BODY_BYTES + 1);
    assert!(jet_email::message(
        &sender, &vec![recipient.clone()], &vec![], &"subject".to_string(),
        &over_body_limit, &String::new(), &vec![],
    ).is_err());

    let max_subject = "s".repeat(jet_email::MAX_HEADER_BYTES - "Subject: ".len() - 2);
    assert!(jet_email::message(
        &sender, &vec![recipient.clone()], &vec![], &max_subject,
        &"body".to_string(), &String::new(), &vec![],
    ).is_ok());
    assert!(jet_email::message(
        &sender, &vec![recipient], &vec![], &format!("{max_subject}s"),
        &"body".to_string(), &String::new(), &vec![],
    ).is_err());

    assert!(jet_email::attachment(
        &format!("{}.txt", "a".repeat(900)), &"application/octet-stream".to_string(), &vec![],
    ).is_ok());
    assert!(jet_email::attachment(
        &format!("{}.txt", "a".repeat(960)), &"application/octet-stream".to_string(), &vec![],
    ).is_err());

    let tiny = jet_email::attachment(
        &"tiny.bin".to_string(), &"application/octet-stream".to_string(), &vec![0],
    ).unwrap();
    assert!(jet_email::message(
        &sender, &vec![jet_email::address(&"to@example.com".to_string()).unwrap()], &vec![],
        &"subject".to_string(), &"body".to_string(), &String::new(),
        &vec![tiny; jet_email::MAX_ATTACHMENTS + 1],
    ).is_err());
}

#[test]
fn core_email_envelope_reports_and_errors_follow_smtp_law() {
    use email_native::jet_email;
    let from = jet_email::address(&"sender@example.com".to_string()).unwrap();
    let visible = jet_email::address(&"visible@example.net".to_string()).unwrap();
    let hidden = jet_email::address(&"hidden@example.org".to_string()).unwrap();
    let message = jet_email::message(
        &from, &vec![visible.clone()], &vec![hidden.clone()], &"subject".to_string(),
        &"body".to_string(), &String::new(), &vec![],
    ).unwrap();
    assert_eq!(message.envelope().recipients, vec![visible.clone(), hidden.clone()]);
    let replacement = jet_email::envelope(&from, &vec![hidden.clone()]).unwrap();
    let replaced = message.with_envelope(&replacement).unwrap();
    assert_eq!(replaced.envelope().recipients, vec![hidden]);
    assert!(!String::from_utf8(jet_email::serialize(&replaced).unwrap()).unwrap().contains("Bcc:"));
    let error = jet_email::envelope(&from, &vec![]).unwrap_err();
    match error {
        jet_email::Error::Configuration { operation, server, code, reason } => {
            assert_eq!(operation, "envelope");
            assert_eq!((server, code), (None, None));
            assert!(reason.contains("recipient"));
        }
        other => panic!("unexpected envelope error: {other:?}"),
    }
}

#[test]
fn core_email_smtp_config_limits_and_trust_follow_ratified_law() {
    use email_native::jet_email;
    let safe = jet_email::Limits::safe();
    assert_eq!(
        (
            safe.max_reply_line_bytes,
            safe.max_reply_lines,
            safe.max_capabilities,
            safe.max_recipients,
            safe.max_message_bytes,
            safe.max_auth_challenge_bytes,
        ),
        (512, 100, 100, 100, 33_554_432, 4096),
    );
    let mut invalid = safe.clone();
    invalid.max_reply_line_bytes = 63;
    invalid.max_reply_lines = 0;
    let first = invalid.validate().unwrap_err();
    assert!(matches!(first, jet_email::Error::Configuration { reason, .. }
        if reason.starts_with("max_reply_line_bytes")));

    let pem = b"-----BEGIN CERTIFICATE-----\nMAMCAQE=\n-----END CERTIFICATE-----\n".to_vec();
    let mut config: jet_email::SMTPConfig<()> = jet_email::SMTPConfig {
        host: "smtp.example.com".to_string(),
        port: 587,
        security: jet_email::SMTPSecurity::StartTls,
        auth: jet_email::SMTPAuth::None,
        recipient_policy: jet_email::RecipientPolicy::RequireAll,
        trust: jet_email::TLSTrust::SystemPlusCa { pem },
        limits: safe.clone(),
        dkim: Err(JetAbsent),
    };
    jet_email::validate_smtp_config(&config).unwrap();
    config.dkim = Ok(jet_email::DkimConfig {
        domain: "example.com".to_string(), selector: "login-2026".to_string(),
        private_key: (), signed_headers: vec!["subject".to_string()],
    });
    assert!(matches!(jet_email::validate_smtp_config(&config),
        Err(jet_email::Error::Configuration { reason, .. }) if reason.contains("include `from`")));
    config.dkim.as_mut().unwrap().signed_headers = vec!["from".to_string(), "FROM".to_string()];
    assert!(matches!(jet_email::validate_smtp_config(&config),
        Err(jet_email::Error::Configuration { reason, .. }) if reason.contains("duplicate")));
    config.dkim.as_mut().unwrap().signed_headers = vec!["from".to_string(), "received".to_string()];
    assert!(matches!(jet_email::validate_smtp_config(&config),
        Err(jet_email::Error::Configuration { reason, .. }) if reason.contains("hop header")));

    let malformed: jet_email::SMTPConfig<()> = jet_email::SMTPConfig {
        host: "smtp.example.com".to_string(),
        port: 465,
        security: jet_email::SMTPSecurity::TLS,
        auth: jet_email::SMTPAuth::None,
        recipient_policy: jet_email::RecipientPolicy::DeliverAccepted,
        trust: jet_email::TLSTrust::SystemPlusCa {
            pem: b"-----BEGIN PRIVATE KEY-----\nAA==\n-----END PRIVATE KEY-----".to_vec(),
        },
        limits: safe,
        dkim: Err(JetAbsent),
    };
    assert!(matches!(jet_email::validate_smtp_config(&malformed),
        Err(jet_email::Error::Configuration { reason, .. }) if reason.contains("certificate")));
}

#[test]
fn core_email_smtp_reply_parser_bounds_and_tls_auth_order_are_real() {
    use email_native::jet_email;
    let limits = jet_email::Limits::safe();
    let mut wire = &b"250-smtp.example\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n"[..];
    let reply = jet_email::read_smtp_reply(&mut wire, &limits).unwrap();
    let caps = jet_email::smtp_capabilities(&reply, &limits).unwrap();
    assert_eq!(caps.names, vec!["STARTTLS", "AUTH"]);

    let mut state = jet_email::SMTPState::new();
    state.greeting(&jet_email::SMTPReply { code: 220, lines: vec!["ready".to_string()] }).unwrap();
    let caps = state.ehlo(&reply, &limits).unwrap();
    assert!(state.authenticate("PLAIN", 10, &limits).is_err());
    state.start_tls(&caps).unwrap();
    state.verified_tls();
    assert!(state.authenticate("PLAIN", 10, &limits).is_err());
    state.ehlo(&reply, &limits).unwrap();
    state.authenticate("PLAIN", 10, &limits).unwrap();
    assert!(state.authenticate("PLAIN", 4097, &limits).is_err());

    let mut tight = limits;
    tight.max_reply_line_bytes = 64;
    let oversized = format!("250 {}\r\n", "x".repeat(61));
    assert!(jet_email::read_smtp_reply(&mut oversized.as_bytes(), &tight).is_err());
    let mut changed = &b"250-first\r\n251 final\r\n"[..];
    assert!(jet_email::read_smtp_reply(&mut changed, &tight).is_err());
}

#[test]
fn core_email_smtp_transaction_starttls_auth_rcpt_and_data_are_real() {
    use email_native::jet_email;
    use std::io::{Read, Write};

    struct Script {
        replies: std::io::Cursor<Vec<u8>>,
        writes: Vec<u8>,
        verified_tls: bool,
        upgrades: usize,
        closed: bool,
    }
    impl Read for Script {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> { self.replies.read(out) }
    }
    impl Write for Script {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    impl jet_email::SMTPTransport for Script {
        fn verified_tls(&self) -> bool { self.verified_tls }
        fn start_tls(&mut self, _server: &str, _trust: &jet_email::TLSTrust) -> Result<(), String> {
            self.verified_tls = true;
            self.upgrades += 1;
            Ok(())
        }
        fn close(&mut self) { self.closed = true; }
    }

    let sender = jet_email::address(&"sender@example.com".to_string()).unwrap();
    let accepted = jet_email::address(&"ok@example.net".to_string()).unwrap();
    let rejected = jet_email::address(&"no@example.org".to_string()).unwrap();
    let message = jet_email::message(
        &sender, &vec![accepted.clone(), rejected.clone()], &vec![], &"subject".to_string(),
        &"first\r\n.second".to_string(), &String::new(), &vec![],
    ).unwrap();
    let config = jet_email::SMTPConfig {
        host: "smtp.example.com".to_string(),
        port: 587,
        security: jet_email::SMTPSecurity::StartTls,
        auth: jet_email::SMTPAuth::Password {
            username: "mailer".to_string(),
            password: b"secret".to_vec(),
        },
        recipient_policy: jet_email::RecipientPolicy::DeliverAccepted,
        trust: jet_email::TLSTrust::System,
        limits: jet_email::Limits::safe(),
        dkim: Err(JetAbsent),
    };
    let replies = concat!(
        "220 relay ready\r\n",
        "250-relay\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n",
        "220 begin TLS\r\n",
        "250-relay\r\n250 AUTH PLAIN LOGIN\r\n",
        "235 authenticated\r\n",
        "250 sender accepted\r\n",
        "250 recipient accepted\r\n",
        "550 recipient rejected\r\n",
        "354 send data\r\n",
        "250 queued as q-1\r\n",
        "221 bye\r\n",
    );
    let mut transport = Script {
        replies: std::io::Cursor::new(replies.as_bytes().to_vec()),
        writes: Vec::new(), verified_tls: false, upgrades: 0, closed: false,
    };
    let report = jet_email::smtp_transaction(
        &mut transport, &config, &message, &jet_email::NoopSmtpControl,
    ).unwrap();
    assert_eq!((report.accepted.len(), report.rejected.len()), (1, 1));
    assert_eq!((report.response_code, report.response.as_str()), (250, "queued as q-1"));
    assert!(!report.accepted_at.is_empty());
    assert_eq!(transport.upgrades, 1);
    assert!(transport.closed);
    let wire = String::from_utf8(transport.writes).unwrap();
    assert!(wire.starts_with("EHLO localhost\r\nSTARTTLS\r\nEHLO localhost\r\nAUTH PLAIN "));
    assert!(!wire.contains("secret"));
    assert!(wire.contains("MAIL FROM:<sender@example.com>\r\n"));
    assert!(wire.contains("RCPT TO:<ok@example.net>\r\nRCPT TO:<no@example.org>\r\nDATA\r\n"));
    assert!(wire.contains("\r\n.\r\nQUIT\r\n"));
    assert_eq!(
        jet_email::smtp_dot_stuff(b"first\r\n.second\r\n").unwrap(),
        b"first\r\n..second\r\n.\r\n",
    );
}

#[test]
fn core_email_smtp_transaction_require_all_and_delivery_unknown_are_honest() {
    use email_native::jet_email;
    use std::io::{Read, Write};

    struct Script { replies: std::io::Cursor<Vec<u8>>, writes: Vec<u8>, closed: bool }
    impl Read for Script {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> { self.replies.read(out) }
    }
    impl Write for Script {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes.extend_from_slice(bytes); Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    impl jet_email::SMTPTransport for Script {
        fn verified_tls(&self) -> bool { true }
        fn start_tls(&mut self, _server: &str, _trust: &jet_email::TLSTrust) -> Result<(), String> {
            panic!("implicit TLS must not upgrade")
        }
        fn close(&mut self) { self.closed = true; }
    }
    struct Stop(jet_email::SMTPStop);
    impl jet_email::SMTPControl for Stop {
        fn checkpoint(&self, _operation: &str) -> Result<(), jet_email::SMTPStop> { Err(self.0) }
        fn accepted_at(&self) -> String { panic!("stopped transaction cannot be accepted") }
    }

    let sender = jet_email::address(&"sender@example.com".to_string()).unwrap();
    let recipient = jet_email::address(&"recipient@example.net".to_string()).unwrap();
    let message = jet_email::message(
        &sender, &vec![recipient], &vec![], &"subject".to_string(),
        &"body".to_string(), &String::new(), &vec![],
    ).unwrap();
    let config = |policy| jet_email::SMTPConfig {
        host: "smtp.example.com".to_string(), port: 465,
        security: jet_email::SMTPSecurity::TLS, auth: jet_email::SMTPAuth::None,
        recipient_policy: policy, trust: jet_email::TLSTrust::System,
        limits: jet_email::Limits::safe(),
        dkim: Err(JetAbsent),
    };

    for (stop, timed_out) in [
        (jet_email::SMTPStop::Cancelled, false),
        (jet_email::SMTPStop::TimedOut, true),
    ] {
        let mut stopped = Script {
            replies: std::io::Cursor::new(Vec::new()), writes: Vec::new(), closed: false,
        };
        let error = jet_email::smtp_transaction(
            &mut stopped, &config(jet_email::RecipientPolicy::RequireAll), &message, &Stop(stop),
        ).unwrap_err();
        assert_eq!(matches!(error, jet_email::Error::TimedOut { .. }), timed_out);
        assert_eq!(matches!(error, jet_email::Error::Cancelled { .. }), !timed_out);
        assert!(stopped.writes.is_empty());
        assert!(stopped.closed);
    }

    let reject_replies = concat!(
        "220 ready\r\n", "250 relay\r\n", "250 sender\r\n", "550 no\r\n",
    );
    let mut reject = Script {
        replies: std::io::Cursor::new(reject_replies.as_bytes().to_vec()),
        writes: Vec::new(), closed: false,
    };
    assert!(matches!(jet_email::smtp_transaction(
        &mut reject, &config(jet_email::RecipientPolicy::RequireAll), &message,
        &jet_email::NoopSmtpControl,
    ), Err(jet_email::Error::Rejected { code: Some(550), .. })));
    assert!(!String::from_utf8(reject.writes).unwrap().contains("DATA\r\n"));
    assert!(reject.closed);

    let unknown_replies = concat!(
        "220 ready\r\n", "250 relay\r\n", "250 sender\r\n", "250 recipient\r\n",
        "354 continue\r\n",
    );
    let mut unknown = Script {
        replies: std::io::Cursor::new(unknown_replies.as_bytes().to_vec()),
        writes: Vec::new(), closed: false,
    };
    assert!(matches!(jet_email::smtp_transaction(
        &mut unknown, &config(jet_email::RecipientPolicy::DeliverAccepted), &message,
        &jet_email::NoopSmtpControl,
    ), Err(jet_email::Error::DeliveryUnknown { operation, .. }) if operation == "data_response"));
    assert_eq!(String::from_utf8(unknown.writes).unwrap().matches("DATA\r\n").count(), 1);
    assert!(unknown.closed);
}

#[test]
fn core_email_limits_are_constructible_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_email_limits_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "email_limits", r#"
use core.email as email

fn run() {
    safe := email.Limits.safe()
    print("{safe.max_reply_line_bytes},{safe.max_reply_lines},{safe.max_capabilities},{safe.max_recipients},{safe.max_message_bytes},{safe.max_auth_challenge_bytes}")
    strict := email.Limits.{
        max_reply_line_bytes: 64,
        max_reply_lines: 1,
        max_capabilities: 2,
        max_recipients: 3,
        max_message_bytes: 4,
        max_auth_challenge_bytes: 5,
    }
    print("{strict.max_reply_line_bytes},{strict.max_auth_challenge_bytes}")
}
"#, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "512,100,100,100,33554432,4096\n64,5\n");
}

#[test]
fn core_email_mailer_surface_constructs_with_real_secret() {
    let dir = std::env::temp_dir().join(format!("jet_email_mailer_surface_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "email_mailer_surface", r#"
use core.email as email
use core.crypto as crypto

fn run() {
    password :: crypto.Secret.from_text("not-logged")
    dkim_key :: crypto.Secret.from_text("0123456789abcdef0123456789abcdef")
    dkim :: email.DkimConfig.{
        domain: "example.com",
        selector: "login-2026",
        private_key: dkim_key,
        signed_headers: ["from", "subject", "mime-version", "content-type"],
    }
    auth :: SMTPAuth.{ .Password.{ username: "mailer", password: password } }
    config :: email.SMTPConfig.{
        host: "localhost",
        port: 465,
        security: .TLS,
        auth: auth,
        recipient_policy: .RequireAll,
        trust: .System,
        limits: email.Limits.safe(),
        dkim: Val(dkim),
    }
    mailer :: email.smtp(config) ?? panic("mailer config")
    env_mailer :: email.smtp_from_env() ?? panic("environment mailer config")
    sender :: email.address("sender@example.com") ?? panic("sender")
    recipient :: email.address("recipient@example.net") ?? panic("recipient")
    message :: email.message(sender, [recipient], [], "subject", "body", "", []) ?? panic("message")
    if false {
        report :: mailer.send(message) ?? panic("send")
        print(report.response_code)
    }
    print("mailer-ready")
}
"#, &[("SMTP_HOST", "localhost"), ("SMTP_SECURITY", "tls"), ("SMTP_PORT", "465")], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "mailer-ready\n");
    let _ = fs::remove_dir_all(&dir);
}

