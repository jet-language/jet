/* Link REAL composed Jet providers including RtBurn1 Limits with BOTH
 * -Wl,--wrap=jet_rt_alloc -Wl,--wrap=jet_rt_free. No mock providers.
 * Client-created private DP carriers mirror DataABI only, not Rust/MIR.
 * Rust derive/declarations: DataPlot.rs:1377-1392,2256-2267,2373-2386.
 * Every allocation in the scope must be released exactly once at its size/
 * alignment. Limits are opaque: the witness never reads their field layout. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; int64_t *data; } Sequence;
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern int64_t __real_jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(int64_t, int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
#define DECLARE(T) \
    extern int64_t jet_rt_handle_clone_##T(int64_t); \
    extern void jet_rt_handle_drop_##T(int64_t); \
    extern bool jet_rt_handle_eq_##T(int64_t, int64_t)
DECLARE(JetDataPlotField); DECLARE(JetDataPlotSourceFacts);
DECLARE(JetDataPlotPlan); DECLARE(JetDataPlotSelectedRow);
DECLARE(JetDataPlotInspection); DECLARE(JetDataPlotProjection); DECLARE(JetDataPlotRender);
extern int64_t jet_rt_record_new_jet_data_plot_field(Text *, Text *, Text *);
extern int64_t jet_rt_record_new_jet_data_plot_value_number(double);
extern int64_t jet_rt_record_new_jet_data_plot_value_text(Text *);
extern int64_t jet_rt_record_new_jet_data_plot_source_facts(Text *, Text *, Text *, Text *, int64_t, Text *, Text *);
extern int64_t jet_rt_record_new_jet_data_plot_schema(Text *, Text *, Sequence *);
extern int64_t jet_rt_record_new_jet_data_plot_encoding(int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_axis(int64_t, Text *, uint8_t, uint8_t, int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_legend(int64_t, Text *, int64_t, uint8_t);
extern int64_t jet_rt_record_new_jet_data_plot_facet(int64_t, int64_t, Text *, int64_t, int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_accessibility(Text *, Text *, Text *, uint8_t, uint8_t);
extern int64_t jet_rt_record_new_jet_data_plot_layout(double, double, double, double, double, double);
extern int64_t jet_rt_record_new_jet_data_plot_capability(int64_t, Text *, int64_t, Text *, Text *);
extern int64_t jet_rt_record_new_encoding_limits(int64_t, int64_t, int64_t, const int64_t *, int64_t, int64_t);
extern int64_t jet_rt_record_new_data_limits(int64_t, int64_t, int64_t, int64_t, int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_plan_source(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_plan_schema(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_plan_limits(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_plan_accessibility(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_plan_layout(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_inspection_plan(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_projection_plan(int64_t);
extern int64_t jet_rt_handle_field_jet_data_plot_projection_source(int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_render(int64_t, int64_t, Text *, int64_t, Sequence *);
extern Text *jet_rt_record_get_jet_data_plot_render_body(int64_t);
extern int64_t jet_rt_record_get_jet_data_plot_render_source(int64_t);
extern Sequence *jet_rt_record_get_jet_data_plot_render_capabilities(int64_t);
extern Sequence *jet_rt_record_seq_list_rec_jet_data_plot_selected_row_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_jet_data_plot_selected_row_len(Sequence *);
extern int64_t jet_rt_record_seq_list_rec_jet_data_plot_selected_row_get(Sequence *, int64_t);
extern void jet_rt_record_seq_list_rec_jet_data_plot_selected_row_push(Sequence *, int64_t);

static struct { int64_t address, bytes, align; bool freed; } allocations[4096];
static size_t count, frees;
static bool active;
int64_t __wrap_jet_rt_alloc(int64_t bytes, int64_t align) {
    int64_t p = __real_jet_rt_alloc(bytes, align);
    if (active) {
        assert(count < 4096);
        for (size_t i = 0; i < count; ++i) assert(allocations[i].freed || allocations[i].address != p);
        allocations[count].address = p; allocations[count].bytes = bytes;
        allocations[count].align = align; allocations[count++].freed = false;
    }
    return p;
}
void __wrap_jet_rt_free(int64_t p, int64_t bytes, int64_t align) {
    if (active) {
        size_t i = count;
        while (i && (allocations[i - 1].address != p || allocations[i - 1].freed)) --i;
        assert(i > 0); --i;
        assert(allocations[i].bytes == bytes && allocations[i].align == align);
        allocations[i].freed = true; ++frees;
    }
    __real_jet_rt_free(p, bytes, align);
}
static int64_t pointer(const void *p) { return (int64_t)(uintptr_t)p; }
static int64_t *words(int64_t p) { return (int64_t *)(uintptr_t)p; }
static int64_t record(size_t n) {
    int64_t p = jet_rt_alloc((int64_t)n * 8, 8);
    for (size_t i = 0; i < n; ++i) words(p)[i] = 0;
    return p;
}
static Sequence *list(const int64_t *values, size_t count_items, size_t stride_words) {
    Sequence *s = (Sequence *)(uintptr_t)record(3);
    s->length = s->capacity = (int64_t)count_items;
    s->data = count_items ? (int64_t *)(uintptr_t)jet_rt_alloc((int64_t)(count_items * stride_words) * 8, 8) : (int64_t *)(uintptr_t)8;
    for (size_t i = 0; i < count_items * stride_words; ++i) s->data[i] = values[i];
    return s;
}
static int64_t bits(double x) { int64_t result; memcpy(&result, &x, 8); return result; }
static int64_t field(Text *text) { return jet_rt_record_new_jet_data_plot_field(text, text, text); }
static int64_t source(Text *text) { return jet_rt_record_new_jet_data_plot_source_facts(text, text, text, text, 1, text, text); }
static int64_t capability(Text *text) { return jet_rt_record_new_jet_data_plot_capability(0, text, 0, text, text); }
static int64_t plan(Text *text) {
    int64_t p = record(14);
    words(p)[0] = source(text);
    int64_t fields[] = {field(text)};
    words(p)[1] = jet_rt_record_new_jet_data_plot_schema(text, text, list(fields, 1, 1));
    /* Both C constructors consume only the nested owner, borrow scalar args. */
    int64_t encoding_limits = jet_rt_record_new_encoding_limits(64, 4, 64, NULL, 4, 64);
    words(p)[2] = jet_rt_record_new_data_limits(encoding_limits, 8, 8, 8, 8);
    words(p)[3] = 0;
    int64_t encodings[] = {jet_rt_record_new_jet_data_plot_encoding(0, field(text), 0)};
    words(p)[4] = pointer(list(encodings, 1, 1));
    int64_t transform = record(4); words(transform)[1] = field(text); words(transform)[2] = 0; words(transform)[3] = jet_rt_record_new_jet_data_plot_value_number(1);
    words(p)[5] = pointer(list(&transform, 1, 1));
    int64_t domain = record(3); words(domain)[0] = 1; words(domain)[1] = bits(0); words(domain)[2] = bits(1);
    int64_t scale = record(5); words(scale)[2] = domain; words(scale)[3] = 1;
    words(p)[6] = pointer(list(&scale, 1, 1));
    int64_t axis = jet_rt_record_new_jet_data_plot_axis(0, text, 1, 1, 4);
    words(p)[7] = pointer(list(&axis, 1, 1));
    int64_t legend = jet_rt_record_new_jet_data_plot_legend(2, text, 1, 1);
    words(p)[8] = pointer(list(&legend, 1, 1));
    int64_t facet = jet_rt_record_new_jet_data_plot_facet(field(text), 0, text, 1, 1);
    words(p)[9] = pointer(list(&facet, 1, 1));
    int64_t layer = record(5); words(layer)[0] = pointer(jet_rt_string_clone(text)); words(layer)[1] = 1;
    int64_t layer_enc = jet_rt_record_new_jet_data_plot_encoding(1, field(text), 0);
    words(layer)[2] = pointer(list(&layer_enc, 1, 1)); words(layer)[3] = pointer(list(NULL, 0, 1)); words(layer)[4] = bits(1);
    words(p)[10] = pointer(list(&layer, 1, 1));
    int64_t interactions[] = {0, 1}; words(p)[11] = pointer(list(interactions, 2, 1));
    words(p)[12] = jet_rt_record_new_jet_data_plot_accessibility(text, text, text, 1, 1);
    words(p)[13] = jet_rt_record_new_jet_data_plot_layout(320, 200, 10, 20, 30, 40);
    return p;
}
static int64_t selected(Text *text, double value) {
    int64_t p = record(2); words(p)[0] = INT64_MIN;
    int64_t tuples[] = {field(text), jet_rt_record_new_jet_data_plot_value_number(value), field(text), jet_rt_record_new_jet_data_plot_value_text(text)};
    words(p)[1] = pointer(list(tuples, 2, 2));
    return p;
}
int main(void) {
    jet_rt_alloc_init(); active = true;
    Text *text = jet_rt_string_from_static("row", 3);
    int64_t row = selected(text, 1); int64_t row_copy = jet_rt_handle_clone_jet_data_plot_selected_row(row);
    assert(jet_rt_handle_eq_jet_data_plot_selected_row(row, row_copy));
    Sequence *tuples = (Sequence *)(uintptr_t)words(row_copy)[1];
    int64_t temp_field = tuples->data[0], temp_value = tuples->data[1];
    tuples->data[0] = tuples->data[2]; tuples->data[1] = tuples->data[3]; tuples->data[2] = temp_field; tuples->data[3] = temp_value;
    assert(!jet_rt_handle_eq_jet_data_plot_selected_row(row, row_copy));
    jet_rt_handle_drop_jet_data_plot_selected_row(row); jet_rt_handle_drop_jet_data_plot_selected_row(row_copy);
    row = selected(text, NAN); assert(!jet_rt_handle_eq_jet_data_plot_selected_row(row, row)); jet_rt_handle_drop_jet_data_plot_selected_row(row);
    int64_t p = plan(text); int64_t copy = jet_rt_handle_clone_jet_data_plot_plan(p);
    assert(jet_rt_handle_eq_jet_data_plot_plan(p, copy));
    assert(jet_rt_handle_field_jet_data_plot_plan_source(p) == words(p)[0]);
    assert(jet_rt_handle_field_jet_data_plot_plan_schema(p) == words(p)[1]);
    assert(jet_rt_handle_field_jet_data_plot_plan_limits(p) == words(p)[2]);
    assert(jet_rt_handle_field_jet_data_plot_plan_accessibility(p) == words(p)[12]);
    assert(jet_rt_handle_field_jet_data_plot_plan_layout(p) == words(p)[13]);
    assert(words(copy)[2] != words(p)[2]); /* opaque Limits ownership is independent */
    Sequence *interaction_copy = (Sequence *)(uintptr_t)words(copy)[11];
    interaction_copy->data[0] = 1; assert(!jet_rt_handle_eq_jet_data_plot_plan(p, copy)); interaction_copy->data[0] = 0;
    jet_rt_handle_drop_jet_data_plot_plan(p); assert(jet_rt_handle_eq_jet_data_plot_plan(copy, copy)); jet_rt_handle_drop_jet_data_plot_plan(copy);
    int64_t inspection = record(4); words(inspection)[0] = plan(text);
    int64_t indices[] = {INT64_MIN, INT64_MAX}; words(inspection)[1] = pointer(list(indices, 2, 1));
    int64_t columns[] = {field(text)}; words(inspection)[2] = pointer(list(columns, 1, 1));
    Sequence *selected_rows = jet_rt_record_seq_list_rec_jet_data_plot_selected_row_new(0);
    row = selected(text, 2); jet_rt_record_seq_list_rec_jet_data_plot_selected_row_push(selected_rows, row);
    assert(jet_rt_record_seq_list_rec_jet_data_plot_selected_row_len(selected_rows) == 1);
    assert(jet_rt_record_seq_list_rec_jet_data_plot_selected_row_get(selected_rows, 0) == row);
    words(inspection)[3] = pointer(selected_rows);
    int64_t inspection_copy = jet_rt_handle_clone_jet_data_plot_inspection(inspection);
    assert(jet_rt_handle_eq_jet_data_plot_inspection(inspection, inspection_copy));
    assert(jet_rt_handle_field_jet_data_plot_inspection_plan(inspection) == words(inspection)[0]);
    Sequence *index_copy = (Sequence *)(uintptr_t)words(inspection_copy)[1];
    index_copy->data[1] = 0; assert(!jet_rt_handle_eq_jet_data_plot_inspection(inspection, inspection_copy)); index_copy->data[1] = INT64_MAX;
    jet_rt_handle_drop_jet_data_plot_inspection(inspection); assert(jet_rt_handle_eq_jet_data_plot_inspection(inspection_copy, inspection_copy)); jet_rt_handle_drop_jet_data_plot_inspection(inspection_copy);
    int64_t projection = record(4); words(projection)[0] = 1; words(projection)[1] = plan(text); words(projection)[2] = source(text);
    int64_t cap = capability(text); words(projection)[3] = pointer(list(&cap, 1, 1));
    int64_t projection_copy = jet_rt_handle_clone_jet_data_plot_projection(projection);
    assert(jet_rt_handle_eq_jet_data_plot_projection(projection, projection_copy));
    assert(jet_rt_handle_field_jet_data_plot_projection_plan(projection) == words(projection)[1]);
    assert(jet_rt_handle_field_jet_data_plot_projection_source(projection) == words(projection)[2]);
    jet_rt_handle_drop_jet_data_plot_projection(projection); assert(jet_rt_handle_eq_jet_data_plot_projection(projection_copy, projection_copy)); jet_rt_handle_drop_jet_data_plot_projection(projection_copy);
    cap = capability(text);
    int64_t render = jet_rt_record_new_jet_data_plot_render(3, 1, text, source(text), list(&cap, 1, 1));
    int64_t render_copy = jet_rt_handle_clone_jet_data_plot_render(render);
    assert(jet_rt_handle_eq_jet_data_plot_render(render, render_copy));
    Text *body = jet_rt_record_get_jet_data_plot_render_body(render);
    assert(body->length == 3 && memcmp(body->data, "row", 3) == 0); jet_rt_string_drop(body);
    int64_t owned_source = jet_rt_record_get_jet_data_plot_render_source(render);
    assert(owned_source != words(render)[3]); jet_rt_handle_drop_jet_data_plot_source_facts(owned_source);
    assert(pointer(jet_rt_record_get_jet_data_plot_render_capabilities(render)) == words(render)[4]);
    jet_rt_handle_drop_jet_data_plot_render(render); assert(jet_rt_handle_eq_jet_data_plot_render(render_copy, render_copy)); jet_rt_handle_drop_jet_data_plot_render(render_copy);
    jet_rt_handle_drop_jet_data_plot_selected_row(0); jet_rt_handle_drop_jet_data_plot_plan(0);
    jet_rt_handle_drop_jet_data_plot_inspection(0); jet_rt_handle_drop_jet_data_plot_projection(0); jet_rt_handle_drop_jet_data_plot_render(0);
    jet_rt_string_drop(text);
    assert(count > 100 && frees == count);
    for (size_t i = 0; i < count; ++i) assert(allocations[i].freed);
    active = false;
    puts("DataPlot deep witness: 5 families, tuple/Int/list order, opaque Limits ownership, exact-once frees");
    return 0;
}
