// Real native providers only; link with -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len, capacity; } Text;
typedef struct { int64_t tag; Text *payload; } Error;
_Static_assert(sizeof(Error) == 16, "canonical private Error16");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Error *jet_rt_handle_clone_jet_service_error(const Error *);
extern void jet_rt_handle_drop_jet_service_error(Error *);
extern bool jet_rt_handle_eq_jet_service_error(const Error *, const Error *);
extern Text *jet_rt_handle_show_jet_service_error(const Error *);
extern int64_t jet_rt_record_tag_jet_service_error(const Error *);
extern void jet_rt_string_drop(Text *);
#define DECLARE(name) \
    extern Error *jet_rt_record_new_jet_service_error_##name(const Text *); \
    extern Text *jet_rt_record_get_jet_service_error_##name(const Error *);
DECLARE(Full)
DECLARE(Ambiguous)
DECLARE(Unknown)
DECLARE(NotStarted)
DECLARE(Policy)
DECLARE(Unavailable)
DECLARE(Partitioned)
DECLARE(Revoked)
DECLARE(Stale)
DECLARE(Expired)
#undef DECLARE
static Error *(*const ctors[])(const Text *) = {
    jet_rt_record_new_jet_service_error_full, jet_rt_record_new_jet_service_error_ambiguous,
    jet_rt_record_new_jet_service_error_unknown, jet_rt_record_new_jet_service_error_not_started,
    jet_rt_record_new_jet_service_error_policy, jet_rt_record_new_jet_service_error_unavailable,
    jet_rt_record_new_jet_service_error_partitioned, jet_rt_record_new_jet_service_error_revoked,
    jet_rt_record_new_jet_service_error_stale, jet_rt_record_new_jet_service_error_expired
};
static Text *(*const getters[])(const Error *) = {
    jet_rt_record_get_jet_service_error_full, jet_rt_record_get_jet_service_error_ambiguous,
    jet_rt_record_get_jet_service_error_unknown, jet_rt_record_get_jet_service_error_not_started,
    jet_rt_record_get_jet_service_error_policy, jet_rt_record_get_jet_service_error_unavailable,
    jet_rt_record_get_jet_service_error_partitioned, jet_rt_record_get_jet_service_error_revoked,
    jet_rt_record_get_jet_service_error_stale, jet_rt_record_get_jet_service_error_expired
};
static void *watched[3];
static int64_t sizes[3];
static unsigned frees[3];
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    for (unsigned i = 0; i < 3; ++i) {
        if (watched[i] && pointer == watched[i]) {
            assert(bytes == sizes[i] && align == (i == 2 ? 1 : 8));
            assert(++frees[i] == 1);
        }
    }
    __real_jet_rt_free(pointer, bytes, align);
}
static void drop(Error *value) {
    watched[0] = value; sizes[0] = 16;
    watched[1] = value->payload; sizes[1] = 24;
    watched[2] = (void *)value->payload->data; sizes[2] = value->payload->capacity;
    memset(frees, 0, sizeof(frees));
    jet_rt_handle_drop_jet_service_error(value);
    assert(frees[0] == 1 && frees[1] == 1);
    if (sizes[2]) assert(frees[2] == 1);
    memset(watched, 0, sizeof(watched));
}
static void text(const Text *value, const char *bytes, size_t length) {
    assert(value->len == (int64_t)length);
    if (length) assert(memcmp(value->data, bytes, length) == 0);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc == 2) {
        for (unsigned i = 0; i < 10; ++i) {
            char mode[16];
            (void)snprintf(mode, sizeof(mode), "wrong-%u", i);
            if (strcmp(argv[1], mode) == 0) {
                Text input = {"wrong", 5, 5};
                Error *value = ctors[(i + 1) % 10](&input);
                Text *unexpected = getters[i](value);
                jet_rt_string_drop(unexpected); drop(value);
                return 99; // A valid run must stop before this, not fake a fatal result.
            }
        }
        return 64;
    }
    assert(argc == 1);
    static const char raw[] = "raw\0\"\\\n\xe9\x9b\xaa";
    for (unsigned variant = 0; variant < 10; ++variant) {
        for (unsigned empty = 0; empty < 2; ++empty) {
            char input[sizeof(raw)]; memcpy(input, raw, sizeof(raw));
            size_t length = empty ? 0 : sizeof(raw) - 1;
            Text borrowed = {input, (int64_t)length, (int64_t)length};
            Error *value = ctors[variant](&borrowed);
            assert(value->tag == (int64_t)variant && jet_rt_record_tag_jet_service_error(value) == (int64_t)variant);
            assert(value->payload != &borrowed);
            if (length) assert(value->payload->data != borrowed.data);
            Error *clone = jet_rt_handle_clone_jet_service_error(value);
            assert(clone != value && clone->payload != value->payload && jet_rt_handle_eq_jet_service_error(value, clone));
            memset(input, 'x', sizeof(input));
            text(value->payload, raw, length); text(clone->payload, raw, length);
            Text *getter = getters[variant](value), *show = jet_rt_handle_show_jet_service_error(value);
            assert(getter != value->payload && show != value->payload && show != getter);
            for (unsigned other = 0; other < 10; ++other) {
                Error fixture = {(int64_t)other, clone->payload};
                assert(jet_rt_handle_eq_jet_service_error(value, &fixture) == (variant == other));
            }
            Text different = {"different", 9, 9};
            Error fixture = {(int64_t)variant, &different};
            assert(!jet_rt_handle_eq_jet_service_error(value, &fixture));
            drop(value);
            text(getter, raw, length); text(show, raw, length); text(clone->payload, raw, length);
            jet_rt_string_drop(getter); jet_rt_string_drop(show); drop(clone);
        }
    }
    jet_rt_handle_drop_jet_service_error(NULL);
    puts("ServiceError ownership/equality/raw-show OK");
}
