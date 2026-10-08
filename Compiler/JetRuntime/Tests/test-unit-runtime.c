// Native-only fixture: verify the Jet outcome carrier and exact wire ownership.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <pthread.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void *jet_rt_test_unit_outcome(Text *, bool, bool, Text *, Text *);
extern Text *jet_rt_test_outcome_wire(void *);
extern void jet_rt_test_outcome_drop(void *);
extern bool jet_rt_test_finish_wire(Text *);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_test_capture_init(void);
extern void jet_rt_test_capture_begin(void);
extern Text *jet_rt_test_capture_take(void);
extern bool jet_rt_test_capture_print(Text *);
extern void jet_rt_test_print(Text *, bool);
extern void jet_rt_test_expect_fail_enter_scope(int64_t, Text *, Text *);
extern Text *jet_rt_test_expect_fail_leave_scope(int64_t);
extern void jet_rt_test_record_stop(Text *, Text *);
extern bool jet_rt_test_expect_fail_stopped_scope(int64_t *);
extern bool jet_rt_test_expect_fail_catch_scope(int64_t *);
extern Text *jet_rt_test_expect_fail_error(Text *, Text *, Text *, Text *);
extern void jet_rt_test_expect_fail_abort(void);
extern void jet_rt_test_timeout_enter(int64_t, int64_t);
extern bool jet_rt_test_timeout_leave(int64_t, int64_t *);
extern void jet_rt_test_timeout_abort(void);
extern Text *jet_rt_test_timeout_message(int64_t, int64_t);
extern void jet_rt_test_skip_scope(bool);
extern bool jet_rt_test_take_whole_skip(void);
extern void jet_rt_test_skip_abort(void);
extern void jet_rt_testing_record_failure(Text *, Text *);
extern bool jet_rt_testing_take_failure(Text **);
extern void jet_rt_testing_clear_failure(void);

