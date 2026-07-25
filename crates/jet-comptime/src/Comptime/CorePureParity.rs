//! Remaining deterministic Core calls for #392.
//!
//! Algorithms and value layouts mirror the AOT prelude. This module owns one
//! evaluator used by comptime and the REPL; callers never synthesize schemas or
//! fall back after a recognized call fails.

use std::collections::BTreeMap;

use crate::AST::{CtFloat, Type};
use crate::Diagnostics::{Diagnostic, Span};

use crate::Comptime::Builtins::{as_bool, as_int};
use crate::Comptime::Diagnostics::unsupported;
use crate::Comptime::Methods::as_float;
use crate::Comptime::Value::CtValue;

type EvalResult = Result<CtValue, Diagnostic>;

pub(super) fn evaluate(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<EvalResult> {
    let result = match (module, method) {
        ("core.mime", "parse") => mime_parse(args, span),
        ("core.mime", "from_extension") => mime_from_extension(args, span),
        ("core.mime", "extension") => mime_extension(args, span),
        ("core.email", "address") => email_address(args, span),
        ("core.email", "attachment") => email_attachment(args, span),
        ("core.email", "message") => email_message(args, span),
        ("core.email", "envelope") => email_envelope(args, span),
        ("core.email", "serialize") => email_serialize(args, span),
        ("core.encoding.xml", "canonical") => xml_canonical(args, span),
        ("core.time", "period") => period(args, span),
        ("core.time", "period_days") => period_unit(args, span, 2),
        ("core.time", "period_months") => period_unit(args, span, 1),
        ("core.time", "period_years") => period_unit(args, span, 0),
        ("core.time", "from_unix_ms") => datetime_from_unix_ms(args, span),
        ("core.time", "parse_rfc3339") => datetime_parse(args, span),
        ("core.time", "parse_time") => local_time_parse(args, span),
        // Pure zone constructors: UTC is deterministic. Named IANA zones need a
        // host TZif database (filesystem), so comptime keeps UTC aliases only and
        // returns `Err` for everything else — same shape as AOT without tzdb.
        ("core.time", "utc") => Ok(zone_utc()),
        ("core.time", "zone") => zone_named(args, span),
        ("core.time", "zoned") => zoned_from_datetime(args, span),
        ("core.time", "zoned_local") => zoned_from_local(args, span),
        ("core.math", "decimal") => decimal_from_str(args, span),
        ("core.science.measurement", "from") => measurement(args, span),
        ("core.time.date", "new") => date_new_call(args, span),
        ("core.time.date", "parse") => date_parse_call(args, span),
        ("core.time.datetime", "from_timestamp") => datetime_from_timestamp(args, span),
        // D-APPROX1=A: sketch constructors — same algorithms as AOT Jet* sketches.
        ("core.sketch.hll", "new") => Ok(hll_new()),
        ("core.sketch.tdigest", "new") => Ok(tdigest_new()),
        ("core.sketch.cms", "new") => Ok(cms_new()),
        ("core.sketch.reservoir", "new") => reservoir_new(args, span),
        ("core.ui", "point") => ui_point(args, span),
        ("core.ui", "size") => ui_size(args, span),
        ("core.ui", "rect") => ui_rect(args, span),
        ("core.ui", "constraint") => ui_constraint(args, span),
        ("core.ui", "node") => ui_node(args, span, None, None, "Custom"),
        ("core.ui", "node_role") => ui_node_role(args, span),
        ("core.ui", "node_color") => ui_node_color(args, span),
        ("core.ui", "text") => ui_text(args, span),
        ("core.ui", "button") => ui_button(args, span),
        ("core.ui", "box") => ui_box(args, span),
        ("core.ui", "aria_role_button") => Ok(ui_role("Button")),
        ("core.ui", "aria_role_text_input") => Ok(ui_role("TextInput")),
        ("core.ui", "aria_role_label") => Ok(ui_role("Label")),
        ("core.ui", "aria_role_container") => Ok(ui_role("Container")),
        ("core.ui", "key_event") => ui_key_event(args, span),
        ("core.ui", "resize_event") => ui_resize_event(args, span),
        ("core.raylib", "color") => raylib_color(args, span),
        ("core.io", "style_force") => io_style_force(args, span),
        ("core.net", "ip_addr") => net_ip_addr(args, span),
        ("core.net", "ip_to_string") => net_string_field(args, "IpAddr", "text", span),
        ("core.net", "ip_is_ipv4") => net_ip_is_ipv4(args, span),
        ("core.net", "socket_addr_parse") => net_socket_addr_parse(args, span),
        ("core.net", "socket_host") => net_string_field(args, "SocketAddr", "host", span),
        ("core.net", "socket_port") => net_value_field(args, "SocketAddr", "port", span),
        ("core.net", "socket_to_string") => net_string_field(args, "SocketAddr", "text", span),
        ("core.net", "ready_readable") => net_value_field(args, "NetReady", "readable", span),
        ("core.net", "ready_writable") => net_value_field(args, "NetReady", "writable", span),
        ("core.net", "error_operation") => net_string_field(args, "NetError", "operation", span),
        ("core.net", "error_address") => net_value_field(args, "NetError", "address", span),
        ("core.net", "error_name") => net_value_field(args, "NetError", "name", span),
        ("core.net", "error_message") => net_string_field(args, "NetError", "message", span),
        ("core.net", "error_os_code") => net_value_field(args, "NetError", "os_code", span),
        ("core.net", "dns_srv_target") => net_string_field(args, "DnsSrv", "target", span),
        ("core.net", "dns_srv_port") => net_value_field(args, "DnsSrv", "port", span),
        ("core.net", "dns_srv_priority") => net_value_field(args, "DnsSrv", "priority", span),
        ("core.net", "dns_srv_weight") => net_value_field(args, "DnsSrv", "weight", span),
        ("core.net", "udp_packet_data") => net_udp_packet_data(args, span),
        ("core.net", "udp_packet_bytes") => net_value_field(args, "UdpPacket", "data", span),
        ("core.net", "udp_packet_addr") => net_value_field(args, "UdpPacket", "addr", span),
        ("core.net", "udp_packet_original_len") => net_value_field(args, "UdpPacket", "original_len", span),
        ("core.net", "udp_packet_truncated") => net_value_field(args, "UdpPacket", "truncated", span),
        ("core.crypto.expert", "ed25519_verify_strict") => crypto_ed25519_verify(args, span),
        ("core.crypto.expert", "ed25519_sign") => crypto_ed25519_sign(args, span),
        ("core.crypto.expert", "hkdf_sha256") => crypto_hkdf(args, span),
        ("core.crypto.expert", "x25519") => crypto_x25519(args, span),
        ("core.crypto.expert", "xchacha20poly1305_seal") => {
            crypto_aead_seal(args, span, "expert.xchacha20poly1305_seal", 24, false)
        }
        ("core.crypto.expert", "xchacha20poly1305_open") => {
            crypto_aead_open(args, span, "expert.xchacha20poly1305_open", 24)
        }
        ("core.crypto.expert", "aes256gcm_seal") => {
            crypto_aead_seal(args, span, "expert.aes256gcm_seal", 12, true)
        }
        ("core.crypto.expert", "aes256gcm_open") => {
            crypto_aead_open(args, span, "expert.aes256gcm_open", 12)
        }
        ("core.crypto.expert", "argon2id") => crypto_argon2id(args, span),
        ("core.crypto.expert", "secret_bytes") => crypto_extract(args, 0, "Secret", span),
        ("core.crypto.expert", "signing_key_bytes") => crypto_extract(args, 0, "SigningKey", span),
        ("core.crypto.expert", "x25519_secret_bytes") => crypto_extract(args, 0, "X25519SecretKey", span),
        ("core.crypto.expert", "shared_secret_bytes") => crypto_extract(args, 0, "SharedSecret", span),
        // TIR lowers Signature/VerifyKey/… `.bytes()` to jet.crypto.__*_bytes;
        // keep those pure field extracts resident so REPL does not hit E1802.
        ("jet.crypto", "__signature_bytes") => crypto_extract(args, 0, "Signature", span),
        ("jet.crypto", "__verify_key_bytes") => crypto_extract(args, 0, "VerifyKey", span),
        ("jet.crypto", "__x25519_public_bytes") => crypto_extract(args, 0, "X25519PublicKey", span),
        ("jet.crypto", "__sealed_bytes") => crypto_extract(args, 0, "Sealed", span),
        ("jet.crypto", "__digest256_bytes") => crypto_extract(args, 0, "Digest256", span),
        ("jet.crypto", "__digest512_bytes") => crypto_extract(args, 0, "Digest512", span),
        // Typed decode/decode_bytes run in eval_method; arms prove inventory coverage.
        ("core.encoding.xml", "decode") => Err(unsupported(
            "core.encoding.xml.decode() requires a type argument",
            span,
        )),
        ("core.encoding.xml", "decode_bytes") => Err(unsupported(
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
            .map(|date| CtValue::Int((date.day_number() + 6) % 7)),
        ("Date" | "LocalDate", "iso_weekday", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.day_number() % 7 + 1)),
        ("Date" | "LocalDate", "day_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| {
                CtValue::Int(date.day_number() - Date::new(date.year, 1, 1).day_number() + 1)
            }),
        ("Date" | "LocalDate", "iso_week", 0) => date_from_value(recv, type_name, span)
            .map(|date| {
                let thursday = date.add_days(4 - (date.day_number() % 7 + 1));
                CtValue::Int(
                    (thursday.day_number() - Date::new(thursday.year, 1, 1).day_number()) / 7 + 1,
                )
            }),
        ("Date" | "LocalDate", "add_days", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_days(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "add_months", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_months(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "diff_days", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| {
                Ok(CtValue::Int(
                    date.day_number()
                        - date_from_value(&args[0], "LocalDate", span)?.day_number(),
                ))
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
        ("DateTime", "to_unix_ms", 0) => int_field(recv, "DateTime", "secs", span)
            .map(|seconds| CtValue::Int(seconds.saturating_mul(1_000))),
        ("DateTime", "to_string", 0) => datetime_string(recv, span).map(CtValue::Str),
        ("DateTime", "date", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.date().value())
        }
        ("DateTime", "time", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.time().value())
        }
        ("DateTime", "hour", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.time().hour)),
        ("DateTime", "minute", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.time().minute)),
        ("DateTime", "second", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.seconds.rem_euclid(60))),
        ("DateTime", "format_rfc3339", 0) => datetime_from_value(recv, span).map(
            |date_time| {
                CtValue::Str(format!(
                    "{}T{}Z",
                    date_time.date().to_string_fmt(),
                    date_time.time().to_string_fmt()
                ))
            },
        ),
        ("DateTime", "format", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(CtValue::Str(format_time_pattern(
                string_arg(args, 0, span)?,
                date_time.date(),
                date_time.time(),
            )))
        }),
        ("DateTime", "plus_duration", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let millis = int_field(&args[0], crate::Syntax::DURATION_TYPE, "ms", span)?;
                Ok(DateTime {
                    seconds: date_time
                        .seconds
                        .saturating_add(millis.div_euclid(1_000)),
                }
                .value())
            })
        }
        ("DateTime", "truncate" | "round", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let size = match string_arg(args, 0, span)? {
                    "day" => 86_400,
                    "hour" => 3_600,
                    "minute" => 60,
                    _ => 1,
                };
                let seconds = if method == "round" {
                    date_time
                        .seconds
                        .saturating_add(size / 2)
                        .div_euclid(size)
                        * size
                } else {
                    date_time.seconds.div_euclid(size) * size
                };
                Ok(DateTime { seconds }.value())
            })
        }
        ("DateTime", "in_zone", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(ZonedDateTime {
                instant: date_time,
                zone: zone_from_value(&args[0], span)?,
            }
            .value())
        }),
        ("Zone", "name", 0) => string_field(recv, "Zone", "name", span),
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
            let millis = int_field(&args[0], crate::Syntax::DURATION_TYPE, "ms", span)?;
            Ok(ZonedDateTime {
                instant: DateTime {
                    seconds: zoned.instant.seconds.saturating_add(millis.div_euclid(1_000)),
                },
                zone: zoned.zone,
            }
            .value())
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
    Some(solver_require_update(recv, args, span).map(|updated| (CtValue::Unit, updated)))
}

