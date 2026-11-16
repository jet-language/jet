// Function-entry tracer for Tools/perf/hot-text.mjs (Linux x86-64).
//
//   fntrace ADDRS OUT -- prog args...
//
// Puts an int3 at every function start listed in ADDRS (one hex address per
// line), runs prog under ptrace, and writes each function's address to OUT
// the first time any thread enters it, in first-entry order. Each
// breakpoint is removed when first hit, so a function costs one stop.
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ptrace.h>
#include <sys/user.h>
#include <sys/wait.h>
#include <unistd.h>

typedef struct { uint64_t addr; uint8_t orig; uint8_t live; } Site;
static Site *sites;
static size_t count;

static int cmp(const void *a, const void *b) {
    uint64_t x = ((const Site *)a)->addr, y = ((const Site *)b)->addr;
    return x < y ? -1 : x > y;
}

static Site *find(uint64_t addr) {
    size_t lo = 0, hi = count;
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        if (sites[mid].addr < addr) lo = mid + 1; else hi = mid;
    }
    return lo < count && sites[lo].addr == addr ? &sites[lo] : NULL;
}

static void poke_byte(pid_t pid, uint64_t addr, uint8_t byte, uint8_t *old) {
    errno = 0;
    long word = ptrace(PTRACE_PEEKTEXT, pid, (void *)addr, 0);
    if (errno) { perror("peek"); exit(2); }
    if (old) *old = (uint8_t)(word & 0xff);
    word = (word & ~0xffL) | byte;
    if (ptrace(PTRACE_POKETEXT, pid, (void *)addr, (void *)word) < 0) { perror("poke"); exit(2); }
}

int main(int argc, char **argv) {
    if (argc < 5 || strcmp(argv[3], "--")) { fprintf(stderr, "usage: fntrace ADDRS OUT -- prog args\n"); return 64; }
    FILE *in = fopen(argv[1], "r");
    if (!in) { perror(argv[1]); return 66; }
    size_t cap = 1 << 16;
    sites = malloc(cap * sizeof *sites);
    unsigned long long a;
    while (fscanf(in, "%llx", &a) == 1) {
        if (count == cap) sites = realloc(sites, (cap *= 2) * sizeof *sites);
        sites[count++] = (Site){a, 0, 0};
    }
    fclose(in);
    qsort(sites, count, sizeof *sites, cmp);
    FILE *out = fopen(argv[2], "w");
    if (!out) { perror(argv[2]); return 73; }
    pid_t child = fork();
    if (child == 0) {
        ptrace(PTRACE_TRACEME, 0, 0, 0);
        raise(SIGSTOP);
        execvp(argv[4], argv + 4);
        _exit(127);
    }
    int status;
    waitpid(child, &status, 0);
    ptrace(PTRACE_SETOPTIONS, child, 0, PTRACE_O_TRACEEXEC | PTRACE_O_TRACECLONE | PTRACE_O_EXITKILL);
    ptrace(PTRACE_CONT, child, 0, 0);
    waitpid(child, &status, 0);
    if (!(WIFSTOPPED(status) && status >> 8 == (SIGTRAP | (PTRACE_EVENT_EXEC << 8)))) { fprintf(stderr, "no exec stop\n"); return 1; }
    size_t armed = 0;
    for (size_t k = 0; k < count; k++) {
        poke_byte(child, sites[k].addr, 0xcc, &sites[k].orig);
        sites[k].live = 1;
        armed++;
    }
    fprintf(stderr, "fntrace: %zu sites armed\n", armed);
    ptrace(PTRACE_CONT, child, 0, 0);
    size_t hits = 0;
    for (;;) {
        pid_t pid = waitpid(-1, &status, __WALL);
        if (pid < 0) break;
        if (WIFEXITED(status) || WIFSIGNALED(status)) {
            if (pid == child) break;
            continue;
        }
        if (!WIFSTOPPED(status)) continue;
        int sig = WSTOPSIG(status);
        if (status >> 16) { ptrace(PTRACE_CONT, pid, 0, 0); continue; }
        if (sig == SIGTRAP) {
            struct user_regs_struct regs;
            ptrace(PTRACE_GETREGS, pid, 0, &regs);
            Site *s = find(regs.rip - 1);
            if (s) {
                if (s->live) {
                    poke_byte(pid, s->addr, s->orig, NULL);
                    s->live = 0;
                    fprintf(out, "%llx\n", (unsigned long long)s->addr);
                    hits++;
                }
                regs.rip -= 1;
                ptrace(PTRACE_SETREGS, pid, 0, &regs);
                ptrace(PTRACE_CONT, pid, 0, 0);
                continue;
            }
            ptrace(PTRACE_CONT, pid, 0, 0);
            continue;
        }
        if (sig == SIGSTOP) { ptrace(PTRACE_CONT, pid, 0, 0); continue; }
        ptrace(PTRACE_CONT, pid, 0, sig);
    }
    fclose(out);
    fprintf(stderr, "fntrace: %zu functions entered\n", hits);
    return 0;
}
