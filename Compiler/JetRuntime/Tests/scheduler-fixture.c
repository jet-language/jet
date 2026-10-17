/* Transport only: all assertions and task bodies are real Jet code. */
#include <stdint.h>
#include <string.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_scheduler_fixture_run(int64_t);
extern int64_t jet_rt_scheduler_fixture_root_panic(void);
extern int64_t jet_rt_scheduler_fixture_body(int64_t, int64_t);
extern void jet_rt_scheduler_fixture_release(int64_t);
extern void jet_rt_scheduler_fixture_clone(int64_t, int64_t);
extern int64_t jet_rt_scheduler_fixture_exit_worker(int64_t);
extern void jet_rt_scheduler_fixture_exit_body(int64_t);
extern void jet_rt_scheduler_fixture_exit_release(int64_t);
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments((int64_t)argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    if (argc == 2 && strcmp(argv[1], "root-panic") == 0)
        return (int)jet_rt_scheduler_fixture_root_panic();
    const int64_t callbacks[] = {
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_body,
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_release,
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_clone,
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_exit_worker,
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_exit_body,
        (int64_t)(uintptr_t)jet_rt_scheduler_fixture_exit_release,
    };
    return (int)jet_rt_scheduler_fixture_run((int64_t)(uintptr_t)callbacks);
}
