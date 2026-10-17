// Canonical Lower/Values enum storage built with actual runtime owners only.
// Link the real native runtime with -Wl,--wrap=jet_rt_free; no ctor facade.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len, capacity; } Text;
typedef struct { int64_t tag; Text *operation; Text **server; int64_t *code; Text *reason; } Error;
_Static_assert(sizeof(Error) == 40 && offsetof(Error, operation) == 8 &&
               offsetof(Error, server) == 16 && offsetof(Error, code) == 24 &&
               offsetof(Error, reason) == 32, "canonical native EmailError enum");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Text *jet_rt_string_clone(const Text *);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern int64_t jet_int_sub(int64_t, int64_t);
extern int64_t jet_int_compare(int64_t, int64_t);
extern int64_t jet_rt_int_clone(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Error *jet_rt_handle_clone_error(const Error *);
extern void jet_rt_handle_drop_error(Error *);
extern bool jet_rt_handle_eq_error(const Error *, const Error *);
static struct { const void *pointer; int64_t bytes, align; unsigned frees; } watched[9];
static unsigned nwatched;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    for (unsigned i = 0; i < nwatched; ++i) if (pointer == watched[i].pointer) {
        assert(bytes == watched[i].bytes && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, bytes, align);
}
static void track(const void *pointer, int64_t bytes, int64_t align) {
    assert(pointer && nwatched < 9);
    watched[nwatched].pointer = pointer; watched[nwatched].bytes = bytes;
    watched[nwatched].align = align; watched[nwatched].frees = 0; ++nwatched;
}
static void track_text(const Text *text) {
    track(text, 24, 8);
    if (text->capacity) track(text->data, text->capacity, 1);
}
static void watch(const Error *value) {
    track(value, 40, 8); track_text(value->operation);
    if (value->server) { track(value->server, 8, 8); track_text(*value->server); }
    if (value->code) track(value->code, 8, 8);
    track_text(value->reason);
}
static void done(void) {
    for (unsigned i = 0; i < nwatched; ++i) assert(watched[i].frees == 1);
    memset(watched, 0, sizeof(watched)); nwatched = 0;
}
static Error *literal(int64_t tag, const Text *operation, const Text *server,
                      bool has_code, int64_t code, const Text *reason) {
    Error *value = jet_rt_alloc(40, 8); value->tag = tag;
    value->operation = jet_rt_string_clone(operation); value->server = NULL;
    if (server) { value->server = jet_rt_alloc(8, 8); *value->server = jet_rt_string_clone(server); }
    value->code = NULL;
    if (has_code) { value->code = jet_rt_alloc(8, 8); *value->code = jet_rt_int_clone(code); }
    value->reason = jet_rt_string_clone(reason); return value;
}
static void text(const Text *value, const Text *expected) {
    assert(value->len == expected->len);
    if (value->len) assert(memcmp(value->data, expected->data, (size_t)value->len) == 0);
}
static void different_fields(Error *a, Error *b, int64_t zero, int64_t one) {
    Text changed = {"changed", 7, 0};
    Text *saved_text = b->operation; b->operation = jet_rt_string_clone(&changed);
    assert(!jet_rt_handle_eq_error(a, b)); jet_rt_string_drop(b->operation); b->operation = saved_text;
    Text **saved_server = b->server;
    b->server = NULL; if (saved_server) assert(!jet_rt_handle_eq_error(a, b));
    else { b->server = jet_rt_alloc(8, 8); *b->server = jet_rt_string_clone(&changed);
           assert(!jet_rt_handle_eq_error(a, b)); jet_rt_string_drop(*b->server);
           __real_jet_rt_free(b->server, 8, 8); }
    b->server = saved_server;
    if (saved_server) {
        Text *saved_payload = *saved_server; *saved_server = jet_rt_string_clone(&changed);
        assert(!jet_rt_handle_eq_error(a, b)); jet_rt_string_drop(*saved_server);
        *saved_server = saved_payload;
    }
    int64_t *saved_code = b->code; int64_t altered = one;
    if (saved_code) { b->code = NULL; assert(!jet_rt_handle_eq_error(a, b)); b->code = saved_code; }
    if (saved_code && jet_int_compare(*saved_code, one) == 0) altered = zero;
    b->code = &altered; assert(!jet_rt_handle_eq_error(a, b)); b->code = saved_code;
    saved_text = b->reason; b->reason = jet_rt_string_clone(&changed);
    assert(!jet_rt_handle_eq_error(a, b)); jet_rt_string_drop(b->reason); b->reason = saved_text;
    assert(jet_rt_handle_eq_error(a, b));
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    Text raw = {"send\0\xc3\xa9", 7, 0}, reason = {"raw\0reason\xe2\x98\x83", 13, 0};
    Text empty = {"", 0, 0}, server = {"mx.\xc3\xa9.test", 10, 0};
    if (argc > 1) {
        Error *bad = literal(strstr(argv[1], "-negative") ? -1 : 11, &empty, NULL, false, 0, &empty);
        Error *good = literal(0, &empty, NULL, false, 0, &empty);
        if (strncmp(argv[1], "bad-clone", 9) == 0) (void)jet_rt_handle_clone_error(bad);
        else if (strncmp(argv[1], "bad-drop", 8) == 0) jet_rt_handle_drop_error(bad);
        else if (strncmp(argv[1], "bad-eq-left", 11) == 0) (void)jet_rt_handle_eq_error(bad, good);
        else if (strncmp(argv[1], "bad-eq-right", 12) == 0) (void)jet_rt_handle_eq_error(good, bad);
        else return 2;
        assert(!"bad variant must stop"); return 1;
    }
    int64_t zero = jet_int_owned_from_i64(0), one = jet_int_owned_from_i64(1);
    int64_t max = jet_int_owned_from_i64(INT64_MAX), big = jet_int_add(max, max);
    int64_t negative = jet_int_sub(zero, big);
    for (int64_t tag = 0; tag < 11; ++tag) for (unsigned mode = 0; mode < 4; ++mode) {
        const Text *srv = (mode & 1) ? ((tag & 1) ? &empty : &server) : NULL;
        int64_t code = tag % 3 == 0 ? zero : tag % 3 == 1 ? big : negative;
        Error *a = literal(tag, &raw, srv, (mode & 2) != 0, code, &reason);
        Error *b = jet_rt_handle_clone_error(a);
        assert(a != b && b->tag == tag && jet_rt_handle_eq_error(a, b));
        assert(a->operation != b->operation && a->reason != b->reason);
        if (srv) assert(a->server != b->server && *a->server != *b->server);
        if (mode & 2) assert(a->code != b->code && jet_int_compare(*b->code, code) == 0);
        b->tag = (tag + 1) % 11; assert(!jet_rt_handle_eq_error(a, b)); b->tag = tag;
        different_fields(a, b, zero, one);
        watch(a); jet_rt_handle_drop_error(a); done();
        text(b->operation, &raw); text(b->reason, &reason);
        if (srv) text(*b->server, srv);
        if (mode & 2) assert(jet_int_compare(*b->code, code) == 0);
        watch(b); jet_rt_handle_drop_error(b); done();
    }
    // Identical arbitrary-precision codes in distinct real Int allocations compare by value.
    int64_t other_big = jet_int_add(max, max);
    Error *a = literal(0, &empty, NULL, true, big, &empty);
    Error *b = literal(0, &empty, NULL, true, other_big, &empty);
    assert(jet_rt_handle_eq_error(a, b));
    watch(a); jet_rt_handle_drop_error(a); done();
    watch(b); jet_rt_handle_drop_error(b); done();
    jet_rt_int_drop(other_big); jet_rt_int_drop(negative); jet_rt_int_drop(big); jet_rt_int_drop(max);
    jet_rt_int_drop(one); jet_rt_int_drop(zero);
    jet_rt_handle_drop_error(NULL); assert(nwatched == 0);
    puts("EmailError canonical40/eleven tags/deep owners/exact codes OK");
}
