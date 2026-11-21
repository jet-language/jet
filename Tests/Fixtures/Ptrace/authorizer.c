/* Run a command any process may trace: prctl(PR_SET_PTRACER,
 * PR_SET_PTRACER_ANY), then exec it in this process, which keeps the
 * leave. Where kernel.yama.ptrace_scope is 1 a process may otherwise trace
 * only the processes it started, so a case's `jet debug --attach` or
 * `jet perf attach` could not take a program the case started beside it.
 * SETUP-PORT of tests/debug.rs:824-870; the authorizer asserts nothing. */
#include <sys/prctl.h>
#include <unistd.h>
#include <stdio.h>
int main(int argc, char **argv) {
    if (argc < 2) return 126;
    if (prctl(0x59616d61, (unsigned long)-1, 0, 0, 0) != 0) return 125;
    execvp(argv[1], argv + 1);
    perror("exec");
    return 126;
}
