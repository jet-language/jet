/* REAL Jet objects only, linked with --wrap=jet_rt_alloc/jet_rt_free.
 * UiCarriersABI private layouts; never source/MIR/Rust allocations.
 * CommonTypes.rs:35-60; Open.rs:88-102; Browser.rs:92-112; Ui.rs:678-682.
 * width-stop/policy-stop are separate process modes: exit70, empty stdout,
 * stderr must match DataPlotStops-ctor-range.stderr. Execution unclaimed. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { unsigned char *data; int64_t length, capacity; } Text;
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern int64_t __real_jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(int64_t, int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
#define DECLARE(T) \
    extern int64_t jet_rt_handle_clone_##T(int64_t); \
    extern void jet_rt_handle_drop_##T(int64_t); \
    extern bool jet_rt_handle_eq_##T(int64_t, int64_t)
DECLARE(TerminalMode); DECLARE(TextWidthAmbiguous); DECLARE(TextWidthControls);
DECLARE(JetTuiDirection); DECLARE(TerminalSize); DECLARE(TerminalPolicy);
DECLARE(TextWidth); DECLARE(JetBrowserProfile); DECLARE(JetBrowserTimeout);
DECLARE(JetBrowserLocked);
extern int64_t jet_rt_record_new_terminal_size(int64_t, int64_t);
extern int64_t jet_rt_record_get_terminal_size_cols(int64_t);
extern int64_t jet_rt_record_get_terminal_size_rows(int64_t);
extern int64_t jet_rt_record_new_terminal_policy(int64_t, int64_t);
extern int64_t jet_rt_record_get_terminal_policy_size(int64_t);
extern int64_t jet_rt_record_get_terminal_policy_mode(int64_t);
extern int64_t jet_rt_record_new_text_width(int64_t, int64_t);
extern int64_t jet_rt_record_get_text_width_ambiguous(int64_t);
extern int64_t jet_rt_record_get_text_width_controls(int64_t);
extern int64_t jet_rt_record_new_jet_browser_profile(Text *, Text *);
extern Text *jet_rt_record_get_jet_browser_profile_name(int64_t);
extern Text *jet_rt_record_get_jet_browser_profile_version(int64_t);
extern int64_t jet_rt_record_new_jet_browser_timeout(int64_t);
extern int64_t jet_rt_record_get_jet_browser_timeout_milliseconds(int64_t);
extern int64_t jet_rt_record_new_jet_browser_locked(Text *, Text *, Text *, Text *, int64_t, Text *);
extern Text *jet_rt_record_get_jet_browser_locked_engine(int64_t);
extern Text *jet_rt_record_get_jet_browser_locked_version(int64_t);
extern Text *jet_rt_record_get_jet_browser_locked_binary(int64_t);
extern Text *jet_rt_record_get_jet_browser_locked_protocol(int64_t);
extern Text *jet_rt_record_get_jet_browser_locked_output_hash(int64_t);
extern int64_t jet_rt_record_get_jet_browser_locked_size(int64_t);

static struct { int64_t address, bytes, alignment; bool live; } ledger[512];
static size_t allocations, frees;
static bool tracking;
int64_t __wrap_jet_rt_alloc(int64_t bytes, int64_t alignment) {
    int64_t address = __real_jet_rt_alloc(bytes, alignment);
    if (tracking) {
        assert(allocations < 512 && address != 0 && bytes > 0);
        assert((uint64_t)address % (uint64_t)alignment == 0);
        for (size_t i = 0; i < allocations; ++i)
            assert(!ledger[i].live || ledger[i].address != address);
        ledger[allocations].address = address;
        ledger[allocations].bytes = bytes;
        ledger[allocations].alignment = alignment;
        ledger[allocations++].live = true;
    }
    return address;
}
void __wrap_jet_rt_free(int64_t address, int64_t bytes, int64_t alignment) {
    if (tracking) {
        size_t i = allocations;
        while (i > 0) {
            --i;
            if (ledger[i].address == address && ledger[i].live) {
                assert(ledger[i].bytes == bytes && ledger[i].alignment == alignment);
                ledger[i].live = false;
                ++frees;
                __real_jet_rt_free(address, bytes, alignment);
                return;
            }
        }
        assert(!"untracked or double free in real provider");
    }
    __real_jet_rt_free(address, bytes, alignment);
}
static bool live(int64_t address) {
    for (size_t i = allocations; i > 0; --i)
        if (ledger[i - 1].address == address) return ledger[i - 1].live;
    assert(!"untracked allocation");
    return false;
}
static void size_is(int64_t address, int64_t bytes) {
    for (size_t i = allocations; i > 0; --i) {
        if (ledger[i - 1].address == address && ledger[i - 1].live) {
            assert(ledger[i - 1].bytes == bytes && ledger[i - 1].alignment == 8);
            return;
        }
    }
    assert(!"missing carrier allocation");
}
static int64_t *words(int64_t address) { return (int64_t *)(uintptr_t)address; }
static void exact_is(int64_t owned, int64_t expected) {
    assert(jet_int_owned_to_i64(owned) == expected);
    jet_rt_int_drop(owned);
}
static void text_is(Text *owned, const void *expected, int64_t length) {
    assert(owned->length == length && memcmp(owned->data, expected, (size_t)length) == 0);
    jet_rt_string_drop(owned);
}
static void enum_pair(int64_t (*clone)(int64_t), void (*drop)(int64_t), bool (*eq)(int64_t, int64_t)) {
    int64_t a = jet_rt_alloc(8, 8), b = jet_rt_alloc(8, 8);
    words(a)[0] = 0; words(b)[0] = 1;
    int64_t copied = clone(a);
    size_is(copied, 8);
    assert(copied != a && eq(a, copied) && !eq(a, b) && !eq(b, a));
    drop(a);
    assert(words(copied)[0] == 0);
    drop(copied); drop(b); drop(0);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    tracking = true;
    if (argc == 2) {
        if (strcmp(argv[1], "width-stop") == 0) (void)jet_rt_record_new_text_width(2, 0);
        else if (strcmp(argv[1], "policy-stop") == 0) {
            int64_t zero = jet_int_owned_from_i64(0);
            int64_t size = jet_rt_record_new_terminal_size(zero, zero);
            jet_rt_int_drop(zero);
            (void)jet_rt_record_new_terminal_policy(size, 2);
        } else return 64;
        return 99;
    }
    if (argc != 1) return 64;
    enum_pair(jet_rt_handle_clone_terminal_mode, jet_rt_handle_drop_terminal_mode, jet_rt_handle_eq_terminal_mode);
    enum_pair(jet_rt_handle_clone_text_width_ambiguous, jet_rt_handle_drop_text_width_ambiguous, jet_rt_handle_eq_text_width_ambiguous);
    enum_pair(jet_rt_handle_clone_text_width_controls, jet_rt_handle_drop_text_width_controls, jet_rt_handle_eq_text_width_controls);
    enum_pair(jet_rt_handle_clone_jet_tui_direction, jet_rt_handle_drop_jet_tui_direction, jet_rt_handle_eq_jet_tui_direction);

    int64_t minimum = jet_int_owned_from_i64(INT64_MIN), maximum = jet_int_owned_from_i64(INT64_MAX);
    int64_t size = jet_rt_record_new_terminal_size(minimum, maximum);
    size_is(size, 16);
    assert(words(size)[0] == INT64_MIN && words(size)[1] == INT64_MAX);
    int64_t size_copy = jet_rt_handle_clone_terminal_size(size);
    assert(jet_rt_handle_eq_terminal_size(size, size_copy));
    words(size_copy)[1] ^= 1;
    assert(!jet_rt_handle_eq_terminal_size(size, size_copy));
    words(size_copy)[1] ^= 1;
    int64_t policy = jet_rt_record_new_terminal_policy(size, 1);
    assert(!live(size));
    size_is(policy, 24);
    assert(words(policy)[0] == INT64_MIN && words(policy)[1] == INT64_MAX && words(policy)[2] == 1);
    size_t before_clone = allocations;
    int64_t policy_copy = jet_rt_handle_clone_terminal_policy(policy);
    assert(allocations == before_clone + 1 && policy_copy != policy);
    assert(jet_rt_handle_eq_terminal_policy(policy, policy_copy));
    for (size_t i = 0; i < 3; ++i) {
        words(policy_copy)[i] ^= 1;
        assert(!jet_rt_handle_eq_terminal_policy(policy, policy_copy));
        words(policy_copy)[i] ^= 1;
    }
    int64_t got_size = jet_rt_record_get_terminal_policy_size(policy);
    assert(got_size != policy && got_size != size_copy);
    size_is(got_size, 16);
    assert(jet_rt_handle_eq_terminal_size(got_size, size_copy));
    exact_is(jet_rt_record_get_terminal_size_cols(got_size), INT64_MIN);
    exact_is(jet_rt_record_get_terminal_size_rows(got_size), INT64_MAX);
    assert(jet_rt_record_get_terminal_policy_mode(policy) == 1);
    jet_rt_handle_drop_terminal_policy(policy);
    assert(words(policy_copy)[1] == INT64_MAX);
    jet_rt_handle_drop_terminal_size(got_size); jet_rt_handle_drop_terminal_size(size_copy);
    jet_rt_handle_drop_terminal_policy(policy_copy);

    for (int64_t a = 0; a < 2; ++a) for (int64_t c = 0; c < 2; ++c) {
        int64_t width = jet_rt_record_new_text_width(a, c), copy = jet_rt_handle_clone_text_width(width);
        size_is(width, 16);
        assert(copy != width && jet_rt_handle_eq_text_width(width, copy));
        assert(jet_rt_record_get_text_width_ambiguous(copy) == a && jet_rt_record_get_text_width_controls(copy) == c);
        words(copy)[0] ^= 1; assert(!jet_rt_handle_eq_text_width(width, copy)); words(copy)[0] ^= 1;
        words(copy)[1] ^= 1; assert(!jet_rt_handle_eq_text_width(width, copy)); words(copy)[1] ^= 1;
        jet_rt_handle_drop_text_width(width); jet_rt_handle_drop_text_width(copy);
    }
    int64_t timeout = jet_rt_record_new_jet_browser_timeout(minimum), timeout_copy = jet_rt_handle_clone_jet_browser_timeout(timeout);
    size_is(timeout, 8);
    assert(words(timeout)[0] == INT64_MIN && jet_rt_handle_eq_jet_browser_timeout(timeout, timeout_copy));
    words(timeout_copy)[0] ^= 1; assert(!jet_rt_handle_eq_jet_browser_timeout(timeout, timeout_copy)); words(timeout_copy)[0] ^= 1;
    exact_is(jet_rt_record_get_jet_browser_timeout_milliseconds(timeout_copy), INT64_MIN);
    jet_rt_handle_drop_jet_browser_timeout(timeout); jet_rt_handle_drop_jet_browser_timeout(timeout_copy);

    unsigned char name[] = {'A', 0, 0xe2, 0x82, 0xac};
    const unsigned char expected_name[] = {'A', 0, 0xe2, 0x82, 0xac};
    Text *name_input = jet_rt_string_from_static(name, 5), *version = jet_rt_string_from_static("v1", 2);
    int64_t profile = jet_rt_record_new_jet_browser_profile(name_input, version), profile_copy = jet_rt_handle_clone_jet_browser_profile(profile);
    size_is(profile, 16);
    name[0] = 'Z'; jet_rt_string_drop(name_input); jet_rt_string_drop(version);
    assert(profile != profile_copy && jet_rt_handle_eq_jet_browser_profile(profile, profile_copy));
    Text *name_getter = jet_rt_record_get_jet_browser_profile_name(profile);
    assert((int64_t)(uintptr_t)name_getter != words(profile)[0]);
    assert(memcmp(name_getter->data, expected_name, 5) == 0);
    name_getter->data[0] = 'X'; jet_rt_string_drop(name_getter);
    assert(jet_rt_handle_eq_jet_browser_profile(profile, profile_copy));
    for (size_t i = 0; i < 2; ++i) {
        Text *field = (Text *)(uintptr_t)words(profile_copy)[i];
        field->data[0] ^= 1; assert(!jet_rt_handle_eq_jet_browser_profile(profile, profile_copy)); field->data[0] ^= 1;
    }
    jet_rt_handle_drop_jet_browser_profile(profile);
    text_is(jet_rt_record_get_jet_browser_profile_name(profile_copy), expected_name, 5);
    text_is(jet_rt_record_get_jet_browser_profile_version(profile_copy), "v1", 2);
    jet_rt_handle_drop_jet_browser_profile(profile_copy);

    const char *labels[] = {"engine", "version", "binary", "protocol", "hash"};
    Text *inputs[5];
    for (size_t i = 0; i < 5; ++i) inputs[i] = jet_rt_string_from_static(labels[i], (int64_t)strlen(labels[i]));
    int64_t locked = jet_rt_record_new_jet_browser_locked(inputs[0], inputs[1], inputs[2], inputs[3], maximum, inputs[4]);
    int64_t locked_copy = jet_rt_handle_clone_jet_browser_locked(locked);
    size_is(locked, 48);
    assert(locked != locked_copy && words(locked)[4] == INT64_MAX && jet_rt_handle_eq_jet_browser_locked(locked, locked_copy));
    for (size_t i = 0; i < 5; ++i) {
        jet_rt_string_drop(inputs[i]);
        size_t slot = i < 4 ? i : 5;
        Text *field = (Text *)(uintptr_t)words(locked_copy)[slot];
        assert(words(locked)[slot] != words(locked_copy)[slot]);
        field->data[0] ^= 1; assert(!jet_rt_handle_eq_jet_browser_locked(locked, locked_copy)); field->data[0] ^= 1;
    }
    words(locked_copy)[4] ^= 1; assert(!jet_rt_handle_eq_jet_browser_locked(locked, locked_copy)); words(locked_copy)[4] ^= 1;
    jet_rt_handle_drop_jet_browser_locked(locked);
    Text *(*getters[])(int64_t) = {jet_rt_record_get_jet_browser_locked_engine, jet_rt_record_get_jet_browser_locked_version, jet_rt_record_get_jet_browser_locked_binary, jet_rt_record_get_jet_browser_locked_protocol, jet_rt_record_get_jet_browser_locked_output_hash};
    for (size_t i = 0; i < 5; ++i) text_is(getters[i](locked_copy), labels[i], (int64_t)strlen(labels[i]));
    exact_is(jet_rt_record_get_jet_browser_locked_size(locked_copy), INT64_MAX);
    jet_rt_handle_drop_jet_browser_locked(locked_copy);
    jet_rt_int_drop(minimum); jet_rt_int_drop(maximum);
    jet_rt_handle_drop_terminal_size(0); jet_rt_handle_drop_terminal_policy(0);
    jet_rt_handle_drop_text_width(0); jet_rt_handle_drop_jet_browser_profile(0);
    jet_rt_handle_drop_jet_browser_timeout(0); jet_rt_handle_drop_jet_browser_locked(0);
    assert(allocations == frees);
    for (size_t i = 0; i < allocations; ++i) assert(!ledger[i].live);
    puts("ui-carriers-basic ok");
    return 0;
}
