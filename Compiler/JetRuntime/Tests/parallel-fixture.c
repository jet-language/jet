/* Transport only; range policy, failures and owner assertions are Jet code. */
#include <stdint.h>
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_parallel_fixture_invoke(int64_t, int64_t, int64_t);
extern int64_t jet_rt_parallel_fixture_run(int64_t);
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments((int64_t)argc, (int64_t)(uintptr_t)argv, (int64_t)(uintptr_t)envp);
    return (int)jet_rt_parallel_fixture_run((int64_t)(uintptr_t)jet_rt_parallel_fixture_invoke);
}