/// D-SOLVER-LIB1=A: `solve.Solver.new(seed)` — same seed/checked/failures layout as AOT.
pub(super) fn solver_new(args: &[CtValue], span: Span) -> EvalResult {
    let seed = as_int(one(args, 0, "Solver", "new", span)?, span)?;
    Ok(structure(
        crate::Syntax::SOLVER_TYPE,
        vec![
            ("seed", CtValue::Int(seed)),
            ("checked", CtValue::Int(0)),
            ("failures", CtValue::Int(0)),
        ],
    ))
}

pub(super) fn display(value: &CtValue) -> Option<String> {
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
        _ => {
            let CtValue::Float(measured) = field(value, "Measurement", "value")? else {
                return None;
            };
            let CtValue::Float(uncertainty) = field(value, "Measurement", "uncertainty")? else {
                return None;
            };
            Some(format!("{measured:?} ± {uncertainty:?}"))
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
        ("role", role.map_or(CtValue::None(Type::Named("UiAriaRole".to_string())), |role| CtValue::Some(Box::new(role)))),
        ("color", color.map_or(CtValue::None(Type::String), |color| CtValue::Some(Box::new(CtValue::Str(color))))),
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
        ("address", address.map_or(CtValue::None(Type::String), |value| CtValue::Some(Box::new(CtValue::Str(value))))),
        ("name", CtValue::None(Type::String)),
        ("message", CtValue::Str(message)),
        ("os_code", CtValue::None(Type::Int)),
    ])
}

fn net_ip_addr(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    Ok(match text.parse::<std::net::IpAddr>() {
        Ok(address) => CtValue::ResOk(Box::new(structure("IpAddr", vec![("text", CtValue::Str(address.to_string()))]))),
        Err(error) => CtValue::ResErr(Box::new(net_error(
            "parse IP address",
            Some(text.to_string()),
            format!("invalid IP address `{text}`: {error}"),
        ))),
    })
}

fn net_ip_is_ipv4(args: &[CtValue], span: Span) -> EvalResult {
    let text = match field(one(args, 0, "core.net", "ip_is_ipv4", span)?, "IpAddr", "text") {
        Some(CtValue::Str(text)) => text,
        _ => return Err(unsupported("malformed IpAddr value", span)),
    };
    Ok(CtValue::Bool(text.parse::<std::net::Ipv4Addr>().is_ok()))
}

fn net_socket_addr_parse(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    Ok(match text.parse::<std::net::SocketAddr>() {
        Ok(address) => CtValue::ResOk(Box::new(structure("SocketAddr", vec![
            ("host", CtValue::Str(address.ip().to_string())),
            ("port", CtValue::Int(i64::from(address.port()))),
            ("text", CtValue::Str(address.to_string())),
        ]))),
        Err(error) => CtValue::ResErr(Box::new(net_error(
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
    match net_value_field(args, "UdpPacket", "data", span)? {
        CtValue::Bytes(value) => Ok(CtValue::Str(String::from_utf8_lossy(&value).into_owned())),
        _ => Err(unsupported("malformed UdpPacket.data value", span)),
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
        return Ok(CtValue::ResErr(Box::new(crypto_error("HKDF-SHA256 output length must be 0..8160"))));
    }
    let bytes = crate::Comptime::CryptoLite::hkdf_sha256(
        &bytes_value(one(args, 0, "core.crypto.expert", "hkdf_sha256", span)?, span)?,
        &bytes_value(one(args, 1, "core.crypto.expert", "hkdf_sha256", span)?, span)?,
        &bytes_value(one(args, 2, "core.crypto.expert", "hkdf_sha256", span)?, span)?,
        length as usize,
    );
    Ok(CtValue::ResOk(Box::new(crypto_secret("Secret", bytes))))
}

fn crypto_ed25519_verify(args: &[CtValue], span: Span) -> EvalResult {
    let public = bytes_value(one(args, 0, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let message = bytes_value(one(args, 1, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let signature = bytes_value(one(args, 2, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    if public.len() != 32 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: public must be exactly 32; got {}",
            public.len()
        )))));
    }
    if signature.len() != 64 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: signature must be exactly 64; got {}",
            signature.len()
        )))));
    }
    if message.len() > 1_073_741_824 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
            "expert.ed25519_verify_strict: message must be at most 1073741824; got {}",
            message.len()
        )))));
    }
    let public: [u8; 32] = public.try_into().expect("length checked");
    let signature: [u8; 64] = signature.try_into().expect("length checked");
    match crate::Comptime::CryptoLite::ed25519_verify_strict(&public, &message, &signature) {
        Ok(valid) => Ok(CtValue::ResOk(Box::new(CtValue::Bool(valid)))),
        Err(()) => Ok(CtValue::ResErr(Box::new(crypto_error(
            "expert.ed25519_verify_strict: Ed25519 public key is not canonical",
        )))),
    }
}

