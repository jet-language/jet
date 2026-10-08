/* Real task runner and producers; this client defines no runtime providers. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { unsigned char *data; int64_t length, capacity; } Text;
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_control_new(void);
extern void jet_rt_control_enter(int64_t, int64_t);
extern void jet_rt_control_leave(int64_t);
extern void jet_rt_scheduler_control_drop(int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, Text *, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_join(int64_t, int64_t, int64_t);
extern int64_t jet_scheduler_native_propagate(void);
extern int64_t jet_rt_record_new_DataTree_Null(void);
extern Text *jet_rt_record_get_DataTree_Text(int64_t);
extern void jet_rt_handle_drop_DataTree(int64_t);
extern int64_t jet_rt_record_tag_JetTaskFailure(int64_t);
extern Text *jet_rt_record_get_JetTaskFailure_Panicked(int64_t);
extern void jet_rt_handle_drop_JetTaskFailure(int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_string_clone(Text *);
extern void jet_panic(Text *, int64_t, Text *);
extern void jet_rt_numeric_stop(int64_t, int64_t, int64_t, int64_t, int64_t);
static const char reason[] = "record variant index out of range";
static const char numeric_reason[] = "integer overflow";
static unsigned released;
static int64_t body(int64_t mode, int64_t out) {
    (void)out;
    if (mode == 0) {
        int64_t tree = jet_rt_record_new_DataTree_Null();
        /* Nested payload -> clone chain must stop before cloning NULL. */
        Text *result = jet_rt_record_get_DataTree_Text(tree);
        assert(result == NULL);
        assert(jet_scheduler_native_propagate() != 0);
        jet_rt_handle_drop_DataTree(tree);
    } else if (mode == 1) {
        Text file = {(unsigned char *)"<core.prelude>", 14, 0};
        Text message = {(unsigned char *)reason, sizeof(reason) - 1, 0};
        jet_panic(&file, 0, &message);
        assert(jet_scheduler_native_propagate() != 0);
    } else {
        jet_rt_numeric_stop(0, 0, 0, (int64_t)(uintptr_t)numeric_reason, sizeof(numeric_reason) - 1);
        assert(jet_scheduler_native_propagate() != 0);
    }
    return 0;
}
static int64_t release(int64_t env) { (void)env; ++released; return 0; }
static int64_t clone(int64_t source, int64_t target) {
    *(int64_t *)(uintptr_t)target = *(int64_t *)(uintptr_t)source;
    return 0;
}
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    if (argc == 2 && strcmp(argv[1], "root-panic") == 0) {
        Text file = {(unsigned char *)"<core.prelude>", 14, 0};
        Text message = {(unsigned char *)reason, sizeof(reason) - 1, 0};
        jet_panic(&file, 0, &message);
        assert(!"root panic must exit 70");
    }
    if (argc == 2 && strcmp(argv[1], "root-variant") == 0) {
        (void)jet_rt_record_get_DataTree_Text(jet_rt_record_new_DataTree_Null());
        assert(!"root wrong variant must exit 70");
    }
    int64_t control = jet_rt_control_new(), save[4] = {0};
    jet_rt_control_enter(control, (int64_t)(uintptr_t)save);
    /* Canonical native metadata: I64 payload, clone callback, Int kind. */
    int64_t meta[9] = {8, (int64_t)(uintptr_t)clone, 0, 0, 0, 0, INT64_C(274877906946), 0, 0};
    Text label = {NULL, 0, 0};
    for (int64_t mode = 0; mode < 3; ++mode) {
        int64_t task = jet_rt_native_task_spawn(17, &label, mode, (int64_t)(uintptr_t)body,
            (int64_t)(uintptr_t)release, (int64_t)(uintptr_t)meta);
        /* Borrow Task.jet's canonical child identity@128 before join consumes it.
           Observe.rs jet_observe_task_failure_message prefixes "<identity>: ". */
        Text *identity = jet_rt_string_clone(*(Text **)((unsigned char *)(uintptr_t)task + 128));
        int64_t out[4] = {0};
        assert(jet_rt_native_task_join(task, 0, (int64_t)(uintptr_t)out) == 0);
        assert(jet_rt_record_tag_JetTaskFailure(out[0]) == 2);
        Text *text = jet_rt_record_get_JetTaskFailure_Panicked(out[0]);
        const char *expected = mode == 2 ? numeric_reason : reason;
        assert(text->length == identity->length + 2 + (int64_t)strlen(expected));
        assert(memcmp(text->data, identity->data, (size_t)identity->length) == 0);
        assert(memcmp(text->data + identity->length, ": ", 2) == 0);
        assert(memcmp(text->data + identity->length + 2, expected, strlen(expected)) == 0);
        jet_rt_string_drop(identity);
        jet_rt_string_drop(text);
        jet_rt_handle_drop_JetTaskFailure(out[0]);
        assert(jet_scheduler_native_propagate() == 0);
    }
    assert(released == 3);
    jet_rt_control_leave((int64_t)(uintptr_t)save);
    jet_rt_scheduler_control_drop(control);
    puts("task variant/panic/numeric joined Panicked(reason); process survived");
    return 0;
}
