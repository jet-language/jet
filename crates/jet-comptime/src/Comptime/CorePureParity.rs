//! Remaining deterministic Core calls for #392.
//!
//! Algorithms and value layouts mirror the AOT prelude. This module owns one
//! evaluator used by comptime and the REPL; callers never synthesize schemas or
//! fall back after a recognized call fails.


use super::mime_kernel;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtFloat, CtReport, CtValue, Type};

use crate::Comptime::Builtins::{as_bool, as_int, exact_big};
use crate::Comptime::Diagnostics::unsupported;
use crate::Comptime::EmailAdapter;
use crate::Comptime::Methods::as_float;
use crate::Comptime::{ServicesLite, SyncLite};
use jet_foundation::Prelude::{jet_as_bytes as as_bytes, tui as tui_kernel};
use jet_foundation::StructuralDebug::jet_debug_map;
use jet_foundation::Syntax::CoreCallPureRoute;

type EvalResult = Result<CtValue, Diagnostic>;

fn text_error(message: String) -> CtValue {
    structure("TextError", vec![("message", CtValue::Str(message))])
}

fn range_error(reason: String) -> CtValue {
    structure("RangeError", vec![("reason", CtValue::Str(reason))])
}
fn ordering_value(ordering: std::cmp::Ordering) -> CtValue {
    CtValue::Enum {
        type_name: crate::Syntax::TYPE_ORDERING.to_string(),
        variant: match ordering {
            std::cmp::Ordering::Less => "Less",
            std::cmp::Ordering::Equal => "Equal",
            std::cmp::Ordering::Greater => "Greater",
        }
        .to_string(),
        args: Vec::new(),
    }
}