fn crypto_ed25519_sign(args: &[CtValue], span: Span) -> EvalResult {
    let seed = bytes_value(one(args, 0, "core.crypto.expert", "ed25519_sign", span)?, span)?;
    let message = bytes_value(one(args, 1, "core.crypto.expert", "ed25519_sign", span)?, span)?;
    if seed.len() != 32 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
            "expert.ed25519_sign: seed must be exactly 32; got {}",
            seed.len()
        )))));
    }
    if message.len() > 1_073_741_824 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
            "expert.ed25519_sign: message must be at most 1073741824; got {}",
            message.len()
        )))));
    }
    let seed: [u8; 32] = seed.try_into().expect("length checked");
    let signature = crate::Comptime::CryptoLite::ed25519_sign(&seed, &message);
    Ok(CtValue::ResOk(Box::new(crypto_secret(
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
        return Some(CtValue::ResErr(Box::new(crypto_error(&format!(
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
        return Some(CtValue::ResErr(Box::new(crypto_error(&format!(
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
        return Some(CtValue::ResErr(Box::new(crypto_error(&format!(
            "{operation}: {label} must be {expected}; got {}",
            input.len()
        )))));
    }
    if aad.len() > 16_777_216 {
        return Some(CtValue::ResErr(Box::new(crypto_error(&format!(
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
    let key = bytes_value(one(args, 0, "core.crypto.expert", operation, span)?, span)?;
    let nonce = bytes_value(one(args, 1, "core.crypto.expert", operation, span)?, span)?;
    let plaintext = bytes_value(one(args, 2, "core.crypto.expert", operation, span)?, span)?;
    let aad = bytes_value(one(args, 3, "core.crypto.expert", operation, span)?, span)?;
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
        Ok(bytes) => Ok(CtValue::ResOk(Box::new(CtValue::Bytes(bytes)))),
        Err(()) => Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
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
    let key = bytes_value(one(args, 0, "core.crypto.expert", operation, span)?, span)?;
    let nonce = bytes_value(one(args, 1, "core.crypto.expert", operation, span)?, span)?;
    let ciphertext = bytes_value(one(args, 2, "core.crypto.expert", operation, span)?, span)?;
    let aad = bytes_value(one(args, 3, "core.crypto.expert", operation, span)?, span)?;
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
        Ok(bytes) => Ok(CtValue::ResOk(Box::new(CtValue::Bytes(bytes)))),
        Err(()) => Ok(CtValue::ResErr(Box::new(crypto_error(
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
            bytes_value(&CtValue::List(bytes.clone()), span)?
        }
        _ => {
            return Err(unsupported(
                "core.crypto.expert.argon2id() needs a Secret password",
                span,
            ))
        }
    };
    let salt = bytes_value(one(args, 1, "core.crypto.expert", "argon2id", span)?, span)?;
    let memory_kib = int_arg(args, 2, span)?;
    let iterations = int_arg(args, 3, span)?;
    let lanes = int_arg(args, 4, span)?;
    let output_length = int_arg(args, 5, span)?;
    if password.len() > 1_048_576 {
        return Ok(CtValue::ResErr(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        ))));
    }
    if !(8..=64).contains(&salt.len()) {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
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
        return Ok(CtValue::ResErr(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        ))));
    }
    if !(16..=64).contains(&output_length) {
        return Ok(CtValue::ResErr(Box::new(crypto_error(&format!(
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
        Ok(bytes) => Ok(CtValue::ResOk(Box::new(crypto_secret("Secret", bytes)))),
        Err(()) => Ok(CtValue::ResErr(Box::new(crypto_error(
            "password hash is outside Jet's accepted policy",
        )))),
    }
}

fn crypto_x25519(args: &[CtValue], span: Span) -> EvalResult {
    let secret = bytes_value(one(args, 0, "core.crypto.expert", "x25519", span)?, span)?;
    let public = bytes_value(one(args, 1, "core.crypto.expert", "x25519", span)?, span)?;
    if secret.len() != 32 || public.len() != 32 {
        return Ok(CtValue::ResErr(Box::new(crypto_error("X25519 keys must contain exactly 32 bytes"))));
    }
    let shared = crate::Comptime::CryptoLite::x25519(&secret, &public).expect("length checked");
    let reject_all_zero = match args.get(2) {
        Some(CtValue::Bool(value)) => *value,
        _ => return Err(unsupported("core.crypto.expert.x25519() needs a Bool third argument", span)),
    };
    if reject_all_zero && shared == [0; 32] {
        return Ok(CtValue::ResErr(Box::new(crypto_error("X25519 peer key does not contribute to a shared secret"))));
    }
    Ok(CtValue::ResOk(Box::new(crypto_secret("Secret", shared.to_vec()))))
}

fn crypto_extract(args: &[CtValue], index: usize, type_name: &str, span: Span) -> EvalResult {
    let value = one(args, index, "core.crypto.expert", "secret_bytes", span)?;
    match field(value, type_name, "bytes") {
        Some(CtValue::Bytes(bytes)) => Ok(CtValue::Bytes(bytes.clone())),
        Some(CtValue::List(bytes)) => bytes_value(&CtValue::List(bytes.clone()), span).map(CtValue::Bytes),
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

fn mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                )
        })
}

fn parse_mime(input: &str) -> Result<CtValue, String> {
    let mut parts = input.split(';');
    let essence = parts.next().unwrap_or("").trim();
    let Some((top, sub)) = essence.split_once('/') else {
        return Err("MIME type needs `type/subtype`".to_string());
    };
    let top = top.trim().to_ascii_lowercase();
    let sub = sub.trim().to_ascii_lowercase();
    if !mime_token(&top) || !mime_token(&sub) {
        return Err(format!("invalid MIME type `{essence}`"));
    }
    let mut params = Vec::new();
    for parameter in parts {
        let parameter = parameter.trim();
        if parameter.is_empty() {
            continue;
        }
        let Some((key, value)) = parameter.split_once('=') else {
            return Err(format!("invalid MIME parameter `{parameter}`"));
        };
        let key = key.trim().to_ascii_lowercase();
        if !mime_token(&key) {
            return Err(format!("invalid MIME parameter `{}`", key.trim()));
        }
        params.push(CtValue::List(vec![
            CtValue::Str(key),
            CtValue::Str(value.trim().trim_matches('"').to_string()),
        ]));
    }
    Ok(structure(
        "Mime",
        vec![
            ("top", CtValue::Str(top)),
            ("sub", CtValue::Str(sub)),
            ("params", CtValue::List(params)),
        ],
    ))
}

fn mime_essence(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
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
    Ok(format!("{top}/{sub}"))
}

fn mime_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let Some(CtValue::List(params)) = field(value, "Mime", "params") else {
        return Err(unsupported("malformed Mime.params value", span));
    };
    let mut output = mime_essence(value, span)?;
    for param in params {
        let CtValue::List(pair) = param else {
            return Err(unsupported("malformed Mime parameter", span));
        };
        let [CtValue::Str(key), CtValue::Str(value)] = pair.as_slice() else {
            return Err(unsupported("malformed Mime parameter", span));
        };
        output.push_str(&format!("; {key}={value}"));
    }
    Ok(output)
}

fn mime_param(value: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let name = string_arg(args, 0, span)?.to_ascii_lowercase();
    let Some(CtValue::List(params)) = field(value, "Mime", "params") else {
        return Err(unsupported("malformed Mime.params value", span));
    };
    Ok(params
        .iter()
        .find_map(|param| match param {
            CtValue::List(pair) => match pair.as_slice() {
                [CtValue::Str(key), CtValue::Str(value)] if key == &name => {
                    Some(CtValue::Some(Box::new(CtValue::Str(value.clone()))))
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(CtValue::None(Type::String)))
}

fn mime_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match parse_mime(string_arg(args, 0, span)?) {
        Ok(value) => CtValue::ResOk(Box::new(value)),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(error))),
    })
}

fn mime_from_extension(args: &[CtValue], span: Span) -> EvalResult {
    let extension = string_arg(args, 0, span)?
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let value = match extension.as_str() {
        "html" | "htm" => Some("text/html"),
        "css" => Some("text/css"),
        "csv" => Some("text/csv"),
        "txt" | "text" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "json" => Some("application/json"),
        "js" | "mjs" => Some("text/javascript"),
        "wasm" => Some("application/wasm"),
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "ico" => Some("image/x-icon"),
        "mp3" => Some("audio/mpeg"),
        "mp4" => Some("video/mp4"),
        "xml" => Some("application/xml"),
        "zip" => Some("application/zip"),
        "gz" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        _ => None,
    };
    Ok(option_string(value))
}

fn mime_extension(args: &[CtValue], span: Span) -> EvalResult {
    let lowered = string_arg(args, 0, span)?.to_ascii_lowercase();
    let mime = lowered.split(';').next().unwrap_or("").trim();
    let value = match mime {
        "text/html" => Some("html"),
        "text/css" => Some("css"),
        "text/csv" => Some("csv"),
        "text/plain" => Some("txt"),
        "text/markdown" => Some("md"),
        "application/json" => Some("json"),
        "text/javascript" | "application/javascript" => Some("js"),
        "application/wasm" => Some("wasm"),
        "application/pdf" => Some("pdf"),
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        "image/x-icon" => Some("ico"),
        "audio/mpeg" => Some("mp3"),
        "video/mp4" => Some("mp4"),
        "application/xml" | "text/xml" => Some("xml"),
        "application/zip" => Some("zip"),
        "application/gzip" => Some("gz"),
        "application/x-tar" => Some("tar"),
        _ => None,
    };
    Ok(option_string(value))
}

fn option_string(value: Option<&str>) -> CtValue {
    value.map_or(CtValue::None(Type::String), |value| {
        CtValue::Some(Box::new(CtValue::Str(value.to_string())))
    })
}

// ── Civil time ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Date {
    year: i64,
    month: i64,
    day: i64,
}

impl Date {
    fn is_leap(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    fn days_in_month(year: i64, month: i64) -> i64 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Self::is_leap(year) => 29,
            2 => 28,
            _ => 30,
        }
    }

    fn new(year: i64, month: i64, day: i64) -> Self {
        let month = month.clamp(1, 12);
        let day = day.clamp(1, Self::days_in_month(year, month));
        Self { year, month, day }
    }

    fn parse(value: &str) -> Result<Self, String> {
        let parts = value.splitn(3, '-').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(format!("invalid date: {value}"));
        }
        let year = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad year: {}", parts[0]))?;
        let month = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad month: {}", parts[1]))?;
        let day = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad day: {}", parts[2]))?;
        if !(1..=12).contains(&month)
            || day < 1
            || day > Self::days_in_month(year, month)
        {
            return Err(format!("date out of range: {value}"));
        }
        Ok(Self::new(year, month, day))
    }

    fn day_number(self) -> i64 {
        let year = self.year - 1;
        365 * year + year / 4 - year / 100
            + year / 400
            + [0_i64, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
                [(self.month - 1) as usize]
            + i64::from(self.month > 2 && Self::is_leap(self.year))
            + self.day
            - 1
    }

    fn from_day_number(mut day: i64) -> Self {
        let mut year = day / 365 + 1;
        loop {
            let start = Self::new(year, 1, 1).day_number();
            let next = Self::new(year + 1, 1, 1).day_number();
            if day >= start && day < next {
                break;
            }
            year += if day < start { -1 } else { 1 };
        }
        day -= Self::new(year, 1, 1).day_number();
        let mut month = 1;
        while month < 12 && day >= Self::days_in_month(year, month) {
            day -= Self::days_in_month(year, month);
            month += 1;
        }
        Self::new(year, month, day + 1)
    }

    fn add_days(self, days: i64) -> Self {
        Self::from_day_number(self.day_number() + days)
    }

    fn add_months(self, months: i64) -> Self {
        let total = self.month - 1 + months;
        let year = self.year + total / 12;
        let month = total % 12 + 1;
        Self::new(year, month, self.day.min(Self::days_in_month(year, month)))
    }

    fn to_string_fmt(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
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

#[derive(Clone, Copy)]
struct LocalTime {
    hour: i64,
    minute: i64,
    second: i64,
}

impl LocalTime {
    fn new(hour: i64, minute: i64, second: i64) -> Self {
        Self {
            hour: hour.clamp(0, 23),
            minute: minute.clamp(0, 59),
            second: second.clamp(0, 59),
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        let parts = value.splitn(3, ':').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(format!("invalid time: {value}"));
        }
        let hour = parts[0]
            .parse::<i64>()
            .map_err(|_| format!("bad hour: {}", parts[0]))?;
        let minute = parts[1]
            .parse::<i64>()
            .map_err(|_| format!("bad minute: {}", parts[1]))?;
        let second = parts[2]
            .parse::<i64>()
            .map_err(|_| format!("bad second: {}", parts[2]))?;
        if !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=59).contains(&second)
        {
            return Err(format!("time out of range: {value}"));
        }
        Ok(Self {
            hour,
            minute,
            second,
        })
    }

    fn seconds(self) -> i64 {
        self.hour * 3600 + self.minute * 60 + self.second
    }

    fn to_string_fmt(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
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

#[derive(Clone, Copy)]
struct DateTime {
    seconds: i64,
}

impl DateTime {
    fn date(self) -> Date {
        let epoch = Date::new(1970, 1, 1).day_number();
        Date::from_day_number(epoch + self.seconds.div_euclid(86_400))
    }

    fn time(self) -> LocalTime {
        let seconds = self.seconds.rem_euclid(86_400);
        LocalTime::new(seconds / 3_600, (seconds / 60) % 60, seconds % 60)
    }

    fn value(self) -> CtValue {
        datetime_value(self.seconds)
    }
}

#[derive(Clone)]
struct Zone {
    name: String,
    offset: i64,
}

impl Zone {
    fn utc() -> Self {
        Self {
            name: "UTC".to_string(),
            offset: 0,
        }
    }

    fn parse_name(name: &str) -> Result<Self, String> {
        if name == "UTC" || name == "Etc/UTC" || name == "Z" {
            return Ok(Self::utc());
        }
        Err(format!(
            "unknown IANA time zone: {name}; comptime supports UTC/Etc/UTC/Z only (host TZif databases are a named ambient boundary)"
        ))
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
    instant: DateTime,
    zone: Zone,
}

impl ZonedDateTime {
    fn from_local(date: Date, time: LocalTime, zone: Zone) -> Self {
        let local_secs = utc_seconds(date, time);
        Self {
            instant: DateTime {
                seconds: local_secs.saturating_sub(zone.offset),
            },
            zone,
        }
    }

    fn offset_seconds(&self) -> i64 {
        self.zone.offset
    }

    fn local_instant(&self) -> DateTime {
        DateTime {
            seconds: self.instant.seconds.saturating_add(self.zone.offset),
        }
    }

    fn date(&self) -> Date {
        self.local_instant().date()
    }

    fn time(&self) -> LocalTime {
        self.local_instant().time()
    }

    fn to_string_fmt(&self) -> String {
        format!(
            "{} {} {} ({})",
            self.date().to_string_fmt(),
            self.time().to_string_fmt(),
            self.zone.name,
            offset_string(self.offset_seconds())
        )
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

fn decimal_from_value(value: &CtValue, span: Span) -> Result<crate::Numeric::CtDecimal, Diagnostic> {
    crate::Numeric::CtDecimal::from_value(value).map_err(|error| unsupported(&error, span))
}

fn zone_named(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match Zone::parse_name(string_arg(args, 0, span)?) {
        Ok(zone) => CtValue::ResOk(Box::new(zone.value())),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(error))),
    })
}

fn zone_from_value(value: &CtValue, span: Span) -> Result<Zone, Diagnostic> {
    Ok(Zone {
        name: match field(value, "Zone", "name") {
            Some(CtValue::Str(name)) => name.clone(),
            _ => {
                return Err(unsupported("malformed Zone.name value", span));
            }
        },
        offset: int_field(value, "Zone", "offset", span)?,
    })
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
    Ok(ZonedDateTime { instant, zone })
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
    Ok(ZonedDateTime { instant, zone }.value())
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

fn offset_string(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!("{sign}{:02}:{:02}", abs / 3_600, (abs / 60) % 60)
}

fn format_zoned_pattern(pattern: &str, zoned: ZonedDateTime) -> String {
    let mut output = format_time_pattern(pattern, zoned.date(), zoned.time());
    output = output.replace("VV", &zoned.zone.name);
    output.replace("XXX", &offset_string(zoned.offset_seconds()))
}

fn date_from_value(value: &CtValue, type_name: &str, span: Span) -> Result<Date, Diagnostic> {
    Ok(Date::new(
        int_field(value, type_name, "year", span)?,
        int_field(value, type_name, "month", span)?,
        int_field(value, type_name, "day", span)?,
    ))
}

fn local_time_from_value(value: &CtValue, span: Span) -> Result<LocalTime, Diagnostic> {
    Ok(LocalTime {
        hour: int_field(value, "LocalTime", "hour", span)?,
        minute: int_field(value, "LocalTime", "minute", span)?,
        second: int_field(value, "LocalTime", "second", span)?,
    })
}

fn datetime_from_value(value: &CtValue, span: Span) -> Result<DateTime, Diagnostic> {
    Ok(DateTime {
        seconds: int_field(value, "DateTime", "secs", span)?,
    })
}

fn date_add_period(date: Date, period: &CtValue, span: Span) -> Result<Date, Diagnostic> {
    let months = int_field(period, "Period", "years", span)?
        .saturating_mul(12)
        .saturating_add(int_field(period, "Period", "months", span)?);
    Ok(date
        .add_months(months)
        .add_days(int_field(period, "Period", "days", span)?))
}

fn date_truncate(date: Date, unit: &str) -> Date {
    match unit {
        "year" => Date::new(date.year, 1, 1),
        "month" => Date::new(date.year, date.month, 1),
        _ => date,
    }
}

fn format_time_pattern(pattern: &str, date: Date, time: LocalTime) -> String {
    let mut output = pattern.to_string();
    let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        [(date.day_number() % 7) as usize];
    output = output.replace("yyyy", &format!("{:04}", date.year));
    output = output.replace(
        "DDD",
        &format!(
            "{:03}",
            date.day_number() - Date::new(date.year, 1, 1).day_number() + 1
        ),
    );
    output = output.replace("EEE", weekday);
    output = output.replace("MM", &format!("{:02}", date.month));
    output = output.replace("dd", &format!("{:02}", date.day));
    output = output.replace("HH", &format!("{:02}", time.hour));
    output = output.replace("mm", &format!("{:02}", time.minute));
    output.replace("ss", &format!("{:02}", time.second))
}

fn period_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    Ok(format!(
        "P{}Y{}M{}D",
        int_field(value, "Period", "years", span)?,
        int_field(value, "Period", "months", span)?,
        int_field(value, "Period", "days", span)?
    ))
}

fn datetime_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let seconds = int_field(value, "DateTime", "secs", span)?;
    let epoch = Date::new(1970, 1, 1).day_number();
    let date = Date::from_day_number(epoch + seconds.div_euclid(86_400));
    let time = seconds.rem_euclid(86_400);
    Ok(format!(
        "{} {:02}:{:02}:{:02} UTC",
        date.to_string_fmt(),
        time / 3_600,
        (time / 60) % 60,
        time % 60
    ))
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
        Ok(date) => CtValue::ResOk(Box::new(date.value())),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(error))),
    })
}

fn period_value(years: i64, months: i64, days: i64) -> CtValue {
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

fn datetime_value(seconds: i64) -> CtValue {
    structure("DateTime", vec![("secs", CtValue::Int(seconds))])
}

fn datetime_from_timestamp(args: &[CtValue], span: Span) -> EvalResult {
    Ok(datetime_value(int_arg(args, 0, span)?))
}

fn datetime_from_unix_ms(args: &[CtValue], span: Span) -> EvalResult {
    Ok(datetime_value(int_arg(args, 0, span)?.div_euclid(1000)))
}

fn utc_seconds(date: Date, time: LocalTime) -> i64 {
    let epoch = Date::new(1970, 1, 1).day_number();
    (date.day_number() - epoch)
        .saturating_mul(86_400)
        .saturating_add(time.seconds())
}

fn parse_datetime(value: &str) -> Result<i64, String> {
    let (date_part, rest) = value
        .split_once('T')
        .ok_or_else(|| format!("invalid RFC3339 datetime: {value}"))?;
    let date = Date::parse(date_part)?;
    let zone_pos = rest
        .find('Z')
        .or_else(|| rest.rfind('+'))
        .or_else(|| rest.get(1..).and_then(|tail| tail.rfind('-').map(|i| i + 1)))
        .ok_or_else(|| format!("RFC3339 datetime needs Z or an offset: {value}"))?;
    let (time_part, zone_part) = rest.split_at(zone_pos);
    let time = LocalTime::parse(time_part.split('.').next().unwrap_or(time_part))?;
    let offset = if zone_part == "Z" {
        0
    } else {
        let sign = if zone_part.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = zone_part[1..]
            .split_once(':')
            .ok_or_else(|| format!("bad RFC3339 offset: {zone_part}"))?;
        let hours = hours
            .parse::<i64>()
            .map_err(|_| format!("bad RFC3339 offset hour: {hours}"))?;
        let minutes = minutes
            .parse::<i64>()
            .map_err(|_| format!("bad RFC3339 offset minute: {minutes}"))?;
        sign * (hours * 3600 + minutes * 60)
    };
    Ok(utc_seconds(date, time) - offset)
}

fn datetime_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match parse_datetime(string_arg(args, 0, span)?) {
        Ok(seconds) => CtValue::ResOk(Box::new(datetime_value(seconds))),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(error))),
    })
}

fn local_time_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(match LocalTime::parse(string_arg(args, 0, span)?) {
        Ok(time) => CtValue::ResOk(Box::new(time.value())),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(error))),
    })
}

fn measurement(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure(
        "Measurement",
        vec![
            ("value", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
            (
                "uncertainty",
                CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?)),
            ),
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
    let (value, uncertainty) = match method {
        "add" => (
            left_value + right_value,
            (left_uncertainty * left_uncertainty + right_uncertainty * right_uncertainty).sqrt(),
        ),
        "sub" => (
            left_value - right_value,
            (left_uncertainty * left_uncertainty + right_uncertainty * right_uncertainty).sqrt(),
        ),
        "mul" => (
            left_value * right_value,
            ((right_value * left_uncertainty).powi(2)
                + (left_value * right_uncertainty).powi(2))
            .sqrt(),
        ),
        "div" => (
            left_value / right_value,
            ((left_uncertainty / right_value).powi(2)
                + (left_value * right_uncertainty / (right_value * right_value)).powi(2))
            .sqrt(),
        ),
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
            return Ok(CtValue::ResErr(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            ))))
        }
    };
    let (mode, comments, inclusive_prefixes) = xml_canonical_options(options, span)?;
    let canonical = jet_foundation::XmlPull::canonical_document(
        &value,
        &jet_foundation::XmlPull::CanonicalOptions {
            mode,
            comments,
            inclusive_prefixes,
        },
    );
    Ok(match canonical {
        Ok(value) => CtValue::ResOk(Box::new(CtValue::Str(value))),
        Err(error) => CtValue::ResErr(Box::new(
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
            ("byte_offset", CtValue::None(Type::Int)),
            ("line", CtValue::None(Type::Int)),
            ("column", CtValue::None(Type::Int)),
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

// Email evaluator follows below; kept in this module so all Packet B calls
// share the same recognized-call/no-fallback contract.

const MAX_RECIPIENTS: usize = 100;
const MAX_ATTACHMENTS: usize = 64;
const MAX_HEADER_BYTES: usize = 998;
const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct Address {
    display: Option<String>,
    mailbox: String,
}

#[derive(Clone)]
struct Attachment {
    filename: String,
    mime: String,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct Envelope {
    from: Address,
    recipients: Vec<Address>,
}

#[derive(Clone)]
struct Message {
    from: Address,
    to: Vec<Address>,
    bcc: Vec<Address>,
    subject: String,
    text: String,
    html: String,
    attachments: Vec<Attachment>,
    envelope: Envelope,
    wire_upper: usize,
}

#[derive(Clone)]
struct EmailError {
    operation: String,
    reason: String,
}

fn email_error(operation: &str, reason: impl Into<String>) -> EmailError {
    EmailError {
        operation: operation.to_string(),
        reason: reason.into(),
    }
}

fn email_error_value(error: EmailError) -> CtValue {
    CtValue::Enum {
        type_name: "EmailError".to_string(),
        variant: "Configuration".to_string(),
        args: vec![
            (
                Some("operation".to_string()),
                CtValue::Str(error.operation),
            ),
            (
                Some("server".to_string()),
                CtValue::None(Type::String),
            ),
            (Some("code".to_string()), CtValue::None(Type::Int)),
            (Some("reason".to_string()), CtValue::Str(error.reason)),
        ],
    }
}

fn email_result(result: Result<CtValue, EmailError>) -> CtValue {
    match result {
        Ok(value) => CtValue::ResOk(Box::new(value)),
        Err(error) => CtValue::ResErr(Box::new(email_error_value(error))),
    }
}

fn address_value(address: &Address) -> CtValue {
    structure(
        "Address",
        vec![
            (
                "display",
                address.display.as_ref().map_or(
                    CtValue::None(Type::String),
                    |display| CtValue::Some(Box::new(CtValue::Str(display.clone()))),
                ),
            ),
            ("mailbox", CtValue::Str(address.mailbox.clone())),
        ],
    )
}

fn address_from_value(value: &CtValue, span: Span) -> Result<Address, Diagnostic> {
    let mailbox = match field(value, "Address", "mailbox") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email call expected Address", span)),
    };
    let display = match field(value, "Address", "display") {
        Some(CtValue::Some(value)) => match value.as_ref() {
            CtValue::Str(value) => Some(value.clone()),
            _ => return Err(unsupported("email Address display is invalid", span)),
        },
        Some(CtValue::None(_)) => None,
        _ => return Err(unsupported("email Address display is invalid", span)),
    };
    Ok(Address { display, mailbox })
}

fn attachment_value(attachment: &Attachment) -> CtValue {
    structure(
        "Attachment",
        vec![
            ("filename", CtValue::Str(attachment.filename.clone())),
            ("mime", CtValue::Str(attachment.mime.clone())),
            ("bytes", CtValue::Bytes(attachment.bytes.clone())),
        ],
    )
}

fn attachment_from_value(value: &CtValue, span: Span) -> Result<Attachment, Diagnostic> {
    let filename = match field(value, "Attachment", "filename") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email call expected Attachment", span)),
    };
    let mime = match field(value, "Attachment", "mime") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("email Attachment content type is invalid", span)),
    };
    let bytes = match field(value, "Attachment", "bytes") {
        Some(value) => bytes_value(value, span)?,
        None => return Err(unsupported("email Attachment bytes are missing", span)),
    };
    Ok(Attachment {
        filename,
        mime,
        bytes,
    })
}

fn envelope_value(envelope: &Envelope) -> CtValue {
    structure(
        "Envelope",
        vec![
            ("from", address_value(&envelope.from)),
            (
                "recipients",
                CtValue::List(envelope.recipients.iter().map(address_value).collect()),
            ),
        ],
    )
}

fn message_value(message: &Message) -> CtValue {
    structure(
        "Message",
        vec![
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
                CtValue::List(message.attachments.iter().map(attachment_value).collect()),
            ),
            ("envelope", envelope_value(&message.envelope)),
            ("wire_upper", CtValue::Int(message.wire_upper as i64)),
        ],
    )
}

fn message_from_value(value: &CtValue, span: Span) -> Result<Message, Diagnostic> {
    let address = |name| {
        field(value, "Message", name)
            .ok_or_else(|| unsupported("email Message field is missing", span))
            .and_then(|value| address_from_value(value, span))
    };
    let addresses = |name| {
        let Some(CtValue::List(values)) = field(value, "Message", name) else {
            return Err(unsupported("email Message address list is invalid", span));
        };
        values
            .iter()
            .map(|value| address_from_value(value, span))
            .collect::<Result<Vec<_>, _>>()
    };
    let text = |name| match field(value, "Message", name) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported("email Message text field is invalid", span)),
    };
    let attachments = match field(value, "Message", "attachments") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| attachment_from_value(value, span))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("email Message attachments are invalid", span)),
    };
    let from = address("from")?;
    let to = addresses("to")?;
    let bcc = addresses("bcc")?;
    let envelope = default_envelope(&from, &to, &bcc).map_err(|error| {
        unsupported(&format!("email Message envelope is invalid: {}", error.reason), span)
    })?;
    let wire_upper = match field(value, "Message", "wire_upper") {
        Some(CtValue::Int(value)) => usize::try_from(*value)
            .map_err(|_| unsupported("email Message wire bound is invalid", span))?,
        _ => return Err(unsupported("email Message wire bound is invalid", span)),
    };
    Ok(Message {
        from,
        to,
        bcc,
        subject: text("subject")?,
        text: text("text")?,
        html: text("html")?,
        attachments,
        envelope,
        wire_upper,
    })
}

fn bytes_value(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
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

fn address_list(value: &CtValue, span: Span) -> Result<Vec<Address>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("email call expected [Address]", span));
    };
    values
        .iter()
        .map(|value| address_from_value(value, span))
        .collect()
}

