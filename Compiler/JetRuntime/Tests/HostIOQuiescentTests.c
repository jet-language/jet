// Seed parity, not Linux host-operation instrumentation. Link the complete
// composed Jet runtime and compare the runner's exact stdout golden.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

extern bool jet_rt_observe_io_quiescent(void);
extern void jet_rt_host_program_start(void);
extern void *jet_fd_adopt_raw(int64_t, void *, bool);
extern int64_t jet_fd_read_raw(void *, void *, int64_t, int64_t *);
extern void jet_fd_drop_raw(void *);

typedef struct {
    void *fd;
    atomic_int tid;
    atomic_bool done;
    unsigned char byte;
    int64_t result, error[4];
} ReadOnce;

static void *read_once(void *pointer) {
    ReadOnce *attempt = pointer;
    atomic_store(&attempt->tid, (int)syscall(SYS_gettid));
    attempt->result = jet_fd_read_raw(attempt->fd, &attempt->byte, 1, attempt->error);
    atomic_store(&attempt->done, true);
    return NULL;
}

// Confirm the actual kernel read, rather than assuming an announced thread
// or a sleep has entered it. No runtime counter or production hook is added.
static void await_pipe_read(ReadOnce *attempt, int pipe_fd) {
    const struct timespec pause = {0, 1000000};
    for (int i = 0; i < 5000; ++i) {
        assert(!atomic_load(&attempt->done));
        int tid = atomic_load(&attempt->tid);
        if (tid) {
            char path[128];
            int count = snprintf(path, sizeof path, "/proc/self/task/%d/syscall", tid);
            assert(count > 0 && (size_t)count < sizeof path);
            FILE *file = fopen(path, "r");
            assert(file);
            long number = -1;
            unsigned long descriptor = 0;
            int fields = fscanf(file, "%ld %lx", &number, &descriptor);
            assert(!fclose(file));
            if (fields == 2 && number == SYS_read && descriptor == (unsigned long)pipe_fd) return;
        }
        assert(!nanosleep(&pause, NULL));
    }
    assert(!"pipe reader did not enter the kernel read");
}

int main(void) {
    jet_rt_host_program_start();
    assert(jet_rt_observe_io_quiescent());
    int pipe_fd[2];
    assert(!pipe(pipe_fd));
    ReadOnce attempt = {.fd = jet_fd_adopt_raw(pipe_fd[0], NULL, false)};
    assert(attempt.fd);
    pthread_t reader;
    assert(!pthread_create(&reader, NULL, read_once, &attempt));
    await_pipe_read(&attempt, pipe_fd[0]);
    // Scheduler.rs:2653 confines all three seed counters to Windows IOCP;
    // Observe.rs:1051-1052 therefore returns true even with this Linux read.
    assert(jet_rt_observe_io_quiescent());
    assert(write(pipe_fd[1], "Q", 1) == 1);
    assert(!pthread_join(reader, NULL));
    assert(atomic_load(&attempt.done) && attempt.result == 1 && attempt.byte == 'Q');
    for (int i = 0; i < 4; ++i) assert(attempt.error[i] == 0);
    assert(!close(pipe_fd[1]));
    jet_fd_drop_raw(attempt.fd);
    assert(jet_rt_observe_io_quiescent());
    puts("io quiescent: true idle, blocked pipe, retired");
    return 0;
}
