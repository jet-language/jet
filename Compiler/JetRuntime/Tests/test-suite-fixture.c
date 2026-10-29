// Runtime batch witness: callbacks execute compiled Jet, not C runner logic.
// The CLI composition separately proves typed named-fn thunk production.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <pthread.h>

typedef int64_t Word;
typedef struct { Word iteration, result, runner; } Suite;
typedef struct { Word refs, code, release; } Runner;
typedef struct { unsigned char *data; Word len, cap; } Text;
extern Word jet_rt_alloc(Word, Word);
extern void jet_rt_free(Word, Word, Word);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_test_suite_install(Word);
extern Word jet_test_suite_new(void);
extern Word jet_test_suite_run(Word);
extern Word jet_test_suite_status(void);
extern Word jet_rt_handle_clone_jet_test_suite(Word);
extern void jet_rt_handle_drop_jet_test_suite(Word);
extern Word jet_rt_handle_show_jet_test_suite(Word);
extern void jet_rt_string_drop(Word);
extern Word jet_rt_test_suite_fixture_first(Word, Word);
extern Word jet_rt_test_suite_fixture_second(Word, Word);
extern Word jet_rt_test_suite_fixture_release(Word);
extern Word jet_rt_test_suite_fixture_state(void);
extern Word jet_rt_test_suite_fixture_key(void);
extern Word jet_rt_tls_get(Word);
extern void jet_rt_program_allocator_configure(Word, Word);
extern void jet_rt_program_allocator_restore(Word, Word);
extern Word jet_rt_program_allocator_fact(Word);

static Word address(const void *p) { return (Word)(uintptr_t)p; }
static Runner *runner(Word (*code)(Word, Word)) {
    Runner *box = (Runner *)(uintptr_t)jet_rt_alloc(24, 8);
    *box = (Runner){1, (Word)(uintptr_t)code,
                   (Word)(uintptr_t)jet_rt_test_suite_fixture_release};
    return box;
}
static Suite *snapshot(void) { return (Suite *)(uintptr_t)jet_test_suite_new(); }
static void drop(Suite *suite) { jet_rt_handle_drop_jet_test_suite(address(suite)); }
static void show(Suite *suite, const char *expected) {
    Word value = jet_rt_handle_show_jet_test_suite(address(suite));
    Text *text = (Text *)(uintptr_t)value;
    assert(text->len == (Word)strlen(expected));
    assert(memcmp(text->data, expected, (size_t)text->len) == 0);
    jet_rt_string_drop(value);
}
static void *worker(void *argument) {
    Runner *own = argument;
    jet_rt_thread_attach();
    assert(jet_rt_tls_get(jet_rt_test_suite_fixture_key()) == 0);
    assert(jet_test_suite_status() == 0);
    Suite *empty = snapshot();
    assert(empty->iteration == 0 && empty->result == 0 && empty->runner == 0);
    assert(jet_test_suite_run(address(empty)) == 0);
    drop(empty);
    Word before = own->refs;
    jet_test_suite_install(address(own));
    assert(own->refs == before + 1);
    Suite *suite = snapshot();
    assert(jet_test_suite_run(address(suite)) == -3);
    assert(jet_test_suite_status() == -3);
    assert(suite->iteration == 0 && suite->result == 0);
    drop(suite);
    jet_rt_thread_detach();
    assert(own->refs == before + 1); // detach frees state, not its intentional pin
    return NULL;
}
int main(void) {
    _Static_assert(sizeof(Suite) == 24 && sizeof(Runner) == 24, "native word layout");
    Runner *first = runner(jet_rt_test_suite_fixture_first);
    Runner *second = runner(jet_rt_test_suite_fixture_second);
    Runner *child = runner(jet_rt_test_suite_fixture_second);
    Suite *empty = snapshot();
    assert(empty->iteration == 0 && empty->result == 0 && empty->runner == 0);
    assert(jet_test_suite_status() == 0 && jet_test_suite_run(address(empty)) == 0);
    show(empty, "Test { iteration: 0, result: 0 }");
    drop(empty);
    jet_test_suite_install(address(first));
    assert(first->refs == 2);
    Suite *old = snapshot();
    assert(old->iteration == 0 && old->result == 0 && old->runner == address(first));
    Suite *copy = (Suite *)(uintptr_t)jet_rt_handle_clone_jet_test_suite(address(old));
    assert(copy != old && memcmp(copy, old, sizeof(*old)) == 0 && first->refs == 2);
    jet_test_suite_install(address(second));
    assert(first->refs == 2 && second->refs == 2 && jet_test_suite_status() == 0);
    --first->refs; // release the original counted owner as generated glue does
    assert(first->refs == 1); // permanent pin keeps both old snapshots callable
    assert(jet_test_suite_run(address(old)) == 17);
    assert(old->iteration == 0 && old->result == 0 && old->runner == address(first));
    Suite *after = snapshot();
    assert(after->iteration == INT64_MIN && after->result == 17 && after->runner == address(second));
    show(after, "Test { iteration: -9223372036854775808, result: 17 }");
    assert(jet_test_suite_run(address(copy)) == 17 && jet_test_suite_status() == 17);
    assert(jet_test_suite_run(address(after)) == -3 && after->result == 17);
    Suite *latest = snapshot();
    show(latest, "Test { iteration: 9223372036854775807, result: -3 }");
    jet_test_suite_install(address(second));
    assert(second->refs == 3 && jet_test_suite_status() == 0);
    assert(jet_test_suite_run(address(copy)) == 17);
    pthread_t thread;
    assert(pthread_create(&thread, NULL, worker, child) == 0);
    assert(pthread_join(thread, NULL) == 0);
    assert(jet_test_suite_status() == 17 && child->refs == 2);
    assert(jet_rt_test_suite_fixture_state() != 0);
    drop(old); drop(copy); drop(after); drop(latest);
    assert(first->refs == 1 && second->refs == 3 && child->refs == 2);
    // Warm TLS already exists; every counted carrier must return exactly 24B.
    jet_rt_program_allocator_configure(1, 0);
    Word live = jet_rt_program_allocator_fact(2);
    Suite *counted = snapshot();
    assert(jet_rt_program_allocator_fact(2) == live + 24);
    Suite *counted_copy = (Suite *)(uintptr_t)jet_rt_handle_clone_jet_test_suite(address(counted));
    assert(jet_rt_program_allocator_fact(2) == live + 48);
    drop(counted); drop(counted_copy);
    assert(jet_rt_program_allocator_fact(2) == live);
    // The first worker warmed the process thread registry. A second worker
    // must release its counted TCB, dynamic table, and suite state at detach.
    assert(pthread_create(&thread, NULL, worker, child) == 0);
    assert(pthread_join(thread, NULL) == 0);
    assert(jet_rt_program_allocator_fact(2) == live && child->refs == 3);
    // Rearming is forbidden here: the destructor frees one state exactly once.
    jet_rt_thread_detach();
    assert(first->refs == 1 && second->refs == 3 && child->refs == 3);
    jet_rt_program_allocator_restore(0, 0);
    puts("test suite: ok");
    return 0;
}
