//! CtValue marshalling for the one Prelude `core.email` kernel.

use crate::AST::{CtReport, CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};

use super::Diagnostics::unsupported;

mod kernel {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    include!("../../../jet-codegen/src/Prelude/CoreLib/Email.rs");
}

use kernel::jet_email;

#[derive(Clone, Copy)]
pub struct RuntimeFns {
    pub tls_begin: fn(std::net::TcpStream, &String) -> Result<i64, String>,
    pub tls_begin_ca: fn(std::net::TcpStream, &String, &Vec<u8>) -> Result<i64, String>,
    pub tls_handshake_step: fn(i64) -> Result<bool, String>,
    pub tls_set_poll_timeout: fn(i64, i64) -> Result<(), String>,
    pub tls_read: fn(i64, i64) -> Result<Vec<u8>, String>,
    pub tls_write_all: fn(i64, &Vec<u8>) -> Result<(), String>,
    pub tls_close: fn(i64) -> Result<(), String>,
    pub wipe: fn(&mut Vec<u8>),
    pub sha256: fn(&[u8]) -> [u8; 32],
    pub ed25519_sign: fn(&Vec<u8>, &[u8]) -> Result<Vec<u8>, String>,
    pub cancelled: fn() -> bool,
    pub remaining_ms: fn() -> Option<i64>,
    pub accepted_at: fn() -> String,
}

fn kernel_runtime(runtime: RuntimeFns) -> jet_email::RuntimeFns {
    jet_email::RuntimeFns {
        tls_begin: runtime.tls_begin,
        tls_begin_ca: runtime.tls_begin_ca,
        tls_handshake_step: runtime.tls_handshake_step,
        tls_set_poll_timeout: runtime.tls_set_poll_timeout,
        tls_read: runtime.tls_read,
        tls_write_all: runtime.tls_write_all,
        tls_close: runtime.tls_close,
        wipe: runtime.wipe,
        sha256: runtime.sha256,
        ed25519_sign: runtime.ed25519_sign,
        cancelled: runtime.cancelled,
        remaining_ms: runtime.remaining_ms,
        accepted_at: runtime.accepted_at,
    }
}

fn copy_secret(value: &Vec<u8>) -> Vec<u8> {
    value.clone()
}

pub fn evaluate(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let result = match method {
        "address" => email_address(args, span),
        "attachment" => email_attachment(args, span),
        "message" => email_message(args, span),
        "envelope" => email_envelope(args, span),
        "serialize" => email_serialize(args, span),
        _ => return None,
    };
    Some(result)
}

pub fn evaluate_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let result = match method {
        "envelope" if args.is_empty() => {
            let message = match message_from_value(recv, span) {
                Ok(message) => message,
                Err(error) => return Some(Err(error)),
            };
            Ok(envelope_value(message.envelope()))
        }
        "with_envelope" if args.len() == 1 => {
            let message = match message_from_value(recv, span) {
                Ok(message) => message,
                Err(error) => return Some(Err(error)),
            };
            let envelope = match envelope_from_value(&args[0], span) {
                Ok(envelope) => envelope,
                Err(error) => return Some(Err(error)),
            };
            Ok(result(message.with_envelope(&envelope), |value| {
                message_value(&value)
            }))
        }
        "send" => return None,
        _ => return None,
    };
    Some(result)
}

fn one<'a>(
    args: &'a [CtValue],
    index: usize,
    method: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    args.get(index).ok_or_else(|| {
        unsupported(&format!("email.{method}(): missing argument {index}"), span)
    })
}

fn string(value: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value),
        _ => Err(unsupported("email call expected String", span)),
    }
}

fn bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                _ => Err(unsupported("email bytes must be a [U8] value", span)),
            })
            .collect(),
        _ => Err(unsupported("email bytes must be a [U8] value", span)),
    }
}

fn field<'a>(value: &'a CtValue, type_name: &str, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct { type_name: actual, fields } if actual == type_name => {
            fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value)
        }
        _ => None,
    }
}

fn structure(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    }
}

