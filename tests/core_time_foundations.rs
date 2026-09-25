//! Source-owned time, calendar, and deadline boundaries across all execution tiers.

mod common;
mod tir_support;

#[test]
fn core_time_epoch_splits_and_rejects_integer_overflow_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_epoch_foundations",
        r#"
use core.time as time

fn run() {
    print(time.from_unix_nanoseconds(-1).format_rfc3339())
    print(time.from_unix_microseconds(-1).format_rfc3339())
    print(time.from_unix_ms(-1).format_rfc3339())
    print(time.from_unix_seconds(-1).format_rfc3339())

    if time.parse("2024-02-29") == {
        .Ok(date) -> print("leap:{date}")
        .Err(_) -> print("leap:rejected")
    }
    if time.parse("2023-02-29") == {
        .Ok(_) -> print("bad-leap:accepted")
        .Err(_) -> print("bad-leap:rejected")
    }
    if time.parse("9223372036854775808-01-01") == {
        .Ok(_) -> print("wide-year:accepted")
        .Err(_) -> print("wide-year:rejected")
    }
}
"#,
        "1969-12-31T23:59:59.999999999Z\n1969-12-31T23:59:59.999999000Z\n1969-12-31T23:59:59.999000000Z\n1969-12-31T23:59:59Z\nleap:2024-02-29\nbad-leap:rejected\nwide-year:rejected\n",
    );
}

#[test]
fn core_time_root_date_and_duration_carriers_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_root_carriers",
        r#"
use core.time as time

fn run() {
    date :: Date{year: 2024, month: 2, day: 29}
    span :: Duration{ns: 1500000000}
    print(date)
    print(date.year())
    print(date.month())
    print(date.day())
    print(span.ns)
    print(time.duration_as_ms(span))
}
"#,
        "2024-02-29\n2024\n2\n29\n1500000000\n1500\n",
    );
}

#[test]
fn core_calendar_leap_and_epoch_boundaries_match_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_calendar_foundations",
        r#"
use core.time.calendar as calendar

fn run() {
    print(calendar.isleap(1900))
    print(calendar.isleap(2000))
    print(calendar.leapdays(1996, 2005))
    print(calendar.leapdays(2005, 1996))

    range :: calendar.monthrange(2024, 2)
    print(range.days)
    print(range.weekday)
    month :: calendar.monthcalendar_start(2024, 2, -1)
    print(month[0][0])
    print(month[0][4])
    print(month[0][6])
    print(month.len())
    print(calendar.timegm(1970, 1, 1, 0, 0, 0))
    print(calendar.timegm(1969, 12, 31, 23, 59, 59))
}
"#,
        "false\ntrue\n3\n3\n29\n3\n0\n1\n3\n5\n0\n-1\n",
    );
}

#[test]
fn core_expiring_deadline_boundaries_are_strict_and_saturating_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_expiring_foundations",
        r#"
use core.time.expiring as expiring

fn run() {
    print(expiring.expired(10, 10))
    print(expiring.expired(10, 11))
    print(expiring.expired(-1, 0))
    print(expiring.remaining_ms(10, 10))
    print(expiring.remaining_ms(10, 9))
    print(expiring.remaining_ms(9223372036854775807, Int.MIN))
    print(expiring.remaining_ms(Int.MIN, Int.MIN))
    print(expiring.remaining_ms(Int.MIN, Int.MAX))
}
"#,
        "false\ntrue\ntrue\n0\n1\n9223372036854775807\n0\n0\n",
    );
}
