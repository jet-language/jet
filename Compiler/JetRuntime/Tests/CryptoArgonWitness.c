// RFC9106 5.3 and JVKW params frozen from Rust argon2=0.5.3; RFC7693 BLAKE2b.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_blake_fixture(const void *, int64_t, const void *);
extern void jet_rt_argon_fixture(const void *, int64_t, const void *, const void *, int64_t, const void *, int64_t, int64_t, int64_t, const void *);
static unsigned char nibble(char c) {
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned char)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static void decode(unsigned char *out, size_t len, const char *hex) {
    assert(strlen(hex) == 2*len);
    for (size_t i = 0; i < len; ++i) out[i] = (unsigned char)((nibble(hex[2*i]) << 4) | nibble(hex[2*i+1]));
}
int main(void) {
    jet_rt_alloc_init();
    unsigned char expected[64], password[32], salt[16], secret[8], ad[12];
    decode(expected, 64, "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce");
    jet_rt_blake_fixture("", 0, expected);
    decode(expected, 64, "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923");
    jet_rt_blake_fixture("abc", 3, expected);
    memset(password, 1, sizeof password); memset(salt, 2, sizeof salt);
    memset(secret, 3, sizeof secret); memset(ad, 4, sizeof ad);
    decode(expected, 32, "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659");
    jet_rt_argon_fixture(password, 32, salt, secret, 8, ad, 12, 32, 4, expected);
    decode(expected, 32, "12344caf384205e77004d3c91f7d8289fb316227708d5c319cabf47230e9fe1a");
    const char *phrase = "deterministic recovery phrase";
    jet_rt_argon_fixture(phrase, (int64_t)strlen(phrase), salt, "", 0, "", 0, 65536, 1, expected);
    puts("blake2b/argon2id witness OK (RFC7693/9106, Rust JVKW fixture, memory+scratch wipe)");
    return 0;
}
