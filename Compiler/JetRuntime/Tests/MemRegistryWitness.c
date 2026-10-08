// Link this witness against the composed native Jet runtime object, never a C
// replacement for its hooks. Observe.rs:915-957 is the reference contract.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

extern void jet_rt_mem_registry_init(void);
extern void jet_rt_mem_arena_open(void);
extern void jet_rt_mem_arena_alloc(uint64_t);
extern void jet_rt_mem_arena_reset(uint64_t, uint64_t);
extern void jet_rt_mem_arena_close(void);
extern void jet_rt_mem_arena_retain(uint64_t);
extern void jet_rt_mem_arena_release(uint64_t);
extern void jet_rt_mem_resources(uint64_t *);
extern void jet_rt_mem_allocator_memory(uint64_t *);
extern void jet_rt_mem_allocation_probe_reset(void);
extern void jet_rt_mem_allocation_probe_take(uint64_t *);

static void facts(uint64_t arenas, uint64_t allocations, uint64_t bytes) {
    uint64_t out[5] = {0xfeed, 0, 0, 0, 0xbeef};
    jet_rt_mem_resources(out + 1);
    assert(out[0] == 0xfeed && out[4] == 0xbeef);
    assert(out[1] == arenas && out[2] == allocations && out[3] == bytes);
}
static void memory(uint64_t retained, uint64_t high_water) {
    uint64_t out[2];
    jet_rt_mem_allocator_memory(out);
    assert(out[0] == retained && out[1] == high_water);
}
static void probe(uint64_t allocations, uint64_t bytes) {
    uint64_t out[2];
    jet_rt_mem_allocation_probe_take(out);
    assert(out[0] == allocations && out[1] == bytes);
}

enum { WORKERS = 8, ALLOCS = 25000 };
static pthread_barrier_t ready, release;
static void wait_barrier(pthread_barrier_t *barrier) {
    int rc = pthread_barrier_wait(barrier);
    assert(rc == 0 || rc == PTHREAD_BARRIER_SERIAL_THREAD);
}
static void *allocate(void *unused) {
    (void)unused;
    jet_rt_mem_arena_open();
    jet_rt_mem_arena_retain(ALLOCS * 8);
    for (int i = 0; i < ALLOCS; ++i) jet_rt_mem_arena_alloc(8);
    wait_barrier(&ready);
    wait_barrier(&release);
    jet_rt_mem_arena_reset(ALLOCS, ALLOCS * 8);
    jet_rt_mem_arena_close();
    jet_rt_mem_arena_release(ALLOCS * 8);
    return NULL;
}
int main(void) {
    // The runtime's init_array must initialize before an entryless library or
    // executable calls the accessor. Repeated init must not clear live facts.
    facts(0, 0, 0);
    jet_rt_mem_arena_open();
    jet_rt_mem_registry_init();
    jet_rt_mem_arena_open();
    jet_rt_mem_arena_retain(4096);
    jet_rt_mem_arena_alloc(0); // sizeof(ZST) counts once but contributes no byte
    jet_rt_mem_arena_alloc(8);
    jet_rt_mem_arena_alloc(3);
    facts(2, 3, 11);
    memory(4096, 11);
    probe(3, 11);
    probe(0, 0);
    facts(2, 3, 11);
    jet_rt_mem_arena_reset(2, 8);
    facts(2, 1, 3);
    jet_rt_mem_allocation_probe_reset();
    facts(2, 1, 3);
    jet_rt_mem_arena_reset(1, 3);
    jet_rt_mem_arena_close();
    jet_rt_mem_arena_close();
    jet_rt_mem_arena_release(4096);
    facts(0, 0, 0);
    memory(0, 11); // rewind/release do not erase process high-water

    assert(pthread_barrier_init(&ready, NULL, WORKERS + 1) == 0);
    assert(pthread_barrier_init(&release, NULL, WORKERS + 1) == 0);
    pthread_t threads[WORKERS];
    for (int i = 0; i < WORKERS; ++i)
        assert(pthread_create(&threads[i], NULL, allocate, NULL) == 0);
    wait_barrier(&ready);
    facts(WORKERS, WORKERS * ALLOCS, WORKERS * ALLOCS * 8);
    memory(WORKERS * ALLOCS * 8, WORKERS * ALLOCS * 8);
    probe(WORKERS * ALLOCS, WORKERS * ALLOCS * 8);
    facts(WORKERS, WORKERS * ALLOCS, WORKERS * ALLOCS * 8);
    wait_barrier(&release);
    for (int i = 0; i < WORKERS; ++i) assert(pthread_join(threads[i], NULL) == 0);
    assert(pthread_barrier_destroy(&ready) == 0);
    assert(pthread_barrier_destroy(&release) == 0);
    facts(0, 0, 0);
    memory(0, WORKERS * ALLOCS * 8);

    // Independent usize fetch_add wraps; only high-water's candidate saturates.
    jet_rt_mem_arena_open();
    jet_rt_mem_arena_alloc(UINT64_MAX);
    facts(1, 1, UINT64_MAX);
    memory(0, UINT64_MAX);
    jet_rt_mem_arena_alloc(2);
    facts(1, 2, 1);
    memory(0, UINT64_MAX);
    probe(2, 1);
    jet_rt_mem_arena_reset(2, 1);
    jet_rt_mem_arena_close();
    facts(0, 0, 0);
    puts("MemRegistry: lifecycle, zero-size, probe isolation, unsigned wrap, concurrent totals ok");
    return 0;
}
