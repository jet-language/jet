# #3126 — Secure email submission and delivery outcomes (SCRIPT-F39)

Status: BLOCKED on the current binary (`jet-debug-snapshot14`, 2026-09-29).
No SMTP cell was exercised.

## Prepared harness (scratch, not in repo)

`~/.cache/jet-dev/scratch/Closer04/smtp/` holds three pieces:
- `server.py`: stdlib-only scripted SMTP server that uses the
  `tests/fixtures/tls/smtp.*` certificates. Modes:
  - `starttls`: advertise STARTTLS, upgrade, accept AUTH, answer
    `550` for `bad@`;
  - `nostarttls`: never advertise STARTTLS;
  - `dropdata`: close after the DATA terminator with no reply;
  - `slow`: stall after DATA.

  It logs every command with the AUTH payload redacted plus a `tls=` flag,
  the DATA address headers, whether the Bcc mailbox appears in DATA, and a
  per-connection count.
- `send.jet`: builds a `Message` with `to: [ada, bad]`, `bcc: [hidden]`,
  `security: .StartTls`, `trust: TLSTrust.SystemPlusCa{pem: <smtp.ca.cert.pem>}`,
  and a RequireAll/DeliverAccepted switch.
- Scenario runner: not written, because the probe cannot compile.

## Blocker

```
jet run send.jet -- localhost <port> all smtp.ca.cert.pem
Error [E0102]: `<corelib>/Core/email::Core/email/email.jet::Mailer` has no method `send`
  --> send.jet:38:22
```

Sema defines `Mailer.send` only as a special handle method
(Compiler/JetSema/Source/Sema/Calls/TextHandles.jet:138,147-148). It is
gated on `sema_text_handle_is_canonical(receiver_value.ty, "Mailer", …)`,
which rejects the Core source type returned by `email.smtp(config)`. The same
failure hides `MIME.param/essence` (#3125). The existing contract test
`tests/corelib_parts/email.rs:452-458` uses the same `mailer.send(message)`
spelling.

A diagnostic inconsistency showed up along the way:
- `trust: .SystemPlusCa(pem: ca)` gives E0003 (parser);
- `.SystemPlusCa(ca)` gives E0303, whose fix-it suggests
  `TLSTrust.SystemPlusCa.{ … }`;
- that spelling gives E0320 ("uses `TLSTrust.SystemPlusCa{…}`, not `.{…}`").

## Verdicts

c1–c4: not exercised. There is no email.smtp claim. Rerun `server.py` +
`send.jet` once `Mailer.send` resolves. Criterion c4 also needs the Rust
harness `tests/corelib_platform_email.rs::core_email_local_smtp_*`, which
does not exist yet.
