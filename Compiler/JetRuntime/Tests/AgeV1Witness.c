// C transport only. Crypto/protocol behavior is executed by the Jet exports.
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_age_open_fixture(const void *, const void *, int64_t, const void *, bool, bool);
extern void jet_rt_age_seal_fixture(const void *, const void *, int64_t, const void *, int64_t, int64_t, const void *, const void *, int64_t, void *);
static unsigned char *read_bytes(const char *path, int64_t *length) {
    FILE *f = fopen(path, "rb");
    if (!f || fseek(f, 0, SEEK_END) != 0) abort();
    long size = ftell(f);
    if (size < 0 || (uint64_t)size > INT64_MAX || fseek(f, 0, SEEK_SET) != 0) abort();
    *length = (int64_t)size;
    unsigned char *bytes = malloc((size_t)size + 1);
    if (!bytes || fread(bytes, 1, (size_t)size, f) != (size_t)size || fclose(f) != 0) abort();
    return bytes;
}
int main(int argc, char **argv) {
    if (argc < 6 || strlen(argv[2]) != 74) return 2;
    jet_rt_alloc_init();
    int64_t length;
    unsigned char *input = read_bytes(argv[3], &length);
    if (strcmp(argv[1], "open") == 0 && argc == 6) {
        unsigned char hash[32] = {0};
        bool check_hash = strcmp(argv[5], "-") != 0;
        if (check_hash) {
            if (strlen(argv[5]) != 64) abort();
            for (size_t i = 0; i < 32; ++i) {
                unsigned value;
                if (sscanf(argv[5] + i * 2, "%2x", &value) != 1) abort();
                hash[i] = (unsigned char)value;
            }
        }
        jet_rt_age_open_fixture(argv[2], input, length, hash, check_hash, strcmp(argv[4], "1") == 0);
    } else if (strcmp(argv[1], "seal") == 0 && argc == 7) {
        int64_t cipher_len;
        unsigned char *expected = read_bytes(argv[4], &cipher_len);
        unsigned char *capture = malloc((size_t)cipher_len + 1);
        int64_t count = strtoll(argv[5], NULL, 10);
        if (!capture || count < 1 || count > 2) abort();
        const char *grease = "-> jet-reference a\nAA\n";
        jet_rt_age_seal_fixture(argv[2], input, length, expected, cipher_len, count, "YELLOW SUBMARINE", grease, (int64_t)strlen(grease), capture);
        FILE *out = fopen(argv[6], "wb");
        if (!out || fwrite(capture, 1, (size_t)cipher_len, out) != (size_t)cipher_len || fclose(out) != 0) abort();
        memset(capture, 0, (size_t)cipher_len);
        free(capture); free(expected);
    } else return 2;
    memset(input, 0, (size_t)length);
    free(input);
    return 0;
}
