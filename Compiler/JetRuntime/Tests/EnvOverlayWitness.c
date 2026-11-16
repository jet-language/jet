// Real final-pack consumer of the native-private EnvError record corridor.
// The runtime's actual init arrays, allocator and text/enum providers supply
// every symbol below. This fixture supplies no provider or fake handle.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct EnvError { int64_t tag; };
struct Text { const unsigned char *data; int64_t length, capacity; };
extern struct EnvError *jet_rt_record_new_env_error_invalid_name(void);
extern struct EnvError *jet_rt_record_new_env_error_invalid_value(void);
extern struct EnvError *jet_rt_record_new_env_error_non_unicode(void);
extern int64_t jet_rt_record_tag_env_error(const struct EnvError *);
extern struct EnvError *jet_rt_handle_clone_env_error(const struct EnvError *);
extern bool jet_rt_handle_eq_env_error(const struct EnvError *, const struct EnvError *);
extern void jet_rt_handle_drop_env_error(struct EnvError *);
extern struct Text *jet_rt_handle_show_env_error(const struct EnvError *);
extern struct Text *jet_rt_handle_debug_env_error(const struct EnvError *);
extern struct Text *jet_rt_handle_display_env_error(const struct EnvError *);
extern void jet_rt_string_drop(struct Text *);

static void exact_text(struct Text *text, const char *expected) {
    assert(text != NULL);
    const size_t length = strlen(expected);
    assert(text->length == (int64_t)length);
    assert(length == 0 || (text->data != NULL && memcmp(text->data, expected, length) == 0));
    jet_rt_string_drop(text);
}

int main(void) {
    _Static_assert(sizeof(struct EnvError) == 8, "EncodingABI native-private enum size");
    struct EnvError *(*const constructors[])(void) = {
        jet_rt_record_new_env_error_invalid_name,
        jet_rt_record_new_env_error_invalid_value,
        jet_rt_record_new_env_error_non_unicode,
    };
    const char *const messages[] = {
        "invalid environment variable name",
        "invalid environment variable value",
        "environment contains a name or value that is not valid Unicode",
    };
    for (int64_t tag = 0; tag < 3; ++tag) {
        struct EnvError *value = constructors[tag]();
        assert(value != NULL && value->tag == tag);
        assert(jet_rt_record_tag_env_error(value) == tag);
        struct EnvError *copy = jet_rt_handle_clone_env_error(value);
        assert(copy != NULL && copy != value);
        assert(jet_rt_record_tag_env_error(copy) == tag);
        assert(jet_rt_handle_eq_env_error(value, copy));
        struct EnvError *other = constructors[(tag + 1) % 3]();
        assert(!jet_rt_handle_eq_env_error(value, other));
        jet_rt_handle_drop_env_error(other);
        jet_rt_handle_drop_env_error(value);
        // Clone owns a different allocation and survives original release.
        assert(jet_rt_record_tag_env_error(copy) == tag);
        exact_text(jet_rt_handle_show_env_error(copy), messages[tag]);
        exact_text(jet_rt_handle_debug_env_error(copy), messages[tag]);
        exact_text(jet_rt_handle_display_env_error(copy), messages[tag]);
        jet_rt_handle_drop_env_error(copy);
    }
    puts("ENVERROR CODEC OK");
    return 0;
}
