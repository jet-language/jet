// age0.10.1 x25519.rs TEST_SK/TEST_PK and RFC4648 unpadded vectors.
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_age_key_fixture(const void *, const void *);
extern void jet_rt_age_base64_fixture(const void *, int64_t, const void *, int64_t, bool);
int main(void) {
    jet_rt_alloc_init();
    jet_rt_age_key_fixture("AGE-SECRET-KEY-1GQ9778VQXMMJVE8SK7J6VT8UJ4HDQAJUVSFCWCM02D8GEWQ72PVQ2Y5J33", "age1t7rxyev2z3rw82stdlrrepyc39nvn86l5078zqkf5uasdy86jp6svpy7pa");
    const char *plain[] = {"", "f", "fo", "foo", "foob", "fooba", "foobar"};
    const char *text[] = {"", "Zg", "Zm8", "Zm9v", "Zm9vYg", "Zm9vYmE", "Zm9vYmFy"};
    for (size_t i = 0; i < 7; ++i) jet_rt_age_base64_fixture(plain[i], (int64_t)strlen(plain[i]), text[i], (int64_t)strlen(text[i]), true);
    const char *bad[] = {"Z", "Zh", "Zm9", "Zg=", "Zg==", "Zg\n", "-g", "_g"};
    for (size_t i = 0; i < 8; ++i) jet_rt_age_base64_fixture("", 64, bad[i], (int64_t)strlen(bad[i]), false);
    puts("age codec witness OK (Rust age Bech32 key pair, RFC4648, padding/checksum/trailing-bit rejection)");
    return 0;
}
