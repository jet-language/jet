/* Real task runner and producers; this client defines no runtime providers. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdatomic.h>
#include <string.h>

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
extern int64_t jet_rt_record_new_DataTree_Null(void);
extern Text *jet_rt_record_get_DataTree_Text(int64_t);
extern void jet_rt_handle_drop_DataTree(int64_t);
extern int64_t jet_rt_record_tag_JetTaskFailure(int64_t);
extern Text *jet_rt_record_get_JetTaskFailure_Panicked(int64_t);
extern void jet_rt_handle_drop_JetTaskFailure(int64_t);
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
extern int64_t jet_rt_record_new_JetAsyncPolicy(int64_t, int64_t);
extern int64_t jet_rt_record_new_EncodingError(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_WatchEvent(int64_t, int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_std_process_cmd(int64_t, int64_t);
extern int64_t jet_process_spec_stdin(int64_t, int64_t);
extern int64_t jet_process_spec_stdout(int64_t, int64_t);
extern int64_t jet_process_spec_stderr(int64_t, int64_t);
extern void jet_rt_handle_drop_ProcessSpec(int64_t);
extern int64_t jet_rt_record_new_TerminalSize(int64_t, int64_t);
extern int64_t jet_rt_record_new_TerminalPolicy(int64_t, int64_t);
extern int64_t jet_rt_record_new_TextWidth(int64_t, int64_t);
extern int64_t jet_string_slice(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_native_bits_new(void);
extern bool jet_bitset_add(int64_t, int64_t);
extern int64_t jet_bit_set_len(int64_t);
extern void jet_rt_handle_drop_JetBitSet(int64_t);
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
extern void jet_rt_handle_drop_JetCounter(int64_t);
extern int64_t jet_coll_chain(void);
extern int64_t jet_coll_chain_push(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_coll_chain_get(int64_t, int64_t, int64_t);
extern bool jet_coll_chain_contains(int64_t, int64_t);
extern void jet_rt_handle_drop_JetChain(int64_t);

static const char reason[] = "record variant index out of range";
static const char numeric_reason[] = "integer overflow";
/* Frozen TrapMessages.jet:323-332, not the freestanding Image literals. */
static const char overflow_reason[] = "This arithmetic overflows the value's type (the result is outside its range)";
static const char range_reason[] = "native Int argument exceeds host range";
static const char divide_reason[] = "divided by zero";
static unsigned released;
enum { MODE_COUNT = 37 };
typedef struct { int64_t mode; atomic_bool ready, proceed; } Env;

