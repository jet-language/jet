// Link the real Jet runtime object with -Wl,--wrap=jet_rt_free.
// This is a client, not a mock/provider. Layouts are DataABI's native carriers.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct {
    int64_t buffer_bytes, max_depth, max_item_bytes;
    int64_t total_present, max_total_bytes, max_expansion_depth, max_expansion_bytes;
} EncodingLimits;
typedef struct {
    EncodingLimits *encoding;
    int64_t max_groups, max_sort_rows, max_join_rows, max_output_rows;
} DataLimits;
_Static_assert(sizeof(EncodingLimits) == 56 && offsetof(EncodingLimits, max_total_bytes) == 32 && offsetof(EncodingLimits, max_expansion_bytes) == 48, "EncodingLimits native ABI");
_Static_assert(sizeof(DataLimits) == 40 && offsetof(DataLimits, max_output_rows) == 32, "DataLimits native ABI");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern EncodingLimits *jet_rt_handle_clone_encoding_limits(EncodingLimits *);
extern void jet_rt_handle_drop_encoding_limits(EncodingLimits *);
extern bool jet_rt_handle_eq_encoding_limits(EncodingLimits *, EncodingLimits *);
extern EncodingLimits *jet_rt_record_new_encoding_limits(int64_t, int64_t, int64_t, const int64_t *, int64_t, int64_t);
extern int64_t jet_rt_record_get_encoding_limits_buffer_bytes(EncodingLimits *);
extern int64_t jet_rt_record_get_encoding_limits_max_depth(EncodingLimits *);
extern int64_t jet_rt_record_get_encoding_limits_max_item_bytes(EncodingLimits *);
extern int64_t jet_rt_record_get_encoding_limits_max_total_bytes(EncodingLimits *, int64_t *);
extern int64_t jet_rt_record_get_encoding_limits_max_expansion_depth(EncodingLimits *);
extern int64_t jet_rt_record_get_encoding_limits_max_expansion_bytes(EncodingLimits *);
extern DataLimits *jet_rt_handle_clone_data_limits(DataLimits *);
extern void jet_rt_handle_drop_data_limits(DataLimits *);
extern bool jet_rt_handle_eq_data_limits(DataLimits *, DataLimits *);
extern DataLimits *jet_rt_record_new_data_limits(EncodingLimits *, int64_t, int64_t, int64_t, int64_t);
extern EncodingLimits *jet_rt_record_get_data_limits_encoding(DataLimits *);
extern int64_t jet_rt_record_get_data_limits_max_groups(DataLimits *);
extern int64_t jet_rt_record_get_data_limits_max_sort_rows(DataLimits *);
extern int64_t jet_rt_record_get_data_limits_max_join_rows(DataLimits *);
extern int64_t jet_rt_record_get_data_limits_max_output_rows(DataLimits *);
static struct { void *pointer; int64_t size; unsigned frees; } watched[8];
static size_t watched_count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < watched_count; ++i) if (watched[i].pointer == pointer) {
        assert(size == watched[i].size && align == 8);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, size, align);
}
static void watch(void *pointer, int64_t size) {
    assert(watched_count < sizeof(watched) / sizeof(watched[0]));
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].pointer != pointer);
    watched[watched_count].pointer = pointer;
    watched[watched_count].size = size;
    watched[watched_count++].frees = 0;
}
static void watch_done(void) {
    assert(intercepted > 0); // Missing --wrap cannot silently pass.
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].frees == 1);
    watched_count = 0;
}
static void expect_int(int64_t owned, int64_t raw) {
    assert(jet_int_owned_to_i64(owned) == raw);
    jet_rt_int_drop(owned);
}
static EncodingLimits *safe(void) {
    return jet_rt_record_new_encoding_limits(65536,256,16777216,NULL,32,8388608);
}
static void encoding_cases(void) {
    EncodingLimits *a = safe(), *b = jet_rt_handle_clone_encoding_limits(a);
    assert(a != b && jet_rt_handle_eq_encoding_limits(a,b));
    expect_int(jet_rt_record_get_encoding_limits_buffer_bytes(a),65536);
    expect_int(jet_rt_record_get_encoding_limits_max_depth(a),256);
    expect_int(jet_rt_record_get_encoding_limits_max_item_bytes(a),16777216);
    expect_int(jet_rt_record_get_encoding_limits_max_expansion_depth(a),32);
    expect_int(jet_rt_record_get_encoding_limits_max_expansion_bytes(a),8388608);
    int64_t out = 42;
    assert(jet_rt_record_get_encoding_limits_max_total_bytes(a,&out) == 0 && out == 42);
    // Every raw field participates, except an absent total's unused word.
    int64_t *fields[] = {&b->buffer_bytes,&b->max_depth,&b->max_item_bytes,&b->total_present,&b->max_expansion_depth,&b->max_expansion_bytes};
    for (size_t i = 0; i < sizeof(fields) / sizeof(fields[0]); ++i) {
        ++*fields[i]; assert(!jet_rt_handle_eq_encoding_limits(a,b)); --*fields[i];
    }
    b->max_total_bytes = 999; assert(jet_rt_handle_eq_encoding_limits(a,b));
    watch(a,56); watch(b,56);
    jet_rt_handle_drop_encoding_limits(a); jet_rt_handle_drop_encoding_limits(b); watch_done();
    int64_t zero = 0;
    a = jet_rt_record_new_encoding_limits(1,2,3,&zero,4,5); b = jet_rt_handle_clone_encoding_limits(a);
    assert(jet_rt_record_get_encoding_limits_max_total_bytes(a,&out) == 1 && out == 0);
    jet_rt_int_drop(out);
    ++b->max_total_bytes; assert(!jet_rt_handle_eq_encoding_limits(a,b)); --b->max_total_bytes;
    watch(a,56); watch(b,56);
    jet_rt_handle_drop_encoding_limits(a); jet_rt_handle_drop_encoding_limits(b); watch_done();
    // Borrowed boxed arguments are not retained; getters produce fresh owners.
    int64_t hi = jet_int_owned_from_i64(INT64_MAX), lo = jet_int_owned_from_i64(INT64_MIN);
    a = jet_rt_record_new_encoding_limits(hi,lo,hi,&hi,lo,hi);
    assert(a->buffer_bytes == INT64_MAX && a->max_depth == INT64_MIN && a->max_total_bytes == INT64_MAX);
    int64_t first, second;
    assert(jet_rt_record_get_encoding_limits_max_total_bytes(a,&first) == 1);
    assert(jet_rt_record_get_encoding_limits_max_total_bytes(a,&second) == 1);
    assert(first != hi && second != hi && first != second);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    expect_int(first,INT64_MAX); expect_int(second,INT64_MAX);
    expect_int(jet_rt_record_get_encoding_limits_buffer_bytes(a),INT64_MAX);
    expect_int(jet_rt_record_get_encoding_limits_max_depth(a),INT64_MIN);
    expect_int(jet_rt_record_get_encoding_limits_max_item_bytes(a),INT64_MAX);
    expect_int(jet_rt_record_get_encoding_limits_max_expansion_depth(a),INT64_MIN);
    expect_int(jet_rt_record_get_encoding_limits_max_expansion_bytes(a),INT64_MAX);
    b = jet_rt_handle_clone_encoding_limits(a); assert(jet_rt_handle_eq_encoding_limits(a,b));
    watch(a,56); watch(b,56);
    jet_rt_handle_drop_encoding_limits(a); jet_rt_handle_drop_encoding_limits(b); watch_done();
    jet_rt_handle_drop_encoding_limits(NULL);
}
static void data_cases(void) {
    int64_t hi = jet_int_owned_from_i64(INT64_MAX), lo = jet_int_owned_from_i64(INT64_MIN);
    EncodingLimits *child = safe();
    DataLimits *a = jet_rt_record_new_data_limits(child,hi,lo,hi,lo);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    DataLimits *b = jet_rt_handle_clone_data_limits(a);
    EncodingLimits *got = jet_rt_record_get_data_limits_encoding(a);
    assert(a->encoding == child && a != b && b->encoding != child && got != child && got != b->encoding);
    assert(jet_rt_handle_eq_data_limits(a,b) && jet_rt_handle_eq_encoding_limits(child,got));
    expect_int(jet_rt_record_get_data_limits_max_groups(a),INT64_MAX);
    expect_int(jet_rt_record_get_data_limits_max_sort_rows(a),INT64_MIN);
    expect_int(jet_rt_record_get_data_limits_max_join_rows(a),INT64_MAX);
    expect_int(jet_rt_record_get_data_limits_max_output_rows(a),INT64_MIN);
    --b->encoding->buffer_bytes; assert(!jet_rt_handle_eq_data_limits(a,b)); ++b->encoding->buffer_bytes;
    int64_t *fields[] = {&b->max_groups,&b->max_sort_rows,&b->max_join_rows,&b->max_output_rows};
    for (size_t i = 0; i < sizeof(fields) / sizeof(fields[0]); ++i) {
        int64_t saved = *fields[i]; *fields[i] = 0;
        assert(!jet_rt_handle_eq_data_limits(a,b)); *fields[i] = saved;
    }
    watch(a,40); watch(child,56); watch(b,40); watch(b->encoding,56); watch(got,56);
    jet_rt_handle_drop_data_limits(a);
    expect_int(jet_rt_record_get_encoding_limits_buffer_bytes(got),65536);
    assert(b->encoding->buffer_bytes == 65536);
    jet_rt_handle_drop_encoding_limits(got); jet_rt_handle_drop_data_limits(b); watch_done();
    jet_rt_handle_drop_data_limits(NULL);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc > 1) {
        int64_t edge = jet_int_owned_from_i64(INT64_MAX);
        int64_t too_big = jet_int_add(edge,1); jet_rt_int_drop(edge);
        if (strcmp(argv[1],"encoding-overflow") == 0) {
            (void)jet_rt_record_new_encoding_limits(1,2,3,&too_big,4,5);
        } else if (strcmp(argv[1],"encoding-field-overflow") == 0) {
            (void)jet_rt_record_new_encoding_limits(too_big,2,3,NULL,4,5);
        } else if (strcmp(argv[1],"data-overflow") == 0) {
            (void)jet_rt_record_new_data_limits(safe(),too_big,2,3,4);
        } else { assert(!"unknown witness mode"); }
        assert(!"native Int argument exceeds host range must stop");
    }
    encoding_cases(); data_cases();
    puts("DataLimits/EncodingLimits ownership/equality/boundary witness OK");
    return 0;
}
