//! Remaining deterministic Core calls for #392.
//!
//! Algorithms and value layouts mirror the AOT prelude. This module owns one
//! evaluator used by comptime and the REPL; callers never synthesize schemas or
//! fall back after a recognized call fails.

use std::collections::BTreeMap;

use super::mime_kernel;
use crate::AST::{as_bytes, CtFloat, CtReport, CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};

use crate::Comptime::Builtins::{as_bool, as_int};
use crate::Comptime::Diagnostics::unsupported;
use crate::Comptime::EmailAdapter;
use crate::Comptime::Methods::as_float;
use jet_foundation::Syntax::CoreCallPureRoute;

type EvalResult = Result<CtValue, Diagnostic>;

pub(super) fn evaluate(
    row: &jet_foundation::Syntax::CoreCallRecord,
    args: &[CtValue],
    span: Span,
) -> Option<EvalResult> {
    let result = match (row.pure_route, row.member) {
        (CoreCallPureRoute::Mime, "parse") => mime_parse(args, span),
        (CoreCallPureRoute::Mime, "from_extension") => mime_from_extension(args, span),
        (CoreCallPureRoute::Mime, "extension") => mime_extension(args, span),
        (CoreCallPureRoute::Email, "address") => return EmailAdapter::evaluate("address", args, span),
        (CoreCallPureRoute::Email, "attachment") => {
            return EmailAdapter::evaluate("attachment", args, span)
        }
        (CoreCallPureRoute::Email, "message") => return EmailAdapter::evaluate("message", args, span),
        (CoreCallPureRoute::Email, "envelope") => return EmailAdapter::evaluate("envelope", args, span),
        (CoreCallPureRoute::Email, "serialize") => return EmailAdapter::evaluate("serialize", args, span),
        (CoreCallPureRoute::EncodingXml, "canonical") => xml_canonical(args, span),
        (CoreCallPureRoute::Time, "period") => period(args, span),
        (CoreCallPureRoute::Time, "period_days") => period_unit(args, span, 2),
        (CoreCallPureRoute::Time, "period_months") => period_unit(args, span, 1),
        (CoreCallPureRoute::Time, "period_years") => period_unit(args, span, 0),
        (CoreCallPureRoute::Time, "from_unix_ms") => datetime_from_unix_ms(args, span),
        (CoreCallPureRoute::Time, "parse_rfc3339") => datetime_parse(args, span),
        (CoreCallPureRoute::Time, "parse_time") => local_time_parse(args, span),
        // Pure zone constructors: UTC is deterministic. Named IANA zones need a
        // host TZif database (filesystem), so comptime keeps UTC aliases only and
        // returns `Err` for everything else — same shape as AOT without tzdb.
        (CoreCallPureRoute::Time, "utc") => Ok(zone_utc()),
        (CoreCallPureRoute::Time, "zone") => zone_named(args, span),
        (CoreCallPureRoute::Time, "zoned") => zoned_from_datetime(args, span),
        (CoreCallPureRoute::Time, "zoned_local") => zoned_from_local(args, span),
        (CoreCallPureRoute::Time, "instant") => Ok(structure("Instant", vec![("start_ns", CtValue::Int(0))])),
        (CoreCallPureRoute::Time, "datetime") => datetime_parts(args, span),
        (CoreCallPureRoute::Time, "time" | "local_time") => local_time_parts(args, span),
        (CoreCallPureRoute::Time, "days_in_month") => time_days_in_month(args, span),
        (CoreCallPureRoute::Time, "is_leap_year") => time_is_leap_year(args, span),
        (CoreCallPureRoute::Time, method @ ("nanoseconds" | "microseconds" | "milliseconds" | "seconds" | "minutes" | "hours")) => {
            duration_ctor(method, args, span)
        }
        (CoreCallPureRoute::Math, "decimal") => decimal_from_str(args, span),
        (CoreCallPureRoute::Math, "fraction") => fraction_new(args, span),
        (CoreCallPureRoute::Measurement, "from") => measurement(args, span),
        (CoreCallPureRoute::Date, "new") => date_new_call(args, span),
        (CoreCallPureRoute::Date, "parse") => date_parse_call(args, span),
        // Wall-clock read — same JetDate::today_utc as AOT/JIT hosts (I9).
        (CoreCallPureRoute::Date, "today") => Ok(Date::today_utc().value()),
        (CoreCallPureRoute::DateTime, "from_timestamp") => datetime_from_timestamp(args, span),
        // D-APPROX1=A: sketch constructors — same algorithms as AOT Jet* sketches.
        (CoreCallPureRoute::SketchHll, "new") => Ok(hll_new()),
        (CoreCallPureRoute::SketchTDigest, "new") => Ok(tdigest_new()),
        (CoreCallPureRoute::SketchCms, "new") => Ok(cms_new()),
        (CoreCallPureRoute::SketchReservoir, "new") => reservoir_new(args, span),
        (CoreCallPureRoute::Ui, "point") => ui_point(args, span),
        (CoreCallPureRoute::Ui, "size") => ui_size(args, span),
        (CoreCallPureRoute::Ui, "rect") => ui_rect(args, span),
        (CoreCallPureRoute::Ui, "constraint") => ui_constraint(args, span),
        (CoreCallPureRoute::Ui, "node") => ui_node(args, span, None, None, "Custom"),
        (CoreCallPureRoute::Ui, "node_role") => ui_node_role(args, span),
        (CoreCallPureRoute::Ui, "node_color") => ui_node_color(args, span),
        (CoreCallPureRoute::Ui, "text") => ui_text(args, span),
        (CoreCallPureRoute::Ui, "button") => ui_button(args, span),
        (CoreCallPureRoute::Ui, "box") => ui_box(args, span),
        (CoreCallPureRoute::Ui, "aria_role_button") => Ok(ui_role("Button")),
        (CoreCallPureRoute::Ui, "aria_role_text_input") => Ok(ui_role("TextInput")),
        (CoreCallPureRoute::Ui, "aria_role_label") => Ok(ui_role("Label")),
        (CoreCallPureRoute::Ui, "aria_role_container") => Ok(ui_role("Container")),
        (CoreCallPureRoute::Ui, "key_event") => ui_key_event(args, span),
        (CoreCallPureRoute::Ui, "resize_event") => ui_resize_event(args, span),
        (CoreCallPureRoute::Raylib, "color") => raylib_color(args, span),
        (CoreCallPureRoute::Io, "style_force") => io_style_force(args, span),
        (CoreCallPureRoute::Net, "ip_addr") => net_ip_addr(args, span),
        (CoreCallPureRoute::Net, "ip_to_string") => net_string_field(args, "IPAddr", "text", span),
        (CoreCallPureRoute::Net, "ip_is_ipv4") => net_ip_is_ipv4(args, span),
        (CoreCallPureRoute::Net, "socket_addr_parse") => net_socket_addr_parse(args, span),
        (CoreCallPureRoute::Net, "socket_host") => net_string_field(args, "SocketAddr", "host", span),
        (CoreCallPureRoute::Net, "socket_port") => net_value_field(args, "SocketAddr", "port", span),
        (CoreCallPureRoute::Net, "socket_to_string") => net_string_field(args, "SocketAddr", "text", span),
        (CoreCallPureRoute::Net, "ready_readable") => net_value_field(args, "NetReady", "readable", span),
        (CoreCallPureRoute::Net, "ready_writable") => net_value_field(args, "NetReady", "writable", span),
        (CoreCallPureRoute::Net, "error_operation") => net_string_field(args, "NetError", "operation", span),
        (CoreCallPureRoute::Net, "error_address") => net_value_field(args, "NetError", "address", span),
        (CoreCallPureRoute::Net, "error_name") => net_value_field(args, "NetError", "name", span),
        (CoreCallPureRoute::Net, "error_message") => net_string_field(args, "NetError", "message", span),
        (CoreCallPureRoute::Net, "error_os_code") => net_value_field(args, "NetError", "os_code", span),
        (CoreCallPureRoute::Net, "dns_srv_target") => net_string_field(args, "DNSSrv", "target", span),
        (CoreCallPureRoute::Net, "dns_srv_port") => net_value_field(args, "DNSSrv", "port", span),
        (CoreCallPureRoute::Net, "dns_srv_priority") => net_value_field(args, "DNSSrv", "priority", span),
        (CoreCallPureRoute::Net, "dns_srv_weight") => net_value_field(args, "DNSSrv", "weight", span),
        (CoreCallPureRoute::Net, "udp_packet_data") => net_udp_packet_data(args, span),
        (CoreCallPureRoute::Net, "udp_packet_bytes") => net_value_field(args, "UDPPacket", "data", span),
        (CoreCallPureRoute::Net, "udp_packet_addr") => net_value_field(args, "UDPPacket", "addr", span),
        (CoreCallPureRoute::Net, "udp_packet_original_len") => net_value_field(args, "UDPPacket", "original_len", span),
        (CoreCallPureRoute::Net, "udp_packet_truncated") => net_value_field(args, "UDPPacket", "truncated", span),
        (CoreCallPureRoute::Crypto, "ed25519_verify_strict") => crypto_ed25519_verify(args, span),
        (CoreCallPureRoute::Crypto, "ed25519_sign") => crypto_ed25519_sign(args, span),
        (CoreCallPureRoute::Crypto, "hkdf_sha256_raw") => crypto_hkdf(args, span),
        (CoreCallPureRoute::Crypto, "x25519_raw") => crypto_x25519(args, span),
        (CoreCallPureRoute::Crypto, "xchacha20poly1305_seal") => {
            crypto_aead_seal(args, span, "expert.xchacha20poly1305_seal", 24, false)
        }
        (CoreCallPureRoute::Crypto, "xchacha20poly1305_open") => {
            crypto_aead_open(args, span, "expert.xchacha20poly1305_open", 24)
        }
        (CoreCallPureRoute::Crypto, "aes256gcm_seal") => {
            crypto_aead_seal(args, span, "expert.aes256gcm_seal", 12, true)
        }
        (CoreCallPureRoute::Crypto, "aes256gcm_open") => {
            crypto_aead_open(args, span, "expert.aes256gcm_open", 12)
        }
        (CoreCallPureRoute::Crypto, "argon2id") => crypto_argon2id(args, span),
        (CoreCallPureRoute::Crypto, "secret_bytes") => crypto_extract(args, 0, "Secret", span),
        (CoreCallPureRoute::Crypto, "signing_key_bytes") => crypto_extract(args, 0, "SigningKey", span),
        (CoreCallPureRoute::Crypto, "x25519_secret_bytes") => crypto_extract(args, 0, "X25519SecretKey", span),
        (CoreCallPureRoute::Crypto, "shared_secret_bytes") => crypto_extract(args, 0, "SharedSecret", span),
        // TIR lowers Signature/VerifyKey/… `.bytes()` to core.crypto.__*_bytes;
        // keep those pure field extracts resident so REPL does not hit E1802.
        (CoreCallPureRoute::Crypto, "__signature_bytes") => crypto_extract(args, 0, "Signature", span),
        (CoreCallPureRoute::Crypto, "__verify_key_bytes") => crypto_extract(args, 0, "VerifyKey", span),
        (CoreCallPureRoute::Crypto, "__x25519_public_bytes") => crypto_extract(args, 0, "X25519PublicKey", span),
        (CoreCallPureRoute::Crypto, "__sealed_bytes") => crypto_extract(args, 0, "Sealed", span),
        (CoreCallPureRoute::Crypto, "__digest256_bytes") => crypto_extract(args, 0, "Digest256", span),
        (CoreCallPureRoute::Crypto, "__digest512_bytes") => crypto_extract(args, 0, "Digest512", span),
        // Typed decode/decode_bytes run in eval_method; arms prove inventory coverage.
        (CoreCallPureRoute::EncodingXml, "decode") => Err(unsupported(
            "core.encoding.xml.decode() requires a type argument",
            span,
        )),
        (CoreCallPureRoute::EncodingXml, "decode_bytes") => Err(unsupported(
            "core.encoding.xml.decode_bytes() requires a type argument",
            span,
        )),
        _ => return None,
    };
    Some(result)
}

