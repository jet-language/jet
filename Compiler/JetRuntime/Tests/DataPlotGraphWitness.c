/* Real Jet provider only: -Wl,--wrap=jet_rt_free.
 * Private client-created carriers mirror DataABI, NOT Rust or MIR layouts.
 * Derive policy: DataPlot.rs:481-485,662-666,692-711,772-804,929-967. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <math.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; int64_t *data; } Sequence;
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(int64_t, int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
#define DECLARE(T) \
    extern int64_t jet_rt_handle_clone_##T(int64_t); \
    extern void jet_rt_handle_drop_##T(int64_t); \
    extern bool jet_rt_handle_eq_##T(int64_t, int64_t)
DECLARE(JetDataPlotField); DECLARE(JetDataPlotValue);
DECLARE(JetDataPlotSchema); DECLARE(JetDataPlotEncoding);
DECLARE(JetDataPlotFacet); DECLARE(JetDataPlotDomain);
DECLARE(JetDataPlotScale); DECLARE(JetDataPlotLayer);
DECLARE(JetDataPlotError);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t jet_rt_record_new_JetDataPlotError(int64_t, Text *, Text *const *, Text *const *, Text *const *, Text *const *, Text *const *, const int64_t *, Text *);
extern int64_t jet_rt_record_get_JetDataPlotError_field(int64_t, Text **);
extern int64_t jet_rt_record_get_JetDataPlotError_channel(int64_t, Text **);
extern int64_t jet_rt_record_get_JetDataPlotError_mark(int64_t, Text **);
extern int64_t jet_rt_record_get_JetDataPlotError_expected(int64_t, Text **);
extern int64_t jet_rt_record_get_JetDataPlotError_actual(int64_t, Text **);
extern int64_t jet_rt_record_get_JetDataPlotError_index(int64_t, int64_t *);
extern int64_t jet_rt_record_new_JetDataPlotField(Text *, Text *, Text *);
extern int64_t jet_rt_record_new_JetDataPlotValue_Number(double);
extern int64_t jet_rt_record_new_JetDataPlotSchema(Text *, Text *, Sequence *);
extern Sequence *jet_rt_record_get_JetDataPlotSchema_columns(int64_t);
extern int64_t jet_rt_record_new_JetDataPlotEncoding(int64_t, int64_t, int64_t);
extern int64_t jet_rt_record_get_JetDataPlotEncoding_field(int64_t);
extern int64_t jet_rt_record_new_JetDataPlotFacet(int64_t, int64_t, Text *, int64_t, int64_t);
extern int64_t jet_rt_handle_field_JetDataPlotScale_domain(int64_t);
extern const bool *jet_rt_handle_field_JetDataPlotScale_clamp(int64_t);
extern const bool *jet_rt_handle_field_JetDataPlotScale_reverse(int64_t);
extern void jet_rt_handle_field_JetDataPlotLayer_name(int64_t, Text *);
extern const double *jet_rt_handle_field_JetDataPlotLayer_opacity(int64_t);

static struct { int64_t address, size, align; bool freed; } watched[512];
static size_t watched_count, calls;
void __wrap_jet_rt_free(int64_t address, int64_t size, int64_t align) {
    ++calls;
    for (size_t i = 0; i < watched_count; ++i) if (watched[i].address == address) {
        assert(!watched[i].freed && watched[i].size == size && watched[i].align == align);
        watched[i].freed = true;
    }
    __real_jet_rt_free(address, size, align);
}
static int64_t pointer(const void *p) { return (int64_t)(uintptr_t)p; }
static int64_t *words(int64_t p) { return (int64_t *)(uintptr_t)p; }
static void watch(int64_t p, int64_t size, int64_t align) {
    assert(watched_count < 512);
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].address != p);
    watched[watched_count].address = p; watched[watched_count].size = size;
    watched[watched_count].align = align; watched[watched_count++].freed = false;
}
static void text_watch(int64_t p) {
    Text *t = (Text *)(uintptr_t)p; watch(p, 24, 8);
    if (t->capacity) watch(pointer(t->data), t->capacity, 1);
}
static void field_watch(int64_t p) {
    watch(p, 24, 8); for (size_t i = 0; i < 3; ++i) text_watch(words(p)[i]);
}
static void value_watch(int64_t p) {
    watch(p, 16, 8); if (words(p)[0] == 0) text_watch(words(p)[1]);
}
static void seq_watch(int64_t p, void (*child)(int64_t)) {
    Sequence *s = (Sequence *)(uintptr_t)p; watch(p, 24, 8);
    if (s->capacity) watch(pointer(s->data), s->capacity * 8, 8);
    for (int64_t i = 0; i < s->length; ++i) child(s->data[i]);
}
static void encoding_watch(int64_t p) { watch(p, 24, 8); field_watch(words(p)[1]); }
static void schema_watch(int64_t p) { watch(p, 24, 8); text_watch(words(p)[0]); text_watch(words(p)[1]); seq_watch(words(p)[2], field_watch); }
static void facet_watch(int64_t p) { watch(p, 40, 8); field_watch(words(p)[0]); text_watch(words(p)[2]); }
static void domain_watch(int64_t p) { watch(p, 24, 8); if (words(p)[0] == 2) seq_watch(words(p)[1], text_watch); }
static void scale_watch(int64_t p) { watch(p, 40, 8); domain_watch(words(p)[2]); }
static void transform_watch(int64_t p) {
    watch(p, 32, 8);
    if (words(p)[0] == 3) { seq_watch(words(p)[1], field_watch); field_watch(words(p)[2]); }
    else { field_watch(words(p)[1]); if (words(p)[0] == 0) value_watch(words(p)[3]); }
}
static void layer_watch(int64_t p) {
    watch(p, 40, 8); text_watch(words(p)[0]);
    seq_watch(words(p)[2], encoding_watch); seq_watch(words(p)[3], transform_watch);
}
static void error_watch(int64_t p) {
    watch(p, 80, 8); text_watch(words(p)[1]);
    for (size_t i = 2; i <= 6; ++i) if (words(p)[i]) text_watch(words(p)[i]);
    text_watch(words(p)[9]);
}
static void finish(size_t before) {
    assert(calls == before + watched_count);
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].freed);
    watched_count = 0;
}
static void drop_pair(int64_t a, int64_t b, void (*visit)(int64_t), void (*drop)(int64_t)) {
    visit(a); visit(b); size_t before = calls;
    drop(a); drop(b); drop(0); finish(before);
}
static int64_t record(size_t count) {
    int64_t p = jet_rt_alloc((int64_t)count * 8, 8);
    for (size_t i = 0; i < count; ++i) words(p)[i] = 0;
    return p;
}
static Sequence *sequence(const int64_t *values, size_t count) {
    int64_t p = record(3);
    Sequence *s = (Sequence *)(uintptr_t)p; s->length = s->capacity = (int64_t)count;
    s->data = count ? (int64_t *)(uintptr_t)jet_rt_alloc((int64_t)count * 8, 8) : (int64_t *)(uintptr_t)8;
    for (size_t i = 0; i < count; ++i) s->data[i] = values[i];
    return s;
}
static int64_t number_bits(double x) { int64_t bits; memcpy(&bits, &x, 8); return bits; }
static int64_t field(Text *text) { return jet_rt_record_new_JetDataPlotField(text, text, text); }
static int64_t domain(int64_t kind, int64_t a, int64_t b) { int64_t p = record(3); words(p)[0] = kind; words(p)[1] = a; words(p)[2] = b; return p; }
int main(void) {
    jet_rt_alloc_init(); Text *a = jet_rt_string_from_static("a", 1); Text *b = jet_rt_string_from_static("b", 1);
    int64_t fields[] = {field(a), field(b)};
    int64_t schema = jet_rt_record_new_JetDataPlotSchema(a, a, sequence(fields, 2));
    int64_t schema_copy = jet_rt_handle_clone_JetDataPlotSchema(schema);
    assert(jet_rt_handle_eq_JetDataPlotSchema(schema, schema_copy));
    Sequence *copy_columns = jet_rt_record_get_JetDataPlotSchema_columns(schema_copy);
    int64_t tmp = copy_columns->data[0]; copy_columns->data[0] = copy_columns->data[1]; copy_columns->data[1] = tmp;
    assert(!jet_rt_handle_eq_JetDataPlotSchema(schema, schema_copy));
    tmp = copy_columns->data[0]; copy_columns->data[0] = copy_columns->data[1]; copy_columns->data[1] = tmp;
    drop_pair(schema, schema_copy, schema_watch, jet_rt_handle_drop_JetDataPlotSchema);
    int64_t encoding = jet_rt_record_new_JetDataPlotEncoding(1, field(a), 2);
    int64_t encoding_copy = jet_rt_handle_clone_JetDataPlotEncoding(encoding);
    int64_t owned = jet_rt_record_get_JetDataPlotEncoding_field(encoding);
    assert(owned != words(encoding)[1] && jet_rt_handle_eq_JetDataPlotField(owned, words(encoding)[1]));
    jet_rt_handle_drop_JetDataPlotField(owned);
    assert(jet_rt_handle_eq_JetDataPlotEncoding(encoding, encoding_copy));
    drop_pair(encoding, encoding_copy, encoding_watch, jet_rt_handle_drop_JetDataPlotEncoding);
    int64_t facet = jet_rt_record_new_JetDataPlotFacet(field(a), 1, a, 2, 3);
    int64_t facet_copy = jet_rt_handle_clone_JetDataPlotFacet(facet);
    assert(jet_rt_handle_eq_JetDataPlotFacet(facet, facet_copy));
    words(facet_copy)[4] = 4; assert(!jet_rt_handle_eq_JetDataPlotFacet(facet, facet_copy));
    drop_pair(facet, facet_copy, facet_watch, jet_rt_handle_drop_JetDataPlotFacet);
    int64_t auto_domain = domain(0, 0, 0);
    drop_pair(auto_domain, jet_rt_handle_clone_JetDataPlotDomain(auto_domain), domain_watch, jet_rt_handle_drop_JetDataPlotDomain);
    int64_t numeric = domain(1, number_bits(-0.0), number_bits(1.0));
    int64_t numeric_copy = jet_rt_handle_clone_JetDataPlotDomain(numeric);
    words(numeric_copy)[1] = number_bits(0.0); assert(jet_rt_handle_eq_JetDataPlotDomain(numeric, numeric_copy));
    words(numeric_copy)[1] = number_bits(NAN); assert(!jet_rt_handle_eq_JetDataPlotDomain(numeric_copy, numeric_copy));
    drop_pair(numeric, numeric_copy, domain_watch, jet_rt_handle_drop_JetDataPlotDomain);
    int64_t categories[] = {pointer(jet_rt_string_clone(a)), pointer(jet_rt_string_clone(b))};
    int64_t scale = record(5); words(scale)[2] = domain(2, pointer(sequence(categories, 2)), 0); words(scale)[3] = 1;
    words(scale)[3] = INT64_C(0x123456789abcde01);
    words(scale)[4] = INT64_C(0x123456789abcde00);
    int64_t scale_copy = jet_rt_handle_clone_JetDataPlotScale(scale);
    assert(jet_rt_handle_eq_JetDataPlotScale(scale, scale_copy));
    assert(words(scale_copy)[3] == 1 && words(scale_copy)[4] == 0);
    assert(jet_rt_handle_field_JetDataPlotScale_domain(scale) == words(scale)[2]);
    assert(*jet_rt_handle_field_JetDataPlotScale_clamp(scale)); assert(!*jet_rt_handle_field_JetDataPlotScale_reverse(scale));
    Sequence *cats = (Sequence *)(uintptr_t)words(words(scale_copy)[2])[1];
    tmp = cats->data[0]; cats->data[0] = cats->data[1]; cats->data[1] = tmp;
    assert(!jet_rt_handle_eq_JetDataPlotScale(scale, scale_copy));
    drop_pair(scale, scale_copy, scale_watch, jet_rt_handle_drop_JetDataPlotScale);
    int64_t transforms[4];
    for (int64_t i = 0; i < 4; ++i) { transforms[i] = record(4); words(transforms[i])[0] = i; }
    words(transforms[0])[1] = field(a); words(transforms[0])[2] = 2; words(transforms[0])[3] = jet_rt_record_new_JetDataPlotValue_Number(2);
    words(transforms[1])[1] = field(a); words(transforms[1])[2] = INT64_C(0x123456789abcde01);
    words(transforms[2])[1] = field(a); words(transforms[2])[2] = number_bits(0.5);
    int64_t group_fields[] = {field(a), field(b)};
    words(transforms[3])[1] = pointer(sequence(group_fields, 2)); words(transforms[3])[2] = field(a); words(transforms[3])[3] = 2;
    int64_t encodings[] = {jet_rt_record_new_JetDataPlotEncoding(0, field(a), 0)};
    int64_t layer = record(5); words(layer)[0] = pointer(jet_rt_string_clone(a)); words(layer)[2] = pointer(sequence(encodings, 1)); words(layer)[3] = pointer(sequence(transforms, 4)); words(layer)[4] = number_bits(1);
    int64_t layer_copy = jet_rt_handle_clone_JetDataPlotLayer(layer);
    assert(jet_rt_handle_eq_JetDataPlotLayer(layer, layer_copy));
    Text header; jet_rt_handle_field_JetDataPlotLayer_name(layer, &header); assert(header.length == 1 && header.data[0] == 'a');
    assert(*jet_rt_handle_field_JetDataPlotLayer_opacity(layer) == 1);
    Sequence *copy_transforms = (Sequence *)(uintptr_t)words(layer_copy)[3];
    assert(words(copy_transforms->data[1])[2] == 1); /* cloned bool ignores dirty upper bytes */
    words(copy_transforms->data[2])[2] = number_bits(NAN);
    assert(!jet_rt_handle_eq_JetDataPlotLayer(layer_copy, layer_copy));
    drop_pair(layer, layer_copy, layer_watch, jet_rt_handle_drop_JetDataPlotLayer);
    Text *empty = jet_rt_string_from_static("", 0);
    int64_t minimum = jet_int_owned_from_i64(INT64_MIN);
    int64_t error = jet_rt_record_new_JetDataPlotError(4, a, &empty, &a, &b, &a, &b, &minimum, b);
    jet_rt_int_drop(minimum); /* ctor copied raw i64, not the Int owner */
    int64_t error_copy = jet_rt_handle_clone_JetDataPlotError(error);
    assert(jet_rt_handle_eq_JetDataPlotError(error, error_copy));
    assert(words(error)[7] == 1 && words(error)[8] == INT64_MIN);
    int64_t owned_index = 0;
    assert(jet_rt_record_get_JetDataPlotError_index(error, &owned_index) == 1);
    assert(jet_int_owned_to_i64(owned_index) == INT64_MIN); jet_rt_int_drop(owned_index);
    int64_t (*getters[])(int64_t, Text **) = {
        jet_rt_record_get_JetDataPlotError_field, jet_rt_record_get_JetDataPlotError_channel,
        jet_rt_record_get_JetDataPlotError_mark, jet_rt_record_get_JetDataPlotError_expected,
        jet_rt_record_get_JetDataPlotError_actual
    };
    for (size_t i = 0; i < 5; ++i) {
        Text *owned_text = NULL;
        assert(getters[i](error, &owned_text) == 1 && owned_text != NULL);
        assert(pointer(owned_text) != words(error)[i + 2]); jet_rt_string_drop(owned_text);
        int64_t saved = words(error_copy)[i + 2]; words(error_copy)[i + 2] = 0;
        assert(!jet_rt_handle_eq_JetDataPlotError(error, error_copy));
        Text *untouched = empty;
        assert(getters[i](error_copy, &untouched) == 0 && untouched == empty);
        words(error_copy)[i + 2] = saved;
    }
    words(error_copy)[7] = 0;
    assert(!jet_rt_handle_eq_JetDataPlotError(error, error_copy)); words(error_copy)[7] = 1;
    words(error_copy)[8] = 0;
    assert(!jet_rt_handle_eq_JetDataPlotError(error, error_copy)); words(error_copy)[8] = INT64_MIN;
    drop_pair(error, error_copy, error_watch, jet_rt_handle_drop_JetDataPlotError);
    int64_t absent = jet_rt_record_new_JetDataPlotError(0, a, NULL, NULL, NULL, NULL, NULL, NULL, b);
    int64_t absent_copy = jet_rt_handle_clone_JetDataPlotError(absent);
    owned_index = 123;
    assert(jet_rt_record_get_JetDataPlotError_index(absent, &owned_index) == 0 && owned_index == 123);
    assert(jet_rt_handle_eq_JetDataPlotError(absent, absent_copy));
    drop_pair(absent, absent_copy, error_watch, jet_rt_handle_drop_JetDataPlotError);
    jet_rt_string_drop(empty);
    jet_rt_string_drop(a); jet_rt_string_drop(b); assert(calls > 100);
    puts("DataPlot graph witness: 7 families, ordered lists, Transform/IEEE domains, optional Error ownership, exact-once frees");
    return 0;
}