pub(super) fn evaluate(
    row: &jet_foundation::Syntax::CoreCallRecord,
    args: &[CtValue],
    span: Span,
) -> Option<EvalResult> {
    let result = match (row.pure_route, row.member) {
        (CoreCallPureRoute::Mime, "parse") => mime_parse(args, span),
        (CoreCallPureRoute::Mime, "from_extension") => mime_from_extension(args, span),
        (CoreCallPureRoute::Mime, "extension") => mime_extension(args, span),
        (CoreCallPureRoute::Email, "address") => {
            return EmailAdapter::evaluate("address", args, span)
        }
        (CoreCallPureRoute::Email, "attachment") => {
            return EmailAdapter::evaluate("attachment", args, span)
        }
        (CoreCallPureRoute::Email, "message") => {
            return EmailAdapter::evaluate("message", args, span)
        }
        (CoreCallPureRoute::Email, "envelope") => {
            return EmailAdapter::evaluate("envelope", args, span)
        }
        (CoreCallPureRoute::Email, "serialize") => {
            return EmailAdapter::evaluate("serialize", args, span)
        }
        (CoreCallPureRoute::EncodingXml, "canonical") => xml_canonical(args, span),
        (CoreCallPureRoute::Time, "period") => period(args, span),
        (CoreCallPureRoute::Time, "period_days") => period_unit(args, span, 2),
        (CoreCallPureRoute::Time, "period_months") => period_unit(args, span, 1),
        (CoreCallPureRoute::Time, "period_years") => period_unit(args, span, 0),
        (CoreCallPureRoute::Time, "from_unix_ms") => datetime_from_unix_ms(args, span),
        (CoreCallPureRoute::Time, "from_unix_seconds") => datetime_from_unix_seconds(args, span),
        (CoreCallPureRoute::Time, "from_unix_microseconds") => {
            datetime_from_unix_microseconds(args, span)
        }
        (CoreCallPureRoute::Time, "from_unix_nanoseconds") => {
            datetime_from_unix_nanoseconds(args, span)
        }
        (CoreCallPureRoute::Time, "parse_rfc3339") => datetime_parse(args, span),
        (CoreCallPureRoute::Time, "parse_iso_week_date") => date_iso_week_parse(args, span),
        (CoreCallPureRoute::Time, "from_iso_week") => date_from_iso_week(args, span),
        (CoreCallPureRoute::Time, "parse_zoned") => zoned_parse(args, span),
        (CoreCallPureRoute::Time, "parse_time") => local_time_parse(args, span),
        // Pure zone constructors: UTC is deterministic. Named IANA zones need a
        // host TZif database (filesystem), so comptime keeps UTC aliases only and
        // returns `Err` for everything else — same shape as AOT without tzdb.
        (CoreCallPureRoute::Time, "utc") => Ok(zone_utc()),
        (CoreCallPureRoute::Time, "zone") => zone_named(args, span),
        (CoreCallPureRoute::Time, "zoned") => zoned_from_datetime(args, span),
        (CoreCallPureRoute::Time, "zoned_local") => zoned_from_local(args, span),
        (CoreCallPureRoute::Time, "instant") => Ok(structure(
            "Instant",
            vec![(
                "start_ns",
                CtValue::Int(super::time_kernel::jet_time_monotonic_now_ns()),
            )],
        )),
        (CoreCallPureRoute::Time, "datetime") => datetime_parts(args, span),
        (CoreCallPureRoute::Time, "time" | "local_time") => local_time_parts(args, span),
        (CoreCallPureRoute::Time, "days_in_month") => time_days_in_month(args, span),
        (CoreCallPureRoute::Time, "is_leap_year") => time_is_leap_year(args, span),
        (CoreCallPureRoute::Math, "pi") => Ok(CtValue::Float(CtFloat::f64(
            super::math_lib_pure::jet_std_math_pi(),
        ))),
        (CoreCallPureRoute::Math, "decimal") => decimal_from_str(args, span),
        (CoreCallPureRoute::Math, "fraction") => fraction_new(args, span),
        (CoreCallPureRoute::Math, "to_bits") => float_arg(args, 0, span)
            .map(|value| CtValue::Int(super::math_lib_pure::jet_std_math_to_bits(value))),
        (CoreCallPureRoute::Math, "from_bits") => one(args, 0, "core.math", "from_bits", span)
            .and_then(|value| {
                exact_big(value).ok_or_else(|| {
                    unsupported(concat!("core", ".math.from_bits expects an Int"), span)
                })
            })
            .map(|big| {
                CtValue::Float(CtFloat::f64(super::math_lib_pure::jet_std_math_from_bits(
                    big.wrapping_u64() as i64,
                )))
            }),
        // D-TYPE2-UNCERT1=A: `core.math.sqrt` keeps the measured grade. The
        // existing Prelude-backed method adapter owns the uncertainty rule;
        // ordinary Float sqrt remains on the core-call path below.
        (CoreCallPureRoute::Math, "sqrt") => match args {
            [value @ CtValue::Struct { type_name, .. }]
                if type_name == crate::Syntax::TYPE_MEASUREMENT =>
            {
                measurement_sqrt(value, span)
            }
            _ => return None,
        },
        (CoreCallPureRoute::Measurement, "from") => measurement(args, span),
        (CoreCallPureRoute::Date, "new") => date_new_call(args, span),
        (CoreCallPureRoute::Date, "parse") => date_parse_call(args, span),
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
        (CoreCallPureRoute::Ui, "ascii") => tui_ascii(args, span),
        (CoreCallPureRoute::Ui, "capabilities") => Ok(tui_capabilities()),
        (CoreCallPureRoute::Ui, "color_ansi16") => tui_color(args, span, "Ansi16", 1),
        (CoreCallPureRoute::Ui, "color_ansi256") => tui_color(args, span, "Ansi256", 1),
        (CoreCallPureRoute::Ui, "color_rgb") => tui_color(args, span, "Rgb", 3),
        (CoreCallPureRoute::Ui, "display_width") => tui_display_width(args, span),
        (CoreCallPureRoute::Ui, "focus_event") => bool_value(args, 0, span)
            .and_then(|value| tui_event("Focus", vec![(Some("focused".to_string()), value)])),
        (CoreCallPureRoute::Ui, "close_event") => tui_event("Close", Vec::new()),
        (CoreCallPureRoute::Ui, "horizontal") => tui_enum("TuiDirection", "Horizontal"),
        (CoreCallPureRoute::Ui, "interrupt_event") => tui_event("Interrupt", Vec::new()),
        (CoreCallPureRoute::Ui, "io_event") => tui_io_event(args, span),
        (CoreCallPureRoute::Ui, "key_event_modifiers") => tui_key_event_modifiers(args, span),
        (CoreCallPureRoute::Ui, "layout") => tui_layout(args, span),
        (CoreCallPureRoute::Ui, "length") => tui_constraint(args, span, "Length"),
        (CoreCallPureRoute::Ui, "fill") => tui_constraint(args, span, "Fill"),
        (CoreCallPureRoute::Ui, "list") => tui_list(args, span),
        (CoreCallPureRoute::Ui, "list_state") => Ok(tui_list_state(0, 0)),
        (CoreCallPureRoute::Ui, "list_state_offset") => {
            tui_list_state_update(args, span, false)
        }
        (CoreCallPureRoute::Ui, "list_state_select") => tui_list_state_update(args, span, true),
        (CoreCallPureRoute::Ui, "list_state_selected") =>
            one(args, 0, "core.tui", "list_state_selected", span)
                .and_then(|state| int_field(state, "TuiListState", "selected", span))
                .map(CtValue::Int),
        (CoreCallPureRoute::Ui, "max") => tui_constraint(args, span, "Max"),
        (CoreCallPureRoute::Ui, "min") => tui_constraint(args, span, "Min"),
        (CoreCallPureRoute::Ui, "percent") => tui_constraint(args, span, "Percent"),
        (CoreCallPureRoute::Ui, "style") => Ok(tui_style_default()),
        (CoreCallPureRoute::Ui, "style_background") => tui_style_color(args, span, true),
        (CoreCallPureRoute::Ui, "style_bold") => tui_style_flag(args, span, "bold"),
        (CoreCallPureRoute::Ui, "style_dim") => tui_style_flag(args, span, "dim"),
        (CoreCallPureRoute::Ui, "style_foreground") => tui_style_color(args, span, false),
        (CoreCallPureRoute::Ui, "style_text") => tui_style_text(args, span),
        (CoreCallPureRoute::Ui, "style_underline") => tui_style_flag(args, span, "underline"),
        (CoreCallPureRoute::Ui, "table") => tui_table(args, span),
        (CoreCallPureRoute::Ui, "timer_event") => tui_timer_event(args, span),
        (CoreCallPureRoute::Ui, "vertical") => tui_enum("TuiDirection", "Vertical"),
        (CoreCallPureRoute::Io, "style_force") => io_style_force(args, span),
        (CoreCallPureRoute::Net, "ip_addr") => net_ip_addr(args, span),
        (CoreCallPureRoute::Net, "ip_to_string") => net_string_field(args, "IPAddr", "text", span),
        (CoreCallPureRoute::Net, "ip_is_ipv4") => net_ip_is_ipv4(args, span),
        (CoreCallPureRoute::Net, "socket_addr_parse") => net_socket_addr_parse(args, span),
        (CoreCallPureRoute::Net, "socket_host") => {
            net_string_field(args, "SocketAddr", "host", span)
        }
        (CoreCallPureRoute::Net, "socket_port") => {
            net_value_field(args, "SocketAddr", "port", span)
        }
        (CoreCallPureRoute::Net, "socket_to_string") => {
            net_string_field(args, "SocketAddr", "text", span)
        }
        (CoreCallPureRoute::Net, "ready_readable") => {
            net_value_field(args, "NetReady", "readable", span)
        }
        (CoreCallPureRoute::Net, "ready_writable") => {
            net_value_field(args, "NetReady", "writable", span)
        }
        (CoreCallPureRoute::Net, "error_operation") => {
            net_string_field(args, "NetError", "operation", span)
        }
        (CoreCallPureRoute::Net, "error_address") => {
            net_value_field(args, "NetError", "address", span)
        }
        (CoreCallPureRoute::Net, "error_name") => net_value_field(args, "NetError", "name", span),
        (CoreCallPureRoute::Net, "error_message") => {
            net_string_field(args, "NetError", "message", span)
        }
        (CoreCallPureRoute::Net, "error_os_code") => {
            net_value_field(args, "NetError", "os_code", span)
        }
        (CoreCallPureRoute::Net, "dns_srv_target") => {
            net_string_field(args, "DNSSrv", "target", span)
        }
        (CoreCallPureRoute::Net, "dns_srv_port") => net_value_field(args, "DNSSrv", "port", span),
        (CoreCallPureRoute::Net, "dns_srv_priority") => {
            net_value_field(args, "DNSSrv", "priority", span)
        }
        (CoreCallPureRoute::Net, "dns_srv_weight") => {
            net_value_field(args, "DNSSrv", "weight", span)
        }
        (CoreCallPureRoute::Net, "udp_packet_data") => net_udp_packet_data(args, span),
        (CoreCallPureRoute::Net, "udp_packet_bytes") => {
            net_value_field(args, "UDPPacket", "data", span)
        }
        (CoreCallPureRoute::Net, "udp_packet_addr") => {
            net_value_field(args, "UDPPacket", "addr", span)
        }
        (CoreCallPureRoute::Net, "udp_packet_original_len") => {
            net_value_field(args, "UDPPacket", "original_len", span)
        }
        (CoreCallPureRoute::Net, "udp_packet_truncated") => {
            net_value_field(args, "UDPPacket", "truncated", span)
        }
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
        (CoreCallPureRoute::Crypto, "signing_key_bytes") => {
            crypto_extract(args, 0, "SigningKey", span)
        }
        (CoreCallPureRoute::Crypto, "x25519_secret_bytes") => {
            crypto_extract(args, 0, "X25519SecretKey", span)
        }
        (CoreCallPureRoute::Crypto, "shared_secret_bytes") => {
            crypto_extract(args, 0, "SharedSecret", span)
        }
        // TIR lowers Signature/VerifyKey/… `.bytes()` to core.crypto.__*_bytes;
        // keep those pure field extracts resident so REPL does not hit E1802.
        (CoreCallPureRoute::Crypto, "__signature_bytes") => {
            crypto_extract(args, 0, "Signature", span)
        }
        (CoreCallPureRoute::Crypto, "__verify_key_bytes") => {
            crypto_extract(args, 0, "VerifyKey", span)
        }
        (CoreCallPureRoute::Crypto, "__x25519_public_bytes") => {
            crypto_extract(args, 0, "X25519PublicKey", span)
        }
        (CoreCallPureRoute::Crypto, "__sealed_bytes") => crypto_extract(args, 0, "Sealed", span),
        (CoreCallPureRoute::Crypto, "__digest256_bytes") => {
            crypto_extract(args, 0, "Digest256", span)
        }
        (CoreCallPureRoute::Crypto, "__digest512_bytes") => {
            crypto_extract(args, 0, "Digest512", span)
        }
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
    let normalized_type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name.as_str());
    let row = jet_foundation::Syntax::core_receiver_method(type_name, method)
        .or_else(|| jet_foundation::Syntax::core_receiver_method(normalized_type_name, method))?;
    if !row
        .coverage
        .contains(jet_foundation::Syntax::CoreCallCoverage::COMPTIME)
    {
        return None;
    }
    if !row.accepts_arity(args.len()) {
        return None;
    }
    let result = match (normalized_type_name, method, args.len()) {
        (
            "Signature" | "Secret" | "SigningKey" | "VerifyKey" | "X25519SecretKey"
            | "X25519PublicKey" | "SharedSecret",
            "bytes",
            0,
        ) => value_field(recv, type_name, "bytes", span),
        ("Mime", "media_type", 0) => string_field(recv, "Mime", "top", span),
        ("Mime", "subtype", 0) => string_field(recv, "Mime", "sub", span),
        ("Mime", "essence", 0) => mime_essence(recv, span).map(CtValue::Str),
        ("Mime", "to_string", 0) => mime_string(recv, span).map(CtValue::Str),
        ("Mime", "param", 1) => mime_param(recv, args, span),
        ("Mime", "params", 0) => value_field(recv, "Mime", "params", span),
        ("Url", "scheme", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.scheme()))
        }
        ("Url", "username", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.username()))
        }
        ("Url", "password", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.password()))
        }
        ("Url", "userinfo", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.userinfo()))
        }
        ("Url", "authority", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.authority()))
        }
        ("Url", "path", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.path()))
        }
        ("Url", "query", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.query()))
        }
        ("Url", "host", 0) => super::url_parts_from_ct(recv, span).map(|url| match url.host() {
            Ok(host) => CtValue::Present(Box::new(CtValue::Str(host))),
            Err(_) => CtValue::absent(Type::String),
        }),
        ("Url", "port", 0) => super::url_parts_from_ct(recv, span).map(|url| match url.port() {
            Ok(port) => CtValue::Present(Box::new(CtValue::Int(port))),
            Err(_) => CtValue::absent(Type::Int),
        }),
        ("Url", "default_port", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| match url.default_port() {
                Ok(port) => CtValue::Present(Box::new(CtValue::Int(port))),
                Err(_) => CtValue::absent(Type::Int),
            })
        }
        ("Url", "fragment", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| match url.fragment() {
                Ok(fragment) => CtValue::Present(Box::new(CtValue::Str(fragment))),
                Err(_) => CtValue::absent(Type::String),
            })
        }
        ("Url", "path_segments", 0) => super::url_parts_from_ct(recv, span)
            .map(|url| CtValue::List(url.path_segments().into_iter().map(CtValue::Str).collect())),
        ("Url", "query_pairs", 0) => super::url_parts_from_ct(recv, span).map(|url| {
            CtValue::List(
                url.query_pairs()
                    .into_iter()
                    .map(|pair| CtValue::List(pair.into_iter().map(CtValue::Str).collect()))
                    .collect(),
            )
        }),
        ("Url", "normalize", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| super::url_parts_to_ct(&url.normalize()))
        }
        ("Url", "join", 1) => super::url_parts_from_ct(recv, span).and_then(|url| {
            let relative = string_arg(args, 0, span)?.to_string();
            Ok(match url.join(&relative) {
                Ok(url) => CtValue::Present(Box::new(super::url_parts_to_ct(&url))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }),
        ("Url", "set_query" | "add_query", 2) => {
            super::url_parts_from_ct(recv, span).and_then(|url| {
                let key = string_arg(args, 0, span)?.to_string();
                let value = string_arg(args, 1, span)?.to_string();
                let updated = if method == "set_query" {
                    url.set_query(&key, &value)
                } else {
                    url.add_query(&key, &value)
                };
                Ok(super::url_parts_to_ct(&updated))
            })
        }
        ("Url", "to_string", 0) => {
            super::url_parts_from_ct(recv, span).map(|url| CtValue::Str(url.to_string_value()))
        }
        ("Date" | "LocalDate", "year" | "month" | "day", 0) => {
            value_field(recv, type_name, method, span)
        }
        ("Date" | "LocalDate", "to_string", 0) => {
            date_from_value(recv, type_name, span).map(|date| CtValue::Str(date.to_string_fmt()))
        }
        ("Date" | "LocalDate", "equal", 1) => {
            date_from_value(recv, type_name, span).and_then(|left| {
                let right = date_from_value(&args[0], "LocalDate", span)?;
                Ok(CtValue::Bool(left.inner == right.inner))
            })
        }
        ("Date" | "LocalDate", "compare", 1) => {
            date_from_value(recv, type_name, span).and_then(|left| {
                let right = date_from_value(&args[0], "LocalDate", span)?;
                Ok(ordering_value(left.inner.cmp(&right.inner)))
            })
        }
        ("Date" | "LocalDate", "weekday", 0) => {
            date_from_value(recv, type_name, span).map(|date| CtValue::Int(date.inner.weekday()))
        }
        ("Date" | "LocalDate", "iso_weekday", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.iso_weekday())),
        ("Date" | "LocalDate", "day_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.day_of_year())),
        ("Date" | "LocalDate", "iso_week", 0) => {
            date_from_value(recv, type_name, span).map(|date| CtValue::Int(date.inner.iso_week()))
        }
        ("Date" | "LocalDate", "iso_week_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.iso_week_year())),
        ("Date" | "LocalDate", "quarter_of_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.quarter_of_year())),
        ("Date" | "LocalDate", "days_in_month", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Int(date.inner.days_in_month())),
        ("Date" | "LocalDate", "is_leap_year", 0) => date_from_value(recv, type_name, span)
            .map(|date| CtValue::Bool(date.inner.is_leap_year())),
        ("Date" | "LocalDate", "replace", 3) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                Ok(Date::from_inner(date.inner.replace(
                    as_int(&args[0], span)?,
                    as_int(&args[1], span)?,
                    as_int(&args[2], span)?,
                ))
                .value())
            })
        }
        ("Date" | "LocalDate", "add_days", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_days(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "add_months", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date.add_months(as_int(&args[0], span)?).value())),
        ("Date" | "LocalDate", "diff_days", 1) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                let other = date_from_value(&args[0], "LocalDate", span)?;
                Ok(CtValue::Int(date.inner.diff_days(&other.inner)))
            })
        }
        ("Date" | "LocalDate", "add_period" | "subtract_period", 1) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                let period = period_from_value(&args[0], span)?;
                let inner = if method == "add_period" {
                    date.inner.add_period(&period)
                } else {
                    date.inner.subtract_period(&period)
                };
                Ok(Date::from_inner(inner).value())
            })
        }
        ("Date" | "LocalDate", "truncate", 1) => date_from_value(recv, type_name, span)
            .and_then(|date| Ok(date_truncate(date, string_arg(args, 0, span)?).value())),
        ("Date" | "LocalDate", "format", 1) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                Ok(CtValue::Str(format_time_pattern(
                    string_arg(args, 0, span)?,
                    date,
                    LocalTime::new(0, 0, 0),
                )))
            })
        }
        ("Date" | "LocalDate", "format_checked", 1) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                Ok(match date.inner.format_checked(&string_arg(args, 0, span)?.to_string()) {
                    Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
                    Err(error) => CtValue::failed(Box::new(text_error(error))),
                })
            })
        }
        ("Date" | "LocalDate", "until" | "since", 5) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                let other = date_from_value(&args[0], "LocalDate", span)?;
                let largest = string_arg(args, 1, span)?;
                let smallest = string_arg(args, 2, span)?;
                let mode = string_arg(args, 3, span)?;
                let increment = int_arg(args, 4, span)?;
                let ns = if method == "until" {
                    date.inner
                        .until_ns(&other.inner, largest, smallest, mode, increment)
                } else {
                    date.inner
                        .since_ns(&other.inner, largest, smallest, mode, increment)
                };
                Ok(duration_value(ns))
            })
        }
        ("Date" | "LocalDate", "with", 4) => {
            date_from_value(recv, type_name, span).and_then(|date| {
                let result = date.inner.with_overflow(
                    int_arg(args, 0, span)?,
                    int_arg(args, 1, span)?,
                    int_arg(args, 2, span)?,
                    string_arg(args, 3, span)?,
                );
                Ok(match result {
                    Ok(value) => CtValue::Present(Box::new(Date::from_inner(value).value())),
                    Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
                })
            })
        }
        ("LocalTime", "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond", 0) => {
            value_field(recv, "LocalTime", method, span)
        }
        ("LocalTime", "add_duration" | "subtract_duration", 1) => {
            local_time_from_value(recv, span).and_then(|time| {
                let ns = duration_ns(&args[0], span)?;
                let inner = if method == "add_duration" {
                    time.inner.add_duration_ns(ns)
                } else {
                    time.inner.subtract_duration_ns(ns)
                };
                Ok(LocalTime::from_inner(inner).value())
            })
        }
        ("LocalTime", "round", 3) => local_time_from_value(recv, span).and_then(|time| {
            Ok(LocalTime::from_inner(time.inner.round_with(
                string_arg(args, 0, span)?,
                int_arg(args, 1, span)?,
                string_arg(args, 2, span)?,
            ))
            .value())
        }),
        ("LocalTime", "truncate" | "floor" | "ceil", 2) => {
            local_time_from_value(recv, span).and_then(|time| {
                let unit = string_arg(args, 0, span)?;
                let increment = int_arg(args, 1, span)?;
                let inner = match method {
                    "truncate" => time.inner.truncate_with(unit, increment),
                    "floor" => time.inner.floor_with(unit, increment),
                    "ceil" => time.inner.ceil_with(unit, increment),
                    _ => unreachable!("local-time alignment method guard"),
                };
                Ok(LocalTime::from_inner(inner).value())
            })
        }
        ("LocalTime", "until" | "since", 5) => {
            local_time_from_value(recv, span).and_then(|time| {
                let other = local_time_from_value(&args[0], span)?;
                let largest = string_arg(args, 1, span)?;
                let smallest = string_arg(args, 2, span)?;
                let mode = string_arg(args, 3, span)?;
                let increment = int_arg(args, 4, span)?;
                let ns = if method == "until" {
                    time.inner
                        .until_ns(&other.inner, largest, smallest, mode, increment)
                } else {
                    time.inner
                        .since_ns(&other.inner, largest, smallest, mode, increment)
                };
                Ok(duration_value(ns))
            })
        }
        ("LocalTime", "format", 1) => local_time_from_value(recv, span).and_then(|time| {
            Ok(CtValue::Str(time.inner.format_pattern(&string_arg(args, 0, span)?.to_string())))
        }),
        ("LocalTime", "format_checked", 1) => {
            local_time_from_value(recv, span).and_then(|time| {
                Ok(match time.inner.format_checked(&string_arg(args, 0, span)?.to_string()) {
                    Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
                    Err(error) => CtValue::failed(Box::new(text_error(error))),
                })
            })
        }
        ("LocalTime", "to_string", 0) => {
            local_time_from_value(recv, span).map(|time| CtValue::Str(time.to_string_fmt()))
        }
        ("LocalTime", "equal", 1) => {
            local_time_from_value(recv, span).and_then(|left| {
                let right = local_time_from_value(&args[0], span)?;
                Ok(CtValue::Bool(left.inner == right.inner))
            })
        }
        ("LocalTime", "compare", 1) => {
            local_time_from_value(recv, span).and_then(|left| {
                let right = local_time_from_value(&args[0], span)?;
                Ok(ordering_value(left.inner.cmp(&right.inner)))
            })
        }
        ("DateTime", "to_timestamp", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.to_timestamp())),
        ("DateTime", "to_unix_ms", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.to_unix_ms())),
        ("DateTime", "to_unix_s", 0) => datetime_from_value(recv, span)
            .map(|date_time| CtValue::Int(date_time.inner.to_unix_seconds())),
        ("DateTime", "to_unix_us", 0) => datetime_from_value(recv, span).map(|date_time| {
            match date_time.inner.to_unix_microseconds() {
                Ok(value) => CtValue::Present(Box::new(CtValue::Int(value))),
                Err(error) => CtValue::failed(Box::new(range_error(error))),
            }
        }),
        ("DateTime", "to_unix_ns", 0) => datetime_from_value(recv, span).map(|date_time| {
            match date_time.inner.to_unix_nanoseconds() {
                Ok(value) => CtValue::Present(Box::new(CtValue::Int(value))),
                Err(error) => CtValue::failed(Box::new(range_error(error))),
            }
        }),
        ("DateTime", "to_string", 0) => datetime_string(recv, span).map(CtValue::Str),
        ("DateTime", "date", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.date().value())
        }
        ("DateTime", "time", 0) => {
            datetime_from_value(recv, span).map(|date_time| date_time.time().value())
        }
        ("DateTime", "hour", 0) => {
            datetime_from_value(recv, span).map(|date_time| CtValue::Int(date_time.inner.hour()))
        }
        ("DateTime", "minute", 0) => {
            datetime_from_value(recv, span).map(|date_time| CtValue::Int(date_time.inner.minute()))
        }
        ("DateTime", "second", 0) => {
            datetime_from_value(recv, span).map(|date_time| CtValue::Int(date_time.inner.second()))
        }
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
        ("DateTime", "plus_duration" | "subtract_duration", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            let ns = duration_ns(&args[0], span)?;
            Ok(if method == "plus_duration" {
                date_time.plus_ns(ns).value()
            } else {
                date_time.plus_ns(ns.saturating_neg()).value()
            })
        }),
        ("DateTime", "add_nanoseconds", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                Ok(date_time.plus_ns(as_int(&args[0], span)?).value())
            })
        }
        ("DateTime", "add_period" | "subtract_period", 1) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let period = period_from_value(&args[0], span)?;
                let inner = if method == "add_period" {
                    date_time.inner.add_period(&period)
                } else {
                    date_time.inner.subtract_period(&period)
                };
                Ok(DateTime::from_inner(inner).value())
            })
        }
        ("DateTime", "difference", 1) => datetime_from_value(recv, span).and_then(|left| {
            let right = datetime_from_value(&args[0], span)?;
            Ok(duration_value(left.inner.difference_ns(&right.inner)))
        }),
        ("DateTime", "truncate" | "floor" | "ceil", 2) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let unit = string_arg(args, 0, span)?;
                let increment = int_arg(args, 1, span)?;
                let inner = match method {
                    "truncate" => date_time.inner.truncate_with(unit, increment),
                    "floor" => date_time.inner.floor_with(unit, increment),
                    "ceil" => date_time.inner.ceil_with(unit, increment),
                    _ => unreachable!("datetime alignment method guard"),
                };
                Ok(DateTime::from_inner(inner).value())
            })
        }
        ("DateTime", "round", 3) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(DateTime::from_inner(date_time.inner.round_with(
                string_arg(args, 0, span)?,
                int_arg(args, 1, span)?,
                string_arg(args, 2, span)?,
            ))
            .value())
        }),
        ("DateTime", "until" | "since", 5) => {
            datetime_from_value(recv, span).and_then(|date_time| {
                let other = datetime_from_value(&args[0], span)?;
                let largest = string_arg(args, 1, span)?;
                let smallest = string_arg(args, 2, span)?;
                let mode = string_arg(args, 3, span)?;
                let increment = int_arg(args, 4, span)?;
                let ns = if method == "until" {
                    date_time.inner.until_ns(&other.inner, largest, smallest, mode, increment)
                } else {
                    date_time.inner.since_ns(&other.inner, largest, smallest, mode, increment)
                };
                Ok(duration_value(ns))
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
        ("DateTime", "with", 7) => datetime_from_value(recv, span).and_then(|date_time| {
            let result = date_time.inner.with_overflow(
                int_arg(args, 0, span)?,
                int_arg(args, 1, span)?,
                int_arg(args, 2, span)?,
                int_arg(args, 3, span)?,
                int_arg(args, 4, span)?,
                int_arg(args, 5, span)?,
                string_arg(args, 6, span)?,
            );
            Ok(match result {
                Ok(value) => CtValue::Present(Box::new(DateTime::from_inner(value).value())),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            })
        }),
        ("DateTime", "format_checked", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(match date_time.inner.format_checked(&string_arg(args, 0, span)?.to_string()) {
                Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
                Err(error) => CtValue::failed(Box::new(text_error(error))),
            })
        }),
        ("Instant", "equal", 1) => instant_start_ns(recv, span).and_then(|left| {
            let right = instant_start_ns(&args[0], span)?;
            Ok(CtValue::Bool(left == right))
        }),
        ("Instant", "compare", 1) => instant_start_ns(recv, span).and_then(|left| {
            let right = instant_start_ns(&args[0], span)?;
            Ok(ordering_value(left.cmp(&right)))
        }),
        ("DateTime", "equal", 1) => {
            datetime_from_value(recv, span).and_then(|left| {
                let right = datetime_from_value(&args[0], span)?;
                Ok(CtValue::Bool(left.inner == right.inner))
            })
        }
        ("DateTime", "compare", 1) => {
            datetime_from_value(recv, span).and_then(|left| {
                let right = datetime_from_value(&args[0], span)?;
                Ok(ordering_value(left.inner.cmp(&right.inner)))
            })
        }
        ("DateTime", "in_zone", 1) => datetime_from_value(recv, span).and_then(|date_time| {
            Ok(ZonedDateTime::from_datetime(date_time, zone_from_value(&args[0], span)?).value())
        }),
        ("Instant", "elapsed_millis", 0) => instant_elapsed_millis(recv, span),
        ("Instant", "elapsed", 0) => instant_elapsed(recv, span),
        ("Zone", "name", 0) => string_field(recv, "Zone", "name", span),
        ("Zone", "next_transition" | "previous_transition", 1) => {
            zone_from_value(recv, span).and_then(|zone| {
                let seconds = int_arg(args, 0, span)?;
                let transition = if method == "next_transition" {
                    zone.inner.next_transition(seconds)
                } else {
                    zone.inner.previous_transition(seconds)
                };
                Ok(option_int(transition))
            })
        }
        ("Zone", "start_of_day", 1) => zone_from_value(recv, span).and_then(|zone| {
            let date = date_from_value(&args[0], "LocalDate", span)?;
            Ok(ZonedDateTime::from_inner(zone.inner.start_of_day_zoned(&date.inner)).value())
        }),
        ("Zone", "hours_in_day", 1) => zone_from_value(recv, span).and_then(|zone| {
            let date = date_from_value(&args[0], "LocalDate", span)?;
            Ok(CtValue::Int(zone.inner.hours_in_day(&date.inner)))
        }),
        ("Fraction", "to_string", 0) => {
            fraction_from_value(recv, span).map(|f| CtValue::Str(f.to_string_rep()))
        }
        ("Fraction", "numerator", 0) => {
            fraction_from_value(recv, span)
                .map(|f| crate::Comptime::Builtins::exact_int_value(f.numerator))
        }
        ("Fraction", "denominator", 0) => {
            fraction_from_value(recv, span)
                .map(|f| crate::Comptime::Builtins::exact_int_value(f.denominator))
        }
        ("Fraction", "to_float", 0) => fraction_from_value(recv, span).map(|f| {
            CtValue::Float(crate::AST::CtFloat::F64(f.to_float()))
        }),
        ("Fraction", "is_zero", 0) => {
            fraction_from_value(recv, span).map(|f| CtValue::Bool(f.is_zero()))
        }
        ("Fraction", "equal", 1) => fraction_from_value(recv, span).and_then(|left| {
            let right = fraction_from_value(&args[0], span)?;
            Ok(CtValue::Bool(left == right))
        }),
        ("Fraction", "add" | "sub" | "mul" | "div", 1) => {
            fraction_from_value(recv, span).and_then(|left| {
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
                    None => Err(unsupported(
                        "a ratio that leaves the range, or divided by zero",
                        span,
                    )),
                }
            })
        }
        ("Decimal", "to_string", 0) => {
            decimal_from_value(recv, span).map(|decimal| CtValue::Str(decimal.to_string_rep()))
        }
        ("Decimal", "equal", 1) => decimal_from_value(recv, span).and_then(|left| {
            let right = decimal_from_value(&args[0], span)?;
            Ok(CtValue::Bool(left == right))
        }),
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
        ("ZonedDateTime", "equal", 1) => {
            zoned_from_value(recv, span).and_then(|left| {
                let right = zoned_from_value(&args[0], span)?;
                Ok(CtValue::Bool(left.inner == right.inner))
            })
        }
        ("ZonedDateTime", "compare", 1) => {
            zoned_from_value(recv, span).and_then(|left| {
                let right = zoned_from_value(&args[0], span)?;
                Ok(ordering_value(left.inner.cmp(&right.inner)))
            })
        }
        ("Decimal", "div", 1) => decimal_from_value(recv, span).and_then(|left| {
            let right = decimal_from_value(&args[0], span)?;
            left.div(&right)
                .map(|value| value.to_value())
                .ok_or_else(|| unsupported("a Decimal quotient that leaves the exact Fraction range, or divided by zero", span))
        }),
        ("Decimal", "round" | "floor" | "ceil", 0) => {
            decimal_from_value(recv, span).map(|decimal| {
                let value = match method {
                    "round" => decimal.round(),
                    "floor" => decimal.floor(),
                    "ceil" => decimal.ceil(),
                    _ => unreachable!("decimal rounding method guard"),
                };
                value.to_value()
            })
        }
        ("ZonedDateTime", "date", 0) => {
            zoned_from_value(recv, span).map(|zoned| zoned.date().value())
        }
        ("ZonedDateTime", "time", 0) => {
            zoned_from_value(recv, span).map(|zoned| zoned.time().value())
        }
        ("ZonedDateTime", "offset_seconds", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Int(zoned.offset_seconds()))
        }
        ("ZonedDateTime", "is_dst", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Bool(zoned.is_dst()))
        }
        ("ZonedDateTime", "to_datetime", 0) => {
            zoned_from_value(recv, span).map(|zoned| zoned.instant.value())
        }
        ("ZonedDateTime", "zone", 0) => {
            zoned_from_value(recv, span).map(|zoned| zoned.zone.value())
        }
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
        ("ZonedDateTime", "subtract_duration", 1) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                let ns = duration_ns(&args[0], span)?;
                Ok(ZonedDateTime::from_inner(zoned.inner.subtract_duration_ns(ns)).value())
            })
        }
        ("ZonedDateTime", "add_period", 1) => zoned_from_value(recv, span).and_then(|zoned| {
            // I9/I8: the civil-vs-absolute rule is the Prelude kernel's, not this
            // tier's. Re-deriving it here (date → add_period → from_local) was a
            // second copy of `JetZonedDateTime::add_period`'s body.
            let period = period_from_value(&args[0], span)?;
            Ok(ZonedDateTime::from_inner(zoned.inner.add_period(&period)).value())
        }),
        ("ZonedDateTime", "subtract_period", 1) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                let period = period_from_value(&args[0], span)?;
                Ok(ZonedDateTime::from_inner(zoned.inner.subtract_period(&period)).value())
            })
        }
        ("ZonedDateTime", "with_time", 2) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                let time = local_time_from_value(&args[0], span)?;
                let disambiguation = string_arg(args, 1, span)?;
                Ok(match zoned.inner.with_time(&time.inner, disambiguation) {
                    Ok(value) => CtValue::Present(Box::new(ZonedDateTime::from_inner(value).value())),
                    Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
                })
            })
        }
        ("ZonedDateTime", "with_zone", 1) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                let zone = zone_from_value(&args[0], span)?;
                Ok(ZonedDateTime::from_inner(zoned.inner.with_zone(&zone.inner)).value())
            })
        }
        ("ZonedDateTime", "until" | "since", 5) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                let other = zoned_from_value(&args[0], span)?;
                let largest = string_arg(args, 1, span)?;
                let smallest = string_arg(args, 2, span)?;
                let mode = string_arg(args, 3, span)?;
                let increment = int_arg(args, 4, span)?;
                let ns = if method == "until" {
                    zoned.inner.until_ns(&other.inner, largest, smallest, mode, increment)
                } else {
                    zoned.inner.since_ns(&other.inner, largest, smallest, mode, increment)
                };
                Ok(duration_value(ns))
            })
        }
        ("ZonedDateTime", "next_transition" | "previous_transition", 0) => {
            zoned_from_value(recv, span).map(|zoned| {
                option_int(if method == "next_transition" {
                    zoned.inner.next_transition()
                } else {
                    zoned.inner.previous_transition()
                })
            })
        }
        ("ZonedDateTime", "start_of_day", 0) => {
            zoned_from_value(recv, span).map(|zoned| {
                ZonedDateTime::from_inner(zoned.inner.start_of_day()).value()
            })
        }
        ("ZonedDateTime", "hours_in_day", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Int(zoned.inner.hours_in_day()))
        }
        ("ZonedDateTime", "format_rfc9557", 0) => {
            zoned_from_value(recv, span).map(|zoned| CtValue::Str(zoned.inner.format_rfc9557()))
        }
        ("ZonedDateTime", "format_checked", 1) => {
            zoned_from_value(recv, span).and_then(|zoned| {
                Ok(match zoned.inner.format_checked(&string_arg(args, 0, span)?.to_string()) {
                    Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
                    Err(error) => CtValue::failed(Box::new(text_error(error))),
                })
            })
        }
        ("Period", "years" | "months" | "days", 0) => {
            value_field(recv, "Period", method, span)
        }
        ("Period", "sign", 0) => period_from_value(recv, span)
            .map(|period| CtValue::Int(period.sign())),
        ("Period", "is_zero", 0) => period_from_value(recv, span)
            .map(|period| CtValue::Bool(period.is_zero())),
        ("Period", "abs" | "negated", 0) => period_from_value(recv, span).map(|period| {
            let value = if method == "abs" { period.abs() } else { period.negated() };
            period_value_from_inner(value)
        }),
        ("Period", "add" | "sub", 1) => period_from_value(recv, span).and_then(|period| {
            let other = period_from_value(&args[0], span)?;
            let value = if method == "add" {
                period.add(&other)
            } else {
                period.sub(&other)
            };
            Ok(period_value_from_inner(value))
        }),
        ("Period", "total_in", 2) => period_from_value(recv, span).and_then(|period| {
            let unit = string_arg(args, 0, span)?;
            let value = match args.get(1) {
                Some(CtValue::Struct { type_name, .. })
                    if type_name == "Date" || type_name == "LocalDate" => {
                    let anchor = date_from_value(&args[1], "LocalDate", span)?;
                    period.total_in_date(unit, &anchor.inner)
                }
                Some(CtValue::Struct { type_name, .. }) if type_name == "DateTime" => {
                    let anchor = datetime_from_value(&args[1], span)?;
                    period.total_in_datetime(unit, &anchor.inner)
                }
                _ => 0.0,
            };
            Ok(CtValue::Float(CtFloat::f64(value)))
        }),
        ("Period", "to_string", 0) => period_string(recv, span).map(CtValue::Str),
        ("Measurement", "value" | "uncertainty", 0) => {
            value_field(recv, "Measurement", method, span)
        }
        ("Measurement", "add" | "sub" | "mul" | "div", 1) => {
            measurement_arithmetic(recv, method, &args[0], span)
        }
        ("Measurement", "sqrt", 0) => measurement_sqrt(recv, span),
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
        return Err(unsupported(
            "Solver.new is not in the Core-call registry",
            span,
        ));
    }
    let seed = as_int(one(args, 0, "Solver", "new", span)?, span)?;
    Ok(solver_value(super::solver_kernel::jet_solver_new(seed)))
}

