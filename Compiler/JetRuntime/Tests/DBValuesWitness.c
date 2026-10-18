/* Real composed Jet object client, --wrap=jet_rt_free; compose
 * TLSRootsWitnessABI.jet for canonical stride. No providers/mocks. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
#include <inttypes.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; unsigned char *data; } Bytes;
typedef struct { int64_t tag; union { int64_t number; uint64_t bits; Text *text; Bytes blob; } value; } Value;
typedef struct { Text *message; } Error;
_Static_assert(sizeof(Value)==32 && _Alignof(Value)==8 && offsetof(Value,value)==8,"DBValue ABI");
_Static_assert(sizeof(Error)==8 && _Alignof(Error)==8,"DBError ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t runtime_tls_u8_stride(void);
extern Text *jet_rt_string_from_static(const char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Value *jet_rt_record_new_db_value_null(void);
extern Value *jet_rt_record_new_db_value_int(int64_t);
extern Value *jet_rt_record_new_db_value_float(double);
extern Value *jet_rt_record_new_db_value_text(Text *);
extern Value *jet_rt_record_new_db_value_bool(uint8_t);
extern Value *jet_rt_record_new_db_value_blob(Bytes *);
extern int64_t jet_rt_record_get_db_value_int(Value *);
extern double jet_rt_record_get_db_value_float(Value *);
extern Text *jet_rt_record_get_db_value_text(Value *);
extern bool jet_rt_record_get_db_value_bool(Value *);
extern Bytes *jet_rt_record_get_db_value_blob(Value *);
extern int64_t jet_rt_record_tag_db_value(Value *);
extern Value *jet_rt_handle_clone_db_value(Value *);
extern void jet_rt_handle_drop_db_value(Value *);
extern bool jet_rt_handle_eq_db_value(Value *,Value *);
extern Text *jet_rt_handle_show_db_value(Value *);
extern Text *jet_rt_handle_display_db_value(Value *);
extern Error *jet_rt_record_new_db_error(Text *);
extern Error *jet_rt_handle_clone_db_error(Error *);
extern void jet_rt_handle_drop_db_error(Error *);
extern bool jet_rt_handle_eq_db_error(Error *,Error *);
extern Text *jet_rt_record_get_db_error_message(Error *);
extern Text *jet_rt_handle_show_db_error(Error *);
extern Text *jet_rt_handle_display_db_error(Error *);
extern Text *jet_rt_handle_debug_db_error(Error *);
struct Watch { void *ptr; int64_t size,align; unsigned freed; };
static struct Watch watched[4096];
static size_t count,intercepted;
static int64_t stride;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[count++]=(struct Watch){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(watched[i-1].size==size && watched[i-1].align==align); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *t) { watch((char *)(t)-8,32,8); if(t->cap) watch(t->data,t->cap,1); }
static void value_watch(Value *v) { watch(v,32,8); if(v->tag==3) text_watch(v->value.text); if(v->tag==5 && v->value.blob.cap) watch(v->value.blob.data,v->value.blob.cap*stride,8); }
static void error_watch(Error *e) { watch(e,8,8); text_watch(e->message); }
static void expect_bytes(Text *t,const char *s,size_t len) { assert(t && t->len==(int64_t)len && !memcmp(t->data,s,len)); text_watch(t); jet_rt_string_drop(t); }
static void expect(Text *t,const char *s) { expect_bytes(t,s,strlen(s)); }
static Bytes *blob(int64_t len,int64_t cap) {
    Bytes *b=jet_rt_alloc(24,8); watch(b,24,8); b->len=len; b->cap=cap; b->data=cap ? jet_rt_alloc(cap*stride,8) : NULL;
    if(cap) { watch(b->data,cap*stride,8); memset(b->data,0x5a,(size_t)(cap*stride)); }
    for(int64_t i=0;i<len;++i) b->data[i*stride]=(unsigned char)(i ? 255 : 0);
    return b;
}
static double from_bits(uint64_t raw) { double f; memcpy(&f,&raw,8); return f; }
static uint64_t bits(double f) { uint64_t raw; memcpy(&raw,&f,8); return raw; }
static void stop_mode(const char *mode) {
    if(!strcmp(mode,"int-overflow") || !strcmp(mode,"int-underflow")) {
        bool negative=!strcmp(mode,"int-underflow"); int64_t bound=jet_int_owned_from_i64(negative ? INT64_MIN : INT64_MAX);
        int64_t one=jet_int_owned_from_i64(negative ? -1 : 1),outside=jet_int_add(bound,one);
        (void)jet_rt_record_new_db_value_int(outside); return;
    }
    Value *null=jet_rt_record_new_db_value_null();
    if(!strcmp(mode,"wrong-int")) { (void)jet_rt_record_get_db_value_int(null); return; }
    if(!strcmp(mode,"wrong-float")) { (void)jet_rt_record_get_db_value_float(null); return; }
    if(!strcmp(mode,"wrong-text")) { (void)jet_rt_record_get_db_value_text(null); return; }
    if(!strcmp(mode,"wrong-bool")) { (void)jet_rt_record_get_db_value_bool(null); return; }
    if(!strcmp(mode,"wrong-blob")) { (void)jet_rt_record_get_db_value_blob(null); return; }
    assert(!"unknown mode");
}
int main(int argc,char **argv) {
    jet_rt_alloc_init(); stride=runtime_tls_u8_stride(); assert(stride==1 || stride==8);
    if(argc==2) { stop_mode(argv[1]); return 99; }
    jet_rt_handle_drop_db_value(NULL); jet_rt_handle_drop_db_error(NULL);
    Value *null=jet_rt_record_new_db_value_null(); value_watch(null); null->value.bits=UINT64_MAX;
    Value *nc=jet_rt_handle_clone_db_value(null); value_watch(nc); assert(jet_rt_handle_eq_db_value(null,nc));
    expect(jet_rt_handle_show_db_value(null),"null"); expect(jet_rt_handle_display_db_value(null),"null");
    jet_rt_handle_drop_db_value(null); jet_rt_handle_drop_db_value(nc);
    const int64_t integers[]={INT64_MIN,-1,0,INT64_MAX};
    for(size_t i=0;i<4;++i) {
        int64_t n=jet_int_owned_from_i64(integers[i]); Value *v=jet_rt_record_new_db_value_int(n); value_watch(v);
        Value *copy=jet_rt_handle_clone_db_value(v); value_watch(copy); assert(v->value.number==integers[i] && jet_rt_handle_eq_db_value(v,copy));
        int64_t got=jet_rt_record_get_db_value_int(v); assert(jet_int_owned_to_i64(got)==integers[i]); jet_rt_int_drop(got); jet_rt_int_drop(n);
        char expected[32]; snprintf(expected,sizeof(expected),"%" PRId64,integers[i]);
        expect(jet_rt_handle_show_db_value(v),expected); expect(jet_rt_handle_display_db_value(v),expected);
        copy->value.number^=1; assert(!jet_rt_handle_eq_db_value(v,copy)); jet_rt_handle_drop_db_value(v); jet_rt_handle_drop_db_value(copy);
    }
    const uint64_t raw[]={UINT64_C(0),UINT64_C(0x8000000000000000),UINT64_C(0x7ff0000000000000),UINT64_C(0xfff0000000000000),UINT64_C(0x7ff8000000000123),UINT64_C(0x3ff8000000000000),UINT64_C(0x3eb0c6f7a0b5ed8d),UINT64_C(0x4415af1d78b58c40),UINT64_C(1)};
    const char *const forms[]={"0","-0","inf","-inf","NaN","1.5","0.000001","100000000000000000000",NULL};
    char subnormal[327]; subnormal[0]='0'; subnormal[1]='.'; memset(subnormal+2,'0',323); subnormal[325]='5'; subnormal[326]='\0';
    for(size_t i=0;i<sizeof(raw)/sizeof(raw[0]);++i) {
        Value *v=jet_rt_record_new_db_value_float(from_bits(raw[i])); value_watch(v);
        Value *copy=jet_rt_handle_clone_db_value(v); value_watch(copy);
        assert(bits(jet_rt_record_get_db_value_float(v))==raw[i] && copy->value.bits==raw[i]);
        bool nan=i==4; assert(jet_rt_handle_eq_db_value(v,copy)==!nan); assert(jet_rt_handle_eq_db_value(v,v)==!nan);
        const char *expected=forms[i] ? forms[i] : subnormal;
        expect(jet_rt_handle_show_db_value(v),expected); expect(jet_rt_handle_display_db_value(v),expected);
        if(i<2) { copy->value.bits^=UINT64_C(0x8000000000000000); assert(jet_rt_handle_eq_db_value(v,copy)); }
        jet_rt_handle_drop_db_value(v); jet_rt_handle_drop_db_value(copy);
    }
    for(unsigned i=0;i<3;++i) {
        uint8_t b=(uint8_t)(i==0 ? 0 : i==1 ? 2 : 255); Value *v=jet_rt_record_new_db_value_bool(b); value_watch(v);
        assert(v->value.number==(int64_t)(b!=0) && jet_rt_record_get_db_value_bool(v)==(b!=0));
        v->value.number=b ? 0x102 : 0x100; Value *copy=jet_rt_handle_clone_db_value(v); value_watch(copy);
        assert(copy->value.number==(int64_t)(b!=0) && jet_rt_handle_eq_db_value(v,copy));
        expect(jet_rt_handle_show_db_value(v),b ? "true" : "false"); expect(jet_rt_handle_display_db_value(v),b ? "true" : "false");
        copy->value.number^=1; assert(!jet_rt_handle_eq_db_value(v,copy)); jet_rt_handle_drop_db_value(v); jet_rt_handle_drop_db_value(copy);
    }
    static const char binary[]="raw\0\"\\\n";
    Text *message=jet_rt_string_from_static(binary,sizeof(binary)-1); text_watch(message);
    Value *tv=jet_rt_record_new_db_value_text(message); value_watch(tv); assert(tv->value.text!=message);
    Value *tc=jet_rt_handle_clone_db_value(tv); value_watch(tc); assert(tc->value.text!=tv->value.text && jet_rt_handle_eq_db_value(tv,tc));
    Text *got=jet_rt_record_get_db_value_text(tv); text_watch(got); assert(got!=tv->value.text);
    expect_bytes(jet_rt_handle_show_db_value(tv),binary,sizeof(binary)-1); expect_bytes(jet_rt_handle_display_db_value(tv),binary,sizeof(binary)-1);
    tc->value.text->data[0]^=1; assert(!jet_rt_handle_eq_db_value(tv,tc)); tc->value.text->data[0]^=1;
    jet_rt_handle_drop_db_value(tv); assert(!memcmp(got->data,binary,sizeof(binary)-1)); jet_rt_string_drop(got); jet_rt_handle_drop_db_value(tc);
    Error *error=jet_rt_record_new_db_error(message); error_watch(error); Error *ec=jet_rt_handle_clone_db_error(error); error_watch(ec);
    assert(error->message!=message && ec->message!=error->message && jet_rt_handle_eq_db_error(error,ec));
    ec->message->data[0]^=1; assert(!jet_rt_handle_eq_db_error(error,ec)); ec->message->data[0]^=1;
    expect_bytes(jet_rt_handle_show_db_error(error),binary,sizeof(binary)-1); expect_bytes(jet_rt_handle_display_db_error(error),binary,sizeof(binary)-1); expect_bytes(jet_rt_handle_debug_db_error(error),binary,sizeof(binary)-1);
    got=jet_rt_record_get_db_error_message(error); text_watch(got); jet_rt_handle_drop_db_error(error); assert(!memcmp(got->data,binary,sizeof(binary)-1));
    jet_rt_string_drop(got); jet_rt_handle_drop_db_error(ec); jet_rt_string_drop(message);
    for(int64_t len=0;len<4;++len) {
        Bytes *input=blob(len,len+2); unsigned char *data=input->data; Value *v=jet_rt_record_new_db_value_blob(input); watch(v,32,8);
        Bytes *view=jet_rt_record_get_db_value_blob(v); assert(view==&v->value.blob && view->data==data);
        Value *copy=jet_rt_handle_clone_db_value(v); value_watch(copy); assert(copy->value.blob.cap==len && jet_rt_handle_eq_db_value(v,copy));
        if(len) { assert(copy->value.blob.data!=data); copy->value.blob.data[0]^=1; assert(!jet_rt_handle_eq_db_value(v,copy)); copy->value.blob.data[0]^=1; }
        const char *const forms_blob[]={"[]","[0]","[0, 255]","[0, 255, 255]"};
        expect(jet_rt_handle_show_db_value(v),forms_blob[len]); expect(jet_rt_handle_display_db_value(v),forms_blob[len]);
        copy->value.blob.len^=1; assert(!jet_rt_handle_eq_db_value(v,copy)); copy->value.blob.len^=1;
        jet_rt_handle_drop_db_value(v); if(len) assert(copy->value.blob.data[0]==0); jet_rt_handle_drop_db_value(copy);
    }
    Text *empty=jet_rt_string_from_static("",0); text_watch(empty); int64_t zero=jet_int_owned_from_i64(0);
    Value *values[]={jet_rt_record_new_db_value_null(),jet_rt_record_new_db_value_int(zero),jet_rt_record_new_db_value_float(0),jet_rt_record_new_db_value_text(empty),jet_rt_record_new_db_value_bool(0),jet_rt_record_new_db_value_blob(blob(0,0))};
    for(size_t i=0;i<6;++i) { watch(values[i],32,8); if(i==3) text_watch(values[i]->value.text); assert(jet_rt_record_tag_db_value(values[i])==(int64_t)i); for(size_t j=0;j<6;++j) assert(jet_rt_handle_eq_db_value(values[i],values[j])==(i==j)); }
    for(size_t i=0;i<6;++i) { jet_rt_handle_drop_db_value(values[i]); }
    jet_rt_string_drop(empty); jet_rt_int_drop(zero);
    assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("DB25: all variants, deep owners, IEEE equality, Rust Display, borrowed blob, raw error OK");
    return 0;
}
