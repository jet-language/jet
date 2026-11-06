// Real Jet object only:. No mock providers.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t hits, misses, size; Text *bound; } Memo;
typedef struct { int64_t domain, kind; Text *path, *detail; int64_t pid, port; } Event;
_Static_assert(sizeof(Memo)==32 && offsetof(Memo,bound)==24,"MemoABI");
_Static_assert(sizeof(Event)==48 && offsetof(Event,path)==16 && offsetof(Event,pid)==32,"WatchABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Memo *jet_rt_record_new_jet_memo_stats(int64_t,int64_t,int64_t,Text *);
extern Memo *jet_rt_handle_clone_jet_memo_stats(Memo *);
extern void jet_rt_handle_drop_jet_memo_stats(Memo *);
extern int64_t jet_rt_record_get_jet_memo_stats_hits(Memo *);
extern int64_t jet_rt_record_get_jet_memo_stats_misses(Memo *);
extern int64_t jet_rt_record_get_jet_memo_stats_size(Memo *);
extern Text *jet_rt_record_get_jet_memo_stats_bound(Memo *);
extern Event *jet_rt_record_new_watch_event(int64_t,int64_t,Text *,Text *,int64_t,int64_t);
extern Event *jet_rt_handle_clone_watch_event(Event *);
extern void jet_rt_handle_drop_watch_event(Event *);
extern bool jet_rt_handle_eq_watch_event(Event *,Event *);
extern Text *jet_rt_handle_show_watch_event(Event *);
extern int64_t jet_rt_record_get_watch_event_domain(Event *);
extern int64_t jet_rt_record_get_watch_event_kind(Event *);
extern Text *jet_rt_record_get_watch_event_path(Event *);
extern Text *jet_rt_record_get_watch_event_detail(Event *);
extern int64_t jet_rt_record_get_watch_event_pid(Event *);
extern int64_t jet_rt_record_get_watch_event_port(Event *);
extern int64_t *jet_rt_handle_clone_watch_kind(int64_t *);
extern void jet_rt_handle_drop_watch_kind(int64_t *);
extern bool jet_rt_handle_eq_watch_kind(int64_t *,int64_t *);
static struct { void *p; int64_t size,align; unsigned frees; } watched[32];
static size_t count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(size==watched[i].size && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
}
// A block watched again before it is freed is another owner of the same
// refcounted text: it stays one entry that must be freed exactly once.
static void watch(void *p,int64_t size,int64_t align) {
    assert(count<32);
    for(size_t i=0;i<count;++i) if(watched[i].p==p) { assert(!watched[i].frees && size==watched[i].size && align==watched[i].align); return; }
    watched[count].p=p; watched[count].size=size; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,40,8); if(t->capacity) watch(t->data,t->capacity,1); }
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(Text *t) { return *(int64_t *)((char *)t-8)&INT64_C(0x3fffffffffffffff); }
static bool text_block(Text *t,void *p) { return p==(char *)t-8 || (t->capacity && p==t->data); }
// An owned copy of t whose first byte differs: clones share t's bytes, so a
// differing field is a different text, never a write through a shared one.
static Text *flipped(Text *t) {
    char bytes[64]; assert(t->length>0 && t->length<=64);
    memcpy(bytes,t->data,(size_t)t->length); bytes[0]^=1;
    return jet_rt_string_from_static(bytes,t->length);
}
static void freed(unsigned before) {
    assert(intercepted>before);
    for(size_t i=0;i<count;++i) assert(watched[i].frees==1);
    count=0;
}
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void expect(Text *t,const char *s) {
    assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); jet_rt_string_drop(t);
}
static void integer(int64_t owner,int64_t expected) { assert(jet_int_owned_to_i64(owner)==expected); jet_rt_int_drop(owner); }
static void memo_cases(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Text *bound=text("unbounded");
    Memo *a=jet_rt_record_new_jet_memo_stats(hi,lo,7,bound),*b=jet_rt_handle_clone_jet_memo_stats(a);
    // Records are reallocated; constructor and clone share the text by owner count.
    assert(a!=b && a->bound==bound && b->bound==bound && owners(bound)==3);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo); jet_rt_string_drop(bound);
    assert(a->hits==INT64_MAX && a->misses==INT64_MIN && a->size==7);
    int64_t x=jet_rt_record_get_jet_memo_stats_hits(a),y=jet_rt_record_get_jet_memo_stats_hits(a);
    assert(x!=y); integer(x,INT64_MAX); integer(y,INT64_MAX);
    integer(jet_rt_record_get_jet_memo_stats_misses(a),INT64_MIN); integer(jet_rt_record_get_jet_memo_stats_size(a),7);
    Text *getter=jet_rt_record_get_jet_memo_stats_bound(a);
    assert(getter==a->bound && owners(getter)==3);
    expect(getter,"unbounded"); expect(jet_rt_record_get_jet_memo_stats_bound(b),"unbounded"); assert(owners(a->bound)==2);
    watch(a,32,8);watch(b,32,8);watch_text(a->bound);watch_text(b->bound);
    unsigned before=intercepted;jet_rt_handle_drop_jet_memo_stats(a);jet_rt_handle_drop_jet_memo_stats(b);freed(before);
    jet_rt_handle_drop_jet_memo_stats(NULL);
}
static void event_cases(void) {
    const char *domains[]={"File","Process","Port"},*kinds[]={"Created","Modified","Removed","Error","Exited","Ready"};
    for(int64_t d=0;d<3;++d) for(int64_t k=0;k<6;++k) {
        Text *path=text("a\"\n"),*detail=text("detail\\raw");
        int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
        Event *a=jet_rt_record_new_watch_event(d,k,path,detail,hi,lo),*b=jet_rt_handle_clone_watch_event(a);
        jet_rt_int_drop(hi);jet_rt_int_drop(lo);jet_rt_string_drop(path);jet_rt_string_drop(detail);
        assert(a!=b && a->path==b->path && a->detail==b->detail && owners(a->path)==2 && owners(a->detail)==2);
        assert(a->pid==INT64_MAX && a->port==INT64_MIN);
        assert(jet_rt_record_get_watch_event_domain(a)==d && jet_rt_record_get_watch_event_kind(a)==k);
        integer(jet_rt_record_get_watch_event_pid(a),INT64_MAX);integer(jet_rt_record_get_watch_event_port(a),INT64_MIN);
        Text *p=jet_rt_record_get_watch_event_path(a),*t=jet_rt_record_get_watch_event_detail(a);
        assert(p==a->path && owners(p)==3 && t==a->detail && owners(t)==3);
        expect(p,"a\"\n");expect(t,"detail\\raw");
        char golden[160];snprintf(golden,sizeof(golden),"WatchEvent { domain: %s, kind: %s, path: a\"\n, detail: detail\\raw }",domains[d],kinds[k]);
        // Unquoted raw text, no pid/port.
        expect(jet_rt_handle_show_watch_event(a),golden);assert(jet_rt_handle_eq_watch_event(a,a) && jet_rt_handle_eq_watch_event(a,b));
        b->domain=(d+1)%3;assert(!jet_rt_handle_eq_watch_event(a,b));b->domain=d;
        b->kind=(k+1)%6;assert(!jet_rt_handle_eq_watch_event(a,b));b->kind=k;
        Text *kept=b->path,*other=flipped(kept);b->path=other;assert(!jet_rt_handle_eq_watch_event(a,b));b->path=kept;jet_rt_string_drop(other);
        kept=b->detail;other=flipped(kept);b->detail=other;assert(!jet_rt_handle_eq_watch_event(a,b));b->detail=kept;jet_rt_string_drop(other);
        b->pid=0;assert(!jet_rt_handle_eq_watch_event(a,b));b->pid=INT64_MAX;
        b->port=0;assert(!jet_rt_handle_eq_watch_event(a,b));b->port=INT64_MIN;
        Text *survivor=jet_rt_record_get_watch_event_path(a);assert(survivor==a->path && owners(survivor)==3);
        watch(a,48,8);watch(b,48,8);watch_text(a->path);watch_text(a->detail);
        unsigned before=intercepted;jet_rt_handle_drop_watch_event(a);jet_rt_handle_drop_watch_event(b);
        // The getter's owner keeps the shared path alive past both records.
        assert(owners(survivor)==1);for(size_t j=0;j<count;++j) assert(watched[j].frees==(text_block(survivor,watched[j].p)?0u:1u));
        expect(survivor,"a\"\n");freed(before);
    }
    jet_rt_handle_drop_watch_event(NULL);
    for(int64_t k=0;k<6;++k) {
        int64_t *a=jet_rt_alloc(8,8);*a=k;int64_t *b=jet_rt_handle_clone_watch_kind(a);
        assert(a!=b && *b==k && jet_rt_handle_eq_watch_kind(a,b));*b=(k+1)%6;assert(!jet_rt_handle_eq_watch_kind(a,b));
        watch(a,8,8);watch(b,8,8);unsigned before=intercepted;jet_rt_handle_drop_watch_kind(a);jet_rt_handle_drop_watch_kind(b);freed(before);
    }
    jet_rt_handle_drop_watch_kind(NULL);
}
int main(int argc,char **argv) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if(argc>1) {
        Text *s=text("");
        if(strcmp(argv[1],"bad-domain")==0) (void)jet_rt_record_new_watch_event(3,0,s,s,0,0);
        else if(strcmp(argv[1],"bad-kind")==0) (void)jet_rt_record_new_watch_event(0,-1,s,s,0,0);
        else if(strcmp(argv[1],"native-overflow")==0) {
            int64_t hi=jet_int_owned_from_i64(INT64_MAX),big=jet_int_add(hi,1);
            (void)jet_rt_record_new_jet_memo_stats(big,0,0,s);
        } else return 2;
        assert(!"stop returned");
    }
    memo_cases();event_cases();puts("record-glue witness OK");return 0;
}
