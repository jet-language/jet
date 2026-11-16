// Calls the real resident-session ABI; no session implementation or codecs.
// Link against the freshly composed Jet runtime library, not a cached pack.
#include <assert.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

typedef uint64_t W;
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern W jet_rt_session_create(void);
extern bool jet_rt_session_begin(W);
extern bool jet_rt_session_end(W);
extern void *jet_rt_session_reserve(W, W, W, W, W, W, W, W, void (*)(void *));
extern bool jet_rt_session_publish(W, W);
extern void *jet_rt_session_resolve(W, W, W, W, W, W, W, W);
extern void *jet_rt_session_take(W, W, W, W, W, W, W, W);
extern bool jet_rt_session_retire(W, W);
extern bool jet_rt_session_destroy(W);
static _Thread_local W order[8];
static _Thread_local unsigned drops;
static _Thread_local W reentrant_session;
static void record_drop(void *payload) {
    assert(drops < 8);
    order[drops++] = *(W *)payload;
    // Retire/destroy must detach the owner before any canonical glue runs.
    if (reentrant_session) {
        assert(jet_rt_session_begin(reentrant_session));
        assert(jet_rt_session_end(reentrant_session));
    }
}
static W *reserve(W id, W binding, void (*glue)(void *)) {
    return jet_rt_session_reserve(id, binding, 1, 2, 3, 4, 8, 8, glue);
}
static W *resolve(W id, W binding) {
    return jet_rt_session_resolve(id, binding, 1, 2, 3, 4, 8, 8);
}
static void carries_places(void) {
    W id = jet_rt_session_create();
    assert(id && !reserve(id, 7, NULL));
    assert(jet_rt_session_begin(id));
    W *ptr = reserve(id, 7, NULL);
    assert(ptr && !resolve(id, 7));
    *ptr = 41;
    assert(jet_rt_session_publish(id, 7));
    assert(!jet_rt_session_publish(id, 7));
    assert(jet_rt_session_end(id) && !resolve(id, 7));
    for (unsigned turn = 0; turn < 3; ++turn) {
        assert(jet_rt_session_begin(id));
        assert(resolve(id, 7) == ptr && *ptr == 41);
        assert(jet_rt_session_end(id));
    }
    assert(jet_rt_session_destroy(id));
}
static void validates_places(void) {
    W id = jet_rt_session_create();
    assert(jet_rt_session_begin(id) && !jet_rt_session_begin(id));
    assert(reserve(id, 1, NULL) && !reserve(id, 1, NULL));
    assert(!jet_rt_session_reserve(id, 2, 1, 2, 3, 4, 8, 3, NULL));
    assert(!jet_rt_session_reserve(id, 2, 1, 2, 3, 4, UINT64_MAX, 8, NULL));
    W *ptr = reserve(id, 3, NULL);
    assert(ptr);
    *ptr = 9;
    assert(jet_rt_session_publish(id, 3));
    for (unsigned key = 0; key < 4; ++key) {
        W keys[] = {1, 2, 3, 4};
        keys[key] ^= 8;
        assert(!jet_rt_session_resolve(id, 3, keys[0], keys[1], keys[2], keys[3], 8, 8));
    }
    assert(!jet_rt_session_resolve(id, 3, 1, 2, 3, 4, 4, 8));
    assert(!jet_rt_session_resolve(id, 3, 1, 2, 3, 4, 8, 16));
    assert(!jet_rt_session_destroy(id));
    assert(jet_rt_session_end(id) && !jet_rt_session_end(id));
    assert(jet_rt_session_begin(id) && !resolve(id, 1));
    assert(jet_rt_session_end(id));
    assert(jet_rt_session_destroy(id) && !jet_rt_session_destroy(id));
}
static void drops_and_moves(void) {
    drops = 0;
    W id = jet_rt_session_create();
    assert(jet_rt_session_begin(id));
    for (W binding = 1; binding <= 3; ++binding) {
        W *ptr = reserve(id, binding, record_drop);
        assert(ptr);
        if (binding != 3) { *ptr = binding; assert(jet_rt_session_publish(id, binding)); }
    }
    assert(jet_rt_session_retire(id, 1) && !jet_rt_session_retire(id, 1));
    W *moved = jet_rt_session_take(id, 2, 1, 2, 3, 4, 8, 8);
    assert(moved && *moved == 2 && !resolve(id, 2));
    assert(!jet_rt_session_take(id, 2, 1, 2, 3, 4, 8, 8));
    assert(jet_rt_session_end(id) && jet_rt_session_destroy(id));
    assert(drops == 1 && order[0] == 1);
}
static void teardown_order_and_unsigned_ids(void) {
    drops = 0;
    W id = jet_rt_session_create();
    assert(jet_rt_session_begin(id));
    W bindings[] = {0, INT64_MAX, (W)INT64_MAX + 1, UINT64_MAX};
    for (unsigned at = 0; at < 4; ++at) {
        W *ptr = reserve(id, bindings[at], record_drop);
        assert(ptr); *ptr = at;
        assert(jet_rt_session_publish(id, bindings[at]));
    }
    assert(jet_rt_session_retire(id, bindings[1]));
    assert(!reserve(id, bindings[1], NULL));
    assert(resolve(id, bindings[2]));
    assert(jet_rt_session_end(id));
    // Drop glue can begin another inactive session without a registry lock.
    reentrant_session = jet_rt_session_create();
    assert(jet_rt_session_destroy(id));
    assert(jet_rt_session_destroy(reentrant_session));
    reentrant_session = 0;
    assert(drops == 4 && order[0] == 1 && order[1] == 3 && order[2] == 2 && order[3] == 0);
}
static void *other_worker(void *argument) {
    jet_rt_thread_attach();
    W id = *(W *)argument;
    assert(!jet_rt_session_begin(id));
    assert(!resolve(id, 1) && !jet_rt_session_destroy(id));
    jet_rt_thread_detach();
    return NULL;
}
static void alignment_and_thread_confinement(void) {
    W id = jet_rt_session_create();
    assert(jet_rt_session_begin(id));
    void *ptr = jet_rt_session_reserve(id, 1, 0, 0, 0, 0, 0, 64, NULL);
    assert(ptr && (uintptr_t)ptr % 64 == 0);
    assert(jet_rt_session_publish(id, 1));
    pthread_t thread;
    assert(!pthread_create(&thread, NULL, other_worker, &id));
    assert(!pthread_join(thread, NULL));
    assert(jet_rt_session_end(id) && jet_rt_session_destroy(id));
}
static void prepares_tls_teardown(void) {
    drops = 0;
    W first = jet_rt_session_create();
    W second = jet_rt_session_create();
    assert(first < second && jet_rt_session_begin(first));
    for (W binding = 1; binding <= 2; ++binding) {
        W *ptr = reserve(first, binding, record_drop);
        assert(ptr); *ptr = 4 + binding;
        assert(jet_rt_session_publish(first, binding));
    }
    assert(jet_rt_session_begin(second));
    W *ptr = reserve(second, 1, record_drop);
    assert(ptr); *ptr = 7;
    assert(jet_rt_session_publish(second, 1));
    assert(jet_rt_session_end(second));
    // Thread exit also owns active sessions; no turn frame is still executing.
}
int main(void) {
    jet_rt_thread_attach();
    carries_places();
    validates_places();
    drops_and_moves();
    teardown_order_and_unsigned_ids();
    alignment_and_thread_confinement();
    prepares_tls_teardown();
    jet_rt_thread_detach();
    assert(drops == 3 && order[0] == 6 && order[1] == 5 && order[2] == 7);
    puts("resident session typed places/lifecycle/drop/thread confinement ok");
    return 0;
}
