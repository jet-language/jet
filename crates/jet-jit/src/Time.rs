//! `core.time` / civil-time marshalling hosts for the shared Prelude kernel.

use super::Concurrency;
use jet_codegen::AST::CtValue;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use crate::Marshal::{alloc_string, clone_string, result_err_msg, result_ok};

pub(crate) mod time_rt {
    include!("../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs");
    include!("../../jet-codegen/src/Prelude/Core/Time.rs");
}

#[derive(Clone)]
pub(crate) enum TimeValue {
    Date(time_rt::JetDate),
    DateTime(time_rt::JetDateTime),
    Period(time_rt::JetPeriod),
    Instant(time_rt::JetInstant),
    Zone(time_rt::JetZone),
    Zoned(time_rt::JetZonedDateTime),
    LocalTime(time_rt::JetLocalTime),
}

pub(crate) fn ambient_date_today_value() -> CtValue {
    let date = time_rt::JetDate::today_utc();
    CtValue::Struct {
        type_name: "LocalDate".to_string(),
        fields: vec![
            ("year".to_string(), CtValue::Int(date.year())),
            ("month".to_string(), CtValue::Int(date.month())),
            ("day".to_string(), CtValue::Int(date.day())),
        ],
    }
}

pub(crate) fn ambient_datetime_now_value() -> CtValue {
    let datetime = time_rt::JetDateTime::now();
    CtValue::Struct {
        type_name: "DateTime".to_string(),
        fields: vec![
            ("secs".to_string(), CtValue::Int(datetime.to_timestamp())),
            (
                "nanos".to_string(),
                CtValue::Int(datetime.nanosecond()),
            ),
        ],
    }
}

pub(crate) fn ambient_monotonic_now_ms() -> i64 {
    time_rt::jet_time_monotonic_now_ns() / 1_000_000
}

pub(crate) fn ambient_instant_value() -> CtValue {
    CtValue::Struct {
        type_name: "Instant".to_string(),
        fields: vec![(
            "start_ns".to_string(),
            CtValue::Int(time_rt::jet_time_monotonic_now_ns()),
        )],
    }
}

extern "C" fn jet_jit_time_start() -> i64 {
    ambient_monotonic_now_ms()
}

extern "C" fn jet_jit_stopwatch_elapsed_millis(start_ms: i64) -> i64 {
    ambient_monotonic_now_ms().saturating_sub(start_ms)
}

fn push(value: TimeValue) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.time_values.push(Some(value));
        rt.time_values.len() as i64
    })
}

fn with_time<R: Default>(handle: i64, f: impl FnOnce(&TimeValue) -> R) -> R {
    Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        match rt.time_values.get(idx).and_then(|s| s.as_ref()) {
            Some(v) => f(v),
            None => R::default(),
        }
    })
}

fn result_err(msg: String) -> i64 {
    result_err_msg(&msg)
}

extern "C" fn jet_jit_date_new(y: i64, m: i64, d: i64) -> i64 {
    push(TimeValue::Date(time_rt::JetDate::new(y, m, d)))
}

extern "C" fn jet_jit_date_today() -> i64 {
    push(TimeValue::Date(time_rt::JetDate::today_utc()))
}

