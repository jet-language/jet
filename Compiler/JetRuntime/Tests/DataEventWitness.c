// Link the real Jet runtime object.
// Define DATA_EVENT_U8_STRIDE for the runtime's single U8 epoch (packed = 1).
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#ifndef DATA_EVENT_U8_STRIDE
#define DATA_EVENT_U8_STRIDE 1
#endif
_Static_assert(DATA_EVENT_U8_STRIDE == 1 || DATA_EVENT_U8_STRIDE == 8, "native U8 epoch");
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; uintptr_t payload; } Event;
typedef struct { int64_t length, capacity; void *data; } Sequence;
_Static_assert(sizeof(Event) == 16 && offsetof(Event, payload) == 8, "DataEvent native ABI");
_Static_assert(sizeof(Sequence) == 24 && offsetof(Sequence, data) == 16, "native List");
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_panic(Text *, int64_t, Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern Event *jet_rt_handle_clone_data_event(Event *);
extern void jet_rt_handle_drop_data_event(Event *);
extern bool jet_rt_handle_eq_data_event(Event *, Event *);
extern int64_t jet_rt_record_tag_data_event(Event *);
extern Event *jet_rt_record_new_data_event_null(void);
extern Event *jet_rt_record_new_data_event_bool(uint8_t);
extern Event *jet_rt_record_new_data_event_int(int64_t);
extern Event *jet_rt_record_new_data_event_float(double);
extern Event *jet_rt_record_new_data_event_number(Text *);
extern Event *jet_rt_record_new_data_event_text(Text *);
extern Event *jet_rt_record_new_data_event_bytes(Sequence *);
extern Event *jet_rt_record_new_data_event_array_start(void);
extern Event *jet_rt_record_new_data_event_array_end(void);
extern Event *jet_rt_record_new_data_event_object_start(void);
extern Event *jet_rt_record_new_data_event_key(Text *);
extern Event *jet_rt_record_new_data_event_object_end(void);
extern bool jet_rt_record_get_data_event_bool(Event *);
extern int64_t jet_rt_record_get_data_event_int(Event *);
extern double jet_rt_record_get_data_event_float(Event *);
extern Text *jet_rt_record_get_data_event_number(Event *);
extern Text *jet_rt_record_get_data_event_text(Event *);
extern Sequence *jet_rt_record_get_data_event_bytes(Event *);
extern Text *jet_rt_record_get_data_event_key(Event *);
static struct { void *pointer; int64_t size, align; unsigned frees; } watched[32];
static size_t watched_count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *pointer, int64_t size, int64_t align) {
    ++intercepted;
    for (size_t i = 0; i < watched_count; ++i) if (watched[i].pointer == pointer) {
        assert(size == watched[i].size && align == watched[i].align);
        assert(++watched[i].frees == 1);
    }
}
// A block watched again before it is freed is another owner of the same
// refcounted text: it stays one entry that must be freed exactly once.
static void watch(void *pointer, int64_t size, int64_t align) {
    assert(watched_count < sizeof(watched) / sizeof(watched[0]));
    for (size_t i = 0; i < watched_count; ++i) if (watched[i].pointer == pointer) {
        assert(!watched[i].frees && size == watched[i].size && align == watched[i].align);
        return;
    }
    watched[watched_count].pointer = pointer;
    watched[watched_count].size = size;
    watched[watched_count].align = align;
    watched[watched_count++].frees = 0;
}
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(Text *text) { return *(int64_t *)((char *)text - 8) & INT64_C(0x3fffffffffffffff); }
static void watch_event(Event *event) {
    watch(event,16,8);
    if (event->tag == 4 || event->tag == 5 || event->tag == 10) {
        Text *text = (Text *)event->payload; watch((char *)(text)-8,104,8);
        if((const char *)text->data!=(const char *)text+32) watch(text->data,text->capacity,1);
    } else if (event->tag == 6) {
        Sequence *seq = (Sequence *)event->payload; watch(seq,24,8);
        if (seq->capacity) watch(seq->data,seq->capacity * DATA_EVENT_U8_STRIDE,8);
    }
}
static void drop_pair(Event *a, Event *b) {
    watched_count = 0; watch_event(a); watch_event(b);
    unsigned before = intercepted;
    jet_rt_handle_drop_data_event(a); jet_rt_handle_drop_data_event(b);
    assert(intercepted > before); // A missing observer cannot silently pass.
    for (size_t i = 0; i < watched_count; ++i) assert(watched[i].frees == 1);
    watched_count = 0;
}
static void scalar_cases(void) {
    Event *(*units[])(void) = {jet_rt_record_new_data_event_null,jet_rt_record_new_data_event_array_start,jet_rt_record_new_data_event_array_end,jet_rt_record_new_data_event_object_start,jet_rt_record_new_data_event_object_end};
    const int64_t tags[] = {0,7,8,9,11};
    for (size_t i = 0; i < sizeof(tags) / sizeof(tags[0]); ++i) {
        Event *a = units[i](), *b = jet_rt_handle_clone_data_event(a);
        assert(a != b && a->payload == 0 && b->payload == 0);
        assert(jet_rt_record_tag_data_event(a) == tags[i] && jet_rt_handle_eq_data_event(a,b));
        if (i) {
            Event *other = units[0](); assert(!jet_rt_handle_eq_data_event(a,other)); jet_rt_handle_drop_data_event(other);
        }
        drop_pair(a,b);
    }
    Event *a = jet_rt_record_new_data_event_bool(255), *b = jet_rt_record_new_data_event_bool(1);
    assert(a->payload == 1 && jet_rt_record_get_data_event_bool(a) && jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_data_event_bool(0); b = jet_rt_record_new_data_event_bool(255);
    assert(!jet_rt_record_get_data_event_bool(a) && !jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    const int64_t edges[] = {0,42,INT64_MIN,INT64_MAX};
    for (size_t i = 0; i < sizeof(edges) / sizeof(edges[0]); ++i) {
        int64_t input = jet_int_owned_from_i64(edges[i]);
        a = jet_rt_record_new_data_event_int(input); b = jet_rt_handle_clone_data_event(a);
        assert(jet_int_owned_to_i64(input) == edges[i] && (int64_t)a->payload == edges[i]);
        int64_t first = jet_rt_record_get_data_event_int(a), second = jet_rt_record_get_data_event_int(a);
        if (i >= 2) assert(first != input && second != input && first != second);
        jet_rt_int_drop(input);
        assert(jet_int_owned_to_i64(first) == edges[i] && jet_int_owned_to_i64(second) == edges[i]);
        jet_rt_int_drop(first); jet_rt_int_drop(second);
        assert(jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    }
    a = jet_rt_record_new_data_event_int(5); b = jet_rt_record_new_data_event_float(5.0);
    assert(!jet_rt_handle_eq_data_event(a,b) && jet_rt_record_get_data_event_float(b) == 5.0); drop_pair(a,b);
    a = jet_rt_record_new_data_event_float(-0.0); b = jet_rt_record_new_data_event_float(0.0);
    assert(jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    union { uint64_t bits; double value; } special = {UINT64_C(0x7ff8000000000001)};
    a = jet_rt_record_new_data_event_float(special.value); b = jet_rt_handle_clone_data_event(a);
    assert(!jet_rt_handle_eq_data_event(a,a) && !jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    special.bits = UINT64_C(0x7ff0000000000000);
    a = jet_rt_record_new_data_event_float(special.value); b = jet_rt_handle_clone_data_event(a);
    assert(jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b); // No JSON render API or finite restriction here.
    jet_rt_handle_drop_data_event(NULL);
}
static void text_cases(void) {
    Event *(*ctors[])(Text *) = {jet_rt_record_new_data_event_number,jet_rt_record_new_data_event_text,jet_rt_record_new_data_event_key};
    Text *(*getters[])(Event *) = {jet_rt_record_get_data_event_number,jet_rt_record_get_data_event_text,jet_rt_record_get_data_event_key};
    const int64_t tags[] = {4,5,10};
    const char bytes[] = "1.2300e+09 \"\\ snow \xc3\xa9";
    for (size_t i = 0; i < sizeof(tags) / sizeof(tags[0]); ++i) {
        Text *input = jet_rt_string_from_static(bytes,sizeof(bytes)-1);
        Event *a = ctors[i](input), *b = jet_rt_handle_clone_data_event(a);
        assert(jet_rt_record_tag_data_event(a) == tags[i]);
        // Events are reallocated; the payload text is shared by owner count.
        assert(a != b && a->payload == (uintptr_t)input && b->payload == a->payload && owners(input) == 3);
        jet_rt_string_drop(input);
        assert(owners(input) == 2);
        Text *got = getters[i](a);
        assert((uintptr_t)got == a->payload && owners(got) == 3 && got->length == (int64_t)sizeof(bytes)-1);
        assert(memcmp(got->data,bytes,sizeof(bytes)-1) == 0);
        jet_rt_string_drop(got); assert(owners(input) == 2);
        assert(jet_rt_handle_eq_data_event(a,b));
        char changed[sizeof(bytes)]; memcpy(changed,bytes,sizeof(bytes)); changed[0] = 'x';
        Text *other = jet_rt_string_from_static(changed,sizeof(changed)-1);
        Event *c = ctors[i](other), *d = jet_rt_handle_clone_data_event(c);
        jet_rt_string_drop(other);
        assert(!jet_rt_handle_eq_data_event(a,c) && jet_rt_handle_eq_data_event(c,d));
        drop_pair(a,b); drop_pair(c,d);
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
    Event *a = jet_rt_record_new_data_event_bytes(seq), *b = jet_rt_handle_clone_data_event(a);
    assert(jet_rt_record_get_data_event_bytes(a) == seq && a->payload == (uintptr_t)seq);
    Sequence *got = jet_rt_record_get_data_event_bytes(b);
    assert(got != seq && got->data != seq->data && jet_rt_handle_eq_data_event(a,b));
    ((unsigned char *)got->data)[0] = 1; assert(!jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_data_event_bytes(sequence(0)); b = jet_rt_handle_clone_data_event(a);
    assert(jet_rt_record_get_data_event_bytes(a)->length == 0 && jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
    a = jet_rt_record_new_data_event_bytes(sequence(0)); b = jet_rt_record_new_data_event_bytes(sequence(1));
    assert(!jet_rt_handle_eq_data_event(a,b)); drop_pair(a,b);
}
int main(int argc, char **argv, char **envp) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    // A stop's report reads the startup capture (HostStartup.jet).
    jet_rt_host_arguments(argc, argv, envp);
    if (argc > 1 && strcmp(argv[1],"panic-reference") == 0) {
        const char file[] = "<core.prelude>", message[] = "record variant index out of range";
        jet_panic(jet_rt_string_from_static(file,sizeof(file)-1),0,jet_rt_string_from_static(message,sizeof(message)-1));
        assert(!"jet_panic reference must stop");
    }
    if (argc > 1 && strcmp(argv[1],"native-overflow") == 0) {
        int64_t edge = jet_int_owned_from_i64(INT64_MAX);
        int64_t too_big = jet_int_add(edge,1); jet_rt_int_drop(edge);
        (void)jet_rt_record_new_data_event_int(too_big);
        assert(!"native Int argument exceeds host range must stop");
    }
    if (argc > 1 && strcmp(argv[1],"wrong-variant") == 0) {
        (void)jet_rt_record_get_data_event_int(jet_rt_record_new_data_event_null());
        assert(!"record variant index out of range must stop");
    }
    scalar_cases(); text_cases(); bytes_cases();
    puts("DataEvent ownership/equality/boundary witness OK");
    return 0;
}
