/* Link only with the Jet runtime object. Bit-exact independent platform libm
 * witness for every compiler-needed Float operation, including NaN payloads,
 * signed zeros, subnormals, infinities, domain edges and fused cancellation.
 * The Jet fixture separately checks exact-Int and pinned-Unicode carriers. */
#include <assert.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define UNARY_ROUTES(X) \
    X(acos, acos) X(acosh, acosh) X(asin, asin) X(asinh, asinh) \
    X(atan, atan) X(atanh, atanh) X(cbrt, cbrt) X(ceil, ceil) \
    X(cos, cos) X(cosh, cosh) X(exp, exp) X(exp_m1, expm1) \
    X(exp2, exp2) X(floor, floor) X(ln, log) X(ln_1p, log1p) \
    X(log10, log10) X(log2, log2) X(sin, sin) X(sinh, sinh) \
    X(sqrt, sqrt) X(tan, tan) X(tanh, tanh) X(trunc, trunc)
#define DECLARE(name, reference) extern double jet_std_math_##name(double);
UNARY_ROUTES(DECLARE)
#undef DECLARE
extern double jet_std_math_atan2(double, double);
extern double jet_std_math_hypot(double, double);
extern double jet_std_math_fmod(double, double);
extern double jet_std_math_fma(double, double, double);
extern double jet_std_math_abs_f64(double);
extern bool jet_std_math_is_finite(double);
extern bool jet_std_math_is_infinite(double);
extern bool jet_std_math_is_nan(double);
extern int64_t jet_std_math_cmp(double, double);
extern void jet_std_math_sin_cos(double, double *, double *);
static double from_bits(uint64_t word) {
    double value;
    memcpy(&value, &word, sizeof value);
    return value;
}
static uint64_t bits(double value) {
    uint64_t word;
    memcpy(&word, &value, sizeof word);
    return word;
}
static void same(double actual, double expected) {
    assert(bits(actual) == bits(expected));
}
int main(void) {
    static const uint64_t words[] = {
        UINT64_C(0), UINT64_C(0x8000000000000000),
        UINT64_C(1), UINT64_C(0x8000000000000001),
        UINT64_C(0x000fffffffffffff), UINT64_C(0x0010000000000000),
        UINT64_C(0x3fefffffffffffff), UINT64_C(0x3ff0000000000000),
        UINT64_C(0x3ff0000000000001), UINT64_C(0xbff0000000000000),
        UINT64_C(0x4000000000000000), UINT64_C(0xc000000000000000),
        UINT64_C(0x7fefffffffffffff), UINT64_C(0xffefffffffffffff),
        UINT64_C(0x7ff0000000000000), UINT64_C(0xfff0000000000000),
        UINT64_C(0x7ff8000000001234), UINT64_C(0xfff8000000001234),
        UINT64_C(0x7ff0000000001234), UINT64_C(0xfff0000000001234),
    };
    for (size_t i = 0; i < sizeof words / sizeof *words; ++i) {
        double x = from_bits(words[i]);
#define CHECK(name, reference) same(jet_std_math_##name(x), reference(x));
        UNARY_ROUTES(CHECK)
#undef CHECK
        assert(bits(jet_std_math_abs_f64(x)) == (words[i] & UINT64_C(0x7fffffffffffffff)));
        uint64_t magnitude = words[i] & UINT64_C(0x7fffffffffffffff);
        assert(jet_std_math_is_finite(x) == (magnitude < UINT64_C(0x7ff0000000000000)));
        assert(jet_std_math_is_infinite(x) == (magnitude == UINT64_C(0x7ff0000000000000)));
        assert(jet_std_math_is_nan(x) == (magnitude > UINT64_C(0x7ff0000000000000)));
        double sine, cosine;
        jet_std_math_sin_cos(x, &sine, &cosine);
        same(sine, sin(x));
        same(cosine, cos(x));
        for (size_t j = 0; j < sizeof words / sizeof *words; ++j) {
            double y = from_bits(words[j]);
            same(jet_std_math_atan2(x, y), atan2(x, y));
            same(jet_std_math_hypot(x, y), hypot(x, y));
            same(jet_std_math_fmod(x, y), fmod(x, y));
            int64_t cmp = x < y ? -1 : x == y ? 0 : x > y ? 1 : isnan(x) ? (isnan(y) ? 0 : 1) : -1;
            assert(jet_std_math_cmp(x, y) == cmp);
            same(jet_std_math_fma(x, y, -1.0), fma(x, y, -1.0));
        }
    }
    double a = 1.0 + 0x1p-27, b = 1.0 - 0x1p-27;
    same(jet_std_math_fma(a, b, -1.0), -0x1p-54);
    puts("CHECK OK — Jet-only compiler math: bit-exact platform libm, payloads, zeros, domains, fused cancellation");
    return 0;
}
