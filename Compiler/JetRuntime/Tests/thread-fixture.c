/* Transport only: every concurrency/TLS assertion lives in ThreadTests.jet. */
#include <stdint.h>
#ifdef JET_SHARED_TLS_FIXTURE
extern int64_t jet_rt_shared_tls_fixture_run(int64_t, int64_t, int64_t);
extern void jet_rt_shared_tls_fixture_payload_drop(int64_t);
extern int64_t jet_rt_shared_tls_fixture_site_worker(int64_t);
extern int64_t jet_rt_shared_tls_fixture_scope_worker(int64_t);
int main(void) {
    return (int)jet_rt_shared_tls_fixture_run(
        (int64_t)(uintptr_t)jet_rt_shared_tls_fixture_payload_drop,
        (int64_t)(uintptr_t)jet_rt_shared_tls_fixture_site_worker,
        (int64_t)(uintptr_t)jet_rt_shared_tls_fixture_scope_worker);
}
#else
extern int64_t jet_rt_thread_fixture_run(int64_t);
extern int64_t jet_rt_thread_fixture_worker(int64_t);
extern int64_t jet_rt_thread_fixture_park_worker(int64_t);
extern int64_t jet_rt_thread_fixture_clone_worker(int64_t);
extern int64_t jet_rt_thread_fixture_destructor(int64_t, int64_t);
extern int64_t jet_rt_thread_fixture_rearm_destructor(int64_t, int64_t);
extern int64_t jet_rt_thread_fixture_rearm_worker(int64_t);
extern int64_t jet_rt_thread_fixture_attached_child(int64_t);
int main(void) {
    const int64_t callbacks[] = {
        (int64_t)(uintptr_t)jet_rt_thread_fixture_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_park_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_clone_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_destructor,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_rearm_destructor,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_rearm_worker,
        (int64_t)(uintptr_t)jet_rt_thread_fixture_attached_child,
    };
    return (int)jet_rt_thread_fixture_run((int64_t)(uintptr_t)callbacks);
}
#endif
