// Real native providers only; no Rust at test time.
// Private carriers are fixed by DataABI.jet, not Rust or checked-IR layouts.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "DataPublicGoldens.h"
typedef struct { unsigned char *data; int64_t len, cap, cache; } Text;
typedef struct { int64_t len, cap; Text **data; } TextList;
typedef struct { Text *scope, *revision; } Authority;
typedef struct { int64_t kind; Text *locator, *member; TextList *parameters; } Source;
typedef struct { Source *source; int64_t format; Authority *authority; } Provenance;
typedef struct { Text *id, *source, *content, *schema; int64_t format; } Snapshot;
_Static_assert(sizeof(Authority)==16 && sizeof(Source)==32 && sizeof(Provenance)==24 && sizeof(Snapshot)==40 && sizeof(TextList)==24,"DataABI private carriers");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern bool jet_rt_string_eq(Text *,Text *);
#define FAMILY(T,name) extern T *jet_rt_handle_clone_##name(T *); extern void jet_rt_handle_drop_##name(T *); extern bool jet_rt_handle_eq_##name(T *,T *)
typedef int64_t DataFormat;
typedef int64_t DataLoaderKind;
typedef Authority DataAuthority;
typedef Source DataSourceIdentity;
typedef Provenance DataProvenance;
typedef Snapshot DataSnapshotIdentity;
FAMILY(DataFormat,data_format); FAMILY(DataLoaderKind,data_loader_kind); FAMILY(DataAuthority,data_authority); FAMILY(DataSourceIdentity,data_source_identity); FAMILY(DataProvenance,data_provenance); FAMILY(DataSnapshotIdentity,data_snapshot_identity);
extern Text *jet_rt_handle_show_data_format(DataFormat *);
extern Text *jet_rt_handle_show_data_loader_kind(DataLoaderKind *);
extern Text *jet_rt_handle_show_data_authority(Authority *);
extern Text *jet_rt_handle_show_data_source_identity(Source *);
extern Text *jet_rt_handle_show_data_snapshot_identity(Snapshot *);
extern Authority *jet_rt_record_new_data_authority(Text *,Text *);
extern Text *jet_rt_record_get_data_authority_scope(Authority *);
extern Text *jet_rt_record_get_data_authority_revision(Authority *);
extern Source *jet_rt_handle_field_data_provenance_source(Provenance *);
extern Authority *jet_rt_handle_field_data_provenance_authority(Provenance *);
extern void jet_rt_handle_field_data_source_identity_locator(Source *,Text *);
extern void jet_rt_handle_field_data_source_identity_member(Source *,Text *);
extern Snapshot *jet_rt_record_new_data_snapshot_identity(Text *,Text *,Text *,Text *,int64_t);
extern Text *jet_rt_record_get_data_snapshot_identity_id(Snapshot *);
extern Text *jet_rt_record_get_data_snapshot_identity_source(Snapshot *);
extern Text *jet_rt_record_get_data_snapshot_identity_content(Snapshot *);
extern Text *jet_rt_record_get_data_snapshot_identity_schema(Snapshot *);
extern int64_t jet_rt_record_get_data_snapshot_identity_format(Snapshot *);
static struct {void *ptr; int64_t size, align; unsigned frees;} watched[128];
static size_t count;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=0;i<count;i++) if(watched[i].ptr==ptr){assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1);}
}
// Texts are shared by refcount, so a carrier and its clone can name the same
// text block; each block is watched once and must be freed exactly once.
static void watch(void *p,int64_t n,int64_t a){for(size_t i=0;i<count;i++)if(watched[i].ptr==p){assert(watched[i].size==n && watched[i].align==a);return;}assert(count<128);watched[count].ptr=p;watched[count].size=n;watched[count].align=a;watched[count++].frees=0;}
static void watch_text(Text *t){watch((char *)(t)-8,40,8);if(t->cap)watch(t->data,t->cap,1);}
static void watch_authority(Authority *a){watch(a,16,8);watch_text(a->scope);watch_text(a->revision);}
static void watch_source_shell(Source *s){watch(s,32,8);watch(s->parameters,24,8);if(s->parameters->cap)watch(s->parameters->data,s->parameters->cap*8,8);}
static void watch_source(Source *s){watch_source_shell(s);watch_text(s->locator);watch_text(s->member);for(int64_t i=0;i<s->parameters->len;i++)watch_text(s->parameters->data[i]);}
static void done(void){for(size_t i=0;i<count;i++)assert(watched[i].frees==1);count=0;}
static int64_t refs(Text *t){return *(int64_t *)((char *)t-8) & 0x3fffffffffffffff;}
// Checks a retained text's bytes and releases that one reference.
static void same(Text *t,const void *s,size_t n,int64_t after){assert(t->len==(int64_t)n && !memcmp(t->data,s,n));jet_rt_string_drop(t);assert(refs(t)==after);}
static Text *bytes(const void *s,size_t n){return jet_rt_string_from_static(s,(int64_t)n);}
// An owned copy whose first byte differs, for inequality probes on shared texts.
static Text *flipped(Text *t){unsigned char b[16];assert(t->len>0 && t->len<=16);memcpy(b,t->data,(size_t)t->len);b[0]^=0x20;Text *c=bytes(b,(size_t)t->len);assert(c!=t && c->len==t->len && !memcmp(c->data,b,(size_t)t->len) && refs(c)==1);return c;}
static Text *text(const char *s){return bytes(s,strlen(s));}
static void expect(Text *t,const void *s,size_t n){assert(t->len==(int64_t)n);assert(!memcmp(t->data,s,n));watch_text(t);jet_rt_string_drop(t);done();}
static void expected(Text *t,const char *s){expect(t,s,strlen(s));}
static Source *source(Text *locator,Text *member,Text *parameter){Source *s=jet_rt_alloc(32,8);s->kind=3;s->locator=locator;s->member=member;s->parameters=jet_rt_alloc(24,8);s->parameters->len=s->parameters->cap=parameter?1:0;s->parameters->data=parameter?jet_rt_alloc(8,8):(Text **)(uintptr_t)8;if(parameter)s->parameters->data[0]=parameter;return s;}
static void policy_goldens(void){
    for(size_t i=0;i<sizeof(parameter_goldens)/sizeof(*parameter_goldens);i++){
        const PublicGolden *g=&parameter_goldens[i];Source *s=source(text(""),text(""),bytes(g->input,g->input_len));Text *out=jet_rt_handle_show_data_source_identity(s);assert(out->len==(int64_t)g->expected_len+7);assert(!memcmp(out->data,"Value:?",7));assert(!memcmp(out->data+7,g->expected,g->expected_len));jet_rt_string_drop(out);jet_rt_handle_drop_data_source_identity(s);
    }
    for(size_t i=0;i<sizeof(locator_goldens)/sizeof(*locator_goldens);i++){
        const PublicGolden *g=&locator_goldens[i];Source *s=source(bytes(g->input,g->input_len),text(""),NULL);Text *out=jet_rt_handle_show_data_source_identity(s);assert(out->len==(int64_t)g->expected_len+6);assert(!memcmp(out->data,"Value:",6));assert(!memcmp(out->data+6,g->expected,g->expected_len));jet_rt_string_drop(out);jet_rt_handle_drop_data_source_identity(s);
    }
    for(size_t i=0;i<sizeof(member_goldens)/sizeof(*member_goldens);i++){
        const PublicGolden *g=&member_goldens[i];Source *s=source(text(""),bytes(g->input,g->input_len),NULL);Text *out=jet_rt_handle_show_data_source_identity(s);size_t prefix=g->input_len?7:6;assert(out->len==(int64_t)(g->expected_len+prefix));assert(!memcmp(out->data,"Value:#",prefix));assert(!memcmp(out->data+prefix,g->expected,g->expected_len));jet_rt_string_drop(out);jet_rt_handle_drop_data_source_identity(s);
    }
    for(size_t i=0;i<sizeof(authority_goldens)/sizeof(*authority_goldens);i++){
        const PublicGolden *g=&authority_goldens[i];Text *input=bytes(g->input,g->input_len),*empty=text("");Authority *a=jet_rt_record_new_data_authority(input,empty);expect(jet_rt_handle_show_data_authority(a),g->expected,g->expected_len);assert(jet_rt_string_eq(a->scope,input));jet_rt_handle_drop_data_authority(a);
        a=jet_rt_record_new_data_authority(empty,input);Text *out=jet_rt_handle_show_data_authority(a);size_t prefix=g->input_len?1:0;assert(out->len==(int64_t)(g->expected_len+prefix));if(prefix)assert(out->data[0]=='@');assert(!memcmp(out->data+prefix,g->expected,g->expected_len));jet_rt_string_drop(out);jet_rt_handle_drop_data_authority(a);jet_rt_string_drop(input);jet_rt_string_drop(empty);
    }
}
static void enums(void){
    const char *formats[]={"csv","json","jsonl","parquet","arrow"},*kinds[]={"File","URL","Database","Value"};
    for(int64_t i=0;i<5;i++){DataFormat *a=jet_rt_alloc(8,8);*a=i;DataFormat *b=jet_rt_handle_clone_data_format(a);assert(a!=b && *a==*b && jet_rt_handle_eq_data_format(a,b));expected(jet_rt_handle_show_data_format(a),formats[i]);*b=(i+1)%5;assert(!jet_rt_handle_eq_data_format(a,b));watch(a,8,8);watch(b,8,8);jet_rt_handle_drop_data_format(a);jet_rt_handle_drop_data_format(b);done();}
    for(int64_t i=0;i<4;i++){DataLoaderKind *a=jet_rt_alloc(8,8);*a=i;DataLoaderKind *b=jet_rt_handle_clone_data_loader_kind(a);assert(a!=b && jet_rt_handle_eq_data_loader_kind(a,b));expected(jet_rt_handle_show_data_loader_kind(a),kinds[i]);*b=(i+1)%4;assert(!jet_rt_handle_eq_data_loader_kind(a,b));watch(a,8,8);watch(b,8,8);jet_rt_handle_drop_data_loader_kind(a);jet_rt_handle_drop_data_loader_kind(b);done();}
    jet_rt_handle_drop_data_format(NULL);jet_rt_handle_drop_data_loader_kind(NULL);
}
static void owners(void){
    Text *scope=text("scope"),*revision=text("rev");Authority *a=jet_rt_record_new_data_authority(scope,revision),*b=jet_rt_handle_clone_data_authority(a);assert(a!=b && a->scope==scope && b->scope==scope && a->revision==revision && b->revision==revision && refs(scope)==3 && refs(revision)==3 && jet_rt_handle_eq_data_authority(a,b));expected(jet_rt_handle_show_data_authority(a),"scope@rev");Text *got=jet_rt_record_get_data_authority_scope(a);assert(got==scope && refs(scope)==4);same(got,"scope",5,3);got=jet_rt_record_get_data_authority_revision(a);assert(got==revision && refs(revision)==4);same(got,"rev",3,3);
    Text *shared=b->scope;b->scope=flipped(shared);assert(!jet_rt_handle_eq_data_authority(a,b));jet_rt_string_drop(b->scope);b->scope=shared;shared=b->revision;b->revision=flipped(shared);assert(!jet_rt_handle_eq_data_authority(a,b));jet_rt_string_drop(b->revision);b->revision=shared;
    watch(a,16,8);watch(b,16,8);jet_rt_handle_drop_data_authority(a);jet_rt_handle_drop_data_authority(b);done();assert(refs(scope)==1 && refs(revision)==1);expected(scope,"scope");expected(revision,"rev");
    Source *s=source(text("loc"),text("mem"),text("key=value")),*copy=jet_rt_handle_clone_data_source_identity(s);assert(s!=copy && s->locator==copy->locator && s->member==copy->member && s->parameters!=copy->parameters && s->parameters->data!=copy->parameters->data && s->parameters->data[0]==copy->parameters->data[0] && refs(s->locator)==2 && refs(s->member)==2 && refs(s->parameters->data[0])==2);assert(jet_rt_handle_eq_data_source_identity(s,copy));
    Text borrowed;jet_rt_handle_field_data_source_identity_locator(s,&borrowed);assert(borrowed.data==s->locator->data && borrowed.len==3 && borrowed.cap==s->locator->cap);jet_rt_handle_field_data_source_identity_member(s,&borrowed);assert(borrowed.data==s->member->data && borrowed.len==3);
    copy->kind=0;assert(!jet_rt_handle_eq_data_source_identity(s,copy));copy->kind=3;
    shared=copy->locator;copy->locator=flipped(shared);assert(!jet_rt_handle_eq_data_source_identity(s,copy));jet_rt_string_drop(copy->locator);copy->locator=shared;
    shared=copy->member;copy->member=flipped(shared);assert(!jet_rt_handle_eq_data_source_identity(s,copy));jet_rt_string_drop(copy->member);copy->member=shared;
    shared=copy->parameters->data[0];copy->parameters->data[0]=flipped(shared);assert(!jet_rt_handle_eq_data_source_identity(s,copy));jet_rt_string_drop(copy->parameters->data[0]);copy->parameters->data[0]=shared;
    watch_source_shell(copy);jet_rt_handle_drop_data_source_identity(copy);done();assert(refs(s->locator)==1 && refs(s->member)==1 && refs(s->parameters->data[0])==1);
    Source *ordered=source(text(""),text(""),text("x=1"));
    Text **items=jet_rt_alloc(24,8);items[0]=ordered->parameters->data[0];items[1]=text("x=2");items[2]=text("x=1");
    jet_rt_free(ordered->parameters->data,8,8);ordered->parameters->data=items;ordered->parameters->len=ordered->parameters->cap=3;
    Source *ordered_copy=jet_rt_handle_clone_data_source_identity(ordered);
    assert(jet_rt_handle_eq_data_source_identity(ordered,ordered_copy));expected(jet_rt_handle_show_data_source_identity(ordered),"Value:?x=1&x=2&x=1");
    Text *swap=ordered_copy->parameters->data[0];ordered_copy->parameters->data[0]=ordered_copy->parameters->data[1];ordered_copy->parameters->data[1]=swap;
    assert(!jet_rt_handle_eq_data_source_identity(ordered,ordered_copy));ordered_copy->parameters->len=2;assert(!jet_rt_handle_eq_data_source_identity(ordered,ordered_copy));ordered_copy->parameters->len=3;
    watch_source(ordered);watch_source(ordered_copy);jet_rt_handle_drop_data_source_identity(ordered);jet_rt_handle_drop_data_source_identity(ordered_copy);done();
    Source *empty_source=source(text(""),text(""),NULL),*empty_copy=jet_rt_handle_clone_data_source_identity(empty_source);
    assert(jet_rt_handle_eq_data_source_identity(empty_source,empty_copy));watch_source(empty_source);watch_source(empty_copy);jet_rt_handle_drop_data_source_identity(empty_source);jet_rt_handle_drop_data_source_identity(empty_copy);done();
    Provenance *p=jet_rt_alloc(24,8);p->source=s;p->format=1;scope=text("scope");revision=text("");p->authority=jet_rt_record_new_data_authority(scope,revision);jet_rt_string_drop(scope);jet_rt_string_drop(revision);Provenance *q=jet_rt_handle_clone_data_provenance(p);assert(p!=q && p->source!=q->source && p->authority!=q->authority && p->authority->scope==q->authority->scope && refs(p->authority->scope)==2 && jet_rt_handle_eq_data_provenance(p,q));assert(jet_rt_handle_field_data_provenance_source(p)==p->source && jet_rt_handle_field_data_provenance_authority(p)==p->authority);q->source->kind=1;assert(!jet_rt_handle_eq_data_provenance(p,q));q->source->kind=3;q->format=2;assert(!jet_rt_handle_eq_data_provenance(p,q));q->format=1;shared=q->authority->scope;q->authority->scope=flipped(shared);assert(!jet_rt_handle_eq_data_provenance(p,q));jet_rt_string_drop(q->authority->scope);q->authority->scope=shared;watch(p,24,8);watch_source(p->source);watch_authority(p->authority);watch(q,24,8);watch_source(q->source);watch_authority(q->authority);jet_rt_handle_drop_data_provenance(p);jet_rt_handle_drop_data_provenance(q);done();
    Text *inputs[]={text("id"),text("source"),text("content"),text("schema")};Snapshot *n=jet_rt_record_new_data_snapshot_identity(inputs[0],inputs[1],inputs[2],inputs[3],4),*m=jet_rt_handle_clone_data_snapshot_identity(n);assert(n!=m && n->id==inputs[0] && m->id==inputs[0] && n->source==inputs[1] && m->source==inputs[1] && n->content==inputs[2] && m->content==inputs[2] && n->schema==inputs[3] && m->schema==inputs[3] && jet_rt_handle_eq_data_snapshot_identity(n,m));for(size_t i=0;i<4;i++)assert(refs(inputs[i])==3);Text *(*getters[])(Snapshot *)={jet_rt_record_get_data_snapshot_identity_id,jet_rt_record_get_data_snapshot_identity_source,jet_rt_record_get_data_snapshot_identity_content,jet_rt_record_get_data_snapshot_identity_schema};Text **fields[]={&m->id,&m->source,&m->content,&m->schema};const char *names[]={"id","source","content","schema"};for(size_t i=0;i<4;i++){got=getters[i](n);assert(got==inputs[i] && refs(got)==4);same(got,names[i],strlen(names[i]),3);shared=*fields[i];*fields[i]=flipped(shared);assert(!jet_rt_handle_eq_data_snapshot_identity(n,m));jet_rt_string_drop(*fields[i]);*fields[i]=shared;}
    assert(jet_rt_record_get_data_snapshot_identity_format(n)==4);expected(jet_rt_handle_show_data_snapshot_identity(n),"SnapshotIdentity { id: id, source: source, content: content, schema: schema, format: arrow }");m->format=0;assert(!jet_rt_handle_eq_data_snapshot_identity(n,m));watch(n,40,8);watch(m,40,8);jet_rt_handle_drop_data_snapshot_identity(n);jet_rt_handle_drop_data_snapshot_identity(m);done();for(size_t i=0;i<4;i++){assert(refs(inputs[i])==1);expected(inputs[i],names[i]);}
    static const unsigned char raw_id[]={'i',0,'d','\n'};
    unsigned char long_raw[4200];memset(long_raw,'x',sizeof(long_raw));memcpy(long_raw,"token=raw",9);
    Text *raw=bytes(raw_id,sizeof(raw_id)),*long_text=bytes(long_raw,sizeof(long_raw));
    Snapshot *raw_snapshot=jet_rt_record_new_data_snapshot_identity(raw,long_text,long_text,long_text,0);
    Text *raw_got=jet_rt_record_get_data_snapshot_identity_id(raw_snapshot);assert(raw_got==raw && refs(raw)==3);same(raw_got,raw_id,sizeof(raw_id),2);
    raw_got=jet_rt_record_get_data_snapshot_identity_source(raw_snapshot);assert(raw_got==long_text && refs(long_text)==5);same(raw_got,long_raw,sizeof(long_raw),4);
    Text *raw_show=jet_rt_handle_show_data_snapshot_identity(raw_snapshot);
    assert(raw_show->len==(int64_t)(sizeof("SnapshotIdentity { id: , source: , content: , schema: , format: csv }")-1+sizeof(raw_id)+3*sizeof(long_raw)) && !memcmp(raw_show->data,"SnapshotIdentity { id: ",23) && !memcmp(raw_show->data+23,raw_id,sizeof(raw_id)));
    assert(!memcmp(raw_show->data+37,long_raw,sizeof(long_raw)));jet_rt_string_drop(raw_show);
    watch(raw_snapshot,40,8);jet_rt_handle_drop_data_snapshot_identity(raw_snapshot);done();assert(refs(raw)==1 && refs(long_text)==1);watch_text(raw);watch_text(long_text);jet_rt_string_drop(raw);jet_rt_string_drop(long_text);done();
    jet_rt_handle_drop_data_authority(NULL);jet_rt_handle_drop_data_source_identity(NULL);jet_rt_handle_drop_data_provenance(NULL);jet_rt_handle_drop_data_snapshot_identity(NULL);
}
int main(int argc,char **argv){assert(jet_rt_alloc_check_observe(0,(int64_t)(uintptr_t)observe_free)==1);jet_rt_alloc_init();if(argc==2){Text *empty=text("");int64_t ordinal=!strcmp(argv[1],"negative")?-1:5;(void)jet_rt_record_new_data_snapshot_identity(empty,empty,empty,empty,ordinal);assert(!"invalid format returned");}enums();owners();policy_goldens();puts("DataMetadata36 ownership/equality/render OK");}