fn address_value(address: &jet_email::Address) -> CtValue {
    structure("Address", vec![
        (
            "display",
            address
                .display
                .as_ref()
                .map_or(CtValue::absent(Type::String), |display| {
                    CtValue::Present(Box::new(CtValue::Str(display.clone())))
                }),
        ),
        ("mailbox", CtValue::Str(address.mailbox.clone())),
    ])
}

fn address_from_value(value: &CtValue, span: Span) -> Result<jet_email::Address, Diagnostic> {
    let mailbox = match field(value, "Address", "mailbox") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email call expected Address", span)),
    };
    let display = match field(value, "Address", "display") {
        Some(CtValue::Present(value)) => match value.as_ref() {
            CtValue::Str(value) => Some(value.clone()),
            _ => return Err(unsupported("email Address display is invalid", span)),
        },
        Some(CtValue::Failed(CtReport::Clean(_))) => None,
        _ => return Err(unsupported("email Address display is invalid", span)),
    };
    Ok(jet_email::Address { display, mailbox })
}

fn address_list(value: &CtValue, span: Span) -> Result<Vec<jet_email::Address>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("email call expected an Address list", span));
    };
    values
        .iter()
        .map(|value| address_from_value(value, span))
        .collect()
}

fn attachment_value(attachment: &jet_email::Attachment) -> CtValue {
    structure("Attachment", vec![
        ("filename", CtValue::Str(attachment.filename.clone())),
        ("mime", CtValue::Str(attachment.mime.clone())),
        ("bytes", CtValue::Bytes(attachment.bytes.clone())),
    ])
}

fn attachment_from_value(value: &CtValue, span: Span) -> Result<jet_email::Attachment, Diagnostic> {
    let filename = match field(value, "Attachment", "filename") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email call expected Attachment", span)),
    };
    let mime = match field(value, "Attachment", "mime") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email Attachment content type is invalid", span)),
    };
    let bytes = field(value, "Attachment", "bytes")
        .ok_or_else(|| unsupported("email Attachment bytes are missing", span))
        .and_then(|value| bytes(value, span))?;
    Ok(jet_email::Attachment { filename, mime, bytes })
}

fn attachment_list(value: &CtValue, span: Span) -> Result<Vec<jet_email::Attachment>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("email call expected an Attachment list", span));
    };
    values
        .iter()
        .map(|value| attachment_from_value(value, span))
        .collect()
}

fn envelope_value(envelope: &jet_email::Envelope) -> CtValue {
    structure("Envelope", vec![
        ("from", address_value(&envelope.from)),
        (
            "recipients",
            CtValue::List(envelope.recipients.iter().map(address_value).collect()),
        ),
    ])
}

fn envelope_from_value(value: &CtValue, span: Span) -> Result<jet_email::Envelope, Diagnostic> {
    let from = field(value, "Envelope", "from")
        .ok_or_else(|| unsupported("email Envelope sender is missing", span))
        .and_then(|value| address_from_value(value, span))?;
    let recipients = field(value, "Envelope", "recipients")
        .ok_or_else(|| unsupported("email Envelope recipient list is missing", span))
        .and_then(|value| address_list(value, span))?;
    Ok(jet_email::Envelope { from, recipients })
}

fn message_value(message: &jet_email::Message) -> CtValue {
    structure("Message", vec![
        ("from", address_value(&message.from)),
        (
            "to",
            CtValue::List(message.to.iter().map(address_value).collect()),
        ),
        (
            "bcc",
            CtValue::List(message.bcc.iter().map(address_value).collect()),
        ),
        ("subject", CtValue::Str(message.subject.clone())),
        ("text", CtValue::Str(message.text.clone())),
        ("html", CtValue::Str(message.html.clone())),
        (
            "attachments",
            CtValue::List(
                message
                    .attachments
                    .iter()
                    .map(attachment_value)
                    .collect(),
            ),
        ),
        ("envelope", envelope_value(&message.envelope)),
        ("wire_upper", CtValue::Int(message.wire_upper as i64)),
    ])
}