fn attachment_list(value: &CtValue, span: Span) -> Result<Vec<Attachment>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(unsupported("email call expected [Attachment]", span));
    };
    values
        .iter()
        .map(|value| attachment_from_value(value, span))
        .collect()
}

fn reject_controls(value: &str, what: &str) -> Result<(), EmailError> {
    if value.chars().any(char::is_control) {
        Err(email_error(
            "InvalidHeader",
            format!("{what} contains a forbidden control character"),
        ))
    } else {
        Ok(())
    }
}

fn parse_address(input: &str) -> Result<Address, EmailError> {
    reject_controls(input, "email address")?;
    let value = input.trim();
    if value.is_empty() || value.len() > 512 {
        return Err(email_error(
            "InvalidAddress",
            "email address must contain 1 to 512 bytes",
        ));
    }
    let opens = value.bytes().filter(|byte| *byte == b'<').count();
    let closes = value.bytes().filter(|byte| *byte == b'>').count();
    let (display, mailbox) = match (opens, closes) {
        (0, 0) => (None, value),
        (1, 1) if value.ends_with('>') => {
            let open = value.rfind('<').unwrap();
            let shown = value[..open].trim();
            if shown.is_empty() {
                return Err(email_error("InvalidAddress", "display name cannot be empty"));
            }
            (
                Some(parse_display(shown)?),
                value[open + 1..value.len() - 1].trim(),
            )
        }
        _ => {
            return Err(email_error(
                "InvalidAddress",
                "display address must have one final `<mailbox>`",
            ))
        }
    };
    validate_mailbox(mailbox)?;
    Ok(Address {
        display,
        mailbox: mailbox.to_string(),
    })
}

