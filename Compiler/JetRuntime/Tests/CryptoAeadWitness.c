// RFC8439 2.5.2/2.8.2/A.5 + XChaCha draft A.3.1, real Jet AEAD only.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_poly_fixture(const void *, const void *, int64_t, const void *);
extern void jet_rt_aead_fixture(bool, const void *, const void *, const void *, int64_t, const void *, int64_t, const void *);
static unsigned char nibble(char c) {
    assert((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'));
    return (unsigned char)(c <= '9' ? c - '0' : c - 'a' + 10);
}
static size_t decode(unsigned char *out, const char *hex) {
    size_t len = strlen(hex)/2; assert(strlen(hex) == 2*len);
    for (size_t i = 0; i < len; ++i) out[i] = (unsigned char)((nibble(hex[2*i]) << 4) | nibble(hex[2*i+1]));
    return len;
}
int main(void) {
    jet_rt_alloc_init();
    unsigned char key[32], nonce[24], aad[12], expected[281], tag[16];
    decode(key, "85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
    decode(tag, "a8061dc1305136c6c22b8baf0c0127a9");
    jet_rt_poly_fixture(key, "Cryptographic Forum Research Group", 34, tag);
    const char *plain = "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    for (size_t i = 0; i < 32; ++i) key[i] = (unsigned char)(128+i);
    decode(aad, "50515253c0c1c2c3c4c5c6c7");
    decode(nonce, "070000004041424344454647");
    decode(expected, "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691");
    jet_rt_aead_fixture(false, key, nonce, aad, 12, plain, 114, expected);
    decode(nonce, "404142434445464748494a4b4c4d4e4f5051525354555657");
    decode(expected, "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49");
    jet_rt_aead_fixture(true, key, nonce, aad, 12, plain, 114, expected);
    decode(key, "1c9240a5eb55d38af333888604f6b5f0473917c1402b80099dca5cbc207075c0");
    decode(nonce, "000000000102030405060708");
    decode(aad, "f33388860000000000004e91");
    size_t len = decode(expected, "64a0861575861af460f062c79be643bd5e805cfd345cf389f108670ac76c8cb24c6cfc18755d43eea09ee94e382d26b0bdb7b73c321b0100d4f03b7f355894cf332f830e710b97ce98c8a84abd0b948114ad176e008d33bd60f982b1ff37c8559797a06ef4f0ef61c186324e2b3506383606907b6a7c02b0f9f6157b53c867e4b9166c767b804d46a59b5216cde7a4e99040c5a40433225ee282a1b0a06c523eaf4534d7f83fa1155b0047718cbc546a0d072b04b3564eea1b422273f548271a0bb2316053fa76991955ebd63159434ecebb4e466dae5a1073a6727627097a1049e617d91d361094fa68f0ff77987130305beaba2eda04df997b714d6c6f2c29a6ad5cb4022b02709beead9d67890cbb22392336fea1851f38");
    const char *a5 = "Internet-Drafts are draft documents valid for a maximum of six months and may be updated, replaced, or obsoleted by other documents at any time. It is inappropriate to use Internet-Drafts as reference material or to cite them other than as /\xe2\x80\x9cwork in progress./\xe2\x80\x9d";
    assert(strlen(a5) == 265 && len == 281);
    jet_rt_aead_fixture(false, key, nonce, aad, 12, a5, 265, expected);
    puts("poly1305/AEAD witness OK (RFC8439/XChaCha KATs, every tag byte rejection, unchanged output, scratch wipe)");
    return 0;
}