fn message_from_value(value: &CtValue, span: Span) -> Result<jet_email::Message, Diagnostic> {
    let from = field(value, "Message", "from")
        .ok_or_else(|| unsupported("email Message sender is missing", span))
        .and_then(|value| address_from_value(value, span))?;
    let list = |name| {
        field(value, "Message", name)
            .ok_or_else(|| unsupported("email Message address list is missing", span))
            .and_then(|value| address_list(value, span))
    };
    let text = |name| match field(value, "Message", name) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported("email Message text field is invalid", span)),
    };
    let attachments = field(value, "Message", "attachments")
        .ok_or_else(|| unsupported("email Message attachments are missing", span))
        .and_then(|value| attachment_list(value, span))?;
    let to = list("to")?;
    let bcc = list("bcc")?;
    let message = jet_email::message(
        &from,
        &to,
        &bcc,
        &text("subject")?,
        &text("text")?,
        &text("html")?,
        &attachments,
    )
    .map_err(|error| {
        unsupported(
            &format!("email Message is invalid: {}", jet_email::error_reason(&error)),
            span,
        )
    })?;
    match field(value, "Message", "envelope") {
        Some(value) => {
            let envelope = envelope_from_value(value, span)?;
            message.with_envelope(&envelope).map_err(|error| {
                unsupported(
                    &format!(
                        "email Message envelope is invalid: {}",
                        jet_email::error_reason(&error)
                    ),
                    span,
                )
            })
        }
        None => Ok(message),
    }
}

fn error_value(error: jet_email::Error) -> CtValue {
    let (variant, _disc, operation, server, code, reason) = jet_email::error_parts(error);
    CtValue::Enum {
        type_name: "EmailError".to_string(),
        variant: variant.to_string(),
        args: vec![
            (Some("operation".to_string()), CtValue::Str(operation)),
            (
                Some("server".to_string()),
                server.map_or(CtValue::absent(Type::String), |value| {
                    CtValue::Present(Box::new(CtValue::Str(value)))
                }),
            ),
            (
                Some("code".to_string()),
                code.map_or(CtValue::absent(Type::Int), |value| {
                    CtValue::Present(Box::new(CtValue::Int(value)))
                }),
            ),
            (Some("reason".to_string()), CtValue::Str(reason)),
        ],
    }
}

fn result<T>(value: Result<T, jet_email::Error>, map: impl FnOnce(T) -> CtValue) -> CtValue {
    match value {
        Ok(value) => CtValue::Present(Box::new(map(value))),
        Err(error) => CtValue::failed(Box::new(error_value(error))),
    }
}

fn email_address(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let input = string(one(args, 0, "address", span)?, span)?.to_string();
    Ok(result(jet_email::address(&input), |value| {
        address_value(&value)
    }))
}

fn email_attachment(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let filename = string(one(args, 0, "attachment", span)?, span)?.to_string();
    let mime = string(one(args, 1, "attachment", span)?, span)?.to_string();
    let bytes = bytes(one(args, 2, "attachment", span)?, span)?;
    Ok(result(
        jet_email::attachment(&filename, &mime, &bytes),
        |value| attachment_value(&value),
    ))
}

fn email_envelope(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let from = address_from_value(one(args, 0, "envelope", span)?, span)?;
    let recipients = address_list(one(args, 1, "envelope", span)?, span)?;
    Ok(result(jet_email::envelope(&from, &recipients), |value| {
        envelope_value(&value)
    }))
}

fn email_message(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let from = address_from_value(one(args, 0, "message", span)?, span)?;
    let to = address_list(one(args, 1, "message", span)?, span)?;
    let bcc = address_list(one(args, 2, "message", span)?, span)?;
    let subject = string(one(args, 3, "message", span)?, span)?.to_string();
    let text = string(one(args, 4, "message", span)?, span)?.to_string();
    let html = string(one(args, 5, "message", span)?, span)?.to_string();
    let attachments = attachment_list(one(args, 6, "message", span)?, span)?;
    Ok(result(
        jet_email::message(&from, &to, &bcc, &subject, &text, &html, &attachments),
        |value| message_value(&value),
    ))
}

fn email_serialize(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let message = message_from_value(one(args, 0, "serialize", span)?, span)?;
    Ok(result(jet_email::serialize(&message), CtValue::Bytes))
}

