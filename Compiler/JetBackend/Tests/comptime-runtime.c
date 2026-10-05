// Exercise the actual canonical runtime's worker-only budget/stop ABI.
// Every failing case runs in a freshly exec'd child: no native-frame unwind.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <pthread.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;
static bool (*begin)(uint64_t, uint64_t);
static void (*step)(uint64_t, uint64_t, uint64_t);
static void (*work)(uint64_t);
static void (*enter)(uint64_t, uint64_t, uint64_t);
static void (*leave)(void);
static void (*end)(void);
static void (*panic_message)(const uint8_t *, size_t);

static void *symbol(void *library, const char *name) {
    void *address = dlsym(library, name);
    if (!address) { fprintf(stderr, "missing %s\n", name); exit(1); }
    return address;
}
static void *callback(void *unused) {
    (void)unused;
    step(8, 99, 103);
    return NULL;
}
static int child(const char *mode, const char *path) {
    void *library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!library) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    begin = symbol(library, "jet_rt_comptime_begin");
    step = symbol(library, "jet_rt_comptime_step");
    work = symbol(library, "jet_rt_comptime_work");
    enter = symbol(library, "jet_rt_comptime_enter");
    leave = symbol(library, "jet_rt_comptime_leave");
    end = symbol(library, "jet_rt_comptime_end");
    panic_message = symbol(library, "jet_rt_panic_message");
    if (!strcmp(mode, "success")) {
        // A persistent worker must reset all request state, not respawn.
        for (int i = 0; i < 1000; ++i) {
            if (!begin(2, 1) || begin(2, 1)) return 1;
            enter(7, 1, 2);
            step(7, 13, 17);
            work(1);
            leave();
            end();
            work(UINT64_MAX); // Ordinary runtime remains unbudgeted.
        }
        return 0;
    }
    if (!begin(!strcmp(mode, "fuel") || !strcmp(mode, "callback") ? 1 : 100, 1)) return 1;
    step(7, 13, 17);
    if (!strcmp(mode, "fuel")) work(1);
    else if (!strcmp(mode, "callback")) {
        pthread_t thread;
        if (pthread_create(&thread, NULL, callback, NULL)) return 1;
        pthread_join(thread, NULL);
    } else if (!strcmp(mode, "depth")) {
        enter(7, 1, 2);
        enter(9, 30, 36);
    } else if (!strcmp(mode, "panic")) {
        const uint8_t message[] = "n must be over 100";
        panic_message(message, sizeof(message) - 1);
    }
    return 1; // Each negative case must terminate through its typed envelope.
}
static int check(const char *executable, const char *library, const char *mode, int expected_status, const char *expected) {
    int pipefd[2];
    if (pipe(pipefd)) return 1;
    posix_spawn_file_actions_t actions;
    if (posix_spawn_file_actions_init(&actions)) return 1;
    // Close the child's read end BEFORE assigning fd 3 (read may itself be 3).
    int error = posix_spawn_file_actions_addclose(&actions, pipefd[0]);
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, pipefd[1], 3);
    if (!error && pipefd[1] != 3) error = posix_spawn_file_actions_addclose(&actions, pipefd[1]);
    char *argv[] = { (char *)executable, "--child", (char *)mode, (char *)library, NULL };
    pid_t pid = 0;
    if (!error) error = posix_spawn(&pid, executable, &actions, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&actions);
    close(pipefd[1]);
    if (error) { close(pipefd[0]); return 1; }
    char result[4096];
    size_t size = 0;
    for (;;) {
        ssize_t got = read(pipefd[0], result + size, sizeof(result) - size - 1);
        if (got < 0 && errno == EINTR) continue;
        if (got < 0) return 1;
        if (!got) break;
        size += (size_t)got;
        if (size == sizeof(result) - 1) return 1;
    }
    close(pipefd[0]);
    result[size] = 0;
    int status;
    while (waitpid(pid, &status, 0) < 0) if (errno != EINTR) return 1;
    if (!WIFEXITED(status) || WEXITSTATUS(status) != expected_status || strcmp(result, expected)) {
        fprintf(stderr, "%s: status=%d envelope=%s\n", mode, status, result);
        return 1;
    }
    return 0;
}
int main(int argc, char **argv) {
    if (argc == 4 && !strcmp(argv[1], "--child")) return child(argv[2], argv[3]);
    if (argc != 2) { fprintf(stderr, "usage: %s <canonical runtime.so>\n", argv[0]); return 2; }
    if (check(argv[0], argv[1], "success", 0, "") ||
        check(argv[0], argv[1], "fuel", 82, "JCT1 2 7 13 17 0\n") ||
        check(argv[0], argv[1], "callback", 82, "JCT1 2 8 99 103 0\n") ||
        check(argv[0], argv[1], "depth", 83, "JCT1 3 9 30 36 0\n") ||
        check(argv[0], argv[1], "panic", 81, "JCT1 1 7 13 17 18\nn must be over 100")) return 1;
    puts("native runtime worker budgets and stop envelopes: pass");
    return 0;
}
