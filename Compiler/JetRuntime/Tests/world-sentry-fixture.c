#include <stdint.h>
#include <string.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_world_getter_fixture(void);
extern int64_t jet_rt_world_timer_fixture(int64_t);
extern int64_t jet_rt_world_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_run(int64_t);
extern int64_t jet_rt_sentry_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_ledger(int64_t);
extern int64_t jet_rt_sentry_fixture_source_gate(void);
extern int64_t jet_rt_sentry_fixture_inline(int64_t);
extern int64_t jet_rt_sentry_inline_stopped(int64_t, int64_t);
typedef struct { const char *ptr; int64_t len, cap; } Text;
extern int64_t jet_rt_test_scope_fault_fixture(Text *, Text *, Text *);
extern int64_t jet_rt_alloc_policy_fixture(void);
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    if (argc == 2 && strcmp(argv[1], "ledger") == 0)
        return (int)jet_rt_sentry_fixture_ledger(0);
    if (argc == 2 && strcmp(argv[1], "ledger-stop") == 0)
        return (int)jet_rt_sentry_fixture_ledger(1);
    if (argc == 2 && strcmp(argv[1], "source-gate") == 0)
        return (int)jet_rt_sentry_fixture_source_gate();
    if (jet_rt_world_getter_fixture()) return 1;
    if (jet_rt_world_timer_fixture((int64_t)(uintptr_t)jet_rt_world_fixture_worker)) return 2;
    if (jet_rt_sentry_fixture_run((int64_t)(uintptr_t)jet_rt_sentry_fixture_worker)) return 3;
    if (jet_rt_sentry_fixture_inline((int64_t)(uintptr_t)jet_rt_sentry_inline_stopped)) return 4;
    Text code = {"E3010", 5, 0}, wrong = {"E3001", 5, 0}, message = {"index 99", 8, 0};
    if (jet_rt_test_scope_fault_fixture(&code, &wrong, &message)) return 5;
    return (int)jet_rt_alloc_policy_fixture();
}
