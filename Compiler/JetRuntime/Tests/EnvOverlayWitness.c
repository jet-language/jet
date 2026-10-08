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
extern struct EnvError *jet_rt_record_new_EnvError_InvalidName(void);
extern struct EnvError *jet_rt_record_new_EnvError_InvalidValue(void);
extern struct EnvError *jet_rt_record_new_EnvError_NonUnicode(void);
extern int64_t jet_rt_record_tag_EnvError(const struct EnvError *);
extern struct EnvError *jet_rt_handle_clone_EnvError(const struct EnvError *);
extern bool jet_rt_handle_eq_EnvError(const struct EnvError *, const struct EnvError *);
extern void jet_rt_handle_drop_EnvError(struct EnvError *);
extern struct Text *jet_rt_handle_show_EnvError(const struct EnvError *);
extern struct Text *jet_rt_handle_debug_EnvError(const struct EnvError *);
extern struct Text *jet_rt_handle_display_EnvError(const struct EnvError *);
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
        jet_rt_record_new_EnvError_InvalidName,
        jet_rt_record_new_EnvError_InvalidValue,
        jet_rt_record_new_EnvError_NonUnicode,
    };
    const char *const messages[] = {
        "invalid environment variable name",
        "invalid environment variable value",
        "environment contains a name or value that is not valid Unicode",
    };
    for (int64_t tag = 0; tag < 3; ++tag) {
        struct EnvError *value = constructors[tag]();
        assert(value != NULL && value->tag == tag);
        assert(jet_rt_record_tag_EnvError(value) == tag);
        struct EnvError *copy = jet_rt_handle_clone_EnvError(value);
        assert(copy != NULL && copy != value);
        assert(jet_rt_record_tag_EnvError(copy) == tag);
        assert(jet_rt_handle_eq_EnvError(value, copy));
        struct EnvError *other = constructors[(tag + 1) % 3]();
        assert(!jet_rt_handle_eq_EnvError(value, other));
        jet_rt_handle_drop_EnvError(other);
        jet_rt_handle_drop_EnvError(value);
        // Clone owns a different allocation and survives original release.
        assert(jet_rt_record_tag_EnvError(copy) == tag);
        exact_text(jet_rt_handle_show_EnvError(copy), messages[tag]);
        exact_text(jet_rt_handle_debug_EnvError(copy), messages[tag]);
        exact_text(jet_rt_handle_display_EnvError(copy), messages[tag]);
        jet_rt_handle_drop_EnvError(copy);
    }
    puts("ENVERROR CODEC OK");
    return 0;
}