fn parse_display(value: &str) -> Result<String, EmailError> {
    if !value.starts_with('"') {
        if value.contains('"') || value.contains('<') || value.contains('>') {
            return Err(email_error(
                "InvalidAddress",
                "display name has an unmatched quote or angle bracket",
            ));
        }
        return Ok(value.to_string());
    }
    if value.len() < 2 || !value.ends_with('"') {
        return Err(email_error(
            "InvalidAddress",
            "quoted display name needs a closing quote",
        ));
    }
    let mut output = String::new();
    let mut escaped = false;
    for character in value[1..value.len() - 1].chars() {
        if escaped {
            if character != '"' && character != '\\' {
                return Err(email_error(
                    "InvalidAddress",
                    "quoted display name may escape only quote or backslash",
                ));
            }
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Err(email_error(
                "InvalidAddress",
                "quoted display name contains an unescaped quote",
            ));
        } else {
            output.push(character);
        }
    }
    if escaped || output.is_empty() {
        return Err(email_error(
            "InvalidAddress",
            "quoted display name is empty or ends with an escape",
        ));
    }
    Ok(output)
}

fn validate_mailbox(mailbox: &str) -> Result<(), EmailError> {
    if mailbox.is_empty() || !mailbox.is_ascii() || mailbox.len() > 254 {
        return Err(email_error(
            "InvalidAddress",
            "mailbox must be 1 to 254 ASCII bytes",
        ));
    }
    let separator = mailbox_separator(mailbox)?;
    let local = &mailbox[..separator];
    let domain = &mailbox[separator + 1..];
    if local.is_empty() || domain.is_empty() || local.len() > 64 || domain.len() > 253 {
        return Err(email_error(
            "InvalidAddress",
            "mailbox local part or domain has an invalid length",
        ));
    }
    if local.starts_with('"') {
        validate_quoted_local(local)?;
    } else if local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.bytes().all(is_atext)
    {
        return Err(email_error(
            "InvalidAddress",
            "mailbox local part is not dot-atom or quoted-string",
        ));
    }
    if domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(email_error(
            "InvalidAddress",
            "mailbox domain has an invalid label",
        ));
    }
    Ok(())
}

fn mailbox_separator(mailbox: &str) -> Result<usize, EmailError> {
    let mut quoted = false;
    let mut escaped = false;
    let mut separator = None;
    for (index, byte) in mailbox.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'@' && !quoted && separator.replace(index).is_some() {
            return Err(email_error(
                "InvalidAddress",
                "mailbox needs exactly one unquoted `@`",
            ));
        }
    }
    if quoted || escaped {
        return Err(email_error(
            "InvalidAddress",
            "mailbox has an unterminated quoted local part",
        ));
    }
    separator.ok_or_else(|| {
        email_error(
            "InvalidAddress",
            "mailbox needs one unquoted `@`",
        )
    })
}

