#include <stdint.h>
#include <string.h>
#include <stdbool.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_world_getter_fixture(void);
extern int64_t jet_rt_world_timer_fixture(int64_t);
extern int64_t jet_rt_world_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_run(int64_t);
extern int64_t jet_rt_sentry_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_ledger(void);
extern int64_t jet_rt_sentry_fixture_inline(int64_t);
extern int64_t jet_rt_sentry_inline_stopped(int64_t, int64_t);
typedef struct { const char *ptr; int64_t len, cap; } Text;
extern int64_t jet_rt_test_scope_fault_fixture(Text *, Text *, Text *);
extern void jet_panic(Text *, int64_t, Text *);
extern void jet_panic_rich(Text *, int64_t, Text *, Text *, int64_t, int64_t, Text *, Text *);
extern void jet_rt_require_stop(int64_t, int64_t *, Text *, Text *, Text *, Text *);
extern int64_t jet_rt_trap_report_fixture(int64_t (*)(int64_t), int64_t, bool);
extern int64_t jet_rt_alloc_policy_fixture(void);
static int64_t trap_callback(int64_t mode) {
    Text file = {"trapfixture.jet", 15, 0}, function = {"check", 5, 0};
    Text source = {" panic()", 8, 0}, message = {"fault", 5, 0}, locals = {"answer=42", 9, 0};
    Text left = {"41", 2, 0}, right = {"42", 2, 0};
    if (mode == 0) jet_panic(&file, 7, &message);
    else if (mode == 1) jet_panic_rich(&file, 7, &function, &source, 2, 3, &message, &locals);
    else {
        int64_t descriptor[] = {(int64_t)(uintptr_t)file.ptr, file.len, 7,
            (int64_t)(uintptr_t)function.ptr, function.len,
            (int64_t)(uintptr_t)source.ptr, source.len, 2, 3};
        jet_rt_require_stop(mode == 2 ? 1 : 0, descriptor, &message, &left, &right, &locals);
    }
    return 0;
}
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    if (argc == 3 && strcmp(argv[1], "trap-report") == 0) {
        int mode = argv[2][0] - '0';
        return (int)jet_rt_trap_report_fixture(trap_callback, mode % 4, mode >= 4);
    }
    if (argc == 2 && strcmp(argv[1], "ledger") == 0)
        return (int)jet_rt_sentry_fixture_ledger();
    if (jet_rt_world_getter_fixture()) return 1;
    if (jet_rt_world_timer_fixture((int64_t)(uintptr_t)jet_rt_world_fixture_worker)) return 2;
    if (jet_rt_sentry_fixture_run((int64_t)(uintptr_t)jet_rt_sentry_fixture_worker)) return 3;
    if (jet_rt_sentry_fixture_inline((int64_t)(uintptr_t)jet_rt_sentry_inline_stopped)) return 4;
    Text code = {"E3010", 5, 0}, wrong = {"E3001", 5, 0}, message = {"index 99", 8, 0};
    if (jet_rt_test_scope_fault_fixture(&code, &wrong, &message)) return 5;
    return (int)jet_rt_alloc_policy_fixture();
}
