/* Real Root/StopCleanup providers; C supplies only callbacks and assertions.
 * This is an ABI/lifetime witness, not evidence of MIR lexical Close lowering.
 * White-box corruption cases are explicitly named and test host-defect gates. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <stdatomic.h>
#include <errno.h>
#include <stddef.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
#define WORD(p) ((int64_t)(uintptr_t)(p))
extern int64_t jet_rt_root_begin(int64_t);
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern void jet_rt_control_retain(int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, Text *, int64_t, int64_t, int64_t, int64_t);
extern void jet_rt_native_channel(int64_t, int64_t, int64_t, int64_t);
extern void jet_rt_native_sender_send(int64_t, int64_t, int64_t);
extern void jet_rt_native_sender_drop(int64_t);
extern void jet_rt_native_receiver_drop(int64_t);
extern int64_t jet_rt_native_task_join(int64_t, int64_t, int64_t);
extern void jet_rt_handle_drop_jet_task_failure(int64_t);
extern int64_t jet_rt_record_new_io_context(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_get_io_error_not_found(int64_t);
extern int64_t jet_rt_record_get_io_error_resource_limit(int64_t);
extern int64_t jet_rt_record_new_io_error_resource_limit(int64_t);
extern void jet_rt_thread_detach(void);
extern void jet_rt_thread_attach(void);
extern void jet_rt_unreachable(int64_t, int64_t);
extern void jet_stdio_stdout_write_raw(int64_t, bool, int64_t, int64_t);
extern int64_t jet_rt_stop_domain(void);
extern bool jet_rt_root_publish_rendered(int64_t, int64_t, int64_t);
extern int64_t jet_rt_root_take_stop(int64_t, int64_t);
extern void jet_rt_root_leave(int64_t, int64_t);
extern int64_t jet_rt_session_cleanup_enter(int64_t);
extern void jet_rt_session_cleanup_leave(int64_t);
extern int64_t jet_rt_session_status(void);
extern int64_t jet_rt_session_poll(void);
extern int64_t jet_rt_session_propagate(void);
extern void jet_rt_session_shield_enter(void);
extern void jet_rt_session_shield_leave(void);
extern int64_t jet_rt_control_new(void);
extern void jet_rt_control_enter(int64_t, int64_t);
extern void jet_rt_control_leave(int64_t);
extern void jet_rt_control_interrupt(int64_t);
extern void jet_rt_scheduler_control_drop(int64_t);
extern bool jet_rt_control_publish_stop(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_control_take_stop(int64_t);
extern int64_t jet_rt_stop_kind(int64_t);
extern int64_t jet_rt_stop_jct_status(int64_t);
extern int64_t jet_rt_stop_owner(int64_t);
extern int64_t jet_rt_stop_start(int64_t);
extern int64_t jet_rt_stop_end(int64_t);
extern int64_t jet_rt_stop_form(int64_t);
extern Text *jet_rt_stop_take_message(int64_t);
extern void jet_rt_stop_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_panic_message(int64_t, int64_t);
extern int64_t jet_loop_range_init(int64_t, int64_t, int64_t, uint8_t, uint8_t);
extern int64_t jet_loop_range_value(int64_t);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(int64_t, int64_t, int64_t);
extern void jet_rt_native_atexit(int64_t);
extern void jet_rt_task_exit_handlers(void);
extern void jet_ctx_push_deadline(int64_t, int64_t);
extern void jet_ctx_pop_deadline(int64_t);
extern void jet_task_sleep_ms_defaulted(int64_t);

static const unsigned char report[] = "Error [E3001]: fixture\nMore: jet-lang.dev/e/E3001";
static void take(int64_t root, int64_t expected_code, const unsigned char *bytes, size_t length) {
    int64_t code = INT64_MIN;
    int64_t record = jet_rt_root_take_stop(root, WORD(&code));
    assert(record && code == expected_code);
    assert(jet_rt_stop_form(record) == 1);
    Text *text = jet_rt_stop_take_message(record);
    jet_rt_stop_drop(record);
    if (!length) { assert(!text || text->length == 0); }
    else {
        assert(text && text->length == (int64_t)length);
        assert(!memcmp(text->data, bytes, length));
    }
    jet_rt_string_drop(text);
    assert(jet_rt_session_status() == 0);
}

/* Closure ABI: refs/code/release, followed by one owned test capture. */
typedef struct { int64_t refs, code, release, id; } Closure;
static int events[32], event_count;
static void release(int64_t word) {
    Closure *closure = (Closure *)(uintptr_t)word;
    events[event_count++] = 10 + (int)closure->id;
    jet_rt_free(word, sizeof *closure, 8);
}
static void callback(int64_t word);
static Closure *closure(int id) {
    Closure *value = (Closure *)(uintptr_t)jet_rt_alloc(sizeof *value, 8);
    *value = (Closure){1, WORD(callback), WORD(release), id};
    return value;
}
static void callback(int64_t word) {
    Closure *value = (Closure *)(uintptr_t)word;
    events[event_count++] = (int)value->id;
    if (value->id == 1) jet_rt_native_atexit(WORD(closure(3))); /* owed next batch */
    if (value->id == 2) {
        assert(jet_rt_root_publish_rendered(WORD(report), sizeof report - 1, 73));
        jet_rt_native_atexit(WORD(closure(4))); /* rejected; capture is still dropped */
        if (jet_rt_session_propagate()) return; /* C's explicit stop edge */
        abort();
    }
    if (value->id == 3) {
        assert(jet_rt_session_status() == 5);
        assert(jet_rt_session_propagate() == 0); /* actual owed cleanup masks old5 */
    }
}

