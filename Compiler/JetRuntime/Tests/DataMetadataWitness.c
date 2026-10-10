// Real native providers only. Link with --wrap=jet_rt_free; no Rust at test time.
// Private carriers are fixed by DataABI.jet, not Rust or checked-MIR layouts.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "DataPublicGoldens.h"
typedef struct { unsigned char *data; int64_t len, cap; } Text;
typedef struct { int64_t len, cap; Text **data; } TextList;
typedef struct { Text *scope, *revision; } Authority;
typedef struct { int64_t kind; Text *locator, *member; TextList *parameters; } Source;
typedef struct { Source *source; int64_t format; Authority *authority; } Provenance;
typedef struct { Text *id, *source, *content, *schema; int64_t format; } Snapshot;
_Static_assert(sizeof(Authority)==16 && sizeof(Source)==32 && sizeof(Provenance)==24 && sizeof(Snapshot)==40 && sizeof(TextList)==24,"DataABI private carriers");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern bool jet_rt_string_eq(Text *,Text *);
#define FAMILY(T) extern T *jet_rt_handle_clone_##T(T *); extern void jet_rt_handle_drop_##T(T *); extern bool jet_rt_handle_eq_##T(T *,T *)
typedef int64_t DataFormat;
typedef int64_t DataLoaderKind;
typedef Authority DataAuthority;
typedef Source DataSourceIdentity;
typedef Provenance DataProvenance;
typedef Snapshot DataSnapshotIdentity;
FAMILY(DataFormat); FAMILY(DataLoaderKind); FAMILY(DataAuthority); FAMILY(DataSourceIdentity); FAMILY(DataProvenance); FAMILY(DataSnapshotIdentity);
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
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=0;i<count;i++) if(watched[i].ptr==ptr){assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1);}
    __real_jet_rt_free(ptr,size,align);
}
static void watch(void *p,int64_t n,int64_t a){assert(count<128);watched[count].ptr=p;watched[count].size=n;watched[count].align=a;watched[count++].frees=0;}
static void watch_text(Text *t){watch(t,24,8);if(t->cap)watch(t->data,t->cap,1);}
static void watch_authority(Authority *a){watch(a,16,8);watch_text(a->scope);watch_text(a->revision);}
static void watch_source(Source *s){watch(s,32,8);watch_text(s->locator);watch_text(s->member);watch(s->parameters,24,8);if(s->parameters->cap)watch(s->parameters->data,s->parameters->cap*8,8);for(int64_t i=0;i<s->parameters->len;i++)watch_text(s->parameters->data[i]);}
static void watch_snapshot(Snapshot *s){watch(s,40,8);watch_text(s->id);watch_text(s->source);watch_text(s->content);watch_text(s->schema);}
static void done(void){for(size_t i=0;i<count;i++)assert(watched[i].frees==1);count=0;}
static Text *bytes(const void *s,size_t n){return jet_rt_string_from_static(s,(int64_t)n);}
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
    Text *scope=text("scope"),*revision=text("rev");Authority *a=jet_rt_record_new_data_authority(scope,revision),*b=jet_rt_handle_clone_data_authority(a);assert(a!=b && a->scope!=b->scope && a->scope!=scope && a->revision!=b->revision && jet_rt_handle_eq_data_authority(a,b));expected(jet_rt_handle_show_data_authority(a),"scope@rev");Text *got=jet_rt_record_get_data_authority_scope(a);assert(got!=a->scope);expected(got,"scope");got=jet_rt_record_get_data_authority_revision(a);assert(got!=a->revision);expected(got,"rev");b->scope->data[0]='S';assert(!jet_rt_handle_eq_data_authority(a,b));b->scope->data[0]='s';b->revision->data[0]='R';assert(!jet_rt_handle_eq_data_authority(a,b));watch_authority(a);watch_authority(b);jet_rt_handle_drop_data_authority(a);jet_rt_handle_drop_data_authority(b);done();expected(scope,"scope");expected(revision,"rev");
    Source *s=source(text("loc"),text("mem"),text("key=value")),*copy=jet_rt_handle_clone_data_source_identity(s);assert(s!=copy && s->locator!=copy->locator && s->member!=copy->member && s->parameters!=copy->parameters && s->parameters->data!=copy->parameters->data && s->parameters->data[0]!=copy->parameters->data[0]);assert(jet_rt_handle_eq_data_source_identity(s,copy));
    Text borrowed;jet_rt_handle_field_data_source_identity_locator(s,&borrowed);assert(borrowed.data==s->locator->data && borrowed.len==3 && borrowed.cap==s->locator->cap);jet_rt_handle_field_data_source_identity_member(s,&borrowed);assert(borrowed.data==s->member->data && borrowed.len==3);
    copy->kind=0;assert(!jet_rt_handle_eq_data_source_identity(s,copy));copy->kind=3;copy->locator->data[0]='L';assert(!jet_rt_handle_eq_data_source_identity(s,copy));copy->locator->data[0]='l';copy->member->data[0]='M';assert(!jet_rt_handle_eq_data_source_identity(s,copy));copy->member->data[0]='m';copy->parameters->data[0]->data[0]='K';assert(!jet_rt_handle_eq_data_source_identity(s,copy));watch_source(copy);jet_rt_handle_drop_data_source_identity(copy);done();
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
    Provenance *p=jet_rt_alloc(24,8);p->source=s;p->format=1;scope=text("scope");revision=text("");p->authority=jet_rt_record_new_data_authority(scope,revision);jet_rt_string_drop(scope);jet_rt_string_drop(revision);Provenance *q=jet_rt_handle_clone_data_provenance(p);assert(p!=q && p->source!=q->source && p->authority!=q->authority && jet_rt_handle_eq_data_provenance(p,q));assert(jet_rt_handle_field_data_provenance_source(p)==p->source && jet_rt_handle_field_data_provenance_authority(p)==p->authority);q->source->kind=1;assert(!jet_rt_handle_eq_data_provenance(p,q));q->source->kind=3;q->format=2;assert(!jet_rt_handle_eq_data_provenance(p,q));q->format=1;q->authority->scope->data[0]='S';assert(!jet_rt_handle_eq_data_provenance(p,q));watch(p,24,8);watch_source(p->source);watch_authority(p->authority);watch(q,24,8);watch_source(q->source);watch_authority(q->authority);jet_rt_handle_drop_data_provenance(p);jet_rt_handle_drop_data_provenance(q);done();
    Text *inputs[]={text("id"),text("source"),text("content"),text("schema")};Snapshot *n=jet_rt_record_new_data_snapshot_identity(inputs[0],inputs[1],inputs[2],inputs[3],4),*m=jet_rt_handle_clone_data_snapshot_identity(n);assert(n!=m && n->id!=m->id && n->source!=m->source && n->content!=m->content && n->schema!=m->schema && jet_rt_handle_eq_data_snapshot_identity(n,m));Text *(*getters[])(Snapshot *)={jet_rt_record_get_data_snapshot_identity_id,jet_rt_record_get_data_snapshot_identity_source,jet_rt_record_get_data_snapshot_identity_content,jet_rt_record_get_data_snapshot_identity_schema};Text **fields[]={&m->id,&m->source,&m->content,&m->schema};const char *names[]={"id","source","content","schema"};for(size_t i=0;i<4;i++){got=getters[i](n);assert(got!=inputs[i]);expected(got,names[i]);unsigned char saved=(*fields[i])->data[0];(*fields[i])->data[0]='X';assert(!jet_rt_handle_eq_data_snapshot_identity(n,m));(*fields[i])->data[0]=saved;}
    assert(jet_rt_record_get_data_snapshot_identity_format(n)==4);expected(jet_rt_handle_show_data_snapshot_identity(n),"SnapshotIdentity { id: id, source: source, content: content, schema: schema, format: arrow }");m->format=0;assert(!jet_rt_handle_eq_data_snapshot_identity(n,m));watch_snapshot(n);watch_snapshot(m);jet_rt_handle_drop_data_snapshot_identity(n);jet_rt_handle_drop_data_snapshot_identity(m);done();for(size_t i=0;i<4;i++)expected(inputs[i],names[i]);
    static const unsigned char raw_id[]={'i',0,'d','\n'};
    unsigned char long_raw[4200];memset(long_raw,'x',sizeof(long_raw));memcpy(long_raw,"token=raw",9);
    Text *raw=bytes(raw_id,sizeof(raw_id)),*long_text=bytes(long_raw,sizeof(long_raw));
    Snapshot *raw_snapshot=jet_rt_record_new_data_snapshot_identity(raw,long_text,long_text,long_text,0);
    expect(jet_rt_record_get_data_snapshot_identity_id(raw_snapshot),raw_id,sizeof(raw_id));
    expect(jet_rt_record_get_data_snapshot_identity_source(raw_snapshot),long_raw,sizeof(long_raw));
    Text *raw_show=jet_rt_handle_show_data_snapshot_identity(raw_snapshot);
    assert(raw_show->len==(int64_t)(sizeof("SnapshotIdentity { id: , source: , content: , schema: , format: csv }")-1+sizeof(raw_id)+3*sizeof(long_raw)) && !memcmp(raw_show->data,"SnapshotIdentity { id: ",23) && !memcmp(raw_show->data+23,raw_id,sizeof(raw_id)));
    assert(!memcmp(raw_show->data+37,long_raw,sizeof(long_raw)));jet_rt_string_drop(raw_show);
    jet_rt_handle_drop_data_snapshot_identity(raw_snapshot);jet_rt_string_drop(raw);jet_rt_string_drop(long_text);
    jet_rt_handle_drop_data_authority(NULL);jet_rt_handle_drop_data_source_identity(NULL);jet_rt_handle_drop_data_provenance(NULL);jet_rt_handle_drop_data_snapshot_identity(NULL);
}
int main(int argc,char **argv){jet_rt_alloc_init();if(argc==2){Text *empty=text("");int64_t ordinal=!strcmp(argv[1],"negative")?-1:5;(void)jet_rt_record_new_data_snapshot_identity(empty,empty,empty,empty,ordinal);assert(!"invalid format returned");}enums();owners();policy_goldens();puts("DataMetadata36 ownership/equality/render OK");}