/// D-DBDRIVER1 / I9: the interpreter's `DBValue` carrier needs the exact
/// `JetShow` projection from `Prelude/CoreLib/JetStd/DBPluginWire.rs`.
/// `Blob` can arrive as a byte buffer from a driver or as a typed `[U8]` list
/// while evaluating a literal; both represent the same database value.
fn db_value_display(value: &CtValue) -> Option<String> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return None;
    };
    let type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name.as_str());
    if type_name != crate::Syntax::TYPE_DB_VALUE {
        return None;
    }
    match (variant.as_str(), args.as_slice()) {
        ("Null", []) => Some("null".to_string()),
        ("Int", [(_, CtValue::Int(value))]) => Some(value.to_string()),
        ("Float", [(_, CtValue::Float(value))]) => Some(value.as_f64().to_string()),
        ("Text", [(_, CtValue::Str(value))]) => Some(value.clone()),
        ("Bool", [(_, CtValue::Bool(value))]) => Some(value.to_string()),
        ("Blob", [(_, value)]) => db_blob_display(value),
        _ => None,
    }
}

fn db_blob_display(value: &CtValue) -> Option<String> {
    as_bytes(value, Span::new(0, 0))
        .ok()
        .map(|bytes| format!("{bytes:?}"))
}

pub(super) fn display(value: &CtValue) -> Option<String> {
    if let Some(text) = db_value_display(value) {
        return Some(text);
    }
    // DataTree values use the ordered JSON projection on every tier. The
    // generic enum renderer below is for nominal enums and would expose the
    // erased `Object(JSONObject { ... })` carrier instead.
    if matches!(
        value,
        CtValue::Enum { type_name, .. } if type_name == "DataTree"
    ) {
        return Some(crate::Comptime::render_datatree_for_tir(value));
    }
    if let Some(text) = io_error_display(value) {
        return Some(text);
    }
    if let CtValue::Struct { type_name, fields } = value {
        if let Some(text) = tuple_display(type_name, fields) {
            return Some(text);
        }
    }
    match value {
        CtValue::List(values) => {
            let values = values
                .iter()
                .map(nested_display)
                .collect::<Option<Vec<_>>>()?;
            return Some(format!("[{}]", values.join(", ")));
        }
        CtValue::Map(entries) => {
            let values = entries
                .iter()
                .map(|(key, value)| {
                    let key = key.to_value();
                    Some((nested_display(&key)?, nested_display(value)?))
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(jet_debug_map(values));
        }
        _ => {}
    }
    // D-SERVICE-RECEIPT2=A / I9: service lifecycle and sync carriers cross
    // the evaluator boundary as the same typed facts that AOT's Prelude
    // `JetShow` renders. Keep opaque handles and audit records on that
    // projection instead of exposing the CtValue carrier fields or error
    // variant names.
    if let Some(text) =
        ServicesLite::service_show_value(value).or_else(|| SyncLite::sync_show_value(value))
    {
        return Some(text);
    }
    let core_type = match value {
        CtValue::Struct { type_name, .. } => type_name
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(type_name.as_str()),
        _ => "",
    };
    let core_display = jet_foundation::Syntax::core_receiver_method(core_type, "__display");
    if core_type == "Mime" {
        return mime_string(value, Span::new(0, 0)).ok();
    }
    if core_type == "Path" {
        return path_string(value, Span::new(0, 0)).ok();
    }
    if core_type == crate::Syntax::TYPE_COMPLEX {
        return crate::Comptime::ComplexParity::to_string(value);
    }
    if core_type == crate::Syntax::TYPE_FRACTION {
        return crate::Numeric::CtFraction::from_value(value)
            .ok()
            .map(|fraction| fraction.to_string_rep());
    }
    if core_type == crate::Syntax::TYPE_DECIMAL {
        return crate::Numeric::CtDecimal::from_value(value)
            .ok()
            .map(|decimal| decimal.to_string_rep());
    }
    if core_type == "DateTime" {
        return datetime_string(value, Span::new(0, 0)).ok();
    }
    if core_type == "LocalDate" {
        return date_from_value(value, core_type, Span::new(0, 0))
            .ok()
            .map(|date| date.to_string_fmt());
    }
    // D-TYPE2-TIME1=A / I9: the canonical nanosecond carrier has exactly one
    // `JetShow` rendering. AOT's `impl JetShow for Duration` and the Cranelift
    // host both call `jet_duration_kernel_show`; the evaluator marshals the
    // carrier out of the struct and calls the same kernel instead of falling
    // through to a structural record render.
    if core_type == crate::Syntax::DURATION_TYPE {
        if let CtValue::Struct { fields, .. } = value {
            if let Some(CtValue::Int(ns)) = fields.iter().find_map(|(name, field)| {
                (name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(name.as_str())
                    == "ns")
                    .then_some(field)
            }) {
                return Some(super::duration_kernel::jet_duration_kernel_show(*ns));
            }
        }
    }
    // A workflow handle is an opaque run identity. Keep the evaluator's
    // rendering aligned with the Prelude handle's `JetShow` implementation;
    // its durable cursor and authority are not user-facing fields.
    if core_type == "ServiceWorkflow" {
        if let CtValue::Struct { fields, .. } = value {
            if let Some(CtValue::Int(run_id)) = fields.iter().find_map(|(name, field)| {
                (name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(name.as_str())
                    == "run_id")
                    .then_some(field)
            }) {
                return Some(run_id.to_string());
            }
        }
    }
    // D-ENCSTREAM-SURFACE1=A / I9: the shared encoding failure has one
    // rendering. AOT's `impl JetShow for EncodingError` and the Cranelift host
    // both call `jet_encoding_error_kernel_show`; the evaluator marshals the
    // seven carrier fields and calls the same kernel.
    if core_type == "EncodingError" {
        if let CtValue::Struct { fields, .. } = value {
            let get = |wanted: &str| -> Option<&CtValue> {
                fields.iter().find_map(|(name, held)| {
                    (name
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name.as_str())
                        == wanted)
                        .then_some(held)
                })
            };
            let variant = |wanted: &str| match get(wanted)? {
                CtValue::Enum { variant, .. } => Some(
                    variant
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(variant.as_str()),
                ),
                _ => None,
            };
            let text = |wanted: &str| match get(wanted)? {
                CtValue::Str(held) => Some(held.as_str()),
                _ => None,
            };
            let optional_int = |wanted: &str| match get(wanted)? {
                CtValue::Present(inner) => match inner.as_ref() {
                    CtValue::Int(held) => Some(Some(*held)),
                    _ => None,
                },
                CtValue::Failed(CtReport::Clean(_)) => Some(None),
                CtValue::Int(held) => Some(Some(*held)),
                _ => None,
            };
            let CtValue::Int(byte_offset) = get("byte_offset")? else {
                return None;
            };
            return Some(
                super::encoding_error_kernel::jet_encoding_error_kernel_show(
                    variant("format")?,
                    variant("kind")?,
                    *byte_offset,
                    optional_int("line")?,
                    optional_int("column")?,
                    text("path")?,
                    text("reason")?,
                ),
            );
        }
    }
    // D-VALIDATE-DECODE1=B / I9: the accumulated decode/validate failure has one
    // rendering. AOT's `impl JetShow for FieldError` and the Cranelift host both
    // call `jet_field_error_kernel_show`; the evaluator marshals the two carrier
    // fields and calls the same kernel. Without this arm a single `{err}` fell
    // through to the structural record dump while a whole `[FieldError]` list
    // rendered the projection — the same value printed two ways.
    if core_type == "FieldError" {
        if let CtValue::Struct { fields, .. } = value {
            let get = |wanted: &str| -> Option<&CtValue> {
                fields.iter().find_map(|(name, held)| {
                    (name
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name.as_str())
                        == wanted)
                        .then_some(held)
                })
            };
            let text = |wanted: &str| match get(wanted)? {
                CtValue::Str(held) => Some(held.as_str()),
                _ => None,
            };
            return Some(super::field_error_kernel::jet_field_error_kernel_show(
                text("path")?,
                text("reason")?,
            ));
        }
    }
    match value {
        // D-CRYPTO-VAULT1=A / I9: a vault `KeyRef` has exactly one rendering,
        // `impl Display for JetVaultKeyRef` in `Prelude/SecretsCrypto.rs`. The
        // ambient host calls that impl while it marshals the handle and carries
        // the text on the value, so the evaluator reads what the Prelude wrote
        // instead of re-deriving `repo:{name}@v{generation}` here.
        CtValue::Struct { type_name, fields }
            if core_display.is_some()
                && type_name == jet_foundation::Syntax::VAULT_KEY_REF_TYPE =>
        {
            fields
                .iter()
                .find(|(name, _)| name == jet_foundation::Syntax::VAULT_KEY_REF_SHOWN)
                .and_then(|(_, shown)| match shown {
                    CtValue::Str(shown) => Some(shown.clone()),
                    _ => None,
                })
        }
        CtValue::Struct { type_name, .. }
            if core_display.is_some() && type_name == "HyperLogLog" =>
        {
            let CtValue::Int(n) = hll_count(value, Span::new(0, 0)).ok()? else {
                return None;
            };
            Some(format!("HyperLogLog(count={n})"))
        }
        CtValue::Struct { type_name, .. } if core_display.is_some() && type_name == "TDigest" => {
            Some("TDigest".to_string())
        }
        CtValue::Struct { type_name, .. }
            if core_display.is_some() && type_name == "CountMinSketch" =>
        {
            Some("CountMinSketch".to_string())
        }
        CtValue::Struct { type_name, .. }
            if core_display.is_some() && type_name == "ReservoirSampler" =>
        {
            let CtValue::Int(count) = field(value, "ReservoirSampler", "count")? else {
                return None;
            };
            Some(format!("ReservoirSampler(n={count})"))
        }
        CtValue::Struct { type_name, .. }
            if core_display.is_some() && type_name == crate::Syntax::SOLVER_TYPE =>
        {
            let failures = int_field(
                value,
                crate::Syntax::SOLVER_TYPE,
                "failures",
                Span::new(0, 0),
            )
            .ok()?;
            let status = if failures == 0 { "ok" } else { "failed" };
            Some(format!("Solver(status: {status}, failures: {failures})"))
        }
        CtValue::Struct { type_name, fields }
            if core_display.is_some() && type_name == "ServiceUpgradeReceipt" =>
        {
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, value)| value)
            };
            let CtValue::Int(from) = field("from_generation")? else {
                return None;
            };
            let CtValue::Int(to) = field("to_generation")? else {
                return None;
            };
            let CtValue::Str(migration) = field("migration")? else {
                return None;
            };
            let CtValue::Bool(rollback_available) = field("rollback_available")? else {
                return None;
            };
            let CtValue::List(pinned) = field("pinned_shards")? else {
                return None;
            };
            let pinned = pinned
                .iter()
                .map(|value| match value {
                    CtValue::Str(value) => Some(value.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some(format!(
                "ServiceUpgradeReceipt(from={from}, to={to}, migration={migration}, rollback_available={rollback_available}, pinned={})",
                pinned.join(",")
            ))
        }
        // Match AOT/JIT `DataError::display_text` / JetShow — not Rust Debug
        // of the mangled `__jet_DataError { __jet_kind: … }` shape (#1250).
        CtValue::Struct { type_name, fields }
            if core_display.is_some()
                && type_name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(type_name.as_str())
                    == "DataError" =>
        {
            let get = |name: &str| -> Option<&CtValue> {
                fields.iter().find_map(|(n, v)| {
                    let n = n
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(n.as_str());
                    (n == name).then_some(v)
                })
            };
            let kind = match get("kind")? {
                CtValue::Enum { variant, .. } => variant
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(variant)
                    .to_string(),
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
        CtValue::Struct { type_name, .. }
            if type_name
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(type_name.as_str())
                == "Url" =>
        {
            super::url_parts_from_ct(value, Span::new(0, 0))
                .ok()
                .map(|url| url.to_string_value())
        }
        CtValue::Struct { type_name, fields }
            if matches!(
                type_name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(type_name.as_str()),
                "Mime"
                    | "Period"
                    | "LocalDate"
                    | "LocalTime"
                    | "DateTime"
                    | "Date"
                    | "Zone"
                    | "ZonedDateTime"
                    | "Instant"
                    | "Envelope"
                    | "Address"
                    | "Message"
                    | "Attachment"
            ) =>
        {
            let ty = type_name
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(type_name);
            let parts: Vec<String> = fields
                .iter()
                .filter(|(name, _)| !name.starts_with(super::URL_INTERNAL_PREFIX))
                .map(|(name, v)| {
                    let field = name
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name);
                    Some(format!("{field}: {}", nested_display(v)?))
                })
                .collect::<Option<Vec<_>>>()?;
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
            if let (Some(CtValue::Float(measured)), Some(CtValue::Float(uncertainty))) = (
                field(value, "Measurement", "value"),
                field(value, "Measurement", "uncertainty"),
            ) {
                Some(super::measurement_kernel::jet_measurement_kernel_show((
                    measured.as_f64(),
                    uncertainty.as_f64(),
                )))
            } else if matches!(value, CtValue::Struct { .. } | CtValue::Enum { .. })
                && core_display.is_none()
            {
                // A non-Core nominal needs the evaluator's declaration metadata
                // for source names, union erasure, and typed nested Debug.
                None
            } else {
                canonical_structural_display(value)
            }
        }
    }
}

fn tuple_display(type_name: &str, fields: &[(String, CtValue)]) -> Option<String> {
    let type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name);
    if type_name != "tuple" && !type_name.starts_with("JetTup_") {
        return None;
    }
    if fields.is_empty() {
        return Some("()".to_string());
    }
    let label = format!(
        "({})",
        fields
            .iter()
            .map(|(name, _)| {
                name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(name)
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    let rendered = fields
        .iter()
        .map(|(name, value)| {
            Some(format!(
                "{}: {}",
                name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(name),
                nested_display(value)?
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{label} {{ {} }}", rendered.join(", ")))
}

fn nested_display(value: &CtValue) -> Option<String> {
    display(value)
}

fn canonical_structural_display(value: &CtValue) -> Option<String> {
    match value {
        CtValue::Struct { type_name, fields } => {
            let type_name = type_name
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(type_name.as_str());
            let fields = fields
                .iter()
                .filter(|(name, _)| !name.starts_with(super::URL_INTERNAL_PREFIX))
                .map(|(name, value)| {
                    let name = name
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name.as_str());
                    Some((name.to_string(), nested_display(value)?))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_record(
                type_name, fields,
            ))
        }
        CtValue::Enum { variant, args, .. } => {
            let variant = variant
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(variant.as_str());
            if args.is_empty() {
                Some(variant.to_string())
            } else {
                let args = args
                    .iter()
                    .map(|(label, value)| {
                        let shown = nested_display(value)?;
                        Some(
                            label
                                .as_ref()
                                .map_or_else(|| shown.clone(), |label| format!("{label}: {shown}")),
                        )
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(format!("{variant}({})", args.join(", ")))
            }
        }
        CtValue::Int(_)
        | CtValue::Float(_)
        | CtValue::Bool(_)
        | CtValue::Char(_)
        | CtValue::Str(_)
        | CtValue::BigInt(_)
        | CtValue::Bytes(_)
        | CtValue::Present(_)
        | CtValue::Failed(_) => Some(value.jet_show()),
        CtValue::List(values) => Some(format!(
            "[{}]",
            values
                .iter()
                .map(nested_display)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        CtValue::Map(entries) => {
            let values = entries
                .iter()
                .map(|(key, value)| {
                    let key = key.to_value();
                    Some((nested_display(&key)?, nested_display(value)?))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_debug_map(values))
        }
        CtValue::Unit => Some(String::new()),
        CtValue::Closure(_) => None,
    }
}

/// The pure evaluator's checked Debug adapter. Keep it separate from
/// CtValue::debug_rust: that method mirrors the erased Rust carrier, while
/// this path mirrors the source-shaped JetDebug implementations.
pub(super) fn debug(value: &CtValue) -> Option<String> {
    if let CtValue::Struct { type_name, fields } = value {
        let type_name = type_name
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(type_name.as_str());
        let field = if type_name == "TextError" {
            "message"
        } else if type_name == "RangeError" {
            "reason"
        } else {
            ""
        };
        if !field.is_empty() {
            if let Some((_, CtValue::Str(text))) = fields.iter().find(|(name, _)| {
                name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(name.as_str())
                    == field
            }) {
                return Some(text.clone());
            }
        }
    }
    canonical_structural_debug(value)
}

fn canonical_structural_debug(value: &CtValue) -> Option<String> {
    match value {
        CtValue::Struct { type_name, fields } => {
            let type_name = type_name
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(type_name.as_str());
            let fields = fields
                .iter()
                .filter(|(name, _)| {
                    !name.starts_with(super::URL_INTERNAL_PREFIX)
                        && !crate::Syntax::is_memo_storage_name(name)
                })
                .enumerate()
                .map(|(storage_index, (name, value))| {
                    let name = name
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(name.as_str());
                    Some(jet_foundation::StructuralDebug::JetDebugField {
                        name: name.to_string(),
                        value: debug(value)?,
                        storage_index,
                        redacted: jet_foundation::StructuralDebug::jet_debug_field_metadata(
                            type_name,
                        )
                        .and_then(|metadata| {
                            metadata
                                .iter()
                                .find(|(field_name, _)| *field_name == name)
                        })
                        .is_some_and(|(_, redacted)| *redacted),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                type_name, fields,
            ))
        }
        CtValue::Enum { variant, args, .. } => {
            let variant = variant
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(variant.as_str());
            if args.is_empty() {
                Some(jet_foundation::StructuralDebug::jet_debug_variant(
                    variant, None,
                ))
            } else if args.iter().all(|(label, _)| label.is_some()) {
                let fields = args
                    .iter()
                    .enumerate()
                    .map(|(storage_index, (label, value))| {
                        let name = label.as_deref().unwrap_or("");
                        let name = name
                            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                            .unwrap_or(name);
                        Some(jet_foundation::StructuralDebug::JetDebugField {
                            name: name.to_string(),
                            value: debug(value)?,
                            storage_index,
                            redacted: jet_foundation::StructuralDebug::jet_debug_field_metadata(
                                variant,
                            )
                            .and_then(|metadata| {
                                metadata
                                    .iter()
                                    .find(|(field_name, _)| *field_name == name)
                            })
                            .is_some_and(|(_, redacted)| *redacted),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(jet_foundation::StructuralDebug::jet_debug_record_fields(
                    variant, fields,
                ))
            } else {
                let values = args
                    .iter()
                    .map(|(_, value)| debug(value))
                    .collect::<Option<Vec<_>>>()?;
                Some(jet_foundation::StructuralDebug::jet_debug_variant(
                    variant,
                    Some(values.join(", ")),
                ))
            }
        }
        CtValue::Int(value) => Some(value.to_string()),
        CtValue::Float(value) => Some(value.render()),
        CtValue::Bool(value) => Some(value.to_string()),
        CtValue::Char(value) => Some(format!("{value:?}")),
        CtValue::Str(value) => Some(format!("{value:?}")),
        CtValue::BigInt(value) => Some(value.to_string_rep()),
        CtValue::Bytes(value) => Some(format!("{value:?}")),
        CtValue::List(values) => Some(format!(
            "[{}]",
            values
                .iter()
                .map(debug)
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        CtValue::Map(entries) => {
            let values = entries
                .iter()
                .map(|(key, value)| {
                    let key = key.to_value();
                    Some((debug(&key)?, debug(value)?))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(jet_foundation::StructuralDebug::jet_debug_map(values))
        }
        CtValue::Present(value) => Some(jet_foundation::StructuralDebug::jet_debug_optional(
            Some(debug(value)?),
        )),
        CtValue::Failed(CtReport::Clean(_)) => {
            Some(jet_foundation::StructuralDebug::jet_debug_optional(None))
        }
        CtValue::Failed(CtReport::Told(value)) => Some(format!("Err({})", debug(value)?)),
        CtValue::Unit => Some("()".to_string()),
        CtValue::Closure(_) => None,
    }
}

/// D-NET-IOERROR1: the interpreter marshals the packed IOError shape through
/// the same Prelude renderer used by AOT and resident JIT tiers.
fn io_error_display(value: &CtValue) -> Option<String> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return None;
    };
    let type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name);
    if type_name != "IOError" {
        return None;
    }
    let variant_name = variant
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(variant.as_str());
    if variant_name == "ResourceLimit" {
        let Some((
            _,
            CtValue::Enum {
                type_name: limit_type,
                variant: limit_variant,
                ..
            },
        )) = args.first()
        else {
            return None;
        };
        let limit_type = limit_type
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(limit_type);
        if limit_type != "ProcessResourceLimit" {
            return None;
        }
        let limit_variant = limit_variant
            .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
            .unwrap_or(limit_variant.as_str());
        let limit = jet_foundation::Syntax::PROCESS_RESOURCE_LIMIT_VARIANTS
            .iter()
            .position(|name| *name == limit_variant)?;
        return Some(format!(
            "process resource limit exceeded: {}",
            jet_foundation::StructuralDebug::jet_show_process_resource_limit(limit as i64)
        ));
    }
    let variant = match variant_name {
        "InvalidInput" => 0,
        "NotFound" => 1,
        "PermissionDenied" => 2,
        "TimedOut" => 3,
        "Cancelled" => 4,
        "Closed" => 5,
        "Protocol" => 6,
        "Other" => 7,
        _ => return None,
    };
    let Some((_, CtValue::Struct { type_name, fields })) = args.first() else {
        return None;
    };
    let type_name = type_name
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(type_name);
    if type_name != "IOContext" {
        return None;
    }
    let field = |wanted: &str| {
        fields.iter().find_map(|(name, value)| {
            (name == wanted
                || name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX) == Some(wanted))
            .then_some(value)
        })
    };
    let CtValue::Enum {
        variant: operation_variant,
        ..
    } = field("operation")?
    else {
        return None;
    };
    let operation = match operation_variant
        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(operation_variant.as_str())
    {
        "Read" => 0,
        "Write" => 1,
        "Flush" => 2,
        "Connect" => 3,
        "Accept" => 4,
        "Close" => 5,
        "Resolve" => 6,
        "Codec" => 7,
        _ => return None,
    };
    let optional_text = |wanted: &str| {
        field(wanted).and_then(|value| match value {
            CtValue::Present(inner) => match inner.as_ref() {
                CtValue::Str(text) => Some(Some(text.as_str())),
                _ => None,
            },
            CtValue::Failed(CtReport::Clean(_)) => Some(None),
            CtValue::Str(text) => Some(Some(text.as_str())),
            _ => None,
        })
    };
    Some(jet_foundation::StructuralDebug::jet_show_io_error(
        variant,
        operation,
        optional_text("resource")?,
        optional_text("cause")?,
    ))
}

fn path_string(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("malformed Path value", span));
    };
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("inner", CtValue::Str(path)) => Some(path.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("malformed Path value", span))
}

fn one<'a>(
    args: &'a [CtValue],
    index: usize,
    module: &str,
    method: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    args.get(index)
        .ok_or_else(|| unsupported(&format!("{module}.{method}(): missing arg {index}"), span))
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
    Ok(structure(
        "Point",
        vec![
            ("x", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
            ("y", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
        ],
    ))
}

fn ui_size(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure(
        "Size",
        vec![
            (
                "width",
                CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?)),
            ),
            (
                "height",
                CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?)),
            ),
        ],
    ))
}

fn ui_rect(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure(
        "Rect",
        vec![
            ("x", CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?))),
            ("y", CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?))),
            (
                "width",
                CtValue::Float(CtFloat::f64(float_arg(args, 2, span)?)),
            ),
            (
                "height",
                CtValue::Float(CtFloat::f64(float_arg(args, 3, span)?)),
            ),
        ],
    ))
}

fn ui_constraint(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure(
        "SizeConstraint",
        vec![
            (
                "min_width",
                CtValue::Float(CtFloat::f64(float_arg(args, 0, span)?)),
            ),
            (
                "min_height",
                CtValue::Float(CtFloat::f64(float_arg(args, 1, span)?)),
            ),
            (
                "max_width",
                CtValue::Float(CtFloat::f64(float_arg(args, 2, span)?)),
            ),
            (
                "max_height",
                CtValue::Float(CtFloat::f64(float_arg(args, 3, span)?)),
            ),
        ],
    ))
}

fn ui_node_value_with_metadata(
    label: String,
    width: f64,
    height: f64,
    role: Option<CtValue>,
    color: Option<String>,
    kind: &str,
    children: Vec<CtValue>,
    accessibility: Option<CtValue>,
    shortcut: Option<CtValue>,
) -> CtValue {
    structure(
        "UiNode",
        vec![
            ("label", CtValue::Str(label)),
            ("width", CtValue::Float(CtFloat::f64(width))),
            ("height", CtValue::Float(CtFloat::f64(height))),
            (
                "role",
                role.map_or(
                    CtValue::absent(Type::Named("UiAriaRole".to_string())),
                    |role| CtValue::Present(Box::new(role)),
                ),
            ),
            (
                "accessibility",
                accessibility.map_or(
                    CtValue::absent(Type::Named("UiAccessibility".to_string())),
                    |metadata| CtValue::Present(Box::new(metadata)),
                ),
            ),
            (
                "ime",
                CtValue::absent(Type::Named("UiImeMode".to_string())),
            ),
            (
                "color",
                color.map_or(CtValue::absent(Type::String), |color| {
                    CtValue::Present(Box::new(CtValue::Str(color)))
                }),
            ),
            ("kind", ui_kind(kind)),
            ("children", CtValue::List(children)),
            (
                "shortcut",
                shortcut.map_or(
                    CtValue::absent(Type::Named("UiShortcut".to_string())),
                    |shortcut| CtValue::Present(Box::new(shortcut)),
                ),
            ),
        ],
    )
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
    ui_node_value_with_metadata(
        label,
        width,
        height,
        role,
        color,
        kind,
        children,
        None,
        None,
    )
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
    let role = args
        .get(3)
        .cloned()
        .ok_or_else(|| unsupported("core.ui.node_role(): missing role", span))?;
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
    let shortcut = match args.get(1) {
        Some(CtValue::Present(inner)) => Some(inner.as_ref().clone()),
        _ => None,
    };
    let accessible_label = match args.get(2) {
        Some(CtValue::Present(inner)) => match inner.as_ref() {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    };
    let accessibility = accessible_label.map(|name| {
        structure(
            "UiAccessibility",
            vec![
                (
                    "name",
                    CtValue::Present(Box::new(CtValue::Str(name))),
                ),
                (
                    "description",
                    CtValue::absent(Type::String),
                ),
                ("states", CtValue::List(Vec::new())),
            ],
        )
    });
    Ok(ui_node_value_with_metadata(
        label.clone(),
        label.chars().count() as f64 + 4.0,
        1.0,
        Some(ui_role("Button")),
        None,
        "Button",
        Vec::new(),
        accessibility,
        shortcut,
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
        width = width.max(as_float(
            field(child, "UiNode", "width")
                .ok_or_else(|| unsupported("core.ui.box() needs UiNode children", span))?,
            span,
        )?);
        height += as_float(
            field(child, "UiNode", "height")
                .ok_or_else(|| unsupported("core.ui.box() needs UiNode children", span))?,
            span,
        )?;
    }
    Ok(ui_node_value(
        String::new(),
        width,
        height,
        Some(ui_role("Container")),
        None,
        "Box",
        children,
    ))
}

fn ui_key_event(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: "InputEvent".to_string(),
        variant: "Key".to_string(),
        args: vec![(
            Some("code".to_string()),
            CtValue::Str(string_arg(args, 0, span)?.to_string()),
        )],
    })
}

fn ui_resize_event(args: &[CtValue], span: Span) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: "InputEvent".to_string(),
        variant: "Resize".to_string(),
        args: vec![(Some("size".to_string()), ui_size(args, span)?)],
    })
}

fn tui_event(variant: &str, args: Vec<(Option<String>, CtValue)>) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: "TuiEvent".to_string(),
        variant: variant.to_string(),
        args,
    })
}

