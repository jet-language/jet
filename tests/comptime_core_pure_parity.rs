//! #392 Packet B: remaining deterministic Core constructors/codecs must run
//! through the public REPL/comptime path and agree with generated Rust.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet::REPL::run_transcript;

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);

const PIVOT_DECLS: &str = "use core.data as data\nstruct PivotRow { team: String; bucket: String; score: Float }\nfn pivot_view() -> String {\n    prefix :: \"p\"\n    rows :: [PivotRow.{ team: \"B\", bucket: \"y\", score: 5.0 }, PivotRow.{ team: \"A\", bucket: \"x\", score: 1.5 }, PivotRow.{ team: \"A\", bucket: \"x\", score: 2.5 }]\n    groups :: data.pivot_sum(rows, (row) => \"{prefix}{row.team}\", (row) => row.bucket, (row) => row.score)\n    return \"{groups[0].key}:{groups[0].count}:{groups[0].sum}:{groups[0].mean}|{groups[1].key}:{groups[1].count}:{groups[1].sum}:{groups[1].mean}\"\n}";
const PIVOT_EXPR: &str = "pivot_view()";
const CIVIL_FN: &str = "fn civil_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    p :: time.period(0, 1, 2)\n    dt :: datetime.from_timestamp(-1)\n    span :: Duration.milliseconds(1500) ?? panic(\"duration\")\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.add_period(p).to_string()}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.plus_duration(span).to_timestamp()}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const CIVIL_DECLS: &str = "use core.time as time\nuse core.time.date as date\nuse core.time.datetime as datetime\nfn civil_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    p :: time.period(0, 1, 2)\n    dt :: datetime.from_timestamp(-1)\n    span :: Duration.milliseconds(1500) ?? panic(\"duration\")\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.add_period(p).to_string()}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.plus_duration(span).to_timestamp()}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const CIVIL_EXPR: &str = "civil_view()";
const CIVIL_DEV_DECLS: &str = "use core.time.date as date\nuse core.time.datetime as datetime\nfn civil_dev_view() -> String {\n    d :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    other :: date.new(2024, 2, 1)\n    dt :: datetime.from_timestamp(-1)\n    return \"{d.weekday()}|{d.iso_weekday()}|{d.day_of_year()}|{d.iso_week()}|{d.add_days(1).to_string()}|{d.add_months(12).to_string()}|{d.diff_days(other)}|{d.truncate(\"month\").to_string()}|{d.format(\"EEE yyyy-DDD\")}|{dt.date().to_string()}|{dt.time().to_string()}|{dt.hour()}:{dt.minute()}:{dt.second()}|{dt.format_rfc3339()}|{dt.format(\"yyyy-MM-dd HH:mm:ss\")}|{dt.truncate(\"minute\").to_timestamp()}|{dt.round(\"minute\").to_timestamp()}\"\n}";
const MEASUREMENT_FN: &str = "fn measurement_math() -> String {\n    a :: measurement.from(3.0, 4.0)\n    b :: measurement.from(0.0, 3.0)\n    q :: measurement.from(8.0, 0.0).div(measurement.from(2.0, 0.0))\n    return \"{a.add(b).value()}|{a.add(b).uncertainty()}|{a.sub(b).value()}|{a.sub(b).uncertainty()}|{a.mul(b).value()}|{a.mul(b).uncertainty()}|{q.value()}|{q.uncertainty()}\"\n}";
const MEASUREMENT_DECLS: &str = "use core.science.measurement as measurement\nfn measurement_math() -> String {\n    a :: measurement.from(3.0, 4.0)\n    b :: measurement.from(0.0, 3.0)\n    q :: measurement.from(8.0, 0.0).div(measurement.from(2.0, 0.0))\n    return \"{a.add(b).value()}|{a.add(b).uncertainty()}|{a.sub(b).value()}|{a.sub(b).uncertainty()}|{a.mul(b).value()}|{a.mul(b).uncertainty()}|{q.value()}|{q.uncertainty()}\"\n}";
const MEASUREMENT_EXPR: &str = "measurement_math()";
const SCALAR_DECLS: &str = r#"fn scalar_view() -> String {
    i8: I8 :: -12
    i16: I16 :: -1234
    i32: I32 :: -123456
    u8: U8 :: 255
    u16: U16 :: 1234
    u32: U32 :: 123456
    u64: U64 :: 123456789
    nan :: Float.parse("NaN") ?? 0.0
    infinity :: Float.parse("inf") ?? 0.0
    return "{"a@b@c".after("@")}|{"a@b@c".before("@")}|{"no-sep".after("@")}|{"no-sep".before("@")}|{"é🙂".bytes()}|{"aé🙂z".slice(1, 2)}|{nan.is_nan()}|{infinity.is_infinite()}|{1.0.is_finite()}|{i8.to_string()}|{i16.to_string()}|{i32.to_string()}|{u8.to_string()}|{u16.to_string()}|{u32.to_string()}|{u64.to_string()}"
}"#;
const SCALAR_EXPR: &str = "scalar_view()";
const SCALAR_EXPECTED: &str = "b@c|a|no-sep|no-sep|[195, 169, 240, 159, 153, 130]|é🙂|true|true|true|-12|-1234|-123456|255|1234|123456|123456789";
const F32_MATH_DECLS: &str = r#"use core.math as math
fn f32_math_view() -> String {
    rounded: F32 :: 16777217.0
    max: F32 :: F32.MAX
    positive_overflow: F32 :: max + max
    negative_overflow: F32 :: -max + -max
    negative_zero: F32 :: -0.0
    nan: F32 :: F32.NAN
    root_input: F32 :: 2.0
    exponent: F32 :: 3.0
    low: F32 :: 1.0
    high: F32 :: 4.0
    t: F32 :: 0.25
    floor_input: F32 :: 1.75
    ceil_input: F32 :: 1.25
    wide: Float :: 16777217.0
    wide_root: Float :: 2.0
    return "{math.to_bits(rounded)}|{math.sqrt(root_input)}|{math.pow(root_input, exponent)}|{math.floor(floor_input)}|{math.ceil(ceil_input)}|{math.sin(root_input)}|{math.atan2(root_input, exponent)}|{math.hypot(root_input, exponent)}|{math.lerp(low, high, t)}|{math.min(low, high)}|{math.max(low, high)}|{math.clamp(exponent, low, root_input)}|{math.abs(-root_input)}|{math.is_inf(positive_overflow)}|{math.is_inf(negative_overflow)}|{math.is_nan(nan)}|{math.is_finite(rounded)}|{math.sign(negative_zero)}|{math.to_bits(negative_zero)}|{wide}|{math.sqrt(wide_root)}"
}"#;
const F32_MATH_EXPECTED: &str = "1266679808|1.4142135|8.0|1.0|2.0|0.9092974|0.5880026|3.6055512|1.75|1.0|4.0|2.0|2.0|true|true|true|true|0|2147483648|16777217.0|1.4142135623730951";
const PRIMITIVE_INSTANCE_DECLS: &str = r#"fn primitive_instance_view() -> String {
    return "{true.to_string()}|{false.to_string()}|{'e'.to_string()}|{'é'.to_string()}"
}"#;
const PRIMITIVE_INSTANCE_EXPECTED: &str = "true|false|e|é";
const INTEGER_BIT_QUERIES_DECLS: &str = r#"fn bit_byte(value: Int) -> U8 {
    return U8.from_int(value) ?? 0
}
fn bit_parameter(value: U8) -> Int {
    return (value).leading_zeros()
}
fn bit_inferred_local() -> Int {
    value := U8.from_int(13) ?? 0
    return value.leading_zeros()
}
fn integer_bit_queries_view() -> String {
    int: Int :: -1
    i8: I8 :: -2
    i16: I16 :: -32768
    i32: I32 :: 0
    u8: U8 :: 13
    u16: U16 :: 256
    u32: U32 :: 2147483648
    u64: U64 :: 255
    return "{int.count_ones()}:{int.count_zeros()}:{int.leading_zeros()}:{int.trailing_zeros()}|{i8.count_ones()}:{i8.count_zeros()}:{i8.leading_zeros()}:{i8.trailing_zeros()}|{i16.count_ones()}:{i16.count_zeros()}:{i16.leading_zeros()}:{i16.trailing_zeros()}|{i32.count_ones()}:{i32.count_zeros()}:{i32.leading_zeros()}:{i32.trailing_zeros()}|{u8.count_ones()}:{u8.count_zeros()}:{u8.leading_zeros()}:{u8.trailing_zeros()}|{u16.count_ones()}:{u16.count_zeros()}:{u16.leading_zeros()}:{u16.trailing_zeros()}|{u32.count_ones()}:{u32.count_zeros()}:{u32.leading_zeros()}:{u32.trailing_zeros()}|{u64.count_ones()}:{u64.count_zeros()}:{u64.leading_zeros()}:{u64.trailing_zeros()}|{bit_byte(13).leading_zeros()}|{bit_parameter(13)}|{bit_inferred_local()}"
}"#;
const INTEGER_BIT_QUERIES_EXPECTED: &str =
    "64:0:0:0|7:1:0:1|1:15:0:15|0:32:32:32|3:5:4:0|1:15:7:8|1:31:0:31|8:56:56:0|4|4|4";
