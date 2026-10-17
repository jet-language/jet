// Real Jet object only: link -Wl,--wrap=jet_rt_free. No mock providers.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t hits, misses, size; Text *bound; } Memo;
typedef struct { int64_t fields[6]; } Limits;
typedef struct { int64_t domain, kind; Text *path, *detail; int64_t pid, port; } Event;
_Static_assert(sizeof(Memo)==32 && offsetof(Memo,bound)==24,"MemoABI");
_Static_assert(sizeof(Limits)==48,"MailLimitsABI");
_Static_assert(sizeof(Event)==48 && offsetof(Event,path)==16 && offsetof(Event,pid)==32,"WatchABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
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
extern Limits *jet_rt_record_new_limits(int64_t,int64_t,int64_t,int64_t,int64_t,int64_t);
extern Limits *jet_rt_handle_clone_limits(Limits *);
extern void jet_rt_handle_drop_limits(Limits *);
extern bool jet_rt_handle_eq_limits(Limits *,Limits *);
extern int64_t jet_rt_record_get_limits_max_reply_line_bytes(Limits *);
extern int64_t jet_rt_record_get_limits_max_reply_lines(Limits *);
extern int64_t jet_rt_record_get_limits_max_capabilities(Limits *);
extern int64_t jet_rt_record_get_limits_max_recipients(Limits *);
extern int64_t jet_rt_record_get_limits_max_message_bytes(Limits *);
extern int64_t jet_rt_record_get_limits_max_auth_challenge_bytes(Limits *);
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
void __wrap_jet_rt_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(size==watched[i].size && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,size,align);
}
static void watch(void *p,int64_t size,int64_t align) {
    assert(count<32);
    for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].size=size; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
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
    assert(a!=b && a->bound!=bound && b->bound!=a->bound && b->bound->data!=a->bound->data);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo); jet_rt_string_drop(bound);
    assert(a->hits==INT64_MAX && a->misses==INT64_MIN && a->size==7);
    int64_t x=jet_rt_record_get_jet_memo_stats_hits(a),y=jet_rt_record_get_jet_memo_stats_hits(a);
    assert(x!=y); integer(x,INT64_MAX); integer(y,INT64_MAX);
    integer(jet_rt_record_get_jet_memo_stats_misses(a),INT64_MIN); integer(jet_rt_record_get_jet_memo_stats_size(a),7);
    Text *getter=jet_rt_record_get_jet_memo_stats_bound(a);
    assert(getter!=a->bound && getter->data!=a->bound->data);
    a->bound->data[0]='X'; expect(getter,"unbounded"); expect(jet_rt_record_get_jet_memo_stats_bound(b),"unbounded");
    watch(a,32,8);watch(b,32,8);watch_text(a->bound);watch_text(b->bound);
    unsigned before=intercepted;jet_rt_handle_drop_jet_memo_stats(a);jet_rt_handle_drop_jet_memo_stats(b);freed(before);
    jet_rt_handle_drop_jet_memo_stats(NULL);
}
static void limits_cases(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Limits *a=jet_rt_record_new_limits(hi,lo,3,4,5,6),*b=jet_rt_handle_clone_limits(a);
    jet_rt_int_drop(hi);jet_rt_int_drop(lo);assert(a!=b && a->fields[0]==INT64_MAX && a->fields[1]==INT64_MIN);
    int64_t (*getters[])(Limits *)={jet_rt_record_get_limits_max_reply_line_bytes,jet_rt_record_get_limits_max_reply_lines,jet_rt_record_get_limits_max_capabilities,jet_rt_record_get_limits_max_recipients,jet_rt_record_get_limits_max_message_bytes,jet_rt_record_get_limits_max_auth_challenge_bytes};
    assert(jet_rt_handle_eq_limits(a,a) && jet_rt_handle_eq_limits(a,b));
    for(size_t i=0;i<6;++i) { integer(getters[i](a),a->fields[i]); b->fields[i]^=1;assert(!jet_rt_handle_eq_limits(a,b));b->fields[i]^=1; }
    watch(a,48,8);watch(b,48,8);unsigned before=intercepted;jet_rt_handle_drop_limits(a);jet_rt_handle_drop_limits(b);freed(before);jet_rt_handle_drop_limits(NULL);
}
static void event_cases(void) {
    const char *domains[]={"File","Process","Port"},*kinds[]={"Created","Modified","Removed","Error","Exited","Ready"};
    for(int64_t d=0;d<3;++d) for(int64_t k=0;k<6;++k) {
        Text *path=text("a\"\n"),*detail=text("detail\\raw");
        int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
        Event *a=jet_rt_record_new_watch_event(d,k,path,detail,hi,lo),*b=jet_rt_handle_clone_watch_event(a);
        jet_rt_int_drop(hi);jet_rt_int_drop(lo);jet_rt_string_drop(path);jet_rt_string_drop(detail);
        assert(a!=b && a->path!=b->path && a->detail!=b->detail && a->path->data!=b->path->data && a->detail->data!=b->detail->data);
        assert(a->pid==INT64_MAX && a->port==INT64_MIN);
        assert(jet_rt_record_get_watch_event_domain(a)==d && jet_rt_record_get_watch_event_kind(a)==k);
        integer(jet_rt_record_get_watch_event_pid(a),INT64_MAX);integer(jet_rt_record_get_watch_event_port(a),INT64_MIN);
        Text *p=jet_rt_record_get_watch_event_path(a),*t=jet_rt_record_get_watch_event_detail(a);
        assert(p!=a->path && p->data!=a->path->data && t!=a->detail && t->data!=a->detail->data);
        expect(p,"a\"\n");expect(t,"detail\\raw");
        char golden[160];snprintf(golden,sizeof(golden),"WatchEvent { domain: %s, kind: %s, path: a\"\n, detail: detail\\raw }",domains[d],kinds[k]);
        // CommonTypes.rs:6772-6777: unquoted raw text, no pid/port.
        expect(jet_rt_handle_show_watch_event(a),golden);assert(jet_rt_handle_eq_watch_event(a,a) && jet_rt_handle_eq_watch_event(a,b));
        b->domain=(d+1)%3;assert(!jet_rt_handle_eq_watch_event(a,b));b->domain=d;
        b->kind=(k+1)%6;assert(!jet_rt_handle_eq_watch_event(a,b));b->kind=k;
        b->path->data[0]='X';assert(!jet_rt_handle_eq_watch_event(a,b));b->path->data[0]='a';
        b->detail->data[0]='X';assert(!jet_rt_handle_eq_watch_event(a,b));b->detail->data[0]='d';
        b->pid=0;assert(!jet_rt_handle_eq_watch_event(a,b));b->pid=INT64_MAX;
        b->port=0;assert(!jet_rt_handle_eq_watch_event(a,b));b->port=INT64_MIN;
        Text *survivor=jet_rt_record_get_watch_event_path(a);
        watch(a,48,8);watch(b,48,8);watch_text(a->path);watch_text(a->detail);watch_text(b->path);watch_text(b->detail);
        unsigned before=intercepted;jet_rt_handle_drop_watch_event(a);jet_rt_handle_drop_watch_event(b);freed(before);expect(survivor,"a\"\n");
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
    memo_cases();limits_cases();event_cases();puts("record-glue witness OK");return 0;
}
