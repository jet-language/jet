// Link real Jet Atomic.jet definitions; the cell is inline8, never a handle.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>

#define DECLARE(kind) \
    extern void jet_rt_atomic_##kind##_init(int64_t *, int64_t); \
    extern int64_t jet_rt_atomic_##kind##_load(int64_t *); \
    extern void jet_rt_atomic_##kind##_store(int64_t *, int64_t); \
    extern bool jet_rt_atomic_##kind##_compare_exchange(int64_t *, int64_t, int64_t)
DECLARE(bool);
DECLARE(i32);
DECLARE(u32);
DECLARE(i64);
DECLARE(u64);
#define DECLARE_ADD(kind) extern int64_t jet_rt_atomic_##kind##_add(int64_t *, int64_t)
DECLARE_ADD(i32);
DECLARE_ADD(u32);
DECLARE_ADD(i64);
DECLARE_ADD(u64);

typedef struct {
    void (*init)(int64_t *, int64_t);
    int64_t (*load)(int64_t *);
    void (*store)(int64_t *, int64_t);
    int64_t (*add)(int64_t *, int64_t);
    bool (*cas)(int64_t *, int64_t, int64_t);
} Operations;
#define OPS(kind) {jet_rt_atomic_##kind##_init, jet_rt_atomic_##kind##_load, jet_rt_atomic_##kind##_store, jet_rt_atomic_##kind##_add, jet_rt_atomic_##kind##_compare_exchange}
static const Operations operations[] = {OPS(i32), OPS(u32), OPS(i64), OPS(u64)};

typedef struct { const Operations *ops; int64_t *cell; bool compare; } Writer;
static void *increment(void *argument) {
    Writer *writer = argument;
    for (int i = 0; i < 25000; ++i) {
        if (writer->compare) {
            int64_t old;
            do { old = writer->ops->load(writer->cell); }
            while (!writer->ops->cas(writer->cell, old, old + 1));
        } else {
            int64_t old = writer->ops->add(writer->cell, 1);
            assert(old >= 0 && old < 100000);
        }
    }
    return NULL;
}
static void concurrent(const Operations *ops, bool compare) {
    int64_t cell = 0;
    ops->init(&cell, 0);
    Writer writer = {ops, &cell, compare};
    pthread_t threads[4];
    for (int i = 0; i < 4; ++i) assert(!pthread_create(&threads[i], NULL, increment, &writer));
    for (int i = 0; i < 4; ++i) assert(!pthread_join(threads[i], NULL));
    assert(ops->load(&cell) == 100000);
}

static void barrier(pthread_barrier_t *value) {
    int result = pthread_barrier_wait(value);
    assert(result == 0 || result == PTHREAD_BARRIER_SERIAL_THREAD);
}
typedef struct {
    pthread_barrier_t *start, *finish;
    int64_t *mine, *other, *observed;
} StoreBuffering;
static void *store_then_load(void *argument) {
    StoreBuffering *test = argument;
    for (int i = 0; i < 20000; ++i) {
        barrier(test->start);
        jet_rt_atomic_bool_store(test->mine, 1);
        *test->observed = jet_rt_atomic_bool_load(test->other);
        barrier(test->finish);
    }
    return NULL;
}
static void seq_cst_store_buffering(void) {
    int64_t left = 0, right = 0, observed[2] = {0, 0};
    pthread_barrier_t start, finish;
    assert(!pthread_barrier_init(&start, NULL, 3));
    assert(!pthread_barrier_init(&finish, NULL, 3));
    StoreBuffering tests[2] = {{&start, &finish, &left, &right, &observed[0]}, {&start, &finish, &right, &left, &observed[1]}};
    pthread_t threads[2];
    for (int i = 0; i < 2; ++i) assert(!pthread_create(&threads[i], NULL, store_then_load, &tests[i]));
    for (int i = 0; i < 20000; ++i) {
        jet_rt_atomic_bool_store(&left, 0);
        jet_rt_atomic_bool_store(&right, 0);
        barrier(&start);
        barrier(&finish);
        assert(observed[0] == 1 || observed[1] == 1); // SC forbids both zero.
    }
    for (int i = 0; i < 2; ++i) assert(!pthread_join(threads[i], NULL));
    assert(!pthread_barrier_destroy(&start));
    assert(!pthread_barrier_destroy(&finish));
}