const BYTE_BUFFER_DECLS: &str = r#"fn byte_buffer_view() -> String {
    buffer := ByteBuffer.new()
    empty_before :: buffer.is_empty()
    buffer.write_u8(18)
    buffer.write_u16_le(13398)
    buffer.write_u16_be(30874)
    buffer.write_u32_le(16909060)
    buffer.write_u32_be(84281096)
    buffer.write_u64_le(72623859790382856)
    buffer.write_u64_be(1230066625199609624)
    buffer.write_bytes([9, 10])
    length :: buffer.len()
    bytes :: buffer.to_bytes()
    buffer.clear()
    from := ByteBuffer.from([255, 0])
    return "{empty_before}|{length}|{bytes}|{buffer.is_empty()}|{buffer.len()}|{from.to_bytes()}|{from.len()}"
}"#;
const BYTE_BUFFER_EXPECTED: &str = "true|31|[18, 86, 52, 120, 154, 4, 3, 2, 1, 5, 6, 7, 8, 8, 7, 6, 5, 4, 3, 2, 1, 17, 18, 19, 20, 21, 22, 23, 24, 9, 10]|true|0|[255, 0]|2";
const DEQUE_DECLS: &str = r#"fn deque_view() -> String {
    deque: Deque<Int> := Deque.new()
    empty_before :: deque.is_empty()
    missing_front :: deque.peek_front() ?? -1
    missing_back :: deque.peek_back() ?? -1
    deque.push_back(2)
    deque.push_front(1)
    deque.push_back(3)
    length :: deque.len()
    front :: deque.peek_front() ?? -1
    back :: deque.peek_back() ?? -1
    popped_front :: deque.pop_front() ?? -1
    popped_back :: deque.pop_back() ?? -1
    remaining :: deque.peek_front() ?? -1
    deque.clear()
    empty_pop :: deque.pop_front() ?? -1
    return "{empty_before}|{missing_front}|{missing_back}|{length}|{front}|{back}|{popped_front}|{popped_back}|{remaining}|{deque.is_empty()}|{deque.len()}|{empty_pop}"
}"#;
const DEQUE_EXPECTED: &str = "true|-1|-1|3|1|3|1|3|2|true|0|-1";
const LRU_DECLS: &str = r#"fn lru_view() -> String {
    cache: Lru<String, Int> := Lru.new(2)
    empty_before :: cache.is_empty()
    first :: cache.add("a", 1) ?? -1
    added_b :: cache.add_new("b", 2)
    duplicate_b :: cache.add_new("b", 99)
    got_a :: cache.get("a") ?? -1
    displaced_a :: cache.add("a", 10) ?? -1
    evicted :: cache.add("c", 3) ?? -1
    keys :: cache.keys()
    removed_a :: cache.remove("a") ?? -1
    missing :: cache.remove("missing") ?? -1
    length :: cache.len()
    cache.clear()
    zero: Lru<String, Int> := Lru.new(-2)
    zero_add :: zero.add("x", 7) ?? -1
    zero_add_new :: zero.add_new("x", 7)
    return "{empty_before}|{cache.capacity()}|{first}|{added_b}|{duplicate_b}|{got_a}|{displaced_a}|{evicted}|{cache.has_key("b")}|{keys}|{removed_a}|{missing}|{length}|{cache.is_empty()}|{zero.capacity()}|{zero_add}|{zero_add_new}|{zero.len()}"
}"#;
const LRU_EXPECTED: &str = "true|2|-1|true|false|1|1|-1|false|[c, a]|10|-1|1|true|0|-1|false|0";
const MAP_DECLS: &str = r#"fn add_map(values: &[String: Int], key: String, value: Int) -> Int {
    return values.add(key, value) ?? -1
}
fn counted_map(hits: &Int) -> [String: Int] {
    hits += 1
    return ["a": 1]
}
fn map_view() -> String {
    values: [String: Int] := ["b": 2, "a": 1]
    empty_before :: values.is_empty()
    fresh_c :: values.add("c", 3) ?? -1
    displaced_b :: add_map(&values, "b", 20)
    added_d :: values.add_new("d", 4)
    duplicate_a :: values.add_new("a", 99)
    seen: [String] := []
    values.each((key, value) => {
        require((key == "a" && value == 1) || (key == "b" && value == 20) || (key == "c" && value == 3) || (key == "d" && value == 4), "Map.each pair")
        seen.push(key)
    })
    keys :: values.keys()
    entries :: values.values()
    got_a :: values.get("a") ?? -1
    has_a :: values.has_key("a")
    has_z :: values.has_key("z")
    removed_c :: values.remove("c") ?? -1
    length :: values.len()
    values.clear()
    return "{empty_before}|{fresh_c}|{displaced_b}|{added_d}|{duplicate_a}|{seen}|{keys}|{entries}|{got_a}|{has_a}|{has_z}|{removed_c}|{length}|{values.is_empty()}|{values.len()}"
}"#;
const MAP_EXPECTED: &str = "false|-1|2|true|false|[a, b, c, d]|[a, b, c, d]|[1, 20, 3, 4]|1|true|false|3|3|true|0";
const POOL_DECLS: &str = r#"fn pool_view() -> String {
    pool := Pool<String>.new()
    first :: pool.add("first")
    second :: pool.add("second")
    initial :: pool.ids()
    removed :: pool.remove(first) ?? "missing"
    stale_remove :: pool.remove(first) ?? "stale"
    replacement :: pool.add("third")
    live :: pool.ids()
    replacement_value :: pool.remove(replacement) ?? "missing"
    second_value :: pool.remove(second) ?? "missing"
    return "{removed}|{stale_remove}|{initial.len()}|{initial[0] == first}|{initial[1] == second}|{replacement == first}|{live.len()}|{live[0] == replacement}|{live[1] == second}|{replacement_value}|{second_value}|{pool.ids().len()}"
}"#;
const POOL_EXPECTED: &str = "first|stale|2|true|true|false|2|true|true|third|second|0";
const INLINE_HOF_DECLS: &str = r#"fn inline_hof_view() -> String {
    values := [1, 2, 3, 4]
    each_seen: [Int] := []
    shadow := 99
    values.each((shadow: Int) => { each_seen.push(shadow) })
    predicate_seen: Set<Int> := Set.from([0])
    has_three :: values.any((n: Int) => predicate_seen.add(n) && n == 3)
    fold_seen: [Int: Int] := [0: 0]
    total :: values.fold(0, (acc: Int, n: Int) => fold_seen.add(n, n) ?? (acc + n))
    partition_seen: Set<Int> := Set.from([0])
    partition_shadow := 88
    split :: values.partition((partition_shadow: Int) => partition_seen.add(partition_shadow) && partition_shadow % 2 == 0)
    return "{each_seen}|{shadow}|{predicate_seen.len()}:{predicate_seen.has(1)}:{predicate_seen.has(2)}:{predicate_seen.has(3)}:{predicate_seen.has(4)}|{has_three}|{fold_seen.values()}|{total}|{partition_shadow}|{partition_seen.len()}:{partition_seen.has(1)}:{partition_seen.has(2)}:{partition_seen.has(3)}:{partition_seen.has(4)}|{split.false_}|{split.true_}"
}"#;
const INLINE_HOF_EXPECTED: &str =
    "[1, 2, 3, 4]|99|4:true:true:true:false|true|[0, 1, 2, 3, 4]|10|88|5:true:true:true:true|[1, 3]|[2, 4]";