pub(super) fn evaluate_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<EvalResult> {
    let CtValue::Struct { type_name, .. } = recv else {
        return None;
    };
    let row = jet_foundation::Syntax::core_receiver_method(type_name, method)?;
    if !row.accepts_arity(args.len()) {
        return None;
    }
    let result = match (type_name.as_str(), method, args.len()) {
        (
            "Signature"
                | "Secret"
                | "SigningKey"
                | "VerifyKey"
                | "X25519SecretKey"
                | "X25519PublicKey"
                | "SharedSecret",
            "bytes",
            0,
        ) => value_field(recv, type_name, "bytes", span),
        ("Mime", "media_type", 0) => string_field(recv, "Mime", "top", span),
        ("Mime", "subtype", 0) => string_field(recv, "Mime", "sub", span),
        ("Mime", "essence", 0) => mime_essence(recv, span).map(CtValue::Str),
        ("Mime", "to_string", 0) => mime_string(recv, span).map(CtValue::Str),
        ("Mime", "param", 1) => mime_param(recv, args, span),
        ("Mime", "params", 0) => value_field(recv, "Mime", "params", span),
        ("Date" | "LocalDate", "year" | "month" | "day", 0) => {
            value_field(recv, type_name, method, span)
        }
        ("Date" | "LocalDate", "to_string", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Str(date.to_string_fmt())),
        ("Date" | "LocalDate", "weekday", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.weekday())),
        ("Date" | "LocalDate", "iso_weekday", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.iso_weekday())),
        ("Date" | "LocalDate", "day_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.day_of_year())),
        ("Date" | "LocalDate", "iso_week", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.iso_week())),
        ("Date" | "LocalDate", "quarter_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.quarter_of_year())),
        ("Date" | "LocalDate", "days_in_month", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.days_in_month())),
        ("Date" | "LocalDate", "is_leap_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Bool(date.inner.is_leap_year())),
        ("Date" | "LocalDate", "replace", 3) => date_from_value(recv, type_name, span).and_then(
            |date| {
                Ok(Date::from_inner(date.inner.replace(
                    as_int(&args[0], span)?,
                    as_int(&args[1], span)?,
                    as_int(&args[2], span)?,
                ))
                .value())
            },
        ),
        ("Date" | "LocalDate", "add_days", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_days(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "add_months", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_months(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "diff_days", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| {
                let other = date_from_value(&args[0], "LocalDate", span)?;
                Ok(CtValue::Int(date.inner.diff_days(&other.inner)))
            }),
        ("Date" | "LocalDate", "add_period", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| date_add_period(date, &args[0], span).map(Date::value)),
        ("Date" | "LocalDate", "truncate", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date_truncate(date, string_arg(args, 0, span)?).value())),
        ("Date" | "LocalDate", "format", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| {
                Ok(CtValue::Str(format_time_pattern(
                    string_arg(args, 0, span)?,
                    date,
                    LocalTime::new(0, 0, 0),
                )))
            }),
        ("LocalTime", "hour" | "minute" | "second", 0) => {
            value_field(recv, "LocalTime", method, span)
        }
        ("LocalTime", "to_string", 0) => local_time_from_value(recv, span)
            .map(|time| CtValue::Str(time.to_string_fmt())),
        ("DateTime", "to_timestamp", 0) => value_field(recv, "DateTime", "secs", span),
        ("DateTime", "to_unix_ms", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.to_unix_ms())),
        ("DateTime", "to_string", 0) => datetime_string(recv, span).map(CtValue::Str),
        ("DateTime", "date", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.date().value())
        }
        ("DateTime", "time", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.time().value())
        }
        ("DateTime", "hour", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.hour())),
        ("DateTime", "minute", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.minute())),
        ("DateTime", "second", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.second())),
        ("DateTime", "millisecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.millisecond())),
        ("DateTime", "microsecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.microsecond())),
        ("DateTime", "nanosecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.nanosecond())),
        ("DateTime", "format_rfc3339", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Str(date_time.inner.format_rfc3339())),
        ("DateTime", "format", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(CtValue::Str(format_time_pattern(
                string_arg(args, 0, span)?,
                date_time.date(),
                date_time.time(),
            )))
        }),
        ("DateTime", "plus_duration", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let ns = duration_ns(&args[0], span)?;
                Ok(date_time.plus_ns(ns).value())
            })
        }
        ("DateTime", "difference", 1) => datetime_from_value(recv, span).and_then(|left| {
            let right = datetime_from_value(&args[0], span)?;
            Ok(duration_value(left.inner.difference_ns(&right.inner)))
        }),
        ("DateTime", "truncate" | "round" | "floor" | "ceil", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                Ok(date_time.align(string_arg(args, 0, span)?, method).value())
            })
        }
        ("DateTime", "replace", 6) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(DateTime::from_inner(date_time.inner.replace(
                as_int(&args[0], span)?,
                as_int(&args[1], span)?,
                as_int(&args[2], span)?,
                as_int(&args[3], span)?,
                as_int(&args[4], span)?,
                as_int(&args[5], span)?,
            ))
            .value())
        }),
        ("DateTime", "in_zone", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(ZonedDateTime::from_datetime(date_time, zone_from_value(&args[0], span)?).value())
        }),
        ("Instant", "elapsed_millis", 0) => Ok(CtValue::Int(0)),
        ("Instant", "elapsed", 0) => Ok(duration_value(0)),
        ("Zone", "name", 0) => string_field(recv, "Zone", "name", span),
        ("Fraction", "to_string", 0) => fraction_from_value(recv, span)
            .map(|f| CtValue::Str(f.to_string_rep())),
        ("Fraction", "numerator", 0) => fraction_from_value(recv, span)
            .map(|f| CtValue::Int(f.numerator)),
        ("Fraction", "denominator", 0) => fraction_from_value(recv, span)
            .map(|f| CtValue::Int(f.denominator)),
        ("Fraction", "to_float", 0) => fraction_from_value(recv, span)
            .map(|f| CtValue::Float(crate::AST::CtFloat::F64(f.numerator as f64 / f.denominator as f64))),
        ("Fraction", "is_zero", 0) => fraction_from_value(recv, span)
            .map(|f| CtValue::Bool(f.numerator == 0)),
        ("Fraction", "equal", 1) => fraction_from_value(recv, span).and_then(|left| {
            let right = fraction_from_value(&args[0], span)?;
            Ok(CtValue::Bool(left == right))
        }),
        ("Fraction", "add" | "sub" | "mul" | "div", 1) => fraction_from_value(recv, span).and_then(|left| {
            let right = fraction_from_value(&args[0], span)?;
            let out = match method {
                "add" => left.add(&right),
                "sub" => left.sub(&right),
                "mul" => left.mul(&right),
                "div" => left.div(&right),
                _ => unreachable!("fraction method guard"),
            };
            match out {
                Some(value) => Ok(value.to_value()),
                None => Err(unsupported("a ratio that leaves the range, or divided by zero", span)),
            }
        }),
        ("Decimal", "to_string", 0) => decimal_from_value(recv, span)
            .map(|decimal| CtValue::Str(decimal.to_string_rep())),
        ("Decimal", "add" | "sub" | "mul", 1) => decimal_from_value(recv, span).and_then(|left| {
            let right = decimal_from_value(&args[0], span)?;
            let out = match method {
                "add" => left.add(&right),
                "sub" => left.sub(&right),
                "mul" => left.mul(&right),
                _ => unreachable!("decimal method guard"),
            };
            Ok(out.to_value())
        }),
        ("ZonedDateTime", "date", 0) => zoned_from_value(recv, span).map(|zoned| zoned.date().value()),
        ("ZonedDateTime", "time", 0) => zoned_from_value(recv, span).map(|zoned| zoned.time().value()),
        ("ZonedDateTime", "offset_seconds", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Int(zoned.offset_seconds()))
        }
        ("ZonedDateTime", "is_dst", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Bool(zoned.is_dst()))
        }
        ("ZonedDateTime", "to_datetime", 0) => {
            zoned_from_value(recv, span).map(|zoned| zoned.instant.value())
        }
        ("ZonedDateTime", "zone", 0) => zoned_from_value(recv, span).map(|zoned| zoned.zone.value()),
        ("ZonedDateTime", "to_string", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Str(zoned.to_string_fmt()))
        }
        ("ZonedDateTime", "format", 1) => zoned_from_value(recv, span).and_then(|zoned| {
            Ok(CtValue::Str(format_zoned_pattern(
                string_arg(args, 0, span)?,
                zoned,
            )))
        }),
        ("ZonedDateTime", "add_duration", 1) => zoned_from_value(recv, span).and_then(|zoned| {
            let ns = duration_ns(&args[0], span)?;
            Ok(ZonedDateTime::from_inner(zoned.inner.add_duration_ns(ns)).value())
        }),
        ("ZonedDateTime", "add_period", 1) => zoned_from_value(recv, span).and_then(|zoned| {
            let date = date_add_period(zoned.date(), &args[0], span)?;
            Ok(ZonedDateTime::from_local(date, zoned.time(), zoned.zone).value())
        }),
        ("Period", "to_string", 0) => period_string(recv, span).map(CtValue::Str),
        ("Measurement", "value" | "uncertainty", 0) => {
            value_field(recv, "Measurement", method, span)
        }
        ("Measurement", "add" | "sub" | "mul" | "div", 1) => {
            measurement_arithmetic(recv, method, &args[0], span)
        }
        // D-APPROX1=A: non-mutating sketch queries (mutations write back in dispatch).
        ("HyperLogLog", "count", 0) => hll_count(recv, span),
        ("CountMinSketch", "count", 1) => cms_count(recv, args, span),
        ("TDigest", "quantile", 1) => tdigest_quantile(recv, args, span),
        ("ReservoirSampler", "sample", 0) => reservoir_sample(recv, span),
        // D-SOLVER-LIB1=A: finite solver queries (`.require` mutates in dispatch).
        ("Solver", "failure_count", 0) => solver_failure_count(recv, span),
        ("Solver", "status", 0) => solver_status(recv, span),
        _ => return None,
    };
    Some(result)
}

