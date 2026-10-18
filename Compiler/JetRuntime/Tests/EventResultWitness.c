// Link real Jet exports with -Wl,--wrap=jet_rt_free.
// invalid-tag mode must terminate via shared record-variant panic (exit101).
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern int64_t *jet_rt_handle_clone_jet_event_result(int64_t *);
extern void jet_rt_handle_drop_jet_event_result(int64_t *);
extern bool jet_rt_handle_eq_jet_event_result(int64_t *, int64_t *);
extern Text *jet_rt_handle_show_jet_event_result(int64_t *);
extern void jet_rt_string_drop(Text *);
static struct { void *pointer; int64_t size, align; unsigned frees; } watched[6];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < count; ++i) if (pointer == watched[i].pointer) {
        assert(size == watched[i].size && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, size, align);
}
static void watch(void *pointer, int64_t size, int64_t align) {
    assert(count < 6);
    for (size_t i = 0; i < count; ++i) assert(pointer != watched[i].pointer);
    watched[count].pointer = pointer;
    watched[count].size = size;
    watched[count].align = align;
    watched[count++].frees = 0;
}
static int64_t *new_result(int64_t ordinal) {
    int64_t *result = jet_rt_alloc(8, 8);
    *result = ordinal;
    return result;
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc == 2 && strcmp(argv[1], "invalid-tag") == 0) {
        (void)jet_rt_handle_show_jet_event_result(new_result(2));
        fputs("invalid-tag unexpectedly returned\n", stderr);
        return 99;
    }
    assert(argc == 1);
    const char *expected[] = {"Handled", "Ignored"};
    for (int64_t ordinal = 0; ordinal < 2; ++ordinal) {
        int64_t *a = new_result(ordinal);
        int64_t *b = jet_rt_handle_clone_jet_event_result(a);
        assert(a != b && *a == ordinal && *b == ordinal);
        assert(jet_rt_handle_eq_jet_event_result(a, a) && jet_rt_handle_eq_jet_event_result(a, b));
        Text *first = jet_rt_handle_show_jet_event_result(a);
        Text *second = jet_rt_handle_show_jet_event_result(a);
        assert(first != second && first->data != second->data);
        assert(first->length == 7 && second->length == 7);
        assert(memcmp(first->data, expected[ordinal], 7) == 0);
        assert(memcmp(second->data, expected[ordinal], 7) == 0);
        first->data[0] = '?';
        assert(memcmp(second->data, expected[ordinal], 7) == 0 && *a == ordinal);
        *b = 1 - ordinal;
        assert(*a == ordinal && !jet_rt_handle_eq_jet_event_result(a, b));
        assert(!jet_rt_handle_eq_jet_event_result(b, a) && jet_rt_handle_eq_jet_event_result(b, b));
        watch(a, 8, 8); watch(b, 8, 8);
        watch((char *)(first)-8,32,8); watch(first->data, first->capacity, 1);
        watch((char *)(second)-8,32,8); watch(second->data, second->capacity, 1);
        unsigned before = intercepted;
        jet_rt_handle_drop_jet_event_result(a); jet_rt_handle_drop_jet_event_result(b);
        jet_rt_string_drop(first); jet_rt_string_drop(second);
        assert(intercepted == before + count);
        for (size_t i = 0; i < count; ++i) assert(watched[i].frees == 1);
        count = 0;
    }
    unsigned before = intercepted;
    jet_rt_handle_drop_jet_event_result(NULL);
    assert(intercepted == before && intercepted != 0);
    puts("EventResult ownership/equality/Show OK");
    return 0;
}