int main(void) {
    _Static_assert(sizeof(int64_t) == 8 && _Alignof(int64_t) >= 8, "inline AtomicU64 carrier");
    int64_t cell;
    jet_rt_atomic_bool_init(&cell, 0);
    assert(jet_rt_atomic_bool_load(&cell) == 0);
    assert(jet_rt_atomic_bool_compare_exchange(&cell, 0, 1));
    assert(!jet_rt_atomic_bool_compare_exchange(&cell, 0, 0));
    assert(jet_rt_atomic_bool_load(&cell) == 1);
    jet_rt_atomic_bool_store(&cell, 0);
    assert(cell == 0);
    jet_rt_atomic_i32_init(&cell, -1);
    assert(cell == UINT32_MAX && jet_rt_atomic_i32_load(&cell) == -1);
    assert(jet_rt_atomic_i32_add(&cell, 1) == -1 && cell == 0);
    assert(jet_rt_atomic_i32_compare_exchange(&cell, 0, INT32_MAX));
    assert(jet_rt_atomic_i32_add(&cell, 1) == INT32_MAX && jet_rt_atomic_i32_load(&cell) == INT32_MIN);
    assert(cell == UINT64_C(0x80000000));
    assert(jet_rt_atomic_i32_compare_exchange(&cell, INT32_MIN, -7));
    jet_rt_atomic_i32_store(&cell, -2);
    assert(jet_rt_atomic_i32_load(&cell) == -2 && cell == UINT64_C(0xfffffffe));
    jet_rt_atomic_u32_init(&cell, UINT32_MAX);
    assert(jet_rt_atomic_u32_add(&cell, 1) == UINT32_MAX && cell == 0);
    assert(jet_rt_atomic_u32_compare_exchange(&cell, 0, UINT32_MAX)); // wrap then CAS
    assert(!jet_rt_atomic_u32_compare_exchange(&cell, 0, 1));
    jet_rt_atomic_u32_store(&cell, UINT64_C(0x180000001));
    assert(jet_rt_atomic_u32_load(&cell) == UINT64_C(0x80000001) && cell == UINT64_C(0x80000001));
    jet_rt_atomic_i64_init(&cell, INT64_MAX);
    assert(jet_rt_atomic_i64_add(&cell, 1) == INT64_MAX && jet_rt_atomic_i64_load(&cell) == INT64_MIN);
    assert(jet_rt_atomic_i64_compare_exchange(&cell, INT64_MIN, -7));
    assert(!jet_rt_atomic_i64_compare_exchange(&cell, INT64_MIN, 1));
    jet_rt_atomic_i64_store(&cell, -9);
    assert(jet_rt_atomic_i64_load(&cell) == -9);
    jet_rt_atomic_u64_init(&cell, -1);
    assert(jet_rt_atomic_u64_add(&cell, 1) == -1 && jet_rt_atomic_u64_load(&cell) == 0);
    assert(jet_rt_atomic_u64_compare_exchange(&cell, 0, INT64_MIN));
    assert(!jet_rt_atomic_u64_compare_exchange(&cell, 0, 1));
    jet_rt_atomic_u64_store(&cell, -2);
    assert(jet_rt_atomic_u64_load(&cell) == -2);
    for (unsigned i = 0; i < sizeof operations / sizeof operations[0]; ++i) {
        concurrent(&operations[i], false);
        concurrent(&operations[i], true);
    }
    seq_cst_store_buffering();
    puts("Atomic word: five kinds, wrap/CAS, four writers, SeqCst store-buffering");
    return 0;
}
