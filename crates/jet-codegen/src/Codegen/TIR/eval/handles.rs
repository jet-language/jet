//! Exhaustive THandleOp dispatch (#777).
use std::collections::HashMap;

use super::browser;
use super::unsupported;
use crate::Codegen::TIR::THandleOp;
use crate::Comptime::Builtins::{
    apply_method, apply_mutating, apply_mutating_with_type, exact_big, exact_int_value,
};
use crate::Comptime::{CtValue, DevSink};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::Type;
use jet_foundation::Reflection::ReflectionField;
use std::sync::{Arc, Mutex};

// The shared Duration kernel is one file included per engine; this instance
// only reads the error reason, so the unused arithmetic entry points stay.
#[allow(dead_code)]
mod duration_kernel {
    include!("../../../Prelude/Core/Duration.rs");
}

#[allow(dead_code)]
mod time_kernel {
    pub(crate) use jet_foundation::Monotonic::jet_time_monotonic_now_ns;
    include!("../../../Prelude/Core/Time.rs");
}

mod path_kernel {
    include!("../../../Prelude/Core/Path.rs");
}

fn civil_time_field<'a>(value: &'a CtValue, wanted: &str) -> Option<&'a CtValue> {
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields.iter().find_map(|(name, value)| {
        let name = name
            .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(name);
        (name == wanted).then_some(value)
    })
}

fn duration_ns_value(value: &CtValue) -> Option<i64> {
    match value {
        CtValue::Struct { fields, .. } => fields.iter().find_map(|(name, value)| {
            (name == "ns").then(|| match value {
                CtValue::Int(value) => Some(*value),
                _ => None,
            })?
        }),
        _ => None,
    }
}

fn duration_scaled_value(
    recv: &CtValue,
    args: &[CtValue],
    divide: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let value = duration_ns_value(recv)
        .ok_or_else(|| unsupported("Duration scaling needs a Duration receiver", span))?;
    let factor = args
        .first()
        .and_then(exact_big)
        .and_then(|value| value.try_i64())
        .ok_or_else(|| unsupported("Duration scaling needs a word-sized Int factor", span))?;
    let value = if divide {
        duration_kernel::jet_duration_kernel_divide(value, factor)
    } else {
        duration_kernel::jet_duration_kernel_scale(value, factor)
    }
    .ok_or_else(|| unsupported(duration_kernel::jet_duration_kernel_scale_error_reason(), span))?;
    Ok(CtValue::Struct {
        type_name: crate::Syntax::DURATION_TYPE.to_string(),
        fields: vec![("ns".to_string(), CtValue::Int(value))],
    })
}

fn civil_time_int(value: &CtValue, field: &str) -> Option<i64> {
    match civil_time_field(value, field) {
        Some(CtValue::Int(value)) => Some(*value),
        _ => None,
    }
}

fn civil_time_date(value: &CtValue) -> Option<time_kernel::JetDate> {
    Some(time_kernel::JetDate::new(
        civil_time_int(value, "year")?,
        civil_time_int(value, "month")?,
        civil_time_int(value, "day")?,
    ))
}

fn civil_time_local_time(value: &CtValue) -> Option<time_kernel::JetLocalTime> {
    Some(time_kernel::JetLocalTime::new(
        civil_time_int(value, "hour")?,
        civil_time_int(value, "minute")?,
        civil_time_int(value, "second")?,
    ))
}

fn civil_time_datetime(value: &CtValue) -> Option<time_kernel::JetDateTime> {
    Some(time_kernel::JetDateTime::from_timestamp_ns(
        civil_time_int(value, "secs")?,
        u32::try_from(civil_time_int(value, "nanos")?).ok()?,
    ))
}

fn civil_time_zone(value: &CtValue) -> Option<time_kernel::JetZone> {
    let CtValue::Str(name) = civil_time_field(value, "name")? else {
        return None;
    };
    time_kernel::JetZone::named(name).ok()
}

fn civil_time_zoned(value: &CtValue) -> Option<time_kernel::JetZonedDateTime> {
    let datetime = civil_time_datetime(civil_time_field(value, "instant")?)?;
    let zone = civil_time_zone(civil_time_field(value, "zone")?)?;
    Some(datetime.in_zone(&zone))
}

enum CivilTimeKernelValue {
    Date(time_kernel::JetDate),
    LocalTime(time_kernel::JetLocalTime),
    DateTime(time_kernel::JetDateTime),
    Instant(i64),
    Zoned(time_kernel::JetZonedDateTime),
    Duration(i64),
}

fn civil_time_kind(value: &CtValue) -> Option<&str> {
    let CtValue::Struct { type_name, .. } = value else {
        return None;
    };
    let type_name = type_name
        .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name);
    matches!(
        type_name,
        "Date"
            | "LocalDate"
            | "LocalTime"
            | "DateTime"
            | "Instant"
            | "ZonedDateTime"
            | "Duration"
    )
    .then_some(type_name)
}

fn canonical_civil_time_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "Date" | "LocalDate" => Some("Date"),
        "LocalTime" => Some("LocalTime"),
        "DateTime" => Some("DateTime"),
        "Instant" => Some("Instant"),
        "ZonedDateTime" => Some("ZonedDateTime"),
        "Duration" => Some("Duration"),
        _ => None,
    }
}

fn civil_time_kernel_value(
    kind: &str,
    value: &CtValue,
    side: &str,
    span: Span,
) -> Result<CivilTimeKernelValue, Diagnostic> {
    let malformed = || unsupported(&format!("malformed {kind} {side}"), span);
    match kind {
        "Date" | "LocalDate" => civil_time_date(value)
            .map(CivilTimeKernelValue::Date)
            .ok_or_else(malformed),
        "LocalTime" => civil_time_local_time(value)
            .map(CivilTimeKernelValue::LocalTime)
            .ok_or_else(malformed),
        "DateTime" => civil_time_datetime(value)
            .map(CivilTimeKernelValue::DateTime)
            .ok_or_else(malformed),
        "Instant" => civil_time_int(value, "start_ns")
            .map(CivilTimeKernelValue::Instant)
            .ok_or_else(malformed),
        "ZonedDateTime" => civil_time_zoned(value)
            .map(CivilTimeKernelValue::Zoned)
            .ok_or_else(malformed),
        "Duration" => civil_time_int(value, "ns")
            .map(CivilTimeKernelValue::Duration)
            .ok_or_else(malformed),
        _ => Err(unsupported(
            &format!("unsupported civil-time comparison `{kind}`"),
            span,
        )),
    }
}

fn civil_time_kernel_pair(
    kind: &str,
    left: &CtValue,
    right: &CtValue,
    span: Span,
) -> Result<(CivilTimeKernelValue, CivilTimeKernelValue), Diagnostic> {
    Ok((
        civil_time_kernel_value(kind, left, "value", span)?,
        civil_time_kernel_value(kind, right, "argument", span)?,
    ))
}

fn civil_time_kernel_order(
    kind: &str,
    left: &CtValue,
    right: &CtValue,
    span: Span,
) -> Result<std::cmp::Ordering, Diagnostic> {
    let (left, right) = civil_time_kernel_pair(kind, left, right, span)?;
    match (left, right) {
        (CivilTimeKernelValue::Date(left), CivilTimeKernelValue::Date(right)) => {
            Ok(left.cmp(&right))
        }
        (
            CivilTimeKernelValue::LocalTime(left),
            CivilTimeKernelValue::LocalTime(right),
        ) => Ok(left.cmp(&right)),
        (
            CivilTimeKernelValue::DateTime(left),
            CivilTimeKernelValue::DateTime(right),
        ) => Ok(left.cmp(&right)),
        (
            CivilTimeKernelValue::Instant(left),
            CivilTimeKernelValue::Instant(right),
        ) => Ok(jet_time_instant_ordering(
            time_kernel::jet_time_instant_compare(left, right),
        )),
        (
            CivilTimeKernelValue::Zoned(left),
            CivilTimeKernelValue::Zoned(right),
        ) => Ok(left.cmp(&right)),
        (
            CivilTimeKernelValue::Duration(left),
            CivilTimeKernelValue::Duration(right),
        ) => Ok(left.cmp(&right)),
        _ => Err(unsupported(
            &format!("cannot compare civil-time `{kind}` values"),
            span,
        )),
    }
}

fn civil_time_kernel_equal(
    kind: &str,
    left: &CtValue,
    right: &CtValue,
    span: Span,
) -> Result<bool, Diagnostic> {
    let (left, right) = civil_time_kernel_pair(kind, left, right, span)?;
    match (left, right) {
        (CivilTimeKernelValue::Date(left), CivilTimeKernelValue::Date(right)) => Ok(left == right),
        (
            CivilTimeKernelValue::LocalTime(left),
            CivilTimeKernelValue::LocalTime(right),
        ) => Ok(left == right),
        (
            CivilTimeKernelValue::DateTime(left),
            CivilTimeKernelValue::DateTime(right),
        ) => Ok(left == right),
        (
            CivilTimeKernelValue::Instant(left),
            CivilTimeKernelValue::Instant(right),
        ) => Ok(left == right),
        (
            CivilTimeKernelValue::Zoned(left),
            CivilTimeKernelValue::Zoned(right),
        ) => Ok(left == right),
        (
            CivilTimeKernelValue::Duration(left),
            CivilTimeKernelValue::Duration(right),
        ) => Ok(left == right),
        _ => Err(unsupported(
            &format!("cannot compare civil-time `{kind}` values"),
            span,
        )),
    }
}

/// Compare two runtime carriers through the shared Prelude time kernel. `None`
/// leaves ordinary list comparison to its existing evaluator path.
pub(super) fn civil_time_order_values(
    left: &CtValue,
    right: &CtValue,
    span: Span,
) -> Result<Option<std::cmp::Ordering>, Diagnostic> {
    let (Some(left_kind), Some(right_kind)) = (civil_time_kind(left), civil_time_kind(right))
    else {
        return Ok(None);
    };
    let (Some(left_kind), Some(right_kind)) = (
        canonical_civil_time_kind(left_kind),
        canonical_civil_time_kind(right_kind),
    ) else {
        return Ok(None);
    };
    if left_kind != right_kind {
        return Ok(None);
    }
    civil_time_kernel_order(left_kind, left, right, span).map(Some)
}

