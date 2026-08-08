//! Remaining deterministic Core calls for #392.
//!
//! Algorithms and value layouts mirror the AOT prelude. This module owns one
//! evaluator used by comptime and the REPL; callers never synthesize schemas or
//! fall back after a recognized call fails.

use std::collections::BTreeMap;

use super::mime_kernel;
use crate::AST::{CtFloat, CtReport, CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};

use crate::Comptime::Builtins::{as_bool, as_int};
use crate::Comptime::Diagnostics::unsupported;
use crate::Comptime::EmailAdapter;
use crate::Comptime::Methods::as_float;

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
        ("core.email", method) => return EmailAdapter::evaluate(method, args, span),
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
        ("core.time", "instant") => Ok(structure("Instant", vec![("start_ns", CtValue::Int(0))])),
        ("core.time", "datetime") => datetime_parts(args, span),
        ("core.time", "time" | "local_time") => local_time_parts(args, span),
        ("core.time", "days_in_month") => time_days_in_month(args, span),
        ("core.time", "is_leap_year") => time_is_leap_year(args, span),
        ("core.time", "nanoseconds" | "microseconds" | "milliseconds" | "seconds" | "minutes" | "hours") => {
            duration_ctor(method, args, span)
        }
        ("core.math", "decimal") => decimal_from_str(args, span),
        ("core.math", "fraction") => fraction_new(args, span),
        ("core.science.measurement", "from") => measurement(args, span),
        ("core.time.date", "new") => date_new_call(args, span),
        ("core.time.date", "parse") => date_parse_call(args, span),
        // Wall-clock read — same JetDate::today_utc as AOT/JIT hosts (I9).
        ("core.time.date", "today") => Ok(Date::today_utc().value()),
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
        ("core.net", "ip_to_string") => net_string_field(args, "IPAddr", "text", span),
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
        ("core.net", "dns_srv_target") => net_string_field(args, "DNSSrv", "target", span),
        ("core.net", "dns_srv_port") => net_value_field(args, "DNSSrv", "port", span),
        ("core.net", "dns_srv_priority") => net_value_field(args, "DNSSrv", "priority", span),
        ("core.net", "dns_srv_weight") => net_value_field(args, "DNSSrv", "weight", span),
        ("core.net", "udp_packet_data") => net_udp_packet_data(args, span),
        ("core.net", "udp_packet_bytes") => net_value_field(args, "UDPPacket", "data", span),
        ("core.net", "udp_packet_addr") => net_value_field(args, "UDPPacket", "addr", span),
        ("core.net", "udp_packet_original_len") => net_value_field(args, "UDPPacket", "original_len", span),
        ("core.net", "udp_packet_truncated") => net_value_field(args, "UDPPacket", "truncated", span),
        ("core.crypto.expert", "ed25519_verify_strict") => crypto_ed25519_verify(args, span),
        ("core.crypto.expert", "ed25519_sign") => crypto_ed25519_sign(args, span),
        ("core.crypto.expert", "hkdf_sha256_raw") => crypto_hkdf(args, span),
        ("core.crypto.expert", "x25519_raw") => crypto_x25519(args, span),
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
        // TIR lowers Signature/VerifyKey/… `.bytes()` to core.crypto.__*_bytes;
        // keep those pure field extracts resident so REPL does not hit E1802.
        ("core.crypto", "__signature_bytes") => crypto_extract(args, 0, "Signature", span),
        ("core.crypto", "__verify_key_bytes") => crypto_extract(args, 0, "VerifyKey", span),
        ("core.crypto", "__x25519_public_bytes") => crypto_extract(args, 0, "X25519PublicKey", span),
        ("core.crypto", "__sealed_bytes") => crypto_extract(args, 0, "Sealed", span),
        ("core.crypto", "__digest256_bytes") => crypto_extract(args, 0, "Digest256", span),
        ("core.crypto", "__digest512_bytes") => crypto_extract(args, 0, "Digest512", span),
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
        ("Date" | "LocalDate", "quarter_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int((date.month - 1) / 3 + 1)),
        ("Date" | "LocalDate", "days_in_month", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(Date::days_in_month(date.year, date.month))),
        ("Date" | "LocalDate", "is_leap_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Bool(Date::is_leap(date.year))),
        ("Date" | "LocalDate", "replace", 3) => date_from_value(recv, type_name, span).and_then(
            |_date| {
                Ok(Date::new(
                    as_int(&args[0], span)?,
                    as_int(&args[1], span)?,
                    as_int(&args[2], span)?,
                )
                .value())
            },
        ),
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
        ("DateTime", "millisecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int((date_time.nanos / 1_000_000) as i64)),
        ("DateTime", "microsecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int((date_time.nanos / 1_000) as i64)),
        ("DateTime", "nanosecond", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.nanos as i64)),
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
                let ns = duration_ns(&args[0], span)?;
                Ok(date_time.plus_ns(ns).value())
            })
        }
        ("DateTime", "difference", 1) => datetime_from_value(recv, span).and_then(|left| {
            let right = datetime_from_value(&args[0], span)?;
            Ok(duration_value(left.total_ns().saturating_sub(right.total_ns())))
        }),
        ("DateTime", "truncate" | "round" | "floor" | "ceil", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                Ok(date_time.align(string_arg(args, 0, span)?, method).value())
            })
        }
        ("DateTime", "replace", 6) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(DateTime::from_parts(
                as_int(&args[0], span)?,
                as_int(&args[1], span)?,
                as_int(&args[2], span)?,
                as_int(&args[3], span)?,
                as_int(&args[4], span)?,
                as_int(&args[5], span)?,
                date_time.nanos,
            )
            .value())
        }),
        ("DateTime", "in_zone", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(ZonedDateTime {
                instant: date_time,
                zone: zone_from_value(&args[0], span)?,
            }
            .value())
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
            Ok(ZonedDateTime {
                instant: zoned.instant.plus_ns(ns),
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
        // of the mangled `user_DataError { user_kind: … }` shape (#1250).
        CtValue::Struct { type_name, fields }
            if type_name.strip_prefix("user_").unwrap_or(type_name.as_str()) == "DataError" =>
        {
            let get = |name: &str| -> Option<&CtValue> {
                fields.iter().find_map(|(n, v)| {
                    let n = n.strip_prefix("user_").unwrap_or(n.as_str());
                    (n == name).then_some(v)
                })
            };
            let kind = match get("kind")? {
                CtValue::Enum { variant, .. } => {
                    variant.strip_prefix("user_").unwrap_or(variant).to_string()
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
        // not Rust `user_*` Debug — matching AOT JetShow for these foreign types.
        CtValue::Struct { type_name, fields }
            if matches!(
                type_name.strip_prefix("user_").unwrap_or(type_name.as_str()),
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
            let ty = type_name.strip_prefix("user_").unwrap_or(type_name);
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, v)| {
                    let field = name.strip_prefix("user_").unwrap_or(name);
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
    Ok(match text.parse::<std::net::IpAddr>() {
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
    Ok(CtValue::Bool(text.parse::<std::net::Ipv4Addr>().is_ok()))
}

fn net_socket_addr_parse(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    Ok(match text.parse::<std::net::SocketAddr>() {
        Ok(address) => CtValue::Present(Box::new(structure("SocketAddr", vec![
            ("host", CtValue::Str(address.ip().to_string())),
            ("port", CtValue::Int(i64::from(address.port()))),
            ("text", CtValue::Str(address.to_string())),
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
        &bytes_value(one(args, 0, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        &bytes_value(one(args, 1, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        &bytes_value(one(args, 2, "core.crypto.expert", "hkdf_sha256_raw", span)?, span)?,
        length as usize,
    );
    Ok(CtValue::Present(Box::new(crypto_secret("Secret", bytes))))
}

fn crypto_ed25519_verify(args: &[CtValue], span: Span) -> EvalResult {
    let public = bytes_value(one(args, 0, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let message = bytes_value(one(args, 1, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
    let signature = bytes_value(one(args, 2, "core.crypto.expert", "ed25519_verify_strict", span)?, span)?;
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
    let seed = bytes_value(one(args, 0, "core.crypto.expert", "ed25519_sign", span)?, span)?;
    let message = bytes_value(one(args, 1, "core.crypto.expert", "ed25519_sign", span)?, span)?;
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
    let secret = bytes_value(one(args, 0, "core.crypto.expert", "x25519_raw", span)?, span)?;
    let public = bytes_value(one(args, 1, "core.crypto.expert", "x25519_raw", span)?, span)?;
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

    fn today_utc() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        let days_since_1970 = secs / 86_400;
        let epoch = Date::new(1970, 1, 1).day_number();
        Date::from_day_number(epoch + days_since_1970)
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
    nanos: u32,
}

impl DateTime {
    fn from_parts(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        nanos: u32,
    ) -> Self {
        let date = Date::new(year, month, day);
        let time = LocalTime::new(hour, minute, second);
        Self {
            seconds: utc_seconds(date, time),
            nanos: nanos % 1_000_000_000,
        }
    }

    fn date(self) -> Date {
        let epoch = Date::new(1970, 1, 1).day_number();
        Date::from_day_number(epoch + self.seconds.div_euclid(86_400))
    }

    fn time(self) -> LocalTime {
        let seconds = self.seconds.rem_euclid(86_400);
        LocalTime::new(seconds / 3_600, (seconds / 60) % 60, seconds % 60)
    }

    fn total_ns(self) -> i64 {
        self.seconds
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nanos as i64)
    }

    fn from_total_ns(total: i64) -> Self {
        Self {
            seconds: total.div_euclid(1_000_000_000),
            nanos: total.rem_euclid(1_000_000_000) as u32,
        }
    }

    fn plus_ns(self, ns: i64) -> Self {
        Self::from_total_ns(self.total_ns().saturating_add(ns))
    }

    fn align(self, unit: &str, method: &str) -> Self {
        let size_ns: i64 = match unit {
            "day" => 86_400 * 1_000_000_000,
            "hour" => 3_600 * 1_000_000_000,
            "minute" => 60 * 1_000_000_000,
            "second" => 1_000_000_000,
            "millisecond" => 1_000_000,
            "microsecond" => 1_000,
            _ => return self,
        };
        let total = self.total_ns();
        let floored = total.div_euclid(size_ns) * size_ns;
        let aligned = match method {
            "round" => (total + size_ns / 2).div_euclid(size_ns) * size_ns,
            "ceil" if total != floored => floored.saturating_add(size_ns),
            _ => floored, // truncate / floor
        };
        Self::from_total_ns(aligned)
    }

    fn value(self) -> CtValue {
        datetime_value(self.seconds, self.nanos)
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
                nanos: 0,
            },
            zone,
        }
    }

    fn offset_seconds(&self) -> i64 {
        self.zone.offset
    }

    fn is_dst(&self) -> bool {
        // Comptime Zone is fixed-offset UTC-only; DST is always false there.
        false
    }

    fn local_instant(&self) -> DateTime {
        DateTime {
            seconds: self.instant.seconds.saturating_add(self.zone.offset),
            nanos: self.instant.nanos,
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
        nanos: int_field(value, "DateTime", "nanos", span).unwrap_or(0) as u32,
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
        Ok(date) => CtValue::Present(Box::new(date.value())),
        Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
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
    Ok(datetime_value(int_arg(args, 0, span)?, 0))
}

fn datetime_from_unix_ms(args: &[CtValue], span: Span) -> EvalResult {
    let ms = int_arg(args, 0, span)?;
    Ok(datetime_value(
        ms.div_euclid(1_000),
        (ms.rem_euclid(1_000) as u32).saturating_mul(1_000_000),
    ))
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
        Ok(seconds) => CtValue::Present(Box::new(datetime_value(seconds, 0))),
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
    let ns = match args.first() {
        Some(CtValue::Int(n)) => n.checked_mul(scale),
        Some(CtValue::Float(n)) => {
            let scaled = n.as_f64() * scale as f64;
            (scaled.is_finite()
                && scaled >= i64::MIN as f64
                && scaled < 9_223_372_036_854_775_808.0)
                .then_some(scaled.trunc() as i64)
        }
        _ => None,
    };
    Ok(match ns {
        Some(ns) => CtValue::Present(Box::new(duration_value(ns))),
        None => CtValue::failed(Box::new(structure(
            crate::Syntax::DURATION_RANGE_ERROR_TYPE,
            vec![(
                "reason",
                CtValue::Str(
                    "duration must be finite and inside the supported range".to_string(),
                ),
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
            return Ok(CtValue::failed(Box::new(xml_shape_error(
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
            CtValue::failed(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            )))
        );
    }
}
