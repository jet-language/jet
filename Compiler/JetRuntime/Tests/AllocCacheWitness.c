// Multi-thread witness for the allocator's per-thread block caches. Link with
// a runtime fixture object that includes AllocTests.jet (its two exported
// probes) and -lpthread. Every payload word carries a tag unique to its
// allocation; a block handed out twice or freed into the wrong slab corrupts
// a tag that a later check catches. Modes: `stress` (default) and `bench`.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

extern void *jet_rt_alloc(int64_t size, int64_t align);
extern void jet_rt_free(void *pointer, int64_t size, int64_t align);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
#ifdef JET_ALLOC_BASELINE
// A pre-cache runtime has no probes; only `bench` runs against it.
static int64_t alloc_test_live_blocks(int64_t width) { (void)width; abort(); }
static void alloc_test_cache_flush(void) { abort(); }
#else
extern int64_t alloc_test_live_blocks(int64_t width);
extern void alloc_test_cache_flush(void);
#endif

static const int64_t sizes[] = {1, 8, 24, 40, 100, 200, 480, 1000, 2000, 4000, 4072};
static const int64_t aligns[] = {1, 8, 16, 64, 4096};
#define SIZES (sizeof sizes / sizeof *sizes)
#define ALIGNS (sizeof aligns / sizeof *aligns)

static int64_t class_of(int64_t size, int64_t align) {
    int64_t effective = size ? size : 1;
    if (effective > 4096 || align > 4096) return 0;
    int64_t needed = effective + (align >= 8 ? 24 : 16) + (align == 8 ? 0 : align - 1);
    for (int64_t c = 32; c <= 4096; c *= 2) if (needed <= c) return c;
    return 0;
}

static void fill(unsigned char *p, int64_t size, uint64_t tag) {
    for (int64_t i = 0; i < size; ++i) p[i] = (unsigned char)(tag >> ((i & 7) * 8)) ^ (unsigned char)i;
}
static void check(const unsigned char *p, int64_t size, uint64_t tag, const char *where) {
    for (int64_t i = 0; i < size; ++i) {
        unsigned char expect = (unsigned char)(tag >> ((i & 7) * 8)) ^ (unsigned char)i;
        if (p[i] != expect) { fprintf(stderr, "%s: payload %p byte %lld corrupt (tag %llx)\n", where, (const void *)p, (long long)i, (unsigned long long)tag); abort(); }
    }
}

typedef struct { void *pointer; int64_t size, align; uint64_t tag; } Block;

// A bounded hand-off queue: producers allocate and fill, consumers verify and free.
typedef struct {
    pthread_mutex_t lock; pthread_cond_t changed;
    Block *ring; size_t cap, head, count; int producers_done;
} Queue;

static void queue_push(Queue *q, Block b) {
    pthread_mutex_lock(&q->lock);
    while (q->count == q->cap) pthread_cond_wait(&q->changed, &q->lock);
    q->ring[(q->head + q->count) % q->cap] = b; q->count++;
    pthread_cond_broadcast(&q->changed);
    pthread_mutex_unlock(&q->lock);
}
static int queue_pop(Queue *q, Block *out) {
    pthread_mutex_lock(&q->lock);
    while (q->count == 0 && !q->producers_done) pthread_cond_wait(&q->changed, &q->lock);
    if (q->count == 0) { pthread_mutex_unlock(&q->lock); return 0; }
    *out = q->ring[q->head]; q->head = (q->head + 1) % q->cap; q->count--;
    pthread_cond_broadcast(&q->changed);
    pthread_mutex_unlock(&q->lock);
    return 1;
}

typedef struct { Queue *queue; unsigned id; unsigned rounds; } Worker;

static void *producer(void *argument) {
    Worker *w = argument;
    jet_rt_thread_attach();
    uint64_t tag = (uint64_t)w->id << 48;
    // Half of the blocks are freed by the producer itself (exercising the
    // owner's cache and its overflow), half travel to a consumer thread.
    for (unsigned r = 0; r < w->rounds; ++r) {
        Block local[64]; unsigned held = 0;
        for (unsigned i = 0; i < 64; ++i) {
            int64_t size = sizes[(r + i) % SIZES], align = aligns[(r * 7 + i) % ALIGNS];
            Block b = { jet_rt_alloc(size, align), size, align, ++tag };
            assert(b.pointer && ((uintptr_t)b.pointer & (uintptr_t)(align - 1)) == 0);
            fill(b.pointer, size, b.tag);
            if (i & 1) queue_push(w->queue, b); else local[held++] = b;
        }
        for (unsigned i = 0; i < held; ++i) {
            check(local[i].pointer, local[i].size, local[i].tag, "producer");
            jet_rt_free(local[i].pointer, local[i].size, local[i].align);
        }
    }
    jet_rt_thread_detach();
    return NULL;
}

