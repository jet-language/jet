#define _POSIX_C_SOURCE 200809L
// Link real Jet runtime with -pthread -Wl,--wrap=jet_rt_free. The wrapper
// observes releases and always forwards; no clock, worker or allocator mocks.
#include <assert.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
extern uintptr_t jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(uintptr_t, int64_t, int64_t);
extern void __real_jet_rt_free(uintptr_t, int64_t, int64_t);
extern uintptr_t jet_rt_native_realtime_start(int64_t, int64_t, uintptr_t,
    uintptr_t, uintptr_t, uintptr_t, uintptr_t);
extern uintptr_t jet_rt_native_realtime_worker(uintptr_t);
extern int64_t jet_rt_native_realtime_deadline_offset(int64_t, int64_t,
    int64_t, int64_t, uintptr_t);
extern uintptr_t jet_rt_handle_clone_jet_realtime_stream(uintptr_t);
extern void jet_rt_handle_drop_jet_realtime_stream(uintptr_t);
extern void jet_rt_cancel(uintptr_t);
extern bool jet_rt_is_cancelled(uintptr_t);
extern int64_t jet_rt_next_deadline(uintptr_t);
extern uintptr_t jet_rt_receipt(uintptr_t);
extern uintptr_t jet_rt_handle_clone_jet_realtime_receipt(uintptr_t);
extern void jet_rt_handle_drop_jet_realtime_receipt(uintptr_t);
extern bool jet_rt_handle_eq_jet_realtime_receipt(uintptr_t, uintptr_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern uintptr_t jet_rt_record_new_jet_realtime_receipt(int64_t, int64_t, int64_t,
    int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_requested_rate_hz(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_requested_frames(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_completed_callbacks(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_completed_frames(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_missed(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_max_lateness_ns(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_start_identity(uintptr_t);
extern int64_t jet_rt_record_get_jet_realtime_receipt_end_identity(uintptr_t);
extern int64_t jet_time_raw_host_monotonic_ns(void);
extern uintptr_t jet_rt_time_monotonic_provider_set(uintptr_t);
extern void jet_rt_panic_message(uintptr_t, int64_t);
extern uintptr_t jet_rt_native_realtime_ring_new(int64_t);
extern bool jet_rt_native_realtime_ring_publish(uintptr_t, uintptr_t, int64_t);
extern bool jet_rt_native_realtime_ring_publish_zeroed(uintptr_t);
extern bool jet_rt_native_realtime_ring_pop(uintptr_t, uintptr_t, uintptr_t);
extern void jet_rt_handle_drop_jet_realtime_sample_ring(uintptr_t);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
struct List { uint64_t len, cap; uintptr_t data; };
_Static_assert(sizeof(struct List) == 24, "borrowed List24<Float>");
static uintptr_t watched[16];
static unsigned freed[16], watch_count;
static pthread_mutex_t observer = PTHREAD_MUTEX_INITIALIZER;
void __wrap_jet_rt_free(uintptr_t pointer, int64_t size, int64_t alignment) {
    assert(pthread_mutex_lock(&observer) == 0);
    for (unsigned i = 0; i < watch_count; ++i)
        if (watched[i] == pointer) assert(++freed[i] == 1);
    assert(pthread_mutex_unlock(&observer) == 0);
    __real_jet_rt_free(pointer, size, alignment);
}
static void watch(uintptr_t pointer) {
    assert(pthread_mutex_lock(&observer) == 0);
    assert(pointer && watch_count < 16);
    for (unsigned i = 0; i < watch_count; ++i) assert(watched[i] != pointer);
    watched[watch_count++] = pointer;
    assert(pthread_mutex_unlock(&observer) == 0);
}
static void releases(void) {
    assert(pthread_mutex_lock(&observer) == 0);
    assert(watch_count > 0);
    for (unsigned i = 0; i < watch_count; ++i) assert(freed[i] == 1);
    memset(watched, 0, sizeof(watched)); memset(freed, 0, sizeof(freed));
    watch_count = 0;
    assert(pthread_mutex_unlock(&observer) == 0);
}
static uint64_t word(uintptr_t pointer, unsigned offset) {
    uint64_t result;
    memcpy(&result, (const unsigned char *)pointer + offset, sizeof(result));
    return result;
}
static __uint128_t sat_mul(__uint128_t a, __uint128_t b) {
    __uint128_t max = ~(__uint128_t)0;
    return b && a > max / b ? max : a * b;
}
static int64_t offset_oracle(int64_t rate, int64_t frames, uint64_t low, uint64_t high) {
    if (rate <= 0 || frames <= 0) return 0;
    __uint128_t index = ((__uint128_t)high << 64) | low;
    __uint128_t numerator = sat_mul(sat_mul((uint64_t)frames, 1000000000), index);
    __uint128_t quotient = numerator / (uint64_t)rate;
    return quotient > INT64_MAX ? INT64_MAX : (int64_t)quotient;
}
static void deadlines(void) {
    uintptr_t scratch = jet_rt_alloc(1056, 8);
    const int64_t rates[] = {1, 48000, INT64_MAX};
    const int64_t frames[] = {1, 256, INT64_MAX};
    const uint64_t lows[] = {0, 1, 2, 3, INT64_MAX, UINT64_MAX};
    const uint64_t highs[] = {0, 1, UINT64_MAX};
    for (unsigned r = 0; r < 3; ++r)
        for (unsigned f = 0; f < 3; ++f)
            for (unsigned l = 0; l < 6; ++l)
                for (unsigned h = 0; h < 3; ++h)
                    assert(jet_rt_native_realtime_deadline_offset(rates[r], frames[f],
                        (int64_t)lows[l], (int64_t)highs[h], scratch) ==
                        offset_oracle(rates[r], frames[f], lows[l], highs[h]));
    assert(jet_rt_native_realtime_deadline_offset(48000, 256, 1, 0, scratch) == 5333333);
    assert(jet_rt_native_realtime_deadline_offset(48000, 256, 2, 0, scratch) == 10666666);
    assert(jet_rt_native_realtime_deadline_offset(48000, 256, 3, 0, scratch) == 16000000);
    assert(jet_rt_native_realtime_deadline_offset(0, 256, 1, 0, scratch) == 0);
    jet_rt_free(scratch, 1056, 8);
    puts("Realtime deadlines: rational drift, 128-bit index and saturation ok");
}
static void ring(void) {
    uintptr_t value = jet_rt_native_realtime_ring_new(2);
    uint64_t samples[] = {UINT64_C(0x8000000000000000), UINT64_C(0x7ff8000000000123)};
    uintptr_t data = 0xfeed;
    int64_t length = 0xbeef;
    assert(!jet_rt_native_realtime_ring_pop(value, (uintptr_t)&data, (uintptr_t)&length));
    assert(data == 0xfeed && length == 0xbeef);
    assert(!jet_rt_native_realtime_ring_publish(value, UINTPTR_MAX, 1));
    assert(jet_rt_native_realtime_ring_publish(value, (uintptr_t)samples, 2));
    assert(jet_rt_native_realtime_ring_publish_zeroed(value));
    assert(!jet_rt_native_realtime_ring_publish(value, UINTPTR_MAX, 2));
    assert(jet_rt_native_realtime_ring_pop(value, (uintptr_t)&data, (uintptr_t)&length));
    assert(length == 2 && memcmp((const void *)data, samples, 16) == 0);
    jet_rt_free(data, 16, 8);
    assert(jet_rt_native_realtime_ring_pop(value, (uintptr_t)&data, (uintptr_t)&length));
    assert(length == 2 && word(data, 0) == 0 && word(data, 8) == 0);
    jet_rt_free(data, 16, 8);
    // Sequence wrap must not become signed overflow or a false full condition.
    ((uint64_t *)value)[1] = UINT64_MAX; ((uint64_t *)value)[2] = UINT64_MAX;
    assert(jet_rt_native_realtime_ring_publish(value, (uintptr_t)samples, 2));
    assert(jet_rt_native_realtime_ring_pop(value, (uintptr_t)&data, (uintptr_t)&length));
    assert(memcmp((const void *)data, samples, 16) == 0);
    jet_rt_free(data, 16, 8);
    watch(value); watch(word(value, 24)); watch(word(value, 32));
    jet_rt_handle_drop_jet_realtime_sample_ring(value);
    releases();
    puts("Realtime ring: two slots, length rejection, Float bits, zero fill, wrap and exact frees ok");
}
struct Statistics {
    _Atomic unsigned ready, calls, finalized;
    _Atomic uint64_t skipped;
    _Atomic uintptr_t stream;
};
struct Callback { struct Statistics *stats; unsigned mode; uintptr_t buffer; };
static void invoke(struct Callback *callback, const struct List *buffer) {
    while (!atomic_load_explicit(&callback->stats->ready, memory_order_acquire)) sched_yield();
    assert(buffer->len == 256 && buffer->cap == 256);
    if (!callback->buffer) callback->buffer = buffer->data;
    assert(callback->buffer == buffer->data);
    for (unsigned i = 0; i < 256; ++i) assert(word(buffer->data, i * 8) == 0);
    unsigned count = atomic_fetch_add_explicit(&callback->stats->calls, 1, memory_order_relaxed);
    if (callback->mode == 1) {
        static const char message[] = "realtime witness panic";
        jet_rt_panic_message((uintptr_t)message, sizeof(message) - 1);
    } else if (callback->mode == 2) {
        jet_rt_cancel(atomic_load_explicit(&callback->stats->stream, memory_order_acquire));
    } else if (count == 0) {
        atomic_store_explicit(&callback->stats->skipped, 1, memory_order_release);
    }
}
struct RingThread { uintptr_t ring; };
static void *ring_writer(void *argument) {
    jet_rt_thread_attach();
    uintptr_t ring = ((struct RingThread *)argument)->ring;
    for (uint64_t i = 1; i <= 1000; ++i) {
        uint64_t bits[] = {i, ~i};
        while (!jet_rt_native_realtime_ring_publish(ring, (uintptr_t)bits, 2))
            sched_yield();
    }
    jet_rt_thread_detach();
    return NULL;
}
static void *ring_reader(void *argument) {
    jet_rt_thread_attach();
    uintptr_t ring = ((struct RingThread *)argument)->ring;
    for (uint64_t i = 1; i <= 1000; ++i) {
        uintptr_t data;
        int64_t length;
        while (!jet_rt_native_realtime_ring_pop(ring, (uintptr_t)&data, (uintptr_t)&length))
            sched_yield();
        assert(length == 2 && word(data, 0) == i && word(data, 8) == ~i);
        jet_rt_free(data, 16, 8);
    }
    jet_rt_thread_detach();
    return NULL;
}
static void concurrent_ring(void) {
    struct RingThread context = {jet_rt_native_realtime_ring_new(2)};
    pthread_t writer, reader;
    assert(pthread_create(&writer, NULL, ring_writer, &context) == 0);
    assert(pthread_create(&reader, NULL, ring_reader, &context) == 0);
    assert(pthread_join(writer, NULL) == 0 && pthread_join(reader, NULL) == 0);
    watch(context.ring); watch(word(context.ring, 24)); watch(word(context.ring, 32));
    jet_rt_handle_drop_jet_realtime_sample_ring(context.ring);
    releases();
}
static void receipt_carriers(void) {
    const int64_t values[] = {INT64_MAX, INT64_MIN, 3, 4, 5, 6, 7, 8};
    int64_t words[8];
    for (unsigned i = 0; i < 8; ++i) words[i] = jet_int_owned_from_i64(values[i]);
    uintptr_t receipt = jet_rt_record_new_jet_realtime_receipt(words[0], words[1],
        words[2], words[3], words[4], words[5], words[6], words[7]);
    for (unsigned i = 0; i < 8; ++i) jet_rt_int_drop(words[i]);
    int64_t (*getters[])(uintptr_t) = {
        jet_rt_record_get_jet_realtime_receipt_requested_rate_hz,
        jet_rt_record_get_jet_realtime_receipt_requested_frames,
        jet_rt_record_get_jet_realtime_receipt_completed_callbacks,
        jet_rt_record_get_jet_realtime_receipt_completed_frames,
        jet_rt_record_get_jet_realtime_receipt_missed,
        jet_rt_record_get_jet_realtime_receipt_max_lateness_ns,
        jet_rt_record_get_jet_realtime_receipt_start_identity,
        jet_rt_record_get_jet_realtime_receipt_end_identity};
    for (unsigned i = 0; i < 8; ++i) {
        assert(word(receipt, i * 8) == (uint64_t)values[i]);
        int64_t field = getters[i](receipt);
        assert(jet_int_owned_to_i64(field) == values[i]);
        jet_rt_int_drop(field);
    }
    watch(receipt);
    jet_rt_handle_drop_jet_realtime_receipt(receipt);
    releases();
    puts("Realtime receipt: native constructor and all eight canonical Int getters preserve i64 extrema");
}
static void release(struct Callback *callback) {
    atomic_fetch_add_explicit(&callback->stats->finalized, 1, memory_order_release);
    jet_rt_free((uintptr_t)callback, sizeof(*callback), 8);
}
static void pause_briefly(void) {
    struct timespec delay = {0, 1000000};
    (void)nanosleep(&delay, NULL);
}
static _Atomic unsigned clock_calls;
static int64_t clock_base;
// Real registered controlled-provider hook exercises the existing clock chain.
static uintptr_t controlled_clock(int64_t *output) {
    *output = clock_base + atomic_fetch_add_explicit(&clock_calls, 1, memory_order_relaxed);
    return 1;
}
static void stream_case(unsigned mode, bool last_drop) {
    struct Statistics stats = {0};
    struct Callback *callback = (void *)jet_rt_alloc(sizeof(*callback), 8);
    *callback = (struct Callback){&stats, mode, 0};
    int64_t sample = jet_time_raw_host_monotonic_ns();
    clock_base = jet_int_owned_to_i64(sample); jet_rt_int_drop(sample);
    atomic_store_explicit(&clock_calls, 0, memory_order_relaxed);
    uintptr_t previous = jet_rt_time_monotonic_provider_set((uintptr_t)controlled_clock);
    uintptr_t stream = jet_rt_native_realtime_start(48000, 256, (uintptr_t)callback,
        (uintptr_t)invoke, (uintptr_t)release, (uintptr_t)jet_rt_native_realtime_worker,
        (uintptr_t)&stats.skipped);
    jet_rt_time_monotonic_provider_set(previous);
    assert(stream && word(stream, 0) >= 1 && word(stream, 8) == 1);
    assert(word(stream, 64) == (uint64_t)clock_base && word(stream, 192) == (uint64_t)(clock_base + 1));
    watch(stream); watch((uintptr_t)callback);
    watch(word(stream, 16)); watch(word(word(stream, 16), 64));
    watch(word(stream, 40)); watch(word(stream, 176)); watch(word(stream, 200));
    int64_t deadline = jet_rt_next_deadline(stream);
    assert(jet_int_owned_to_i64(deadline) == clock_base + 5333333);
    jet_rt_int_drop(deadline);
    atomic_store_explicit(&stats.stream, stream, memory_order_release);
    uintptr_t clone = jet_rt_handle_clone_jet_realtime_stream(stream);
    assert(clone == stream && word(stream, 8) == 2);
    jet_rt_handle_drop_jet_realtime_stream(clone);
    assert(!jet_rt_is_cancelled(stream));
    atomic_store_explicit(&stats.ready, 1, memory_order_release);
    if (mode == 0) {
        for (unsigned i = 0; atomic_load_explicit(&stats.calls, memory_order_acquire) < 3; ++i) {
            assert(i < 5000); pause_briefly();
        }
    } else {
        for (unsigned i = 0; !atomic_load_explicit(&stats.finalized, memory_order_acquire); ++i) {
            assert(i < 5000); pause_briefly();
        }
        assert(jet_rt_is_cancelled(stream));
    }
    if (!last_drop) {
        jet_rt_cancel(stream);
        assert(jet_rt_is_cancelled(stream));
        uintptr_t receipt = jet_rt_receipt(stream);
        uintptr_t copy = jet_rt_handle_clone_jet_realtime_receipt(receipt);
        assert(copy != receipt && jet_rt_handle_eq_jet_realtime_receipt(copy, receipt));
        for (unsigned i = 0; i < 8; ++i) {
            ((uint64_t *)copy)[i] ^= 1;
            assert(!jet_rt_handle_eq_jet_realtime_receipt(copy, receipt));
            ((uint64_t *)copy)[i] ^= 1;
        }
        unsigned calls = atomic_load_explicit(&stats.calls, memory_order_acquire);
        assert(word(receipt, 0) == 48000 && word(receipt, 8) == 256);
        assert(word(receipt, 16) == (mode == 0 ? calls - 1 : 0));
        assert(word(receipt, 24) == word(receipt, 16) * 256);
        assert(word(receipt, 32) <= calls);
        assert(word(receipt, 48) == (uint64_t)(clock_base + 1));
        assert(word(receipt, 56) >= word(receipt, 48));
        jet_rt_handle_drop_jet_realtime_receipt(copy);
        jet_rt_handle_drop_jet_realtime_receipt(receipt);
    }
    jet_rt_handle_drop_jet_realtime_stream(stream);
    assert(atomic_load_explicit(&stats.finalized, memory_order_acquire) == 1);
    assert(atomic_load_explicit(&clock_calls, memory_order_relaxed) == 2); // no inheritance
    unsigned calls = atomic_load_explicit(&stats.calls, memory_order_acquire);
    pause_briefly();
    assert(atomic_load_explicit(&stats.calls, memory_order_acquire) == calls);
    releases();
}
int main(void) {
    jet_rt_thread_attach();
    deadlines(); ring(); concurrent_ring(); receipt_carriers();
    stream_case(0, false); stream_case(0, true);
    puts("Realtime lifecycle: borrowed reusable buffer, clock selection, skip accounting, clone, cancel, last drop and joined exact frees ok");
    stream_case(1, false); stream_case(2, false);
    puts("Realtime panic and self-join: stop caught, cancellation published and callback release once ok");
    jet_rt_thread_detach();
    return 0;
}
