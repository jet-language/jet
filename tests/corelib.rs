#[allow(unused_imports)]
use jet_foundation::Outcome::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

mod common;

mod dns_resolver_policy {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Top/DNSResolverPolicy.rs");

    pub fn resolv_conf(text: &str) -> Vec<String> {
        jet_net_dns_parse_resolv_conf(text)
    }

    pub fn scutil(text: &str) -> Vec<String> {
        jet_net_dns_parse_scutil(text)
    }

    pub fn windows(text: &str) -> Vec<String> {
        jet_net_dns_parse_windows_addresses(text)
    }
}

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
    include!("../crates/jet-codegen/src/Prelude/CoreLib/Email.rs");
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
    dkim := email.DkimConfig.{
        domain: "example.com",
        selector: "login-2026",
        private_key: dkim_key,
        signed_headers: ["from", "subject", "mime-version", "content-type"],
    }
    auth := email.SMTPAuth.Password.{ username: "mailer", password: password }
    config := email.SMTPConfig.{
        host: "localhost",
        port: 465,
        security: .TLS,
        auth: auth,
        recipient_policy: .RequireAll,
        trust: .System,
        limits: email.Limits.safe(),
        dkim: Val(dkim),
    }
    mailer := email.smtp(config) ?? panic("mailer config")
    env_mailer := email.smtp_from_env() ?? panic("environment mailer config")
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

#[test]
fn encoding_stream_foundation_types_are_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_encoding_foundation_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("foundation.json");
    let bad_path = dir.join("bad-limits.json");
    let path_text = path.to_string_lossy().replace('\\', "\\\\");
    let bad_text = bad_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.csv as csv
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.files as files

fn keep_error(v: ^encoding.EncodingError) => encoding.EncodingError {{ return v }}
fn keep_cause(v: ^encoding.EncodingCause) => encoding.EncodingCause {{ return v }}
fn keep_event(v: ^encoding.DataEvent) => encoding.DataEvent {{ return v }}
fn keep_format(v: ^encoding.EncodingFormat) => encoding.EncodingFormat {{ return v }}
fn keep_kind(v: ^encoding.EncodingErrorKind) => encoding.EncodingErrorKind {{ return v }}
fn keep_json_reader(v: ^json.JSONReader) => json.JSONReader {{ return v }}
fn keep_json_writer(v: ^json.JSONWriter) => json.JSONWriter {{ return v }}
fn keep_jsonl_reader(v: ^jsonl.JSONLReader) => jsonl.JSONLReader {{ return v }}
fn keep_jsonl_writer(v: ^jsonl.JSONLWriter) => jsonl.JSONLWriter {{ return v }}
fn keep_csv_reader(v: ^csv.CSVReader) => csv.CSVReader {{ return v }}
fn keep_csv_writer(v: ^csv.CSVWriter) => csv.CSVWriter {{ return v }}
fn keep_xml_reader(v: ^xml.XMLReader) => xml.XMLReader {{ return v }}
fn keep_xml_writer(v: ^xml.XMLWriter) => xml.XMLWriter {{ return v }}
fn keep_cbor_reader(v: ^cbor.CBORReader) => cbor.CBORReader {{ return v }}
fn keep_cbor_writer(v: ^cbor.CBORWriter) => cbor.CBORWriter {{ return v }}

fn run() {{
    limits := encoding.EncodingLimits.safe()
    print("{{limits.buffer_bytes}}:{{limits.max_depth}}:{{limits.max_item_bytes}}:{{limits.max_expansion_depth}}:{{limits.max_expansion_bytes}}")
    if limits.max_total_bytes == None {{ print(true) }} else {{ print(false) }}
    print(keep_format(^encoding.EncodingFormat.JSON) == encoding.EncodingFormat.JSON)
    print(keep_kind(^encoding.EncodingErrorKind.Limit) == encoding.EncodingErrorKind.Limit)

    cause := encoding.EncodingCause.{{ kind: "io", os_code: None, message: "nope" }}
    kept_cause := keep_cause(^cause)
    print(kept_cause.kind)
    print(kept_cause.message)

    err := encoding.EncodingError.{{
        format: encoding.EncodingFormat.JSON,
        kind: encoding.EncodingErrorKind.Limit,
        byte_offset: 0,
        line: None,
        column: None,
        path: "",
        reason: "buffer_bytes 1 is outside 4096..16777216",
        cause: None,
    }}
    kept_err := keep_error(^err)
    print(kept_err.format == encoding.EncodingFormat.JSON)
    print(kept_err.kind == encoding.EncodingErrorKind.Limit)
    print(kept_err.byte_offset)
    if kept_err.cause == None {{ print(true) }} else {{ print(false) }}
    print("{{kept_err}}")

    event := encoding.DataEvent.Null
    kept_event := keep_event(^event)
    print(true)

    output :: files.create("{path_text}") ?? panic("create")
    writer :: json.writer(^output, limits, false) ?? panic("writer")
    kept_writer := keep_json_writer(^writer)
    kept_writer.write(encoding.DataEvent.Null) ?? panic("write")
    kept_writer.finish() ?? panic("finish")

    bad := encoding.EncodingLimits.safe()
    bad.buffer_bytes = 1
    bad_output :: files.create("{bad_text}") ?? panic("bad create")
    if json.writer(^bad_output, bad, false) == {{
        .Ok(_) -> {{ print("limits-missed") }}
        .Err(reject) -> {{
            print("{{reject}}")
            print(reject.format == encoding.EncodingFormat.JSON)
            print(reject.kind == encoding.EncodingErrorKind.Limit)
            print(reject.reason)
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "encoding_foundation", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        concat!(
            "65536:256:16777216:32:8388608\n",
            "true\n",
            "true\n",
            "true\n",
            "io\n",
            "nope\n",
            "true\n",
            "true\n",
            "0\n",
            "true\n",
            "JSON Limit at byte 0: buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "JSON Limit at byte 0, line 1, column 1: buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "true\n",
            "buffer_bytes 1 is outside 4096..16777216\n",
        )
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "null");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_reader_writer_are_real_incremental_handles() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("events.json");
    let path_text = path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits :: encoding.EncodingLimits.safe()
    output :: files.create("{path_text}") ?? panic("create")
    writer :: json.writer(^output, limits, false) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("write")
    writer.write(encoding.DataEvent.Key("message")) ?? panic("write")
    writer.write(encoding.DataEvent.Text("hi ☺")) ?? panic("write")
    writer.write(encoding.DataEvent.Key("values")) ?? panic("write")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("write")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("write")
    writer.write(encoding.DataEvent.Bool(true)) ?? panic("write")
    writer.write(encoding.DataEvent.Null) ?? panic("write")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("write")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("write")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish")

    input :: files.open("{path_text}") ?? panic("open")
    reader :: json.reader(^input, encoding.EncodingLimits.safe()) ?? panic("reader")
    count := 0
    loop count < 11 {{
        maybe_event :: reader.next() ?? panic("next")
        if maybe_event == None {{ print("eof") }} else {{ print("event") }}
        count++
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "event\nevent\nevent\nevent\nevent\nevent\nevent\nevent\nevent\nevent\neof\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"message":"hi ☺","values":[7,true,null]}"#);
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_defaults_paths_limits_and_terminal_errors_are_stable() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_hostile_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("hostile.json");
    let default_path = dir.join("default.json");
    let limited_path = dir.join("limited.json");
    fs::write(&input_path, r#"{"o":[0,{"i":"\u263a"}]}"#).unwrap();
    let input_text = input_path.to_string_lossy().replace('\\', "\\\\");
    let default_text = default_path.to_string_lossy().replace('\\', "\\\\");
    let limited_text = limited_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    default_output :: files.create("{default_text}") ?? panic("create")
    default_writer :: json.writer(^default_output) ?? panic("default writer")
    default_writer.write(encoding.DataEvent.Null) ?? panic("default write")
    default_writer.finish() ?? panic("default finish")
    default_input :: files.open("{default_text}") ?? panic("default open")
    default_reader :: json.reader(^default_input) ?? panic("default reader")
    if default_reader.next() == {{
        .Ok(_) -> {{ print(true) }}
        .Err(_) -> {{ print(false) }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 2
    input :: files.open("{input_text}") ?? panic("open")
    reader :: json.reader(^input, limits) ?? panic("reader")
    count := 0
    loop count < 8 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("reader-not-latched") }}
                    .Err(second) -> {{
                        print(first.path)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}

    finished_output :: files.create("{limited_text}") ?? panic("create")
    finished_writer :: json.writer(^finished_output) ?? panic("writer")
    finished_writer.write(encoding.DataEvent.Null) ?? panic("write")
    finished_writer.finish() ?? panic("finish")
    after_finish :: finished_writer.write(encoding.DataEvent.Null)
    if after_finish == {{
        .Ok(_) -> {{ print("finish-missed") }}
        .Err(first) -> {{
            after_flush :: finished_writer.flush()
            if after_flush == {{
                .Ok(_) -> {{ print("finish-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    escaped_limits := encoding.EncodingLimits.safe()
    escaped_limits.max_item_bytes = 1
    escaped_output :: files.create("{limited_text}") ?? panic("create")
    escaped_writer :: json.writer(^escaped_output, escaped_limits) ?? panic("writer")
    escaped_result :: escaped_writer.write(encoding.DataEvent.Text("\n"))
    if escaped_result == {{
        .Ok(_) -> {{ print("escape-missed") }}
        .Err(first) -> {{
            escaped_again :: escaped_writer.finish()
            if escaped_again == {{
                .Ok(_) -> {{ print("escape-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_hostile", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\n$[\"o\"][1][\"i\"]\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read_to_string(&default_path).unwrap(), "null");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_rejects_whole_events_and_records_before_partial_output() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_atomic_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let text_path = dir.join("text.json");
    let key_path = dir.join("key.json");
    let depth_path = dir.join("depth.json");
    let jsonl_path = dir.join("record.jsonl");
    let nonfinite_path = dir.join("nonfinite.jsonl");
    let text = text_path.to_string_lossy().replace('\\', "\\\\");
    let key = key_path.to_string_lossy().replace('\\', "\\\\");
    let depth = depth_path.to_string_lossy().replace('\\', "\\\\");
    let jsonl = jsonl_path.to_string_lossy().replace('\\', "\\\\");
    let nonfinite = nonfinite_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    text_limits := encoding.EncodingLimits.safe()
    text_limits.max_total_bytes = Val(5)
    text_output :: files.create("{text}") ?? panic("create text")
    text_writer :: json.writer(^text_output, text_limits) ?? panic("text writer")
    text_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    text_error :: text_writer.write(encoding.DataEvent.Text("abcd"))
    if text_error == {{
        .Ok(_) -> {{ print("text-limit-missed") }}
        .Err(first) -> {{
            again :: text_writer.finish()
            if again == {{
                .Ok(_) -> {{ print("text-terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}

    key_limits := encoding.EncodingLimits.safe()
    key_limits.max_total_bytes = Val(5)
    key_output :: files.create("{key}") ?? panic("create key")
    key_writer :: json.writer(^key_output, key_limits) ?? panic("key writer")
    key_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    key_result :: key_writer.write(encoding.DataEvent.Key("abc"))
    if key_result == {{ .Ok(_) -> {{ print("key-limit-missed") }} .Err(_) -> {{ print(true) }} }}

    depth_limits := encoding.EncodingLimits.safe()
    depth_limits.max_depth = 1
    depth_output :: files.create("{depth}") ?? panic("create depth")
    depth_writer :: json.writer(^depth_output, depth_limits) ?? panic("depth writer")
    depth_writer.write(encoding.DataEvent.ArrayStart) ?? panic("outer")
    depth_result :: depth_writer.write(encoding.DataEvent.ArrayStart)
    if depth_result == {{ .Ok(_) -> {{ print("depth-limit-missed") }} .Err(_) -> {{ print(true) }} }}

    record_limits := encoding.EncodingLimits.safe()
    record_limits.max_total_bytes = Val(5)
    record_output :: files.create("{jsonl}") ?? panic("create record")
    record_writer :: jsonl.writer(^record_output, record_limits) ?? panic("record writer")
    record_result :: record_writer.write(DataTree.Array([DataTree.Int(1), DataTree.Text("abcd")]))
    if record_result == {{
        .Ok(_) -> {{ print("record-limit-missed") }}
        .Err(first) -> {{
            again :: record_writer.flush()
            if again == {{ .Ok(_) -> {{ print("record-terminal-missed") }} .Err(second) -> {{ print(first.reason == second.reason) }} }}
        }}
    }}

    nonfinite_output :: files.create("{nonfinite}") ?? panic("create nonfinite")
    nonfinite_writer :: jsonl.writer(^nonfinite_output) ?? panic("nonfinite writer")
    nonfinite_result :: nonfinite_writer.write(DataTree.Array([DataTree.Int(1), DataTree.Float(0.0 / 0.0)]))
    if nonfinite_result == {{ .Ok(_) -> {{ print("nonfinite-missed") }} .Err(_) -> {{ print(true) }} }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_atomic", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read_to_string(&text_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "{");
    assert_eq!(fs::read_to_string(&depth_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&jsonl_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&nonfinite_path).unwrap(), "");
    assert_eq!(stderr, "");
    let dev_path = dir.join("json_stream_atomic.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSON stream atomic default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&text_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&key_path).unwrap(), "{");
    assert_eq!(fs::read_to_string(&depth_path).unwrap(), "[");
    assert_eq!(fs::read_to_string(&jsonl_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&nonfinite_path).unwrap(), "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_canonical_stream_sorts_nested_objects_and_latches_rejections() {
    let dir = std::env::temp_dir().join(format!("jet_json_canonical_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output_path = dir.join("canonical.json");
    let duplicate_path = dir.join("duplicate.json");
    let limited_path = dir.join("limited.json");
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let duplicate = duplicate_path.to_string_lossy().replace('\\', "\\\\");
    let limited = limited_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    writer.write(encoding.DataEvent.Key("z")) ?? panic("key")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("nested object")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(2)) ?? panic("int")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Text("x")) ?? panic("text")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("nested end")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Bool(true)) ?? panic("bool")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")

    data := DataTree.Object([
        "z": DataTree.Array([DataTree.Int(1), DataTree.Object(["b": DataTree.Int(2), "a": DataTree.Text("x")])]),
        "a": DataTree.Bool(true),
    ])
    print(json.canonical(data) ?? panic("value is not canonical JSON"))

    duplicate_output :: files.create("{duplicate}") ?? panic("duplicate create")
    duplicate_writer :: json.writer(^duplicate_output, encoding.EncodingLimits.safe(), true) ?? panic("duplicate writer")
    duplicate_writer.write(encoding.DataEvent.ObjectStart) ?? panic("duplicate object")
    duplicate_writer.write(encoding.DataEvent.Key("same")) ?? panic("first key")
    duplicate_writer.write(encoding.DataEvent.Int(1)) ?? panic("first value")
    duplicate_result :: duplicate_writer.write(encoding.DataEvent.Key("same"))
    if duplicate_result == {{
        .Ok(_) -> {{ print("duplicate-missed") }}
        .Err(first) -> {{
            again :: duplicate_writer.finish()
            if again == {{
                .Ok(_) -> {{ print("terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 8
    limited_output :: files.create("{limited}") ?? panic("limited create")
    limited_writer :: json.writer(^limited_output, limits, true) ?? panic("limited writer")
    limited_writer.write(encoding.DataEvent.ObjectStart) ?? panic("limited object")
    limited_writer.write(encoding.DataEvent.Key("long")) ?? panic("limited key")
    limited_writer.write(encoding.DataEvent.Text("value")) ?? panic("limited value")
    limited_result :: limited_writer.write(encoding.DataEvent.ObjectEnd)
    if limited_result == {{
        .Ok(_) -> {{ print("limit-missed") }}
        .Err(first) -> {{
            again :: limited_writer.flush()
            if again == {{
                .Ok(_) -> {{ print("limit-terminal-missed") }}
                .Err(second) -> {{ print(first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_canonical_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    let expected = r#"{"a":true,"z":[1,{"a":"x","b":2}]}"#;
    assert_eq!(stdout, format!("{expected}\ntrue\ntrue\n"));
    assert_eq!(fs::read_to_string(&output_path).unwrap(), expected);
    assert_eq!(fs::read_to_string(&duplicate_path).unwrap(), "");
    assert_eq!(fs::read_to_string(&limited_path).unwrap(), "");
    assert_eq!(stderr, "");
    // No quick-run (default `jet run`) leg here: `core.files.create` isn't
    // supported by the shared deopt/interpreter ambient evaluator yet
    // (E0956, card #1583) — matches the AOT-only pattern already used by
    // `json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain`
    // just below, the sibling file-IO-heavy stream test.
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain() {
    let dir = std::env::temp_dir().join(format!("jet_json_jcs_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let canonical_path = dir.join("canonical.json");
    let int_path = dir.join("int.json");
    let bytes_path = dir.join("bytes.json");
    let nonfinite_path = dir.join("nonfinite.json");
    let duplicate_path = dir.join("duplicate.json");
    let path = |path: &std::path::Path| path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    output :: files.create("{}") ?? panic("create canonical")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer canonical")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("object")
    writer.write(encoding.DataEvent.Key("𐀀")) ?? panic("astral key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("astral value")
    writer.write(encoding.DataEvent.Key("")) ?? panic("bmp key")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Float(1e30)) ?? panic("positive exponent")
    writer.write(encoding.DataEvent.Float(1e20)) ?? panic("decimal cutover")
    writer.write(encoding.DataEvent.Float(1e-7)) ?? panic("negative exponent")
    writer.write(encoding.DataEvent.Float(-0.0)) ?? panic("negative zero")
    writer.write(encoding.DataEvent.Int(9007199254740992)) ?? panic("exact Int boundary")
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    writer.finish() ?? panic("finish")

    int_output :: files.create("{}") ?? panic("create int")
    int_writer :: json.writer(^int_output, encoding.EncodingLimits.safe(), true) ?? panic("int writer")
    if int_writer.write(encoding.DataEvent.Int(9007199254740993)) == {{
        .Ok(_) -> print("int accepted")
        .Err(error) -> print(error.reason)
    }}

    bytes_output :: files.create("{}") ?? panic("create bytes")
    bytes_writer :: json.writer(^bytes_output, encoding.EncodingLimits.safe(), true) ?? panic("bytes writer")
    bytes :: [U8].{{ U8.from_int(1) ?? panic("byte") }}
    if bytes_writer.write(encoding.DataEvent.Bytes(bytes)) == {{
        .Ok(_) -> print("bytes accepted")
        .Err(error) -> print(error.reason)
    }}

    nonfinite_output :: files.create("{}") ?? panic("create nonfinite")
    nonfinite_writer :: json.writer(^nonfinite_output, encoding.EncodingLimits.safe(), true) ?? panic("nonfinite writer")
    if nonfinite_writer.write(encoding.DataEvent.Float(0.0 / 0.0)) == {{
        .Ok(_) -> print("nonfinite accepted")
        .Err(error) -> print(error.reason)
    }}

    duplicate_output :: files.create("{}") ?? panic("create duplicate")
    duplicate_writer :: json.writer(^duplicate_output, encoding.EncodingLimits.safe(), true) ?? panic("duplicate writer")
    duplicate_writer.write(encoding.DataEvent.ObjectStart) ?? panic("duplicate object")
    duplicate_writer.write(encoding.DataEvent.Key("same")) ?? panic("first key")
    duplicate_writer.write(encoding.DataEvent.Null) ?? panic("first value")
    if duplicate_writer.write(encoding.DataEvent.Key("same")) == {{
        .Ok(_) -> print("duplicate accepted")
        .Err(error) -> print(error.reason)
    }}
}}
"#,
        path(&canonical_path),
        path(&int_path),
        path(&bytes_path),
        path(&nonfinite_path),
        path(&duplicate_path),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_jcs_stream", &source, &[], None);
    assert_eq!(code, 0, "RFC 8785 stream test failed: {stderr}");
    assert_eq!(
        fs::read_to_string(&canonical_path).unwrap(),
        "{\"𐀀\":1,\"\":[1e+30,100000000000000000000,1e-7,0,9007199254740992]}"
    );
    assert_eq!(
        stdout,
        "JCS requires Int exactly representable as IEEE 754 binary64; encode this integer as Text\nJSON cannot encode Bytes; encode bytes as Text explicitly\nJCS cannot encode a non-finite Float\nJCS requires unique object keys\n"
    );
    for rejected in [&int_path, &bytes_path, &nonfinite_path, &duplicate_path] {
        assert_eq!(fs::read(rejected).unwrap(), b"");
    }
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rfc8785_corpus_manifest_hashes_and_provenance_are_pinned() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/encoding/rfc8785");
    let manifest = fs::read_to_string(root.join("MANIFEST.tsv")).unwrap();
    let mut count = 0;
    for line in manifest.lines().filter(|line| !line.starts_with('#') && !line.is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "bad corpus manifest row: {line}");
        let bytes = fs::read(root.join(fields[0])).unwrap();
        assert_eq!(jet::SHA256::sha256_hex(&bytes), fields[1], "hash drift: {}", fields[0]);
        assert!(fields[2].starts_with("https://www.rfc-editor.org/rfc/rfc8785.html#"));
        assert_eq!(fields[3], "IETF-Trust-Legal-Provisions");
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn json_canonical_stream_matches_every_finite_rfc8785_appendix_b_vector() {
    if !common::have_rustc() {
        eprintln!("note: skipping RFC 8785 Appendix B stream corpus (need rustc)");
        return;
    }
    let corpus = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/encoding/rfc8785/appendix-b.tsv"),
    )
    .unwrap();
    let cases = corpus
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (bits, expected) = line.split_once('\t').unwrap();
            (u64::from_str_radix(bits, 16).unwrap() as i64, expected)
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 24);
    let writes = cases
        .iter()
        .map(|(bits, _)| {
            let bits = if *bits == i64::MIN {
                "(-9223372036854775807 - 1)".to_string()
            } else {
                bits.to_string()
            };
            format!(
                "    writer.write(encoding.DataEvent.Float(math.from_bits({bits}))) ?? panic(\"Appendix B value\")"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = format!(
        "[{}]",
        cases.iter().map(|(_, expected)| *expected).collect::<Vec<_>>().join(",")
    );
    let dir = std::env::temp_dir().join(format!("jet_json_jcs_appendix_b_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("appendix-b.json");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files
use core.math as math

fn run() {{
    output :: files.create("{}") ?? panic("create")
    writer :: json.writer(^output, encoding.EncodingLimits.safe(), true) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
{}
    writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    writer.finish() ?? panic("finish")
}}
"#,
        output.to_string_lossy().replace('\\', "\\\\"),
        writes,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_jcs_appendix_b", &source, &[], None);
    assert_eq!(code, 0, "RFC 8785 Appendix B corpus failed: {stderr}");
    assert_eq!(stdout, "");
    assert_eq!(fs::read_to_string(&output).unwrap(), expected);
    // No quick-run (default `jet run`) leg here: `core.files.create` isn't
    // supported by the shared deopt/interpreter ambient evaluator yet
    // (E0956, card #1583) — matches the AOT-only pattern already used by
    // `json_canonical_stream_matches_rfc8785_numbers_key_order_and_domain`,
    // the sibling file-IO-heavy stream test.
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_stream_records_are_incremental_bounded_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output_path = dir.join("out.jsonl");
    let limited_path = dir.join("limited.jsonl");
    let input_path = dir.join("input.jsonl");
    let malformed_path = dir.join("malformed.jsonl");
    fs::write(&input_path, "\r\n  \r\n\"first\"\r\n[2,\"second\"]\n").unwrap();
    fs::write(&malformed_path, "{\"ok\":1}\n{\"bad\":[2,]}\n").unwrap();
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let limited = limited_path.to_string_lossy().replace('\\', "\\\\");
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("alpha")) ?? panic("write")
    writer.write(DataTree.Array([DataTree.Int(1), DataTree.Text("beta")])) ?? panic("write")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    after_finish :: writer.write(DataTree.Null)
    if after_finish == {{
        .Ok(_) -> {{ print("write-after-finish-missed") }}
        .Err(first) -> {{
            after_terminal :: writer.flush()
            if after_terminal == {{
                .Ok(_) -> {{ print("terminal-not-latched") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    input :: files.open("{input}") ?? panic("open")
    reader :: jsonl.reader(^input) ?? panic("reader")
    first_result :: reader.next() ?? panic("first")
    if first_result == {{
        Val(value) -> {{ print(value.text() ?? "bad") }}
        None -> {{ print("missing-first") }}
    }}
    second_result :: reader.next() ?? panic("second")
    if second_result == {{
        Val(value) -> {{
            first :: value.at(0) ?? DataTree.Int(-1)
            second :: value.at(1) ?? DataTree.Text("bad")
            print(first.int() ?? -1)
            print(second.text() ?? "bad")
        }}
        None -> {{ print("missing-second") }}
    }}
    eof_result :: reader.next() ?? panic("eof")
    if eof_result == None {{ print("eof") }} else {{ print("bad-eof") }}
    eof_again :: reader.next() ?? panic("eof again")
    if eof_again == None {{ print("eof-again") }} else {{ print("bad-eof-again") }}

    malformed_input :: files.open("{malformed}") ?? panic("open malformed")
    malformed_reader :: jsonl.reader(^malformed_input) ?? panic("reader malformed")
    first_malformed :: malformed_reader.next() ?? panic("first malformed record")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ print("malformed-missed") }}
        .Err(first) -> {{
            malformed_again :: malformed_reader.next()
            if malformed_again == {{
                .Ok(_) -> {{ print("malformed-not-latched") }}
                .Err(second) -> {{
                    print(first.line ?? -1)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    limits := encoding.EncodingLimits.safe()
    limits.max_item_bytes = 2
    limited_output :: files.create("{limited}") ?? panic("limited create")
    limited_writer :: jsonl.writer(^limited_output, limits) ?? panic("limited writer")
    limited_result :: limited_writer.write(DataTree.Text("three"))
    if limited_result == {{
        .Ok(_) -> {{ print("limit-missed") }}
        .Err(first) -> {{
            limited_again :: limited_writer.finish()
            if limited_again == {{
                .Ok(_) -> {{ print("limit-not-latched") }}
                .Err(second) -> {{
                    print(first.byte_offset == second.byte_offset && first.reason == second.reason)
                }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\nfirst\n2\nsecond\neof\neof-again\n2\n$[1][\"bad\"][1]\ntrue\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "\"alpha\"\n[1,\"beta\"]\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_fold_heap_budget_rejects_growth_before_large_record_allocation() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let array_path = dir.join("array.jsonl");
    let object_path = dir.join("object.jsonl");
    let valid_path = dir.join("valid.jsonl");
    let scalar_path = dir.join("scalar.jsonl");
    let near_string_path = dir.join("near-string.jsonl");
    let array = format!("[{}]\n", std::iter::repeat("0").take(256).collect::<Vec<_>>().join(","));
    let object = format!(
        "{{{}}}\n",
        (0..256)
            .map(|index| format!(r#""key{index:04}":"""#))
            .collect::<Vec<_>>()
            .join(",")
    );
    fs::write(&array_path, array).unwrap();
    fs::write(&object_path, object).unwrap();
    fs::write(&valid_path, format!("[{}]\n", std::iter::repeat("0").take(32).collect::<Vec<_>>().join(","))).unwrap();
    fs::write(&scalar_path, "1\n").unwrap();
    fs::write(&near_string_path, format!("{{\"{}\":0}}\n", "k".repeat(100_000))).unwrap();
    let array_path = array_path.to_string_lossy().replace('\\', "\\\\");
    let object_path = object_path.to_string_lossy().replace('\\', "\\\\");
    let valid_path = valid_path.to_string_lossy().replace('\\', "\\\\");
    let scalar_path = scalar_path.to_string_lossy().replace('\\', "\\\\");
    let near_string_path = near_string_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn run() {{
    near_limits := encoding.EncodingLimits.safe()
    near_limits.buffer_bytes = 4096
    near_limits.max_depth = 1
    near_limits.max_item_bytes = 100000
    near_limits.max_expansion_bytes = 0
    near_input :: files.open("{near_string_path}") ?? panic("near string open")
    near_reader :: jsonl.reader(^near_input, near_limits) ?? panic("near string reader")
    near_result :: near_reader.next()
    if near_result == {{
        .Ok(_) -> {{ print("near-string-limit-missed") }}
        .Err(first) -> {{
            near_again :: near_reader.next()
            if near_again == {{
                .Ok(_) -> {{ print("near-string-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset == 100003)
                    print(first.path)
                    print(first.reason)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    valid_limits := encoding.EncodingLimits.safe()
    valid_limits.max_item_bytes = 512
    valid_input :: files.open("{valid_path}") ?? panic("valid open")
    valid_reader :: jsonl.reader(^valid_input, valid_limits) ?? panic("valid reader")
    valid_record :: valid_reader.next() ?? panic("valid next")
    if valid_record == {{
        Val(value) -> {{ last :: value.at(31) ?? DataTree.Int(-1); print(last.int() ?? -1) }}
        None -> {{ print("valid-missing") }}
    }}

    scalar_limits := encoding.EncodingLimits.safe()
    scalar_limits.max_item_bytes = 7
    scalar_input :: files.open("{scalar_path}") ?? panic("scalar open")
    scalar_reader :: jsonl.reader(^scalar_input, scalar_limits) ?? panic("scalar reader")
    scalar_result :: scalar_reader.next()
    if scalar_result == {{
        .Ok(_) -> {{ print("scalar-limit-missed") }}
        .Err(first) -> {{
            scalar_again :: scalar_reader.next()
            if scalar_again == {{
                .Ok(_) -> {{ print("scalar-terminal-missed") }}
                .Err(second) -> {{ print(first.path); print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason) }}
            }}
        }}
    }}

    array_limits := encoding.EncodingLimits.safe()
    array_limits.max_item_bytes = 512
    array_input :: files.open("{array_path}") ?? panic("array open")
    array_reader :: jsonl.reader(^array_input, array_limits) ?? panic("array reader")
    array_result :: array_reader.next()
    if array_result == {{
        .Ok(_) -> {{ print("array-limit-missed") }}
        .Err(first) -> {{
            array_again :: array_reader.next()
            if array_again == {{
                .Ok(_) -> {{ print("array-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset < 256)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}

    object_limits := encoding.EncodingLimits.safe()
    object_limits.max_item_bytes = 512
    object_input :: files.open("{object_path}") ?? panic("object open")
    object_reader :: jsonl.reader(^object_input, object_limits) ?? panic("object reader")
    object_result :: object_reader.next()
    if object_result == {{
        .Ok(_) -> {{ print("object-limit-missed") }}
        .Err(first) -> {{
            object_again :: object_reader.next()
            if object_again == {{
                .Ok(_) -> {{ print("object-terminal-missed") }}
                .Err(second) -> {{
                    print(first.byte_offset < 2048)
                    print(first.path)
                    print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\n$[0]\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n0\n$[0]\ntrue\ntrue\n$[0][63]\ntrue\ntrue\n$[0][\"key0073\"]\ntrue\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_json_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.json");
    let heap_path = dir.join("heap.json");
    let key = "k".repeat(100_000);
    fs::write(&heap_path, format!("{{\"{key}\":0}}")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: json.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("int")
    writer.flush() ?? panic("flush")
    // no finish — Drop must close the handle without claiming success
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: unfinished bytes still on this path.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "[7")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: json.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(encoding.DataEvent.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "null")

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 100000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: json.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "json_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\n100003\n$\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null");
    assert_eq!(stderr, "");
    let dev_path = dir.join("json_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSON stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn json_stream_number_token_stays_under_counting_allocator_ceiling() {
    if !common::have_rustc() {
        eprintln!("note: skipping JSON counting-allocator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_counted_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("number.json");
    fs::write(&input_path, format!("1{}", "0".repeat(149_999))).unwrap();
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let malformed_path = dir.join("malformed.json");
    fs::write(&malformed_path, format!("1{}+", "0".repeat(199_998))).unwrap();
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.json as json
use core.files as files

fn run() {{
    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{input}") ?? panic("open number")
    reader :: json.reader(^input, limits) ?? panic("create reader")
    result :: reader.next()
    if result == {{
        .Ok(_) -> {{ panic("oversized number allocation accepted") }}
        .Err(first) -> {{
            print(first.reason)
            again :: reader.next()
            if again == {{
                .Ok(_) -> {{ panic("number allocation error not terminal") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
            }}
        }}
    }}

    malformed_limits := encoding.EncodingLimits.safe()
    malformed_limits.buffer_bytes = 4096
    malformed_limits.max_depth = 1
    malformed_limits.max_item_bytes = 200000
    malformed_limits.max_expansion_bytes = 0
    malformed_input :: files.open("{malformed}") ?? panic("open malformed number")
    malformed_reader :: json.reader(^malformed_input, malformed_limits) ?? panic("create malformed reader")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ panic("malformed number accepted") }}
        .Err(first) -> {{
            print(first.reason)
            again :: malformed_reader.next()
            if again == {{
                .Ok(_) -> {{ panic("malformed number error not terminal") }}
                .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let path = dir.join("counted.jet");
    fs::write(&path, &source).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(&source, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, &source, &diags))
    });
    let renamed = out.rust.replacen(
        "fn jet_enc_json_reader_next(",
        "fn jet_enc_json_reader_next_inner(",
        1,
    );
    assert_ne!(renamed, out.rust, "generated JSON reader seam changed");
    let allocator = r#"
mod jet_json_alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub struct CountingAlloc;
    static COUNTING: AtomicBool = AtomicBool::new(false);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    fn add(size: usize) {
        let live = LIVE.fetch_add(size, Ordering::SeqCst) + size;
        PEAK.fetch_max(live, Ordering::SeqCst);
    }
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc(layout);
            if !ptr.is_null() && COUNTING.load(Ordering::SeqCst) { add(layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if COUNTING.load(Ordering::SeqCst) { LIVE.fetch_sub(layout.size(), Ordering::SeqCst); }
            System.dealloc(ptr, layout);
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
            let counting = COUNTING.load(Ordering::SeqCst);
            if counting { LIVE.fetch_sub(old.size(), Ordering::SeqCst); }
            let next = System.realloc(ptr, old, new_size);
            if counting { if next.is_null() { add(old.size()); } else { add(new_size); } }
            next
        }
    }
    pub fn begin() {
        LIVE.store(0, Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
    }
    pub fn finish() -> usize {
        COUNTING.store(false, Ordering::SeqCst);
        PEAK.load(Ordering::SeqCst)
    }
}
#[global_allocator]
static JET_JSON_ALLOC: jet_json_alloc_probe::CountingAlloc = jet_json_alloc_probe::CountingAlloc;
fn jet_enc_json_reader_next(reader: &mut jet_std::JSONReader) -> Result<JetOutcome<jet_std::DataEvent, JetAbsent>, jet_std::EncodingError> {
    let ceiling = jet_encoding_codec_heap_ceiling(&reader.limits);
    jet_json_alloc_probe::begin();
    let result = jet_enc_json_reader_next_inner(reader);
    let peak = jet_json_alloc_probe::finish();
    assert!(peak <= ceiling, "JSON requested allocation peak {peak} exceeded {ceiling}");
    result
}
"#;
    let rs = dir.join("counted.rs");
    let bin = dir.join("counted");
    let generated = renamed.replacen("#![allow(warnings)]", "", 1);
    assert_ne!(generated, renamed, "generated crate attribute changed");
    fs::write(&rs, format!("#![allow(warnings)]\n{allocator}\n{generated}")).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output().unwrap();
    assert!(rustc.status.success(), "rustc rejected counted JSON program:\n{}", String::from_utf8_lossy(&rustc.stderr));
    let run = Command::new(&bin).current_dir(&dir).output().unwrap();
    assert!(run.status.success(), "counted JSON program failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "JSON number allocation exceeded the bounded codec heap ceiling\ntrue\ninvalid JSON number\ntrue\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn jsonl_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_jsonl_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.jsonl");
    let heap_path = dir.join("heap.jsonl");
    let key = "k".repeat(100_000);
    fs::write(&heap_path, format!("{{\"{key}\":0}}\n")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.jsonl as jsonl
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: jsonl.writer(^output) ?? panic("writer")
    writer.write(DataTree.Text("alpha")) ?? panic("record")
    writer.flush() ?? panic("flush")
    // no finish — Drop must leave the record LF unwritten (incomplete wire)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete bytes (no record LF) still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "\"alpha\"")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: jsonl.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(DataTree.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "null\n")
    // Honesty: unfinished Drop wire ≠ finished wire for the same value.
    print(leftover != "\"alpha\"\n")

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 100000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: jsonl.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "jsonl_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n100003\n$[0]\nJSON string allocation exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("jsonl_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("JSONL stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "null\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_csv_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.csv");
    let heap_path = dir.join("heap.csv");
    // Capacity doubles to 131072; the next byte charges past the shared codec
    // heap ceiling while still under max_item_bytes (same counting allocator).
    let field = "x".repeat(131_072);
    fs::write(&heap_path, format!("{field}y")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: csv.writer(^output) ?? panic("writer")
    writer.write(["alpha", "beta"]) ?? panic("record")
    writer.flush() ?? panic("flush")
    // no finish — Drop must leave the record CRLF unwritten (incomplete wire)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete bytes (no record CRLF) still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "alpha,beta")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: csv.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(["done"]) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    // finished is "done\\r\\n"; Jet has no \\r escape — prove via length + prefix.
    print(finished.starts_with("done") && finished.len() == 6)
    // Honesty: unfinished Drop wire ≠ finished row terminator.
    print(leftover.len() == 10)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: csv.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "csv_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n131073\n$[0][0]\nCSV record heap exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "done\r\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("csv_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CSV stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "done\r\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_xml_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.xml");
    let heap_path = dir.join("heap.xml");
    // Attribute text under max_item_bytes; raw_bytes→Array<Int> DataTree slots
    // charge past the shared codec heap ceiling (same counting allocator).
    // Keep modest: ByteLexer retains per-scalar units, so 128KiB hung the suite.
    let attr = "x".repeat(8_192);
    fs::write(&heap_path, format!("<r a=\"{attr}\"/>")).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn xml_name(local: String) => DataTree {{
    return DataTree.Object([
        "raw": DataTree.Text(~local),
        "prefix": DataTree.Null,
        "local": DataTree.Text(~local),
        "namespace_uri": DataTree.Null,
    ])
}}

fn document_start() => DataTree {{
    return DataTree.Object([
        "$xml_event": DataTree.Text("document_start"),
        "encoding": DataTree.Null,
        "bom": DataTree.Array([]),
    ])
}}

fn document_end() => DataTree {{
    return DataTree.Object(["$xml_event": DataTree.Text("document_end")])
}}

fn element_start(empty_style: String) => DataTree {{
    return DataTree.Object([
        "$xml_event": DataTree.Text("element_start"),
        "name": xml_name("r"),
        "namespaces": DataTree.Array([]),
        "attributes": DataTree.Array([]),
        "empty_style": DataTree.Text(~empty_style),
        "open_lexical": DataTree.Object([
            "raw_text": DataTree.Null,
            "raw_bytes": DataTree.Null,
            "semantic": DataTree.Object([
                "name": xml_name("r"),
                "namespaces": DataTree.Array([]),
                "attributes": DataTree.Array([]),
                "empty_style": DataTree.Text(~empty_style),
            ]),
        ]),
    ])
}}

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: xml.writer(^output) ?? panic("writer")
    writer.write(document_start()) ?? panic("document_start")
    writer.write(element_start("explicit")) ?? panic("open root")
    writer.flush() ?? panic("flush")
    // no element_end / document_end / finish — Drop leaves incomplete open tag
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete open element still here.
    leftover :: files.read("{partial}") ?? panic("same-path read after Drop")
    print(leftover == "<r>")
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: xml.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(document_start()) ?? panic("reopen start")
    reopen_writer.write(element_start("empty")) ?? panic("reopen empty root")
    reopen_writer.write(document_end()) ?? panic("reopen end")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read("{partial}") ?? panic("same-path read after finish")
    print(finished == "<r/>")
    // Honesty: unfinished Drop wire ≠ finished complete document.
    print(leftover != finished)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: xml.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 8 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        format!(
            "true\ntrue\ntrue\n{}\n$\nXML event heap exceeded the bounded codec heap ceiling\ntrue\n",
            fs::metadata(&heap_path).unwrap().len()
        )
    );
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "<r/>");
    assert_eq!(stderr, "");
    let dev_path = dir.join("xml_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("XML stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&partial_path).unwrap(), "<r/>");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_drop_and_codec_heap_ceiling_are_enforced() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_stream_drop_heap_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let partial_path = dir.join("partial.cbor");
    let heap_path = dir.join("heap.cbor");
    // Capacity doubles to 131072; the next byte charges past the shared codec
    // heap ceiling while still under max_item_bytes (same counting allocator).
    let text = vec![b'x'; 131_073];
    let mut heap_bytes = Vec::new();
    heap_bytes.push(0x7a); // text, 4-byte length
    heap_bytes.extend_from_slice(&(131_073u32).to_be_bytes());
    heap_bytes.extend_from_slice(&text);
    fs::write(&heap_path, &heap_bytes).unwrap();
    let partial = partial_path.to_string_lossy().replace('\\', "\\\\");
    let heap = heap_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn write_unfinished(path: String) {{
    output :: files.create(path) ?? panic("create partial")
    writer :: cbor.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("array")
    writer.write(encoding.DataEvent.Int(7)) ?? panic("int")
    writer.flush() ?? panic("flush")
    // no ArrayEnd / finish — Drop leaves buffered items unwritten (incomplete)
}}

fn run() {{
    write_unfinished("{partial}")
    // Same-path reopen after Drop: incomplete leftover still here (empty wire).
    leftover :: files.read_bytes("{partial}") ?? panic("same-path read after Drop")
    empty :: [U8].{{}}
    print(leftover == empty)
    // Same-path recreate: Drop must have released the unfinished writer handle.
    reopen_out :: files.create("{partial}") ?? panic("same-path recreate after Drop")
    reopen_writer :: cbor.writer(^reopen_out) ?? panic("reopen writer")
    reopen_writer.write(encoding.DataEvent.Null) ?? panic("reopen write")
    reopen_writer.finish() ?? panic("reopen finish")
    finished :: files.read_bytes("{partial}") ?? panic("same-path read after finish")
    null_wire :: [U8].{{ 246 }}
    print(finished == null_wire)
    // Honesty: unfinished Drop wire ≠ finished complete root.
    print(leftover != finished)

    limits := encoding.EncodingLimits.safe()
    limits.buffer_bytes = 4096
    limits.max_depth = 1
    limits.max_item_bytes = 150000
    limits.max_expansion_bytes = 0
    input :: files.open("{heap}") ?? panic("heap open")
    reader :: cbor.reader(^input, limits) ?? panic("heap reader")
    count := 0
    loop count < 4 {{
        result :: reader.next()
        if result == {{
            .Ok(_) -> {{ count++ }}
            .Err(first) -> {{
                again :: reader.next()
                if again == {{
                    .Ok(_) -> {{ print("heap-not-latched") }}
                    .Err(second) -> {{
                        print(first.byte_offset)
                        print(first.path)
                        print(first.reason)
                        print(first.byte_offset == second.byte_offset && first.path == second.path && first.reason == second.reason)
                    }}
                }}
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_stream_drop_heap", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    // Header is 5 bytes (0x7a + u32 length); fail when doubling past 131072 payload bytes.
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\n131077\n$\nCBOR stream heap exceeded the bounded codec heap ceiling\ntrue\n"
    );
    assert_eq!(fs::read(&partial_path).unwrap(), [0xf6]);
    assert_eq!(stderr, "");
    let dev_path = dir.join("cbor_stream_drop_heap.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CBOR stream drop/heap default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read(&partial_path).unwrap(), [0xf6]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_whole_value_handles_multiline_quotes_crlf_and_typed_decode() {
    let dir = std::env::temp_dir().join(format!("jet_csv_whole_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.csv as csv

#Codable
struct Note { name: String, note: String }

fn run() {
    raw :: "name,note\nAda,\"line1\nline2\"\nLin,\"said \"\"hi\"\"\"\n"
    rows :: csv.parse(raw) ?? panic("parse")
    print(rows.len())
    print(rows[1][1])
    print(rows[2][1])
    print(csv.to_string(rows).replace("\n", "|"))

    notes :: csv.decode<Note>(raw) ?? panic("decode")
    print(notes.len())
    print(notes[0].name)
    print(notes[0].note)

    if csv.parse("a,\"unterminated") == {
        .Ok(_) -> { print("unterminated-missed") }
        .Err(message) -> { print(message.contains("quoted field ended before its closing quote")) }
    }
    if csv.parse("a,\"ok\"junk") == {
        .Ok(_) -> { print("closing-junk-missed") }
        .Err(message) -> { print(message.contains("may follow a closing quote")) }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "csv_whole", source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "3\nline1\nline2\nsaid \"hi\"\nname,note|Ada,\"line1|line2\"|Lin,\"said \"\"hi\"\"\"\n2\nAda\nline1\nline2\ntrue\ntrue\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn csv_stream_records_are_incremental_rfc4180_bounded_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_csv_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input_path = dir.join("input.csv");
    let output_path = dir.join("output.csv");
    let malformed_path = dir.join("malformed.csv");
    let invalid_utf8_path = dir.join("invalid-utf8.csv");
    let item_limit_path = dir.join("item-limit.csv");
    let total_limit_path = dir.join("total-limit.csv");
    fs::write(&input_path, "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail").unwrap();
    fs::write(&malformed_path, "\"bad").unwrap();
    fs::write(&invalid_utf8_path, [b'a', b',', 0xff]).unwrap();
    fs::write(&item_limit_path, "\"abcd\"\r\n").unwrap();
    fs::write(&total_limit_path, "a,b\r\n").unwrap();
    let input = input_path.to_string_lossy().replace('\\', "\\\\");
    let output = output_path.to_string_lossy().replace('\\', "\\\\");
    let malformed = malformed_path.to_string_lossy().replace('\\', "\\\\");
    let invalid_utf8 = invalid_utf8_path.to_string_lossy().replace('\\', "\\\\");
    let item_limit = item_limit_path.to_string_lossy().replace('\\', "\\\\");
    let total_limit = total_limit_path.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.csv as csv
use core.files as files

fn run() {{
    output :: files.create("{output}") ?? panic("create")
    writer :: csv.writer(^output) ?? panic("writer")
    writer.write(["a", "b,b", "c\"c", "line1\nline2"]) ?? panic("write first")
    writer.write(["last", "", "tail"]) ?? panic("write second")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    after_finish :: writer.write(["late"])
    if after_finish == {{
        .Ok(_) -> {{ print("write-after-finish-missed") }}
        .Err(writer_first) -> {{
            after_terminal :: writer.flush()
            if after_terminal == {{
                .Ok(_) -> {{ print("writer-terminal-missed") }}
                .Err(writer_second) -> {{ print(writer_first.byte_offset == writer_second.byte_offset && writer_first.reason == writer_second.reason) }}
            }}
        }}
    }}

    input :: files.open("{input}") ?? panic("open")
    reader :: csv.reader(^input) ?? panic("reader")
    first :: reader.next() ?? panic("first")
    if first == {{
        Val(row) -> {{ print(row[0]); print(row[1]); print(row[2]); print(row[3]) }}
        None -> {{ print("first-missing") }}
    }}
    second :: reader.next() ?? panic("second")
    if second == {{
        Val(row) -> {{ print(row[0]); print(row[1] == ""); print(row[2]) }}
        None -> {{ print("second-missing") }}
    }}
    eof :: reader.next() ?? panic("eof")
    if eof == {{ Val(_) -> {{ print(false) }} None -> {{ print(true) }} }}
    eof_again :: reader.next() ?? panic("eof again")
    if eof_again == {{ Val(_) -> {{ print(false) }} None -> {{ print(true) }} }}

    malformed_input :: files.open("{malformed}") ?? panic("malformed open")
    malformed_reader :: csv.reader(^malformed_input) ?? panic("malformed reader")
    malformed_result :: malformed_reader.next()
    if malformed_result == {{
        .Ok(_) -> {{ print("malformed-missed") }}
        .Err(malformed_first) -> {{
            malformed_again :: malformed_reader.next()
            if malformed_again == {{
                .Ok(_) -> {{ print("malformed-terminal-missed") }}
                .Err(malformed_second) -> {{ print(malformed_first.path); print(malformed_first.byte_offset == malformed_second.byte_offset && malformed_first.reason == malformed_second.reason) }}
            }}
        }}
    }}

    invalid_utf8_input :: files.open("{invalid_utf8}") ?? panic("invalid utf8 open")
    invalid_utf8_reader :: csv.reader(^invalid_utf8_input) ?? panic("invalid utf8 reader")
    invalid_utf8_result :: invalid_utf8_reader.next()
    if invalid_utf8_result == {{
        .Ok(_) -> {{ print("invalid-utf8-missed") }}
        .Err(error) -> {{
            print(error.byte_offset)
            print(error.line ?? 0)
            print(error.column ?? 0)
            print(error.path)
        }}
    }}

    item_limits := encoding.EncodingLimits.safe()
    item_limits.max_item_bytes = 3
    item_input :: files.open("{item_limit}") ?? panic("item open")
    item_reader :: csv.reader(^item_input, item_limits) ?? panic("item reader")
    item_result :: item_reader.next()
    if item_result == {{
        .Ok(_) -> {{ print("item-limit-missed") }}
        .Err(item_first) -> {{
            item_again :: item_reader.next()
            if item_again == {{
                .Ok(_) -> {{ print("item-terminal-missed") }}
                .Err(item_second) -> {{ print(item_first.path); print(item_first.byte_offset == item_second.byte_offset && item_first.reason == item_second.reason) }}
            }}
        }}
    }}

    total_limits := encoding.EncodingLimits.safe()
    total_limits.max_total_bytes = Val(3)
    total_input :: files.open("{total_limit}") ?? panic("total open")
    total_reader :: csv.reader(^total_input, total_limits) ?? panic("total reader")
    total_result :: total_reader.next()
    if total_result == {{
        .Ok(_) -> {{ print("total-limit-missed") }}
        .Err(total_first) -> {{
            total_again :: total_reader.next()
            if total_again == {{
                .Ok(_) -> {{ print("total-terminal-missed") }}
                .Err(total_second) -> {{ print(total_first.byte_offset); print(total_first.path); print(total_first.reason == total_second.reason) }}
            }}
        }}
    }}

    writer_limits := encoding.EncodingLimits.safe()
    writer_limits.max_item_bytes = 3
    limited_output :: files.create("{output}.limited") ?? panic("limited create")
    limited_writer :: csv.writer(^limited_output, writer_limits) ?? panic("limited writer")
    limited_result :: limited_writer.write(["abcd"])
    if limited_result == {{
        .Ok(_) -> {{ print("writer-limit-missed") }}
        .Err(limited_first) -> {{
            limited_again :: limited_writer.finish()
            if limited_again == {{
                .Ok(_) -> {{ print("writer-limit-terminal-missed") }}
                .Err(limited_second) -> {{ print(limited_first.path); print(limited_first.reason == limited_second.reason) }}
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "csv_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "true\na\nb,b\nc\"c\nline1\nline2\nlast\ntrue\ntail\ntrue\ntrue\n$[0][0]\ntrue\n3\n1\n4\n$[0][1]\n$[0][0]\ntrue\n3\n$[0][1]\ntrue\n$[0][0]\ntrue\n"
    );
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail\r\n");
    assert_eq!(stderr, "");
    let dev_path = dir.join("csv_stream.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CSV stream default-dev failed: {other:?}"),
    }
    assert_eq!(fs::read_to_string(&output_path).unwrap(), "a,\"b,b\",\"c\"\"c\",\"line1\nline2\"\r\nlast,,tail\r\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_is_incremental_bounded_deterministic_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_stream_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("output.cbor");
    let float_output = dir.join("float.cbor");
    let indefinite = dir.join("indefinite.cbor");
    let truncated = dir.join("truncated.cbor");
    let half = dir.join("half.cbor");
    let non_shortest = dir.join("non-shortest.cbor");
    let nested = dir.join("nested.cbor");
    fs::write(&indefinite, [0x9f, 0x01, 0x7f, 0x61, b'a', 0xff, 0x42, 0x01, 0x02, 0xff]).unwrap();
    fs::write(&truncated, [0x63, b'a']).unwrap();
    fs::write(&half, [0xf9, 0x3c, 0x00]).unwrap();
    fs::write(&non_shortest, [0x18, 0x01]).unwrap();
    fs::write(&nested, [0x81, 0x80]).unwrap();
    let output_text = output.to_string_lossy().replace('\\', "\\\\");
    let float_output_text = float_output.to_string_lossy().replace('\\', "\\\\");
    let indefinite_text = indefinite.to_string_lossy().replace('\\', "\\\\");
    let truncated_text = truncated.to_string_lossy().replace('\\', "\\\\");
    let half_text = half.to_string_lossy().replace('\\', "\\\\");
    let non_shortest_text = non_shortest.to_string_lossy().replace('\\', "\\\\");
    let nested_text = nested.to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn run() {{
    output :: files.create("{output_text}") ?? panic("create")
    writer :: cbor.writer(^output) ?? panic("writer")
    writer.write(encoding.DataEvent.ObjectStart) ?? panic("start")
    writer.write(encoding.DataEvent.Key("b")) ?? panic("key")
    writer.write(encoding.DataEvent.Text("xy")) ?? panic("text")
    writer.write(encoding.DataEvent.Key("a")) ?? panic("key")
    writer.write(encoding.DataEvent.Int(1)) ?? panic("int")
    writer.write(encoding.DataEvent.ObjectEnd) ?? panic("end")
    writer.flush() ?? panic("flush")
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("finish twice")
    float_file :: files.create("{float_output_text}") ?? panic("float create")
    float_writer :: cbor.writer(^float_file) ?? panic("float writer")
    float_writer.write(encoding.DataEvent.ArrayStart) ?? panic("float array")
    float_writer.write(encoding.DataEvent.Float(1.0)) ?? panic("float write")
    float_writer.write(encoding.DataEvent.Float(100000.0)) ?? panic("float32 write")
    float_writer.write(encoding.DataEvent.Float(1.1)) ?? panic("float64 write")
    float_writer.write(encoding.DataEvent.Float(0.0 / 0.0)) ?? panic("nan write")
    float_writer.write(encoding.DataEvent.Float(-0.0)) ?? panic("negative zero write")
    float_writer.write(encoding.DataEvent.ArrayEnd) ?? panic("float array end")
    float_writer.finish() ?? panic("float finish")
    whole_tree :: DataTree.Object(["b": DataTree.Text("xy"), "a": DataTree.Int(1)])
    expected_whole :: [U8].{{ 162, 97, 97, 1, 97, 98, 98, 120, 121 }}
    print((cbor.to_bytes_canonical(whole_tree) ?? panic("whole encode")) == expected_whole)
    after :: writer.write(encoding.DataEvent.Null)
    if after == {{
        .Ok(_) -> print(false)
        .Err(writer_first) -> {{
            again :: writer.flush()
            if again == {{
                .Ok(_) -> print(false)
                .Err(writer_second) -> print(writer_first.reason == writer_second.reason)
            }}
        }}
    }}

    input :: files.open("{output_text}") ?? panic("open")
    reader :: cbor.reader(^input) ?? panic("reader")
    count := 0
    loop count < 6 {{
        event :: reader.next() ?? panic("next")
        if event == {{
            Val(_) -> count++
            None -> print("early")
        }}
    }}
    eof :: reader.next() ?? panic("eof")
    if eof == {{
        None -> print(count)
        Val(_) -> print("late")
    }}

    indef_input :: files.open("{indefinite_text}") ?? panic("indef open")
    indef_reader :: cbor.reader(^indef_input) ?? panic("indef reader")
    indef_count := 0
    loop indef_count < 5 {{
        indef_event :: indef_reader.next() ?? panic("indef next")
        if indef_event == {{
            Val(_) -> indef_count++
            None -> print("indef early")
        }}
    }}
    print(indef_count)

    half_input :: files.open("{half_text}") ?? panic("half open")
    half_reader :: cbor.reader(^half_input) ?? panic("half reader")
    if half_reader.next() == {{
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }}

    short_input :: files.open("{non_shortest_text}") ?? panic("short open")
    short_reader :: cbor.reader(^short_input) ?? panic("short reader")
    if short_reader.next() == {{
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }}

    depth_limits := encoding.EncodingLimits.safe()
    depth_limits.max_depth = 1
    nested_input :: files.open("{nested_text}") ?? panic("nested open")
    nested_reader :: cbor.reader(^nested_input, depth_limits) ?? panic("nested reader")
    root_event :: nested_reader.next() ?? panic("root array")
    if nested_reader.next() == {{
        .Ok(_) -> print(false)
        .Err(depth_error) -> print(depth_error.reason == "max_depth 1 exceeded")
    }}

    bad_input :: files.open("{truncated_text}") ?? panic("bad open")
    bad_reader :: cbor.reader(^bad_input) ?? panic("bad reader")
    first_bad :: bad_reader.next()
    if first_bad == {{
        .Ok(_) -> print("missed")
        .Err(bad_first) -> {{
            second_bad :: bad_reader.next()
            if second_bad == {{
                .Ok(_) -> print("unlatched")
                .Err(bad_second) -> {{
                    print(bad_first.byte_offset)
                    print(bad_first.path)
                    print(bad_first.reason == bad_second.reason)
                }}
            }}
        }}
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_stream", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "true\ntrue\n6\n5\ntrue\ntrue\ntrue\n2\n$\ntrue\n");
    assert_eq!(fs::read(&output).unwrap(), [0xa2, 0x61, b'a', 0x01, 0x61, b'b', 0x62, b'x', b'y']);
    assert_eq!(fs::read(&float_output).unwrap(), [
        0x85, 0xf9, 0x3c, 0x00, 0xfa, 0x47, 0xc3, 0x50, 0x00,
        0xfb, 0x3f, 0xf1, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a,
        0xf9, 0x7e, 0x00, 0xf9, 0x80, 0x00,
    ]);
    assert_eq!(stderr, "");
    let dev_path = dir.join("cbor_stream.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("CBOR stream default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_hostile_inputs_and_replacement_limits_are_exact() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixtures: &[(&str, &[u8])] = &[
        ("indef-map.cbor", &[0xbf, 0x61, b'a', 0x7f, 0x61, b'x', 0x61, b'y', 0xff, 0xff]),
        ("indef-bytes.cbor", &[0x5f, 0x42, 1, 2, 0x41, 3, 0xff]),
        ("duplicate.cbor", &[0xbf, 0x61, b'a', 1, 0x61, b'a', 2, 0xff]),
        ("nontext.cbor", &[0xa1, 1, 2]),
        ("tag.cbor", &[0xc0, 1]),
        ("range.cbor", &[0x1b, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        ("trunc-int.cbor", &[0x1a, 0]),
        ("trunc-float.cbor", &[0xfa, 0, 0]),
        ("trunc-indef.cbor", &[0x7f, 0x62, b'a']),
        ("trailing.cbor", &[1, 2]),
        ("nested.cbor", &[0x81, 0xa1, 0x61, b'x', 0x1a, 0]),
    ];
    for (name, bytes) in fixtures { fs::write(dir.join(name), bytes).unwrap(); }
    let path = |name: &str| dir.join(name).to_string_lossy().replace('\\', "\\\\");
    let array_ok = path("array-ok.cbor");
    let array_fail = path("array-fail.cbor");
    let object_ok = path("object-ok.cbor");
    let object_fail = path("object-fail.cbor");
    let incomplete = path("incomplete.cbor");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn reader_terminal(reader: &cbor.CBORReader, reason: String) => Bool {{
    repeated :: reader.next()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn writer_terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    map_limits := encoding.EncodingLimits.safe()
    map_limits.max_item_bytes = 3
    map_input :: files.open("{}") ?? panic("map open")
    map_reader :: cbor.reader(^map_input, map_limits) ?? panic("map reader")
    map_count := 0
    loop map_count < 4 {{
        map_event :: map_reader.next() ?? panic("map error")
        if map_event == {{
            Val(_) -> map_count++
            None -> panic("map eof")
        }}
    }}
    print(map_count)

    tight_limits := encoding.EncodingLimits.safe()
    tight_limits.max_item_bytes = 2
    tight_input :: files.open("{}") ?? panic("tight open")
    tight_reader := cbor.reader(^tight_input, tight_limits) ?? panic("tight reader")
    tight_object :: tight_reader.next() ?? panic("tight object")
    tight_key :: tight_reader.next() ?? panic("tight key")
    tight_first :: tight_reader.next()
    if tight_first == {{
        .Ok(_) -> panic("combined key/chunk budget missed")
        .Err(first) -> {{
            print(first.path == "$[\"a\"]" && first.byte_offset == 6 && reader_terminal(&tight_reader, ~first.reason))
        }}
    }}

    bytes_input :: files.open("{}") ?? panic("bytes open")
    bytes_reader :: cbor.reader(^bytes_input) ?? panic("bytes reader")
    bytes_event :: bytes_reader.next() ?? panic("bytes event")
    if bytes_event == {{
        Val(_) -> print(true)
        None -> print(false)
    }}

    short_input :: files.open("{}") ?? panic("short open")
    short_reader :: cbor.reader(^short_input) ?? panic("short reader")
    short_event :: short_reader.next() ?? panic("short event")
    if short_event == {{
        Val(_) -> print(true)
        None -> print(false)
    }}

    duplicate_input :: files.open("{}") ?? panic("duplicate open")
    duplicate_reader := cbor.reader(^duplicate_input) ?? panic("duplicate reader")
    duplicate_object :: duplicate_reader.next() ?? panic("duplicate object")
    duplicate_key :: duplicate_reader.next() ?? panic("duplicate key")
    duplicate_value :: duplicate_reader.next() ?? panic("duplicate value")
    duplicate_first :: duplicate_reader.next()
    if duplicate_first == {{
        .Err(first) -> {{
            print(first.byte_offset == 4 && first.path == "$" && reader_terminal(&duplicate_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    nontext_input :: files.open("{}") ?? panic("nontext open")
    nontext_reader :: cbor.reader(^nontext_input) ?? panic("nontext reader")
    nontext_object :: nontext_reader.next() ?? panic("nontext object")
    if nontext_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$" && e.reason == "CBOR map key must be text")
        .Ok(_) -> print(false)
    }}

    tag_input :: files.open("{}") ?? panic("tag open")
    tag_reader :: cbor.reader(^tag_input) ?? panic("tag reader")
    if tag_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 0 && e.path == "$" && e.reason == "CBOR tags are outside DataEvent")
        .Ok(_) -> print(false)
    }}

    range_input :: files.open("{}") ?? panic("range open")
    range_reader :: cbor.reader(^range_input) ?? panic("range reader")
    if range_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR integer is outside Jet Int")
        .Ok(_) -> print(false)
    }}

    int_input :: files.open("{}") ?? panic("int open")
    int_reader := cbor.reader(^int_input) ?? panic("int reader")
    if int_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 2 && reader_terminal(&int_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    float_input :: files.open("{}") ?? panic("float open")
    float_reader := cbor.reader(^float_input) ?? panic("float reader")
    if float_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 3 && reader_terminal(&float_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    indef_input :: files.open("{}") ?? panic("indef open")
    indef_reader := cbor.reader(^indef_input) ?? panic("indef reader")
    if indef_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 3 && reader_terminal(&indef_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    trailing_input :: files.open("{}") ?? panic("trailing open")
    trailing_reader := cbor.reader(^trailing_input) ?? panic("trailing reader")
    trailing_root :: trailing_reader.next() ?? panic("root")
    if trailing_reader.next() == {{
        .Err(first) -> {{
            print(first.byte_offset == 1 && reader_terminal(&trailing_reader, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    nested_input :: files.open("{}") ?? panic("nested open")
    nested_reader :: cbor.reader(^nested_input) ?? panic("nested reader")
    nested_array :: nested_reader.next() ?? panic("nested array")
    nested_object :: nested_reader.next() ?? panic("nested object")
    nested_key :: nested_reader.next() ?? panic("nested key")
    if nested_reader.next() == {{
        .Err(e) -> print(e.byte_offset == 6 && e.path == "$[0][\"x\"]")
        .Ok(_) -> print(false)
    }}

    array_limits := encoding.EncodingLimits.safe()
    array_limits.max_item_bytes = 2
    array_output :: files.create("{array_ok}") ?? panic("array output")
    array_writer :: cbor.writer(^array_output, array_limits) ?? panic("array writer")
    array_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array start")
    array_writer.write(encoding.DataEvent.Null) ?? panic("array null")
    array_writer.write(encoding.DataEvent.ArrayEnd) ?? panic("array end")
    array_writer.finish() ?? panic("array finish")

    array_tight := encoding.EncodingLimits.safe()
    array_tight.max_item_bytes = 1
    array_fail_output :: files.create("{array_fail}") ?? panic("array fail output")
    array_fail_writer := cbor.writer(^array_fail_output, array_tight) ?? panic("array fail writer")
    array_fail_writer.write(encoding.DataEvent.ArrayStart) ?? panic("array fail start")
    array_fail_writer.write(encoding.DataEvent.Null) ?? panic("array fail null")
    if array_fail_writer.write(encoding.DataEvent.ArrayEnd) == {{
        .Err(first) -> {{
            print(writer_terminal(&array_fail_writer, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}

    object_limits := encoding.EncodingLimits.safe()
    object_limits.max_item_bytes = 4
    object_output :: files.create("{object_ok}") ?? panic("object output")
    object_writer :: cbor.writer(^object_output, object_limits) ?? panic("object writer")
    object_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object start")
    object_writer.write(encoding.DataEvent.Key("a")) ?? panic("object key")
    object_writer.write(encoding.DataEvent.Null) ?? panic("object null")
    object_writer.write(encoding.DataEvent.ObjectEnd) ?? panic("object end")
    object_writer.finish() ?? panic("object finish")

    object_tight := encoding.EncodingLimits.safe()
    object_tight.max_item_bytes = 3
    object_fail_output :: files.create("{object_fail}") ?? panic("object fail output")
    object_fail_writer :: cbor.writer(^object_fail_output, object_tight) ?? panic("object fail writer")
    object_fail_writer.write(encoding.DataEvent.ObjectStart) ?? panic("object fail start")
    object_fail_writer.write(encoding.DataEvent.Key("a")) ?? panic("object fail key")
    object_fail_writer.write(encoding.DataEvent.Null) ?? panic("object fail null")
    if object_fail_writer.write(encoding.DataEvent.ObjectEnd) == {{
        .Err(_) -> print(true)
        .Ok(_) -> print(false)
    }}

    incomplete_output :: files.create("{incomplete}") ?? panic("incomplete output")
    incomplete_writer := cbor.writer(^incomplete_output) ?? panic("incomplete writer")
    incomplete_writer.write(encoding.DataEvent.ArrayStart) ?? panic("incomplete start")
    incomplete_writer.flush() ?? panic("incomplete flush")
    if incomplete_writer.finish() == {{
        .Err(first) -> {{
            print(writer_terminal(&incomplete_writer, ~first.reason))
        }}
        .Ok(_) -> print(false)
    }}
}}
"#,
        path("indef-map.cbor"), path("indef-map.cbor"), path("indef-bytes.cbor"),
        path("non-shortest.cbor"), path("duplicate.cbor"), path("nontext.cbor"),
        path("tag.cbor"), path("range.cbor"), path("trunc-int.cbor"),
        path("trunc-float.cbor"), path("trunc-indef.cbor"), path("trailing.cbor"),
        path("nested.cbor"),
    );
    fs::write(dir.join("non-shortest.cbor"), [0x18, 0x01]).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_hostile", &source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}\nsource:\n{source}");
    assert_eq!(stdout, "4\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n");
    assert_eq!(fs::read(&array_ok).unwrap(), [0x81, 0xf6]);
    assert_eq!(fs::read(&object_ok).unwrap(), [0xa1, 0x61, b'a', 0xf6]);
    assert!(fs::read(&incomplete).unwrap().is_empty());
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_stream_workspace_growth_is_prospective_and_terminal() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_workspace_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let success = dir.join("success.cbor").to_string_lossy().replace('\\', "\\\\");
    let rejected = dir.join("rejected.cbor").to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.finish()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn close_array(writer: &cbor.CBORWriter) {{
    result :: writer.write(encoding.DataEvent.ArrayEnd)
    if result == {{
        .Err(error) -> panic("{{error.reason}}")
        .Ok(_) -> return
    }}
}}

fn run() {{
    roomy := encoding.EncodingLimits.safe()
    roomy.max_item_bytes = 9
    output :: files.create("{success}") ?? panic("create")
    writer := cbor.writer(^output, roomy) ?? panic("writer")
    writer.write(encoding.DataEvent.ArrayStart) ?? panic("start")
    loop _, 0..7 {{ writer.write(encoding.DataEvent.Null) ?? panic("null") }}
    close_array(&writer)
    writer.finish() ?? panic("finish")

    tight := encoding.EncodingLimits.safe()
    tight.max_item_bytes = 7
    rejected_output :: files.create("{rejected}") ?? panic("create rejected")
    rejected_writer := cbor.writer(^rejected_output, tight) ?? panic("rejected writer")
    rejected_writer.write(encoding.DataEvent.ArrayStart) ?? panic("rejected start")
    loop _, 0..6 {{ rejected_writer.write(encoding.DataEvent.Null) ?? panic("accepted null") }}
    if rejected_writer.write(encoding.DataEvent.Null) == {{
        .Err(first) -> {{
            print(first.reason == "max_item_bytes 7 exceeded")
            print(terminal(&rejected_writer, ~first.reason))
        }}
        .Ok(_) -> {{ print(false); print(false) }}
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_workspace", &source, &[], None);
    assert_eq!(code, 0, "CBOR workspace program failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\n");
    assert_eq!(fs::read(dir.join("success.cbor")).unwrap(), [0x88, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6]);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn cbor_stream_io_errors_latch_in_aot_and_default_dev() {
    let dir = std::env::temp_dir().join(format!("jet_cbor_io_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let directory = dir.to_string_lossy().replace('\\', "\\\\");
    let source = format!(r#"
use core.encoding as encoding
use core.encoding.cbor as cbor
use core.files as files

fn reader_terminal(reader: &cbor.CBORReader, reason: String) => Bool {{
    repeated :: reader.next()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn writer_terminal(writer: &cbor.CBORWriter, reason: String) => Bool {{
    repeated :: writer.flush()
    if repeated == {{
        .Err(error) -> return error.reason == reason
        .Ok(_) -> return false
    }}
    return false
}}

fn run() {{
    directory_input :: files.open("{directory}") ?? panic("directory open")
    directory_reader := cbor.reader(^directory_input) ?? panic("directory reader")
    if directory_reader.next() == {{
        .Err(first) -> print(reader_terminal(&directory_reader, ~first.reason))
        .Ok(_) -> print(false)
    }}
    full_output :: files.create("/dev/full") ?? panic("full open")
    full_writer := cbor.writer(^full_output) ?? panic("full writer")
    full_writer.write(encoding.DataEvent.Null) ?? panic("full buffered write")
    if full_writer.flush() == {{
        .Err(first) -> print(writer_terminal(&full_writer, ~first.reason))
        .Ok(_) -> print(false)
    }}
}}
"#);
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_io", &source, &[], None);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "true\ntrue\n", ""));
    let path = dir.join("cbor_io.jet");
    fs::write(&path, &source).unwrap();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, "true\ntrue\n", ""));
        }
        other => panic!("CBOR default-dev fallback failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_codable_bytes_and_original_wire_canonical_validation() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor whole-value test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding as encoding
use core.encoding.cbor as cbor

#Codable
struct Packet { id: Int, payload: [U8] }

fn run() {
    packet := Packet.{ id: 7, payload: [222, 173] }
    wire := cbor.to_bytes(packet) ?? panic("encode")
    stable := cbor.to_bytes_canonical(packet) ?? panic("canonical encode")
    back := Packet.{ cbor.decode<Packet>(wire) ?? panic("decode") }
    raw_wire := cbor.to_bytes([1, 2, 255]) ?? panic("byte encode")
    raw := [U8].{ cbor.decode<[U8]>(raw_wire) ?? panic("byte decode") }
    print(wire)
    print(stable == wire)
    print(back.id)
    print(back.payload)
    print(raw)

    strict := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    // 0x18 0x01 is valid CBOR for 1, but not shortest/Core deterministic.
    rejected := cbor.parse([24, 1], strict) ?? DataTree.Int(-1)
    print(rejected.int() ?? -2)
    strict_decode := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    if cbor.decode<[Int]>([129, 97, 120], strict_decode) == {
        .Ok(_) -> print("unexpected success")
        .Err(error) -> print("{error[0].path}|{error[0].reason}")
    }
    if cbor.decode<Int>([65, 0]) == {
        .Ok(_) -> print("unexpected success")
        .Err(error) -> print("{error[0].path}|{error[0].reason}")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole", source, &[], None);
    assert_eq!(code, 0, "CBOR whole-value program failed: {stderr}");
    assert_eq!(
        stdout,
        "[162, 98, 105, 100, 7, 103, 112, 97, 121, 108, 111, 97, 100, 66, 222, 173]\ntrue\n7\n[222, 173]\n[1, 2, 255]\n-1\n[0]|expected Int, found text \"x\"\n|expected Int, found Bytes\n"
    );
    let path = dir.join("cbor_whole.jet");
    fs::write(&path, source).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).expect("CBOR fixture loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diag| !matches!(diag.severity, jet::Diagnostics::Severity::Error)),
        "CBOR fixture must type-check: {diagnostics:?}"
    );
    jet_jit::try_compile_bundle(&bundle).expect("CBOR fixture must compile for resident JIT");
    jet_jit::reset_jit_trace_for_test();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: dev_stdout,
            stderr: dev_stderr,
            exit_code,
        } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
            assert!(
                jet_jit::jit_executed_for_test(),
                "CBOR whole-value fixture must execute resident JIT"
            );
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "CBOR whole-value fixture must not silently deopt"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test(),
                "CBOR whole-value fixture must not fall back"
            );
        }
        other => panic!("CBOR whole-value default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_live_allocation_and_preferred_float_validation() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor whole-value limits test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_limits_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn run() {
    strict := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1024, require_canonical: true }
    if cbor.parse([249, 62, 0], ~strict) == {
        .Ok(value) -> print(value.float() ?? -1.0)
        .Err(_) -> print(-2.0)
    }
    if cbor.parse([250, 63, 192, 0, 0], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR Float does not use its preferred shortest encoding")
    }
    if cbor.parse([249, 126, 1], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR NaN is not the canonical 0xf97e00 encoding")
    }
    if cbor.parse([249, 126, 0], ~strict) == {
        .Ok(_) -> print(true)
        .Err(_) -> print(false)
    }

    tiny := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 3, require_canonical: false }
    if cbor.parse([130, 1, 2], tiny) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR array allocation exceeds max_bytes 3")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole_limits", source, &[], None);
    assert_eq!(code, 0, "CBOR whole-value limits program failed: {stderr}");
    assert_eq!(stdout, "1.5\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn cbor_whole_indefinite_values_obey_normal_canonical_and_limit_laws() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR indefinite-value test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_indefinite_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding as encoding
use core.encoding.cbor as cbor

#Codable
struct Packet { name: String, data: [U8] }

fn run() {
    array := [Int].{ cbor.decode<[Int]>([159, 1, 2, 255]) ?? panic("indefinite array") }
    text := cbor.parse([127, 97, 97, 98, 98, 99, 255]) ?? panic("indefinite text")
    print(array)
    print(text.text() ?? "bad")

    // {_ "name": (_ "J", "et"), "data": (_ h'0102', h'03')}
    packet := Packet.{ cbor.decode<Packet>([191, 100, 110, 97, 109, 101, 127, 97, 74, 98, 101, 116, 255, 100, 100, 97, 116, 97, 95, 66, 1, 2, 65, 3, 255, 255]) ?? panic("typed indefinite decode") }
    print(packet.name)
    print(packet.data)

    strict := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 1073741824, require_canonical: true }
    if cbor.parse([159, 1, 255], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.path == "$" && e.reason == "indefinite-length CBOR is not Core deterministic")
    }
    if cbor.parse([129, 127, 97, 120, 255], ~strict) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$[0]")
    }

    item_limited := cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1024, require_canonical: false }
    if cbor.parse([159, 1, 2, 255], item_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 2 && e.path == "$[1]" && e.reason == "max_items 2 exceeded")
    }
    chunk_limited := cbor.CBOROptions.{ max_depth: 256, max_items: 2, max_bytes: 1024, require_canonical: false }
    if cbor.parse([127, 97, 97, 97, 98, 255], chunk_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 3 && e.path == "$" && e.reason == "max_items 2 exceeded")
    }
    depth_limited := cbor.CBOROptions.{ max_depth: 1, max_items: 100, max_bytes: 64, require_canonical: false }
    if cbor.parse([159, 127, 97, 120, 255, 255], depth_limited) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.path == "$[0]" && e.reason == "max_depth 1 exceeded")
    }

    if cbor.parse([127, 65, 120, 255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 1 && e.reason == "indefinite CBOR string contains a wrong or indefinite chunk")
    }
    if cbor.parse([159, 1]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 2 && e.reason == "indefinite CBOR array ended before its break")
    }
    if cbor.parse([191, 97, 107, 255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 3 && e.reason == "indefinite CBOR map break appears where a value is required")
    }
    if cbor.parse([255]) == {
        .Ok(_) -> print(false)
        .Err(e) -> print(e.byte_offset == 0 && e.reason == "CBOR break outside an indefinite container")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_indefinite", source, &[], None);
    assert_eq!(code, 0, "CBOR indefinite-value program failed: {stderr}");
    assert_eq!(
        stdout,
        "[1, 2]\nabc\nJet\n[1, 2, 3]\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn cbor_whole_hostile_byte_corpus_matches_aot_and_default_dev() {
    if !common::have_rustc() {
        eprintln!("note: skipping CBOR hostile whole-value corpus (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_whole_corpus_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn wire(values: [Int]) => [U8] {
    bytes := [U8].{}
    loop value, values {
        bytes.push(U8.from_int(value) ?? panic("corpus byte outside U8"))
    }
    return bytes
}

fn accepted(values: [Int]) => Bool {
    if cbor.parse(wire(values)) == {
        .Ok(_) -> return true
        .Err(_) -> return false
    }
    return false
}

fn rejected(values: [Int], offset: Int, path: String, reason: String) => Bool {
    if cbor.parse(wire(values)) == {
        .Ok(_) -> return false
        .Err(error) -> return error.byte_offset == offset && error.path == path && error.reason == reason
    }
    return false
}

fn canonical_rejected(values: [Int], offset: Int, path: String, reason: String) => Bool {
    strict := cbor.CBOROptions.{
        max_depth: 256,
        max_items: 1000000,
        max_bytes: 1073741824,
        require_canonical: true,
    }
    if cbor.parse(wire(values), strict) == {
        .Ok(_) -> return false
        .Err(error) -> return error.byte_offset == offset && error.path == path && error.reason == reason
    }
    return false
}

fn run() {
    empty := [Int].{}
    // RFC 8949 argument widths, scalar families, nested containers, preferred
    // floats, and every supported normal-mode indefinite family.
    print(accepted([0]))
    print(accepted([23]))
    print(accepted([24, 24]))
    print(accepted([25, 1, 0]))
    print(accepted([26, 0, 1, 0, 0]))
    print(accepted([27, 0, 0, 0, 1, 0, 0, 0, 0]))
    print(accepted([32]))
    print(accepted([56, 24]))
    print(accepted([96]))
    print(accepted([99, 226, 130, 172]))
    print(accepted([131, 1, 130, 2, 3, 161, 97, 107, 245]))
    print(accepted([246]))
    print(accepted([249, 62, 0]))
    print(accepted([250, 71, 195, 80, 0]))
    print(accepted([127, 97, 97, 98, 98, 99, 255]))
    print(accepted([159, 1, 2, 255]))
    print(accepted([191, 97, 107, 1, 255]))

    // Truncation at each structural layer, reserved heads, invalid text,
    // closed DataTree byte identity, duplicate/non-text keys, tags/simple
    // values, stray breaks, trailing roots, and signed-range overflow.
    print(rejected(empty, 0, "$", "CBOR value is missing"))
    print(rejected([28], 0, "$", "indefinite/reserved CBOR length is unsupported by whole-value decoding"))
    print(rejected([26, 0], 2, "$", "CBOR length argument is truncated"))
    print(rejected([27, 128, 0, 0, 0, 0, 0, 0, 0], 0, "$", "CBOR integer is outside Jet Int"))
    print(rejected([59, 128, 0, 0, 0, 0, 0, 0, 0], 0, "$", "CBOR integer is outside Jet Int"))
    print(rejected([65, 0], 0, "$", "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>"))
    print(rejected([98, 97], 2, "$", "CBOR byte/text string is truncated"))
    print(rejected([97, 255], 0, "$", "CBOR text is not UTF-8"))
    print(rejected([127, 98, 97], 3, "$", "CBOR byte/text string chunk is truncated"))
    print(rejected([130, 1], 2, "$[1]", "CBOR value is missing"))
    print(rejected([161, 1, 2], 1, "$", "CBOR map key must be text"))
    print(rejected([162, 97, 97, 1, 97, 97, 2], 4, "$", "duplicate CBOR text map key"))
    print(rejected([161, 97, 97], 3, "$[\"a\"]", "CBOR value is missing"))
    print(rejected([192, 1], 0, "$", "CBOR tags are unsupported"))
    print(rejected([247], 0, "$", "unsupported CBOR simple value 23"))
    print(rejected([255], 0, "$", "CBOR break outside an indefinite container"))
    print(rejected([249, 0], 2, "$", "CBOR Float16 is truncated"))
    print(rejected([1, 2], 1, "$", "trailing CBOR data after root value"))

    // Original wire, not a normalized tree, determines strict acceptance.
    print(canonical_rejected([24, 1], 0, "$", "CBOR argument does not use its shortest form"))
    print(canonical_rejected([120, 1, 97], 0, "$", "CBOR argument does not use its shortest form"))
    print(canonical_rejected([162, 97, 98, 1, 97, 97, 2], 4, "$", "CBOR map keys are not in Core deterministic bytewise order"))
    print(canonical_rejected([250, 63, 192, 0, 0], 0, "$", "CBOR Float does not use its preferred shortest encoding"))
    print(canonical_rejected([159, 1, 255], 0, "$", "indefinite-length CBOR is not Core deterministic"))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "cbor_whole_corpus", source, &[], None);
    assert_eq!(code, 0, "CBOR hostile whole-value corpus failed: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 40, "hostile corpus case count drifted: {stdout}");
    assert!(lines.iter().all(|line| *line == "true"), "hostile corpus mismatch: {stdout}");
    assert_eq!(stderr, "");

    let path = dir.join("cbor_whole_corpus.jet");
    fs::write(&path, source).unwrap();
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout: dev_stdout,
            stderr: dev_stderr,
            exit_code,
        } => assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new())),
        other => panic!("CBOR hostile corpus default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cbor_whole_requested_allocation_stays_under_counting_allocator_ceiling() {
    if !common::have_rustc() {
        eprintln!("note: skipping cbor counting-allocator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_cbor_counted_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.cbor as cbor

fn run() {
    options := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 100, require_canonical: false }
    value := cbor.parse([130, 97, 120, 97, 121], ~options) ?? panic("definite parse")
    indefinite := cbor.parse([159, 97, 120, 97, 121, 255], ~options) ?? panic("indefinite parse")
    if cbor.parse([130, 97, 120], options) == {
        .Ok(_) -> panic("truncated array accepted")
        .Err(e) -> print(e.path == "$[1]" && e.reason == "CBOR value is missing")
    }

    roomy := cbor.CBOROptions.{ max_depth: 256, max_items: 1000000, max_bytes: 256, require_canonical: false }
    if cbor.parse([129, 130, 97, 120], ~roomy) == {
        .Ok(_) -> panic("nested truncation accepted")
        .Err(e) -> print(e.path == "$[0][1]" && e.reason == "CBOR value is missing")
    }
    if cbor.parse([162, 97, 97, 1, 97, 97, 2], ~roomy) == {
        .Ok(_) -> panic("duplicate key accepted")
        .Err(e) -> print(e.path == "$" && e.reason == "duplicate CBOR text map key")
    }
    if cbor.decode<[Int]>([129, 97, 120], roomy) == {
        .Ok(_) -> panic("typed mismatch accepted")
        .Err(e) -> print(e[0].path == "[0]" && e[0].reason.contains("expected Int"))
    }
    print(true)
}
"#;
    let path = dir.join("counted.jet");
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(source, &shown).unwrap_or_else(|diags| {
        panic!("front end rejected fixture:\n{}", jet::render_diagnostics(&shown, source, &diags))
    });
    let parse_renamed = out.rust.replacen("fn jet_enc_cbor_parse(", "fn jet_enc_cbor_parse_inner(", 1);
    assert_ne!(parse_renamed, out.rust, "generated CBOR parser seam changed");
    let renamed = parse_renamed.replacen("fn jet_enc_cbor_decode<T: user_Decode>(", "fn jet_enc_cbor_decode_inner<T: user_Decode>(", 1);
    assert_ne!(renamed, parse_renamed, "generated CBOR typed decoder seam changed");
    let allocator = r#"
mod jet_cbor_alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub struct CountingAlloc;
    static COUNTING: AtomicBool = AtomicBool::new(false);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    fn add(size: usize) {
        let live = LIVE.fetch_add(size, Ordering::SeqCst) + size;
        let mut peak = PEAK.load(Ordering::SeqCst);
        while live > peak {
            match PEAK.compare_exchange(peak, live, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(next) => peak = next,
            }
        }
    }
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = System.alloc(layout);
            if !ptr.is_null() && COUNTING.load(Ordering::SeqCst) { add(layout.size()); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if COUNTING.load(Ordering::SeqCst) { LIVE.fetch_sub(layout.size(), Ordering::SeqCst); }
            System.dealloc(ptr, layout);
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
            let counting = COUNTING.load(Ordering::SeqCst);
            if counting { LIVE.fetch_sub(old.size(), Ordering::SeqCst); }
            let next = System.realloc(ptr, old, new_size);
            if counting { if next.is_null() { add(old.size()); } else { add(new_size); } }
            next
        }
    }
    pub fn begin() { LIVE.store(0, Ordering::SeqCst); PEAK.store(0, Ordering::SeqCst); COUNTING.store(true, Ordering::SeqCst); }
    pub fn finish() -> usize { COUNTING.store(false, Ordering::SeqCst); PEAK.load(Ordering::SeqCst) }
}
#[global_allocator]
static JET_CBOR_ALLOC: jet_cbor_alloc_probe::CountingAlloc = jet_cbor_alloc_probe::CountingAlloc;
fn jet_enc_cbor_parse(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<jet_std::DataTree, jet_std::CBORError> {
    let ceiling = options.max_bytes as usize;
    jet_cbor_alloc_probe::begin();
    let result = jet_enc_cbor_parse_inner(bytes, options);
    let peak = jet_cbor_alloc_probe::finish();
    assert!(peak <= ceiling, "CBOR requested allocation peak {peak} exceeded {ceiling}");
    result
}
fn jet_enc_cbor_decode<T: user_Decode>(bytes: &Vec<u8>, options: jet_std::CBOROptions) -> Result<T, Vec<jet_std::FieldError>> {
    let ceiling = options.max_bytes as usize;
    jet_cbor_alloc_probe::begin();
    let result = jet_enc_cbor_decode_inner(bytes, options);
    let peak = jet_cbor_alloc_probe::finish();
    assert!(peak <= ceiling, "CBOR typed requested allocation peak {peak} exceeded {ceiling}");
    result
}
"#;
    let rs = dir.join("counted.rs");
    let bin = dir.join("counted");
    let generated = renamed.replacen("#![allow(warnings)]", "", 1);
    assert_ne!(generated, renamed, "generated crate attribute changed");
    fs::write(&rs, format!("#![allow(warnings)]\n{allocator}\n{generated}")).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output().unwrap();
    assert!(rustc.status.success(), "rustc rejected counted CBOR program:\n{}", String::from_utf8_lossy(&rustc.stderr));
    let run = Command::new(&bin).current_dir(&dir).output().unwrap();
    assert!(run.status.success(), "counted CBOR program failed:\n{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\ntrue\ntrue\ntrue\ntrue\n");
}

fn compile_temp(name: &str, src: &str) -> jet::CompileOutput {
    let dir = std::env::temp_dir().join(format!("jet_corelib_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    })
}

#[test]
fn invariant_refinement_proves_fixed_array_index() {
    let src = r#"
#Invariant("value >= 0 && value < 4")
Index4 :: distinct Int

fn pick(xs: [String#4], i: Index4) => String {
    return xs[i]
}

fn run() {
    words :: [String#4].{ "zero", "one", "two", "three" }
    print(pick(words, Index4.from_int(2)))
}
"#;
    let out = compile_temp("refinement_index.jet", src);
    assert!(
        !out.rust.contains("jet_index_vec(&"),
        "proof-carrying fixed-array index should not emit runtime list bounds helper:\n{}",
        out.rust
    );
}

#[test]
fn comptime_find_glob_records_sorted_lock_inputs() {
    let dir = std::env::temp_dir().join(format!(
        "jet_comptime_find_{}_{}",
        std::process::id(),
        "lock"
    ));
    fs::create_dir_all(dir.join("inputs/nested")).unwrap();
    fs::write(dir.join("inputs/alpha-1.txt"), "alpha").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.txt"), "beta").unwrap();
    fs::write(dir.join("inputs/nested/gamma-3.txt"), "gamma").unwrap();
    fs::write(dir.join("inputs/nested/beta-2.md"), "skip").unwrap();
    let src = r#"
$paths :: find("inputs/**/{{alpha,beta}}-[0-9].t?t")

fn run() {
    print(paths.join("|"))
}
"#;
    let path = dir.join("main.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected find fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let paths: Vec<&str> = out
        .comptime_inputs
        .iter()
        .map(|input| input.path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["inputs/alpha-1.txt", "inputs/nested/beta-2.txt"]
    );
    assert!(out
        .comptime_inputs
        .iter()
        .all(|input| input.hash.len() == 64));
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

#[test]
fn core_args_audit_surface_runs_and_reports_suggestions() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_args_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.args as args

fn run() {
    spec :: args.spec()
        .flag_short("verbose", "v", "print extra detail")
        .option_env("profile", "config profile", "NAME", "JET_ARGS_PROFILE")
        .option_int("jobs", "worker count", "N")
        .repeat("tag", "classification tag", "TAG")
    parsed :: spec.parse(["tool", "-vv", "--jobs", "8", "--tag", "a", "--tag=b"]) ?? panic("parse failed")
    print(parsed.flag("verbose"))
    print(parsed.option("profile") ?? "")
    print(parsed.option_int("jobs") ?? 0)
    print(parsed.options("tag").len())
    if spec.parse(["tool", "--verbse"]) == {
        .Ok(_) -> {
            print("unexpected")
        }
        .Err(e) -> {
            print(e)
        }
    }
}
"#;
    let (_code, stdout, stderr) = build_and_run(
        &dir,
        "args_audit",
        src,
        &[("JET_ARGS_PROFILE", "dev")],
        None,
    );
    assert!(
        stdout.contains("unknown option `--verbse`")
            && stdout.contains("did you mean `--verbose`?"),
        "core.args suggestion missing:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.starts_with("true\ndev\n8\n2\n"));
}

#[test]
fn core_args_parse_or_exit_handles_cli_boundaries_and_keeps_parse_pure() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_args_exit_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.args as args
use core.io as io

fn run() {
    spec :: args.spec()
        .flag("verbose", "print extra detail")
    parsed :: spec.parse_or_exit(io.args())
    embedded :: spec.parse(["embedded", "--verbose"]) ?? panic("pure parse failed")
    print(parsed.flag("verbose"))
    print(embedded.flag("verbose"))
}
"#;
    let path = dir.join("args_parse_or_exit.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected parse_or_exit fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rust = dir.join("args_parse_or_exit.rs");
    let bin = dir.join("args_parse_or_exit");
    fs::write(&rust, out.rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rust)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let normal = Command::new(&bin).arg("--verbose").output().unwrap();
    assert_eq!(normal.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&normal.stdout), "true\ntrue\n");
    assert!(normal.stderr.is_empty());

    let help = Command::new(&bin).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("Usage: args_parse_or_exit [options]"));
    assert!(help_stdout.contains("--help"));
    assert!(help.stderr.is_empty());

    let bad = Command::new(&bin).arg("--verbse").output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    let bad_stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(bad_stderr.contains("unknown option `--verbse`"));
    assert!(bad_stderr.contains("did you mean `--verbose`?"));
}

#[test]
fn core_args_nested_subcommand_does_not_overflow() {
    let src = r#"
use core.args as args

fn run() {
    serve :: args.spec()
        .option_int("port", "listen port", "PORT")
    spec :: args.spec()
        .flag_short("verbose", "v", "print extra detail")
        .option_int("jobs", "worker count", "N")
        .option_default("mode", "run mode", "MODE", "fast")
        .option_choice("color", "color policy", "WHEN", "auto,always,never")
        .repeat("tag", "classification tag", "TAG")
        .subcommand("serve", "run the server", serve)
        .version("args-audit 1.0")
    print(spec.help().contains("serve"))
}
"#;
    let out = compile_temp("args_nested_subcommand.jet", src);
    assert!(out.rust.contains("jet_args_subcommand"));
}

#[test]
fn core_os_facts_and_interrupt_hook_compile() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_os_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
    print(os.name().len() > 0)
    print(os.family().len() > 0)
    print(os.arch().len() > 0)
    print(os.cpu_count() >= 1)
    print(os.pid() >= 1)
    print(os.hostname().len() > 0)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "os_facts", src, &[], None);
    assert_eq!(code, 0, "core.os program failed: {stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn core_os_interrupt_prelude_is_emitted_only_when_used() {
    let facts_only = compile_temp(
        "os_facts_only.jet",
        r#"
use core.os as os

fn run() {
    print(os.name())
}
"#,
    );
    assert!(
        !facts_only.rust.contains("mod jet_os_interrupt")
            && !facts_only.rust.contains("SetConsoleCtrlHandler")
            && !facts_only.rust.contains("jet_std_os_on_interrupt"),
        "ordinary core.os facts should not inherit signal FFI"
    );
    assert!(
        facts_only.rust.contains("JET_INTERRUPT_HANDLER_DEPTH")
            && facts_only.rust.contains("fn jet_runtime_should_unwind()"),
        "safe central panic-boundary state must remain available without signal FFI"
    );

    let with_interrupt = compile_temp(
        "os_interrupt.jet",
        r#"
use core.os as os

fn run() {
    os.on_interrupt(() => {
        print("interrupted")
    })
}
"#,
    );
    assert!(
        with_interrupt.rust.contains("mod jet_os_interrupt")
            && with_interrupt.rust.contains("SetConsoleCtrlHandler")
            && with_interrupt.rust.contains("CTRL_C_EVENT")
            && with_interrupt.rust.contains("AtomicUsize")
            && with_interrupt.rust.contains("catch_unwind")
            && with_interrupt.rust.contains("struct PanicBoundary")
            && with_interrupt.rust.contains("impl Drop for PanicBoundary")
            && with_interrupt.rust.contains("#[cfg(not(any(unix, windows)))]")
            && with_interrupt.rust.contains("interrupt handling is unavailable on this target")
            && with_interrupt.rust.contains("jet_std_os_on_interrupt")
            && !with_interrupt.rust.contains("let _ = handler"),
        "on_interrupt should keep its Unix/Windows dispatcher and no silent no-op"
    );
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_handlers_are_additive_and_ordered() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!("jet_corelib_interrupt_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process

fn run() {
    os.on_interrupt(() => { panic("first handler failed") })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}

"#;
    let out = compile_temp("os_interrupt_runtime.jet", src);
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-runtime");
    fs::write(&rs, out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(rustc.status.success(), "rustc failed:\n{}", String::from_utf8_lossy(&rustc.stderr));

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n", "registration was not ready before run continued");
    unsafe extern "C" { fn kill(pid: i32, signal: i32) -> i32; }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let status = child.wait().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(status.success(), "interrupt child failed: {status}");
    assert_eq!(rest, "second\n");
}

#[cfg(unix)]
#[test]
fn core_os_interrupt_deadline_diagnostic_unwinds_inside_handler_boundary() {
    use std::io::{BufRead, Read};
    use std::process::Stdio;

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_interrupt_deadline_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.os as os
use core.process as process
use core.time as time

fn run() {
    os.on_interrupt(() => {
        #Context(deadline: time.now()) {
            time.sleep(5)
        }
    })
    os.on_interrupt(() => {
        print("second")
        process.exit(0)
    })
    print("ready")
    loop { }
}
"#;
    let out = compile_temp("os_interrupt_deadline.jet", src);
    assert!(
        out.rust.contains("jet_interrupt_handler_panic_enter")
            && out.rust.contains("jet_interrupt_handler_panic_leave"),
        "interrupt handlers need a boundary distinct from scheduler-task identity"
    );
    let rs = dir.join("main.rs");
    let bin = dir.join("interrupt-deadline");
    fs::write(&rs, out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc failed:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );

    let mut child = Command::new(&bin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    assert_eq!(unsafe { kill(child.id() as i32, 2) }, 0);
    let output = child.wait_with_output().unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(
        output.status.success(),
        "interrupt child failed: {}",
        output.status
    );
    assert_eq!(rest, "second\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error [E3003]: deadline exceeded while waiting in time sleep"));
    assert!(stderr.contains("Why: this wait point observed the task context deadline"));
    assert!(stderr.contains("Fix: raise the deadline budget or shorten the work"));
}

#[test]
fn core_os_interrupt_runtime_failures_use_the_boundary_aware_helpers() {
    let task_mem = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs");
    let time = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs");
    let scheduler = include_str!("../crates/jet-codegen/src/Prelude/Scheduler.rs");
    assert!(!task_mem.contains("process::exit(70)"));
    assert!(!time.contains("process::exit(70)"));
    assert_eq!(scheduler.matches("process::exit(70)").count(), 1);
    assert!(task_mem.contains("super::jet_panic(\"<core.tasks>\""));
    // The deadline helper binds the rendered E3003 to a local so it can feed
    // interrupt handlers, native wait boundaries, and explicitly typed deadline
    // tasks while ordinary scheduler tasks retain the exact process-fatal E3003
    // diagnostic. It still routes its fatal path through `jet_runtime_diagnostic`,
    // never `process::exit`.
    assert!(time.contains("jet_runtime_diagnostic(rendered)"));
    assert!(time.contains("jet_interrupt_handler_should_unwind()"));
    assert!(time.contains("jet_scheduler_wait_boundary_should_unwind()"));
    assert!(time.contains("jet_typed_deadline_boundary_should_unwind()"));
    assert!(scheduler.contains("fn jet_scheduler_fatal(msg: &str) -> !"));
    assert!(scheduler.contains("struct JetSchedulerWaitBoundary"));
    assert!(scheduler.contains("let _boundary = JetSchedulerWaitBoundary::enter()"));
    assert!(scheduler.contains("struct JetTypedDeadlineBoundary"));
    let (ordinary_task_spawn, typed_task_spawn) = task_mem
        .split_once("pub(crate) fn spawn_typed_deadline")
        .expect("typed-deadline task spawn must remain explicit");
    assert!(!ordinary_task_spawn.contains("JetTypedDeadlineBoundary::enter()"));
    let typed_task_spawn = typed_task_spawn
        .split_once("pub fn pause")
        .expect("typed-deadline task spawn boundary")
        .0;
    assert!(typed_task_spawn.contains(
        "let _typed_deadline_boundary = super::JetTypedDeadlineBoundary::enter()"
    ));
    let core = include_str!("../crates/jet-codegen/src/Prelude/Core.rs");
    assert!(core.contains("fn jet_runtime_should_unwind() -> bool"));
    assert!(core.contains("jet_scheduler_in_task() || jet_interrupt_handler_should_unwind()"));
    assert!(core.contains("if jet_runtime_should_unwind()"));
    assert!(core.contains("if jet_interrupt_handler_should_unwind()"));
}

fn build_and_run(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(&str, &str)],
    stdin: Option<&str>,
) -> (i32, String, String) {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(text) = stdin {
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let Some(mut input) = child.stdin.take() {
            use std::io::Write;
            input.write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        return (
            out.status.code().unwrap_or(0),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        );
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn build_and_run_multi(
    dir: &PathBuf,
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let src = fs::read_to_string(&entry_path).unwrap();
    let shown = entry_path.to_string_lossy();
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected multi-file fixture:\n{}",
            jet::render_diagnostics(&shown, &src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated multi-file code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn jet_string_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn dns_name_wire(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in name.trim_end_matches('.').split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

fn dns_fixture_response(query: &[u8]) -> Vec<u8> {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos += 1;
    let qtype = u16::from_be_bytes([query[pos], query[pos + 1]]);
    let question_end = pos + 4;
    let mut resp = Vec::new();
    resp.extend_from_slice(&query[0..2]);
    resp.extend_from_slice(&0x8180u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&query[12..question_end]);
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u32.to_be_bytes());
    let rdata = match qtype {
        16 => {
            let mut r = Vec::new();
            r.push(3);
            r.extend_from_slice(b"jet");
            r
        }
        33 => {
            let mut r = Vec::new();
            r.extend_from_slice(&10u16.to_be_bytes());
            r.extend_from_slice(&20u16.to_be_bytes());
            r.extend_from_slice(&443u16.to_be_bytes());
            r.extend_from_slice(&dns_name_wire("srv.example.test"));
            r
        }
        _ => Vec::new(),
    };
    resp.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    resp.extend_from_slice(&rdata);
    resp
}

fn dns_question_end(query: &[u8]) -> usize {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos + 5
}

fn dns_truncated_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8380u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    // A TC response may end inside a declared record. The client must validate
    // the authenticated header/question, then retry over TCP without parsing
    // an explicitly incomplete UDP record body.
    response.push(0xc0);
    response
}

fn dns_cname_additional_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let alias = dns_name_wire("alias.example.test");
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8180u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    response.extend_from_slice(&[0xc0, 0x0c]);
    response.extend_from_slice(&5u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&(alias.len() as u16).to_be_bytes());
    response.extend_from_slice(&alias);
    response.extend_from_slice(&alias);
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&4u16.to_be_bytes());
    response.extend_from_slice(&[192, 0, 2, 42]);
    response
}

fn bind_dns_dual_protocol_fixture() -> (
    std::net::TcpListener,
    std::net::UdpSocket,
    std::net::SocketAddr,
) {
    const MAX_ATTEMPTS: usize = 64;

    for attempt in 1..=MAX_ATTEMPTS {
        let tcp = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(tcp) => tcp,
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempt < MAX_ATTEMPTS =>
            {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => break,
            Err(error) => panic!("failed to bind DNS fixture TCP listener: {error}"),
        };
        let addr = tcp.local_addr().unwrap_or_else(|error| {
            panic!("failed to read DNS fixture TCP listener address: {error}")
        });
        match std::net::UdpSocket::bind(addr) {
            Ok(udp) => return (tcp, udp, addr),
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempt < MAX_ATTEMPTS =>
            {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => break,
            Err(error) => panic!("failed to bind DNS fixture UDP socket at {addr}: {error}"),
        }
    }

    panic!("failed to reserve one TCP/UDP DNS fixture port after {MAX_ATTEMPTS} attempts")
}

#[test]
fn core_net_dns_txt_and_srv_are_real_udp_queries() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut buf = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut buf).unwrap();
            let resp = dns_fixture_response(&buf[..n]);
            socket.send_to(&resp, peer).unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    txts :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("txt")
    print(txts[0])
    srvs :: net.dns_srv_at("{}", "_jet._tcp.example.test", 1000) ?? panic("srv")
    print("{{net.dns_srv_target(srvs[0])}}:{{net.dns_srv_port(srvs[0])}}")
}}
"#,
        addr, addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_fixture", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "jet\nsrv.example.test:443\n");
}

#[test]
fn core_net_dns_udp_truncation_retries_tcp_and_reads_cname_additional() {
    use std::io::{Read, Write};

    let (tcp, udp, addr) = bind_dns_dual_protocol_fixture();
    let server = std::thread::spawn(move || {
        let mut udp_query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut udp_query).unwrap();
        udp.send_to(&dns_truncated_response(&udp_query[..n]), peer)
            .unwrap();

        let (mut stream, _) = tcp.accept().unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut tcp_query = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut tcp_query).unwrap();
        let response = dns_cname_additional_response(&tcp_query);
        stream
            .write_all(&(response.len() as u16).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_tcp_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    ips :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("dns")
    print(net.ip_to_string(ips[0]))
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_tcp_cname", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "192.0.2.42\n");
}

#[test]
fn core_net_dns_timeout_is_one_budget_across_udp_and_tcp() {
    use std::io::Read;

    let (tcp, udp, addr) = bind_dns_dual_protocol_fixture();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = udp.recv_from(&mut query).unwrap();
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(70));
        udp.send_to(&dns_truncated_response(&query[..n]), peer).unwrap();
        let (mut stream, _) = tcp.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_millis(500))).unwrap();
        let mut prefix = [0u8; 2];
        stream.read_exact(&mut prefix).unwrap();
        let mut request = vec![0u8; u16::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut request).unwrap();
        let mut closed = [0u8; 1];
        let _ = stream.read(&mut closed);
        started.elapsed()
    });
    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_budget_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 120) ?? panic("dns total timeout")
}}
"#, addr);
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_total_budget", &src, &[], None);
    let elapsed = server.join().unwrap();
    assert_ne!(code, 0, "stalled DNS TCP fallback unexpectedly succeeded");
    assert!(stderr.contains("dns total timeout"), "{stderr}");
    assert!(elapsed < std::time::Duration::from_millis(190), "UDP and TCP each received a fresh timeout: {elapsed:?}");
}

#[test]
fn core_net_dns_platform_resolver_policy_uses_native_sources() {
    let net = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
    assert!(net.contains("#[cfg(target_os = \"linux\")]"));
    assert!(net.contains("read_to_string(\"/etc/resolv.conf\")"));
    assert!(net.contains("#[cfg(target_os = \"macos\")]"));
    assert!(net.contains("Command::new(\"scutil\").arg(\"--dns\")"));
    assert!(net.contains("#[cfg(windows)]"));
    assert!(net.contains("Get-DNSClientServerAddress"));
    assert!(net.contains("$_.ServerAddresses"));
    assert!(!net.contains("Command::new(\"ipconfig\")"));
    assert!(!net.contains("1.1.1.1"));
}

#[test]
fn core_net_dns_platform_resolver_parsers_accept_native_fixtures() {
    assert_eq!(
        dns_resolver_policy::resolv_conf(
            "# generated\nnameserver 192.0.2.53 # vpn\nnameserver 2001:db8::53\nsearch example.test\n"
        ),
        ["192.0.2.53:53", "[2001:db8::53]:53"]
    );
    assert_eq!(
        dns_resolver_policy::scutil(
            "resolver #1\n  nameserver[0] : 192.0.2.54\n  nameserver[1] : 2001:db8::54\n  search domain[0] : example.test\n"
        ),
        ["192.0.2.54:53", "[2001:db8::54]:53"]
    );
    assert_eq!(
        dns_resolver_policy::windows("{192.0.2.55, 2001:db8::55}\r\n\r\n"),
        ["192.0.2.55:53", "[2001:db8::55]:53"]
    );
}

#[test]
fn core_net_dns_platform_resolver_parsers_reject_noise_and_malformed_entries() {
    assert!(dns_resolver_policy::resolv_conf(
        "nameserver nope\nnot-nameserver 192.0.2.1\nnameserver [broken\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::scutil(
        "nameserver[x] : 192.0.2.1\nnameserver[0 : 192.0.2.2\nnameserver[] : 192.0.2.3\n"
    )
    .is_empty());
    assert!(dns_resolver_policy::windows(
        "InterfaceAlias Ethernet\nServerAddresses nope, 999.1.1.1\n"
    )
    .is_empty());
}

#[test]
fn core_net_dns_rejects_wrong_transaction_id() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        let mut response = dns_fixture_response(&query[..n]);
        let wrong = u16::from_be_bytes([response[0], response[1]]).wrapping_add(1);
        response[0..2].copy_from_slice(&wrong.to_be_bytes());
        socket.send_to(&response, peer).unwrap();
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_bad_id_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("forged DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_bad_id", &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "forged transaction ID was accepted");
    assert!(stderr.contains("forged DNS accepted"), "{stderr}");
}

#[test]
fn core_net_dns_transaction_ids_are_not_a_fixed_sequence() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..9 {
            let mut query = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut query).unwrap();
            ids.insert(u16::from_be_bytes([query[0], query[1]]));
            socket
                .send_to(&dns_fixture_response(&query[..n]), peer)
                .unwrap();
        }
        ids
    });

    let dir = std::env::temp_dir().join(format!("jet_core_net_dns_ids_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    loop _i, 0..8 {{
        _ :: net.dns_txt_at("{}", "service.example.test", 1000) ?? panic("dns")
    }}
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, "dns_ids", &src, &[], None);
    let ids = server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert!(ids.len() > 1, "all nine DNS queries reused one transaction ID");
}

fn run_rejected_dns_response(tag: &str, make_response: fn(&[u8]) -> Vec<u8>) -> String {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut query = [0u8; 512];
        let (n, peer) = socket.recv_from(&mut query).unwrap();
        socket
            .send_to(&make_response(&query[..n]), peer)
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_reject_{}_{}",
        tag,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    _ :: net.dns_a_at("{}", "service.example.test", 1000) ?? panic("invalid DNS accepted")
}}
"#,
        addr
    );
    let (code, _stdout, stderr) = build_and_run(&dir, tag, &src, &[], None);
    server.join().unwrap();
    assert_ne!(code, 0, "invalid DNS response was accepted: {tag}");
    stderr
}

#[test]
fn core_net_dns_rejects_non_response_and_cyclic_compression() {
    fn non_response(query: &[u8]) -> Vec<u8> {
        let mut response = dns_fixture_response(query);
        response[2] &= 0x7f;
        response
    }
    fn cyclic_compression(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let record_start = end;
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response.push(0xc0 | ((record_start >> 8) as u8 & 0x3f));
        response.push(record_start as u8);
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 0, 2, 1]);
        response
    }

    let qr = run_rejected_dns_response("dns_not_response", non_response);
    assert!(qr.contains("invalid DNS accepted"), "{qr}");
    let cycle = run_rejected_dns_response("dns_pointer_cycle", cyclic_compression);
    assert!(cycle.contains("invalid DNS accepted"), "{cycle}");
}

#[test]
fn core_net_dns_rejects_reserved_header_forward_pointer_and_impossible_counts() {
    fn reserved_header(query: &[u8]) -> Vec<u8> {
        let mut response = dns_cname_additional_response(query);
        response[3] |= 0x40;
        response
    }
    fn forward_pointer(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let record_start = end;
        let pointer_target = record_start + 6;
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response.push(0xc0 | ((pointer_target >> 8) as u8 & 0x3f));
        response.push(pointer_target as u8);
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 0, 2, 1]);
        response
    }
    fn impossible_counts(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8180u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&u16::MAX.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response
    }

    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let responses: [fn(&[u8]) -> Vec<u8>; 3] =
            [reserved_header, forward_pointer, impossible_counts];
        for response in responses {
            let mut query = [0u8; 512];
            let (n, peer) = socket.recv_from(&mut query).unwrap();
            socket.send_to(&response(&query[..n]), peer).unwrap();
        }
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_hostile_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net

fn run() {{
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("reserved DNS header accepted")
        .Err(_) -> print("rejected")
    }}
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("forward DNS pointer accepted")
        .Err(_) -> print("rejected")
    }}
    if net.dns_a_at("{0}", "service.example.test", 1000) == {{
        .Ok(_) -> panic("impossible DNS counts accepted")
        .Err(_) -> print("rejected")
    }}
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_hostile_bounds", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "rejected\nrejected\nrejected\n");
}

#[test]
fn core_net_dns_wire_lookup_observes_task_cancellation() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_dns_cancel_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {{
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    lookup :: tasks.spawn(() => {{
        ready_tx.send(1)
        if net.dns_a_at("{}", "service.example.test", 5000) == {{
            .Ok(_) -> print("unexpected DNS response")
            .Err(error) -> print(net.error_message(error))
        }}
    }})
    _ready :: ready_rx.receive() ?? panic("ready")
    time.sleep(50)
    lookup.cancel()
    lookup.join()
}}
"#,
        addr
    );
    let (code, stdout, stderr) = build_and_run(&dir, "dns_task_cancel", &src, &[], None);
    drop(socket);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "network operation cancelled during DNS lookup for `service.example.test`\n");
}

#[test]
fn core_net_dns_nxdomain_is_an_error() {
    fn nxdomain(query: &[u8]) -> Vec<u8> {
        let end = dns_question_end(query);
        let mut response = Vec::new();
        response.extend_from_slice(&query[0..2]);
        response.extend_from_slice(&0x8183u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&query[12..end]);
        response
    }

    let stderr = run_rejected_dns_response("dns_nxdomain", nxdomain);
    assert!(stderr.contains("invalid DNS accepted"), "{stderr}");
}

#[test]
fn core_net_ratified_named_forms_require_exact_labels() {
    let cases = [
        ("tcp accept", "fn check(listener: TcpListener, d: Duration) { result :: listener.accept(d) }", "deadline:"),
        ("tcp read", "fn check(stream: TcpStream, d: Duration) { result :: stream.read(1, banana: d) }", "deadline:"),
        ("tcp read text", "fn check(stream: TcpStream, d: Duration) { result :: stream.read_text(1, d) }", "deadline:"),
        ("tcp write", "fn check(stream: TcpStream, d: Duration) { result :: stream.write([1], potato: d) }", "deadline:"),
        ("tcp write all", "fn check(stream: TcpStream, d: Duration) { result :: stream.write_all([1], d) }", "deadline:"),
        ("tcp write text", "fn check(stream: TcpStream, d: Duration) { result :: stream.write_text(\"x\", turnip: d) }", "deadline:"),
        ("tcp ready", "fn check(stream: TcpStream, d: Duration) { result :: stream.ready(.Read, d) }", "deadline:"),
        ("udp send", "fn check(socket: UdpSocket, address: SocketAddr, d: Duration) { result :: socket.send_to([1], address, banana: d) }", "deadline:"),
        ("udp receive", "fn check(socket: UdpSocket, d: Duration) { result :: socket.receive(1, d) }", "deadline:"),
        ("udp ready", "fn check(socket: UdpSocket, d: Duration) { result :: socket.ready(.Read, potato: d) }", "deadline:"),
        ("unix connect", "fn check(d: Duration) { result :: net.unix_connect(\"/tmp/jet-label-test\", d) }", "deadline:"),
        ("unix accept", "fn check(listener: UnixListener, d: Duration) { result :: listener.accept(banana: d) }", "deadline:"),
        ("unix read", "fn check(stream: UnixStream, d: Duration) { result :: stream.read(1, d) }", "deadline:"),
        ("unix write", "fn check(stream: UnixStream, d: Duration) { result :: stream.write_all([1], potato: d) }", "deadline:"),
        ("unix ready", "fn check(stream: UnixStream, d: Duration) { result :: stream.ready(.Write, d) }", "deadline:"),
        ("tls read", "fn check(stream: TLSStream, d: Duration) { result :: stream.read(1, banana: d) }", "deadline:"),
        ("tls write", "fn check(stream: TLSStream, d: Duration) { result :: stream.write_all([1], d) }", "deadline:"),
        ("tls ready", "fn check(stream: TLSStream, d: Duration) { result :: stream.ready(.Read, potato: d) }", "deadline:"),
        ("tls close write", "fn check(stream: TLSStream, d: Duration) { result :: stream.close_write(d) }", "deadline:"),
        ("tls version bounds", "fn check() { result :: tls.ClientConfig.default().with_version_bounds(.Tls12, .Tls13) }", "min:"),
        ("tls client identity", "fn check() { result :: tls.ClientIdentity.from_pem([], []) }", "cert_chain:"),
        (
            "tls client",
            "fn check(stream: TcpStream, d: Duration) { cfg :: tls.ClientConfig.default(); result :: tls.client(^stream, banana: \"localhost\", potato: cfg, turnip: d) }",
            "server_name:",
        ),
    ];
    for (name, body, expected_fix) in cases {
        let source = format!("use core.net as net\nuse core.tls as tls\n{body}\n");
        let diags = jet::compile(&source).expect_err(name);
        assert!(
            diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(expected_fix)),
            "{name} did not reject its missing/wrong label precisely: {diags:?}",
        );
        if name == "tls client" {
            for label in ["server_name:", "config:", "deadline:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "tls.client accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
        if name == "tls version bounds" {
            for label in ["min:", "max:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "with_version_bounds accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
        if name == "tls client identity" {
            for label in ["cert_chain:", "private_key:"] {
                assert!(
                    diags.iter().any(|diag| matches!(diag.code.as_str(), "E0764" | "E0769") && diag.fix.contains(label)),
                    "ClientIdentity.from_pem accepted or misreported `{label}`: {diags:?}",
                );
            }
        }
    }
}

#[test]
fn core_net_tcp_read_uses_scheduler_and_returns_typed_cancellation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_cancel_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_cancel",
        r#"
use core.net as net
use core.tasks as tasks

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    server :: tasks.spawn(() => {
        stream := net.tcp_accept(listener) ?? panic("accept")
        ready_tx.send(1)
        if stream.read(1) == {
            .Ok(_) -> print("unexpected read")
            .Err(error) -> print(net.error_message(error))
        }
    })
    _client :: net.tcp_connect(address) ?? panic("connect")
    _ready :: ready_rx.receive() ?? panic("ready")
    server.cancel()
    server.join()
}

"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp read cancelled\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_accept_and_ready_are_scheduler_interrupt_points() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_accept_ready_interrupts_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_accept_ready_interrupts",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    cancelled_listener :: net.tcp_listen("127.0.0.1:0") ?? panic("cancel listen")
    cancelled_address :: net.socket_to_string(net.listener_local_socket_addr(cancelled_listener) ?? panic("cancel address"))
    (accept_tx, accept_rx) :: tasks.channel<Int>()
    cancelled_accept :: tasks.spawn(() => {
        accept_tx.send(1)
        if cancelled_listener.accept() == {
            .Ok(_) -> print("accept unexpectedly succeeded")
            .Err(error) -> print(net.error_message(error))
        }
    })
    _accept_ready :: accept_rx.receive() ?? panic("accept ready")
    time.sleep(10)
    cancelled_accept.cancel()
    time.sleep(10)
    release_accept :: net.tcp_connect(cancelled_address) ?? panic("release accept")
    release_accept.close() ?? panic("release close")
    cancelled_accept.join()

    ready_listener :: net.tcp_listen("127.0.0.1:0") ?? panic("ready listen")
    ready_address :: net.socket_to_string(net.listener_local_socket_addr(ready_listener) ?? panic("ready address"))
    ready_client :: net.tcp_connect(ready_address) ?? panic("ready connect")
    ready_server := net.tcp_accept(ready_listener) ?? panic("ready accept")
    write_interest :: NetReadyInterest.Write
    write_ready :: ready_server.ready(write_interest, deadline: Duration.milliseconds(1000) ?? panic("write ready deadline")) ?? panic("write ready")
    print(net.ready_readable(write_ready))
    print(net.ready_writable(write_ready))
    interest :: NetReadyInterest.Read
    (wait_tx, wait_rx) :: tasks.channel<Int>()
    ready_wait :: tasks.spawn(() => {
        wait_tx.send(1)
        if ready_server.ready(interest, deadline: Duration.milliseconds(1000) ?? panic("ready deadline")) == {
            .Ok(_) -> print("ready unexpectedly succeeded")
            .Err(error) -> print(net.error_message(error))
        }
    })
    _wait_ready :: wait_rx.receive() ?? panic("wait ready")
    time.sleep(10)
    ready_wait.cancel()
    ready_wait.join()
    ready_client.close() ?? panic("ready client close")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp accept cancelled\nfalse\ntrue\ntcp ready cancelled\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_udp_loopback_preserves_datagram_truncation_metadata() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_udp_truncation_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_udp_truncation",
        r#"
use core.net as net

fn run() {
    server :: net.udp_bind("127.0.0.1:0") ?? panic("server bind")
    client :: net.udp_bind("127.0.0.1:0") ?? panic("client bind")
    address :: net.udp_local_addr(server) ?? panic("server address")
    budget :: Duration.seconds(1) ?? panic("deadline")
    payload :: [U8].{ 0, 255, 1, 2, 3 }
    sent :: client.send_to(payload, address, deadline: budget) ?? panic("send")
    packet :: server.receive(3, deadline: budget) ?? panic("receive")
    print("{sent}:{net.udp_packet_bytes(packet)}:{net.udp_packet_original_len(packet)}:{net.udp_packet_truncated(packet)}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "5:[0, 255, 1]:5:true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_udp_same_handle_readiness_cancels_and_close_is_idempotent() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_udp_ready_close_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_udp_ready_close",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    socket :: net.udp_bind("127.0.0.1:0") ?? panic("bind")
    interest :: NetReadyInterest.Read
    (ready_tx, ready_rx) :: tasks.channel<Int>()
    waiter :: tasks.spawn(() => {
        ready_tx.send(1)
        if socket.ready(interest, deadline: Duration.seconds(1) ?? panic("deadline")) == {
            .Ok(_) -> panic("udp unexpectedly ready")
            .Err(error) -> print(net.error_message(error))
        }
    })
    _ready :: ready_rx.receive() ?? panic("ready")
    time.sleep(10)
    waiter.cancel()
    waiter.join()

    closed :: net.udp_bind("127.0.0.1:0") ?? panic("closed bind")
    closed.close() ?? panic("close")
    closed.close() ?? panic("second close")
    if net.udp_receive(closed, 1) == {
        .Ok(_) -> panic("closed receive succeeded")
        .Err(error) -> print(net.error_message(error))
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "udp ready cancelled\nudp receive failed: socket is closed\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_happy_eyeballs_uses_one_deadline_and_live_loopback() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_happy_eyeballs_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_happy_eyeballs",
        r#"
use core.net as net

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    address :: net.listener_local_socket_addr(listener) ?? panic("address")
    if net.tcp_connect_timeout(address, 0) == {
        .Ok(_) -> panic("expired connect succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    client :: net.tcp_connect_happy("localhost", net.socket_port(address), 1000) ?? panic("happy connect")
    server := listener.accept() ?? panic("accept")
    client.write_text("happy") ?? panic("write")
    print(server.read_text(5) ?? panic("read"))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp connect\nhappy\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_tcp_per_call_deadlines_bound_accept_read_and_write() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_per_call_deadlines_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_per_call_deadlines",
        r#"
use core.net as net

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    expired :: Duration.milliseconds(0) ?? panic("duration")
    if listener.accept(deadline: expired) == {
        .Ok(_) -> panic("expired accept succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    client := net.tcp_connect(address) ?? panic("connect")
    server := listener.accept() ?? panic("accept")
    if server.read(1, deadline: expired) == {
        .Ok(_) -> panic("expired read succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
    byte :: [U8].{ 1 }
    if client.write(byte, deadline: expired) == {
        .Ok(_) -> panic("expired write succeeded")
        .Err(error) -> print(net.error_operation(error))
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "tcp accept\ntcp read\ntcp write\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_net_tcp_implements_nominal_io_reader_writer() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_io_contract_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_io_contract",
        r#"
use core.net as net
use core.tasks as tasks

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}

fn send_four<T: Writer>(&stream: T) => Int ? IOError {
    stream.write_all([1, 2, 3, 4])?
    return .Ok(4)
}

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: tasks.spawn(() => {
        stream := net.tcp_accept(listener) ?? panic("accept")
        if receive(&stream, 0) == {
            .Ok(_) -> panic("zero limit looked like EOF")
            .Err(_) -> print("invalid")
        }
        bytes :: receive(&stream, 4) ?? panic("read")
        print("read:{bytes.len()}")
        eof :: receive(&stream, 4) ?? panic("eof")
        if eof.len() == 0 { print("eof") }
    })
    client := net.tcp_connect(address) ?? panic("connect")
    _count :: send_four(&client) ?? panic("write")
    client.close() ?? panic("close")
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "invalid\nread:4\neof\n");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_net_unix_stream_implements_nominal_io_reader_writer() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_unix_io_contract_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("stream.sock"));
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {{
    return stream.read(limit)
}}

fn send_four<T: Writer>(&stream: T) => Int ? IOError {{
    first :: stream.write([1, 2])?
    stream.write_all([3, 4])?
    return .Ok(first)
}}

fn run() {{
    listener :: net.unix_listen("{socket}") ?? panic("listen")
    server :: tasks.spawn(() => {{
        stream := net.unix_accept(listener) ?? panic("accept")
        if receive(&stream, 0) == {{
            .Ok(_) -> panic("zero limit looked like EOF")
            .Err(error) -> {{
                if error == {{
                    .InvalidInput(context) -> print(if context.operation == .Read {{ "invalid" }} else {{ "wrong-operation" }})
                    else -> {{ print("wrong-error") }}
                }}
            }}
        }}
        first :: receive(&stream, 2) ?? panic("first read")
        second :: receive(&stream, 2) ?? panic("second read")
        print("read:{{first.len()}}+{{second.len()}}")
        eof :: receive(&stream, 2) ?? panic("eof")
        if eof.len() == 0 {{ print("eof") }}
        net.unix_write_all_bytes(&stream, [9]) ?? panic("reply")
        net.unix_close(&stream) ?? panic("server close")
    }})
    client := net.unix_connect("{socket}") ?? panic("connect")
    first_count :: send_four(&client) ?? panic("write")
    print("wrote:{{first_count}}")
    net.unix_shutdown(&client, .Write) ?? panic("half close")
    reply :: receive(&client, 1) ?? panic("reply")
    print("reply:{{reply.len()}}")
    if net.unix_write_all_bytes(&client, [5]) == {{
        .Ok(_) -> panic("write after half-close succeeded")
        .Err(error) -> print(if net.error_operation(error) == "unix write" {{ "half-closed" }} else {{ "wrong-half-close" }})
    }}
    net.unix_close(&client) ?? panic("close")
    net.unix_close(&client) ?? panic("second close")
    if receive(&client, 1) == {{
        .Ok(_) -> panic("closed read succeeded")
        .Err(error) -> {{
            if error == {{
                .Closed(context) -> print(if context.operation == .Read {{ "closed" }} else {{ "wrong-close-operation" }})
                else -> {{ print("wrong-close-error") }}
            }}
        }}
    }}
    server.join()
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "unix_io_contract", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["closed", "eof", "half-closed", "invalid", "read:2+2", "reply:1", "wrote:2"]);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn core_net_unix_same_handle_deadline_readiness_and_close() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_unix_same_handle_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("same-handle.sock"));
    let source = format!(
        r#"
use core.net as net

fn run() {{
    listener :: net.unix_listen("{socket}") ?? panic("listen")
    budget :: Duration.seconds(1) ?? panic("budget")
    client := net.unix_connect("{socket}", deadline: budget) ?? panic("connect")
    server := listener.accept(deadline: budget) ?? panic("accept")
    client.set_timeout(budget) ?? panic("persistent timeout")
    both :: NetReadyInterest.ReadWrite
    observed :: client.ready(both, deadline: budget) ?? panic("read-write readiness")
    print(net.ready_readable(observed))
    print(net.ready_writable(observed))
    interest :: NetReadyInterest.Read
    expired :: Duration.milliseconds(0) ?? panic("expired")
    if client.ready(interest, deadline: expired) == {{
        .Ok(_) -> panic("expired readiness succeeded")
        .Err(error) -> print(net.error_operation(error))
    }}
    payload :: [U8].{{ 7 }}
    client.write_all(payload, deadline: budget) ?? panic("write")
    print(server.read(1, deadline: budget) ?? panic("read"))
    client.close() ?? panic("close")
    client.close() ?? panic("second close")
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "unix_same_handle", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\ntrue\nunix ready\n[7]\n");
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn core_net_udp_and_unix_waits_use_typed_scheduler_interrupts() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_datagram_unix_interrupts_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let socket = jet_string_path(&dir.join("interrupt.sock"));
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks

fn run() {{
    udp_timeout :: net.udp_bind("127.0.0.1:0") ?? panic("udp timeout bind")
    net.udp_set_timeout(udp_timeout, 20) ?? panic("udp timeout")
    if net.udp_receive(udp_timeout, 8) == {{
        .Ok(_) -> panic("udp timeout returned data")
        .Err(error) -> print(net.error_message(error))
    }}

    udp :: net.udp_bind("127.0.0.1:0") ?? panic("udp bind")
    (udp_ready_tx, udp_ready_rx) :: tasks.channel<Int>()
    udp_wait :: tasks.spawn(() => {{
        udp_ready_tx.send(1)
        if net.udp_receive(udp, 8) == {{
            .Ok(_) -> panic("udp cancel returned data")
            .Err(error) -> print(net.error_message(error))
        }}
    }})
    _udp_ready :: udp_ready_rx.receive() ?? panic("udp ready")
    udp_wait.cancel()
    udp_wait.join()

    listener :: net.unix_listen("{socket}") ?? panic("unix listen")
    (unix_ready_tx, unix_ready_rx) :: tasks.channel<Int>()
    unix_wait :: tasks.spawn(() => {{
        unix_ready_tx.send(1)
        if net.unix_accept(listener) == {{
            .Ok(_) -> panic("unix cancel accepted stream")
            .Err(error) -> print(net.error_message(error))
        }}
    }})
    _unix_ready :: unix_ready_rx.receive() ?? panic("unix ready")
    unix_wait.cancel()
    unix_wait.join()
}}
"#
    );
    let (code, stdout, stderr) =
        build_and_run(&dir, "net_datagram_unix_interrupts", &source, &[], None);
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        ["deadline exceeded while waiting in udp receive", "udp receive cancelled", "unix accept cancelled"]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_ioerror_preserves_kind_operation_and_resource() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_tree_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.files as fs
use core.net as net
use core.process as process

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}

fn operation_name(operation: IOOperation) => String {
    if operation == {
        .Read -> return "read"
        .Write -> return "write"
        .Flush -> return "flush"
        .Connect -> return "connect"
        .Accept -> return "accept"
        .Close -> return "close"
        .Resolve -> return "resolve"
        .Codec -> return "codec"
    }
}

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    address :: net.socket_to_string(net.listener_local_socket_addr(listener) ?? panic("address"))
    client := net.tcp_connect(address) ?? panic("connect")
    if receive(&client, 0) == {
        .Ok(_) -> panic("zero read succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "read" -> "invalid-read" else -> "invalid-other")
                else -> { print("other") }
            }
        }
    }
    if fs.read("definitely-missing/ioerror-tree") == {
        .Ok(_) -> panic("missing file read succeeded")
        .Err(error) -> {
            if error == {
                .NotFound(context) -> print(context.resource ?? "missing-resource")
                else -> { print("other") }
            }
        }
    }
    if fs.write(".", "cannot replace directory") == {
        .Ok(_) -> panic("directory write succeeded")
        .Err(error) -> {
            if error == {
                .Other(context) -> print(if operation_name(context.operation) == "write" -> "write" else -> "wrong-write-operation")
                else -> { print("wrong-write-kind") }
            }
        }
    }
    if process.cmd([]).run() == {
        .Ok(_) -> panic("empty command succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> "empty-command" else -> "wrong-command-operation")
                else -> { print("wrong-command-kind") }
            }
        }
    }
    if process.pipeline([]) == {
        .Ok(_) -> panic("empty pipeline succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> "empty-pipeline" else -> "wrong-pipeline-operation")
                else -> { print("wrong-pipeline-kind") }
            }
        }
    }
    if process.cmd(["unused"]).env("BAD=NAME", "value").run() == {
        .Ok(_) -> panic("invalid environment succeeded")
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> print(if operation_name(context.operation) == "resolve" -> context.resource ?? "missing-env-resource" else -> "wrong-env-operation")
                else -> { print("wrong-env-kind") }
            }
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ioerror_tree",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    let expected = "invalid-read\ndefinitely-missing/ioerror-tree\nwrite\nempty-command\nempty-pipeline\nBAD=NAME\n";
    assert_eq!(stdout, expected);
    let file = dir.join("ioerror_tree.jet");
    fs::write(&file, source).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected, ""));
        }
        other => panic!("IOError tree did not run in default dev: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_ioerror_debug_renders_in_aot_and_dev() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_debug_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // Direct core values alone do not emit `jet_std`; this unused helper keeps the AOT prelude present.
    let source = r#"
fn activate_core() => String {
    return input() ?? ""
}

fn fail() => Int ? IOError {
    return .Err(IOError.InvalidInput(IOContext.{
        operation: .Resolve,
        resource: None,
        os_code: None,
        cause: Val("debug"),
    }))
}

fn fail_other() => Int ? IOError {
    return .Err(IOError.Other(IOContext.{
        cause: Val("denied"),
        os_code: Val(13),
        resource: Val("out.txt"),
        operation: .Write,
    }))
}

fn run() {
    if fail() == {
        .Ok(_) -> panic("failure succeeded")
        .Err(error) -> print("{error#Debug}")
    }
    if fail_other() == {
        .Ok(_) -> panic("other failure succeeded")
        .Err(error) -> print("{error#Debug}")
    }
}
"#;
    let expected_aot = concat!(
        "InvalidInput(IOContext { operation: Resolve, resource: None, os_code: None, cause: Some(\"debug\") })\n",
        "Other(IOContext { operation: Write, resource: Some(\"out.txt\"), os_code: Some(13), cause: Some(\"denied\") })\n",
    );
    let expected_dev = expected_aot;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ioerror_debug",
        source,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, expected_aot);
    let file = dir.join("ioerror_debug.jet");
    fs::write(&file, source).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout.as_str(), stderr.as_str()), (0, expected_dev, ""));
        }
        other => panic!("IOError Debug did not run in default dev: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn core_ioerror_native_flush_preserves_operation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_ioerror_flush_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "ioerror_flush",
        r#"
use core.files as files

fn run() {
    output := files.create("/dev/full") ?? panic("open")
    output.write_line("buffered") ?? panic("buffer")
    if output.flush() == {
        .Ok(_) -> panic("flush succeeded")
        .Err(error) -> {
            if error == {
                .Other(context) -> print(if context.operation == .Flush { "flush" } else { "wrong-flush-operation" })
                else -> { print("wrong-flush-kind") }
            }
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "flush\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_read_persistent_timeout_uses_scheduler_budget() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_task_timeout_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_task_timeout",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    client :: tasks.spawn(() => {
        stream := net.tcp_connect(address) ?? panic("connect")
        time.sleep(100)
        stream.close() ?? panic("close")
    })
    stream := net.tcp_accept(listener) ?? panic("accept")
    net.set_read_timeout(&stream, 20) ?? panic("timeout")
    if stream.read(1) == {
        .Ok(_) -> print("unexpected read")
        .Err(error) -> print(net.error_message(error))
    }
    client.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("deadline exceeded while waiting in tcp read"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_expired_deadlines_return_typed_timeouts() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_expired_deadlines_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_expired_deadlines",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: tasks.spawn(() => {
        first := net.tcp_accept(listener) ?? return
        time.sleep(100)
        first.close() ?? return
        second := net.tcp_accept(listener) ?? return
        time.sleep(100)
        second.close() ?? return
    })

    first := net.tcp_connect(address) ?? panic("first connect")
    net.set_read_timeout(&first, 0) ?? panic("zero timeout")
    if first.read(1) == {
        .Ok(_) -> print("unexpected first read")
        .Err(error) -> print(net.error_message(error))
    }
    first.close() ?? panic("first close")

    second := net.tcp_connect(address) ?? panic("second connect")
    #Context(deadline: time.now() - 1) {
        if second.read(1) == {
            .Ok(_) -> print("unexpected second read")
            .Err(error) -> print(net.error_message(error))
        }
    }
    second.close() ?? panic("second close")
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "deadline exceeded while waiting in tcp read\ndeadline exceeded while waiting in tcp read\n"
    );
    assert!(!stderr.contains("E3003"), "typed timeout escaped as runtime deadline: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_net_tcp_write_all_uses_one_absolute_deadline() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_net_write_deadline_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "net_write_deadline",
        r#"
use core.net as net
use core.tasks as tasks
use core.time as time

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("listen")
    typed_address :: net.listener_local_socket_addr(listener) ?? panic("address")
    address :: net.socket_to_string(typed_address)
    server :: tasks.spawn(() => {
        stream := net.tcp_accept(listener) ?? return
        loop {
            chunk := stream.read(65536) ?? return
            if chunk.len() == 0 {
                return
            }
            time.sleep(15)
        }
    })
    client := net.tcp_connect(address) ?? panic("connect")
    net.set_write_timeout(&client, 80) ?? panic("timeout")
    started := time.now()
    if client.write_text("x".repeat(16000000)) == {
        .Ok(_) -> print("unexpected write")
        .Err(error) -> print(net.error_message(error))
    }
    elapsed := time.now() - started
    print(elapsed < 300)
    client.close() ?? panic("close")
    server.join()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "deadline exceeded while waiting in tcp write\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_tls_byte_stream_runs_real_local_handshake_and_close_notify() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_surface_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let cert = dir.join("leaf.cert.pem");
    let key = dir.join("leaf.key.pem");
    let csr = dir.join("leaf.csr.pem");
    let extensions = dir.join("leaf.ext");
    fs::write(&extensions, "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n").unwrap();
    let req = Command::new("openssl").args(["req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-keyout"])
        .arg(&key).arg("-out").arg(&csr).output().unwrap();
    assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
    let sign = Command::new("openssl").args(["x509", "-req", "-days", "1", "-set_serial", "2", "-CA"])
        .arg(&ca_cert).arg("-CAkey").arg(&ca_key).arg("-extfile").arg(&extensions)
        .arg("-in").arg(&csr).arg("-out").arg(&cert).output().unwrap();
    assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
    let mut server = Command::new("openssl")
        .args(["s_server", "-quiet", "-www", "-alpn", "http/1.0", "-accept", &port.to_string(), "-cert"])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let src = r#"
use core.net as net
use core.tls as tls

fn receive<T: Reader>(&stream: T, limit: Int) => [U8] ? IOError {
    return stream.read(limit)
}


fn send<T: Writer>(&stream: T, bytes: [U8]) => Int ? IOError {
    empty_count :: stream.write([])?
    stream.write_all(bytes)?
    return .Ok(empty_count)
}

fn zero_rejected<T: Reader>(&stream: T) => Bool {
    if stream.read(0) == {
        .Ok(_) -> return false
        .Err(error) -> {
            if error == {
                .InvalidInput(context) -> return context.operation == .Read
                else -> { return false }
            }
        }
    }
    return false
}

fn run() {
    tcp :: net.tcp_connect("127.0.0.1:$PORT") ?? panic("tcp")
    budget :: Duration.seconds(1) ?? panic("deadline")
    cfg :: tls.ClientConfig.default().with_alpn(["http/1.0"]) ?? panic("ALPN")
    secure := tls.client(^tcp, server_name: "localhost", config: cfg, deadline: budget) ?? panic("tls handshake")
    request :: [U8].{ 71, 69, 84, 32, 47, 32, 72, 84, 84, 80, 47, 49, 46, 48, 13, 10, 13, 10 }
    interest :: NetReadyInterest.Write
    readiness :: secure.ready(interest, deadline: budget) ?? panic("ready")
    print(net.ready_readable(readiness))
    print(net.ready_writable(readiness))
    print(zero_rejected(&secure))
    empty :: [U8].{}
    empty_count :: send(&secure, empty) ?? panic("empty write")
    secure.write_all(request, deadline: budget) ?? panic("write bytes")
    print(empty_count)
    read_interest :: NetReadyInterest.Read
    response_ready :: secure.ready(read_interest, deadline: budget) ?? panic("response ready")
    print(net.ready_readable(response_ready))
    response :: secure.read(4096, deadline: budget) ?? panic("read bytes")
    print(response.len() > 0)
    secure.close() ?? panic("close notify")
    secure.close() ?? panic("idempotent close")
    if receive(&secure, 1) == {
        .Ok(_) -> panic("closed read succeeded")
        .Err(error) -> {
            if error == {
                .Closed(context) -> print(if context.operation == .Read { "closed" } else { "wrong-operation" })
                else -> { print("wrong-error") }
            }
        }
    }
}
"#.replace("$PORT", &port.to_string());
    let cert_text = ca_cert.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(&dir, "tls_byte_surface", &src, &[("SSL_CERT_FILE", &cert_text)], None);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "false\ntrue\ntrue\n0\ntrue\ntrue\nclosed\n");
}
#[test]
fn core_tls_expert_config_peer_identity_and_directional_close_are_real() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_expert_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ca_cert = root.join("tests/fixtures/tls/localhost.cert.pem");
    let ca_key = root.join("tests/fixtures/tls/localhost.key.pem");
    let serial = dir.join("ca.srl");
    let make_cert = |name: &str, usage: &str| {
        let cert = dir.join(format!("{name}.cert.pem"));
        let key = dir.join(format!("{name}.key.pem"));
        let csr = dir.join(format!("{name}.csr.pem"));
        let ext = dir.join(format!("{name}.ext"));
        fs::write(
            &ext,
            format!("basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\nextendedKeyUsage={usage}\n"),
        ).unwrap();
        let mut request = Command::new("openssl");
        request.args(["req", "-new", "-newkey", "rsa:2048", "-nodes"]);
        if name == "localhost" {
            let config = dir.join("legacy-dn.cnf");
            fs::write(
                &config,
                "[req]\nprompt=no\ndistinguished_name=dn\nstring_mask=default\n[dn]\nCN=Télét\n",
            ).unwrap();
            request.arg("-config").arg(config);
        } else {
            request.arg("-subj").arg(format!("/CN={name}"));
        }
        let req = request.arg("-keyout").arg(&key).arg("-out").arg(&csr).output().unwrap();
        assert!(req.status.success(), "{}", String::from_utf8_lossy(&req.stderr));
        let sign = Command::new("openssl")
            .args(["x509", "-req", "-days", "1", "-CAcreateserial", "-CAserial"])
            .arg(&serial).arg("-CA")
            .arg(&ca_cert).arg("-CAkey").arg(&ca_key).arg("-extfile").arg(&ext)
            .arg("-in").arg(&csr).arg("-out").arg(&cert).output().unwrap();
        assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
        (cert, key)
    };
    let (server_cert, server_key) = make_cert("localhost", "serverAuth");
    let (client_cert, client_key) = make_cert("jet-client", "clientAuth");
    let parsed = Command::new("openssl").args(["asn1parse", "-in"])
        .arg(&server_cert).output().unwrap();
    assert!(parsed.status.success(), "{}", String::from_utf8_lossy(&parsed.stderr));
    assert!(String::from_utf8_lossy(&parsed.stdout).contains("T61STRING"));
    let mut server = Command::new("openssl")
        .args(["s_server", "-quiet", "-www", "-alpn", "http/1.0", "-Verify", "1", "-verify_return_error", "-accept", &port.to_string(), "-CAfile"])
        .arg(&ca_cert).arg("-cert").arg(&server_cert).arg("-key").arg(&server_key)
        .arg("-cert_chain").arg(&ca_cert)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    let jet_bytes = |path: &std::path::Path| {
        fs::read(path)
            .unwrap()
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let source = format!(r#"
use core.net as net
use core.tls as tls

fn invalid_alpn() => [String] {{
    return [""]
}}

fn run() {{
    ca :: [U8].{{ {} }}
    client_cert :: [U8].{{ {} }}
    client_key :: [U8].{{ {} }}
    wrong_key :: [U8].{{ {} }}
    roots :: tls.RootCertificates.from_pem(ca) ?? panic("root validation")
    identity :: tls.ClientIdentity.from_pem(cert_chain: client_cert, private_key: client_key) ?? panic("identity validation")
    if tls.ClientIdentity.from_pem(cert_chain: client_cert, private_key: wrong_key) == {{
        .Ok(_) -> panic("mismatched identity accepted")
        .Err(_) -> print("mismatch-rejected")
    }}
    if tls.ClientConfig.default().with_version_bounds(min: .Tls13, max: .Tls12) == {{
        .Ok(_) -> panic("reversed TLS versions accepted")
        .Err(_) -> print("bounds-rejected")
    }}
    _plus :: tls.ClientConfig.default().with_trust(.SystemPlus(roots)) ?? panic("system plus")
    cfg0 :: tls.ClientConfig.default().with_trust(.CustomOnly(roots)) ?? panic("custom trust")
    cfg1 :: cfg0.with_client_identity(identity) ?? panic("client identity")
    cfg2 :: cfg1.with_version_bounds(min: .Tls12, max: .Tls13) ?? panic("version bounds")
    tcp :: net.tcp_connect("127.0.0.1:{}") ?? panic("tcp")
    if cfg2.with_alpn(invalid_alpn()) == {{
        .Ok(_) -> panic("empty dynamic ALPN accepted")
        .Err(error) -> if error == {{
            .InvalidInput(context) -> print(if context.operation == .Connect {{ "alpn-rejected" }} else {{ "wrong-alpn-operation" }})
            else -> {{ panic("wrong ALPN error") }}
        }}
    }}
    cfg :: cfg2.with_alpn(["http/1.0"]) ?? panic("ALPN")
    budget :: Duration.seconds(2) ?? panic("budget")
    secure := tls.client(^tcp, server_name: "localhost", config: cfg, deadline: budget) ?? panic("mTLS")
    peer :: secure.peer_identity()
    print(peer.verified_server_name)
    print(peer.certificate_chain.len() == 2)
    print(peer.leaf.der == peer.certificate_chain[0].der)
    print(peer.leaf.der.len() > 0)
    print(peer.leaf.sha256.len())
    print(peer.leaf.spki_sha256.len())
    print(peer.leaf.dns_names.contains("localhost"))
    print(peer.leaf.valid_from_unix_ms < peer.leaf.valid_until_unix_ms)
    print(peer.leaf.subject.contains("CN=T") && peer.leaf.subject.contains("\\xc3"))
    print(peer.leaf.issuer.len() > 0)
    request :: [U8].{{ 71, 69, 84, 32, 47, 32, 72, 84, 84, 80, 47, 49, 46, 48, 13, 10, 13, 10 }}
    secure.write_all(request, deadline: budget) ?? panic("request")
    secure.close_write(deadline: budget) ?? panic("close write")
    secure.close_write(deadline: budget) ?? panic("repeat close write")
    one :: [U8].{{ 1 }}
    if secure.write_all(one, deadline: budget) == {{
        .Ok(_) -> panic("write after close_write succeeded")
        .Err(error) -> if error == {{
            .Closed(context) -> print(if context.operation == .Write {{ "write-closed" }} else {{ "wrong-write-operation" }})
            else -> {{ panic("wrong post-close error") }}
        }}
    }}
    total := 0
    loop {{
        chunk :: secure.read(4096, deadline: budget) ?? panic("response read")
        if chunk.is_empty() {{ break }}
        total += chunk.len()
    }}
    print(total > 0)
    secure.close() ?? panic("close")
    print(secure.peer_identity().verified_server_name)
}}
"#,
        jet_bytes(&ca_cert),
        jet_bytes(&client_cert),
        jet_bytes(&client_key),
        jet_bytes(&server_key),
        port,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "tls_expert_surface", &source, &[], None);
    let _ = server.kill();
    let _ = server.wait();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "mismatch-rejected\nbounds-rejected\nalpn-rejected\nlocalhost\ntrue\ntrue\ntrue\n32\n32\ntrue\ntrue\ntrue\ntrue\nwrite-closed\ntrue\nlocalhost\n",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_tls_identity_drop_and_protocol_mapping_use_shared_runtime_laws() {
    let dir = std::env::temp_dir().join(format!("jet_core_tls_runtime_laws_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "tls_runtime_laws.jet",
        "use core.tls as tls\nfn run() { _config :: tls.ClientConfig.default() }\n",
    );
    let mut rust = compiled.rust;
    rust = rust.replacen("fn main()", "fn jet_generated_main()", 1);
    rust.push_str(r#"
fn main() {
    let zeroized = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<u8>>::new()));
    let observed = std::rc::Rc::clone(&zeroized);
    jet_crypto_entropy_set_zeroize_test_observer(move |bytes| {
        observed.borrow_mut().push(bytes.to_vec());
    });
    {
        let identity = JetTLSClientIdentity {
            cert_chain: vec![1, 2, 3],
            private_key: JetCryptoSecretBytes::new(vec![0xa5; 7]),
        };
        let config = jet_tls_client_config_with_client_identity(
            jet_tls_client_config_default(),
            &identity,
        ).unwrap();
        assert!(jet_tls_client_config_with_version_bounds(
            config,
            JetTLSVersion::Tls13,
            JetTLSVersion::Tls12,
        ).is_err());
    }
    jet_crypto_entropy_clear_zeroize_test_observer();
    assert_eq!(&*zeroized.borrow(), &vec![vec![0; 7], vec![0; 7]]);

    let cause = "TLS protocol truncation: peer closed without close-notify".to_string();
    match jet_net_tls_io_result::<()>(.Err(cause.clone()), jet_std::IOOperation::Read).unwrap_err() {
        jet_std::IOError::Protocol(context) => {
            assert_eq!(context.operation, jet_std::IOOperation::Read);
            assert_eq!(context.cause, Ok(cause));
        }
        other => panic!("expected Protocol(Read), got {other:?}"),
    }
}
"#);
    let rs = dir.join("runtime_laws.rs");
    let bin = dir.join("runtime_laws");
    fs::write(&rs, rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", "--cfg", "test"])
        .arg(&rs).arg("-o").arg(&bin);
    if let Some(link) = compiled.ffi {
        rustc.arg("--extern").arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc.arg("-L").arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let ran = Command::new(bin).output().unwrap();
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn core_tls_stalled_handshake_observes_timeout_and_cancellation() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_tls_stalled_handshake_{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (peer, _) = listener.accept().unwrap();
            peers.push(peer);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
    let source = format!(
        r#"
use core.net as net
use core.tasks as tasks
use core.tls as tls

fn run() {{
    timed := net.tcp_connect("{address}") ?? panic("timeout tcp")
    net.set_timeout(&timed, 30) ?? panic("timeout budget")
    if net.tls_connect(^timed, "localhost") == {{
        .Ok(_) -> panic("stalled handshake succeeded")
        .Err(error) -> print("{{net.error_operation(error)}}:{{net.error_message(error)}}")
    }}

    (ready_tx, ready_rx) :: tasks.channel<Int>()
    blocked :: tasks.spawn(() => {{
        tcp := net.tcp_connect("{address}") ?? panic("cancel tcp")
        ready_tx.send(1)
        if tls.client(^tcp, "localhost") == {{
            .Ok(_) -> panic("cancelled handshake succeeded")
            .Err(error) -> print("{{net.error_operation(error)}}:{{net.error_message(error)}}")
        }}
    }})
    _ready :: ready_rx.receive() ?? panic("ready")
    blocked.cancel()
    blocked.join()
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "tls_stalled_handshake", &source, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "{stderr}");
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "tls handshake:deadline exceeded while waiting in tls handshake",
            "tls handshake:tls handshake cancelled",
        ]
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn canonical_core_import_resolves() {
    let out = compile_temp(
        "core_imports.jet",
        r#"
use core.files as fs

fn run() {
    print(fs.exists("/tmp"))
}
"#,
    );
    assert!(out.rust.contains("jet_std_fs_exists"));
}

#[test]
fn importing_core_without_calls_is_free_in_codegen() {
    let out = compile_temp(
        "core_import_only.jet",
        r#"
use core.files as fs
use core.io as io
use core.env as env
use core.process as process
use core.math as math
use core.random as random
use core.time as time
use core.encoding.json as json

struct Plain {
    value: String
}

fn identity(value: ^Plain) => Plain {
    return value
}

fn run() {
    print("ok")
}
"#,
    );
    assert!(!out.rust.contains("mod jet_std"));
    assert!(!out.rust.contains("jet_std_fs_read"));
    assert!(out.rust.contains("fn main()"));
}

#[test]
fn core_data_import_and_codegen_resolve() {
    let out = compile_temp(
        "core_data_import.jet",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    rows :: data.csv<Ticket>("team,minutes\nCore,4.0") ?? panic("bad csv")
    print(data.count(rows))
}
"#,
    );
    assert!(out.rust.contains("jet_enc_csv_decode"));
    assert!(
        out.rust.contains("jet_enc_csv_decode::<user_Ticket>"),
        "core.data.csv must lower its sema-owned list element type exactly:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("jet_enc_csv_decode::<Vec<user_Ticket>>"),
        "core.data.csv nested its list result at the runtime boundary:\n{}",
        out.rust
    );
    assert!(out.rust.contains("jet_data_count"));
}

#[test]
fn core_files_depth_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/files_depth.jet"])
        .output()
        .expect("run files_depth");
    assert!(
        out.status.success(),
        "files_depth failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/files_depth.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn core_watcher_example_runs() {
    let jet = jet_bin();
    if !jet.exists() {
        return;
    }
    let out = Command::new(&jet)
        .args(["run", "examples/features/io/watcher.jet"])
        .output()
        .expect("run watcher");
    assert!(
        out.status.success(),
        "watcher failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = fs::read_to_string("examples/features/expected/io/watcher.out").unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[cfg(unix)]
#[test]
fn core_process_builder_pipeline_and_spawn_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_process_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();

    let probe = dir.join("probe.sh");
    let emit = dir.join("emit.sh");
    let cat = dir.join("cat.sh");
    let lines = dir.join("lines.sh");
    write_executable(
        &probe,
        "#!/bin/sh\nprintf 'env=%s\\n' \"$JET_PROCESS_TEST\"\nprintf 'cwd=%s\\n' \"$(pwd)\"\nread line\nprintf 'stdin=%s\\n' \"$line\"\n",
    );
    write_executable(&emit, "#!/bin/sh\nprintf 'pipe-ok\\n'\n");
    write_executable(&cat, "#!/bin/sh\ncat\n");
    write_executable(&lines, "#!/bin/sh\nprintf 'line-one\\nline-two\\n'\n");

    let src = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    timeout :: Duration.seconds(2) ?? panic("duration")
    spec :: process.cmd(["{probe}"]).cwd("{work}").env_clear().env("JET_PROCESS_TEST", "ok").stdin(.Capture).stdout(.Capture).stderr(.Capture).timeout(timeout).output_limit(10000)
    probe_child :: spec.spawn() ?? panic("spawn failed")
    probe_child.stdin.write("from-stdin\n") ?? panic("write failed")
    result :: probe_child.wait() ?? panic("wait failed")
    print(result.success)
    print(result.code)
    print(result.timed_out)
    print(result.output)

    piped :: process.pipeline([process.cmd(["{emit}"]), process.cmd(["{cat}"])]) ?? panic("pipeline failed")
    print(piped.success)
    print(piped.output)

    child :: process.cmd(["{lines}"]).stdout(.Stream).spawn() ?? panic("spawn failed")
    loop line, child.stdout.lines() {{
        print(line)
    }}
    waited :: child.wait() ?? panic("wait failed")
    print(waited.success)
}}
"#,
        probe = jet_string_path(&probe),
        work = jet_string_path(&work),
        emit = jet_string_path(&emit),
        cat = jet_string_path(&cat),
        lines = jet_string_path(&lines)
    );

    let (code, stdout, stderr) = build_and_run(&dir, "process_api", &src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.contains("true\n0\nfalse\n"), "{stdout}");
    assert!(stdout.contains("env=ok\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("cwd={}\n", work.display())),
        "{stdout}"
    );
    assert!(stdout.contains("stdin=from-stdin\n"), "{stdout}");
    assert!(stdout.contains("pipe-ok\n"), "{stdout}");
    assert!(stdout.contains("line-one\n"), "{stdout}");
}

/// D-PROCESS-SESSION1=A (#1181): `.terminal()` is the one opt-in for a
/// terminal-backed session, and it lives on the same `ProcessSpec`. Argv
/// execution with no terminal stays the default. Unix run/spawn use a real PTY;
/// pipeline stages reject terminal specs rather than coercing them to pipes.
#[cfg(unix)]
#[test]
fn core_process_terminal_uses_unix_pty_for_run_and_spawn() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_terminal_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    plain :: process.cmd(["echo", "plain-ok"]).stdout(.Capture).run() ?? panic("default run failed")
    print(plain.output.trim())

    run_result :: process.cmd(["printf", "run-ok"]).terminal().run() ?? panic("terminal run failed")
    print(run_result.output.contains("run-ok"))

    child :: process.cmd(["printf", "spawn-ok"]).terminal().spawn() ?? panic("terminal spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 100, rows: 30 }) ?? panic("resize failed")
            print("spawn: session")
        }
        .None -> { print("spawn: no session") }
    }
    waited :: child.wait() ?? panic("terminal wait failed")
    print(waited.output.contains("spawn-ok"))

    if process.pipeline([process.cmd(["echo", "a"]), process.cmd(["cat"]).terminal()]) == {
        .Ok(_) -> { print("pipeline: accepted") }
        .Err(_) -> { print("pipeline: refused") }
    }
    if process.cmd([]).terminal().run() == {
        .Ok(_) -> { print("empty: accepted") }
        .Err(e) -> {
            if e == {
                .InvalidInput(_) -> { print("empty: invalid") }
                else -> { print("empty: wrong error") }
            }
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(stdout.starts_with("plain-ok\n"), "{stdout}");
    assert!(stdout.contains("true\nspawn: session\ntrue\npipeline: refused\nempty: invalid\n"), "{stdout}");
    // The production path carries the native PTY primitive into the emitted
    // program; this guards against a test-only or pipe fallback.
    let compiled = compile_temp("process_terminal_text.jet", src);
    assert!(
        compiled.rust.contains("posix_openpt"),
        "the Unix terminal path must include the native PTY backend"
    );
}

/// D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D (#1181): the beginner and expert
/// forms share one ProcessSpec. Stable host facts advertise the Unix PTY and a
/// policy controls the initial terminal size and mode.
#[cfg(unix)]
#[test]
fn core_process_terminal_policy_and_capabilities_are_typed_and_resizable() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_process_terminal_policy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.process as process

fn run() {
    policy :: TerminalPolicy.{
        size: TerminalSize.{ cols: 120, rows: 40 },
        mode: .Raw
    }
    plan :: process.cmd(["echo", "hi"]).terminal(policy)
    facts :: plan.capabilities()
    print(facts.has(TerminalFact.terminal))
    print(facts.has(TerminalFact.resize))
    print(facts.has(TerminalFact.raw))
    print(facts.has("preview_x"))
    if plan.run() == {
        .Ok(_) -> { print("terminal:ok") }
        .Err(_) -> { print("terminal:unavailable") }
    }
    child :: process.cmd(["echo", "plain"]).stdout(.Capture).spawn() ?? panic("spawn failed")
    if child.terminal == {
        .Val(session) -> {
            session.resize(TerminalSize.{ cols: 80, rows: 24 }) ?? panic("resize failed")
            print("plain child unexpectedly has terminal")
        }
        .None -> { print("plain child has no terminal") }
    }
    waited :: child.wait() ?? panic("wait failed")
    print(waited.output.trim())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "process_terminal_policy", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\nfalse\nterminal:ok\nplain child has no terminal\nplain\n"
    );

    let typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).capabilities()
    print(facts.has(TerminalFact.reszie))
}
"#,
    )
    .expect_err("stable fact typos must fail in sema");
    assert!(
        typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`TerminalFact` has no key `reszie`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{typo:?}"
    );

    let preview_typo = jet::compile(
        r#"use core.process as process
fn run() {
    facts :: process.cmd(["echo", "x"]).capabilities()
    print(facts.has("reszie"))
}
"#,
    )
    .expect_err("close preview-string typos must suggest the stable fact");
    assert!(
        preview_typo.iter().any(|diag| {
            diag.code == "E0302"
                && diag.what.contains("`reszie` looks like `resize`")
                && diag.fix.contains("`TerminalFact.resize`")
        }),
        "{preview_typo:?}"
    );

    let plain_child_terminal = jet::compile(
        r#"use core.process as process
fn run() {
    child :: process.cmd(["echo", "plain"]).spawn() ?? panic("spawn failed")
    child.terminal.resize(TerminalSize.{ cols: 80, rows: 24 })
}
"#,
    )
    .expect_err("a plain child must not expose a TerminalSession");
    assert!(
        plain_child_terminal
            .iter()
            .any(|diag| {
                diag.code == "E0311"
                    && diag.what
                        == "`.resize()` needs `TerminalSession`, not `TerminalSession?`"
                    && diag.fix.contains("session.resize(size)")
            }),
        "{plain_child_terminal:?}"
    );
}

#[cfg(unix)]
#[test]
fn core_process_sh_typed_text_keeps_each_hole_one_argv_item() {
    let dir = std::env::temp_dir().join(format!("jet_core_process_sh_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "process_sh_typed_text",
        r#"
use core.process as process

fn run() {
    hostile :: "two words;*.jet"
    expected :: Sh.{"printf <%s> {hostile}"}
    first :: process.run(expected) ?? panic("typed-head command failed")
    print(first.output)

    second :: process.run(Sh.{"printf [%s] {hostile}"}) ?? panic("second typed-head failed")
    print(second.output)

    audited :: Sh.raw("printf raw")
    third :: process.run(audited) ?? panic("raw command failed")
    print(third.output)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "<two words;*.jet>\n[two words;*.jet]\nraw\n");
}

#[test]
fn core_time_calendar_zone_and_dst_run() {
    let source_zone = std::env::var_os("TZDIR")
        .map(|dir| PathBuf::from(dir).join("America/New_York"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("/usr/share/zoneinfo/America/New_York"));
    if !source_zone.exists() {
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_time_calendar_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let tzdb = dir.join("tzdb");
    fs::create_dir_all(tzdb.join("America")).unwrap();
    fs::copy(&source_zone, tzdb.join("America/New_York")).unwrap();
    let src = r#"
use core.time as time
use core.time.date as date

fn run() {
    zone :: time.zone("America/New_York") ?? panic("missing zone")
    local :: time.zoned_local(date.new(2024, 3, 10), time.time(1, 30, 0), zone)
    print(local.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    civil :: local.add_period(time.period_days(1))
    day :: Duration.hours(24) ?? panic("duration")
    absolute :: local.add_duration(day)
    print(civil.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(absolute.format("yyyy-MM-dd HH:mm:ss VV XXX"))
    print(local.to_datetime().format_rfc3339())
    parsed :: time.parse_rfc3339("2024-03-10T06:30:00Z") ?? panic("bad parse")
    print(parsed.in_zone(zone).format("yyyy-MM-dd HH:mm:ss VV XXX"))
}
"#;
    let tzdb_env = tzdb.to_string_lossy().into_owned();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "time_calendar",
        src,
        &[("JET_TZDB_DIR", &tzdb_env)],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "2024-03-10 01:30:00 America/New_York -05:00\n2024-03-11 01:30:00 America/New_York -04:00\n2024-03-11 02:30:00 America/New_York -04:00\n2024-03-10T06:30:00Z\n2024-03-10 01:30:00 America/New_York -05:00\n"
    );
}

#[test]
fn core_url_mime_parse_join_query_and_http_url_run() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_url_mime_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.mime as mime
use core.url as url

fn run() {
    base :: url.parse("https://Bücher.example:443/a/./b/../c?x=1#frag") ?? panic("bad url")
    print(base.to_string())
    print(base.host() ?? "none")
    print(base.path())
    print(base.query_pairs()[0][0])
    print(base.query_pairs()[0][1])
    rel :: base.join("../notify?user=ada lovelace&user=grace#done") ?? panic("bad join")
    print(rel.to_string())
    print(rel.path_segments().join("|"))
    print(rel.fragment() ?? "none")
    print(url.query([["user", "ada lovelace"], ["user", "grace"], ["empty", ""]]))
    print(url.percent_encode("a b/c"))
    print(url.percent_decode("a%20b%2Fc") ?? "bad")
    html :: mime.parse("Text/HTML; charset=UTF-8") ?? panic("bad mime")
    print(html.essence())
    print(html.param("charset") ?? "none")
    print(mime.from_extension("png") ?? "none")
    print(mime.extension("image/png") ?? "none")
    png :: mime.parse("image/png") ?? panic("bad mime")
    print(url.data(png, "<h1>Hi</h1>").to_string())
    print(url.file("/tmp/a b.txt").to_string())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "url_mime", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "https://xn--bcher-kva.example:443/a/c?x=1#frag\nxn--bcher-kva.example\n/a/c\nx\n1\nhttps://xn--bcher-kva.example:443/notify?user=ada%20lovelace&user=grace#done\nnotify\ndone\nuser=ada%20lovelace&user=grace&empty=\na%20b%2Fc\na b/c\ntext/html\nUTF-8\nimage/png\npng\ndata:image/png,%3Ch1%3EHi%3C%2Fh1%3E\nfile:///tmp/a%20b.txt\n"
    );
}

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
    recipients.{ [~recipient]
    count := 1
    loop count < 101 { recipients.push(~recipient); count++ }
    if email.message(~sender, recipients, [], "subject", "text", "", []) == {
        .Ok(_) -> panic("recipient bound ignored")
        .Err(_) -> print("recipient-bound")
    }
    too_large := [U8].{ 0 }
    count = 1
    loop count < 26214401 { too_large.push(0); count++ }
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

#[test]
fn core_email_policy_envelope_and_reports_are_real_jet_values() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_email_policy_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.email as email

fn error_text(problem: email.EmailError) => String {
    if problem == {
        .Configuration(_, _, _, _) -> { return "matched" }
        .TLS(_, _, _, _) -> { return "tls-error" }
    }
    return "unknown"
}

fn run() {
    sender :: email.address("sender@example.com") ?? panic("sender")
    visible :: email.address("visible@example.net") ?? panic("visible")
    hidden :: email.address("hidden@example.org") ?? panic("hidden")
    message :: email.message(~sender, [~visible], [~hidden], "subject", "body", "", []) ?? panic("message")
    original_bytes :: email.serialize(~message) ?? panic("serialize original")
    default_envelope :: message.envelope()
    envelope :: email.envelope(sender, [~hidden]) ?? panic("envelope")
    replaced :: message.with_envelope(envelope) ?? panic("replace")
    bytes :: email.serialize(replaced) ?? panic("serialize")
    start_tls := email.SMTPSecurity.StartTls
    transport_tls := email.SMTPSecurity.TLS
    require_all := email.RecipientPolicy.RequireAll
    recipient := email.RecipientReport.{
        address: hidden,
        accepted: true,
        code: 250,
        message: "accepted",
    }
    report := email.SendReport.{
        server: "smtp.example.com",
        accepted: [recipient],
        rejected: [],
        response_code: 250,
        response: "queued",
        accepted_at: "2026-07-13T17:00:00Z",
    }
    problem := email.EmailError.Configuration.{
        operation: "send",
        server: Val("smtp.example.com"),
        code: Val(451),
        reason: "stopped",
    }
    tls_problem := email.EmailError.TLS.{
        operation: "handshake",
        server: Val("smtp.example.com"),
        code: Val(525),
        reason: "certificate",
    }
    print(start_tls == .StartTls)
    print(transport_tls == .TLS)
    print(require_all == .RequireAll)
    print(default_envelope.recipients.len())
    print(original_bytes == bytes)
    print(report.server)
    print(report.accepted.len())
    print(error_text(problem))
    print(error_text(tls_problem))
    print(bytes.len() > 0)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "email_policy", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "true\ntrue\ntrue\n2\ntrue\nsmtp.example.com\n1\nmatched\ntls-error\ntrue\n");
    let file = dir.join("email_policy.jet");
    fs::write(&file, src).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout: dev_stdout, stderr: dev_stderr, exit_code } => {
            assert_eq!((exit_code, dev_stdout, dev_stderr), (0, stdout, String::new()));
        }
        other => panic!("email policy default-dev failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_accepts_typed_url_in_codegen() {
    let out = compile_temp(
        "http_url.jet",
        r#"
use core.http.client as http
use core.url as url

fn run() {
    u :: url.parse("https://example.com/a") ?? panic("bad url")
    req :: http.request("GET", u).timeout(1)
}
"#,
    );
    assert!(
        out.rust.contains(".to_string_value()"),
        "typed Url should render to String at HTTP boundary:\n{}",
        out.rust
    );
}

#[test]
fn core_http_client_preserves_repeated_headers_over_a_socket() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0; 4096];
        let read = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..read]);
        let warnings = request
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("warning:"))
            .collect::<Vec<_>>()
            .join("\n");
        let first = warnings.find("one").expect("first Warning value");
        let second = warnings.find("two").expect("second Warning value");
        assert!(first < second, "repeated Warning values changed: {request}");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-A: one\r\nX-B: middle\r\nX-A: two\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: close\r\n\r\n\xff\0",
            )
            .unwrap();
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_headers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_bridge_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_headers.rs");
    let bin = dir.join("bridge_headers");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let request_headers = vec![
        "Warning".to_string(), "one".to_string(),
        "Warning".to_string(), "two".to_string(),
    ];
    let (_, body, _, headers) = bridge::jet_http_client_send_impl(
        "GET", &url, &request_headers, None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap();
    assert_eq!(
        bridge::jet_http_client_body_read_impl(body, 8).unwrap(),
        Some(vec![255, 0]),
    );
    assert_eq!(bridge::jet_http_client_body_read_impl(body, 8).unwrap(), None);
    let selected = headers.chunks_exact(2)
        .filter(|pair| matches!(pair[0].as_str(), "x-a" | "x-b" | "set-cookie"))
        .flat_map(|pair| [pair[0].clone(), pair[1].clone()])
        .collect::<Vec<_>>();
    assert_eq!(selected, vec![
        "x-a", "one", "x-b", "middle", "x-a", "two",
        "set-cookie", "a=1", "set-cookie", "b=2",
    ]);
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", harness.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
    rustc.arg("--extern").arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc.arg("-L").arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "bridge harness compile failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let output = Command::new(&bin).arg(format!("http://{addr}/")).output().unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "bridge harness failed:\n{}", String::from_utf8_lossy(&output.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_exposes_binary_body_once() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nX-A: one\r\nX-A: two\r\nConnection: close\r\n\r\n\0\xff\x01")
            .unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_binary_body_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = format!(
        r#"
use core.http.client as client

fn run() {{
    response :: client.get("http://{addr}/") ?? panic("request")
    values :: response.headers.all("x-a")
    print(values.len())
    print(values[0])
    print(values[1])
    body :: response.body()
    bytes :: body.bytes(8) ?? panic("body")
    print(bytes.len())
    print(bytes[0])
    print(bytes[1])
    second :: body.bytes(8)
    if second == {{
        .Ok(_) -> {{ print("reused") }}
        .Err(error) -> {{
            if error == {{
                .BodyConsumed -> {{ print("consumed") }}
                else -> {{ print("wrong-error") }}
            }}
        }}
    }}
}}
"#,
    );
    let (code, stdout, stderr) = build_and_run(&dir, "http_binary_body", &source, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "2\none\ntwo\n3\n0\n255\nconsumed\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_nominal_message_and_body_surface_is_executable() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_nominal_surface_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input");
    let output = dir.join("output");
    fs::write(&input, b"reader").unwrap();
    let source = format!(
        r#"
use core.http as http
use core.mime as mime
use core.files as files

fn run() {{
    print(http.Method.custom("PURGE") ?? panic("method"))
    print(http.Status.new(299) ?? panic("status"))
    print(http.Version.http_1_1())
    print(http.HeaderName.new("X-Test") ?? panic("name"))
    print(http.HeaderValue.new("ok") ?? panic("value"))
    print(http.Body.empty().bytes(1) ?? panic("empty"))
    print(http.Body.bytes([0, 255]).bytes(2) ?? panic("bytes"))
    print(http.Body.text("hello").text(5) ?? panic("text"))
    print(http.Body.text("hello", mime.parse("text/custom") ?? panic("mime")).text(5) ?? panic("custom"))
    print(http.Body.form(["a": "b"]).text(16) ?? panic("form"))
    print(http.Body.json(42).json<Int>(16) ?? panic("json"))
    input :: files.open("{input}") ?? panic("open")
    body :: http.Body.reader(^input, 6) ?? panic("reader")
    output :: files.create("{output}") ?? panic("create")
    print(body.copy_to(^output, 6) ?? panic("copy"))
}}
"#,
        input = jet_string_path(&input),
        output = jet_string_path(&output),
    );
    let (code, stdout, stderr) = build_and_run(&dir, "http_nominal_surface", &source, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "PURGE\n299\nHTTP/1.1\nX-Test\nok\n[]\n[0, 255]\nhello\nhello\na=b\n42\n6\n"
    );
    assert_eq!(fs::read(output).unwrap(), b"reader");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_multipart_boundary_does_not_collide_with_fields() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0, "multipart request ended before its body");
            request.extend_from_slice(&chunk[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            )
            .unwrap();
        String::from_utf8(request).unwrap()
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_multipart_boundary_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_multipart_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"POST\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_multipart_boundary.rs");
    let bin = dir.join("bridge_multipart_boundary");
    fs::write(
        &harness,
        r#"
fn main() {
    let url = std::env::args().nth(1).unwrap();
    let long_candidate = format!("jet-http-boundary{}", "-".repeat(53));
    let candidates = (0u64..300)
        .map(|suffix| format!("jet-http-boundary-{suffix:016x}"))
        .collect::<String>();
    let line_break_name =
        format!("safe\"\r\nX-Extra: yes\r\n{long_candidate}{candidates}");
    let multipart = vec![
        line_break_name,
        format!("before\r\n--{long_candidate}\r\n{candidates}\r\nafter"),
    ];
    let response = bridge::jet_http_client_send_impl(
        "POST", &url, &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &multipart,
    ).unwrap();
    let body = bridge::jet_http_client_body_read_impl(response.1, 8).unwrap().unwrap();
    assert_eq!((response.0, body.as_slice()), (200, b"ok".as_slice()));
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}/"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = server.join().unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    let boundary = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-type: multipart/form-data; boundary="))
        .unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    let long_candidate = format!("jet-http-boundary{}", "-".repeat(53));
    let candidates = (0u64..300)
        .map(|suffix| format!("jet-http-boundary-{suffix:016x}"))
        .collect::<String>();
    let raw_field_name = format!("safe\"\r\nX-Extra: yes\r\n{long_candidate}{candidates}");
    let field_name = format!(
        "safe%22%0D%0AX-Extra: yes%0D%0A{long_candidate}{candidates}"
    );
    let field_value = format!("before\r\n--{long_candidate}\r\n{candidates}\r\nafter");
    assert!((1..=70).contains(&boundary.len()), "invalid boundary length: {boundary}");
    assert!(boundary.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    assert_eq!(boundary, "jet-http-boundary-000000000000012c");
    assert!(!raw_field_name.contains(boundary), "multipart name collided");
    assert!(!field_value.contains(boundary), "multipart value collided");
    let (part_headers, _) = body
        .strip_prefix(&format!("--{boundary}\r\n"))
        .unwrap()
        .split_once("\r\n\r\n")
        .unwrap();
    assert_eq!(
        part_headers,
        format!("Content-Disposition: form-data; name=\"{field_name}\"")
    );
    assert_eq!(
        part_headers.lines().count(),
        1,
        "multipart field name produced extra header lines"
    );
    assert_eq!(content_length, body.len());
    assert_eq!(
        body,
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\n\r\n{field_value}\r\n--{boundary}--\r\n"
        )
    );
    assert_eq!(body.matches(&format!("--{boundary}")).count(), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_owns_pre_response_errors() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let accepted = std::sync::Arc::new(AtomicUsize::new(0));
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server_stop = stop.clone();
    let server_accepted = accepted.clone();
    let server_captured = captured.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request_index = server_accepted.fetch_add(1, Ordering::AcqRel);
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap();
                    server_captured.lock().unwrap().push(request[..read].to_vec());
                    let response: Option<&[u8]> = match request_index {
                        0 => Some(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        1 => Some(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"jet\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        _ => None,
                    };
                    if let Some(response) = response {
                        stream.write_all(response).unwrap();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_timeout_range_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_timeout_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_timeout_range.rs");
    let bin = dir.join("bridge_timeout_range");
    fs::write(
        &harness,
        r#"
fn main() {
    std::env::set_var("NO_PROXY", "127.0.0.1");
    std::env::set_var("no_proxy", "127.0.0.1");
    let url = std::env::args().nth(1).unwrap();
    let cases = [
        (Some(-1), None, None, None),
        (None, Some(-1), None, None),
        (None, None, Some(-1), None),
        (None, None, None, Some(-1)),
    ];
    let errors = cases.into_iter().map(|(timeout, connect, read, total)| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, timeout, connect, read, total, None, None, None, None, None, None,
            &[], &[], &[],
        ).err()
    }).collect::<Vec<_>>();
    assert!(errors.into_iter().all(|error| matches!(error, Some(bridge::JetHTTPBridgeError::Timeout))));
    let unsupported_url = url.replacen("http://", "ftp://", 1);
    let url_errors = ["http://[".to_string(), unsupported_url].map(|url| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, None, None, None, None, None, None, None, None, None, None,
            &[], &[], &[],
        ).unwrap_err()
    });
    assert!(url_errors.into_iter().all(|error| matches!(error, bridge::JetHTTPBridgeError::InvalidUrl)));
    let refused_url = "http://127.0.0.1:0/".to_string();
    let connection_error = bridge::jet_http_client_send_impl(
        "GET", &refused_url, &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(connection_error, bridge::JetHTTPBridgeError::Connect));
    let proxy_error = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, None, Some("ftp://proxy.invalid"),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_error, bridge::JetHTTPBridgeError::Proxy));
    let proxy_connection_error = bridge::jet_http_client_send_impl(
        "GET", &"https://example.invalid/".to_string(), &[], None, None, None, None, None, None, None, None, None, None,
        Some(url.as_str()),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_connection_error, bridge::JetHTTPBridgeError::Proxy));
    let proxy_auth_error = bridge::jet_http_client_send_impl(
        "GET", &"https://auth.invalid/".to_string(), &[], None, None, None, None, None, None, None, None, None, None,
        Some(url.as_str()),
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(proxy_auth_error, bridge::JetHTTPBridgeError::Proxy));
    let io_error = bridge::jet_http_client_send_impl(
        "GET", &format!("{url}io"), &[], None, None, None, None, None, None, None, None, None, None, None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(io_error, bridge::JetHTTPBridgeError::IO));
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021", harness.to_str().unwrap(), "-o", bin.to_str().unwrap()]);
    rustc.arg("--extern").arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc.arg("-L").arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "bridge harness compile failed:\n{}", String::from_utf8_lossy(&built.stderr));
    let output = Command::new(&bin).arg(format!("http://{addr}/")).output().unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert!(output.status.success(), "bridge harness failed:\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(accepted.load(Ordering::Acquire), 3, "pre-response transport count changed");
    let requests = captured.lock().unwrap();
    assert!(
        requests[0].starts_with(b"CONNECT example.invalid:443 HTTP/1.1\r\n")
            && requests[1].starts_with(b"CONNECT auth.invalid:443 HTTP/1.1\r\n")
            && requests[2].starts_with(b"GET /io HTTP/1.1\r\n"),
        "unexpected requests: {:?}",
        requests
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_rejects_invalid_redirect_limits_before_transport() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let accepted = std::sync::Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server_stop = stop.clone();
    let server_accepted = accepted.clone();
    let server_requests = requests.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    server_accepted.fetch_add(1, Ordering::AcqRel);
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap();
                    let request = String::from_utf8_lossy(&request[..read]);
                    let target = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_ascii_whitespace().nth(1))
                        .unwrap();
                    server_requests.lock().unwrap().push(target.to_string());
                    let response = match target {
                        "/redirect" => "HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 8\r\nConnection: close\r\n\r\nredirect".to_string(),
                        "/target" => "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string(),
                        _ => {
                            let (chain, step) = target.rsplit_once('/').unwrap();
                            let step = step.parse::<usize>().unwrap();
                            let final_step = match chain {
                                "/within" => 10,
                                "/over" => 11,
                                _ => panic!("unexpected target {target}"),
                            };
                            if step == final_step {
                                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string()
                            } else {
                                format!(
                                    "HTTP/1.1 302 Found\r\nLocation: {chain}/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                    step + 1
                                )
                            }
                        }
                    };
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_redirect_range_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let compiled = compile_temp(
        "http_redirect_seed.jet",
        "use core.http.client as http\nfn run() { req :: http.request(\"GET\", \"http://127.0.0.1/\") }\n",
    );
    let link = compiled.ffi.expect("HTTP client bridge");
    let harness = dir.join("bridge_redirect_range.rs");
    let bin = dir.join("bridge_redirect_range");
    fs::write(
        &harness,
        r#"
fn main() {
    let base = std::env::args().nth(1).unwrap();
    let url = format!("{base}/redirect");
    let errors = [-1, i64::from(u32::MAX) + 1].into_iter().map(|redirects| {
        bridge::jet_http_client_send_impl(
            "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(redirects), None,
            &[], &[], &[],
        ).err()
    }).collect::<Vec<_>>();
    assert!(errors.into_iter().all(|error| matches!(error, Some(bridge::JetHTTPBridgeError::Redirect))));
    let stopped = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(0), None,
        &[], &[], &[],
    ).unwrap();
    let stopped_body = bridge::jet_http_client_body_read_impl(stopped.1, 16).unwrap().unwrap();
    assert_eq!((stopped.0, stopped_body.as_slice()), (302, b"redirect".as_slice()));
    let followed = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None,
        Some(i64::from(u32::MAX)), None, &[], &[], &[],
    ).unwrap();
    let followed_body = bridge::jet_http_client_body_read_impl(followed.1, 8).unwrap().unwrap();
    assert_eq!((followed.0, followed_body.as_slice()), (200, b"ok".as_slice()));
    let explicit = bridge::jet_http_client_send_impl(
        "GET", &url, &[], None, None, None, None, None, None, None, None, None, Some(1), None,
        &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(explicit, bridge::JetHTTPBridgeError::Redirect));
    let within = bridge::jet_http_client_send_impl(
        "GET", &format!("{base}/within/0"), &[], None, None, None, None, None, None, None, None, None,
        None, None, &[], &[], &[],
    ).unwrap();
    let within_body = bridge::jet_http_client_body_read_impl(within.1, 8).unwrap().unwrap();
    assert_eq!((within.0, within_body.as_slice()), (200, b"ok".as_slice()));
    let over = bridge::jet_http_client_send_impl(
        "GET", &format!("{base}/over/0"), &[], None, None, None, None, None, None, None, None, None,
        None, None, &[], &[], &[],
    ).unwrap_err();
    assert!(matches!(over, bridge::JetHTTPBridgeError::Redirect));
}
"#,
    )
    .unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args([
        "--edition",
        "2021",
        harness.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    rustc
        .arg("--extern")
        .arg(format!("bridge={}", link.rlib_path.display()));
    for dependency in link.dependency_dirs().filter(|path| path.is_dir()) {
        rustc
            .arg("-L")
            .arg(format!("dependency={}", dependency.display()));
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "bridge harness compile failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(&bin)
        .arg(format!("http://{addr}"))
        .output()
        .unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert!(
        output.status.success(),
        "bridge harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        accepted.load(Ordering::Acquire),
        26,
        "invalid redirect limit reached transport or redirect boundary behavior changed"
    );
    let expected = ["/redirect", "/redirect", "/target", "/redirect"]
        .into_iter()
        .map(str::to_string)
        .chain((0..=10).map(|step| format!("/within/{step}")))
        .chain((0..=10).map(|step| format!("/over/{step}")))
        .collect::<Vec<_>>();
    assert_eq!(
        *requests.lock().unwrap(),
        expected,
        "redirect boundaries sent an unexpected request sequence"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_client_bounds_and_strictly_decodes_response_bodies() {
    use std::io::{Read, Write};

    const LIMIT: usize = 8 * 1024 * 1024;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let cases = [
            ("200 OK", vec![b'a'; LIMIT], false, None),
            ("200 OK", vec![0xff], false, None),
            ("404 Not Found", b"missing".to_vec(), false, None),
            ("413 Payload Too Large", vec![b'b'; LIMIT + 1], false, None),
            ("200 OK", vec![b'c'; LIMIT], true, None),
            ("413 Payload Too Large", vec![b'd'; LIMIT + 1], true, None),
            ("200 OK", b"no".to_vec(), false, Some(5)),
            ("502 Bad Gateway", b"no".to_vec(), true, Some(2)),
        ];
        for (status, body, chunked, claimed_len) in cases {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            if chunked {
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
                    claimed_len.unwrap_or(body.len())
                )
                .unwrap();
                let _ = stream.write_all(&body);
                if claimed_len.is_none() {
                    let _ = stream.write_all(b"\r\n0\r\n\r\n");
                } else {
                    let _ = stream.write_all(b"\r\n");
                }
            } else {
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    claimed_len.unwrap_or(body.len())
                )
                .unwrap();
                let _ = stream.write_all(&body);
            }
        }
        for response in [
            "NOT HTTP\r\nConnection: close\r\n\r\n".to_string(),
            format!("HTTP/1.1 200 OK\r\n{}\r\n", "X: y\r\n".repeat(102)),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_body_bounds_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.http.client as client

fn run() {
    first :: client.get("http://__ADDR__/")
    if first == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print(bytes.len()) }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    second :: client.get("http://__ADDR__/")
    if second == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected utf8 success: {text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    third :: client.get("http://__ADDR__/")
    if third == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("{response.status()}:{text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    fourth :: client.get("http://__ADDR__/")
    if fourth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print("unexpected oversized success: {bytes.len()}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    fifth :: client.get("http://__ADDR__/")
    if fifth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print(bytes.len()) }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    sixth :: client.get("http://__ADDR__/")
    if sixth == {
        .Ok(response) -> {
            if response.body().bytes(8388608) == {
                .Ok(bytes) -> { print("unexpected chunked oversized success: {bytes.len()}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    seventh :: client.get("http://__ADDR__/")
    if seventh == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected partial content-length success: {text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    eighth :: client.get("http://__ADDR__/")
    if eighth == {
        .Ok(response) -> {
            if response.body().text(8388608) == {
                .Ok(text) -> { print("unexpected partial chunked success: {response.status()}:{text}") }
                .Err(error) -> { print(error) }
            }
        }
        .Err(error) -> { print(error) }
    }
    ninth :: client.get("http://__ADDR__/")
    if ninth == {
        .Ok(response) -> { print("unexpected malformed status success: {response.status()}") }
        .Err(error) -> { print(error) }
    }
    tenth :: client.get("http://__ADDR__/")
    if tenth == {
        .Ok(response) -> { print("unexpected malformed header success: {response.status()}") }
        .Err(error) -> { print(error) }
    }
}
"#
    .replace("__ADDR__", &addr.to_string());
    let (code, stdout, stderr) = build_and_run(&dir, "http_body_bounds", &src, &[], None);
    server.join().unwrap();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout,
        "8388608\nunsupported HTTP body encoding\n404:missing\nHTTP body exceeds 8388608-byte limit\n8388608\nHTTP body exceeds 8388608-byte limit\nHTTP I/O failed during transport\nHTTP I/O failed during transport\ninvalid HTTP framing\ninvalid HTTP header\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_http_server_public_response_appends_repeated_headers() {
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_http_server_headers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.http.client as client
use core.http.server as server
use core.net as net
use core.tasks as tasks

fn run() {
    listener :: net.tcp_listen("127.0.0.1:0") ?? panic("bind")
    addr :: listener.local_addr() ?? panic("address")
    mux :: server.mux()
    mux.get("/", (req: HTTPRequest) =>
        .Ok(server.response(200, "ok")
            .header("Set-Cookie", "a=1")
            .header("Set-Cookie", "b=2"))
    )
    serving :: tasks.spawn(() =>
        server.serve_once_listener(listener, mux) ?? panic("serve")
    )
    response :: client.get("http://{addr}/") ?? panic("get")
    cookies :: response.cookies()
    print(cookies.len())
    print(cookies[0])
    print(cookies[1])
    serving.join()
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "http_server_headers", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "2\na=1\nb=2\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_typed_csv_group_stats_status_and_plot() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_core",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

#Codable
struct Budget {
    team: String
    owner: String
}

fn must_stay_deferred(ticket: Ticket) => Bool {
    panic("lazy filter ran before collect")
    return false
}

fn missing_minutes() => Float? = None

fn run() {
    raw :: "team,minutes\nCore,4.0\nTools,5.0\nCore,8.0\nTools,7.0"
    rows :: data.csv<Ticket>(raw) ?? panic("bad csv")
    budget_raw :: "team,owner\nCore,Ada\nCore,Lin\nTools,Grace"
    budgets :: data.csv<Budget>(budget_raw) ?? panic("bad budget")
    print(data.count(rows))
    table :: data.table(rows)
    lazy :: data.lazy(table)
    deferred :: data.lazy_filter(lazy, (t) => must_stay_deferred(t))
    print(data.plan(deferred)[1])
    planned :: data.lazy_sort_by(data.lazy_filter(lazy, (t) => t.minutes >= 6.0), (t) => t.team)
    collected :: data.collect(planned) ?? panic("collect")
    print(data.count(table))
    print(data.count(planned))
    print(data.count(data.rows(collected)))
    print(data.plan(planned)[2])
    loop ticket, data.rows(collected) {
        print("planned:{ticket.team}:{ticket.minutes}")
    }
    maybe_minutes :: [ Val(2.0), missing_minutes(), Val(6.0), missing_minutes() ]
    series :: data.series(maybe_minutes)
    print(data.count(series))
    print(data.missing_count(series))
    groups :: data.group_mean(rows, (t) => t.team, (t) => t.minutes) ?? panic("group")
    loop g, groups {
        print("{g.key}:{g.count}:{g.sum}:{g.mean}")
    }
    values :: [2.0, 4.0, 6.0]
    print(data.sum(values) ?? panic("sum"))
    print(data.mean(values) ?? panic("mean"))
    joined :: data.inner_join(rows, budgets, (t) => t.team, (b) => b.team) ?? panic("join")
    loop pair, joined {
        print("{pair.left.team}:{pair.right.owner}")
    }
    left :: data.left_join(rows, [budgets[0]], (t) => t.team, (b) => b.team) ?? panic("left")
    loop pair, left {
        if pair.right == {
            Val(budget) -> print("{pair.left.team}:{budget.owner}")
            None -> print("{pair.left.team}:none")
        }
    }
    pivot :: data.pivot_sum(rows, (t) => t.team, (t) => if t.minutes >= 6.0 -> "long" else -> "short", (t) => t.minutes) ?? panic("pivot")
    loop cell, pivot {
        print("{cell.row_key}|{cell.column_key}:{cell.count}")
    }
    rolling :: data.rolling_mean([2.0, 4.0, 6.0], 2) ?? panic("rolling")
    print(rolling[2])
    counts :: data.group_count(rows, (t) => t.team) ?? panic("count")
    print(data.bar_text(counts) ?? panic("bar"))
    print((data.bar_svg(counts) ?? panic("svg")).len())
    status :: data.status()
    print("{status[0].step}:{status[0].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data program failed: {stderr}");
    assert_eq!(
        stdout,
        "4\nfilter\n4\n2\n2\nsort_by\nplanned:Core:8.0\nplanned:Tools:7.0\n4\n2\nCore:2:12.0:6.0\nTools:2:12.0:6.0\n12.0\n4.0\nCore:Ada\nCore:Lin\nTools:Grace\nCore:Ada\nCore:Lin\nTools:Grace\nCore:Ada\nTools:none\nCore:Ada\nTools:none\nCore|long:1\nCore|short:1\nTools|long:1\nTools|short:1\n5.0\nCore | ## 2\nTools | ## 2\n531\ncore.data.csv:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_stream_limits_and_typed_errors() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data stream test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let csv_path = dir.join("events.csv");
    fs::write(&csv_path, "service,latency_ms\napi,10.0\napi,20.0\ndb,5.0\napi,30.0\n").unwrap();
    let path_lit = csv_path.to_string_lossy().replace('\\', "\\\\");
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_stream",
        &format!(
            r#"
use core.data as data
use core.files as files

#Codable
struct Event {{
    service: String
    latency_ms: Float
}}

fn run() {{
    input :: files.open("{path_lit}") ?? panic("open")
    limits := data.DataLimits.safe()
    limits.max_groups = 1
    reader :: data.csv_reader<Event>(input, limits) ?? panic("reader")
    first :: reader.next() ?? panic("next")
    if first == {{
        Val(row) -> print("first:{{row.service}}")
        None -> panic("eof")
    }}
    groups := data.group_mean(reader, (e) => e.service, (e) => e.latency_ms)
    if groups == {{
        .Ok(_) -> print("unexpected ok")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
    empty := data.mean([Float].{{}})
    if empty == {{
        .Ok(_) -> print("unexpected mean")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
    bad := data.quantile([1.0, 2.0], 1.5)
    if bad == {{
        .Ok(_) -> print("unexpected q")
        .Err(error) -> print("{{error.kind}} {{error.operation}}")
    }}
}}
"#
        ),
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data stream program failed: {stderr}");
    assert_eq!(
        stdout,
        "first:api\nLimit group_mean\nEmpty mean\nInvalidArgument quantile\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_schema_ingest_and_select() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data schema test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_schema_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_schema",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    raw :: "team,minutes\nCore,4.0\nTools,5.0\nCore,8.0"
    rows :: data.csv<Ticket>(raw) ?? panic("bad csv")
    table :: data.table(rows)
    cols :: data.schema(table)
    loop c, cols {
        print("{c.name}:{c.type_name}")
    }
    selected :: data.filter(data.rows(table), (t) => t.minutes >= 5.0)
    print("selected:{data.count(selected)}")
    loop t, selected {
        print("{t.team}:{t.minutes}")
    }
    print("{data.status()[5].step}:{data.status()[5].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data schema program failed: {stderr}");
    assert_eq!(
        stdout,
        "team:String\nminutes:Float\nselected:2\nTools:5.0\nCore:8.0\ncore.data.schema:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_json_ingest_and_select() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data json test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_data_json_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_json",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

fn run() {
    raw :: "[{{\"team\":\"Core\",\"minutes\":4.0}},{{\"team\":\"Tools\",\"minutes\":5.0}},{{\"team\":\"Core\",\"minutes\":8.0}}]"
    rows :: data.json<Ticket>(raw) ?? panic("bad json")
    table :: data.table(rows)
    cols :: data.schema(table)
    loop c, cols {
        print("{c.name}:{c.type_name}")
    }
    selected :: data.filter(data.rows(table), (t) => t.minutes >= 5.0)
    print("selected:{data.count(selected)}")
    loop t, selected {
        print("{t.team}:{t.minutes}")
    }
    print("{data.status()[6].step}:{data.status()[6].path}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data json program failed: {stderr}");
    assert_eq!(
        stdout,
        "team:String\nminutes:Float\nselected:2\nTools:5.0\nCore:8.0\ncore.data.json:native\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_data_schema_empty_table_and_series_law() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.data empty schema test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_data_schema_empty_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "data_schema_empty",
        r#"
use core.data as data

#Codable
struct Ticket {
    team: String
    minutes: Float
}

struct Empty {}

struct Box<T> {
    value: T
}

fn run() {
    empty_rows := [Ticket].{}
    empty_table :: data.table(empty_rows)
    loop c, data.schema(empty_table) {
        print("empty:{c.name}:{c.type_name}")
    }

    nums :: data.series([1.0, 2.0])
    loop c, data.schema(nums) {
        print("float:{c.name}:{c.type_name}")
    }

    tickets :: data.series([Ticket.{team: "Core", minutes: 4.0}])
    loop c, data.schema(tickets) {
        print("struct:{c.name}:{c.type_name}")
    }

    empty_tickets := [Ticket].{}
    empty_series :: data.series(empty_tickets)
    loop c, data.schema(empty_series) {
        print("empty_series:{c.name}:{c.type_name}")
    }

    empty_units := [Empty].{}
    print("empty_struct:{data.count(data.schema(data.table(empty_units)))}")

    boxed := [Box<Int>].{}
    loop c, data.schema(data.table(boxed)) {
        print("generic:{c.name}:{c.type_name}")
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.data empty schema program failed: {stderr}");
    assert_eq!(
        stdout,
        "empty:team:String\nempty:minutes:Float\nfloat:value:Float\nstruct:value:Ticket\nempty_series:value:Ticket\nempty_struct:0\ngeneric:value:Int\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_input_reads_a_line_from_stdin() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping io.input test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_input_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "input_demo",
        r#"
use core.io as io

fn run() {
    name :: io.input("name? ") ?? panic("read failed")
    print("hello, {name}")
}
"#,
        &[],
        Some("Ada\n"),
    );
    assert_eq!(code, 0, "stdin demo failed");
    assert!(
        stdout.contains("hello, Ada"),
        "expected greeting on stdout, got stdout={stdout:?} stderr={stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_prompt_helpers_validate_choices_and_refuse_non_tty_secrets() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.io prompt test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_prompts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = include_str!("../examples/features/io/terminal_parity.jet");
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "terminal_parity",
        source,
        &[],
        Some("\nnot-a-number\n3\n2\n"),
    );
    assert_eq!(code, 0, "prompt fixture failed: {stderr}");
    assert_eq!(
        stdout,
        include_str!("../examples/features/expected/io/terminal_parity.out")
    );
    assert_eq!(
        stderr,
        include_str!("../examples/features/expected/io/terminal_parity.stderr.out")
    );

    #[cfg(unix)]
    {
        let shell = r#"
{
  sleep 0.2
  printf '\r'
  sleep 0.1
  printf 'bad\r3\r2\r'
  sleep 0.2
  printf 'swordfish\r'
} | timeout 8s script -qec '"$JET_PROMPT_BIN"' /dev/null
"#;
        let output = Command::new("sh")
            .args(["-c", shell])
            .env("JET_PROMPT_BIN", dir.join("terminal_parity"))
            .env("NO_COLOR", "1")
            .output()
            .expect("run prompt fixture under PTY");
        let shown = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "PTY prompt failed:\n{shown}");
        assert!(shown.contains("secret length: 9"), "{shown}");
        assert!(!shown.contains("swordfish"), "secret was echoed:\n{shown}");
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn random_and_time_output_pins_with_seed_and_epoch() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping random/time pin test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_time_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "time_random",
        r#"
use core.random as random
use core.time as time

fn run() {
    random.seed(42)
    print(random.int(1, 100))
    print(random.float())
    print(time.now())
}
"#,
        &[("LEX_TEST_EPOCH", "1700000000000")],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "9\n0.05534409481976061\n1700000000000\n");
    let _ = fs::remove_dir_all(&dir);
}

/// #1788/#1781: an immutable `::` binding of a `core.random` call must read
/// the runtime-seeded PRNG exactly like a mutable `:=` binding does. Before
/// the fix, sema's D-VERDICT-1308-1 implicit fold treated `random.float()` as
/// a foldable pure call and baked its value at compile time from a disjoint
/// ambient interpreter PRNG, so two identical `seed(11); x :: random.float()`
/// pairs never matched and never landed on the seeded stream either.
#[test]
fn immutable_binding_of_random_call_reads_the_seeded_stream() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping immutable-random-binding test (need rustc)");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_random_immutable_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "random_immutable",
        r#"
use core.random as random

fn run() {
    random.seed(11)
    a :: random.float()
    random.seed(11)
    b :: random.float()
    print(a == b)
    random.seed(11)
    c := random.float()
    print(a == c)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(
        stdout, "true\ntrue\n",
        "reseeded `::` bindings must match each other and the `:=` binding's seeded draw"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// #1799: an immutable `::` binding of `date.today()` must read the runtime
/// clock. Before the fix, D-VERDICT-1308-1 folded the ambient wall-clock read
/// into the generated literal, so the artifact kept the build date forever.
#[test]
fn immutable_binding_of_date_today_reads_the_runtime_clock() {
    let src = r#"
use core.time.date as date

fn run() {
    a :: date.today()
    b :: date.today()
    print(a == b)
}
"#;
    let compiled = compile_temp("date_today_immutable", src);
    let user_run = compiled
        .rust
        .split_once("pub fn user_run() {")
        .and_then(|(_, body)| body.split_once("\n}\n").map(|(body, _)| body))
        .expect("generated Rust must contain the user_run body");
    assert_eq!(
        user_run.matches("JetDate::today_utc()").count(),
        2,
        "both immutable date.today() calls must remain runtime reads:\n{user_run}"
    );

    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping immutable-date-today-binding test (need rustc)");
        return;
    }
    let dir =
        std::env::temp_dir().join(format!("jet_corelib_date_today_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(&dir, "date_today_immutable", src, &[], None);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout, "true\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn random_distribution_surface_is_deterministic() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping random distribution test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_random_dist_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "random_dist",
        r#"
use core.random as random

fn run() {
    random.seed(7)
    print(random.bool(1.0))
    print(random.float_range(10.0, 20.0) >= 10.0)
    random.seed(11)
    a := random.normal(0.0, 1.0)
    random.seed(11)
    b := random.normal(0.0, 1.0)
    print(a == b)
    print(random.exponential(2.0) >= 0.0)
    items := ["red", "green", "blue"]
    weights := [0.0, 1.0, 0.0]
    print(random.weighted_pick(items, weights) ?? "none")
    print(random.sample(items, 2).len())
    print(random.bytes(4).len())
    rng := random.rng(99)
    print(rng.float_range(1.0, 2.0) >= 1.0)
    print(rng.bool(1.0))
    print(rng.weighted_pick(items, weights) ?? "none")
    print(rng.sample(items, 2).len())
    print(rng.bytes(3).len())
    child := rng.split()
    print(child.int(1, 1))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "random distribution test failed: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\ntrue\ntrue\ngreen\n2\n4\ntrue\ntrue\ngreen\n2\n3\n1\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn encoding_breadth_codecs_share_data_tree() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping encoding breadth test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_encoding_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "encoding_breadth",
        r#"
use core.encoding.json as json
use core.encoding.jsonl as jsonl
use core.encoding.xml as xml
use core.encoding.cbor as cbor
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn run() {
    data := json.parse("{{\"b\":2,\"a\":1}}") ?? panic("json")
    print(json.canonical(data) ?? panic("value is not canonical JSON"))
    print(json.events(data).contains("object_start $"))
    rows := jsonl.parse("{{\"a\":1}}\n{{\"a\":2}}\n") ?? panic("jsonl")
    print(rows.len())
    print(jsonl.to_string(rows).contains("\"a\":1"))
    source := "<r xmlns=\"urn:r\" xmlns:h=\"urn:h\" h:a=\"x&amp;y\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><h:c/></r>"
    doc := xml.parse(source) ?? panic("xml")
    print(xml.to_string(doc))
    print((doc.field("$xml") ?? panic("document tag")).text() ?? "bad")
    root := (doc.field("children") ?? panic("document children")).at(0) ?? panic("root")
    name := root.field("name") ?? panic("root name")
    print((name.field("namespace_uri") ?? panic("root namespace")).text() ?? "bad")
    content := root.field("children") ?? panic("root children")
    entity := content.at(1) ?? panic("entity")
    comment := content.at(2) ?? panic("comment")
    cdata := content.at(3) ?? panic("cdata")
    pi := content.at(4) ?? panic("pi")
    print((entity.field("$xml") ?? panic("entity tag")).text() ?? "bad")
    print((comment.field("$xml") ?? panic("comment tag")).text() ?? "bad")
    print((cdata.field("$xml") ?? panic("cdata tag")).text() ?? "bad")
    print((pi.field("$xml") ?? panic("pi tag")).text() ?? "bad")
    encoded := cbor.to_bytes(data) ?? panic("cbor encode")
    print(encoded.len() > 0)
    decoded := cbor.parse(encoded) ?? panic("cbor parse")
    print(json.canonical(decoded) ?? panic("value is not canonical JSON"))
    bytes :: [U8].{ 104, 105 }
    u := base64.encode_url(bytes)
    print(u)
    print((base64.decode_url(u) ?? panic("base64url")).len())
    b32 := base32.encode(bytes)
    print(b32)
    print((base32.decode(b32) ?? panic("base32")).len())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "encoding breadth test failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"a\":1,\"b\":2}\ntrue\n2\ntrue\n<r xmlns=\"urn:r\" xmlns:h=\"urn:h\" h:a=\"x&amp;y\">a&amp;<!--c--><![CDATA[<x>]]><?go now?><h:c/></r>\ndocument\nurn:r\nentity_ref\ncomment\ncdata\nprocessing_instruction\ntrue\n{\"a\":1,\"b\":2}\naGk\n2\nNBUQ====\n2\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_dual_limits_validate_in_ratified_order_and_fuse_stronger_bounds() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML dual-limits test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_dual_limits_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let probe = dir.join("probe.xml");
    fs::write(&probe, "<a><b><c/></b></a>").unwrap();
    let probe = probe.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    // EncodingLimits fail first even when XMLLimits are also illegal.
    enc_bad := encoding.EncodingLimits.safe()
    enc_bad.buffer_bytes = 1
    xml_bad := xml.XMLParseOptions.safe()
    xml_bad.limits.max_depth = 0
    input1 :: files.open("{probe}") ?? panic("open1")
    if xml.reader(^input1, enc_bad, xml_bad) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // EncodingLimits ok → XMLLimits field order: max_depth before max_nodes.
    enc_ok := encoding.EncodingLimits.safe()
    xml_depth := xml.XMLParseOptions.safe()
    xml_depth.limits.max_depth = 0
    xml_depth.limits.max_nodes = 0
    input2 :: files.open("{probe}") ?? panic("open2")
    if xml.reader(^input2, enc_ok, xml_depth) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // Cross-field XMLLimits after ranges.
    enc_ok2 := encoding.EncodingLimits.safe()
    xml_cross := xml.XMLParseOptions.safe()
    xml_cross.limits.max_depth = 2
    xml_cross.limits.max_entity_depth = 3
    input3 :: files.open("{probe}") ?? panic("open3")
    if xml.reader(^input3, enc_ok2, xml_cross) == {{
        .Ok(_) -> {{ print("accepted") }}
        .Err(error) -> {{
            print(error.format == encoding.EncodingFormat.XML)
            print(error.kind == encoding.EncodingErrorKind.Limit)
            print(error.byte_offset)
            print(error.line ?? -1)
            print(error.column ?? -1)
            print(error.path)
            print(error.reason)
        }}
    }}

    // Encoding depth tighter than XML depth: error names the fused bound.
    // XMLLimits must be self-valid before EncodingLimits fusion.
    enc_tight := encoding.EncodingLimits.safe()
    enc_tight.max_depth = 2
    xml_loose := xml.XMLParseOptions.safe()
    xml_loose.limits.max_depth = 8
    xml_loose.limits.max_entity_depth = 8
    input4 :: files.open("{probe}") ?? panic("open4")
    reader :: xml.reader(^input4, enc_tight, xml_loose) ?? panic("fused reader")
    loop true {{
        result :: reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("depth-missed"); break }}
                }}
            }}
            .Err(error) -> {{
                print(error.kind == encoding.EncodingErrorKind.Limit)
                print(error.reason)
                break
            }}
        }}
    }}

    // XML depth tighter than Encoding depth.
    enc_loose := encoding.EncodingLimits.safe()
    xml_tight := xml.XMLParseOptions.safe()
    xml_tight.limits.max_depth = 1
    xml_tight.limits.max_entity_depth = 1
    deep_input :: files.open("{probe}") ?? panic("open deep")
    deep_reader :: xml.reader(^deep_input, enc_loose, xml_tight) ?? panic("xml-tight reader")
    loop true {{
        result :: deep_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("xml-depth-missed"); break }}
                }}
            }}
            .Err(error) -> {{
                print(error.kind == encoding.EncodingErrorKind.Limit)
                print(error.reason)
                break
            }}
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_dual_limits", &source, &[], None);
    assert_eq!(code, 0, "XML dual-limits test failed: {stderr}");
    assert_eq!(
        stdout,
        concat!(
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "buffer_bytes 1 is outside 4096..16777216\n",
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "XML limit `max_depth` must be between 1 and 4096\n",
            "true\n",
            "true\n",
            "0\n",
            "-1\n",
            "-1\n",
            "\n",
            "XML limit `max_entity_depth` exceeds `max_depth`\n",
            "true\n",
            "XML element nesting exceeds max_depth (2)\n",
            "true\n",
            "XML element nesting exceeds max_depth (1)\n",
        )
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_whole_byte_verbs_match_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML whole-byte parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_whole_bytes_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn same_bytes(left: [U8], right: [U8]) => Bool {
    if left.len() != right.len() { return false }
    loop index, 0..<left.len() {
        if left[index] != right[index] { return false }
    }
    return true
}

fn summarize() => String {
    plain :: [U8].{ 60, 114, 62, 111, 107, 60, 47, 114, 62 }
    utf8_bom :: [U8].{ 239, 187, 191, 60, 63, 120, 109, 108, 32, 118, 101, 114, 115, 105, 111, 110, 61, 39, 49, 46, 48, 39, 32, 101, 110, 99, 111, 100, 105, 110, 103, 61, 39, 85, 84, 70, 45, 56, 39, 63, 62, 60, 114, 62, 195, 169, 240, 159, 153, 130, 60, 47, 114, 62 }
    utf16 :: [U8].{ 255, 254, 60, 0, 63, 0, 120, 0, 109, 0, 108, 0, 32, 0, 118, 0, 101, 0, 114, 0, 115, 0, 105, 0, 111, 0, 110, 0, 61, 0, 39, 0, 49, 0, 46, 0, 48, 0, 39, 0, 32, 0, 101, 0, 110, 0, 99, 0, 111, 0, 100, 0, 105, 0, 110, 0, 103, 0, 61, 0, 39, 0, 85, 0, 84, 0, 70, 0, 45, 0, 49, 0, 54, 0, 39, 0, 63, 0, 62, 0, 60, 0, 114, 0, 62, 0, 233, 0, 61, 216, 66, 222, 60, 0, 47, 0, 114, 0, 62, 0 }
    conflict :: [U8].{ 255, 254, 60, 0, 63, 0, 120, 0, 109, 0, 108, 0, 32, 0, 118, 0, 101, 0, 114, 0, 115, 0, 105, 0, 111, 0, 110, 0, 61, 0, 39, 0, 49, 0, 46, 0, 48, 0, 39, 0, 32, 0, 101, 0, 110, 0, 99, 0, 111, 0, 100, 0, 105, 0, 110, 0, 103, 0, 61, 0, 39, 0, 85, 0, 84, 0, 70, 0, 45, 0, 56, 0, 39, 0, 63, 0, 62, 0, 60, 0, 114, 0, 47, 0, 62, 0 }

    plain_doc := xml.parse_bytes(plain) ?? panic("plain parse")
    plain_out := xml.to_bytes(plain_doc) ?? panic("plain render")
    utf8_doc := xml.parse_bytes(utf8_bom) ?? panic("UTF-8 BOM parse")
    utf8_out := xml.to_bytes(utf8_doc, xml.XMLRenderOptions.{ encoding: .UTF8BOM, lexical: .PreserveValid }) ?? panic("UTF-8 BOM render")
    utf16_doc := xml.parse_bytes(utf16) ?? panic("UTF-16 parse")
    utf16_out := xml.to_bytes(utf16_doc, xml.XMLRenderOptions.{ encoding: .UTF16LE, lexical: .PreserveValid }) ?? panic("UTF-16 render")

    conflict_result :: DataTree ? XMLError.{ xml.parse_bytes(conflict) }
    if conflict_result == {
        .Ok(_) -> return "encoding-conflict-missed"
        .Err(error) -> {
            reason_ok :: error.reason == "XML declaration conflicts with detected input encoding"
            return "{same_bytes(plain_out, plain)}|{same_bytes(utf8_out, utf8_bom)}|{same_bytes(utf16_out, utf16)}|{reason_ok}|{error.byte_offset}|{error.line}|{error.column}|{error.path}|{error.reason}"
        }
    }
    return "unreachable"
}

$expected :: summarize()

fn run() {
    print(expected)
    print(summarize())
}
"#;
    let expected = concat!(
        "true|true|true|true|2|1|1|$|XML declaration conflicts with detected input encoding\n",
        "true|true|true|true|2|1|1|$|XML declaration conflicts with detected input encoding\n",
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_whole_bytes", source, &[], None);
    assert_eq!(code, 0, "XML whole-byte AOT fixture failed: {stderr}");
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_whole_bytes.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new()));
        }
        other => panic!("XML whole-byte default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_10_fifth_edition_char_errors_match_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML character parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_chars_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn show(result: DataTree ? XMLError) => String {
    if result == {
        .Ok(_) -> { return "accepted" }
        .Err(error) -> {
            return "{error.byte_offset}|{error.line}|{error.column}|{error.path}|{error.reason}"
        }
    }
    return "unreachable"
}

$numeric :: show(xml.parse("<r>&#0;</r>"))
$attribute :: show(xml.parse("<r a='&#0;'/>"))
$namespace :: show(xml.parse("<r xmlns='&#0;'/>"))

fn run() {
    runtime_numeric :: show(xml.parse("<r>&#0;</r>"))
    runtime_attribute :: show(xml.parse("<r a='&#0;'/>"))
    runtime_namespace :: show(xml.parse("<r xmlns='&#0;'/>"))
    print("{numeric}|{runtime_numeric}")
    print("{attribute}|{runtime_attribute}")
    print("{namespace}|{runtime_namespace}")
}
"#;
    let expected = concat!(
        "3|1|4|$/r|invalid numeric character reference|3|1|4|$/r|invalid numeric character reference\n",
        "6|1|7|$|invalid numeric character reference|6|1|7|$|invalid numeric character reference\n",
        "10|1|11|$|invalid numeric character reference|10|1|11|$|invalid numeric character reference\n",
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_chars", &source, &[], None);
    assert_eq!(code, 0, "XML character AOT fixture failed: {stderr}");
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_chars.jet");
    fs::write(&dev_path, &source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new()));
        }
        other => panic!("XML character default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_attribute_whitespace_normalization_matches_comptime_aot_and_dev() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML attribute normalization parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_xml_attribute_normalization_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.xml as xml

fn summarize(source: String) => String {
    doc := xml.parse(source) ?? panic("xml")
    root := (doc.field("children") ?? panic("document children")).at(0) ?? panic("root")
    namespace := ((root.field("namespaces") ?? panic("namespaces")).at(0) ?? panic("namespace")).field("namespace_uri") ?? panic("namespace URI")
    attributes := root.field("attributes") ?? panic("attributes")
    literal := ((attributes.at(0) ?? panic("literal attribute")).field("normalized_value") ?? panic("literal normalized value")).text() ?? "bad"
    reference := ((attributes.at(1) ?? panic("reference attribute")).field("normalized_value") ?? panic("reference normalized value")).text() ?? "bad"
    namespace_ok := (namespace.text() ?? "bad") == "urn: foo bar"
    literal_ok := literal == "A B C D E"
    lexical_ok := xml.to_string(doc) == source
    return "{namespace_ok}|{literal_ok}|{reference.len()}|{lexical_ok}"
}

$cr :: String.from_bytes([13]) ?? panic("CR")
$close :: "/>"
$source :: "<r xmlns='urn:\tfoo\nbar' a='A\tB\nC{cr}\nD{cr}E' b='&#xD;&#xA;&#x9;'{close}"
$normalized :: summarize(source)

fn run() {
    runtime := summarize(source)
    print("{normalized}|{runtime}")
}
"#;
    let expected = "true|true|3|true|true|true|3|true\n";
    let (code, stdout, stderr) =
        build_and_run(&dir, "xml_attribute_normalization", source, &[], None);
    assert_eq!(
        code, 0,
        "XML attribute normalization AOT fixture failed: {stderr}"
    );
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");

    let dev_path = dir.join("xml_attribute_normalization.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, exit_code } => {
            assert_eq!(
                (exit_code, stdout, stderr),
                (0, expected.to_string(), String::new())
            );
        }
        other => panic!("XML attribute normalization default-dev fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn base_decoders_preserve_2026_union_with_comptime_aot_and_dev_parity() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping base decoder parity test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_base_decoder_parity_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.encoding.base64 as base64
use core.encoding.base32 as base32

fn show64(text: String) => String {
    if base64.decode(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

fn show64url(text: String) => String {
    if base64.decode_url(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

fn show32(text: String) => String {
    if base32.decode(text) == {
        .Ok(bytes) -> { return "OK:{bytes}" }
        .Err(reason) -> { return "ERR:{reason}" }
    }
    return "unreachable"
}

$standard_ws :: show64("Z g = =\n")
$standard_unpadded :: show64("Zg")
$standard_interior :: show64("Zg=A")
$standard_excess :: show64("Zg====")
$standard_bits :: show64("Zh==")
$standard_padding :: show64("=AAA")
$standard_alphabet :: show64("Zg-=")
$standard_size :: show64("A")
$url_outer_ws :: show64url(" \tZg==\n")
$url_interior :: show64url("Zg=A")
$url_standard_alphabet :: show64url("+w")
$url_bits :: show64url("Zh")
$url_padding :: show64url("=AAA")
$url_size :: show64url("A")
$base32_loose :: show32("m=y======\n")
$base32_bits :: show32("MZ======")
$base32_short :: show32("A")
$base32_alphabet :: show32("M0======")

fn run() {
    r_standard_ws := show64("Z g = =\n")
    r_standard_unpadded := show64("Zg")
    r_standard_interior := show64("Zg=A")
    r_standard_excess := show64("Zg====")
    r_standard_bits := show64("Zh==")
    r_standard_padding := show64("=AAA")
    r_standard_alphabet := show64("Zg-=")
    r_standard_size := show64("A")
    r_url_outer_ws := show64url(" \tZg==\n")
    r_url_interior := show64url("Zg=A")
    r_url_standard_alphabet := show64url("+w")
    r_url_bits := show64url("Zh")
    r_url_padding := show64url("=AAA")
    r_url_size := show64url("A")
    r_base32_loose := show32("m=y======\n")
    r_base32_bits := show32("MZ======")
    r_base32_short := show32("A")
    r_base32_alphabet := show32("M0======")
    print("{standard_ws}|{r_standard_ws}")
    print("{standard_unpadded}|{r_standard_unpadded}")
    print("{standard_interior}|{r_standard_interior}")
    print("{standard_excess}|{r_standard_excess}")
    print("{standard_bits}|{r_standard_bits}")
    print("{standard_padding}|{r_standard_padding}")
    print("{standard_alphabet}|{r_standard_alphabet}")
    print("{standard_size}|{r_standard_size}")
    print("{url_outer_ws}|{r_url_outer_ws}")
    print("{url_interior}|{r_url_interior}")
    print("{url_standard_alphabet}|{r_url_standard_alphabet}")
    print("{url_bits}|{r_url_bits}")
    print("{url_padding}|{r_url_padding}")
    print("{url_size}|{r_url_size}")
    print("{base32_loose}|{r_base32_loose}")
    print("{base32_bits}|{r_base32_bits}")
    print("{base32_short}|{r_base32_short}")
    print("{base32_alphabet}|{r_base32_alphabet}")
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "base_decoder_parity", source, &[], None);
    assert_eq!(code, 0, "base decoder AOT parity fixture failed: {stderr}");
    let expected = concat!(
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "ERR:invalid base64 at byte 0: padding may appear only at the end|ERR:invalid base64 at byte 0: padding may appear only at the end\n",
        "ERR:invalid base64 at byte 2: byte 0x2D is not in the standard base64 alphabet|ERR:invalid base64 at byte 2: byte 0x2D is not in the standard base64 alphabet\n",
        "ERR:invalid base64 at byte 1: encoded length cannot represent whole bytes|ERR:invalid base64 at byte 1: encoded length cannot represent whole bytes\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[251]|OK:[251]\n",
        "OK:[102]|OK:[102]\n",
        "ERR:invalid base64url at byte 0: padding may appear only at the end|ERR:invalid base64url at byte 0: padding may appear only at the end\n",
        "ERR:invalid base64url at byte 1: encoded length cannot represent whole bytes|ERR:invalid base64url at byte 1: encoded length cannot represent whole bytes\n",
        "OK:[102]|OK:[102]\n",
        "OK:[102]|OK:[102]\n",
        "OK:[]|OK:[]\n",
        "ERR:invalid base32 at byte 1: byte 0x30 is not in the base32 alphabet|ERR:invalid base32 at byte 1: byte 0x30 is not in the base32 alphabet\n",
    );
    assert_eq!(stdout, expected);
    let dev_path = dir.join("base_decoder_parity.jet");
    fs::write(&dev_path, source).unwrap();
    match jet::Interpreter::dev_iteration(dev_path.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => assert_eq!((exit_code, stdout, stderr), (0, expected.to_string(), String::new())),
        other => panic!("base decoder default-dev parity fixture failed: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_reader_is_incremental_exact_and_terminal() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML stream test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_stream_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let boundary_paths = (4078..=4110)
        .map(|padding| {
            let path = dir.join(format!("boundary-{padding}.xml"));
            fs::write(
                &path,
                format!(
                    "{}<r xmlns=\"urn:r\" a=\"x&amp;y\">é</r>",
                    " ".repeat(padding)
                ),
            )
            .unwrap();
            format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let malformed = dir.join("malformed.xml");
    fs::write(&malformed, "<r>").unwrap();
    let invalid_char = dir.join("invalid-char.xml");
    fs::write(&invalid_char, b"<r>\x01</r>").unwrap();
    let limited = dir.join("limited.xml");
    fs::write(&limited, "<root>text</root>").unwrap();
    let encoding_conflict = dir.join("encoding-conflict.xml");
    let mut encoding_conflict_bytes = vec![0xff, 0xfe];
    encoding_conflict_bytes.extend(
        "<?xml version='1.0' encoding='UTF-8'?><r/>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    fs::write(&encoding_conflict, encoding_conflict_bytes).unwrap();
    let malformed = malformed.to_string_lossy().replace('\\', "\\\\");
    let invalid_char = invalid_char.to_string_lossy().replace('\\', "\\\\");
    let limited = limited.to_string_lossy().replace('\\', "\\\\");
    let encoding_conflict = encoding_conflict.to_string_lossy().replace('\\', "\\\\");

    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    paths :: [String].{{ {boundary_paths} }}
    passed := 0
    loop path, paths {{
        input :: files.open(path) ?? panic("open boundary")
        reader :: xml.reader(^input) ?? panic("reader defaults")
        document_start := false
        root_start := false
        document_end := false
        loop true {{
            maybe :: reader.next() ?? panic("boundary next")
            if maybe == {{
                Val(event) -> {{
                    event_kind := (event.field("$xml_event") ?? panic("event tag")).text() ?? ""
                    if event_kind == "document_start" {{
                        wire_encoding := (event.field("encoding") ?? panic("encoding")).text() ?? ""
                        document_start = wire_encoding == "UTF-8"
                    }}
                    if event_kind == "element_start" {{
                        name := event.field("name") ?? panic("name")
                        local := (name.field("local") ?? panic("local")).text() ?? ""
                        namespace := (name.field("namespace_uri") ?? panic("namespace")).text() ?? ""
                        root_start = local == "r" && namespace == "urn:r"
                    }}
                    if event_kind == "document_end" {{ document_end = true }}
                }}
                None -> {{ break }}
            }}
        }}
        eof_again :: reader.next() ?? panic("fused eof")
        if eof_again == {{
            Val(_) -> {{}}
            None -> {{ if document_start && root_start && document_end {{ passed++ }} }}
        }}
    }}
    print(passed)

    malformed_input :: files.open("{malformed}") ?? panic("open malformed")
    malformed_reader :: xml.reader(^malformed_input) ?? panic("malformed reader")
    loop true {{
        result :: malformed_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("malformed-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                again :: malformed_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("terminal-missed") }}
                    .Err(second) -> {{ print(first.byte_offset == second.byte_offset && first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    invalid_char_input :: files.open("{invalid_char}") ?? panic("open invalid character")
    invalid_char_reader :: xml.reader(^invalid_char_input) ?? panic("invalid character reader")
    loop true {{
        result :: invalid_char_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("invalid-character-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                print(first.kind == encoding.EncodingErrorKind.Syntax)
                print(first.byte_offset)
                print(first.line ?? -1)
                print(first.column ?? -1)
                print(first.path)
                print(first.reason)
                again :: invalid_char_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("invalid-character-terminal-missed") }}
                    .Err(second) -> {{ print(first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    total_limits := encoding.EncodingLimits.safe()
    total_limits.max_total_bytes = Val(6)
    total_input :: files.open("{limited}") ?? panic("open total")
    total_reader :: xml.reader(^total_input, total_limits, xml.XMLParseOptions.safe()) ?? panic("total reader")
    loop true {{
        result :: total_reader.next()
        if result == {{
            .Ok(maybe) -> {{
                if maybe == {{
                    Val(_) -> {{}}
                    None -> {{ print("total-missed"); break }}
                }}
            }}
            .Err(first) -> {{
                again :: total_reader.next()
                if again == {{
                    .Ok(_) -> {{ print("total-terminal-missed") }}
                    .Err(second) -> {{ print(first.byte_offset); print(first.reason == second.reason) }}
                }}
                break
            }}
        }}
    }}

    conflict_input :: files.open("{encoding_conflict}") ?? panic("open encoding conflict")
    conflict_reader :: xml.reader(^conflict_input) ?? panic("encoding conflict reader")
    conflict_start :: conflict_reader.next() ?? panic("encoding conflict document start")
    if conflict_start == None {{ panic("missing document start") }}
    conflict :: conflict_reader.next()
    if conflict == {{
        .Ok(_) -> {{ print("encoding-conflict-missed") }}
        .Err(error) -> {{
            print(error.kind == encoding.EncodingErrorKind.Syntax)
            print(error.byte_offset)
            print(error.reason)
        }}
    }}
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_stream", &source, &[], None);
    assert_eq!(code, 0, "XML stream test failed: {stderr}");
    assert_eq!(
        stdout,
        "33\ntrue\ntrue\n3\n1\n4\n$/r\nXML contains forbidden character U+0001\ntrue\n7\ntrue\ntrue\n2\nXML declaration conflicts with detected input encoding\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_stream_writer_and_canonical_surface_run_end_to_end() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML writer test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_writer_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.xml");
    let output = dir.join("output.xml");
    let utf16 = dir.join("output-utf16.xml");
    let source = "<?xml version='1.0'?><r xmlns:p='urn:p' p:a='x&amp;y'>z<p:e/></r>";
    fs::write(&input, source).unwrap();
    let source_code = format!(r#"
use core.encoding.xml as xml
use core.encoding as encoding
use core.files as files

fn run() {{
    input :: files.open("{}") ?? panic("open")
    output :: files.create("{}") ?? panic("create")
    reader :: xml.reader(^input) ?? panic("reader")
    writer :: xml.writer(^output) ?? panic("writer")
    loop true {{
        maybe :: reader.next() ?? panic("next")
        if maybe == {{
            Val(event) -> {{ writer.write(event) ?? panic("write") }}
            None -> {{ break }}
        }}
    }}
    writer.finish() ?? panic("finish")
    writer.finish() ?? panic("idempotent finish")
    input16 :: files.open("{}") ?? panic("open utf16 source")
    output16 :: files.create("{}") ?? panic("create utf16")
    reader16 :: xml.reader(^input16) ?? panic("reader utf16")
    render16 := xml.XMLRenderOptions.{{ encoding: .UTF16LE, lexical: .Deterministic }}
    writer16 :: xml.writer(^output16, encoding.EncodingLimits.safe(), render16) ?? panic("writer utf16")
    loop true {{
        maybe :: reader16.next() ?? panic("next utf16")
        if maybe == {{
            Val(event) -> {{ writer16.write(event) ?? panic("write utf16") }}
            None -> {{ break }}
        }}
    }}
    writer16.finish() ?? panic("finish utf16")
    tree :: xml.parse("<r xmlns:q='urn:q' q:z='2' a='1'><e/></r>") ?? panic("parse")
    options := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: ["q"] }}
    print(xml.canonical(tree, options) ?? panic("canonical"))
}}
"#, input.to_string_lossy().replace('\\', "\\\\"), output.to_string_lossy().replace('\\', "\\\\"), input.to_string_lossy().replace('\\', "\\\\"), utf16.to_string_lossy().replace('\\', "\\\\"));
    let (code, stdout, stderr) = build_and_run(&dir, "xml_writer", &source_code, &[], None);
    assert_eq!(code, 0, "XML writer test failed: {stderr}");
    assert_eq!(stdout, "<r xmlns:q=\"urn:q\" a=\"1\" q:z=\"2\"><e></e></r>\n");
    assert_eq!(fs::read(&output).unwrap(), source.as_bytes());
    let deterministic =
        "<?xml version=\"1.0\"?><r xmlns:p='urn:p' p:a='x&amp;y'>z<p:e/></r>";
    let mut expected16 = vec![0xff, 0xfe];
    expected16.extend(deterministic.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(fs::read(&utf16).unwrap(), expected16);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn xml_reader_writer_hostile_state_and_exclusive_c14n() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping XML hostile/c14n surface test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_xml_hostile_c14n_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("round.xml");
    fs::write(
        &input,
        "<?xml version='1.0'?>\n<!--c-->\n<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>\n",
    )
    .unwrap();
    let input = input.to_string_lossy().replace('\\', "\\\\");
    let source = format!(
        r#"
use core.encoding as encoding
use core.encoding.xml as xml
use core.files as files

fn run() {{
    // Fold/unfold round trip through XMLReader/XMLWriter keeps order + lexical.
    input :: files.open("{input}") ?? panic("open")
    out_path := "{input}.out"
    output :: files.create(out_path) ?? panic("create")
    reader :: xml.reader(^input) ?? panic("reader")
    writer :: xml.writer(^output) ?? panic("writer")
    loop true {{
        maybe :: reader.next() ?? panic("next")
        if maybe == {{
            Val(event) -> {{ writer.write(event) ?? panic("write") }}
            None -> {{ break }}
        }}
    }}
    writer.finish() ?? panic("finish")
    print(files.read(out_path) ?? panic("read out"))

    // Hostile: document_end before document_start → State, no bytes.
    bad_out :: files.create("{input}.bad") ?? panic("bad create")
    bad :: xml.writer(^bad_out) ?? panic("bad writer")
    end := DataTree.Object(["$xml_event": DataTree.Text("document_end")])
    if bad.write(end) == {{
        .Ok(_) -> {{ print("hostile-missed") }}
        .Err(error) -> {{
            print(error.kind == encoding.EncodingErrorKind.State)
            print(error.reason)
        }}
    }}

    // Exclusive C14N omits unused xmlns on ancestors; utilized prefixes move down.
    tree :: xml.parse("<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>") ?? panic("parse")
    options := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: [] }}
    print(xml.canonical(tree, options) ?? panic("canonical"))

    // InclusiveNamespaces PrefixList forces unused b onto the apex.
    forced := xml.XMLCanonical.{{ mode: .Exclusive10, comments: false, inclusive_prefixes: ["b"] }}
    print(xml.canonical(tree, forced) ?? panic("forced"))
}}
"#
    );
    let (code, stdout, stderr) = build_and_run(&dir, "xml_hostile_c14n", &source, &[], None);
    assert_eq!(code, 0, "XML hostile/c14n surface failed: {stderr}");
    assert_eq!(
        stdout,
        "<?xml version='1.0'?>\n<!--c-->\n<root xmlns:a='urn:a' xmlns:b='urn:b'><a:child b:x='1'/></root>\n\ntrue\nXML writer expects document_start first\n<root><a:child xmlns:a=\"urn:a\" xmlns:b=\"urn:b\" b:x=\"1\"></a:child></root>\n<root xmlns:b=\"urn:b\"><a:child xmlns:a=\"urn:a\" b:x=\"1\"></a:child></root>\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn text_unicode_audit_surface_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping text unicode test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_text_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "text_unicode",
        r#"
use core.text as text

fn run() {
    print(text.caseless_eq("Straße", "STRASSE"))
    print(text.nfc("é") == "é")
    print(text.nfkc("ﬃ"))
    print(text.graphemes("é👍").len())
    print(text.words("Hi, κόσμε 123.").len())
    print(text.sentences("One. Two!").len())
    print(text.display_width("表a"))
    print(text.is_alphabetic("Ж"))
    print(text.is_numeric("٣"))
    print(text.pad_start("7", 3, "0"))
    print(text.center("x", 3, "."))
    print(text.starts_any("jetpack", ["jet", "go"]))
    print(text.char_indices("éa")[1])
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "text unicode test failed: {stderr}");
    assert_eq!(
        stdout,
        "true\ntrue\nffi\n2\n3\n2\n3\ntrue\ntrue\n007\n.x.\ntrue\n2:a\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn db_checked_sql_params_feed_parameterized_execute() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping db checked sql test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_db_sql_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "db_checked_sql",
        r#"
use core.db as db

fn run() {
    conn := db.open_memory()
    policy :: db.policy("person", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "owner")
    created :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate")
    skipped :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT, active INTEGER)"
    ]) ?? panic("migrate again")
    id :: 7
    name :: "Ada"
    insert :: SQL.{"INSERT INTO person (id, name, active) VALUES ({id}, {name}, 1)"}
    _inserted :: scoped.execute(insert.template(), db.params(insert)) ?? panic("insert")
    failed :: db.transaction(scoped, "bad batch", [
        "INSERT INTO person (id, name, active) VALUES (8, 'Grace', 1)",
        "INSERT INTO missing_table VALUES (1)"
    ]) ?? 0
    row :: scoped.query_one("SELECT id, name, active FROM person WHERE id = ?", [DBValue.Int(7)]) ?? panic("query")
    found :: row ?? panic("missing")
    count :: scoped.query_one("SELECT COUNT(*) AS n FROM person", []) ?? panic("count")
    counted :: count ?? panic("missing count")
    print(created)
    print(skipped)
    print(failed)
    print(db.row_int(found, "id") ?? 0)
    print(db.row_text(found, "name") ?? "bad")
    print(db.row_int(found, "active") ?? 0)
    print(db.row_int(counted, "n") ?? 0)
    _closed :: scoped.close()
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "db checked sql test failed: {stderr}");
    assert_eq!(stdout, "1\n0\n0\n7\nAda\n1\n1\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_db_implements_driver_trait() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping db Driver trait test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_db_driver_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.db as db

fn count_people<T: Driver>(&conn: T) => Int ? DBError {
    row :: conn.query_one("SELECT COUNT(*) AS n FROM person", [])?
    found :: row ?? panic("missing")
    return .Ok(db.row_int(found, "n") ?? 0)
}

fn run() {
    conn := db.open_memory()
    policy :: db.policy("person", "true") ?? panic("policy")
    scoped := conn.with_policy(policy, "owner")
    _ :: db.migrate(scoped, "person-v1", [
        "CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT)"
    ]) ?? panic("create")
    _ :: scoped.execute(
        "INSERT INTO person (id, name) VALUES (?, ?)",
        [DBValue.Int(1), DBValue.Text("Ada")]
    ) ?? panic("insert")
    n :: count_people(&scoped) ?? panic("count")
    print(n)
    _closed :: scoped.close()
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "db_driver_trait",
        src,
        &[],
        None,
    );
    assert_eq!(code, 0, "db Driver trait AOT failed: {stderr}");
    assert_eq!(stdout, "1\n");

    // I9: default `jet run` (Cranelift) must share the same Driver meaning.
    let jet = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/jet");
    let path = dir.join("db_driver_trait_jit.jet");
    fs::write(&path, src).unwrap();
    let out = Command::new(&jet)
        .arg("run")
        .arg(&path)
        .output()
        .expect("spawn jet run for Driver JIT");
    assert!(
        out.status.success(),
        "db Driver trait JIT failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_fmt_human_formatting_surface_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.fmt runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_fmt_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "human_format",
        r#"
use core.fmt as fmt

fn run() {
    print(fmt.number(1204331))
    print(fmt.decimal(1234.5678, 2))
    print(fmt.percent(0.1234, 1))
    print(fmt.bytes(1500000000))
    print(fmt.duration(222000))
    print(fmt.ordinal(21))
    print(fmt.plural(2, "row", "rows"))
    print(fmt.pad_left("7", 3, "0"))
    print(fmt.pad_right("go", 4, "."))
    print(fmt.pad_center("x", 3, "."))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.fmt program failed: {stderr}");
    assert_eq!(
        stdout,
        "1,204,331\n1,234.57\n12.3%\n1.5 GB\n3m 42s\n21st\n2 rows\n007\ngo..\n.x.\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_log_structured_file_sink_runs() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.log file sink test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_log_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "log_file",
        r#"
use core.log as log

fn run() {
    log.set_sink("jsonl", "service.log")
    s :: log.span("request")
    log.enter(s)
    log.info_fields("served", [log.field("route", "/"), log.int("status", 200), log.redact("token")])
    log.close(s)
    print("done")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.log file sink failed: {stderr}");
    assert_eq!(stdout, "done\n");
    let log = fs::read_to_string(dir.join("service.log")).expect("service.log must be written");
    assert!(log.contains("\"body\":\"served\""), "log: {log}");
    assert!(log.contains("\"route\":\"/\""), "log: {log}");
    assert!(log.contains("\"status\":200"), "log: {log}");
    assert!(log.contains("\"token\":\"[redacted]\""), "log: {log}");
    assert!(log.contains("\"spans\":[\"request\"]"), "log: {log}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_testing_helpers_run_against_files() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping core.testing helper test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_testing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("corpus")).unwrap();
    fs::write(dir.join("fixture.txt"), "fixture").unwrap();
    fs::write(dir.join("golden.txt"), "gold").unwrap();
    fs::write(dir.join("corpus/a.txt"), "alpha").unwrap();
    fs::write(dir.join("corpus/b.txt"), "beta").unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "testing_helpers",
        r#"
use core.testing as testing

module perf.testing {
    budgets: [Budget.{
        name: "parse",
        scope: .Bench("parse"),
        metric: .BenchTime(.P50),
        provider: .BenchMeasurement("parse"),
        comparison: .AbsoluteFrom("local/testing-helpers"),
        limit: .AtMost(5ms),
        enforcement: .Warn,
    }]
}

fn run() {
    print(testing.fixture("fixture.txt"))
    print(testing.golden("golden.txt", "gold"))
    print(testing.snap("case", "snap"))
    print(testing.corpus("corpus").len())
    print(testing.temp_dir("case").len() > 0)
    clock :: testing.fake_clock(99)
    rng := testing.fake_rng(5)
    print(clock.now())
    print(rng.int(1, 4) >= 1)
}

#Bench("parse") {}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "core.testing helpers failed: {stderr}");
    assert_eq!(
        stdout,
        "fixture\ntrue\ntrue\n2\ntrue\n99\ntrue\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("__snapshots__/case.snap")).unwrap(),
        "snap"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deadline_context_exceed_reports_e3003() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping deadline runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_deadline_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, _stdout, stderr) = build_and_run(
        &dir,
        "deadline_exceeded",
        r#"
use core.time as time

fn run() {
    #Context(deadline: time.now()) {
        time.sleep(5)
    }
}

"#,
        &[],
        None,
    );
    assert_eq!(code, 70, "deadline exceed should stop with runtime code 70");
    assert!(
        stderr.contains("Error [E3003]"),
        "deadline exceed should report E3003, got: {stderr:?}"
    );
    assert!(
        stderr.contains("E3003"),
        "deadline exceed should carry code E3003, got: {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn process_wait_observes_inherited_deadline() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping process deadline runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "jet_corelib_process_deadline_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let sleeper = dir.join("sleep.sh");
    write_executable(&sleeper, "#!/bin/sh\nsleep 2\n");
    let source = format!(
        r#"
use core.process as process
use core.time as time

fn run() {{
    child :: process.cmd(["{sleeper}"]).spawn() ?? panic("spawn failed")
    #Context(deadline: time.now() + 20) {{
        child.wait() ?? panic("wait failed")
    }}
}}
"#,
        sleeper = jet_string_path(&sleeper)
    );
    let (code, _stdout, stderr) =
        build_and_run(&dir, "process_wait_deadline", &source, &[], None);
    assert_eq!(code, 70, "process wait deadline should stop with runtime code 70");
    assert!(
        stderr.contains("Error [E3003]") && stderr.contains("process wait"),
        "process wait should report its compiler-owned deadline boundary: {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// SL9 / R10: importing every core module without calling it must not bloat the binary.
#[test]
fn importing_all_core_modules_without_calls_stays_hello_world_sized() {
    let jet = jet_bin();
    let have_rustc = common::have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping core use size test (need jet + rustc)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("jet_corelib_size_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();

    fs::write(
        dir.join("hello.jet"),
        "fn run() {\n    print(\"hello, world\");\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("core_import_only.jet"),
        r#"
use core.files as fs
use core.io as io
use core.env as env
use core.process as process
use core.math as math
use core.random as random
use core.time as time
use core.encoding.json as json

fn run() {
    print("ok")
}
"#,
    )
    .unwrap();

    let hello = Command::new(&jet)
        .args(["build", "--small", "hello.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(hello.status.success(), "hello build failed");
    let imports = Command::new(&jet)
        .args(["build", "--small", "core_import_only.jet"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(imports.status.success(), "import-only build failed");

    let hello_size = fs::metadata(dir.join("build/hello")).unwrap().len();
    let import_size = fs::metadata(dir.join("build/core_import_only"))
        .unwrap()
        .len();
    assert!(
        import_size <= hello_size.saturating_add(4096),
        "import-only binary ({import_size} bytes) should stay within 4 KiB of hello ({hello_size} bytes)"
    );

    let _ = fs::remove_dir_all(&dir);
}

// D-JSON3=B: lenient decode (core.encoding.json.decode) surfaces coercions via log lines.
// Probes: (a) string→number coercion line + plain value; (b) clean JSON = no log lines;
// (c) multiple coercions = one line each.
#[test]
fn json_decode_lenient_surfaces_coercions() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping json_decode_lenient_surfaces_coercions (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_decode_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Probe (a): string→number coercion appears in stderr; value is usable in arithmetic.
    let (code_a, stdout_a, stderr_a) = build_and_run(
        &dir,
        "json_coerce_a",
        r#"
use core.encoding.json as json
fn run() {
    data :: json.decode("{{\"port\":\"8080\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n + 1)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_a, 0, "probe (a) failed: {stderr_a}");
    assert_eq!(
        stdout_a, "8081\n",
        "probe (a): decoded value should be plain number + 1"
    );
    assert!(
        stderr_a.contains("json coerce")
            && stderr_a.contains("port")
            && stderr_a.contains("number"),
        "probe (a): coercion log line missing or malformed; got: {stderr_a}"
    );

    // Probe (b): clean JSON (no string values that look like numbers/bools) → no coercion lines.
    let (code_b, stdout_b, stderr_b) = build_and_run(
        &dir,
        "json_coerce_b",
        r#"
use core.encoding.json as json
fn run() {
    data :: json.decode("{{\"port\":8080,\"name\":\"api\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_b, 0, "probe (b) failed: {stderr_b}");
    assert_eq!(stdout_b, "8080\n", "probe (b): value should be 8080");
    assert!(
        !stderr_b.contains("json coerce"),
        "probe (b): spurious coercion line emitted for clean JSON; got: {stderr_b}"
    );

    // Probe (c): multiple coercions → one log line each.
    let (code_c, stdout_c, stderr_c) = build_and_run(
        &dir,
        "json_coerce_c",
        r#"
use core.encoding.json as json
fn run() {
    data :: json.decode("{{\"port\":\"8080\",\"enabled\":\"true\"}}") ?? panic("bad json")
    if data == Object(m) {
        if m["port"] == Int(n) {
            print(n)
        }
        if m["enabled"] == Bool(b) {
            print(b)
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_c, 0, "probe (c) failed: {stderr_c}");
    assert_eq!(
        stdout_c, "8080\ntrue\n",
        "probe (c): both coerced values should come back plain"
    );
    let coerce_lines: Vec<&str> = stderr_c
        .lines()
        .filter(|l| l.contains("json coerce"))
        .collect();
    assert_eq!(
        coerce_lines.len(),
        2,
        "probe (c): expected 2 coercion lines, got {}; stderr: {stderr_c}",
        coerce_lines.len()
    );
    // Each line names its field.
    assert!(
        coerce_lines.iter().any(|l| l.contains("port")),
        "probe (c): no coercion line for 'port'"
    );
    assert!(
        coerce_lines.iter().any(|l| l.contains("enabled")),
        "probe (c): no coercion line for 'enabled'"
    );

    let _ = fs::remove_dir_all(&dir);
}

// D-PARSE-1: the user-facing JSON parser is full RFC 8259 — exponents,
// `\uXXXX` (with surrogate pairs), every escape — and rejects invalid input
// (bad escapes, raw control chars) with a clear line/message.
#[test]
fn json_parser_is_rfc8259_complete() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping json_parser_is_rfc8259_complete (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_json_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Probe (a): exponent number, BMP `\u` escape, a surrogate pair, and a `\t`
    // escape — all parsed, then re-serialized (keys sort, `\t` re-escaped).
    let (code_a, stdout_a, stderr_a) = build_and_run(
        &dir,
        "json_full_a",
        r#"
use core.encoding.json as json
fn run() {
    raw :: "{{\"big\":1.5e3,\"acc\":\"caf\\u00e9\",\"grin\":\"\\uD83D\\uDE00\",\"tab\":\"a\\tb\"}}"
    data :: json.parse(raw) ?? panic("bad json")
    print(json.to_string(data))
}
"#,
        &[],
        None,
    );
    assert_eq!(code_a, 0, "probe (a) failed: {stderr_a}");
    // D-ENC-DYN1=A+: `json.parse` yields the `Data` value; an integral-valued number
    // (`1.5e3` == 1500) collapses to `Int`, so it re-serializes as `1500` (not `1500.0`).
    assert_eq!(
        stdout_a, "{\"acc\":\"café\",\"big\":1500,\"grin\":\"😀\",\"tab\":\"a\\tb\"}\n",
        "probe (a): full parse + re-serialize"
    );

    // Probe (b): an invalid escape is rejected with a clear message.
    let (code_b, stdout_b, stderr_b) = build_and_run(
        &dir,
        "json_full_b",
        r#"
use core.encoding.json as json
fn run() {
    if json.parse("{{\"x\":\"a\\qb\"}}") == {
        .Ok(_) -> { print("OK") }
        .Err(e) -> { print("ERR: {e.message}") }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code_b, 0, "probe (b) failed: {stderr_b}");
    assert_eq!(
        stdout_b, "ERR: invalid escape in string\n",
        "probe (b): bad escape rejected"
    );

    // Probe (c): a raw control character (literal tab) inside a string is rejected.
    let (code_c, stdout_c, stderr_c) = build_and_run(
        &dir,
        "json_full_c",
        "
use core.encoding.json as json
fn run() {
    if json.parse(\"{{\\\"x\\\":\\\"a\tb\\\"}}\") == {
        .Ok(_) -> { print(\"OK\") }
        .Err(e) -> { print(\"ERR: {e.message}\") }
    }
}
",
        &[],
        None,
    );
    assert_eq!(code_c, 0, "probe (c) failed: {stderr_c}");
    assert_eq!(
        stdout_c, "ERR: control character in string\n",
        "probe (c): raw control char rejected"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn channel_stress_1000_messages() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping channel stress test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_channel_stress_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "channel_stress",
        r#"
use core.tasks as tasks

fn run() {
(sender, ch) : tasks.channel<Int>()
    producer :: tasks.spawn(() => {
        loop i, 1..1000 {
            sender.send(i)
        }
    })
    producer.join()
    total: Int = 0
    loop i, 1..1000 {
        total = total + (ch.receive() ?? panic("channel closed"))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "channel stress failed: {stderr}");
    assert_eq!(stdout, "500500\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scheduler_spawn_1000_tasks() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping scheduler spawn test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_spawn_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn",
        r#"
use core.tasks as tasks

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..1000 {
        dup :: ~sender
        tasks.spawn(() => {
            dup.send(1)
        })
    }
    total := 0
    loop i, 1..1000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "scheduler spawn stress failed: {stderr}");
    assert_eq!(stdout, "1000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scheduler_spawn_10000_tasks() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping 10k scheduler spawn test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_10k_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn_10k",
        r#"
use core.tasks as tasks

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..10000 {
        dup :: ~sender
        tasks.spawn(() => {
            dup.send(1)
        })
    }
    total := 0
    loop i, 1..10000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "10k scheduler spawn failed: {stderr}");
    assert_eq!(stdout, "10000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "local 100k parked-task stress; run with --ignored"]
fn scheduler_spawn_100000_tasks_bench() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping 100k scheduler bench (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_scheduler_100k_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "scheduler_spawn_100k",
        r#"
use core.tasks as tasks

fn run() {
(sender, ch) :: tasks.channel<Int>()
    loop i, 1..100000 {
        dup :: ~sender
        tasks.spawn(() => {
            dup.send(1)
        })
    }
    total := 0
    loop i, 1..100000 {
        total = (total + (ch.receive() ?? panic("channel closed")))
    }
    print(total)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "100k scheduler bench failed: {stderr}");
    assert_eq!(stdout, "100000\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn race_cancels_losing_task() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping race cancel test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_race_cancel_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "race_cancel",
        r#"
use core.tasks as tasks
use core.time as time

fn fast_nine() => Int {
    return 9
}

fn slow_one() => Int {
    time.sleep(300)
    return 1
}

fn run() {
    taskgroup g {
        slow :: g.task => slow_one()
        fast :: g.task => fast_nine()
        winner :: g.race([slow, fast])
        print(winner)
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "race cancel test failed: {stderr}");
    assert_eq!(stdout, "9\n");
    let _ = fs::remove_dir_all(&dir);
}

/// c45 drift-guard: `core_module_items` in Sema/CheckerCoreLib must cover
/// every module in `Loader::KNOWN_CORE_MODULES` (and no extras).
///
/// `core_module_items` is `pub(crate)` so we can't call it directly from here.
/// Instead we parse the source file and extract the string literals used as
/// match arm heads — the same technique used in tests/decisions.rs for
/// Source/Syntax.rs. This breaks if the match arm format changes, which is
/// exactly the right tripwire: a format change must be mirrored here.
#[test]
fn core_module_items_covers_known_core_modules() {
    let src = fs::read_to_string("crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs")
        .expect("CheckerCoreLib/module_items.rs must exist");

    // Extract the `core_module_items` function body.
    let fn_start = src
        .find("fn core_module_items(")
        .expect("core_module_items function not found in CheckerCoreLib/module_items.rs");
    // Find the closing `}` at top-level indent (just after the last arm).
    let fn_body = &src[fn_start..];
    // Collect ALL string literals from match arm heads (handles `"a" | "b" => &[` form too).
    let mut items_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // `core.lang` is generated from the marker registry and returns before the
    // static match table, so it has no ordinary arm to extract.
    if fn_body.contains("if module == \"core.lang\"") {
        items_keys.insert("core.lang".to_string());
    }
    for line in fn_body.lines() {
        let trimmed = line.trim();
        // A match arm head: `"core.files" => &[` or `"core.log" => &[`
        if trimmed.starts_with('"') && trimmed.contains("=>") {
            let arm_head = trimmed.split("=>").next().unwrap_or("");
            let mut rest = arm_head;
            while let Some(start) = rest.find('"') {
                rest = &rest[start + 1..];
                if let Some(end) = rest.find('"') {
                    items_keys.insert(rest[..end].to_string());
                    rest = &rest[end + 1..];
                } else {
                    break;
                }
            }
        }
        // Stop when we reach the wildcard arm or the closing brace of the function.
        if trimmed == "_ => &[]," || trimmed == "_ => &[]" {
            break;
        }
    }

    // D-CORENS1 / D-CORENS-CANON1: every Core module keeps its canonical
    // `core.*` key through the checker tables. No internal `jet.*` rewrite is
    // allowed to hide a missing or extra module arm.
    let known: std::collections::BTreeSet<String> = jet::Loader::KNOWN_CORE_MODULES
        .iter()
        .map(|s| s.to_string())
        .collect();

    let missing_from_items: Vec<&String> =
        known.iter().filter(|m| !items_keys.contains(*m)).collect();
    let extra_in_items: Vec<&String> = items_keys.iter().filter(|m| !known.contains(*m)).collect();

    assert!(
        missing_from_items.is_empty(),
        "core_module_items is missing arms for modules in KNOWN_CORE_MODULES: {:?}\n\
         Add a match arm in Source/Sema/CheckerCoreLib.rs for each.",
        missing_from_items
    );
    assert!(
        extra_in_items.is_empty(),
        "core_module_items has arms for modules NOT in KNOWN_CORE_MODULES: {:?}\n\
         Either add to KNOWN_CORE_MODULES in Source/Loader.rs or remove the arm.",
        extra_in_items
    );
}

#[test]
fn compiler_sources_reject_retired_jet_ring_keys() {
    // D-CORENS-CANON1: keep the registry guard broader than one table. A new
    // quoted `jet.<ring>` dispatch key in any compiler source must fail this
    // test instead of silently restoring a second internal namespace.
    let roots = [
        "Source",
        "crates/jet-foundation/src",
        "crates/jet-driver/src",
        "crates/jet-sema/src",
        "crates/jet-codegen/src",
        "crates/jet-comptime/src",
        "crates/jet-jit/src",
        "crates/jet-repl/src",
    ];
    let retired = [
        "\"jet.log\"",
        "\"jet.crypto\"",
        "\"jet.http\"",
        "\"jet.regex\"",
        "\"jet.reactive\"",
        "\"jet.db\"",
        "\"jet.plugin\"",
        "\"jet.time\"",
    ];
    let mut pending = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    while let Some(path) = pending.pop() {
        let metadata = fs::metadata(&path).unwrap_or_else(|error| {
            panic!("failed to inspect compiler source {}: {error}", path.display())
        });
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).unwrap_or_else(|error| {
                panic!("failed to read compiler source {}: {error}", path.display())
            }) {
                pending.push(entry.unwrap().path());
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read compiler source {}: {error}", path.display())
        });
        for &key in &retired {
            assert!(
                !source.contains(key),
                "retired internal module key {key} found in {}",
                path.display()
            );
        }
    }
}

#[test]
fn core_reference_lists_every_built_core_module() {
    let docs = fs::read_to_string("docs/reference/core-library.md")
        .expect("core library reference must exist");
    let missing: Vec<&str> = jet::Loader::KNOWN_CORE_MODULES
        .iter()
        .copied()
        .filter(|module| *module != "core")
        .filter(|module| !docs.contains(&format!("`{module}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/reference/core-library.md must list every built Core module from KNOWN_CORE_MODULES: {:?}",
        missing
    );
}

#[test]
fn jet_raylib_namespace_is_not_a_core_module_alias() {
    assert!(jet::Syntax::is_known_core_module("core.raylib"));
    assert!(!jet::Syntax::is_known_core_module("jet.raylib"));

    let src = r#"
use jet.raylib as rl

fn run() {
    print("nope")
}
"#;
    let diags = jet::compile(src).expect_err("jet.raylib must be rejected");
    assert!(
        diags.iter().any(|d| d.code == "E0341"),
        "expected E0341 for retired namespace, got: {:?}",
        diags.iter().map(|d| d.code.to_string()).collect::<Vec<_>>()
    );
}

/// c136 / D-SERDE9-12: generic `#[Codable]` is first-class. The derive injects
/// `T: Encode`/`T: Decode` on exactly the wire-reaching params (D-SERDE9/10); a
/// phantom/skip-only param gets no serde bound (it still gets structural Clone).
/// E2413 is retired (D-SERDE12).
#[test]
fn generic_codable_injects_wire_param_bounds() {
    let out = compile_temp(
        "generic_serde.jet",
        r#"
use core.encoding.json as json

#Codable
struct Wrap<T> {
    value: T
}

#Codable
struct Tagged<K> {
    raw: Int
    #Skip marker: K?
}

fn run() {
    print("x")
}
"#,
    );
    let rs = &out.rust;
    // D-SERDE9: the wire-reaching param T carries `user_Encode`/`user_Decode`.
    assert!(
        rs.contains("impl<T: user_Encode") && rs.contains("user_Encode for user_Wrap<T>"),
        "Wrap's Encode impl must bound T: user_Encode\n{rs}"
    );
    assert!(
        rs.contains("impl<T: user_Decode") && rs.contains("user_Decode for user_Wrap<T>"),
        "Wrap's Decode impl must bound T: user_Decode\n{rs}"
    );
    // D-SERDE10: the phantom param K gets NO Encode/Decode bound (only Clone).
    // (D-MEM1 S6: struct renamed `Id<K>` -> `Tagged<K>` — `Id<T>` is now the
    // reserved `Pool<T>` handle type.)
    assert!(
        rs.contains("impl<K: Clone> user_Encode for user_Tagged<K>"),
        "Tagged's Encode impl must NOT bound K with user_Encode (phantom param)\n{rs}"
    );
    assert!(
        rs.contains("impl<K: Clone> user_Decode for user_Tagged<K>"),
        "Tagged's Decode impl must NOT bound K with user_Decode (phantom param)\n{rs}"
    );
    assert!(
        !rs.contains("K: user_Encode") && !rs.contains("K: user_Decode"),
        "phantom param K must never get a serde bound\n{rs}"
    );
}

/// c136: a generic `#[Codable]` value round-trips through json encode/decode, and
/// a phantom-param type serializes regardless of its phantom argument (D-SERDE10).
#[test]
fn generic_codable_round_trips() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping generic serde round-trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_gserde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, _stderr) = build_and_run(
        &dir,
        "gserde",
        r#"
use core.encoding.json as json

#Codable
struct Wrap<T> {
    value: T
}

#Codable
struct Tagged<K> {
    raw: Int
    #Skip marker: K?
}

fn run() {
    wi :: Wrap<Int>.{ value: 7 }
    print(json.to_string(wi))
    back :: json.decode<Wrap<Int>>("{{\"value\":42}}") ?? panic("bad")
    print(back.value)
    id :: Tagged<Wrap<Int>>.{ raw: 9, marker: None }
    print(json.to_string(id))
    rid :: json.decode<Tagged<Wrap<Int>>>("{{\"raw\":3}}") ?? panic("bad id")
    print(rid.raw)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "generic serde program should run cleanly");
    assert_eq!(stdout, "{\"value\":7}\n42\n{\"raw\":9}\n3\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full TOML adapter (D-ENC-DYN1=A+) ──────────────────────────────────
// Nested `[table]`s, arrays-of-tables, dotted keys, and typed scalars decode into
// nested `#[Codable]` structs, and the rich tree round-trips through `to_string`.
#[test]
fn toml_full_nested_decode_and_round_trip() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping toml_full_nested_decode_and_round_trip (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_toml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode into nested structs + array-of-tables.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "toml_typed",
        r#"
use core.encoding.toml as toml
#Codable
struct Server { host: String  port: Int }
#Codable
struct Config { title: String  server: Server  ports: [Int] }
fn run() {
    raw :: "title = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    cfg :: toml.decode<Config>(raw) ?? panic("bad toml")
    print(cfg.title)
    print(cfg.server.host)
    print(cfg.server.port)
    print(cfg.ports.len())
    print(toml.to_string(cfg))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "toml typed decode failed: {stderr}");
    assert_eq!(
        stdout,
        "jet\ndb.local\n5432\n2\ntitle = \"jet\"\nports = [80, 443]\n\n[server]\nhost = \"db.local\"\nport = 5432\n"
    );

    // Dynamic parse → rich tree → round-trip identity for a nested document.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "toml_dyn",
        r#"
use core.encoding.toml as toml
fn run() {
    raw :: "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n"
    d :: toml.parse(raw) ?? panic("bad")
    print(toml.to_string(d))
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "toml dynamic parse failed: {stderr2}");
    assert_eq!(stdout2, "name = \"a\"\n\n[db]\nhost = \"h\"\nport = 1\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── card #131 S1-bridge: hand-written `impl T.Encode` / `impl T.Decode` (D-SERDE2) ──
// A hand codec passes sema and MUST produce Rust rustc accepts (I2). The Jet-facing
// verbs `encode`/`decode` bridge internally to the Rust `user_Encode`/`user_Decode`
// traits' `jet_encode(&self) -> DataTree` / `jet_decode(&DataTree) -> Result<Self, …>`.
// The impl uses a custom wire key (`"email"`, not the field name `addr`) so the round
// trip can only succeed through the HAND methods, never a derive.
#[test]
fn hand_written_encode_decode_round_trips() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping hand_written_encode_decode_round_trips (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_hand_codec_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "hand_codec",
        r#"
use core.encoding.json as json

struct Email { addr: String }

impl Email.Encode {
    fn encode(self) => DataTree {
        m :: [String: DataTree].{ "email": DataTree.Text(~self.addr) }
        return DataTree.Object(m)
    }
}

impl Email.Decode {
    fn decode(tree: DataTree) => Email ? [FieldError] {
        f := tree.field("email") ?? DataTree.Text("")
        s := f.text() ?? ""
        return .Ok(Email.{addr: s})
    }
}

fn run() {
    e := Email.{addr: "a@b.com"}
    s := json.to_string(e)
    print(s)
    back := json.decode<Email>(s) ?? panic("decode failed")
    print(back.addr)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hand codec round trip failed: {stderr}");
    // Custom wire key proves the hand `encode` ran; `back.addr` proves hand `decode` ran.
    assert_eq!(stdout, "{\"email\":\"a@b.com\"}\na@b.com\n");
    let _ = fs::remove_dir_all(&dir);
}

/// card #131: `DataTree.decode<T>()` dispatches primitive, container,
/// generated, and hand-written Decode implementations through one spelling.
#[test]
fn datatree_decode_dispatches_all_decode_impl_kinds() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_datatree_decode_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
#Codable
struct Point { x: Int }
struct Email { addr: String }
impl Email.Decode {
    fn decode(tree: DataTree) => Email ? [FieldError] {
        value := tree.field("address") ?? DataTree.Text("")
        return .Ok(Email.{ addr: value.text() ?? "" })
    }
}

fn run() {
    i_tree := DataTree.Int(41)
    xs_tree := DataTree.Array([DataTree.Int(1), DataTree.Int(2)])
    p_tree := DataTree.Object(["x": DataTree.Int(7)])
    e_tree := DataTree.Object(["address": DataTree.Text("a@b")])
    i := i_tree.decode<Int>() ?? panic("primitive")
    xs := xs_tree.decode<[Int]>() ?? panic("list")
    p := p_tree.decode<Point>() ?? panic("derive")
    e := e_tree.decode<Email>() ?? panic("hand")
    print(i + xs[1] + p.x)
    print(e.addr)
}
"#;
    let out = compile_temp("datatree_decode.jet", src);
    assert!(out.rust.contains("<i64 as user_Decode>::jet_decode"));
    assert!(out.rust.contains("<user_Point as user_Decode>::jet_decode"));
    assert!(out.rust.contains("<user_Email as user_Decode>::jet_decode"));
    let (code, stdout, stderr) = build_and_run(&dir, "datatree_decode", src, &[], None);
    assert_eq!(code, 0, "DataTree.decode dispatch failed: {stderr}");
    assert_eq!(stdout, "50\na@b\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generated_enum_codecs_reenter_jet_pipeline() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_enum_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json
#Codable
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}
fn run() {
    a := Event.Idle
    b := Event.Count(3)
    c := Event.Named.{ name: "x", enabled: true }
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
    back := json.decode<Event>("{{\"Count\":7}}") ?? panic("decode")
    if back == .Count(n) { print(n) }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "enum_serde", src, &[], None);
    assert_eq!(code, 0, "generated enum codec failed: {stderr}");
    assert_eq!(stdout, "\"Idle\"\n{\"Count\":3}\n{\"Named\":{\"name\":\"x\",\"enabled\":true}}\n7\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-SERDE7: internal tags apply uniformly to unit, single-payload, and
/// named-payload variants. Exact JSON plus decode proves the AOT contract.
#[test]
fn generated_internal_tagged_enum_round_trips_every_variant_shape() {
    let dir = std::env::temp_dir().join(format!("jet_tagged_enum_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#[Codable, Discriminant("type")]
enum Event {
    Idle
    Count(Int)
    Named(name: String, enabled: Bool)
}

fn run() {
    unit := Event.Idle
    tuple := Event.Count(3)
    named := Event.Named.{ name: "x", enabled: true }
    print(json.to_string(unit))
    print(json.to_string(tuple))
    print(json.to_string(named))
    a := json.decode<Event>("{{\"type\":\"Idle\"}}") ?? panic("unit")
    b := json.decode<Event>("{{\"type\":\"Count\",\"value\":7}}") ?? panic("tuple")
    c := json.decode<Event>("{{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}}") ?? panic("named")
    print(json.to_string(a))
    print(json.to_string(b))
    print(json.to_string(c))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "tagged_enum", src, &[], None);
    assert_eq!(code, 0, "generated internally tagged enum failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":3}\n{\"type\":\"Named\",\"name\":\"x\",\"enabled\":true}\n{\"type\":\"Idle\"}\n{\"type\":\"Count\",\"value\":7}\n{\"type\":\"Named\",\"name\":\"y\",\"enabled\":false}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn builtin_codec_expansion_has_no_ast_transplant_or_rust_fallback() {
    let bundle = include_str!("../crates/jet-sema/src/Sema/Bundle.rs");
    let serde = include_str!("../crates/jet-sema/src/Sema/Registration/Serde.rs");
    let items = include_str!("../crates/jet-codegen/src/Codegen/Items.rs");
    assert!(bundle.contains(
        "super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);"
    ));
    assert!(serde.contains("let (tokens, lex_diags) = crate::Lexer::lex(source);"));
    assert!(serde.contains("crate::Parser::parse(&tokens)"));
    assert!(serde.contains(".Ok(generated) => Some(generated.items)"));
    assert!(serde.contains("Some(Item::Impl(imp))"));
    assert!(serde.contains("imp.is_generated_serde = true"));
    assert!(serde.contains("Some(trigger_span)"));
    assert!(!serde.contains("__JetSerdeCarrier"));
    assert!(!serde.contains("__JetSerdeGenerated"));
    assert!(!serde.contains("trait_impls.extend"));
    assert!(!items.contains("emit_struct_serde"));
    assert!(!items.contains("emit_enum_serde"));
}

/// Card #131: generated struct codecs preserve field-policy behavior while
/// running through ordinary Jet bodies: absent options stay off the wire and
/// computed fields encode through their getter without becoming decode slots.
#[test]
fn generated_struct_codecs_preserve_option_and_computed_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Record {
    base: Int
    note: String?
    doubled: Int => base * 2
}

fn run() {
    value := Record.{ base: 4, note: None }
    print(json.to_string(value))
    back := json.decode<Record>("{{\"base\":5,\"doubled\":999}}") ?? panic("decode")
    print(back.base)
    print(back.doubled)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde", src, &[], None);
    assert_eq!(code, 0, "generated struct codec failed: {stderr}");
    assert_eq!(stdout, "{\"base\":4,\"doubled\":8}\n5\n10\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nested_pattern_subjects_clone_read_self_and_keep_take_self_by_value() {
    fn method_body<'a>(rust: &'a str, name: &str) -> &'a str {
        let tail = rust
            .split_once(&format!("fn {}", jet::AST::mangle(name)))
            .map(|(_, tail)| tail)
            .unwrap_or_else(|| panic!("missing generated method `{name}`"));
        let next_method = tail.find("\n    fn user_");
        let impl_end = tail.find("\n}\n");
        let end = match (next_method, impl_end) {
            (Some(a), Some(b)) => a.min(b),
            (Some(end), None) | (None, Some(end)) => end,
            (None, None) => tail.len(),
        };
        &tail[..end]
    }

    let dir = std::env::temp_dir().join(format!("jet_nested_pattern_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
struct Inner { note: String? }
struct Envelope {
    inner: Inner

    fn borrowed(self) => String {
        if self.inner.note == Val(value) { return value }
        return "none"
    }

    fn owned(^self) => String {
        if self.inner.note == Val(value) { return value }
        return "none"
    }
}
fn owned_local_nested_field_remains_reusable() {
    local := Envelope.{ inner: Inner.{ note: Val("local") } }
    if local.inner.note == Val(value) { print(value) }
    if local.inner.note == Val(value) { print(value) }
}
fn run() {
    borrowed := Envelope.{ inner: Inner.{ note: Val("borrowed") } }
    print(borrowed.borrowed())
    owned := Envelope.{ inner: Inner.{ note: Val("owned") } }
    print(owned.owned())
    owned_local_nested_field_remains_reusable()
}
"#;
    let out = compile_temp("nested_pattern_borrow_provenance.jet", src);
    let borrowed = method_body(&out.rust, "borrowed");
    let owned = method_body(&out.rust, "owned");
    assert!(borrowed.contains(".clone()"), "{borrowed}");
    assert!(!owned.contains(".clone()"), "{owned}");

    let (code, stdout, stderr) = build_and_run(&dir, "nested_pattern", src, &[], None);
    assert_eq!(code, 0, "nested borrowed/take-self proof failed: {stderr}");
    assert_eq!(stdout, "borrowed\nowned\nlocal\nlocal\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Derived objects keep declaration order across renamed fields and optional
/// omission. Ordinary maps retain their independent key-ordering behavior.
#[test]
fn generated_struct_encode_preserves_order_with_rename_and_option() {
    let dir = std::env::temp_dir().join(format!("jet_struct_serde_order_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Encode
struct Wire {
    first: String
    #Rename("wireSecond") second: String
    maybe: String?
    last: Int
}

fn run() {
    absent := Wire.{ first: "a", second: "b", maybe: None, last: 4 }
    present := Wire.{ first: "a", second: "b", maybe: Val("c"), last: 4 }
    arbitrary := [String: Int].{ "z": 1, "a": 2 }
    print(json.to_string(absent))
    print(json.to_string(present))
    print(json.to_string(arbitrary))
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_serde_order", src, &[], None);
    assert_eq!(code, 0, "ordered generated struct codec failed: {stderr}");
    assert_eq!(
        stdout,
        "{\"first\":\"a\",\"wireSecond\":\"b\",\"last\":4}\n{\"first\":\"a\",\"wireSecond\":\"b\",\"maybe\":\"c\",\"last\":4}\n{\"a\":2,\"z\":1}\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: flatten, rename, and decode defaults are behavior of generated
/// Jet codec bodies, not a hidden Rust-only derive path.
#[test]
fn generated_struct_codecs_preserve_flatten_rename_and_default() {
    let dir = std::env::temp_dir().join(format!("jet_struct_markers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Inner { x: Int  y: Bool }

#[Codable, RenameAll(camel)]
struct Outer {
    display_name: String
    #Flatten inner: Inner
    count: Int = 4 + 5
}

fn run() {
    value := Outer.{ display_name: "n", inner: Inner.{ x: 1, y: true }, count: 2 }
    print(json.to_string(value))
    back := json.decode<Outer>("{{\"displayName\":\"m\",\"x\":3,\"y\":false}}") ?? panic("decode")
    print(back.display_name)
    print(back.inner.x)
    print(back.count)
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_markers", src, &[], None);
    assert_eq!(code, 0, "generated marker codec failed: {stderr}");
    assert_eq!(stdout, "{\"count\":2,\"displayName\":\"n\",\"x\":1,\"y\":true}\nm\n3\n9\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131 / D-SERDE8: strict unknown-key rejection is emitted as ordinary
/// Jet control flow and carries the offending wire path plus E2412 reason.
#[test]
fn generated_struct_decode_denies_unknown_fields() {
    let dir = std::env::temp_dir().join(format!("jet_struct_deny_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#[Codable, DenyUnknownFields]
struct Strict { name: String }

fn run() {
    result := json.decode<Strict>("{{\"name\":\"x\",\"extra\":1}}")
    if result == .Err(errors) {
        loop error; errors {
            print(error.path)
            print(error.reason)
        }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_deny", src, &[], None);
    assert_eq!(code, 0, "generated strict codec failed: {stderr}");
    assert_eq!(stdout, "extra\nE2412: unknown field `extra`\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generated_struct_decode_accumulates_nested_errors_and_validation() {
    let dir = std::env::temp_dir().join(format!("jet_struct_decode_errors_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

#Codable
struct Inner { left: Int  right: Bool }

#Codable
struct Outer { inner: Inner  count: Int }

#Codable
struct Account {
    email: String
    age: Int

    validate {
        check(email.contains("@"), at: email, "email")
        check(age >= 18, at: age, "age")
    }
}

fn run() {
    malformed := json.decode<Outer>("{{\"inner\":{{\"left\":\"bad\",\"right\":\"bad\"}},\"count\":\"bad\"}}")
    if malformed == .Err(errors) {
        print(errors.len())
        loop error; errors { print(error.path) }
    }
    invalid := json.decode<Account>("{{\"email\":\"missing-at\",\"age\":12}}")
    if invalid == .Err(errors) {
        print(errors.len())
        loop error; errors { print(error.path) }
    }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "struct_decode_errors", src, &[], None);
    assert_eq!(code, 0, "generated decoder accumulation failed: {stderr}");
    assert_eq!(stdout, "3\ninner.left\ninner.right\ncount\n2\nemail\nage\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #131: built-in Decode fragments live beside their source type, so a
/// consumer can dispatch through an imported type without entry-local aliases.
/// The nested List/Option/Map fields also prove D-SERDE16's public dispatch.
#[test]
fn generated_decode_dispatches_across_module_boundaries() {
    let dir = std::env::temp_dir().join(format!("jet_serde_module_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
#Codable
pub struct Address { pub city: String }

#Codable
pub struct Order {
    pub shipping: Address
    pub quantities: [Int]
    pub coupon: String?
    pub labels: [String: Int]
}

"#;
    let main = r#"
use core.encoding.json as json
use orders

fn run() {
    order := json.decode<orders.Order>("{{\"shipping\":{{\"city\":\"Paris\"}},\"quantities\":[2,3],\"coupon\":null,\"labels\":{{\"fragile\":1}}}}") ?? panic("decode")
    print(json.to_string(order))
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "serde_module",
        "main.jet",
        &[("main.jet", main), ("orders.jet", lib)],
    );
    assert_eq!(code, 0, "cross-module generated decode failed: {stderr}");
    assert_eq!(stdout, "{\"shipping\":{\"city\":\"Paris\"},\"quantities\":[2,3],\"labels\":{\"fragile\":1}}\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-METADERIVE1 orphan law: expansion is legal when either derive provider
/// or target type is entry-local. Both directions must generate usable code.
#[test]
fn user_derive_orphan_rule_allows_either_local_side() {
    let dir = std::env::temp_dir().join(format!("jet_derive_orphan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let lib = r#"
derive T.RemoteLabel {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ fn remote_label(self) => String {{ return \"remote:$name\" }} }}")
}

#LocalLabel
pub struct RemoteType { pub value: Int }

pub fn remote_type_label() => String {
    value := RemoteType.{ value: 2 }
    return value.local_label()
}
"#;
    let main = r#"
use labels

derive T.LocalLabel {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ pub fn local_label(self) => String {{ return \"local:$name\" }} }}")
}

#RemoteLabel
struct LocalType { value: Int }

fn run() {
    local := LocalType.{ value: 1 }
    print(local.remote_label())
    print(labels.remote_type_label())
}
"#;
    let (code, stdout, stderr) = build_and_run_multi(
        &dir,
        "derive_orphan",
        "main.jet",
        &[("main.jet", main), ("labels.jet", lib)],
    );
    assert_eq!(code, 0, "local-orphan derive dispatch failed: {stderr}");
    assert_eq!(stdout, "remote:LocalType\nlocal:RemoteType\n");
    let _ = fs::remove_dir_all(&dir);
}

/// D-LAYOUT-FACTS1=B: the focused fact and full reflection projection share
/// one typed layout model, including typed field selection and explicit
/// provenance for the default, C, and columnar declarations.
#[test]
fn user_derive_layout_fact_matches_reflection_projection() {
    let dir = std::env::temp_dir().join(format!("jet_layout_facts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
derive T.LayoutFacts {
    info :: T.$layout
    reflected :: T.reflect().layout
    selected :: info[.count]
    kind :: info.kind
    target :: info.target
    guarantee :: info.guarantee
    source :: info.source
    reflected_kind :: reflected.kind
    field_name :: selected.name
    name :: T.reflect().name
    emit("impl $name {{ fn layout_facts(self) => String {{ return \"$kind:$target:$guarantee:$source:$reflected_kind:$field_name\" }} }}")
}

#LayoutFacts
struct Packet {
    count: Int
    label: String
}

#Layout(c)
struct CPacket {
    count: U32
    flag: U8

    derive LayoutFacts
}

#Layout(columnar)
struct ColumnPacket {
    count: Int
    label: String

    derive LayoutFacts
}

fn run() {
    packet := Packet.{ count: 2, label: "ok" }
    c_packet := CPacket.{ count: 2, flag: 1 }
    column_packet := ColumnPacket.{ count: 2, label: "ok" }
    print(packet.layout_facts())
    print(c_packet.layout_facts())
    print(column_packet.layout_facts())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "layout_facts", src, &[], None);
    assert_eq!(code, 0, "layout facts derive failed: {stderr}");
    assert_eq!(
        stdout,
        "default:unknown:physical layout unspecified:struct declaration:default:count\n"
            .to_string()
            + "c:unknown:repr(C) declaration:struct declaration:c:count\n"
            + "columnar:unknown:columnar storage declaration:struct declaration:columnar:count\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Card #129 / R11: generated declarations are ordinary Jet items. They must
/// be registered before later generated code (here `#[Codable]`) is checked,
/// and `#[Default(expr)]` must retain its exact compile-time value.
#[test]
fn user_derive_generated_struct_reenters_registration_and_serde() {
    let dir = std::env::temp_dir().join(format!("jet_derive_reentry_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.encoding.json as json

derive T.ConfigSchema {
    emit("""
#Codable
struct GeneratedConfig {{
    ports: [Int] = [80, 443]
}}
""")
}

#ConfigSchema
struct Schema<T> { witness: T }

fn run() {
    config := json.decode<GeneratedConfig>("{{}}") ?? panic("decode")
    print(config.ports)
}
"#;
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "user_derive_generated_struct",
        src,
        &[],
        None,
    );
    assert_eq!(code, 0, "generated struct did not re-enter registration: {stderr}");
    assert_eq!(stdout, "[80, 443]\n");
    let _ = fs::remove_dir_all(&dir);
}

/// Card #129 / D-METADERIVE1: an emitted inherent impl keeps the target's
/// generic identity through sema, TIR, AOT, and default `jet dev`.
#[test]
fn user_derive_generic_impl_runs_in_aot_and_default_dev() {
    let dir = std::env::temp_dir().join(format!("jet_derive_generic_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
derive T.TypeName {
    info :: T.reflect()
    name :: info.name
    param :: info.type_params[0].name
    emit("impl $name {{ fn get_value(self) => $param {{ return ~self.value }} fn type_name(self) => String {{ return \"$name\" }} }}")
}

#TypeName
struct Box<T> { value: T }

fn run() {
    boxed := Box<Int>.{ value: 7 }
    n := boxed.get_value()
    print(n)
    print(boxed.type_name())
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "user_derive_generic", src, &[], None);
    assert_eq!(code, 0, "generic user derive failed in AOT: {stderr}");
    assert_eq!(stdout, "7\nBox\n");

    let file = dir.join("user_derive_generic.jet");
    fs::write(&file, src).unwrap();
    match jet::Interpreter::dev_iteration(file.to_str().unwrap(), false, false) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert_eq!(exit_code, 0, "generic user derive failed in dev: {stderr}");
            assert_eq!(stdout, "7\nBox\n");
        }
        other => panic!("generic user derive did not run in default dev: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

/// R11 also means generated code gets ordinary semantic rejection. A derive
/// cannot smuggle a non-duplicable function value through explicit `copy`.
#[test]
fn user_derive_generated_non_clonable_copy_is_rejected_in_sema() {
    let src = r#"
derive T.CopyCallback {
    info :: T.reflect()
    name :: info.name
    emit("impl $name {{ fn duplicate(self) => fn(Int) => Int {{ return ~self.callback }} }}")
}

#CopyCallback
struct Handler { callback: fn(Int) => Int }

fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("generated function copy must be rejected");
    assert!(
        diags.iter().any(|diag| diag.code == "E0211"),
        "expected generated code to re-enter cloneability checking: {diags:?}"
    );
}

/// #495 / I2: a field read from a bare (`Read`) parameter is still rooted in
/// the borrowed parameter. The explicit `~` required by E0209 must produce
/// owned values for both shallow and nested fields, compile through rustc, and
/// run with the expected data.
#[test]
fn consuming_core_constructor_copies_borrowed_field_explicitly() {
    let dir = std::env::temp_dir().join(format!(
        "jet_core_borrowed_field_copy_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "core_borrowed_field_copy",
        r#"
use core.encoding.json as json

struct Address { text: String }
struct Email { addr: String, nested: Address, items: [Address] }

fn pick() => Int {
    return 0
}

fn encoded(e: Email, i: Int) => String {
    shallow := DataTree.Text(~e.addr)
    nested := DataTree.Text(~e.nested.text)
    indexed := DataTree.Text(~e.items[0].text)
    computed := DataTree.Text(~e.items[i + 1].text)
    called := DataTree.Text(~e.items[pick()].text)
    parenthesized := DataTree.Text(~e.items[-(-i)].text)
    conditional := DataTree.Text(~e.items[if i == 0 { 0 } else { 1 }].text)
    return "{json.to_string(shallow)}|{json.to_string(nested)}|{json.to_string(indexed)}|{json.to_string(computed)}|{json.to_string(called)}|{json.to_string(parenthesized)}|{json.to_string(conditional)}"
}

fn slice_data(xs: [DataTree]) => DataTree {
    return DataTree.Array(xs[0..1])
}

fn run() {
    e := Email.{addr: "a@b.com", nested: Address.{text: "inside"}, items: [Address.{text: "zero"}, Address.{text: "item"}]}
    sliced := slice_data([DataTree.Text("slice0"), DataTree.Text("slice1")])
    print("{encoded(e, 0)}|{json.to_string(sliced)}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "explicit field copy failed to compile/run: {stderr}");
    assert_eq!(
        stdout,
        "\"a@b.com\"|\"inside\"|\"zero\"|\"item\"|\"zero\"|\"zero\"|\"zero\"|[\"slice0\",\"slice1\"]\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ── c152: full YAML adapter (D-ENC-YAML1 = A) ────────────────────────────────
// Block mappings + sequences, flow collections, typed scalars, block scalars,
// comments, document markers, and anchors/aliases.
#[test]
fn yaml_full_nested_decode_and_features() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping yaml_full_nested_decode_and_features (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_yaml_full_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Typed decode of a nested document with a block sequence of mappings.
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "yaml_typed",
        r#"
use core.encoding.yaml as yaml
#Codable
struct Service { name: String  port: Int }
#Codable
struct Config { app: String  services: [Service] }
fn run() {
    raw :: "app: myapp\nservices:\n  - name: web\n    port: 80\n  - name: db\n    port: 5432\n"
    cfg :: yaml.decode<Config>(raw) ?? panic("bad yaml")
    print(cfg.app)
    print(cfg.services.len())
    print(cfg.services[0].name)
    print(cfg.services[1].port)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "yaml typed decode failed: {stderr}");
    assert_eq!(stdout, "myapp\n2\nweb\n5432\n");

    // Advanced features: flow collections, comments, `---`, anchors/aliases, block scalar.
    let (code2, stdout2, stderr2) = build_and_run(
        &dir,
        "yaml_adv",
        r#"
use core.encoding.yaml as yaml
fn run() {
    raw :: "---\n# a config\nflowlist: [1, 2, 3]\nbase: &b\n  host: local\n  port: 80\nuse: *b\nnote: |\n  one\n  two\n"
    d :: yaml.parse(raw) ?? panic("bad yaml")
    if d == Object(top) {
        if top["flowlist"] == Array(xs) {
            print(xs.len())
        }
        if top["use"] == Object(u) {
            print(u.len())
        }
        if top["note"] == Text(s) {
            print(s.contains("one"))
        }
    }
}
"#,
        &[],
        None,
    );
    assert_eq!(code2, 0, "yaml advanced features failed: {stderr2}");
    assert_eq!(stdout2, "3\n2\ntrue\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── D-MIGRATE3=A / D-MIGRATE4=A: decode-time migration transparency ──────────
// `decode_traced<T>` sits beside `decode<T>` on every codec that shares the
// decode machinery. `MigrationStatus.migrated` is false and `.from`/`.steps`
// are empty both for a plain type (no `#PublishedSchema`) and for a
// `#PublishedSchema` type decoding data already shaped like the current
// struct (the "fresh" case). This test covers those non-migrated cases; the
// migrated paths (D-MIGRATE4 runtime chain) are `decode_traced_migration_*`
// below.
#[test]
fn decode_traced_json_plain_and_published_fresh() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_json_plain_and_published_fresh (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_traced_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "decode_traced_json",
        r#"
use core.encoding.json as json

#Codable
struct Point { x: Int  y: Int }

#[PublishedSchema, Codable]
struct UserRecord { id: Int  display_name: String }

migration UserRecord {
    rename name -> display_name
}

fn run() {
    // Plain (non-#PublishedSchema) type: decode_traced still works.
    p :: json.decode_traced<Point>("{{\"x\":1,\"y\":2}}") ?? panic("bad point")
    print(p.value.x)
    print(p.migration.migrated)
    print(p.migration.from)
    print(p.migration.steps.len())

    // #PublishedSchema type, fresh data (matches the current shape exactly):
    // still reports migrated: false — nothing runtime-converted it.
    r :: json.decode_traced<UserRecord>("{{\"id\":1,\"display_name\":\"Ada\"}}") ?? panic("bad user")
    print(r.value.display_name)
    print(r.migration.migrated)

    // decode<T> (untraced) is untouched: same call, no DecodeResult wrapper.
    plain :: json.decode<Point>("{{\"x\":3,\"y\":4}}") ?? panic("bad plain")
    print(plain.x)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "decode_traced json program failed: {stderr}");
    assert_eq!(stdout, "1\nfalse\n\n0\nAda\nfalse\n3\n");
    let _ = fs::remove_dir_all(&dir);
}

// A second codec exercising the same DecodeResult/MigrationStatus plumbing —
// proves the traced method isn't a json-only special case (D-ENC1 shares the
// decode machinery across json/csv/toml/yaml).
#[test]
fn decode_traced_toml_and_csv() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_toml_and_csv (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_decode_traced_toml_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "decode_traced_toml",
        r#"
use core.encoding.toml as toml
use core.encoding.csv as csv

#Codable
struct Config { port: Int }

fn run() {
    r :: toml.decode_traced<Config>("port = 8080\n") ?? panic("bad toml")
    print(r.value.port)
    print(r.migration.migrated)

    cr :: csv.decode_traced<Config>("port\n8080\n9090\n") ?? panic("bad csv")
    print(cr.value.len())
    print(cr.value[0].port)
    print(cr.migration.migrated)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "decode_traced toml/csv program failed: {stderr}");
    assert_eq!(stdout, "8080\nfalse\n2\n8080\nfalse\n");
    let _ = fs::remove_dir_all(&dir);
}

// ── D-MIGRATE4=A: the runtime migration chain ────────────────────────────────
// Decoding a `#PublishedSchema` type tries the current shape first; on
// mismatch it detects which historical shape the data's field-name set
// matches (newest matching version preferred) and walks the migration blocks
// forward, oldest→current. `decode_traced` reports `from` + `steps`
// ("v1->v2" style, one per block applied); plain `decode` applies the same
// chain silently. Data matching no shape keeps the ordinary decode error.
// This covers: a two-block chain (v1→v3: remove + rename + `change … via`),
// the newest-match rule (v2 data walks one step, not two), the silent plain
// `decode`, and garbage still erroring.
#[test]
fn decode_traced_migration_chain() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_migration_chain (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_migrate_chain_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "migrate_chain",
        r#"
use core.encoding.json as json

#Codable
struct Rank { value: Int }

// v1: { legacy_id, name, score: Int }
// v2: { name, score: Int }     (block 1: remove legacy_id)
// v3: { title, score: Rank }   (block 2: rename + change via)
#[PublishedSchema, Codable]
struct Profile {
    title: String
    score: Rank
}

migration Profile {
    remove legacy_id
}

migration Profile {
    rename name => title
    change score: Int => Rank via { (n) => Rank.{ value: n } }
}

fn run() {
    // v1 data walks both steps.
    v1 :: "{{\"legacy_id\": 9, \"name\": \"Ada\", \"score\": 95}}"
    r :: json.decode_traced<Profile>(v1) ?? panic("bad v1")
    print(r.value.title)
    print(r.value.score.value)
    print(r.migration.migrated)
    print(r.migration.from)
    print(r.migration.steps.len())
    print(r.migration.steps[0])
    print(r.migration.steps[1])

    // v2 data matches the newer historical shape — one step, not two.
    v2 :: "{{\"name\": \"Grace\", \"score\": 7}}"
    r2 :: json.decode_traced<Profile>(v2) ?? panic("bad v2")
    print(r2.migration.from)
    print(r2.migration.steps.len())

    // Plain decode applies the same chain silently.
    p :: json.decode<Profile>(v1) ?? panic("bad plain")
    print(p.title)
    print(p.score.value)

    // Data matching no shape keeps the ordinary decode error.
    g :: json.decode<Profile>("{{\"nonsense\": 1}}") ?? Profile.{ title: "rejected", score: Rank.{ value: 0 } }
    print(g.title)
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "migration chain program failed: {stderr}");
    assert_eq!(
        stdout,
        "Ada\n95\ntrue\nv1\n2\nv1->v2\nv2->v3\nv2\n1\nAda\n95\nrejected\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// D-MIGRATE4 across codecs (D-ENC1: one decode machinery): an `add … = default`
// migration fills old records in toml and csv exactly as in json. The csv case
// also proves per-row application (every row of an old-header file migrates,
// the batch-level status reports it once).
#[test]
fn decode_traced_migration_toml_and_csv() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping decode_traced_migration_toml_and_csv (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_migrate_codecs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let (code, stdout, stderr) = build_and_run(
        &dir,
        "migrate_codecs",
        r#"
use core.encoding.toml as toml
use core.encoding.csv as csv

#[PublishedSchema, Codable]
struct Config {
    port: Int
    host: String
}

migration Config {
    add host: String = "localhost"
}

fn run() {
    t :: toml.decode_traced<Config>("port = 8080\n") ?? panic("bad toml")
    print(t.value.host)
    print(t.migration.migrated)
    print(t.migration.from)

    c :: csv.decode_traced<Config>("port\n1\n2\n") ?? panic("bad csv")
    print(c.value.len())
    print(c.value[1].host)
    print(c.migration.migrated)
    print(c.migration.steps[0])
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "migration codec program failed: {stderr}");
    assert_eq!(stdout, "localhost\ntrue\nv1\n2\nlocalhost\ntrue\nv1->v2\n");
    let _ = fs::remove_dir_all(&dir);
}

// D-MIGRATE4 zero cost: a type without migration blocks — published or not —
// gets NO runtime chain code: no step functions, no per-type
// `jet_decode_traced` override. Compile-only (asserts on the generated Rust).
#[test]
fn migration_free_types_emit_no_runtime_chain() {
    let src = r#"
use core.encoding.json as json

#Codable
struct Point { x: Int  y: Int }

#[PublishedSchema, Codable]
struct UserRecord { id: Int  display_name: String }

fn run() {
    p :: json.decode<Point>("{{\"x\":1,\"y\":2}}") ?? panic("bad")
    print(p.x)
    u :: json.decode_traced<UserRecord>("{{\"id\":1,\"display_name\":\"Ada\"}}") ?? panic("bad")
    print(u.value.id)
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_migrate_free_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("migration_free.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !out.rust.contains("jet_migrate_step_"),
        "no step functions may be emitted for migration-free types"
    );
    // The only `jet_decode_traced` definitions are the prelude's (the trait
    // default) — no per-type override in the user section.
    let user_section = out
        .rust
        .split("impl user_Decode for user_")
        .skip(1)
        .collect::<String>();
    assert!(
        !user_section.contains("fn jet_decode_traced"),
        "no per-type jet_decode_traced override may be emitted for migration-free types"
    );
}

/// I9 for the typed text codecs: `decode<T>` and `decode_traced<T>` mean the
/// same thing under the full build and under default `jet run`, which reaches
/// them through the canonical TIR evaluator. One fixture covers every codec
/// that shares the decode machinery, fresh and migrated records, per-row csv
/// migration, and a parse failure's wording.
#[test]
fn typed_codec_decode_matches_between_full_build_and_quick_run() {
    let jet = jet_bin();
    let have_rustc = common::have_rustc();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping typed codec decode tier parity (need jet + rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_typed_decode_tiers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let src = r#"
use core.encoding.json as json
use core.encoding.toml as toml
use core.encoding.yaml as yaml
use core.encoding.csv as csv

#[PublishedSchema, Codable]
struct Config {
    port: Int
    host: String
}

migration Config {
    add host: String = "localhost"
}

#Codable
struct Point { x: Int  y: Int }

#Codable
struct Rank { value: Int }

// Two blocks, so a v1 record walks two steps and a v2 record walks one.
#[PublishedSchema, Codable]
struct Profile {
    title: String
    score: Rank
}

migration Profile {
    remove legacy_id
}

migration Profile {
    rename name => title
    change score: Int => Rank via { (n) => Rank.{ value: n } }
}

fn run() {
    // json, record already in the current shape.
    fresh :: json.decode_traced<Config>("{{\"port\": 1, \"host\": \"a\"}}") ?? panic("bad fresh")
    print("{fresh.value.port} {fresh.value.host} {fresh.migration.migrated} {fresh.migration.from} {fresh.migration.steps.len()}")

    // json, record in the historical shape: the chain fills the added field.
    old :: json.decode_traced<Config>("{{\"port\": 2}}") ?? panic("bad old")
    print("{old.value.port} {old.value.host} {old.migration.migrated} {old.migration.from} {old.migration.steps[0]}")

    // Untraced decode walks the same chain and drops the status.
    plain :: json.decode<Config>("{{\"port\": 3}}") ?? panic("bad plain")
    print("{plain.port} {plain.host}")

    // A type with no migration blocks reports a fresh status.
    p :: json.decode_traced<Point>("{{\"x\": 4, \"y\": 5}}") ?? panic("bad point")
    print("{p.value.x} {p.value.y} {p.migration.migrated}")

    // A record two shapes behind walks both steps; one shape behind walks one.
    far :: json.decode_traced<Profile>("{{\"legacy_id\": 9, \"name\": \"Ada\", \"score\": 95}}") ?? panic("bad v1")
    print("{far.value.title} {far.value.score.value} {far.migration.from} {far.migration.steps.len()} {far.migration.steps[0]} {far.migration.steps[1]}")
    near :: json.decode_traced<Profile>("{{\"name\": \"Grace\", \"score\": 7}}") ?? panic("bad v2")
    print("{near.value.title} {near.migration.from} {near.migration.steps.len()} {near.migration.steps[0]}")

    t :: toml.decode_traced<Config>("port = 6\n") ?? panic("bad toml")
    print("{t.value.port} {t.value.host} {t.migration.migrated} {t.migration.from}")

    y :: yaml.decode<Config>("port: 7\nhost: b\n") ?? panic("bad yaml")
    print("{y.port} {y.host}")

    // csv decodes to a list; every row migrates and the batch reports it once.
    rows :: csv.decode_traced<Config>("port\n8\n9\n") ?? panic("bad csv")
    print("{rows.value.len()} {rows.value[0].port} {rows.value[1].host} {rows.migration.migrated} {rows.migration.steps[0]}")

    // A field that does not fit is an ordinary decode error, not a crash,
    // and a csv row error keeps its `row <n>` path prefix.
    if json.decode<Config>("{{\"port\": \"nope\", \"host\": \"h\"}}") == {
        .Ok(v) -> print("unexpected {v.port}")
        .Err(errs) -> print("err {errs.len()} {errs[0].path} {errs[0].reason}")
    }
    if csv.decode<Config>("port,host\nnope,h\n") == {
        .Ok(v) -> print("unexpected {v.len()}")
        .Err(errs) -> print("row err {errs.len()} {errs[0].path} {errs[0].reason}")
    }
}
"#;

    let (code, compiled, stderr) = build_and_run(&dir, "typed_decode_tiers", src, &[], None);
    assert_eq!(code, 0, "full build failed: {stderr}");
    assert_eq!(
        compiled,
        "1 a false  0\n\
         2 localhost true v1 v1->v2\n\
         3 localhost\n\
         4 5 false\n\
         Ada 95 v1 2 v1->v2 v2->v3\n\
         Grace v2 1 v2->v3\n\
         6 localhost true v1\n\
         7 b\n\
         2 8 localhost true v1->v2\n\
         err 1 port expected Int, found text \"nope\"\n\
         row err 1 row 1.port expected Int, found text \"nope\"\n"
    );

    // `jet run` wants the source under its own extension; `build_and_run`
    // names its fixture after the crate it emits.
    let quick_path = dir.join("typed_decode_tiers.jet");
    fs::write(&quick_path, src).unwrap();
    let quick = Command::new(&jet)
        .arg("run")
        .arg(&quick_path)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        quick.status.success(),
        "quick run failed:\n{}",
        String::from_utf8_lossy(&quick.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&quick.stdout),
        compiled,
        "typed codec decode must mean the same thing on both tiers (I9)"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn perf_static_api_lowers_to_core_helpers() {
    let out = compile_temp(
        "perf_static.jet",
        r#"
fn run() => () ? {
    print(Perf.default_fidelity())
    Perf.override_fidelity(0.25)?
    print(Perf.fidelity())
    Perf.reset_fidelity()
}
"#,
    );
    assert!(out.rust.contains("jet_perf_default_fidelity()"));
    assert!(out.rust.contains("jet_perf_override_fidelity(0.25"));
    assert!(out.rust.contains("jet_perf_fidelity()"));
    assert!(out.rust.contains("jet_perf_reset_fidelity()"));
}

#[test]
fn perf_set_fidelity_alias_is_not_exported() {
    let src = r#"
use core.perf as perf

fn run() => () ? {
    perf.set_fidelity(0.25)?
}
"#;
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_alias_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("perf_alias.jet");
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let diags = jet::compile_with_path(src, &shown).expect_err("set_fidelity alias must not exist");
    let rendered = jet::render_diagnostics(&shown, src, &diags);
    assert!(
        rendered.contains("set_fidelity"),
        "diagnostic should name retired alias, got:\n{rendered}"
    );
    assert!(
        rendered.contains("has no item"),
        "diagnostic should reject retired alias, got:\n{rendered}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn perf_override_is_range_checked_and_resettable() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping perf runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_perf_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "perf_runtime",
        r#"
use core.perf as perf

fn run() => () ? {
    print(perf.default_fidelity())
    perf.override_fidelity(0.25)?
    print(perf.fidelity())
    perf.reset_fidelity()
    print(perf.fidelity())
    perf.override_fidelity(1.25)?
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 1, "out-of-range override should fail");
    assert_eq!(stdout, "1.0\n0.25\n1.0\n");
    assert!(
        stderr.contains("core.perf.Perf.override_fidelity needs 0.0 through 1.0"),
        "range error should be in Jet runtime terms, got {stderr:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn option_zip_and_lift2_combinators() {
    // D-HOLE1: `.zip`/`Option.lift2` — both present -> a present result; either
    // absent -> `None`. No general "hole" type; these are plain library combinators
    // on `T?`.
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping option combinator test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_option_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "option_combinators",
        r#"
fn run() {
    both_a :: Val(2.0)
    both_b :: Val(5.0)
    print(both_a.zip(both_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_a, both_b))

    a_only :: Val(2.0)
    b_missing ::  None 
    print(a_only.zip(b_missing).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, a_only, b_missing))

    both_missing_a ::  None 
    both_missing_b ::  None 
    print(both_missing_a.zip(both_missing_b).map((pair) => pair.a * pair.b))
    print(Option.lift2((x, y) => x * y, both_missing_a, both_missing_b))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "option combinator fixture failed: {stderr}");
    assert_eq!(
        stdout, "10.0\n10.0\nnull\nnull\nnull\nnull\n",
        "unexpected option combinator output: {stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn event_scope_subscribe_once_priority_and_hook_run() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping event runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_runtime",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.with_policy<Int>(event.policy_sync())
    sub :: ev.on(scope, (n) => { print("low {n}") })
    ev.on_priority(scope, 10, (n) => { print("high {n}") })
    ev.once(scope, (n) => { print("once {n}") })
    print(ev.emit(1).summary())
    sub.unsubscribe()
    print(ev.emit(2).summary())
    print(scope.active_count())

    hook :: event.hook<Int, String>("base")
    hook.on(scope, (n) => "seen {n}")
    print(hook.run(7, "fallback"))
    scope.cancel()
    print(hook.run(8, "fallback"))
}

"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "high 1\nlow 1\nonce 1\nevent delivered=3 queued=0 dropped=0\nhigh 2\nevent delivered=1 queued=0 dropped=0\n1\nseen 7\nfallback\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_scheduler_dispatch_and_invalid_capacity() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_scheduler",
        r#"
use core.event as event
use core.tasks as tasks

enum LocalState { Closed }

fn run() {
    local :: LocalState.Closed
    print("local={local == .Closed}")
    bad :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 0, overflow: .Block }, .Collect)
    if bad == {
        .Ok(_) -> print("bad accepted")
        .Err(_) -> print("invalid capacity")
    }
    scope :: event.scope()
    ev :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (started_tx, started_rx) :: tasks.channel<Int>()
    (release_tx, release_rx) :: tasks.channel<Int>()
    ev.on(scope, (n: Int) => {
        started_tx.send(~n)
        released :: release_rx.receive() ?? panic("release")
    })
    first :: ev.emit_async(1)
    started_first :: started_rx.receive() ?? panic("started")
    second :: ev.emit_async(2)
    third :: ev.emit_async(3)
    print("queued={ev.queued_count()} running={ev.running_count()} blocked={ev.blocked_count()}")
    ev.close()
    release_tx.send(1)
    started_second :: started_rx.receive() ?? panic("second started")
    release_tx.send(2)
    first_report :: first.join()
    second_report :: second.join()
    third_report :: third.join()
    print("delivered={first_report.delivered_handlers() + second_report.delivered_handlers()}")
    print("delivered state={first_report.state() == .Delivered}")
    print("closed={!third_report.accepted() && third_report.state() == .Closed}")
    print(third_report.trace().summary())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event runtime failed: {stderr}");
    assert_eq!(stdout, "local=true\ninvalid capacity\nqueued=1 running=1 blocked=1\ndelivered=2\ndelivered state=true\nclosed=true\npending -> terminal:Closed\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_overflow_and_failure_policies() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_policies_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_policies",
        r#"
use core.event as event
use core.tasks as tasks

fn panic_log_handler(n: Int) => () ? String {
    panic("log boom")
    return .Err("unreachable")
}

fn panic_ignore_handler(n: Int) => () ? String {
    panic("ignore boom")
    return .Err("unreachable")
}

fn run() {
    newest_scope :: event.scope()
    newest :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .DropNewest }, .Collect) ?? panic("policy")
    (newest_started_tx, newest_started_rx) :: tasks.channel<Int>()
    (newest_release_tx, newest_release_rx) :: tasks.channel<Int>()
    newest.on(newest_scope, (n: Int) => {
        newest_started_tx.send(~n)
        released_newest :: newest_release_rx.receive() ?? panic("release")
    })
    newest_first :: newest.emit_async(1)
    newest_started_first :: newest_started_rx.receive() ?? panic("started")
    newest_second :: newest.emit_async(2)
    newest_third :: newest.emit_async(3)
    newest_report :: newest_third.join()
    print("newest={!newest_report.accepted() && newest_report.state() == .DroppedNewest}")
    newest_release_tx.send(1)
    newest_started_second :: newest_started_rx.receive() ?? panic("second")
    newest_release_tx.send(2)
    newest_first.join()
    newest_second.join()

    oldest_scope :: event.scope()
    oldest :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .DropOldest }, .Collect) ?? panic("policy")
    (oldest_started_tx, oldest_started_rx) :: tasks.channel<Int>()
    (oldest_release_tx, oldest_release_rx) :: tasks.channel<Int>()
    oldest.on(oldest_scope, (n: Int) => {
        oldest_started_tx.send(~n)
        released_oldest :: oldest_release_rx.receive() ?? panic("release")
    })
    oldest_first :: oldest.emit_async(1)
    oldest_started_first :: oldest_started_rx.receive() ?? panic("started")
    oldest_evicted :: oldest.emit_async(2)
    oldest_third :: oldest.emit_async(3)
    oldest_report :: oldest_evicted.join()
    print("oldest={oldest_report.accepted() && oldest_report.state() == .DroppedOldest}")
    oldest_release_tx.send(1)
    oldest_started_third :: oldest_started_rx.receive() ?? panic("third")
    oldest_release_tx.send(3)
    oldest_first.join()
    oldest_third.join()

    once_scope :: event.scope()
    once_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 2, overflow: .Block }, .Collect) ?? panic("policy")
    (once_started_tx, once_started_rx) :: tasks.channel<Int>()
    (once_release_tx, once_release_rx) :: tasks.channel<Int>()
    once_event.on_priority(once_scope, 10, (n: Int) => {
        if n == 1 {
            once_started_tx.send(~n)
            released_once :: once_release_rx.receive() ?? panic("release")
        }
    })
    once_event.once(once_scope, (n: Int) => {})
    once_first :: once_event.emit_async(1)
    once_started :: once_started_rx.receive() ?? panic("started")
    once_second :: once_event.emit_async(2)
    once_release_tx.send(1)
    once_first_report :: once_first.join()
    once_second_report :: once_second.join()
    print("once first={once_first_report.delivered_handlers()} second={once_second_report.delivered_handlers()}")

    failure_scope :: event.scope()
    collect :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    collect.on_priority(failure_scope, 10, (n: Int) => .Err("high"))
    collect.on_priority(failure_scope, 0, (n: Int) => .Err("low"))
    collected :: collect.emit_async(1).join()
    print("collect={collected.state() == .HandlerFailed} handlers={collected.delivered_handlers()} failures={collected.failures().len()}")
    print(collected.trace().summary())

    stop :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .StopFirst) ?? panic("policy")
    stop.on_priority(failure_scope, 10, (n: Int) => .Err("first"))
    stop.on_priority(failure_scope, 0, (n: Int) => {})
    stopped :: stop.emit_async(1).join()
    print("stop={stopped.state() == .HandlerFailed} handlers={stopped.delivered_handlers()} failures={stopped.failures().len()}")

    log_errors :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Log) ?? panic("policy")
    log_errors.on_priority(failure_scope, 10, (n: Int) => .Err("logged secret"))
    log_errors.on_priority(failure_scope, 0, (n: Int) => {})
    logged_error :: log_errors.emit_async(1).join()
    print("log error={logged_error.state() == .Delivered} handlers={logged_error.delivered_handlers()} failures={logged_error.failures().len()} traced={logged_error.trace().summary().contains("failed")}")

    ignore_errors :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Ignore) ?? panic("policy")
    ignore_errors.on_priority(failure_scope, 10, (n: Int) => .Err("ignored secret"))
    ignore_errors.on_priority(failure_scope, 0, (n: Int) => {})
    ignored_error :: ignore_errors.emit_async(1).join()
    print("ignore error={ignored_error.state() == .Delivered} handlers={ignored_error.delivered_handlers()} failures={ignored_error.failures().len()} traced={ignored_error.trace().summary().contains("failed")}")

    panic_log :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Log) ?? panic("policy")
    panic_log.on_priority(failure_scope, 10, (n: Int) => panic_log_handler(n))
    panic_log.on_priority(failure_scope, 0, (n: Int) => {})
    logged_panic :: panic_log.emit_async(1).join()
    print("panic log={logged_panic.state() == .HandlerFailed} handlers={logged_panic.delivered_handlers()} failures={logged_panic.failures().len()} traced={logged_panic.trace().summary().contains("panic:log boom")}")

    panic_ignore :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Ignore) ?? panic("policy")
    panic_ignore.on_priority(failure_scope, 10, (n: Int) => panic_ignore_handler(n))
    panic_ignore.on_priority(failure_scope, 0, (n: Int) => {})
    ignored_panic :: panic_ignore.emit_async(1).join()
    print("panic ignore={ignored_panic.state() == .HandlerFailed} handlers={ignored_panic.delivered_handlers()} failures={ignored_panic.failures().len()} traced={ignored_panic.trace().summary().contains("panic:ignore boom")}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event policies failed: {stderr}");
    assert_eq!(
        stdout,
        "newest=true\noldest=true\nonce first=2 second=1\ncollect=true handlers=2 failures=2\nqueued -> running -> handler:0:failed -> handler:1:failed -> terminal:HandlerFailed\nstop=true handlers=1 failures=1\nlog error=true handlers=2 failures=0 traced=true\nignore error=true handlers=2 failures=0 traced=false\npanic log=true handlers=1 failures=1 traced=true\npanic ignore=true handlers=1 failures=1 traced=true\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_scope_cancel_and_inherited_deadline_are_single_terminal() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_lifecycle_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_lifecycle",
        r#"
use core.event as event
use core.tasks as tasks
use core.time as time

fn owner_teardown_task() => Task<DispatchReport<String>> {
    owner_scope :: event.scope()
    ev :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (started_tx, started_rx) :: tasks.channel<Int>()
    (release_tx, release_rx) :: tasks.channel<Int>()
    ev.on(owner_scope, (n: Int) => {
        started_tx.send(~n)
        held_sender :: ~release_tx
        released :: release_rx.receive() ?? panic("release")
    })
    running :: ev.emit_async(98)
    started :: started_rx.receive() ?? panic("started")
    queued :: ev.emit_async(99)
    return queued
}

fn run() {
    cancel_scope :: event.scope()
    cancelled :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (cancel_started_tx, cancel_started_rx) :: tasks.channel<Int>()
    (cancel_release_tx, cancel_release_rx) :: tasks.channel<Int>()
    cancelled.on(cancel_scope, (n: Int) => {
        cancel_started_tx.send(~n)
        released :: cancel_release_rx.receive() ?? panic("release")
    })
    cancel_running :: cancelled.emit_async(1)
    started :: cancel_started_rx.receive() ?? panic("started")
    cancel_queued :: cancelled.emit_async(2)
    cancel_pending :: cancelled.emit_async(3)
    print("before-cancel q={cancelled.queued_count()} r={cancelled.running_count()} p={cancelled.blocked_count()}")
    cancel_scope.cancel()
    pending_report :: cancel_pending.join()
    queued_report :: cancel_queued.join()
    running_report :: cancel_running.join()
    print("cancel pending={!pending_report.accepted() && pending_report.state() == .Cancelled} trace={pending_report.trace().summary()}")
    print("cancel queued={queued_report.accepted() && queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
    print("cancel running={running_report.accepted() && running_report.state() == .Cancelled} trace={running_report.trace().summary()}")
    print("after-cancel q={cancelled.queued_count()} r={cancelled.running_count()} p={cancelled.blocked_count()}")

    queued_scope :: event.scope()
    queued_deadline :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (queued_started_tx, queued_started_rx) :: tasks.channel<Int>()
    (queued_release_tx, queued_release_rx) :: tasks.channel<Int>()
    queued_deadline.on(queued_scope, (n: Int) => {
        queued_started_tx.send(~n)
        released :: queued_release_rx.receive() ?? panic("release")
    })
    queued_running :: queued_deadline.emit_async(10)
    queued_started :: queued_started_rx.receive() ?? panic("started")
    #Context(deadline: time.now() + 20) {
        expires_queued :: queued_deadline.emit_async(11)
        queued_expired :: expires_queued.join()
        print("deadline queued={queued_expired.accepted() && queued_expired.state() == .DeadlineExceeded} trace={queued_expired.trace().summary()}")
    }
    queued_scope.cancel()
    queued_running.join()

    pending_scope :: event.scope()
    pending_deadline :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    (pending_started_tx, pending_started_rx) :: tasks.channel<Int>()
    (pending_release_tx, pending_release_rx) :: tasks.channel<Int>()
    pending_deadline.on(pending_scope, (n: Int) => {
        pending_started_tx.send(~n)
        released :: pending_release_rx.receive() ?? panic("release")
    })
    pending_running :: pending_deadline.emit_async(20)
    pending_started :: pending_started_rx.receive() ?? panic("started")
    pending_queued :: pending_deadline.emit_async(21)
    #Context(deadline: time.now() + 20) {
        expires_pending :: pending_deadline.emit_async(22)
        pending_expired :: expires_pending.join()
        print("deadline pending={!pending_expired.accepted() && pending_expired.state() == .DeadlineExceeded} trace={pending_expired.trace().summary()}")
    }
    pending_scope.cancel()
    pending_queued.join()
    pending_running.join()

    owner_task :: owner_teardown_task()
    owner_report :: owner_task.join()
    print("owner teardown={owner_report.accepted() && owner_report.state() == .Cancelled} trace={owner_report.trace().summary()}")
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "async event lifecycle failed: {stderr}");
    assert_eq!(
        stdout,
        "before-cancel q=1 r=1 p=1\ncancel pending=true trace=pending -> terminal:Cancelled\ncancel queued=true trace=queued -> terminal:Cancelled\ncancel running=true trace=queued -> running -> terminal:Cancelled\nafter-cancel q=0 r=0 p=0\ndeadline queued=true trace=queued -> terminal:DeadlineExceeded\ndeadline pending=true trace=pending -> terminal:DeadlineExceeded\nowner teardown=true trace=queued -> terminal:Cancelled\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn async_event_terminal_transition_rejects_terminal_expected_phase() {
    let source = include_str!(
        "../crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs"
    );
    let complete_entry = source
        .split_once("fn complete_entry(")
        .expect("async event terminal transition")
        .1
        .split_once("fn complete_report(")
        .expect("async event report transition")
        .0;
    let terminal_guard = complete_entry
        .find("if expected == JET_EVENT_TERMINAL")
        .expect("terminal phase must be absorbing");
    let phase_cas = complete_entry
        .find("entry.phase.compare_exchange(")
        .expect("terminal transition CAS");
    assert!(
        terminal_guard < phase_cas,
        "TERMINAL -> TERMINAL must be rejected before the phase CAS"
    );
}

#[test]
fn async_event_cancel_and_close_winners_remain_immutable_after_task_drain() {
    let have_rustc = common::have_rustc();
    if !have_rustc { return; }
    let dir = std::env::temp_dir().join(format!("jet_corelib_async_event_absorbing_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "async_event_absorbing",
        r#"
use core.event as event
use core.tasks as tasks
use core.time as time

fn run() {
    (cancel_gate_started_tx, cancel_gate_started_rx) :: tasks.channel<Int>()
    (cancel_gate_release_tx, cancel_gate_release_rx) :: tasks.channel<Int>()
    cancel_gate :: tasks.spawn(() => {
        cancel_gate_started_tx.send(1)
        released :: cancel_gate_release_rx.receive() ?? panic("cancel gate")
    })
    cancel_gate_started :: cancel_gate_started_rx.receive() ?? panic("cancel gate start")

    cancel_scope :: event.scope()
    cancel_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    #Context(deadline: time.now() + 100000) {
        cancel_queued :: cancel_event.emit_async(1)
        cancel_pending :: cancel_event.emit_async(2)
        cancel_event.on(cancel_scope, (n: Int) => {})
        cancel_scope.cancel()
        cancel_gate_release_tx.send(1)
        queued_report :: cancel_queued.join()
        pending_report :: cancel_pending.join()
        print("cancel queued={queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
        print("cancel pending={pending_report.state() == .Cancelled} trace={pending_report.trace().summary()}")
    }
    print("cancel counts={cancel_event.queued_count()},{cancel_event.running_count()},{cancel_event.blocked_count()}")

    (close_gate_started_tx, close_gate_started_rx) :: tasks.channel<Int>()
    (close_gate_release_tx, close_gate_release_rx) :: tasks.channel<Int>()
    close_gate :: tasks.spawn(() => {
        close_gate_started_tx.send(1)
        released :: close_gate_release_rx.receive() ?? panic("close gate")
    })
    close_gate_started :: close_gate_started_rx.receive() ?? panic("close gate start")

    close_scope :: event.scope()
    close_event :: event.async_result<Int, String>(AsyncPolicy.{ capacity: 1, overflow: .Block }, .Collect) ?? panic("policy")
    close_event.on(close_scope, (n: Int) => {})
    #Context(deadline: time.now() + 100000) {
        close_queued :: close_event.emit_async(3)
        close_pending :: close_event.emit_async(4)
        close_event.close()
        close_scope.cancel()
        close_gate_release_tx.send(1)
        queued_report :: close_queued.join()
        pending_report :: close_pending.join()
        print("close queued={queued_report.state() == .Cancelled} trace={queued_report.trace().summary()}")
        print("close pending={pending_report.state() == .Closed} trace={pending_report.trace().summary()}")
    }
    print("close counts={close_event.queued_count()},{close_event.running_count()},{close_event.blocked_count()}")
}
"#,
        &[("JET_SCHEDULER_THREADS", "1")],
        None,
    );
    assert_eq!(code, 0, "async event absorbing terminal failed: {stderr}");
    assert_eq!(
        stdout,
        "cancel queued=true trace=queued -> terminal:Cancelled\ncancel pending=true trace=pending -> terminal:Cancelled\ncancel counts=0,0,0\nclose queued=true trace=queued -> terminal:Cancelled\nclose pending=true trace=pending -> terminal:Closed\nclose counts=0,0,0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn event_sync_dispatch_handles_mutation_reentrancy_and_owner_drop() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_event_hostile_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "event_sync_hostile",
        r#"
use core.event as event

fn run() {
    scope :: event.scope()
    ev :: event.new<Int>()
    late :: ev.on(scope, (n) => { print("late {n}") })
    ev.on_priority(scope, 10, (n) => { print("killer {n}"); late.unsubscribe() })
    print(ev.emit(1).summary())
    print("listeners={ev.listener_count()}")

    additions :: event.scope()
    growing :: event.new<Int>()
    growing.on(additions, (n) => {
        print("root {n}")
        _ :: growing.on(additions, (m: Int) => { print("added {m}") })
    })
    print(growing.emit(1).summary())
    print(growing.emit(2).summary())

    nested_scope :: event.scope()
    nested :: event.new<Int>()
    nested.once(nested_scope, (n) => {
        print("once {n}")
        if n == 1 { nested.emit(2) }
    })
    print(nested.emit(1).summary())
    print("nested-listeners={nested.listener_count()}")

    owned :: event.new<Int>()
    if true {
        owner :: event.scope()
        owned.on(owner, (n) => { print("leaked {n}") })
    }
    print(owned.emit(9).summary())

    cancelled :: event.scope()
    stopped :: event.new<Int>()
    cancelled.cancel()
    stopped_sub :: stopped.on(cancelled, (n) => { print("cancelled event {n}") })
    print("cancelled-active={stopped_sub.is_active()}")
    print(stopped.emit(10).summary())
    stopped_hook :: event.hook<Int, String>("base")
    stopped_hook.on(cancelled, (n) => "cancelled hook {n}")
    print(stopped_hook.run(10, "fallback"))

    order_scope :: event.scope()
    ordered :: event.new<Int>()
    ordered.on_priority(order_scope, 5, (n) => { print("first {n}") })
    ordered.on_priority(order_scope, 5, (n) => { print("second {n}") })
    ordered.on(order_scope, (n) => { print("low {n}") })
    print(ordered.emit(3).summary())

    depth_scope :: event.scope()
    depth :: event.new<Int>()
    depth.on_priority(depth_scope, 5, (n) => {
        print("enter {n}")
        if n == 1 { print(depth.emit(2).summary()) }
    })
    depth.on(depth_scope, (n) => { print("leave {n}") })
    print(depth.emit(1).summary())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "hostile event runtime failed: {stderr}");
    assert_eq!(
        stdout,
        "killer 1\nevent delivered=1 queued=0 dropped=0\nlisteners=1\nroot 1\nevent delivered=1 queued=0 dropped=0\nroot 2\nadded 2\nevent delivered=2 queued=0 dropped=0\nonce 1\nevent delivered=1 queued=0 dropped=0\nnested-listeners=0\nevent delivered=0 queued=0 dropped=0\ncancelled-active=false\nevent delivered=0 queued=0 dropped=0\nfallback\nfirst 3\nsecond 3\nlow 3\nevent delivered=3 queued=0 dropped=0\nenter 1\nenter 2\nleave 2\nevent delivered=2 queued=0 dropped=0\nleave 1\nevent delivered=2 queued=0 dropped=0\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_solver_records_bool_constraints_in_order() {
    let have_rustc = common::have_rustc();
    if !have_rustc {
        eprintln!("note: skipping solver runtime test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_corelib_solve_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "solve_runtime",
        r#"
use core.solve as solve

fn run() {
    solver := solve.Solver.new(42)
    solver.require(1 + 1 == 2)
    solver.require(2 * 3 == 5)
    solver.require(true)
    print(solver.status())
    print(solver.failure_count())
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "failed\n1\n");
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn solve_require_needs_mutable_solver() {
    let src = r#"
use core.solve as solve

fn run() {
    solver :: solve.Solver.new(1)
    solver.require(true)
}
"#;
    let res = jet::compile(src);
    assert!(res.is_err(), "solver.require on immutable solver must fail");
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_solver_type_name_is_reserved() {
    let src = r#"
struct Solver { value: Int }

fn run() {}
"#;
    let diags = jet::compile(src).expect_err("Solver is a reserved Core handle name");
    assert!(
        diags.iter().any(|d| d.code == "E0106"),
        "expected E0106, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn solve_constructor_is_static_only() {
    let src = r#"
use core.solve as solve

fn run() {
    solver := solve.Solver.new(1)
    solver.new(2)
}
"#;
    let diags = jet::compile(src).expect_err("solver.new must not be an instance method");
    assert!(
        !diags.is_empty(),
        "expected a diagnostic for instance constructor"
    );
}

#[test]
fn game_scene_asset_registration_needs_mutable_scene() {
    let src = r#"
use core.game as game

fn run() {
    scene :: game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
}
"#;
    let diags = jet::compile(src).expect_err("asset registration must need edit access");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_needs_mutable_scene_lvalue() {
    let src = r#"
use core.game as game

fn run() {
    print(game.run(game.Scene.new("arcade")))
}
"#;
    let diags = jet::compile(src).expect_err("game.run must reject temporary scene");
    assert!(
        diags.iter().any(|d| d.code == "E0202"),
        "expected E0202, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_run_rejects_transposed_labels() {
    let src = r#"
use core.game as game

fn run() {
    scene := game.Scene.new("arcade")
    replay :: game.Replay.record("runs/demo.jetreplay")
    backend :: game.Backend.headless()
    print(game.run(scene, backend: backend, replay))
}
"#;
    let diags = jet::compile(src).expect_err("game.run labels must match positional shape");
    assert!(
        diags.iter().any(|d| matches!(d.code.as_str(), "E0764" | "E0769")),
        "expected E0764/E0769, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn game_headless_scene_replay_transcript_is_deterministic() {
    let dir = std::env::temp_dir().join(format!("jet_corelib_game_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run(
        &dir,
        "game_headless",
        r#"
use core.game as game

module perf.game {
    budgets: [
        Budget.{ name: "frame", scope: .Scene("arcade"), metric: .FrameTime(.P99), provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(16ms) },
        Budget.{ name: "memory", scope: .Scene("arcade"), metric: .MemoryHighWater, provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(96MiB) },
        Budget.{ name: "assets", scope: .Scene("arcade"), metric: .SceneAssetBytes, provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(256KiB) },
        Budget.{ name: "draws", scope: .Scene("arcade"), metric: .DrawCalls(.P99), provider: .SceneProbe("arcade"), comparison: .AbsoluteFrom("local/arcade"), limit: .AtMost(4) },
    ]
}

struct Position { x: Int }
struct Velocity { dx: Int }

fn run() {
    scene := game.Scene.new("arcade")
    scene.assets.image("assets/player.png") ?? panic("image")
    scene.assets.sound("assets/jump.wav") ?? panic("sound")
    scene.input.bind("jump", "Space")
    scene.component<Position>()
    scene.component<Velocity>()
    hits :: scene.query<Position, Velocity>()
    print("query {hits.len()}")
    print("row {hits[0]}")
    backend := game.Backend.headless()
    n := 0
    loop {
        if !backend.should_continue() { break }
        backend.present()
        n += 1
    }
    print("budget {n}")
    scene.on_frame((frame) => {
        if frame.input.pressed("jump") {
            print("hook jump {frame.index}")
        }
    })
    replay :: game.Replay.record("runs/demo.jetreplay")
    print(game.run(scene, replay: replay))
}
"#,
        &[],
        None,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "query 1\nrow Position{x:0},Velocity{dx:0}\nbudget 3\nhook jump 1\nscene:arcade\nbackend:headless/none/none\nreplay:runs/demo.jetreplay\nassets:image:assets/player.png,sound:assets/jump.wav\ninput:jump=Space\ncomponents:Position,Velocity\nframe:0 input:none\nframe:1 input:jump\nframe:2 input:none\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

// D-AUTH2=A / D-AUTH-TOKENPOLICY1=A: exercise the public Jet surface so the
// existing JSON parser, HMAC implementation, Ed25519 bridge, nominal claims,
// codegen, and linker all participate in the proof.
#[test]
fn core_auth_strict_jwt_and_paseto_hostile_matrix() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = r#"
use core.auth as auth

fn run() {
    jwt_key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    no_skew :: Duration.milliseconds(0) ?? panic("duration")
    valid_jwt := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrY"
    wrong_aud := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImJpbGxpbmciLCJleHAiOjQxMDI0NDQ4MDB9.4HckXFIKTMLaJr8Zjz8hYC0NQ9gO1xbLzZwoNxU1ew4"
    missing_exp := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkifQ.w3V9KixrW5iIdce6fH3-kTGBF1BoIAVN9jlaASUZyo8"
    missing_aud := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImV4cCI6NDEwMjQ0NDgwMH0.DvdDttFvdgTOXtC2L5P1zfs2bIMtiEwN3al4EAHYyf8"
    wrong_alg := "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJleHAiOjQxMDI0NDQ4MDB9.Nq0tUwRS8BvslH3fvzVydHKrce-EcFBuLy7OpgQ2ICk"
    duplicate_header := "eyJhbGciOiJIUzI1NiIsImFsZyI6IlJTMjU2In0.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo0MTAyNDQ0ODAwfQ.MVJzUJG0exT9xheHOk7OpVqtfue7C_625krxtNm99qw"
    escaped_duplicate_header := "eyJhbGciOiJIUzI1NiIsIlx1MDA2MWxnIjoiUlMyNTYifQ.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo0MTAyNDQ0ODAwfQ.z6ZtYWs143-PSZdfZSqrtX1lZOOb5KiXh_J-H6nr5gs"
    duplicate_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiYXVkIjoiYmlsbGluZyIsImV4cCI6NDEwMjQ0NDgwMH0.OVeIFJjjIN6Py2ZsvNiOFERv0Syt2nDTF2ZUZwWQkS0"
    escaped_duplicate_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiXHUwMDYxdWQiOiJiaWxsaW5nIiwiZXhwIjo0MTAyNDQ0ODAwfQ.-RJABGbCML2FgyJx4iWT4NsklKovltcY_lyVzDNTec4"
    object_audience := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOnsieCI6ImdhdGV3YXkifSwiZXhwIjo0MTAyNDQ0ODAwfQ.BNUK56f_MGWL-7vRscOjDZGWtXZA18muouezh3BFg-Q"
    object_expiry := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjp7Im4iOjQxMDI0NDQ4MDB9fQ.X1BTPgGav4pUqxQVq2uMYt4_VYEHfMRGP1aI5V50k2g"
    wrong_issuer := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiaXNzIjoib3RoZXIiLCJleHAiOjQxMDI0NDQ4MDB9.ZVsh0LK7bvsylhpzu4i8TrgthCbSaelpKaoxWqF5-G4"
    expired := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo5NDY2ODQ4MDB9.P-GYVR6Tc1zwSdZCEX6kbv4eSryvnxlevXfHU0MJMEg"
    overflow_expiry := "eyJhbGciOiJIUzI1NiJ9.eyJhdWQiOiJnYXRld2F5IiwiZXhwIjo5MjIzMzcyMDM2ODU0Nzc1MDAwfQ.jHiJ1xzrrSVPwIEX-EujI-xiDDdgc7AvP6HsMWrb_L8"
    noncanonical_base64 := "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB9.3gbnbn_u-GjiQuGusiLrnMUzlo5c9rPeqAO0iWZxhrZ"
    if auth.verify_jwt(valid_jwt, key: jwt_key, audience: "gateway", issuer: "partner", clock_skew: no_skew) == {
        .Ok(claims) -> { print("ok:{claims.audience}") }
        .Err(_) -> { print("rejected") }
    }
    if auth.verify_jwt(wrong_aud, key: jwt_key, audience: "gateway") == {
        .Ok(_) -> { print("accepted") }
        .Err(error) -> {
            if error == {
                .WrongAudience(expected, actual) -> { print("aud:{expected}:{actual}") }
                else -> { print("wrong-error") }
            }
        }
    }
    if auth.verify_jwt(missing_exp, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(missing_aud, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(wrong_alg, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt("{valid_jwt}x", key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_jwt(duplicate_header, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("duplicate-header-accepted") } .Err(_) -> { print("duplicate-header-rejected") } }
    if auth.verify_jwt(escaped_duplicate_header, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("escaped-duplicate-header-accepted") } .Err(_) -> { print("escaped-duplicate-header-rejected") } }
    if auth.verify_jwt(duplicate_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("duplicate-audience-accepted") } .Err(_) -> { print("duplicate-audience-rejected") } }
    if auth.verify_jwt(escaped_duplicate_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("escaped-duplicate-audience-accepted") } .Err(_) -> { print("escaped-duplicate-audience-rejected") } }
    if auth.verify_jwt(object_audience, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("object-audience-accepted") } .Err(_) -> { print("object-audience-rejected") } }
    if auth.verify_jwt(object_expiry, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("object-expiry-accepted") } .Err(_) -> { print("object-expiry-rejected") } }
    if auth.verify_jwt(wrong_issuer, key: jwt_key, audience: "gateway", issuer: "partner") == { .Ok(_) -> { print("issuer-accepted") } .Err(_) -> { print("issuer-rejected") } }
    if auth.verify_jwt(expired, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("expired-accepted") } .Err(_) -> { print("expired-rejected") } }
    if auth.verify_jwt(overflow_expiry, key: jwt_key, audience: "gateway") == { .Ok(_) -> { print("overflow-accepted") } .Err(_) -> { print("overflow-rejected") } }
    if auth.verify_jwt(noncanonical_base64, key: jwt_key, audience: "gateway", issuer: "partner") == { .Ok(_) -> { print("noncanonical-accepted") } .Err(_) -> { print("noncanonical-rejected") } }
    weak_key :: [U8].{ 115, 104, 111, 114, 116 }
    if auth.verify_jwt(valid_jwt, key: weak_key, audience: "gateway") == { .Ok(_) -> { print("weak-key-accepted") } .Err(error) -> { if error == { .WeakKey -> { print("weak-key-rejected") } else -> { print("weak-key-wrong-error") } } } }

    public_key :: [U8].{ 198, 185, 67, 192, 34, 178, 159, 209, 168, 14, 60, 124, 14, 126, 172, 99, 191, 6, 53, 9, 101, 220, 114, 205, 7, 138, 24, 227, 74, 150, 126, 45 }
    footer :: [U8].{ 107, 105, 100, 45, 49 }
    implicit :: [U8].{ 116, 101, 110, 97, 110, 116, 45, 97 }
    paseto := "v4.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    bad_signature := "v4.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAg.a2lkLTE"
    wrong_purpose := "v4.local.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    wrong_version := "v3.public.eyJzdWIiOiJhbGljZSIsImF1ZCI6ImdhdGV3YXkiLCJpc3MiOiJwYXJ0bmVyIiwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3MDAwMDAwMDB99cRKnMLYsWG_FHDSPR15TvgcHSv6gYcTBIy9ToyrtIMVWk4i5vp1sgI5rehiGKdAoyKHQ1zKXDe0It-WADRzAw.a2lkLTE"
    bad :: [U8].{ 98, 97, 100 }
    zero_key :: [U8].{ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == {
        .Ok(claims) -> { print("ok:{claims.audience}") }
        .Err(_) -> { print("rejected") }
    }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: bad, implicit: implicit) == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_paseto(paseto, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: bad) == { .Ok(_) -> { print("accepted") } .Err(_) -> { print("rejected") } }
    if auth.verify_paseto(wrong_version, key: public_key, audience: "gateway") == { .Ok(_) -> { print("wrong-version-accepted") } .Err(_) -> { print("wrong-version-rejected") } }
    if auth.verify_paseto(wrong_purpose, key: public_key, audience: "gateway") == { .Ok(_) -> { print("wrong-purpose-accepted") } .Err(_) -> { print("wrong-purpose-rejected") } }
    if auth.verify_paseto(paseto, key: bad, audience: "gateway") == { .Ok(_) -> { print("short-paseto-key-accepted") } .Err(_) -> { print("short-paseto-key-rejected") } }
    if auth.verify_paseto(paseto, key: zero_key, audience: "gateway") == { .Ok(_) -> { print("zero-paseto-key-accepted") } .Err(_) -> { print("zero-paseto-key-rejected") } }
    if auth.verify_paseto(bad_signature, key: public_key, audience: "gateway", issuer: "partner", clock_skew: no_skew, footer: footer, implicit: implicit) == { .Ok(_) -> { print("bad-signature-accepted") } .Err(_) -> { print("bad-signature-rejected") } }
}
"#;
    let (code, stdout, stderr) = build_and_run(&dir, "strict_tokens", source, &[], None);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stdout,
        "ok:gateway\naud:gateway:billing\nrejected\nrejected\nrejected\nrejected\nduplicate-header-rejected\nescaped-duplicate-header-rejected\nduplicate-audience-rejected\nescaped-duplicate-audience-rejected\nobject-audience-rejected\nobject-expiry-rejected\nissuer-rejected\nexpired-rejected\noverflow-rejected\nnoncanonical-rejected\nweak-key-rejected\nok:gateway\nrejected\nrejected\nwrong-version-rejected\nwrong-purpose-rejected\nshort-paseto-key-rejected\nzero-paseto-key-rejected\nbad-signature-rejected\n"
    );
    assert_eq!(stderr, "");
    let _ = fs::remove_dir_all(&dir);
}

fn auth_test_b64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let bits = (a << 16) | (b << 8) | c;
        out.push(TABLE[((bits >> 18) & 63) as usize] as char);
        out.push(TABLE[((bits >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { out.push(TABLE[((bits >> 6) & 63) as usize] as char); }
        if chunk.len() > 2 { out.push(TABLE[(bits & 63) as usize] as char); }
    }
    out
}

fn auth_test_jwt(payload: &str) -> String {
    let key = b"0123456789abcdef0123456789abcdef";
    let header = auth_test_b64url(br#"{"alg":"HS256"}"#);
    let payload = auth_test_b64url(payload.as_bytes());
    let signed = format!("{header}.{payload}");
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let mut inner = Vec::with_capacity(64 + signed.len());
    inner.extend(block.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(signed.as_bytes());
    let inner = jet::SHA256::sha256(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend(block.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner);
    format!("{signed}.{}", auth_test_b64url(&jet::SHA256::sha256(&outer)))
}

#[test]
fn core_auth_expiry_equality_and_subsecond_skew() {
    let dir = std::env::temp_dir().join(format!("jet_core_auth_clock_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.auth as auth
use core.env as env

fn run() {
    key :: [U8].{ 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 97, 98, 99, 100, 101, 102 }
    token := env.get("JET_AUTH_CLOCK_TOKEN") ?? panic("token")
    zero :: Duration.milliseconds(0) ?? panic("zero")
    skew :: Duration.milliseconds(1500) ?? panic("skew")
    if auth.verify_jwt(token, key: key, audience: "gateway", issuer: "clock", clock_skew: zero) == {
        .Ok(_) -> { print("equality-accepted") }
        .Err(error) -> { if error == { .TokenExpired -> { print("equality-expired") } else -> { print("wrong-error") } } }
    }
    if auth.verify_jwt(token, key: key, audience: "gateway", issuer: "clock", clock_skew: skew) == {
        .Ok(_) -> { print("subsecond-skew-accepted") }
        .Err(_) -> { print("subsecond-skew-rejected") }
    }
}
"#;
    let shown = dir.join("clock.jet");
    fs::write(&shown, src).unwrap();
    let compiled = jet::compile_with_path(src, shown.to_str().unwrap()).unwrap();
    let rs = dir.join("clock.rs");
    let bin = dir.join("clock");
    fs::write(&rs, compiled.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc.args(["--edition", "2021"]).arg(&rs).arg("-o").arg(&bin);
    if let Some(link) = compiled.ffi {
        rustc.arg("--extern").arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) { rustc.arg("-L").arg(format!("dependency={}", deps_dir.display())); }
    }
    let built = rustc.output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let expires_at = now.as_secs() + 2;
    let token = auth_test_jwt(&format!(r#"{{"aud":"gateway","iss":"clock","exp":{expires_at}}}"#));
    let boundary_ms = u128::from(expires_at) * 1_000;
    while std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() < boundary_ms {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let run = Command::new(&bin).env("JET_AUTH_CLOCK_TOKEN", token).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "equality-expired\nsubsecond-skew-accepted\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn core_auth_requires_named_key_and_audience() {
    let src = r#"
use core.auth as auth

fn run() {
    token := "a.b.c"
    key := [0, 1, 2]
    auth.verify_jwt(token, key, "gateway")
}
"#;
    let diags = jet::compile(src).expect_err("auth trust inputs must be named");
    assert!(
        diags.iter().filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0764" | "E0769")).count() >= 2,
        "expected key:/audience: label diagnostics, got {diags:?}"
    );
}

#[test]
fn tracked_float_origin_reports_binding_site_and_plain_float_is_untracked() {
    let dir = std::env::temp_dir().join(format!(
        "jet_float_binding_origin_aot_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let name = "float_binding_origin";
    let src = "fn run() {\n    #Track speed :: 3.5\n    plain :: 3.5\n    copied :: speed\n    print(speed.origin())\n    print(plain.origin())\n    print(copied.origin())\n    print(next().origin())\n}\nfn next() => Float {\n    print(\"evaluated\")\n    return 3.5\n}\n";
    let (code, stdout, stderr) = build_and_run(&dir, name, src, &[], None);
    let source_path = dir.join(name);

    assert_eq!(code, 0, "tracked Float runtime failed: {stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "tracked `speed` at {}:2:12: #Track speed :: 3.5\nuntracked\nuntracked\nevaluated\nuntracked\n",
            source_path.display()
        )
    );
    let _ = fs::remove_dir_all(&dir);
}
