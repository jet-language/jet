// Test-only C callbacks: real budget rail, deliberately unwritten stop outs.
// The linked Jet fixture counts its own bounded, single-thread slab payloads.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

extern bool jet_rt_comptime_begin(uint64_t fuel, uint64_t depth);
extern void jet_rt_comptime_work(uint64_t units);
extern void jet_rt_comptime_end(void);
extern uint64_t jet_rt_session_propagate(void);
extern int64_t jet_rt_string_from_static(int64_t data, int64_t length);
extern void jet_rt_free(int64_t address, int64_t size, int64_t alignment);
extern void collections_stop_test(int64_t family, int64_t unary, int64_t binary,
                                  int64_t show, int64_t release, int64_t list_drop);
static int calls, stop_at, releases, lists_dropped;
static bool stopped(void) {
    ++calls;
    if (calls == stop_at) jet_rt_comptime_work(UINT64_C(1000001));
    return jet_rt_session_propagate() != 0;
}
static int64_t unary(int64_t env, int64_t input, int64_t output) {
    (void)env;
    if (!stopped()) *(int64_t *)(intptr_t)output = *(int64_t *)(intptr_t)input;
    return 0;
}
static int64_t binary(int64_t env, int64_t a, int64_t b, int64_t output) {
    (void)env; (void)a; (void)b;
    if (!stopped()) *(unsigned char *)(intptr_t)output = 0;
    return 0;
}
static int64_t show(int64_t input) {
    (void)input;
    // Poison must never be read or dropped by join on the stopped edge.
    if (stopped()) return 1;
    static const char text[] = "x";
    return jet_rt_string_from_static((int64_t)(intptr_t)text, 1);
}
static void release(int64_t env) {
    (void)env;
    ++releases;
}
static void list_drop(int64_t slot) {
    int64_t *header = (int64_t *)(intptr_t)slot;
    if (header[1]) jet_rt_free(header[2], header[1] * 8, 8);
    header[0] = header[1] = header[2] = 0;
    ++lists_dropped;
}
extern int64_t jet_rt_alloc(int64_t size, int64_t alignment);
extern void jet_rt_progress_state_advance(int64_t state);
extern void collections_review_test(int64_t family, int64_t unary, int64_t binary,
                                    int64_t clone, int64_t drop, int64_t box_drop);
static int review_family, clones, drops, binaries, hooks, advances;
static int64_t armed;
void coll_stop_arm(int64_t address) { assert(!armed); armed = address; }
void coll_stop_after_free(int64_t address) {
    if (armed && address == armed) {
        armed = 0;
        ++hooks;
        jet_rt_comptime_work(UINT64_C(1000001));
    }
}
void coll_stop_progress_advance(int64_t state) {
    jet_rt_progress_state_advance(state);
    ++advances;
    jet_rt_comptime_work(UINT64_C(1000001));
}
static void owned_clone(int64_t input, int64_t output) {
    int64_t payload = jet_rt_alloc(8, 8);
    *(int64_t *)(intptr_t)payload = **(int64_t **)(intptr_t)input;
    *(int64_t *)(intptr_t)output = payload;
    ++clones;
}
static void owned_drop(int64_t slot) {
    int64_t *word = (int64_t *)(intptr_t)slot;
    assert(*word);
    jet_rt_free(*word, 8, 8);
    *word = 0;
    ++drops;
}
static void boxed_drop(int64_t slot) {
    int64_t box = *(int64_t *)(intptr_t)slot;
    assert(*(int64_t *)(intptr_t)box == 0);
    owned_drop(box + 8);
    jet_rt_free(box, 16, 8);
}
static int64_t owned_unary(int64_t env, int64_t input, int64_t output) {
    (void)env;
    if (review_family == 7 || review_family == 8) {
        int64_t box = jet_rt_alloc(16, 8);
        *(int64_t *)(intptr_t)box = 0;
        *(int64_t *)(intptr_t)(box + 8) = jet_rt_alloc(8, 8);
        *(int64_t *)(intptr_t)output = box;
        coll_stop_arm(box);
    } else if (!stopped()) {
        owned_clone(input, output);
    }
    return 0;
}
static int64_t owned_binary(int64_t env, int64_t a, int64_t b, int64_t output) {
    (void)env; (void)a; (void)b; (void)output;
    ++binaries;
    assert(!"zip callback must not run after either source stops");
    return 0;
}
extern int64_t jet_rt_alloc(int64_t size, int64_t alignment);
extern void jet_rt_progress_state_advance(int64_t state);
extern void collections_review_test(int64_t family, int64_t unary, int64_t binary,
                                    int64_t clone, int64_t drop, int64_t box_drop);