pub(super) fn civil_time_value_kind(value: &CtValue) -> Option<&'static str> {
    civil_time_kind(value).and_then(canonical_civil_time_kind)
}

fn civil_time_ordering(value: std::cmp::Ordering) -> CtValue {
    let variant = match value {
        std::cmp::Ordering::Less => "Less",
        std::cmp::Ordering::Equal => "Equal",
        std::cmp::Ordering::Greater => "Greater",
    };
    CtValue::Enum {
        type_name: crate::Syntax::TYPE_ORDERING.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn eval_civil_time_comparison(
    kind: &str,
    method: &str,
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let rhs = args
        .first()
        .ok_or_else(|| unsupported(&format!("{kind}.{method} argument"), span))?;
    if args.len() != 1 {
        return Err(unsupported(
            &format!("{kind}.{method} expects one argument"),
            span,
        ));
    }

    let result = match method {
        "equal" => CtValue::Bool(civil_time_kernel_equal(kind, recv, rhs, span)?),
        "compare" => civil_time_ordering(civil_time_kernel_order(kind, recv, rhs, span)?),
        _ => {
            return Err(unsupported(
                &format!("unsupported civil-time comparison `{kind}.{method}`"),
                span,
            ));
        }
    };
    Ok(result)
}

fn jet_time_instant_ordering(value: i64) -> std::cmp::Ordering {
    value.cmp(&0)
}

fn handle_op_name(op: &THandleOp) -> String {
    let name = match op {
        THandleOp::HTTPClientMethod { kind, method } => {
            return format!("HTTPClient:{kind}:{method}")
        }
        THandleOp::HTTPServerMethod { kind, method } => {
            return format!("HTTPServer:{kind}:{method}")
        }
        // These subset forms have the same Prelude owner as the named HTTP
        // methods above. Keep one ambient spelling so the evaluator cannot
        // fall through to its context-free E0956 arms while AOT/JIT call the
        // HTTP Prelude directly.
        THandleOp::HTTPClientNew => "HTTPClientNew",
        THandleOp::HTTPReqField(field) => return format!("HTTPServer:HTTPRequest:{field}"),
        THandleOp::HTTPReqHeader => "HTTPServer:HTTPRequest:header",
        THandleOp::HTTPReqParam => "HTTPServer:HTTPRequest:param",
        THandleOp::HTTPReqTrailers => "HTTPServer:HTTPRequest:trailers",
        THandleOp::HTTPRespField(field) => return format!("HTTPServer:HTTPResponse:{field}"),
        THandleOp::HTTPRespHeader => "HTTPClient:HTTPResponse:header",
        THandleOp::HTTPRespTrailers => "HTTPServer:HTTPResponse:trailers",
        THandleOp::HTTPRouterRegister { verb, .. } => {
            return format!("HTTPServer:HTTPRouterRegister:{verb}")
        }
        THandleOp::ArgsSpecFlag => "ArgsSpecFlag",
        THandleOp::ArgsSpecFlagShort => "ArgsSpecFlagShort",
        THandleOp::ArgsSpecOption => "ArgsSpecOption",
        THandleOp::ArgsSpecOptionShort => "ArgsSpecOptionShort",
        THandleOp::ArgsSpecOptionDefault => "ArgsSpecOptionDefault",
        THandleOp::ArgsSpecOptionEnv => "ArgsSpecOptionEnv",
        THandleOp::ArgsSpecOptionInt => "ArgsSpecOptionInt",
        THandleOp::ArgsSpecOptionFloat => "ArgsSpecOptionFloat",
        THandleOp::ArgsSpecOptionChoice => "ArgsSpecOptionChoice",
        THandleOp::ArgsSpecRepeat => "ArgsSpecRepeat",
        THandleOp::ArgsSpecRequiredOption => "ArgsSpecRequiredOption",
        THandleOp::ArgsSpecPositional => "ArgsSpecPositional",
        THandleOp::ArgsSpecDescription => "ArgsSpecDescription",
        THandleOp::ArgsSpecSubcommand => "ArgsSpecSubcommand",
        THandleOp::ArgsSpecVersion => "ArgsSpecVersion",
        THandleOp::ArgsSpecCompletion => "ArgsSpecCompletion",
        THandleOp::ArgsSpecHelp => "ArgsSpecHelp",
        THandleOp::ArgsSpecParse => "ArgsSpecParse",
        THandleOp::ArgsSpecParseOrExit => "ArgsSpecParseOrExit",
        THandleOp::ParsedArgsFlag => "ParsedArgsFlag",
        THandleOp::ParsedArgsOption => "ParsedArgsOption",
        THandleOp::ParsedArgsOptionInt => "ParsedArgsOptionInt",
        THandleOp::ParsedArgsOptionFloat => "ParsedArgsOptionFloat",
        THandleOp::ParsedArgsOptions => "ParsedArgsOptions",
        THandleOp::ParsedArgsSubcommand => "ParsedArgsSubcommand",
        THandleOp::ParsedArgsPositional => "ParsedArgsPositional",
        THandleOp::ProcessSpecMethod { method } => return format!("ProcessSpec:{method}"),
        THandleOp::ProcessChildMethod { method } => return format!("ProcessChild:{method}"),
        THandleOp::ProcessStdinWrite => "ProcessStdin:write",
        THandleOp::EmailMethod { method } => return format!("EmailMethod:{method}"),
        THandleOp::TerminalSessionResize => "TerminalSessionResize",
        THandleOp::DBWithPolicy => "DBWithPolicy",
        THandleOp::ServiceRuntimeSend => "ServiceRuntimeSend",
        THandleOp::ServiceRuntimeRetry => "ServiceRuntimeRetry",
        THandleOp::ServiceRuntimeDeadLetter => "ServiceRuntimeDeadLetter",
        THandleOp::ServiceRuntimeRetain => "ServiceRuntimeRetain",
        THandleOp::ServiceRuntimeCommit => "ServiceRuntimeCommit",
        THandleOp::DBQuery => "DBQuery",
        THandleOp::DBQueryOne => "DBQueryOne",
        THandleOp::DBExecute => "DBExecute",
        THandleOp::DBLive => "DBLive",
        THandleOp::DBBegin => "DBBegin",
        THandleOp::DBCommit => "DBCommit",
        THandleOp::DBRollback => "DBRollback",
        THandleOp::DBClose => "DBClose",
        THandleOp::PathFrom => "PathFrom",
        THandleOp::PathHome => "PathHome",
        THandleOp::PathWriteAtomic => "PathWriteAtomic",
        THandleOp::PathToString => "PathToString",
        THandleOp::PathJoin => "PathJoin",
        THandleOp::PathNormalize => "PathNormalize",
        THandleOp::DBValueInt => "DBValueInt",
        THandleOp::DBValueFloat => "DBValueFloat",
        THandleOp::DBValueText => "DBValueText",
        THandleOp::DBValueBool => "DBValueBool",
        THandleOp::DBValueIsNull => "DBValueIsNull",
        THandleOp::FileReaderReadLine => "FileReaderReadLine",
        THandleOp::FileWriterWriteLine => "FileWriterWriteLine",
        THandleOp::FileWriterFlush => "FileWriterFlush",
        THandleOp::JSONReaderNext => "JSONReaderNext",
        THandleOp::JSONWriterWrite => "JSONWriterWrite",
        THandleOp::JSONWriterFlush => "JSONWriterFlush",
        THandleOp::JSONWriterFinish => "JSONWriterFinish",
        THandleOp::JSONLReaderNext => "JSONLReaderNext",
        THandleOp::JSONLWriterWrite => "JSONLWriterWrite",
        THandleOp::JSONLWriterFlush => "JSONLWriterFlush",
        THandleOp::JSONLWriterFinish => "JSONLWriterFinish",
        THandleOp::CSVReaderNext => "CSVReaderNext",
        THandleOp::CSVWriterWrite => "CSVWriterWrite",
        THandleOp::CSVWriterFlush => "CSVWriterFlush",
        THandleOp::CSVWriterFinish => "CSVWriterFinish",
        THandleOp::XMLReaderNext => "XMLReaderNext",
        THandleOp::XMLWriterWrite => "XMLWriterWrite",
        THandleOp::XMLWriterFlush => "XMLWriterFlush",
        THandleOp::XMLWriterFinish => "XMLWriterFinish",
        THandleOp::CBORReaderNext => "CBORReaderNext",
        THandleOp::CBORWriterWrite => "CBORWriterWrite",
        THandleOp::CBORWriterFlush => "CBORWriterFlush",
        THandleOp::CBORWriterFinish => "CBORWriterFinish",
        THandleOp::TcpListenerAccept => "TcpListenerAccept",
        THandleOp::TcpListenerLocalAddr => "TcpListenerLocalAddr",
        THandleOp::TcpStreamRead => "TcpStreamRead",
        THandleOp::TcpStreamWrite => "TcpStreamWrite",
        THandleOp::TcpStreamPeerAddr => "TcpStreamPeerAddr",
        THandleOp::TcpStreamLocalAddr => "TcpStreamLocalAddr",
        THandleOp::TcpStreamClose => "TcpStreamClose",
        THandleOp::TcpStreamReadBytes => "TcpStreamReadBytes",
        THandleOp::TcpStreamReadText => "TcpStreamReadText",
        THandleOp::TcpStreamWriteBytes => "TcpStreamWriteBytes",
        THandleOp::TcpStreamWriteAllBytes => "TcpStreamWriteAllBytes",
        THandleOp::TcpStreamWriteText => "TcpStreamWriteText",
        THandleOp::TcpStreamShutdown => "TcpStreamShutdown",
        THandleOp::TcpStreamReady => "TcpStreamReady",
        THandleOp::TLSStreamReadDeadline => "TLSStreamReadDeadline",
        THandleOp::TLSStreamWriteAllDeadline => "TLSStreamWriteAllDeadline",
        THandleOp::TLSStreamReady => "TLSStreamReady",
        THandleOp::TLSStreamClose => "TLSStreamClose",
        THandleOp::TLSStreamCloseWrite => "TLSStreamCloseWrite",
        THandleOp::TLSStreamPeerIdentity => "TLSStreamPeerIdentity",
        THandleOp::TLSClientConfigDefault => "TLSClientConfigDefault",
        THandleOp::TLSClientConfigWithAlpn => "TLSClientConfigWithAlpn",
        THandleOp::TLSRootCertificatesFromPem => "TLSRootCertificatesFromPem",
        THandleOp::TLSClientIdentityFromPem => "TLSClientIdentityFromPem",
        THandleOp::TLSClientConfigWithTrust => "TLSClientConfigWithTrust",
        THandleOp::TLSClientConfigWithIdentity => "TLSClientConfigWithIdentity",
        THandleOp::TLSClientConfigWithVersionBounds => "TLSClientConfigWithVersionBounds",
        THandleOp::UdpSocketReady => "UdpSocketReady",
        THandleOp::UdpSocketClose => "UdpSocketClose",
        THandleOp::UdpSocketReceiveDeadline => "UdpSocketReceiveDeadline",
        THandleOp::UdpSocketSendToDeadline => "UdpSocketSendToDeadline",
        // D-LIB-CALLGRANT1=A: route the interpreter through the same ambient
        // Prelude loader used by the Cranelift host.
        THandleOp::ModOnTick => "ModOnTick",
        THandleOp::PluginCall => "PluginCall",
        THandleOp::PluginCallInt => "PluginCallInt",
        THandleOp::PluginCallBool => "PluginCallBool",
        THandleOp::PluginCallText => "PluginCallText",
        THandleOp::ReaderOver => "ReaderOver",
        THandleOp::ReaderReadU8 => "ReaderReadU8",
        THandleOp::ReaderReadI8 => "ReaderReadI8",
        THandleOp::ReaderReadU16Le => "ReaderReadU16Le",
        THandleOp::ReaderReadU16Be => "ReaderReadU16Be",
        THandleOp::ReaderReadI16Le => "ReaderReadI16Le",
        THandleOp::ReaderReadI16Be => "ReaderReadI16Be",
        THandleOp::ReaderReadU32Le => "ReaderReadU32Le",
        THandleOp::ReaderReadU32Be => "ReaderReadU32Be",
        THandleOp::ReaderReadI32Le => "ReaderReadI32Le",
        THandleOp::ReaderReadI32Be => "ReaderReadI32Be",
        THandleOp::ReaderReadU64Le => "ReaderReadU64Le",
        THandleOp::ReaderReadU64Be => "ReaderReadU64Be",
        THandleOp::ReaderReadI64Le => "ReaderReadI64Le",
        THandleOp::ReaderReadI64Be => "ReaderReadI64Be",
        THandleOp::ReaderReadF32Le => "ReaderReadF32Le",
        THandleOp::ReaderReadF32Be => "ReaderReadF32Be",
        THandleOp::ReaderReadF64Le => "ReaderReadF64Le",
        THandleOp::ReaderReadF64Be => "ReaderReadF64Be",
        THandleOp::ReaderPeek => "ReaderPeek",
        THandleOp::ReaderSeek => "ReaderSeek",
        THandleOp::ReaderSkip => "ReaderSkip",
        THandleOp::ReaderTake => "ReaderTake",
        THandleOp::ReaderRemaining => "ReaderRemaining",
        THandleOp::ReaderAtEnd => "ReaderAtEnd",
        _ => "",
    };
    name.to_string()
}

pub(super) fn path_string(recv: &CtValue) -> Option<String> {
    match recv {
        CtValue::Str(s) => Some(s.clone()),
        CtValue::Struct { type_name, fields } if type_name == "Path" => {
            fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
                ("inner", CtValue::Str(s)) => Some(s.clone()),
                _ => None,
            })
        }
        _ => None,
    }
}

