// Link the real Jet runtime object with -Wl,--wrap=jet_rt_free.
// This client is not a runtime provider. Define DATA_TREE_U8_STRIDE to match
// Collections.jet's single RT_HOST_U8_STRIDE epoch (canonical packed = 1).
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#ifndef DATA_TREE_U8_STRIDE
#define DATA_TREE_U8_STRIDE 1
#endif
_Static_assert(DATA_TREE_U8_STRIDE == 1 || DATA_TREE_U8_STRIDE == 8, "native U8 epoch");
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; uintptr_t payload; } Tree;
typedef struct { int64_t length, capacity; void *data; } Sequence;
typedef struct { Text *key; Tree *value; } Entry;
_Static_assert(sizeof(Tree) == 16 && offsetof(Tree, payload) == 8, "DataABI Tree");
_Static_assert(sizeof(Sequence) == 24 && offsetof(Sequence, data) == 16, "native List");
_Static_assert(sizeof(Entry) == 16 && offsetof(Entry, value) == 8, "DataABI Entry");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_panic(Text *, int64_t, Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern Tree *jet_rt_handle_clone_dyn(Tree *);
extern void jet_rt_handle_drop_dyn(Tree *);
extern bool jet_rt_handle_eq_dyn(Tree *, Tree *);
extern Text *jet_rt_handle_show_dyn(Tree *);
extern Text *jet_rt_handle_display_dyn(Tree *);
extern Text *jet_rt_handle_debug_dyn(Tree *);
extern int64_t jet_rt_record_tag_dyn(Tree *);
extern Tree *jet_rt_record_new_dyn_null(void);
extern Tree *jet_rt_record_new_dyn_bool(uint8_t);
extern Tree *jet_rt_record_new_dyn_int(int64_t);
extern Tree *jet_rt_record_new_dyn_float(double);
extern Tree *jet_rt_record_new_dyn_number(Text *);
extern Tree *jet_rt_record_new_dyn_text(Text *);
extern Tree *jet_rt_record_new_dyn_bytes(Sequence *);
extern Tree *jet_rt_record_new_dyn_array(Sequence *);
extern Tree *jet_rt_record_new_dyn_object(Sequence *);
extern bool jet_rt_record_get_dyn_bool(Tree *);
extern int64_t jet_rt_record_get_dyn_int(Tree *);
extern double jet_rt_record_get_dyn_float(Tree *);
extern Text *jet_rt_record_get_dyn_number(Tree *);
extern Text *jet_rt_record_get_dyn_text(Tree *);
extern Sequence *jet_rt_record_get_dyn_bytes(Tree *);
extern Sequence *jet_rt_record_get_dyn_array(Tree *);
extern Sequence *jet_rt_record_get_dyn_object(Tree *);
extern Sequence *jet_rt_record_seq_list_rec_dyn_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_dyn_len(Sequence *);
extern Tree *jet_rt_record_seq_list_rec_dyn_get(Sequence *, int64_t);
extern void jet_rt_record_seq_list_rec_dyn_push(Sequence *, Tree *);
extern Sequence *jet_rt_record_seq_entries_rec_dyn_new(int64_t);
extern int64_t jet_rt_record_seq_entries_rec_dyn_len(Sequence *);
extern Tree *jet_rt_record_seq_entries_rec_dyn_get(Sequence *, int64_t, Text **);
extern void jet_rt_record_seq_entries_rec_dyn_push(Sequence *, Text *, Tree *);
static struct { void *pointer; int64_t size, align; unsigned frees; } watched[256];
static size_t watched_count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < watched_count; ++i) if (watched[i].pointer == pointer) {
        assert(size == watched[i].size && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
    __real_jet_rt_free(pointer, size, align);
}
static void watch(void *pointer, int64_t size, int64_t align) {
    assert(watched_count < sizeof(watched) / sizeof(watched[0]));
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].pointer != pointer);
    watched[watched_count].pointer = pointer;
    watched[watched_count].size = size;
    watched[watched_count].align = align;
    watched[watched_count++].frees = 0;
}
static void watch_text(Text *text) {
    watch((char *)(text)-8,32,8);
    if (text->capacity) watch(text->data, text->capacity, 1);
}
static void watch_tree(Tree *tree) {
    watch(tree, 16, 8);
    if (tree->tag == 4 || tree->tag == 5) watch_text((Text *)tree->payload);
    if (tree->tag == 7) watch((void *)tree->payload, 32, 8);
    if (tree->tag == 6 || tree->tag >= 8) {
        Sequence *sequence = (Sequence *)tree->payload;
        int64_t stride = tree->tag == 9 ? 16 : tree->tag == 6 ? DATA_TREE_U8_STRIDE : 8;
        watch(sequence, 24, 8);
        if (sequence->capacity) watch(sequence->data, sequence->capacity * stride, 8);
        for (int64_t i = 0; tree->tag != 6 && i < sequence->length; ++i) {
            if (tree->tag == 9) { Entry *e = sequence->data; watch_text(e[i].key); watch_tree(e[i].value); }
            else { Tree **children = sequence->data; watch_tree(children[i]); }
        }
    }
}
static void drop_pair(Tree *a, Tree *b) {
    watched_count = 0;
    watch_tree(a); watch_tree(b);
    unsigned before = intercepted;
    jet_rt_handle_drop_dyn(a); jet_rt_handle_drop_dyn(b);
    assert(intercepted > before); // A link missing --wrap cannot silently pass.
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].frees == 1);
    watched_count = 0;
}
static Text *text(const char *bytes) { return jet_rt_string_from_static(bytes, (int64_t)strlen(bytes)); }
static void expect(Text *actual, const char *expected) {
    assert(actual->length == (int64_t)strlen(expected));
    assert(memcmp(actual->data, expected, (size_t)actual->length) == 0);
    jet_rt_string_drop(actual);
}
static void scalar_cases(void) {
    Tree *a = jet_rt_record_new_dyn_null(), *b = jet_rt_handle_clone_dyn(a);
    assert(jet_rt_record_tag_dyn(a) == 0 && jet_rt_handle_eq_dyn(a,b));
    expect(jet_rt_handle_show_dyn(a), "null"); drop_pair(a,b);
    a = jet_rt_record_new_dyn_bool(255); b = jet_rt_record_new_dyn_bool(0);
    assert(jet_rt_record_get_dyn_bool(a) && !jet_rt_record_get_dyn_bool(b));
    assert(!jet_rt_handle_eq_dyn(a,b)); expect(jet_rt_handle_show_dyn(a), "true"); drop_pair(a,b);
    a = jet_rt_record_new_dyn_int(42); b = jet_rt_handle_clone_dyn(a);
    int64_t integer = jet_rt_record_get_dyn_int(a); assert(integer == 42); jet_rt_int_drop(integer);
    assert(jet_rt_handle_eq_dyn(a,b)); expect(jet_rt_handle_show_dyn(a), "42"); drop_pair(a,b);
    // Canonical Dyn retains a borrowed exact Int owner; clones and getters
    // acquire references to that immutable value, including both i64 edges.
    const int64_t edges[] = {INT64_MAX, INT64_MIN};
    const char *decimal[] = {"9223372036854775807", "-9223372036854775808"};
    for (size_t i = 0; i < 2; ++i) {
        integer = jet_int_owned_from_i64(edges[i]);
        a = jet_rt_record_new_dyn_int(integer); b = jet_rt_handle_clone_dyn(a);
        assert((int64_t)a->payload == integer && a->payload == b->payload);
        int64_t first = jet_rt_record_get_dyn_int(a);
        int64_t second = jet_rt_record_get_dyn_int(a);
        assert(first == integer && second == integer);
        assert(jet_int_owned_to_i64(first) == edges[i] && jet_int_owned_to_i64(second) == edges[i]);
        jet_rt_int_drop(integer); jet_rt_int_drop(first); jet_rt_int_drop(second);
        assert(jet_rt_handle_eq_dyn(a,b));
        expect(jet_rt_handle_show_dyn(b), decimal[i]); drop_pair(a,b);
    }
    a = jet_rt_record_new_dyn_float(-0.0); b = jet_rt_record_new_dyn_float(0.0);
    assert(jet_rt_handle_eq_dyn(a,b)); expect(jet_rt_handle_show_dyn(a), "-0.0"); drop_pair(a,b);
    union { uint64_t bits; double value; } nan = { UINT64_C(0x7ff8000000000001) };
    a = jet_rt_record_new_dyn_float(nan.value); b = jet_rt_handle_clone_dyn(a);
    assert(!jet_rt_handle_eq_dyn(a,a) && !jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_dyn_float(5.0); b = jet_rt_record_new_dyn_int(5);
    assert(jet_rt_record_get_dyn_float(a) == 5.0 && !jet_rt_handle_eq_dyn(a,b));
    expect(jet_rt_handle_show_dyn(a), "5.0"); drop_pair(a,b);
}
static void text_cases(void) {
    Text *input = text("\"\\\b\t\n\f\r\001\037 snow \xc3\xa9");
    Tree *a = jet_rt_record_new_dyn_text(input), *b = jet_rt_handle_clone_dyn(a);
    assert(a->payload != (uintptr_t)input && b->payload != a->payload);
    jet_rt_string_drop(input);
    expect(jet_rt_handle_show_dyn(a), "\"\\\b\t\n\f\r\001\037 snow \xc3\xa9");
    Text *got = jet_rt_record_get_dyn_text(b); assert((uintptr_t)got != b->payload); jet_rt_string_drop(got);
    ((Text *)b->payload)->data[0] = 'x'; assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    input = text("1.2300e+09"); a = jet_rt_record_new_dyn_number(input); b = jet_rt_record_new_dyn_text(input);
    jet_rt_string_drop(input);
    expect(jet_rt_record_get_dyn_number(a), "1.2300e+09");
    expect(jet_rt_record_get_dyn_text(b), "1.2300e+09");
    expect(jet_rt_handle_display_dyn(a), "1.2300e+09");
    expect(jet_rt_handle_debug_dyn(b), "\"1.2300e+09\"");
    assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
}
static Tree *object(const char *first, const char *second) {
    Sequence *sequence = jet_rt_record_seq_entries_rec_dyn_new(0);
    Text *k = text(first); jet_rt_record_seq_entries_rec_dyn_push(sequence,k,jet_rt_record_new_dyn_int(1)); jet_rt_string_drop(k);
    k = text(second); jet_rt_record_seq_entries_rec_dyn_push(sequence,k,jet_rt_record_new_dyn_int(2)); jet_rt_string_drop(k);
    return jet_rt_record_new_dyn_object(sequence);
}
static void sequence_cases(void) {
    Tree *a = object("z", "a"), *b = jet_rt_handle_clone_dyn(a);
    assert(jet_rt_handle_eq_dyn(a,b)); expect(jet_rt_handle_show_dyn(a), "{\"z\":1,\"a\":2}");
    Sequence *seq = jet_rt_record_get_dyn_object(b); assert(jet_rt_record_seq_entries_rec_dyn_len(seq) == 2);
    Text *key = NULL; Tree *child = jet_rt_record_seq_entries_rec_dyn_get(seq,0,&key);
    assert(child == ((Entry *)seq->data)[0].value && key != ((Entry *)seq->data)[0].key);
    expect(key,"z"); ((Entry *)seq->data)[0].key->data[0] = 'q';
    assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    a = object("x", "x"); b = object("x", "y");
    expect(jet_rt_handle_show_dyn(a), "{\"x\":1,\"x\":2}"); assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    a = object("z", "a"); b = object("a", "z"); assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    seq = jet_rt_record_seq_list_rec_dyn_new(0);
    for (int64_t i = 0; i < 7; ++i) jet_rt_record_seq_list_rec_dyn_push(seq,jet_rt_record_new_dyn_int(i));
    jet_rt_record_seq_list_rec_dyn_push(seq,object("z","a"));
    a = jet_rt_record_new_dyn_array(seq); b = jet_rt_handle_clone_dyn(a);
    assert(jet_rt_record_get_dyn_array(a) == seq && jet_rt_record_seq_list_rec_dyn_len(seq) == 8);
    assert(jet_rt_record_seq_list_rec_dyn_get(seq,0) == ((Tree **)seq->data)[0]);
    expect(jet_rt_handle_show_dyn(a), "[0,1,2,3,4,5,6,{\"z\":1,\"a\":2}]");
    assert(jet_rt_handle_eq_dyn(a,b));
    ((Tree **)((Sequence *)b->payload)->data)[0]->payload = 77;
    assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_dyn_array(jet_rt_record_seq_list_rec_dyn_new(0)); b = jet_rt_handle_clone_dyn(a);
    expect(jet_rt_handle_show_dyn(a), "[]"); assert(jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_dyn_object(jet_rt_record_seq_entries_rec_dyn_new(0)); b = jet_rt_handle_clone_dyn(a);
    expect(jet_rt_handle_show_dyn(a), "{}"); drop_pair(a,b);
    seq = jet_rt_alloc(24,8); seq->length = seq->capacity = 3; seq->data = jet_rt_alloc(3 * DATA_TREE_U8_STRIDE,8);
    memset(seq->data,0,3 * DATA_TREE_U8_STRIDE);
    unsigned char *bytes = seq->data; bytes[0] = 0; bytes[DATA_TREE_U8_STRIDE] = 127; bytes[2 * DATA_TREE_U8_STRIDE] = 255;
    a = jet_rt_record_new_dyn_bytes(seq); b = jet_rt_handle_clone_dyn(a);
    assert(jet_rt_record_get_dyn_bytes(a) == seq && jet_rt_handle_eq_dyn(a,b));
    expect(jet_rt_handle_show_dyn(a), "AH//");
    ((unsigned char *)((Sequence *)b->payload)->data)[0] = 1; assert(!jet_rt_handle_eq_dyn(a,b)); drop_pair(a,b);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc > 1 && (strcmp(argv[1], "list-capacity-negative") == 0 || strcmp(argv[1], "list-capacity-overflow") == 0)) {
        int64_t capacity = strcmp(argv[1], "list-capacity-negative") == 0 ? -1 : INT64_MAX / 8 + 1;
        (void)jet_rt_record_seq_list_rec_dyn_new(capacity);
        assert(!"std Vec capacity overflow must stop");
    }
    if (argc > 1 && (strcmp(argv[1], "entries-capacity-negative") == 0 || strcmp(argv[1], "entries-capacity-overflow") == 0)) {
        int64_t capacity = strcmp(argv[1], "entries-capacity-negative") == 0 ? -1 : INT64_MAX / 16 + 1;
        (void)jet_rt_record_seq_entries_rec_dyn_new(capacity);
        assert(!"std Vec capacity overflow must stop");
    }
    if (argc > 1 && (strcmp(argv[1], "list-oob") == 0 || strcmp(argv[1], "list-negative") == 0)) {
        int64_t index = strcmp(argv[1], "list-negative") == 0 ? -1 : 0;
        (void)jet_rt_record_seq_list_rec_dyn_get(jet_rt_record_seq_list_rec_dyn_new(0),index);
        assert(!"std Vec bounds must stop");
    }
    if (argc > 1 && (strcmp(argv[1], "entries-oob") == 0 || strcmp(argv[1], "entries-negative") == 0)) {
        int64_t index = strcmp(argv[1], "entries-negative") == 0 ? -1 : 0;
        Text *key = NULL;
        (void)jet_rt_record_seq_entries_rec_dyn_get(jet_rt_record_seq_entries_rec_dyn_new(0),index,&key);
        assert(!"std Vec bounds must stop");
    }
    if (argc > 1 && strcmp(argv[1], "panic-reference") == 0) {
        const char file[] = "<core.prelude>", message[] = "record variant index out of range";
        jet_panic(jet_rt_string_from_static(file,sizeof(file)-1),0,jet_rt_string_from_static(message,sizeof(message)-1));
        assert(!"jet_panic reference must stop");
    }
    if (argc > 1 && strcmp(argv[1], "wrong-variant") == 0) {
        (void)jet_rt_record_get_dyn_int(jet_rt_record_new_dyn_null());
        assert(!"record variant index out of range must stop");
    }
    if (argc > 1 && strcmp(argv[1], "nonfinite") == 0) {
        union { uint64_t bits; double value; } infinity = { UINT64_C(0x7ff0000000000000) };
        Tree *tree = jet_rt_record_new_dyn_float(infinity.value);
        (void)jet_rt_handle_show_dyn(tree);
        assert(!"non-finite JSON render must stop");
    }
    if (argc > 1 && strcmp(argv[1], "exact-int-large") == 0) {
        int64_t edge = jet_int_owned_from_i64(INT64_MAX);
        int64_t too_big = jet_int_add(edge, 1);
        jet_rt_int_drop(edge);
        Tree *tree = jet_rt_record_new_dyn_int(too_big);
        assert((int64_t)tree->payload == too_big);
        jet_rt_int_drop(too_big);
        expect(jet_rt_handle_show_dyn(tree), "9223372036854775808");
        jet_rt_handle_drop_dyn(tree);
        return 0;
    }
    scalar_cases(); text_cases(); sequence_cases();
    assert(intercepted > 0);
    puts("Dyn ownership/equality/JSON witness OK");
    return 0;
}
