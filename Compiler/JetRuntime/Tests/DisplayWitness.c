// Freestanding oracle client. All formatting/ownership is the native Jet
// runtime; these syscall-only test transports neither link nor use libc.
#include <stdint.h>
#include <stddef.h>
#include "benchmark_bits.h"

typedef struct { unsigned char *ptr; uint64_t len, cap; } JetCText;
extern void *jet_rt_alloc(uint64_t, uint64_t);
extern void jet_rt_free(void *, uint64_t, uint64_t);
extern JetCText *jet_rt_i64_to_string(int64_t);
extern JetCText *jet_rt_u64_to_string(uint64_t);
extern JetCText *jet_rt_int_to_string(int64_t);
extern JetCText *jet_rt_bool_to_string(uint8_t);
extern JetCText *jet_rt_char_to_string(int64_t);
extern JetCText *jet_rt_float_to_string(double);
extern JetCText *jet_rt_float_debug(double);
extern JetCText *jet_rt_float32_to_string(int64_t);
extern JetCText *jet_rt_float32_debug(int64_t);
extern int64_t jet_rt_test_f64_bits(double);
extern double jet_rt_test_f64_from_bits(int64_t);
extern void (*__init_array_start[])(void), (*__init_array_end[])(void);

static long syscall3(long number, long a, long b, long c) {
    register long result __asm__("rax") = number;
    __asm__ volatile("syscall" : "+a"(result) : "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return result;
}
static int failed;
static void write_all(long fd, const void *bytes, uint64_t count) {
    const unsigned char *at = bytes;
    while (count) {
        long n = syscall3(1, fd, (long)at, (long)count);
        if (n == -4) continue;
        if (n <= 0) { failed = 1; return; }
        at += n; count -= (uint64_t)n;
    }
}
static void release(JetCText *text) {
    if (text->len > text->cap || (!text->cap && text->ptr != (void *)1)) failed = 1;
    if (text->cap) jet_rt_free(text->ptr, text->cap, 1);
    jet_rt_free(text, 24, 8);
}
static void expect(JetCText *text, const unsigned char *expected, uint64_t length) {
    if (text->len != length) failed = 1;
    else for (uint64_t i = 0; i < length; ++i) if (text->ptr[i] != expected[i]) failed = 1;
    release(text);
}
#define EXPECT(value, text) expect(value, (const unsigned char *)(text), sizeof(text) - 1)
static void scalars(void) {
    EXPECT(jet_rt_i64_to_string(INT64_MIN), "-9223372036854775808");
    EXPECT(jet_rt_i64_to_string(INT64_MAX), "9223372036854775807");
    EXPECT(jet_rt_i64_to_string(0), "0");
    // Negative-domain digits: `%`/`/%` floor, so these pin truncated digits.
    EXPECT(jet_rt_i64_to_string(7), "7");
    EXPECT(jet_rt_i64_to_string(-7), "-7");
    EXPECT(jet_rt_i64_to_string(9), "9");
    EXPECT(jet_rt_i64_to_string(10), "10");
    EXPECT(jet_rt_i64_to_string(99), "99");
    EXPECT(jet_rt_i64_to_string(-99), "-99");
    EXPECT(jet_rt_i64_to_string(100), "100");
    EXPECT(jet_rt_i64_to_string(-100), "-100");
    EXPECT(jet_rt_i64_to_string(1234567890123), "1234567890123");
    EXPECT(jet_rt_u64_to_string(UINT64_MAX), "18446744073709551615");
    EXPECT(jet_rt_u64_to_string(UINT64_C(1) << 63), "9223372036854775808");
    EXPECT(jet_rt_u64_to_string(0), "0");
    EXPECT(jet_rt_u64_to_string(9), "9");
    EXPECT(jet_rt_u64_to_string(10), "10");
    EXPECT(jet_rt_bool_to_string(0), "false");
    EXPECT(jet_rt_bool_to_string(1), "true");
    EXPECT(jet_rt_bool_to_string(255), "true");
    EXPECT(jet_rt_char_to_string(0), "\0");
    EXPECT(jet_rt_char_to_string(13), "\r");
    EXPECT(jet_rt_char_to_string('{'), "{");
    EXPECT(jet_rt_char_to_string('}'), "}");
    EXPECT(jet_rt_char_to_string(127), "\x7f");
    EXPECT(jet_rt_char_to_string(128), "\xc2\x80");
    EXPECT(jet_rt_char_to_string(2047), "\xdf\xbf");
    EXPECT(jet_rt_char_to_string(2048), "\xe0\xa0\x80");
    EXPECT(jet_rt_char_to_string(65535), "\xef\xbf\xbf");
    EXPECT(jet_rt_char_to_string(65536), "\xf0\x90\x80\x80");
    EXPECT(jet_rt_char_to_string(1114111), "\xf4\x8f\xbf\xbf");
    EXPECT(jet_rt_char_to_string(-1), "\xef\xbf\xbd");
    EXPECT(jet_rt_char_to_string(55296), "\xef\xbf\xbd");
    EXPECT(jet_rt_char_to_string(1114112), "\xef\xbf\xbd");
    EXPECT(jet_rt_int_to_string(-INT64_C(4611686018427387904)), "-4611686018427387904");
    EXPECT(jet_rt_int_to_string(INT64_C(4611686018427387903)), "4611686018427387903");
    EXPECT(jet_rt_int_to_string(0), "0");
    EXPECT(jet_rt_int_to_string(7), "7");
    EXPECT(jet_rt_int_to_string(-7), "-7");
    EXPECT(jet_rt_int_to_string(99), "99");
    EXPECT(jet_rt_int_to_string(-99), "-99");
    // A borrowed approved base-1e9 boxed carrier. No Rust JetInt is involved.
    uint32_t limbs[] = {1, 0, 234567890, 1};
    struct { uint64_t refs, retired_next, negative; uint32_t *limbs; uint64_t len, cap; } node = {1, 0, 0, limbs, 4, 4};
    int64_t tagged = (int64_t)((uintptr_t)&node | (UINT64_C(1) << 62));
    EXPECT(jet_rt_int_to_string(tagged), "1234567890000000000000000001");
    node.negative = 1;
    EXPECT(jet_rt_int_to_string(tagged), "-1234567890000000000000000001");
    EXPECT(jet_rt_float32_to_string(INT64_C(0x80000000)), "-0");
    EXPECT(jet_rt_float32_debug(INT64_C(0x80000000)), "-0.0");
    EXPECT(jet_rt_float32_to_string(INT64_C(0x3f800000)), "1");
    EXPECT(jet_rt_float32_debug(INT64_C(0x3f800000)), "1.0");
    EXPECT(jet_rt_float32_to_string(INT64_C(0x3dcccccd)), "0.1");
    EXPECT(jet_rt_float32_debug(INT64_C(0x3dcccccd)), "0.1");
    EXPECT(jet_rt_float32_to_string(INT64_C(0x7f800000)), "inf");
    EXPECT(jet_rt_float32_debug(INT64_C(0xff800000)), "-inf");
    EXPECT(jet_rt_float32_to_string(INT64_C(0x7fc00042)), "NaN");
    EXPECT(jet_rt_float32_debug(INT64_C(0xff800001)), "NaN");
    EXPECT(jet_rt_float32_to_string((int64_t)UINT64_C(0xdeadbeef3dcccccd)), "0.1");
    EXPECT(jet_rt_float32_debug((int64_t)UINT64_C(0xdeadbeef80000000)), "-0.0");
}
static unsigned char input[4096];
static uint64_t next, available;
static int read_byte(void) {
    if (next == available) {
        long n;
        do n = syscall3(0, 0, (long)input, sizeof(input)); while (n == -4);
        if (n < 0) failed = 1;
        if (n <= 0) return -1;
        next = 0; available = (uint64_t)n;
    }
    return input[next++];
}
static void write_hex(uint64_t word) {
    unsigned char text[16];
    for (unsigned i = 0; i < 16; ++i) {
        unsigned digit = (unsigned)(word >> (4 * (15 - i))) & 15;
        text[i] = (unsigned char)(digit < 10 ? '0' + digit : 'a' + digit - 10);
    }
    write_all(1, text, sizeof(text));
}
static void benchmark(void) {
    struct { int64_t seconds, nanoseconds; } start, end;
    if (syscall3(228, 1, (long)&start, 0) != 0) { failed = 1; return; }
    uint64_t digest = UINT64_C(14695981039346656037);
    for (uint64_t iteration = 0; iteration < DISPLAY_BENCH_ITERATIONS; ++iteration) {
        union { uint64_t bits; double value; } value = { .bits = benchmark_bits[iteration % DISPLAY_BENCH_VALUES] };
        JetCText *text = jet_rt_float_to_string(value.value);
        for (uint64_t i = 0; i < text->len; ++i)
            digest = (digest ^ text->ptr[i]) * UINT64_C(1099511628211);
        digest = (digest ^ '\n') * UINT64_C(1099511628211);
        release(text);
    }
    if (syscall3(228, 1, (long)&end, 0) != 0) { failed = 1; return; }
    uint64_t elapsed = (uint64_t)((end.seconds - start.seconds) * INT64_C(1000000000) + end.nanoseconds - start.nanoseconds);
    write_all(1, "benchmark\t1000000\t", sizeof("benchmark\t1000000\t") - 1);
    write_hex(digest); write_all(1, "\t", 1);
    write_hex(elapsed); write_all(1, "\n", 1);
}

__attribute__((used, noreturn)) void display_witness_main(void) {
    for (void (**init)(void) = __init_array_start; init != __init_array_end; ++init) (*init)();
    scalars();
    for (;;) {
        unsigned char hex[16];
        uint64_t bits = 0;
        int byte = read_byte();
        if (byte < 0) break;
        if (byte == 'B') {
            if (read_byte() != '\n') failed = 1;
            else benchmark();
            break;
        }
        int narrow = byte == 'F';
        if (narrow) byte = read_byte();
        for (unsigned i = 0; i < 16; ++i) {
            if (i) byte = read_byte();
            int digit = byte >= '0' && byte <= '9' ? byte - '0' : byte >= 'a' && byte <= 'f' ? byte - 'a' + 10 : -1;
            if (digit < 0) { failed = 1; goto finished; }
            hex[i] = (unsigned char)byte;
            bits = (bits << 4) | (unsigned)digit;
        }
        if (read_byte() != '\n') { failed = 1; break; }
        JetCText *display, *debug;
        if (narrow) {
            display = jet_rt_float32_to_string((int64_t)bits);
            debug = jet_rt_float32_debug((int64_t)bits);
        } else {
            union { uint64_t bits; double value; } value = { .bits = bits };
            union { double value; uint64_t bits; } back = { .value = jet_rt_test_f64_from_bits((int64_t)bits) };
            if (back.bits != bits || (uint64_t)jet_rt_test_f64_bits(value.value) != bits) failed = 1;
            display = jet_rt_float_to_string(value.value);
            debug = jet_rt_float_debug(value.value);
        }
        write_all(1, hex, 16); write_all(1, "\t", 1);
        write_all(1, display->ptr, display->len); write_all(1, "\t", 1);
        write_all(1, debug->ptr, debug->len); write_all(1, "\n", 1);
        release(display); release(debug);
    }
finished:
    if (failed) write_all(2, "display witness scalar/transport failure\n", sizeof("display witness scalar/transport failure\n") - 1);
    syscall3(231, failed, 0, 0);
    __builtin_unreachable();
}
__attribute__((naked, noreturn)) void _start(void) {
    __asm__ volatile("andq $-16, %rsp\ncall display_witness_main\nud2");
}