static void *consumer(void *argument) {
    Worker *w = argument;
    jet_rt_thread_attach();
    Block b; unsigned long freed = 0;
    while (queue_pop(w->queue, &b)) {
        check(b.pointer, b.size, b.tag, "consumer");
        jet_rt_free(b.pointer, b.size, b.align);
        freed++;
    }
    jet_rt_thread_detach();
    return (void *)(uintptr_t)freed;
}

// A thread that leaves with every class's cache full (and some half-full).
// All threads of one round are attached at once (the barrier), so the
// process-lifetime thread registry reaches its peak size in the warm-up.
static pthread_barrier_t exit_barrier;
static void *exit_full(void *argument) {
    unsigned id = (unsigned)(uintptr_t)argument;
    jet_rt_thread_attach();
    pthread_barrier_wait(&exit_barrier);
    for (int64_t c = 32; c <= 4096; c *= 2) {
        int64_t size = c - 24;
        unsigned n = 64 + (id % 3) * 17;
        void **held = calloc(n, sizeof *held);
        for (unsigned i = 0; i < n; ++i) { held[i] = jet_rt_alloc(size, 8); assert(held[i]); fill(held[i], size, ((uint64_t)id << 40) | (uint64_t)c << 8 | i); }
        for (unsigned i = 0; i < n; ++i) { check(held[i], size, ((uint64_t)id << 40) | (uint64_t)c << 8 | i, "exit"); jet_rt_free(held[i], size, 8); }
        free(held);
    }
    jet_rt_thread_detach();
    return NULL;
}

static void snapshot(int64_t out[8]) { for (int c = 0; c < 8; ++c) out[c] = alloc_test_live_blocks(32 << c); }
static void expect_baseline(const int64_t base[8], const char *phase) {
    int64_t now[8]; snapshot(now);
    for (int c = 0; c < 8; ++c) if (now[c] != base[c]) { fprintf(stderr, "%s: class %d live %lld, baseline %lld\n", phase, 32 << c, (long long)now[c], (long long)base[c]); abort(); }
}

static void single_thread(void) {
    for (size_t s = 0; s < SIZES; ++s) for (size_t a = 0; a < ALIGNS; ++a) {
        int64_t size = sizes[s], align = aligns[a];
        Block held[150];
        for (unsigned i = 0; i < 150; ++i) {
            held[i].pointer = jet_rt_alloc(size, align); held[i].size = size; held[i].align = align; held[i].tag = 0xA000000000000000ull | (uint64_t)(s * 1000 + a * 200 + i);
            assert(held[i].pointer && ((uintptr_t)held[i].pointer & (uintptr_t)(align - 1)) == 0);
            fill(held[i].pointer, size, held[i].tag);
        }
        for (unsigned i = 0; i < 150; ++i) check(held[i].pointer, size, held[i].tag, "single");
        // Free in an order that crosses the cache's 64/32 bounds twice, then
        // reallocate: the LIFO cache hands back exactly these blocks.
        for (unsigned i = 0; i < 150; i += 2) jet_rt_free(held[i].pointer, size, align);
        for (unsigned i = 1; i < 150; i += 2) check(held[i].pointer, size, held[i].tag, "single-after-partial-free");
        for (unsigned i = 1; i < 150; i += 2) jet_rt_free(held[i].pointer, size, align);
        if (class_of(size, align)) {
            void *again = jet_rt_alloc(size, align);
            assert(again == held[149].pointer);
            jet_rt_free(again, size, align);
        }
    }
}

static double now_s(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (double)t.tv_sec + (double)t.tv_nsec / 1e9; }

