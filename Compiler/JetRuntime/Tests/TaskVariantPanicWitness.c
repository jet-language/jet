/* Real task runner and producers; this client defines no runtime providers. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdatomic.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

typedef struct { unsigned char *data; int64_t length, capacity; } Text;
#define WORD(p) ((int64_t)(uintptr_t)(p))
extern void jet_rt_host_arguments(int64_t, int64_t, int64_t);
extern int64_t jet_rt_control_new(void);
extern void jet_rt_control_enter(int64_t, int64_t);
extern void jet_rt_control_leave(int64_t);
extern void jet_rt_scheduler_control_drop(int64_t);
extern bool jet_rt_control_publish_stop(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_spawn(int64_t, Text *, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_task_join(int64_t, int64_t, int64_t);
extern void jet_rt_native_task_cancel(int64_t);
extern int64_t jet_scheduler_native_propagate(void);
extern void jet_scheduler_shield_enter(void);
extern void jet_scheduler_shield_leave(void);
extern int64_t jet_rt_record_new_dyn_null(void);
extern Text *jet_rt_record_get_dyn_text(int64_t);
extern void jet_rt_handle_drop_dyn(int64_t);
extern int64_t jet_rt_record_tag_jet_task_failure(int64_t);
extern Text *jet_rt_record_get_jet_task_failure_panicked(int64_t);
extern void jet_rt_handle_drop_jet_task_failure(int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_string_clone(Text *);
extern void jet_panic(Text *, int64_t, Text *);
extern void jet_rt_numeric_stop(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_rt_panic_overflow(void);
extern void jet_rt_panic_division_by_zero(void);
extern void jet_rt_panic_list_bounds(int64_t, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_mul(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t jet_rt_record_new_jet_async_policy(int64_t, int64_t);
extern int64_t jet_rt_record_new_encoding_error(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_encoding_cause(int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_watch_event(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_std_process_cmd(int64_t, int64_t);
extern int64_t jet_process_spec_stdin(int64_t, int64_t);
extern int64_t jet_process_spec_stdout(int64_t, int64_t);
extern int64_t jet_process_spec_stderr(int64_t, int64_t);
extern void jet_rt_handle_drop_process_spec(int64_t);
extern int64_t jet_rt_record_new_terminal_size(int64_t, int64_t);
extern int64_t jet_rt_record_new_terminal_policy(int64_t, int64_t);
extern int64_t jet_rt_record_new_text_width(int64_t, int64_t);
extern int64_t jet_string_slice(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_bits_new(void);
extern bool jet_bitset_add(int64_t, int64_t);
extern int64_t jet_bit_set_len(int64_t);
extern void jet_rt_handle_drop_jet_bit_set(int64_t);
extern int64_t jet_coll_counter(void);
extern int64_t jet_coll_counter_add(int64_t, int64_t, int64_t);
extern int64_t jet_coll_counter_inc(int64_t, int64_t);
extern int64_t jet_coll_counter_dec(int64_t, int64_t);
extern int64_t jet_coll_counter_get(int64_t, int64_t);
extern int64_t jet_coll_counter_set_count(int64_t, int64_t, int64_t);
extern int64_t jet_coll_counter_total(int64_t);
extern int64_t jet_coll_counter_elements(int64_t, int64_t);
extern int64_t jet_coll_counter_merge_add(int64_t, int64_t);
extern int64_t jet_coll_counter_subtract(int64_t, int64_t);
extern void jet_rt_handle_drop_jet_counter(int64_t);
extern int64_t jet_coll_chain(void);
extern int64_t jet_coll_chain_push(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_coll_chain_get(int64_t, int64_t, int64_t);
extern bool jet_coll_chain_contains(int64_t, int64_t);
extern void jet_rt_handle_drop_jet_chain(int64_t);
extern int64_t jet_int_div(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_rem(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_floor_div(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_mod(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_div_euclid(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_rem_euclid(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_pow(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_shl(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_shr(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_int_checked_fixed(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_int_clone(int64_t);
extern bool jet_bit_set_has(int64_t, int64_t);
extern void jet_bitset_remove(int64_t, int64_t);
extern int64_t jet_loop_range_init(int64_t, int64_t, int64_t, uint8_t, uint8_t);
extern int64_t jet_rt_record_new_data_event_int(int64_t);
extern int64_t jet_rt_record_new_dyn_int(int64_t);
extern int64_t jet_rt_record_get_dyn_int(int64_t);
extern int64_t jet_rt_record_new_duration(int64_t);
extern int64_t jet_rt_record_new_encoding_limits(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_data_limits(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_io_context(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_browser_locked(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_browser_timeout(int64_t);
extern int64_t jet_rt_record_new_jet_date(int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_period(int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_delivery(int64_t, int64_t, uint8_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_memo_stats(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_limits(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_log_span(int64_t, int64_t);
extern int64_t jet_rt_record_new_plugin_frame(int64_t, int64_t, int64_t);
extern int64_t message_parts(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_string_repeat(int64_t, int64_t);
extern int64_t jet_string_slice_builtin(int64_t, int64_t, int64_t);
extern void jet_rt_native_channel(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_receiver_clone(int64_t);
extern void jet_rt_native_receiver_drop(int64_t);
extern void jet_rt_native_sender_drop(int64_t);
extern void jet_rt_native_select_wait(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern int64_t jet_fd_open_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_adopt_raw(int64_t, int64_t, bool);
extern int64_t jet_fd_adopt_unbuffered_raw(int64_t, int64_t);
extern void jet_fd_drop_raw(int64_t);
extern int64_t jet_fd_number_raw(int64_t);
extern int64_t jet_fd_read_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_write_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_write_unbuffered_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_seek_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_fd_chmod_raw(int64_t, int64_t, int64_t);
extern void jet_fd_lock_raw(int64_t, int64_t, int64_t);
extern void jet_fd_truncate_raw(int64_t, int64_t, int64_t);
extern int64_t jet_fd_directory_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_open_at_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_fs_mkdir_raw(int64_t, int64_t, int64_t);
extern int64_t jet_fs_readlink_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fs_xattr_get_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_fd_mkdir_at_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_fd_readlink_at_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_fd_rename_at_raw(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_fd_resize_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_getpgid_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_getsid_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_setuid_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_setgid_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_setpgid_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_kill_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_getpriority_raw(int64_t, int64_t, int64_t, int64_t);
extern void jet_sys_setpriority_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_sys_umask_raw(int64_t, int64_t);
extern void jet_sys_waitpid_raw(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_std_os_close_fd(int64_t);
extern void jet_std_process_exit(int64_t);
extern int64_t jet_sys_errno_text_raw(int64_t);
extern int64_t jet_host_os_error_text_raw(int64_t, int64_t);
extern void jet_process_kill_raw(int64_t, int64_t, bool, int64_t);
extern int64_t jet_process_read_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_process_write_raw(int64_t, int64_t, int64_t, int64_t, int64_t);
extern void jet_process_close_stream_raw(int64_t, int64_t, int64_t);
extern void jet_process_resize_raw(int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_process_take_stream_raw(int64_t, int64_t, int64_t);
extern void jet_socket_stream_timeout_set_raw(int64_t, int64_t, int64_t);
extern void jet_udp_socket_timeout_set_raw(int64_t, int64_t);

static const char reason[] = "record variant index out of range";
static const char numeric_reason[] = "integer overflow";
/* Frozen TrapMessages.jet:323-332, not the freestanding Image literals. */
static const char overflow_reason[] = "This arithmetic overflows the value's type (the result is outside its range)";
static const char range_reason[] = "native Int argument exceeds host range";
static const char divide_reason[] = "divided by zero";
static unsigned released;
enum { MODE_COUNT = 37, INT_MODE_BASE = 200, INT_MODE_COUNT = 115 };
typedef struct { int64_t mode; atomic_bool ready, proceed; } Env;

