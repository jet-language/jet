// Real native runtime only.
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
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern int64_t jet_int_sub(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_string_from_static(const void *, int64_t);
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
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *pointer, int64_t bytes, int64_t align) {
    for (unsigned i = 0; i < 3; ++i) if (watched[i].pointer && pointer == watched[i].pointer) {
        assert(bytes == watched[i].bytes && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
}
// Watches a record shell and its text: the counted text block at t-8 and its byte buffer.
static void watch(void *shell, int64_t bytes, Text *text) {
    watched[0].pointer = shell; watched[0].bytes = bytes; watched[0].align = 8;
    watched[1].pointer = (char *)text - 8; watched[1].bytes = 40; watched[1].align = 8;
    watched[2].pointer = text->capacity ? (void *)text->data : NULL; watched[2].bytes = text->capacity; watched[2].align = 1;
    for (unsigned i = 0; i < 3; ++i) watched[i].frees = 0;
}
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(const Text *t) { return *(const int64_t *)((const char *)t - 8) & INT64_C(0x3fffffffffffffff); }
// The record shell is freed once while another owner keeps its text alive.
static void shell_only(void) {
    assert(watched[0].frees == 1 && watched[1].frees == 0 && watched[2].frees == 0);
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
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if (argc == 2) {
        bool high = strcmp(argv[1], "range-high") == 0;
        if (!high && strcmp(argv[1], "range-low") != 0) return 64;
        int64_t bound = jet_int_owned_from_i64(high ? INT64_MAX : INT64_MIN);
        int64_t outside = high ? jet_int_add(bound, 1) : jet_int_sub(bound, 1);
        Text *store = jet_rt_string_from_static("store", 5);
        Runtime *unexpected = jet_rt_record_new_jet_service_runtime(store, outside);
        jet_rt_handle_drop_jet_service_runtime(unexpected);
        jet_rt_string_drop(store);
        jet_rt_int_drop(outside); jet_rt_int_drop(bound);
        return 99; // Canonical narrowing stop must occur in the real constructor.
    }
    assert(argc == 1);
    static const char raw[] = "path\0\xe9\x9b\xaa";
    static const int64_t values[] = {INT64_MIN, -1, 0, INT64_MAX};
    static const char *const decimals[] = {"-9223372036854775808)", "-1)", "0)", "9223372036854775807)"};
    for (unsigned empty = 0; empty < 2; ++empty) {
        size_t length = empty ? 0 : sizeof(raw) - 1;
        Text *input = jet_rt_string_from_static(raw, (int64_t)length);
        Store *a = jet_rt_record_new_jet_service_state_store(input);
        Store *b = jet_rt_handle_clone_jet_service_state_store(a);
        // Records are reallocated; constructor and clone share the text by owner count.
        assert(a != b && a->path == input && b->path == input && owners(input) == 3);
        jet_rt_string_drop(input);
        assert(jet_rt_handle_eq_jet_service_state_store(a, b));
        Text *different = jet_rt_string_from_static("other", 5); Store fixture = {different};
        assert(!jet_rt_handle_eq_jet_service_state_store(a, &fixture));
        jet_rt_string_drop(different);
        Text *getter = jet_rt_record_get_jet_service_state_store_path(a);
        assert(getter == a->path && owners(getter) == 3);
        text(a->path, raw, length); text(b->path, raw, length);
        Text *show = jet_rt_handle_show_jet_service_state_store(a), *display = jet_rt_handle_display_jet_service_state_store(a);
        assert(show != display && show != a->path && display != a->path);
        render(show, "ServiceStateStore(", raw, length, ")");
        render(display, "ServiceStateStore(", raw, length, ")");
        watch(a, 8, a->path); jet_rt_handle_drop_jet_service_state_store(a); shell_only();
        assert(owners(getter) == 2);
        text(getter, raw, length); jet_rt_string_drop(getter); shell_only();
        watch(b, 8, b->path); jet_rt_handle_drop_jet_service_state_store(b); done();
        for (unsigned i = 0; i < 4; ++i) {
            Text *source = jet_rt_string_from_static(raw, (int64_t)length);
            int64_t argument = jet_int_owned_from_i64(values[i]);
            Runtime *r = jet_rt_record_new_jet_service_runtime(source, argument);
            assert(jet_int_owned_to_i64(argument) == values[i]);
            jet_rt_int_drop(argument);
            Runtime *clone = jet_rt_handle_clone_jet_service_runtime(r);
            assert(clone != r && clone->store == source && r->store == source && owners(source) == 3);
            jet_rt_string_drop(source);
            assert(r->retention == values[i] && clone->retention == values[i]);
            int64_t first = jet_rt_record_get_jet_service_runtime_retention_ms(r);
            int64_t second = jet_rt_record_get_jet_service_runtime_retention_ms(r);
            assert(jet_int_owned_to_i64(first) == values[i] && jet_int_owned_to_i64(second) == values[i]);
            if (values[i] == INT64_MIN || values[i] == INT64_MAX) assert(first != second);
            jet_rt_int_drop(first); jet_rt_int_drop(second);
            Text *fresh = jet_rt_record_get_jet_service_runtime_store(r);
            assert(fresh == r->store && fresh == clone->store && owners(fresh) == 3);
            text(r->store, raw, length); text(clone->store, raw, length);
            Text *rs = jet_rt_handle_show_jet_service_runtime(r), *rd = jet_rt_handle_display_jet_service_runtime(r);
            assert(rs != rd && rs != r->store && rd != r->store);
            char suffix[80]; (void)snprintf(suffix, sizeof(suffix), ", retention_ms=%s", decimals[i]);
            render(rs, "ServiceRuntime(store=", raw, length, suffix);
            render(rd, "ServiceRuntime(store=", raw, length, suffix);
            watch(r, 16, r->store); jet_rt_handle_drop_jet_service_runtime(r); shell_only();
            assert(owners(fresh) == 2);
            text(fresh, raw, length); jet_rt_string_drop(fresh); shell_only(); text(clone->store, raw, length);
            watch(clone, 16, clone->store); jet_rt_handle_drop_jet_service_runtime(clone); done();
        }
    }
    jet_rt_handle_drop_jet_service_state_store(NULL); jet_rt_handle_drop_jet_service_runtime(NULL);
    puts("ServiceValues ownership/getters/render OK");
}
