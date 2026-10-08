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
extern Enum *jet_rt_handle_clone_ProcessResourceLimit(const Enum *);
extern void jet_rt_handle_drop_ProcessResourceLimit(Enum *);
extern bool jet_rt_handle_eq_ProcessResourceLimit(const Enum *, const Enum *);
extern Text *jet_rt_handle_show_ProcessResourceLimit(const Enum *);
extern Text *jet_rt_handle_display_ProcessResourceLimit(const Enum *);
extern Text *jet_rt_handle_debug_ProcessResourceLimit(const Enum *);
extern Enum *jet_rt_handle_clone_ProcessStreamMode(const Enum *);
extern void jet_rt_handle_drop_ProcessStreamMode(Enum *);
extern bool jet_rt_handle_eq_ProcessStreamMode(const Enum *, const Enum *);
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
        Enum *clone = jet_rt_handle_clone_ProcessResourceLimit(&value);
        assert(clone != &value && clone->tag == tag);
        assert(jet_rt_handle_eq_ProcessResourceLimit(&value, clone));
        for (int64_t other = 0; other < 5; ++other) {
            Enum different = {other};
            assert(jet_rt_handle_eq_ProcessResourceLimit(&value, &different) == (tag == other));
        }
        value.tag = (tag + 1) % 5;
        assert(clone->tag == tag && !jet_rt_handle_eq_ProcessResourceLimit(&value, clone));
        Text *show = jet_rt_handle_show_ProcessResourceLimit(clone);
        Text *display = jet_rt_handle_display_ProcessResourceLimit(clone);
        Text *dbg = jet_rt_handle_debug_ProcessResourceLimit(clone);
        assert(show != display && show != dbg && display != dbg);
        text(show, human[tag]); text(display, human[tag]); text(dbg, debug[tag]);
        watched = clone; frees = 0;
        jet_rt_handle_drop_ProcessResourceLimit(clone);
        assert(frees == 1); watched = NULL;
    }
    for (int64_t tag = 0; tag < 3; ++tag) {
        Enum value = {tag};
        Enum *clone = jet_rt_handle_clone_ProcessStreamMode(&value);
        assert(clone != &value && clone->tag == tag && jet_rt_handle_eq_ProcessStreamMode(&value, clone));
        for (int64_t other = 0; other < 3; ++other) {
            Enum different = {other};
            assert(jet_rt_handle_eq_ProcessStreamMode(&value, &different) == (tag == other));
        }
        value.tag = (tag + 1) % 3;
        assert(clone->tag == tag && !jet_rt_handle_eq_ProcessStreamMode(&value, clone));
        watched = clone; frees = 0;
        jet_rt_handle_drop_ProcessStreamMode(clone);
        assert(frees == 1); watched = NULL;
    }
    jet_rt_handle_drop_ProcessResourceLimit(NULL);
    jet_rt_handle_drop_ProcessStreamMode(NULL);
    assert(intercepted > 0);
    puts("ProcessEnums ownership/equality/render OK");
}