static int64_t cancelled_record(void) {
    int64_t save[4], control = jet_rt_control_new();
    jet_rt_control_enter(control, WORD(save));
    assert(jet_rt_stop_domain() == 0);
    assert(jet_rt_control_publish_stop(1, -1, -1, -1, -1, 0, 0, 0));
    int64_t record = jet_rt_control_take_stop(control);
    jet_rt_control_leave(WORD(save));
    jet_rt_scheduler_control_drop(control);
    return record;
}

/* The real scheduler mints origin16 while the actual Task body is active;
 * its capture release runs after body_leave with that owned cleanup token. */
static atomic_bool task_released;
static int64_t task_body(int64_t environment, int64_t output) {
    (void)environment;
    (void)output;
    assert(jet_rt_stop_domain() == 2);
    int64_t saved = jet_rt_session_cleanup_enter(0);
    assert(saved == 2);
    jet_rt_session_cleanup_leave(saved);
    jet_rt_panic_message(WORD("task-origin"), 11);
    assert(jet_rt_session_status() == 5 && jet_rt_session_propagate() == 5);
    return 0; /* explicit C stop edge; no result slot was initialized */
}
static void task_release(int64_t environment) {
    (void)environment;
    assert(jet_rt_stop_domain() == 2);
    assert(jet_rt_session_status() == 5 && jet_rt_session_propagate() == 0);
    atomic_store_explicit(&task_released, true, memory_order_release);
}
static void task_clone(int64_t source, int64_t target) {
    *(int64_t *)(uintptr_t)target = *(int64_t *)(uintptr_t)source;
}
static int slot_drops;
static void slot_drop(int64_t slot) {
    assert(*(int64_t *)(uintptr_t)slot == 42);
    assert(jet_rt_stop_domain() == 1 && jet_rt_session_status() == 5);
    assert(jet_rt_session_propagate() == 0);
    int64_t saved = jet_rt_session_cleanup_enter(5);
    assert((saved & 4) != 0 && ((uint64_t)saved >> 8) == 5);
    assert((saved & 16) == 0); /* Root cleanup is not constructor-static Task origin */
    jet_rt_session_cleanup_leave(saved);
    slot_drops++;
}
static void central_drop(int64_t root) {
    /* Canonical NativeMeta is 72 bytes: seven scalar/callback words and two
     * recursively owned child descriptors. Both child pointers are absent. */
    int64_t metadata[9] = {8, WORD(task_clone), WORD(slot_drop), 0, 0, 0, INT64_C(274877906946), 0, 0};
    _Static_assert(sizeof metadata == 72, "canonical NativeMeta width");
    int64_t sender = 0, receiver = 0, value = 42;
    jet_rt_native_channel(0, 0, WORD(&sender), WORD(&receiver));
    assert(sender && receiver && sender == receiver);
    jet_rt_native_sender_send(sender, WORD(&value), WORD(metadata));
    assert(slot_drops == 0 && jet_rt_session_status() == 0);
    assert(jet_rt_root_publish_rendered(WORD(report), sizeof report - 1, 73));
    jet_rt_native_sender_drop(sender);
    jet_rt_native_receiver_drop(receiver); /* real queued-slot destructor path */
    assert(slot_drops == 1 && jet_rt_session_status() == 5);
    assert(jet_rt_session_propagate() == 5); /* exact flags restored after callback */
    take(root, 73, report, sizeof report - 1);
}

