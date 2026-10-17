
#include <pthread.h>
#include <stdint.h>

typedef int32_t (*callback_t)(int32_t);
typedef struct { callback_t callback; int32_t input; int32_t output; } Job;

static void *run_job(void *raw) {
    Job *job = (Job *)raw;
    job->output = job->callback(job->input);
    return 0;
}

int32_t foreign_callback(callback_t callback, int32_t input) {
    Job job = { callback, input, 0 };
    pthread_t thread;
    if (pthread_create(&thread, 0, run_job, &job) != 0) return -1;
    if (pthread_join(thread, 0) != 0) return -2;
    return job.output;
}

typedef struct { int32_t input; int32_t output; } AsyncJob;

static void *run_async_job(void *raw) {
    AsyncJob *job = (AsyncJob *)raw;
    job->output = job->input + 1;
    return 0;
}

int32_t foreign_async(int32_t input) {
    AsyncJob job = { input, 0 };
    pthread_t thread;
    if (pthread_create(&thread, 0, run_async_job, &job) != 0) return -1;
    if (pthread_join(thread, 0) != 0) return -2;
    return job.output;
}
