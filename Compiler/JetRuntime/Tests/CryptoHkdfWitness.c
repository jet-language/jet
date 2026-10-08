// Official RFC4231 cases1/2/6 and RFC5869 SHA256 cases1/2/3.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_hmac_fixture(const void *, int64_t, const void *, int64_t, const void *);
extern void jet_rt_hkdf_fixture(const void *, int64_t, const void *, int64_t, const void *, int64_t, int64_t, const void *, const void *);
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
    unsigned char key[131], digest[32], ikm[80], salt[80], info[80], prk[32], okm[82];
    memset(key, 0x0b, 20);
    decode(digest, 32, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    jet_rt_hmac_fixture(key, 20, "Hi There", 8, digest);
    decode(digest, 32, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    jet_rt_hmac_fixture("Jefe", 4, "what do ya want for nothing?", 28, digest);
    memset(key, 0xaa, sizeof key);
    decode(digest, 32, "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
    const char *long_key_msg = "Test Using Larger Than Block-Size Key - Hash Key First";
    jet_rt_hmac_fixture(key, 131, long_key_msg, (int64_t)strlen(long_key_msg), digest);
    memset(ikm, 0x0b, 22);
    for (size_t i = 0; i < 13; ++i) salt[i] = (unsigned char)i;
    for (size_t i = 0; i < 10; ++i) info[i] = (unsigned char)(0xf0+i);
    decode(prk, 32, "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5");
    decode(okm, 42, "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
    jet_rt_hkdf_fixture(ikm, 22, salt, 13, info, 10, 42, prk, okm);
    for (size_t i = 0; i < 80; ++i) { ikm[i] = (unsigned char)i; salt[i] = (unsigned char)(0x60+i); info[i] = (unsigned char)(0xb0+i); }
    decode(prk, 32, "06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244");
    decode(okm, 82, "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71cc30c58179ec3e87c14c01d5c1f3434f1d87");
    jet_rt_hkdf_fixture(ikm, 80, salt, 80, info, 80, 82, prk, okm);
    memset(ikm, 0x0b, 22);
    decode(prk, 32, "19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04");
    decode(okm, 42, "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8");
    jet_rt_hkdf_fixture(ikm, 22, "", 0, "", 0, 42, prk, okm);
    puts("hmac/hkdf witness OK (RFC4231/5869 PRK+OKM KATs, long key, empty salt, scratch wipe)");
    return 0;
}