const MAP_CALL_RECEIVER_DECLS: &str = r#"fn map_call_receiver_view() -> String {
    receiver_hits := 0
    found :: counted_map(&receiver_hits).has_key("a")
    return "{found}|{receiver_hits}"
}"#;
const RNG_DECLS: &str = r#"use core.random as random
fn rng_view() -> String {
    rng := random.rng(99)
    items := ["a", "b", "c", "d"]
    weights := [1.0, 2.0, 3.0, 4.0]
    deck := [1, 2, 3, 4, 5]
    int_draw :: rng.int(1, 100)
    float_draw :: rng.float()
    range_draw :: rng.float_range(-2.0, 2.0)
    coin :: rng.bool()
    chance :: rng.bool(0.25)
    normal :: rng.normal(1.0, 2.0)
    exponential :: rng.exponential(1.5)
    bytes :: rng.bytes(4)
    picked :: rng.pick(items) ?? "none"
    weighted :: rng.weighted_pick(items, weights) ?? "none"
    sample :: rng.sample(items, 2)
    rng.shuffle(&deck)
    child := rng.split()
    child_draw :: child.int(1, 100)
    after_split :: rng.int(1, 100)
    return "{int_draw}|{float_draw}|{range_draw}|{coin}|{chance}|{normal}|{exponential}|{bytes}|{picked}|{weighted}|{sample}|{deck}|{child_draw}|{after_split}"
}"#;
const RNG_EXPECTED: &str = "4|0.0316577610861849|1.3390388981797772|true|true|-0.6237918784672982|0.21210139132324568|[62, 20, 83, 254]|b|c|[c, a]|[1, 2, 5, 3, 4]|71|87";
const TESTING_FAKE_RNG_FN: &str = r#"fn testing_fake_rng_view() -> String {
    first := testing.fake_rng(99)
    second := testing.fake_rng(99)
    return "{first.int(1, 100)}|{second.int(1, 100)}|{first.float()}|{second.float()}"
}"#;
const TESTING_FAKE_RNG_EXPECTED: &str = "4|4|0.0316577610861849|0.0316577610861849";
const TESTING_FAKE_CLOCK_FN: &str = r#"fn testing_fake_clock_view() -> String {
    clock := testing.fake_clock(42)
    canonical := time.clock(42)
    initial :: clock.now()
    ticked :: clock.tick(8)
    after_tick :: clock.now()
    advanced :: clock.advance(100)
    after_advance :: clock.now()
    duration :: Duration.milliseconds(25) ?? panic("duration")
    waited :: clock.wait(duration)
    canonical_ticked :: canonical.tick(8)
    return "{initial}|{ticked}|{after_tick}|{advanced}|{after_advance}|{waited}|{clock.now()}|{canonical_ticked}|{canonical.now()}"
}"#;
const TESTING_FAKE_CLOCK_EXPECTED: &str = "42|50|50|100|100|125|125|50|50";
const TESTING_FAKE_CLOCK_WRITEBACK_DECLS: &str = r#"struct ClockHolder { clock: Clock }
fn drive(clock: &Clock) -> String {
    ticked :: clock.tick(1)
    advanced :: clock.advance(10)
    duration :: Duration.milliseconds(2) ?? panic("duration")
    waited :: clock.wait(duration)
    return "{ticked}|{advanced}|{waited}|{clock.now()}"
}
fn counted_clock(hits: &Int) -> Clock {
    hits += 1
    return testing.fake_clock(7)
}
fn testing_fake_clock_writeback_view() -> String {
    clock := testing.fake_clock(5)
    borrowed :: drive(&clock)
    holder := ClockHolder.{ clock: testing.fake_clock(5) }
    field_tick :: holder.clock.tick(2)
    field_now :: holder.clock.now()
    receiver_hits := 0
    counted_now :: counted_clock(&receiver_hits).now()
    return "{borrowed}|{clock.now()};{field_tick}|{field_now}|{counted_now}|{receiver_hits}"
}"#;
const TESTING_FAKE_CLOCK_WRITEBACK_EXPECTED: &str = "6|10|12|12|12;7|7|7|1";
const BIT_SET_DECLS: &str = r#"fn add_bit(values: &BitSet, bit: Int) -> Bool {
    return values.add(bit)
}
fn bit_set_view() -> String {
    bits: BitSet := BitSet.new()
    empty_before :: bits.is_empty()
    negative_added :: bits.add(-1)
    added_four :: bits.add(4)
    added_one :: bits.add(1)
    duplicate_four :: bits.add(4)
    param_added :: add_bit(&bits, 9)
    before_remove :: bits.to_list()
    count_before :: bits.count()
    len_before :: bits.len()
    has_four :: bits.has(4)
    bits.remove(4)
    bits.remove(-1)
    after_remove :: bits.to_list()
    bits.clear()
    return "{empty_before}|{negative_added}|{added_four}|{added_one}|{duplicate_four}|{param_added}|{before_remove}|{count_before}|{len_before}|{has_four}|{after_remove}|{bits.is_empty()}|{bits.count()}|{bits.len()}|{bits.to_list()}"
}"#;
const BIT_SET_EXPECTED: &str = "true|false|true|true|false|true|[1, 4, 9]|3|10|true|[1, 9]|true|0|0|[]";
const PRIORITY_QUEUE_DECLS: &str = r#"fn push_priority(values: &PriorityQueue<Int>, value: Int) {
    values.push(value)
}
fn counted_priority(hits: &Int) -> PriorityQueue<Int> {
    hits += 1
    return PriorityQueue.from([2, 6])
}
fn priority_queue_view() -> String {
    values: PriorityQueue<Int> := PriorityQueue.from([4, 1, 7, 3, 7])
    initial_len :: values.len()
    initial_empty :: values.is_empty()
    initial_peek :: values.peek() ?? -1
    initial_sorted :: values.to_sorted_list()
    values.push(5)
    push_priority(&values, 9)
    after_push :: values.to_sorted_list()
    after_push_len :: values.len()
    popped_nine :: values.pop() ?? -1
    popped_seven :: values.pop() ?? -1
    after_pop :: values.to_sorted_list()
    words: PriorityQueue<String> := PriorityQueue.from(["a", "z", "m"])
    empty: PriorityQueue<Int> := PriorityQueue.new()
    receiver_hits := 0
    counted_values := counted_priority(&receiver_hits)
    counted_peek :: counted_values.peek() ?? -1
    counted_values.push(8)
    values.clear()
    return "{initial_len}|{initial_empty}|{initial_peek}|{initial_sorted}|{after_push}|{after_push_len}|{popped_nine}|{popped_seven}|{after_pop}|{words.peek() ?? "none"}|{words.to_sorted_list()}|{values.is_empty()}|{values.len()}|{values.peek() ?? -1}|{empty.pop() ?? -1}|{counted_peek}|{counted_values.to_sorted_list()}|{receiver_hits}"
}"#;
const PRIORITY_QUEUE_EXPECTED: &str = "5|false|7|[7, 7, 4, 3, 1]|[9, 7, 7, 5, 4, 3, 1]|7|9|7|[7, 5, 4, 3, 1]|z|[z, m, a]|true|0|-1|-1|6|[8, 6, 2]|1";
const PRIORITY_QUEUE_CALL_RECEIVER_DECLS: &str = r#"fn priority_queue_call_receiver_view() -> String {
    receiver_hits := 0
    value :: counted_priority(&receiver_hits).peek() ?? -1
    return "{value}|{receiver_hits}"
}"#;
const SET_DECLS: &str = r#"fn set_view() -> String {
    values: Set<Int> := Set.from([3, 1, 2, 3])
    initial := values.to_list()
    initial.sort()
    initial_len :: values.len()
    initial_empty :: values.is_empty()
    had_two :: values.has(2)
    added_four :: values.add(4)
    duplicate_two :: values.add(2)
    values.remove(2)
    has_two :: values.has(2)
    current := values.to_list()
    current.sort()
    other: Set<Int> := Set.from([5, 4, 0])
    combined_values :: values.union(other)
    combined := combined_values.to_list()
    combined.sort()
    after_union := values.to_list()
    after_union.sort()
    words: Set<String> := Set.from(["z", "a", "m", "a"])
    word_list := words.to_list()
    word_list.sort()
    additional: Set<Int> := Set.from([9, 7])
    additional_added :: additional.add(8)
    additional_list := additional.to_list()
    additional_list.sort()
    values.clear()
    cleared_list := values.to_list()
    cleared_list.sort()
    return "{initial}|{initial_len}|{initial_empty}|{had_two}|{added_four}|{duplicate_two}|{has_two}|{current}|{combined}|{after_union}|{word_list}|{additional_added}|{additional_list}|{values.is_empty()}|{values.len()}|{cleared_list}"
}"#;
const SET_EXPECTED: &str = "[1, 2, 3]|3|false|true|true|false|false|[1, 3, 4]|[0, 1, 3, 4, 5]|[1, 3, 4]|[a, m, z]|true|[7, 8, 9]|true|0|[]";
const SET_CALL_RECEIVER_DECLS: &str = r#"fn add_set(values: &Set<Int>, value: Int) -> Bool {
    return values.add(value)
}
fn counted_set(hits: &Int) -> Set<Int> {
    hits += 1
    return Set.from([2, 6])
}
fn set_call_receiver_view() -> String {
    values: Set<Int> := Set.from([9, 7])
    added :: add_set(&values, 8)
    receiver_hits := 0
    value :: counted_set(&receiver_hits).has(6)
    return "{added}|{values.has(8)}|{value}|{receiver_hits}"
}"#;
const BAG_DECLS: &str = r#"enum BagToken {
    Red
    Blue
}
fn add_bag(values: &Bag<Int>, value: Int) -> Bool {
    return values.add(value)
}
fn counted_bag(hits: &Int) -> Bag<Int> {
    hits += 1
    values: Bag<Int> := Bag.new()
    values.add(6)
    return values
}
fn bag_view() -> String {
    values: Bag<Int> := Bag.new()
    empty_before :: values.is_empty()
    added_four :: values.add(4)
    duplicate_four :: values.add(4)
    added_two :: add_bag(&values, 2)
    length_before :: values.len()
    count_four_before :: values.count(4)
    has_two :: values.has(2)
    any_large :: values.any((value) => value > 3)
    any_negative :: values.any((value) => value < 0)
    values.remove(4)
    count_four_after_one :: values.count(4)
    values.remove(4)
    values.remove(99)
    words: Bag<String> := Bag.new()
    words.add("a")
    words.add("z")
    words.add("a")
    tokens: Bag<BagToken> := Bag.new()
    tokens.add(BagToken.Red)
    tokens.add(BagToken.Red)
    empty: Bag<Int> := Bag.new()
    return "{empty_before}|{added_four}|{duplicate_four}|{added_two}|{length_before}|{count_four_before}|{has_two}|{any_large}|{any_negative}|{count_four_after_one}|{values.has(4)}|{values.len()}|{values.is_empty()}|{words.count("a")}|{words.len()}|{words.any((value) => value == "z")}|{tokens.count(BagToken.Red)}|{tokens.has(BagToken.Blue)}|{empty.any((value) => value == 0)}"
}"#;
const BAG_EXPECTED: &str = "true|true|true|true|3|2|true|true|false|1|false|1|false|2|3|true|2|false|false";
const BAG_CALL_RECEIVER_DECLS: &str = r#"fn bag_call_receiver_view() -> String {
    receiver_hits := 0
    count :: counted_bag(&receiver_hits).count(6)
    return "{count}|{receiver_hits}"
}"#;
const SORTED_SET_DECLS: &str = r#"fn add_through_param(values: &SortedSet<Int>, value: Int) -> Bool {
    return values.add(value)
}
fn sorted_set_view() -> String {
    values: SortedSet<Int> := SortedSet.from([3, 1, 2, 3])
    initial :: values.to_list()
    initial_len :: values.len()
    initial_empty :: values.is_empty()
    first :: values.first() ?? -1
    last :: values.last() ?? -1
    had_two :: values.has(2)
    added_four :: values.add(4)
    duplicate_two :: values.add(2)
    values.remove(2)
    has_two :: values.has(2)
    current :: values.to_list()
    other: SortedSet<Int> := SortedSet.from([5, 4, 0])
    combined :: values.union(other).to_list()
    after_union :: values.to_list()
    words: SortedSet<String> := SortedSet.from(["z", "a", "m", "a"])
    through_param: SortedSet<Int> := SortedSet.from([9, 7])
    param_added :: add_through_param(&through_param, 8)
    values.clear()
    empty: SortedSet<Int> := SortedSet.new()
    return "{initial}|{initial_len}|{initial_empty}|{first}|{last}|{had_two}|{added_four}|{duplicate_two}|{has_two}|{current}|{combined}|{after_union}|{words.to_list()}|{param_added}|{through_param.to_list()}|{values.is_empty()}|{values.len()}|{values.first() ?? -1}|{empty.is_empty()}"
}"#;
const SORTED_SET_EXPECTED: &str = "[1, 2, 3]|3|false|1|3|true|true|false|false|[1, 3, 4]|[0, 1, 3, 4, 5]|[1, 3, 4]|[a, m, z]|true|[7, 8, 9]|true|0|-1|true";
const SORTED_SET_FIELD_DECLS: &str = r#"struct SortedSetHolder { values: SortedSet<Int> }
fn sorted_set_field_view() -> String {
    values: SortedSet<Int> := SortedSet.from([9, 7])
    holder := SortedSetHolder.{ values: values }
    added :: holder.values.add(8)
    return "{added}|{holder.values.to_list()}"
}"#;
const SORTED_SET_FIELD_EXPECTED: &str = "true|[7, 8, 9]";

