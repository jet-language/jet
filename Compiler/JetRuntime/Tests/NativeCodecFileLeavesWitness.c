// C client of the published RC runtime object; no fixture exports, Rust
// provider, alternate runtime, or guessed Rust enum/Box layout is required.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#ifndef RT_HOST_U8_STRIDE
#error "runner must provide the RC-pinned native U8 element stride"
#endif
#ifndef DATA_TREE_INT
#error "runner must provide the RC-pinned DataABI tags"
#endif
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
typedef struct { W len, cap; unsigned char *data; } Bytes;
_Static_assert(RT_HOST_U8_STRIDE == 1, "native List<U8> must match packed C u8 storage");
extern unsigned char *jet_rt_bytes_pack(const Bytes *);
extern Bytes *jet_rt_bytes_unpack(const unsigned char *, W);
extern void jet_rt_bytes_free(unsigned char *, W);
extern void *jet_rt_http_body_from_payload(Bytes *, W, Text *);
extern W jet_rt_http_body_take(void *, W, void **);
extern W jet_rt_http_body_next(void *, Bytes **);
extern void jet_rt_http_body_list_drop(Bytes *);
extern W jet_rt_http_body_chunks_drop(void *);
extern W jet_rt_http_body_drop(void *);
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
extern W jet_fs_directory_open(W, Text *, W *, void **);
extern void jet_fs_directory_close(W);
extern W jet_fs_directory_list(W, Bytes **, W *, void **);
extern void jet_terminal8_read_all(Text **, W *, Text **);
typedef struct { W stride, clone, drop, show, equal, compare, kind; const void *child0, *child1; } Meta;
typedef struct { W len, cap; W *data; } Words;
extern W jet_rt_codec_csv(Text *, uintptr_t, const Meta *, Words *, uintptr_t **, W *);
extern W jet_rt_codec_decode_int(void *, W *, uintptr_t **, W *);
extern W jet_dyn_field(void *, Text *, void **, uintptr_t **, W *);
extern void *jet_rt_record_new_dyn_text(Text *);
extern W jet_rt_codec_env(Text *, Text *, const Words *, Text *, uintptr_t, W *, uintptr_t **, W *);
extern W jet_rt_logical_env_set(Text *, Text *);
extern W jet_rt_logical_env_unset(Text *);
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
// A checked Decode callback for `struct { n: Int }`: it borrows the row tree
// and decodes its Text cell, which succeeds only inside the text-cell scope.
static W csv_rows;
static uint64_t csv_int_row(void *tree, W *value, W *tag, uintptr_t **errors, W *count) {
    ++csv_rows;
    Text *name = text("n"); void *cell = NULL;
    assert(jet_dyn_field(tree, name, &cell, errors, count) == 1);
    jet_rt_string_drop(name);
    *tag = jet_rt_codec_decode_int(cell, value, errors, count);
    jet_rt_handle_drop_dyn(cell);
    return 0;
}
static void csv(void) {
    const Meta integer = { .stride = 8, .kind = 1 };
    Words rows = {0}; uintptr_t *errors = NULL; W count = 0;
    Text *s = text("n,extra\n 7 \n\"4\"\"2\"\r\n-3,ignored,more\n");
    csv_rows = 0;
    assert(jet_rt_codec_csv(s, (uintptr_t)csv_int_row, &integer, &rows, &errors, &count) == 0);
    jet_rt_string_drop(s);
    assert(csv_rows == 3 && count == 1);
    text_is(jet_rt_record_get_field_error_path((void *)errors[0]), "row 2");
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "expected Int, found text \"4\\\"2\"");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
    s = text("n\n 7 \n-3");
    csv_rows = 0;
    assert(jet_rt_codec_csv(s, (uintptr_t)csv_int_row, &integer, &rows, &errors, &count) == 1);
    jet_rt_string_drop(s);
    assert(csv_rows == 2 && rows.len == 2 && rows.cap == 2 && rows.data[0] == 7 && rows.data[1] == -3);
    jet_rt_free(rows.data, 2 * (W)sizeof(W), 8);
    // Outside the kernel the same Text cell is the plain "found text" failure.
    s = text("7"); void *cell = jet_rt_record_new_dyn_text(s); W value = 0;
    jet_rt_string_drop(s);
    assert(jet_rt_codec_decode_int(cell, &value, &errors, &count) == 0 && count == 1);
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "expected Int, found text \"7\"");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
    jet_rt_handle_drop_dyn(cell);
    s = text("n\na\"b\n");
    assert(jet_rt_codec_csv(s, (uintptr_t)csv_int_row, &integer, &rows, &errors, &count) == 0 && count == 1);
    jet_rt_string_drop(s);
    text_is(jet_rt_record_get_field_error_path((void *)errors[0]), "");
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "E2701: CSV row 2, line 2, column 2 \xe2\x80\x94 quote inside an unquoted field");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
}
// A checked Decode callback for `struct { port: Int }`: the field's errors
// are framed under `port`, as a derived decoder does.
static uint64_t env_port(void *tree, W *value, W *tag, uintptr_t **errors, W *count) {
    Text *name = text("port"); void *cell = NULL;
    assert(jet_dyn_field(tree, name, &cell, errors, count) == 1);
    jet_rt_string_drop(name);
    *tag = jet_rt_codec_decode_int(cell, value, errors, count);
    jet_rt_handle_drop_dyn(cell);
    if (*tag == 0) {
        uintptr_t *framed = NULL; Text *segment = text("port");
        assert(jet_rt_codec_under(segment, *errors, *count, &framed) == *count);
        jet_rt_string_drop(segment);
        for (W i = 0; i < *count; ++i) jet_rt_handle_drop_field_error((void *)(*errors)[i]);
        jet_rt_free(*errors, *count * (W)sizeof(uintptr_t), 8);
        *errors = framed;
    }
    return 0;
}
static void env(void) {
    Text *key = text("JET_WITNESS_PORT"), *setting = text(" 8080 ");
    assert(jet_rt_logical_env_set(key, setting) == 0);
    jet_rt_string_drop(setting);
    Text *prefix = text("jet_witness_"), *file = text(""), *names = text("PORT=port\n");
    Words allow = {0}; W value = 0; uintptr_t *errors = NULL; W count = 0;
    assert(jet_rt_codec_env(prefix, file, &allow, names, (uintptr_t)env_port, &value, &errors, &count) == 1 && value == 8080);
    setting = text("eighty");
    assert(jet_rt_logical_env_set(key, setting) == 0);
    jet_rt_string_drop(setting);
    assert(jet_rt_codec_env(prefix, file, &allow, names, (uintptr_t)env_port, &value, &errors, &count) == 0 && count == 1);
    text_is(jet_rt_record_get_field_error_path((void *)errors[0]), "port");
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "E2416: env var JET_WITNESS_PORT -> port: expected Int, found text \"eighty\"");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
    assert(jet_rt_logical_env_unset(key) == 1);
    jet_rt_string_drop(key); jet_rt_string_drop(file);
    file = text("/etc/.env");
    assert(jet_rt_codec_env(prefix, file, &allow, names, (uintptr_t)env_port, &value, &errors, &count) == 0 && count == 1);
    text_is(jet_rt_record_get_field_error_reason((void *)errors[0]), "E2416: Dotenv.file must be project-relative");
    jet_rt_handle_drop_field_error((void *)errors[0]); jet_rt_free(errors, (W)sizeof(uintptr_t), 8);
    jet_rt_string_drop(file); jet_rt_string_drop(prefix); jet_rt_string_drop(names);
}
static void line(void *reader, const unsigned char *expected, W size) {
    unsigned char *bytes = NULL; W count = -1; void *error = NULL;
    assert(jet_file_reader_line_bytes_raw(reader, &bytes, &count, &error) == 1 && count == size && error == NULL);
    for (W i = 0; i < size; ++i) assert(bytes[i * RT_HOST_U8_STRIDE] == expected[i]);
    if (size) jet_rt_free(bytes, size * RT_HOST_U8_STRIDE, 8); else assert(bytes == NULL);
}
static void packed_boundary(void) {
    unsigned char input[256];
    for (size_t i = 0; i < sizeof input; ++i) input[i] = (unsigned char)i;
    Bytes borrowed = {256, 256, input};
    unsigned char *packed = jet_rt_bytes_pack(&borrowed);
    assert(packed != input && memcmp(packed, input, sizeof input) == 0);
    Bytes *returned = jet_rt_bytes_unpack(packed, 256);
    assert(returned->len == 256 && returned->cap == 256 && returned->data != packed);
    assert(memcmp(returned->data, input, sizeof input) == 0);
    packed[0] = 99; /* The result owns an independent packed allocation. */
    assert(returned->data[0] == 0 && input[0] == 0);
    jet_rt_bytes_free(packed, 256);
    void *body = jet_rt_http_body_from_payload(returned, 0, text("application/octet-stream"));
    void *chunks = NULL;
    assert(jet_rt_http_body_take(body, 17, &chunks) == 0 && chunks != NULL);
    W offset = 0;
    for (;;) {
        Bytes *chunk = NULL;
        W status = jet_rt_http_body_next(chunks, &chunk);
        if (status == 0) { assert(chunk == NULL); break; }
        assert(status == 1 && chunk != NULL && chunk->len > 0 && chunk->len <= 17);
        assert(chunk->cap == chunk->len && offset + chunk->len <= 256);
        assert(memcmp(chunk->data, input + offset, (size_t)chunk->len) == 0);
        offset += chunk->len;
        jet_rt_http_body_list_drop(chunk);
    }
    assert(offset == 256 && jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
}
static void directory_entries(void) {
    char directory[] = "/tmp/jet-native-directory-XXXXXX";
    assert(mkdtemp(directory) != NULL);
    int fd = open(directory, O_RDONLY | O_DIRECTORY); assert(fd >= 0);
    assert(mkdirat(fd, "sub", 0700) == 0);
    int file = openat(fd, "plain", O_CREAT | O_WRONLY, 0600); assert(file >= 0 && close(file) == 0);
    file = openat(fd, "bad-\377", O_CREAT | O_WRONLY, 0600); assert(file >= 0 && close(file) == 0);
    assert(symlinkat("sub", fd, "link") == 0);
    Text *name = text(directory); W key = 0; void *error = NULL;
    assert(jet_fs_directory_open(-1, name, &key, &error) == 1 && error == NULL);
    jet_rt_string_drop(name);
    for (unsigned pass = 0; pass != 2; ++pass) {
        Bytes *rows = NULL; W count = 0; unsigned found = 0;
        assert(jet_fs_directory_list(key, &rows, &count, &error) == 1 && error == NULL && count == 4);
        for (W i = 0; i != count; ++i) {
            const unsigned char *bytes = rows[i].data; W length = rows[i].len;
            assert(rows[i].cap == length && length > 1);
            if (length == 4 && memcmp(bytes + 1, "sub", 3) == 0) { assert(bytes[0] == 1); found |= 1; }
            else if (length == 6 && memcmp(bytes + 1, "plain", 5) == 0) { assert(bytes[0] == 0); found |= 2; }
            else if (length == 5 && memcmp(bytes + 1, "link", 4) == 0) { assert(bytes[0] == 0); found |= 4; }
            else { assert(length == 6 && bytes[0] == 0 && memcmp(bytes + 1, "bad-\377", 5) == 0); found |= 8; }
            jet_rt_free(rows[i].data, rows[i].cap * RT_HOST_U8_STRIDE, 8);
        }
        assert(found == 15); jet_rt_free(rows, count * (W)sizeof(*rows), 8);
    }
    jet_fs_directory_close(key);
    Bytes *rows = NULL; W count = 0;
    assert(jet_fs_directory_list(key, &rows, &count, &error) == 0 && error != NULL);
    jet_rt_handle_drop_io_error(error); jet_rt_int_drop(key);
    assert(unlinkat(fd, "plain", 0) == 0 && unlinkat(fd, "bad-\377", 0) == 0);
    assert(unlinkat(fd, "link", 0) == 0 && unlinkat(fd, "sub", AT_REMOVEDIR) == 0);
    assert(close(fd) == 0 && rmdir(directory) == 0);
}
static FILE *stdin_bytes(const unsigned char *bytes, size_t length) {
    FILE *file = tmpfile(); assert(file != NULL);
    assert(fwrite(bytes, 1, length, file) == length && fseek(file, 0, SEEK_SET) == 0);
    assert(dup2(fileno(file), STDIN_FILENO) == STDIN_FILENO);
    return file;
}
static void terminal_read_all(void) {
    int saved = dup(STDIN_FILENO); assert(saved >= 0);
    unsigned char bytes[8194]; memset(bytes, 'x', sizeof(bytes));
    memcpy(bytes + 4095, "\360\237\231\202", 4);
    FILE *file = stdin_bytes(bytes, sizeof(bytes));
    Text *value = NULL, *error = NULL; W tag = -1;
    jet_terminal8_read_all(&value, &tag, &error);
    assert(tag == 0 && error == NULL && value->len == (W)sizeof(bytes));
    assert(memcmp(value->data, bytes, sizeof(bytes)) == 0); jet_rt_string_drop(value);
    value = NULL; tag = -1;
    jet_terminal8_read_all(&value, &tag, &error);
    assert(tag == 0 && error == NULL); text_is(value, "");
    assert(fclose(file) == 0);
    const unsigned char invalid[] = { 0xc3 };
    file = stdin_bytes(invalid, sizeof(invalid)); value = NULL; tag = -1;
    jet_terminal8_read_all(&value, &tag, &error);
    assert(tag == 1 && error != NULL); text_is(value, "");
    text_is(error, "stream did not contain valid UTF-8"); error = NULL;
    assert(fclose(file) == 0 && close(STDIN_FILENO) == 0);
    value = NULL; tag = -1;
    jet_terminal8_read_all(&value, &tag, &error);
    assert(tag == 1 && error != NULL && error->len != 0); text_is(value, "");
    jet_rt_string_drop(error);
    assert(dup2(saved, STDIN_FILENO) == STDIN_FILENO && close(saved) == 0);
}
static void exercise(void) {
    packed_boundary();
    directory_entries();
    terminal_read_all();
    codecs();
    csv();
    env();
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
    puts("CHECK OK — Jet-only Dyn encodes, FieldError frames, JSON, CSV and env codecs and shared buffered lines");
    return 0;
}