fn validate_quoted_local(local: &str) -> Result<(), EmailError> {
    if local.len() < 2 || !local.ends_with('"') {
        return Err(email_error(
            "InvalidAddress",
            "quoted mailbox local part needs a closing quote",
        ));
    }
    let mut escaped = false;
    for byte in local[1..local.len() - 1].bytes() {
        if escaped {
            if !(33..=126).contains(&byte) {
                return Err(email_error(
                    "InvalidAddress",
                    "quoted mailbox escape is not printable ASCII",
                ));
            }
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' || !(32..=126).contains(&byte) {
            return Err(email_error(
                "InvalidAddress",
                "quoted mailbox local part contains an invalid byte",
            ));
        }
    }
    if escaped {
        return Err(email_error(
            "InvalidAddress",
            "quoted mailbox local part ends with an escape",
        ));
    }
    Ok(())
}

fn is_atext(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'/'
                | b'='
                | b'?'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
                | b'.'
        )
}

fn email_address(args: &[CtValue], span: Span) -> EvalResult {
    Ok(email_result(
        parse_address(string_arg(args, 0, span)?).map(|address| address_value(&address)),
    ))
}

fn make_attachment(
    filename: &str,
    mime: &str,
    bytes: Vec<u8>,
) -> Result<Attachment, EmailError> {
    reject_controls(filename, "attachment filename")?;
    reject_controls(mime, "attachment content type")?;
    if filename.trim().is_empty() || filename.contains('/') || filename.contains('\\') {
        return Err(email_error(
            "InvalidAttachment",
            "attachment filename must be a plain non-empty name",
        ));
    }
    if !valid_mime(mime) {
        return Err(email_error(
            "InvalidAttachment",
            "attachment content type must be `type/subtype`",
        ));
    }
    ensure_physical_header_len("Content-Type", mime.len())?;
    let disposition_len = "attachment; filename*=UTF-8''"
        .len()
        .checked_add(percent_encoded_len(filename)?)
        .ok_or_else(|| {
            email_error(
                "LimitExceeded",
                "attachment header length overflow",
            )
        })?;
    ensure_physical_header_len("Content-Disposition", disposition_len)?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(email_error(
            "LimitExceeded",
            format!("attachment exceeds {MAX_ATTACHMENT_BYTES} bytes"),
        ));
    }
    Ok(Attachment {
        filename: filename.to_string(),
        mime: mime.to_ascii_lowercase(),
        bytes,
    })
}

fn email_attachment(args: &[CtValue], span: Span) -> EvalResult {
    let filename = string_arg(args, 0, span)?;
    let mime = string_arg(args, 1, span)?;
    let bytes = args
        .get(2)
        .ok_or_else(|| unsupported("email.attachment(): missing bytes", span))?;
    let bytes = bytes_value(bytes, span)?;
    Ok(email_result(
        make_attachment(filename, mime, bytes)
            .map(|attachment| attachment_value(&attachment)),
    ))
}

fn make_envelope(
    from: &Address,
    recipients: &[Address],
) -> Result<Envelope, EmailError> {
    if recipients.is_empty() {
        return Err(email_error(
            "envelope",
            "email envelope needs at least one recipient",
        ));
    }
    if recipients.len() > MAX_RECIPIENTS {
        return Err(email_error(
            "envelope",
            format!("email envelope exceeds {MAX_RECIPIENTS} recipients"),
        ));
    }
    Ok(Envelope {
        from: from.clone(),
        recipients: recipients.to_vec(),
    })
}

fn default_envelope(
    from: &Address,
    to: &[Address],
    bcc: &[Address],
) -> Result<Envelope, EmailError> {
    let mut recipients = Vec::with_capacity(to.len().saturating_add(bcc.len()));
    recipients.extend_from_slice(to);
    recipients.extend_from_slice(bcc);
    make_envelope(from, &recipients)
}

fn email_envelope(args: &[CtValue], span: Span) -> EvalResult {
    let from = address_from_value(
        args.get(0)
            .ok_or_else(|| unsupported("email.envelope(): missing sender", span))?,
        span,
    )?;
    let recipients = address_list(
        args.get(1)
            .ok_or_else(|| unsupported("email.envelope(): missing recipients", span))?,
        span,
    )?;
    Ok(email_result(
        make_envelope(&from, &recipients).map(|envelope| envelope_value(&envelope)),
    ))
}

fn make_message(
    from: Address,
    to: Vec<Address>,
    bcc: Vec<Address>,
    subject: String,
    text: String,
    html: String,
    attachments: Vec<Attachment>,
) -> Result<Message, EmailError> {
    reject_controls(&subject, "subject")?;
    if subject.len() > MAX_HEADER_VALUE_BYTES {
        return Err(email_error(
            "LimitExceeded",
            format!("subject exceeds {MAX_HEADER_VALUE_BYTES} bytes"),
        ));
    }
    if to.is_empty() {
        return Err(email_error(
            "InvalidMessage",
            "message needs at least one visible recipient",
        ));
    }
    let recipients = to
        .len()
        .checked_add(bcc.len())
        .ok_or_else(|| email_error("LimitExceeded", "recipient count overflow"))?;
    if recipients > MAX_RECIPIENTS {
        return Err(email_error(
            "LimitExceeded",
            format!("message exceeds {MAX_RECIPIENTS} recipients"),
        ));
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(email_error(
            "LimitExceeded",
            format!("message exceeds {MAX_ATTACHMENTS} attachments"),
        ));
    }
    if text.is_empty() && html.is_empty() {
        return Err(email_error(
            "InvalidMessage",
            "message needs text or HTML content",
        ));
    }
    if text.len() > MAX_BODY_BYTES || html.len() > MAX_BODY_BYTES {
        return Err(email_error(
            "LimitExceeded",
            format!("each message body is limited to {MAX_BODY_BYTES} bytes"),
        ));
    }
    let wire_upper = prospective_wire_upper(
        &from,
        &to,
        &subject,
        &text,
        &html,
        &attachments,
    )?;
    if wire_upper > MAX_MESSAGE_BYTES {
        return Err(email_error(
            "LimitExceeded",
            format!("serialized message exceeds {MAX_MESSAGE_BYTES} bytes"),
        ));
    }
    ensure_rendered_address_header("From", std::slice::from_ref(&from))?;
    ensure_rendered_address_header("To", &to)?;
    ensure_encoded_header_lines("Subject", &subject)?;
    let envelope = default_envelope(&from, &to, &bcc)?;
    Ok(Message {
        from,
        to,
        bcc,
        subject,
        text,
        html,
        attachments,
        envelope,
        wire_upper,
    })
}

fn email_message(args: &[CtValue], span: Span) -> EvalResult {
    let from = address_from_value(
        args.get(0)
            .ok_or_else(|| unsupported("email.message(): missing sender", span))?,
        span,
    )?;
    let to = address_list(
        args.get(1)
            .ok_or_else(|| unsupported("email.message(): missing recipients", span))?,
        span,
    )?;
    let bcc = address_list(
        args.get(2)
            .ok_or_else(|| unsupported("email.message(): missing bcc", span))?,
        span,
    )?;
    let subject = string_arg(args, 3, span)?.to_string();
    let text = string_arg(args, 4, span)?.to_string();
    let html = string_arg(args, 5, span)?.to_string();
    let attachments = attachment_list(
        args.get(6)
            .ok_or_else(|| unsupported("email.message(): missing attachments", span))?,
        span,
    )?;
    Ok(email_result(
        make_message(from, to, bcc, subject, text, html, attachments)
            .map(|message| message_value(&message)),
    ))
}

fn prospective_wire_upper(
    from: &Address,
    to: &[Address],
    subject: &str,
    text: &str,
    html: &str,
    attachments: &[Attachment],
) -> Result<usize, EmailError> {
    let mut total = 4096_usize;
    checked_add(&mut total, rendered_address_len(from))?;
    for address in to {
        checked_add(
            &mut total,
            rendered_address_len(address).saturating_add(4),
        )?;
    }
    checked_add(
        &mut total,
        encoded_header_len(subject).saturating_add(32),
    )?;
    checked_add(
        &mut total,
        base64_lines_len(text.len()).saturating_add(256),
    )?;
    checked_add(
        &mut total,
        base64_lines_len(html.len()).saturating_add(256),
    )?;
    for attachment in attachments {
        checked_add(&mut total, base64_lines_len(attachment.bytes.len()))?;
        checked_add(&mut total, attachment.mime.len())?;
        checked_add(&mut total, percent_encoded_len(&attachment.filename)?)?;
        checked_add(&mut total, 512)?;
    }
    checked_add(
        &mut total,
        attachments.len().saturating_mul(256),
    )?;
    Ok(total)
}

fn checked_add(total: &mut usize, amount: usize) -> Result<(), EmailError> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| email_error("LimitExceeded", "message size overflow"))?;
    Ok(())
}

fn serialize_message(message: &Message) -> Result<Vec<u8>, EmailError> {
    let mixed = boundary(message, "mixed");
    let alternative = boundary(message, "alternative");
    let mut output = String::with_capacity(message.wire_upper.min(MAX_MESSAGE_BYTES));
    push_header(&mut output, "From", &render_address(&message.from))?;
    push_header(&mut output, "To", &render_addresses(&message.to, "To"))?;
    push_header(&mut output, "Subject", &encode_header(&message.subject))?;
    push_header(&mut output, "MIME-Version", "1.0")?;
    if message.attachments.is_empty() {
        render_body(&mut output, message, &alternative)?;
    } else {
        push_header(
            &mut output,
            "Content-Type",
            &format!("multipart/mixed; boundary=\"{mixed}\""),
        )?;
        output.push_str("\r\n");
        output.push_str(&format!("--{mixed}\r\n"));
        render_body(&mut output, message, &alternative)?;
        for attachment in &message.attachments {
            output.push_str(&format!("\r\n--{mixed}\r\n"));
            push_header(&mut output, "Content-Type", &attachment.mime)?;
            push_header(&mut output, "Content-Transfer-Encoding", "base64")?;
            push_header(
                &mut output,
                "Content-Disposition",
                &format!(
                    "attachment; filename*=UTF-8''{}",
                    percent_encode(&attachment.filename)?
                ),
            )?;
            output.push_str("\r\n");
            output.push_str(&base64_lines(&attachment.bytes));
        }
        output.push_str(&format!("\r\n--{mixed}--\r\n"));
    }
    if output.len() > message.wire_upper || output.len() > MAX_MESSAGE_BYTES {
        return Err(email_error(
            "LimitExceeded",
            "serialized message exceeded its checked wire bound",
        ));
    }
    Ok(output.into_bytes())
}

