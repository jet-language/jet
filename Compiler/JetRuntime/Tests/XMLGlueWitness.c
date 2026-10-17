// Real Jet object client. Link -Wl,--wrap=jet_rt_free; no runtime substitutes.
// Native XMLABI layouts, not Rust layout. Rust goldens cite CommonTypes249-279
// -> Core/EncodingError8-28; enum spelling cites CommonTypes223-236.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; } Enum;
typedef struct {
    int64_t kind, byte_present, byte_offset, line_present, line, column_present, column;
    Text *path, *reason;
} Error;
typedef struct { int64_t encoding, lexical; } Render;
_Static_assert(sizeof(Error)==72 && offsetof(Error,path)==56 && offsetof(Error,column)==48, "XML error ABI");
_Static_assert(sizeof(Render)==16 && offsetof(Render,lexical)==8, "XML render ABI");
_Static_assert(sizeof(Enum)==8, "XML enum ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Enum *jet_rt_handle_clone_xml_reason(Enum *);
extern void jet_rt_handle_drop_xml_reason(Enum *);
extern bool jet_rt_handle_eq_xml_reason(Enum *,Enum *);
extern Enum *jet_rt_handle_clone_xml_encoding(Enum *);
extern void jet_rt_handle_drop_xml_encoding(Enum *);
extern bool jet_rt_handle_eq_xml_encoding(Enum *,Enum *);
extern Enum *jet_rt_handle_clone_xml_lexical_policy(Enum *);
extern void jet_rt_handle_drop_xml_lexical_policy(Enum *);
extern bool jet_rt_handle_eq_xml_lexical_policy(Enum *,Enum *);
extern Error *jet_rt_record_new_xml_error(int64_t,const int64_t *,const int64_t *,const int64_t *,Text *,Text *);
extern Error *jet_rt_handle_clone_xml_error(Error *);
extern void jet_rt_handle_drop_xml_error(Error *);
extern bool jet_rt_handle_eq_xml_error(Error *,Error *);
extern int64_t jet_rt_record_get_xml_error_kind(Error *);
extern int64_t jet_rt_record_get_xml_error_byte_offset(Error *,int64_t *);
extern int64_t jet_rt_record_get_xml_error_line(Error *,int64_t *);
extern int64_t jet_rt_record_get_xml_error_column(Error *,int64_t *);
extern Text *jet_rt_record_get_xml_error_path(Error *);
extern Text *jet_rt_record_get_xml_error_reason(Error *);
extern Text *jet_rt_handle_show_xml_error(Error *);
extern Text *jet_rt_handle_display_xml_error(Error *);
extern Text *jet_rt_handle_debug_xml_error(Error *);
extern Render *jet_rt_record_new_xml_render_options(int64_t,int64_t);
extern Render *jet_rt_handle_clone_xml_render_options(Render *);
extern void jet_rt_handle_drop_xml_render_options(Render *);
extern bool jet_rt_handle_eq_xml_render_options(Render *,Render *);
extern int64_t jet_rt_record_get_xml_render_options_encoding(Render *);
extern int64_t jet_rt_record_get_xml_render_options_lexical(Render *);
static struct { void *pointer; int64_t bytes, align; unsigned frees; } watched[64];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<64);
    for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes;
    watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_error(Error *e) { watch(e,72,8); watch_text(e->path); watch_text(e->reason); }
