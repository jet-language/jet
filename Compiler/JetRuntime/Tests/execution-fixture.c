/* Actual UNIT1 + Host-tail native pack exports; no substitute runtime.
 * Run after pack init, as with comptime-fixture.c; link -lpthread.
 */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern bool jet_rt_execution_begin(uint64_t, uint64_t, uint64_t);
extern void jet_rt_execution_end(void);
extern uint64_t jet_rt_execution_stop_write(void);
extern bool jet_rt_comptime_begin(uint64_t, uint64_t);
extern void jet_rt_comptime_step(uint64_t, uint64_t, uint64_t);
extern void jet_rt_comptime_enter(uint64_t, uint64_t, uint64_t);
extern uint32_t jet_rt_comptime_stop_write(void);
extern uint64_t jet_rt_session_status(void);
extern bool jet_rt_control_publish_stop(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_join(int64_t, int64_t, int64_t);
extern void jet_rt_handle_drop_jet_task_failure(int64_t);
extern void jet_stdio_stdout_write_raw(int64_t, bool, int64_t, int64_t);

static uint64_t write_comptime(void) { return jet_rt_comptime_stop_write(); }

/* Return the complete length/newline/body frame, not just decoded JSON. */
static void capture(uint64_t (*emit)(void), uint64_t status, char output[1024]) {
    int channel[2];
    assert(pipe(channel) == 0);
    int input = fcntl(channel[0], F_DUPFD_CLOEXEC, 10);
    assert(input >= 10);
    assert(dup2(channel[1], 3) == 3);
    if (channel[0] != 3) assert(close(channel[0]) == 0);
    if (channel[1] != 3) assert(close(channel[1]) == 0);
    assert(emit() == status);
    assert(close(3) == 0);
    size_t at = 0;
    for (;;) {
        ssize_t count = read(input, output + at, 1023 - at);
        assert(count >= 0);
        if (!count) break;
        at += (size_t)count;
        assert(at < 1023);
    }
    output[at] = '\0';
    assert(close(input) == 0);
}

static void expect_frame(const char *actual, const char *body) {
    char expected[1024];
    int count = snprintf(expected, sizeof expected, "%zu\n%s", strlen(body), body);
    assert(count > 0 && (size_t)count < sizeof expected);
    assert(strcmp(actual, expected) == 0);
}

static void budget_case(unsigned mode, bool depth) {
    /* mode 2 proves ordinary comptime restores site presence after mode 0. */
    if (mode == 2) assert(jet_rt_comptime_begin(0, depth ? 0 : UINT64_MAX));
    else assert(jet_rt_execution_begin(0, depth ? 0 : UINT64_MAX, mode));
    assert(!jet_rt_execution_begin(1, 1, mode == 0 ? 1 : 0));
    char empty[1024], old_frame[1024], new_frame[1024], repeated[1024];
    capture(jet_rt_execution_stop_write, 0, empty);
    assert(empty[0] == '\0');
    if (depth) jet_rt_comptime_enter(UINT64_MAX, UINT64_MAX - 1, UINT64_MAX);
    else jet_rt_comptime_step(UINT64_MAX, UINT64_MAX - 1, UINT64_MAX);
    uint64_t rail = depth ? 4 : 3;
    assert(jet_rt_session_status() == rail);
    capture(write_comptime, depth ? 3 : 2, old_frame);
    capture(jet_rt_execution_stop_write, 1, new_frame);
    capture(jet_rt_execution_stop_write, 1, repeated);
    assert(strcmp(old_frame, new_frame) == 0);
    assert(strcmp(new_frame, repeated) == 0);
    assert(jet_rt_session_status() == rail);
    char body[512];
    int count = snprintf(body, sizeof body,
        "{\"kind\":\"%s\",\"jct_status\":%u,\"owner\":%s,\"start\":%s,\"end\":%s,\"message\":\"\"}",
        depth ? "BudgetDepth" : "BudgetFuel", depth ? 3U : 2U,
        mode ? "18446744073709551615" : "null",
        mode ? "18446744073709551614" : "null",
        mode ? "18446744073709551615" : "null");
    assert(count > 0 && (size_t)count < sizeof body);
    expect_frame(new_frame, body);
    jet_rt_execution_end();
}

/* Scheduler.rs:855-884 old encode goldens, including exact control escapes
 * and unchanged UTF-8 bytes. Comptime's old writer only accepted budgets. */
static const char message[] = "quote\"slash\\lf\ncr\rtab\tctl\001é中";
static const char escaped[] = "quote\\\"slash\\\\lf\\ncr\\rtab\\tctl\\u0001é中";
static void publish(int64_t kind) {
    assert(jet_rt_control_publish_stop(kind, -1, -1, -1, -1,
        (int64_t)(intptr_t)message, (int64_t)strlen(message), 0));
}
static void signal_case(int64_t kind, const char *name) {
    assert(jet_rt_execution_begin(UINT64_MAX, UINT64_MAX, 0));
    publish(kind);
    char frame[1024], repeated[1024], body[512];
    capture(jet_rt_execution_stop_write, 1, frame);
    capture(jet_rt_execution_stop_write, 1, repeated);
    assert(strcmp(frame, repeated) == 0);
    assert(jet_rt_session_status() == (uint64_t)kind);
    int count = snprintf(body, sizeof body,
        "{\"kind\":\"%s\",\"jct_status\":null,\"owner\":null,\"start\":null,\"end\":null,\"message\":\"%s\"}", name, escaped);
    assert(count > 0 && (size_t)count < sizeof body);
    expect_frame(frame, body);
    jet_rt_execution_end();
}

static int64_t panic_body(int64_t env, int64_t out) {
    (void)env;
    (void)out; /* Pending Stop forbids initializing the Unit result. */
    publish(5); /* Real Task.run body admission, not a forged TLS flag. */
    assert(jet_rt_session_status() == 5);
    assert(jet_rt_execution_stop_write() == 1);
    assert(jet_rt_session_status() == 5);
    return 0;
}
static int64_t release_borrowed_env(int64_t env) {
    assert(env == 0); /* No owned environment to release. */
    return 0;
}
static uint64_t panic_emit(void) {
    /* Actual native descriptor: stride, clone, drop, show, equal, compare,
       kind=Unit(11), child0, child1. Task copies it before scheduling. */
    int64_t meta[9] = {0, 0, 0, 0, 0, 0, 11, 0, 0};
    static const char label_bytes[] = "execution panic encoder";
    int64_t label[3] = {(int64_t)(intptr_t)label_bytes, sizeof label_bytes - 1, 0};
    int64_t child = jet_rt_native_task_spawn(0, (int64_t)(intptr_t)label, 0,
        (int64_t)(intptr_t)panic_body, (int64_t)(intptr_t)release_borrowed_env,
        (int64_t)(intptr_t)meta);
    assert(child != 0);
    int64_t failure = 0;
    assert(jet_rt_native_task_join(child, 0, (int64_t)(intptr_t)&failure) == 0);
    assert(failure != 0);
    jet_rt_handle_drop_jet_task_failure(failure);
    return 1;
}
static void panic_case(void) {
    assert(jet_rt_execution_begin(UINT64_MAX, UINT64_MAX, 0));
    char frame[1024], body[512];
    capture(panic_emit, 1, frame);
    int count = snprintf(body, sizeof body,
        "{\"kind\":\"Panic\",\"jct_status\":null,\"owner\":null,\"start\":null,\"end\":null,\"message\":\"%s\"}", escaped);
    assert(count > 0 && (size_t)count < sizeof body);
    expect_frame(frame, body);
    jet_rt_execution_end();
}

static void flush_case(void) {
    int channel[2];
    assert(pipe(channel) == 0);
    int saved = fcntl(STDOUT_FILENO, F_DUPFD_CLOEXEC, 10);
    assert(saved >= 10);
    assert(dup2(channel[1], STDOUT_FILENO) == STDOUT_FILENO);
    assert(close(channel[1]) == 0);
    assert(jet_rt_execution_begin(UINT64_MAX, UINT64_MAX, 0));
    static const char bytes[] = "partial stdout without LF";
    int64_t text[3] = {(int64_t)(intptr_t)bytes, sizeof bytes - 1, 0};
    int64_t error = -1;
    bool write_zero = true;
    jet_stdio_stdout_write_raw((int64_t)(intptr_t)text, false,
        (int64_t)(intptr_t)&error, (int64_t)(intptr_t)&write_zero);
    assert(error == 0 && !write_zero);
    jet_rt_execution_end();
    assert(dup2(saved, STDOUT_FILENO) == STDOUT_FILENO);
    assert(close(saved) == 0);
    char output[sizeof bytes];
    size_t at = 0;
    for (;;) {
        ssize_t count = read(channel[0], output + at, sizeof output - at);
        assert(count >= 0);
        if (!count) break;
        at += (size_t)count;
        assert(at < sizeof output);
    }
    assert(close(channel[0]) == 0);
    assert(at == sizeof bytes - 1 && memcmp(output, bytes, at) == 0);
}

int main(void) {
    jet_rt_thread_attach();
    for (unsigned mode = 0; mode < 3; ++mode) {
        budget_case(mode, false);
        budget_case(mode, true);
    }
    signal_case(1, "Cancelled");
    signal_case(2, "Deadline");
    panic_case();
    flush_case();
    char empty[1024];
    capture(jet_rt_execution_stop_write, 0, empty);
    assert(empty[0] == '\0');
    jet_rt_thread_detach();
    puts("CHECK OK — native execution: budget byte equality, modes, MAX sites, stop goldens, nonconsuming, stream flush");
    return 0;
}