/// D-APPROX1=A: mutating sketch `.add` — returns `(Unit, updated_receiver)`.
pub(super) fn sketch_add(
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<(CtValue, CtValue), Diagnostic>> {
    let CtValue::Struct { type_name, .. } = recv else {
        return None;
    };
    jet_foundation::Syntax::core_receiver_method(type_name, "add")?;
    let result = match type_name.as_str() {
        "HyperLogLog" => hll_add(recv, args, span),
        "TDigest" => tdigest_add(recv, args, span),
        "CountMinSketch" => cms_add(recv, args, span),
        "ReservoirSampler" => reservoir_add(recv, args, span),
        _ => return None,
    };
    Some(result.map(|updated| (CtValue::Unit, updated)))
}

/// D-SOLVER-LIB1=A: `solver.require(ok)` — returns `(Unit, updated_receiver)`.
pub(super) fn solver_require(
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
) -> Option<Result<(CtValue, CtValue), Diagnostic>> {
    let CtValue::Struct { type_name, .. } = recv else {
        return None;
    };
    if type_name != crate::Syntax::SOLVER_TYPE {
        return None;
    }
    jet_foundation::Syntax::core_receiver_method(type_name, "require")?;
    Some(solver_require_update(recv, args, span).map(|updated| (CtValue::Unit, updated)))
}

/// D-SOLVER-LIB1=A: `solve.Solver.new(seed)` — same seed/checked/failures layout as AOT.
pub(super) fn solver_new(args: &[CtValue], span: Span) -> EvalResult {
    if jet_foundation::Syntax::core_receiver_method(crate::Syntax::SOLVER_TYPE, "new").is_none() {
        return Err(unsupported("Solver.new is not in the Core-call registry", span));
    }
    let seed = as_int(one(args, 0, "Solver", "new", span)?, span)?;
    Ok(solver_value(super::solver_kernel::jet_solver_new(seed)))
}

pub(super) fn display(value: &CtValue) -> Option<String> {
    if let CtValue::Struct { type_name, .. } = value {
        let type_name = type_name
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(type_name.as_str());
        jet_foundation::Syntax::core_receiver_method(type_name, "__display")?;
    }
    match value {
        CtValue::Struct { type_name, .. } if type_name == "HyperLogLog" => {
            let CtValue::Int(n) = hll_count(value, Span::new(0, 0)).ok()? else {
                return None;
            };
            Some(format!("HyperLogLog(count={n})"))
        }
        CtValue::Struct { type_name, .. } if type_name == "TDigest" => Some("TDigest".to_string()),
        CtValue::Struct { type_name, .. } if type_name == "CountMinSketch" => {
            Some("CountMinSketch".to_string())
        }
        CtValue::Struct { type_name, .. } if type_name == "ReservoirSampler" => {
            let CtValue::Int(count) = field(value, "ReservoirSampler", "count")? else {
                return None;
            };
            Some(format!("ReservoirSampler(n={count})"))
        }
        CtValue::Struct { type_name, .. } if type_name == crate::Syntax::SOLVER_TYPE => {
            let failures = int_field(value, crate::Syntax::SOLVER_TYPE, "failures", Span::new(0, 0)).ok()?;
            let status = if failures == 0 { "ok" } else { "failed" };
            Some(format!("Solver(status: {status}, failures: {failures})"))
        }
        CtValue::Struct { type_name, fields } if type_name == "ServiceUpgradeReceipt" => {
            let field = |name: &str| fields.iter().find(|(field, _)| field == name).map(|(_, value)| value);
            let CtValue::Int(from) = field("from_generation")? else { return None; };
            let CtValue::Int(to) = field("to_generation")? else { return None; };
            let CtValue::Str(migration) = field("migration")? else { return None; };
            let CtValue::Bool(rollback_available) = field("rollback_available")? else { return None; };
            let CtValue::List(pinned) = field("pinned_shards")? else { return None; };
            let pinned = pinned
                .iter()
                .map(|value| match value { CtValue::Str(value) => Some(value.clone()), _ => None })
                .collect::<Option<Vec<_>>>()?;
            Some(format!(
                "ServiceUpgradeReceipt(from={from}, to={to}, migration={migration}, rollback_available={rollback_available}, pinned={})",
                pinned.join(",")
            ))
        }
        // Match AOT/JIT `DataError::display_text` / JetShow — not Rust Debug
        // of the mangled `__jet_DataError { __jet_kind: … }` shape (#1250).
        CtValue::Struct { type_name, fields }
            if type_name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(type_name.as_str()) == "DataError" =>
        {
            let get = |name: &str| -> Option<&CtValue> {
                fields.iter().find_map(|(n, v)| {
                    let n = n.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(n.as_str());
                    (n == name).then_some(v)
                })
            };
            let kind = match get("kind")? {
                CtValue::Enum { variant, .. } => {
                    variant.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(variant).to_string()
                }
                _ => return None,
            };
            let operation = match get("operation")? {
                CtValue::Str(s) => s.as_str(),
                _ => return None,
            };
            let reason = match get("reason")? {
                CtValue::Str(s) => s.as_str(),
                _ => return None,
            };
            let opt_int = |name: &str| -> Option<i64> {
                match get(name)? {
                    CtValue::Present(inner) => match inner.as_ref() {
                        CtValue::Int(n) => Some(*n),
                        _ => None,
                    },
                    CtValue::Int(n) => Some(*n),
                    _ => None,
                }
            };
            let mut out = format!("{kind} {operation}");
            if let Some(row) = opt_int("row") {
                out.push_str(&format!(", row {row}"));
            }
            if let Some(column) = opt_int("column") {
                out.push_str(&format!(", column {column}"));
            }
            if let Some(index) = opt_int("index") {
                out.push_str(&format!(", index {index}"));
            }
            out.push_str(&format!(": {reason}"));
            Some(out)
        }
        // Core pure structs: REPL/transcript show uses Type(field: jet_show) —
        // not Rust `__jet_*` Debug — matching AOT JetShow for these foreign types.
        CtValue::Struct { type_name, fields }
            if matches!(
                type_name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(type_name.as_str()),
                "Mime"
                    | "Period"
                    | "LocalDate"
                    | "LocalTime"
                    | "DateTime"
                    | "Date"
                    | "Zone"
                    | "ZonedDateTime"
                    | "Instant"
                    | "Url"
                    | "Envelope"
                    | "Address"
                    | "Message"
                    | "Attachment"
            ) =>
        {
            let ty = type_name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(type_name);
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, v)| {
                    let field = name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name);
                    let shown = display(v).unwrap_or_else(|| match v {
                        CtValue::List(xs) => {
                            let inner: Vec<String> = xs
                                .iter()
                                .map(|x| display(x).unwrap_or_else(|| x.jet_show()))
                                .collect();
                            format!("[{}]", inner.join(", "))
                        }
                        _ => v.jet_show(),
                    });
                    format!("{field}: {shown}")
                })
                .collect();
            Some(format!("{ty}({})", parts.join(", ")))
        }
        CtValue::Present(inner) => {
            // Option payloads in Address.display etc. show as the inner jet_show
            // (null for None is handled by jet_show); keep Some unwrapped in
            // nested core-struct display via the field map above.
            display(inner)
        }
        CtValue::Failed(CtReport::Clean(_)) => Some("null".to_string()),
        _ => {
            let CtValue::Float(measured) = field(value, "Measurement", "value")? else {
                return None;
            };
            let CtValue::Float(uncertainty) = field(value, "Measurement", "uncertainty")? else {
                return None;
            };
            Some(super::measurement_kernel::jet_measurement_kernel_show((
                measured.as_f64(),
                uncertainty.as_f64(),
            )))
        }
    }
}

fn one<'a>(
    args: &'a [CtValue],
    index: usize,
    module: &str,
    method: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    args.get(index).ok_or_else(|| {
        unsupported(
            &format!("{module}.{method}(): missing arg {index}"),
            span,
        )
    })
}

fn string_arg<'a>(args: &'a [CtValue], index: usize, span: Span) -> Result<&'a str, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value),
        _ => Err(unsupported("Core call expected a String argument", span)),
    }
}

fn int_arg(args: &[CtValue], index: usize, span: Span) -> Result<i64, Diagnostic> {
    let value = args
        .get(index)
        .ok_or_else(|| unsupported("Core call is missing an Int argument", span))?;
    as_int(value, span)
}