fn tui_enum(type_name: &str, variant: &str) -> EvalResult {
    tui_event_for_type(type_name, variant, Vec::new())
}

fn tui_event_for_type(
    type_name: &str,
    variant: &str,
    args: Vec<(Option<String>, CtValue)>,
) -> EvalResult {
    Ok(CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args,
    })
}

fn bool_value(args: &[CtValue], index: usize, span: Span) -> Result<CtValue, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Bool(value)) => Ok(CtValue::Bool(*value)),
        _ => Err(unsupported("Core call expected a Bool argument", span)),
    }
}

fn tui_key_event_modifiers(args: &[CtValue], span: Span) -> EvalResult {
    tui_event(
        "Key",
        vec![
            (Some("code".to_string()), CtValue::Str(string_arg(args, 0, span)?.to_string())),
            (Some("modifiers".to_string()), CtValue::Int(int_arg(args, 1, span)?.clamp(0, 255))),
        ],
    )
}

fn tui_io_event(args: &[CtValue], span: Span) -> EvalResult {
    let payload = match one(args, 1, "core.tui", "io_event", span)? {
        CtValue::List(values) => CtValue::List(values.clone()),
        CtValue::Bytes(values) => CtValue::Bytes(values.clone()),
        _ => return Err(unsupported("core.tui.io_event() needs [Byte]", span)),
    };
    tui_event(
        "Io",
        vec![
            (Some("channel".to_string()), CtValue::Str(string_arg(args, 0, span)?.to_string())),
            (Some("payload".to_string()), payload),
        ],
    )
}

