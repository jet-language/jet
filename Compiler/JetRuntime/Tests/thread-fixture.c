/* Transport only: every concurrency/TLS assertion lives in ThreadTests.jet. */
#include <stdint.h>
extern int64_t jet_rt_thread_fixture_run(int64_t);
extern int64_t jet_rt_thread_fixture_worker(int64_t);
extern int64_t jet_rt_thread_fixture_park_worker(int64_t);
extern int64_t jet_rt_thread_fixture_clone_worker(int64_t);
extern int64_t jet_rt_thread_fixture_destructor(int64_t, int64_t);
int main(void) {
    const int64_t callbacks[] = {
        (int64_t)(uintptr_t)jet_rt_thread_fixture_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_park_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_clone_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_destructor,
    };
    return (int)jet_rt_thread_fixture_run((int64_t)(uintptr_t)callbacks);
}
