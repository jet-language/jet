// Test-host transport only: actual compiled entry, real task spawn/join and
// real capability exports. No replacement Poll, scheduler, TLS, or trap leaf.
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdatomic.h>
extern int __real_jet_rt_main(void (*)(void));
extern int64_t jet_rt_poll_call_enter(void);
extern void jet_rt_poll_call_leave(int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_join(int64_t, int64_t, int64_t);
extern int64_t jet_rt_string_from_static(const char *, int64_t);
extern void jet_rt_string_drop(int64_t);
extern int64_t jet_rt_record_tag_jet_task_failure(int64_t);
extern void jet_rt_handle_drop_jet_task_failure(int64_t);
extern void jet_rt_stdout_flush(void);
#ifdef JET_FX_TASK_MESSAGE
extern int64_t jet_rt_record_get_jet_task_failure_panicked(int64_t);
#endif
#ifdef JET_FX_CANCEL_DURING_CLEANUP
extern int64_t __real_jet_rt_cleanup_enter(void);
extern void jet_rt_native_task_cancel(int64_t);
static _Atomic int64_t current_child;
static _Atomic int cancel_requested;
int64_t __wrap_jet_rt_cleanup_enter(void) {
    int64_t saved = __real_jet_rt_cleanup_enter();
    if (!atomic_exchange(&cancel_requested, 1)) jet_rt_native_task_cancel(atomic_load(&current_child));
    return saved;
}
#endif
static int64_t invoke(int64_t env, int64_t out) {
    // Task.jet lends body bit1. Nested capabilities must restore bit2 and
    // finally return to bit1, without lending any capability to the callback.
    int64_t outer = jet_rt_poll_call_enter();
    int64_t inner = jet_rt_poll_call_enter();
    jet_rt_poll_call_leave(inner);
    int64_t retained = jet_rt_poll_call_enter();
    jet_rt_poll_call_leave(retained);
    jet_rt_poll_call_leave(outer);
    int64_t restored = jet_rt_poll_call_enter();
    jet_rt_poll_call_leave(restored);
    if (outer != 1 || inner != 3 || retained != 3 || restored != 1) return 0;
#ifdef JET_FX_CANCEL_DURING_CLEANUP
    while (!atomic_load(&current_child)) {}
#endif
    ((void (*)(void))(uintptr_t)env)();
    *(int64_t *)(uintptr_t)out = 0;
    return 0;
}
static int64_t release_environment(int64_t env) { (void)env; return 0; }
static int64_t clone_word(int64_t source, int64_t target) {
    *(int64_t *)(uintptr_t)target = *(int64_t *)(uintptr_t)source;
    return 0;
}
int __wrap_jet_rt_main(void (*entry)(void)) {
    // Root never acquires bit2, including nested enter/leave.
    int64_t outer = jet_rt_poll_call_enter(), inner = jet_rt_poll_call_enter();
    jet_rt_poll_call_leave(inner);
    jet_rt_poll_call_leave(outer);
    if (outer != 0 || inner != 0) return 1;
    int64_t meta[9] = {8, (int64_t)(uintptr_t)clone_word, 0, 0, 0, 0, INT64_C(274877906946), 0, 0};
    int64_t label = jet_rt_string_from_static("native capability", 17), outcome = 0;
    int64_t task = jet_rt_native_task_spawn(17, label, (int64_t)(uintptr_t)entry,
        (int64_t)(uintptr_t)invoke, (int64_t)(uintptr_t)release_environment, (int64_t)(uintptr_t)meta);
#ifdef JET_FX_CANCEL_DURING_CLEANUP
    atomic_store(&current_child, task);
#endif
    jet_rt_string_drop(label);
    int64_t success = jet_rt_native_task_join(task, 0, (int64_t)(uintptr_t)&outcome);
#ifdef JET_FX_TASK_PASS
    if (!success || outcome != 0) return 1;
#else
#ifdef JET_FX_CANCEL_DURING_CLEANUP
    if (success || jet_rt_record_tag_jet_task_failure(outcome) != 0) return 1;
#else
    if (success || jet_rt_record_tag_jet_task_failure(outcome) != 2) return 1;
#endif
#ifdef JET_FX_TASK_MESSAGE
    int64_t reason = jet_rt_record_get_jet_task_failure_panicked(outcome);
    const int64_t *text = (const int64_t *)(uintptr_t)reason;
    const char *bytes = (const char *)(uintptr_t)text[0];
    size_t length = (size_t)text[1], needle_length = strlen(JET_FX_TASK_MESSAGE);
    int found = 0;
    for (size_t at = 0; at + needle_length <= length; ++at)
        if (!memcmp(bytes + at, JET_FX_TASK_MESSAGE, needle_length)) { found = 1; break; }
    jet_rt_string_drop(reason);
    if (!found) return 1;
#endif
    jet_rt_handle_drop_jet_task_failure(outcome);
#endif
    jet_rt_stdout_flush();
#ifdef JET_FX_TASK_PASS
    puts("expectation task passed");
#elif defined(JET_FX_CANCEL_DURING_CLEANUP)
    puts("task cancelled; process continued");
#else
    puts("task published; process continued");
#endif
    return 0;
}

#ifdef JET_FX_LIBRARY_ENTRY
extern void run(void);
#ifdef JET_FX_PLAIN_ROOT
extern int jet_rt_main(void (*)(void));
int main(void) { return jet_rt_main(run); }
#else
int main(void) { return __wrap_jet_rt_main(run); }
#endif
#endif