fn tui_timer_event(args: &[CtValue], span: Span) -> EvalResult {
    tui_event(
        "Timer",
        vec![
            (Some("id".to_string()), CtValue::Str(string_arg(args, 0, span)?.to_string())),
            (Some("elapsed_ms".to_string()), CtValue::Int(int_arg(args, 1, span)?)),
        ],
    )
}

fn tui_color(args: &[CtValue], span: Span, variant: &str, arity: usize) -> EvalResult {
    let values = (0..arity)
        .map(|index| int_arg(args, index, span))
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let color = match (variant, values.as_slice()) {
        ("Ansi16", [index]) => tui_kernel::color_ansi16(*index),
        ("Ansi256", [index]) => tui_kernel::color_ansi256(*index),
        ("Rgb", [red, green, blue]) => tui_kernel::color_rgb(*red, *green, *blue),
        _ => return Err(unsupported("malformed TuiColor value", span)),
    };
    Ok(tui_kernel_color_value(color))
}
fn tui_enum_parts<'a>(
    value: &'a CtValue,
    expected: &str,
    span: Span,
) -> Result<(&'a str, &'a [(Option<String>, CtValue)]), Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported(
            &format!("core.tui expected {expected}"),
            span,
        ));
    };
    if type_name != expected {
        return Err(unsupported(
            &format!("core.tui expected {expected}"),
            span,
        ));
    }
    Ok((variant, args))
}

