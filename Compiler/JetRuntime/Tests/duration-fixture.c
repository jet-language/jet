/* Current Jet-only runtime C ABI witness: no Rust build/oracle. Borrowed exact
 * Int inputs, an owned ns exact-Int word (CAbiRoutes `jet_int_from_i64(d.ns)`)
 * or owned RangeError8 output, and all six declared units. */
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

typedef int64_t H;
typedef struct { const char *data; uint64_t len, cap; } Text;
typedef struct { uint64_t refs, retired_next, negative; uint32_t *limbs; uint64_t len, cap; } IntNode;
_Static_assert(sizeof(IntNode) == 48, "canonical base-1e9 exact Int");
extern H jet_duration_from_int(H, H, H *, H *);
extern H jet_int_owned_to_i64(H);
extern void jet_rt_int_drop(H);
extern Text *jet_rt_record_get_range_error_reason(H);
extern void jet_rt_handle_drop_range_error(H);
extern void jet_rt_string_drop(Text *);
static const int64_t scales[] = {1, 1000, 1000000, 1000000000, 60000000000LL, 3600000000000LL};

/* Construct magnitude directly from integers, not a floating-point oracle. */
static H boxed(IntNode *node, uint32_t limbs[3], uint64_t magnitude, int negative) {
    *node = (IntNode){17, 0, (uint64_t)negative, limbs, 0, 3};
    do {
        limbs[node->len++] = (uint32_t)(magnitude % UINT64_C(1000000000));
        magnitude /= UINT64_C(1000000000);
    } while (magnitude);
    assert(((uintptr_t)node >> 62) == 0);
    return (H)((uintptr_t)node | (UINT64_C(1) << 62));
}
static void accepted(H value, int unit, int64_t ns) {
    H output = 71, error = 73;
    assert(jet_duration_from_int(value, unit, &output, &error) == 1);
    assert(error == 73); /* Only the selected out pointer is written. */
    assert(jet_int_owned_to_i64(output) == ns);
    jet_rt_int_drop(output);
}
static void rejected(H value, int unit) {
    H output = 71, error = 73;
    assert(jet_duration_from_int(value, unit, &output, &error) == 0);
    assert(output == 71);
    Text *reason = jet_rt_record_get_range_error_reason(error);
    static const char expected[] = "duration is outside the supported range";
    assert(reason->len == sizeof expected - 1);
    assert(memcmp(reason->data, expected, sizeof expected - 1) == 0);
    jet_rt_string_drop(reason);
    jet_rt_handle_drop_range_error(error);
}
static void invalid_unit(void) {
    int fds[2];
    assert(pipe(fds) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (!child) {
        assert(close(fds[0]) == 0);
        assert(dup2(fds[1], STDERR_FILENO) == STDERR_FILENO);
        assert(close(fds[1]) == 0);
        H output = 71, error = 73;
        (void)jet_duration_from_int(0, 6, &output, &error);
        _exit(99);
    }
    assert(close(fds[1]) == 0);
    char report[4096] = {0};
    size_t length = 0;
    for (;;) {
        ssize_t count = read(fds[0], report + length, sizeof report - length - 1);
        assert(count >= 0);
        if (!count) break;
        length += (size_t)count;
        assert(length < sizeof report - 1);
    }
    assert(close(fds[0]) == 0);
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 70);
    assert(strstr(report, "native Int argument exceeds host range"));
    assert(strstr(report, "<core.prelude>"));
}
int main(void) {
    for (int unit = 0; unit < 6; ++unit) {
        accepted(0, unit, 0);
        accepted(1, unit, scales[unit]);
        accepted(-1, unit, -scales[unit]);
        int64_t high = INT64_MAX / scales[unit];
        int64_t low = INT64_MIN / scales[unit];
        IntNode node;
        uint32_t limbs[3] = {0};
        accepted(boxed(&node, limbs, (uint64_t)high, 0), unit, high * scales[unit]);
        assert(node.refs == 17 && node.retired_next == 0);
        uint64_t negative_magnitude = (uint64_t)(-(low + 1)) + 1;
        accepted(boxed(&node, limbs, negative_magnitude, 1), unit, low * scales[unit]);
        assert(node.refs == 17 && node.retired_next == 0);
        rejected(boxed(&node, limbs, (uint64_t)high + 1, 0), unit);
        assert(node.refs == 17 && node.retired_next == 0);
        rejected(boxed(&node, limbs, negative_magnitude + 1, 1), unit);
        assert(node.refs == 17 && node.retired_next == 0);
    }
    invalid_unit();
    puts("CHECK OK — Duration: borrowed exact Int, six units, signed boundaries, owned RangeError, invalid-unit stop");
}
