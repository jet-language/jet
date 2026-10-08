#define _POSIX_C_SOURCE 200809L
// Real Jet vault state/codec only; link -Wl,--wrap=jet_rt_free for zeroization.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>
typedef struct { const unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; Text **data; } TextList;
extern void jet_rt_alloc_init(void);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_crypto_entropy_init(void);
extern void jet_rt_vault_test_hooks_init(int64_t (*)(void *, void *));
extern int64_t jet_rt_vault_test_hooks_destroy(void *, void *);
extern void errors_and_handles_are_redacted(int64_t, intptr_t, const Text *, const Text *, const Text *, const TextList *);
extern void jet_rt_vault_crypto_projection_fixture(void);
extern void jet_rt_vault_legacy_fixture(const void *, int64_t, const void *);
extern void jet_rt_vault_invalid_legacy_fixture(const void *, int64_t);
typedef struct { int64_t length, capacity; unsigned char *data; } ByteList;
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_string_drop(Text *);
extern void stored_key_bytes_are_redacted_and_zeroized_on_drop(const Text *, ByteList *, const Text *, const Text *, void **);
extern void jet_rt_vault_record_fixture_drop(void *);
extern Text *jet_rt_vault_record_fixture_debug(void *);
extern void jet_rt_vault_origin_hash_bound_fixture(void *, const Text *, const unsigned char *);
extern void jvlt2_codec_is_canonical_bounded_and_preserves_legacy_strings(const Text *, const Text *, const Text *, const Text *, const Text *);
extern void jet_rt_vault_codec_duplicate_fixture(void *, const Text *);
extern void jet_rt_vault_linux_dir_fixture(const Text *, int64_t);
extern void jet_rt_vault_linux_inode_fixture(const Text *, const TextList *, const Text *, const Text *);
extern void imported_origin_round_trips_and_invalid_combinations_reject(const Text *, const Text *);
static struct { void *data; int64_t capacity; unsigned frees; } observed[2];
static size_t observed_count;
void __wrap_jet_rt_free(void *data, int64_t size, int64_t align) {
    for (size_t i = 0; i < observed_count; ++i) if (data == observed[i].data) {
        assert(size == observed[i].capacity && align == 8 && ++observed[i].frees == 1);
        for (int64_t byte = 0; byte < size; ++byte) assert(((const unsigned char *)data)[byte] == 0);
    }
    __real_jet_rt_free(data, size, align);
}
static Text text(const char *s) { return (Text){(const unsigned char *)s, (int64_t)strlen(s), 0}; }
static const char *const display[] = {
    "invalid vault key name", "vault key not found", "vault key has a different type",
    "vault key is revoked", "vault is locked", "vault write authority denied",
    "vault changed since this write was prepared", "vault provider is unsupported on this platform",
    "vault store has invalid encoding", "vault write visibility is known but durability is uncertain",
    "vault cryptography failed: OpenFailed", "vault read failed at <vault-store>",
    "internal vault failure; incident vault-17"
};
static const char *const debug[] = {
    "InvalidName", "NotFound", "WrongType", "Revoked", "Locked", "AuthorityDenied", "Conflict",
    "UnsupportedProvider", "InvalidEncoding", "DurabilityUnknown", "Crypto(OpenFailed)",
    "IO { operation: \"read\", redacted_path: \"<vault-store>\" }", "Internal { incident_id: \"vault-17\" }"
};
static void zeroization_fixture(void) {
    unsigned char *data = jet_rt_alloc(40, 8);
    memset(data, 0x5a, 40);
    ByteList bytes = {32, 40, data};
    Text name = text("transport"), redacted = text("<redacted>"), forbidden = text("90, 90");
    void *parts[3];
    stored_key_bytes_are_redacted_and_zeroized_on_drop(&name, &bytes, &redacted, &forbidden, parts);
    assert(bytes.length == 0 && bytes.capacity == 0 && bytes.data == NULL);
    ByteList *clone_key;
    memcpy(&clone_key, (const unsigned char *)parts[1] + 176, sizeof(clone_key));
    assert(clone_key->length == 32 && clone_key->capacity == 32);
    assert(memcmp(clone_key->data, data, 32) == 0);
    // Seed830-861 serialization golden, independently computed with Node's
    // X25519/SHA256 for this actual fixture key. A4097-byte origin exceeds both
    // the name limit and the old bounded concatenation workspace.
    unsigned char origin_data[4097];
    memset(origin_data, 'x', sizeof(origin_data));
    Text origin_name = {origin_data, (int64_t)sizeof(origin_data), 0};
    static const unsigned char origin_hash[32] = {
        0xf9,0xd5,0x7d,0x14,0xf9,0x94,0x58,0x8b,0x07,0x9e,0xc3,0xc2,0x9b,0x37,0xef,0xab,
        0xda,0xf2,0x9f,0xac,0x76,0x0f,0x2e,0x29,0x62,0x8a,0xa1,0x9c,0xd7,0x22,0xb2,0xed
    };
    jet_rt_vault_origin_hash_bound_fixture(parts[0], &origin_name, origin_hash);
    Text invalid_origin = text(".");
    jet_rt_vault_codec_duplicate_fixture(parts[0], &invalid_origin);
    observed[0].data = data; observed[0].capacity = 40;
    observed[1].data = clone_key->data; observed[1].capacity = 32; observed_count = 2;
    jet_rt_string_drop(parts[2]);
    // A pure Debug golden separately witnesses field order, fixed-array list
    // rendering and all64 unsigned generation bits; Debug never validates codec.
    unsigned char *record = parts[0];
    memset(record + 16, 0xff, 8);
    memset(record + 72, 3, 32); memset(record + 104, 4, 32); memset(record + 136, 5, 32);
    Text *shown = jet_rt_vault_record_fixture_debug(record);
    const char *golden = "VaultRecord { name: \"transport\", key_type: 2, generation: 18446744073709551615, status: Active, provenance: Imported, opaque_id: [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2], created_unix_ms: 10, status_unix_ms: 10, record_hash: [3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3], public_key_hash: [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4], reason_hash: [5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5], origin: None, key: \"<redacted>\" }";
    assert(shown->length == (int64_t)strlen(golden) && memcmp(shown->data, golden, strlen(golden)) == 0);
    jet_rt_string_drop(shown);
    jet_rt_vault_record_fixture_drop(parts[0]);
    assert(observed[0].frees == 1 && observed[1].frees == 0);
    jet_rt_vault_record_fixture_drop(parts[1]);
    assert(observed[0].frees == 1 && observed[1].frees == 1);
    observed_count = 0;
}
// Filesystem setup only; openat2/ownership policy and cleanup run in real Jet.
static void linux_dir_fixture(void) {
    char root[] = "/tmp/jet-vault-linux-XXXXXX", child[128], actual[128];
    assert(mkdtemp(root) != NULL);
    assert(snprintf(child, sizeof(child), "%s/.jet", root) > 0);
    assert(snprintf(actual, sizeof(actual), "%s/actual", root) > 0);
    Text path = text(root);
    assert(mkdir(child, 0700) == 0);
    jet_rt_vault_linux_dir_fixture(&path, -1);
    Text names[] = {text("."), text(".secrets.age.next.fixture"), text("secrets.age"),
        text(".secrets.age.lock"), text(".secrets.age.lock.quarantine"), text(".secrets.age.lock.acquired")};
    Text *paths[] = {&names[0], &names[1], &names[2], &names[3], &names[4], &names[5]};
    TextList list = {6, 6, paths};
    Text payload = text("real anonymous file bytes"), fault = text("substitute-lock-after-check");
    jet_rt_vault_linux_inode_fixture(&path, &list, &payload, &fault);
    assert(chmod(child, 0777) == 0);
    jet_rt_vault_linux_dir_fixture(&path, 7);
    assert(chmod(child, 0700) == 0);
    assert(rename(child, actual) == 0 && symlink(actual, child) == 0);
    jet_rt_vault_linux_dir_fixture(&path, 7);
    assert(unlink(child) == 0 && rmdir(actual) == 0 && rmdir(root) == 0);
}
int main(void) {
    jet_rt_alloc_init();
    jet_rt_thread_init();
    jet_rt_crypto_entropy_init();
    jet_rt_vault_test_hooks_init(jet_rt_vault_test_hooks_destroy);
    Text forbidden[] = {text("secrets.age"), text("AGE-SECRET"), text("/home/"), text("hunter2"), text("ciphertext sentinel")};
    Text *needles[] = {&forbidden[0], &forbidden[1], &forbidden[2], &forbidden[3], &forbidden[4]};
    TextList list = {5, 5, needles};
    for (int64_t kind = 0; kind < 13; ++kind) {
        Text first = text(kind == 11 ? "read" : "vault-17");
        Text second = text("<vault-store>");
        Text expected_display = text(display[kind]), expected_debug = text(debug[kind]);
        errors_and_handles_are_redacted(kind, kind == 10 ? 0 : (intptr_t)&first, &second, &expected_display, &expected_debug, &list);
    }
    static const char *const crypto_display[] = {
        "vault cryptography failed: OpenFailed", "vault cryptography failed: VerifyFailed",
        "vault cryptography failed: KeyRejected", "vault cryptography failed: Length",
        "vault cryptography failed: Unavailable"
    };
    static const char *const crypto_debug[] = {
        "Crypto(OpenFailed)", "Crypto(VerifyFailed)", "Crypto(KeyRejected)", "Crypto(Length)", "Crypto(Unavailable)"
    };
    for (intptr_t ordinal = 0; ordinal < 5; ++ordinal) {
        Text expected_display = text(crypto_display[ordinal]), expected_debug = text(crypto_debug[ordinal]);
        errors_and_handles_are_redacted(10, ordinal, NULL, &expected_display, &expected_debug, &list);
    }
    jet_rt_vault_crypto_projection_fixture();
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
    Text codec_name = text("db_password"), codec_value = text("hunter2");
    Text bad_name = text(" ../secret "), non_nfc = text("e\xcc\x81"), nfc = text("\xc3\xa9");
    jvlt2_codec_is_canonical_bounded_and_preserves_legacy_strings(&codec_name, &codec_value, &bad_name, &non_nfc, &nfc);
    Text release = text("release"), source = text("source");
    imported_origin_round_trips_and_invalid_combinations_reject(&release, &source);
    zeroization_fixture();
    linux_dir_fixture();
    puts("vault witness OK (13 errors, legacy+JVLT2 codec, imported origin/default entropy, native Secret redaction/wipe, long-origin hash, duplicate priority)");
    jet_rt_thread_detach();
    return 0;
}