fn tui_kernel_profile(value: &CtValue, span: Span) -> Result<tui_kernel::ColorProfile, Diagnostic> {
    let (variant, args) = tui_enum_parts(value, "TuiColorProfile", span)?;
    if !args.is_empty() {
        return Err(unsupported("core.tui color profile takes no arguments", span));
    }
    match variant {
        "Ansi16" => Ok(tui_kernel::ColorProfile::Ansi16),
        "Ansi256" => Ok(tui_kernel::ColorProfile::Ansi256),
        "TrueColor" => Ok(tui_kernel::ColorProfile::TrueColor),
        "Ascii" => Ok(tui_kernel::ColorProfile::Ascii),
        _ => Err(unsupported("unknown TuiColorProfile variant", span)),
    }
}

fn tui_kernel_color(value: &CtValue, span: Span) -> Result<tui_kernel::Color, Diagnostic> {
    let (variant, args) = tui_enum_parts(value, "TuiColor", span)?;
    let values = args
        .iter()
        .map(|(_, value)| as_int(value, span))
        .collect::<Result<Vec<_>, _>>()?;
    match (variant, values.as_slice()) {
        ("Ansi16", [index]) => Ok(tui_kernel::color_ansi16(*index)),
        ("Ansi256", [index]) => Ok(tui_kernel::color_ansi256(*index)),
        ("Rgb", [red, green, blue]) => Ok(tui_kernel::color_rgb(*red, *green, *blue)),
        _ => Err(unsupported("malformed TuiColor value", span)),
    }
}

fn tui_kernel_color_value(color: tui_kernel::Color) -> CtValue {
    let (variant, values) = match color {
        tui_kernel::Color::Ansi16(index) => ("Ansi16", vec![i64::from(index)]),
        tui_kernel::Color::Ansi256(index) => ("Ansi256", vec![i64::from(index)]),
        tui_kernel::Color::Rgb(red, green, blue) => (
            "Rgb",
            vec![i64::from(red), i64::from(green), i64::from(blue)],
        ),
    };
    CtValue::Enum {
        type_name: "TuiColor".to_string(),
        variant: variant.to_string(),
        args: values
            .into_iter()
            .map(|value| (None, CtValue::Int(value)))
            .collect(),
    }
}
fn tui_kernel_bool_field(value: &CtValue, name: &str, span: Span) -> Result<bool, Diagnostic> {
    match field(value, "TuiCapabilities", name).or_else(|| field(value, "TuiStyle", name)) {
        Some(CtValue::Bool(value)) => Ok(*value),
        _ => Err(unsupported(
            &format!("malformed core.tui boolean field `{name}`"),
            span,
        )),
    }
}

fn tui_kernel_int_field(value: &CtValue, type_name: &str, name: &str, span: Span) -> Result<i64, Diagnostic> {
    as_int(
        field(value, type_name, name)
            .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))?,
        span,
    )
}

fn tui_kernel_capabilities(value: &CtValue, span: Span) -> Result<tui_kernel::Capabilities, Diagnostic> {
    let profile = tui_kernel_profile(
        field(value, "TuiCapabilities", "profile")
            .ok_or_else(|| unsupported("malformed TuiCapabilities.profile value", span))?,
        span,
    )?;
    Ok(tui_kernel::Capabilities {
        profile,
        color: tui_kernel_bool_field(value, "color", span)?,
        unicode: tui_kernel_bool_field(value, "unicode", span)?,
        mouse: tui_kernel_bool_field(value, "mouse", span)?,
        resize: tui_kernel_bool_field(value, "resize", span)?,
        clipboard: tui_kernel_bool_field(value, "clipboard", span)?,
        width: tui_kernel_int_field(value, "TuiCapabilities", "width", span)?
            .max(1) as usize,
        height: tui_kernel_int_field(value, "TuiCapabilities", "height", span)?
            .max(1) as usize,
    })
}

fn tui_kernel_capabilities_value(capabilities: tui_kernel::Capabilities) -> CtValue {
    let profile = match capabilities.profile {
        tui_kernel::ColorProfile::Ansi16 => tui_profile("Ansi16"),
        tui_kernel::ColorProfile::Ansi256 => tui_profile("Ansi256"),
        tui_kernel::ColorProfile::TrueColor => tui_profile("TrueColor"),
        tui_kernel::ColorProfile::Ascii => tui_profile("Ascii"),
    };
    structure(
        "TuiCapabilities",
        vec![
            ("profile", profile),
            ("color", CtValue::Bool(capabilities.color)),
            ("unicode", CtValue::Bool(capabilities.unicode)),
            ("mouse", CtValue::Bool(capabilities.mouse)),
            ("resize", CtValue::Bool(capabilities.resize)),
            ("clipboard", CtValue::Bool(capabilities.clipboard)),
            ("width", CtValue::Int(capabilities.width as i64)),
            ("height", CtValue::Int(capabilities.height as i64)),
        ],
    )
}

fn tui_kernel_optional_color(
    value: &CtValue,
    name: &str,
    span: Span,
) -> Result<Option<tui_kernel::Color>, Diagnostic> {
    match field(value, "TuiStyle", name) {
        Some(CtValue::Present(inner)) => tui_kernel_color(inner, span).map(Some),
        Some(CtValue::Failed(CtReport::Clean(_))) => Ok(None),
        _ => Err(unsupported(
            &format!("malformed TuiStyle.{name} value"),
            span,
        )),
    }
}

fn tui_kernel_style(value: &CtValue, span: Span) -> Result<tui_kernel::Style, Diagnostic> {
    Ok(tui_kernel::Style {
        foreground: tui_kernel_optional_color(value, "foreground", span)?,
        background: tui_kernel_optional_color(value, "background", span)?,
        bold: tui_kernel_bool_field(value, "bold", span)?,
        dim: tui_kernel_bool_field(value, "dim", span)?,
        underline: tui_kernel_bool_field(value, "underline", span)?,
    })
}

fn tui_kernel_constraint(
    value: &CtValue,
    span: Span,
) -> Result<tui_kernel::Constraint, Diagnostic> {
    let (variant, args) = tui_enum_parts(value, "TuiConstraint", span)?;
    let amount = args
        .first()
        .map(|(_, value)| as_float(value, span))
        .transpose()?
        .unwrap_or(1.0);
    match variant {
        "Length" => Ok(tui_kernel::length(amount)),
        "Min" => Ok(tui_kernel::min(amount)),
        "Max" => Ok(tui_kernel::max(amount)),
        "Percent" => Ok(tui_kernel::percent(amount)),
        "Fill" => Ok(tui_kernel::fill(amount)),
        _ => Err(unsupported("unknown TuiConstraint variant", span)),
    }
}

fn tui_kernel_rect(value: &CtValue, span: Span) -> Result<tui_kernel::Rect, Diagnostic> {
    Ok(tui_kernel::Rect {
        x: as_float(
            field(value, "Rect", "x")
                .ok_or_else(|| unsupported("core.tui.layout() needs Rect", span))?,
            span,
        )?,
        y: as_float(
            field(value, "Rect", "y")
                .ok_or_else(|| unsupported("core.tui.layout() needs Rect", span))?,
            span,
        )?,
        width: as_float(
            field(value, "Rect", "width")
                .ok_or_else(|| unsupported("core.tui.layout() needs Rect", span))?,
            span,
        )?,
        height: as_float(
            field(value, "Rect", "height")
                .ok_or_else(|| unsupported("core.tui.layout() needs Rect", span))?,
            span,
        )?,
    })
}