fn enum_args<'a>(
    value: &'a CtValue,
    type_name: &str,
    variant: &str,
) -> Option<&'a [(Option<String>, CtValue)]> {
    match value {
        CtValue::Enum {
            type_name: actual,
            variant: actual_variant,
            args,
        } if actual == type_name && actual_variant == variant => Some(args.as_slice()),
        _ => None,
    }
}

fn enum_arg<'a>(args: &'a [(Option<String>, CtValue)], name: &str) -> Option<&'a CtValue> {
    args.iter().find_map(|(field, value)| {
        (field.as_deref() == Some(name)).then_some(value)
    })
}

fn int(value: &CtValue, what: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(*value),
        _ => Err(unsupported(&format!("email {what} must be Int"), span)),
    }
}

fn secret(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match value {
        CtValue::Struct { type_name, fields } if type_name == "Secret" => fields
            .iter()
            .find_map(|(name, value)| (name == "bytes").then_some(value))
            .ok_or_else(|| unsupported("email Secret.bytes is missing", span))
            .and_then(|value| bytes(value, span)),
        _ => bytes(value, span),
    }
}

fn string_list(value: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("email expected a String list", span));
    };
    values
        .iter()
        .map(|value| string(value, span).map(str::to_string))
        .collect()
}

fn optional<T>(
    value: &CtValue,
    span: Span,
    map: impl FnOnce(&CtValue) -> Result<T, Diagnostic>,
) -> Result<Option<T>, Diagnostic> {
    match value {
        CtValue::Failed(CtReport::Clean(_)) => Ok(None),
        CtValue::Present(value) => map(value).map(Some),
        _ => Err(unsupported("email optional value is invalid", span)),
    }
}

fn limits_from_value(value: &CtValue, span: Span) -> Result<jet_email::Limits, Diagnostic> {
    let field_int = |name| {
        field(value, "Limits", name)
            .ok_or_else(|| unsupported(&format!("email Limits.{name} is missing"), span))
            .and_then(|value| int(value, &format!("Limits.{name}"), span))
    };
    Ok(jet_email::Limits {
        max_reply_line_bytes: field_int("max_reply_line_bytes")?,
        max_reply_lines: field_int("max_reply_lines")?,
        max_capabilities: field_int("max_capabilities")?,
        max_recipients: field_int("max_recipients")?,
        max_message_bytes: field_int("max_message_bytes")?,
        max_auth_challenge_bytes: field_int("max_auth_challenge_bytes")?,
    })
}

pub fn limits_safe_value() -> CtValue {
    let limits = jet_email::Limits::safe();
    structure(
        "Limits",
        vec![
            ("max_reply_line_bytes", CtValue::Int(limits.max_reply_line_bytes)),
            ("max_reply_lines", CtValue::Int(limits.max_reply_lines)),
            ("max_capabilities", CtValue::Int(limits.max_capabilities)),
            ("max_recipients", CtValue::Int(limits.max_recipients)),
            ("max_message_bytes", CtValue::Int(limits.max_message_bytes)),
            (
                "max_auth_challenge_bytes",
                CtValue::Int(limits.max_auth_challenge_bytes),
            ),
        ],
    )
}

fn auth_from_value(
    value: &CtValue,
    span: Span,
) -> Result<jet_email::SMTPAuth<Vec<u8>>, Diagnostic> {
    if enum_args(value, "SMTPAuth", "None").is_some() {
        return Ok(jet_email::SMTPAuth::None);
    }
    let Some(args) = enum_args(value, "SMTPAuth", "Password") else {
        return Err(unsupported("email SMTPAuth is invalid", span));
    };
    let username = enum_arg(args, "username")
        .ok_or_else(|| unsupported("email SMTPAuth.username is missing", span))
        .and_then(|value| string(value, span).map(str::to_string))?;
    let password = enum_arg(args, "password")
        .ok_or_else(|| unsupported("email SMTPAuth.password is missing", span))
        .and_then(|value| secret(value, span))?;
    Ok(jet_email::SMTPAuth::Password { username, password })
}