fn float_arg(args: &[CtValue], index: usize, span: Span) -> Result<f64, Diagnostic> {
    let value = args
        .get(index)
        .ok_or_else(|| unsupported("Core call is missing a Float argument", span))?;
    as_float(value, span)
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

fn ui_role(variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: "UiAriaRole".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn ui_kind(variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: "UiNodeKind".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn ui_point(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure("Point", vec![
        ("x", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
        ("y", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
    ]))
}

fn ui_size(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure("Size", vec![
        ("width", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
        ("height", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
    ]))
}

fn ui_rect(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure("Rect", vec![
        ("x", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
        ("y", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
        ("width", CtValue::Float(CtFloat::f64(float_arg(args, 2, span)?))),
        ("height", CtValue::Float(CtFloat::f64(float_arg(args, 3, span)?))),
    ]))
}

fn ui_constraint(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure("SizeConstraint", vec![
        ("min_width", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
        ("min_height", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
        ("max_width", CtValue::Float(CtFloat::f64(float_arg(args, 2, span)?))),
        ("max_height", CtValue::Float(CtFloat::f64(float_arg(args, 3, span)?))),
    ]))
}

fn ui_node_value(
    label: String,
    width: f64,
    height: f64,
    role: Option<CtValue>,
    color: Option<String>,
    kind: &str,
    children: Vec<CtValue>,
) -> CtValue {
    structure("UiNode", vec![
        ("label", CtValue::Str(label)),
        ("width", CtValue::Float(CtFloat::f64(width))),
        ("height", CtValue::Float(CtFloat::f64(height))),
        ("role", role.map_or(CtValue::absent(Type::Named("UiAriaRole".to_string())), |role| CtValue::Present(Box::new(role)))),
        ("color", color.map_or(CtValue::absent(Type::String), |color| CtValue::Present(Box::new(CtValue::Str(color))))),
        ("kind", ui_kind(kind)),
        ("children", CtValue::List(children)),
    ])
}

fn ui_node(
    args: &[CtValue],
    span: Span,
    role: Option<CtValue>,
    color: Option<String>,
    kind: &str,
) -> EvalResult {
    Ok(ui_node_value(
        string_arg(args, 0, span)?.to_string(),
        float_arg(args, 1, span)?,
        float_arg(args, 2, span)?,
        role,
        color,
        kind,
        Vec::new(),
    ))
}

fn ui_node_role(args: &[CtValue], span: Span) -> EvalResult {
    let role = args.get(3).cloned().ok_or_else(|| unsupported("core.ui.node_role(): missing role", span))?;
    let kind = match &role {
        CtValue::Enum { variant, .. } if variant == "Button" => "Button",
        CtValue::Enum { variant, .. } if variant == "TextInput" => "TextInput",
        _ => "Custom",
    };
    ui_node(args, span, Some(role), None, kind)
}

fn ui_node_color(args: &[CtValue], span: Span) -> EvalResult {
    let color = string_arg(args, 3, span)?.to_string();
    ui_node(args, span, Some(ui_role("Label")), Some(color), "Custom")
}

fn ui_text(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?.to_string();
    Ok(ui_node_value(
        text.clone(),
        text.chars().count() as f64,
        1.0,
        Some(ui_role("Label")),
        None,
        "Text",
        Vec::new(),
    ))
}

fn ui_button(args: &[CtValue], span: Span) -> EvalResult {
    let label = string_arg(args, 0, span)?.to_string();
    Ok(ui_node_value(
        label.clone(),
        label.chars().count() as f64 + 4.0,
        1.0,
        Some(ui_role("Button")),
        None,
        "Button",
        Vec::new(),
    ))
}

fn ui_box(args: &[CtValue], span: Span) -> EvalResult {
    let children = match args.first() {
        Some(CtValue::List(children)) => children.clone(),
        _ => return Err(unsupported("core.ui.box() needs [UiNode]", span)),
    };
    let mut width = 0.0_f64;
    let mut height = 0.0_f64;
    for child in &children {
        width = width.max(as_float(field(child, "UiNode", "width").ok_or_else(|| unsupported("core.ui.box() needs UiNode children", span))?, span)?);
        height += as_float(field(child, "UiNode", "height").ok_or_else(|| unsupported("core.ui.box() needs UiNode children", span))?, span)?;
    }
    Ok(ui_node_value(String::new(), width, height, Some(ui_role("Container")), None, "Box", children))
}

fn ui_key_event(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: "InputEvent".to_string(),
        variant: "Key".to_string(),
        args: vec![(Some("code".to_string()), CtValue::Str(string_arg(args, 0, span)?.to_string()))],
    })
}

fn ui_resize_event(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: "InputEvent".to_string(),
        variant: "Resize".to_string(),
        args: vec![(Some("size".to_string()), ui_size(args, span)?)],
    })
}

fn raylib_color(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure("RaylibColor", vec![
        ("r", CtValue::Int(int_arg(args, 0, span)?)),
        ("g", CtValue::Int(int_arg(args, 1, span)?)),
        ("b", CtValue::Int(int_arg(args, 2, span)?)),
        ("a", CtValue::Int(int_arg(args, 3, span)?)),
    ]))
}

fn io_style_force(args: &[CtValue], span: Span) -> EvalResult {
    let style = string_arg(args, 0, span)?;
    let text = string_arg(args, 1, span)?;
    let code = match style {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    };
    Ok(CtValue::Str(code.map_or_else(|| text.to_string(), |code| format!("\u{1b}[{code}m{text}\u{1b}[0m"))))
}

fn net_error(operation: &str, address: Option<String>, message: String) -> CtValue {
    structure("NetError", vec![
        ("operation", CtValue::Str(operation.to_string())),
        ("address", address.map_or(CtValue::absent(Type::String), |value| CtValue::Present(Box::new(CtValue::Str(value))))),
        ("name", CtValue::absent(Type::String)),
        ("message", CtValue::Str(message)),
        ("os_code", CtValue::absent(Type::Int)),
    ])
}

fn net_ip_addr(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    let input = text.to_string();
    Ok(match super::net_pure_kernel::jet_net_pure_parse_ip(&input) {
        Ok(address) => CtValue::Present(Box::new(structure("IPAddr", vec![("text", CtValue::Str(address.to_string()))]))),
        Err(error) => CtValue::failed(Box::new(net_error(
            "parse IP address",
            Some(text.to_string()),
            format!("invalid IP address `{text}`: {error}"),
        ))),
    })
}

fn net_ip_is_ipv4(args: &[CtValue], span: Span) -> EvalResult {
    let text = match field(one(args, 0, "core.net", "ip_is_ipv4", span)?, "IPAddr", "text") {
        Some(CtValue::Str(text)) => text,
        _ => return Err(unsupported("malformed IPAddr value", span)),
    };
    let input = text.to_string();
    let address = match super::net_pure_kernel::jet_net_pure_parse_ip(&input) {
        Ok(address) => address,
        Err(_) => return Ok(CtValue::Bool(false)),
    };
    Ok(CtValue::Bool(
        super::net_pure_kernel::jet_net_pure_ip_is_ipv4(&address),
    ))
}

fn net_socket_addr_parse(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    let input = text.to_string();
    Ok(match super::net_pure_kernel::jet_net_pure_parse_socket_addr(&input) {
        Ok(address) => CtValue::Present(Box::new(structure("SocketAddr", vec![
            (
                "host",
                CtValue::Str(super::net_pure_kernel::jet_net_pure_socket_host(&address)),
            ),
            (
                "port",
                CtValue::Int(super::net_pure_kernel::jet_net_pure_socket_port(&address)),
            ),
            (
                "text",
                CtValue::Str(super::net_pure_kernel::jet_net_pure_socket_to_string(&address)),
            ),
        ]))),
        Err(error) => CtValue::failed(Box::new(net_error(
            "parse socket address",
            Some(text.to_string()),
            format!("invalid socket address `{text}`: {error}"),
        ))),
    })
}

fn net_value_field(args: &[CtValue], type_name: &str, name: &str, span: Span) -> EvalResult {
    field(one(args, 0, "core.net", name, span)?, type_name, name)
        .cloned()
        .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))
}

fn net_string_field(args: &[CtValue], type_name: &str, name: &str, span: Span) -> EvalResult {
    match net_value_field(args, type_name, name, span)? {
        CtValue::Str(value) => Ok(CtValue::Str(value)),
        _ => Err(unsupported(&format!("malformed {type_name}.{name} value"), span)),
    }
}

fn net_udp_packet_data(args: &[CtValue], span: Span) -> EvalResult {
    match net_value_field(args, "UDPPacket", "data", span)? {
        CtValue::Bytes(value) => Ok(CtValue::Str(String::from_utf8_lossy(&value).into_owned())),
        _ => Err(unsupported("malformed UDPPacket.data value", span)),
    }
}

fn crypto_secret(type_name: &str, bytes: Vec<u8>) -> CtValue {
    structure(type_name, vec![("bytes", CtValue::Bytes(bytes))])
}

fn crypto_error(reason: &str) -> CtValue {
    structure("CryptoError", vec![("reason", CtValue::Str(reason.to_string()))])
}

fn crypto_hkdf(args: &[CtValue], span: Span) -> EvalResult {
    let length = int_arg(args, 3, span)?;
    if !(0..=8_160).contains(&length) {
        return Ok(CtValue::failed(Box::new(crypto_error("HKDF-SHA256 output length must be 0..8160"))));
    }
    let bytes = crate::Comptime::CryptoLite::hkdf_sha256(
        &as_bytes(one(args, 0, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        &as_bytes(one(args, 1, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        &as_bytes(one(args, 2, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        length as usize,
    );
    Ok(CtValue::Present(Box::new(crypto_secret("Secret", bytes))))
}

fn crypto_ed25519_verify(args: &[CtValue], span: Span) -> EvalResult {
    let public = as_bytes(one(args, 0, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let message = as_bytes(one(args, 1, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let signature = as_bytes(one(args, 2, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    if public.len() != 32 {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: public must be exactly 32; got {}",
            public.len()
        )))));
    }
    if signature.len() != 64 {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: signature must be exactly 64; got {}",
            signature.len()
        )))));
    }
    if message.len() > 1_073_741_824 {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: message must be at most 1073741824; got {}",
            message.len()
        )))));
    }
    let public: [u8; 32] = public.try_into().expect("length checked");
    let signature: [u8; 64] = signature.try_into().expect("length checked");
    match crate::Comptime::CryptoLite::ed25519_verify_strict(&public, &message, &signature) {
        Ok(valid) => Ok(CtValue::Present(Box::new(CtValue::Bool(valid)))),
        Err(()) => Ok(CtValue::failed(Box::new(crypto_error(
            "expert.ed25519_verify_strict: Ed25519 public key is not canonical",
        )))),
    }
}

fn crypto_ed25519_sign(args: &[CtValue], span: Span) -> EvalResult {
    let seed = as_bytes(one(args, 0, "core.crypto.expert", "ed25519_sign", span)?, span)?;
    let message = as_bytes(one(args, 1, "core.crypto.expert", "ed25519_sign", span)?, span)?;
    if seed.len() != 32 {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.ed25519_sign: seed must be exactly 32; got {}",
            seed.len()
        )))));
    }
    if message.len() > 1_073_741_824 {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.ed25519_sign: message must be at most 1073741824; got {}",
            message.len()
        )))));
    }
    let seed: [u8; 32] = seed.try_into().expect("length checked");
    let signature = crate::Comptime::CryptoLite::ed25519_sign(&seed, &message);
    Ok(CtValue::Present(Box::new(crypto_secret(
        "Signature",
        signature.to_vec(),
    ))))
}

fn crypto_aead_lengths(
    operation: &str,
    key: &[u8],
    nonce: &[u8],
    nonce_length: usize,
    input: &[u8],
    aad: &[u8],
    opening: bool,
) -> Option<CtValue> {
    if key.len() != 32 {
        return Some(CtValue::failed(Box::new(crypto_error(&format!(
            "{operation}: key must be exactly 32; got {}",
            key.len()
        )))));
    }
    let nonce_expected = if nonce_length == 24 {
        "exactly 24"
    } else {
        "exactly 12"
    };
    if nonce.len() != nonce_length {
        return Some(CtValue::failed(Box::new(crypto_error(&format!(
            "{operation}: nonce must be {nonce_expected}; got {}",
            nonce.len()
        )))));
    }
    let (minimum, maximum, label, expected) = if opening {
        (16usize, 1_073_741_840usize, "ciphertext", "16..=1073741840")
    } else {
        (0usize, 1_073_741_824usize, "plaintext", "at most 1073741824")
    };
    if input.len() < minimum || input.len() > maximum {
        return Some(CtValue::failed(Box::new(crypto_error(&format!(
            "{operation}: {label} must be {expected}; got {}",
            input.len()
        )))));
    }
    if aad.len() > 16_777_216 {
        return Some(CtValue::failed(Box::new(crypto_error(&format!(
            "{operation}: aad must be at most 16777216; got {}",
            aad.len()
        )))));
    }
    None
}

