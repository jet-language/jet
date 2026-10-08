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
    date :: time.parse("2024-02-29") ?? panic("valid leap date")
    span :: time.duration_ns(1500000000)
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

#[test]
fn core_calendar_predicates_preserve_signed_years_and_invalid_months_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_calendar_predicates",
        r#"
use core.time as time
use core.time.calendar as calendar

fn run() {
    years :: [Int]{-400, -100, -4, 0, 1900, 2000, 2023, 2024}
    loop year in years {
        print(time.is_leap_year(year))
        print(calendar.isleap(year))
    }
    months :: [Int]{0, 1, 2, 4, 12, 13}
    loop month in months -> print(time.days_in_month(2024, month))
    print(time.days_in_month(1900, 2))
    print(time.days_in_month(2000, 2))
}
"#,
        "true\ntrue\nfalse\nfalse\ntrue\ntrue\ntrue\ntrue\nfalse\nfalse\ntrue\ntrue\nfalse\nfalse\ntrue\ntrue\n30\n31\n29\n30\n31\n30\n28\n29\n",
    );
}

#[test]
fn civil_with_methods_preserve_time_error_reasons_across_tiers() {
    tir_support::assert_tiers_agree(
        "core_time_typed_overflow",
        r#"
use core.time as time

fn run() {
    date :: time.new(2024, 1, 31)
    if date.with(2024, 2, 31, "reject") == {
        .Ok(_) -> print("date accepted")
        .Err(error) -> print(error.reason == "date fields are outside the valid range: 2024-02-31")
    }
    datetime :: time.datetime(2024, 1, 31, 12, 0, 0)
    if datetime.with(2024, 1, 31, 24, 0, 0, "reject") == {
        .Ok(_) -> print("time accepted")
        .Err(error) -> print(error.reason == "time fields are outside the valid range: 24:00:00")
    }
    zoned :: datetime.in_zone(time.utc())
    if zoned.with_time(time.local_time(1, 2, 3), "invalid") == {
        .Ok(_) -> print("zone accepted")
        .Err(error) -> print(error.reason == "invalid disambiguation: invalid")
    }
}
"#,
        "true\ntrue\ntrue\n",
    );
}

