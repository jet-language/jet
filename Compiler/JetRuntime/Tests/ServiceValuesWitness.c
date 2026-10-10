// Real native runtime only, linked with -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len, capacity; } Text;
typedef struct { Text *path; } Store;
typedef struct { Text *store; int64_t retention; } Runtime;
_Static_assert(sizeof(Store) == 8 && sizeof(Runtime) == 16, "private service values");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern int64_t jet_int_sub(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern Store *jet_rt_record_new_jet_service_state_store(const Text *);
extern Store *jet_rt_handle_clone_jet_service_state_store(const Store *);
extern void jet_rt_handle_drop_jet_service_state_store(Store *);
extern bool jet_rt_handle_eq_jet_service_state_store(const Store *, const Store *);
extern Text *jet_rt_record_get_jet_service_state_store_path(const Store *);
extern Text *jet_rt_handle_show_jet_service_state_store(const Store *);
extern Text *jet_rt_handle_display_jet_service_state_store(const Store *);
extern Runtime *jet_rt_record_new_jet_service_runtime(const Text *, int64_t);
extern Runtime *jet_rt_handle_clone_jet_service_runtime(const Runtime *);
extern void jet_rt_handle_drop_jet_service_runtime(Runtime *);
extern Text *jet_rt_record_get_jet_service_runtime_store(const Runtime *);
extern int64_t jet_rt_record_get_jet_service_runtime_retention_ms(const Runtime *);
extern Text *jet_rt_handle_show_jet_service_runtime(const Runtime *);
extern Text *jet_rt_handle_display_jet_service_runtime(const Runtime *);
static struct { void *pointer; int64_t bytes, align; unsigned frees; } watched[3];
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    for (unsigned i = 0; i < 3; ++i) if (watched[i].pointer && pointer == watched[i].pointer) {
        assert(bytes == watched[i].bytes && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, bytes, align);
}
static void watch(void *shell, int64_t bytes, Text *text) {
    watched[0].pointer = shell; watched[0].bytes = bytes; watched[0].align = 8;
    watched[1].pointer = text; watched[1].bytes = 24; watched[1].align = 8;
    watched[2].pointer = (void *)text->data; watched[2].bytes = text->capacity; watched[2].align = 1;
    for (unsigned i = 0; i < 3; ++i) watched[i].frees = 0;
}
static void done(void) {
    assert(watched[0].frees == 1 && watched[1].frees == 1);
    if (watched[2].bytes) assert(watched[2].frees == 1);
    memset(watched, 0, sizeof(watched));
}
static void text(const Text *value, const char *expected, size_t length) {
    assert(value->len == (int64_t)length);
    if (length) assert(memcmp(value->data, expected, length) == 0);
}
static void render(Text *value, const char *prefix, const char *raw, size_t length, const char *suffix) {
    size_t pre = strlen(prefix), post = strlen(suffix);
    assert(value->len == (int64_t)(pre + length + post));
    assert(memcmp(value->data, prefix, pre) == 0);
    if (length) assert(memcmp(value->data + pre, raw, length) == 0);
    assert(memcmp(value->data + pre + length, suffix, post) == 0);
    jet_rt_string_drop(value);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc == 2) {
        bool high = strcmp(argv[1], "range-high") == 0;
        if (!high && strcmp(argv[1], "range-low") != 0) return 64;
        int64_t bound = jet_int_owned_from_i64(high ? INT64_MAX : INT64_MIN);
        int64_t outside = high ? jet_int_add(bound, 1) : jet_int_sub(bound, 1);
        Text store = {"store", 5, 5};
        Runtime *unexpected = jet_rt_record_new_jet_service_runtime(&store, outside);
        jet_rt_handle_drop_jet_service_runtime(unexpected);
        jet_rt_int_drop(outside); jet_rt_int_drop(bound);
        return 99; // Canonical narrowing stop must occur in the real constructor.
    }
    assert(argc == 1);
    static const char raw[] = "path\0\xe9\x9b\xaa";
    static const int64_t values[] = {INT64_MIN, -1, 0, INT64_MAX};
    static const char *const decimals[] = {"-9223372036854775808)", "-1)", "0)", "9223372036854775807)"};
    for (unsigned empty = 0; empty < 2; ++empty) {
        char input[sizeof(raw)]; memcpy(input, raw, sizeof(raw));
        size_t length = empty ? 0 : sizeof(raw) - 1;
        Text borrowed = {input, (int64_t)length, (int64_t)length};
        Store *a = jet_rt_record_new_jet_service_state_store(&borrowed);
        Store *b = jet_rt_handle_clone_jet_service_state_store(a);
        assert(a != b && a->path != b->path && a->path != &borrowed);
        assert(jet_rt_handle_eq_jet_service_state_store(a, b));
        Text different = {"other", 5, 5}; Store fixture = {&different};
        assert(!jet_rt_handle_eq_jet_service_state_store(a, &fixture));
        Text *getter = jet_rt_record_get_jet_service_state_store_path(a);
        assert(getter != a->path && getter != b->path);
        memset(input, 'x', sizeof(input));
        text(a->path, raw, length); text(b->path, raw, length);
        Text *show = jet_rt_handle_show_jet_service_state_store(a), *display = jet_rt_handle_display_jet_service_state_store(a);
        assert(show != display);
        render(show, "ServiceStateStore(", raw, length, ")");
        render(display, "ServiceStateStore(", raw, length, ")");
        watch(a, 8, a->path); jet_rt_handle_drop_jet_service_state_store(a); done();
        text(getter, raw, length); jet_rt_string_drop(getter);
        watch(b, 8, b->path); jet_rt_handle_drop_jet_service_state_store(b); done();
        for (unsigned i = 0; i < 4; ++i) {
            memcpy(input, raw, sizeof(raw));
            int64_t argument = jet_int_owned_from_i64(values[i]);
            Runtime *r = jet_rt_record_new_jet_service_runtime(&borrowed, argument);
            assert(jet_int_owned_to_i64(argument) == values[i]);
            jet_rt_int_drop(argument);
            Runtime *clone = jet_rt_handle_clone_jet_service_runtime(r);
            assert(clone != r && clone->store != r->store && r->store != &borrowed);
            assert(r->retention == values[i] && clone->retention == values[i]);
            int64_t first = jet_rt_record_get_jet_service_runtime_retention_ms(r);
            int64_t second = jet_rt_record_get_jet_service_runtime_retention_ms(r);
            assert(jet_int_owned_to_i64(first) == values[i] && jet_int_owned_to_i64(second) == values[i]);
            if (values[i] == INT64_MIN || values[i] == INT64_MAX) assert(first != second);
            jet_rt_int_drop(first); jet_rt_int_drop(second);
            Text *fresh = jet_rt_record_get_jet_service_runtime_store(r);
            assert(fresh != r->store && fresh != clone->store);
            memset(input, 'x', sizeof(input));
            text(r->store, raw, length); text(clone->store, raw, length);
            Text *rs = jet_rt_handle_show_jet_service_runtime(r), *rd = jet_rt_handle_display_jet_service_runtime(r);
            assert(rs != rd);
            char suffix[80]; (void)snprintf(suffix, sizeof(suffix), ", retention_ms=%s", decimals[i]);
            render(rs, "ServiceRuntime(store=", raw, length, suffix);
            render(rd, "ServiceRuntime(store=", raw, length, suffix);
            watch(r, 16, r->store); jet_rt_handle_drop_jet_service_runtime(r); done();
            text(fresh, raw, length); jet_rt_string_drop(fresh); text(clone->store, raw, length);
            watch(clone, 16, clone->store); jet_rt_handle_drop_jet_service_runtime(clone); done();
        }
    }
    jet_rt_handle_drop_jet_service_state_store(NULL); jet_rt_handle_drop_jet_service_runtime(NULL);
    puts("ServiceValues ownership/getters/render OK");
}