fn trust_from_value(value: &CtValue, span: Span) -> Result<jet_email::TLSTrust, Diagnostic> {
    if enum_args(value, "TLSTrust", "System").is_some() {
        return Ok(jet_email::TLSTrust::System);
    }
    let Some(args) = enum_args(value, "TLSTrust", "SystemPlusCa") else {
        return Err(unsupported("email TLSTrust is invalid", span));
    };
    let pem = enum_arg(args, "pem")
        .ok_or_else(|| unsupported("email TLSTrust.pem is missing", span))
        .and_then(|value| bytes(value, span))?;
    Ok(jet_email::TLSTrust::SystemPlusCa { pem })
}

fn dkim_from_value(
    value: &CtValue,
    span: Span,
) -> Result<jet_email::DkimConfig<Vec<u8>>, Diagnostic> {
    let domain = field(value, "DkimConfig", "domain")
        .ok_or_else(|| unsupported("email DkimConfig.domain is missing", span))
        .and_then(|value| string(value, span).map(str::to_string))?;
    let selector = field(value, "DkimConfig", "selector")
        .ok_or_else(|| unsupported("email DkimConfig.selector is missing", span))
        .and_then(|value| string(value, span).map(str::to_string))?;
    let private_key = field(value, "DkimConfig", "private_key")
        .ok_or_else(|| unsupported("email DkimConfig.private_key is missing", span))
        .and_then(|value| secret(value, span))?;
    let signed_headers = field(value, "DkimConfig", "signed_headers")
        .ok_or_else(|| unsupported("email DkimConfig.signed_headers is missing", span))
        .and_then(|value| string_list(value, span))?;
    Ok(jet_email::DkimConfig {
        domain,
        selector,
        private_key,
        signed_headers,
    })
}

fn smtp_config_from_value(
    value: &CtValue,
    span: Span,
) -> Result<jet_email::SMTPConfig<Vec<u8>>, Diagnostic> {
    let host = field(value, "SMTPConfig", "host")
        .ok_or_else(|| unsupported("email SMTPConfig.host is missing", span))
        .and_then(|value| string(value, span).map(str::to_string))?;
    let port = field(value, "SMTPConfig", "port")
        .ok_or_else(|| unsupported("email SMTPConfig.port is missing", span))
        .and_then(|value| int(value, "SMTPConfig.port", span))?;
    let security = match field(value, "SMTPConfig", "security") {
        Some(value) if enum_args(value, "SMTPSecurity", "StartTls").is_some() => {
            jet_email::SMTPSecurity::StartTls
        }
        Some(value) if enum_args(value, "SMTPSecurity", "TLS").is_some() => {
            jet_email::SMTPSecurity::TLS
        }
        _ => return Err(unsupported("email SMTPConfig.security is invalid", span)),
    };
    let auth = field(value, "SMTPConfig", "auth")
        .ok_or_else(|| unsupported("email SMTPConfig.auth is missing", span))
        .and_then(|value| auth_from_value(value, span))?;
    let recipient_policy = match field(value, "SMTPConfig", "recipient_policy") {
        Some(value) if enum_args(value, "RecipientPolicy", "RequireAll").is_some() => {
            jet_email::RecipientPolicy::RequireAll
        }
        Some(value) if enum_args(value, "RecipientPolicy", "DeliverAccepted").is_some() => {
            jet_email::RecipientPolicy::DeliverAccepted
        }
        _ => return Err(unsupported("email SMTPConfig.recipient_policy is invalid", span)),
    };
    let trust = field(value, "SMTPConfig", "trust")
        .ok_or_else(|| unsupported("email SMTPConfig.trust is missing", span))
        .and_then(|value| trust_from_value(value, span))?;
    let limits = field(value, "SMTPConfig", "limits")
        .ok_or_else(|| unsupported("email SMTPConfig.limits is missing", span))
        .and_then(|value| limits_from_value(value, span))?;
    let dkim = field(value, "SMTPConfig", "dkim")
        .ok_or_else(|| unsupported("email SMTPConfig.dkim is missing", span))
        .and_then(|value| optional(value, span, |value| dkim_from_value(value, span)))?
        .map_or(Err(kernel::JetAbsent), Ok);
    Ok(jet_email::SMTPConfig {
        host,
        port,
        security,
        auth,
        recipient_policy,
        trust,
        limits,
        dkim,
    })
}

fn mailer_value(handle: usize) -> CtValue {
    structure("Mailer", vec![("handle", CtValue::Int(handle as i64 + 1))])
}

