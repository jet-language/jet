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
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/wait.h>
#include <time.h>
#include <dirent.h>
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
extern void jet_rt_vault_linux_read_fixture(const Text *, const Text *, const Text *, const Text *);
extern void jet_rt_vault_identity_path_fixture(const Text *, _Bool, const Text *);
extern void jet_rt_vault_recipients_fixture(const Text *, int64_t, int64_t, const void *, const Text *);
extern void jet_rt_vault_plaintext_fixture(const ByteList *, int64_t, int64_t, int64_t, const void *);
extern void jet_rt_vault_linux_stream_fixture(int64_t, const Text *);
extern void jet_rt_vault_identity_stream_fixture(const Text *, const Text *);
extern void jet_rt_vault_uint_fixture(const Text *, _Bool, _Bool, int64_t, const Text *);
extern void jet_rt_vault_proc_parse_fixture(const Text *, _Bool, int64_t);
extern void jet_rt_vault_proc_read_fixture(void);
extern void jet_rt_vault_lock_fields_fixture(const Text *, int64_t, _Bool, int64_t, int64_t, const Text *);
extern void jet_rt_vault_lock_guard_fixture(const Text *, const Text *);
extern void jet_rt_vault_lock_acquire_fixture(const Text *);
extern void jet_rt_vault_lifecycle_strings_fixture(const Text *, const Text *, const Text *);
extern void jet_rt_vault_reference_fixture(void *, const Text *, const Text *);
extern void jet_rt_vault_plan_auth_fixture(const Text *, const Text *, const Text *, const Text *, const void *, const void *, _Bool, int64_t (*)(void *, void *, int64_t));
extern void jet_rt_vault_trust_path_fixture(const Text *, const Text *);
extern void jet_rt_vault_headless_fixture(void);
extern void jet_rt_vault_commit_boundary_fixture(const Text *, const Text *, const Text *, const Text *, const void *, const void *, const void *, int64_t (*)(void *, void *, int64_t));
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern char **environ;
extern void jet_rt_free(void *, int64_t, int64_t);
typedef struct { int64_t refs; int64_t (*invoke)(void *, const Text *); int64_t (*release)(void *); int64_t answer; } VaultAuthorizer;
extern void jet_rt_vault_set_test_authorizer(VaultAuthorizer *);
extern void jet_rt_vault_clear_test_authorizer(void);
extern void imported_origin_round_trips_and_invalid_combinations_reject(const Text *, const Text *);
extern void jet_rt_vault_crypto_fixture(const ByteList *);
extern void jet_rt_vault_crypto_unavailable_fixture(int64_t (*)(void *, void *, int64_t));
extern void jet_rt_vault_fixture_provision(Text **);
extern void absent_store_reads_do_not_require_mutation_recipients(const Text *, const Text *);
extern void typed_generate_rotate_retire_revoke_and_conflict(const Text *, const TextList *);
extern void authority_is_exact_and_denial_generates_no_key(const Text *, const Text *, const Text *, const Text *);
extern void linux_openat2_rejects_a_symlinked_vault_directory(const Text *, const Text *, const Text *);
extern void jet_rt_vault_fixture_prepared(const Text *, const Text *, const Text *, const Text *, void **);
extern void provider_change_invalidates_an_authorized_plan(const Text *, void *, void *);
extern void string_updates_preserve_typed_rows_in_the_shared_atomic_store(const Text *, const Text *, const Text *, const Text *, const Text *, const Text *);
extern void lifecycle_timestamps_do_not_decrease_when_wall_clock_rolls_back(const Text *, const Text *, const Text *, const Text *, const Text *, const Text *);
extern void decoder_rejects_a_downgraded_current_generation(const Text *, const Text *, const Text *, const Text *, const Text *);
extern void jet_rt_vault_fixture_import_plan(const Text *, const Text *, const Text *, void **);
extern void jet_rt_vault_fixture_operation(void *, const Text *);
extern void jet_rt_vault_fixture_binding_authorize(void *, const Text *, void **);
extern void exact_operation_preview_provider_and_session_are_bound(const Text *, void *, void *, const Text *);
extern void jet_rt_vault_fixture_generate_plan(const Text *, const Text *, const Text *, void **);
extern void jet_rt_vault_fixture_trust(void *, const Text *);
extern void jet_rt_vault_fixture_legacy_cipher(const Text *, const ByteList *, ByteList **);
extern void jet_rt_vault_fixture_bytes_drop(ByteList *);
extern void historical_pairs_migrate_with_the_previewed_uuid_and_exact_start_hash(const Text *, const Text *, const Text *, const Text *, const Text *, const Text *, const Text *);
extern void jet_rt_vault_fixture_fault_commit(const Text *, const Text *, const Text *, const Text *, const Text *, int64_t, void **);
extern void jet_rt_vault_fixture_durability(const Text *, const Text *, const Text *, const Text *, const Text *);
extern void linux_install_cancellation_and_durability_boundaries_are_literal(const Text *, const Text *, void *);
extern void jet_rt_vault_fixture_concurrent_prepare(const Text *, const Text *, const Text *, const Text *, const Text *, void **);
extern int64_t jet_rt_vault_fixture_spawn(int64_t (*)(intptr_t), intptr_t);
extern int64_t jet_rt_vault_fixture_join(int64_t, intptr_t *);
extern int64_t jet_rt_vault_fixture_rotation_worker(const Text *, void *, void *, const Text *);
extern void jet_rt_vault_fixture_reader_conflict(const Text *, const Text *);
extern void concurrent_reader_defers_backup_recovery_until_writer_releases_lock(const Text *, const Text *, const ByteList *);
extern void stale_lock_requires_age_owner_dead_pid_nonce_and_unchanged_store(const Text *, void *, const Text *);
extern void jet_rt_vault_fixture_guard(const Text *, void *, void **);
extern void lock_drop_never_unlinks_an_inode_substituted_after_its_check(void *, const Text *);
extern void anonymous_temp_substitution_never_reaches_the_visible_store(const Text *, const Text *, const Text *, const Text *, const Text *);
extern void jet_rt_vault_set_test_fault(const Text *);
extern void *jet_rt_vault_test_barrier_new(int64_t);
extern void jet_rt_vault_test_barrier_wait(void *);
extern void jet_rt_vault_test_barrier_drop(void *);
extern void jet_rt_vault_set_test_backup_barrier(const Text *, void *, void *);
static unsigned unavailable_calls;
static int64_t unavailable_entropy(void *context, void *out, int64_t count) {
    assert(context == NULL && count == 32);
    ++unavailable_calls;
    memset(out, 0xa5, (size_t)count);
    return 0;
}
static struct { void *data; int64_t capacity; unsigned frees; } observed[2];
static size_t observed_count;
static unsigned stream_error_frees;
static int observe_stream_error;
void __wrap_jet_rt_free(void *data, int64_t size, int64_t align) {
    if (observe_stream_error && size == 8192) {
        assert(align == 8 && ++stream_error_frees == 1);
        for (int64_t byte = 0; byte < size; ++byte) assert(((const unsigned char *)data)[byte] == 0);
    }
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
    Text ref_display = text("repo:transport@v18446744073709551615");
    Text ref_debug = text("repo:transport@v18446744073709551615#02020202");
    jet_rt_vault_reference_fixture(parts[0], &ref_display, &ref_debug);
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
    unsigned char raw[8193];
    for (size_t i = 0; i < sizeof(raw); ++i) raw[i] = (unsigned char)(i % 256);
    Text binary = {raw, 8192, 0};
    jet_rt_vault_linux_read_fixture(&path, &names[2], &names[1], &binary);
    binary.length = 8193;
    jet_rt_vault_linux_read_fixture(&path, &names[2], &names[1], &binary);
    assert(chmod(child, 0777) == 0);
    jet_rt_vault_linux_dir_fixture(&path, 7);
    assert(chmod(child, 0700) == 0);
    assert(rename(child, actual) == 0 && symlink(actual, child) == 0);
    jet_rt_vault_linux_dir_fixture(&path, 7);
    assert(unlink(child) == 0 && rmdir(actual) == 0 && rmdir(root) == 0);
}
static void input_write_all(int fd, const unsigned char *data, size_t length) {
    size_t at = 0;
    while (at < length) {
        ssize_t written = write(fd, data + at, length - at);
        if (written < 0 && errno == EINTR) continue;
        assert(written > 0);
        at += (size_t)written;
    }
}
static volatile sig_atomic_t identity_interrupts;
static void identity_interrupt(int signal_number) {
    (void)signal_number;
    ++identity_interrupts;
}
static void wait_input_writer(pid_t child) {
    int status;
    pid_t waited;
    do { waited = waitpid(child, &status, 0); } while (waited < 0 && errno == EINTR);
    assert(waited == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
}
static void stream_read_fixtures(void) {
    unsigned char raw[8193];
    for (size_t i = 0; i < sizeof(raw); ++i) raw[i] = (unsigned char)(i % 256);
    for (size_t length = 8192; length <= sizeof(raw); ++length) {
        int descriptors[2];
        assert(pipe(descriptors) == 0);
        pid_t child = fork();
        assert(child >= 0);
        if (child == 0) {
            assert(close(descriptors[0]) == 0);
            input_write_all(descriptors[1], raw, length);
            assert(close(descriptors[1]) == 0);
            _exit(0);
        }
        assert(close(descriptors[1]) == 0);
        Text expected = {raw, (int64_t)length, 0};
        jet_rt_vault_linux_stream_fixture(descriptors[0], &expected);
        assert(close(descriptors[0]) == 0);
        wait_input_writer(child);
    }
    int null_file = open("/dev/null", O_RDONLY);
    assert(null_file >= 0);
    Text empty = text("");
    jet_rt_vault_linux_stream_fixture(null_file, &empty);
    assert(close(null_file) == 0);
    observe_stream_error = 1;
    jet_rt_vault_linux_stream_fixture(-1, NULL);
    observe_stream_error = 0;
    assert(stream_error_frees == 1);

    char root[] = "/tmp/jet-vault-identity-XXXXXX", keys[128], identity[128];
    assert(mkdtemp(root) != NULL);
    assert(snprintf(keys, sizeof(keys), "%s/keys", root) > 0);
    assert(snprintf(identity, sizeof(identity), "%s/keys/secrets.identity", root) > 0);
    assert(mkdir(keys, 0700) == 0 && mkfifo(identity, 0600) == 0);
    struct sigaction action = {0}, previous;
    action.sa_handler = identity_interrupt;
    assert(sigemptyset(&action.sa_mask) == 0 && sigaction(SIGUSR1, &action, &previous) == 0);
    Text expected = text("AGE-SECRET-KEY-1GQ9778VQXMMJVE8SK7J6VT8UJ4HDQAJUVSFCWCM02D8GEWQ72PVQ2Y5J33");
    pid_t parent = getpid(), child = fork();
    assert(child >= 0);
    if (child == 0) {
        const struct timespec pause = {0, 50000000};
        for (unsigned i = 0; i < 4; ++i) {
            assert(nanosleep(&pause, NULL) == 0 && kill(parent, SIGUSR1) == 0);
        }
        int writer = open(identity, O_WRONLY | O_NONBLOCK);
        assert(writer >= 0);
        input_write_all(writer, expected.data, (size_t)expected.length);
        assert(close(writer) == 0);
        _exit(0);
    }
    Text path = text(root);
    jet_rt_vault_identity_stream_fixture(&path, &expected);
    wait_input_writer(child);
    assert(identity_interrupts > 0 && sigaction(SIGUSR1, &previous, NULL) == 0);
    assert(unlink(identity) == 0 && rmdir(keys) == 0 && rmdir(root) == 0);
}
static void fixture_hash(unsigned char out[32], const char *hex) {
    assert(strlen(hex) == 64);
    for (size_t i = 0; i < 32; ++i) {
        unsigned value;
        assert(sscanf(hex + i * 2, "%2x", &value) == 1);
        out[i] = (unsigned char)value;
    }
}
static void canonical_read_fixtures(void) {
    const char *current[] = {".", "./", "./.", ".//", "././", ".///.///"};
    for (size_t i = 0; i < sizeof(current) / sizeof(current[0]); ++i) {
        Text root = text(current[i]);
        jet_rt_vault_identity_path_fixture(&root, 1, NULL);
    }
    const char *roots[] = {"", "/", "repo", "repo/", "a/..", "..", "./.."};
    const char *paths[] = {"keys/secrets.identity", "/keys/secrets.identity", "repo/keys/secrets.identity",
        "repo/keys/secrets.identity", "a/../keys/secrets.identity", "../keys/secrets.identity", "./../keys/secrets.identity"};
    for (size_t i = 0; i < sizeof(roots) / sizeof(roots[0]); ++i) {
        Text root = text(roots[i]), expected = text(paths[i]);
        jet_rt_vault_identity_path_fixture(&root, 0, &expected);
    }
    Text recipient = text("age1t7rxyev2z3rw82stdlrrepyc39nvn86l5078zqkf5uasdy86jp6svpy7pa");
    unsigned char hash[32];
    fixture_hash(hash, "1a9258274b198888562a2487abb14465a488f7858694a873b1f2e39459bf0795");
    jet_rt_vault_recipients_fixture(&recipient, 1, -1, hash, &recipient);
    Text trimmed = text("\xc2\x85 \tage1t7rxyev2z3rw82stdlrrepyc39nvn86l5078zqkf5uasdy86jp6svpy7pa\xe2\x80\xa8\r\n\n");
    jet_rt_vault_recipients_fixture(&trimmed, 1, -1, hash, &recipient);
    char upper[63], duplicates[126], nonseparator[127];
    for (size_t i = 0; i < 62; ++i) {
        unsigned char byte = recipient.data[i];
        upper[i] = (char)(byte >= 'a' && byte <= 'z' ? byte - 'a' + 'A' : byte);
    }
    upper[62] = 0;
    Text uppercase = text(upper);
    jet_rt_vault_recipients_fixture(&uppercase, 1, -1, hash, &recipient);
    memcpy(duplicates, recipient.data, 62);
    duplicates[62] = '\n';
    memcpy(duplicates + 63, upper, 63);
    Text duplicate = text(duplicates);
    jet_rt_vault_recipients_fixture(&duplicate, 0, 8, NULL, NULL);
    memcpy(nonseparator, recipient.data, 62);
    nonseparator[62] = (char)0xc2;
    nonseparator[63] = (char)0x85;
    memcpy(nonseparator + 64, upper, 63);
    Text invalid_line = text(nonseparator), empty = text(" \n\t\r\n");
    jet_rt_vault_recipients_fixture(&invalid_line, 0, 8, NULL, NULL);
    jet_rt_vault_recipients_fixture(&empty, 0, 8, NULL, NULL);
    const unsigned char invalid_utf8[] = {0xff};
    Text invalid = {invalid_utf8, 1, 0};
    jet_rt_vault_recipients_fixture(&invalid, 0, 8, NULL, NULL);
    static unsigned char historical[] = {
        3,0,0,0, 4,0,0,0,'z','e','t','a',1,0,0,0,'Z',
        5,0,0,0,'a','l','p','h','a',1,0,0,0,'A', 4,0,0,0,'b','e','t','a',1,0,0,0,'B'
    };
    ByteList plain = {(int64_t)sizeof(historical), (int64_t)sizeof(historical), historical};
    fixture_hash(hash, "2c953f4ed1de1c8c078565f26cc0270f06d682d9baaa93e9917dbd60d68b471c");
    jet_rt_vault_plaintext_fixture(&plain, 1, 3, -1, hash);
    static unsigned char duplicate_bad_names[] = {
        2,0,0,0, 5,0,0,0,' ','.','.','/',' ',0,0,0,0, 5,0,0,0,' ','.','.','/',' ',0,0,0,0
    };
    plain = (ByteList){(int64_t)sizeof(duplicate_bad_names), (int64_t)sizeof(duplicate_bad_names), duplicate_bad_names};
    jet_rt_vault_plaintext_fixture(&plain, 0, 0, 8, NULL);
    unsigned char typed[76] = {'J','V','L','T',2};
    memset(typed + 8, 7, 16);
    typed[24] = 1;
    fixture_hash(typed + 44, "936503d3d3b1f85212ffd9fbcb83ccbd76d7610e0c459ceea26776c6a3e00039");
    plain = (ByteList){76, 76, typed};
    fixture_hash(hash, "031ee22add6dd48f32c41cf83ef105e40df8a7aee31e8cc9bcfe0df4640bc6fd");
    jet_rt_vault_plaintext_fixture(&plain, 2, 0, -1, hash);
    typed[75] ^= 1;
    jet_rt_vault_plaintext_fixture(&plain, 0, 0, 8, NULL);
}
static void process_start_fixtures(void) {
    static const struct { const char *input, *decimal; int narrow, valid; int64_t value; } numbers[] = {
        {"0", "0", 0, 1, 0}, {"+0", "0", 1, 1, 0}, {"000000000000001", "1", 0, 1, 1},
        {"4294967295", "4294967295", 1, 1, 4294967295},
        {"4294967296", NULL, 1, 0, 0}, {"+4294967295", "4294967295", 1, 1, 4294967295},
        {"9223372036854775807", "9223372036854775807", 0, 1, INT64_MAX},
        {"9223372036854775808", "9223372036854775808", 0, 1, INT64_MIN},
        {"+18446744073709551615", "18446744073709551615", 0, 1, -1},
        {"00018446744073709551615", "18446744073709551615", 0, 1, -1},
        {"18446744073709551616", NULL, 0, 0, 0}, {"184467440737095516150", NULL, 0, 0, 0},
        {"", NULL, 0, 0, 0}, {"+", NULL, 0, 0, 0}, {"-0", NULL, 0, 0, 0},
        {" 1", NULL, 0, 0, 0}, {"1 ", NULL, 0, 0, 0}, {"0x10", NULL, 0, 0, 0},
        {"\xef\xbc\x91", NULL, 0, 0, 0}
    };
    for (size_t i = 0; i < sizeof(numbers) / sizeof(numbers[0]); ++i) {
        Text input = text(numbers[i].input), decimal = text(numbers[i].decimal ? numbers[i].decimal : "");
        jet_rt_vault_uint_fixture(&input, numbers[i].narrow, numbers[i].valid, numbers[i].value, &decimal);
    }
    char stat[256];
    const char *values[] = {"+18446744073709551615", "0", "18446744073709551616", "-1", "+"};
    for (size_t i = 0; i < sizeof(values) / sizeof(values[0]); ++i) {
        int written = snprintf(stat, sizeof(stat), "9 (has ) ) nested) ");
        assert(written > 0);
        size_t at = (size_t)written;
        for (unsigned field = 0; field < 19; ++field) {
            written = snprintf(stat + at, sizeof(stat) - at, "%s", field == 7 ? "\xc2\x85x\xe2\x80\xa8" : "x ");
            assert(written > 0 && (size_t)written < sizeof(stat) - at);
            at += (size_t)written;
        }
        written = snprintf(stat + at, sizeof(stat) - at, "%s tail", values[i]);
        assert(written > 0 && (size_t)written < sizeof(stat) - at);
        Text input = text(stat);
        jet_rt_vault_proc_parse_fixture(&input, i < 2, i == 0 ? -1 : 0);
    }
    Text no_close = text("no closing parenthesis"), too_short = text("1 (name) 0 0");
    jet_rt_vault_proc_parse_fixture(&no_close, 0, 0);
    jet_rt_vault_proc_parse_fixture(&too_short, 0, 0);
    unsigned char invalid_bytes[] = {'1',' ','(',0xff,')'};
    Text invalid = {invalid_bytes, (int64_t)sizeof(invalid_bytes), 0};
    jet_rt_vault_proc_parse_fixture(&invalid, 0, 0);
    jet_rt_vault_proc_read_fixture();
}
static void lock_guard_fixtures(void) {
    char root[] = "/tmp/jet-vault-guard-XXXXXX", dir[128], lock[160], acquired[160], done[192];
    assert(mkdtemp(root) != NULL);
    assert(snprintf(dir, sizeof(dir), "%s/.jet", root) > 0 && mkdir(dir, 0700) == 0);
    assert(snprintf(lock, sizeof(lock), "%s/.secrets.age.lock", dir) > 0);
    assert(snprintf(acquired, sizeof(acquired), "%s/.secrets.age.lock.acquired", dir) > 0);
    assert(snprintf(done, sizeof(done), "%s/.secrets.age.lock.done.ab000000000000000000000000000000", dir) > 0);
    Text path = text(root), fault = text("substitute-lock-after-check");
    jet_rt_vault_lock_guard_fixture(&path, NULL);
    assert(access(lock, F_OK) == -1 && errno == ENOENT);
    assert(access(done, F_OK) == -1 && errno == ENOENT);
    jet_rt_vault_lock_acquire_fixture(&path);
    assert(access(lock, F_OK) == -1 && errno == ENOENT);
    jet_rt_vault_lock_guard_fixture(&path, &fault);
    struct stat before, after;
    assert(stat(lock, &before) == 0);
    char bytes[12] = {0};
    int fd = open(lock, O_RDONLY);
    assert(fd >= 0 && read(fd, bytes, sizeof(bytes)) == 11 && close(fd) == 0);
    assert(memcmp(bytes, "replacement", 11) == 0);
    assert(stat(lock, &after) == 0 && before.st_dev == after.st_dev && before.st_ino == after.st_ino);
    assert(access(done, F_OK) == -1 && errno == ENOENT);
    assert(unlink(lock) == 0 && unlink(acquired) == 0 && rmdir(dir) == 0 && rmdir(root) == 0);
}
static void lock_fields_fixtures(void) {
    static const char base[] =
        "repo=00000000000000000000000000000000 revision=0 pid=0 process-start=0 "
        "nonce=ABCDEF00000000000000000000000000 "
        "hash=0000000000000000000000000000000000000000000000000000000000000000";
    static const struct { const char *suffix; int valid; int64_t revision, pid, start; const char *nonce; } cases[] = {
        {"", 1, 0, 0, 0, "ABCDEF00000000000000000000000000"},
        {"\xc2\x85pid=+4294967295\xe2\x80\xa8process-start=+18446744073709551615 revision=+18446744073709551615", 1, -1, 4294967295LL, -1, "ABCDEF00000000000000000000000000"},
        {" pid=17 pid=no", 0, 0, 0, 0, ""},
        {" revision=1 revision=", 0, 0, 0, 0, ""},
        {" process-start=1 process-start=18446744073709551616", 0, 0, 0, 0, ""},
        {" pid=4294967296", 0, 0, 0, 0, ""},
        {" revision=-0", 0, 0, 0, 0, ""},
        {" repo=", 0, 0, 0, 0, ""},
        {" hash=", 0, 0, 0, 0, ""},
        {" hash=0000000000000000000000000000000000000000000000000000000000000000=extra", 0, 0, 0, 0, ""},
        {" nonce=00000000000000000000000000000000", 0, 0, 0, 0, ""},
        {" nonce=GBCDEF00000000000000000000000000", 0, 0, 0, 0, ""},
        {" nonce=abcdef00000000000000000000000000", 1, 0, 0, 0, "abcdef00000000000000000000000000"},
        {" pid=no pid=+0 revision=no revision=0 process-start=no process-start=0 ignored=a=b noequals", 1, 0, 0, 0, "ABCDEF00000000000000000000000000"},
        {" nonce=x nonce=ABCDEF00000000000000000000000000", 1, 0, 0, 0, "ABCDEF00000000000000000000000000"},
        {"\xff", 0, 0, 0, 0, ""}
    };
    char buffer[512];
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); ++i) {
        int n = snprintf(buffer, sizeof(buffer), "%s%s", base, cases[i].suffix);
        assert(n >= 0 && (size_t)n < sizeof(buffer));
        Text input = text(buffer), nonce = text(cases[i].nonce);
        jet_rt_vault_lock_fields_fixture(&input, cases[i].revision, cases[i].valid, cases[i].pid, cases[i].start, &nonce);
    }
    // Missing fields independently: remove each entire token from the literal.
    for (unsigned field = 0; field < 6; ++field) {
        const char *begin = base;
        for (unsigned i = 0; i < field; ++i) begin = strchr(begin, ' ') + 1;
        const char *end = strchr(begin, ' ');
        if (!end) end = base + strlen(base); else ++end;
        size_t prefix = (size_t)(begin - base);
        memcpy(buffer, base, prefix);
        strcpy(buffer + prefix, end);
        Text input = text(buffer), nonce = text("");
        jet_rt_vault_lock_fields_fixture(&input, 0, 0, 0, 0, &nonce);
    }
}
static const char preview_golden[] = "store SigningKey release 0 -> 2 in repository 07070707070707070707070707070707 at revision 9; reason: <none>; authority reason: initial signer; expires_unix_ms: 123456";
static unsigned authorizer_calls;
static int64_t plan_authorizer(void *raw, const Text *preview) {
    VaultAuthorizer *closure = raw;
    assert(preview->length == (int64_t)strlen(preview_golden));
    assert(memcmp(preview->data, preview_golden, strlen(preview_golden)) == 0);
    ++authorizer_calls;
    return closure->answer;
}
static int64_t plan_authorizer_release(void *raw) {
    jet_rt_free(raw, (int64_t)sizeof(VaultAuthorizer), 8);
    return 0;
}
static void plan_auth_fixtures(void) {
    unsigned char op[32], preview[32];
    fixture_hash(op, "75c0c0605ef7585d948b1d70fa281d2670096615407872a28d5ff804a7bdea69");
    fixture_hash(preview, "22e75a073b0c9e7c8abcf54aa5939e36633347addd28040c0736e0a4fc4cc43d");
    Text name = text("release"), empty = text(""), reason = text("initial signer"), expected = text(preview_golden);
    for (int granted = 0; granted < 2; ++granted) {
        VaultAuthorizer *closure = jet_rt_alloc((int64_t)sizeof(VaultAuthorizer), 8);
        *closure = (VaultAuthorizer){1, plan_authorizer, plan_authorizer_release, granted};
        jet_rt_vault_set_test_authorizer(closure);
        jet_rt_vault_plan_auth_fixture(&name, &empty, &reason, &expected, op, preview, granted, unavailable_entropy);
        jet_rt_vault_clear_test_authorizer();
    }
    unsigned char rotation_hash[32], retired_hash[32], revoked_hash[32];
    fixture_hash(rotation_hash, "541781f99f753e38913a068b209eff23d965787264efa4f086ea20008cb98911");
    fixture_hash(retired_hash, "0399f46cba25ec76bfe7f6b04d45c3ed5b580307280548f7bb3a8680a71afded");
    fixture_hash(revoked_hash, "9e47845eef7978b4b6e3211a60f7679faec87ad1b9f9c966c29acf4da1d3223e");
    Text retired = text("retired"), revoked = text("revoked");
    jet_rt_vault_commit_boundary_fixture(&name, &empty, &retired, &revoked, rotation_hash, retired_hash, revoked_hash, unavailable_entropy);
    assert(authorizer_calls == 2 && unavailable_calls == 1);
}
static void trust_home_fixtures(void) {
    Text empty = text(""), relative = text(".jet/trust"), trailing = text("/tmp/raw/"), joined = text("/tmp/raw/.jet/trust");
    Text raw = text("raw\xff"), raw_joined = text("raw\xff/.jet/trust");
    jet_rt_vault_trust_path_fixture(&empty, &relative);
    jet_rt_vault_trust_path_fixture(&trailing, &joined);
    jet_rt_vault_trust_path_fixture(&raw, &raw_joined);
    char root[] = "/tmp/jet-vault-home-XXXXXX", dir[128], trust[160], invalid[128], raw_dir[160], raw_trust[192], slash[128];
    assert(mkdtemp(root) != NULL);
    assert(snprintf(dir, sizeof(dir), "%s/.jet", root) > 0 && mkdir(dir, 0700) == 0);
    assert(snprintf(trust, sizeof(trust), "%s/trust", dir) > 0);
    assert(snprintf(invalid, sizeof(invalid), "%s/raw\xff", root) > 0 && mkdir(invalid, 0700) == 0);
    assert(snprintf(raw_dir, sizeof(raw_dir), "%s/.jet", invalid) > 0 && mkdir(raw_dir, 0700) == 0);
    assert(snprintf(raw_trust, sizeof(raw_trust), "%s/trust", raw_dir) > 0);
    assert(snprintf(slash, sizeof(slash), "%s/", root) > 0);
    static const unsigned char grant[] = "\xc2\x85grant:repo:vault.write:00000000000000000000000000000000\xe2\x80\xa8\r\n";
    const char *files[] = {trust, raw_trust};
    for (unsigned i = 0; i < 2; ++i) {
        int fd = open(files[i], O_WRONLY | O_CREAT | O_EXCL, 0600);
        assert(fd >= 0);
        input_write_all(fd, grant, sizeof(grant) - 1);
        assert(close(fd) == 0);
    }
    const char *homes[] = {"", slash, invalid};
    for (unsigned i = 0; i < 3; ++i) {
        pid_t child = fork(); assert(child >= 0);
        if (child == 0) {
            assert(chdir(root) == 0 && setenv("HOME", homes[i], 1) == 0);
            char *const args[] = {"/proc/self/exe", "--vault-home-grant", NULL};
            execv(args[0], args);
            _exit(127);
        }
        int status; assert(waitpid(child, &status, 0) == child);
        assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    }
    assert(unlink(trust) == 0 && unlink(raw_trust) == 0);
    assert(rmdir(dir) == 0 && rmdir(raw_dir) == 0 && rmdir(invalid) == 0 && rmdir(root) == 0);
}
// Exact suite filesystem transport. No vault policy lives in C.
static void fixture_remove_tree(const char *path) {
    struct stat st;
    assert(lstat(path, &st) == 0);
    if (!S_ISDIR(st.st_mode)) { assert(unlink(path) == 0); return; }
    DIR *dir = opendir(path); assert(dir != NULL);
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        char child[512];
        int count = snprintf(child, sizeof(child), "%s/%s", path, entry->d_name);
        assert(count > 0 && (size_t)count < sizeof(child));
        fixture_remove_tree(child);
    }
    assert(closedir(dir) == 0 && rmdir(path) == 0);
}
static void fixture_path(char *out, size_t capacity, const char *root, const char *relative) {
    int count = snprintf(out, capacity, "%s/%s", root, relative);
    assert(count > 0 && (size_t)count < capacity);
}
static void fixture_write(const char *path, const unsigned char *bytes, size_t length) {
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600); assert(fd >= 0);
    input_write_all(fd, bytes, length);
    assert(close(fd) == 0);
}
static void fixture_recipients(const char *root, const Text *recipient) {
    char path[512]; fixture_path(path, sizeof(path), root, ".jet/secrets-recipients");
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600); assert(fd >= 0);
    input_write_all(fd, recipient->data, (size_t)recipient->length);
    input_write_all(fd, (const unsigned char *)"\n", 1);
    assert(close(fd) == 0);
}
static Text fixture_scratch(char *root, int provisioned) {
    assert(mkdtemp(root) != NULL);
    char path[512]; fixture_path(path, sizeof(path), root, ".jet");
    assert(mkdir(path, 0700) == 0);
    if (provisioned) {
        fixture_path(path, sizeof(path), root, "keys"); assert(mkdir(path, 0700) == 0);
        Text *keys[2]; jet_rt_vault_fixture_provision(keys);
        fixture_path(path, sizeof(path), root, "keys/secrets.identity");
        fixture_write(path, keys[0]->data, (size_t)keys[0]->length);
        fixture_recipients(root, keys[1]);
        jet_rt_string_drop(keys[0]); jet_rt_string_drop(keys[1]);
    }
    return text(root);
}
static int64_t filesystem_authorizer(void *raw, const Text *preview) {
    VaultAuthorizer *closure = raw;
    if (!closure->answer) {
        static const char needle[] = "generate SigningKey release";
        int found = 0;
        for (int64_t at = 0; at + (int64_t)sizeof(needle) - 1 <= preview->length; ++at)
            if (memcmp(preview->data + at, needle, sizeof(needle) - 1) == 0) { found = 1; break; }
        assert(found);
    }
    return closure->answer;
}
static void fixture_authorizer(int granted) {
    VaultAuthorizer *closure = jet_rt_alloc((int64_t)sizeof(VaultAuthorizer), 8);
    *closure = (VaultAuthorizer){1, filesystem_authorizer, plan_authorizer_release, granted};
    jet_rt_vault_set_test_authorizer(closure);
}
static void filesystem_batch_a(void) {
    Text name = text("release"), empty = text(""), initial = text("initial signer");
    char absent[] = "/tmp/jet-vault-absent-read-XXXXXX";
    Text root = fixture_scratch(absent, 0);
    absent_store_reads_do_not_require_mutation_recipients(&root, &name);
    fixture_remove_tree(absent);
    char lifecycle[] = "/tmp/jet-vault-lifecycle-XXXXXX";
    root = fixture_scratch(lifecycle, 1); fixture_authorizer(1);
    Text lifecycle_texts[] = {name, text("transport"), empty, text("initial release key"),
        text("stale write"), text("quarterly rotation"), text("suspected exposure"), text("incident response")};
    Text *lifecycle_items[8];
    for (size_t i = 0; i < 8; ++i) lifecycle_items[i] = &lifecycle_texts[i];
    TextList texts = {8, 8, lifecycle_items};
    typed_generate_rotate_retire_revoke_and_conflict(&root, &texts);
    fixture_remove_tree(lifecycle);
    char authority[] = "/tmp/jet-vault-authority-XXXXXX", path[512];
    root = fixture_scratch(authority, 1); fixture_authorizer(0);
    Text unapproved = text("unapproved");
    authority_is_exact_and_denial_generates_no_key(&root, &name, &empty, &unapproved);
    fixture_path(path, sizeof(path), authority, ".jet/secrets.age");
    assert(access(path, F_OK) == -1 && errno == ENOENT);
    fixture_remove_tree(authority);
    char downgrade[] = "/tmp/jet-vault-downgrade-XXXXXX";
    root = fixture_scratch(downgrade, 1); fixture_authorizer(1);
    Text first = text("first"), second = text("second");
    decoder_rejects_a_downgraded_current_generation(&root, &name, &empty, &first, &second);
    fixture_remove_tree(downgrade);
    char symlink_root[] = "/tmp/jet-vault-symlink-XXXXXX", actual[512];
    root = fixture_scratch(symlink_root, 0);
    fixture_path(actual, sizeof(actual), symlink_root, "actual"); assert(mkdir(actual, 0700) == 0);
    fixture_path(path, sizeof(path), symlink_root, ".jet");
    assert(rmdir(path) == 0 && symlink(actual, path) == 0);
    linux_openat2_rejects_a_symlinked_vault_directory(&root, &name, &empty);
    fixture_remove_tree(symlink_root);
    char changed[] = "/tmp/jet-vault-provider-change-XXXXXX";
    root = fixture_scratch(changed, 1); fixture_authorizer(1);
    void *authority_objects[2];
    jet_rt_vault_fixture_prepared(&root, &name, &empty, &initial, authority_objects);
    Text *other[2]; jet_rt_vault_fixture_provision(other);
    fixture_recipients(changed, other[1]);
    jet_rt_string_drop(other[0]); jet_rt_string_drop(other[1]);
    provider_change_invalidates_an_authorized_plan(&root, authority_objects[0], authority_objects[1]);
    fixture_path(path, sizeof(path), changed, ".jet/secrets.age");
    assert(access(path, F_OK) == -1 && errno == ENOENT);
    fixture_remove_tree(changed);
    char hybrid[] = "/tmp/jet-vault-hybrid-store-XXXXXX";
    root = fixture_scratch(hybrid, 1); fixture_authorizer(1);
    Text token = text("token"), secret = text("secret");
    string_updates_preserve_typed_rows_in_the_shared_atomic_store(&root, &name, &empty, &initial, &token, &secret);
    fixture_remove_tree(hybrid);
    char clock[] = "/tmp/jet-vault-clock-rollback-XXXXXX";
    root = fixture_scratch(clock, 1); fixture_authorizer(1);
    Text rotation = text("rollback rotation"), fault = text("clock-rollback");
    lifecycle_timestamps_do_not_decrease_when_wall_clock_rolls_back(&root, &name, &empty, &initial, &rotation, &fault);
    fixture_remove_tree(clock);
}
static void fixture_append(unsigned char *out, size_t *length, const void *bytes, size_t count) {
    assert(*length + count <= 512);
    memcpy(out + *length, bytes, count); *length += count;
}
static void fixture_append_uint(unsigned char *out, size_t *length, uint64_t value, size_t count) {
    unsigned char bytes[8];
    for (size_t i = 0; i < count; ++i) bytes[i] = (unsigned char)(value >> (8 * i));
    fixture_append(out, length, bytes, count);
}
static void filesystem_batch_b(void) {
    Text name = text("release"), empty = text(""), reason = text("approved release signer");
    char binding[] = "/tmp/jet-vault-binding-XXXXXX";
    Text root = fixture_scratch(binding, 1);
    void *raw_plan; jet_rt_vault_fixture_import_plan(&root, &name, &empty, &raw_plan);
    const unsigned char *plan = raw_plan;
    unsigned char operation[512], preview[512], zeros[72] = {0};
    size_t op_len = 0, preview_len = 0;
    fixture_append(operation, &op_len, "JVLT2 operation", 15);
    fixture_append(operation, &op_len, plan + 40, 16);
    fixture_append(operation, &op_len, plan + 56, 8);
    fixture_append(operation, &op_len, plan + 64, 32);
    fixture_append(operation, &op_len, plan + 96, 32);
    fixture_append(operation, &op_len, (const unsigned char[]){2,1}, 2);
    fixture_append_uint(operation, &op_len, 7, 2);
    fixture_append(operation, &op_len, "release", 7);
    fixture_append(operation, &op_len, zeros, 1);
    fixture_append(operation, &op_len, zeros, 16);
    fixture_append_uint(operation, &op_len, 0, 8);
    fixture_append(operation, &op_len, zeros, 16);
    fixture_append(operation, &op_len, zeros, 32);
    fixture_append_uint(operation, &op_len, 0, 2);
    fixture_append(operation, &op_len, plan + 128, 32);
    fixture_append(operation, &op_len, plan + 160, 32);
    fixture_append(operation, &op_len, zeros, 32);
    Text operation_stream = {operation, (int64_t)op_len, 0};
    jet_rt_vault_fixture_operation(raw_plan, &operation_stream);
    fixture_authorizer(1);
    void *write; jet_rt_vault_fixture_binding_authorize(raw_plan, &reason, &write);
    fixture_append(preview, &preview_len, "JVLT2 authority preview", 23);
    fixture_append(preview, &preview_len, plan + 216, 32);
    fixture_append(preview, &preview_len, (const unsigned char[]){2,1}, 2);
    fixture_append(preview, &preview_len, plan + 40, 16);
    fixture_append_uint(preview, &preview_len, 0, 8);
    fixture_append_uint(preview, &preview_len, 7, 2);
    fixture_append(preview, &preview_len, "release", 7);
    fixture_append_uint(preview, &preview_len, 0, 8);
    fixture_append_uint(preview, &preview_len, 1, 8);
    fixture_append_uint(preview, &preview_len, 0, 2);
    fixture_append_uint(preview, &preview_len, 23, 2);
    fixture_append(preview, &preview_len, "approved release signer", 23);
    fixture_append(preview, &preview_len, plan + 280, 8);
    Text preview_stream = {preview, (int64_t)preview_len, 0};
    exact_operation_preview_provider_and_session_are_bound(&root, raw_plan, write, &preview_stream);
    jet_rt_vault_fixture_generate_plan(&root, &name, &empty, &raw_plan);
    plan = raw_plan;
    char trust[512]; fixture_path(trust, sizeof(trust), binding, "trust");
    char grant[80] = "grant:user:vault.write:";
    static const char hex[] = "0123456789abcdef";
    size_t at = strlen(grant);
    for (size_t i = 0; i < 16; ++i) {
        unsigned char byte = plan[40 + i];
        grant[at++] = hex[byte >> 4]; grant[at++] = hex[byte & 15];
    }
    grant[at++] = '\n';
    fixture_write(trust, (const unsigned char *)grant, at);
    Text trust_path = text(trust); jet_rt_vault_fixture_trust(raw_plan, &trust_path);
    fixture_remove_tree(binding);

    char migration[] = "/tmp/jet-vault-migration-XXXXXX", path[512];
    root = fixture_scratch(migration, 1);
    fixture_path(path, sizeof(path), migration, ".jet/secrets-recipients");
    unsigned char recipient_raw[128];
    int fd = open(path, O_RDONLY); assert(fd >= 0);
    ssize_t got = read(fd, recipient_raw, sizeof(recipient_raw));
    assert(got == 63 && close(fd) == 0);
    Text recipient = {recipient_raw, (int64_t)got, 0};
    unsigned char legacy_raw[] = {1,0,0,0, 5,0,0,0, 't','o','k','e','n', 6,0,0,0, 's','e','c','r','e','t'};
    ByteList legacy = {(int64_t)sizeof(legacy_raw), 0, legacy_raw};
    ByteList *ciphertext; jet_rt_vault_fixture_legacy_cipher(&recipient, &legacy, &ciphertext);
    fixture_path(path, sizeof(path), migration, ".jet/secrets.age");
    fixture_write(path, ciphertext->data, (size_t)ciphertext->length);
    jet_rt_vault_fixture_bytes_drop(ciphertext);
    static const unsigned char starting_raw[] = {
        'J','V','L','T','1',' ','s','t','a','r','t','i','n','g',' ','s','t','o','r','e',
        1,0,0,0, 5,0, 't','o','k','e','n', 6,0,0,0, 's','e','c','r','e','t'
    };
    Text starting = {starting_raw, (int64_t)sizeof(starting_raw), 0};
    Text token = text("token"), value = text("secret"), migrate = text("migrate and provision");
    fixture_authorizer(1);
    historical_pairs_migrate_with_the_previewed_uuid_and_exact_start_hash(&root, &name, &empty, &migrate, &token, &value, &starting);
    fixture_remove_tree(migration);
}
static void fixture_no_name(const char *root, const char *needle, const char *exact) {
    char path[512]; fixture_path(path, sizeof(path), root, ".jet");
    DIR *dir = opendir(path); assert(dir != NULL);
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        assert(strstr(entry->d_name, needle) == NULL);
        if (exact != NULL) assert(strcmp(entry->d_name, exact) != 0);
    }
    assert(closedir(dir) == 0);
}
static unsigned char *fixture_read(const char *path, size_t *length) {
    int fd = open(path, O_RDONLY); assert(fd >= 0);
    struct stat st; assert(fstat(fd, &st) == 0 && st.st_size >= 0);
    *length = (size_t)st.st_size;
    unsigned char *data = malloc(*length + 1); assert(data != NULL);
    size_t at = 0;
    while (at < *length) {
        ssize_t got = read(fd, data + at, *length - at);
        if (got < 0 && errno == EINTR) continue;
        assert(got > 0); at += (size_t)got;
    }
    assert(close(fd) == 0);
    return data;
}
typedef struct { const Text *root, *fault; void *plan, *write; } VaultWriter;
static int64_t fixture_writer(intptr_t raw) {
    VaultWriter *writer = (VaultWriter *)raw;
    return jet_rt_vault_fixture_rotation_worker(writer->root, writer->plan, writer->write, writer->fault);
}
static void fixture_hex(char *out, const unsigned char *raw, size_t count) {
    static const char digits[] = "0123456789abcdef";
    for (size_t i = 0; i < count; ++i) {
        out[2 * i] = digits[raw[i] >> 4]; out[2 * i + 1] = digits[raw[i] & 15];
    }
    out[2 * count] = '\0';
}
static void filesystem_batch_c(void) {
    Text name = text("release"), empty = text(""), initial = text("initial signer");
    Text cancel_before = text("cancel-before-install"), cancel_after = text("cancel-after-install");
    Text durability = text("durability-after-install"), rotate_fault = text("rotate with durability fault");
    char faults[] = "/tmp/jet-vault-faults-XXXXXX", store_path[512], backup_path[512];
    Text root = fixture_scratch(faults, 1); fixture_authorizer(1);
    void *first = NULL;
    jet_rt_vault_fixture_fault_commit(&root, &name, &empty, &initial, &cancel_before, 6, &first);
    fixture_path(store_path, sizeof(store_path), faults, ".jet/secrets.age");
    assert(access(store_path, F_OK) == -1 && errno == ENOENT);
    fixture_no_name(faults, ".next.", ".secrets.age.backup");
    jet_rt_vault_set_test_fault(&cancel_after);
    jet_rt_vault_fixture_fault_commit(&root, &name, &empty, &initial, &cancel_after, -1, &first);
    jet_rt_vault_fixture_durability(&root, &name, &empty, &rotate_fault, &durability);
    fixture_path(backup_path, sizeof(backup_path), faults, ".jet/.secrets.age.backup");
    assert(access(backup_path, F_OK) == 0 && "authenticated old inode remains as recovery backup");
    fixture_write(store_path, (const unsigned char *)"substituted", 11);
    linux_install_cancellation_and_durability_boundaries_are_literal(&root, &name, first);
    assert(access(backup_path, F_OK) == -1 && errno == ENOENT);
    fixture_remove_tree(faults);

    char concurrent[] = "/tmp/jet-vault-reader-writer-backup-XXXXXX";
    root = fixture_scratch(concurrent, 1); fixture_authorizer(1);
    Text rotation = text("concurrent rotation");
    void *objects[2]; jet_rt_vault_fixture_concurrent_prepare(&root, &name, &empty, &initial, &rotation, objects);
    void *entered = jet_rt_vault_test_barrier_new(2), *release = jet_rt_vault_test_barrier_new(2);
    jet_rt_vault_set_test_backup_barrier(&root, entered, release);
    VaultWriter writer = {&root, &durability, objects[0], objects[1]};
    int64_t thread = jet_rt_vault_fixture_spawn(fixture_writer, (intptr_t)&writer); assert(thread >= 0);
    jet_rt_vault_test_barrier_wait(entered);
    fixture_path(backup_path, sizeof(backup_path), concurrent, ".jet/.secrets.age.backup");
    struct stat st; assert(stat(backup_path, &st) == 0);
    ino_t backup_inode = st.st_ino;
    jet_rt_vault_fixture_reader_conflict(&root, &name);
    assert(stat(backup_path, &st) == 0 && st.st_ino == backup_inode);
    jet_rt_vault_test_barrier_wait(release);
    intptr_t result; assert(jet_rt_vault_fixture_join(thread, &result) == 0 && result == 9);
    jet_rt_vault_set_test_backup_barrier(NULL, NULL, NULL);
    assert(stat(backup_path, &st) == 0 && st.st_ino == backup_inode);
    size_t backup_length; unsigned char *backup_raw = fixture_read(backup_path, &backup_length);
    ByteList backup_bytes = {(int64_t)backup_length, 0, backup_raw};
    concurrent_reader_defers_backup_recovery_until_writer_releases_lock(&root, &name, &backup_bytes);
    free(backup_raw);
    assert(access(backup_path, F_OK) == -1 && errno == ENOENT);
    jet_rt_vault_test_barrier_drop(entered); jet_rt_vault_test_barrier_drop(release);
    fixture_remove_tree(concurrent);

    char stale[] = "/tmp/jet-vault-stale-lock-XXXXXX", lock_path[512], lock_text[256];
    root = fixture_scratch(stale, 1); fixture_authorizer(1);
    void *raw_plan; jet_rt_vault_fixture_generate_plan(&root, &name, &empty, &raw_plan);
    const unsigned char *plan = raw_plan;
    char uuid[33], hash[65]; fixture_hex(uuid, plan + 40, 16); fixture_hex(hash, plan + 64, 32);
    int written = snprintf(lock_text, sizeof(lock_text),
        "repo=%s revision=0 pid=999999 process-start=1 nonce=11111111111111111111111111111111 hash=%s\n", uuid, hash);
    assert(written > 0 && (size_t)written < sizeof(lock_text));
    fixture_path(lock_path, sizeof(lock_path), stale, ".jet/.secrets.age.lock");
    fixture_write(lock_path, (const unsigned char *)lock_text, (size_t)written);
    struct timespec times[2]; assert(clock_gettime(CLOCK_REALTIME, &times[0]) == 0);
    times[0].tv_sec -= 601; times[1] = times[0];
    int fd = open(lock_path, O_RDONLY); assert(fd >= 0 && futimens(fd, times) == 0 && close(fd) == 0);
    Text recover = text("recover stale lock");
    stale_lock_requires_age_owner_dead_pid_nonce_and_unchanged_store(&root, raw_plan, &recover);
    assert(access(lock_path, F_OK) == -1 && errno == ENOENT);
    fixture_no_name(stale, "lock.stale", NULL);
    fixture_remove_tree(stale);

    char substitution[] = "/tmp/jet-vault-lock-substitution-XXXXXX", acquired[512];
    root = fixture_scratch(substitution, 1);
    jet_rt_vault_fixture_generate_plan(&root, &name, &empty, &raw_plan);
    void *guard; jet_rt_vault_fixture_guard(&root, raw_plan, &guard);
    Text lock_fault = text("substitute-lock-after-check");
    lock_drop_never_unlinks_an_inode_substituted_after_its_check(guard, &lock_fault);
    fixture_path(lock_path, sizeof(lock_path), substitution, ".jet/.secrets.age.lock");
    assert(stat(lock_path, &st) == 0); ino_t replacement_inode = st.st_ino;
    size_t replacement_length; unsigned char *replacement = fixture_read(lock_path, &replacement_length);
    assert(replacement_length == 11 && memcmp(replacement, "replacement", 11) == 0); free(replacement);
    assert(stat(lock_path, &st) == 0 && st.st_ino == replacement_inode);
    fixture_path(acquired, sizeof(acquired), substitution, ".jet/.secrets.age.lock.acquired");
    assert(stat(acquired, &st) == 0 && st.st_ino != replacement_inode);
    fixture_remove_tree(substitution);

    char temp[] = "/tmp/jet-vault-temp-substitution-XXXXXX";
    root = fixture_scratch(temp, 1); fixture_authorizer(1);
    Text temp_fault = text("substitute-temp-before-install");
    anonymous_temp_substitution_never_reaches_the_visible_store(&root, &name, &empty, &initial, &temp_fault);
    fixture_path(store_path, sizeof(store_path), temp, ".jet/secrets.age");
    assert(access(store_path, F_OK) == -1 && errno == ENOENT);
    fixture_remove_tree(temp);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    jet_rt_host_arguments(argc, argv, environ);
    jet_rt_thread_init();
    jet_rt_crypto_entropy_init();
    jet_rt_vault_test_hooks_init(jet_rt_vault_test_hooks_destroy);
    if (argc == 2 && strcmp(argv[1], "--vault-home-grant") == 0) {
        jet_rt_vault_headless_fixture();
        jet_rt_thread_detach();
        return 0;
    }
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
    jet_rt_vault_crypto_unavailable_fixture(unavailable_entropy);
    assert(unavailable_calls == 1);
    unsigned char crypto_raw[65537];
    for (size_t i = 0; i < sizeof(crypto_raw); ++i) crypto_raw[i] = (unsigned char)(i % 256);
    ByteList crypto_plain = {0, 0, crypto_raw};
    jet_rt_vault_crypto_fixture(&crypto_plain);
    crypto_plain.length = crypto_plain.capacity = 65536;
    jet_rt_vault_crypto_fixture(&crypto_plain);
    crypto_plain.length = crypto_plain.capacity = 65537;
    jet_rt_vault_crypto_fixture(&crypto_plain);
    canonical_read_fixtures();
    stream_read_fixtures();
    process_start_fixtures();
    lock_fields_fixtures();
    lock_guard_fixtures();
    Text lifecycle_name = text("."), rollback = text("clock-rollback"), missing_root = text("/proc/jet-vault-no-such-root");
    jet_rt_vault_lifecycle_strings_fixture(&lifecycle_name, &rollback, &missing_root);
    plan_auth_fixtures();
    trust_home_fixtures();
    filesystem_batch_a();
    filesystem_batch_b();
    filesystem_batch_c();
    puts("vault witness OK (13 errors, legacy+JVLT2 codec, imported origin/default entropy, native Secret redaction/wipe, long-origin hash, duplicate priority)");
    jet_rt_thread_detach();
    return 0;
}
