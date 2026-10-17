// Real runtime C-ABI client; no Rust driver/runtime-layout interpretation.
// Mirrors the retired native_backend_stop_and_euclid_exports contract and
// retains allocator process modes for run-alloc-parity.mjs.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *ptr; uint64_t len,cap; } TextCarrier;
typedef TextCarrier *Text;
_Static_assert(sizeof(TextCarrier)==24 && offsetof(TextCarrier,cap)==16,"canonical E4 Text24");
extern int32_t jet_rt_main(void (*)(void));
extern Text jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text);
extern void *jet_rt_alloc(int64_t,int64_t);
extern Text jet_rt_string_clone(Text);
extern const unsigned char *jet_rt_string_data(Text);
extern int64_t jet_rt_string_byte_len(Text);
extern int64_t jet_char_len(Text);
extern bool jet_string_is_empty(Text);
extern Text jet_rt_string_builder_new(void);
extern void jet_rt_string_builder_push_static(Text,const void *,int64_t);
extern void jet_rt_string_builder_push(Text,Text);
extern Text jet_rt_string_builder_finish(Text);
extern Text jet_rt_int_to_string(int64_t);
extern void jet_term_write_stdout_line(Text,uint8_t);
extern Text jet_rt_require_debug_string(Text);
extern int64_t jet_int_div_euclid(int64_t,int64_t,const void *,int64_t,int64_t);
extern int64_t jet_int_rem_euclid(int64_t,int64_t,const void *,int64_t,int64_t);
extern void jet_rt_numeric_stop(const void *,int64_t,int64_t,const void *,int64_t);
extern void jet_rt_require_stop(int64_t,const uintptr_t *,Text,Text,Text,Text);
#ifndef C_ABI_ALLOC_ONLY
static const char file[]="main.jet", function[]="run", source[]="    #require_eq(total, 2)";
static Text text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void print_owned(Text value) { jet_term_write_stdout_line(value,1); jet_rt_string_drop(value); }
static void require_stop(int64_t kind,Text message,Text left,Text right,Text locals) {
    // Lower/Require.jet's eleven-word immutable descriptor. Context words0/0.
    const uintptr_t words[]={ (uintptr_t)file,sizeof(file)-1,3,
        (uintptr_t)function,sizeof(function)-1,(uintptr_t)source,sizeof(source)-1,5,23,0,0 };
    _Static_assert(sizeof(words)==11*sizeof(uintptr_t),"require descriptor11");
    jet_rt_require_stop(kind,words,message,left,right,locals);
}
static void values(void) {
    print_owned(jet_rt_int_to_string(jet_int_div_euclid(-7,2,file,sizeof(file)-1,4)));
    print_owned(jet_rt_int_to_string(jet_int_rem_euclid(-7,2,file,sizeof(file)-1,4)));
    Text raw=text("a\"b\n"); print_owned(jet_rt_require_debug_string(raw)); jet_rt_string_drop(raw);
}
static void text_carriers(void) {
    static const unsigned char input[]={ 'h',0xc3,0xa9,0,0xf0,0x9f,0x98,0x80 };
    const uint64_t len=sizeof(input);
    unsigned char *ptr=jet_rt_alloc((int64_t)len,1); memcpy(ptr,input,(size_t)len);
    Text carrier=jet_rt_alloc(24,8); *carrier=(TextCarrier){ptr,len,len};
    assert(jet_rt_string_data(carrier)==ptr && jet_rt_string_byte_len(carrier)==(int64_t)len && jet_char_len(carrier)==4);
    Text clone=jet_rt_string_clone(carrier); assert(clone!=carrier && clone->ptr!=ptr); jet_rt_string_drop(carrier);
    assert(clone->len==len && !memcmp(clone->ptr,input,(size_t)len)); jet_rt_string_drop(clone);
    Text empty=jet_rt_string_builder_new(); assert((uintptr_t)empty->ptr==1 && empty->len==0 && empty->cap==0 && jet_string_is_empty(empty));
    unsigned char seed[2*sizeof(input)]; memcpy(seed,input,(size_t)len); memcpy(seed+len,input,(size_t)len);
    jet_rt_string_builder_push_static(empty,seed,(int64_t)sizeof(seed));
    unsigned char *first=empty->ptr; jet_rt_string_builder_push(empty,empty);
    assert(empty->ptr!=first && empty->len==sizeof(seed)*2);
    const unsigned char *window=empty->ptr+len;
    jet_rt_string_builder_push_static(empty,window,(int64_t)len); // aliases + grows
    unsigned char *grown=empty->ptr; window=empty->ptr+len;
    jet_rt_string_builder_push_static(empty,window,(int64_t)len); // aliases without growth
    assert(empty->ptr==grown);
    window=empty->ptr; uint64_t count=empty->len;
    jet_rt_string_builder_push_static(empty,window,(int64_t)count); // aliases + grows
    Text finished=jet_rt_string_builder_finish(empty); assert(finished==empty && finished->len==12*len);
    for(uint64_t i=0;i<12;++i) assert(!memcmp(finished->ptr+i*len,input,(size_t)len));
    jet_rt_string_drop(finished); jet_rt_string_drop(NULL);
}
static void euclid_by_zero(void) { (void)jet_int_rem_euclid(1,0,file,sizeof(file)-1,6); }
static void numeric_stop(void) {
    static const char message[]="This addition overflows the value's type (the result is outside its range)";
    jet_rt_numeric_stop(file,sizeof(file)-1,7,message,sizeof(message)-1);
}
static void require(void) { require_stop(0,text("condition failed"),NULL,NULL,text("total = 1")); }
static void require_eq(void) { require_stop(1,NULL,text("1"),text("2"),NULL); }
static void panic_case(void) { require_stop(2,text("boom"),NULL,NULL,NULL); }
#endif
static void alloc_invalid_layout(void) { (void)jet_rt_alloc(1,3); }
static void alloc_failure(void) { (void)jet_rt_alloc(INT64_MAX-7,8); }
int main(int argc,char **argv) {
    void (*entry)(void)=NULL;
    if(argc==2) {
#ifndef C_ABI_ALLOC_ONLY
        if(!strcmp(argv[1],"values")) entry=values;
        else if(!strcmp(argv[1],"text-carriers")) entry=text_carriers;
        else if(!strcmp(argv[1],"euclid-by-zero")) entry=euclid_by_zero;
        else if(!strcmp(argv[1],"numeric-stop")) entry=numeric_stop;
        else if(!strcmp(argv[1],"require")) entry=require;
        else if(!strcmp(argv[1],"require-eq")) entry=require_eq;
        else if(!strcmp(argv[1],"panic")) entry=panic_case;
#endif
        if(!strcmp(argv[1],"alloc-invalid-layout")) entry=alloc_invalid_layout;
        else if(!strcmp(argv[1],"alloc-failure")) entry=alloc_failure;
    }
    if(!entry) { fputs("unknown C ABI case\n",stderr); return 2; }
    return jet_rt_main(entry);
}
