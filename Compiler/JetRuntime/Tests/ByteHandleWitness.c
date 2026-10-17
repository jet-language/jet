// Link the real composed Jet runtime with -Wl,--wrap=jet_rt_free.
// BYTE_HANDLE_U8_STRIDE must match RT_HOST_U8_STRIDE (canonical packed 1).
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#ifndef BYTE_HANDLE_U8_STRIDE
#define BYTE_HANDLE_U8_STRIDE 1
#endif
_Static_assert(BYTE_HANDLE_U8_STRIDE==1 || BYTE_HANDLE_U8_STRIDE==8,"native U8 stride");
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; unsigned char *data; } Sequence;
typedef struct { Sequence *bytes; uint64_t position; } Buffer;
typedef struct { Sequence *bytes; } Path;
typedef struct { Text *table,*expression; int64_t compiled; } Policy;
_Static_assert(sizeof(Buffer)==16 && offsetof(Buffer,position)==8,"ByteHandlesABI buffer");
_Static_assert(sizeof(Path)==8 && sizeof(Policy)==24 && offsetof(Policy,compiled)==16,"ByteHandlesABI path/policy");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern Text *jet_rt_string_clone(const Text *);
extern void jet_rt_string_drop(Text *);
extern Sequence *jet_rt_bytes_unpack(const unsigned char *,int64_t);
#define RENDER_DECL(T,N) \
extern Text *jet_rt_handle_show_##N(T *); \
extern Text *jet_rt_handle_display_##N(T *); \
extern Text *jet_rt_handle_debug_##N(T *);
extern Buffer *jet_rt_handle_clone_jet_byte_buffer(Buffer *);
extern void jet_rt_handle_drop_jet_byte_buffer(Buffer *);
RENDER_DECL(Buffer,jet_byte_buffer)
extern Path *jet_rt_handle_clone_jet_path(Path *);
extern void jet_rt_handle_drop_jet_path(Path *);
RENDER_DECL(Path,jet_path)
extern Policy *jet_rt_handle_clone_jet_row_policy(Policy *);
extern void jet_rt_handle_drop_jet_row_policy(Policy *);
extern void jet_rt_handle_field_jet_row_policy_table(Policy *,Text *);
extern void jet_rt_handle_field_jet_row_policy_expression(Policy *,Text *);
extern Sequence *jet_rt_native_bits_new(void);
extern Sequence *jet_rt_handle_clone_jet_bit_set(Sequence *);
extern void jet_rt_handle_drop_jet_bit_set(Sequence *);
extern bool jet_bitset_add(Sequence *,int64_t);
extern void jet_bitset_remove(Sequence *,int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
RENDER_DECL(Sequence,jet_bit_set)
#define ENUM_DECL(N) \
extern int64_t *jet_rt_handle_clone_##N(int64_t *); \
extern void jet_rt_handle_drop_##N(int64_t *); \
extern bool jet_rt_handle_eq_##N(int64_t *,int64_t *);
ENUM_DECL(jet_failure_policy)
ENUM_DECL(jet_dispatch_state)
ENUM_DECL(jet_event_config_error)
ENUM_DECL(jet_late_event_disposition)
static struct { void *pointer; int64_t size,align; unsigned frees; } watched[32];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) { assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1); }
    __real_jet_rt_free(p,size,align);
}
static void watch(void *p,int64_t size,int64_t align) {
    assert(count<32);for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p;watched[count].size=size;watched[count].align=align;watched[count++].frees=0;
}
static void freed(unsigned before) { assert(intercepted>before);for(size_t i=0;i<count;++i) assert(watched[i].frees==1);count=0; }
static void watch_text(Text *t) { watch(t,24,8);if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_sequence(Sequence *s,int64_t stride) { watch(s,24,8);if(s->capacity) watch(s->data,s->capacity*stride,8); }
static void expect(Text *t,const void *bytes,size_t n) { assert(t->length==(int64_t)n);assert(memcmp(t->data,bytes,n)==0);jet_rt_string_drop(t); }
static void expect_s(Text *t,const char *s) { expect(t,s,strlen(s)); }
static unsigned byte_at(Sequence *s,int64_t at) {
    if(BYTE_HANDLE_U8_STRIDE==1) return s->data[at];
    uint64_t word;memcpy(&word,s->data+at*BYTE_HANDLE_U8_STRIDE,sizeof word);return (unsigned)(word&255);
}
static void put_byte(Sequence *s,int64_t at,unsigned byte) {
    if(BYTE_HANDLE_U8_STRIDE==1) s->data[at]=(unsigned char)byte;
    else { uint64_t word=byte;memcpy(s->data+at*BYTE_HANDLE_U8_STRIDE,&word,sizeof word); }
}
static void buffer_cases(void) {
    unsigned char values[256];char golden[1200];size_t used=0;
    golden[used++]='[';
    for(unsigned i=0;i<256;++i) { values[i]=i;int n=snprintf(golden+used,sizeof golden-used,"%s%u",i?", ":"",i);assert(n>0 && (size_t)n<sizeof golden-used);used+=(size_t)n; }
    golden[used++]=']';
    for(int full=0;full<2;++full) {
        Buffer *a=jet_rt_alloc(16,8);a->bytes=jet_rt_bytes_unpack(values,full?256:0);a->position=UINT64_MAX;
        Buffer *b=jet_rt_handle_clone_jet_byte_buffer(a);
        assert(a!=b && a->bytes!=b->bytes && b->position==UINT64_MAX && b->bytes->length==a->bytes->length);
        if(full) {
            assert(a->bytes->data!=b->bytes->data);
            for(int64_t i=0;i<256;++i) assert(byte_at(b->bytes,i)==(unsigned)i);
            put_byte(a->bytes,255,0);assert(byte_at(b->bytes,255)==255);put_byte(a->bytes,255,255);
        }
        a->position=full?UINT64_C(1)<<63:0;
        const char *expected=full?golden:"[]";size_t n=full?used:2;
        expect(jet_rt_handle_show_jet_byte_buffer(a),expected,n);
        expect(jet_rt_handle_display_jet_byte_buffer(a),expected,n);
        expect(jet_rt_handle_debug_jet_byte_buffer(a),expected,n);
        expect(jet_rt_handle_debug_jet_byte_buffer(b),expected,n);
        watch(a,16,8);watch(b,16,8);watch_sequence(a->bytes,BYTE_HANDLE_U8_STRIDE);watch_sequence(b->bytes,BYTE_HANDLE_U8_STRIDE);
        unsigned before=intercepted;jet_rt_handle_drop_jet_byte_buffer(a);jet_rt_handle_drop_jet_byte_buffer(b);freed(before);
    }
    jet_rt_handle_drop_jet_byte_buffer(NULL);
}
static void path_case(const unsigned char *input,int64_t n,const void *golden,size_t length) {
    Path *a=jet_rt_alloc(8,8);a->bytes=jet_rt_bytes_unpack(input,n);
    Path *b=jet_rt_handle_clone_jet_path(a);assert(a!=b && a->bytes!=b->bytes && b->bytes->length==n);
    for(int64_t i=0;i<n;++i) assert(byte_at(a->bytes,i)==byte_at(b->bytes,i));
    if(n) { assert(a->bytes->data!=b->bytes->data);unsigned original=byte_at(a->bytes,0);put_byte(a->bytes,0,original^1);assert(byte_at(b->bytes,0)==original);put_byte(a->bytes,0,original); }
    expect(jet_rt_handle_show_jet_path(a),golden,length);
    expect(jet_rt_handle_display_jet_path(a),golden,length);
    expect(jet_rt_handle_debug_jet_path(a),golden,length);
    expect(jet_rt_handle_debug_jet_path(b),golden,length);
    watch(a,8,8);watch(b,8,8);watch_sequence(a->bytes,BYTE_HANDLE_U8_STRIDE);watch_sequence(b->bytes,BYTE_HANDLE_U8_STRIDE);
    unsigned before=intercepted;jet_rt_handle_drop_jet_path(a);jet_rt_handle_drop_jet_path(b);freed(before);
}
static void path_cases(void) {
    unsigned char empty[1]={0};path_case(empty,0,"",0);
    unsigned char valid[]={'/','"','\t',0,0xc3,0xa9};
    static const char valid_text[]={'/','"','\t',0,(char)0xc3,(char)0xa9};
    path_case(valid,6,valid_text,sizeof valid_text);
    unsigned char malformed[]={'a',0xff,0xe2,0x82,0x7f,0xed,0xa0,0x80,0xe2,0x82};
    static const char lossy[]="a\xef\xbf\xbd\xef\xbf\xbd\x7f\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd\xef\xbf\xbd";
    path_case(malformed,10,lossy,sizeof lossy-1);
    jet_rt_handle_drop_jet_path(NULL);
}
static void policy_cases(void) {
    for(int64_t kind=0;kind<2;++kind) {
        Policy *a=jet_rt_alloc(24,8);a->table=jet_rt_string_from_static("accounts",8);a->expression=jet_rt_string_from_static(kind?"owner == user":"true",kind?13:4);a->compiled=kind;
        Policy *b=jet_rt_handle_clone_jet_row_policy(a);
        assert(a!=b && a->table!=b->table && a->expression!=b->expression && a->table->data!=b->table->data && a->expression->data!=b->expression->data && b->compiled==kind);
        Text table={0},expression={0};unsigned before=intercepted;
        jet_rt_handle_field_jet_row_policy_table(a,&table);jet_rt_handle_field_jet_row_policy_expression(a,&expression);
        assert(intercepted==before && table.data==a->table->data && table.length==a->table->length && table.capacity==a->table->capacity);
        assert(expression.data==a->expression->data && expression.length==a->expression->length && expression.capacity==a->expression->capacity);
        Text *owned=jet_rt_string_clone(&expression);a->expression->data[0]='X';
        assert(b->expression->data[0]!=(unsigned char)'X');expect_s(owned,kind?"owner == user":"true");
        watch(a,24,8);watch(b,24,8);watch_text(a->table);watch_text(a->expression);watch_text(b->table);watch_text(b->expression);
        before=intercepted;jet_rt_handle_drop_jet_row_policy(a);jet_rt_handle_drop_jet_row_policy(b);freed(before);
        // The two stack descriptors were lent; neither may be dropped.
    }
    jet_rt_handle_drop_jet_row_policy(NULL);
}
static void enum_cases(int64_t variants,int64_t *(*clone)(int64_t *),void (*drop)(int64_t *),bool (*eq)(int64_t *,int64_t *)) {
    for(int64_t i=0;i<variants;++i) {
        int64_t *a=jet_rt_alloc(8,8);*a=i;int64_t *b=clone(a);assert(a!=b && *b==i && eq(a,a) && eq(a,b));
        for(int64_t j=0;j<variants;++j) { *b=j;assert(eq(a,b)==(i==j)); }
        watch(a,8,8);watch(b,8,8);unsigned before=intercepted;drop(a);drop(b);freed(before);
    }
    drop(NULL);
}
static void bitset_cases(void) {
    Sequence *a=jet_rt_native_bits_new();expect_s(jet_rt_handle_show_jet_bit_set(a),"[]");expect_s(jet_rt_handle_debug_jet_bit_set(a),"[]");expect_s(jet_rt_handle_display_jet_bit_set(a),"[]");
    Sequence *empty=jet_rt_handle_clone_jet_bit_set(a);assert(empty!=a && empty->length==0);
    watch_sequence(empty,8);unsigned empty_before=intercepted;jet_rt_handle_drop_jet_bit_set(empty);freed(empty_before);
    assert(!jet_bitset_add(a,jet_int_owned_from_i64(-1)));
    assert(jet_bitset_add(a,jet_int_owned_from_i64(INT64_MAX)));
    assert(jet_bitset_add(a,jet_int_owned_from_i64(2)));
    assert(jet_bitset_add(a,jet_int_owned_from_i64(0)));
    assert(!jet_bitset_add(a,jet_int_owned_from_i64(2)));
    Sequence *b=jet_rt_handle_clone_jet_bit_set(a);assert(a!=b && a->data!=b->data && b->length==3);
    expect_s(jet_rt_handle_show_jet_bit_set(a),"[0, 2, 9223372036854775807]");expect_s(jet_rt_handle_debug_jet_bit_set(a),"[0, 2, 9223372036854775807]");expect_s(jet_rt_handle_display_jet_bit_set(a),"[0, 2, 9223372036854775807]");
    jet_bitset_remove(a,jet_int_owned_from_i64(2));expect_s(jet_rt_handle_show_jet_bit_set(a),"[0, 9223372036854775807]");expect_s(jet_rt_handle_show_jet_bit_set(b),"[0, 2, 9223372036854775807]");
    watch_sequence(a,8);watch_sequence(b,8);unsigned before=intercepted;jet_rt_handle_drop_jet_bit_set(a);jet_rt_handle_drop_jet_bit_set(b);freed(before);
    jet_rt_handle_drop_jet_bit_set(NULL);
}
int main(void) {
    jet_rt_alloc_init();buffer_cases();path_cases();policy_cases();bitset_cases();
    enum_cases(4,jet_rt_handle_clone_jet_failure_policy,jet_rt_handle_drop_jet_failure_policy,jet_rt_handle_eq_jet_failure_policy);
    enum_cases(7,jet_rt_handle_clone_jet_dispatch_state,jet_rt_handle_drop_jet_dispatch_state,jet_rt_handle_eq_jet_dispatch_state);
    enum_cases(1,jet_rt_handle_clone_jet_event_config_error,jet_rt_handle_drop_jet_event_config_error,jet_rt_handle_eq_jet_event_config_error);
    enum_cases(2,jet_rt_handle_clone_jet_late_event_disposition,jet_rt_handle_drop_jet_late_event_disposition,jet_rt_handle_eq_jet_late_event_disposition);
    assert(intercepted>0);puts("Jet byte/policy/enum glue: ok");return 0;
}
