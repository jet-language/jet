#define _GNU_SOURCE
#include <inttypes.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

extern int64_t jet_closure_ref_test_run(void);
extern void jet_rt_alloc_init(void);
extern int64_t __real_jet_rt_alloc(int64_t size, int64_t align);
extern void __real_jet_rt_free(int64_t pointer, int64_t size, int64_t align);

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_barrier_t first_pair;
static pthread_t threads[32];
static unsigned thread_count, arrivals, box_allocs, box_frees;
static int64_t box;

static void require(int condition, const char *message) {
    if (!condition) { fprintf(stderr, "closure-refs-shared: %s\n", message); abort(); }
}

int64_t __wrap_jet_rt_alloc(int64_t size, int64_t align) {
    int64_t pointer = __real_jet_rt_alloc(size, align);
    if (size == 168) {
        pthread_mutex_lock(&lock);
        require(align == 8 && pointer != 0 && box == 0 && box_allocs == 0,
                "unexpected allocation in the eighteen-capture closure class");
        box = pointer;
        box_allocs++;
        pthread_mutex_unlock(&lock);
    }
    return pointer;
}

void __wrap_jet_rt_free(int64_t pointer, int64_t size, int64_t align) {
    pthread_mutex_lock(&lock);
    if (pointer == box && box != 0) {
        require(size == 168 && align == 8 && box_frees == 0,
                "closure free layout or retirement mismatch");
        box = 0;
        box_frees++;
    } else {
        require(size != 168, "unmatched closure-class free");
    }
    pthread_mutex_unlock(&lock);
    __real_jet_rt_free(pointer, size, align);
}

// Inputs cross an actual C import, so no source-time constant folding can
// erase the eighteen captures whose generated box the allocator observes.
int64_t jet_closure_ref_test_input(int64_t index) { return index; }

void jet_closure_ref_test_enter(void) {
    pthread_t self = pthread_self();
    pthread_mutex_lock(&lock);
    unsigned found = 0;
    for (unsigned i = 0; i < thread_count; i++) found |= pthread_equal(self, threads[i]);
    if (!found) {
        require(thread_count < 32, "too many worker threads");
        threads[thread_count++] = self;
    }
    unsigned position = ++arrivals;
    pthread_mutex_unlock(&lock);
    // Require two real workers before either starts the read/clone/drop loop.
    // A single-thread fallback is not concurrency evidence and cannot pass.
    if (position <= 2) {
        int result = pthread_barrier_wait(&first_pair);
        require(result == 0 || result == PTHREAD_BARRIER_SERIAL_THREAD, "worker barrier failed");
    }
}

int main(void) {
    require(pthread_barrier_init(&first_pair, NULL, 2) == 0, "barrier init failed");
    jet_rt_alloc_init();
    int64_t checksum = jet_closure_ref_test_run();
    require(checksum == INT64_C(54720000), "wrong callback checksum");
    require(arrivals == 32 && thread_count >= 2, "not an N-thread Shared witness");
    require(box_allocs == 1 && box_frees == 1 && box == 0, "closure allocation is not balanced");
    printf("{\"checksum\":%" PRId64 ",\"workers\":%u,\"threads\":%u,\"closure_allocs\":%u,\"closure_frees\":%u,\"closure_live\":0}\n",
            checksum, arrivals, thread_count, box_allocs, box_frees);
    require(pthread_barrier_destroy(&first_pair) == 0, "barrier destroy failed");
    return 0;
}
