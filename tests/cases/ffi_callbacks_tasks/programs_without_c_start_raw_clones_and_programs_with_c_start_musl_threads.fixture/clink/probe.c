// Where a C call runs, as C sees it. The initial thread marks its
// own thread-local and records its pthread_self; a thread musl started has
// its own of both, and a raw clone shares the initial thread's (its FS).
#include <pthread.h>
#include <stdint.h>
#include <sys/syscall.h>
#include <unistd.h>

static __thread int64_t marked;
static pthread_t initial;

void probe_mark_initial(void) {
    marked = 1;
    initial = pthread_self();
}

// 0: the initial thread; 1: a thread with its own musl state; 2: another
// thread that shares the initial thread's musl state.
int64_t probe_place(void) {
    if (syscall(SYS_gettid) == getpid()) return 0;
    if (marked == 0 && !pthread_equal(pthread_self(), initial)) return 1;
    return 2;
}
