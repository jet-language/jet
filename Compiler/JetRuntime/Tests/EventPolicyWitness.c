// Link real composed Jet runtime exports with -Wl,--wrap=jet_rt_free.
// Native policy carriers share EncodingABI's owned eight-byte ordinal word.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern int64_t *jet_rt_handle_clone_jet_hook_policy(int64_t *);
extern void jet_rt_handle_drop_jet_hook_policy(int64_t *);
extern int64_t *jet_rt_handle_clone_jet_event_policy(int64_t *);
extern void jet_rt_handle_drop_jet_event_policy(int64_t *);
typedef int64_t *(*Clone)(int64_t *);
typedef void (*Drop)(int64_t *);
static struct { void *pointer; unsigned frees; } watched[3];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t alignment) {
    ++intercepted;
    for (size_t i = 0; i < count; ++i) {
        if (pointer == watched[i].pointer) {
            assert(size == 8 && alignment == 8);
            assert(++watched[i].frees == 1);
        }
    }
    __real_jet_rt_free(pointer, size, alignment);
}
static void watch(void *pointer) {
    assert(count < 3);
    for (size_t i = 0; i < count; ++i) assert(pointer != watched[i].pointer);
    watched[count].pointer = pointer;
    watched[count++].frees = 0;
}
static void policy_cases(Clone clone, Drop drop) {
    for (unsigned repeat = 0; repeat < 16; ++repeat) {
        count = 0;
        int64_t *owner = jet_rt_alloc(8, 8);
        *owner = 0; // Only admitted ordinal in either policy.
        int64_t *first = clone(owner);
        int64_t *second = clone(first);
        assert(owner != first && first != second && owner != second);
        assert(*owner == 0 && *first == 0 && *second == 0);
        watch(owner); watch(first); watch(second);
        unsigned before = intercepted;
        drop(owner);
        assert(*first == 0 && *second == 0);
        drop(first);
        assert(*second == 0);
        drop(second);
        assert(intercepted == before + 3);
        for (size_t i = 0; i < count; ++i) assert(watched[i].frees == 1);
        count = 0;
        before = intercepted;
        drop(NULL);
        assert(intercepted == before);
    }
}
int main(void) {
    jet_rt_alloc_init();
    policy_cases(jet_rt_handle_clone_jet_hook_policy, jet_rt_handle_drop_jet_hook_policy);
    policy_cases(jet_rt_handle_clone_jet_event_policy, jet_rt_handle_drop_jet_event_policy);
    assert(intercepted != 0);
    puts("EventPolicy/HookPolicy ownership OK");
    return 0;
}
