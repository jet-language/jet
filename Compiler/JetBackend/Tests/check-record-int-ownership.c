// Counting rail for the actual compiled native record/Int ownership fixture.
// Every operation delegates to the real Jet runtime; no value body is mocked.
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

extern void *__real_jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern int64_t __real_jet_rt_int_clone(int64_t);
extern void __real_jet_rt_int_drop(int64_t);

static struct { void *pointer; int live; } allocations[512];
static int active, target_set, clones, drops;
static int64_t target;

void jet_test_record_int_count_begin(void) {
    for (size_t i = 0; i < 512; ++i) allocations[i].live = 0;
    target_set = clones = drops = 0;
    active = 1;
}
void jet_test_record_int_count_node(int64_t value) {
    target = value;
    if (value >= -INT64_C(4611686018427387904) && value <= INT64_C(4611686018427387903)) {
        fputs("record Int witness requires a boxed node\n", stderr);
        abort();
    }
    target_set = 1;
}
void *__wrap_jet_rt_alloc(int64_t size, int64_t align) {
    void *pointer = __real_jet_rt_alloc(size, align);
    if (active) {
        for (size_t i = 0; i < 512; ++i) {
            if (!allocations[i].live) {
                allocations[i].pointer = pointer;
                allocations[i].live = 1;
                return pointer;
            }
        }
        fputs("record Int counting rail exhausted\n", stderr);
        abort();
    }
    return pointer;
}
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    if (active) {
        for (size_t i = 0; i < 512; ++i) {
            if (allocations[i].live && allocations[i].pointer == pointer) {
                allocations[i].live = 0;
                break;
            }
        }
    }
    __real_jet_rt_free(pointer, size, align);
}
int64_t __wrap_jet_rt_int_clone(int64_t value) {
    if (active && target_set && value == target) ++clones;
    return __real_jet_rt_int_clone(value);
}
void __wrap_jet_rt_int_drop(int64_t value) {
    if (active && target_set && value == target) ++drops;
    __real_jet_rt_int_drop(value);
}
void jet_test_record_int_count_end(void) {
    size_t live = 0;
    for (size_t i = 0; i < 512; ++i) live += allocations[i].live != 0;
    active = 0;
    if (!target_set || clones != 2 || drops != 3 || live != 0) {
        fprintf(stderr, "record Int ownership: clones=%d drops=%d leaked allocations=%zu\n", clones, drops, live);
        abort();
    }
}
