// Direct C ABI witness: validates numeric value independently of jet_int_compare
// and the runtime's formatter, so a mutually wrong parser/compare cannot pass.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

typedef struct {
    uint64_t refs, retired_next, negative;
    uint32_t *limbs;
    uint64_t len, cap;
} IntNode;
_Static_assert(sizeof(IntNode) == 48, "exact Int C carrier size");

extern int64_t jet_rt_int_from_static(const unsigned char *, uint64_t);
extern int64_t jet_rt_int_clone(int64_t);
extern void jet_rt_int_drop(int64_t);
#ifdef JET_INT_NATIVE_FIXTURE
extern void jet_test_int_hazard_fixture(void);
#endif
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern double jet_int_owned_to_f64(int64_t);
extern int64_t jet_int_compare(int64_t, int64_t);
extern int64_t jet_int_checked_fixed(int64_t, int64_t, const unsigned char *, uint64_t, int64_t);
#define BINARY(name) extern int64_t jet_int_##name(int64_t, int64_t);
BINARY(add) BINARY(sub) BINARY(mul) BINARY(bit_and) BINARY(bit_or) BINARY(bit_xor)
#define LOCATED(name) extern int64_t jet_int_##name(int64_t, int64_t, const unsigned char *, uint64_t, int64_t);
LOCATED(div) LOCATED(rem) LOCATED(mod) LOCATED(floor_div)
LOCATED(div_euclid) LOCATED(rem_euclid) LOCATED(pow) LOCATED(shl) LOCATED(shr)

static int failures;
static int64_t parse(const char *text) {
    return jet_rt_int_from_static((const unsigned char *)text, strlen(text));
}

static void value(const char *label, int64_t raw, const char *expected) {
    uint32_t limbs[160] = {0};
    size_t len = 0;
    int negative = expected[0] == '-';
    const char *digits = expected + negative;
    size_t end = strlen(digits);
    do {
        size_t start = end > 9 ? end - 9 : 0;
        uint32_t limb = 0;
        for (size_t i = start; i < end; ++i) limb = limb * 10 + (uint32_t)(digits[i] - '0');
        limbs[len++] = limb;
        end = start;
    } while (end);
    int ok = 1;
    if (raw >= -(INT64_C(1) << 62) && raw < (INT64_C(1) << 62)) {
        char text[32];
        snprintf(text, sizeof(text), "%" PRId64, raw);
        ok = !strcmp(text, expected);
    } else {
        IntNode *node = (IntNode *)(uintptr_t)((uint64_t)raw & ((UINT64_C(1) << 62) - 1));
        ok = node->refs != 0 && node->retired_next == 0 && node->negative == (uint64_t)negative && node->len == len && node->cap >= len;
        if (ok) ok = !memcmp(limbs, node->limbs, len * sizeof(uint32_t));
    }
    if (!ok) { fprintf(stderr, "%s: wrong value (expected %s)\n", label, expected); ++failures; }
    printf("%s\t%s\t%s\n", label, expected, ok ? "pass" : "wrong");
    jet_rt_int_drop(raw);
}

static void word_value(const char *label, int64_t word, int64_t expected) {
    int ok = word == expected;
    if (!ok) { fprintf(stderr, "%s: wrong fixed word\n", label); ++failures; }
    printf("%s\t%" PRId64 "\t%s\n", label, word, ok ? "pass" : "wrong");
}

static void float_value(const char *label, int64_t raw, uint64_t expected) {
    union { double f; uint64_t bits; } actual = { .f = jet_int_owned_to_f64(raw) };
    int ok = actual.bits == expected;
    if (!ok) { fprintf(stderr, "%s: float bits %016" PRIx64 " != %016" PRIx64 "\n", label, actual.bits, expected); ++failures; }
    printf("%s\t%016" PRIx64 "\t%s\n", label, actual.bits, ok ? "pass" : "wrong");
    jet_rt_int_drop(raw);
}

#include "int-corpus.inc"

int main(int argc, char **argv) {
    // Independent stopped processes allow byte-for-byte Rust/native report
    // and exit-status comparisons without narrowing the success corpus.
    const unsigned char *file = (const unsigned char *)"int-cabi.jet";
    if (argc == 2) {
        if (!strcmp(argv[1], "zero")) jet_int_div(1, 0, file, 12, 7);
        else if (!strcmp(argv[1], "exponent")) jet_int_pow(2, -1, file, 12, 7);
        else if (!strcmp(argv[1], "shift-negative")) jet_int_shl(1, -1, file, 12, 7);
        else if (!strcmp(argv[1], "shift-wide")) jet_int_shr(1, parse("9223372036854775808"), file, 12, 7);
        else if (!strcmp(argv[1], "fixed")) jet_int_checked_fixed(128, 0, file, 12, 7);
        else if (!strcmp(argv[1], "native")) jet_int_owned_to_i64(parse("9223372036854775808"));
        else return 64;
        return 65; // Every named case must stop before returning.
    }
    int_corpus();
#ifdef JET_INT_NATIVE_FIXTURE
    jet_test_int_hazard_fixture();
#endif
    return failures ? 1 : 0;
}
