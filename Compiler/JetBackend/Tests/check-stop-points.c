// Scripted session stop rail for the stop-points fixture (Fixtures/StopPoints.jet),
// linked with stop-points.o and the compiled runtime by check-native-fixtures.mjs
// with -Wl,--wrap=jet_rt_string_from_static,--wrap=jet_rt_string_drop.
//
// JET_FX_STOP_AT=n: an interrupt becomes pending just before the n-th
// jet_rt_session_poll (Check). JET_FX_SIGNAL_AT_PROPAGATE=n: it becomes pending
// during the n-th jet_rt_session_propagate, after its callee returned, so that
// Propagate (delivered-only) still reads 0. A Check delivers a pending interrupt
// unless a Shield is open; Propagate reads only a delivered stop, masked inside
// a Shield. At exit stderr gets "polls=<n> made=<m> dropped=<d>": the Strings the
// program made from static text and how many of exactly those it dropped.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static uint64_t polls, propagates, pending, delivered, shield_depth;
static long stop_at = -1, signal_at = -1;
static int ready;
static void *made[64];
static int made_count, dropped_count;

static void report(void) {
    fprintf(stderr, "polls=%llu made=%d dropped=%d\n", (unsigned long long)polls, made_count, dropped_count);
}

static void setup(void) {
    if (ready) return;
    ready = 1;
    const char *at = getenv("JET_FX_STOP_AT");
    if (at) stop_at = atol(at);
    const char *signal = getenv("JET_FX_SIGNAL_AT_PROPAGATE");
    if (signal) signal_at = atol(signal);
    atexit(report);
}

uint64_t jet_rt_session_poll(void) {
    setup();
    polls += 1;
    if ((long)polls == stop_at) pending = 1;
    if (shield_depth > 0) return 0;
    if (pending) {
        pending = 0;
        delivered = 1;
    }
    return delivered;
}

uint64_t jet_rt_session_propagate(void) {
    setup();
    propagates += 1;
    if ((long)propagates == signal_at) pending = 1;
    return shield_depth > 0 ? 0 : delivered;
}

uint64_t jet_rt_session_status(void) {
    setup();
    return delivered;
}

void jet_rt_session_shield_enter(void) { shield_depth += 1; }

void jet_rt_session_shield_leave(void) {
    if (shield_depth == 0) abort();
    shield_depth -= 1;
}

void *__real_jet_rt_string_from_static(const uint8_t *bytes, uint64_t len);
void __real_jet_rt_string_drop(void *string);

void *__wrap_jet_rt_string_from_static(const uint8_t *bytes, uint64_t len) {
    setup();
    void *string = __real_jet_rt_string_from_static(bytes, len);
    if (made_count < 64) made[made_count] = string;
    made_count += 1;
    return string;
}

void __wrap_jet_rt_string_drop(void *string) {
    for (int i = 0; i < made_count && i < 64; i += 1) {
        if (made[i] == string) {
            made[i] = NULL;
            dropped_count += 1;
        }
    }
    __real_jet_rt_string_drop(string);
}
