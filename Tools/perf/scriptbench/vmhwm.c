// The reference reading `Bench.jet --check-floor` holds the launcher's peak
// against: `vmhwm REPORT COMMAND [ARG...]` runs COMMAND as its only tracee,
// stops it at its exit (PTRACE_EVENT_EXIT, before its memory is gone), and
// writes the VmHWM line of its /proc/<pid>/status, in KiB, to REPORT:
//   vmhwm <KiB>
// Nothing else is traced or timed, so the reading is the process's own peak
// and no more. It exits with the command's code (128 + N for signal N), or 64
// when it cannot run at all.
#define _GNU_SOURCE
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: vmhwm REPORT COMMAND [ARG...]\n");
        return 64;
    }
    FILE *report = fopen(argv[1], "we");
    if (!report) {
        perror("vmhwm: cannot write the report");
        return 64;
    }
    pid_t pid = fork();
    if (pid < 0) return 64;
    if (pid == 0) {
        if (ptrace(PTRACE_TRACEME, 0, NULL, NULL) != 0 || raise(SIGSTOP) != 0) _exit(126);
        execvp(argv[2], &argv[2]);
        _exit(127);
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid || !WIFSTOPPED(status)) {
        fprintf(stderr, "vmhwm: the command did not stop for tracing\n");
        return 64;
    }
    if (ptrace(PTRACE_SETOPTIONS, pid, NULL, (void *)(long)(PTRACE_O_TRACEEXIT | PTRACE_O_EXITKILL)) != 0) return 64;
    ptrace(PTRACE_CONT, pid, NULL, NULL);
    long kb = -1;
    for (;;) {
        if (waitpid(pid, &status, 0) != pid) return 64;
        if (WIFEXITED(status) || WIFSIGNALED(status)) break;
        int deliver = 0;
        if (status >> 16 == PTRACE_EVENT_EXIT) {
            char path[64];
            snprintf(path, sizeof path, "/proc/%d/status", (int)pid);
            FILE *proc = fopen(path, "re");
            char line[256];
            while (proc && fgets(line, sizeof line, proc)) {
                if (strncmp(line, "VmHWM:", 6) == 0) {
                    kb = strtol(line + 6, NULL, 10);
                    break;
                }
            }
            if (proc) fclose(proc);
        } else if (WSTOPSIG(status) != SIGTRAP) {
            deliver = WSTOPSIG(status);
        }
        ptrace(PTRACE_CONT, pid, NULL, (void *)(long)deliver);
    }
    fprintf(report, "vmhwm %ld\n", kb);
    fclose(report);
    return WIFSIGNALED(status) ? 128 + WTERMSIG(status) : WEXITSTATUS(status);
}
