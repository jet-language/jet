/* REAL Jet objects only, linked with -Wl,--wrap=jet_rt_free.
 * Layout mirrors DataABI.jet private carriers, never Rust/source/MIR.
 * Rust derive Clone/PartialEq cites: DataPlot.rs:359-366,409-413,585-593,
 * 834-840,885-890,1046-1052,1080-1087,1181-1187. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <math.h>
#include <string.h>

typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; int64_t *data; } Sequence;
extern void jet_rt_alloc_init(void);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_free(int64_t, int64_t, int64_t);
extern void __real_jet_rt_free(int64_t, int64_t, int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
#define DECLARE(T) \
    extern int64_t jet_rt_handle_clone_##T(int64_t); \
    extern void jet_rt_handle_drop_##T(int64_t); \
    extern bool jet_rt_handle_eq_##T(int64_t, int64_t)
DECLARE(JetDataPlotField); DECLARE(JetDataPlotAccessibility);
DECLARE(JetDataPlotLayout); DECLARE(JetDataPlotAxis);
DECLARE(JetDataPlotLegend); DECLARE(JetDataPlotCapability);
DECLARE(JetDataPlotSourceFacts); DECLARE(JetDataPlotValue);
extern int64_t jet_rt_record_new_jet_data_plot_field(Text *, Text *, Text *);
extern int64_t jet_rt_record_new_jet_data_plot_accessibility(Text *, Text *, Text *, uint8_t, uint8_t);
extern int64_t jet_rt_record_new_jet_data_plot_layout(double, double, double, double, double, double);
extern int64_t jet_rt_record_new_jet_data_plot_axis(int64_t, Text *, uint8_t, uint8_t, int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_legend(int64_t, Text *, int64_t, uint8_t);
extern int64_t jet_rt_record_new_jet_data_plot_capability(int64_t, Text *, int64_t, Text *, Text *);
extern int64_t jet_rt_record_new_jet_data_plot_source_facts(Text *, Text *, Text *, Text *, int64_t, Text *, Text *);
extern int64_t jet_rt_record_get_jet_data_plot_axis_ticks(int64_t);
extern int64_t jet_rt_record_get_jet_data_plot_source_facts_rows(int64_t);
extern Text *jet_rt_record_get_jet_data_plot_source_facts_row_type(int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_value_text(Text *);
extern int64_t jet_rt_record_new_jet_data_plot_value_integer(int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_value_number(double);
extern int64_t jet_rt_record_new_jet_data_plot_value_boolean(uint8_t);
extern int64_t jet_rt_record_new_jet_data_plot_value_null(void);
extern int64_t jet_rt_record_get_jet_data_plot_value_integer(int64_t);
extern int64_t jet_rt_record_tag_jet_data_plot_value(int64_t);
extern Sequence *jet_rt_record_seq_list_rec_jet_data_plot_field_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_jet_data_plot_field_len(Sequence *);
extern int64_t jet_rt_record_seq_list_rec_jet_data_plot_field_get(Sequence *, int64_t);
extern void jet_rt_record_seq_list_rec_jet_data_plot_field_push(Sequence *, int64_t);

static struct { int64_t address, bytes, alignment; bool freed; } watched[128];
static size_t watched_count, intercepted;
void __wrap_jet_rt_free(int64_t address, int64_t bytes, int64_t alignment) {
    ++intercepted;
    for (size_t i = 0; i < watched_count; ++i) {
        if (watched[i].address == address) {
            assert(!watched[i].freed);
            assert(watched[i].bytes == bytes && watched[i].alignment == alignment);
            watched[i].freed = true;
        }
    }
    __real_jet_rt_free(address, bytes, alignment);
}
static void watch(int64_t address, int64_t bytes, int64_t alignment) {
    assert(watched_count < 128);
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].address != address);
    watched[watched_count].address = address;
    watched[watched_count].bytes = bytes;
    watched[watched_count].alignment = alignment;
    watched[watched_count++].freed = false;
}
static void watch_text(Text *value) {
    watch((int64_t)(uintptr_t)value, 24, 8);
    if (value->capacity) watch((int64_t)(uintptr_t)value->data, value->capacity, 1);
}
static int64_t *words(int64_t value) { return (int64_t *)(uintptr_t)value; }
static void watch_record(int64_t value, int64_t bytes, uint64_t texts) {
    watch(value, bytes, 8);
    for (int64_t i = 0; i < bytes / 8; ++i)
        if (texts & (UINT64_C(1) << i)) watch_text((Text *)(uintptr_t)words(value)[i]);
}
static void finish(size_t before) {
    assert(intercepted == before + watched_count);
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].freed);
    watched_count = 0;
}
static void pair(int64_t value, int64_t bytes, uint64_t texts,
                 int64_t (*clone)(int64_t), void (*drop)(int64_t),
                 bool (*equal)(int64_t, int64_t)) {
    int64_t copy = clone(value);
    assert(copy != value && equal(value, copy));
    if (texts) {
        size_t i = 0;
        while (!(texts & (UINT64_C(1) << i))) ++i;
        Text *a = (Text *)(uintptr_t)words(value)[i];
        Text *b = (Text *)(uintptr_t)words(copy)[i];
        assert(a != b && a->data != b->data && a->length == b->length);
        unsigned char old = b->data[0];
        b->data[0] = (unsigned char)(old ^ 1);
        assert(a->data[0] == old && !equal(value, copy));
        b->data[0] = old;
    }
    watch_record(value, bytes, texts); watch_record(copy, bytes, texts);
    size_t before = intercepted;
    drop(value);
    assert(equal(copy, copy));
    drop(copy); drop(0);
    finish(before);
}
#define PAIR(T, V, BYTES, MASK) pair(V, BYTES, MASK, jet_rt_handle_clone_##T, jet_rt_handle_drop_##T, jet_rt_handle_eq_##T)
static void scalar_value(int64_t a, int64_t b, bool equal) {
    assert(jet_rt_handle_eq_jet_data_plot_value(a, b) == equal);
    watch_record(a, 16, 0); watch_record(b, 16, 0);
    size_t before = intercepted;
    jet_rt_handle_drop_jet_data_plot_value(a); jet_rt_handle_drop_jet_data_plot_value(b);
    finish(before);
}
int main(void) {
    jet_rt_alloc_init();
    static const unsigned char bytes[] = {'A', 0, 0xc3, 0xa9};
    Text *text = jet_rt_string_from_static(bytes, 4);
    PAIR(JetDataPlotField, jet_rt_record_new_jet_data_plot_field(text, text, text), 24, 7);
    PAIR(JetDataPlotAccessibility, jet_rt_record_new_jet_data_plot_accessibility(text, text, text, 255, 0), 40, 7);
    PAIR(JetDataPlotLayout, jet_rt_record_new_jet_data_plot_layout(320, 200, 10, 20, 30, 40), 48, 0);
    int64_t max = jet_int_owned_from_i64(INT64_MAX);
    int64_t min = jet_int_owned_from_i64(INT64_MIN);
    int64_t axis = jet_rt_record_new_jet_data_plot_axis(1, text, 255, 0, max);
    int64_t ticks = jet_rt_record_get_jet_data_plot_axis_ticks(axis);
    assert(jet_int_owned_to_i64(ticks) == INT64_MAX); jet_rt_int_drop(ticks);
    assert(words(axis)[4] == INT64_MAX); /* raw i64, not an Int handle */
    PAIR(JetDataPlotAxis, axis, 40, 2);
    PAIR(JetDataPlotLegend, jet_rt_record_new_jet_data_plot_legend(2, text, 3, 1), 32, 2);
    PAIR(JetDataPlotCapability, jet_rt_record_new_jet_data_plot_capability(3, text, 1, text, text), 40, 26);
    int64_t source = jet_rt_record_new_jet_data_plot_source_facts(text, text, text, text, min, text, text);
    int64_t rows = jet_rt_record_get_jet_data_plot_source_facts_rows(source);
    assert(jet_int_owned_to_i64(rows) == INT64_MIN); jet_rt_int_drop(rows);
    Text *row_type = jet_rt_record_get_jet_data_plot_source_facts_row_type(source);
    assert(row_type->length == 4 && memcmp(row_type->data, bytes, 4) == 0);
    jet_rt_string_drop(row_type);
    PAIR(JetDataPlotSourceFacts, source, 56, 111);
    int64_t integer = jet_rt_record_new_jet_data_plot_value_integer(max);
    int64_t result = jet_rt_record_get_jet_data_plot_value_integer(integer);
    assert(jet_int_owned_to_i64(result) == INT64_MAX); jet_rt_int_drop(result);
    PAIR(JetDataPlotValue, integer, 16, 0);
    PAIR(JetDataPlotValue, jet_rt_record_new_jet_data_plot_value_text(text), 16, 2);
    int64_t nan = jet_rt_record_new_jet_data_plot_value_number(NAN);
    assert(!jet_rt_handle_eq_jet_data_plot_value(nan, nan));
    scalar_value(nan, jet_rt_handle_clone_jet_data_plot_value(nan), false);
    scalar_value(jet_rt_record_new_jet_data_plot_value_number(-0.0), jet_rt_record_new_jet_data_plot_value_number(0.0), true);
    scalar_value(jet_rt_record_new_jet_data_plot_value_integer(1), jet_rt_record_new_jet_data_plot_value_number(1.0), false);
    scalar_value(jet_rt_record_new_jet_data_plot_value_boolean(255), jet_rt_record_new_jet_data_plot_value_boolean(1), true);
    scalar_value(jet_rt_record_new_jet_data_plot_value_null(), jet_rt_record_new_jet_data_plot_value_null(), true);
    int64_t la = jet_rt_record_new_jet_data_plot_layout(NAN, 1, 2, 3, 4, 5);
    int64_t lb = jet_rt_handle_clone_jet_data_plot_layout(la);
    assert(!jet_rt_handle_eq_jet_data_plot_layout(la, la) && !jet_rt_handle_eq_jet_data_plot_layout(la, lb));
    watch_record(la, 48, 0); watch_record(lb, 48, 0);
    size_t before = intercepted;
    jet_rt_handle_drop_jet_data_plot_layout(la); jet_rt_handle_drop_jet_data_plot_layout(lb); finish(before);
    Sequence *sequence = jet_rt_record_seq_list_rec_jet_data_plot_field_new(0);
    int64_t child = jet_rt_record_new_jet_data_plot_field(text, text, text);
    jet_rt_record_seq_list_rec_jet_data_plot_field_push(sequence, child);
    assert(jet_rt_record_seq_list_rec_jet_data_plot_field_len(sequence) == 1);
    assert(jet_rt_record_seq_list_rec_jet_data_plot_field_get(sequence, 0) == child);
    watch((int64_t)(uintptr_t)sequence, 24, 8);
    watch((int64_t)(uintptr_t)sequence->data, sequence->capacity * 8, 8);
    watch_record(child, 24, 7);
    before = intercepted;
    /* Caller owns this unattached temporary holder. Schema consumes it in slice3. */
    jet_rt_handle_drop_jet_data_plot_field(child);
    jet_rt_free((int64_t)(uintptr_t)sequence->data, sequence->capacity * 8, 8);
    jet_rt_free((int64_t)(uintptr_t)sequence, 24, 8); finish(before);
    jet_rt_int_drop(max); jet_rt_int_drop(min); jet_rt_string_drop(text);
    assert(intercepted > 100);
    puts("DataPlot flat witness: 8 families, independent text/Int getters, IEEE equality, exact-once frees");
    return 0;
}
