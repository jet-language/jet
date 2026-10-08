#include <stdint.h>
#include <string.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_world_getter_fixture(void);
extern int64_t jet_rt_world_timer_fixture(int64_t);
extern int64_t jet_rt_world_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_run(int64_t);
extern int64_t jet_rt_sentry_fixture_worker(int64_t);
extern int64_t jet_rt_sentry_fixture_ledger(void);
extern int64_t jet_rt_sentry_fixture_inline(int64_t);
extern int64_t jet_rt_sentry_inline_stopped(int64_t, int64_t);
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    if (argc == 2 && strcmp(argv[1], "ledger") == 0)
        return (int)jet_rt_sentry_fixture_ledger();
    if (jet_rt_world_getter_fixture()) return 1;
    if (jet_rt_world_timer_fixture((int64_t)(uintptr_t)jet_rt_world_fixture_worker)) return 2;
    if (jet_rt_sentry_fixture_run((int64_t)(uintptr_t)jet_rt_sentry_fixture_worker)) return 3;
    return (int)jet_rt_sentry_fixture_inline((int64_t)(uintptr_t)jet_rt_sentry_inline_stopped);
}
