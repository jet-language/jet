/* Real Jet object client: link --wrap=jet_rt_free; no provider implementations.
 * CAbiHost carrier layouts; frozen seed 89622-89747 (record_index_stop),
 * 72109-72218 (renders); current Host adds AlreadyExists/Unsupported. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
#include <inttypes.h>
typedef struct { unsigned char *data; int64_t len,capacity; } Text;
typedef struct { int64_t operation; Text *resource; int64_t present,code; Text *cause; } Context;
typedef struct { int64_t kind,operation; Text *resource; int64_t present,code; Text *cause; int64_t limit; } Error;
_Static_assert(sizeof(Context)==40 && _Alignof(Context)==8 && offsetof(Context,cause)==32,"context ABI");
_Static_assert(sizeof(Error)==56 && _Alignof(Error)==8 && offsetof(Error,resource)==16 && offsetof(Error,limit)==48,"error ABI");
_Static_assert(sizeof(Text)==24 && _Alignof(Text)==8,"E4 ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Context *jet_rt_record_new_io_context(int64_t,Text **,int64_t *,Text **);
extern Context *jet_rt_handle_clone_io_context(Context *);
extern void jet_rt_handle_drop_io_context(Context *);
extern bool jet_rt_handle_eq_io_context(Context *,Context *);
extern Text *jet_rt_handle_debug_io_context(Context *);
extern int64_t jet_rt_record_get_io_context_operation(Context *);
extern int64_t jet_rt_record_get_io_context_resource(Context *,Text **);
extern int64_t jet_rt_record_get_io_context_cause(Context *,Text **);
extern int64_t jet_rt_record_get_io_context_os_code(Context *,int64_t *);
extern Error *jet_rt_handle_clone_io_error(Error *);
extern void jet_rt_handle_drop_io_error(Error *);
extern bool jet_rt_handle_eq_io_error(Error *,Error *);
extern Text *jet_rt_handle_debug_io_error(Error *);
extern Text *jet_rt_handle_show_io_error(Error *);
extern Text *jet_rt_handle_display_io_error(Error *);
extern int64_t jet_rt_record_tag_io_error(Error *);
extern Error *jet_rt_record_new_io_error_resource_limit(int64_t);
extern int64_t jet_rt_record_get_io_error_resource_limit(Error *);
extern int64_t *jet_rt_handle_clone_io_operation(int64_t *);
extern void jet_rt_handle_drop_io_operation(int64_t *);
extern bool jet_rt_handle_eq_io_operation(int64_t *,int64_t *);
#define VARIANT(name) extern Error *jet_rt_record_new_io_error_##name(Context *); extern Context *jet_rt_record_get_io_error_##name(Error *)
VARIANT(InvalidInput); VARIANT(NotFound); VARIANT(PermissionDenied); VARIANT(TimedOut);
VARIANT(Cancelled); VARIANT(Closed); VARIANT(Protocol); VARIANT(Other);
VARIANT(AlreadyExists); VARIANT(Unsupported);
#undef VARIANT
typedef Error *(*Ctor)(Context *);
typedef Context *(*Get)(Error *);
static const Ctor ctors[]={jet_rt_record_new_io_error_invalid_input,jet_rt_record_new_io_error_not_found,jet_rt_record_new_io_error_permission_denied,jet_rt_record_new_io_error_timed_out,jet_rt_record_new_io_error_cancelled,jet_rt_record_new_io_error_closed,jet_rt_record_new_io_error_protocol,jet_rt_record_new_io_error_other,jet_rt_record_new_io_error_already_exists,jet_rt_record_new_io_error_unsupported};
static const Get getters[]={jet_rt_record_get_io_error_invalid_input,jet_rt_record_get_io_error_not_found,jet_rt_record_get_io_error_permission_denied,jet_rt_record_get_io_error_timed_out,jet_rt_record_get_io_error_cancelled,jet_rt_record_get_io_error_closed,jet_rt_record_get_io_error_protocol,jet_rt_record_get_io_error_other,jet_rt_record_get_io_error_already_exists,jet_rt_record_get_io_error_unsupported};
static const char *const variants[]={"InvalidInput","NotFound","PermissionDenied","TimedOut","Cancelled","Closed","Protocol","Other","AlreadyExists","Unsupported"};
static const char *const kinds[]={"invalid input","not found","permission denied","timed out","cancelled","closed","protocol error","I/O error","I/O error","I/O error"};
static const char *const operations[]={"Read","Write","Flush","Connect","Accept","Close","Resolve","Codec"};
static const char *const lower_ops[]={"read","write","flush","connect","accept","close","resolve","codec"};
struct Watch { void *ptr; int64_t size,align; unsigned freed; };
static struct Watch watched[32768];
static size_t count,intercepted;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[count++]=(struct Watch){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(watched[i-1].size==size && watched[i-1].align==align);
        watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *text) {
    if(!text) return;
    watch(text,24,8);
    if(text->capacity) watch(text->data,text->capacity,1);
}
static void context_watch(Context *c) { watch(c,40,8); text_watch(c->resource); text_watch(c->cause); }
static void error_watch(Error *e,bool children) { watch(e,56,8); if(children) { text_watch(e->resource); text_watch(e->cause); } }
static void expect(Text *got,const char *wanted) {
    assert(got && got->len==(int64_t)strlen(wanted));
    assert(!memcmp(got->data,wanted,(size_t)got->len));
    text_watch(got); jet_rt_string_drop(got);
}
static void independent(Context *a,Context *b) {
    assert(a!=b && a->resource!=b->resource && a->cause!=b->cause);
    if(a->resource->len) assert(a->resource->data!=b->resource->data);
    if(a->cause->len) assert(a->cause->data!=b->cause->data);
}
static void context_eq_fields(Context *a,Context *b) {
    assert(jet_rt_handle_eq_io_context(a,b));
    b->operation^=1; assert(!jet_rt_handle_eq_io_context(a,b)); b->operation^=1;
    b->present=0; assert(!jet_rt_handle_eq_io_context(a,b)); b->present=0x100;
    b->code^=1; assert(!jet_rt_handle_eq_io_context(a,b)); b->code^=1;
    Text *keep=b->resource; b->resource=NULL; assert(!jet_rt_handle_eq_io_context(a,b)); b->resource=keep;
    keep=b->cause; b->cause=NULL; assert(!jet_rt_handle_eq_io_context(a,b)); b->cause=keep;
    if(b->resource->len) { b->resource->data[0]^=1; assert(!jet_rt_handle_eq_io_context(a,b)); b->resource->data[0]^=1; }
    if(b->cause->len) { b->cause->data[0]^=1; assert(!jet_rt_handle_eq_io_context(a,b)); b->cause->data[0]^=1; }
    a->present=0; b->present=0; b->code^=1; assert(jet_rt_handle_eq_io_context(a,b)); b->code^=1;
    a->present=1; b->present=0x100; assert(jet_rt_handle_eq_io_context(a,b));
}
static void stop_mode(const char *mode) {
    if(!strcmp(mode,"bad-op-low")) { (void)jet_rt_record_new_io_context(-1,NULL,NULL,NULL); return; }
    if(!strcmp(mode,"bad-op-high")) { (void)jet_rt_record_new_io_context(8,NULL,NULL,NULL); return; }
    if(!strcmp(mode,"bad-limit-low")) { (void)jet_rt_record_new_io_error_resource_limit(-1); return; }
    if(!strcmp(mode,"bad-limit-high")) { (void)jet_rt_record_new_io_error_resource_limit(5); return; }
    if(!strcmp(mode,"code-overflow") || !strcmp(mode,"code-underflow")) {
        bool negative=!strcmp(mode,"code-underflow");
        int64_t bound=jet_int_owned_from_i64(negative ? INT64_MIN : INT64_MAX);
        int64_t one=jet_int_owned_from_i64(negative ? -1 : 1);
        int64_t outside=jet_int_add(bound,one);
        (void)jet_rt_record_new_io_context(0,NULL,&outside,NULL); return;
    }
    Context *c=jet_rt_record_new_io_context(0,NULL,NULL,NULL);
    Error *e=jet_rt_record_new_io_error_other(c);
    if(!strcmp(mode,"wrong-limit")) { (void)jet_rt_record_get_io_error_resource_limit(e); return; }
    if(!strcmp(mode,"wrong-context")) { (void)jet_rt_record_get_io_error_not_found(e); return; }
    if(!strcmp(mode,"limit-as-context")) { (void)jet_rt_record_get_io_error_other(jet_rt_record_new_io_error_resource_limit(0)); return; }
    assert(!"unknown mode");
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) { stop_mode(argv[1]); return 99; }
    jet_rt_handle_drop_io_context(NULL); jet_rt_handle_drop_io_error(NULL); jet_rt_handle_drop_io_operation(NULL);
    for(int64_t op=0;op<8;++op) {
        int64_t *handle=jet_rt_alloc(8,8); *handle=op; watch(handle,8,8);
        int64_t *copy=jet_rt_handle_clone_io_operation(handle); watch(copy,8,8);
        assert(copy!=handle && *copy==op && jet_rt_handle_eq_io_operation(handle,copy));
        *copy=(op+1)%8; assert(!jet_rt_handle_eq_io_operation(handle,copy));
        jet_rt_handle_drop_io_operation(copy); jet_rt_handle_drop_io_operation(handle);
    }
    const int64_t codes[]={INT64_MIN,-1,0,INT64_MAX};
    const char *const texts[]={"","r"};
    for(int64_t op=0;op<8;++op) for(size_t v=0;v<10;++v) for(unsigned mask=0;mask<8;++mask) for(size_t ci=0;ci<4;++ci) for(size_t ti=0;ti<2;++ti) {
        Text *resource=jet_rt_string_from_static(texts[ti],(int64_t)strlen(texts[ti]));
        Text *cause=jet_rt_string_from_static(texts[ti],(int64_t)strlen(texts[ti]));
        text_watch(resource); text_watch(cause);
        int64_t code=jet_int_owned_from_i64(codes[ci]);
        Context *c=jet_rt_record_new_io_context(op,mask&1 ? &resource : NULL,mask&2 ? &code : NULL,mask&4 ? &cause : NULL);
        context_watch(c);
        assert(jet_rt_record_get_io_context_operation(c)==op);
        assert(!c->resource || c->resource!=resource); assert(!c->cause || c->cause!=cause);
        Text *out=NULL; int64_t out_code=0;
        assert(jet_rt_record_get_io_context_resource(c,&out)==(int64_t)(mask&1));
        if(mask&1) { assert(out!=c->resource); expect(out,texts[ti]); }
        assert(jet_rt_record_get_io_context_cause(c,&out)==(int64_t)((mask>>2)&1));
        if(mask&4) { assert(out!=c->cause); expect(out,texts[ti]); }
        assert(jet_rt_record_get_io_context_os_code(c,&out_code)==(int64_t)((mask>>1)&1));
        if(mask&2) { assert(jet_int_owned_to_i64(out_code)==codes[ci]); jet_rt_int_drop(out_code); c->present=0x100; }
        Context *cc=jet_rt_handle_clone_io_context(c); context_watch(cc);
        assert(jet_rt_handle_eq_io_context(c,cc));
        if(mask==7) { independent(c,cc); context_eq_fields(c,cc); }
        else if(!(mask&2)) { cc->code=INT64_MIN; assert(jet_rt_handle_eq_io_context(c,cc)); }
        char r[64],o[64],a[64],ctx[512],debug[640],show[256];
        snprintf(r,sizeof(r),mask&1 ? "Val(\"%s\")" : "None",texts[ti]);
        snprintf(a,sizeof(a),mask&4 ? "Val(\"%s\")" : "None",texts[ti]);
        if(mask&2) snprintf(o,sizeof(o),"Val(%" PRId64 ")",codes[ci]); else strcpy(o,"None");
        snprintf(ctx,sizeof(ctx),"IOContext { operation: %s, resource: %s, os_code: %s, cause: %s }",operations[op],r,o,a);
        expect(jet_rt_handle_debug_io_context(c),ctx);
        Text *transferred_resource=c->resource,*transferred_cause=c->cause;
        Error *e=ctors[v](c); error_watch(e,false);
        assert(e->resource==transferred_resource && e->cause==transferred_cause);
        assert(jet_rt_record_tag_io_error(e)==(int64_t)(v<8 ? v : v+1));
        Context *got=getters[v](e); context_watch(got); assert(jet_rt_handle_eq_io_context(cc,got));
        if(mask==7) independent(cc,got);
        Error *ec=jet_rt_handle_clone_io_error(e); error_watch(ec,true);
        assert(jet_rt_handle_eq_io_error(e,ec)); ec->kind^=1; assert(!jet_rt_handle_eq_io_error(e,ec)); ec->kind^=1;
        ec->operation^=1; assert(!jet_rt_handle_eq_io_error(e,ec)); ec->operation^=1;
        if(mask&2) { ec->present=1; assert(jet_rt_handle_eq_io_error(e,ec)); ec->code^=1; assert(!jet_rt_handle_eq_io_error(e,ec)); ec->code^=1; }
        else { ec->code=INT64_MAX; assert(jet_rt_handle_eq_io_error(e,ec)); }
        snprintf(debug,sizeof(debug),"%s(%s)",variants[v],ctx); expect(jet_rt_handle_debug_io_error(e),debug);
        snprintf(show,sizeof(show),"%s during %s%s%s%s%s%s",kinds[v],lower_ops[op],mask&1 ? " `" : "",mask&1 ? texts[ti] : "",mask&1 ? "`" : "",mask&4 ? ": " : "",mask&4 ? texts[ti] : "");
        expect(jet_rt_handle_show_io_error(e),show); expect(jet_rt_handle_display_io_error(e),show);
        jet_rt_handle_drop_io_error(e); expect(jet_rt_handle_debug_io_context(got),ctx);
        jet_rt_handle_drop_io_context(got); jet_rt_handle_drop_io_context(cc); jet_rt_handle_drop_io_error(ec);
        jet_rt_string_drop(resource); jet_rt_string_drop(cause); jet_rt_int_drop(code);
        /* Watched addresses may be reused only after all owners in this cell die. */
        for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
        count=0;
    }
    const char *const limits[]={"WallTime","CpuTime","Memory","OpenFiles","Output"};
    const char *const limit_show[]={"wall time","CPU time","memory","open file handles","output"};
    for(int64_t i=0;i<5;++i) {
        Error *e=jet_rt_record_new_io_error_resource_limit(i); error_watch(e,true);
        Error *copy=jet_rt_handle_clone_io_error(e); error_watch(copy,true);
        assert(jet_rt_record_tag_io_error(e)==8 && jet_rt_record_get_io_error_resource_limit(e)==i);
        assert(jet_rt_handle_eq_io_error(e,copy)); copy->operation=7; copy->present=0x100; copy->code=INT64_MAX;
        assert(jet_rt_handle_eq_io_error(e,copy)); copy->limit=(i+1)%5; assert(!jet_rt_handle_eq_io_error(e,copy));
        char debug[80],show[120]; snprintf(debug,sizeof(debug),"ResourceLimit(%s)",limits[i]);
        snprintf(show,sizeof(show),"process resource limit exceeded: %s",limit_show[i]);
        expect(jet_rt_handle_debug_io_error(e),debug); expect(jet_rt_handle_show_io_error(e),show); expect(jet_rt_handle_display_io_error(e),show);
        jet_rt_handle_drop_io_error(e); jet_rt_handle_drop_io_error(copy);
    }
    static const char raw[]="a\0\n\r\t\\\"'\x7f\xc2\x85\xcc\x81\xe2\x80\x8d\xf0\x9f\x98\x80";
    Text *quoted=jet_rt_string_from_static(raw,sizeof(raw)-1); text_watch(quoted);
    Context *q=jet_rt_record_new_io_context(7,&quoted,NULL,&quoted); context_watch(q);
    expect(jet_rt_handle_debug_io_context(q),"IOContext { operation: Codec, resource: Val(\"a\\0\\n\\r\\t\\\\\\\"'\\u{7f}\\u{85}\\u{301}\\u{200d}\xf0\x9f\x98\x80\"), os_code: None, cause: Val(\"a\\0\\n\\r\\t\\\\\\\"'\\u{7f}\\u{85}\\u{301}\\u{200d}\xf0\x9f\x98\x80\") }");
    jet_rt_handle_drop_io_context(q); jet_rt_string_drop(quoted);
    assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("IO37: operations=8 variants=11 limits=5 ownership/eq/render/options/word-presence OK");
    return 0;
}
