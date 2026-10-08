// Actual Jet providers only: weak reservations never retain the payload.
#define _GNU_SOURCE
#include <assert.h>
#include <stdbool.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>

typedef uintptr_t W;
typedef struct { W stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern W jet_rt_native_shared_new(W, const Meta *, W);
extern void jet_rt_native_shared_drop(W);
extern W jet_rt_native_shared_downgrade(W);
extern bool jet_rt_native_shared_weak_upgrade(W, W);
extern void jet_rt_native_shared_weak_drop(W);
extern void jet_rt_program_allocator_configure(W, W);
extern void jet_rt_program_allocator_restore(W, W);
extern W jet_rt_program_allocator_fact(W);
static const Meta word_meta = {8,0,0,0,0,0,(UINT64_C(64)<<32)|2,0,0};
static atomic_uint drops;
static void payload_drop(W slot) {
    assert(*(W *)slot == 37);
    assert(atomic_fetch_add(&drops, 1) == 0);
}
static W create(void) {
    W payload = 37;
    return jet_rt_native_shared_new((W)&payload, &word_meta, (W)payload_drop);
}
static void lifetime(void) {
    jet_rt_program_allocator_configure(1, 0);
    W base = jet_rt_program_allocator_fact(2);
    W cell = create(), weak = jet_rt_native_shared_downgrade(cell);
    W another = jet_rt_native_shared_downgrade(cell);
    W retained = 0;
    assert(jet_rt_native_shared_weak_upgrade(weak, (W)&retained) && retained == cell);
    jet_rt_native_shared_drop(cell);
    assert(atomic_load(&drops) == 0);
    jet_rt_native_shared_drop(retained);
    assert(atomic_load(&drops) == 1);
    // Descriptor and payload are gone; only the Cell96 weak allocation lives.
    assert(jet_rt_program_allocator_fact(2) == base + 96);
    retained = 123;
    assert(!jet_rt_native_shared_weak_upgrade(weak, (W)&retained) && retained == 123);
    jet_rt_native_shared_weak_drop(weak);
    assert(jet_rt_program_allocator_fact(2) == base + 96);
    jet_rt_native_shared_weak_drop(another);
    assert(jet_rt_program_allocator_fact(2) == base);
    jet_rt_program_allocator_restore(0, 0);
    puts("Shared weak no resurrection/payload once/last weak frees Cell96 ok");
}
typedef struct { W strong, weak; pthread_barrier_t *gate; } Race;
static void wait_gate(pthread_barrier_t *gate) {
    int status = pthread_barrier_wait(gate);
    assert(status == 0 || status == PTHREAD_BARRIER_SERIAL_THREAD);
}
static void *drop_racer(void *opaque) {
    Race *r = opaque;
    jet_rt_thread_attach();
    wait_gate(r->gate);
    jet_rt_native_shared_drop(r->strong);
    jet_rt_thread_detach();
    return NULL;
}
static void *upgrade_racer(void *opaque) {
    Race *r = opaque;
    jet_rt_thread_attach();
    wait_gate(r->gate);
    W retained = 123;
    if (jet_rt_native_shared_weak_upgrade(r->weak, (W)&retained)) {
        assert(retained == r->strong && atomic_load(&drops) == 0);
        jet_rt_native_shared_drop(retained);
    } else {
        assert(retained == 123);
    }
    jet_rt_thread_detach();
    return NULL;
}
static void race(void) {
    for (unsigned i = 0; i < 256; ++i) {
        atomic_store(&drops, 0);
        pthread_barrier_t gate;
        assert(!pthread_barrier_init(&gate, NULL, 2));
        W cell = create();
        Race r = {cell, jet_rt_native_shared_downgrade(cell), &gate};
        pthread_t threads[2];
        assert(!pthread_create(&threads[0], NULL, drop_racer, &r));
        assert(!pthread_create(&threads[1], NULL, upgrade_racer, &r));
        for (unsigned t = 0; t < 2; ++t) assert(!pthread_join(threads[t], NULL));
        assert(!pthread_barrier_destroy(&gate));
        assert(atomic_load(&drops) == 1);
        W retained = 123;
        assert(!jet_rt_native_shared_weak_upgrade(r.weak, (W)&retained) && retained == 123);
        jet_rt_native_shared_weak_drop(r.weak);
    }
    puts("Shared weak upgrade versus final strong drop 256 races ok");
}
int main(void) {
    jet_rt_thread_attach();
    lifetime();
    race();
    jet_rt_thread_detach();
    return 0;
}
