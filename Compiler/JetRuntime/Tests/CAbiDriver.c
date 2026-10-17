// Real runtime C-ABI client; no Rust driver/runtime-layout interpretation.
// Mirrors the retired native_backend_stop_and_euclid_exports contract and
// retains allocator process modes for run-alloc-parity.mjs.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <pthread.h>
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
static int64_t count_of(Text value) { return ((const int64_t *)(const void *)value)[-1]; }
static void text_carriers(void) {
    static const unsigned char input[]={ 'h',0xc3,0xa9,0,0xf0,0x9f,0x98,0x80 };
    const uint64_t len=sizeof(input);
    // P2 (#5041): a heap text is [count][data,len,cap]; clone shares it.
    Text carrier=jet_rt_string_from_static(input,(int64_t)len); const unsigned char *ptr=carrier->ptr;
    assert(jet_rt_string_data(carrier)==ptr && jet_rt_string_byte_len(carrier)==(int64_t)len && jet_char_len(carrier)==4 && count_of(carrier)==1);
    Text clone=jet_rt_string_clone(carrier); assert(clone==carrier && clone->ptr==ptr && count_of(carrier)==2); jet_rt_string_drop(carrier);
    assert(count_of(clone)==1 && clone->len==len && !memcmp(clone->ptr,input,(size_t)len)); jet_rt_string_drop(clone);
    // rt_static_text headers (cap 0 over rodata) are immortal: no count word is read or written.
    static TextCarrier literal={ (unsigned char *)(uintptr_t)input, sizeof(input), 0 };
    assert(jet_rt_string_clone(&literal)==&literal); jet_rt_string_drop(&literal); jet_rt_string_drop(&literal);
    assert(literal.ptr==input && literal.len==len && literal.cap==0);
    Text empty=jet_rt_string_builder_new(); assert((uintptr_t)empty->ptr==1 && empty->len==0 && empty->cap==0 && jet_string_is_empty(empty) && count_of(empty)==1);
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
// A builder must be unique when written: a shared one is a runtime defect stop.
static void shared_text_write(void) {
    Text builder=jet_rt_string_builder_new(); Text alias=jet_rt_string_clone(builder);
    jet_rt_string_builder_push_static(builder,"x",1);
    jet_rt_string_drop(alias); jet_rt_string_drop(builder);
}
// Atomic counts: 8 threads retain and release one block concurrently; then
// 8 concurrent final releases see exactly one last owner.
extern void jet_rt_rc_retain(int64_t *); extern bool jet_rt_rc_release(int64_t *); extern bool jet_rt_rc_unique(int64_t *);
static int64_t shared_count;
static int last_owners;
static void *rc_churn(void *unused) {
    (void)unused;
    for(int i=0;i<100000;++i) { jet_rt_rc_retain(&shared_count); assert(!jet_rt_rc_release(&shared_count)); }
    jet_rt_rc_retain(&shared_count);
    return NULL;
}
static void *rc_last(void *unused) {
    (void)unused;
    if(jet_rt_rc_release(&shared_count)) __atomic_add_fetch(&last_owners,1,__ATOMIC_SEQ_CST);
    return NULL;
}
static void refcount_threads(void) {
    pthread_t threads[8];
    shared_count=1;
    for(int i=0;i<8;++i) assert(!pthread_create(&threads[i],NULL,rc_churn,NULL));
    for(int i=0;i<8;++i) assert(!pthread_join(threads[i],NULL));
    assert(shared_count==9 && !jet_rt_rc_unique(&shared_count));
    assert(!jet_rt_rc_release(&shared_count) && shared_count==8);
    for(int i=0;i<8;++i) assert(!pthread_create(&threads[i],NULL,rc_last,NULL));
    for(int i=0;i<8;++i) assert(!pthread_join(threads[i],NULL));
    assert(last_owners==1 && shared_count==0);
    int64_t immortal=INT64_MIN; jet_rt_rc_retain(&immortal); assert(!jet_rt_rc_release(&immortal) && immortal==INT64_MIN && !jet_rt_rc_unique(&immortal));
    puts("refcount ok");
}
// P2 (#5041): every align-8 heap block (List-family buffers, boxes) carries
// its count 24 bytes before the payload, in the allocator's block header.
// For every small class and a large mapping: 8 threads retain/release one
// block while allocating and filling private blocks of the same class (the
// count and the payload never disturb each other); then 9 concurrent final
// releases see exactly one last owner, which frees the block from its own
// thread, not the allocating one.
extern void jet_rt_free(void *,int64_t,int64_t);
extern void jet_rt_block_retain(void *); extern bool jet_rt_block_release(void *); extern bool jet_rt_block_unique(void *);
static const int64_t block_sizes[]={8,24,40,104,232,488,1000,2024,4000,70000};
static void *shared_block;
static int64_t shared_size;
static int block_last;
static void *block_churn(void *unused) {
    (void)unused;
    for(int i=0;i<20000;++i) { jet_rt_block_retain(shared_block); assert(!jet_rt_block_release(shared_block)); }
    jet_rt_block_retain(shared_block);
    for(int i=0;i<64;++i) {
        void *own=jet_rt_alloc(shared_size,8);
        assert(((const int64_t *)own)[-3]==1 && jet_rt_block_unique(own));
        memset(own,0xA5,(size_t)shared_size);
        assert(jet_rt_block_unique(own));
        jet_rt_free(own,shared_size,8);
    }
    return NULL;
}
static void *block_final(void *unused) {
    (void)unused;
    if(jet_rt_block_release(shared_block)) { __atomic_add_fetch(&block_last,1,__ATOMIC_SEQ_CST); jet_rt_free(shared_block,shared_size,8); }
    return NULL;
}
static void block_threads(void) {
    for(size_t s=0;s<sizeof(block_sizes)/sizeof(block_sizes[0]);++s) {
        pthread_t threads[9];
        shared_size=block_sizes[s]; block_last=0;
        shared_block=jet_rt_alloc(shared_size,8);
        assert(((const int64_t *)shared_block)[-3]==1 && jet_rt_block_unique(shared_block));
        memset(shared_block,0x5A,(size_t)shared_size);
        for(int i=0;i<8;++i) assert(!pthread_create(&threads[i],NULL,block_churn,NULL));
        for(int i=0;i<8;++i) assert(!pthread_join(threads[i],NULL));
        assert(((const int64_t *)shared_block)[-3]==9 && !jet_rt_block_unique(shared_block));
        for(int64_t at=0;at<shared_size;++at) assert(((const unsigned char *)shared_block)[at]==0x5A);
        for(int i=0;i<9;++i) assert(!pthread_create(&threads[i],NULL,block_final,NULL));
        for(int i=0;i<9;++i) assert(!pthread_join(threads[i],NULL));
        assert(block_last==1);
    }
    puts("blocks ok");
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
extern void jet_rt_host_arguments(int64_t,char **,char **);
extern char **environ;
int main(int argc,char **argv) {
    // Startup env/argv capture precedes any runtime entry (HostStartup.jet).
    jet_rt_host_arguments(argc,argv,environ);
    void (*entry)(void)=NULL;
    if(argc==2) {
#ifndef C_ABI_ALLOC_ONLY
        if(!strcmp(argv[1],"values")) entry=values;
        else if(!strcmp(argv[1],"text-carriers")) entry=text_carriers;
        else if(!strcmp(argv[1],"shared-text-write")) entry=shared_text_write;
        else if(!strcmp(argv[1],"refcount-threads")) entry=refcount_threads;
        else if(!strcmp(argv[1],"block-threads")) entry=block_threads;
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
