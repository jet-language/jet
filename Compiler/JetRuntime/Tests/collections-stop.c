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
static int64_t release(int64_t env) {
    (void)env;
    ++releases;
    return 0;
}
static int64_t list_drop(int64_t slot) {
    int64_t *header = (int64_t *)(intptr_t)slot;
    if (header[1]) jet_rt_free(header[2], header[1] * 8, 8);
    header[0] = header[1] = header[2] = 0;
    ++lists_dropped;
    return 0;
}
int main(void) {
    for (int family = 0; family != 11; ++family) {
        calls = releases = lists_dropped = 0;
        stop_at = family == 0 ? 1 : 2;
        assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
        collections_stop_test(family, (int64_t)(intptr_t)unary,
                              (int64_t)(intptr_t)binary, (int64_t)(intptr_t)show,
                              (int64_t)(intptr_t)release, (int64_t)(intptr_t)list_drop);
        assert(calls == stop_at);
        assert(releases == (family == 8 ? 0 : 1));
        if (family == 7) assert(lists_dropped == 1);
        jet_rt_comptime_end();
        assert(jet_rt_session_propagate() == 0);
        printf("family %d: live payloads restored\n", family);
    }
    return 0;
}
