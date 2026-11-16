// RFC8439 2.3.2/2.4.2 and XChaCha draft 2.2.1/A.3.1 encryption KATs.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_chacha_fixture(int64_t, const void *, const void *, int64_t, const void *, int64_t, const void *);
static unsigned char nibble(char c) {
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned char)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static void decode(unsigned char *out, size_t len, const char *hex) {
    assert(strlen(hex) == 2*len);
    for (size_t i = 0; i < len; ++i)
        out[i] = (unsigned char)((nibble(hex[2*i]) << 4) | nibble(hex[2*i+1]));
}
int main(void) {
    jet_rt_alloc_init();
    unsigned char key[32], nonce[24], expected[114];
    for (size_t i = 0; i < 32; ++i) key[i] = (unsigned char)i;
    decode(nonce, 12, "000000090000004a00000000");
    decode(expected, 64, "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4ed2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e");
    jet_rt_chacha_fixture(0, key, nonce, 1, "", 64, expected);
    decode(nonce, 16, "000000090000004a0000000031415927");
    decode(expected, 32, "82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc");
    jet_rt_chacha_fixture(1, key, nonce, 0, "", 32, expected);
    const char *plain = "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    assert(strlen(plain) == 114);
    decode(nonce, 12, "000000000000004a00000000");
    decode(expected, 114, "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0bf91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d807ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab77937365af90bbf74a35be6b40b8eedf2785e42874d");
    jet_rt_chacha_fixture(2, key, nonce, 1, plain, 114, expected);
    for (size_t i = 0; i < 32; ++i) key[i] = (unsigned char)(128+i);
    decode(nonce, 24, "404142434445464748494a4b4c4d4e4f5051525354555657");
    decode(expected, 114, "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52e");
    jet_rt_chacha_fixture(3, key, nonce, 1, plain, 114, expected);
    puts("chacha witness OK (RFC8439/XChaCha draft KATs, scratch wipe)");
    return 0;
}
