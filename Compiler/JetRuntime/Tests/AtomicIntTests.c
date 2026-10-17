// Exact Atomic<Int>: real Jet values/owners/allocator refusal, no mocks.
#include <assert.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdatomic.h>
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_compare(int64_t, int64_t);
extern int64_t jet_rt_int_clone(int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_thread_bootstrap(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_atomic_int_init(int64_t *, int64_t);
extern int64_t jet_rt_atomic_int_load(int64_t *);
extern void jet_rt_atomic_int_store(int64_t *, int64_t);
extern void jet_rt_atomic_int_drop(int64_t *);
extern int64_t jet_rt_atomic_int_add(int64_t *, int64_t);
extern bool jet_rt_atomic_int_compare_exchange(int64_t *, int64_t, int64_t);
extern bool jet_rt_atomic_int_try_add(int64_t *, int64_t, int64_t *);
extern void jet_rt_fault_allocation_arm(int64_t, bool);
extern int64_t jet_rt_fault_allocation_fact(int64_t);
extern void jet_rt_fault_allocation_clear(void);
extern void jet_rt_program_allocator_configure(int64_t, int64_t);
extern void jet_rt_program_allocator_restore(int64_t, int64_t);
extern int64_t jet_rt_program_allocator_fact(int64_t);
static const int64_t base = INT64_C(4611686018427387905);
static void check_owned(int64_t value, int64_t expected) {
    assert(jet_int_owned_to_i64(value) == expected);
    jet_rt_int_drop(value);
}
static void *increment(void *argument) {
    jet_rt_thread_bootstrap();
    int64_t *place = argument;
    for (int i = 0; i < 1000; ++i) {
        int64_t previous = jet_rt_atomic_int_add(place, 1);
        int64_t number = jet_int_owned_to_i64(previous);
        assert(number >= base && number < base + 4000);
        jet_rt_int_drop(previous);
    }
    jet_rt_thread_detach();
    return NULL;
}
int main(void) {
    int64_t place;
    jet_rt_atomic_int_init(&place, jet_int_owned_from_i64(base));
    check_owned(jet_rt_atomic_int_load(&place), base);
    int64_t expected = jet_int_owned_from_i64(base); // distinct node, same number
    assert(expected != place);
    assert(jet_rt_atomic_int_compare_exchange(&place, expected, jet_int_owned_from_i64(base + 1)));
    assert(!jet_rt_atomic_int_compare_exchange(&place, jet_int_owned_from_i64(base), 7));
    check_owned(jet_rt_atomic_int_load(&place), base + 1);
    check_owned(jet_rt_atomic_int_add(&place, 1), base + 1);
    check_owned(jet_rt_atomic_int_load(&place), base + 2);
    jet_rt_atomic_int_store(&place, jet_int_owned_from_i64(INT64_MAX));
    int64_t result[4] = {723, 724, 725, 726};
    int64_t before = place;
    int64_t delta = jet_int_owned_from_i64(INT64_MAX);
    int64_t delta_guard = jet_rt_int_clone(delta);
    _Atomic uint64_t *delta_refs = (_Atomic uint64_t *)(uintptr_t)((uint64_t)delta & ((UINT64_C(1) << 62) - 1));
    assert(atomic_load(delta_refs) == 2);
    // Warm fault state before the policy interval; exactly ONE spilling probe.
    jet_rt_fault_allocation_arm(1, true);
    jet_rt_program_allocator_configure(1, 0);
    assert(!jet_rt_atomic_int_try_add(&place, delta, result));
    assert(result[0] == 723 && result[1] == 48 && place == before);
    assert(jet_rt_fault_allocation_fact(0) == 1 && jet_rt_fault_allocation_fact(1) == 1);
    assert(jet_rt_program_allocator_fact(2) == 0);
    assert(atomic_load(delta_refs) == 1); // consumed delta exactly once
    jet_rt_int_drop(delta_guard);
    jet_rt_program_allocator_restore(0, 0);
    jet_rt_fault_allocation_arm(0, false);
    jet_rt_program_allocator_configure(1, 1); // node48 capacity refusal
    assert(!jet_rt_atomic_int_try_add(&place, 1, result));
    assert(result[1] == 48 && place == before && jet_rt_program_allocator_fact(2) == 0);
    assert(jet_rt_fault_allocation_fact(0) == 1);
    jet_rt_program_allocator_restore(0, 0);
    jet_rt_fault_allocation_arm(0, false);
    jet_rt_program_allocator_configure(1, 48); // admits node48, refuses limbs16
    assert(!jet_rt_atomic_int_try_add(&place, 1, result));
    assert(result[1] == 16 && place == before && jet_rt_program_allocator_fact(2) == 0);
    assert(jet_rt_fault_allocation_fact(0) == 1); // no second probe on limbs
    jet_rt_program_allocator_restore(0, 0);
    jet_rt_fault_allocation_clear();
    assert(jet_rt_atomic_int_try_add(&place, 1, result));
    check_owned(result[0], INT64_MAX);
    // Observe an exact value >I64.MAX before recovering the old value.
    int64_t greater = jet_rt_atomic_int_load(&place);
    int64_t maximum = jet_int_owned_from_i64(INT64_MAX);
    assert(jet_int_compare(greater, maximum) == 1);
    jet_rt_int_drop(greater);
    jet_rt_int_drop(maximum);
    int64_t beyond = jet_rt_atomic_int_add(&place, -1);
    jet_rt_int_drop(beyond);
    check_owned(jet_rt_atomic_int_load(&place), INT64_MAX);
    jet_rt_atomic_int_store(&place, 5);
    jet_rt_fault_allocation_arm(1, true);
    assert(jet_rt_atomic_int_try_add(&place, 2, result));
    assert(result[0] == 5 && place == 7 && jet_rt_fault_allocation_fact(0) == 0);
    jet_rt_fault_allocation_clear();
    jet_rt_atomic_int_store(&place, jet_int_owned_from_i64(base));
    pthread_t threads[4];
    for (int i = 0; i < 4; ++i) assert(!pthread_create(&threads[i], NULL, increment, &place));
    for (int i = 0; i < 4; ++i) assert(!pthread_join(threads[i], NULL));
    check_owned(jet_rt_atomic_int_load(&place), base + 4000);
    jet_rt_atomic_int_drop(&place);
    assert(place == 0);
    puts("Atomic Int: exact owners, numeric CAS, refusal cleanup, four writers");
    return 0;
}
