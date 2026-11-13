// The host of a runtime test fragment run as a program: jet_rt_main runs the
// fragment's entry as a program's main runs its entry, to the real process
// end, and the fragment's ended check, if its unit exports one, then sees
// what that end left (Tools/agent/CompilerRegistries.jet plain transport,
// tests/cases/runtime_fragments).
#include <stdint.h>
extern char **environ;
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern int64_t jet_rt_main(void (*entry)(void));
extern void jet_rt_process_entry_return(void);
extern void jet_rt_plain_fixture(void);
extern void jet_rt_plain_fixture_ended(void) __attribute__((weak));

// A program's entry returns through jet_rt_process_entry_return (JetIR
// ir_emit_return), which marks the process ending.
static void entry(void) {
    jet_rt_plain_fixture();
    jet_rt_process_entry_return();
}

int main(int argc, char **argv) {
    jet_rt_host_arguments(argc, argv, environ);
    int64_t status = jet_rt_main(entry);
    if (status != 0) return (int)status;
    if (jet_rt_plain_fixture_ended) jet_rt_plain_fixture_ended();
    return 0;
}
