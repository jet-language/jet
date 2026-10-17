#include "counter.h"
#include <pthread.h>

typedef struct {
    int64_t value;
} mixed_c_job;

static void *mixed_c_worker(void *raw) {
    mixed_c_job *job = (mixed_c_job *)raw;
    job->value += 1;
    return 0;
}

int64_t mixed_c_value(int64_t seed) {
    return seed + 1;
}

int64_t mixed_c_threaded(int64_t seed) {
    mixed_c_job job = { seed };
    pthread_t worker;
    if (pthread_create(&worker, 0, mixed_c_worker, &job) != 0) return -1;
    if (pthread_join(worker, 0) != 0) return -2;
    return job.value;
}