fn path_value(s: String) -> CtValue {
    CtValue::Struct {
        type_name: "Path".to_string(),
        fields: vec![("inner".to_string(), CtValue::Str(s))],
    }
}

fn reflect_inner(recv: &CtValue) -> Option<&CtValue> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == "__Reflect" => fields
            .iter()
            .find_map(|(name, value)| (name == "value").then_some(value)),
        _ => None,
    }
}

fn reflected_type_name(recv: &CtValue) -> Option<String> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    if type_name != "__Reflect" {
        return None;
    }
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("type_name", CtValue::Str(name)) => Some(name.clone()),
            _ => None,
        })
        .or_else(|| reflect_inner(recv).map(|value| value.jet_type().leaf_name()))
}

fn reflection_rows<'a>(
    value: &CtValue,
    reflection_fields: Option<&'a HashMap<String, Vec<ReflectionField>>>,
) -> Option<&'a [ReflectionField]> {
    let Some(reflection_fields) = reflection_fields else {
        return None;
    };
    let CtValue::Struct { type_name, .. } = value else {
        return None;
    };
    reflection_fields
        .get(type_name)
        .or_else(|| {
            reflection_fields.get(
                type_name
                    .strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(type_name),
            )
        })
        .map(Vec::as_slice)
}

fn reflect_path_for_type(ty: &Type, reflect_paths: Option<&HashMap<String, String>>) -> String {
    match (ty, reflect_paths) {
        (Type::Named(name) | Type::Apply { name, .. }, Some(paths)) => {
            paths.get(name).cloned().unwrap_or_else(|| ty.name())
        }
        _ => ty.name(),
    }
}

pub(super) fn reflect_value_carrier(
    value: &CtValue,
    declared_type: Option<&Type>,
    reflection_fields: Option<&HashMap<String, Vec<ReflectionField>>>,
    reflect_paths: Option<&HashMap<String, String>>,
    struct_type_params: Option<&HashMap<String, Vec<String>>>,
) -> CtValue {
    let inferred_type;
    let ty = if let Some(ty) = declared_type {
        ty
    } else {
        inferred_type = value.jet_type();
        &inferred_type
    };
    let mut fields = vec![
        ("value".to_string(), value.clone()),
        ("type_name".to_string(), CtValue::Str(ty.leaf_name())),
        (
            "path".to_string(),
            CtValue::Str(reflect_path_for_type(ty, reflect_paths)),
        ),
    ];
    if let Some(rows) = reflection_rows(value, reflection_fields) {
        if let CtValue::Struct {
            fields: value_fields,
            ..
        } = value
        {
            let reflected_fields = rows
                .iter()
                .filter_map(|field| {
                    let field_value = value_fields.iter().find_map(|(actual, value)| {
                        (actual == &field.name
                            || actual.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
                                == Some(field.name.as_str()))
                        .then_some(value)
                    })?;
                    let declared = struct_type_params.map_or_else(
                        || field.ty.clone(),
                        |params| {
                            crate::Codegen::TIR::substitute_reflect_field_type(
                                params, ty, &field.ty,
                            )
                        },
                    );
                    Some(CtValue::Struct {
                        type_name: "__ReflectField".to_string(),
                        fields: vec![
                            ("name".to_string(), CtValue::Str(field.name.clone())),
                            (
                                "value".to_string(),
                                reflect_value_carrier(
                                    field_value,
                                    Some(&declared),
                                    reflection_fields,
                                    reflect_paths,
                                    struct_type_params,
                                ),
                            ),
                        ],
                    })
                })
                .collect();
            fields.push((
                "__reflected_fields".to_string(),
                CtValue::List(reflected_fields),
            ));
        }
    }
    CtValue::Struct {
        type_name: "__Reflect".to_string(),
        fields,
    }
}

fn reflect_path(recv: &CtValue) -> Option<CtValue> {
    match recv {
        CtValue::Struct { type_name, fields } if type_name == "__Reflect" => fields
            .iter()
            .find_map(|(name, value)| (name == "path").then_some(value.clone())),
        _ => None,
    }
}

fn reflect_handle(recv: &CtValue, method: &str, span: Span) -> Result<CtValue, Diagnostic> {
    match method {
        "type_name" => reflected_type_name(recv)
            .map(CtValue::Str)
            .ok_or_else(|| unsupported("reflect value", span)),
        "path" => reflect_path(recv)
            .or_else(|| reflected_type_name(recv).map(CtValue::Str))
            .ok_or_else(|| unsupported("reflect value", span)),
        // `Value.display()` needs the evaluator's user-function table. The
        // HandleMethod evaluator routes it through `EvalCtx::show_value`; a
        // context-free fallback would silently use JetShow instead.
        "display" => Err(unsupported("reflect display evaluator", span)),
        // D-ANY-JAI1: `.fields()` is populated for a struct receiver and empty
        // for anything else. A carrier built without `__reflected_fields` means
        // ONE of those two things, and the receiver says which: a non-struct
        // value has no rows by construction (`reflect.of(42).fields()` is
        // legitimately empty), while a struct value whose rows were never
        // registered means this evaluator has no reflection model. The fragment
        // evaluator behind comptime folding is assembled without the item walk
        // that registers them (`eval::eval_expr_hook` / `eval_block_hook`), so
        // answering "empty" there would bake a WRONG constant into the program
        // — the declared fields are simply invisible. Decline instead, and the
        // call stays a runtime `reflect.of(...)` the AOT/JIT emitters build
        // from the registered rows.
        "fields" => match recv {
            CtValue::Struct { type_name, fields } if type_name == "__Reflect" => fields
                .iter()
                .find_map(|(name, value)| (name == "__reflected_fields").then_some(value.clone()))
                .map_or_else(
                    || match reflect_inner(recv) {
                        Some(CtValue::Struct { .. }) => {
                            Err(unsupported("reflect fields evaluator", span))
                        }
                        _ => Ok(CtValue::List(Vec::new())),
                    },
                    Ok,
                ),
            _ => Err(unsupported("reflect value", span)),
        },
        "name" => match recv {
            CtValue::Struct { type_name, fields } if type_name == "__ReflectField" => fields
                .iter()
                .find_map(|(name, value)| (name == "name").then_some(value.clone()))
                .ok_or_else(|| unsupported("reflect field", span)),
            _ => Err(unsupported("reflect field", span)),
        },
        "value" => match recv {
            CtValue::Struct { type_name, fields } if type_name == "__ReflectField" => fields
                .iter()
                .find_map(|(name, value)| (name == "value").then_some(value.clone()))
                .ok_or_else(|| unsupported("reflect field", span)),
            _ => Err(unsupported("reflect field", span)),
        },
        _ => Err(unsupported("reflect handle", span)),
    }
}

