// Real Jet-object client. CSVGlueABI only.
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
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Row *jet_rt_handle_clone_csv_row(Row *);
extern void jet_rt_handle_drop_csv_row(Row *);
extern bool jet_rt_handle_eq_csv_row(Row *,Row *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[48];
static size_t count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<48);
    for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align;
    watched[count++].frees=0;
}
// Texts are shared by refcount: a row clone owns its own row, list and slots,
// and retains each field text instead of copying it.
static int64_t owners(Text *t) { return *(int64_t *)((char *)t-8) & 0x3fffffffffffffff; }
static void watch_row(Row *row,bool texts) {
    watch(row,16,8); watch(row->fields,24,8);
    if(row->fields->capacity) watch(row->fields->data,row->fields->capacity*8,8);
    for(int64_t i=0;texts && i<row->fields->length;++i) {
        Text *t=row->fields->data[i]; watch((char *)(t)-8,40,8);
        if(t->capacity) watch(t->data,t->capacity,1);
    }
}
static void done(void) { assert(intercepted); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
// An owned copy whose first byte differs, for inequality probes on shared texts.
static Text *flipped(Text *t) {
    unsigned char bytes[16]; assert(t->length>0 && t->length<=16);
    memcpy(bytes,t->data,(size_t)t->length); bytes[0]^=1;
    Text *copy=jet_rt_string_from_static(bytes,t->length);
    assert(copy->length==t->length && memcmp(copy->data,bytes,(size_t)t->length)==0 && owners(copy)==1);
    return copy;
}
static Row *row(int64_t length,int64_t spare,int64_t line) {
    static const char *values[]={"","duplicate","duplicate","\xcf\x80"};
    static const unsigned char nul[]={'a',0,'b'};
    Row *r=jet_rt_alloc(16,8); r->line=line; r->fields=jet_rt_alloc(24,8);
    r->fields->length=length; r->fields->capacity=length+spare;
    r->fields->data=r->fields->capacity ? jet_rt_alloc(r->fields->capacity*8,8) : (void *)(uintptr_t)8;
    for(int64_t i=0;i<length;++i) r->fields->data[i]=i==4 ? jet_rt_string_from_static(nul,3) : jet_rt_string_from_static(values[i],(int64_t)strlen(values[i]));
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
        assert(first==second && owners(first)==2);
        if(first->length) {
            Text *probe=flipped(first); b->fields->data[i]=probe;
            assert(!jet_rt_handle_eq_csv_row(a,b));
            b->fields->data[i]=second; jet_rt_string_drop(probe);
        }
    }
    b->line^=1; assert(!jet_rt_handle_eq_csv_row(a,b)); b->line^=1;
    if(length) { --b->fields->length; assert(!jet_rt_handle_eq_csv_row(a,b)); ++b->fields->length; }
    if(length>3) {
        Text *saved=b->fields->data[1]; b->fields->data[1]=b->fields->data[3]; b->fields->data[3]=saved;
        assert(!jet_rt_handle_eq_csv_row(a,b)); saved=b->fields->data[1]; b->fields->data[1]=b->fields->data[3]; b->fields->data[3]=saved;
    }
    watch_row(a,false); jet_rt_handle_drop_csv_row(a); done();
    for(int64_t i=0;i<length;++i) assert(owners(b->fields->data[i])==1);
    assert(jet_rt_handle_eq_csv_row(b,b) && b->line==line);
    if(length==5) assert(b->fields->data[4]->length==3 && b->fields->data[4]->data[1]==0);
    watch_row(b,true); jet_rt_handle_drop_csv_row(b); done();
}
int main(void) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init(); check(0,0,INT64_MIN); check(0,4,INT64_MAX); check(1,0,-1); check(5,3,INT64_MAX);
    jet_rt_handle_drop_csv_row(NULL); puts("CSVGlue recursive ownership/equality OK"); return 0;
}
