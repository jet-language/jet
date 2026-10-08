#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

typedef uintptr_t W;
extern char **environ;
extern void jet_rt_host_arguments(W argc, W argv, W envp);
extern int32_t jet_rt_main(void (*entry)(void));
extern void jet_rt_observe_runtime_start_from_env(void);
extern W jet_rt_observe_fixture_writer(void);
extern W jet_rt_observe_fixture_periodic_started(W writer);
extern W jet_rt_observe_fixture_sequence(W writer);

static int enabled;
static const char *directory;

static size_t thread_count(void) {
    DIR *tasks = opendir("/proc/self/task");
    assert(tasks);
    size_t count = 0;
    struct dirent *entry;
    while ((entry = readdir(tasks))) {
        if (entry->d_name[0] >= '0' && entry->d_name[0] <= '9') ++count;
    }
    assert(!closedir(tasks));
    return count;
}

static size_t file_count(void) {
    DIR *files = opendir(directory);
    assert(files);
    size_t count = 0;
    struct dirent *entry;
    while ((entry = readdir(files))) {
        if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) ++count;
    }
    assert(!closedir(files));
    return count;
}

static void entry(void) {
    W writer = jet_rt_observe_fixture_writer();
    if (!enabled) {
        assert(!writer);
        assert(thread_count() == 1);
        for (unsigned i = 0; i < 100; ++i) jet_rt_observe_runtime_start_from_env();
        assert(!jet_rt_observe_fixture_writer());
        assert(thread_count() == 1);
        assert(!file_count());
        puts("Observe disabled startup: no writer, thread, or output");
        return;
    }
    assert(writer);
    assert(jet_rt_observe_fixture_periodic_started(writer) == 1);
    // The actual detached writer must run, not merely set its spawn CAS bit.
    // Two publications prove the actual periodic loop ran more than once.
    struct timespec pause = {.tv_sec = 0, .tv_nsec = 10000000};
    for (unsigned i = 0; i < 500 && jet_rt_observe_fixture_sequence(writer) < 2; ++i) {
        assert(!nanosleep(&pause, NULL));
    }
    assert(jet_rt_observe_fixture_sequence(writer) >= 2);
    assert(thread_count() == 2);
    for (unsigned i = 0; i < 100; ++i) jet_rt_observe_runtime_start_from_env();
    assert(jet_rt_observe_fixture_writer() == writer);
    assert(jet_rt_observe_fixture_periodic_started(writer) == 1);
    assert(thread_count() == 2);
    puts("Observe enabled startup: one actual periodic writer after repeated start");
    char published[4096];
    int length = snprintf(published, sizeof(published), "%s/jet-observe-%ld.json", directory, (long)getpid());
    assert(length > 0 && (size_t)length < sizeof(published));
    assert(!access(published, F_OK));
}

int main(int argc, char **argv) {
    assert(argc == 3);
    assert(!strcmp(argv[1], "enabled") || !strcmp(argv[1], "unset"));
    enabled = !strcmp(argv[1], "enabled");
    directory = argv[2];
    assert(!file_count());
    assert(thread_count() == 1);
    const char *flag = getenv("JET_OBSERVE");
    assert(enabled ? flag && !strcmp(flag, "1") : !flag);
    // Capture comes before the Jet main's logical-env seed and native inits.
    jet_rt_host_arguments((W)argc, (W)argv, (W)environ);
    assert(jet_rt_main(entry) == 0);
    if (!enabled) assert(!file_count());
    // The writer is a detached process-lifetime daemon: child exit is teardown.
    return 0;
}