fn db_value_result(recv: &CtValue, want: &str, span: Span) -> Result<CtValue, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = recv
    else {
        return Err(unsupported("DBValue accessor receiver", span));
    };
    if type_name != "DBValue" {
        return Err(unsupported("DBValue accessor receiver", span));
    }
    let ok = |v| Ok(CtValue::Present(Box::new(v)));
    let err = |msg: String| Ok(CtValue::failed(Box::new(CtValue::Str(msg))));
    match (want, variant.as_str(), args.as_slice()) {
        ("is_null", "Null", _) => Ok(CtValue::Bool(true)),
        ("is_null", _, _) => Ok(CtValue::Bool(false)),
        ("int", "Int", [(_, CtValue::Int(n))]) => ok(CtValue::Int(*n)),
        ("float", "Float", [(_, CtValue::Float(f))]) => ok(CtValue::Float(f.clone())),
        ("float", "Int", [(_, CtValue::Int(n))]) => {
            ok(CtValue::Float(crate::AST::CtFloat::f64(*n as f64)))
        }
        ("text", "Text", [(_, CtValue::Str(s))]) => ok(CtValue::Str(s.clone())),
        ("bool", "Bool", [(_, CtValue::Bool(b))]) => ok(CtValue::Bool(*b)),
        ("int", _, _) => err(format!("expected an int, got {variant}")),
        ("float", _, _) => err(format!("expected a float, got {variant}")),
        ("text", _, _) => err(format!("expected text, got {variant}")),
        ("bool", _, _) => err(format!("expected a bool, got {variant}")),
        _ => Err(unsupported("DBValue accessor", span)),
    }
}

fn datatree_int_result(recv: &CtValue) -> CtValue {
    let result = match recv {
        CtValue::Enum { variant, args, .. } => match (variant.as_str(), args.as_slice()) {
            ("Int", [(_, value @ (CtValue::Int(_) | CtValue::BigInt(_)))]) => {
                Ok(value.clone())
            }
            // Typed-JSON lexical `Number` carrier: same projection as the
            // Prelude accessor (DataTree.rs `int()`), so a hand `decode`
            // reads one protocol on every tier.
            ("Number", [(_, CtValue::Str(text))]) => {
                jet_foundation::Numeric::CtBigInt::from_json_number(text)
                    .map(exact_int_value)
                    .map_err(|_| {
                        format!(
                            "expected int, got {}",
                            crate::Comptime::render_datatree_for_tir(recv)
                        )
                    })
            }
            _ => Err(format!(
                "expected int, got {}",
                crate::Comptime::render_datatree_for_tir(recv)
            )),
        },
        _ => Err(format!(
            "expected int, got {}",
            crate::Comptime::render_datatree_for_tir(recv)
        )),
    };
    match result {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(reason) => CtValue::failed(Box::new(decode_error(String::new(), reason))),
    }
}

fn decode_error(path: String, reason: String) -> CtValue {
    CtValue::List(vec![CtValue::Struct {
        type_name: "FieldError".to_string(),
        fields: vec![
            ("path".to_string(), CtValue::Str(path)),
            ("reason".to_string(), CtValue::Str(reason)),
        ],
    }])
}

fn datatree_payload<'a>(recv: &'a CtValue, variant: &str) -> Option<&'a CtValue> {
    match recv {
        CtValue::Enum {
            type_name,
            variant: actual,
            args,
        } if (type_name == "JSON" || type_name == "DataTree") && actual == variant => {
            args.first().map(|(_, value)| value)
        }
        _ => None,
    }
}

fn datatree_field_result(recv: &CtValue, args: &[CtValue]) -> CtValue {
    let name = match args.first() {
        Some(CtValue::Str(name)) => name,
        _ => {
            return CtValue::failed(Box::new(decode_error(
                String::new(),
                "field name must be Text".to_string(),
            )));
        }
    };
    let result = match datatree_payload(recv, "Object") {
        Some(CtValue::Map(fields)) => fields
            .get(&crate::AST::CtKey::Str(name.clone()))
            .cloned()
            .ok_or_else(|| decode_error(name.clone(), format!("field `{name}` not found"))),
        Some(CtValue::Struct { type_name, fields }) if type_name == "JSONObject" => fields
            .iter()
            .find_map(|(field, value)| (field == name).then(|| value.clone()))
            .ok_or_else(|| decode_error(name.clone(), format!("field `{name}` not found"))),
        _ => Err(decode_error(
            name.clone(),
            format!(
                "expected object, got {}",
                crate::Comptime::render_datatree_for_tir(recv)
            ),
        )),
    };
    match result {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(error) => CtValue::failed(Box::new(error)),
    }
}

fn datatree_at_result(recv: &CtValue, args: &[CtValue]) -> CtValue {
    let index = match args.first() {
        Some(CtValue::Int(index)) => *index,
        _ => -1,
    };
    let result = match datatree_payload(recv, "Array") {
        Some(CtValue::List(items)) => {
            let resolved = if index < 0 {
                items.len().wrapping_sub(index.unsigned_abs() as usize)
            } else {
                index as usize
            };
            items.get(resolved).cloned().ok_or_else(|| {
                decode_error(
                    format!("[{index}]"),
                    format!("index {index} out of bounds (len {})", items.len()),
                )
            })
        }
        _ => Err(decode_error(
            format!("[{index}]"),
            format!(
                "expected array, got {}",
                crate::Comptime::render_datatree_for_tir(recv)
            ),
        )),
    };
    match result {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(error) => CtValue::failed(Box::new(error)),
    }
}

fn datatree_scalar_result(recv: &CtValue, variant: &str, name: &str) -> CtValue {
    let value = match (variant, datatree_payload(recv, variant)) {
        ("Float", Some(value)) => Some(value.clone()),
        ("Float", None) => datatree_payload(recv, "Int")
            .and_then(|value| match value {
                CtValue::Int(value) => {
                    Some(CtValue::Float(crate::AST::CtFloat::f64(*value as f64)))
                }
                _ => None,
            })
            .or_else(|| match datatree_payload(recv, "Number") {
                // Typed-JSON lexical carrier, projected as in DataTree.rs `float()`.
                Some(CtValue::Str(text)) => text
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(|value| CtValue::Float(crate::AST::CtFloat::f64(value))),
                _ => None,
            }),
        // Typed-JSON text carrier reads as ordinary text (DataTree.rs `text()`).
        ("Text", None) => datatree_payload(recv, "TypedText").cloned(),
        (_, Some(value)) => Some(value.clone()),
        _ => None,
    };
    match value {
        Some(value) => CtValue::Present(Box::new(value)),
        None => CtValue::failed(Box::new(decode_error(
            String::new(),
            format!(
                "expected {name}, got {}",
                crate::Comptime::render_datatree_for_tir(recv)
            ),
        ))),
    }
}

fn stream_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match value {
        CtValue::Bytes(bytes) => Ok(bytes.clone()),
        CtValue::List(items) => items
            .iter()
            .map(|item| match item {
                CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                _ => Err(unsupported("stream.write_bytes expects bytes", span)),
            })
            .collect(),
        _ => Err(unsupported("stream.write_bytes expects bytes", span)),
    }
}

fn stream_write(
    sink: Option<&Arc<Mutex<DevSink>>>,
    to_stderr: bool,
    text: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let direct = if to_stderr {
        super::term_semantics::jet_term_stderr_is_program_stream()
    } else {
        super::term_semantics::jet_term_stdout_is_program_stream()
    };
    if direct {
        let result = if to_stderr {
            super::term_semantics::jet_term_write_stderr(text, false)
        } else {
            super::term_semantics::jet_term_write_stdout(text, false)
        };
        result.map_err(|error| unsupported(&format!("write stream: {error}"), span))?;
    } else {
        let Some(sink) = sink else {
            return Err(unsupported("stream output without a runtime sink", span));
        };
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        if to_stderr {
            sink.stderr.push_str(text);
        } else {
            sink.stdout.push_str(text);
        }
    }
    Ok(CtValue::Present(Box::new(CtValue::Unit)))
}

fn stream_flush(
    sink: Option<&Arc<Mutex<DevSink>>>,
    to_stderr: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let direct = if to_stderr {
        super::term_semantics::jet_term_stderr_is_program_stream()
    } else {
        super::term_semantics::jet_term_stdout_is_program_stream()
    };
    if direct {
        let result = if to_stderr {
            super::term_semantics::jet_term_write_stderr("", true)
        } else {
            super::term_semantics::jet_term_write_stdout("", true)
        };
        result.map_err(|error| unsupported(&format!("flush stream: {error}"), span))?;
    } else if sink.is_none() {
        return Err(unsupported("stream flush without a runtime sink", span));
    }
    Ok(CtValue::Present(Box::new(CtValue::Unit)))
}

