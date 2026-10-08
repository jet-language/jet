// Link the real Jet runtime object with -Wl,--wrap=jet_rt_free.
// Define DATA_EVENT_U8_STRIDE for the runtime's single U8 epoch (base = 8).
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#ifndef DATA_EVENT_U8_STRIDE
#define DATA_EVENT_U8_STRIDE 8
#endif
_Static_assert(DATA_EVENT_U8_STRIDE == 1 || DATA_EVENT_U8_STRIDE == 8, "native U8 epoch");
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; uintptr_t payload; } Event;
typedef struct { int64_t length, capacity; void *data; } Sequence;
_Static_assert(sizeof(Event) == 16 && offsetof(Event, payload) == 8, "DataEvent native ABI");
_Static_assert(sizeof(Sequence) == 24 && offsetof(Sequence, data) == 16, "native List");
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
extern Event *jet_rt_handle_clone_DataEvent(Event *);
extern void jet_rt_handle_drop_DataEvent(Event *);
extern bool jet_rt_handle_eq_DataEvent(Event *, Event *);
extern int64_t jet_rt_record_tag_DataEvent(Event *);
extern Event *jet_rt_record_new_DataEvent_Null(void);
extern Event *jet_rt_record_new_DataEvent_Bool(uint8_t);
extern Event *jet_rt_record_new_DataEvent_Int(int64_t);
extern Event *jet_rt_record_new_DataEvent_Float(double);
extern Event *jet_rt_record_new_DataEvent_Number(Text *);
extern Event *jet_rt_record_new_DataEvent_Text(Text *);
extern Event *jet_rt_record_new_DataEvent_Bytes(Sequence *);
extern Event *jet_rt_record_new_DataEvent_ArrayStart(void);
extern Event *jet_rt_record_new_DataEvent_ArrayEnd(void);
extern Event *jet_rt_record_new_DataEvent_ObjectStart(void);
extern Event *jet_rt_record_new_DataEvent_Key(Text *);
extern Event *jet_rt_record_new_DataEvent_ObjectEnd(void);
extern bool jet_rt_record_get_DataEvent_Bool(Event *);
extern int64_t jet_rt_record_get_DataEvent_Int(Event *);
extern double jet_rt_record_get_DataEvent_Float(Event *);
extern Text *jet_rt_record_get_DataEvent_Number(Event *);
extern Text *jet_rt_record_get_DataEvent_Text(Event *);
extern Sequence *jet_rt_record_get_DataEvent_Bytes(Event *);
extern Text *jet_rt_record_get_DataEvent_Key(Event *);
static struct { void *pointer; int64_t size, align; unsigned frees; } watched[32];
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
static void watch_event(Event *event) {
    watch(event,16,8);
    if (event->tag == 4 || event->tag == 5 || event->tag == 10) {
        Text *text = (Text *)event->payload; watch(text,24,8);
        if (text->capacity) watch(text->data,text->capacity,1);
    } else if (event->tag == 6) {
        Sequence *seq = (Sequence *)event->payload; watch(seq,24,8);
        if (seq->capacity) watch(seq->data,seq->capacity * DATA_EVENT_U8_STRIDE,8);
    }
}
static void drop_pair(Event *a, Event *b) {
    watched_count = 0; watch_event(a); watch_event(b);
    unsigned before = intercepted;
    jet_rt_handle_drop_DataEvent(a); jet_rt_handle_drop_DataEvent(b);
    assert(intercepted > before); // A missing --wrap cannot silently pass.
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].frees == 1);
    watched_count = 0;
}
static void scalar_cases(void) {
    Event *(*units[])(void) = {jet_rt_record_new_DataEvent_Null,jet_rt_record_new_DataEvent_ArrayStart,jet_rt_record_new_DataEvent_ArrayEnd,jet_rt_record_new_DataEvent_ObjectStart,jet_rt_record_new_DataEvent_ObjectEnd};
    const int64_t tags[] = {0,7,8,9,11};
    for (size_t i = 0; i < sizeof(tags) / sizeof(tags[0]); ++i) {
        Event *a = units[i](), *b = jet_rt_handle_clone_DataEvent(a);
        assert(a != b && a->payload == 0 && b->payload == 0);
        assert(jet_rt_record_tag_DataEvent(a) == tags[i] && jet_rt_handle_eq_DataEvent(a,b));
        if (i) {
            Event *other = units[0](); assert(!jet_rt_handle_eq_DataEvent(a,other)); jet_rt_handle_drop_DataEvent(other);
        }
        drop_pair(a,b);
    }
    Event *a = jet_rt_record_new_DataEvent_Bool(255), *b = jet_rt_record_new_DataEvent_Bool(1);
    assert(a->payload == 1 && jet_rt_record_get_DataEvent_Bool(a) && jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataEvent_Bool(0); b = jet_rt_record_new_DataEvent_Bool(255);
    assert(!jet_rt_record_get_DataEvent_Bool(a) && !jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    const int64_t edges[] = {0,42,INT64_MIN,INT64_MAX};
    for (size_t i = 0; i < sizeof(edges) / sizeof(edges[0]); ++i) {
        int64_t input = jet_int_owned_from_i64(edges[i]);
        a = jet_rt_record_new_DataEvent_Int(input); b = jet_rt_handle_clone_DataEvent(a);
        assert(jet_int_owned_to_i64(input) == edges[i] && (int64_t)a->payload == edges[i]);
        int64_t first = jet_rt_record_get_DataEvent_Int(a), second = jet_rt_record_get_DataEvent_Int(a);
        if (i >= 2) assert(first != input && second != input && first != second);
        jet_rt_int_drop(input);
        assert(jet_int_owned_to_i64(first) == edges[i] && jet_int_owned_to_i64(second) == edges[i]);
        jet_rt_int_drop(first); jet_rt_int_drop(second);
        assert(jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    }
    a = jet_rt_record_new_DataEvent_Int(5); b = jet_rt_record_new_DataEvent_Float(5.0);
    assert(!jet_rt_handle_eq_DataEvent(a,b) && jet_rt_record_get_DataEvent_Float(b) == 5.0); drop_pair(a,b);
    a = jet_rt_record_new_DataEvent_Float(-0.0); b = jet_rt_record_new_DataEvent_Float(0.0);
    assert(jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    union { uint64_t bits; double value; } special = {UINT64_C(0x7ff8000000000001)};
    a = jet_rt_record_new_DataEvent_Float(special.value); b = jet_rt_handle_clone_DataEvent(a);
    assert(!jet_rt_handle_eq_DataEvent(a,a) && !jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    special.bits = UINT64_C(0x7ff0000000000000);
    a = jet_rt_record_new_DataEvent_Float(special.value); b = jet_rt_handle_clone_DataEvent(a);
    assert(jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b); // No JSON render API or finite restriction here.
    jet_rt_handle_drop_DataEvent(NULL);
}
static void text_cases(void) {
    Event *(*ctors[])(Text *) = {jet_rt_record_new_DataEvent_Number,jet_rt_record_new_DataEvent_Text,jet_rt_record_new_DataEvent_Key};
    Text *(*getters[])(Event *) = {jet_rt_record_get_DataEvent_Number,jet_rt_record_get_DataEvent_Text,jet_rt_record_get_DataEvent_Key};
    const int64_t tags[] = {4,5,10};
    const char bytes[] = "1.2300e+09 \"\\ snow \xc3\xa9";
    for (size_t i = 0; i < sizeof(tags) / sizeof(tags[0]); ++i) {
        Text *input = jet_rt_string_from_static(bytes,sizeof(bytes)-1);
        Event *a = ctors[i](input), *b = jet_rt_handle_clone_DataEvent(a);
        assert(jet_rt_record_tag_DataEvent(a) == tags[i]);
        assert(a->payload != (uintptr_t)input && b->payload != a->payload);
        jet_rt_string_drop(input);
        Text *got = getters[i](a);
        assert((uintptr_t)got != a->payload && got->length == (int64_t)sizeof(bytes)-1);
        assert(memcmp(got->data,bytes,sizeof(bytes)-1) == 0);
        got->data[0] = 'x'; jet_rt_string_drop(got);
        assert(jet_rt_handle_eq_DataEvent(a,b));
        ((Text *)b->payload)->data[0] = 'x'; assert(!jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    }
}
static Sequence *sequence(int64_t length) {
    Sequence *seq = jet_rt_alloc(24,8); seq->length = seq->capacity = length;
    seq->data = length ? jet_rt_alloc(length * DATA_EVENT_U8_STRIDE,8) : (void *)(uintptr_t)8;
    if (length) memset(seq->data,0,(size_t)(length * DATA_EVENT_U8_STRIDE));
    return seq;
}
static void bytes_cases(void) {
    Sequence *seq = sequence(3);
    unsigned char *bytes = seq->data; bytes[0] = 0; bytes[DATA_EVENT_U8_STRIDE] = 127; bytes[2 * DATA_EVENT_U8_STRIDE] = 255;
    Event *a = jet_rt_record_new_DataEvent_Bytes(seq), *b = jet_rt_handle_clone_DataEvent(a);
    assert(jet_rt_record_get_DataEvent_Bytes(a) == seq && a->payload == (uintptr_t)seq);
    Sequence *got = jet_rt_record_get_DataEvent_Bytes(b);
    assert(got != seq && got->data != seq->data && jet_rt_handle_eq_DataEvent(a,b));
    ((unsigned char *)got->data)[0] = 1; assert(!jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataEvent_Bytes(sequence(0)); b = jet_rt_handle_clone_DataEvent(a);
    assert(jet_rt_record_get_DataEvent_Bytes(a)->length == 0 && jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_DataEvent_Bytes(sequence(0)); b = jet_rt_record_new_DataEvent_Bytes(sequence(1));
    assert(!jet_rt_handle_eq_DataEvent(a,b)); drop_pair(a,b);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc > 1 && strcmp(argv[1],"panic-reference") == 0) {
        const char file[] = "<core.prelude>", message[] = "record variant index out of range";
        jet_panic(jet_rt_string_from_static(file,sizeof(file)-1),0,jet_rt_string_from_static(message,sizeof(message)-1));
        assert(!"jet_panic reference must stop");
    }
    if (argc > 1 && strcmp(argv[1],"native-overflow") == 0) {
        int64_t edge = jet_int_owned_from_i64(INT64_MAX);
        int64_t too_big = jet_int_add(edge,1); jet_rt_int_drop(edge);
        (void)jet_rt_record_new_DataEvent_Int(too_big);
        assert(!"native Int argument exceeds host range must stop");
    }
    if (argc > 1 && strcmp(argv[1],"wrong-variant") == 0) {
        (void)jet_rt_record_get_DataEvent_Int(jet_rt_record_new_DataEvent_Null());
        assert(!"record variant index out of range must stop");
    }
    scalar_cases(); text_cases(); bytes_cases();
    puts("DataEvent ownership/equality/boundary witness OK");
    return 0;
}
