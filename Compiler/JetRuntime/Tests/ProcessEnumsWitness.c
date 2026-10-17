// Link real Jet runtime object with -Wl,--wrap=jet_rt_free. No provider mocks.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int64_t tag; } Enum;
typedef struct { const char *data; int64_t len, capacity; } Text;
_Static_assert(sizeof(Enum) == 8, "shared native enum shell");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Enum *jet_rt_handle_clone_process_resource_limit(const Enum *);
extern void jet_rt_handle_drop_process_resource_limit(Enum *);
extern bool jet_rt_handle_eq_process_resource_limit(const Enum *, const Enum *);
extern Text *jet_rt_handle_show_process_resource_limit(const Enum *);
extern Text *jet_rt_handle_display_process_resource_limit(const Enum *);
extern Text *jet_rt_handle_debug_process_resource_limit(const Enum *);
extern Enum *jet_rt_handle_clone_process_stream_mode(const Enum *);
extern void jet_rt_handle_drop_process_stream_mode(Enum *);
extern bool jet_rt_handle_eq_process_stream_mode(const Enum *, const Enum *);
extern void jet_rt_string_drop(Text *);
static void *watched;
static unsigned frees, intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    ++intercepted;
    if (pointer == watched) { assert(bytes == 8 && align == 8); assert(++frees == 1); }
    __real_jet_rt_free(pointer, bytes, align);
}
static void text(Text *owned, const char *expected) {
    assert(owned->len == (int64_t)strlen(expected));
    assert(memcmp(owned->data, expected, (size_t)owned->len) == 0);
    jet_rt_string_drop(owned);
}
int main(void) {
    static const char *human[] = {"wall time", "CPU time", "memory", "open file handles", "output"};
    static const char *debug[] = {"WallTime", "CpuTime", "Memory", "OpenFiles", "Output"};
    jet_rt_alloc_init();
    // Borrow native value fixtures; clone is the sole producer in this family.
    for (int64_t tag = 0; tag < 5; ++tag) {
        Enum value = {tag};
        Enum *clone = jet_rt_handle_clone_process_resource_limit(&value);
        assert(clone != &value && clone->tag == tag);
        assert(jet_rt_handle_eq_process_resource_limit(&value, clone));
        for (int64_t other = 0; other < 5; ++other) {
            Enum different = {other};
            assert(jet_rt_handle_eq_process_resource_limit(&value, &different) == (tag == other));
        }
        value.tag = (tag + 1) % 5;
        assert(clone->tag == tag && !jet_rt_handle_eq_process_resource_limit(&value, clone));
        Text *show = jet_rt_handle_show_process_resource_limit(clone);
        Text *display = jet_rt_handle_display_process_resource_limit(clone);
        Text *dbg = jet_rt_handle_debug_process_resource_limit(clone);
        assert(show != display && show != dbg && display != dbg);
        text(show, human[tag]); text(display, human[tag]); text(dbg, debug[tag]);
        watched = clone; frees = 0;
        jet_rt_handle_drop_process_resource_limit(clone);
        assert(frees == 1); watched = NULL;
    }
    for (int64_t tag = 0; tag < 3; ++tag) {
        Enum value = {tag};
        Enum *clone = jet_rt_handle_clone_process_stream_mode(&value);
        assert(clone != &value && clone->tag == tag && jet_rt_handle_eq_process_stream_mode(&value, clone));
        for (int64_t other = 0; other < 3; ++other) {
            Enum different = {other};
            assert(jet_rt_handle_eq_process_stream_mode(&value, &different) == (tag == other));
        }
        value.tag = (tag + 1) % 3;
        assert(clone->tag == tag && !jet_rt_handle_eq_process_stream_mode(&value, clone));
        watched = clone; frees = 0;
        jet_rt_handle_drop_process_stream_mode(clone);
        assert(frees == 1); watched = NULL;
    }
    jet_rt_handle_drop_process_resource_limit(NULL);
    jet_rt_handle_drop_process_stream_mode(NULL);
    assert(intercepted > 0);
    puts("ProcessEnums ownership/equality/render OK");
}
