/* Actual native exact-Int parse/radix Result outputs and ownership. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern int64_t jet_int_parse(int64_t, int64_t, int64_t);
extern int64_t jet_int_from_radix(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_to_radix(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_compare(int64_t, int64_t);
extern int64_t jet_rt_int_from_static(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t jet_rt_string_from_static(int64_t, int64_t);
extern void jet_rt_string_drop(int64_t);
struct text { const char *data; int64_t len, capacity; };
static int64_t text(const char *value) {
    return jet_rt_string_from_static((int64_t)(intptr_t)value, (int64_t)strlen(value));
}
static void equal_text(int64_t actual, const char *expected) {
    const struct text *header = (const struct text *)(intptr_t)actual;
    assert(header->len == (int64_t)strlen(expected));
    assert(memcmp(header->data, expected, (size_t)header->len) == 0);
    jet_rt_string_drop(actual);
}
static int64_t parsed(const char *value) {
    int64_t input = text(value), ok = -7, err = -9;
    assert(jet_int_parse(input, (int64_t)(intptr_t)&ok, (int64_t)(intptr_t)&err) == 1);
    assert(err == -9);
    jet_rt_string_drop(input);
    return ok;
}
static void parse_error(const char *input, const char *expected) {
    int64_t value = text(input), ok = -7, err = -9;
    assert(jet_int_parse(value, (int64_t)(intptr_t)&ok, (int64_t)(intptr_t)&err) == 0);
    assert(ok == -7);
    equal_text(err, expected);
    jet_rt_string_drop(value);
}
static void radix_error(const char *input, int64_t radix, const char *expected) {
    int64_t value = text(input), ok = -7, err = -9;
    assert(jet_int_from_radix(value, radix, (int64_t)(intptr_t)&ok, (int64_t)(intptr_t)&err) == 0);
    assert(ok == -7);
    equal_text(err, expected);
    ok = -7; err = -9;
    if (radix < 2 || radix > 36) {
        assert(jet_int_to_radix(0, radix, (int64_t)(intptr_t)&ok, (int64_t)(intptr_t)&err) == 0);
        assert(ok == -7);
        equal_text(err, expected);
    }
    jet_rt_string_drop(value);
}
int main(void) {
    assert(parsed("\xe2\x80\x83 +42\xc2\xa0") == 42);
    assert(parsed("-0") == 0);
    assert(parsed("+000000") == 0);
    parse_error("  +_  ", "cannot parse `  +_  ` as an integer");
    parse_error("", "cannot parse `` as an integer");
    parse_error("0x10", "cannot parse `0x10` as an integer");
    parse_error("1.5", "cannot parse `1.5` as an integer");
    int64_t big = parsed("-1234567890123456789012345678901234567890");
    for (int64_t radix = 2; radix <= 36; ++radix) {
        int64_t rendered = -7, err = -9;
        assert(jet_int_to_radix(big, radix, (int64_t)(intptr_t)&rendered, (int64_t)(intptr_t)&err) == 1);
        assert(err == -9);
        int64_t roundtrip = -7;
        assert(jet_int_from_radix(rendered, radix, (int64_t)(intptr_t)&roundtrip, (int64_t)(intptr_t)&err) == 1);
        assert(err == -9 && jet_int_compare(big, roundtrip) == 0);
        jet_rt_int_drop(roundtrip);
        jet_rt_string_drop(rendered);
    }
    jet_rt_int_drop(big);
    int64_t zero_text = -7, err = -9;
    assert(jet_int_to_radix(0, 2, (int64_t)(intptr_t)&zero_text, (int64_t)(intptr_t)&err) == 1);
    assert(err == -9);
    equal_text(zero_text, "0");
    int64_t upper = text(" +Zz "), upper_value = -7;
    assert(jet_int_from_radix(upper, 36, (int64_t)(intptr_t)&upper_value, (int64_t)(intptr_t)&err) == 1);
    assert(upper_value == 1295);
    jet_rt_string_drop(upper);
    radix_error("   +  ", 10, "integer radix text is empty");
    radix_error("  +z  ", 16, "invalid base-16 integer `+z`");
    radix_error("0xff", 16, "invalid base-16 integer `0xff`");
    radix_error("1", 1, "integer radix must be between 2 and 36, got 1");
    radix_error("1", -2, "integer radix must be between 2 and 36, got -2");
    int64_t wide = parsed("9223372036854775808");
    radix_error("1", wide, "exact Int does not fit the native i64 ABI");
    jet_rt_int_drop(wide);
    puts("CHECK OK — native integer parse/radix: Unicode trim, exact limbs, bases2..36, selected-slot ownership, precise errors");
    return 0;
}
