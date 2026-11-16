// Transport only: deterministic callback bytes enter the ONE entropy provider.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_crypto_entropy_init(void);
extern void jet_rt_entropy_fixture_set(int64_t, int64_t);
extern void jet_rt_entropy_fixture_clear(void);
extern int64_t jet_rt_os_entropy_fixture_stride(void);
extern int64_t jet_rt_os_entropy_fixture_world(int64_t, void **, int64_t *);
extern int64_t jet_random_os_entropy(int64_t, void **, int64_t *);
extern void jet_rt_free(void *, int64_t, int64_t);
typedef struct { unsigned calls; int mode; int64_t filled; unsigned char *base; } Script;
static int64_t step(int64_t context, int64_t address, int64_t count) {
    Script *s = (Script *)(uintptr_t)context;
    unsigned char *out = (unsigned char *)(uintptr_t)address;
    ++s->calls;
    if (s->calls == 1) s->base = out;
    assert(out == s->base + s->filled);
    if (s->mode == 1 && s->calls == 2) { memset(out, 0x5a, (size_t)count); return -5; }
    if (s->mode == 0 && s->calls == 2) { memset(out, 0xa5, (size_t)count); return -4; }
    if (s->mode == 0 && s->calls == 3) for (int64_t i=0; i<count; ++i) assert(out[i] == 0);
    int64_t amount = s->mode != 2 && s->calls == 1 ? 3 : count;
    assert(amount <= count);
    for (int64_t i=0; i<amount; ++i) out[i] = (unsigned char)(0x80 + s->filled + i);
    s->filled += amount;
    return amount;
}
static void install(Script *s) { jet_rt_entropy_fixture_set((int64_t)(uintptr_t)step, (int64_t)(uintptr_t)s); }
static void success(int64_t count, int mode, unsigned calls, int64_t stride) {
    Script s = {0, mode, 0, NULL}; install(&s);
    void *data = NULL; int64_t length = -1;
    assert(jet_random_os_entropy(count, &data, &length) == 1);
    assert(length == count && s.calls == calls && data == s.base);
    const unsigned char *bytes = data;
    for (int64_t i=0; i<count; ++i) {
        assert(bytes[i*stride] == (unsigned char)(0x80+i));
        for (int64_t j=1; j<stride; ++j) assert(bytes[i*stride+j] == 0);
    }
    jet_rt_free(data, count*stride, stride);
    jet_rt_entropy_fixture_clear();
}
int main(int argc, char **argv) {
    jet_rt_alloc_init(); jet_rt_thread_init(); jet_rt_crypto_entropy_init();
    if (argc == 3 && strcmp(argv[1], "world") == 0) {
        void *data = NULL; int64_t length = -1;
        (void)jet_rt_os_entropy_fixture_world(strtoll(argv[2], NULL, 10), &data, &length);
        return 99; // Expected terminal rejection must never return to the host.
    }
    int64_t stride = jet_rt_os_entropy_fixture_stride(); assert(stride == 1 || stride == 8);
    Script s = {0, 2, 0, NULL}; install(&s);
    void *data = (void *)(uintptr_t)73; int64_t length = 91;
    assert(jet_random_os_entropy(-1, &data, &length) == 0 && data == (void *)(uintptr_t)73 && length == 91);
    assert(jet_random_os_entropy(1048577, &data, &length) == 0 && data == (void *)(uintptr_t)73 && length == 91);
    assert(jet_random_os_entropy(0, &data, &length) == 1 && data == NULL && length == 0 && s.calls == 0);
    jet_rt_entropy_fixture_clear();
    success(7, 0, 3, stride); success(256, 2, 1, stride); success(1048576, 2, 1, stride);
    s = (Script){0, 1, 0, NULL}; install(&s); data = (void *)(uintptr_t)73; length = 91;
    assert(jet_random_os_entropy(7, &data, &length) == 0 && s.calls == 2 && data == (void *)(uintptr_t)73 && length == 91);
    jet_rt_entropy_fixture_clear(); jet_rt_thread_detach();
    puts("os_entropy empty=Some bounds=None untouched=ok provider_calls=0");
    puts("os_entropy partial_interrupt=ok widening=ok max=ok failure=None");
    return 0;
}
