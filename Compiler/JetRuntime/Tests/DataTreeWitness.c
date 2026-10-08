// Link the real Jet runtime object with -Wl,--wrap=jet_rt_free.
// This client is not a runtime provider. Define DATA_TREE_U8_STRIDE to match
// Collections.jet's single RT_HOST_U8_STRIDE epoch (base aa926ddfb = 8).
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#ifndef DATA_TREE_U8_STRIDE
#define DATA_TREE_U8_STRIDE 8
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
extern int64_t jet_int_owned_from_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Tree *jet_rt_handle_clone_DataTree(Tree *);
extern void jet_rt_handle_drop_DataTree(Tree *);
extern bool jet_rt_handle_eq_DataTree(Tree *, Tree *);
extern Text *jet_rt_handle_show_DataTree(Tree *);
extern Text *jet_rt_handle_display_DataTree(Tree *);
extern Text *jet_rt_handle_debug_DataTree(Tree *);
extern int64_t jet_rt_record_tag_DataTree(Tree *);
extern Tree *jet_rt_record_new_DataTree_Null(void);
extern Tree *jet_rt_record_new_DataTree_Bool(uint8_t);
extern Tree *jet_rt_record_new_DataTree_Int(int64_t);
extern Tree *jet_rt_record_new_DataTree_Float(double);
extern Tree *jet_rt_record_new_DataTree_Number(Text *);
extern Tree *jet_rt_record_new_DataTree_TypedText(Text *);
extern Tree *jet_rt_record_new_DataTree_Text(Text *);
extern Tree *jet_rt_record_new_DataTree_Bytes(Sequence *);
extern Tree *jet_rt_record_new_DataTree_Array(Sequence *);
extern Tree *jet_rt_record_new_DataTree_Object(Sequence *);
extern bool jet_rt_record_get_DataTree_Bool(Tree *);
extern int64_t jet_rt_record_get_DataTree_Int(Tree *);
extern double jet_rt_record_get_DataTree_Float(Tree *);
extern Text *jet_rt_record_get_DataTree_Number(Tree *);
extern Text *jet_rt_record_get_DataTree_TypedText(Tree *);
extern Text *jet_rt_record_get_DataTree_Text(Tree *);
extern Sequence *jet_rt_record_get_DataTree_Bytes(Tree *);
extern Sequence *jet_rt_record_get_DataTree_Array(Tree *);
extern Sequence *jet_rt_record_get_DataTree_Object(Tree *);
extern Sequence *jet_rt_record_seq_List_Rec_DataTree_new(int64_t);
extern int64_t jet_rt_record_seq_List_Rec_DataTree_len(Sequence *);
extern Tree *jet_rt_record_seq_List_Rec_DataTree_get(Sequence *, int64_t);
extern void jet_rt_record_seq_List_Rec_DataTree_push(Sequence *, Tree *);
extern Sequence *jet_rt_record_seq_Entries_Rec_DataTree_new(int64_t);
extern int64_t jet_rt_record_seq_Entries_Rec_DataTree_len(Sequence *);
extern Tree *jet_rt_record_seq_Entries_Rec_DataTree_get(Sequence *, int64_t, Text **);
extern void jet_rt_record_seq_Entries_Rec_DataTree_push(Sequence *, Text *, Tree *);
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
    watch(text, 24, 8);
    if (text->capacity) watch(text->data, text->capacity, 1);
}
static void watch_tree(Tree *tree) {
    watch(tree, 16, 8);
    if (tree->tag >= 4 && tree->tag <= 6) watch_text((Text *)tree->payload);
    if (tree->tag >= 7) {
        Sequence *sequence = (Sequence *)tree->payload;
        int64_t stride = tree->tag == 9 ? 16 : tree->tag == 7 ? DATA_TREE_U8_STRIDE : 8;
        watch(sequence, 24, 8);
        if (sequence->capacity) watch(sequence->data, sequence->capacity * stride, 8);
        for (int64_t i = 0; tree->tag != 7 && i < sequence->length; ++i) {
            if (tree->tag == 9) { Entry *e = sequence->data; watch_text(e[i].key); watch_tree(e[i].value); }
            else { Tree **children = sequence->data; watch_tree(children[i]); }
        }
    }
}
static void drop_pair(Tree *a, Tree *b) {
    watched_count = 0;
    watch_tree(a); watch_tree(b);
    unsigned before = intercepted;
    jet_rt_handle_drop_DataTree(a); jet_rt_handle_drop_DataTree(b);
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
    Tree *a = jet_rt_record_new_DataTree_Null(), *b = jet_rt_handle_clone_DataTree(a);
    assert(jet_rt_record_tag_DataTree(a) == 0 && jet_rt_handle_eq_DataTree(a,b));
    expect(jet_rt_handle_show_DataTree(a), "null"); drop_pair(a,b);
    a = jet_rt_record_new_DataTree_Bool(255); b = jet_rt_record_new_DataTree_Bool(0);
    assert(jet_rt_record_get_DataTree_Bool(a) && !jet_rt_record_get_DataTree_Bool(b));
    assert(!jet_rt_handle_eq_DataTree(a,b)); expect(jet_rt_handle_show_DataTree(a), "true"); drop_pair(a,b);
    a = jet_rt_record_new_DataTree_Int(42); b = jet_rt_handle_clone_DataTree(a);
    int64_t integer = jet_rt_record_get_DataTree_Int(a); assert(integer == 42); jet_rt_int_drop(integer);
    assert(jet_rt_handle_eq_DataTree(a,b)); expect(jet_rt_handle_show_DataTree(a), "42"); drop_pair(a,b);
    // Canonical boxed Int is retained independently (private fixture's final
    // owner goes through the real native retirement provider, not a C stub).
    integer = jet_int_owned_from_i64(INT64_MAX);
    a = jet_rt_record_new_DataTree_Int(integer); b = jet_rt_handle_clone_DataTree(a);
    jet_rt_int_drop(integer); jet_rt_handle_drop_DataTree(a);
    expect(jet_rt_handle_show_DataTree(b), "9223372036854775807"); jet_rt_handle_drop_DataTree(b);
    a = jet_rt_record_new_DataTree_Float(-0.0); b = jet_rt_record_new_DataTree_Float(0.0);
    assert(jet_rt_handle_eq_DataTree(a,b)); expect(jet_rt_handle_show_DataTree(a), "-0.0"); drop_pair(a,b);
    union { uint64_t bits; double value; } nan = { UINT64_C(0x7ff8000000000001) };
    a = jet_rt_record_new_DataTree_Float(nan.value); b = jet_rt_handle_clone_DataTree(a);
    assert(!jet_rt_handle_eq_DataTree(a,a) && !jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataTree_Float(5.0); b = jet_rt_record_new_DataTree_Int(5);
    assert(jet_rt_record_get_DataTree_Float(a) == 5.0 && !jet_rt_handle_eq_DataTree(a,b));
    expect(jet_rt_handle_show_DataTree(a), "5.0"); drop_pair(a,b);
}
static void text_cases(void) {
    Text *input = text("\"\\\b\t\n\f\r\001\037 snow \xc3\xa9");
    Tree *a = jet_rt_record_new_DataTree_Text(input), *b = jet_rt_handle_clone_DataTree(a);
    assert(a->payload != (uintptr_t)input && b->payload != a->payload);
    jet_rt_string_drop(input);
    expect(jet_rt_handle_show_DataTree(a), "\"\\\"\\\\\\b\\t\\n\\f\\r\\u0001\\u001f snow \xc3\xa9\"");
    Text *got = jet_rt_record_get_DataTree_Text(b); assert((uintptr_t)got != b->payload); jet_rt_string_drop(got);
    ((Text *)b->payload)->data[0] = 'x'; assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    input = text("1.2300e+09"); a = jet_rt_record_new_DataTree_Number(input); b = jet_rt_record_new_DataTree_TypedText(input);
    jet_rt_string_drop(input);
    expect(jet_rt_record_get_DataTree_Number(a), "1.2300e+09");
    expect(jet_rt_record_get_DataTree_TypedText(b), "1.2300e+09");
    expect(jet_rt_handle_display_DataTree(a), "1.2300e+09");
    expect(jet_rt_handle_debug_DataTree(b), "\"1.2300e+09\"");
    assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
}
static Tree *object(const char *first, const char *second) {
    Sequence *sequence = jet_rt_record_seq_Entries_Rec_DataTree_new(0);
    Text *k = text(first); jet_rt_record_seq_Entries_Rec_DataTree_push(sequence,k,jet_rt_record_new_DataTree_Int(1)); jet_rt_string_drop(k);
    k = text(second); jet_rt_record_seq_Entries_Rec_DataTree_push(sequence,k,jet_rt_record_new_DataTree_Int(2)); jet_rt_string_drop(k);
    return jet_rt_record_new_DataTree_Object(sequence);
}
static void sequence_cases(void) {
    Tree *a = object("z", "a"), *b = jet_rt_handle_clone_DataTree(a);
    assert(jet_rt_handle_eq_DataTree(a,b)); expect(jet_rt_handle_show_DataTree(a), "{\"z\":1,\"a\":2}");
    Sequence *seq = jet_rt_record_get_DataTree_Object(b); assert(jet_rt_record_seq_Entries_Rec_DataTree_len(seq) == 2);
    Text *key = NULL; Tree *child = jet_rt_record_seq_Entries_Rec_DataTree_get(seq,0,&key);
    assert(child == ((Entry *)seq->data)[0].value && key != ((Entry *)seq->data)[0].key);
    expect(key,"z"); ((Entry *)seq->data)[0].key->data[0] = 'q';
    assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    a = object("x", "x"); b = object("x", "y");
    expect(jet_rt_handle_show_DataTree(a), "{\"x\":1,\"x\":2}"); assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    a = object("z", "a"); b = object("a", "z"); assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    seq = jet_rt_record_seq_List_Rec_DataTree_new(0);
    for (int64_t i = 0; i < 7; ++i) jet_rt_record_seq_List_Rec_DataTree_push(seq,jet_rt_record_new_DataTree_Int(i));
    jet_rt_record_seq_List_Rec_DataTree_push(seq,object("z","a"));
    a = jet_rt_record_new_DataTree_Array(seq); b = jet_rt_handle_clone_DataTree(a);
    assert(jet_rt_record_get_DataTree_Array(a) == seq && jet_rt_record_seq_List_Rec_DataTree_len(seq) == 8);
    assert(jet_rt_record_seq_List_Rec_DataTree_get(seq,0) == ((Tree **)seq->data)[0]);
    expect(jet_rt_handle_show_DataTree(a), "[0,1,2,3,4,5,6,{\"z\":1,\"a\":2}]");
    assert(jet_rt_handle_eq_DataTree(a,b));
    ((Tree **)((Sequence *)b->payload)->data)[0]->payload = 77;
    assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataTree_Array(jet_rt_record_seq_List_Rec_DataTree_new(0)); b = jet_rt_handle_clone_DataTree(a);
    expect(jet_rt_handle_show_DataTree(a), "[]"); assert(jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataTree_Object(jet_rt_record_seq_Entries_Rec_DataTree_new(0)); b = jet_rt_handle_clone_DataTree(a);
    expect(jet_rt_handle_show_DataTree(a), "{}"); drop_pair(a,b);
    seq = jet_rt_alloc(24,8); seq->length = seq->capacity = 3; seq->data = jet_rt_alloc(3 * DATA_TREE_U8_STRIDE,8);
    memset(seq->data,0,3 * DATA_TREE_U8_STRIDE);
    unsigned char *bytes = seq->data; bytes[0] = 0; bytes[DATA_TREE_U8_STRIDE] = 127; bytes[2 * DATA_TREE_U8_STRIDE] = 255;
    a = jet_rt_record_new_DataTree_Bytes(seq); b = jet_rt_handle_clone_DataTree(a);
    assert(jet_rt_record_get_DataTree_Bytes(a) == seq && jet_rt_handle_eq_DataTree(a,b));
    expect(jet_rt_handle_show_DataTree(a), "[0,127,255]");
    ((unsigned char *)((Sequence *)b->payload)->data)[0] = 1; assert(!jet_rt_handle_eq_DataTree(a,b)); drop_pair(a,b);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc > 1 && strcmp(argv[1], "nonfinite") == 0) {
        union { uint64_t bits; double value; } infinity = { UINT64_C(0x7ff0000000000000) };
        Tree *tree = jet_rt_record_new_DataTree_Float(infinity.value);
        (void)jet_rt_handle_show_DataTree(tree);
        assert(!"non-finite JSON render must stop");
    }
    scalar_cases(); text_cases(); sequence_cases();
    assert(intercepted > 0);
    puts("DataTree ownership/equality/JSON witness OK");
    return 0;
}