static void done(void) {
    assert(intercepted>0);
    for(size_t i=0;i<count;++i) assert(watched[i].frees==1);
    count=0;
}
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) {
    assert(t->length==(int64_t)strlen(s));
    assert(memcmp(t->data,s,(size_t)t->length)==0);
}
static Error *error(int64_t kind,const int64_t *offset,const int64_t *line,const int64_t *column,const char *path,const char *reason) {
    Text *p=text(path), *r=text(reason);
    Error *e=jet_rt_record_new_xml_error(kind,offset,line,column,p,r);
    assert(e->path!=p && e->reason!=r);
    jet_rt_string_drop(p); jet_rt_string_drop(r);
    return e;
}
static void rendered(Error *e,const char *golden) {
    Text *a=jet_rt_handle_show_xml_error(e), *b=jet_rt_handle_display_xml_error(e), *c=jet_rt_handle_debug_xml_error(e);
    assert(a!=b && a!=c && b!=c);
    text_is(a,golden); text_is(b,golden); text_is(c,golden);
    watch_text(a); watch_text(b); watch_text(c);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c); done();
}
static void enum_cases(int64_t variants,Enum *(*clone)(Enum *),void (*drop)(Enum *),bool (*eq)(Enum *,Enum *)) {
    for(int64_t i=0;i<variants;++i) {
        Enum *a=jet_rt_alloc(8,8); a->tag=i;
        Enum *b=clone(a);
        assert(a!=b && eq(a,b) && b->tag==i && eq(a,a));
        b->tag=(i+1)%variants; assert(!eq(a,b));
        watch(a,8,8); watch(b,8,8); drop(a); drop(b); done();
    }
    drop(NULL);
}
static void render_options(void) {
    for(int64_t i=0;i<4;++i) for(int64_t j=0;j<2;++j) {
        Render *a=jet_rt_record_new_xml_render_options(i,j), *b=jet_rt_handle_clone_xml_render_options(a);
        assert(a!=b && jet_rt_handle_eq_xml_render_options(a,b));
        assert(jet_rt_record_get_xml_render_options_encoding(a)==i && jet_rt_record_get_xml_render_options_lexical(a)==j);
        b->encoding^=1; assert(!jet_rt_handle_eq_xml_render_options(a,b)); b->encoding^=1;
        b->lexical^=1; assert(!jet_rt_handle_eq_xml_render_options(a,b)); b->lexical^=1;
        watch(a,16,8); watch(b,16,8);
        jet_rt_handle_drop_xml_render_options(a); jet_rt_handle_drop_xml_render_options(b); done();
    }
    jet_rt_handle_drop_xml_render_options(NULL);
}
static void error_cases(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX), lo=jet_int_owned_from_i64(INT64_MIN);
    Error *a=error(2,&hi,&lo,&hi,"/\xcf\x80","oops"), *b=jet_rt_handle_clone_xml_error(a);
    assert(a!=b && a->path!=b->path && a->reason!=b->reason);
    assert(a->path->data!=b->path->data && a->reason->data!=b->reason->data);
    assert(jet_rt_handle_eq_xml_error(a,a) && jet_rt_handle_eq_xml_error(a,b));
    assert(a->byte_offset==INT64_MAX && a->line==INT64_MIN && a->column==INT64_MAX);
    int64_t out1, out2, line, column;
    assert(jet_rt_record_get_xml_error_byte_offset(a,&out1)==1);
    assert(jet_rt_record_get_xml_error_byte_offset(a,&out2)==1);
    assert(out1!=hi && out2!=hi && out1!=out2);
    assert(jet_rt_record_get_xml_error_line(a,&line)==1 && line!=lo);
    assert(jet_rt_record_get_xml_error_column(a,&column)==1 && column!=hi);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    assert(jet_int_owned_to_i64(out1)==INT64_MAX && jet_int_owned_to_i64(out2)==INT64_MAX);
    assert(jet_int_owned_to_i64(line)==INT64_MIN && jet_int_owned_to_i64(column)==INT64_MAX);
    jet_rt_int_drop(out1); jet_rt_int_drop(out2); jet_rt_int_drop(line); jet_rt_int_drop(column);
    assert(jet_rt_record_get_xml_error_kind(a)==2);
    Text *p=jet_rt_record_get_xml_error_path(a), *r=jet_rt_record_get_xml_error_reason(a);
    assert(p!=a->path && p!=b->path && r!=a->reason && r!=b->reason);
    int64_t *fields[]={&b->kind,&b->byte_present,&b->byte_offset,&b->line_present,&b->line,&b->column_present,&b->column};
    for(size_t i=0;i<7;++i) { *fields[i]^=1; assert(!jet_rt_handle_eq_xml_error(a,b)); *fields[i]^=1; }
    b->path->data[0]^=1; assert(!jet_rt_handle_eq_xml_error(a,b)); b->path->data[0]^=1;
    b->reason->data[0]^=1; assert(!jet_rt_handle_eq_xml_error(a,b)); b->reason->data[0]^=1;
    rendered(a,"XML MismatchedTag at byte 9223372036854775807, line -9223372036854775808, column 9223372036854775807, path /\xcf\x80: oops");
    watch_error(a); watch_error(b); jet_rt_handle_drop_xml_error(a); jet_rt_handle_drop_xml_error(b); done();
    text_is(p,"/\xcf\x80"); text_is(r,"oops");
    watch_text(p); watch_text(r); jet_rt_string_drop(p); jet_rt_string_drop(r); done();
    int64_t zero=0;
    a=error(0,NULL,NULL,NULL,"","x"); b=error(0,&zero,NULL,NULL,"","x");
    assert(!jet_rt_handle_eq_xml_error(a,b));
    int64_t out=42;
    assert(jet_rt_record_get_xml_error_byte_offset(a,&out)==0 && out==42);
    assert(jet_rt_record_get_xml_error_line(a,&out)==0 && out==42);
    assert(jet_rt_record_get_xml_error_column(a,&out)==0 && out==42);
    jet_rt_handle_drop_xml_error(b); b=jet_rt_handle_clone_xml_error(a);
    b->byte_offset=99; b->line=99; b->column=99; assert(jet_rt_handle_eq_xml_error(a,b));
    watch_error(a); watch_error(b); jet_rt_handle_drop_xml_error(a); jet_rt_handle_drop_xml_error(b); done();
    const char *names[]={"InvalidEncoding","Malformed","MismatchedTag","InvalidName","Namespace","DuplicateAttribute","Entity","EntityCycle","Limit","Canonicalization","Shape","Unsupported"};
    for(int64_t i=0;i<12;++i) for(unsigned mask=0;mask<8;++mask) {
        int64_t offset=-7, l=0, c=-3;
        a=error(i,(mask&1)?&offset:NULL,(mask&2)?&l:NULL,(mask&4)?&c:NULL,(mask&1)?"/p":"","bad");
        char golden[256];
        int n=snprintf(golden,sizeof(golden),"XML %s at byte %d%s%s%s: bad",names[i],(mask&1)?-7:0,(mask&2)?", line 0":"",(mask&4)?", column -3":"",(mask&1)?", path /p":"");
        assert(n>=0 && (size_t)n<sizeof(golden)); rendered(a,golden);
        watch_error(a); jet_rt_handle_drop_xml_error(a); done();
    }
    jet_rt_handle_drop_xml_error(NULL);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1) {
        if(strcmp(argv[1],"encoding-range")==0) { (void)jet_rt_record_new_xml_render_options(4,0); }
        else if(strcmp(argv[1],"lexical-range")==0) { (void)jet_rt_record_new_xml_render_options(0,-1); }
        else if(strcmp(argv[1],"kind-range")==0) { (void)error(12,NULL,NULL,NULL,"","x"); }
        else if(strcmp(argv[1],"native-overflow")==0) {
            int64_t hi=jet_int_owned_from_i64(INT64_MAX), over=jet_int_add(hi,1);
            (void)error(0,&over,NULL,NULL,"","x");
        } else return 2;
        assert(!"range/overflow mode returned"); return 1;
    }
    enum_cases(12,jet_rt_handle_clone_xml_reason,jet_rt_handle_drop_xml_reason,jet_rt_handle_eq_xml_reason);
    enum_cases(4,jet_rt_handle_clone_xml_encoding,jet_rt_handle_drop_xml_encoding,jet_rt_handle_eq_xml_encoding);
    enum_cases(2,jet_rt_handle_clone_xml_lexical_policy,jet_rt_handle_drop_xml_lexical_policy,jet_rt_handle_eq_xml_lexical_policy);
    render_options(); error_cases(); puts("XMLGlue ownership/equality/render OK"); return 0;
}