fn exact_values(inputs: &[&str]) -> Vec<String> {
    let output = run_transcript(inputs, None);
    assert!(!output.contains("error ["), "transcript failed:\n{output}");
    output
        .lines()
        .filter(|line| line.contains(" : "))
        .map(str::to_string)
        .collect()
}

#[test]
fn public_transcript_covers_remaining_core_pure_families() {
    let values = exact_values(&[
        "use core.mime as mime",
        "use core.time as time",
        "use core.time.date as date",
        "use core.time.datetime as datetime",
        "use core.science.measurement as measurement",
        "mime.from_extension(\".PNG\") ?? \"none\"",
        "mime.extension(\"Text/HTML; charset=UTF-8\") ?? \"none\"",
        "mime.parse(\"Text/HTML; charset=UTF-8\")",
        "time.period(1, 2, 3)",
        "time.period_days(4)",
        "time.period_months(5)",
        "time.period_years(6)",
        "time.parse_time(\"23:59:58\")",
        "time.parse_rfc3339(\"2024-02-29T12:34:56+02:30\")",
        "date.new(2024, 13, 40)",
        "date.parse(\"2024-02-29\")",
        "datetime.from_timestamp(-1)",
        "time.from_unix_ms(-1)",
        "measurement.from(12.5, 0.25).value()",
    ]);
    assert_eq!(
        values,
        [
            "\"image/png\" : String",
            "\"html\" : String",
            "Mime(top: text, sub: html, params: [[charset, UTF-8]]) : Result",
            "Period(years: 1, months: 2, days: 3) : Period",
            "Period(years: 0, months: 0, days: 4) : Period",
            "Period(years: 0, months: 5, days: 0) : Period",
            "Period(years: 6, months: 0, days: 0) : Period",
            "LocalTime(hour: 23, minute: 59, second: 58) : Result",
            "DateTime(secs: 1709201096) : Result",
            "LocalDate(year: 2024, month: 12, day: 31) : LocalDate",
            "LocalDate(year: 2024, month: 2, day: 29) : Result",
            "DateTime(secs: -1) : DateTime",
            "DateTime(secs: -1) : DateTime",
            "12.5 : Float",
        ]
    );
}

#[test]
fn public_transcript_covers_integer_bit_queries_exactly() {
    let values = exact_values(&[
        INTEGER_BIT_QUERIES_DECLS,
        "integer_bit_queries_view()",
    ]);
    assert_eq!(
        values,
        [format!("\"{INTEGER_BIT_QUERIES_EXPECTED}\" : String")]
    );
}

#[test]
fn primitive_static_stringification_is_rejected_by_sema() {
    let source = r#"fn run() {
    print(Bool.to_string())
    print(Char.to_string())
}"#;
    let diagnostics = jet::compile(source)
        .expect_err("primitive stringification belongs to values, not static type names");
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.what.as_str()))
            .collect::<Vec<_>>(),
        [
            ("E0102", "`Bool` has no static method `to_string`"),
            ("E0102", "`Char` has no static method `to_string`"),
        ]
    );
}

#[test]
fn public_transcript_keeps_checked_types_in_imported_functions() {
    let values = exact_values(&[
        "use core.math as math",
        r#"fn imported_g(x: U8) -> Int {
    y :: math.abs(-1.0)
    return (x).leading_zeros()
}"#,
        "imported_g(13)",
    ]);
    assert_eq!(values, ["4 : Int"]);
}