static void task_origin(void) {
    /* Actual metadata and Task control layout, identical to the task witness:
     * payload I64 / clone callback / canonical Int tag; child.control at8. */
    int64_t metadata[9] = {8, WORD(task_clone), 0, 0, 0, 0, INT64_C(274877906946), 0, 0};
    Text label = {NULL, 0, 0};
    atomic_init(&task_released, false);
    int64_t task = jet_rt_native_task_spawn(17, &label, 0, WORD(task_body), WORD(task_release), WORD(metadata));
    int64_t control = ((int64_t *)(uintptr_t)task)[1];
    jet_rt_control_retain(control);
    int64_t result[4] = {0};
    assert(jet_rt_native_task_join(task, 0, WORD(result)) == 0);
    assert(atomic_load_explicit(&task_released, memory_order_acquire));
    jet_rt_handle_drop_jet_task_failure(result[0]);
    /* Keeping the real control alive does not make this caller a Task frame. */
    int64_t save[4];
    jet_rt_control_enter(control, WORD(save));
    assert(jet_rt_stop_domain() == 0);
    int64_t saved = jet_rt_session_cleanup_enter(0);
    assert(saved == 2 && jet_rt_stop_domain() == 0); /* constructor TASK2 is not origin */
    jet_rt_session_cleanup_leave(saved);
    jet_rt_control_leave(WORD(save));
    jet_rt_scheduler_control_drop(control);
    assert(jet_rt_stop_domain() == 1 && jet_rt_session_status() == 0);
}
static void deny_gs_operation(uint32_t operation) {
    /* Real kernel refusal, not a fake runtime/TLS provider. Other syscalls
     * remain available so the cold failure can execute its direct exit101. */
    struct sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, SYS_arch_prctl, 0, 3),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, args[0])),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, operation, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    struct sock_fprog program = {(unsigned short)(sizeof filter / sizeof *filter), filter};
    assert(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0);
    assert(prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program) == 0);
}