static Text *text(const char *s, int64_t n) {
    Text *value = jet_rt_alloc(24, 8);
    value->ptr = n ? jet_rt_alloc(n, 1) : (unsigned char *)1;
    value->len = value->cap = n;
    if (n) memcpy(value->ptr, s, (size_t)n);
    return value;
}
static void drop_text(Text *value) {
    if (value->cap) jet_rt_free(value->ptr, value->cap, 1);
    jet_rt_free(value, 24, 8);
}
static Text *outcome(const char *name, bool expected, bool skip, const char *out, const char *err) {
    Text *borrowed = text(name, (int64_t)strlen(name));
    void *value = jet_rt_test_unit_outcome(borrowed, expected, skip,
        text(out, (int64_t)strlen(out)), err ? text(err, (int64_t)strlen(err)) : NULL);
    drop_text(borrowed); // The outcome must own a clone of the borrowed name.
    return jet_rt_test_outcome_wire(value);
}
static void expect_wire(Text *wire, const char *expected) {
    assert(wire->len == (int64_t)strlen(expected));
    assert(memcmp(wire->ptr, expected, (size_t)wire->len) == 0);
    drop_text(wire);
}
static void print_capture(const char *s) {
    Text *value = text(s, (int64_t)strlen(s));
    jet_rt_test_print(value, false);
    drop_text(value);
}
static void *capture_thread(void *arg) {
    int number = *(int *)arg;
    char line[32], expected[34];
    int n = snprintf(line, sizeof line, "thread-%d", number);
    assert(n > 0 && (size_t)n < sizeof line);
    snprintf(expected, sizeof expected, "%s\n", line);
    jet_rt_thread_attach();
    for (int i = 0; i < 100; i++) {
        jet_rt_test_capture_begin();
        print_capture(line);
        expect_wire(jet_rt_test_capture_take(), expected);
    }
    jet_rt_thread_detach();
    return NULL;
}
static void capture_fixtures(void) {
    Text *outside = text("outside", 7);
    assert(!jet_rt_test_capture_print(outside));
    drop_text(outside);
    jet_rt_test_capture_begin();
    print_capture("outer");
    jet_rt_test_capture_begin();
    print_capture("inner");
    jet_rt_test_capture_init(); // Repeated init cannot lose a live capture.
    expect_wire(jet_rt_test_capture_take(), "inner\n");
    expect_wire(jet_rt_test_capture_take(), "outer\n");
    for (int i = 0; i < 32; i++) jet_rt_test_capture_begin();
    print_capture("");
    expect_wire(jet_rt_test_capture_take(), "\n");
    for (int i = 1; i < 32; i++) expect_wire(jet_rt_test_capture_take(), "");
    // An unmatched nested begin is reclaimed by the TLS destructor at detach.
    jet_rt_test_capture_begin();
    print_capture("abandoned outer");
    jet_rt_test_capture_begin();
    print_capture("abandoned inner");
    jet_rt_thread_detach();
    jet_rt_thread_attach();
    outside = text("outside", 7);
    assert(!jet_rt_test_capture_print(outside));
    drop_text(outside);
    jet_rt_test_capture_begin();
    print_capture("parent");
    pthread_t threads[8];
    int numbers[8];
    for (int i = 0; i < 8; i++) {
        numbers[i] = i;
        assert(pthread_create(&threads[i], NULL, capture_thread, &numbers[i]) == 0);
    }
    for (int i = 0; i < 8; i++) assert(pthread_join(threads[i], NULL) == 0);
    expect_wire(jet_rt_test_capture_take(), "parent\n");
}
// Core.rs:678-860 policy witnesses against the Jet cells, not a C substitute.
static void scope_fixtures(void) {
    Text *code = text("E3010", 5), *message = text("index 99", 8);
    Text *wrong = text("E3001", 5), *needle = text("99", 2);
    Text *ordinary = text("__jet_test_ordinary_failure__", 29);
    int64_t found = -77, timer[2] = {-78, -79};
    Text *pair[2] = {(Text *)(uintptr_t)123, (Text *)(uintptr_t)456};
    assert(!jet_rt_testing_take_failure(pair));
    assert(pair[0] == (Text *)(uintptr_t)123 && pair[1] == (Text *)(uintptr_t)456);
    assert(!jet_rt_test_expect_fail_stopped_scope(&found) && found == -77);
    assert(!jet_rt_test_expect_fail_catch_scope(&found) && found == -77);
    assert(!jet_rt_test_timeout_leave(999, timer) && timer[0] == -78 && timer[1] == -79);

    assert(jet_rt_test_expect_fail_error(code, needle, code, message) == NULL);
    assert(jet_rt_test_expect_fail_error(NULL, NULL, code, message) == NULL);
    expect_wire(jet_rt_test_expect_fail_error(wrong, NULL, code, message),
                "expected a stop with E3001, got E3010: index 99");
    expect_wire(jet_rt_test_expect_fail_error(wrong, needle, code, message),
                "expected a stop with E3001 whose message contains \"99\", got E3010: index 99");
    Text *other_needle = text("absent", 6);
    expect_wire(jet_rt_test_expect_fail_error(NULL, other_needle, code, message),
                "expected a stop whose message contains \"absent\", got E3010: index 99");
    expect_wire(jet_rt_test_expect_fail_error(NULL, NULL, NULL, NULL),
                "expected this region to fail, but it passed");
    expect_wire(jet_rt_test_expect_fail_error(code, NULL, NULL, NULL),
                "expected this region to stop with E3010, but it passed");
    expect_wire(jet_rt_test_expect_fail_error(NULL, needle, NULL, NULL),
                "expected a stop whose message contains \"99\", but it passed");
    expect_wire(jet_rt_test_expect_fail_error(code, needle, NULL, NULL),
                "expected this region to stop with E3010 whose message contains \"99\", but it passed");
    Text *quoted = text("\"\n\\", 3);
    expect_wire(jet_rt_test_expect_fail_error(NULL, quoted, NULL, NULL),
                "expected a stop whose message contains \"\\\"\\n\\\\\", but it passed");
    drop_text(quoted);
    drop_text(other_needle);
    expect_wire(jet_rt_test_timeout_message(INT64_MAX, INT64_MIN),
                "timeout: region took 9223372036854775807ns, limit was -9223372036854775808ns");

    jet_rt_test_timeout_enter(100, -1); // depth zero retained by inner catch.
    jet_rt_test_expect_fail_enter_scope(0, code, needle);
    assert(jet_rt_test_expect_fail_leave_scope(0) == NULL); // frame stays.
    jet_rt_test_timeout_enter(101, 5); // depth one also retained.
    jet_rt_test_expect_fail_enter_scope(7, code, needle);
    jet_rt_test_timeout_enter(102, 6); // depth two pruned by catch.
    jet_rt_test_expect_fail_enter_scope(8, ordinary, NULL);
    jet_rt_test_record_stop(code, message);
    jet_rt_test_record_stop(wrong, ordinary); // first stop wins on all frames.
    assert(jet_rt_test_expect_fail_stopped_scope(&found) && found == 7);
    assert(jet_rt_test_expect_fail_catch_scope(&found) && found == 7);
    expect_wire(jet_rt_test_expect_fail_leave_scope(7), "E3010");
    assert(jet_rt_test_expect_fail_leave_scope(8) == NULL); // child truncated.
    assert(!jet_rt_test_expect_fail_stopped_scope(&found)); // parent stop reset.
    assert(!jet_rt_test_timeout_leave(102, timer));
    assert(jet_rt_test_timeout_leave(101, timer) && timer[0] >= 0 && timer[1] == 5);
    assert(jet_rt_test_timeout_leave(100, timer) && timer[0] >= 0 && timer[1] == -1);
    jet_rt_test_record_stop(code, message);
    assert(jet_rt_test_expect_fail_stopped_scope(&found) && found == 0); // zero is Some.
    assert(jet_rt_test_expect_fail_catch_scope(&found) && found == 0);
    expect_wire(jet_rt_test_expect_fail_leave_scope(0), "E3010");
    // Duplicate IDs: latest completed result precedes latest stopped frame.
    jet_rt_test_expect_fail_enter_scope(9, code, NULL);
    jet_rt_test_expect_fail_enter_scope(9, code, NULL);
    jet_rt_test_record_stop(code, message);
    assert(jet_rt_test_expect_fail_catch_scope(&found) && found == 9);
    expect_wire(jet_rt_test_expect_fail_leave_scope(9), "E3010");
    assert(jet_rt_test_expect_fail_leave_scope(9) == NULL);
    jet_rt_test_record_stop(code, message);
    expect_wire(jet_rt_test_expect_fail_leave_scope(9), "E3010");
    jet_rt_test_timeout_enter(3, 1);
    jet_rt_test_timeout_enter(3, 2);
    assert(jet_rt_test_timeout_leave(3, timer) && timer[1] == 2);
    assert(jet_rt_test_timeout_leave(3, timer) && timer[1] == 1);
    jet_rt_test_timeout_enter(3, 4);
    jet_rt_test_timeout_abort();
    assert(!jet_rt_test_timeout_leave(3, timer));
    jet_rt_test_expect_fail_enter_scope(10, code, NULL);
    jet_rt_test_record_stop(code, message);
    assert(jet_rt_test_expect_fail_catch_scope(&found));
    jet_rt_test_expect_fail_abort();
    assert(jet_rt_test_expect_fail_leave_scope(10) == NULL);
    assert(!jet_rt_test_expect_fail_stopped_scope(&found));
    jet_rt_test_skip_scope(false);
    assert(!jet_rt_test_take_whole_skip());
    jet_rt_test_skip_scope(true);
    jet_rt_test_skip_scope(false);
    assert(jet_rt_test_take_whole_skip());
    assert(!jet_rt_test_take_whole_skip());
    jet_rt_test_skip_scope(true);
    jet_rt_test_skip_abort();
    assert(!jet_rt_test_take_whole_skip());
    jet_rt_testing_record_failure(code, message);
    jet_rt_testing_record_failure(wrong, ordinary); // replacement owns clones.
    assert(jet_rt_testing_take_failure(pair));
    expect_wire(pair[0], "E3001");
    expect_wire(pair[1], "__jet_test_ordinary_failure__");
    jet_rt_testing_record_failure(code, message);
    jet_rt_testing_clear_failure();
    assert(!jet_rt_testing_take_failure(pair));
    // Capture take must not delete any of the non-capture cells.
    jet_rt_test_skip_scope(true);
    jet_rt_test_capture_begin();
    expect_wire(jet_rt_test_capture_take(), "");
    assert(jet_rt_test_take_whole_skip());
    // Abandon every owned row + capture; the single TLS destructor reclaims all.
    jet_rt_test_expect_fail_enter_scope(11, code, needle);
    jet_rt_test_record_stop(code, message);
    jet_rt_test_timeout_enter(12, 1);
    jet_rt_testing_record_failure(code, message);
    jet_rt_test_skip_scope(true);
    jet_rt_test_capture_begin();
    print_capture("abandoned scope");
    drop_text(code); drop_text(message); drop_text(wrong); drop_text(needle); drop_text(ordinary);
    jet_rt_thread_detach();
    jet_rt_thread_attach();
    found = -77;
    assert(!jet_rt_test_expect_fail_stopped_scope(&found) && found == -77);
    assert(!jet_rt_test_timeout_leave(12, timer));
    assert(!jet_rt_test_take_whole_skip());
    assert(!jet_rt_testing_take_failure(pair));
}

