// The cost of one futex wake on this host: a thread sleeps on a futex word
// for 1 ms, the longest an idle carrier spins before it sleeps, and another
// stores 1 and wakes it. Each trial's cost is the time from just before the
// wake to the moment the woken thread runs. Writes the 99th percentile of
// TRIALS trials, in nanoseconds, to the file its argument names.
#define _GNU_SOURCE
#include <linux/futex.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

#define TRIALS 2000
#define IDLE_NS 1000000L

static atomic_int word;
static atomic_int answered;
static int64_t woke[TRIALS];

static int64_t now_ns(void) {
    struct timespec at;
    clock_gettime(CLOCK_MONOTONIC, &at);
    return (int64_t)at.tv_sec * 1000000000 + at.tv_nsec;
}

static void futex_wait(atomic_int *address, int value) {
    syscall(SYS_futex, address, FUTEX_WAIT_PRIVATE, value, NULL, NULL, 0);
}

static void futex_wake(atomic_int *address) {
    syscall(SYS_futex, address, FUTEX_WAKE_PRIVATE, 1, NULL, NULL, 0);
}

static void *sleeper(void *unused) {
    (void)unused;
    for (int trial = 0; trial < TRIALS; trial++) {
        while (atomic_load(&word) == 0) futex_wait(&word, 0);
        woke[trial] = now_ns();
        atomic_store(&word, 0);
        atomic_store(&answered, trial + 1);
        futex_wake(&answered);
    }
    return NULL;
}

static int ascending(const void *left, const void *right) {
    int64_t a = *(const int64_t *)left;
    int64_t b = *(const int64_t *)right;
    return (a > b) - (a < b);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: wake FILE\n");
        return 2;
    }
    pthread_t thread;
    if (pthread_create(&thread, NULL, sleeper, NULL) != 0) {
        fprintf(stderr, "wake: cannot start the sleeper\n");
        return 1;
    }
    static int64_t cost[TRIALS];
    for (int trial = 0; trial < TRIALS; trial++) {
        struct timespec idle = {0, IDLE_NS};
        nanosleep(&idle, NULL);
        int64_t sent = now_ns();
        atomic_store(&word, 1);
        futex_wake(&word);
        int seen;
        while ((seen = atomic_load(&answered)) != trial + 1) futex_wait(&answered, seen);
        cost[trial] = woke[trial] - sent;
    }
    pthread_join(thread, NULL);
    qsort(cost, TRIALS, sizeof cost[0], ascending);
    FILE *out = fopen(argv[1], "w");
    if (out == NULL) {
        fprintf(stderr, "wake: cannot write %s\n", argv[1]);
        return 1;
    }
    fprintf(out, "%lld\n", (long long)cost[TRIALS * 99 / 100]);
    fclose(out);
    return 0;
}
