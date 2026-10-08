// Client of the actual emitted Jet Shared providers and #4795 payload glue.
// No Rust seed, replacement Shared implementation or simulated CAS lives here.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>

typedef uintptr_t W;
typedef struct { W stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern W jet_rt_native_shared_new(W, const Meta *, W);
extern W jet_rt_native_shared_clone(W);
extern void jet_rt_native_shared_drop(W);
extern W jet_rt_shared_site_publish(W, W, W);
extern void jet_rt_global_reset_registered(void);
extern W jet_rt_shared_fixture_registration_count(W);
static const Meta word_meta = {8,0,0,0,0,0,2,0,0};
static atomic_uint drops[5];
static _Atomic W world_cell, process_cell;
static void payload_drop(W slot) {
    W id = *(W *)slot;
    assert(id < 5);
    assert(atomic_fetch_add(&drops[id], 1) == 0);
}
typedef struct { W id, cell, lifetime, candidate, winner; pthread_barrier_t *gate; } Racer;
static void *publish(void *opaque) {
    Racer *r = opaque;
    jet_rt_thread_attach();
    r->candidate = jet_rt_native_shared_new((W)&r->id, &word_meta, (W)payload_drop);
    int status = pthread_barrier_wait(r->gate);
    assert(status == 0 || status == PTHREAD_BARRIER_SERIAL_THREAD);
    r->winner = jet_rt_shared_site_publish(r->cell, r->candidate, r->lifetime);
    jet_rt_thread_detach();
    return NULL;
}
static W race(_Atomic W *cell, W lifetime, W first) {
    pthread_barrier_t gate;
    pthread_t threads[2];
    Racer r[2] = {{first,(W)cell,lifetime,0,0,&gate}, {first+1,(W)cell,lifetime,0,0,&gate}};
    assert(!pthread_barrier_init(&gate, NULL, 2));
    for (unsigned i = 0; i < 2; ++i) assert(!pthread_create(&threads[i], NULL, publish, &r[i]));
    for (unsigned i = 0; i < 2; ++i) assert(!pthread_join(threads[i], NULL));
    assert(!pthread_barrier_destroy(&gate));
    W winner = atomic_load(cell);
    assert(winner && r[0].winner == winner && r[1].winner == winner);
    assert(r[0].candidate != r[1].candidate);
    for (unsigned i = 0; i < 2; ++i) {
        assert(atomic_load(&drops[r[i].id]) == (r[i].candidate != winner));
    }
    return winner;
}
int main(void) {
    jet_rt_thread_attach();
    W winner = race(&world_cell, 1, 0);
    assert(jet_rt_shared_fixture_registration_count((W)&world_cell) == 1);
    W retained = jet_rt_native_shared_clone(winner);
    jet_rt_global_reset_registered();
    assert(atomic_load(&world_cell) == 0);
    assert(atomic_load(&drops[0]) + atomic_load(&drops[1]) == 1);
    jet_rt_global_reset_registered();
    assert(atomic_load(&drops[0]) + atomic_load(&drops[1]) == 1);
    // A later World reuses the same pinned registry node, never registering twice.
    W id = 2;
    W next = jet_rt_native_shared_new((W)&id, &word_meta, (W)payload_drop);
    assert(jet_rt_shared_site_publish((W)&world_cell, next, 1) == next);
    assert(jet_rt_shared_fixture_registration_count((W)&world_cell) == 1);
    jet_rt_native_shared_drop(retained);
    assert(atomic_load(&drops[0]) == 1 && atomic_load(&drops[1]) == 1);
    jet_rt_global_reset_registered();
    assert(atomic_load(&world_cell) == 0 && atomic_load(&drops[2]) == 1);
    puts("Shared World race/loser once/register once/reset owner only ok");
    winner = race(&process_cell, 0, 3);
    assert(jet_rt_shared_fixture_registration_count((W)&process_cell) == 0);
    jet_rt_global_reset_registered();
    assert(atomic_load(&process_cell) == winner);
    assert(atomic_load(&drops[3]) + atomic_load(&drops[4]) == 1);
    jet_rt_native_shared_drop(atomic_exchange(&process_cell, 0));
    assert(atomic_load(&drops[3]) == 1 && atomic_load(&drops[4]) == 1);
    puts("Shared Process race/loser once/no World registration ok");
    jet_rt_thread_detach();
    return 0;
}