fn tui_profile(variant: &str) -> CtValue {
    CtValue::Enum {
        type_name: "TuiColorProfile".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn tui_capabilities() -> CtValue {
    tui_kernel_capabilities_value(tui_kernel::capabilities(
        tui_kernel::ColorProfile::Ascii,
        80,
        24,
    ))
}

fn tui_constraint(args: &[CtValue], span: Span, variant: &str) -> EvalResult {
    let amount = float_arg(args, 0, span)?;
    let constraint = match variant {
        "Length" => tui_kernel::length(amount),
        "Min" => tui_kernel::min(amount),
        "Max" => tui_kernel::max(amount),
        "Percent" => tui_kernel::percent(amount),
        "Fill" => tui_kernel::fill(amount),
        _ => return Err(unsupported("unknown TuiConstraint variant", span)),
    };
    let amount = match constraint {
        tui_kernel::Constraint::Length(value)
        | tui_kernel::Constraint::Min(value)
        | tui_kernel::Constraint::Max(value)
        | tui_kernel::Constraint::Percent(value) => value,
        tui_kernel::Constraint::Fill(weight) => f64::from(weight),
    };
    tui_event_for_type(
        "TuiConstraint",
        variant,
        vec![(None, CtValue::Float(CtFloat::f64(amount)))],
    )
}

fn tui_style_default() -> CtValue {
    structure(
        "TuiStyle",
        vec![
            (
                "foreground",
                CtValue::absent(Type::Named("TuiColor".to_string())),
            ),
            (
                "background",
                CtValue::absent(Type::Named("TuiColor".to_string())),
            ),
            ("bold", CtValue::Bool(false)),
            ("dim", CtValue::Bool(false)),
            ("underline", CtValue::Bool(false)),
        ],
    )
}

fn tui_style_color(args: &[CtValue], span: Span, background: bool) -> EvalResult {
    let mut style = structure_fields(args, 0, "TuiStyle", span)?;
    let color = tui_kernel_color(one(args, 1, "core.tui", "style_color", span)?, span)?;
    let field_name = if background {
        "background"
    } else {
        "foreground"
    };
    set_structure_field(
        &mut style,
        field_name,
        CtValue::Present(Box::new(tui_kernel_color_value(color))),
        span,
    )?;
    Ok(style)
}

fn tui_style_flag(args: &[CtValue], span: Span, field_name: &str) -> EvalResult {
    let mut style = structure_fields(args, 0, "TuiStyle", span)?;
    let value = bool_value(args, 1, span)?;
    set_structure_field(&mut style, field_name, value, span)?;
    Ok(style)
}
fn tui_style_text(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    let style = tui_kernel_style(one(args, 1, "core.tui", "style_text", span)?, span)?;
    let capabilities =
        tui_kernel_capabilities(one(args, 2, "core.tui", "style_text", span)?, span)?;
    Ok(CtValue::Str(tui_kernel::style_text(
        text,
        &style,
        &capabilities,
    )))
}

fn structure_fields(
    args: &[CtValue],
    index: usize,
    type_name: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match one(args, index, "core.tui", type_name, span)? {
        CtValue::Struct {
            type_name: actual,
            fields,
        } if actual == type_name => Ok(CtValue::Struct {
            type_name: actual.clone(),
            fields: fields.clone(),
        }),
        _ => Err(unsupported(
            &format!("core.tui expected {type_name}"),
            span,
        )),
    }
}

fn set_structure_field(
    value: &mut CtValue,
    wanted: &str,
    replacement: CtValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let CtValue::Struct { fields, .. } = value else {
        return Err(unsupported("core.tui expected a structure", span));
    };
    if let Some((_, field)) = fields.iter_mut().find(|(name, _)| name == wanted) {
        *field = replacement;
        Ok(())
    } else {
        Err(unsupported(
            &format!("core.tui structure has no `{wanted}` field"),
            span,
        ))
    }
}

fn tui_ascii(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    Ok(CtValue::Str(tui_kernel::ascii(text)))
}

fn tui_display_width(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    Ok(CtValue::Int(tui_kernel::display_width(text) as i64))
}

fn tui_rect_value(x: f64, y: f64, width: f64, height: f64) -> CtValue {
    structure(
        "Rect",
        vec![
            ("x", CtValue::Float(CtFloat::f64(x))),
            ("y", CtValue::Float(CtFloat::f64(y))),
            ("width", CtValue::Float(CtFloat::f64(width))),
            ("height", CtValue::Float(CtFloat::f64(height))),
        ],
    )
}


fn tui_layout(args: &[CtValue], span: Span) -> EvalResult {
    let rect = one(args, 0, "core.tui", "layout", span)?;
    let direction = one(args, 1, "core.tui", "layout", span)?;
    let constraints = match one(args, 2, "core.tui", "layout", span)? {
        CtValue::List(values) => values,
        _ => return Err(unsupported("core.tui.layout() needs [TuiConstraint]", span)),
    };
    let direction = match tui_enum_parts(direction, "TuiDirection", span)?.0 {
        "Horizontal" => tui_kernel::Direction::Horizontal,
        "Vertical" => tui_kernel::Direction::Vertical,
        _ => return Err(unsupported("unknown TuiDirection variant", span)),
    };
    let constraints = constraints
        .iter()
        .map(|constraint| tui_kernel_constraint(constraint, span))
        .collect::<Result<Vec<_>, _>>()?;
    let rects = tui_kernel::layout(tui_kernel_rect(rect, span)?, direction, &constraints);
    Ok(CtValue::List(
        rects
            .into_iter()
            .map(|rect| tui_rect_value(rect.x, rect.y, rect.width, rect.height))
            .collect(),
    ))
}

fn tui_list_state(selected: i64, offset: i64) -> CtValue {
    structure(
        "TuiListState",
        vec![
            ("selected", CtValue::Int(selected.max(0))),
            ("offset", CtValue::Int(offset.max(0))),
        ],
    )
}

fn tui_list_state_update(args: &[CtValue], span: Span, selected: bool) -> EvalResult {
    let state = structure_fields(args, 0, "TuiListState", span)?;
    let value = int_arg(args, 1, span)?.max(0);
    let field_name = if selected { "selected" } else { "offset" };
    let mut updated = state;
    set_structure_field(&mut updated, field_name, CtValue::Int(value), span)?;
    Ok(updated)
}

fn tui_list(args: &[CtValue], span: Span) -> EvalResult {
    let items = match one(args, 0, "core.tui", "list", span)? {
        CtValue::List(values) => values,
        _ => return Err(unsupported("core.tui.list() needs [String]", span)),
    };
    let children = items
        .iter()
        .map(|item| {
            let text = match item {
                CtValue::Str(value) => value.clone(),
                _ => return Err(unsupported("core.tui.list() needs [String]", span)),
            };
            ui_text(&[CtValue::Str(text)], span)
        })
        .collect::<Result<Vec<_>, _>>()?;
    ui_box(&[CtValue::List(children)], span)
}

fn tui_table(args: &[CtValue], span: Span) -> EvalResult {
    let headers = match one(args, 0, "core.tui", "table", span)? {
        CtValue::List(values) => values,
        _ => return Err(unsupported("core.tui.table() needs [String] headers", span)),
    };
    let rows = match one(args, 1, "core.tui", "table", span)? {
        CtValue::List(values) => values,
        _ => return Err(unsupported("core.tui.table() needs [[String]] rows", span)),
    };
    let mut lines = Vec::with_capacity(rows.len() + 1);
    let join = |row: &[CtValue]| -> Result<CtValue, Diagnostic> {
        let mut line = String::new();
        for (index, value) in row.iter().enumerate() {
            if index != 0 {
                line.push_str(" | ");
            }
            line.push_str(string_arg(std::slice::from_ref(value), 0, span)?);
        }
        Ok(CtValue::Str(line))
    };
    lines.push(join(headers)?);
    for row in rows {
        let CtValue::List(values) = row else {
            return Err(unsupported("core.tui.table() needs [[String]] rows", span));
        };
        lines.push(join(values)?);
    }
    tui_list(&[CtValue::List(lines)], span)
}

fn raylib_color(args: &[CtValue], span: Span) -> EvalResult {
    Ok(structure(
        "RaylibColor",
        vec![
            ("r", CtValue::Int(int_arg(args, 0, span)?)),
            ("g", CtValue::Int(int_arg(args, 1, span)?)),
            ("b", CtValue::Int(int_arg(args, 2, span)?)),
            ("a", CtValue::Int(int_arg(args, 3, span)?)),
        ],
    ))
}

fn io_style_force(args: &[CtValue], span: Span) -> EvalResult {
    let style = string_arg(args, 0, span)?;
    let text = string_arg(args, 1, span)?;
    Ok(CtValue::Str(super::term_semantics::jet_term_style_force(
        style, text,
    )))
}

fn net_error(operation: &str, address: Option<String>, message: String) -> CtValue {
    structure(
        "NetError",
        vec![
            ("operation", CtValue::Str(operation.to_string())),
            (
                "address",
                address.map_or(CtValue::absent(Type::String), |value| {
                    CtValue::Present(Box::new(CtValue::Str(value)))
                }),
            ),
            ("name", CtValue::absent(Type::String)),
            ("message", CtValue::Str(message)),
            ("os_code", CtValue::absent(Type::Int)),
        ],
    )
}

fn net_ip_addr(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?;
    let input = text.to_string();
    Ok(
        match super::net_pure_kernel::jet_net_pure_parse_ip(&input) {
            Ok(address) => CtValue::Present(Box::new(structure(
                "IPAddr",
                vec![("text", CtValue::Str(address.to_string()))],
            ))),
            Err(error) => CtValue::failed(Box::new(net_error(
                "parse IP address",
                Some(text.to_string()),
                format!("invalid IP address `{text}`: {error}"),
            ))),
        },
    )
}

fn net_ip_is_ipv4(args: &[CtValue], span: Span) -> EvalResult {
    let text = match field(
        one(args, 0, "core.net", "ip_is_ipv4", span)?,
        "IPAddr",
        "text",
    ) {
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
    Ok(
        match super::net_pure_kernel::jet_net_pure_parse_socket_addr(&input) {
            Ok(address) => CtValue::Present(Box::new(structure(
                "SocketAddr",
                vec![
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
                        CtValue::Str(super::net_pure_kernel::jet_net_pure_socket_to_string(
                            &address,
                        )),
                    ),
                ],
            ))),
            Err(error) => CtValue::failed(Box::new(net_error(
                "parse socket address",
                Some(text.to_string()),
                format!("invalid socket address `{text}`: {error}"),
            ))),
        },
    )
}

fn net_value_field(args: &[CtValue], type_name: &str, name: &str, span: Span) -> EvalResult {
    field(one(args, 0, "core.net", name, span)?, type_name, name)
        .cloned()
        .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))
}

