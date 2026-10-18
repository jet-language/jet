// Real Jet-object client; link -Wl,--wrap=jet_rt_free. No runtime providers.
// Layout assertions mirror EncodingABI.jet, never Rust layout.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; } Enum;
typedef struct { Text *kind; int64_t present, code; Text *message; } Cause;
typedef struct {
    int64_t format, kind, offset, line_present, line, column_present, column;
    Text *path, *reason; Cause *cause;
} Error;
_Static_assert(sizeof(Enum)==8, "EncodingABI enum8");
_Static_assert(sizeof(Cause)==32 && offsetof(Cause, code)==16 && offsetof(Cause,message)==24, "EncodingABI cause32");
_Static_assert(sizeof(Error)==80 && offsetof(Error,path)==56 && offsetof(Error,cause)==72, "EncodingABI error80");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t, int64_t);
extern void jet_rt_int_drop(int64_t);
extern Enum *jet_rt_handle_clone_encoding_format(Enum *);
extern void jet_rt_handle_drop_encoding_format(Enum *);
extern bool jet_rt_handle_eq_encoding_format(Enum *, Enum *);
extern Enum *jet_rt_handle_clone_encoding_error_kind(Enum *);
extern void jet_rt_handle_drop_encoding_error_kind(Enum *);
extern bool jet_rt_handle_eq_encoding_error_kind(Enum *, Enum *);
extern Cause *jet_rt_record_new_encoding_cause(Text *, const int64_t *, Text *);
extern Cause *jet_rt_handle_clone_encoding_cause(Cause *);
extern void jet_rt_handle_drop_encoding_cause(Cause *);
extern bool jet_rt_handle_eq_encoding_cause(Cause *, Cause *);
extern Text *jet_rt_record_get_encoding_cause_kind(Cause *);
extern Text *jet_rt_record_get_encoding_cause_message(Cause *);
extern int64_t jet_rt_record_get_encoding_cause_os_code(Cause *, int64_t *);
extern Error *jet_rt_record_new_encoding_error(int64_t,int64_t,int64_t,const int64_t *,const int64_t *,Text *,Text *,Cause *);
extern Error *jet_rt_handle_clone_encoding_error(Error *);
extern void jet_rt_handle_drop_encoding_error(Error *);
extern bool jet_rt_handle_eq_encoding_error(Error *, Error *);
extern int64_t jet_rt_record_get_encoding_error_format(Error *);
extern int64_t jet_rt_record_get_encoding_error_kind(Error *);
extern int64_t jet_rt_record_get_encoding_error_byte_offset(Error *);
extern int64_t jet_rt_record_get_encoding_error_line(Error *,int64_t *);
extern int64_t jet_rt_record_get_encoding_error_column(Error *,int64_t *);
extern Text *jet_rt_record_get_encoding_error_path(Error *);
extern Text *jet_rt_record_get_encoding_error_reason(Error *);
extern int64_t jet_rt_record_get_encoding_error_cause(Error *,Cause **);
extern Text *jet_rt_handle_show_encoding_error(Error *);
extern Text *jet_rt_handle_display_encoding_error(Error *);
extern Text *jet_rt_handle_debug_encoding_error(Error *);
static struct { void *pointer; int64_t bytes, align; unsigned frees; } watched[80];
static size_t watched_count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    ++intercepted;
    for(size_t i=0;i<watched_count;++i) if(watched[i].pointer==pointer) {
        assert(watched[i].bytes==bytes && watched[i].align==align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(pointer,bytes,align);
}
static void watch(void *pointer,int64_t bytes,int64_t align) {
    assert(pointer && watched_count<80);
    for(size_t i=0;i<watched_count;++i) assert(watched[i].pointer!=pointer);
    watched[watched_count].pointer=pointer;
    watched[watched_count].bytes=bytes;
    watched[watched_count++].align=align;
}
static void watch_text(Text *t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_cause(Cause *c) { watch(c,32,8); watch_text(c->kind); watch_text(c->message); }
static void watch_error(Error *e) { watch(e,80,8); watch_text(e->path); watch_text(e->reason); if(e->cause) watch_cause(e->cause); }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); }
static void rendered(Error *e,const char *golden) {
    // Rust cites: CommonTypes182-217 -> Core/EncodingError8-28.
    Text *a=jet_rt_handle_show_encoding_error(e), *b=jet_rt_handle_display_encoding_error(e), *c=jet_rt_handle_debug_encoding_error(e);
    text_is(a,golden); text_is(b,golden); text_is(c,golden);
    assert(a!=b && b!=c && a!=c);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c);
}
static Cause *cause(const char *kind,const int64_t *code,const char *message) {
    Text *a=text(kind), *b=text(message);
    Cause *c=jet_rt_record_new_encoding_cause(a,code,b);
    assert(c->kind!=a && c->message!=b);
    jet_rt_string_drop(a); jet_rt_string_drop(b);
    return c;
}
static Error *error(int64_t format,int64_t kind,int64_t offset,const int64_t *line,const int64_t *column,const char *path,const char *reason,Cause *c) {
    Text *a=text(path), *b=text(reason);
    Error *e=jet_rt_record_new_encoding_error(format,kind,offset,line,column,a,b,c);
    assert(e->path!=a && e->reason!=b && e->cause==c); // ctor consumes c, unlike text
    jet_rt_string_drop(a); jet_rt_string_drop(b);
    return e;
}
static void enum_truth(void) {
    for(int64_t i=0;i<7;++i) {
        Enum *a=jet_rt_alloc(8,8); a->tag=i;
        Enum *b=jet_rt_handle_clone_encoding_format(a);
        assert(a!=b && b->tag==i && jet_rt_handle_eq_encoding_format(a,b));
        b->tag=(i+1)%7; assert(!jet_rt_handle_eq_encoding_format(a,b));
        jet_rt_handle_drop_encoding_format(a); jet_rt_handle_drop_encoding_format(b);
    }
    for(int64_t i=0;i<6;++i) {
        Enum *a=jet_rt_alloc(8,8); a->tag=i;
        Enum *b=jet_rt_handle_clone_encoding_error_kind(a);
        assert(a!=b && b->tag==i && jet_rt_handle_eq_encoding_error_kind(a,b));
        b->tag=(i+1)%6; assert(!jet_rt_handle_eq_encoding_error_kind(a,b));
        jet_rt_handle_drop_encoding_error_kind(a); jet_rt_handle_drop_encoding_error_kind(b);
    }
    jet_rt_handle_drop_encoding_format(NULL); jet_rt_handle_drop_encoding_error_kind(NULL);
}
static void equality(Error *a) {
    // Every field in EncodingTypes63-80 participates, including omitted render cause.
    Error *b=jet_rt_handle_clone_encoding_error(a);
    assert(jet_rt_handle_eq_encoding_error(a,a) && jet_rt_handle_eq_encoding_error(a,b));
    int64_t *fields[]={&b->format,&b->kind,&b->offset,&b->line_present,&b->line,&b->column_present,&b->column};
    for(size_t i=0;i<7;++i) { *fields[i]^=1; assert(!jet_rt_handle_eq_encoding_error(a,b)); *fields[i]^=1; }
    b->path->data[0]^=1; assert(!jet_rt_handle_eq_encoding_error(a,b)); b->path->data[0]^=1;
    b->reason->data[0]^=1; assert(!jet_rt_handle_eq_encoding_error(a,b)); b->reason->data[0]^=1;
    Cause *saved=b->cause; b->cause=NULL; assert(!jet_rt_handle_eq_encoding_error(a,b)); b->cause=saved;
    saved->kind->data[0]^=1; assert(!jet_rt_handle_eq_encoding_cause(a->cause,saved)); assert(!jet_rt_handle_eq_encoding_error(a,b)); saved->kind->data[0]^=1;
    saved->present^=1; assert(!jet_rt_handle_eq_encoding_cause(a->cause,saved)); saved->present^=1;
    saved->code^=1; assert(!jet_rt_handle_eq_encoding_cause(a->cause,saved)); saved->code^=1;
    saved->message->data[0]^=1; assert(!jet_rt_handle_eq_encoding_cause(a->cause,saved)); saved->message->data[0]^=1;
    assert(jet_rt_handle_eq_encoding_error(a,b)); jet_rt_handle_drop_encoding_error(b);
    int64_t zero=jet_int_owned_from_i64(0);
    Cause *abs=cause("IO",NULL,"x"), *present=cause("IO",&zero,"x");
    assert(!jet_rt_handle_eq_encoding_cause(abs,present));
    abs->code=17; Cause *clone=jet_rt_handle_clone_encoding_cause(abs); clone->code=99;
    assert(jet_rt_handle_eq_encoding_cause(abs,clone)); // inactive payload is ignored
    jet_rt_handle_drop_encoding_cause(abs); jet_rt_handle_drop_encoding_cause(present); jet_rt_handle_drop_encoding_cause(clone);
    Error *x=error(0,0,zero,NULL,NULL,"","x",NULL), *y=error(0,0,zero,&zero,NULL,"","x",NULL);
    assert(!jet_rt_handle_eq_encoding_error(x,y)); y->line_present=0; y->line=77; y->column=88;
    assert(jet_rt_handle_eq_encoding_error(x,y)); jet_rt_handle_drop_encoding_error(x); jet_rt_handle_drop_encoding_error(y);
    jet_rt_int_drop(zero);
}
static void labels(void) {
    static const char *formats[]={"JSON","JSONL","CSV","TOML","YAML","XML","CBOR"};
    static const char *kinds[]={"Syntax","Truncated","Unsupported","Limit","IO","State"};
    int64_t zero=jet_int_owned_from_i64(0);
    for(int64_t f=0;f<7;++f) for(int64_t k=0;k<6;++k) {
        Error *e=error(f,k,zero,NULL,NULL,"","why",NULL); char golden[160];
        int n=snprintf(golden,sizeof golden,"%s %s at byte 0: why",formats[f],kinds[k]); assert(n>0 && (size_t)n<sizeof golden);
        assert(jet_rt_record_get_encoding_error_format(e)==f && jet_rt_record_get_encoding_error_kind(e)==k);
        rendered(e,golden); int64_t out=123; Cause *c=(Cause *)(uintptr_t)123;
        assert(jet_rt_record_get_encoding_error_line(e,&out)==0 && out==123);
        assert(jet_rt_record_get_encoding_error_column(e,&out)==0 && out==123);
        assert(jet_rt_record_get_encoding_error_cause(e,&c)==0 && c==(Cause *)(uintptr_t)123);
        jet_rt_handle_drop_encoding_error(e);
    }
    Error *e=error(6,5,zero,NULL,&zero,"","column only",NULL);
    rendered(e,"CBOR State at byte 0, column 0: column only"); jet_rt_handle_drop_encoding_error(e); jet_rt_int_drop(zero);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        int64_t n=jet_int_owned_from_i64(INT64_MAX);
        Text *empty=text("");
        if(strcmp(argv[1],"format-range")==0) (void)jet_rt_record_new_encoding_error(7,0,n,NULL,NULL,empty,empty,NULL);
        else if(strcmp(argv[1],"kind-range")==0) (void)jet_rt_record_new_encoding_error(0,-1,n,NULL,NULL,empty,empty,NULL);
        else if(strcmp(argv[1],"native-overflow")==0) {
            int64_t one=jet_int_owned_from_i64(1), wide=jet_int_add(n,one);
            (void)jet_rt_record_new_encoding_error(0,0,wide,NULL,NULL,empty,empty,NULL);
        } else { fputs("unknown mode\n",stderr); return 2; }
        fputs("required range stop returned\n",stderr); return 1;
    }
    enum_truth(); labels();
    int64_t min=jet_int_owned_from_i64(INT64_MIN), max=jet_int_owned_from_i64(INT64_MAX), zero=jet_int_owned_from_i64(0);
    Cause *c=cause("IO",&min,"hidden cause");
    Error *a=error(0,4,min,&zero,&max,"$.name","bad UTF-8",c);
    assert(a->offset==INT64_MIN && a->line==0 && a->column==INT64_MAX && a->cause->code==INT64_MIN);
    jet_rt_int_drop(min); jet_rt_int_drop(max); jet_rt_int_drop(zero); // no exact owner retained
    rendered(a,"JSON IO at byte -9223372036854775808, line 0, column 9223372036854775807, path $.name: bad UTF-8");
    equality(a);
    Error *b=jet_rt_handle_clone_encoding_error(a);
    assert(a!=b && a->path!=b->path && a->path->data!=b->path->data && a->reason!=b->reason && a->cause!=b->cause);
    assert(a->cause->kind!=b->cause->kind && a->cause->message->data!=b->cause->message->data);
    Cause *g=NULL; assert(jet_rt_record_get_encoding_error_cause(a,&g)==1 && g!=a->cause && jet_rt_handle_eq_encoding_cause(g,a->cause));
    Text *path=jet_rt_record_get_encoding_error_path(a), *reason=jet_rt_record_get_encoding_error_reason(a);
    Text *kind=jet_rt_record_get_encoding_cause_kind(g), *message=jet_rt_record_get_encoding_cause_message(g);
    assert(path!=a->path && reason!=a->reason && kind!=g->kind && message!=g->message);
    int64_t offset=jet_rt_record_get_encoding_error_byte_offset(a), offset2=jet_rt_record_get_encoding_error_byte_offset(a), line=0,column=0,code=0;
    assert(offset!=offset2 && jet_int_owned_to_i64(offset)==INT64_MIN && jet_int_owned_to_i64(offset2)==INT64_MIN);
    assert(jet_rt_record_get_encoding_error_line(a,&line)==1 && jet_int_owned_to_i64(line)==0);
    assert(jet_rt_record_get_encoding_error_column(a,&column)==1 && jet_int_owned_to_i64(column)==INT64_MAX);
    assert(jet_rt_record_get_encoding_cause_os_code(g,&code)==1 && jet_int_owned_to_i64(code)==INT64_MIN);
    jet_rt_int_drop(offset); jet_rt_int_drop(offset2); jet_rt_int_drop(line); jet_rt_int_drop(column); jet_rt_int_drop(code);
    // No subsequent allocations until watched owners are destroyed: addresses cannot be recycled.
    watch_error(a); watch_error(b); watch_cause(g); watch_text(path); watch_text(reason); watch_text(kind); watch_text(message);
    jet_rt_handle_drop_encoding_error(a);
    text_is(b->path,"$.name"); text_is(b->cause->message,"hidden cause");
    text_is(path,"$.name"); text_is(reason,"bad UTF-8"); text_is(kind,"IO"); text_is(message,"hidden cause");
    jet_rt_handle_drop_encoding_error(b); jet_rt_handle_drop_encoding_cause(g);
    jet_rt_string_drop(path); jet_rt_string_drop(reason); jet_rt_string_drop(kind); jet_rt_string_drop(message);
    assert(intercepted>0 && watched_count>0);
    for(size_t i=0;i<watched_count;++i) assert(watched[i].frees==1);
    jet_rt_handle_drop_encoding_error(NULL); jet_rt_handle_drop_encoding_cause(NULL);
    puts("EncodingErrors ownership/equality/render OK"); return 0;
}