fn email_serialize(args: &[CtValue], span: Span) -> EvalResult {
    let message = message_from_value(
        args.first()
            .ok_or_else(|| unsupported("email.serialize(): missing message", span))?,
        span,
    )?;
    Ok(email_result(
        serialize_message(&message).map(CtValue::Bytes),
    ))
}

fn render_body(
    output: &mut String,
    message: &Message,
    alternative: &str,
) -> Result<(), EmailError> {
    if message.html.is_empty() {
        text_part(output, "text/plain", &message.text)?;
    } else if message.text.is_empty() {
        text_part(output, "text/html", &message.html)?;
    } else {
        push_header(
            output,
            "Content-Type",
            &format!("multipart/alternative; boundary=\"{alternative}\""),
        )?;
        output.push_str("\r\n");
        output.push_str(&format!("--{alternative}\r\n"));
        text_part(output, "text/plain", &message.text)?;
        output.push_str(&format!("\r\n--{alternative}\r\n"));
        text_part(output, "text/html", &message.html)?;
        output.push_str(&format!("\r\n--{alternative}--\r\n"));
    }
    Ok(())
}

fn text_part(output: &mut String, mime: &str, body: &str) -> Result<(), EmailError> {
    push_header(output, "Content-Type", &format!("{mime}; charset=utf-8"))?;
    push_header(output, "Content-Transfer-Encoding", "base64")?;
    output.push_str("\r\n");
    output.push_str(&base64_lines(body.as_bytes()));
    Ok(())
}

fn push_header(output: &mut String, name: &str, value: &str) -> Result<(), EmailError> {
    validate_folded_header(name, value)?;
    output.push_str(name);
    output.push_str(": ");
    output.push_str(value);
    output.push_str("\r\n");
    Ok(())
}

fn validate_folded_header(name: &str, value: &str) -> Result<(), EmailError> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] == b'\r'
            && (bytes.get(index + 1) != Some(&b'\n')
                || !matches!(bytes.get(index + 2), Some(b' ' | b'\t')))
        {
            return Err(email_error(
                "InvalidHeader",
                format!("{name} contains an invalid fold"),
            ));
        }
        if bytes[index] == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
            return Err(email_error(
                "InvalidHeader",
                format!("{name} contains a bare newline"),
            ));
        }
        if bytes[index] < 32 && !matches!(bytes[index], b'\r' | b'\n' | b'\t') {
            return Err(email_error(
                "InvalidHeader",
                format!("{name} contains a control byte"),
            ));
        }
    }
    for (index, line) in value.split("\r\n").enumerate() {
        let prefix = if index == 0 { name.len() + 2 } else { 0 };
        if prefix.saturating_add(line.len()).saturating_add(2) > MAX_HEADER_BYTES {
            return Err(email_error(
                "LimitExceeded",
                format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes"),
            ));
        }
    }
    Ok(())
}

fn ensure_physical_header_len(name: &str, value_len: usize) -> Result<(), EmailError> {
    if name
        .len()
        .saturating_add(2)
        .saturating_add(value_len)
        .saturating_add(2)
        > MAX_HEADER_BYTES
    {
        Err(email_error(
            "LimitExceeded",
            format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes"),
        ))
    } else {
        Ok(())
    }
}

fn ensure_encoded_header_lines(name: &str, value: &str) -> Result<(), EmailError> {
    if value.is_ascii() {
        ensure_physical_header_len(name, value.len())
    } else if name.len() + 2 + 72 + 2 > MAX_HEADER_BYTES {
        Err(email_error(
            "LimitExceeded",
            format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes"),
        ))
    } else {
        Ok(())
    }
}

fn ensure_rendered_address_header(
    name: &str,
    addresses: &[Address],
) -> Result<(), EmailError> {
    validate_folded_header(name, &render_addresses(addresses, name))
}

fn render_addresses(addresses: &[Address], name: &str) -> String {
    let mut output = String::new();
    let mut physical = name.len() + 2;
    for (index, address) in addresses.iter().enumerate() {
        let rendered = render_address(address);
        let separator = if index == 0 { "" } else { ", " };
        if index > 0
            && physical
                + separator.len()
                + rendered.lines().next().unwrap_or("").len()
                + 2
                > MAX_HEADER_BYTES
        {
            output.push_str(",\r\n ");
            physical = 1;
        } else {
            output.push_str(separator);
            physical += separator.len();
        }
        output.push_str(&rendered);
        physical = rendered.rsplit("\r\n").next().unwrap_or("").len()
            + if rendered.contains("\r\n") {
                0
            } else {
                physical
            };
    }
    output
}

fn render_address(address: &Address) -> String {
    match &address.display {
        Some(display) if display.is_ascii() => {
            format!(
                "{} <{}>",
                render_ascii_display(display),
                address.mailbox
            )
        }
        Some(display) => format!("{} <{}>", encode_header(display), address.mailbox),
        None => address.mailbox.clone(),
    }
}

fn render_ascii_display(display: &str) -> String {
    let phrase_safe = display
        .split(' ')
        .all(|word| !word.is_empty() && word.bytes().all(is_atext));
    if phrase_safe {
        display.to_string()
    } else {
        format!("\"{}\"", display.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn rendered_address_len(address: &Address) -> usize {
    match &address.display {
        Some(display) if display.is_ascii() => {
            let safe = display
                .split(' ')
                .all(|word| !word.is_empty() && word.bytes().all(is_atext));
            let shown = if safe {
                display.len()
            } else {
                2 + display
                    .bytes()
                    .filter(|byte| matches!(byte, b'\\' | b'"'))
                    .count()
                    + display.len()
            };
            shown
                .saturating_add(address.mailbox.len())
                .saturating_add(3)
        }
        Some(display) => encoded_header_len(display)
            .saturating_add(address.mailbox.len())
            .saturating_add(3),
        None => address.mailbox.len(),
    }
}

fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    let mut output = String::with_capacity(encoded_header_len(value));
    let mut start = 0_usize;
    for (index, character) in value.char_indices() {
        if index > start && index + character.len_utf8() - start > 45 {
            if !output.is_empty() {
                output.push_str("\r\n ");
            }
            output.push_str("=?UTF-8?B?");
            output.push_str(&base64(&value.as_bytes()[start..index]));
            output.push_str("?=");
            start = index;
        }
    }
    if start < value.len() {
        if !output.is_empty() {
            output.push_str("\r\n ");
        }
        output.push_str("=?UTF-8?B?");
        output.push_str(&base64(&value.as_bytes()[start..]));
        output.push_str("?=");
    }
    output
}

fn encoded_header_len(value: &str) -> usize {
    if value.is_ascii() {
        return value.len();
    }
    let mut total = 0_usize;
    let mut start = 0_usize;
    let mut chunks = 0_usize;
    for (index, character) in value.char_indices() {
        if index > start && index + character.len_utf8() - start > 45 {
            total = total
                .saturating_add(12)
                .saturating_add(base64_len(index - start));
            start = index;
            chunks += 1;
        }
    }
    if start < value.len() {
        total = total
            .saturating_add(12)
            .saturating_add(base64_len(value.len() - start));
        chunks += 1;
    }
    total.saturating_add(chunks.saturating_sub(1).saturating_mul(3))
}

fn valid_mime(value: &str) -> bool {
    let mut parts = value.split('/');
    let top = parts.next().unwrap_or("");
    let sub = parts.next().unwrap_or("");
    !top.is_empty()
        && !sub.is_empty()
        && parts.next().is_none()
        && top.bytes().chain(sub.bytes()).all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                )
        })
}

fn boundary(message: &Message, label: &str) -> String {
    let mut state = crate::SHA256::sha256(label.as_bytes());
    let mut index = 1_u8;
    let mut absorb = |bytes: &[u8]| {
        let digest = crate::SHA256::sha256(bytes);
        for slot in 0..32 {
            state[slot] = state[slot]
                .wrapping_add(digest[(slot + index as usize) % 32])
                .rotate_left((index % 7) as u32)
                ^ index;
        }
        index = index.wrapping_add(1);
    };
    absorb(message.subject.as_bytes());
    absorb(message.text.as_bytes());
    absorb(message.html.as_bytes());
    absorb(message.from.mailbox.as_bytes());
    for address in &message.to {
        absorb(address.mailbox.as_bytes());
    }
    for address in &message.bcc {
        absorb(address.mailbox.as_bytes());
    }
    for attachment in &message.attachments {
        absorb(&attachment.bytes);
    }
    let digest = crate::SHA256::sha256(&state);
    let suffix = digest[..24]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("jet-{label}-{suffix}")
}

fn base64_lines_len(bytes: usize) -> usize {
    let encoded = base64_len(bytes);
    if encoded == 0 {
        0
    } else {
        encoded.saturating_add(((encoded + 75) / 76).saturating_sub(1).saturating_mul(2))
    }
}

fn base64_len(bytes: usize) -> usize {
    bytes.saturating_add(2) / 3 * 4
}