pub(super) fn eval_handle_with_type_and_sink(
    op: &THandleOp,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
    resolved_ret: Option<&Type>,
    sink: Option<&Arc<Mutex<DevSink>>>,
) -> Result<CtValue, Diagnostic> {
    if let Some(result) = browser::handle(op, recv, args, span) {
        return result;
    }
    let op_name = handle_op_name(op);
    // A nominal Reader/Writer uses the Prelude's IOError conversion, while
    // the concrete core.net methods expose NetError. Keep that distinction at
    // the evaluator boundary so both surfaces call the same ambient Prelude
    // adapter without changing the user-facing operation table.
    let io_op_name = match (op, resolved_ret) {
        (THandleOp::TcpStreamReadBytes, Some(Type::Result { err, .. }))
            if matches!(err.as_ref(), Type::Named(name) if name == "IOError")
                && args.len() == 1 =>
        {
            "TcpStreamReadBytesIO"
        }
        (THandleOp::TcpStreamWriteBytes, Some(Type::Result { err, .. }))
            if matches!(err.as_ref(), Type::Named(name) if name == "IOError")
                && args.len() == 1 =>
        {
            "TcpStreamWriteBytesIO"
        }
        (THandleOp::TcpStreamWriteAllBytes, Some(Type::Result { err, .. }))
            if matches!(err.as_ref(), Type::Named(name) if name == "IOError")
                && args.len() == 1 =>
        {
            "TcpStreamWriteAllBytesIO"
        }
        _ => op_name.as_str(),
    };
    // UDP readiness, deadline I/O, and close are runtime-only operations. Keep
    // them on the ambient bridge so the interpreter marshals through the same
    // Prelude socket operation as AOT and JIT, rather than growing local policy.
    let udp_ambient = matches!(
        op,
        THandleOp::UdpSocketReady
            | THandleOp::UdpSocketClose
            | THandleOp::UdpSocketReceiveDeadline
            | THandleOp::UdpSocketSendToDeadline
    );
    if udp_ambient {
        if let Some(result) = crate::Comptime::try_ambient_handle(io_op_name, recv, args, span) {
            return result;
        }
    } else if !op_name.is_empty() {
        if let Some(result) = crate::Comptime::eval_args_handle(&op_name, recv, args, span) {
            return result;
        }
        if let Some(result) = crate::Comptime::try_ambient_handle(io_op_name, recv, args, span) {
            return result;
        }
    }
    match op {
        THandleOp::PathFrom => {
            let s = path_string(recv)
                .or_else(|| args.first().and_then(path_string))
                .ok_or_else(|| unsupported("Path.from expects text", span))?;
            Ok(path_value(s))
        }
        THandleOp::PathHome => Ok(path_value(path_kernel::jet_std_path_home())),
        THandleOp::PathToString => {
            let s = path_string(recv).ok_or_else(|| unsupported("Path.to_string", span))?;
            Ok(CtValue::Str(s))
        }
        THandleOp::PathJoin => {
            let base = path_string(recv).ok_or_else(|| unsupported("Path.join recv", span))?;
            let part = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Err(unsupported("Path.join expects text", span)),
            };
            Ok(path_value(
                std::path::Path::new(&base)
                    .join(part)
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        THandleOp::PathWriteAtomic => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.write_atomic", span))?;
            let bytes = match args.first() {
                Some(CtValue::Bytes(b)) => b.clone(),
                Some(CtValue::List(items)) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            CtValue::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                            _ => return Err(unsupported("Path.write_atomic bytes", span)),
                        }
                    }
                    out
                }
                _ => return Err(unsupported("Path.write_atomic expects bytes", span)),
            };
            match std::fs::write(&path, bytes) {
                Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::failed(Box::new(CtValue::Str(e.to_string())))),
            }
        }
        THandleOp::DBValueInt => db_value_result(recv, "int", span),
        THandleOp::DBValueFloat => db_value_result(recv, "float", span),
        THandleOp::DBValueText => db_value_result(recv, "text", span),
        THandleOp::DBValueBool => db_value_result(recv, "bool", span),
        THandleOp::DBValueIsNull => db_value_result(recv, "is_null", span),
        // Runtime-tier only (jet-jit ambient); comptime has no SQLite host.
        THandleOp::DBWithPolicy => Err(unsupported("handle `DBWithPolicy`", span)),
        THandleOp::ServiceRuntimeSend => {
            crate::Comptime::ServicesLite::apply_runtime_method(recv, "send", args, span)
        }
        THandleOp::ServiceRuntimeRetry => {
            crate::Comptime::ServicesLite::apply_runtime_method(recv, "retry", args, span)
        }
        THandleOp::ServiceRuntimeDeadLetter => {
            crate::Comptime::ServicesLite::apply_runtime_method(recv, "dead_letter", args, span)
        }
        THandleOp::ServiceRuntimeRetain => {
            crate::Comptime::ServicesLite::apply_runtime_method(recv, "retain", args, span)
        }
        THandleOp::ServiceRuntimeCommit => {
            crate::Comptime::ServicesLite::apply_runtime_method(recv, "commit", args, span)
        }
        THandleOp::DBQuery => Err(unsupported("handle `DBQuery`", span)),
        THandleOp::DBQueryOne => Err(unsupported("handle `DBQueryOne`", span)),
        THandleOp::DBExecute => Err(unsupported("handle `DBExecute`", span)),
        THandleOp::DBLive => Err(unsupported("handle `DBLive`", span)),
        THandleOp::DBBegin => Err(unsupported("handle `DBBegin`", span)),
        THandleOp::DBCommit => Err(unsupported("handle `DBCommit`", span)),
        THandleOp::DBRollback => Err(unsupported("handle `DBRollback`", span)),
        THandleOp::DBClose => Err(unsupported("handle `DBClose`", span)),
        THandleOp::DurationNew { unit, float } => duration_new(recv, unit, *float, span),
        THandleOp::DurationScale => duration_scaled_value(recv, args, false, span),
        THandleOp::DurationDivide => duration_scaled_value(recv, args, true, span),
        THandleOp::ClockNow => apply_method(recv, "now", args.to_vec(), span),
        THandleOp::ClockTick => {
            apply_mutating_with_type(recv, "tick", args.to_vec(), span, resolved_ret)
        }
        THandleOp::ClockAdvance => {
            apply_mutating_with_type(recv, "advance", args.to_vec(), span, resolved_ret)
        }
        THandleOp::ClockWait => {
            apply_mutating_with_type(recv, "wait", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngInt => {
            apply_mutating_with_type(recv, "int", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngFloat => {
            apply_mutating_with_type(recv, "float", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngFloatRange => {
            apply_mutating_with_type(recv, "float_range", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngBool => {
            apply_mutating_with_type(recv, "bool", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngBoolP => {
            apply_mutating_with_type(recv, "bool", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngNormal => {
            apply_mutating_with_type(recv, "normal", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngExponential => {
            apply_mutating_with_type(recv, "exponential", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngBytes => {
            apply_mutating_with_type(recv, "bytes", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngSplit => {
            apply_mutating_with_type(recv, "split", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngPick => {
            apply_mutating_with_type(recv, "pick", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngWeightedPick => {
            apply_mutating_with_type(recv, "weighted_pick", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngSample => {
            apply_mutating_with_type(recv, "sample", args.to_vec(), span, resolved_ret)
        }
        THandleOp::RngShuffle => {
            let mut state = match recv {
                CtValue::Struct { type_name, fields } if type_name == crate::Syntax::RNG_TYPE => {
                    fields
                        .iter()
                        .find_map(|(name, value)| match (name.as_str(), value) {
                            ("state", CtValue::Int(state)) => Some(*state as u64),
                            _ => None,
                        })
                        .unwrap_or(0)
                }
                _ => {
                    return Err(unsupported("Rng.shuffle receiver", span));
                }
            };
            let value = crate::Comptime::apply_seeded_rng_method_with_type(
                &mut state,
                "shuffle",
                args,
                span,
                resolved_ret,
            )?;
            *recv = CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(state as i64))],
            };
            Ok(value)
        }
        THandleOp::FakeLocale => crate::Comptime::apply_fake_method(recv, "locale", args, span),
        THandleOp::FakeName => crate::Comptime::apply_fake_method(recv, "name", args, span),
        THandleOp::FakeEmail => crate::Comptime::apply_fake_method(recv, "email", args, span),
        THandleOp::FakeHost => crate::Comptime::apply_fake_method(recv, "host", args, span),
        THandleOp::FakeAddress => crate::Comptime::apply_fake_method(recv, "address", args, span),
        THandleOp::SolverNew => {
            let seed = match recv {
                CtValue::Int(n) => *n,
                _ => {
                    return Err(unsupported("Solver.new expects an Int seed", span));
                }
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::SOLVER_TYPE.to_string(),
                fields: vec![
                    ("seed".to_string(), CtValue::Int(seed)),
                    ("checked".to_string(), CtValue::Int(0)),
                    ("failures".to_string(), CtValue::Int(0)),
                ],
            })
        }
        THandleOp::SolverRequire => apply_mutating(recv, "require", args.to_vec(), span),
        THandleOp::SolverFailureCount => apply_method(recv, "failure_count", args.to_vec(), span),
        THandleOp::SolverStatus => apply_method(recv, "status", args.to_vec(), span),
        THandleOp::MeasurementMethod { method } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::CivilTimeMethod { kind, method }
            if matches!(method.as_str(), "equal" | "compare") =>
        {
            eval_civil_time_comparison(kind, method, recv, args, span)
        }
        THandleOp::CivilTimeMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::PreciseMethod { type_name, method } => {
            apply_method(recv, method, args.to_vec(), span).or_else(|_| {
                Err(unsupported(
                    &format!("precise `{type_name}.{method}`"),
                    span,
                ))
            })
        }
        THandleOp::DurationIn { .. } => apply_method(recv, "in", args.to_vec(), span),
        THandleOp::DurationIsZero => {
            let ns = match recv {
                CtValue::Struct { fields, .. } => fields
                    .iter()
                    .find(|(n, _)| n == "ns")
                    .and_then(|(_, v)| match v {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap_or(0),
                _ => 0,
            };
            Ok(CtValue::Bool(duration_kernel::jet_duration_kernel_is_zero(
                ns,
            )))
        }
        THandleOp::DurationTotalSeconds => {
            let ns = match recv {
                CtValue::Struct { fields, .. } => fields
                    .iter()
                    .find(|(n, _)| n == "ns")
                    .and_then(|(_, v)| match v {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap_or(0),
                _ => 0,
            };
            Ok(CtValue::Int(
                duration_kernel::jet_duration_kernel_total_seconds(ns),
            ))
        }
        THandleOp::DurationDifference => {
            let a = match recv {
                CtValue::Struct { fields, .. } => fields
                    .iter()
                    .find(|(n, _)| n == "ns")
                    .and_then(|(_, v)| match v {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap_or(0),
                _ => 0,
            };
            let b = match args.first() {
                Some(CtValue::Struct { fields, .. }) => fields
                    .iter()
                    .find(|(n, _)| n == "ns")
                    .and_then(|(_, v)| match v {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap_or(0),
                _ => 0,
            };
            Ok(CtValue::Struct {
                type_name: "Duration".to_string(),
                fields: vec![(
                    "ns".to_string(),
                    CtValue::Int(duration_kernel::jet_duration_kernel_difference(a, b)),
                )],
            })
        }
        THandleOp::DurationSecondsValue => {
            let ns = match recv {
                CtValue::Struct { fields, .. } => fields
                    .iter()
                    .find(|(n, _)| n == "ns")
                    .and_then(|(_, v)| match v {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap_or(0),
                _ => 0,
            };
            Ok(CtValue::Float(crate::AST::CtFloat::f64(
                duration_kernel::jet_duration_kernel_seconds_value(ns),
            )))
        }
        THandleOp::FileReaderReadLine => Err(unsupported("handle `FileReaderReadLine`", span)),
        THandleOp::FileWriterWriteLine => Err(unsupported("handle `FileWriterWriteLine`", span)),
        THandleOp::FileWriterFlush => Err(unsupported("handle `FileWriterFlush`", span)),
        THandleOp::JSONReaderNext => Err(unsupported("handle `JSONReaderNext`", span)),
        THandleOp::JSONWriterWrite => Err(unsupported("handle `JSONWriterWrite`", span)),
        THandleOp::JSONWriterFlush => Err(unsupported("handle `JSONWriterFlush`", span)),
        THandleOp::JSONWriterFinish => Err(unsupported("handle `JSONWriterFinish`", span)),
        THandleOp::JSONLReaderNext => Err(unsupported("handle `JSONLReaderNext`", span)),
        THandleOp::JSONLWriterWrite => Err(unsupported("handle `JSONLWriterWrite`", span)),
        THandleOp::JSONLWriterFlush => Err(unsupported("handle `JSONLWriterFlush`", span)),
        THandleOp::JSONLWriterFinish => Err(unsupported("handle `JSONLWriterFinish`", span)),
        THandleOp::CSVReaderNext => Err(unsupported("handle `CSVReaderNext`", span)),
        THandleOp::DataStreamNext => Err(unsupported("handle `DataStreamNext`", span)),
        THandleOp::XMLReaderNext => Err(unsupported("handle `XMLReaderNext`", span)),
        THandleOp::XMLWriterWrite => Err(unsupported("handle `XMLWriterWrite`", span)),
        THandleOp::XMLWriterFlush => Err(unsupported("handle `XMLWriterFlush`", span)),
        THandleOp::XMLWriterFinish => Err(unsupported("handle `XMLWriterFinish`", span)),
        THandleOp::CSVWriterWrite => Err(unsupported("handle `CSVWriterWrite`", span)),
        THandleOp::CSVWriterFlush => Err(unsupported("handle `CSVWriterFlush`", span)),
        THandleOp::CSVWriterFinish => Err(unsupported("handle `CSVWriterFinish`", span)),
        THandleOp::CBORReaderNext => Err(unsupported("handle `CBORReaderNext`", span)),
        THandleOp::CBORWriterWrite => Err(unsupported("handle `CBORWriterWrite`", span)),
        THandleOp::CBORWriterFlush => Err(unsupported("handle `CBORWriterFlush`", span)),
        THandleOp::CBORWriterFinish => Err(unsupported("handle `CBORWriterFinish`", span)),
        THandleOp::StdinReadLine => Err(unsupported("handle `StdinReadLine`", span)),
        THandleOp::StdoutWrite => {
            let Some(CtValue::Str(text)) = args.first() else {
                return Err(unsupported("stream.write expects text", span));
            };
            stream_write(sink, false, text, span)
        }
        THandleOp::StdoutWriteLine => {
            let Some(CtValue::Str(text)) = args.first() else {
                return Err(unsupported("stream.write_line expects text", span));
            };
            stream_write(sink, false, &format!("{text}\n"), span)
        }
        THandleOp::StdoutWriteBytes => {
            let Some(value) = args.first() else {
                return Err(unsupported("stream.write_bytes expects bytes", span));
            };
            let bytes = stream_bytes(value, span)?;
            stream_write(sink, false, &String::from_utf8_lossy(&bytes), span)
        }
        THandleOp::StdoutFlush => stream_flush(sink, false, span),
        THandleOp::StdoutIsTty => Ok(CtValue::Bool(
            super::term_semantics::jet_term_stdout_is_terminal(),
        )),
        THandleOp::StderrWrite => {
            let Some(CtValue::Str(text)) = args.first() else {
                return Err(unsupported("stream.write expects text", span));
            };
            stream_write(sink, true, text, span)
        }
        THandleOp::StderrWriteLine => {
            let Some(CtValue::Str(text)) = args.first() else {
                return Err(unsupported("stream.write_line expects text", span));
            };
            stream_write(sink, true, &format!("{text}\n"), span)
        }
        THandleOp::StderrWriteBytes => {
            let Some(value) = args.first() else {
                return Err(unsupported("stream.write_bytes expects bytes", span));
            };
            let bytes = stream_bytes(value, span)?;
            stream_write(sink, true, &String::from_utf8_lossy(&bytes), span)
        }
        THandleOp::StderrFlush => stream_flush(sink, true, span),
        THandleOp::StderrIsTty => Ok(CtValue::Bool(
            super::term_semantics::jet_term_stderr_is_terminal(),
        )),
        THandleOp::StopwatchElapsedMillis => {
            let CtValue::Struct { type_name, fields } = recv else {
                return Err(unsupported("StopwatchElapsedMillis receiver", span));
            };
            if type_name != "Stopwatch" {
                return Err(unsupported("StopwatchElapsedMillis receiver", span));
            }
            let start_ms = fields
                .iter()
                .find_map(|(name, value)| {
                    (name == "start_ms").then_some(match value {
                        CtValue::Int(value) => Some(*value),
                        _ => None,
                    })
                })
                .flatten()
                .ok_or_else(|| unsupported("StopwatchElapsedMillis start", span))?;
            Ok(CtValue::Int(
                time_kernel::jet_time_monotonic_now_ns()
                    .saturating_div(1_000_000)
                    .saturating_sub(start_ms),
            ))
        }
        THandleOp::TestSuiteRun => {
            let CtValue::Struct { type_name, fields } = recv else {
                return Err(unsupported("TestSuiteRun receiver", span));
            };
            if type_name != "TestSuite" {
                return Err(unsupported("TestSuiteRun receiver", span));
            }
            let iteration = fields
                .iter()
                .find_map(|(name, value)| {
                    (name == "iteration").then_some(match value {
                        CtValue::Int(value) => *value,
                        _ => 0,
                    })
                })
                .unwrap_or(0);
            let result = fields
                .iter()
                .find_map(|(name, value)| {
                    (name == "result").then_some(match value {
                        CtValue::Int(value) => *value,
                        _ => 0,
                    })
                })
                .unwrap_or(0);
            let mut suite = crate::command_suite::JetTestSuite {
                iteration,
                result,
                runner: None,
            };
            let status = crate::command_suite::jet_test_suite_run(&mut suite);
            if let CtValue::Struct { fields, .. } = recv {
                for (name, value) in fields.iter_mut() {
                    match name.as_str() {
                        "iteration" => *value = CtValue::Int(suite.iteration),
                        "result" => *value = CtValue::Int(suite.result),
                        _ => {}
                    }
                }
            }
            Ok(CtValue::Int(status))
        }
        THandleOp::GameSceneNew => Err(unsupported("handle `GameSceneNew`", span)),
        THandleOp::GameReplayRecord => Err(unsupported("handle `GameReplayRecord`", span)),
        THandleOp::GameBackendHeadless => Err(unsupported("handle `GameBackendHeadless`", span)),
        THandleOp::GameBackendShouldContinue => {
            Err(unsupported("handle `GameBackendShouldContinue`", span))
        }
        THandleOp::GameBackendPresent => Err(unsupported("handle `GameBackendPresent`", span)),
        THandleOp::GameSceneOnFrame => Err(unsupported("handle `GameSceneOnFrame`", span)),
        THandleOp::GameSceneComponent => Err(unsupported("handle `GameSceneComponent`", span)),
        THandleOp::GameSceneQuery => Err(unsupported("handle `GameSceneQuery`", span)),
        THandleOp::GameAssetsImage => Err(unsupported("handle `GameAssetsImage`", span)),
        THandleOp::GameAssetsSound => Err(unsupported("handle `GameAssetsSound`", span)),
        THandleOp::GameInputBind => Err(unsupported("handle `GameInputBind`", span)),
        THandleOp::GameInputPressed => Err(unsupported("handle `GameInputPressed`", span)),
        THandleOp::TcpListenerAccept => Err(unsupported("handle `TcpListenerAccept`", span)),
        THandleOp::TcpListenerLocalAddr => Err(unsupported("handle `TcpListenerLocalAddr`", span)),
        THandleOp::TcpStreamRead => Err(unsupported("handle `TcpStreamRead`", span)),
        THandleOp::TcpStreamWrite => Err(unsupported("handle `TcpStreamWrite`", span)),
        THandleOp::TcpStreamPeerAddr => Err(unsupported("handle `TcpStreamPeerAddr`", span)),
        THandleOp::TcpStreamLocalAddr => Err(unsupported("handle `TcpStreamLocalAddr`", span)),
        THandleOp::TcpStreamClose => Err(unsupported("handle `TcpStreamClose`", span)),
        THandleOp::TcpStreamReadBytes => Err(unsupported("handle `TcpStreamReadBytes`", span)),
        THandleOp::TcpStreamReadText => Err(unsupported("handle `TcpStreamReadText`", span)),
        THandleOp::TcpStreamWriteBytes => Err(unsupported("handle `TcpStreamWriteBytes`", span)),
        THandleOp::TcpStreamWriteAllBytes => {
            Err(unsupported("handle `TcpStreamWriteAllBytes`", span))
        }
        THandleOp::TcpStreamWriteText => Err(unsupported("handle `TcpStreamWriteText`", span)),
        THandleOp::TcpStreamShutdown => Err(unsupported("handle `TcpStreamShutdown`", span)),
        THandleOp::TcpStreamReady => Err(unsupported("handle `TcpStreamReady`", span)),
        THandleOp::UdpSocketReady => Err(unsupported("handle `UdpSocketReady`", span)),
        THandleOp::UdpSocketClose => Err(unsupported("handle `UdpSocketClose`", span)),
        THandleOp::UdpSocketReceiveDeadline => {
            Err(unsupported("handle `UdpSocketReceiveDeadline`", span))
        }
        THandleOp::UdpSocketSendToDeadline => {
            Err(unsupported("handle `UdpSocketSendToDeadline`", span))
        }
        THandleOp::UnixListenerAcceptDeadline => {
            Err(unsupported("handle `UnixListenerAcceptDeadline`", span))
        }
        THandleOp::UnixStreamReadDeadline => {
            Err(unsupported("handle `UnixStreamReadDeadline`", span))
        }
        THandleOp::UnixStreamWriteAllDeadline => {
            Err(unsupported("handle `UnixStreamWriteAllDeadline`", span))
        }
        THandleOp::UnixStreamReady => Err(unsupported("handle `UnixStreamReady`", span)),
        THandleOp::UnixStreamClose => Err(unsupported("handle `UnixStreamClose`", span)),
        THandleOp::UnixStreamSetTimeout => Err(unsupported("handle `UnixStreamSetTimeout`", span)),
        THandleOp::TLSStreamReadDeadline => {
            Err(unsupported("handle `TLSStreamReadDeadline`", span))
        }
        THandleOp::TLSStreamWriteAllDeadline => {
            Err(unsupported("handle `TLSStreamWriteAllDeadline`", span))
        }
        THandleOp::TLSStreamReady => Err(unsupported("handle `TLSStreamReady`", span)),
        THandleOp::TLSStreamClose => Err(unsupported("handle `TLSStreamClose`", span)),
        THandleOp::TLSStreamCloseWrite => Err(unsupported("handle `TLSStreamCloseWrite`", span)),
        THandleOp::TLSStreamPeerIdentity => {
            Err(unsupported("handle `TLSStreamPeerIdentity`", span))
        }
        THandleOp::TLSClientConfigDefault => {
            Err(unsupported("handle `TLSClientConfigDefault`", span))
        }
        THandleOp::TLSClientConfigWithAlpn => {
            Err(unsupported("handle `TLSClientConfigWithAlpn`", span))
        }
        THandleOp::TLSRootCertificatesFromPem => {
            Err(unsupported("handle `TLSRootCertificatesFromPem`", span))
        }
        THandleOp::TLSClientIdentityFromPem => {
            Err(unsupported("handle `TLSClientIdentityFromPem`", span))
        }
        THandleOp::TLSClientConfigWithTrust => {
            Err(unsupported("handle `TLSClientConfigWithTrust`", span))
        }
        THandleOp::TLSClientConfigWithIdentity => {
            Err(unsupported("handle `TLSClientConfigWithIdentity`", span))
        }
        THandleOp::TLSClientConfigWithVersionBounds => Err(unsupported(
            "handle `TLSClientConfigWithVersionBounds`",
            span,
        )),
        THandleOp::HTTPClientNew => Err(unsupported("handle `HTTPClientNew`", span)),
        THandleOp::AllocAlloc | THandleOp::AllocTryAlloc | THandleOp::AllocReset => Err(
            unsupported("allocator dispatch must use the evaluator runtime", span),
        ),
        THandleOp::HTTPReqField(_) => Err(unsupported("handle `HTTPReqField`", span)),
        THandleOp::HTTPReqHeader => Err(unsupported("handle `HTTPReqHeader`", span)),
        THandleOp::HTTPReqParam => Err(unsupported("handle `HTTPReqParam`", span)),
        THandleOp::HTTPReqTrailers => Err(unsupported("handle `HTTPReqTrailers`", span)),
        THandleOp::HTTPRespField(_) => Err(unsupported("handle `HTTPRespField`", span)),
        THandleOp::HTTPRespHeader => Err(unsupported("handle `HTTPRespHeader`", span)),
        THandleOp::HTTPRespTrailers => Err(unsupported("handle `HTTPRespTrailers`", span)),
        THandleOp::ArgsSpecFlag => Err(unsupported("handle `ArgsSpecFlag`", span)),
        THandleOp::ArgsSpecFlagShort => Err(unsupported("handle `ArgsSpecFlagShort`", span)),
        THandleOp::ArgsSpecOption => Err(unsupported("handle `ArgsSpecOption`", span)),
        THandleOp::ArgsSpecOptionShort => Err(unsupported("handle `ArgsSpecOptionShort`", span)),
        THandleOp::ArgsSpecOptionDefault => {
            Err(unsupported("handle `ArgsSpecOptionDefault`", span))
        }
        THandleOp::ArgsSpecOptionEnv => Err(unsupported("handle `ArgsSpecOptionEnv`", span)),
        THandleOp::ArgsSpecOptionInt => Err(unsupported("handle `ArgsSpecOptionInt`", span)),
        THandleOp::ArgsSpecOptionFloat => Err(unsupported("handle `ArgsSpecOptionFloat`", span)),
        THandleOp::ArgsSpecOptionChoice => Err(unsupported("handle `ArgsSpecOptionChoice`", span)),
        THandleOp::ArgsSpecRepeat => Err(unsupported("handle `ArgsSpecRepeat`", span)),
        THandleOp::ArgsSpecRequiredOption => {
            Err(unsupported("handle `ArgsSpecRequiredOption`", span))
        }
        THandleOp::ArgsSpecPositional => Err(unsupported("handle `ArgsSpecPositional`", span)),
        THandleOp::ArgsSpecDescription => Err(unsupported("handle `ArgsSpecDescription`", span)),
        THandleOp::ArgsSpecSubcommand => Err(unsupported("handle `ArgsSpecSubcommand`", span)),
        THandleOp::ArgsSpecVersion => Err(unsupported("handle `ArgsSpecVersion`", span)),
        THandleOp::ArgsSpecCompletion => Err(unsupported("handle `ArgsSpecCompletion`", span)),
        THandleOp::ArgsSpecHelp => Err(unsupported("handle `ArgsSpecHelp`", span)),
        THandleOp::ArgsSpecParse => Err(unsupported("handle `ArgsSpecParse`", span)),
        THandleOp::ArgsSpecParseOrExit => Err(unsupported("handle `ArgsSpecParseOrExit`", span)),
        THandleOp::ParsedArgsFlag => Err(unsupported("handle `ParsedArgsFlag`", span)),
        THandleOp::ParsedArgsOption => Err(unsupported("handle `ParsedArgsOption`", span)),
        THandleOp::ParsedArgsOptionInt => Err(unsupported("handle `ParsedArgsOptionInt`", span)),
        THandleOp::ParsedArgsOptionFloat => {
            Err(unsupported("handle `ParsedArgsOptionFloat`", span))
        }
        THandleOp::ParsedArgsOptions => Err(unsupported("handle `ParsedArgsOptions`", span)),
        THandleOp::ParsedArgsSubcommand => Err(unsupported("handle `ParsedArgsSubcommand`", span)),
        THandleOp::ParsedArgsPositional => Err(unsupported("handle `ParsedArgsPositional`", span)),
        THandleOp::ProcessSpecMethod { .. } => Err(unsupported("handle `ProcessSpecMethod`", span)),
        THandleOp::ProcessChildMethod { .. } => {
            Err(unsupported("handle `ProcessChildMethod`", span))
        }
        THandleOp::TerminalSessionResize => {
            Err(unsupported("handle `TerminalSessionResize`", span))
        }
        THandleOp::ProcessStdinWrite => Err(unsupported("handle `ProcessStdinWrite`", span)),
        THandleOp::ReflectValueTypeName => reflect_handle(recv, "type_name", span),
        THandleOp::ReflectValuePath => reflect_handle(recv, "path", span),
        // Handled before this context-free dispatch in `eval/exprs.rs`, where
        // the Display-aware evaluator is available.
        THandleOp::ReflectValueDisplay => Err(unsupported("reflect display evaluator", span)),
        THandleOp::ReflectValueFields => reflect_handle(recv, "fields", span),
        THandleOp::ReflectFieldName => reflect_handle(recv, "name", span),
        THandleOp::ReflectFieldValue => reflect_handle(recv, "value", span),
        THandleOp::TaskJoin => match recv {
            CtValue::Struct { type_name, fields } if type_name == "__JetTirTask" => fields
                .iter()
                .find_map(|(name, value)| {
                    (name == "value").then(|| CtValue::Present(Box::new(value.clone())))
                })
                .ok_or_else(|| unsupported("task result", span)),
            _ => Err(unsupported("task receiver", span)),
        },
        // D-COROUTINE1=A: task control is handled in `exprs.rs`, which can
        // reach the evaluator's task table. Nothing routes here.
        THandleOp::TaskDetach
        | THandleOp::TaskPause
        | THandleOp::TaskResume
        | THandleOp::TaskCancel => Err(unsupported("task control outside the evaluator", span)),
        THandleOp::ChannelReceive => Err(unsupported("handle `ChannelReceive`", span)),
        THandleOp::SenderSend => Err(unsupported("handle `SenderSend`", span)),
        THandleOp::ChannelClose => Err(unsupported("handle `ChannelClose`", span)),
        THandleOp::HTTPRouterRegister { .. } => {
            Err(unsupported("handle `HTTPRouterRegister`", span))
        }
        THandleOp::MathMethod {
            method, reduce_op, ..
        } => {
            let mut argv = args.to_vec();
            if let Some(op) = reduce_op {
                // Lowering resolves the `ReduceOp` value into `reduce_op` and drops
                // the source arg — restore it for MathLayout::apply_method.
                argv.insert(0, CtValue::Str(op.clone()));
            }
            apply_method(recv, method, argv, span)
        }
        THandleOp::ReactiveGet => Err(unsupported("handle `ReactiveGet`", span)),
        THandleOp::ReactiveSet => Err(unsupported("handle `ReactiveSet`", span)),
        THandleOp::ReactiveEffectMethod { .. } => {
            Err(unsupported("handle `ReactiveEffectMethod`", span))
        }
        THandleOp::EventMethod { .. } => {
            // Unreachable: exprs.rs HandleCall intercepts EventMethod and routes
            // through EvalCtx::eval_event_method (needs callables) before eval_handle.
            unreachable!("EventMethod dispatched in exprs.rs before eval_handle");
        }
        THandleOp::WatchMethod { .. } => Err(unsupported("handle `WatchMethod`", span)),
        THandleOp::LayoutMethod { .. } => Err(unsupported("handle `LayoutMethod`", span)),
        THandleOp::LoadableMethod { method } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::ExpiringMethod { .. } => Err(unsupported("handle `ExpiringMethod`", span)),
        THandleOp::SketchMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::UrlMimeMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::EmailMethod { method } => {
            crate::Comptime::EmailAdapter::evaluate_method(recv, method, args, span).map_or_else(
                || apply_method(recv, method, args.to_vec(), span),
                |result| result,
            )
        }
        THandleOp::RegexMethod { method, .. } => apply_method(recv, method, args.to_vec(), span),
        THandleOp::HTTPClientMethod { .. } => Err(unsupported("handle `HTTPClientMethod`", span)),
        THandleOp::HTTPServerMethod { .. } => Err(unsupported("handle `HTTPServerMethod`", span)),
        THandleOp::DataTreeField => Ok(datatree_field_result(recv, args)),
        THandleOp::DataTreeAt => Ok(datatree_at_result(recv, args)),
        THandleOp::DataTreeInt | THandleOp::JSONInt => Ok(datatree_int_result(recv)),
        THandleOp::DataTreeText => Ok(datatree_scalar_result(recv, "Text", "text")),
        THandleOp::DataTreeBool => Ok(datatree_scalar_result(recv, "Bool", "bool")),
        THandleOp::DataTreeFloat => Ok(datatree_scalar_result(recv, "Float", "float")),
        THandleOp::DataTreeDecode(_) => Err(unsupported("handle `DataTreeDecode`", span)),
        THandleOp::SerdeEncode => Err(unsupported("handle `SerdeEncode`", span)),
        THandleOp::JSONField => Err(unsupported("handle `JSONField`", span)),
        THandleOp::JSONAt => Err(unsupported("handle `JSONAt`", span)),
        THandleOp::JSONText => Err(unsupported("handle `JSONText`", span)),
        THandleOp::JSONBool => Err(unsupported("handle `JSONBool`", span)),
        THandleOp::JSONFloat => Err(unsupported("handle `JSONFloat`", span)),
        THandleOp::PathParent => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.parent", span))?;
            Ok(match path_kernel::jet_std_path_parent_opt(&path) {
                Some(parent) => CtValue::Present(Box::new(path_value(parent))),
                None => CtValue::failed(Box::new(CtValue::Unit)),
            })
        }
        THandleOp::PathExtension => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.extension", span))?;
            Ok(match path_kernel::jet_std_path_extension_opt(&path) {
                Some(extension) => CtValue::Present(Box::new(CtValue::Str(extension))),
                None => CtValue::failed(Box::new(CtValue::Unit)),
            })
        }
        THandleOp::PathStem => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.stem", span))?;
            Ok(match path_kernel::jet_std_path_stem_opt(&path) {
                Some(stem) => CtValue::Present(Box::new(CtValue::Str(stem))),
                None => CtValue::failed(Box::new(CtValue::Unit)),
            })
        }
        THandleOp::PathNormalize => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.normalize", span))?;
            Ok(path_value(path_kernel::jet_std_path_normalize(&path)))
        }
        THandleOp::PathWalk => {
            let path = path_string(recv).ok_or_else(|| unsupported("Path.walk", span))?;
            Ok(CtValue::List(
                path_kernel::jet_std_path_walk(&path)
                    .into_iter()
                    .map(path_value)
                    .collect(),
            ))
        }
        THandleOp::UiBackendMethod { .. } => Err(unsupported("handle `UiBackendMethod`", span)),
        THandleOp::DevServerMethod { .. } => Err(unsupported("handle `DevServerMethod`", span)),
        THandleOp::AppMethod { .. } => Err(unsupported("handle `AppMethod`", span)),
        THandleOp::PluginCall => Err(unsupported("handle `PluginCall`", span)),
        THandleOp::PluginCallInt => Err(unsupported("handle `PluginCallInt`", span)),
        THandleOp::PluginCallBool => Err(unsupported("handle `PluginCallBool`", span)),
        THandleOp::PluginCallText => Err(unsupported("handle `PluginCallText`", span)),
        // D-LIB-CALLGRANT1=A: interpreter ambient owns the actual loader and
        // call; this context-free fallback must never invent a second policy.
        THandleOp::ModOnTick => Err(unsupported("handle `ModOnTick`", span)),
        // D-SHIFT1: `binary.Reader` / `text.Cursor` marshal to the shared
        // `jet_foundation::StreamCursor` kernel AOT splices into its prelude.
        THandleOp::ReaderOver
        | THandleOp::ReaderReadU8
        | THandleOp::ReaderReadI8
        | THandleOp::ReaderReadU16Le
        | THandleOp::ReaderReadU16Be
        | THandleOp::ReaderReadI16Le
        | THandleOp::ReaderReadI16Be
        | THandleOp::ReaderReadU32Le
        | THandleOp::ReaderReadU32Be
        | THandleOp::ReaderReadI32Le
        | THandleOp::ReaderReadI32Be
        | THandleOp::ReaderReadU64Le
        | THandleOp::ReaderReadU64Be
        | THandleOp::ReaderReadI64Le
        | THandleOp::ReaderReadI64Be
        | THandleOp::ReaderReadF32Le
        | THandleOp::ReaderReadF32Be
        | THandleOp::ReaderReadF64Le
        | THandleOp::ReaderReadF64Be
        | THandleOp::ReaderPeek
        | THandleOp::ReaderSeek
        | THandleOp::ReaderSkip
        | THandleOp::ReaderTake
        | THandleOp::ReaderRemaining
        | THandleOp::ReaderAtEnd
        | THandleOp::CursorOver
        | THandleOp::CursorTakeUntil
        | THandleOp::CursorSkipWs
        | THandleOp::CursorTakePattern { .. }
        | THandleOp::ReaderTakePattern { .. } => super::stream::eval(op, recv, args, span),
    }
}

fn duration_new(
    recv: &CtValue,
    unit: &str,
    float: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let scale = match unit {
        "Nanoseconds" => 1_i64,
        "Microseconds" => 1_000,
        "Milliseconds" => 1_000_000,
        "Seconds" => 1_000_000_000,
        "Minutes" => 60_000_000_000,
        "Hours" => 3_600_000_000_000,
        _ => return Err(unsupported(&format!("Duration unit `{unit}`"), span)),
    };
    let (ms, reason) = if float {
        let n = match recv {
            CtValue::Float(n) => n.as_f64(),
            CtValue::Int(n) => *n as f64,
            _ => {
                return Err(unsupported(
                    "Duration constructor expects a numeric value",
                    span,
                ));
            }
        };
        (
            duration_kernel::jet_duration_kernel_from_float(n, scale),
            duration_kernel::jet_duration_kernel_float_error_reason(),
        )
    } else {
        (
            match recv {
                CtValue::Int(n) => duration_kernel::jet_duration_kernel_from_int(*n, scale),
                _ => None,
            },
            duration_kernel::jet_duration_kernel_int_error_reason(),
        )
    };
    Ok(match ms {
        Some(ms) => CtValue::Present(Box::new(CtValue::Struct {
            type_name: crate::Syntax::DURATION_TYPE.to_string(),
            fields: vec![("ns".to_string(), CtValue::Int(ms))],
        })),
        None => CtValue::failed(Box::new(CtValue::Struct {
            type_name: crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
            fields: vec![("reason".to_string(), CtValue::Str(reason.to_string()))],
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::{datatree_int_result, reflect_handle, reflect_value_carrier};
    use crate::Diagnostics::Span;
    use crate::AST::{CtFloat, CtReport, CtValue, Type};

    fn tree(variant: &str, value: CtValue) -> CtValue {
        CtValue::Enum {
            type_name: "DataTree".to_string(),
            variant: variant.to_string(),
            args: vec![(None, value)],
        }
    }

    fn decode_reason(value: CtValue) -> Option<String> {
        let CtValue::Failed(CtReport::Told(error)) = value else {
            return None;
        };
        let CtValue::List(errors) = *error else {
            return None;
        };
        errors.into_iter().find_map(|error| {
            let CtValue::Struct { fields, .. } = error else {
                return None;
            };
            fields
                .into_iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("reason", CtValue::Str(reason)) => Some(reason),
                    _ => None,
                })
        })
    }

    #[test]
    fn reflected_field_keeps_declared_inline_range_type() {
        let declared = Type::InlineRange {
            base: Box::new(Type::Int),
            lo: 0,
            hi: 10,
        };
        let carrier = reflect_value_carrier(&CtValue::Int(3), Some(&declared), None, None, None);
        assert_eq!(
            reflect_handle(&carrier, "type_name", Span::new(0, 0))
                .expect("reflected field type name"),
            CtValue::Str("Int(0..10)".to_string())
        );
    }

    #[test]
    fn datatree_int_rejects_float_and_text_with_canonical_reasons() {
        assert_eq!(
            datatree_int_result(&tree("Int", CtValue::Int(7))),
            CtValue::Present(Box::new(CtValue::Int(7)))
        );
        assert_eq!(
            decode_reason(datatree_int_result(&tree(
                "Float",
                CtValue::Float(CtFloat::f64(7.0)),
            ))),
            Some("expected int, got 7.0".to_string())
        );
        assert_eq!(
            decode_reason(datatree_int_result(&tree(
                "Text",
                CtValue::Str("7".to_string()),
            ))),
            Some("expected int, got \"7\"".to_string())
        );
    }
}