fn net_string_field(args: &[CtValue], type_name: &str, name: &str, span: Span) -> EvalResult {
    match net_value_field(args, type_name, name, span)? {
        CtValue::Str(value) => Ok(CtValue::Str(value)),
        _ => Err(unsupported(
            &format!("malformed {type_name}.{name} value"),
            span,
        )),
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
    structure(
        "CryptoError",
        vec![("reason", CtValue::Str(reason.to_string()))],
    )
}

fn crypto_hkdf(args: &[CtValue], span: Span) -> EvalResult {
    let length = int_arg(args, 3, span)?;
    if !(0..=8_160).contains(&length) {
        return Ok(CtValue::failed(Box::new(crypto_error(
            "HKDF-SHA256 output length must be 0..8160",
        ))));
    }
    let bytes = crate::Comptime::CryptoLite::hkdf_sha256(
        &as_bytes(
            one(args, 0, "core.crypto.expert", "hkdf_sha256_raw", span)?,
            span,
        )?,
        &as_bytes(
            one(args, 1, "core.crypto.expert", "hkdf_sha256_raw", span)?,
            span,
        )?,
        &as_bytes(
            one(args, 2, "core.crypto.expert", "hkdf_sha256_raw", span)?,
            span,
        )?,
        length as usize,
    );
    Ok(CtValue::Present(Box::new(crypto_secret("Secret", bytes))))
}

fn crypto_ed25519_verify(args: &[CtValue], span: Span) -> EvalResult {
    let public = as_bytes(
        one(args, 0, "core.crypto.expert", "ed25519_verify_strict", span)?,
        span,
    )?;
    let message = as_bytes(
        one(args, 1, "core.crypto.expert", "ed25519_verify_strict", span)?,
        span,
    )?;
    let signature = as_bytes(
        one(args, 2, "core.crypto.expert", "ed25519_verify_strict", span)?,
        span,
    )?;
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
    let seed = as_bytes(
        one(args, 0, "core.crypto.expert", "ed25519_sign", span)?,
        span,
    )?;
    let message = as_bytes(
        one(args, 1, "core.crypto.expert", "ed25519_sign", span)?,
        span,
    )?;
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
        (
            0usize,
            1_073_741_824usize,
            "plaintext",
            "at most 1073741824",
        )
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
    if let Some(error) = crypto_aead_lengths(
        operation,
        &key,
        &nonce,
        nonce_length,
        &plaintext,
        &aad,
        false,
    ) {
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
    if let Some(error) = crypto_aead_lengths(
        operation,
        &key,
        &nonce,
        nonce_length,
        &ciphertext,
        &aad,
        true,
    ) {
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
        Some(CtValue::List(bytes)) => as_bytes(&CtValue::List(bytes.clone()), span)?,
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
        || memory_kib
            .checked_mul(iterations)
            .is_none_or(|value| value > 1_048_576)
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
    let secret = as_bytes(
        one(args, 0, "core.crypto.expert", "x25519_raw", span)?,
        span,
    )?;
    let public = as_bytes(
        one(args, 1, "core.crypto.expert", "x25519_raw", span)?,
        span,
    )?;
    if secret.len() != 32 || public.len() != 32 {
        return Ok(CtValue::failed(Box::new(crypto_error(
            "X25519 keys must contain exactly 32 bytes",
        ))));
    }
    let shared = crate::Comptime::CryptoLite::x25519(&secret, &public).expect("length checked");
    if shared == [0; 32] {
        return Ok(CtValue::failed(Box::new(crypto_error(
            "X25519 peer key does not contribute to a shared secret",
        ))));
    }
    Ok(CtValue::Present(Box::new(crypto_secret(
        "Secret",
        shared.to_vec(),
    ))))
}

fn crypto_extract(args: &[CtValue], index: usize, type_name: &str, span: Span) -> EvalResult {
    let value = one(args, index, "core.crypto.expert", "secret_bytes", span)?;
    match field(value, type_name, "bytes") {
        Some(CtValue::Bytes(bytes)) => Ok(CtValue::Bytes(bytes.clone())),
        Some(CtValue::List(bytes)) => {
            as_bytes(&CtValue::List(bytes.clone()), span).map(CtValue::Bytes)
        }
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

fn value_field(value: &CtValue, type_name: &str, name: &str, span: Span) -> EvalResult {
    field(value, type_name, name)
        .cloned()
        .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))
}

fn int_field(value: &CtValue, type_name: &str, name: &str, span: Span) -> Result<i64, Diagnostic> {
    as_int(
        field(value, type_name, name)
            .ok_or_else(|| unsupported(&format!("malformed {type_name}.{name} value"), span))?,
        span,
    )
}

fn string_field(value: &CtValue, type_name: &str, name: &str, span: Span) -> EvalResult {
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
    let CtValue::Str(top) =
        field(value, "Mime", "top").ok_or_else(|| unsupported("malformed Mime.top value", span))?
    else {
        return Err(unsupported("malformed Mime.top value", span));
    };
    let CtValue::Str(sub) =
        field(value, "Mime", "sub").ok_or_else(|| unsupported("malformed Mime.sub value", span))?
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

fn option_int(value: Option<i64>) -> CtValue {
    value.map_or(CtValue::absent(Type::Int), |value| {
        CtValue::Present(Box::new(CtValue::Int(value)))
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
    millisecond: i64,
    microsecond: i64,
    nanosecond: i64,
}

impl LocalTime {
    fn from_inner(inner: super::time_kernel::JetLocalTime) -> Self {
        Self {
            hour: inner.hour(),
            minute: inner.minute(),
            second: inner.second(),
            millisecond: inner.millisecond(),
            microsecond: inner.microsecond(),
            nanosecond: inner.nanosecond(),
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
                ("millisecond", CtValue::Int(self.millisecond)),
                ("microsecond", CtValue::Int(self.microsecond)),
                ("nanosecond", CtValue::Int(self.nanosecond)),
            ],
        )
    }
}

#[derive(Clone)]
struct DateTime {
    inner: super::time_kernel::JetDateTime,
    seconds: i64,
    nanos: u32,
    leap_second: bool,
}

impl DateTime {
    fn from_inner(inner: super::time_kernel::JetDateTime) -> Self {
        Self {
            seconds: inner.unix_seconds_anchor(),
            nanos: inner.nanosecond() as u32,
            leap_second: inner.is_leap_second(),
            inner,
        }
    }

    fn from_timestamp_ns(seconds: i64, nanos: u32) -> Self {
        Self::from_inner(super::time_kernel::JetDateTime::from_timestamp_ns_with_leap(
            seconds, nanos, false,
        ))
    }
    fn from_timestamp_ns_with_leap(seconds: i64, nanos: u32, leap_second: bool) -> Self {
        Self::from_inner(super::time_kernel::JetDateTime::from_timestamp_ns_with_leap(
            seconds, nanos, leap_second,
        ))
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
        LocalTime::from_inner(self.inner.time_for_output())
    }

    fn total_ns(&self) -> i64 {
        self.inner
            .total_nanoseconds()
            .clamp(i64::MIN as i128, i64::MAX as i128) as i64
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
        datetime_value(self.seconds, self.nanos, self.leap_second)
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
        Self {
            inner,
            name,
            offset,
        }
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
        Self {
            inner,
            instant,
            zone,
        }
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
            vec![
                ("instant", self.instant.value()),
                ("zone", self.zone.value()),
            ],
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
    let numerator = crate::Comptime::Builtins::exact_big(
        args.first()
            .ok_or_else(|| unsupported("a ratio top that is not a whole number", span))?,
    )
    .ok_or_else(|| unsupported("a ratio top that is not a whole number", span))?;
    let denominator = crate::Comptime::Builtins::exact_big(
        args.get(1)
            .ok_or_else(|| unsupported("a ratio bottom that is not a whole number", span))?,
    )
    .ok_or_else(|| unsupported("a ratio bottom that is not a whole number", span))?;
    Ok(
        match crate::Numeric::CtFraction::from_bigints(numerator, denominator) {
            Some(value) => CtValue::Present(Box::new(value.to_value())),
            None => CtValue::absent(crate::AST::Type::Named(
                crate::Syntax::TYPE_FRACTION.to_string(),
            )),
        },
    )
}

fn fraction_from_value(
    value: &CtValue,
    span: Span,
) -> Result<crate::Numeric::CtFraction, Diagnostic> {
    crate::Numeric::CtFraction::from_value(value).map_err(|error| unsupported(&error, span))
}

fn decimal_from_value(
    value: &CtValue,
    span: Span,
) -> Result<crate::Numeric::CtDecimal, Diagnostic> {
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
    let disambiguation = args
        .get(3)
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.as_str()),
            _ => Err(unsupported(
                "time.zoned_local expects a disambiguation String",
                span,
            )),
        })
        .transpose()?
        .unwrap_or("compatible");
    Ok(
        match super::time_kernel::JetZonedDateTime::from_local_with_disambiguation(
            &date.inner,
            &time.inner,
            &zone.inner,
            disambiguation,
        ) {
            Ok(zoned) => CtValue::Present(Box::new(ZonedDateTime::from_inner(zoned).value())),
            Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
        },
    )
}

fn zoned_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        match super::time_kernel::jet_time_parse_zoned(&string_arg(args, 0, span)?.to_string()) {
            Ok(zoned) => CtValue::Present(Box::new(ZonedDateTime::from_inner(zoned).value())),
            Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
        },
    )
}

fn format_zoned_pattern(pattern: &str, zoned: ZonedDateTime) -> String {
    zoned.inner.format_pattern(&pattern.to_string())
}

fn date_from_value(value: &CtValue, type_name: &str, span: Span) -> Result<Date, Diagnostic> {
    let field_type_name = match value {
        CtValue::Struct {
            type_name: actual, ..
        } => {
            let actual = actual
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(actual.as_str());
            if matches!(type_name, "Date" | "LocalDate") && matches!(actual, "Date" | "LocalDate") {
                actual
            } else {
                type_name
            }
        }
        _ => type_name,
    };
    Ok(Date::new(
        int_field(value, field_type_name, "year", span)?,
        int_field(value, field_type_name, "month", span)?,
        int_field(value, field_type_name, "day", span)?,
    ))
}

fn local_time_from_value(value: &CtValue, span: Span) -> Result<LocalTime, Diagnostic> {
    let nanos = match field(value, "LocalTime", "nanosecond") {
        Some(value) => as_int(value, span)?
            .clamp(0, 999_999_999)
            .try_into()
            .unwrap_or(0),
        None => 0,
    };
    Ok(LocalTime::from_inner(
        super::time_kernel::JetLocalTime::with_nanosecond(
            int_field(value, "LocalTime", "hour", span)?,
            int_field(value, "LocalTime", "minute", span)?,
            int_field(value, "LocalTime", "second", span)?,
            nanos,
        ),
    ))
}

fn datetime_from_value(value: &CtValue, span: Span) -> Result<DateTime, Diagnostic> {
    let leap_second = match field(value, "DateTime", "leap_second") {
        Some(CtValue::Bool(value)) => *value,
        _ => {
            return Err(unsupported(
                "malformed DateTime.leap_second value",
                span,
            ))
        }
    };
    Ok(DateTime::from_timestamp_ns_with_leap(
        int_field(value, "DateTime", "secs", span)?,
        int_field(value, "DateTime", "nanos", span)? as u32,
        leap_second,
    ))
}

/// One decode of a `Period` struct value into the Prelude kernel's carrier.
fn period_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::time_kernel::JetPeriod, Diagnostic> {
    Ok(super::time_kernel::JetPeriod::new(
        int_field(value, "Period", "years", span)?,
        int_field(value, "Period", "months", span)?,
        int_field(value, "Period", "days", span)?,
    ))
}

fn date_add_period(date: Date, period: &CtValue, span: Span) -> Result<Date, Diagnostic> {
    Ok(Date::from_inner(
        date.inner.add_period(&period_from_value(period, span)?),
    ))
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
    Ok(period_from_value(value, span)?.to_string_fmt())
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
    period_value_from_inner(period)
}

fn period_value_from_inner(period: super::time_kernel::JetPeriod) -> CtValue {
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

fn datetime_value(seconds: i64, nanos: u32, leap_second: bool) -> CtValue {
    structure(
        "DateTime",
        vec![
            ("secs", CtValue::Int(seconds)),
            ("nanos", CtValue::Int(nanos as i64)),
            ("leap_second", CtValue::Bool(leap_second)),
        ],
    )
}

fn instant_start_ns(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    int_field(value, "Instant", "start_ns", span)
}

fn instant_elapsed_millis(value: &CtValue, span: Span) -> EvalResult {
    Ok(CtValue::Int(
        super::time_kernel::jet_time_instant_elapsed_ns(
            super::time_kernel::jet_time_monotonic_now_ns(),
            instant_start_ns(value, span)?,
        )
        .saturating_div(1_000_000),
    ))
}

fn instant_elapsed(value: &CtValue, span: Span) -> EvalResult {
    Ok(duration_value(
        super::time_kernel::jet_time_instant_elapsed_ns(
            super::time_kernel::jet_time_monotonic_now_ns(),
            instant_start_ns(value, span)?,
        ),
    ))
}

fn duration_value(ns: i64) -> CtValue {
    structure(crate::Syntax::DURATION_TYPE, vec![("ns", CtValue::Int(ns))])
}

fn duration_ns(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    int_field(value, crate::Syntax::DURATION_TYPE, "ns", span)
}

fn datetime_from_timestamp(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        DateTime::from_inner(super::time_kernel::JetDateTime::from_timestamp(int_arg(
            args, 0, span,
        )?))
        .value(),
    )
}

fn datetime_from_unix_ms(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        DateTime::from_inner(super::time_kernel::JetDateTime::from_unix_ms(int_arg(
            args, 0, span,
        )?))
        .value(),
    )
}

fn datetime_from_unix_seconds(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        DateTime::from_inner(super::time_kernel::JetDateTime::from_unix_seconds(int_arg(
            args, 0, span,
        )?))
        .value(),
    )
}

fn datetime_from_unix_microseconds(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        DateTime::from_inner(super::time_kernel::JetDateTime::from_unix_microseconds(
            int_arg(args, 0, span)?,
        ))
        .value(),
    )
}

fn datetime_from_unix_nanoseconds(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        DateTime::from_inner(super::time_kernel::JetDateTime::from_unix_nanoseconds(
            int_arg(args, 0, span)?,
        ))
        .value(),
    )
}

fn date_iso_week_parse(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        match super::time_kernel::JetDate::parse_iso_week_date(string_arg(args, 0, span)?) {
            Ok(date) => CtValue::Present(Box::new(Date::from_inner(date).value())),
            Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
        },
    )
}

fn date_from_iso_week(args: &[CtValue], span: Span) -> EvalResult {
    Ok(
        match super::time_kernel::jet_time_from_iso_week(
            int_arg(args, 0, span)?,
            int_arg(args, 1, span)?,
            int_arg(args, 2, span)?,
        ) {
            Ok(date) => CtValue::Present(Box::new(Date::from_inner(date).value())),
            Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
        },
    )
}

fn datetime_parse(args: &[CtValue], span: Span) -> EvalResult {
    let text = string_arg(args, 0, span)?.to_string();
    Ok(match super::time_kernel::jet_time_parse_rfc3339(&text) {
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

fn measurement_arithmetic(left: &CtValue, method: &str, right: &CtValue, span: Span) -> EvalResult {
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
fn measurement_sqrt(value: &CtValue, span: Span) -> EvalResult {
    let measured = match field(value, "Measurement", "value") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.value value", span)),
    };
    let uncertainty = match field(value, "Measurement", "uncertainty") {
        Some(CtValue::Float(value)) => value.as_f64(),
        _ => return Err(unsupported("malformed Measurement.uncertainty value", span)),
    };
    let (value, uncertainty) =
        super::measurement_kernel::jet_measurement_kernel_sqrt((measured, uncertainty));
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
        Err(error) => CtValue::failed(Box::new(crate::Comptime::EncodingLite::xml_error_value(
            error,
        ))),
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
    crate::Comptime::EncodingLite::xml_shape_error_value(reason.to_string())
}


// Email uses the shared Prelude kernel through `EmailAdapter`.

// ── D-APPROX1=A: CtValue adapters for the shared Prelude kernel ──────────────

fn hll_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::sketch_kernel::JetHyperLogLog, Diagnostic> {
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
    Ok(super::sketch_kernel::JetHyperLogLog::from_registers(
        registers,
    ))
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

fn tdigest_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::sketch_kernel::JetTDigest, Diagnostic> {
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

fn cms_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::sketch_kernel::JetCountMinSketch, Diagnostic> {
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
        return Err(unsupported(
            "malformed ReservoirSampler.reservoir value",
            span,
        ));
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
    Ok(reservoir_value(
        &super::sketch_kernel::JetReservoirSampler::new(int_arg(args, 0, span)?),
    ))
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

fn solver_from_value(
    value: &CtValue,
    span: Span,
) -> Result<super::solver_kernel::jet_std::Solver, Diagnostic> {
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
    Ok(CtValue::Int(
        super::solver_kernel::jet_solver_failure_count(&solver),
    ))
}

fn solver_status(recv: &CtValue, span: Span) -> EvalResult {
    let solver = solver_from_value(recv, span)?;
    Ok(CtValue::Str(super::solver_kernel::jet_solver_status(
        &solver,
    )))
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
        let actual = xml_canonical(&[CtValue::Bytes(vec![1, 2, 3]), options], Span::new(0, 0))
            .expect("invalid DataTree is a user Result error");
        assert_eq!(
            actual,
            CtValue::failed(Box::new(xml_shape_error(
                "XML tree cannot contain Float or Bytes values",
            )))
        );
    }

    #[test]
    fn service_lifecycle_display_and_json_use_typed_facts() {
        let receipt = CtValue::Struct {
            type_name: "DeliveryReceipt".to_string(),
            fields: vec![
                ("id".to_string(), CtValue::Str("d-1".to_string())),
                (
                    "state".to_string(),
                    CtValue::Enum {
                        type_name: "DeliveryState".to_string(),
                        variant: "Accepted".to_string(),
                        args: Vec::new(),
                    },
                ),
                ("attempts".to_string(), CtValue::Int(1)),
                ("retention_until".to_string(), CtValue::Int(20)),
                ("deadline".to_string(), CtValue::Int(30)),
                (
                    "idempotency_key".to_string(),
                    CtValue::Str("order-1".to_string()),
                ),
                ("duplicate".to_string(), CtValue::Bool(false)),
                ("authority".to_string(), CtValue::Str("orders".to_string())),
                ("generation".to_string(), CtValue::Int(2)),
                ("signature".to_string(), CtValue::Str("sig".to_string())),
            ],
        };
        assert_eq!(
            display(&receipt),
            Some("DeliveryReceipt(id=d-1, state=Accepted, attempts=1, retention_until=20, deadline=30, key=order-1, duplicate=false, authority=orders, generation=2, signature=sig)".to_string())
        );
        assert_eq!(
            receipt.to_json(),
            "{\"id\":\"d-1\",\"state\":\"Accepted\",\"attempts\":1,\"retention_until\":20,\"deadline\":30,\"idempotency_key\":\"order-1\",\"duplicate\":false,\"authority\":\"orders\",\"generation\":2,\"signature\":\"sig\"}"
        );
        let error = CtValue::Enum {
            type_name: "ServiceError".to_string(),
            variant: "Partitioned".to_string(),
            args: vec![(None, CtValue::Str("authority partitioned".to_string()))],
        };
        assert_eq!(display(&error), Some("authority partitioned".to_string()));
    }
}
