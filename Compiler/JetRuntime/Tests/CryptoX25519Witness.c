// Official RFC 7748 sections 5.2/6.1. Link CryptoX25519Tests.jet, no C crypto.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_x25519_fixture(const void *, const void *, const void *);
extern void jet_rt_x25519_iteration_fixture(int64_t, const void *);
extern void jet_rt_x25519_negative_carry_fixture(void);
static unsigned char nibble(char c) {
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned char)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static void decode(unsigned char out[32], const char *hex) {
    assert(strlen(hex) == 64);
    for (size_t i = 0; i < 32; ++i)
        out[i] = (unsigned char)((nibble(hex[2*i]) << 4) | nibble(hex[2*i+1]));
}
static void kat(const char *scalar, const char *point, const char *expected) {
    unsigned char k[32], u[32], answer[32];
    decode(k, scalar); decode(u, point); decode(answer, expected);
    jet_rt_x25519_fixture(k, u, answer);
    // RFC 7748 decoders MUST ignore the most significant bit of u.
    u[31] ^= 128;
    jet_rt_x25519_fixture(k, u, answer);
}
int main(void) {
    jet_rt_alloc_init();
    jet_rt_x25519_negative_carry_fixture();
    kat("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4",
        "e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c",
        "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552");
    kat("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d",
        "e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493",
        "95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957");
    const char *alice = "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
    const char *bob = "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb";
    const char *base = "0900000000000000000000000000000000000000000000000000000000000000";
    const char *apub = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";
    const char *bpub = "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f";
    const char *shared = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
    kat(alice, base, apub); kat(bob, base, bpub);
    kat(alice, bpub, shared); kat(bob, apub, shared);
    const char *zero = "0000000000000000000000000000000000000000000000000000000000000000";
    kat(alice, zero, zero);
    kat(alice, "0100000000000000000000000000000000000000000000000000000000000000", zero);
    unsigned char expected[32];
    decode(expected, "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079");
    jet_rt_x25519_iteration_fixture(1, expected);
    decode(expected, "684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51");
    jet_rt_x25519_iteration_fixture(1000, expected);
    puts("x25519 witness OK (RFC 7748 KATs, DH, top-bit, low-order, 1/1000 iterations, scratch wipe)");
    return 0;
}