fn mailer_handle(value: &CtValue) -> Option<usize> {
    match value {
        CtValue::Struct { type_name, fields } if type_name == "Mailer" => fields
            .iter()
            .find_map(|(name, value)| match (name.as_str(), value) {
                ("handle", CtValue::Int(value)) if *value > 0 => {
                    Some(*value as usize - 1)
                }
                _ => None,
            }),
        _ => None,
    }
}

fn ambient_mailers() -> &'static std::sync::Mutex<Vec<Option<jet_email::Mailer>>> {
    use std::sync::OnceLock;
    static MAILERS: OnceLock<std::sync::Mutex<Vec<Option<jet_email::Mailer>>>> = OnceLock::new();
    MAILERS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn store_mailer(mailer: jet_email::Mailer) -> CtValue {
    let mut mailers = ambient_mailers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    mailers.push(Some(mailer));
    mailer_value(mailers.len() - 1)
}

fn wipe_smtp_config(config: &mut jet_email::SMTPConfig<Vec<u8>>, runtime: RuntimeFns) {
    if let jet_email::SMTPAuth::Password { password, .. } = &mut config.auth {
        (runtime.wipe)(password);
    }
    if let Ok(dkim) = &mut config.dkim {
        (runtime.wipe)(&mut dkim.private_key);
    }
}

fn recipient_report_value(report: &jet_email::RecipientReport) -> CtValue {
    structure(
        "RecipientReport",
        vec![
            ("address", address_value(&report.address)),
            ("accepted", CtValue::Bool(report.accepted)),
            ("code", CtValue::Int(report.code)),
            ("message", CtValue::Str(report.message.clone())),
        ],
    )
}

fn send_report_value(report: &jet_email::SendReport) -> CtValue {
    structure(
        "SendReport",
        vec![
            ("server", CtValue::Str(report.server.clone())),
            (
                "accepted",
                CtValue::List(report.accepted.iter().map(recipient_report_value).collect()),
            ),
            (
                "rejected",
                CtValue::List(report.rejected.iter().map(recipient_report_value).collect()),
            ),
            ("response_code", CtValue::Int(report.response_code)),
            ("response", CtValue::Str(report.response.clone())),
            ("accepted_at", CtValue::Str(report.accepted_at.clone())),
        ],
    )
}

pub fn ambient_core_call(
    method: &str,
    args: &[CtValue],
    span: Span,
    runtime: RuntimeFns,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(result) = evaluate(method, args, span) {
        return Some(result);
    }
    let result = match method {
        "smtp" => {
            let mut config = match args.first() {
                Some(value) => match smtp_config_from_value(value, span) {
                    Ok(config) => config,
                    Err(error) => return Some(Err(error)),
                },
                None => return Some(Err(unsupported("email.smtp(): missing config", span))),
            };
            let smtp_result = jet_email::smtp(&config, copy_secret, kernel_runtime(runtime));
            wipe_smtp_config(&mut config, runtime);
            result(smtp_result, store_mailer)
        }
        "smtp_from_env" if args.is_empty() => result(
            jet_email::smtp_from_env(kernel_runtime(runtime)),
            store_mailer,
        ),
        _ => return None,
    };
    Some(Ok(result))
}

pub fn ambient_handle(
    op: &str,
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let method = op.strip_prefix("EmailMethod:")?;
    if method != "send" {
        return evaluate_method(recv, method, args, span);
    }
    let Some(index) = mailer_handle(recv) else {
        return Some(Err(unsupported("email Mailer receiver", span)));
    };
    let message = match args.first() {
        Some(value) => match message_from_value(value, span) {
            Ok(message) => message,
            Err(error) => return Some(Err(error)),
        },
        None => return Some(Err(unsupported("email Mailer.send(): missing message", span))),
    };
    let mut mailers = ambient_mailers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(Some(mut mailer)) = mailers.get_mut(index).map(Option::take) else {
        return Some(Err(unsupported("email Mailer handle", span)));
    };
    let send_result = mailer.send(message);
    mailers[index] = Some(mailer);
    Some(Ok(result(send_result, send_report_value)))
}