#[test]
fn public_transcript_composes_email_and_codecs_exactly() {
    let values = exact_values(&[
        "use core.email as email",
        "use core.encoding.xml as xml",
        "sender :: email.address(\"Mara <mara@example.com>\") ?? panic(\"sender\")",
        "recipient :: email.address(\"ada@example.net\") ?? panic(\"recipient\")",
        "attachment :: email.attachment(\"note.txt\", \"Text/Plain\", [104, 105]) ?? panic(\"attachment\")",
        "message :: email.message(sender, [recipient], [], \"Hello\", \"body\", \"\", [attachment]) ?? panic(\"message\")",
        "email.envelope(sender, [recipient])",
        "fn serialized(message: Message) -> Bool {\n    if email.serialize(message) == {\n        Ok(_) -> return true\n        Err(_) -> return false\n    }\n    return false\n}",
        "serialized(message)",
        "xml.canonical(xml.parse(\"<r b=\\\"2\\\" a=\\\"1\\\"><x/></r>\") ?? panic(\"xml\"), xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) ?? panic(\"canonical\")",
    ]);
    assert_eq!(
        values,
        [
            "Envelope(from: Address(display: Mara, mailbox: mara@example.com), recipients: [Address(display: null, mailbox: ada@example.net)]) : Result",
            "true : Bool",
            "\"<r a=\"1\" b=\"2\"><x></x></r>\" : String",
        ]
    );
}

#[test]
fn public_transcript_covers_civil_and_measurement_value_methods_exactly() {
    let values = exact_values(&[
        "use core.time as time",
        "use core.time.date as date",
        "use core.time.datetime as datetime",
        CIVIL_FN,
        CIVIL_EXPR,
        "use core.science.measurement as measurement",
        MEASUREMENT_FN,
        MEASUREMENT_EXPR,
    ]);
    assert_eq!(
        values,
        [
            "\"2|4|60|9|2024-03-01|2025-02-28|28|2024-03-31|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|0|-60|0\" : String",
            "\"3.0|5.0|3.0|5.0|0.0|9.0|4.0|0.0\" : String",
        ]
    );
}