extern "C" fn jet_jit_date_parse(s: i64) -> i64 {
    match time_rt::JetDate::parse(&clone_string(s)) {
        Ok(d) => result_ok(push(TimeValue::Date(d)) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_datetime_from_timestamp(ts: i64) -> i64 {
    push(TimeValue::DateTime(time_rt::JetDateTime::from_timestamp(ts)))
}

extern "C" fn jet_jit_datetime_now() -> i64 {
    push(TimeValue::DateTime(time_rt::JetDateTime::now()))
}

extern "C" fn jet_jit_time_parse_rfc3339(s: i64) -> i64 {
    match time_rt::JetDateTime::parse_rfc3339(&clone_string(s)) {
        Ok(dt) => result_ok(push(TimeValue::DateTime(dt)) as u64),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_time_from_unix_ms(ms: i64) -> i64 {
    push(TimeValue::DateTime(time_rt::JetDateTime::from_unix_ms(ms)))
}

extern "C" fn jet_jit_time_utc() -> i64 {
    push(TimeValue::Zone(time_rt::JetZone::utc()))
}

extern "C" fn jet_jit_time_period_months(months: i64) -> i64 {
    push(TimeValue::Period(time_rt::JetPeriod::months(months)))
}

extern "C" fn jet_jit_time_period(years: i64, months: i64, days: i64) -> i64 {
    push(TimeValue::Period(time_rt::JetPeriod::new(years, months, days)))
}

extern "C" fn jet_jit_time_period_days(days: i64) -> i64 {
    push(TimeValue::Period(time_rt::JetPeriod::days(days)))
}

extern "C" fn jet_jit_time_period_years(years: i64) -> i64 {
    push(TimeValue::Period(time_rt::JetPeriod::years(years)))
}

extern "C" fn jet_jit_time_zone(name: i64) -> i64 {
    match time_rt::JetZone::named(&clone_string(name)) {
        Ok(zone) => result_ok(push(TimeValue::Zone(zone)) as u64),
        Err(error) => result_err(error),
    }
}

extern "C" fn jet_jit_time_zoned_local(date: i64, time: i64, zone: i64) -> i64 {
    let date = with_time(date, |value| match value {
        TimeValue::Date(date) => Some(date.clone()),
        _ => None,
    });
    let time = with_time(time, |value| match value {
        TimeValue::LocalTime(time) => Some(time.clone()),
        _ => None,
    });
    let zone = with_time(zone, |value| match value {
        TimeValue::Zone(zone) => Some(zone.clone()),
        _ => None,
    });
    match (date, time, zone) {
        (Some(date), Some(time), Some(zone)) => {
            push(TimeValue::Zoned(time_rt::JetZonedDateTime::from_local(
                &date, &time, &zone,
            )))
        }
        _ => 0,
    }
}

extern "C" fn jet_jit_time_parse_time(value: i64) -> i64 {
    match time_rt::JetLocalTime::parse(&clone_string(value)) {
        Ok(time) => result_ok(push(TimeValue::LocalTime(time)) as u64),
        Err(error) => result_err(error),
    }
}

extern "C" fn jet_jit_time_instant() -> i64 {
    push(TimeValue::Instant(time_rt::JetInstant::now()))
}

extern "C" fn jet_jit_time_zoned(dt: i64, zone: i64) -> i64 {
    let datetime = with_time(dt, |v| match v {
        TimeValue::DateTime(d) => Some(d.clone()),
        _ => None,
    });
    let z = with_time(zone, |v| match v {
        TimeValue::Zone(z) => Some(z.clone()),
        _ => None,
    });
    match (datetime, z) {
        (Some(dt), Some(zone)) => push(TimeValue::Zoned(dt.in_zone(&zone))),
        _ => 0,
    }
}

extern "C" fn jet_jit_time_days_in_month(year: i64, month: i64) -> i64 {
    time_rt::JetDate::days_in_month_of(year, month.clamp(1, 12))
}

extern "C" fn jet_jit_time_is_leap_year(year: i64) -> i8 {
    i8::from(time_rt::JetDate::is_leap(year))
}

extern "C" fn jet_jit_time_datetime(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    push(TimeValue::DateTime(time_rt::JetDateTime::from_parts(
        year, month, day, hour, minute, second, 0,
    )))
}

extern "C" fn jet_jit_time_local_time(hour: i64, minute: i64, second: i64) -> i64 {
    push(TimeValue::LocalTime(time_rt::JetLocalTime::new(
        hour, minute, second,
    )))
}

/// `core.time.{nanoseconds,…}` — Result ok bits = Duration ns.
extern "C" fn jet_jit_time_duration_unit(value: i64, unit: i64) -> i64 {
    let scale = match unit {
        0 => 1i64,
        1 => 1_000,
        2 => 1_000_000,
        3 => 1_000_000_000,
        4 => 60_000_000_000,
        5 => 3_600_000_000_000,
        _ => 1,
    };
    match crate::runtime_host::duration_kernel::jet_duration_kernel_from_int(value, scale) {
        Some(ns) => result_ok(ns as u64),
        None => result_err(
            crate::runtime_host::duration_kernel::jet_duration_kernel_int_error_reason()
                .into(),
        ),
    }
}

/// Civil-time method dispatch. `kind`: 0=Date, 1=DateTime, 2=Period, 3=Instant, 4=Zone, 5=Zoned.
/// `method` is a string handle; args packed as i64 list handle (or 0).
extern "C" fn jet_jit_civil_time_method(
    recv: i64,
    method: i64,
    arg0: i64,
    arg1: i64,
    arg2: i64,
    arg3: i64,
    arg4: i64,
    arg5: i64,
) -> i64 {
    let method = clone_string(method);
    with_time(recv, |v| match (v, method.as_str()) {
        (TimeValue::Date(d), "year") => d.year(),
        (TimeValue::Date(d), "month") => d.month(),
        (TimeValue::Date(d), "day") => d.day(),
        (TimeValue::Date(d), "to_string") => alloc_string(d.to_string_fmt()),
        (TimeValue::Date(d), "add_days") => push(TimeValue::Date(d.add_days(arg0))),
        (TimeValue::Date(d), "add_months") => push(TimeValue::Date(d.add_months(arg0))),
        (TimeValue::Date(d), "diff_days") => {
            let other = with_time(arg0, |o| match o {
                TimeValue::Date(od) => Some(od.clone()),
                _ => None,
            });
            other.map(|o| d.diff_days(&o)).unwrap_or(0)
        }
        (TimeValue::Date(d), "weekday") => d.weekday(),
        (TimeValue::Date(d), "day_of_year") => d.day_of_year(),
        (TimeValue::Date(d), "iso_weekday") => d.iso_weekday(),
        (TimeValue::Date(d), "iso_week") => d.iso_week(),
        (TimeValue::Date(d), "quarter_of_year") => d.quarter_of_year(),
        (TimeValue::Date(d), "days_in_month") => d.days_in_month(),
        (TimeValue::Date(d), "is_leap_year") => {
            if d.is_leap_year() {
                1
            } else {
                0
            }
        }
        (TimeValue::Date(d), "replace") => {
            push(TimeValue::Date(d.replace(arg0, arg1, arg2)))
        }
        (TimeValue::Date(d), "add_period") => {
            let period = with_time(arg0, |o| match o {
                TimeValue::Period(p) => Some(p.clone()),
                _ => None,
            });
            period
                .map(|p| push(TimeValue::Date(d.add_period(&p))))
                .unwrap_or(0)
        }
        (TimeValue::Date(d), "format") => {
            alloc_string(d.format_pattern(&clone_string(arg0)))
        }
        (TimeValue::DateTime(dt), "to_timestamp") => dt.to_timestamp(),
        (TimeValue::DateTime(dt), "date") => push(TimeValue::Date(dt.date())),
        (TimeValue::DateTime(dt), "time") => push(TimeValue::LocalTime(dt.time())),
        (TimeValue::DateTime(dt), "hour") => dt.hour(),
        (TimeValue::DateTime(dt), "minute") => dt.minute(),
        (TimeValue::DateTime(dt), "second") => dt.second(),
        (TimeValue::DateTime(dt), "millisecond") => dt.millisecond(),
        (TimeValue::DateTime(dt), "microsecond") => dt.microsecond(),
        (TimeValue::DateTime(dt), "nanosecond") => dt.nanosecond(),
        (TimeValue::DateTime(dt), "to_string") => alloc_string(dt.to_string_fmt()),
        (TimeValue::DateTime(dt), "format_rfc3339") => alloc_string(dt.format_rfc3339()),
        (TimeValue::DateTime(dt), "to_unix_ms") => dt.to_unix_ms(),
        (TimeValue::DateTime(dt), "format") => {
            alloc_string(dt.format_pattern(&clone_string(arg0)))
        }
        (TimeValue::DateTime(dt), "truncate" | "floor") => {
            push(TimeValue::DateTime(dt.floor(&clone_string(arg0))))
        }
        (TimeValue::DateTime(dt), "ceil") => {
            push(TimeValue::DateTime(dt.ceil(&clone_string(arg0))))
        }
        (TimeValue::DateTime(dt), "round") => {
            push(TimeValue::DateTime(dt.round(&clone_string(arg0))))
        }
        (TimeValue::DateTime(dt), "replace") => push(TimeValue::DateTime(dt.replace(
            arg0, arg1, arg2, arg3, arg4, arg5,
        ))),
        (TimeValue::DateTime(dt), "in_zone") => {
            let zone = with_time(arg0, |o| match o {
                TimeValue::Zone(z) => Some(z.clone()),
                _ => None,
            });
            zone.map(|z| push(TimeValue::Zoned(dt.in_zone(&z)))).unwrap_or(0)
        }
        (TimeValue::DateTime(dt), "plus_duration") => {
            // Duration is raw ns i64 after Result unwrap (I9 Duration ABI).
            push(TimeValue::DateTime(dt.plus_duration_ns(arg0)))
        }
        (TimeValue::DateTime(dt), "difference") => {
            let other = with_time(arg0, |o| match o {
                TimeValue::DateTime(od) => Some(od.clone()),
                _ => None,
            });
            other.map(|o| dt.difference_ns(&o)).unwrap_or(0)
        }
        (TimeValue::Instant(i), "elapsed_millis") => i.elapsed_millis(),
        (TimeValue::Instant(i), "elapsed") => i.elapsed_nanos(),
        (TimeValue::LocalTime(t), "to_string") => alloc_string(t.to_string_fmt()),
        (TimeValue::Zoned(z), "format") => {
            alloc_string(z.format_pattern(&clone_string(arg0)))
        }
        (TimeValue::Zoned(z), "offset_seconds") => z.offset_seconds(),
        (TimeValue::Zoned(z), "is_dst") => {
            if z.is_dst() {
                1
            } else {
                0
            }
        }
        _ => 0,
    })
}

host_fns! {
    struct TimeHostFns;
    register: register_time_symbols;
    declare: declare_time_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        for _ in 0..3 {
            ternary.params.push(AbiParam::new(types::I64));
        }
        ternary.returns.push(AbiParam::new(types::I64));
        let mut quaternary = Signature::new(cc);
        for _ in 0..4 {
            quaternary.params.push(AbiParam::new(types::I64));
        }
        quaternary.returns.push(AbiParam::new(types::I64));
        let mut hexary = Signature::new(cc);
        for _ in 0..6 {
            hexary.params.push(AbiParam::new(types::I64));
        }
        hexary.returns.push(AbiParam::new(types::I64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut octonary = Signature::new(cc);
        for _ in 0..8 {
            octonary.params.push(AbiParam::new(types::I64));
        }
        octonary.returns.push(AbiParam::new(types::I64));


    }
    date_new: "jet_jit_date_new" => jet_jit_date_new: ternary;
    date_today: "jet_jit_date_today" => jet_jit_date_today: nullary;
    start: "jet_jit_time_start" => jet_jit_time_start: nullary;
    stopwatch_elapsed: "jet_jit_stopwatch_elapsed_millis" => jet_jit_stopwatch_elapsed_millis: unary;
    date_parse: "jet_jit_date_parse" => jet_jit_date_parse: unary;
    datetime_from_timestamp: "jet_jit_datetime_from_timestamp" => jet_jit_datetime_from_timestamp: unary;
    datetime_now: "jet_jit_datetime_now" => jet_jit_datetime_now: nullary;
    parse_rfc3339: "jet_jit_time_parse_rfc3339" => jet_jit_time_parse_rfc3339: unary;
    from_unix_ms: "jet_jit_time_from_unix_ms" => jet_jit_time_from_unix_ms: unary;
    utc: "jet_jit_time_utc" => jet_jit_time_utc: nullary;
    period: "jet_jit_time_period" => jet_jit_time_period: ternary;
    period_days: "jet_jit_time_period_days" => jet_jit_time_period_days: unary;
    period_months: "jet_jit_time_period_months" => jet_jit_time_period_months: unary;
    period_years: "jet_jit_time_period_years" => jet_jit_time_period_years: unary;
    zone: "jet_jit_time_zone" => jet_jit_time_zone: unary;
    zoned_local: "jet_jit_time_zoned_local" => jet_jit_time_zoned_local: ternary;
    parse_time: "jet_jit_time_parse_time" => jet_jit_time_parse_time: unary;
    instant: "jet_jit_time_instant" => jet_jit_time_instant: nullary;
    zoned: "jet_jit_time_zoned" => jet_jit_time_zoned: binary;
    days_in_month: "jet_jit_time_days_in_month" => jet_jit_time_days_in_month: binary;
    is_leap_year: "jet_jit_time_is_leap_year" => jet_jit_time_is_leap_year: unary_i8;
    datetime: "jet_jit_time_datetime" => jet_jit_time_datetime: hexary;
    local_time: "jet_jit_time_local_time" => jet_jit_time_local_time: ternary;
    duration_unit: "jet_jit_time_duration_unit" => jet_jit_time_duration_unit: binary;
    civil_method: "jet_jit_civil_time_method" => jet_jit_civil_time_method: octonary;
}