static const char *expected_reason(int64_t mode) {
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
    jet_rt_handle_drop_JetCounter(empty);
    return result;
}
static void assert_count(int64_t counter, Text *key, int64_t count) {
    int64_t carrier = jet_coll_counter_get(counter, WORD(key));
    assert(jet_int_owned_to_i64(carrier) == count);
    jet_rt_int_drop(carrier);
}
static void run_stop(int64_t mode) {
    Text key = {(unsigned char *)"key", 3, 0};
    Text other = {(unsigned char *)"other", 5, 0};
    if (mode == 0) {
        int64_t tree = jet_rt_record_new_DataTree_Null();
        /* Nested payload -> clone chain must stop before cloning NULL. */
        assert(jet_rt_record_get_DataTree_Text(tree) == NULL);
        jet_rt_handle_drop_DataTree(tree);
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
        assert(jet_rt_record_new_JetAsyncPolicy(1, 3) == 0);
    } else if (mode == 7 || mode == 8) {
        /* Reject tags before touching borrowed text or optional slots. */
        assert(jet_rt_record_new_EncodingError(mode == 7 ? 7 : 0, mode == 8 ? 6 : 0,
            0, 0, 0, WORD(&key), WORD(&other), 0) == 0);
    } else if (mode == 9 || mode == 10) {
        assert(jet_rt_record_new_WatchEvent(mode == 9 ? 3 : 0, mode == 10 ? 6 : 0,
            WORD(&key), WORD(&other), 0, 0) == 0);
    } else if (mode >= 11 && mode <= 13) {
        int64_t spec = jet_std_process_cmd(0, 0);
        int64_t result = mode == 11 ? jet_process_spec_stdin(spec, -1) :
            mode == 12 ? jet_process_spec_stdout(spec, -1) : jet_process_spec_stderr(spec, -1);
        assert(result == 0);
        jet_rt_handle_drop_ProcessSpec(spec);
    } else if (mode == 14) {
        int64_t size = jet_rt_record_new_TerminalSize(80, 24);
        /* Constructor consumes Size before tag rejection, just like seed. */
        assert(jet_rt_record_new_TerminalPolicy(size, 2) == 0);
    } else if (mode == 15 || mode == 16) {
        assert(jet_rt_record_new_TextWidth(mode == 15 ? 2 : 0, mode == 16 ? 2 : 0) == 0);
    } else if (mode == 17) {
        assert(jet_string_slice(WORD(&key), 0, 1, WORD(&other), -1) == 0);
    } else if (mode == 18) {
        int64_t bits = jet_rt_native_bits_new();
        int64_t last = jet_int_owned_from_i64(INT64_MAX);
        assert(jet_bitset_add(bits, last));
        jet_rt_int_drop(last);
        assert(jet_bit_set_len(bits) == 0);
        jet_rt_handle_drop_JetBitSet(bits);
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
            jet_rt_handle_drop_JetCounter(counter);
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
        if (right != 0) jet_rt_handle_drop_JetCounter(right);
        jet_rt_handle_drop_JetCounter(counter);
    } else if (mode == 26 || mode == 27) {
        int64_t empty = jet_coll_chain();
        int64_t keys[1] = {WORD(&key)};
        int64_t chain = jet_coll_chain_push(empty, WORD(keys), 1, 0, 0);
        jet_rt_handle_drop_JetChain(empty);
        int64_t sentinel = INT64_C(0x12345678);
        if (mode == 26) assert(jet_coll_chain_get(chain, WORD(&key), WORD(&sentinel)) == 0);
        else assert(!jet_coll_chain_contains(chain, WORD(&key)));
        assert(sentinel == INT64_C(0x12345678));
        jet_rt_handle_drop_JetChain(chain);
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
static int64_t release(int64_t env) { (void)env; ++released; return 0; }
static int64_t clone(int64_t source, int64_t target) {
    *(int64_t *)(uintptr_t)target = *(int64_t *)(uintptr_t)source;
    return 0;
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
        assert(!"Int conversion stays process-fatal in this leaf");
    }
    int64_t control = jet_rt_control_new(), save[4] = {0};
    jet_rt_control_enter(control, WORD(save));
    /* Canonical native metadata: I64 payload, clone callback, Int kind. */
    int64_t meta[9] = {8, WORD(clone), 0, 0, 0, 0, INT64_C(274877906946), 0, 0};
    Text label = {NULL, 0, 0};
    int64_t first = 0, last = MODE_COUNT;
    if (argc == 2 && strncmp(argv[1], "double-", 7) == 0) {
        first = 100 + strtol(argv[1] + 7, NULL, 10);
        last = first + 1;
    }
    for (int64_t mode = first; mode < last; ++mode) {
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
        assert(jet_rt_record_tag_JetTaskFailure(out[0]) == 2);
        Text *text = jet_rt_record_get_JetTaskFailure_Panicked(out[0]);
        const char *expected = expected_reason(mode);
        assert(text->length == identity->length + 2 + (int64_t)strlen(expected));
        assert(memcmp(text->data, identity->data, (size_t)identity->length) == 0);
        assert(memcmp(text->data + identity->length, ": ", 2) == 0);
        assert(memcmp(text->data + identity->length + 2, expected, strlen(expected)) == 0);
        jet_rt_string_drop(identity);
        jet_rt_string_drop(text);
        jet_rt_handle_drop_JetTaskFailure(out[0]);
        assert(jet_scheduler_native_propagate() == 0);
    }
    assert(released == MODE_COUNT);
    jet_rt_control_leave(WORD(save));
    jet_rt_scheduler_control_drop(control);
    puts("task variant/panic/numeric/checked-ops joined Panicked(reason); process survived");
    return 0;
}
