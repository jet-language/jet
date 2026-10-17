/* Actual native Float/Text Result kernels. strtod is an independent C-only
 * decimal oracle for the long mantissa corpus, never linked as a provider.
 */
#include <assert.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern int64_t jet_float_parse(int64_t, int64_t, int64_t);
extern int64_t jet_rt_string_from_static(int64_t, int64_t);
extern void jet_rt_string_drop(int64_t);
struct text { const char *data; int64_t len, capacity; };
static uint64_t parsed(const char *input) {
    int64_t value = jet_rt_string_from_static((int64_t)(intptr_t)input, (int64_t)strlen(input));
    uint64_t bits = UINT64_C(0xdeadbeefdeadbeef);
    int64_t error = -9;
    assert(jet_float_parse(value, (int64_t)(intptr_t)&bits, (int64_t)(intptr_t)&error) == 1);
    assert(error == -9);
    jet_rt_string_drop(value);
    return bits;
}
static void rejected(const char *input) {
    int64_t value = jet_rt_string_from_static((int64_t)(intptr_t)input, (int64_t)strlen(input));
    uint64_t bits = UINT64_C(0xdeadbeefdeadbeef);
    int64_t error = -9;
    assert(jet_float_parse(value, (int64_t)(intptr_t)&bits, (int64_t)(intptr_t)&error) == 0);
    assert(bits == UINT64_C(0xdeadbeefdeadbeef));
    const struct text *header = (const struct text *)(intptr_t)error;
    char expected[256];
    int count = snprintf(expected, sizeof expected, "cannot parse `%s` as a float", input);
    assert(count >= 0 && count < (int)sizeof expected);
    assert(header->len == count && memcmp(header->data, expected, (size_t)count) == 0);
    jet_rt_string_drop(error);
    jet_rt_string_drop(value);
}
static uint64_t oracle(const char *input) {
    char *end;
    double value = strtod(input, &end);
    assert(*end == '\0');
    uint64_t bits;
    memcpy(&bits, &value, sizeof bits);
    return bits;
}
// Frozen oracle: cheap.sh Tools/agent/jet-env rustc -V reported
// rustc 1.97.1 (8bab26f4f 2026-07-14), built from a source tarball.
// Probe: rustc --edition=2021 scratch/RtG1e/float-seed-probe.rs -o
// scratch/RtG1e/float-seed-probe; float-seed-probe float-parse.seed.golden.
// Each frame is decimal body length + '\n' + (UTF8 input hex<TAB>Ok/Err<TAB>
// IEEE bits hex/error hex). Native parity uses this output, not Rust1.90.
static unsigned hex_digit(char value) {
    if (value >= '0' && value <= '9') return (unsigned)(value - '0');
    assert(value >= 'a' && value <= 'f');
    return (unsigned)(value - 'a' + 10);
}
static void seed_goldens(const char *path) {
    FILE *file = fopen(path, "rb");
    assert(file != NULL);
    unsigned frames = 0;
    char header[32];
    while (fgets(header, sizeof header, file)) {
        char *end;
        size_t length = (size_t)strtoull(header, &end, 10);
        assert(*end == '\n' && end[1] == '\0');
        char *body = malloc(length + 1);
        assert(body != NULL && fread(body, 1, length, file) == length);
        body[length] = '\0';
        char *status = strchr(body, '\t');
        assert(status != NULL);
        *status++ = '\0';
        char *expected = strchr(status, '\t');
        assert(expected != NULL);
        *expected++ = '\0';
        size_t bytes = strlen(body);
        assert(bytes % 2 == 0);
        char *input = malloc(bytes / 2 + 1);
        assert(input != NULL);
        for (size_t i = 0; i < bytes; i += 2) input[i / 2] = (char)(hex_digit(body[i]) * 16 + hex_digit(body[i + 1]));
        input[bytes / 2] = '\0';
        if (strcmp(status, "Ok") == 0) {
            uint64_t bits = strtoull(expected, &end, 16);
            assert(*end == '\0' && parsed(input) == bits);
        } else {
            assert(strcmp(status, "Err") == 0);
            rejected(input);
        }
        free(input);
        free(body);
        ++frames;
    }
    assert(!ferror(file) && fclose(file) == 0 && frames == 297);
}
int main(int argc, char **argv) {
    assert(argc == 2);
    seed_goldens(argv[1]);
    static const struct { const char *input; uint64_t bits; } rows[] = {
        {"1.", UINT64_C(0x3ff0000000000000)},
        {".5", UINT64_C(0x3fe0000000000000)},
        {"1.e2", UINT64_C(0x4059000000000000)},
        {"-0.0", UINT64_C(0x8000000000000000)},
        {"\xe2\x80\x83 -0e0\xc2\xa0", UINT64_C(0x8000000000000000)},
        {"9007199254740993", UINT64_C(0x4340000000000000)},
        {"-9007199254740993", UINT64_C(0xc340000000000000)},
        {"5e-324", UINT64_C(1)},
        {"2.4703282292062328e-324", UINT64_C(1)},
        {"2.4703282292062327e-324", UINT64_C(0)},
        {"1.7976931348623157e308", UINT64_C(0x7fefffffffffffff)},
        {"1.7976931348623158e308", UINT64_C(0x7fefffffffffffff)},
        {"1.7976931348623159e308", UINT64_C(0x7ff0000000000000)},
        {"1e99999999", UINT64_C(0x7ff0000000000000)},
        {"1e-99999999", UINT64_C(0)},
        {"-1e-99999999", UINT64_C(0x8000000000000000)},
        {"Inf", UINT64_C(0x7ff0000000000000)},
        {"+iNF", UINT64_C(0x7ff0000000000000)},
        {"-InF", UINT64_C(0xfff0000000000000)},
        {"inFINITY", UINT64_C(0x7ff0000000000000)},
        {"+INfinity", UINT64_C(0x7ff0000000000000)},
        {"-inFinity", UINT64_C(0xfff0000000000000)},
        {"nAN", UINT64_C(0x7ff8000000000000)},
        {"+NAn", UINT64_C(0x7ff8000000000000)},
        {"-nan", UINT64_C(0xfff8000000000000)},
    };
    for (size_t i = 0; i < sizeof rows / sizeof rows[0]; ++i) {
        uint64_t actual = parsed(rows[i].input);
        if (actual != rows[i].bits) fprintf(stderr, "%s: got %016" PRIx64 " expected %016" PRIx64 "\n", rows[i].input, actual, rows[i].bits);
        assert(actual == rows[i].bits);
    }
    static const char *invalid[] = {".", "e5", "1e", "_1", "0x1", "  1e  ", "", "+", "1..2", "1e+", "1 2", "nan(1)", "infinite"};
    for (size_t i = 0; i < sizeof invalid / sizeof invalid[0]; ++i) rejected(invalid[i]);
    char long_text[816];
    long_text[0] = '1';
    memset(long_text + 1, '0', 799);
    strcpy(long_text + 800, "e-799");
    assert(parsed(long_text) == UINT64_C(0x3ff0000000000000));
    for (size_t i = 1; i < 800; ++i) long_text[i] = (char)('0' + (i * 7 + 3) % 10);
    assert(parsed(long_text) == oracle(long_text));
    uint64_t random = UINT64_C(0x17318df7270b7c82);
    for (unsigned i = 0; i < 256; ++i) {
        random = random * UINT64_C(6364136223846793005) + 1;
        char decimal[64];
        int count = snprintf(decimal, sizeof decimal, "%" PRIu64 "e%d", random, (int)((random >> 32) % 633) - 324);
        assert(count > 0 && count < (int)sizeof decimal);
        assert(parsed(decimal) == oracle(decimal));
    }
    puts("CHECK OK — native float parse: grammar, signs/specials, exact half-even, subnormal/overflow edges, 800 digits, 256 decimal oracle cases");
    return 0;
}