typedef struct { unsigned long ops; double seconds; } BenchResult;
static void *bench_thread(void *argument) {
    BenchResult *r = argument;
    jet_rt_thread_attach();
    enum { LIVE = 48, ROUNDS = 4000000 };
    void *live[LIVE] = {0}; int64_t size_of[LIVE] = {0};
    double start = now_s(); unsigned long ops = 0;
    for (unsigned i = 0; i < ROUNDS; ++i) {
        unsigned slot = (i * 2654435761u) % LIVE;
        if (live[slot]) { jet_rt_free(live[slot], size_of[slot], 8); ops++; }
        int64_t size = sizes[i % 9]; // classes 32..4096 but mostly small
        live[slot] = jet_rt_alloc(size, 8); size_of[slot] = size; ops++;
        *(volatile uint64_t *)live[slot] = i;
    }
    for (unsigned s = 0; s < LIVE; ++s) if (live[s]) { jet_rt_free(live[s], size_of[s], 8); ops++; }
    r->seconds = now_s() - start; r->ops = ops;
    jet_rt_thread_detach();
    return NULL;
}
static void bench(unsigned threads) {
    pthread_t ids[64]; BenchResult results[64];
    double start = now_s();
    for (unsigned t = 0; t < threads; ++t) assert(!pthread_create(&ids[t], NULL, bench_thread, &results[t]));
    unsigned long total = 0; double longest = 0;
    for (unsigned t = 0; t < threads; ++t) { assert(!pthread_join(ids[t], NULL)); total += results[t].ops; if (results[t].seconds > longest) longest = results[t].seconds; }
    double wall = now_s() - start;
    printf("bench threads=%u ops=%lu wall=%.3fs ops/s=%.0f (slowest thread %.3fs)\n", threads, total, wall, (double)total / wall, longest);
}

int main(int argc, char **argv) {
    jet_rt_thread_attach();
    if (argc > 1 && !strcmp(argv[1], "bench")) {
        unsigned threads = argc > 2 ? (unsigned)atoi(argv[2]) : 1;
        if (threads < 1 || threads > 64) return 2;
        bench(threads);
        return 0;
    }
    // Warm the process-lifetime thread registry to its peak (17 attached
    // threads) before the baseline: its growth moves one block between
    // classes, which is not a leak.
    pthread_t warm[16];
    assert(!pthread_barrier_init(&exit_barrier, NULL, 16));
    for (unsigned i = 0; i < 16; ++i) assert(!pthread_create(&warm[i], NULL, exit_full, (void *)(uintptr_t)i));
    for (unsigned i = 0; i < 16; ++i) assert(!pthread_join(warm[i], NULL));
    assert(!pthread_barrier_destroy(&exit_barrier));
    alloc_test_cache_flush();
    int64_t base[8]; snapshot(base);
    single_thread();
    alloc_test_cache_flush();
    expect_baseline(base, "single-thread");

    enum { PRODUCERS = 8, CONSUMERS = 8, ROUNDS = 400 };
    Block ring[1024];
    Queue queue = { PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER, ring, 1024, 0, 0, 0 };
    Worker producers[PRODUCERS], consumers[CONSUMERS];
    pthread_t pids[PRODUCERS], cids[CONSUMERS];
    for (unsigned i = 0; i < CONSUMERS; ++i) { consumers[i] = (Worker){ &queue, i, 0 }; assert(!pthread_create(&cids[i], NULL, consumer, &consumers[i])); }
    for (unsigned i = 0; i < PRODUCERS; ++i) { producers[i] = (Worker){ &queue, i + 1, ROUNDS }; assert(!pthread_create(&pids[i], NULL, producer, &producers[i])); }
    for (unsigned i = 0; i < PRODUCERS; ++i) assert(!pthread_join(pids[i], NULL));
    pthread_mutex_lock(&queue.lock); queue.producers_done = 1; pthread_cond_broadcast(&queue.changed); pthread_mutex_unlock(&queue.lock);
    unsigned long consumed = 0;
    for (unsigned i = 0; i < CONSUMERS; ++i) { void *n; assert(!pthread_join(cids[i], &n)); consumed += (uintptr_t)n; }
    assert(consumed == (unsigned long)PRODUCERS * ROUNDS * 32);
    alloc_test_cache_flush();
    expect_baseline(base, "cross-thread");

    pthread_t eids[12];
    assert(!pthread_barrier_init(&exit_barrier, NULL, 12));
    for (unsigned round = 0; round < 3; ++round) {
        for (unsigned i = 0; i < 12; ++i) assert(!pthread_create(&eids[i], NULL, exit_full, (void *)(uintptr_t)(i + round * 12)));
        for (unsigned i = 0; i < 12; ++i) assert(!pthread_join(eids[i], NULL));
    }
    assert(!pthread_barrier_destroy(&exit_barrier));
    alloc_test_cache_flush();
    expect_baseline(base, "exit-with-full-caches");
    printf("alloc cache witness: ok (single %zu layouts x150, cross-thread %lu hand-offs, 36 full-cache exits)\n", SIZES * ALIGNS, consumed);
    return 0;
}
