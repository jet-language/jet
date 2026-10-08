/* Link against the composed native runtime object/library, with -lpthread.
 * Actual exported kernels; no replacement runtime, callbacks, or budget model.
 */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern bool jet_rt_comptime_begin(uint64_t, uint64_t);
extern void jet_rt_comptime_step(uint64_t, uint64_t, uint64_t);
extern void jet_rt_comptime_enter(uint64_t, uint64_t, uint64_t);
extern void jet_rt_comptime_leave(void);
extern void jet_rt_comptime_work(uint64_t);
extern void jet_rt_comptime_end(void);
extern uint32_t jet_rt_comptime_stop_write(void);
extern int64_t jet_rt_control_new(void);
extern void jet_rt_control_enter(int64_t, int64_t);
extern void jet_rt_control_leave(int64_t);
extern void jet_rt_scheduler_control_drop(int64_t);
extern int64_t jet_rt_control_take_stop(int64_t);
extern int64_t jet_rt_stop_kind(int64_t);
extern int64_t jet_rt_stop_owner(int64_t);
extern int64_t jet_rt_stop_start(int64_t);
extern int64_t jet_rt_stop_end(int64_t);
extern void jet_rt_stop_drop(int64_t);

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t ready = PTHREAD_COND_INITIALIZER;
static bool first_done;
static bool depth_case;

static void *worker(void *argument) {
    const uintptr_t index = (uintptr_t)argument;
    jet_rt_thread_attach();
    int64_t saved[4] = {0};
    int64_t control = jet_rt_control_new();
    jet_rt_control_enter(control, (int64_t)(intptr_t)saved);
    if (index == 0) {
        /* U64.MAX is real present site metadata, not absent sentinel -1. */
        if (depth_case) jet_rt_comptime_enter(UINT64_MAX, UINT64_MAX - 1, UINT64_MAX);
        else jet_rt_comptime_step(UINT64_MAX, UINT64_MAX - 1, UINT64_MAX);
        assert(pthread_mutex_lock(&lock) == 0);
        first_done = true;
        assert(pthread_cond_signal(&ready) == 0);
        assert(pthread_mutex_unlock(&lock) == 0);
    } else {
        assert(pthread_mutex_lock(&lock) == 0);
        while (!first_done) assert(pthread_cond_wait(&ready, &lock) == 0);
        assert(pthread_mutex_unlock(&lock) == 0);
        /* Both enter's stopped path and step's spent-fuel path retain FIRST. */
        if (depth_case) jet_rt_comptime_step(22, 33, 44);
        else jet_rt_comptime_enter(22, 33, 44);
    }
    int64_t stop = jet_rt_control_take_stop(control);
    assert(stop != 0);
    assert(jet_rt_stop_kind(stop) == (depth_case ? 4 : 3));
    assert((uint64_t)jet_rt_stop_owner(stop) == UINT64_MAX);
    assert((uint64_t)jet_rt_stop_start(stop) == UINT64_MAX - 1);
    assert((uint64_t)jet_rt_stop_end(stop) == UINT64_MAX);
    jet_rt_stop_drop(stop);
    jet_rt_comptime_leave();
    jet_rt_control_leave((int64_t)(intptr_t)saved);
    jet_rt_scheduler_control_drop(control);
    jet_rt_thread_detach();
    return NULL;
}

static void check_frame(uint32_t expected) {
    int channel[2];
    assert(pipe(channel) == 0);
    int input = fcntl(channel[0], F_DUPFD_CLOEXEC, 10);
    assert(input >= 10);
    assert(dup2(channel[1], 3) == 3);
    if (channel[0] != 3) assert(close(channel[0]) == 0);
    if (channel[1] != 3) assert(close(channel[1]) == 0);
    assert(jet_rt_comptime_stop_write() == expected);
    assert(close(3) == 0);
    FILE *stream = fdopen(input, "r");
    assert(stream != NULL);
    char line[32];
    assert(fgets(line, sizeof line, stream) != NULL);
    size_t length = (size_t)strtoul(line, NULL, 10);
    char body[192] = {0};
    assert(length < sizeof body);
    assert(fread(body, 1, length, stream) == length);
    char wanted[192];
    int count = snprintf(wanted, sizeof wanted,
        "{\"kind\":\"%s\",\"jct_status\":%u,\"owner\":18446744073709551615,"
        "\"start\":18446744073709551614,\"end\":18446744073709551615,\"message\":\"\"}",
        expected == 2 ? "BudgetFuel" : "BudgetDepth", expected);
    assert(count >= 0 && (size_t)count == length);
    assert(strcmp(body, wanted) == 0);
    assert(fgetc(stream) == EOF);
    assert(fclose(stream) == 0);
}

int main(void) {
    jet_rt_thread_attach();
    /* Unsigned full range: MAX fuel minus MAX succeeds with zero left;
       a zero charge must not manufacture a stop. */
    assert(jet_rt_comptime_begin(UINT64_MAX, UINT64_MAX));
    assert(!jet_rt_comptime_begin(1, 1));
    jet_rt_comptime_work(UINT64_MAX);
    jet_rt_comptime_work(0);
    assert(jet_rt_comptime_stop_write() == 0);
    jet_rt_comptime_end();
    for (unsigned mode = 0; mode < 2; ++mode) {
        depth_case = mode != 0;
        first_done = false;
        assert(jet_rt_comptime_begin(0, depth_case ? 0 : UINT64_MAX));
        pthread_t first, second;
        assert(pthread_create(&first, NULL, worker, (void *)(uintptr_t)0) == 0);
        assert(pthread_create(&second, NULL, worker, (void *)(uintptr_t)1) == 0);
        assert(pthread_join(first, NULL) == 0);
        assert(pthread_join(second, NULL) == 0);
        jet_rt_comptime_end();
        check_frame(depth_case ? 3 : 2);
    }
    jet_rt_thread_detach();
    puts("CHECK OK — native comptime: unsigned fuel, two-thread first-stop, full-range site, framed channel");
    return 0;
}
