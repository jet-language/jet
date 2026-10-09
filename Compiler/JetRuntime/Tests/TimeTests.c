// Compose with the canonical L4/L7/L8 object; this fixture is word-U8 epoch.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void jet_time_raw_wall_sample(int64_t *, int64_t *, uint8_t *);
extern int64_t jet_time_raw_host_monotonic_ns(void);
extern int64_t jet_time_raw_wall_override_ms(int64_t *);
extern int64_t jet_time_raw_world_wall_ms(int64_t *);
extern int64_t jet_time_raw_world_monotonic_ns(int64_t *);
extern int64_t jet_time_raw_controlled_monotonic_ns(int64_t *);
extern int64_t jet_rt_time_monotonic_provider_set(int64_t);
extern int64_t jet_time_raw_zone_env(Text *, Text **);
extern int64_t jet_time_raw_zone_file_bytes(Text *, uint64_t **, int64_t *);
extern int64_t jet_time_raw_embedded_zone_bytes(Text *, uint64_t **, int64_t *);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_free(void *, int64_t, int64_t);
static Text borrowed(const char *value) { return (Text){(unsigned char *)value, (int64_t)strlen(value), 0}; }
static int64_t max_monotonic(int64_t *out) { *out = INT64_MAX; return 0x101; }
static int64_t absent_monotonic(int64_t *out) { (void)out; return 0x100; }
int main(void) {
    static char *argv[] = {"time-fixture", NULL};
    static char zone[] = "TZDIR=raw_\xff";
    static char *env[] = {"JET_PROVE_REPLAY_TIME_MS=not-a-number", "LEX_TEST_EPOCH=-17", zone, NULL};
    jet_rt_host_arguments(1, argv, env);
    int64_t sample = 17;
    assert(jet_time_raw_world_wall_ms(&sample) == 0 && sample == 17);
    assert(jet_time_raw_world_monotonic_ns(&sample) == 0 && sample == 17);
    int64_t previous = jet_rt_time_monotonic_provider_set((int64_t)(uintptr_t)&max_monotonic);
    assert(jet_time_raw_controlled_monotonic_ns(&sample) == 1);
    assert(jet_int_owned_to_i64(sample) == INT64_MAX);
    jet_rt_int_drop(sample);
    assert(jet_rt_time_monotonic_provider_set((int64_t)(uintptr_t)&absent_monotonic) == (int64_t)(uintptr_t)&max_monotonic);
    sample = 17;
    assert(jet_time_raw_controlled_monotonic_ns(&sample) == 0 && sample == 17);
    assert(jet_rt_time_monotonic_provider_set(previous) == (int64_t)(uintptr_t)&absent_monotonic);
    // A caller may discard its startup arrays after capture; reads use the
    // one runtime-owned snapshot, not std logical overlay state.
    zone[6] = 'X';
    int64_t override = 0;
    assert(jet_time_raw_wall_override_ms(&override) == 1 && override == -17);
    jet_rt_int_drop(override);
    Text key = borrowed("TZDIR"), *value = NULL;
    assert(jet_time_raw_zone_env(&key, &value) == 1);
    const unsigned char expected[] = {'r', 'a', 'w', '_', 0xef, 0xbf, 0xbd};
    assert(value->len == sizeof(expected) && memcmp(value->ptr, expected, sizeof(expected)) == 0);
    jet_rt_string_drop(value);
    key = borrowed("absent");
    assert(jet_time_raw_zone_env(&key, &value) == 0);
    struct timespec before, after;
    assert(clock_gettime(CLOCK_REALTIME, &before) == 0);
    int64_t seconds = 0, nanos = 0;
    uint8_t pre_epoch = 1;
    jet_time_raw_wall_sample(&seconds, &nanos, &pre_epoch);
    assert(clock_gettime(CLOCK_REALTIME, &after) == 0);
    int64_t decoded = jet_int_owned_to_i64(seconds);
    assert(!pre_epoch && decoded >= before.tv_sec && decoded <= after.tv_sec && nanos >= 0 && nanos < 1000000000);
    jet_rt_int_drop(seconds);
    jet_rt_int_drop(nanos);
    int64_t first = jet_time_raw_host_monotonic_ns(), second = jet_time_raw_host_monotonic_ns();
    assert(jet_int_owned_to_i64(first) >= 0 && jet_int_owned_to_i64(second) >= jet_int_owned_to_i64(first));
    jet_rt_int_drop(first);
    jet_rt_int_drop(second);
    char path[] = "/tmp/jet-time-facts-XXXXXX";
    int fd = mkstemp(path);
    assert(fd >= 0);
    const unsigned char payload[] = {255, 0, 'B'};
    assert(write(fd, payload, sizeof(payload)) == sizeof(payload) && close(fd) == 0);
    key = borrowed(path);
    uint64_t *data = NULL;
    int64_t length = 0;
    assert(jet_time_raw_zone_file_bytes(&key, &data, &length) == 1 && length == 3);
    for (int i = 0; i < 3; ++i) assert(data[i] == payload[i]);
    jet_rt_free(data, length * 8, 8);
    assert(unlink(path) == 0 && jet_time_raw_zone_file_bytes(&key, &data, &length) == 0);
    key = borrowed("UTC");
    // Native embeds no zone bytes; wasm's embedded table is a different
    // platform fact, with the same Core search policy above either transport.
    assert(jet_time_raw_embedded_zone_bytes(&key, &data, &length) == 0);
    return 0;
}
