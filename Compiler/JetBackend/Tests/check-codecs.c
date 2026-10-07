// Monomorphic adapters must preserve canonical scalar policy and ownership.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>

typedef void *Handle;
extern Handle jet_rt_string_from_static(const char *, int64_t);
extern void jet_rt_string_drop(Handle);
extern void jet_rt_handle_drop_DataTree(Handle);
extern void jet_rt_handle_drop_FieldError(Handle);
extern void jet_rt_free(void *, int64_t, int64_t);
extern Handle jet_rt_codec_encode_U8(int64_t);
extern Handle jet_rt_codec_encode_I64(int64_t);
extern int64_t jet_rt_codec_decode_U8(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_decode_U64(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_decode_I64(Handle, int64_t *, Handle **, int64_t *);
extern int64_t jet_rt_codec_parse_json(Handle, Handle *, Handle **, int64_t *);

static void drop_errors(Handle *errors, int64_t length) {
    for (int64_t i = 0; i < length; ++i) jet_rt_handle_drop_FieldError(errors[i]);
    if (length) jet_rt_free(errors, length * (int64_t)sizeof(Handle), 8);
}

int main(void) {
    const int64_t bytes[] = {0, 127, 128, 255};
    for (size_t i = 0; i < sizeof(bytes) / sizeof(bytes[0]); ++i) {
        Handle tree = jet_rt_codec_encode_U8(bytes[i]);
        int64_t value = -1, count = -1;
        Handle *errors = NULL;
        assert(jet_rt_codec_decode_U8(tree, &value, &errors, &count) == 1);
        assert(value == bytes[i]);
        jet_rt_handle_drop_DataTree(tree);
    }
    Handle minimum = jet_rt_codec_encode_I64(INT64_MIN);
    int64_t value = 0, count = -1;
    Handle *errors = NULL;
    assert(jet_rt_codec_decode_I64(minimum, &value, &errors, &count) == 1);
    assert(value == INT64_MIN);
    assert(jet_rt_codec_decode_U8(minimum, &value, &errors, &count) == 0);
    assert(count == 1);
    drop_errors(errors, count);
    jet_rt_handle_drop_DataTree(minimum);

    // The typed JSON intrinsic preserves integers outside signed/binary64 range.
    Handle text = jet_rt_string_from_static("18446744073709551615", 20);
    Handle tree = NULL;
    assert(jet_rt_codec_parse_json(text, &tree, &errors, &count) == 1);
    assert(jet_rt_codec_decode_U64(tree, &value, &errors, &count) == 1);
    assert((uint64_t)value == UINT64_MAX);
    jet_rt_handle_drop_DataTree(tree);
    jet_rt_string_drop(text);

    text = jet_rt_string_from_static("{", 1);
    assert(jet_rt_codec_parse_json(text, &tree, &errors, &count) == 0);
    assert(count == 1);
    drop_errors(errors, count);
    jet_rt_string_drop(text);
    return 0;
}
