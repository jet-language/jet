// C-caller witness for the Jet runtime object's per-position C export law.
// `jet_rt_slice_end` mixes I64 words with a U8 flag. System V leaves the bits
// above a narrow argument unspecified, so the raw calls pass garbage upper
// bits: the export entry must read only the low byte of `exclusive`.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int64_t jet_rt_slice_end(int64_t start, int64_t end, uint8_t exclusive, int64_t len,
                         int64_t file, int64_t file_len, int64_t line);
typedef int64_t (*raw_slice_end)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t,
                                 uint64_t, uint64_t);

// The object's audited imports (imports.list) are reached only on trap and
// stdout paths, which these calls never take.
void jet_int_owned_from_i64(void) { abort(); }
void jet_int_owned_to_i64(void) { abort(); }
void jet_std_io_stdout(void) { abort(); }
void jet_std_io_stdout_flush(void) { abort(); }

static int failures = 0;

static void check(const char *what, int64_t got, int64_t want) {
    printf("%s = %lld (want %lld)\n", what, (long long)got, (long long)want);
    if (got != want) failures++;
}

int main(void) {
    check("exclusive", jet_rt_slice_end(0, 5, 1, 10, 0, 0, 0), 5);
    check("inclusive", jet_rt_slice_end(0, 5, 0, 10, 0, 0, 0), 6);
    raw_slice_end raw = (raw_slice_end)(void (*)(void))jet_rt_slice_end;
    check("inclusive, garbage upper bits", raw(0, 5, 0xffffffffffffff00ull, 10, 0, 0, 0), 6);
    check("exclusive, garbage upper bits", raw(0, 5, 0xabcdef0000000001ull, 10, 0, 0, 0), 5);
    return failures == 0 ? 0 : 1;
}