static const char *expected_reason(int64_t mode) {
    if (mode >= INT_MODE_BASE) {
        int64_t test = mode - INT_MODE_BASE;
        if (test <= 5) return divide_reason;
        if (test == 6) return "Negative default Int exponent";
        if (test >= 7 && test <= 10) return "Invalid shift count";
        if (test >= 11 && test <= 13) return "Value doesn't fit in destination type";
        if (test == 15) return overflow_reason;
        if (test == 24) return reason;
        return range_reason;
    }
    if (mode == 2 || (mode >= 33 && mode <= 36)) return numeric_reason;
    if (mode == 3) return "the list has 1 items, so position 1 doesn't exist";
    if (mode == 4) return "the list has 9223372036854775807 items, so position -9223372036854775808 doesn't exist";
    if (mode >= 5 && mode <= 17) return range_reason;
    if ((mode >= 18 && mode <= 25) || mode == 30 || mode == 31) return overflow_reason;
    if (mode == 26 || mode == 27) return "the list has 0 items, so position 0 doesn't exist";
    if (mode == 29) return divide_reason;
    if (mode == 32) return "the list has -9223372036854775808 items, so position 9223372036854775807 doesn't exist";
    return reason;
}
static int64_t counter_with(Text *key, int64_t count) {
    int64_t empty = jet_coll_counter();
    int64_t result = jet_coll_counter_add(empty, WORD(key), count);
    jet_rt_handle_drop_jet_counter(empty);
    return result;
}
static void assert_count(int64_t counter, Text *key, int64_t count) {
    int64_t carrier = jet_coll_counter_get(counter, WORD(key));
    assert(jet_int_owned_to_i64(carrier) == count);
    jet_rt_int_drop(carrier);
}
/* Every case gets a fresh real task; publication is never cleared in a body. */
static void run_int_stop(int64_t test) {
    Text key = {(unsigned char *)"key", 3, 0};
    int64_t maximum = jet_int_owned_from_i64(INT64_MAX);
    int64_t big = jet_int_mul(maximum, 2);
    jet_rt_int_drop(maximum);
    int64_t *node = (int64_t *)(uintptr_t)(big & INT64_C(4611686018427387903));
    assert(node[0] == 1);
    const int64_t sentinel = INT64_C(0x12345678);
    int64_t out[4] = {sentinel, sentinel, sentinel, sentinel};
    int64_t a = WORD(&out[0]), b = WORD(&out[1]), c = WORD(&out[2]), d = WORD(&out[3]);
    if (test <= 5) {
        int64_t (*const divisions[])(int64_t, int64_t, int64_t, int64_t, int64_t) = {
            jet_int_div, jet_int_rem, jet_int_floor_div, jet_int_mod, jet_int_div_euclid, jet_int_rem_euclid
        };
        assert(divisions[test](big, 0, 0, 0, 0) == 0);
    } else if (test == 6) assert(jet_int_pow(big, -1, 0, 0, 0) == 0);
    else if (test >= 7 && test <= 10) {
        int64_t count = test < 9 ? -1 : big;
        assert((test % 2 ? jet_int_shl(big, count, 0, 0, 0) : jet_int_shr(big, count, 0, 0, 0)) == 0);
    } else if (test >= 11 && test <= 13) {
        assert(jet_int_checked_fixed(test == 12 ? -1 : big, test == 12 ? 4 : test == 13 ? 8 : 0, 0, 0, 0) == 0);
    } else if (test == 14) assert(jet_int_owned_to_i64(big) == 0);
    else if (test == 15) jet_rt_panic_overflow();
    else if (test >= 16 && test <= 18) {
        int64_t bits = jet_rt_native_bits_new();
        if (test == 16) assert(!jet_bit_set_has(bits, big));
        if (test == 17) assert(!jet_bitset_add(bits, big));
        if (test == 18) jet_bitset_remove(bits, big);
        jet_rt_handle_drop_jet_bit_set(bits);
    } else if (test >= 19 && test <= 21) {
        assert(jet_loop_range_init(test == 20 ? big : 0, test == 21 ? big : 1, test == 19 ? big : 1, 1, 0) == 0);
    } else if (test == 22) assert(jet_rt_record_new_jet_async_policy(big, 0) == 0);
    else if (test == 23) assert(jet_rt_record_new_data_event_int(big) == 0);
    else if (test == 24) {
        /* Canonical Dyn keeps large exact Int instead of narrowing it. */
        int64_t tree = jet_rt_record_new_dyn_int(big);
        assert(tree != 0 && jet_scheduler_native_propagate() == 0);
        int64_t retained = jet_rt_record_get_dyn_int(tree);
        assert(retained == big);
        jet_rt_int_drop(retained);
        /* Exercise the real variant guard after the successful owner path. */
        assert(jet_rt_record_get_dyn_text(tree) == NULL);
        jet_rt_handle_drop_dyn(tree);
    }
    else if (test == 25) assert(jet_rt_record_new_duration(big) == 0);
    else if (test == 26) assert(jet_rt_record_new_encoding_cause(WORD(&key), WORD(&big), WORD(&key)) == 0);
    else if (test >= 27 && test <= 29) {
        int64_t cause = jet_rt_record_new_encoding_cause(WORD(&key), 0, WORD(&key));
        /* Cause is adopted even when a later optional narrowing stops. */
        assert(jet_rt_record_new_encoding_error(0, 0, test == 27 ? big : 0,
            test == 28 ? WORD(&big) : 0, test == 29 ? WORD(&big) : 0, WORD(&key), WORD(&key), cause) == 0);
    } else if (test >= 30 && test <= 35) {
        int64_t args[6] = {1, 1, 1, 0, 1, 1};
        args[test - 30] = test == 33 ? WORD(&big) : big;
        assert(jet_rt_record_new_encoding_limits(args[0], args[1], args[2], args[3], args[4], args[5]) == 0);
    } else if (test >= 36 && test <= 39) {
        int64_t encoding = jet_rt_record_new_encoding_limits(1, 1, 1, 0, 1, 1);
        int64_t args[4] = {1, 1, 1, 1};
        args[test - 36] = big;
        assert(jet_rt_record_new_data_limits(encoding, args[0], args[1], args[2], args[3]) == 0);
    } else if (test == 40) assert(jet_rt_record_new_io_context(0, 0, WORD(&big), 0) == 0);
    else if (test == 41) assert(jet_rt_record_new_jet_browser_locked(0, 0, 0, 0, big, 0) == 0);
    else if (test == 42) assert(jet_rt_record_new_jet_browser_timeout(big) == 0);
    else if (test == 43) assert(jet_rt_record_new_jet_date(2026, 10, big) == 0);
    else if (test == 44) assert(jet_rt_record_new_jet_period(1, 1, big) == 0);
    else if (test == 45) assert(jet_rt_record_new_jet_delivery(WORD(&key), WORD(&key), 0, 0, big) == 0);
    else if (test == 46) assert(jet_rt_record_new_jet_memo_stats(1, 1, big, 0) == 0);
    else if (test == 47) assert(jet_rt_record_new_limits(1, 1, 1, 1, 1, big) == 0);
    else if (test == 48) assert(jet_rt_record_new_log_span(big, WORD(&key)) == 0);
    else if (test == 49) assert(jet_rt_record_new_plugin_frame(WORD(&key), WORD(&key), big) == 0);
    else if (test == 50) assert(jet_rt_record_new_terminal_size(80, big) == 0);
    else if (test == 51) assert(jet_rt_record_new_watch_event(0, 0, WORD(&key), WORD(&key), 0, big) == 0);
    else if (test == 52) assert(message_parts(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, big) == 0);
    else if (test == 53) assert(jet_string_repeat(WORD(&key), big) == 0);
    else if (test >= 54 && test <= 56) {
        assert(jet_string_slice(WORD(&key), test == 54 ? big : 0, test == 55 ? big : 1, WORD(&key), test == 56 ? big : 0) == 0);
    } else if (test == 57 || test == 58) {
        assert(jet_string_slice_builtin(WORD(&key), test == 57 ? big : 0, test == 58 ? big : 1) == 0);
    } else if (test == 59) jet_rt_native_channel(1, big, a, b);
    else if (test >= 60 && test <= 62) {
        int64_t tx = 0, rx = 0;
        jet_rt_native_channel(0, 0, WORD(&tx), WORD(&rx));
        int64_t *receivers = (int64_t *)(uintptr_t)jet_rt_alloc(8, 8);
        receivers[0] = jet_rt_native_receiver_clone(rx);
        int64_t *durations = (int64_t *)(uintptr_t)jet_rt_alloc(24, 8);
        int64_t tail = jet_int_owned_from_i64(INT64_MAX);
        for (int64_t i = 0; i < 3; ++i) durations[i] = jet_rt_int_clone(i == test - 60 ? big : tail);
        int64_t recvs[3] = {1, 1, WORD(receivers)}, timers[3] = {3, 3, WORD(durations)};
        jet_rt_native_select_wait(WORD(recvs), WORD(timers), 1, a, b);
        for (int i = 0; i < 3; ++i) assert(recvs[i] == 0 && timers[i] == 0);
        assert(*(int64_t *)(uintptr_t)(tail & INT64_C(4611686018427387903)) == 1);
        /* The consumed clone dropped; the caller's original receiver lives. */
        assert(*(int64_t *)((unsigned char *)(uintptr_t)rx + 128) == 1);
        jet_rt_int_drop(tail);
        jet_rt_native_receiver_drop(rx);
        jet_rt_native_sender_drop(tx);
    } else if (test >= 63 && test <= 85) {
        int native = open("/dev/null", O_RDWR);
        assert(native >= 0);
        int64_t fd = jet_fd_adopt_raw(native, WORD(&key), false);
        switch (test) {
        case 63: assert(jet_fd_open_raw(WORD(&key), big, 0, a) == 0); break;
        case 64: assert(jet_fd_open_raw(WORD(&key), 0, big, a) == 0); break;
        case 65: assert(jet_fd_adopt_raw(big, WORD(&key), false) == 0); break;
        case 66: assert(jet_fd_adopt_unbuffered_raw(big, WORD(&key)) == 0); break;
        case 67: assert(jet_fd_read_raw(fd, b, big, a) == 0); break;
        case 68: assert(jet_fd_write_raw(fd, b, big, a, c) == 0); break;
        case 69: assert(jet_fd_write_unbuffered_raw(fd, b, big, a) == 0); break;
        case 70: assert(jet_fd_seek_raw(fd, big, 0, a, b) == 0); break;
        case 71: assert(jet_fd_seek_raw(fd, 0, big, a, b) == 0); break;
        case 72: jet_fd_chmod_raw(fd, big, a); break;
        case 73: jet_fd_lock_raw(fd, big, a); break;
        case 74: jet_fd_truncate_raw(fd, big, a); break;
        case 75: assert(jet_fd_directory_raw(fd, b, big, a) == 0); break;
        case 76: assert(jet_fd_open_at_raw(fd, WORD(&key), big, 0, a) == 0); break;
        case 77: assert(jet_fd_open_at_raw(fd, WORD(&key), 0, big, a) == 0); break;
        case 78: jet_fs_mkdir_raw(WORD(&key), big, a); break;
        case 79: assert(jet_fs_readlink_raw(WORD(&key), b, big, a) == 0); break;
        case 80: assert(jet_fs_xattr_get_raw(WORD(&key), WORD(&key), b, big, a) == 0); break;
        case 81: jet_fd_mkdir_at_raw(fd, WORD(&key), big, a); break;
        case 82: assert(jet_fd_readlink_at_raw(fd, WORD(&key), b, big, a) == 0); break;
        case 83: jet_fd_rename_at_raw(fd, WORD(&key), fd, WORD(&key), big, a); break;
        case 84: jet_fd_resize_raw(fd, big, 1, a); break;
        case 85: jet_fd_resize_raw(fd, 1, big, a); break;
        }
        assert(jet_fd_number_raw(fd) == native);
        assert(fcntl(native, F_GETFD) >= 0);
        jet_fd_drop_raw(fd);
    } else {
        /* Invalid narrowing must precede syscalls and touching host carriers.
         * Null Process/socket inputs deliberately detect premature access. */
        switch (test) {
        case 86: jet_sys_getpgid_raw(big, a, b, c); break;
        case 87: jet_sys_getsid_raw(big, a, b, c); break;
        case 88: jet_sys_setuid_raw(big, a, b, c); break;
        case 89: jet_sys_setgid_raw(big, a, b, c); break;
        case 90: jet_sys_setpgid_raw(big, 0, a, b, c); break;
        case 91: jet_sys_setpgid_raw(0, big, a, b, c); break;
        case 92: jet_sys_kill_raw(big, 0, a, b, c); break;
        case 93: jet_sys_kill_raw(0, big, a, b, c); break;
        case 94: jet_sys_getpriority_raw(big, a, b, c); break;
        case 95: jet_sys_setpriority_raw(big, 0, a, b, c); break;
        case 96: jet_sys_setpriority_raw(0, big, a, b, c); break;
        case 97: assert(jet_sys_umask_raw(big, a) == 0); break;
        case 98: jet_sys_waitpid_raw(big, 0, a, b, c, d); break;
        case 99: jet_sys_waitpid_raw(0, big, a, b, c, d); break;
        case 100: jet_std_os_close_fd(big); break;
        case 101: jet_std_process_exit(big); break;
        case 102: assert(jet_sys_errno_text_raw(big) == 0); break;
        case 103: assert(jet_host_os_error_text_raw(0, big) == 0); break;
        case 104: jet_process_kill_raw(0, big, false, a); break;
        case 105: assert(jet_process_read_raw(0, big, b, 1, a) == 0); break;
        case 106: assert(jet_process_read_raw(0, 0, b, big, a) == 0); break;
        case 107: assert(jet_process_write_raw(0, big, b, 1, a) == 0); break;
        case 108: assert(jet_process_write_raw(0, 0, b, big, a) == 0); break;
        case 109: jet_process_close_stream_raw(0, big, a); break;
        case 110: jet_process_resize_raw(0, big, 1, a); break;
        case 111: jet_process_resize_raw(0, 1, big, a); break;
        case 112: assert(jet_process_take_stream_raw(0, big, a) == 0); break;
        case 113: jet_socket_stream_timeout_set_raw(0, 1, big); break;
        case 114: jet_udp_socket_timeout_set_raw(0, big); break;
        default: assert(!"unknown Int witness case");
        }
    }
    assert(jet_scheduler_native_propagate() != 0);
    for (int i = 0; i < 4; ++i) assert(out[i] == sentinel);
    assert(node[0] == 1); /* Every borrowed exact-Int owner survived. */
    assert(key.length == 3 && memcmp(key.data, "key", 3) == 0);
    jet_rt_int_drop(big);
}
static void run_stop(int64_t mode) {
    Text key = {(unsigned char *)"key", 3, 0};
    Text other = {(unsigned char *)"other", 5, 0};
    if (mode >= INT_MODE_BASE) {
        run_int_stop(mode - INT_MODE_BASE);
        return;
    }
    if (mode == 0) {
        int64_t tree = jet_rt_record_new_dyn_null();
        /* Nested payload -> clone chain must stop before cloning NULL. */
        assert(jet_rt_record_get_dyn_text(tree) == NULL);
        jet_rt_handle_drop_dyn(tree);
    } else if (mode == 1 || mode == 28) {
        Text file = {(unsigned char *)"<core.prelude>", 14, 0};
        Text message = {(unsigned char *)reason, sizeof(reason) - 1, 0};
        if (mode == 28) jet_scheduler_shield_enter();
        jet_panic(&file, 0, &message);
        assert(jet_scheduler_native_propagate() != 0);
        if (mode == 28) jet_scheduler_shield_leave();
    } else if (mode == 2 || mode == 29) {
        const char *text = expected_reason(mode);
        jet_rt_numeric_stop(0, 0, 0, WORD(text), (int64_t)strlen(text));
    } else if (mode == 3 || mode == 4 || mode == 5 || mode == 32) {
        int64_t native = mode == 3 ? 1 : mode == 4 ? INT64_MIN : INT64_MAX;
        int64_t index = jet_int_owned_from_i64(native);
        if (mode == 5) {
            int64_t big = jet_int_mul(index, 2);
            jet_rt_int_drop(index);
            index = big;
        }
        int64_t length = mode == 3 ? 1 : mode == 32 ? INT64_MIN : INT64_MAX;
        jet_rt_panic_list_bounds(length, index);
        jet_rt_int_drop(index);
    } else if (mode == 6) {
        assert(jet_rt_record_new_jet_async_policy(1, 3) == 0);
    } else if (mode == 7 || mode == 8) {
        /* Reject tags before touching borrowed text or optional slots. */
        /* Error adopts a real cause even when its tag validation fails. */
        int64_t cause = mode == 7 ? jet_rt_record_new_encoding_cause(WORD(&key), 0, WORD(&other)) : 0;
        assert(jet_rt_record_new_encoding_error(mode == 7 ? 7 : 0, mode == 8 ? 6 : 0,
            0, 0, 0, WORD(&key), WORD(&other), cause) == 0);
    } else if (mode == 9 || mode == 10) {
        assert(jet_rt_record_new_watch_event(mode == 9 ? 3 : 0, mode == 10 ? 6 : 0,
            WORD(&key), WORD(&other), 0, 0) == 0);
    } else if (mode >= 11 && mode <= 13) {
        int64_t spec = jet_std_process_cmd(0, 0);
        int64_t result = mode == 11 ? jet_process_spec_stdin(spec, -1) :
            mode == 12 ? jet_process_spec_stdout(spec, -1) : jet_process_spec_stderr(spec, -1);
        assert(result == 0);
        jet_rt_handle_drop_process_spec(spec);
    } else if (mode == 14) {
        int64_t size = jet_rt_record_new_terminal_size(80, 24);
        /* Constructor consumes Size before tag rejection, just like seed. */
        assert(jet_rt_record_new_terminal_policy(size, 2) == 0);
    } else if (mode == 15 || mode == 16) {
        assert(jet_rt_record_new_text_width(mode == 15 ? 2 : 0, mode == 16 ? 2 : 0) == 0);
    } else if (mode == 17) {
        assert(jet_string_slice(WORD(&key), 0, 1, WORD(&other), -1) == 0);
    } else if (mode == 18) {
        int64_t bits = jet_rt_native_bits_new();
        int64_t last = jet_int_owned_from_i64(INT64_MAX);
        assert(jet_bitset_add(bits, last));
        jet_rt_int_drop(last);
        assert(jet_bit_set_len(bits) == 0);
        jet_rt_handle_drop_jet_bit_set(bits);
    } else if ((mode >= 19 && mode <= 25) || mode == 30 || mode == 31) {
        int64_t initial = mode == 20 || mode == 31 ? INT64_MIN : INT64_MAX;
        int64_t counter = counter_with(&key, initial);
        int64_t result = 0, right = 0;
        if (mode == 19) result = jet_coll_counter_add(counter, WORD(&key), 1);
        if (mode == 20) result = jet_coll_counter_set_count(counter, WORD(&key), INT64_MAX);
        if (mode == 30) result = jet_coll_counter_inc(counter, WORD(&key));
        if (mode == 31) result = jet_coll_counter_dec(counter, WORD(&key));
        if (mode == 21 || mode == 22) {
            int64_t with_other = jet_coll_counter_add(counter, WORD(&other), 1);
            jet_rt_handle_drop_jet_counter(counter);
            counter = with_other;
            int64_t sentinel = INT64_C(0x12345678);
            result = mode == 21 ? jet_coll_counter_total(counter) :
                jet_coll_counter_elements(counter, WORD(&sentinel));
            assert(sentinel == INT64_C(0x12345678));
        }
        if (mode >= 23 && mode <= 25) {
            right = counter_with(&key, mode == 23 ? 1 : mode == 24 ? INT64_MIN : -1);
            result = mode == 23 ? jet_coll_counter_merge_add(counter, right) :
                jet_coll_counter_subtract(counter, right);
        }
        assert(result == 0);
        /* Borrowed inputs must remain intact; only the partial copy is freed. */
        assert_count(counter, &key, initial);
        if (right != 0) jet_rt_handle_drop_jet_counter(right);
        jet_rt_handle_drop_jet_counter(counter);
    } else if (mode == 26 || mode == 27) {
        int64_t empty = jet_coll_chain();
        int64_t keys[1] = {WORD(&key)};
        int64_t chain = jet_coll_chain_push(empty, WORD(keys), 1, 0, 0);
        jet_rt_handle_drop_jet_chain(empty);
        int64_t sentinel = INT64_C(0x12345678);
        if (mode == 26) assert(jet_coll_chain_get(chain, WORD(&key), WORD(&sentinel)) == 0);
        else assert(!jet_coll_chain_contains(chain, WORD(&key)));
        assert(sentinel == INT64_C(0x12345678));
        jet_rt_handle_drop_jet_chain(chain);
    } else if (mode >= 101 && mode <= 105) {
        /* Actual task cleanup carries an already-delivered stop. */
        assert(jet_rt_control_publish_stop(mode - 100, -1, -1, -1, -1, WORD(reason), sizeof(reason) - 1, 0));
        assert(jet_scheduler_native_propagate() != 0);
        jet_rt_numeric_stop(0, 0, 0, WORD(numeric_reason), sizeof(numeric_reason) - 1);
        assert(!"panic during propagation must abort");
    } else {
        assert(!"unknown witness mode");
    }
    assert(jet_scheduler_native_propagate() != 0);
}
static int64_t body(int64_t environment, int64_t out) {
    (void)out;
    Env *env = (Env *)(uintptr_t)environment;
    int64_t mode = env->mode;
    if (mode == 33 || mode == 34) {
        if (mode == 33) jet_scheduler_shield_enter();
        atomic_store_explicit(&env->ready, true, memory_order_release);
        /* No scheduler poll: the parent's real cancel remains a request. */
        while (!atomic_load_explicit(&env->proceed, memory_order_acquire)) { }
        run_stop(2);
        if (mode == 33) jet_scheduler_shield_leave();
    } else if (mode == 35 || mode == 36) {
        jet_scheduler_shield_enter();
        assert(jet_rt_control_publish_stop(mode - 34, -1, -1, -1, -1, WORD(reason), sizeof(reason) - 1, 0));
        assert(jet_scheduler_native_propagate() == 0);
        run_stop(2);
        jet_scheduler_shield_leave();
    } else {
        run_stop(mode);
    }
    return 0;
}
static void release(int64_t env) { (void)env; ++released; }
static void clone(int64_t source, int64_t target) {
    *(int64_t *)(uintptr_t)target = *(int64_t *)(uintptr_t)source;
}
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, WORD(argv), WORD(envp));
    if (argc == 2 && strcmp(argv[1], "root-panic") == 0) run_stop(1);
    if (argc == 2 && strcmp(argv[1], "root-variant") == 0) run_stop(0);
    if (argc == 2 && strcmp(argv[1], "root-overflow-export") == 0) jet_rt_panic_overflow();
    if (argc == 2 && strcmp(argv[1], "root-divide-export") == 0) jet_rt_panic_division_by_zero();
    if (argc == 2 && strncmp(argv[1], "root-mode-", 10) == 0) run_stop(strtol(argv[1] + 10, NULL, 10));
    if (argc == 2 && strcmp(argv[1], "root-int-range") == 0) {
        int64_t maximum = jet_int_owned_from_i64(INT64_MAX);
        (void)jet_int_owned_to_i64(jet_int_mul(maximum, 2));
        assert(!"root Int conversion must remain process-fatal");
    }
    int64_t control = jet_rt_control_new(), save[4] = {0};
    jet_rt_control_enter(control, WORD(save));
    /* Canonical native metadata: I64 payload, clone callback, Int kind. */
    int64_t meta[9] = {8, WORD(clone), 0, 0, 0, 0, INT64_C(274877906946), 0, 0};
    Text label = {NULL, 0, 0};
    int64_t first = 0, last = MODE_COUNT + INT_MODE_COUNT;
    bool double_panic = argc == 2 && strncmp(argv[1], "double-", 7) == 0;
    if (double_panic) {
        first = 100 + strtol(argv[1] + 7, NULL, 10);
        last = first + 1;
    }
    for (int64_t ordinal = first; ordinal < last; ++ordinal) {
        int64_t mode = !double_panic && ordinal >= MODE_COUNT && ordinal < MODE_COUNT + INT_MODE_COUNT ?
            INT_MODE_BASE + ordinal - MODE_COUNT : ordinal;
        Env env = {.mode = mode};
        atomic_init(&env.ready, false);
        atomic_init(&env.proceed, false);
        int64_t task = jet_rt_native_task_spawn(17, &label, WORD(&env), WORD(body), WORD(release), WORD(meta));
        if (mode == 33 || mode == 34) {
            while (!atomic_load_explicit(&env.ready, memory_order_acquire)) { }
            jet_rt_native_task_cancel(task);
            atomic_store_explicit(&env.proceed, true, memory_order_release);
        }
        /* Observe.rs prefixes the canonical child identity + ': '. */
        Text *identity = jet_rt_string_clone(*(Text **)((unsigned char *)(uintptr_t)task + 128));
        int64_t out[4] = {0};
        assert(jet_rt_native_task_join(task, 0, WORD(out)) == 0);
        assert(jet_rt_record_tag_jet_task_failure(out[0]) == 2);
        Text *text = jet_rt_record_get_jet_task_failure_panicked(out[0]);
        const char *expected = expected_reason(mode);
        assert(text->length == identity->length + 2 + (int64_t)strlen(expected));
        assert(memcmp(text->data, identity->data, (size_t)identity->length) == 0);
        assert(memcmp(text->data + identity->length, ": ", 2) == 0);
        assert(memcmp(text->data + identity->length + 2, expected, strlen(expected)) == 0);
        jet_rt_string_drop(identity);
        jet_rt_string_drop(text);
        jet_rt_handle_drop_jet_task_failure(out[0]);
        assert(jet_scheduler_native_propagate() == 0);
    }
    assert(released == MODE_COUNT + INT_MODE_COUNT);
    jet_rt_control_leave(WORD(save));
    jet_rt_scheduler_control_drop(control);
    puts("task variant/panic/numeric/checked-ops joined Panicked(reason); process survived");
    return 0;
}
