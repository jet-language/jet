// The gauntlet's launcher: `launch REPORT COMMAND [ARG...]` runs COMMAND with
// the launcher's stdin, stderr and environment (a bare name looked up on
// PATH, as execvp does), relays its stdout to the launcher's, waits for it,
// and writes REPORT. timer.py, Timer.jet and run.mjs run every measured
// command through it, so the numbers are the command's own, not theirs.
//
// Peak memory is each process's own. ru_maxrss can't give it: Linux carries a
// process's high-water mark across exec, so a command forked by python or
// node reads at least the pages it copied from its parent (a 15 MB floor
// under python), and the kernel's per-CPU RSS counters make small readings
// approximate. So the launcher traces the command and every process it
// starts (fork, vfork and clone events only; system calls run untraced), and
// at each one's exit reads its exact VmHWM from /proc, the peak of the memory
// it has had since its last exec. The peak is the largest of them, as GNU
// time's %M is the largest process.
//
// A process has one tracer, so a launcher that is itself traced (TracerPid in
// /proc/self/status: a timed timer, or a debugger) cannot trace the command:
// its children belong to that tracer. It runs the command untraced then, and
// the peak is the command's ru_maxrss from wait4, which carries the
// launcher's own small high-water mark across the fork, a floor of the
// launcher's size rather than python's or node's.
//
// REPORT gets one line when the command is about to exec:
//   start <pid> <monotonic ns>
// and one when it has been reaped and its stdout has closed:
//   end <exit code> <wall ns> <first stdout ns> <peak KiB> <user us> <system us> <exec errno>
// The wall time runs from just before exec to just after the reap, and the
// first stdout byte's time (-1 for none) from the same start; both are read
// here. User and system time are the command's and its reaped descendants'.
// The exit code is minus the signal's number for a signal. An exec errno
// other than 0 means COMMAND did not start (exit code 127) and says why. The
// launcher exits with the command's code (128 + N for signal N), or 64 when
// it cannot run at all.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int64_t monotonic_ns(void) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (int64_t)now.tv_sec * 1000000000 + now.tv_nsec;
}

static void fail(const char *what) {
    fprintf(stderr, "launch: %s: %s\n", what, strerror(errno));
    exit(64);
}

// The VmHWM of process `pid` in KiB, or 0 when /proc no longer has it.
static long vm_hwm_kb(pid_t pid) {
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/status", (int)pid);
    FILE *status = fopen(path, "re");
    if (!status) return 0;
    char line[256];
    long kb = 0;
    while (fgets(line, sizeof line, status)) {
        if (strncmp(line, "VmHWM:", 6) == 0) {
            kb = strtol(line + 6, NULL, 10);
            break;
        }
    }
    fclose(status);
    return kb;
}

// Whether the launcher itself is traced: TracerPid in /proc/self/status is
// not 0.
static int traced(void) {
    FILE *status = fopen("/proc/self/status", "re");
    if (!status) return 0;
    char line[256];
    long tracer = 0;
    while (fgets(line, sizeof line, status)) {
        if (strncmp(line, "TracerPid:", 10) == 0) {
            tracer = strtol(line + 10, NULL, 10);
            break;
        }
    }
    fclose(status);
    return tracer != 0;
}

// The traced processes seen stopped so far: a process the launcher has not
// seen yet stops first with the SIGSTOP tracing starts it with, which is
// swallowed; a later SIGSTOP is the program's own and is passed on.
static pid_t *seen;
static size_t seen_count;
static size_t seen_capacity;

static int first_stop(pid_t pid) {
    for (size_t i = 0; i < seen_count; i += 1) {
        if (seen[i] == pid) return 0;
    }
    if (seen_count == seen_capacity) {
        seen_capacity = seen_capacity ? seen_capacity * 2 : 64;
        seen = realloc(seen, seen_capacity * sizeof *seen);
        if (!seen) fail("realloc");
    }
    seen[seen_count++] = pid;
    return 1;
}

// The relay: copies the command's stdout to the launcher's, noting when its
// first byte arrives. It runs on its own thread because ptrace requests must
// come from the thread that forked the command.
struct relay {
    int from;
    int64_t first_ns;
};