fn base64_lines(bytes: &[u8]) -> String {
    let encoded = base64(bytes);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(base64_len(bytes.len()));
    for chunk in bytes.chunks(3) {
        let bits = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        output.push(TABLE[(bits >> 18) as usize] as char);
        output.push(TABLE[((bits >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((bits >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(bits & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn percent_encoded_len(value: &str) -> Result<usize, EmailError> {
    value.bytes().try_fold(0_usize, |total, byte| {
        total
            .checked_add(
                if byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'_' | b'.' | b'~')
                {
                    1
                } else {
                    3
                },
            )
            .ok_or_else(|| {
                email_error(
                    "LimitExceeded",
                    "attachment filename encoding length overflow",
                )
            })
    })
}

fn percent_encode(value: &str) -> Result<String, EmailError> {
    let mut output = String::with_capacity(percent_encoded_len(value)?);
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(output)
}

// ── D-APPROX1=A: core.sketch — mirrors AOT Jet* sketches in Prelude/Core.rs ──

const CMS_COLS: usize = 256;
const HLL_REGS: usize = 256;
const TDIGEST_DELTA: f64 = 100.0;

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn fnv1a_h2(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325u64.wrapping_add(0xdeadbeef);
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

fn hll_new() -> CtValue {
    structure(
        "HyperLogLog",
        vec![(
            "registers",
            CtValue::List((0..HLL_REGS).map(|_| CtValue::Int(0)).collect()),
        )],
    )
}

fn hll_registers(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    let CtValue::List(regs) = field(value, "HyperLogLog", "registers")
        .ok_or_else(|| unsupported("malformed HyperLogLog.registers value", span))?
    else {
        return Err(unsupported("malformed HyperLogLog.registers value", span));
    };
    regs.iter()
        .map(|reg| match reg {
            CtValue::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
            _ => Err(unsupported("malformed HyperLogLog register", span)),
        })
        .collect()
}

fn hll_value(registers: Vec<u8>) -> CtValue {
    structure(
        "HyperLogLog",
        vec![(
            "registers",
            CtValue::List(registers.into_iter().map(|n| CtValue::Int(n as i64)).collect()),
        )],
    )
}

fn hll_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let item = string_arg(args, 0, span)?;
    let mut regs = hll_registers(recv, span)?;
    let h = fnv1a(item.as_bytes());
    let reg = (h & 0xFF) as usize;
    let rest = h >> 8;
    let lz = if rest == 0 {
        57u8
    } else {
        rest.leading_zeros() as u8 + 1
    };
    if lz > regs[reg] {
        regs[reg] = lz;
    }
    Ok(hll_value(regs))
}

fn hll_count(recv: &CtValue, span: Span) -> EvalResult {
    let regs = hll_registers(recv, span)?;
    let m = regs.len() as f64;
    let zeros = regs.iter().filter(|&&v| v == 0).count();
    let estimate = if zeros > 0 {
        m * (m / zeros as f64).ln()
    } else {
        let sum: f64 = regs.iter().map(|&v| 2f64.powi(-(v as i32))).sum();
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        alpha * m * m / sum
    };
    Ok(CtValue::Int(estimate.round() as i64))
}

fn tdigest_new() -> CtValue {
    structure("TDigest", vec![("centroids", CtValue::List(Vec::new()))])
}

fn tdigest_centroids(value: &CtValue, span: Span) -> Result<Vec<(f64, f64)>, Diagnostic> {
    let CtValue::List(items) = field(value, "TDigest", "centroids")
        .ok_or_else(|| unsupported("malformed TDigest.centroids value", span))?
    else {
        return Err(unsupported("malformed TDigest.centroids value", span));
    };
    items
        .iter()
        .map(|item| match item {
            CtValue::List(pair) if pair.len() == 2 => {
                Ok((as_float(&pair[0], span)?, as_float(&pair[1], span)?))
            }
            _ => Err(unsupported("malformed TDigest centroid", span)),
        })
        .collect()
}

fn tdigest_value(centroids: Vec<(f64, f64)>) -> CtValue {
    structure(
        "TDigest",
        vec![(
            "centroids",
            CtValue::List(
                centroids
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

fn tdigest_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let v = float_arg(args, 0, span)?;
    let mut cs = tdigest_centroids(recv, span)?;
    let idx = cs.partition_point(|&(m, _)| m < v);
    cs.insert(idx, (v, 1.0));
    let total: f64 = cs.iter().map(|(_, w)| w).sum();
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(cs.len());
    let mut cum = 0.0f64;
    for &(mean, weight) in cs.iter() {
        if merged.is_empty() {
            merged.push((mean, weight));
            cum += weight;
            continue;
        }
        let last = merged.last_mut().unwrap();
        let q = cum / total;
        let limit = 4.0 * total * q * (1.0 - q) / TDIGEST_DELTA;
        if last.1 + weight <= limit.max(1.0) {
            let new_w = last.1 + weight;
            last.0 = (last.0 * last.1 + mean * weight) / new_w;
            last.1 = new_w;
        } else {
            merged.push((mean, weight));
            cum += weight;
        }
    }
    Ok(tdigest_value(merged))
}

fn tdigest_quantile(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let q = float_arg(args, 0, span)?;
    let cs = tdigest_centroids(recv, span)?;
    if cs.is_empty() {
        return Ok(CtValue::Float(CtFloat::f64(0.0)));
    }
    let total: f64 = cs.iter().map(|(_, w)| w).sum();
    let target = q * total;
    let mut cum = 0.0f64;
    for &(mean, weight) in cs.iter() {
        cum += weight;
        if cum >= target {
            return Ok(CtValue::Float(CtFloat::f64(mean)));
        }
    }
    Ok(CtValue::Float(CtFloat::f64(cs.last().unwrap().0)))
}

fn cms_new() -> CtValue {
    structure(
        "CountMinSketch",
        vec![(
            "rows",
            CtValue::List(
                (0..4)
                    .map(|_| {
                        CtValue::List((0..CMS_COLS).map(|_| CtValue::Int(0)).collect())
                    })
                    .collect(),
            ),
        )],
    )
}

fn cms_rows(value: &CtValue, span: Span) -> Result<[[u32; CMS_COLS]; 4], Diagnostic> {
    let CtValue::List(rows) = field(value, "CountMinSketch", "rows")
        .ok_or_else(|| unsupported("malformed CountMinSketch.rows value", span))?
    else {
        return Err(unsupported("malformed CountMinSketch.rows value", span));
    };
    if rows.len() != 4 {
        return Err(unsupported("malformed CountMinSketch.rows value", span));
    }
    let mut out = [[0u32; CMS_COLS]; 4];
    for (row_idx, row) in rows.iter().enumerate() {
        let CtValue::List(cols) = row else {
            return Err(unsupported("malformed CountMinSketch row", span));
        };
        if cols.len() != CMS_COLS {
            return Err(unsupported("malformed CountMinSketch row", span));
        }
        for (col_idx, cell) in cols.iter().enumerate() {
            let CtValue::Int(n) = cell else {
                return Err(unsupported("malformed CountMinSketch cell", span));
            };
            if !(0..=u32::MAX as i64).contains(n) {
                return Err(unsupported("malformed CountMinSketch cell", span));
            }
            out[row_idx][col_idx] = *n as u32;
        }
    }
    Ok(out)
}

fn cms_value(rows: [[u32; CMS_COLS]; 4]) -> CtValue {
    structure(
        "CountMinSketch",
        vec![(
            "rows",
            CtValue::List(
                rows.into_iter()
                    .map(|row| {
                        CtValue::List(row.into_iter().map(|n| CtValue::Int(n as i64)).collect())
                    })
                    .collect(),
            ),
        )],
    )
}

fn cms_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let key = string_arg(args, 0, span)?;
    let bytes = key.as_bytes();
    let h1 = fnv1a(bytes);
    let h2 = fnv1a_h2(bytes);
    let mut tbl = cms_rows(recv, span)?;
    for row in 0..4usize {
        let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
        tbl[row][col] = tbl[row][col].saturating_add(1);
    }
    Ok(cms_value(tbl))
}

fn cms_count(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let key = string_arg(args, 0, span)?;
    let bytes = key.as_bytes();
    let h1 = fnv1a(bytes);
    let h2 = fnv1a_h2(bytes);
    let tbl = cms_rows(recv, span)?;
    let min = (0..4usize)
        .map(|row| {
            let col = ((h1.wrapping_add(h2.wrapping_mul(row as u64 + 1))) & 0xFF) as usize;
            tbl[row][col]
        })
        .min()
        .unwrap();
    Ok(CtValue::Int(min as i64))
}

fn reservoir_new(args: &[CtValue], span: Span) -> EvalResult {
    let capacity = int_arg(args, 0, span)?.max(1);
    Ok(structure(
        "ReservoirSampler",
        vec![
            ("capacity", CtValue::Int(capacity)),
            ("count", CtValue::Int(0)),
            ("rng", CtValue::Int(0xdeadbeef_cafebabeu64 as i64)),
            ("reservoir", CtValue::List(Vec::new())),
        ],
    ))
}

fn reservoir_add(recv: &CtValue, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let item = string_arg(args, 0, span)?.to_string();
    let capacity = int_field(recv, "ReservoirSampler", "capacity", span)? as usize;
    let mut count = int_field(recv, "ReservoirSampler", "count", span)? as u64;
    let mut rng = int_field(recv, "ReservoirSampler", "rng", span)? as u64;
    let CtValue::List(mut reservoir) = value_field(recv, "ReservoirSampler", "reservoir", span)?
    else {
        return Err(unsupported("malformed ReservoirSampler.reservoir value", span));
    };
    count += 1;
    if reservoir.len() < capacity {
        reservoir.push(CtValue::Str(item));
    } else {
        let mut x = rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        rng = x;
        let j = (x % count) as usize;
        if j < capacity {
            reservoir[j] = CtValue::Str(item);
        }
    }
    Ok(structure(
        "ReservoirSampler",
        vec![
            ("capacity", CtValue::Int(capacity as i64)),
            ("count", CtValue::Int(count as i64)),
            ("rng", CtValue::Int(rng as i64)),
            ("reservoir", CtValue::List(reservoir)),
        ],
    ))
}

fn reservoir_sample(recv: &CtValue, span: Span) -> EvalResult {
    value_field(recv, "ReservoirSampler", "reservoir", span)
}

// ── D-SOLVER-LIB1=A: mirrors AOT jet_solver_* in MathRandomTime.rs ──────────

fn solver_require_update(recv: &CtValue, args: &[CtValue], span: Span) -> EvalResult {
    let seed = int_field(recv, crate::Syntax::SOLVER_TYPE, "seed", span)?;
    let checked = int_field(recv, crate::Syntax::SOLVER_TYPE, "checked", span)?;
    let failures = int_field(recv, crate::Syntax::SOLVER_TYPE, "failures", span)?;
    let ok = as_bool(one(args, 0, "Solver", "require", span)?, span)?;
    Ok(structure(
        crate::Syntax::SOLVER_TYPE,
        vec![
            ("seed", CtValue::Int(seed)),
            ("checked", CtValue::Int(checked + 1)),
            (
                "failures",
                CtValue::Int(if ok { failures } else { failures + 1 }),
            ),
        ],
    ))
}

fn solver_failure_count(recv: &CtValue, span: Span) -> EvalResult {
    Ok(CtValue::Int(int_field(
        recv,
        crate::Syntax::SOLVER_TYPE,
        "failures",
        span,
    )?))
}

fn solver_status(recv: &CtValue, span: Span) -> EvalResult {
    let failures = int_field(recv, crate::Syntax::SOLVER_TYPE, "failures", span)?;
    Ok(CtValue::Str(
        if failures == 0 { "ok" } else { "failed" }.to_string(),
    ))
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
            CtValue::ResErr(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            )))
        );
    }
}
