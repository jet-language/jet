// #6689 c1, C-linking shape: the carriers come from the C closure's libc
// (musl pthread_create), so each carrier is a libc thread with its own
// pthread_self. On a raw clone every thread would read the main thread's.
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdint.h>
#include <time.h>

static atomic_int arrived;

static int64_t now_ns(void) {
    struct timespec at;
    clock_gettime(CLOCK_MONOTONIC, &at);
    return (int64_t)at.tv_sec * 1000000000 + at.tv_nsec;
}

// Holds this carrier until `parties` callers are inside at once (or 10 s
// pass), so each caller is on its own carrier; its pthread_self.
int64_t carrier_meet(int64_t parties) {
    atomic_fetch_add(&arrived, 1);
    int64_t deadline = now_ns() + 10000000000;
    while (atomic_load(&arrived) < parties && now_ns() < deadline) sched_yield();
    return (int64_t)(uintptr_t)pthread_self();
}

int64_t carrier_met(void) { return atomic_load(&arrived); }

int64_t carrier_self(void) { return (int64_t)(uintptr_t)pthread_self(); }