fn crypto_aead_seal(
    args: &[CtValue],
    span: Span,
    operation: &str,
    nonce_length: usize,
    aes: bool,
) -> EvalResult {
    let key = as_bytes(one(args, 0, "core.crypto.expert", operation, span)?, span)?;
    let nonce = as_bytes(one(args, 1, "core.crypto.expert", operation, span)?, span)?;
    let plaintext = as_bytes(one(args, 2, "core.crypto.expert", operation, span)?, span)?;
    let aad = as_bytes(one(args, 3, "core.crypto.expert", operation, span)?, span)?;
    if let Some(error) =
        crypto_aead_lengths(operation, &key, &nonce, nonce_length, &plaintext, &aad, false)
    {
        return Ok(error);
    }
    let sealed = if aes {
        crate::Comptime::CryptoLite::aes256gcm_seal(&key, &nonce, &plaintext, &aad)
    } else {
        crate::Comptime::CryptoLite::xchacha20poly1305_seal(&key, &nonce, &plaintext, &aad)
    };
    match sealed {
        Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
        Err(()) => Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "Jet could not preserve a cryptographic invariant; incident expert-{}-seal",
            if aes { "aes" } else { "xchacha" }
        ))))),
    }
}

fn crypto_aead_open(
    args: &[CtValue],
    span: Span,
    operation: &str,
    nonce_length: usize,
) -> EvalResult {
    let key = as_bytes(one(args, 0, "core.crypto.expert", operation, span)?, span)?;
    let nonce = as_bytes(one(args, 1, "core.crypto.expert", operation, span)?, span)?;
    let ciphertext = as_bytes(one(args, 2, "core.crypto.expert", operation, span)?, span)?;
    let aad = as_bytes(one(args, 3, "core.crypto.expert", operation, span)?, span)?;
    if let Some(error) =
        crypto_aead_lengths(operation, &key, &nonce, nonce_length, &ciphertext, &aad, true)
    {
        return Ok(error);
    }
    let opened = if nonce_length == 12 {
        crate::Comptime::CryptoLite::aes256gcm_open(&key, &nonce, &ciphertext, &aad)
    } else {
        crate::Comptime::CryptoLite::xchacha20poly1305_open(&key, &nonce, &ciphertext, &aad)
    };
    match opened {
        Ok(bytes) => Ok(CtValue::Present(Box::new(CtValue::Bytes(bytes)))),
        Err(()) => Ok(CtValue::failed(Box::new(crypto_error(
            "encrypted data could not be opened",
        )))),
    }
}

fn crypto_argon2id(args: &[CtValue], span: Span) -> EvalResult {
    let password = match field(
        one(args, 0, "core.crypto.expert", "argon2id", span)?,
        "Secret",
        "bytes",
    ) {
        Some(CtValue::Bytes(bytes)) => bytes.clone(),
        Some(CtValue::List(bytes)) => {
            as_bytes(&CtValue::List(bytes.clone()), span)?
        }
        _ => {
            return Err(unsupported(
                "core.crypto.expert.argon2id() needs a Secret password",
                span,
            ))
        }
    };
    let salt = as_bytes(one(args, 1, "core.crypto.expert", "argon2id", span)?, span)?;
    let memory_kib = int_arg(args, 2, span)?;
    let iterations = int_arg(args, 3, span)?;
    let lanes = int_arg(args, 4, span)?;
    let output_length = int_arg(args, 5, span)?;
    if password.len() > 1_048_576 {
        return Ok(CtValue::failed(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        ))));
    }
    if !(8..=64).contains(&salt.len()) {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.argon2id: salt must be 8..=64; got {}",
            salt.len()
        )))));
    }
    if !(8_192..=262_144).contains(&memory_kib)
        || !(1..=10).contains(&iterations)
        || !(1..=8).contains(&lanes)
        || memory_kib < 8 * lanes
        || memory_kib.checked_mul(iterations).is_none_or(|value| value > 1_048_576)
    {
        return Ok(CtValue::failed(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        ))));
    }
    if !(16..=64).contains(&output_length) {
        return Ok(CtValue::failed(Box::new(crypto_error(&format!(
            "expert.argon2id: output length must be 16..64; got {output_length}"
        )))));
    }
    match crate::Comptime::CryptoLite::argon2id(
        &password,
        &salt,
        memory_kib as u32,
        iterations as u32,
        lanes as u32,
        output_length as usize,
    ) {
        Ok(bytes) => Ok(CtValue::Present(Box::new(crypto_secret("Secret", bytes)))),
        Err(()) => Ok(CtValue::failed(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        )))),
    }
}

fn crypto_x25519(args: &[CtValue], span: Span) -> EvalResult {
    let secret = as_bytes(one(args, 0, "core.crypto.expert", "x25519_raw", span)?, span)?;
    let public = as_bytes(one(args, 1, "core.crypto.expert", "x25519_raw", span)?, span)?;
    if secret.len() != 32 || public.len() != 32 {
        return Ok(CtValue::failed(Box::new(crypto_error("X25519 keys must contain exactly 32 bytes"))));
    }
    let shared = crate::Comptime::CryptoLite::x25519(&secret, &public).expect("length checked");
    if shared == [0; 32] {
        return Ok(CtValue::failed(Box::new(crypto_error("X25519 peer key does not contribute to a shared secret"))));
    }
    Ok(CtValue::Present(Box::new(crypto_secret("Secret", shared.to_vec()))))
}

fn crypto_extract(args: &[CtValue], index: usize, type_name: &str, span: Span) -> EvalResult {
    let value = one(args, index, "core.crypto.expert", "secret_bytes", span)?;
    match field(value, type_name, "bytes") {
        Some(CtValue::Bytes(bytes)) => Ok(CtValue::Bytes(bytes.clone())),
        Some(CtValue::List(bytes)) => as_bytes(&CtValue::List(bytes.clone()), span).map(CtValue::Bytes),
        _ => Err(unsupported(&format!("malformed {type_name} value"), span)),
    }
}

fn field<'a>(value: &'a CtValue, type_name: &str, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct {
            type_name: actual,
            fields,
        } if actual == type_name => fields
            .iter()
            .find(|(field_name, _)| field_name == name)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn value_field(
    value: &CtValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> EvalResult {
    field(value, type_name, name)
        .cloned()
        .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))
}

fn int_field(
    value: &CtValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    as_int(
        field(value, type_name, name)
            .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))?,
        span,
    )
}

fn string_field(
    value: &CtValue,
    type_name: &str,
    name: &str,
    span: Span,
) -> EvalResult {
    match field(value, type_name, name) {
        Some(CtValue::Str(value)) => Ok(CtValue::Str(value.clone())),
        _ => Err(unsupported(
            &format!("malformed {type_name}.{name} value"),
            span,
        )),
    }
}

// ── MIME ───────────────────────────────────────────────────────────────────

fn parse_mime(input: &str) -> Result<CtValue, String> {
    let parts = mime_kernel::jet_mime_parse_parts(input)?;
    let params = parts
        .params
        .into_iter()
        .map(|(key, value)| CtValue::List(vec![CtValue::Str(key), CtValue::Str(value)]))
        .collect();
    Ok(structure(
        "Mime",
        vec![
            ("top", CtValue::Str(parts.top)),
            ("sub", CtValue::Str(parts.sub)),
            ("params", CtValue::List(params)),
        ],
    ))
}

fn mime_parts(value: &CtValue, span: Span) -> Result<mime_kernel::JetMimeParts, Diagnostic> {
    let CtValue::Str(top) = field(value, "Mime", "top")
        .ok_or_else(|| unsupported("malformed Mime.top value", span))?
    else {
        return Err(unsupported("malformed Mime.top value", span));
    };
    let CtValue::Str(sub) = field(value, "Mime", "sub")
        .ok_or_else(|| unsupported("malformed Mime.sub value", span))?
    else {
        return Err(unsupported("malformed Mime.sub value", span));
    };
    let Some(CtValue::List(values)) = field(value, "Mime", "params") else {
        return Err(unsupported("malformed Mime.params value", span));
    };
    let mut params = Vec::with_capacity(values.len());
    for value in values {
        let CtValue::List(pair) = value else {
            return Err(unsupported("malformed Mime parameter", span));
        };
        let [CtValue::Str(key), CtValue::Str(value)] = pair.as_slice() else {
            return Err(unsupported("malformed Mime parameter", span));
        };
        params.push((key.clone(), value.clone()));
    }
    Ok(mime_kernel::JetMimeParts {
        top: top.clone(),
        sub: sub.clone(),
        params,
    })
}

fn mime_essence(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let parts = mime_parts(value, span)?;
    Ok(mime_kernel::jet_mime_essence(&parts.top, &parts.sub))
}

fn mime_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let parts = mime_parts(value, span)?;
    Ok(mime_kernel::jet_mime_to_string(
        &parts.top,
        &parts.sub,
        &parts.params,
    ))
}

fn mime_param(value: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let parts = mime_parts(value, span)?;
    Ok(option_string(mime_kernel::jet_mime_param(
        &parts.params,
        string_arg(args, 0, span)?,
    )))
}

fn mime_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match parse_mime(string_arg(args, 0, span)?) {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}

fn mime_from_extension(args: &[CtValue], span: Span) -> EvalResult {
    Ok(option_string(mime_kernel::jet_mime_from_extension(
        string_arg(args, 0, span)?,
    )))
}

fn mime_extension(args: &[CtValue], span: Span) -> EvalResult {
    Ok(option_string(mime_kernel::jet_extension_from_mime(
        string_arg(args, 0, span)?,
    )))
}

fn option_string(value: Option<&str>) -> CtValue {
    value.map_or(CtValue::absent(Type::String), |value| {
        CtValue::Present(Box::new(CtValue::Str(value.to_string())))
    })
}

// ── Civil time: CtValue adapters for the shared Prelude kernel ──────────────

#[derive(Clone)]
struct Date {
    inner: super::time_kernel::JetDate,
    year: i64,
    month: i64,
    day: i64,
}

impl Date {
    fn from_inner(inner: super::time_kernel::JetDate) -> Self {
        Self {
            year: inner.year(),
            month: inner.month(),
            day: inner.day(),
            inner,
        }
    }

    fn is_leap(year: i64) -> bool {
        super::time_kernel::JetDate::is_leap(year)
    }

    fn days_in_month(year: i64, month: i64) -> i64 {
        super::time_kernel::JetDate::days_in_month_of(year, month)
    }

    fn new(year: i64, month: i64, day: i64) -> Self {
        Self::from_inner(super::time_kernel::JetDate::new(year, month, day))
    }

    fn today_utc() -> Self {
        Self::from_inner(super::time_kernel::JetDate::today_utc())
    }

    fn parse(value: &str) -> Result<Self, String> {
        super::time_kernel::JetDate::parse(value).map(Self::from_inner)
    }

    fn day_number(&self) -> i64 {
        self.inner.to_day_number()
    }

    fn from_day_number(day: i64) -> Self {
        Self::from_inner(super::time_kernel::JetDate::from_day_number(day))
    }

    fn add_days(&self, days: i64) -> Self {
        Self::from_inner(self.inner.add_days(days))
    }

    fn add_months(&self, months: i64) -> Self {
        Self::from_inner(self.inner.add_months(months))
    }

    fn to_string_fmt(&self) -> String {
        self.inner.to_string_fmt()
    }

    fn value(self) -> CtValue {
        structure(
            "LocalDate",
            vec![
                ("year", CtValue::Int(self.year)),
                ("month", CtValue::Int(self.month)),
                ("day", CtValue::Int(self.day)),
            ],
        )
    }
}

