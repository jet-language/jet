// Link the real Jet SHA-256 runtime and Sha256Tests.jet. No host hash provider.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_sha256_fixture(const void *, int64_t, const void *);
static unsigned char nibble(char c) {
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned char)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static void kat(const void *input, size_t length, const char *hex) {
    unsigned char expected[32];
    assert(strlen(hex) == 64);
    for (size_t i = 0; i < 32; ++i)
        expected[i] = (unsigned char)((nibble(hex[2*i]) << 4) | nibble(hex[2*i+1]));
    jet_rt_sha256_fixture(input, (int64_t)length, expected);
}
int main(void) {
    jet_rt_alloc_init();
    kat("", 0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    kat("abc", 3, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    const char *message = "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert(strlen(message) == 56);
    kat(message, 56, "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    unsigned char *million = malloc(1000000);
    assert(million != NULL);
    memset(million, 'a', 1000000);
    kat(million, 1000000, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    free(million);
    puts("sha256 witness OK (4 FIPS KATs, split spans, scratch wipe)");
    return 0;
}
