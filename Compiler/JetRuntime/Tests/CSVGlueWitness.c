// Real Jet-object client; link -Wl,--wrap=jet_rt_free. CSVGlueABI only.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; Text **data; } Fields;
typedef struct { Fields *fields; int64_t line; } Row;
_Static_assert(sizeof(Row)==16 && offsetof(Row,line)==8,"CSVRow16");
_Static_assert(sizeof(Fields)==24 && offsetof(Fields,data)==16,"List24");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_new(unsigned char *,int64_t);
extern Row *jet_rt_handle_clone_csv_row(Row *);
extern void jet_rt_handle_drop_csv_row(Row *);
extern bool jet_rt_handle_eq_csv_row(Row *,Row *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[48];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<48);
    for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align;
    watched[count++].frees=0;
}
static void watch_row(Row *row) {
    watch(row,16,8); watch(row->fields,24,8);
    if(row->fields->capacity) watch(row->fields->data,row->fields->capacity*8,8);
    for(int64_t i=0;i<row->fields->length;++i) {
        Text *t=row->fields->data[i]; watch((char *)(t)-8,32,8);
        if(t->capacity) watch(t->data,t->capacity,1);
    }
}
static void done(void) { assert(intercepted); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static Row *row(int64_t length,int64_t spare,int64_t line) {
    static const char *values[]={"","duplicate","duplicate","\xcf\x80"};
    static const unsigned char nul[]={'a',0,'b'};
    Row *r=jet_rt_alloc(16,8); r->line=line; r->fields=jet_rt_alloc(24,8);
    r->fields->length=length; r->fields->capacity=length+spare;
    r->fields->data=r->fields->capacity ? jet_rt_alloc(r->fields->capacity*8,8) : (void *)(uintptr_t)8;
    for(int64_t i=0;i<length;++i) r->fields->data[i]=i==4 ? jet_rt_string_new((unsigned char *)nul,3) : jet_rt_string_new((unsigned char *)values[i],(int64_t)strlen(values[i]));
    return r;
}
static void check(int64_t length,int64_t spare,int64_t line) {
    Row *a=row(length,spare,line),*b=jet_rt_handle_clone_csv_row(a);
    assert(a!=b && a->fields!=b->fields && b->line==line);
    assert(b->fields->length==length && b->fields->capacity==length);
    assert(jet_rt_handle_eq_csv_row(a,a) && jet_rt_handle_eq_csv_row(a,b));
    if(length) assert(a->fields->data!=b->fields->data);
    for(int64_t i=0;i<length;++i) {
        Text *first=a->fields->data[i],*second=b->fields->data[i];
        assert(first!=second && first->length==second->length);
        if(first->length) {
            assert(first->data!=second->data && memcmp(first->data,second->data,(size_t)first->length)==0);
            second->data[0]^=1; assert(!jet_rt_handle_eq_csv_row(a,b)); second->data[0]^=1;
        }
    }
    b->line^=1; assert(!jet_rt_handle_eq_csv_row(a,b)); b->line^=1;
    if(length) { --b->fields->length; assert(!jet_rt_handle_eq_csv_row(a,b)); ++b->fields->length; }
    if(length>3) {
        Text *saved=b->fields->data[1]; b->fields->data[1]=b->fields->data[3]; b->fields->data[3]=saved;
        assert(!jet_rt_handle_eq_csv_row(a,b)); saved=b->fields->data[1]; b->fields->data[1]=b->fields->data[3]; b->fields->data[3]=saved;
    }
    watch_row(a); jet_rt_handle_drop_csv_row(a); done();
    assert(jet_rt_handle_eq_csv_row(b,b) && b->line==line);
    if(length==5) assert(b->fields->data[4]->length==3 && b->fields->data[4]->data[1]==0);
    watch_row(b); jet_rt_handle_drop_csv_row(b); done();
}
int main(void) {
    jet_rt_alloc_init(); check(0,0,INT64_MIN); check(0,4,INT64_MAX); check(1,0,-1); check(5,3,INT64_MAX);
    jet_rt_handle_drop_csv_row(NULL); puts("CSVGlue recursive ownership/equality OK"); return 0;
}