int main(int argc, char **argv, char **envp) {
    assert(argc == 2);
    jet_rt_host_arguments(argc, WORD(argv), WORD(envp));
    const char *mode = argv[1];
    assert(jet_rt_stop_domain() == 0);
    int64_t zero = jet_rt_session_cleanup_enter(0);
    assert(zero == 0);
    jet_rt_session_cleanup_leave(zero); /* owned zero token is restored */
    if (!strcmp(mode, "bare-cancel")) {
        int64_t record = cancelled_record();
        assert(jet_rt_stop_kind(record) == 1);
        jet_rt_stop_drop(record);
        puts("ROOT CLEANUP OK");
        return 0;
    }
    if (!strcmp(mode, "cold-gs0-unreachable") || !strcmp(mode, "cold-gs0-layout") ||
        !strcmp(mode, "cold-probe-error") || !strcmp(mode, "cold-set-error")) {
        Text text = {(unsigned char *)"cold-buffered", 13, 0};
        int64_t error = INT64_MIN;
        unsigned char write_zero = 1;
        jet_stdio_stdout_write_raw(WORD(&text), false, WORD(&error), WORD(&write_zero));
        assert(error == 0 && write_zero == 0);
        if (!strcmp(mode, "cold-probe-error")) {
            deny_gs_operation(4100);
            jet_rt_unreachable(0, 0);
        } else if (!strcmp(mode, "cold-set-error")) {
            deny_gs_operation(4097);
            jet_rt_thread_attach(); /* valid repeated attach; its first clear is genuinely refused */
        } else {
            jet_rt_thread_detach(); /* actual provider clears GS before releasing the TCB */
            if (!strcmp(mode, "cold-gs0-layout")) (void)jet_rt_alloc(-1, 8);
            else jet_rt_unreachable(0, 0);
        }
        abort(); /* expected direct70/101; never a GS:0 load or fabricated Root */
    }
    int64_t save[4], root = jet_rt_root_begin(WORD(save));
    assert(root && jet_rt_stop_domain() == 1 && jet_rt_session_status() == 0);
    if (!strcmp(mode, "normal")) {
        int64_t code = INT64_MIN;
        assert(jet_rt_root_take_stop(root, WORD(&code)) == 0);
        assert(code == INT64_MIN); /* NULL is success, not a fabricated exit */
    } else if (!strcmp(mode, "exit0")) {
        int64_t saved = jet_rt_session_cleanup_enter(0);
        assert(jet_rt_root_publish_rendered(0, 0, 0));
        assert(jet_rt_session_status() == 5 && jet_rt_session_propagate() == 5);
        jet_rt_session_cleanup_leave(saved); /* must not erase known code0 */
        take(root, 0, NULL, 0);
    } else if (!strcmp(mode, "exact-report")) {
        assert(jet_rt_root_publish_rendered(WORD(report), sizeof report - 1, 73));
        int64_t old = jet_rt_session_cleanup_enter(5);
        assert(jet_rt_session_status() == 5 && jet_rt_session_poll() == 0);
        int64_t mismatch = jet_rt_session_cleanup_enter(1);
        assert(jet_rt_session_propagate() == 5); /* never mint a fictitious kind1 mask */
        jet_rt_session_cleanup_leave(mismatch);
        assert(jet_rt_session_propagate() == 0);
        jet_rt_session_cleanup_leave(old);
        assert(jet_rt_session_propagate() == 5);
        take(root, 73, report, sizeof report - 1);
    } else if (!strcmp(mode, "central-drop")) {
        central_drop(root);
    } else if (!strcmp(mode, "nested")) {
        int64_t inner_save[4], inner = jet_rt_root_begin(WORD(inner_save));
        assert(jet_rt_stop_domain() == 1);
        assert(jet_rt_root_publish_rendered(0, 0, 19));
        take(inner, 19, NULL, 0);
        jet_rt_root_leave(inner, WORD(inner_save));
        assert(jet_rt_stop_domain() == 1 && jet_rt_session_status() == 0);
    } else if (!strcmp(mode, "fifo")) {
        jet_rt_native_atexit(WORD(closure(1)));
        jet_rt_native_atexit(WORD(closure(2)));
        jet_rt_task_exit_handlers();
        const int expected[] = {1, 11, 2, 14, 12, 3, 13};
        assert(event_count == (int)(sizeof expected / sizeof *expected));
        assert(!memcmp(events, expected, sizeof expected));
        jet_rt_task_exit_handlers();
        assert(event_count == 7); /* queue is drained once */
        take(root, 73, report, sizeof report - 1);
    } else if (!strcmp(mode, "task-origin")) {
        task_origin();
    } else if (!strcmp(mode, "range-zero-step") || !strcmp(mode, "range-exhausted")) {
        int64_t cursor[5] = {7, 7, 1, 1, 0}, unchanged[5] = {7, 7, 1, 1, 0};
        int64_t result = !strcmp(mode, "range-zero-step")
            ? jet_loop_range_init(0, 1, 0, 1, 1)
            : jet_loop_range_value(WORD(cursor));
        /* The returned word is not an ordinary handle/Int: poll before use. */
        assert(jet_rt_session_propagate() == 5);
        assert(result == 0);
        assert(!memcmp(cursor, unchanged, sizeof cursor));
        int64_t code = INT64_MIN, record = jet_rt_root_take_stop(root, WORD(&code));
        assert(record && code == 70 && jet_rt_stop_kind(record) == 5);
        assert(jet_rt_stop_form(record) == 1);
        Text *text = jet_rt_stop_take_message(record);
        jet_rt_stop_drop(record);
        assert(text && text->length > 0);
        jet_rt_string_drop(text);
        assert(jet_rt_session_status() == 0);
    } else if (!strcmp(mode, "deadline")) {
        static const unsigned char deadline[] =
            "Error [E3003]: Deadline exceeded while waiting in time sleep\n"
            " Why: This wait point observed the task context deadline from `#Context(deadline: …)`\n"
            " Fix: Raise the deadline budget or shorten the work before this wait point\n"
            "More: jet-lang.dev/e/E3003";
        int64_t deadline_save[2];
        jet_ctx_push_deadline(-1, WORD(deadline_save));
        jet_task_sleep_ms_defaulted(0);
        jet_ctx_pop_deadline(WORD(deadline_save));
        assert(jet_rt_session_status() == 2);
        take(root, 70, deadline, sizeof deadline - 1); /* full E3003, no invented LF */
    } else if (!strcmp(mode, "budget3") || !strcmp(mode, "budget4")) {
        int64_t kind = !strcmp(mode, "budget3") ? 3 : 4, status = kind - 1;
        assert(jet_rt_control_publish_stop(kind, status, -1, INT64_MIN, INT64_MAX, 0, 0, 0));
        int64_t code = INT64_MIN, record = jet_rt_root_take_stop(root, WORD(&code));
        assert(record && code == 80 + status && jet_rt_stop_kind(record) == kind);
        assert(jet_rt_stop_form(record) == 1 && jet_rt_stop_jct_status(record) == status);
        assert(jet_rt_stop_owner(record) == -1 && jet_rt_stop_start(record) == INT64_MIN);
        assert(jet_rt_stop_end(record) == INT64_MAX);
        Text *text = jet_rt_stop_take_message(record);
        assert(!text || text->length == 0); /* no fabricated human budget diagnostic */
        jet_rt_stop_drop(record);
        jet_rt_string_drop(text);
        assert(jet_rt_session_status() == 0);
    } else if (!strcmp(mode, "raw-publish") || !strcmp(mode, "panic-message") ||
        !strcmp(mode, "host-context-operation") || !strcmp(mode, "host-error-context") ||
        !strcmp(mode, "host-limit-tag") || !strcmp(mode, "host-limit-range")) {
        if (!strcmp(mode, "raw-publish")) {
            assert(jet_rt_control_publish_stop(5, 7, -1, INT64_MIN, INT64_MAX,
                WORD("root-reason"), 11, 0));
        } else if (!strcmp(mode, "panic-message")) {
            jet_rt_panic_message(WORD("root-reason"), 11);
        } else {
            int64_t error[7] = {0}, unchanged[7] = {0};
            if (!strcmp(mode, "host-context-operation")) {
                /* Borrowed sentinel arguments must not be inspected on refusal. */
                assert(jet_rt_record_new_io_context(-1, 1, 1, 1) == 0);
            } else if (!strcmp(mode, "host-error-context")) {
                assert(jet_rt_record_get_io_error_not_found(WORD(error)) == 0);
            } else if (!strcmp(mode, "host-limit-tag")) {
                assert(jet_rt_record_get_io_error_resource_limit(WORD(error)) == 0);
            } else {
                assert(jet_rt_record_new_io_error_resource_limit(-1) == 0);
            }
            assert(!memcmp(error, unchanged, sizeof error));
        }
        int64_t code, record = jet_rt_root_take_stop(root, WORD(&code));
        assert(record && code == 70 && jet_rt_stop_kind(record) == 5);
        assert(jet_rt_stop_form(record) == 1);
        if (!strcmp(mode, "raw-publish")) {
            assert(jet_rt_stop_jct_status(record) == 7 && jet_rt_stop_owner(record) == -1);
            assert(jet_rt_stop_start(record) == INT64_MIN && jet_rt_stop_end(record) == INT64_MAX);
        }
        Text *text = jet_rt_stop_take_message(record);
        jet_rt_stop_drop(record);
        assert(text && fwrite(text->data, 1, (size_t)text->length, stdout) == (size_t)text->length);
        jet_rt_string_drop(text);
    } else if (!strcmp(mode, "root-cancel-publisher")) {
        (void)jet_rt_control_publish_stop(1, -1, -1, -1, -1, 0, 0, 0);
        abort(); /* expected actual host defect101 before copy */
    } else if (!strcmp(mode, "root-interrupt-producer")) {
        jet_rt_control_interrupt(root);
        abort(); /* expected host defect101 before writing pending48 */
    } else if (!strcmp(mode, "corrupt-root-poll")) {
        ((int64_t *)(uintptr_t)root)[6] = 1; /* explicit invalid pending48 */
        (void)jet_rt_session_poll();
        abort();
    } else if (!strcmp(mode, "corrupt-root-take")) {
        int64_t record = cancelled_record();
        ((int64_t *)(uintptr_t)root)[4] = 1;
        ((int64_t *)(uintptr_t)root)[5] = record; /* transfer the real owned bare record */
        int64_t code;
        (void)jet_rt_root_take_stop(root, WORD(&code));
        abort(); /* never success / exit0 / exit70 */
    } else if (!strcmp(mode, "double-panic-shield") || !strcmp(mode, "nested-inherited-stop")) {
        assert(jet_rt_root_publish_rendered(WORD(report), sizeof report - 1, 73));
        (void)jet_rt_session_cleanup_enter(5);
        jet_rt_session_shield_enter();
        if (!strcmp(mode, "nested-inherited-stop")) {
            int64_t inner_save[4];
            (void)jet_rt_root_begin(WORD(inner_save));
            assert(jet_rt_stop_domain() == 1 && jet_rt_session_status() == 0);
        }
        jet_rt_panic_message(WORD("second"), 6);
        abort(); /* actual delivered cleanup/inheritance beats the shield */
    } else {
        fprintf(stderr, "unknown Root witness mode: %s\n", mode);
        return 2;
    }
    jet_rt_root_leave(root, WORD(save));
    assert(jet_rt_stop_domain() == 0);
    puts("ROOT CLEANUP OK");
    return 0;
}
