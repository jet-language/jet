mod common;
mod tir_support;

#[test]
fn checked_time_format_grammar_and_diagnostics_match_across_tiers() {
    assert!(
        jet_foundation::Registry::diagnostic("E2703").is_some(),
        "checked time-format failures need one registered diagnostic"
    );
    let source = r#"
use core.time as time


fn run() {
    datetime :: time.datetime(2024, 3, 9, 12, 34, 56)
    zoned :: time.zoned(datetime, time.utc())

    if datetime.format_checked("yyyy-MM-dd HH:mm:ss.SSSSSSSSS") == {
        .Ok(text) -> { print("tokens:ok:{text}") }
        .Err(_) -> { print("tokens:err:unexpected") }
    }
    if datetime.format_checked("'day' EEEE 'month' MMMM") == {
        .Ok(text) -> { print("names:ok:{text}") }
        .Err(_) -> { print("names:err:unexpected") }
    }
    if datetime.format_checked("%F %T %A %B") == {
        .Ok(text) -> { print("percent:ok:{text}") }
        .Err(_) -> { print("percent:err:unexpected") }
    }
    if zoned.format_checked("VV XXX") == {
        .Ok(text) -> { print("zone-ok:ok:{text}") }
        .Err(_) -> { print("zone-ok:err:unexpected") }
    }
    if zoned.format_checked("yyyy|MM|dd|DDD|HH|mm|ss|SSS|SSSSSS|SSSSSSSSS|EEE|EEEE|MMM|MMMM|VV|XXX") == {
        .Ok(text) -> { print("all-jet:ok:{text}") }
        .Err(_) -> { print("all-jet:err:unexpected") }
    }
    if zoned.format_checked("%%|%A|%a|%B|%b|%Y|%y|%m|%d|%e|%j|%H|%I|%M|%S|%p|%z|%Z|%F|%T|%R|%D|%f") == {
        .Ok(text) -> { print("all-percent:ok:{text}") }
        .Err(_) -> { print("all-percent:err:unexpected") }
    }
    if datetime.format_checked("'broken") == {
        .Ok(text) -> { print("unterminated:ok:{text}") }
        .Err(error) -> { print("unterminated:err:{error:Debug}") }
    }
    if datetime.format_checked("%") == {
        .Ok(text) -> { print("dangling-percent:ok:{text}") }
        .Err(error) -> { print("dangling-percent:err:{error:Debug}") }
    }
    if datetime.format_checked("%Q") == {
        .Ok(text) -> { print("unknown-percent:ok:{text}") }
        .Err(error) -> { print("unknown-percent:err:{error:Debug}") }
    }
    if datetime.format_checked("qq") == {
        .Ok(text) -> { print("unknown-token:ok:{text}") }
        .Err(error) -> { print("unknown-token:err:{error:Debug}") }
    }
    if datetime.format_checked("%z") == {
        .Ok(text) -> { print("missing-zone-percent:ok:{text}") }
        .Err(error) -> { print("missing-zone-percent:err:{error:Debug}") }
    }
    if datetime.format_checked("VV") == {
        .Ok(text) -> { print("missing-zone-token:ok:{text}") }
        .Err(error) -> { print("missing-zone-token:err:{error:Debug}") }
    }
    return
}
"#;
    let expected = concat!(
        "tokens:ok:2024-03-09 12:34:56.000000000\n",
        "names:ok:day Saturday month March\n",
        "percent:ok:2024-03-09 12:34:56 Saturday March\n",
        "zone-ok:ok:UTC +00:00\n",
        "all-jet:ok:2024|03|09|069|12|34|56|000|000000|000000000|Sat|Saturday|Mar|March|UTC|+00:00\n",
        "all-percent:ok:%|Saturday|Sat|March|Mar|2024|24|03|09| 9|069|12|12|34|56|PM|+00:00|UTC|2024-03-09|12:34:56|12:34|03/09/24|000000000\n",
        "unterminated:err:E2703: unterminated format literal\n",
        "dangling-percent:err:E2703: format ends after `%`\n",
        "unknown-percent:err:E2703: unsupported format token `%Q`\n",
        "unknown-token:err:E2703: unsupported format token `q`\n",
        "missing-zone-percent:err:E2703: format token `%z` requires a zone\n",
        "missing-zone-token:err:E2703: format token `VV` requires a zone\n",
    );

    tir_support::assert_tiers_agree("time_format_checked", source, expected);
}

#[test]
fn epoch_precision_round_trips_and_reports_overflow_across_tiers() {
    assert!(
        jet_foundation::Registry::diagnostic("E2704").is_some(),
        "epoch overflow failures need one registered diagnostic"
    );
    let source = r#"
use core.time as time


fn run() {
    exact :: time.from_unix_nanoseconds(1710502245123456789)
    negative :: time.from_unix_nanoseconds(-1710502245123456789)
    too_large :: time.from_unix_seconds(10000000000000)

    print("seconds:{exact.to_unix_s()}")
    if exact.to_unix_us() == {
        .Ok(number) -> { print("microseconds:ok:{number}") }
        .Err(_) -> { print("microseconds:err:unexpected") }
    }
    if exact.to_unix_ns() == {
        .Ok(number) -> { print("nanoseconds:ok:{number}") }
        .Err(_) -> { print("nanoseconds:err:unexpected") }
    }
    if negative.to_unix_us() == {
        .Ok(number) -> { print("negative-microseconds:ok:{number}") }
        .Err(_) -> { print("negative-microseconds:err:unexpected") }
    }
    if negative.to_unix_ns() == {
        .Ok(number) -> { print("negative-nanoseconds:ok:{number}") }
        .Err(_) -> { print("negative-nanoseconds:err:unexpected") }
    }
    if too_large.to_unix_us() == {
        .Ok(number) -> { print("overflow-microseconds:ok:{number}") }
        .Err(error) -> { print("overflow-microseconds:err:{error:Debug}") }
    }
    if too_large.to_unix_ns() == {
        .Ok(number) -> { print("overflow-nanoseconds:ok:{number}") }
        .Err(error) -> { print("overflow-nanoseconds:err:{error:Debug}") }
    }
    return
}
"#;
    let expected = concat!(
        "seconds:1710502245\n",
        "microseconds:ok:1710502245123456\n",
        "nanoseconds:ok:1710502245123456789\n",
        "negative-microseconds:ok:-1710502245123457\n",
        "negative-nanoseconds:ok:-1710502245123456789\n",
        "overflow-microseconds:err:E2704: Unix epoch microseconds do not fit in Int\n",
        "overflow-nanoseconds:err:E2704: Unix epoch nanoseconds do not fit in Int\n",
    );

    tir_support::assert_tiers_agree("time_epoch_precision", source, expected);
}