#[test]
fn public_transcript_covers_lru_methods_exactly() {
    let values = exact_values(&[LRU_DECLS, "lru_view()"]);
    assert_eq!(values, [format!("\"{LRU_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_covers_map_methods_exactly() {
    let values = exact_values(&[MAP_DECLS, "map_view()"]);
    assert_eq!(values, [format!("\"{MAP_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_preserves_sequential_inline_hof_mutations_exactly() {
    let values = exact_values(&[INLINE_HOF_DECLS, "inline_hof_view()"]);
    assert_eq!(
        values,
        [format!("\"{INLINE_HOF_EXPECTED}\" : String")]
    );
}

#[test]
fn public_transcript_comptime_only_evaluates_map_call_receiver_once() {
    let values = exact_values(&[
        MAP_DECLS,
        MAP_CALL_RECEIVER_DECLS,
        "map_call_receiver_view()",
    ]);
    assert_eq!(values, ["\"true|1\" : String"]);
}

#[test]
fn public_transcript_covers_deque_methods_exactly() {
    let values = exact_values(&[DEQUE_DECLS, "deque_view()"]);
    assert_eq!(values, [format!("\"{DEQUE_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_covers_bit_set_methods_exactly() {
    let values = exact_values(&[BIT_SET_DECLS, "bit_set_view()"]);
    assert_eq!(values, [format!("\"{BIT_SET_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_covers_testing_fake_rng_exactly() {
    let values = exact_values(&[
        "use core.testing as testing",
        TESTING_FAKE_RNG_FN,
        "testing_fake_rng_view()",
    ]);
    assert_eq!(
        values,
        [format!("\"{TESTING_FAKE_RNG_EXPECTED}\" : String")]
    );
}

#[test]
fn public_transcript_covers_testing_fake_clock_exactly() {
    let values = exact_values(&[
        "use core.testing as testing",
        "use core.time as time",
        TESTING_FAKE_CLOCK_FN,
        "testing_fake_clock_view()",
    ]);
    assert_eq!(
        values,
        [format!("\"{TESTING_FAKE_CLOCK_EXPECTED}\" : String")]
    );
}

#[test]
fn public_transcript_covers_testing_fake_clock_writeback_exactly() {
    let values = exact_values(&[
        "use core.testing as testing",
        TESTING_FAKE_CLOCK_WRITEBACK_DECLS,
        "testing_fake_clock_writeback_view()",
    ]);
    assert_eq!(
        values,
        [format!("\"{TESTING_FAKE_CLOCK_WRITEBACK_EXPECTED}\" : String")]
    );
}

#[test]
fn public_transcript_covers_priority_queue_methods_exactly() {
    let values = exact_values(&[PRIORITY_QUEUE_DECLS, "priority_queue_view()"]);
    assert_eq!(
        values,
        [format!("\"{PRIORITY_QUEUE_EXPECTED}\" : String")]
    );
}

#[test]
fn public_transcript_comptime_only_evaluates_priority_queue_call_receiver_once() {
    // The public comptime/REPL evaluator supports this read-only call receiver
    // exactly once. TIR/AOT currently rejects the same nested receiver shape.
    let values = exact_values(&[
        PRIORITY_QUEUE_DECLS,
        PRIORITY_QUEUE_CALL_RECEIVER_DECLS,
        "priority_queue_call_receiver_view()",
    ]);
    assert_eq!(values, ["\"6|1\" : String"]);
}

#[test]
fn public_transcript_covers_set_methods_exactly() {
    let values = exact_values(&[SET_DECLS, "set_view()"]);
    assert_eq!(values, [format!("\"{SET_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_comptime_only_evaluates_set_call_receiver_once() {
    // Raw Set.to_list order is intentionally unspecified. This direct read-only
    // call receiver instead proves exact public comptime evaluation count.
    let values = exact_values(&[
        SET_DECLS,
        SET_CALL_RECEIVER_DECLS,
        "set_call_receiver_view()",
    ]);
    assert_eq!(values, ["\"true|true|true|1\" : String"]);
}

#[test]
fn public_transcript_covers_bag_methods_exactly() {
    let values = exact_values(&[BAG_DECLS, "bag_view()"]);
    assert_eq!(values, [format!("\"{BAG_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_comptime_only_evaluates_bag_call_receiver_once() {
    // The public comptime/REPL path supports this direct call receiver. TIR/AOT
    // retains the existing nested-receiver boundary in method_calls.rs.
    let values = exact_values(&[
        BAG_DECLS,
        BAG_CALL_RECEIVER_DECLS,
        "bag_call_receiver_view()",
    ]);
    assert_eq!(values, ["\"1|1\" : String"]);
}

#[test]
fn public_transcript_covers_sorted_set_methods_exactly() {
    let values = exact_values(&[SORTED_SET_DECLS, "sorted_set_view()"]);
    assert_eq!(values, [format!("\"{SORTED_SET_EXPECTED}\" : String")]);
}

#[test]
fn public_transcript_covers_sorted_set_field_writeback_exactly() {
    // Direct field receivers work in the public comptime path. Generated-Rust
    // lowering for this source shape remains a pre-existing TIR boundary.
    let values = exact_values(&[SORTED_SET_FIELD_DECLS, "sorted_set_field_view()"]);
    assert_eq!(
        values,
        [format!("\"{SORTED_SET_FIELD_EXPECTED}\" : String")]
    );
}

fn parity_source(expression: &str, imports: &str) -> String {
    format!(
        "{imports}\ncomptime expected = {expression}\n\nfn run() {{\n    actual :: {expression}\n    print(\"{{expected}}\")\n    print(\"{{actual}}\")\n}}\n"
    )
}

fn rustc_aot_stdout(label: &str, source: &str) -> String {
    assert!(common::have_rustc(), "{label} requires rustc");
    let compiled = jet::Driver::compile_generated_src(
        source,
        "comptime_core_pure_parity.jet",
        jet::Sema::CompileMode::Run,
    )
    .unwrap_or_else(|diags| {
        panic!(
            "{label} front-end failure:\n{}",
            jet::render_diagnostics("comptime_core_pure_parity.jet", source, &diags)
        )
    });
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = common::unique_tmp(&format!("jet_core_pure_{id}"));
    fs::create_dir_all(&dir).unwrap();
    let rust = dir.join("main.rs");
    let binary = dir.join("main");
    fs::write(&rust, &compiled.rust).unwrap();
    let mut rustc = Command::new("rustc");
    rustc
        .args(["--edition", "2021"])
        .arg(&rust)
        .arg("-o")
        .arg(&binary);
    if let Some(link) = &compiled.ffi {
        rustc
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for directory in link.dependency_dirs().filter(|directory| directory.is_dir()) {
            rustc
                .arg("-L")
                .arg(format!("dependency={}", directory.display()));
        }
    }
    let built = rustc.output().unwrap();
    assert!(
        built.status.success(),
        "rustc rejected {label}:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(binary).output().unwrap();
    assert!(run.status.success(), "{label} runtime failed");
    String::from_utf8(run.stdout).unwrap()
}

fn check_aot_comptime(label: &str, source: &str) -> String {
    let output = rustc_aot_stdout(label, source);
    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{label} emitted unexpected output: {lines:?}");
    assert_eq!(lines[0], lines[1], "{label} comptime/AOT divergence");
    lines[0].to_string()
}

fn check_dev_tiers(label: &str, source: &str, expected: &str) {
    check_dev_tiers_with_boundary(label, source, expected, false);
}

fn check_dev_tiers_with_boundary(
    label: &str,
    source: &str,
    expected: &str,
    force_interpreter: bool,
) {
    check_dev_tier_output(
        label,
        source,
        &format!("{expected}\n{expected}\n"),
        force_interpreter,
    );
}

fn check_dev_tier_output(
    label: &str,
    source: &str,
    expected: &str,
    force_interpreter: bool,
) {
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = common::unique_tmp(&format!("jet_core_pure_dev_{id}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{label}.jet"));
    fs::write(&path, source).unwrap();
    let path = path.to_string_lossy();
    for (tier, interpreter_only) in [("interpreter", true), ("default-dev", false)] {
        match dev_iteration(&path, force_interpreter && interpreter_only, interpreter_only) {
            RunOutcome::Ran { stdout, stderr, exit_code } => {
                assert_eq!(exit_code, 0, "{label} {tier} exit");
                assert_eq!(stderr, "", "{label} {tier} stderr");
                assert_eq!(stdout, expected, "{label} {tier} stdout");
            }
            RunOutcome::Problems(diags) => panic!("{label} {tier} failed: {diags:?}"),
        }
    }
}

#[test]
fn rustc_backed_aot_comptime_differentials_cover_return_shapes() {
    let cases = [
        (
            "option/string",
            parity_source("mime.extension(\"image/png\") ?? \"none\"", "use core.mime as mime"),
        ),
        (
            "result/bytes",
            parity_source(
                "email_wire()",
                "use core.email as email\nuse core.encoding.hex as hex\nfn email_wire() -> String {\n    message :: email.message(email.address(\"a@example.com\") ?? panic(\"a\"), [email.address(\"b@example.com\") ?? panic(\"b\")], [], \"s\", \"body\", \"\", []) ?? panic(\"m\")\n    return hex.encode(email.serialize(message) ?? panic(\"serialize\"))\n}",
            ),
        ),
        (
            "result/string",
            parity_source(
                "xml_canonical()",
                "use core.encoding.xml as xml\nfn xml_canonical() -> String {\n    tree :: xml.parse(\"<r b=\\\"2\\\" a=\\\"1\\\"><x/></r>\") ?? panic(\"xml\")\n    return xml.canonical(tree, xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) ?? panic(\"canonical\")\n}",
            ),
        ),
        (
            "xml/float-shape-error",
            parity_source(
                "xml_shape_reason(DataTree.Float(1.5))",
                "use core.encoding.xml as xml\nfn xml_shape_reason(tree: DataTree) -> String {\n    if xml.canonical(tree, xml.XMLCanonical.{ mode: .Inclusive11, comments: false, inclusive_prefixes: [] }) == {\n        Ok(_) -> return \"unexpected success\"\n        Err(error) -> return error.reason\n    }\n    return \"unreachable\"\n}",
            ),
        ),
        (
            "mime/observable-methods",
            parity_source(
                "mime_view()",
                "use core.mime as mime\nfn mime_view() -> String {\n    value :: mime.parse(\"Text/HTML; charset=UTF-8\") ?? panic(\"mime\")\n    return \"{value.media_type()}|{value.subtype()}|{value.essence()}|{value.param(\"charset\") ?? \"none\"}|{value.params()}|{value.to_string()}\"\n}",
            ),
        ),
        (
            "date/observable-methods",
            parity_source(
                "date_view()",
                "use core.time.date as date\nfn date_view() -> String {\n    parsed :: date.parse(\"2024-02-29\") ?? panic(\"date\")\n    clamped :: date.new(2024, 13, 40)\n    return \"{parsed.year()}-{parsed.month()}-{parsed.day()}|{parsed.to_string()}|{clamped.to_string()}\"\n}",
            ),
        ),
        (
            "time/observable-methods",
            parity_source(
                "time_view()",
                "use core.time as time\nfn time_view() -> String {\n    local :: time.parse_time(\"23:59:58\") ?? panic(\"time\")\n    datetime :: time.from_unix_ms(-1)\n    period :: time.period(1, 2, 3)\n    return \"{local.hour()}:{local.minute()}:{local.second()}|{local.to_string()}|{datetime.to_timestamp()}|{datetime.to_unix_ms()}|{period.to_string()}\"\n}",
            ),
        ),
        (
            "measurement/observable-methods",
            parity_source(
                "measurement_view()",
                "use core.science.measurement as measurement\nfn measurement_view() -> String {\n    value :: measurement.from(12.5, 0.25)\n    return \"{value.value()}|{value.uncertainty()}\"\n}",
            ),
        ),
    ];
    for (label, source) in cases {
        let _ = check_aot_comptime(label, &source);
    }
}

#[test]
fn rustc_backed_integer_bit_queries_match_all_execution_tiers_exactly() {
    let source = format!(
        "{INTEGER_BIT_QUERIES_DECLS}\nfn run() {{\n    print(\"{{integer_bit_queries_view()}}\")\n}}\n"
    );
    assert_eq!(
        rustc_aot_stdout("integer-bit-queries", &source),
        format!("{INTEGER_BIT_QUERIES_EXPECTED}\n")
    );
    check_dev_tier_output(
        "integer-bit-queries",
        &source,
        &format!("{INTEGER_BIT_QUERIES_EXPECTED}\n"),
        false,
    );
}

#[test]
fn rustc_backed_f32_math_is_native_width_across_comptime_and_dev() {
    let source = parity_source("f32_math_view()", F32_MATH_DECLS);
    assert_eq!(
        check_aot_comptime("f32/native-width-math", &source),
        F32_MATH_EXPECTED
    );
    check_dev_tiers("f32-native-width-math", &source, F32_MATH_EXPECTED);
}

#[test]
fn rustc_backed_civil_and_measurement_methods_match_comptime_exactly() {
    let civil = "2|4|60|9|2024-03-01|2025-02-28|28|2024-03-31|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|0|-60|0";
    let measurement = "3.0|5.0|3.0|5.0|0.0|9.0|4.0|0.0";
    let civil_source = parity_source(CIVIL_EXPR, CIVIL_DECLS);
    let civil_dev = "2|4|60|9|2024-03-01|2025-02-28|28|2024-02-01|Thu 2024-060|1969-12-31|23:59:59|23:59:59|1969-12-31T23:59:59Z|1969-12-31 23:59:59|-60|0";
    let civil_dev_source = parity_source("civil_dev_view()", CIVIL_DEV_DECLS);
    let measurement_source = parity_source(MEASUREMENT_EXPR, MEASUREMENT_DECLS);
    assert_eq!(
        check_aot_comptime("civil/all-deterministic-methods", &civil_source),
        civil
    );
    assert_eq!(
        check_aot_comptime("measurement/arithmetic", &measurement_source),
        measurement
    );
    check_dev_tiers("civil", &civil_dev_source, civil_dev);
    check_dev_tiers("measurement", &measurement_source, measurement);
}

#[test]
fn rustc_backed_datetime_and_measurement_display_are_exact() {
    assert_eq!(
        check_aot_comptime(
            "datetime/negative-unix-ms-display",
            &parity_source(
                "time.from_unix_ms(-1).to_string()",
                "use core.time as time",
            ),
        ),
        "1969-12-31 23:59:59 UTC"
    );
    assert_eq!(
        check_aot_comptime(
            "measurement/interpolation-display",
            "use core.science.measurement as measurement\ncomptime value = measurement.from(12.5, 0.25)\ncomptime expected = \"{value}\"\n\nfn run() {\n    actual :: measurement.from(12.5, 0.25)\n    print(expected)\n    print(actual)\n}\n",
        ),
        "12.5 ± 0.25"
    );
}

#[test]
fn rustc_backed_pivot_sum_invokes_capturing_closures_exactly() {
    assert_eq!(
        check_aot_comptime(
            "data/pivot-sum-closures",
            &parity_source(PIVOT_EXPR, PIVOT_DECLS),
        ),
        "pA|x:2:4.0:2.0|pB|y:1:5.0:5.0"
    );
}

#[test]
fn rustc_backed_scalar_value_methods_match_all_execution_tiers_exactly() {
    let source = parity_source(SCALAR_EXPR, SCALAR_DECLS);
    assert_eq!(
        check_aot_comptime("scalar/value-methods", &source),
        SCALAR_EXPECTED
    );
    check_dev_tiers("scalar", &source, SCALAR_EXPECTED);
}

#[test]
fn rustc_backed_primitive_instance_stringification_matches_all_execution_tiers_exactly() {
    let source = parity_source("primitive_instance_view()", PRIMITIVE_INSTANCE_DECLS);
    assert_eq!(
        check_aot_comptime("primitive-instance-stringification", &source),
        PRIMITIVE_INSTANCE_EXPECTED,
    );
    check_dev_tiers_with_boundary(
        "primitive-instance-stringification",
        &source,
        PRIMITIVE_INSTANCE_EXPECTED,
        true,
    );
}

#[test]
fn rustc_backed_byte_buffer_matches_all_execution_tiers_exactly() {
    let source = parity_source("byte_buffer_view()", BYTE_BUFFER_DECLS);
    assert_eq!(
        check_aot_comptime("byte-buffer/all-methods", &source),
        BYTE_BUFFER_EXPECTED
    );
    check_dev_tiers("byte-buffer", &source, BYTE_BUFFER_EXPECTED);
}

#[test]
fn rustc_backed_lru_matches_all_execution_tiers_exactly() {
    let source = parity_source("lru_view()", LRU_DECLS);
    assert_eq!(check_aot_comptime("lru/all-methods", &source), LRU_EXPECTED);
    check_dev_tiers("lru", &source, LRU_EXPECTED);
}

#[test]
fn rustc_backed_map_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    let source = parity_source("map_view()", MAP_DECLS);
    assert_eq!(check_aot_comptime("map/all-methods", &source), MAP_EXPECTED);
    // Map remains outside the resident JIT subset: forced interpreter proves
    // its comptime execution, while default dev proves the normal AOT fallback.
    check_dev_tiers_with_boundary("map", &source, MAP_EXPECTED, true);
}

#[test]
fn rustc_backed_pool_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    let source = parity_source("pool_view()", POOL_DECLS);
    assert_eq!(check_aot_comptime("pool/generations", &source), POOL_EXPECTED);
    check_dev_tiers_with_boundary("pool", &source, POOL_EXPECTED, true);
}

#[test]
fn rustc_backed_sequential_inline_hofs_match_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly(
) {
    let source = parity_source("inline_hof_view()", INLINE_HOF_DECLS);
    assert_eq!(
        check_aot_comptime("sequential-inline-hof/writeback", &source),
        INLINE_HOF_EXPECTED
    );
    // Mutable captures remain outside the resident JIT subset: force the
    // interpreter explicitly, then prove default dev's normal AOT fallback.
    check_dev_tiers_with_boundary("sequential-inline-hof", &source, INLINE_HOF_EXPECTED, true);
}

#[test]
fn rustc_backed_deque_matches_all_execution_tiers_exactly() {
    let source = parity_source("deque_view()", DEQUE_DECLS);
    assert_eq!(check_aot_comptime("deque/all-methods", &source), DEQUE_EXPECTED);
    check_dev_tiers("deque", &source, DEQUE_EXPECTED);
}

#[test]
fn rustc_backed_bit_set_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    let source = parity_source("bit_set_view()", BIT_SET_DECLS);
    assert_eq!(
        check_aot_comptime("bit-set/all-methods", &source),
        BIT_SET_EXPECTED
    );
    check_dev_tiers_with_boundary("bit-set", &source, BIT_SET_EXPECTED, true);
}

#[test]
fn rustc_backed_priority_queue_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    // TIR currently rejects a nested call receiver before rustc. Materializing
    // that producer once into a named place proves the supported evaluation,
    // mutation, and caller-visible writeback path without hiding the boundary.
    let source = parity_source("priority_queue_view()", PRIORITY_QUEUE_DECLS);
    assert_eq!(
        check_aot_comptime("priority-queue/all-methods", &source),
        PRIORITY_QUEUE_EXPECTED
    );
    check_dev_tiers_with_boundary(
        "priority-queue",
        &source,
        PRIORITY_QUEUE_EXPECTED,
        true,
    );
}

#[test]
fn rustc_backed_set_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    // HashSet iteration order is unspecified, so the Jet source sorts lists
    // after to_list on both sides. Set-typed helper functions and nested call
    // receivers remain TIR boundaries and are covered by the transcript above.
    let source = parity_source("set_view()", SET_DECLS);
    assert_eq!(check_aot_comptime("set/all-methods", &source), SET_EXPECTED);
    check_dev_tiers_with_boundary("set", &source, SET_EXPECTED, true);
}

#[test]
fn rustc_backed_bag_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    let source = parity_source("bag_view()", BAG_DECLS);
    assert_eq!(check_aot_comptime("bag/all-methods", &source), BAG_EXPECTED);
    check_dev_tiers_with_boundary("bag", &source, BAG_EXPECTED, true);
}

#[test]
fn rustc_backed_sorted_set_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly() {
    // Mutating a SortedSet returned directly by a call remains the existing
    // non-place receiver boundary. A named `&SortedSet` receiver proves one
    // mutating call result and its caller-visible writeback exactly.
    let source = parity_source("sorted_set_view()", SORTED_SET_DECLS);
    assert_eq!(
        check_aot_comptime("sorted-set/all-methods", &source),
        SORTED_SET_EXPECTED
    );
    check_dev_tiers_with_boundary("sorted-set", &source, SORTED_SET_EXPECTED, true);
}

#[test]
fn rustc_backed_seeded_rng_methods_match_all_execution_tiers_exactly() {
    let source = parity_source("rng_view()", RNG_DECLS);
    assert_eq!(check_aot_comptime("rng/all-methods", &source), RNG_EXPECTED);
    // `core.random` keeps its ambient-effect E2201 boundary. `try_anyway`
    // proves the seeded handle itself is interpreter-resident; default dev
    // proves its normal AOT fallback remains byte-identical.
    check_dev_tiers_with_boundary("rng", &source, RNG_EXPECTED, true);
}

#[test]
fn rustc_backed_testing_fake_rng_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly(
) {
    let declarations = format!("use core.testing as testing\n{TESTING_FAKE_RNG_FN}");
    let source = parity_source("testing_fake_rng_view()", &declarations);
    assert_eq!(
        check_aot_comptime("testing/fake-rng", &source),
        TESTING_FAKE_RNG_EXPECTED
    );
    // Rng remains outside the resident JIT subset: force the interpreter,
    // then prove default dev's ordinary AOT fallback.
    check_dev_tiers_with_boundary(
        "testing-fake-rng",
        &source,
        TESTING_FAKE_RNG_EXPECTED,
        true,
    );
}

#[test]
fn rustc_backed_testing_fake_clock_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly(
) {
    let declarations = format!(
        "use core.testing as testing\nuse core.time as time\n{TESTING_FAKE_CLOCK_FN}"
    );
    let source = parity_source("testing_fake_clock_view()", &declarations);
    assert_eq!(
        check_aot_comptime("testing/fake-clock", &source),
        TESTING_FAKE_CLOCK_EXPECTED
    );
    // Clock mutation remains outside the resident JIT subset: force the
    // interpreter, then prove default dev's ordinary AOT fallback.
    check_dev_tiers_with_boundary(
        "testing-fake-clock",
        &source,
        TESTING_FAKE_CLOCK_EXPECTED,
        true,
    );
}

#[test]
fn rustc_backed_testing_fake_clock_writeback_matches_aot_comptime_forced_interpreter_and_default_dev_fallback_exactly(
) {
    let declarations = format!(
        "use core.testing as testing\n{TESTING_FAKE_CLOCK_WRITEBACK_DECLS}"
    );
    let source = parity_source("testing_fake_clock_writeback_view()", &declarations);
    assert_eq!(
        check_aot_comptime("testing/fake-clock-writeback", &source),
        TESTING_FAKE_CLOCK_WRITEBACK_EXPECTED
    );
    // Clock mutation remains outside the resident JIT subset: force the
    // interpreter, then prove default dev's ordinary AOT fallback.
    check_dev_tiers_with_boundary(
        "testing-fake-clock-writeback",
        &source,
        TESTING_FAKE_CLOCK_WRITEBACK_EXPECTED,
        true,
    );
}

const LINALG_DECLS: &str = r#"fn linalg_view() -> String {
    a: Vec3 :: Vec3(1.0, 2.0, 3.0)
    b: Vec3 :: Vec3(4.0, 5.0, 6.0)
    sum: Vec3 :: a + b
    crossed: Vec3 :: a.cross(b)
    scale: Mat3 :: Mat3(2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0)
    scaled: Vec3 :: scale * Vec3(1.0, 2.0, 3.0)
    v: F32x4 :: F32x4(1.0, 2.0, 3.0, 4.0)
    w: F32x4 :: F32x4(10.0, 20.0, 30.0, 40.0)
    added: F32x4 :: v + w
    d: F64x2 :: F64x2.from_array([1.5, 2.5])
    return "{sum.to_array()}|{a.dot(b)}|{crossed.to_array()}|{Vec3(0.0, 3.0, 4.0).length()}|{Vec3(0.0, 3.0, 4.0).normalize().to_array()}|{scaled.to_array()}|{scale.matmul(scale).to_array()}|{added.to_array()}|{(v * w).to_array()}|{F32x4.splat(7.0).to_array()}|{v[2]}|{v.sum()}|{v.product()}|{v.min()}|{v.max()}|{v.reduce(@Max)}|{v.reduce(@Mul)}|{(d + d).to_array()}|{d.sum()}|{d.product()}|{d.min()}|{d.max()}"
}"#;
const LINALG_EXPECTED: &str = "[5.0, 7.0, 9.0]|32.0|[-3.0, 6.0, -3.0]|5.0|[0.0, 0.6, 0.8]|[2.0, 4.0, 6.0]|[4.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 4.0]|[11.0, 22.0, 33.0, 44.0]|[10.0, 40.0, 90.0, 160.0]|[7.0, 7.0, 7.0, 7.0]|3.0|10.0|24.0|1.0|4.0|4.0|24.0|[3.0, 5.0]|4.0|3.75|1.5|2.5";

const OVERFLOW_DECLS: &str = r#"fn overflow_view() -> String {
    hi: U8 :: 200
    lo: U8 :: 100
    fallback: U8 :: 0
    wrapped :: wrapping(hi + lo)
    saturated :: saturating(hi + lo)
    checked_miss :: checked(hi + lo) ?? fallback
    scratch :: 1
    consume(scratch)
    return "{wrapped}|{saturated}|{checked_miss}"
}"#;
const OVERFLOW_EXPECTED: &str = "44|255|0";
const EXPECT_DECLS: &str = r#"fn expect_view() -> String {
    holder :: expect("ok")
    consume(holder)
    return "expect-ok"
}"#;
const EXPECT_EXPECTED: &str = "expect-ok";

#[test]
fn rustc_backed_linalg_simd_matches_all_execution_tiers_exactly() {
    let source = parity_source("linalg_view()", LINALG_DECLS);
    assert_eq!(
        check_aot_comptime("linalg/simd-direct", &source),
        LINALG_EXPECTED
    );
    check_dev_tiers("linalg-simd-direct", &source, LINALG_EXPECTED);
}

#[test]
fn rustc_backed_overflow_opt_and_consume_match_aot_comptime() {
    let source = parity_source("overflow_view()", OVERFLOW_DECLS);
    assert_eq!(
        check_aot_comptime("overflow/opt-ins", &source),
        OVERFLOW_EXPECTED
    );
    check_dev_tiers("overflow-opt-ins", &source, OVERFLOW_EXPECTED);
}

#[test]
fn public_transcript_covers_linalg_overflow_and_expect_exactly() {
    let values = exact_values(&[LINALG_DECLS, "linalg_view()"]);
    assert_eq!(values, [format!("\"{LINALG_EXPECTED}\" : String")]);
    let values = exact_values(&[OVERFLOW_DECLS, "overflow_view()"]);
    assert_eq!(values, [format!("\"{OVERFLOW_EXPECTED}\" : String")]);
    // `expect` is test-harness-shaped in AOT (JetExpect + snapshot); comptime
    // constructs the wrapper and `consume` discards it — prove via REPL only.
    let values = exact_values(&[EXPECT_DECLS, "expect_view()"]);
    assert_eq!(values, [format!("\"{EXPECT_EXPECTED}\" : String")]);
}

const SOLVER_FN: &str = r#"fn solver_view() -> String {
    ok_solver := solve.Solver.new(7)
    ok_solver.require(true)
    ok_solver.require(1 == 1)
    bad := solve.Solver.new(42)
    bad.require(true)
    bad.require(false)
    bad.require(true)
    return "{ok_solver.status()}|{ok_solver.failure_count()}|{bad.status()}|{bad.failure_count()}"
}"#;
const SOLVER_DECLS: &str = "use core.solve as solve\nfn solver_view() -> String {\n    ok_solver := solve.Solver.new(7)\n    ok_solver.require(true)\n    ok_solver.require(1 == 1)\n    bad := solve.Solver.new(42)\n    bad.require(true)\n    bad.require(false)\n    bad.require(true)\n    return \"{ok_solver.status()}|{ok_solver.failure_count()}|{bad.status()}|{bad.failure_count()}\"\n}";
const SOLVER_EXPECTED: &str = "ok|0|failed|1";

#[test]
fn rustc_backed_solver_matches_aot_comptime_and_dev_tiers() {
    let source = parity_source("solver_view()", SOLVER_DECLS);
    assert_eq!(
        check_aot_comptime("solver/require-status", &source),
        SOLVER_EXPECTED
    );
    check_dev_tiers("solver-require-status", &source, SOLVER_EXPECTED);
}

#[test]
fn public_transcript_covers_solver_exactly() {
    let values = exact_values(&[
        "use core.solve as solve",
        SOLVER_FN,
        "solver_view()",
    ]);
    assert_eq!(values, [format!("\"{SOLVER_EXPECTED}\" : String")]);
}

const ARCHIVE_DECLS: &str = r#"use core.archive as archive
fn archive_view() -> String {
    bytes: [U8] :: [72, 101, 108, 108, 111]
    zipped :: archive.zip_compress("hello.txt", bytes)
    empty: [U8] :: []
    tarred := archive.tar_add(empty, "hello.txt", bytes)
    tarred = archive.tar_add(tarred, "quote\"slash\\.txt", [74, 101, 116])
    zip_bytes :: archive.zip_decompress(zipped)
    tar_bytes :: archive.tar_get(tarred, "quote\"slash\\.txt")
    return "{zip_bytes}|{tar_bytes}|{archive.tar_names_json(tarred)}|{archive.tar_get(tarred, "missing").len()}"
}"#;
const ARCHIVE_EXPECTED: &str =
    "[72, 101, 108, 108, 111]|[74, 101, 116]|[\"hello.txt\",\"quote\\\"slash\\\\.txt\"]|0";
const ARCHIVE_INVALID_TAR_NAME_DECLS: &str = r#"use core.archive as archive
fn invalid_tar_name_view(name: String) -> String {
    empty: [U8] :: []
    valid :: archive.tar_add(empty, "keep.txt", [1])
    attempted :: archive.tar_add(valid, name, [2])
    return "{archive.tar_names_json(attempted)}|{archive.tar_get(attempted, "keep.txt")}|{archive.tar_get(attempted, name)}"
}"#;
const ARCHIVE_INVALID_TAR_NAME_EXPECTED: &str = "[\"keep.txt\"]|[1]|[]";

#[test]
fn rustc_backed_archive_matches_aot_comptime_and_dev_tiers() {
    let source = parity_source("archive_view()", ARCHIVE_DECLS);
    assert_eq!(
        check_aot_comptime("archive/all-pure-calls", &source),
        ARCHIVE_EXPECTED
    );
    check_dev_tiers("archive-all-pure-calls", &source, ARCHIVE_EXPECTED);
}

#[test]
fn archive_rejects_invalid_tar_names_across_aot_comptime_and_forced_interpreter() {
    for (label, name) in [
        ("empty", "\"\""),
        ("parent", "\"../x\""),
        ("absolute", "\"/x\""),
    ] {
        let source = parity_source(
            &format!("invalid_tar_name_view({name})"),
            ARCHIVE_INVALID_TAR_NAME_DECLS,
        );
        assert_eq!(
            check_aot_comptime(&format!("archive/invalid-tar-name/{label}"), &source),
            ARCHIVE_INVALID_TAR_NAME_EXPECTED,
        );
        check_dev_tiers_with_boundary(
            &format!("archive-invalid-tar-name-{label}"),
            &source,
            ARCHIVE_INVALID_TAR_NAME_EXPECTED,
            true,
        );
    }
}