static void *scope_thread(void *unused) {
    (void)unused;
    jet_rt_thread_attach();
    for (int i = 0; i < 100; i++) scope_fixtures();
    jet_rt_thread_detach();
    return NULL;
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    jet_rt_thread_init();
    jet_rt_test_capture_init();
    if (argc == 2 && strcmp(argv[1], "capture") == 0) {
        capture_fixtures();
        puts("unit capture OK");
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "scopes") == 0) {
        scope_fixtures();
        jet_rt_test_skip_scope(true); // parent cells remain thread-local.
        pthread_t threads[8];
        for (int i = 0; i < 8; i++) assert(pthread_create(&threads[i], NULL, scope_thread, NULL) == 0);
        for (int i = 0; i < 8; i++) assert(pthread_join(threads[i], NULL) == 0);
        assert(jet_rt_test_take_whole_skip());
        puts("unit scope cells OK");
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "wire") == 0) {
        expect_wire(outcome("ok", false, false, "", NULL), "1 2 0 0 -\nok");
        expect_wire(outcome("bad", false, false, "line\n", "diagnostic"), "0 3 5 10 -\nbadline\ndiagnostic");
        expect_wire(outcome("expected", true, false, "", "stop"), "3 8 0 4 -\nexpectedstop");
        expect_wire(outcome("surprise", true, false, "", NULL), "2 8 0 0 -\nsurprise");
        expect_wire(outcome("skip", false, true, "", NULL), "5 4 0 0 -\nskip");
        expect_wire(outcome("skip expected", true, true, "", NULL), "7 13 0 0 -\nskip expected");
        Text *name = text("drop", 4);
        jet_rt_test_outcome_drop(jet_rt_test_unit_outcome(name, false, false, text("", 0), NULL));
        drop_text(name);
        puts("unit wire OK");
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "empty") == 0) {
        Text *wire = text("", 0);
        bool ok = jet_rt_test_finish_wire(wire);
        drop_text(wire);
        return ok ? 0 : 1;
    }
    if (argc == 2 && strcmp(argv[1], "property") == 0) {
        Text *wire = text("1 1 0 0 1\na", 11);
        jet_rt_test_finish_wire(wire);
        return 99;
    }
    if (argc == 2 && strcmp(argv[1], "malformed") == 0) {
        Text *wire = text("1 4 0 0 -\na", 11);
        jet_rt_test_finish_wire(wire);
        return 99;
    }
    Text *rows[] = {
        outcome("pass", false, false, "pass-out\n", NULL),
        outcome("fail", false, false, "fail-out\n", "fail diagnostic"),
        outcome("expected", true, false, "expected-out\n", "expected diagnostic"),
        outcome("unexpected", true, false, "unexpected-out\n", NULL),
        outcome("skip", false, true, "skip-out\n", NULL),
        outcome("quoted \"\x01\x7f\xc2\x80\xe4\xb8\xad", false, false, "UTF8 \xc3\xa9\xe4\xb8\xad\n", NULL),
    };
    int64_t length = 0;
    for (size_t i = 0; i < sizeof rows / sizeof rows[0]; i++) length += rows[i]->len;
    Text *wire = text("", 0);
    wire->ptr = jet_rt_alloc(length, 1);
    wire->len = wire->cap = length;
    int64_t at = 0;
    for (size_t i = 0; i < sizeof rows / sizeof rows[0]; i++) {
        memcpy(wire->ptr + at, rows[i]->ptr, (size_t)rows[i]->len);
        at += rows[i]->len;
        drop_text(rows[i]);
    }
    if (argc == 2 && strcmp(argv[1], "nonunicode-json") == 0) {
        const char invalid[] = {(char)0xff, 0};
        assert(setenv("JET_TEST_JSON", invalid, 1) == 0);
    }
    bool ok = jet_rt_test_finish_wire(wire);
    drop_text(wire); // finish only borrows the payload, matching the host protocol.
    return ok ? 0 : 1;
}