static int review_family, clones, drops, binaries, hooks, advances;
static int64_t armed;
void coll_stop_arm(int64_t address) { assert(!armed); armed = address; }
void coll_stop_after_free(int64_t address) {
    if (armed && address == armed) {
        armed = 0;
        ++hooks;
        jet_rt_comptime_work(UINT64_C(1000001));
    }
}
void coll_stop_progress_advance(int64_t state) {
    jet_rt_progress_state_advance(state);
    ++advances;
    jet_rt_comptime_work(UINT64_C(1000001));
}
static int64_t owned_clone(int64_t input, int64_t output) {
    int64_t payload = jet_rt_alloc(8, 8);
    *(int64_t *)(intptr_t)payload = **(int64_t **)(intptr_t)input;
    *(int64_t *)(intptr_t)output = payload;
    ++clones;
    return 0;
}
static int64_t owned_drop(int64_t slot) {
    int64_t *word = (int64_t *)(intptr_t)slot;
    assert(*word);
    jet_rt_free(*word, 8, 8);
    *word = 0;
    ++drops;
    return 0;
}
static int64_t boxed_drop(int64_t slot) {
    int64_t box = *(int64_t *)(intptr_t)slot;
    assert(*(int64_t *)(intptr_t)box == 0);
    owned_drop(box + 8);
    jet_rt_free(box, 16, 8);
    return 0;
}
static int64_t owned_unary(int64_t env, int64_t input, int64_t output) {
    (void)env;
    if (review_family == 7 || review_family == 8) {
        int64_t box = jet_rt_alloc(16, 8);
        *(int64_t *)(intptr_t)box = 0;
        *(int64_t *)(intptr_t)(box + 8) = jet_rt_alloc(8, 8);
        *(int64_t *)(intptr_t)output = box;
        coll_stop_arm(box);
    } else if (!stopped()) {
        owned_clone(input, output);
    }
    return 0;
}
static int64_t owned_binary(int64_t env, int64_t a, int64_t b, int64_t output) {
    (void)env; (void)a; (void)b; (void)output;
    ++binaries;
    assert(!"zip callback must not run after either source stops");
    return 0;
}
int main(void) {
    for (int family = 0; family != 13; ++family) {
        calls = releases = lists_dropped = 0;
        stop_at = family == 0 ? 1 : 2;
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        collections_stop_test(family, (int64_t)(intptr_t)unary,
                              (int64_t)(intptr_t)binary, (int64_t)(intptr_t)show,
                              (int64_t)(intptr_t)release, (int64_t)(intptr_t)list_drop);
        assert(calls == stop_at);
        assert(releases == (family == 8 ? 0 : family >= 11 ? 2 : 1));
        if (family == 7) assert(lists_dropped == 1);
        jet_rt_comptime_end();
        assert(jet_rt_session_propagate() == 0);
        printf("family %d: live payloads restored\n", family);
    }
    for (review_family = 0; review_family != 11; ++review_family) {
        calls = clones = drops = binaries = hooks = advances = 0;
        armed = 0;
        stop_at = review_family == 0 ? 3 :
                  review_family == 1 || review_family == 6 ? 2 : 1;
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        collections_review_test(review_family, (int64_t)(intptr_t)owned_unary,
                                (int64_t)(intptr_t)owned_binary,
                                (int64_t)(intptr_t)owned_clone,
                                (int64_t)(intptr_t)owned_drop,
                                (int64_t)(intptr_t)boxed_drop);
        assert(jet_rt_session_propagate() != 0);
        assert(!armed && binaries == 0);
        if (review_family <= 6) assert(calls == stop_at);
        if (review_family >= 7 && review_family <= 9) assert(hooks == 1 && drops == (review_family == 9 ? 1 : 2));
        if (review_family == 10) assert(advances == 1 && drops == 1);
        jet_rt_comptime_end();
        assert(jet_rt_session_propagate() == 0);
        printf("review %d: live payloads restored\n", review_family);
    }
    return 0;
}
