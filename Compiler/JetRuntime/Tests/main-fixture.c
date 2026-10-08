/* Real runtime kernels; callbacks below are user entries, not provider mocks. */
#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <sched.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int32_t jet_rt_main(void (*)(void)); /* caller reads eax, exactly like Loader */
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(int64_t, int64_t, int64_t);
extern int64_t jet_rt_string_from_static(int64_t, int64_t);
extern void jet_rt_string_drop(int64_t);
extern void jet_stdio_stdout_write_raw(int64_t, _Bool, int64_t, int64_t);
extern void jet_stdio_stderr_write_raw(int64_t, _Bool, int64_t, int64_t);
extern void jet_rt_native_atexit(int64_t);
extern void jet_rt_native_channel(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_rt_native_task_detach(int64_t);
extern int64_t jet_rt_native_receiver_receive(int64_t, int64_t);
extern void jet_rt_native_receiver_drop(int64_t);
extern _Bool jet_rt_observe_has_parked_tasks(void);
static const char *mode;
static int64_t word(const void *p) { return (int64_t)(uintptr_t)p; }
static void text(const char *s, int err) {
    int64_t value = jet_rt_string_from_static(word(s), (int64_t)strlen(s));
    int64_t errno_value = 0, zero = 0;
    if (err) jet_stdio_stderr_write_raw(value, 0, word(&errno_value), word(&zero));
    else jet_stdio_stdout_write_raw(value, 0, word(&errno_value), word(&zero));
    assert(errno_value == 0 && zero == 0);
    jet_rt_string_drop(value);
}
static int64_t exit_body(int64_t header) {
    int64_t *h = (int64_t *)(uintptr_t)header;
    text(h[3] == 1 ? "exit1" : "exit2", 0);
    return 0;
}
static int64_t exit_drop(int64_t header) { jet_rt_free(header, 32, 8); return 0; }
static void register_exit(int n) {
    int64_t raw = jet_rt_alloc(32, 8), *h = (int64_t *)(uintptr_t)raw;
    h[0] = 1; h[1] = word(exit_body); h[2] = word(exit_drop); h[3] = n;
    jet_rt_native_atexit(raw);
}
static int64_t blocked_body(int64_t env, int64_t output) {
    (void)output;
    int64_t slot = 0;
    (void)jet_rt_native_receiver_receive(env, word(&slot));
    return 0;
}
static int64_t blocked_drop(int64_t env) { jet_rt_native_receiver_drop(env); return 0; }
static void entry(void) {
    text("entry", 0); /* No LF: normal return must actually flush the buffer. */
    register_exit(1); register_exit(2);
    if (strcmp(mode, "flush") == 0) text("error", 1);
    if (strcmp(mode, "parked") == 0) {
        int64_t tx = 0, rx = 0;
        jet_rt_native_channel(0, 0, word(&tx), word(&rx));
        /* Hold tx through process exit: the real child waits on a live sender. */
        int64_t meta[9] = {8, 0, 0, 0, 0, 0, 11, 0, 0}; /* Unit result */
        int64_t label = jet_rt_string_from_static(word("main-child"), 10);
        int64_t child = jet_rt_native_task_spawn(23, label, rx, word(blocked_body), word(blocked_drop), word(meta));
        jet_rt_string_drop(label);
        jet_rt_native_task_detach(child);
        /* Observe the actual child wait, never manufacture a registry state. */
        while (!jet_rt_observe_has_parked_tasks()) sched_yield();
        (void)tx;
    }
}
int main(int argc, char **argv, char **envp) {
    assert(argc == 2);
    mode = argv[1];
    assert(strcmp(mode, "normal") == 0 || strcmp(mode, "flush") == 0 || strcmp(mode, "parked") == 0);
    jet_rt_host_arguments(argc, word(argv), word(envp));
    int32_t code = jet_rt_main(entry);
    assert(strcmp(mode, "parked") != 0); /* parked edge must exit, not return */
    assert(code == 0);
    return code;
}
