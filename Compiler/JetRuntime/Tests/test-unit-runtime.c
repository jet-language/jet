// Native-only fixture: verify the Jet outcome carrier and exact wire ownership.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void *jet_rt_test_unit_outcome(Text *, bool, bool, Text *, Text *);
extern Text *jet_rt_test_outcome_wire(void *);
extern void jet_rt_test_outcome_drop(void *);
extern bool jet_rt_test_finish_wire(Text *);

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
int main(int argc, char **argv) {
    jet_rt_alloc_init();
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
