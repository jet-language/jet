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

pub(super) fn evaluate(
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
    jet_email::message(
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
            &format!("email Message is invalid: {}", error_reason(&error)),
            span,
        )
    })
}

fn error_parts(
    error: jet_email::Error,
) -> (&'static str, String, Option<String>, Option<i64>, String) {
    macro_rules! parts {
        ($variant:literal, $operation:expr, $server:expr, $code:expr, $reason:expr) => {
            ($variant, $operation, $server, $code, $reason)
        };
    }
    match error {
        jet_email::Error::Configuration {
            operation,
            server,
            code,
            reason,
        } => parts!("Configuration", operation, server, code, reason),
        jet_email::Error::DNS {
            operation,
            server,
            code,
            reason,
        } => parts!("DNS", operation, server, code, reason),
        jet_email::Error::Connect {
            operation,
            server,
            code,
            reason,
        } => parts!("Connect", operation, server, code, reason),
        jet_email::Error::TLS {
            operation,
            server,
            code,
            reason,
        } => parts!("TLS", operation, server, code, reason),
        jet_email::Error::Auth {
            operation,
            server,
            code,
            reason,
        } => parts!("Auth", operation, server, code, reason),
        jet_email::Error::Protocol {
            operation,
            server,
            code,
            reason,
        } => parts!("Protocol", operation, server, code, reason),
        jet_email::Error::Rejected {
            operation,
            server,
            code,
            reason,
        } => parts!("Rejected", operation, server, code, reason),
        jet_email::Error::Transient {
            operation,
            server,
            code,
            reason,
        } => parts!("Transient", operation, server, code, reason),
        jet_email::Error::TimedOut {
            operation,
            server,
            code,
            reason,
        } => parts!("TimedOut", operation, server, code, reason),
        jet_email::Error::Cancelled {
            operation,
            server,
            code,
            reason,
        } => parts!("Cancelled", operation, server, code, reason),
        jet_email::Error::DeliveryUnknown {
            operation,
            server,
            code,
            reason,
        } => parts!("DeliveryUnknown", operation, server, code, reason),
    }
}

fn error_reason(error: &jet_email::Error) -> &str {
    match error {
        jet_email::Error::Configuration { reason, .. }
        | jet_email::Error::DNS { reason, .. }
        | jet_email::Error::Connect { reason, .. }
        | jet_email::Error::TLS { reason, .. }
        | jet_email::Error::Auth { reason, .. }
        | jet_email::Error::Protocol { reason, .. }
        | jet_email::Error::Rejected { reason, .. }
        | jet_email::Error::Transient { reason, .. }
        | jet_email::Error::TimedOut { reason, .. }
        | jet_email::Error::Cancelled { reason, .. }
        | jet_email::Error::DeliveryUnknown { reason, .. } => reason,
    }
}

fn error_value(error: jet_email::Error) -> CtValue {
    let (variant, operation, server, code, reason) = error_parts(error);
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
