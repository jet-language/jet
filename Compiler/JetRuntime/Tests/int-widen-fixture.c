/* Jet-only native-pack C ABI witness. No Rust oracle or parser/formatter
 * dependence: independent base-1e9 nodes exercise precision and finite range.
 * Link this with the composed native pack; run without NDEBUG. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <float.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern double jet_int_checked_widen(int64_t, bool, int64_t, int64_t);
typedef struct {
    uint64_t refs, retired_next, negative;
    uint32_t *limbs;
    uint64_t len, cap;
} IntNode;
_Static_assert(sizeof(IntNode) == 48, "exact Int C carrier size");
static const char filename[] = "int-widen-fixture.jet";
static const uintptr_t file_words[3] = {(uintptr_t)filename, sizeof filename - 1, 0};

/* Construct mantissa * 2^shift, not a float-derived approximation. */
static int64_t boxed(IntNode *node, uint32_t limbs[40], uint64_t mantissa,
                     unsigned shift, bool negative) {
    memset(limbs, 0, 40 * sizeof *limbs);
    *node = (IntNode){1, 0, negative, limbs, 0, 40};
    do {
        limbs[node->len++] = (uint32_t)(mantissa % UINT64_C(1000000000));
        mantissa /= UINT64_C(1000000000);
    } while (mantissa);
    for (unsigned i = 0; i < shift; ++i) {
        uint64_t carry = 0;
        for (uint64_t j = 0; j < node->len; ++j) {
            uint64_t value = (uint64_t)limbs[j] * 2 + carry;
            limbs[j] = (uint32_t)(value % UINT64_C(1000000000));
            carry = value / UINT64_C(1000000000);
        }
        if (carry) {
            assert(node->len < 40);
            limbs[node->len++] = (uint32_t)carry;
        }
    }
    assert(((uintptr_t)node >> 62) == 0);
    return (int64_t)((uintptr_t)node | (UINT64_C(1) << 62));
}

static void accepted(int64_t raw, bool f32, double expected) {
    union { double value; uint64_t bits; } actual = {
        .value = jet_int_checked_widen(raw, f32, (int64_t)(intptr_t)file_words, 27)
    }, wanted = {.value = expected};
    assert(actual.bits == wanted.bits); /* Includes canonical positive zero. */
}

static void rejected(int64_t raw, bool f32, int64_t line, const char *location) {
    int output[2];
    assert(pipe(output) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        assert(close(output[0]) == 0);
        assert(dup2(output[1], STDERR_FILENO) == STDERR_FILENO);
        assert(close(output[1]) == 0);
        (void)jet_int_checked_widen(raw, f32, (int64_t)(intptr_t)file_words, line);
        _exit(99);
    }
    assert(close(output[1]) == 0);
    char text[4096] = {0};
    size_t at = 0;
    for (;;) {
        ssize_t count = read(output[0], text + at, sizeof text - 1 - at);
        assert(count >= 0);
        if (!count) break;
        at += (size_t)count;
        assert(at < sizeof text - 1);
    }
    assert(close(output[0]) == 0);
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 70);
    assert(strstr(text, "Stop [E3010]: `whole number cannot cross into the decimal without losing precision`\n"));
    assert(strstr(text, location));
    assert(!strstr(text, "E3001"));
}

int main(void) {
    for (unsigned target = 0; target < 2; ++target) {
        bool f32 = target != 0;
        int64_t boundary = INT64_C(1) << (f32 ? 24 : 53);
        accepted(0, f32, 0.0);
        for (int sign = -1; sign <= 1; sign += 2) {
            accepted(sign, f32, (double)sign);
            accepted(sign * (boundary - 1), f32, (double)(sign * (boundary - 1)));
            accepted(sign * boundary, f32, (double)(sign * boundary));
            accepted(sign * (boundary + 2), f32, (double)(sign * (boundary + 2)));
            rejected(sign * (boundary + 1), f32, 27, "int-widen-fixture.jet:27");
        }
        IntNode node;
        uint32_t limbs[40], before[40];
        for (unsigned negative = 0; negative < 2; ++negative) {
            int64_t raw = boxed(&node, limbs, 1, 63, negative != 0);
            memcpy(before, limbs, sizeof before);
            accepted(raw, f32, negative ? -0x1p63 : 0x1p63);
            assert(!memcmp(before, limbs, sizeof before));
            assert(node.refs == 1 && node.retired_next == 0 && node.negative == negative);
            raw = boxed(&node, limbs, UINT64_C(9223372036854775807), 0, negative != 0);
            rejected(raw, f32, 27, "int-widen-fixture.jet:27");
            raw = boxed(&node, limbs, 1, 62, negative != 0);
            accepted(raw, f32, negative ? -0x1p62 : 0x1p62);
            unsigned precision = f32 ? 24 : 53;
            unsigned max_bits = f32 ? 128 : 1024;
            raw = boxed(&node, limbs, (UINT64_C(1) << precision) - 1,
                        max_bits - precision, negative != 0);
            double maximum = f32 ? (double)FLT_MAX : DBL_MAX;
            accepted(raw, f32, negative ? -maximum : maximum);
            raw = boxed(&node, limbs, 1, max_bits, negative != 0);
            rejected(raw, f32, 27, "int-widen-fixture.jet:27");
            /* A 54-bit mantissa stays inexact even with 80 trailing zeros. */
            raw = boxed(&node, limbs, UINT64_C(9007199254740993), 80, negative != 0);
            rejected(raw, f32, 27, "int-widen-fixture.jet:27");
        }
    }
    /* Both inline-carrier extremes: -2^62 exact, 2^62-1 inexact. */
    accepted(-(INT64_C(1) << 62), false, -0x1p62);
    accepted(-(INT64_C(1) << 62), true, -0x1p62);
    rejected((INT64_C(1) << 62) - 1, false, 27, "int-widen-fixture.jet:27");
    rejected((INT64_C(1) << 62) - 1, true, 27, "int-widen-fixture.jet:27");
    /* The borrowed filename is length-delimited, and the caller's u32 line
     * is preserved, including its upper bound (not narrowed to signed32). */
    rejected(INT64_C(16777217), true, INT64_C(4294967295),
             "int-widen-fixture.jet:4294967295");
    puts("CHECK OK — exact Int widening: precision24/53, signs, boxed/inline extremes, finite limits, borrowed file:u32-line");
    return 0;
}