#[derive(Clone)]
struct LocalTime {
    inner: super::time_kernel::JetLocalTime,
    hour: i64,
    minute: i64,
    second: i64,
}

impl LocalTime {
    fn from_inner(inner: super::time_kernel::JetLocalTime) -> Self {
        Self {
            hour: inner.hour(),
            minute: inner.minute(),
            second: inner.second(),
            inner,
        }
    }

    fn new(hour: i64, minute: i64, second: i64) -> Self {
        Self::from_inner(super::time_kernel::JetLocalTime::new(hour, minute, second))
    }

    fn parse(value: &str) -> Result<Self, String> {
        super::time_kernel::JetLocalTime::parse(value).map(Self::from_inner)
    }

    fn seconds(&self) -> i64 {
        self.inner.to_seconds()
    }

    fn to_string_fmt(&self) -> String {
        self.inner.to_string_fmt()
    }

    fn value(self) -> CtValue {
        structure(
            "LocalTime",
            vec![
                ("hour", CtValue::Int(self.hour)),
                ("minute", CtValue::Int(self.minute)),
                ("second", CtValue::Int(self.second)),
            ],
        )
    }
}

#[derive(Clone)]
struct DateTime {
    inner: super::time_kernel::JetDateTime,
    seconds: i64,
    nanos: u32,
}

impl DateTime {
    fn from_inner(inner: super::time_kernel::JetDateTime) -> Self {
        Self {
            seconds: inner.to_timestamp(),
            nanos: inner.nanosecond() as u32,
            inner,
        }
    }

    fn from_timestamp_ns(seconds: i64, nanos: u32) -> Self {
        Self::from_inner(super::time_kernel::JetDateTime::from_timestamp_ns(seconds, nanos))
    }

    fn from_parts(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        nanos: u32,
    ) -> Self {
        Self::from_inner(super::time_kernel::JetDateTime::from_parts(
            year, month, day, hour, minute, second, nanos,
        ))
    }

    fn date(&self) -> Date {
        Date::from_inner(self.inner.date())
    }

    fn time(&self) -> LocalTime {
        LocalTime::from_inner(self.inner.time())
    }

    fn total_ns(&self) -> i64 {
        self.seconds
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i64)
    }

    fn from_total_ns(total: i64) -> Self {
        Self::from_timestamp_ns(
            total.div_euclid(1_000_000_000),
            total.rem_euclid(1_000_000_000) as u32,
        )
    }

    fn plus_ns(&self, ns: i64) -> Self {
        Self::from_inner(self.inner.plus_duration_ns(ns))
    }

    fn align(&self, unit: &str, method: &str) -> Self {
        let unit = unit.to_string();
        let inner = match method {
            "round" => self.inner.round(&unit),
            "ceil" => self.inner.ceil(&unit),
            "floor" => self.inner.floor(&unit),
            _ => self.inner.truncate(&unit),
        };
        Self::from_inner(inner)
    }

    fn value(self) -> CtValue {
        datetime_value(self.seconds, self.nanos)
    }
}

#[derive(Clone)]
struct Zone {
    inner: super::time_kernel::JetZone,
    name: String,
    offset: i64,
}

impl Zone {
    fn from_inner(inner: super::time_kernel::JetZone) -> Self {
        let name = inner.name();
        let offset = inner.offset_at_utc(0);
        Self { inner, name, offset }
    }

    fn utc() -> Self {
        Self::from_inner(super::time_kernel::JetZone::utc())
    }

    fn parse_name(name: &str) -> Result<Self, String> {
        super::time_kernel::JetZone::named(&name.to_string()).map(Self::from_inner)
    }

    fn value(self) -> CtValue {
        structure(
            "Zone",
            vec![
                ("name", CtValue::Str(self.name)),
                ("offset", CtValue::Int(self.offset)),
            ],
        )
    }
}

#[derive(Clone)]
struct ZonedDateTime {
    inner: super::time_kernel::JetZonedDateTime,
    instant: DateTime,
    zone: Zone,
}

impl ZonedDateTime {
    fn from_inner(inner: super::time_kernel::JetZonedDateTime) -> Self {
        let instant = DateTime::from_inner(inner.to_datetime());
        let zone = Zone::from_inner(inner.zone());
        Self { inner, instant, zone }
    }

    fn from_datetime(instant: DateTime, zone: Zone) -> Self {
        Self::from_inner(instant.inner.in_zone(&zone.inner))
    }

    fn from_local(date: Date, time: LocalTime, zone: Zone) -> Self {
        Self::from_inner(super::time_kernel::JetZonedDateTime::from_local(
            &date.inner,
            &time.inner,
            &zone.inner,
        ))
    }

    fn offset_seconds(&self) -> i64 {
        self.inner.offset_seconds()
    }

    fn is_dst(&self) -> bool {
        self.inner.is_dst()
    }

    fn date(&self) -> Date {
        Date::from_inner(self.inner.date())
    }

    fn time(&self) -> LocalTime {
        LocalTime::from_inner(self.inner.time())
    }

    fn to_string_fmt(&self) -> String {
        self.inner.to_string_fmt()
    }

    fn value(self) -> CtValue {
        structure(
            "ZonedDateTime",
            vec![("instant", self.instant.value()), ("zone", self.zone.value())],
        )
    }
}

fn zone_utc() -> CtValue {
    Zone::utc().value()
}

fn decimal_from_str(args: &[CtValue], span: Span) -> EvalResult {
    match crate::Numeric::CtDecimal::from_str(string_arg(args, 0, span)?) {
        Ok(decimal) => Ok(decimal.to_value()),
        Err(error) => Err(unsupported(&error, span)),
    }
}

fn fraction_new(args: &[CtValue], span: Span) -> EvalResult {
    let numerator = match args.first() {
        Some(CtValue::Int(n)) => *n,
        _ => return Err(unsupported("a ratio top that is not a whole number", span)),
    };
    let denominator = match args.get(1) {
        Some(CtValue::Int(n)) => *n,
        _ => return Err(unsupported("a ratio bottom that is not a whole number", span)),
    };
    Ok(match crate::Numeric::CtFraction::new(numerator, denominator) {
        Some(value) => CtValue::Present(Box::new(value.to_value())),
        None => CtValue::absent(crate::AST::Type::Named(crate::Syntax::TYPE_FRACTION.to_string())),
    })
}

fn fraction_from_value(value: &CtValue, span: Span) -> Result<crate::Numeric::CtFraction, Diagnostic> {
    crate::Numeric::CtFraction::from_value(value).map_err(|error| unsupported(&error, span))
}

fn decimal_from_value(value: &CtValue, span: Span) -> Result<crate::Numeric::CtDecimal, Diagnostic> {
    crate::Numeric::CtDecimal::from_value(value).map_err(|error| unsupported(&error, span))
}

fn zone_named(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match Zone::parse_name(string_arg(args, 0, span)?) {
        Ok(zone) => CtValue::Present(Box::new(zone.value())),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}

fn zone_from_value(value: &CtValue, span: Span) -> Result<Zone, Diagnostic> {
    let name = match field(value, "Zone", "name") {
        Some(CtValue::Str(name)) => name,
        _ => return Err(unsupported("malformed Zone.name value", span)),
    };
    let _ = int_field(value, "Zone", "offset", span)?;
    Zone::parse_name(name).map_err(|error| unsupported(&error, span))
}

fn zoned_from_value(value: &CtValue, span: Span) -> Result<ZonedDateTime, Diagnostic> {
    let instant = match field(value, "ZonedDateTime", "instant") {
        Some(instant) => datetime_from_value(instant, span)?,
        None => {
            return Err(unsupported("malformed ZonedDateTime.instant value", span));
        }
    };
    let zone = match field(value, "ZonedDateTime", "zone") {
        Some(zone) => zone_from_value(zone, span)?,
        None => {
            return Err(unsupported("malformed ZonedDateTime.zone value", span));
        }
    };
    Ok(ZonedDateTime::from_datetime(instant, zone))
}

fn zoned_from_datetime(args: &[CtValue], span: Span) -> EvalResult {
    let instant = datetime_from_value(
        args.get(0)
            .ok_or_else(|| unsupported("time.zoned expects a DateTime", span))?,
        span,
    )?;
    let zone = zone_from_value(
        args.get(1)
            .ok_or_else(|| unsupported("time.zoned expects a Zone", span))?,
        span,
    )?;
    Ok(ZonedDateTime::from_datetime(instant, zone).value())
}

fn zoned_from_local(args: &[CtValue], span: Span) -> EvalResult {
    let date = date_from_value(
        args.get(0)
            .ok_or_else(|| unsupported("time.zoned_local expects a LocalDate", span))?,
        "LocalDate",
        span,
    )?;
    let time = local_time_from_value(
        args.get(1)
            .ok_or_else(|| unsupported("time.zoned_local expects a LocalTime", span))?,
        span,
    )?;
    let zone = zone_from_value(
        args.get(2)
            .ok_or_else(|| unsupported("time.zoned_local expects a Zone", span))?,
        span,
    )?;
    Ok(ZonedDateTime::from_local(date, time, zone).value())
}

fn format_zoned_pattern(pattern: &str, zoned: ZonedDateTime) -> String {
    zoned.inner.format_pattern(&pattern.to_string())
}

fn date_from_value(value: &CtValue, type_name: &str, span: Span) -> Result<Date, Diagnostic> {
    Ok(Date::new(
        int_field(value, type_name, "year", span)?,
        int_field(value, type_name, "month", span)?,
        int_field(value, type_name, "day", span)?,
    ))
}

fn local_time_from_value(value: &CtValue, span: Span) -> Result<LocalTime, Diagnostic> {
    Ok(LocalTime::new(
        int_field(value, "LocalTime", "hour", span)?,
        int_field(value, "LocalTime", "minute", span)?,
        int_field(value, "LocalTime", "second", span)?,
    ))
}

fn datetime_from_value(value: &CtValue, span: Span) -> Result<DateTime, Diagnostic> {
    Ok(DateTime::from_timestamp_ns(
        int_field(value, "DateTime", "secs", span)?,
        int_field(value, "DateTime", "nanos", span).unwrap_or(0) as u32,
    ))
}

fn date_add_period(date: Date, period: &CtValue, span: Span) -> Result<Date, Diagnostic> {
    let period = super::time_kernel::JetPeriod::new(
        int_field(period, "Period", "years", span)?,
        int_field(period, "Period", "months", span)?,
        int_field(period, "Period", "days", span)?,
    );
    Ok(Date::from_inner(date.inner.add_period(&period)))
}

fn date_truncate(date: Date, unit: &str) -> Date {
    Date::from_inner(date.inner.truncate(&unit.to_string()))
}

fn format_time_pattern(pattern: &str, date: Date, time: LocalTime) -> String {
    super::time_kernel::jet_time_format_pattern(
        &pattern.to_string(),
        &date.inner,
        &time.inner,
        None,
    )
}

fn period_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    Ok(super::time_kernel::JetPeriod::new(
        int_field(value, "Period", "years", span)?,
        int_field(value, "Period", "months", span)?,
        int_field(value, "Period", "days", span)?,
    )
    .to_string_fmt())
}