static void *relay_stdout(void *argument) {
    struct relay *relay = argument;
    char buffer[65536];
    int forwarding = 1;
    for (;;) {
        ssize_t got = read(relay->from, buffer, sizeof buffer);
        if (got < 0 && errno == EINTR) continue;
        if (got <= 0) break;
        if (relay->first_ns < 0) relay->first_ns = monotonic_ns();
        // Once the launcher's stdout is gone the rest is drained unread, as
        // the command's own writes would have gone nowhere.
        for (ssize_t at = 0; forwarding && at < got;) {
            ssize_t written = write(1, buffer + at, (size_t)(got - at));
            if (written < 0 && errno == EINTR) continue;
            if (written <= 0) forwarding = 0;
            else at += written;
        }
    }
    close(relay->from);
    return NULL;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: launch REPORT COMMAND [ARG...]\n");
        return 64;
    }
    // A closed reader of the launcher's stdout must not kill it before the
    // report, and neither must a stop signal sent to the whole process tree
    // (the command gets it too and its exit ends the launcher's wait). The
    // command keeps the dispositions the launcher was given.
    static const int held[] = {SIGPIPE, SIGTERM, SIGINT, SIGHUP};
    enum { HELD = sizeof held / sizeof held[0] };
    struct sigaction given[HELD];
    struct sigaction ignore = {0};
    ignore.sa_handler = SIG_IGN;
    for (int i = 0; i < HELD; i += 1) sigaction(held[i], &ignore, &given[i]);
    FILE *report = fopen(argv[1], "we");
    if (!report) fail("cannot write the report");
    // The child writes its exec errno here; a successful exec closes it.
    int failed[2];
    int output[2];
    // An untraced command waits for one byte here before its exec.
    int go[2];
    if (pipe2(failed, O_CLOEXEC) != 0 || pipe2(output, O_CLOEXEC) != 0 || pipe2(go, O_CLOEXEC) != 0) fail("pipe2");
    int tracing = !traced();
    pid_t pid = fork();
    if (pid < 0) fail("fork");
    if (pid == 0) {
        for (int i = 0; i < HELD; i += 1) sigaction(held[i], &given[i], NULL);
        if (dup2(output[1], 1) != 1) _exit(126);
        // Stop until the launcher has set its trace options (or, untraced,
        // has written the start line), then exec.
        if (tracing) {
            if (ptrace(PTRACE_TRACEME, 0, NULL, NULL) != 0 || raise(SIGSTOP) != 0) _exit(126);
        } else {
            char byte;
            ssize_t got;
            while ((got = read(go[0], &byte, 1)) < 0 && errno == EINTR) {}
            if (got != 1) _exit(126);
        }
        execvp(argv[2], &argv[2]);
        int error = errno;
        ssize_t written = write(failed[1], &error, sizeof error);
        (void)written;
        _exit(127);
    }
    close(failed[1]);
    close(output[1]);
    close(go[0]);
    int status = 0;
    if (tracing) {
        if (waitpid(pid, &status, __WALL) != pid || !WIFSTOPPED(status)) {
            fprintf(stderr, "launch: the command did not stop for tracing\n");
            return 64;
        }
        first_stop(pid);
        long options = PTRACE_O_TRACEEXIT | PTRACE_O_TRACEFORK | PTRACE_O_TRACEVFORK | PTRACE_O_TRACECLONE | PTRACE_O_EXITKILL;
        if (ptrace(PTRACE_SETOPTIONS, pid, NULL, (void *)options) != 0) fail("ptrace");
    }
    struct relay relay = {output[0], -1};
    pthread_t relay_thread;
    errno = pthread_create(&relay_thread, NULL, relay_stdout, &relay);
    if (errno != 0) fail("pthread_create");
    int64_t started = monotonic_ns();
    fprintf(report, "start %d %lld\n", (int)pid, (long long)started);
    fflush(report);
    if (tracing) {
        if (ptrace(PTRACE_CONT, pid, NULL, NULL) != 0) fail("ptrace");
    } else {
        ssize_t written;
        while ((written = write(go[1], "g", 1)) < 0 && errno == EINTR) {}
        if (written != 1) fail("write");
    }
    close(go[1]);
    int exec_errno = 0;
    while (read(failed[0], &exec_errno, sizeof exec_errno) < 0 && errno == EINTR) {}
    close(failed[0]);
    long peak_kb = 0;
    int code = 0;
    struct rusage usage;
    memset(&usage, 0, sizeof usage);
    int64_t ended = started;
    for (;;) {
        struct rusage reaped_usage;
        pid_t stopped = wait4(tracing ? -1 : pid, &status, tracing ? __WALL : 0, &reaped_usage);
        if (stopped < 0) {
            if (errno == EINTR) continue;
            if (errno == ECHILD) break;
            fail("wait4");
        }
        if (WIFEXITED(status) || WIFSIGNALED(status)) {
            if (stopped == pid) {
                ended = monotonic_ns();
                code = WIFSIGNALED(status) ? -WTERMSIG(status) : WEXITSTATUS(status);
                usage = reaped_usage;
                if (!tracing) peak_kb = usage.ru_maxrss;
                break;
            }
            continue;
        }
        if (!tracing || !WIFSTOPPED(status)) continue;
        int event = status >> 16;
        int signal = WSTOPSIG(status);
        int deliver = 0;
        if (event == PTRACE_EVENT_EXIT) {
            long kb = vm_hwm_kb(stopped);
            if (kb > peak_kb) peak_kb = kb;
        } else if (event == 0 && signal != SIGSTOP && signal != SIGTRAP) {
            deliver = signal;
        } else if (event == 0 && signal == SIGSTOP && !first_stop(stopped)) {
            deliver = SIGSTOP;
        }
        ptrace(PTRACE_CONT, stopped, NULL, (void *)(long)deliver);
    }
    pthread_join(relay_thread, NULL);
    int64_t first = relay.first_ns < 0 ? -1 : (relay.first_ns > started ? relay.first_ns - started : 0);
    fprintf(report, "end %d %lld %lld %ld %lld %lld %d\n", code, (long long)(ended - started), (long long)first, peak_kb,
        (long long)usage.ru_utime.tv_sec * 1000000 + usage.ru_utime.tv_usec,
        (long long)usage.ru_stime.tv_sec * 1000000 + usage.ru_stime.tv_usec, exec_errno);
    fclose(report);
    return code < 0 ? 128 - code : code;
}
