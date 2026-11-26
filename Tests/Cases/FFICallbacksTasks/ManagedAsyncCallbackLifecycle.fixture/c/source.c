#define _POSIX_C_SOURCE 200809L
#include "source.h"
#include <pthread.h>
#include <time.h>

static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_cv = PTHREAD_COND_INITIALIZER;

static source_callback g_cb = 0;
static void *g_ctx = 0;
static source_callback g_stale_cb = 0;
static void *g_stale_ctx = 0;

static int64_t g_entered = 0;
static int64_t g_done = 0;
static int64_t g_unsub_count = 0;
static pthread_t g_worker;
static int g_worker_started = 0;
static int64_t g_worker_value = 0;

void *on_data(source_callback callback, void *ctx) {
    pthread_mutex_lock(&g_mu);
    g_cb = callback;
    g_ctx = ctx;
    g_stale_cb = callback;
    g_stale_ctx = ctx;
    pthread_mutex_unlock(&g_mu);
    return ctx;
}

int64_t emit_async(int64_t value) {
    pthread_mutex_lock(&g_mu);
    source_callback cb = g_cb;
    void *ctx = g_ctx;
    pthread_mutex_unlock(&g_mu);
    if (cb != 0) {
        cb(ctx, value);
    }
    return value;
}

int unsubscribe(void *ctx) {
    pthread_mutex_lock(&g_mu);
    if (ctx == 0 || ctx != g_ctx) {
        pthread_mutex_unlock(&g_mu);
        return 1;
    }
    g_cb = 0;
    g_ctx = 0;
    g_unsub_count += 1;
    pthread_mutex_unlock(&g_mu);
    return 0;
}

static void *inflight_worker(void *arg) {
    (void)arg;
    pthread_mutex_lock(&g_mu);
    source_callback cb = g_cb;
    void *ctx = g_ctx;
    int64_t val = g_worker_value;
    pthread_mutex_unlock(&g_mu);
    if (cb != 0) {
        cb(ctx, val);
    }
    return 0;
}

int64_t start_inflight_pthread(int64_t value) {
    pthread_mutex_lock(&g_mu);
    g_entered = 0;
    g_done = 0;
    g_worker_value = value;
    g_worker_started = 1;
    pthread_mutex_unlock(&g_mu);
    if (pthread_create(&g_worker, 0, inflight_worker, 0) != 0) {
        return 1;
    }
    return 0;
}

int64_t wait_inflight_entered(void) {
    pthread_mutex_lock(&g_mu);
    while (g_entered == 0) {
        pthread_cond_wait(&g_cv, &g_mu);
    }
    pthread_mutex_unlock(&g_mu);
    return 1;
}

int64_t mark_inflight_entered_and_delay(void) {
    pthread_mutex_lock(&g_mu);
    g_entered = 1;
    pthread_cond_broadcast(&g_cv);
    pthread_mutex_unlock(&g_mu);
    struct timespec ts;
    ts.tv_sec = 0;
    ts.tv_nsec = 40000000L;
    nanosleep(&ts, 0);
    return 1;
}

int64_t mark_inflight_done(void) {
    pthread_mutex_lock(&g_mu);
    g_done = 1;
    pthread_mutex_unlock(&g_mu);
    return 1;
}

int64_t read_inflight_done(void) {
    pthread_mutex_lock(&g_mu);
    int64_t v = g_done;
    pthread_mutex_unlock(&g_mu);
    return v;
}

int64_t join_inflight_pthread(void) {
    if (g_worker_started) {
        pthread_join(g_worker, 0);
        g_worker_started = 0;
    }
    return 0;
}

int64_t invoke_stale_callback(int64_t value) {
    pthread_mutex_lock(&g_mu);
    source_callback cb = g_stale_cb;
    void *ctx = g_stale_ctx;
    pthread_mutex_unlock(&g_mu);
    if (cb != 0) {
        cb(ctx, value);
    }
    return 0;
}

int64_t read_unsub_count(void) {
    pthread_mutex_lock(&g_mu);
    int64_t v = g_unsub_count;
    pthread_mutex_unlock(&g_mu);
    return v;
}