fn datetime_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    Ok(datetime_from_value(value, span)?.inner.to_string_fmt())
}

fn date_value(year: i64, month: i64, day: i64) -> CtValue {
    Date::new(year, month, day).value()
}

fn date_new_call(args: &[CtValue], span: Span) -> EvalResult {
    Ok(date_value(
        int_arg(args, 0, span)?,
        int_arg(args, 1, span)?,
        int_arg(args, 2, span)?,
    ))
}

fn date_parse_call(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match Date::parse(string_arg(args, 0, span)?) {
        Ok(date) => CtValue::Present(Box::new(date.value())),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}

fn period_value(years: i64, months: i64, days: i64) -> CtValue {
    let period = super::time_kernel::JetPeriod::new(years, months, days);
    let (years, months, days) = period.components();
    structure(
        "Period",
        vec![
            ("years", CtValue::Int(years)),
            ("months", CtValue::Int(months)),
            ("days", CtValue::Int(days)),
        ],
    )
}

fn period(args: &[CtValue], span: Span) -> EvalResult {
    Ok(period_value(
        int_arg(args, 0, span)?,
        int_arg(args, 1, span)?,
        int_arg(args, 2, span)?,
    ))
}

fn period_unit(args: &[CtValue], span: Span, field_index: usize) -> EvalResult {
    let mut fields = [0_i64; 3];
    fields[field_index] = int_arg(args, 0, span)?;
    Ok(period_value(fields[0], fields[1], fields[2]))
}

fn datetime_value(seconds: i64, nanos: u32) -> CtValue {
    structure(
        "DateTime",
        vec![
            ("secs", CtValue::Int(seconds)),
            ("nanos", CtValue::Int(nanos as i64)),
        ],
    )
}

fn duration_value(ns: i64) -> CtValue {
    structure(
        crate::Syntax::DURATION_TYPE,
        vec![("ns", CtValue::Int(ns))],
    )
}

fn duration_ns(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match int_field(value, crate::Syntax::DURATION_TYPE, "ns", span) {
        Ok(ns) => Ok(ns),
        Err(_) => Ok(int_field(value, crate::Syntax::DURATION_TYPE, "ms", span)?
            .saturating_mul(1_000_000)),
    }
}

fn datetime_from_timestamp(args: &[CtValue], span: Span) -> EvalResult {
    Ok(DateTime::from_inner(super::time_kernel::JetDateTime::from_timestamp(
        int_arg(args, 0, span)?,
    ))
    .value())
}

fn datetime_from_unix_ms(args: &[CtValue], span: Span) -> EvalResult {
    Ok(DateTime::from_inner(super::time_kernel::JetDateTime::from_unix_ms(
        int_arg(args, 0, span)?,
    ))
    .value())
}

fn datetime_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match super::time_kernel::JetDateTime::parse_rfc3339(string_arg(args, 0, span)?) {
        Ok(datetime) => CtValue::Present(Box::new(DateTime::from_inner(datetime).value())),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}

fn datetime_parts(args: &[CtValue], span: Span) -> EvalResult {
    Ok(DateTime::from_parts(
        int_arg(args, 0, span)?,
        int_arg(args, 1, span)?,
        int_arg(args, 2, span)?,
        int_arg(args, 3, span)?,
        int_arg(args, 4, span)?,
        int_arg(args, 5, span)?,
        0,
    )
    .value())
}

fn local_time_parts(args: &[CtValue], span: Span) -> EvalResult {
    Ok(LocalTime::new(
        int_arg(args, 0, span)?,
        int_arg(args, 1, span)?,
        int_arg(args, 2, span)?,
    )
    .value())
}

fn time_days_in_month(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Int(Date::days_in_month(
        int_arg(args, 0, span)?,
        int_arg(args, 1, span)?,
    )))
}

fn time_is_leap_year(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Bool(Date::is_leap(int_arg(args, 0, span)?)))
}

fn duration_ctor(method: &str, args: &[CtValue], span: Span) -> EvalResult {
    let unit = crate::Syntax::duration_unit_for_constructor(method)
        .ok_or_else(|| unsupported(&format!("unknown duration constructor `{method}`"), span))?;
    let scale = match unit {
        "Nanoseconds" => 1_i64,
        "Microseconds" => 1_000,
        "Milliseconds" => 1_000_000,
        "Seconds" => 1_000_000_000,
        "Minutes" => 60_000_000_000,
        "Hours" => 3_600_000_000_000,
        _ => unreachable!("closed duration unit set"),
    };
    let (ns, reason) = match args.first() {
        Some(CtValue::Int(n)) => (
            super::duration_kernel::jet_duration_kernel_from_int(*n, scale),
            super::duration_kernel::jet_duration_kernel_int_error_reason(),
        ),
        Some(CtValue::Float(n)) => {
            (
                super::duration_kernel::jet_duration_kernel_from_float(n.as_f64(), scale),
                super::duration_kernel::jet_duration_kernel_float_error_reason(),
            )
        }
        _ => (
            None,
            super::duration_kernel::jet_duration_kernel_float_error_reason(),
        ),
    };
    Ok(match ns {
        Some(ns) => CtValue::Present(Box::new(duration_value(ns))),
        None => CtValue::failed(Box::new(structure(
            crate::Syntax::DURATION_RANGE_ERROR_TYPE,
            vec![(
                "reason",
                CtValue::Str(reason.to_string()),
            )],
        ))),
    })
}

fn local_time_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match LocalTime::parse(string_arg(args, 0, span)?) {
        Ok(time) => CtValue::Present(Box::new(time.value())),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
    })
}

fn measurement(args: &[CtValue], span: Span) -> EvalResult {
    let (value, uncertainty) = super::measurement_kernel::jet_measurement_kernel_new(
        float_arg(args, 0, span)?,
        float_arg(args, 1, span)?,
    );
    Ok(structure(
        "Measurement",
        vec![
            ("value", CtValue::Float(CtFloat::f64(value))),
            ("uncertainty", CtValue::Float(CtFloat::f64(uncertainty))),
        ],
    ))
}

fn measurement_arithmetic(
    left: &CtValue,
    method: &str,
    right: &CtValue,
    span: Span,
) -> EvalResult {
    let left_value = match field(left, "Measurement", "value") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.value value", span)),
    };
    let left_uncertainty = match field(left, "Measurement", "uncertainty") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.uncertainty value", span)),
    };
    let right_value = match field(right, "Measurement", "value") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.value value", span)),
    };
    let right_uncertainty = match field(right, "Measurement", "uncertainty") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.uncertainty value", span)),
    };
    let left = (left_value, left_uncertainty);
    let right = (right_value, right_uncertainty);
    let (value, uncertainty) = match method {
        "add" => super::measurement_kernel::jet_measurement_kernel_add(left, right),
        "sub" => super::measurement_kernel::jet_measurement_kernel_sub(left, right),
        "mul" => super::measurement_kernel::jet_measurement_kernel_mul(left, right),
        "div" => super::measurement_kernel::jet_measurement_kernel_div(left, right),
        _ => unreachable!(),
    };
    Ok(structure(
        "Measurement",
        vec![
            ("value", CtValue::Float(CtFloat::f64(value))),
            ("uncertainty", CtValue::Float(CtFloat::f64(uncertainty))),
        ],
    ))
}

// ── XML canonicalization ───────────────────────────────────────────────────

fn xml_canonical(args: &[CtValue], span: Span) -> EvalResult {
    let tree = one(args, 0, "core.encoding.xml", "canonical", span)?;
    let options = one(args, 1, "core.encoding.xml", "canonical", span)?;
    let value = match crate::Comptime::EncodingLite::xml_from_ct(tree) {
        Ok(value) => value,
        Err(_) => {
            return Ok(CtValue::failed(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            ))))
        }
    };
    let (mode, comments, inclusive_prefixes) = xml_canonical_options(options, span)?;
    let canonical = jet_foundation::XmlKernel::canonical_document(
        &value,
        &jet_foundation::XmlPull::CanonicalOptions {
            mode,
            comments,
            inclusive_prefixes,
        },
    );
    Ok(match canonical {
        Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        Err(error) => CtValue::failed(Box::new(
            crate::Comptime::EncodingLite::xml_error_value(error),
        )),
    })
}

fn xml_canonical_options(
    value: &CtValue,
    span: Span,
) -> Result<(jet_foundation::XmlPull::CanonicalMode, bool, Vec<String>), Diagnostic> {
    let mode = match field(value, "XMLCanonical", "mode") {
        Some(CtValue::Enum { variant, .. }) if variant == "Inclusive11" => {
            jet_foundation::XmlPull::CanonicalMode::Inclusive11
        }
        Some(CtValue::Enum { variant, .. }) if variant == "Exclusive10" => {
            jet_foundation::XmlPull::CanonicalMode::Exclusive10
        }
        _ => return Err(unsupported("XML canonical mode is invalid", span)),
    };
    let comments = match field(value, "XMLCanonical", "comments") {
        Some(CtValue::Bool(value)) => *value,
        _ => return Err(unsupported("XML canonical comments flag is invalid", span)),
    };
    let inclusive_prefixes = match field(value, "XMLCanonical", "inclusive_prefixes") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| match value {
                CtValue::Str(value) => Ok(value.clone()),
                _ => Err(unsupported("XML canonical prefix must be String", span)),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("XML canonical prefixes are invalid", span)),
    };
    Ok((mode, comments, inclusive_prefixes))
}

fn xml_shape_error(reason: &str) -> CtValue {
    structure(
        "XMLError",
        vec![
            (
                "kind",
                CtValue::Enum {
                    type_name: "XMLReason".to_string(),
                    variant: "Shape".to_string(),
                    args: Vec::new(),
                },
            ),
            ("byte_offset", CtValue::absent(Type::Int)),
            ("line", CtValue::absent(Type::Int)),
            ("column", CtValue::absent(Type::Int)),
            ("path", CtValue::Str(String::new())),
            ("reason", CtValue::Str(reason.to_string())),
        ],
    )
}

// ── core.data.pivot_sum ────────────────────────────────────────────────────

