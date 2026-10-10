// C client of the published RC runtime object; no fixture exports, Rust
// provider, alternate runtime, or guessed Rust enum/Box layout is required.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef RT_HOST_U8_STRIDE
#error "runner must provide the RC-pinned native U8 element stride"
#endif
#ifndef DATA_TREE_INT
#error "runner must provide the RC-pinned DataABI tags"
#endif
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
extern int32_t jet_rt_main(void (*body)(void));
extern Text *jet_rt_string_from_static(const void *, W);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_free(void *, W, W);
extern W jet_int_owned_from_i64(W);
extern void jet_rt_int_drop(W);
extern W jet_rt_record_tag_dyn(void *);
extern bool jet_rt_record_get_dyn_bool(void *);
extern double jet_rt_record_get_dyn_float(void *);
extern Text *jet_rt_record_get_dyn_text(void *);
extern Text *jet_rt_record_get_dyn_number(void *);
extern Text *jet_rt_handle_show_dyn(void *);
extern void jet_rt_handle_drop_dyn(void *);
extern void *jet_rt_codec_encode_int(W);
extern void *jet_rt_codec_encode_bool(uint8_t);
extern void *jet_rt_codec_encode_float(double);
extern void *jet_rt_codec_encode_string(Text *);
extern void *jet_rt_codec_encode_char(W);
extern void *jet_rt_codec_encode_u8(W);
extern void *jet_rt_codec_encode_u16(W);
extern void *jet_rt_codec_encode_u32(W);
extern void *jet_rt_codec_encode_u64(W);
extern void *jet_rt_codec_encode_i8(W);
extern void *jet_rt_codec_encode_i16(W);
extern void *jet_rt_codec_encode_i32(W);
extern void *jet_rt_codec_encode_i64(W);
extern void *jet_raw_typed_text(Text *);
extern void *jet_rt_record_new_field_error(Text *, Text *);
extern Text *jet_rt_record_get_field_error_path(void *);
extern Text *jet_rt_record_get_field_error_reason(void *);
extern void jet_rt_handle_drop_field_error(void *);
extern W jet_rt_codec_under(Text *, const uintptr_t *, W, uintptr_t **);
extern W jet_rt_codec_parse_json(Text *, void **, uintptr_t **, W *);
extern W jet_std_files_open_reader_nonblocking(Text *, void **, void **);
extern W jet_file_reader_line_bytes_raw(void *, unsigned char **, W *, void **);
extern W jet_fd_read_raw(void *, void *, W, W *);
extern W jet_fd_seek_raw(void *, W, W, W *, bool *);
extern void jet_fd_close_raw(void *, W *, bool *);
extern void jet_fd_drop_raw(void *);
extern void jet_rt_handle_drop_io_error(void *);
static char path[] = "/tmp/jet-native-codec-files-XXXXXX";
static Text *text(const char *s) { return jet_rt_string_from_static(s, (W)strlen(s)); }
static void text_is(Text *value, const char *expected) {
    size_t len = strlen(expected);
    assert(value->len == (W)len && (len == 0 || memcmp(value->data, expected, len) == 0));
    jet_rt_string_drop(value);
}
static void number(void *tree, const char *expected) {
    assert(jet_rt_record_tag_dyn(tree) == DATA_TREE_INT);
    text_is(jet_rt_handle_show_dyn(tree), expected);
    jet_rt_handle_drop_dyn(tree);
}
static void *field(const char *name, const char *reason) {
    Text *p = text(name), *r = text(reason);
    void *value = jet_rt_record_new_field_error(p, r);
    jet_rt_string_drop(p); jet_rt_string_drop(r);
    return value;
}
static void codecs(void) {
    number(jet_rt_codec_encode_u8(-1), "255");
    number(jet_rt_codec_encode_u16(-1), "65535");
    number(jet_rt_codec_encode_u32(-1), "4294967295");
    number(jet_rt_codec_encode_u64(-1), "18446744073709551615");
    number(jet_rt_codec_encode_u64(INT64_MIN), "9223372036854775808");
    number(jet_rt_codec_encode_i8(255), "-1");
    number(jet_rt_codec_encode_i16(32768), "-32768");
    number(jet_rt_codec_encode_i32(INT64_C(2147483648)), "-2147483648");
    number(jet_rt_codec_encode_i64(INT64_MIN), "-9223372036854775808");
    W exact = jet_int_owned_from_i64(INT64_MAX);
    void *integer = jet_rt_codec_encode_int(exact);
    jet_rt_int_drop(exact);
    number(integer, "9223372036854775807");
    void *boolean = jet_rt_codec_encode_bool(2);
    assert(jet_rt_record_tag_dyn(boolean) == DATA_TREE_BOOL && jet_rt_record_get_dyn_bool(boolean));
    jet_rt_handle_drop_dyn(boolean);
    void *real = jet_rt_codec_encode_float(-0.0);
    double decoded = jet_rt_record_get_dyn_float(real); uint64_t bits;
    memcpy(&bits, &decoded, sizeof bits);
    assert(jet_rt_record_tag_dyn(real) == DATA_TREE_FLOAT && bits == UINT64_C(0x8000000000000000));
    jet_rt_handle_drop_dyn(real);
    Text *s = text("\xc3\xa9");
    void *string = jet_rt_codec_encode_string(s), *typed = jet_raw_typed_text(s);
    jet_rt_string_drop(s);
    assert(jet_rt_record_tag_dyn(string) == DATA_TREE_TEXT && jet_rt_record_tag_dyn(typed) == DATA_TREE_TEXT);
    text_is(jet_rt_record_get_dyn_text(string), "\xc3\xa9");
    text_is(jet_rt_record_get_dyn_text(typed), "\xc3\xa9");
    jet_rt_handle_drop_dyn(string); jet_rt_handle_drop_dyn(typed);
    void *scalar = jet_rt_codec_encode_char(119070), *replacement = jet_rt_codec_encode_char(55296);
    text_is(jet_rt_record_get_dyn_text(scalar), "\xf0\x9d\x84\x9e");
    text_is(jet_rt_record_get_dyn_text(replacement), "\xef\xbf\xbd");
    jet_rt_handle_drop_dyn(scalar); jet_rt_handle_drop_dyn(replacement);
    uintptr_t input[] = {(uintptr_t)field("", "first"), (uintptr_t)field("[1].name", "second"), (uintptr_t)field("name", "third")};
    uintptr_t *framed = NULL; Text *segment = text("items");
    assert(jet_rt_codec_under(segment, input, 3, &framed) == 3);
    jet_rt_string_drop(segment);
    const char *paths[] = {"items", "items[1].name", "items.name"};
    const char *reasons[] = {"first", "second", "third"};
    text_is(jet_rt_record_get_field_error_path((void *)input[0]), "");
    for (int i = 0; i != 3; ++i) {
        assert(framed[i] != input[i]);
        text_is(jet_rt_record_get_field_error_path((void *)framed[i]), paths[i]);
        text_is(jet_rt_record_get_field_error_reason((void *)framed[i]), reasons[i]);
        jet_rt_handle_drop_field_error((void *)input[i]);
        jet_rt_handle_drop_field_error((void *)framed[i]);
    }
    jet_rt_free(framed, 3 * (W)sizeof(uintptr_t), 8);
    segment = text("x"); framed = (uintptr_t *)(uintptr_t)724;
    assert(jet_rt_codec_under(segment, NULL, 0, &framed) == 0 && framed == NULL);
    jet_rt_string_drop(segment);
    s = text("1e400"); void *ok = NULL; uintptr_t *errors = (uintptr_t *)(uintptr_t)724; W count = 725;
    assert(jet_rt_codec_parse_json(s, &ok, &errors, &count) == 1);
    jet_rt_string_drop(s);
    assert(errors == (uintptr_t *)(uintptr_t)724 && count == 725 && jet_rt_record_tag_dyn(ok) == DATA_TREE_NUMBER);
    text_is(jet_rt_record_get_dyn_number(ok), "1e400"); jet_rt_handle_drop_dyn(ok);
    s = text("\n?"); ok = (void *)(uintptr_t)723;
    assert(jet_rt_codec_parse_json(s, &ok, &errors, &count) == 0 && count == 1 && ok == (void *)(uintptr_t)723);
    jet_rt_string_drop(s);
    text_is(jet_rt_record_get_field_error_path((void *)errors[0]), "");
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "invalid JSON (line 2): expected a JSON value");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
}
static void line(void *reader, const unsigned char *expected, W size) {
    unsigned char *bytes = NULL; W count = -1; void *error = NULL;
    assert(jet_file_reader_line_bytes_raw(reader, &bytes, &count, &error) == 1 && count == size && error == NULL);
    for (W i = 0; i < size; ++i) assert(bytes[i * RT_HOST_U8_STRIDE] == expected[i]);
    if (size) jet_rt_free(bytes, size * RT_HOST_U8_STRIDE, 8); else assert(bytes == NULL);
}
static void exercise(void) {
    codecs();
    Text *name = text(path); void *reader = NULL, *error = NULL;
    assert(jet_std_files_open_reader_nonblocking(name, &reader, &error) == 1 && error == NULL);
    jet_rt_string_drop(name);
    unsigned char *first = malloc(20003); assert(first != NULL);
    memset(first, 'a', 20000); memcpy(first + 20000, "\r\r\n", 3);
    line(reader, first, 20003); free(first);
    line(reader, (const unsigned char *)"b\n", 2);
    unsigned char raw[2]; W os_error[4] = {0}; bool zero = false;
    assert(jet_fd_read_raw(reader, raw, 2, os_error) == 2 && raw[0] == 't' && raw[1] == 'a');
    assert(jet_fd_seek_raw(reader, 0, SEEK_CUR, os_error, &zero) == 20007);
    line(reader, (const unsigned char *)"il\r", 3); line(reader, NULL, 0);
    jet_fd_close_raw(reader, os_error, &zero);
    unsigned char *bytes = NULL; W count = 0;
    assert(jet_file_reader_line_bytes_raw(reader, &bytes, &count, &error) == 0);
    jet_rt_handle_drop_io_error(error); jet_fd_drop_raw(reader);
}
int main(void) {
    int fd = mkstemp(path); assert(fd >= 0);
    FILE *file = fdopen(fd, "wb"); assert(file != NULL);
    for (int i = 0; i != 20000; ++i) assert(fputc('a', file) == 'a');
    assert(fwrite("\r\r\nb\ntail\r", 1, 10, file) == 10 && fclose(file) == 0);
    int32_t status = jet_rt_main(exercise);
    assert(unlink(path) == 0 && status == 0);
    puts("CHECK OK — Jet-only Dyn encodes, FieldError frames, JSON codec and shared buffered lines");
    return 0;
}
