// Real Jet error/pair-codec implementation only. No host vault provider.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { const unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; Text **data; } TextList;
extern void jet_rt_alloc_init(void);
extern void errors_and_handles_are_redacted(int64_t, const Text *, const Text *, const Text *, const Text *, const TextList *);
extern void jet_rt_vault_legacy_fixture(const void *, int64_t, const void *);
extern void jet_rt_vault_invalid_legacy_fixture(const void *, int64_t);
static Text text(const char *s) { return (Text){(const unsigned char *)s, (int64_t)strlen(s), 0}; }
static const char *const display[] = {
    "invalid vault key name", "vault key not found", "vault key has a different type",
    "vault key is revoked", "vault is locked", "vault write authority denied",
    "vault changed since this write was prepared", "vault provider is unsupported on this platform",
    "vault store has invalid encoding", "vault write visibility is known but durability is uncertain",
    "vault cryptography failed: encrypted data could not be opened", "vault read failed at <vault-store>",
    "internal vault failure; incident vault-17"
};
static const char *const debug[] = {
    "InvalidName", "NotFound", "WrongType", "Revoked", "Locked", "AuthorityDenied", "Conflict",
    "UnsupportedProvider", "InvalidEncoding", "DurabilityUnknown", "Crypto(OpenFailed)",
    "IO { operation: \"read\", redacted_path: \"<vault-store>\" }", "Internal { incident_id: \"vault-17\" }"
};
int main(void) {
    jet_rt_alloc_init();
    Text forbidden[] = {text("secrets.age"), text("AGE-SECRET"), text("/home/"), text("hunter2"), text("ciphertext sentinel")};
    Text *needles[] = {&forbidden[0], &forbidden[1], &forbidden[2], &forbidden[3], &forbidden[4]};
    TextList list = {5, 5, needles};
    for (int64_t kind = 0; kind < 13; ++kind) {
        Text first = text(kind == 10 ? "encrypted data could not be opened" : kind == 11 ? "read" : "vault-17");
        Text second = text(kind == 10 ? "OpenFailed" : "<vault-store>");
        Text expected_display = text(display[kind]), expected_debug = text(debug[kind]);
        errors_and_handles_are_redacted(kind, &first, &second, &expected_display, &expected_debug, &list);
    }
    static const unsigned char wire[] = {
        2,0,0,0, 5,0,0,0, 't','o','k','e','n', 7,0,0,0, 'h','u','n','t','e','r','2',
        0,0,0,0, 3,0,0,0, 0xc3,0xa9,0
    };
    static const unsigned char digest[] = {
        0x8f,0x78,0x31,0x38,0x3c,0xad,0xac,0x3c,0x5c,0x98,0xf0,0x5a,0xaa,0x62,0xdf,0xa0,
        0xd9,0x79,0x32,0xc0,0xd0,0x46,0x69,0xbf,0x21,0xde,0x7c,0xe6,0x90,0x1f,0xf0,0x58
    };
    assert(sizeof(wire) == 35 && sizeof(digest) == 32);
    jet_rt_vault_legacy_fixture(wire, (int64_t)sizeof(wire), digest);
    unsigned char bad[sizeof(wire)];
    memcpy(bad, wire, sizeof(wire));
    bad[32] = 0xff;
    jet_rt_vault_invalid_legacy_fixture(bad, (int64_t)sizeof(bad));
    static const unsigned char huge_count[] = {0xff,0xff,0xff,0xff};
    jet_rt_vault_invalid_legacy_fixture(huge_count, 4);
    static const unsigned char overlong[] = {1,0,0,0, 0xff,0xff,0xff,0xff};
    jet_rt_vault_invalid_legacy_fixture(overlong, 8);
    puts("vault witness OK (13 exact private errors, legacy codec, domain SHA256)");
    return 0;
}