impl<'a> crate::Comptime::Interpreter::Interp<'a> {
    #[allow(dead_code)]
    pub(in crate::Comptime) fn eval_pivot_sum(
        &mut self,
        args: Vec<CtValue>,
        span: Span,
    ) -> EvalResult {
        let Some(CtValue::List(rows)) = args.first() else {
            return Err(unsupported("`data.pivot_sum()` needs a row list", span));
        };
        let row_key = args
            .get(1)
            .ok_or_else(|| unsupported("`data.pivot_sum()` needs a row-key closure", span))?;
        let column_key = args
            .get(2)
            .ok_or_else(|| unsupported("`data.pivot_sum()` needs a column-key closure", span))?;
        let value = args
            .get(3)
            .ok_or_else(|| unsupported("`data.pivot_sum()` needs a value closure", span))?;
        let mut groups = BTreeMap::<String, (i64, f64)>::new();
        for row in rows {
            let left = self.call_closure(row_key, vec![row.clone()], span)?;
            let right = self.call_closure(column_key, vec![row.clone()], span)?;
            let key = format!(
                "{}|{}",
                crate::Comptime::Methods::as_string(&left, span)?,
                crate::Comptime::Methods::as_string(&right, span)?
            );
            let amount = self.call_closure(value, vec![row.clone()], span)?;
            let amount = as_float(&amount, span)?;
            let entry = groups.entry(key).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += amount;
        }
        Ok(CtValue::List(
            groups
                .into_iter()
                .map(|(key, (count, sum))| {
                    structure(
                        "DataGroup",
                        vec![
                            ("key", CtValue::Str(key)),
                            ("count", CtValue::Int(count)),
                            ("sum", CtValue::Float(CtFloat::f64(sum))),
                            (
                                "mean",
                                CtValue::Float(CtFloat::f64(if count == 0 {
                                    0.0
                                } else {
                                    sum / count as f64
                                })),
                            ),
                        ],
                    )
                })
                .collect(),
        ))
    }
}

// Email uses the shared Prelude kernel through `EmailAdapter`.

// ── D-APPROX1=A: CtValue adapters for the shared Prelude kernel ──────────────

fn hll_from_value(value: &CtValue, span: Span) -> Result<super::sketch_kernel::JetHyperLogLog, Diagnostic> {
    let CtValue::List(registers) = field(value, "HyperLogLog", "registers")
        .ok_or_else(|| unsupported("malformed HyperLogLog.registers value", span))?
    else {
        return Err(unsupported("malformed HyperLogLog.registers value", span));
    };
    let registers = registers
        .iter()
        .map(|value| match value {
            CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
            _ => Err(unsupported("malformed HyperLogLog register", span)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(super::sketch_kernel::JetHyperLogLog::from_registers(registers))
}

fn hll_value(sketch: &super::sketch_kernel::JetHyperLogLog) -> CtValue {
    structure(
        "HyperLogLog",
        vec![(
            "registers",
            CtValue::List(
                sketch
                    .registers()
                    .into_iter()
                    .map(|value| CtValue::Int(value as i64))
                    .collect(),
            ),
        )],
    )
}

fn hll_new() -> CtValue {
    hll_value(&super::sketch_kernel::JetHyperLogLog::new())
}

fn hll_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let sketch = hll_from_value(recv, span)?;
    sketch.add(string_arg(args, 0, span)?);
    Ok(hll_value(&sketch))
}

fn hll_count(recv: &CtValue, span: Span) -> EvalResult {
    Ok(CtValue::Int(hll_from_value(recv, span)?.count()))
}

fn tdigest_from_value(value: &CtValue, span: Span) -> Result<super::sketch_kernel::JetTDigest, Diagnostic> {
    let CtValue::List(items) = field(value, "TDigest", "centroids")
        .ok_or_else(|| unsupported("malformed TDigest.centroids value", span))?
    else {
        return Err(unsupported("malformed TDigest.centroids value", span));
    };
    let centroids = items
        .iter()
        .map(|item| match item {
            CtValue::List(pair) if pair.len() == 2 => {
                Ok((as_float(&pair[0], span)?, as_float(&pair[1], span)?))
            }
            _ => Err(unsupported("malformed TDigest centroid", span)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(super::sketch_kernel::JetTDigest::from_centroids(centroids))
}

fn tdigest_value(sketch: &super::sketch_kernel::JetTDigest) -> CtValue {
    structure(
        "TDigest",
        vec![(
            "centroids",
            CtValue::List(
                sketch
                    .centroids()
                    .into_iter()
                    .map(|(mean, weight)| {
                        CtValue::List(vec![
                            CtValue::Float(CtFloat::f64(mean)),
                            CtValue::Float(CtFloat::f64(weight)),
                        ])
                    })
                    .collect(),
            ),
        )],
    )
}

fn tdigest_new() -> CtValue {
    tdigest_value(&super::sketch_kernel::JetTDigest::new())
}

fn tdigest_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let sketch = tdigest_from_value(recv, span)?;
    sketch.add(float_arg(args, 0, span)?);
    Ok(tdigest_value(&sketch))
}

fn tdigest_quantile(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let sketch = tdigest_from_value(recv, span)?;
    Ok(CtValue::Float(CtFloat::f64(
        sketch.quantile(float_arg(args, 0, span)?),
    )))
}

fn cms_from_value(value: &CtValue, span: Span) -> Result<super::sketch_kernel::JetCountMinSketch, Diagnostic> {
    let CtValue::List(rows) = field(value, "CountMinSketch", "rows")
        .ok_or_else(|| unsupported("malformed CountMinSketch.rows value", span))?
    else {
        return Err(unsupported("malformed CountMinSketch.rows value", span));
    };
    if rows.len() != 4 {
        return Err(unsupported("malformed CountMinSketch.rows value", span));
    }
    let mut out = [[0; super::sketch_kernel::JET_CMS_COLS]; 4];
    for (row_index, row) in rows.iter().enumerate() {
        let CtValue::List(columns) = row else {
            return Err(unsupported("malformed CountMinSketch row", span));
        };
        if columns.len() != super::sketch_kernel::JET_CMS_COLS {
            return Err(unsupported("malformed CountMinSketch row", span));
        }
        for (column_index, cell) in columns.iter().enumerate() {
            let CtValue::Int(value) = cell else {
                return Err(unsupported("malformed CountMinSketch cell", span));
            };
            if !(0..=u32::MAX as i64).contains(value) {
                return Err(unsupported("malformed CountMinSketch cell", span));
            }
            out[row_index][column_index] = *value as u32;
        }
    }
    Ok(super::sketch_kernel::JetCountMinSketch::from_rows(out))
}

fn cms_value(sketch: &super::sketch_kernel::JetCountMinSketch) -> CtValue {
    structure(
        "CountMinSketch",
        vec![(
            "rows",
            CtValue::List(
                sketch
                    .rows()
                    .into_iter()
                    .map(|row| {
                        CtValue::List(
                            row.into_iter()
                                .map(|value| CtValue::Int(value as i64))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        )],
    )
}

fn cms_new() -> CtValue {
    cms_value(&super::sketch_kernel::JetCountMinSketch::new())
}

fn cms_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let sketch = cms_from_value(recv, span)?;
    sketch.add(string_arg(args, 0, span)?);
    Ok(cms_value(&sketch))
}

fn cms_count(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let sketch = cms_from_value(recv, span)?;
    Ok(CtValue::Int(sketch.count(string_arg(args, 0, span)?)))
}

fn reservoir_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::sketch_kernel::JetReservoirSampler, Diagnostic> {
    let capacity = int_field(value, "ReservoirSampler", "capacity", span)?;
    let count = int_field(value, "ReservoirSampler", "count", span)?;
    let rng = int_field(value, "ReservoirSampler", "rng", span)?;
    let CtValue::List(items) = value_field(value, "ReservoirSampler", "reservoir", span)? else {
        return Err(unsupported("malformed ReservoirSampler.reservoir value", span));
    };
    let reservoir = items
        .into_iter()
        .map(|item| match item {
            CtValue::Str(item) => Ok(item),
            _ => Err(unsupported("malformed ReservoirSampler item", span)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(super::sketch_kernel::JetReservoirSampler::from_parts(
        capacity as usize,
        reservoir,
        count as u64,
        rng as u64,
    ))
}

fn reservoir_value(sketch: &super::sketch_kernel::JetReservoirSampler) -> CtValue {
    let (capacity, reservoir, count, rng) = sketch.parts();
    structure(
        "ReservoirSampler",
        vec![
            ("capacity", CtValue::Int(capacity as i64)),
            ("count", CtValue::Int(count as i64)),
            ("rng", CtValue::Int(rng as i64)),
            (
                "reservoir",
                CtValue::List(reservoir.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    )
}

fn reservoir_new(args: &[CtValue], span: Span) -> EvalResult {
    Ok(reservoir_value(&super::sketch_kernel::JetReservoirSampler::new(
        int_arg(args, 0, span)?,
    )))
}

fn reservoir_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let sketch = reservoir_from_value(recv, span)?;
    sketch.add(string_arg(args, 0, span)?.to_string());
    Ok(reservoir_value(&sketch))
}

fn reservoir_sample(recv: &CtValue, span: Span) -> EvalResult {
    Ok(CtValue::List(
        reservoir_from_value(recv, span)?
            .sample()
            .into_iter()
            .map(CtValue::Str)
            .collect(),
    ))
}

// ── D-SOLVER-LIB1=A: CtValue adapter for the shared Prelude kernel ──────────

fn solver_from_value(value: &CtValue, span: Span) -> Result<super::solver_kernel::jet_std::Solver, Diagnostic> {
    Ok(super::solver_kernel::jet_std::Solver {
        seed: int_field(value, crate::Syntax::SOLVER_TYPE, "seed", span)?,
        checked: int_field(value, crate::Syntax::SOLVER_TYPE, "checked", span)?,
        failures: int_field(value, crate::Syntax::SOLVER_TYPE, "failures", span)?,
    })
}

fn solver_value(solver: super::solver_kernel::jet_std::Solver) -> CtValue {
    structure(
        crate::Syntax::SOLVER_TYPE,
        vec![
            ("seed", CtValue::Int(solver.seed)),
            ("checked", CtValue::Int(solver.checked)),
            ("failures", CtValue::Int(solver.failures)),
        ],
    )
}

fn solver_require_update(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let mut solver = solver_from_value(recv, span)?;
    let ok = as_bool(one(args, 0, "Solver", "require", span)?, span)?;
    super::solver_kernel::jet_solver_require(&mut solver, ok);
    Ok(solver_value(solver))
}

fn solver_failure_count(recv: &CtValue, span: Span) -> EvalResult {
    let solver = solver_from_value(recv, span)?;
    Ok(CtValue::Int(super::solver_kernel::jet_solver_failure_count(&solver)))
}

fn solver_status(recv: &CtValue, span: Span) -> EvalResult {
    let solver = solver_from_value(recv, span)?;
    Ok(CtValue::Str(super::solver_kernel::jet_solver_status(&solver)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_canonical_rejects_internal_bytes_with_aot_shape_reason() {
        let options = structure(
            "XMLCanonical",
            vec![
                (
                    "mode",
                    CtValue::Enum {
                        type_name: "XMLCanonicalMode".to_string(),
                        variant: "Inclusive11".to_string(),
                        args: Vec::new(),
                    },
                ),
                ("comments", CtValue::Bool(false)),
                ("inclusive_prefixes", CtValue::List(Vec::new())),
            ],
        );
        let actual = xml_canonical(
            &[CtValue::Bytes(vec![1, 2, 3]), options],
            Span::new(0, 0),
        )
        .expect("invalid DataTree is a user Result error");
        assert_eq!(
            actual,
            CtValue::failed(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            )))
        );
    }
}
