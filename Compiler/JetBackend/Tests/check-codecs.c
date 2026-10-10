// Monomorphic adapters must preserve canonical scalar policy and ownership.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

typedef void *Handle;
extern Handle jet_rt_string_from_static(const char *, int64_t);
extern void jet_rt_string_drop(Handle);
extern void jet_rt_handle_drop_dyn(Handle);
extern void jet_rt_handle_drop_field_error(Handle);
extern void jet_rt_free(void *, int64_t, int64_t);
extern Handle jet_rt_codec_encode_u8(int64_t);
extern Handle jet_rt_codec_encode_i64(int64_t);
extern int64_t jet_rt_codec_decode_u8(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_decode_u64(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_decode_i64(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_parse_json(Handle, Handle *, Handle **, int64_t *);
extern bool jet_rt_string_eq(Handle, Handle);
extern void *jet_rt_alloc(int64_t, int64_t);
extern Handle jet_rt_record_new_field_error(Handle, Handle);
extern Handle jet_rt_record_get_field_error_path(Handle);

typedef struct NativeMeta {
    uint64_t stride, clone, drop, show, equal, compare, kind;
    const struct NativeMeta *child0, *child1;
} NativeMeta;
typedef struct NativeList { uint64_t len, cap; void *data; } NativeList;
extern int64_t jet_rt_codec_csv(Handle, uintptr_t, const NativeMeta *, NativeList *, Handle **, int64_t *);
static uint64_t csv_invocations;
static Handle csv_path, csv_reason;

// A valid custom Decode implementation may ignore the tree and return a
// constant. This witness checks borrowed-mask/value-slot/container ownership.
static uint64_t csv_constant(Handle borrowed_tree, int64_t *value, uint64_t *tag, Handle **errors, int64_t *count) {
    assert(borrowed_tree != NULL);
    (void)errors;
    (void)count;
    ++csv_invocations;
    *value = 7;
    *tag = 1;
    return 0;
}
static uint64_t csv_reject(Handle borrowed_tree, int64_t *value, uint64_t *tag, Handle **errors, int64_t *count) {
    assert(borrowed_tree != NULL);
    (void)value;
    ++csv_invocations;
    *errors = jet_rt_alloc((int64_t)sizeof(Handle), 8);
    (*errors)[0] = jet_rt_record_new_field_error(csv_path, csv_reason);
    *count = 1;
    *tag = 0;
    return 0;
}

static void drop_errors(Handle *errors, int64_t length) {
    for (int64_t i = 0; i < length; ++i) jet_rt_handle_drop_field_error(errors[i]);
    if (length) jet_rt_free(errors, length * (int64_t)sizeof(Handle), 8);
}

int main(void) {
    const int64_t bytes[] = {0, 127, 128, 255};
    for (size_t i = 0; i < sizeof(bytes) / sizeof(bytes[0]); ++i) {
        Handle tree = jet_rt_codec_encode_u8(bytes[i]);
        int64_t value = -1, count = -1;
        Handle *errors = NULL;
        assert(jet_rt_codec_decode_u8(tree, &value, &errors, &count) == 1);
        assert(value == bytes[i]);
        jet_rt_handle_drop_dyn(tree);
    }
    Handle minimum = jet_rt_codec_encode_i64(INT64_MIN);
    int64_t value = 0, count = -1;
    Handle *errors = NULL;
    assert(jet_rt_codec_decode_i64(minimum, &value, &errors, &count) == 1);
    assert(value == INT64_MIN);
    assert(jet_rt_codec_decode_u8(minimum, &value, &errors, &count) == 0);
    assert(count == 1);
    drop_errors(errors, count);
    jet_rt_handle_drop_dyn(minimum);

    // The typed JSON intrinsic preserves integers outside signed/binary64 range.
    Handle text = jet_rt_string_from_static("18446744073709551615", 20);
    Handle tree = NULL;
    assert(jet_rt_codec_parse_json(text, &tree, &errors, &count) == 1);
    assert(jet_rt_codec_decode_u64(tree, &value, &errors, &count) == 1);
    assert((uint64_t)value == UINT64_MAX);
    jet_rt_handle_drop_dyn(tree);
    jet_rt_string_drop(text);

    text = jet_rt_string_from_static("{", 1);
    assert(jet_rt_codec_parse_json(text, &tree, &errors, &count) == 0);
    assert(count == 1);
    drop_errors(errors, count);
    jet_rt_string_drop(text);

    const NativeMeta integer = { .stride = 8, .kind = 1 };
    text = jet_rt_string_from_static("value\n1\n2\n", 10);
    NativeList rows = {0};
    csv_invocations = 0;
    assert(jet_rt_codec_csv(text, (uintptr_t)csv_constant, &integer, &rows, &errors, &count) == 1);
    assert(csv_invocations == 2 && rows.len == 2 && rows.cap == 2);
    assert(((int64_t *)rows.data)[0] == 7 && ((int64_t *)rows.data)[1] == 7);
    jet_rt_free(rows.data, (int64_t)rows.cap * 8, 8);

    csv_path = jet_rt_string_from_static("value", 5);
    csv_reason = jet_rt_string_from_static("invalid", 7);
    csv_invocations = 0;
    assert(jet_rt_codec_csv(text, (uintptr_t)csv_reject, &integer, &rows, &errors, &count) == 0);
    assert(csv_invocations == 2 && count == 2);
    for (int64_t i = 0; i < count; ++i) {
        Handle path = jet_rt_record_get_field_error_path(errors[i]);
        Handle expected = jet_rt_string_from_static(i == 0 ? "row 1.value" : "row 2.value", 11);
        assert(jet_rt_string_eq(path, expected));
        jet_rt_string_drop(path);
        jet_rt_string_drop(expected);
    }
    drop_errors(errors, count);
    jet_rt_string_drop(csv_path);
    jet_rt_string_drop(csv_reason);
    jet_rt_string_drop(text);
    return 0;
}
