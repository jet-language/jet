// Real Jet-object client: -Wl,--wrap=jet_rt_free, no runtime provider mocks.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *name,*value; } Entry;
typedef struct { int64_t length,capacity; Entry *data; } Headers;
_Static_assert(sizeof(Headers)==24 && offsetof(Headers,data)==16,"DataABI sequence24");
_Static_assert(sizeof(Entry)==16 && offsetof(Entry,value)==8,"HTTP header entry16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern Headers *jet_rt_handle_clone_jet_http_headers(Headers *);
extern void jet_rt_handle_drop_jet_http_headers(Headers *);
extern bool jet_rt_handle_eq_jet_http_headers(Headers *,Headers *);
static struct { void *p; int64_t n,a; unsigned freed; } watched[48];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) {
        assert(n==watched[i].n && a==watched[i].a && ++watched[i].freed==1);
    }
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<48); watched[count].p=p; watched[count].n=n; watched[count].a=a; watched[count++].freed=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_headers(Headers *h) {
    watch(h,24,8); if(h->capacity) watch(h->data,h->capacity*16,8);
    for(int64_t i=0;i<h->length;i++) { watch_text(h->data[i].name); watch_text(h->data[i].value); }
}
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static Headers *headers(int64_t capacity) {
    Headers *h=jet_rt_alloc(24,8); h->length=0; h->capacity=capacity;
    h->data=capacity?jet_rt_alloc(capacity*16,8):(void *)(uintptr_t)8; return h;
}
static void entry(Headers *h,const char *name,const char *value) {
    assert(h->length<h->capacity); h->data[h->length].name=text(name); h->data[h->length++].value=text(value);
}
int main(void) {
    jet_rt_alloc_init();
    Headers *empty=headers(0),*empty_copy=jet_rt_handle_clone_jet_http_headers(empty);
    assert(empty!=empty_copy && empty_copy->length==0 && empty_copy->capacity==0);
    assert(jet_rt_handle_eq_jet_http_headers(empty,empty_copy));
    watch_headers(empty); watch_headers(empty_copy);
    jet_rt_handle_drop_jet_http_headers(empty); jet_rt_handle_drop_jet_http_headers(empty_copy); all_freed();
    Headers *a=headers(7); entry(a,"X-Repeat","first"); entry(a,"X-Repeat",""); entry(a,"Mixed-Case","caf\xc3\xa9");
    Headers *b=jet_rt_handle_clone_jet_http_headers(a);
    assert(b!=a && b->data!=a->data && b->length==3 && b->capacity==3);
    assert(jet_rt_handle_eq_jet_http_headers(a,b) && jet_rt_handle_eq_jet_http_headers(b,a));
    for(int i=0;i<3;i++) { assert(a->data[i].name!=b->data[i].name && a->data[i].value!=b->data[i].value); }
    Headers *different=headers(3); entry(different,"x-Repeat","first"); entry(different,"X-Repeat",""); entry(different,"Mixed-Case","caf\xc3\xa9");
    assert(!jet_rt_handle_eq_jet_http_headers(a,different));
    different->data[0].name->data[0]='X'; assert(jet_rt_handle_eq_jet_http_headers(a,different));
    Entry swap=different->data[0]; different->data[0]=different->data[1]; different->data[1]=swap;
    assert(!jet_rt_handle_eq_jet_http_headers(a,different));
    different->length=2; assert(!jet_rt_handle_eq_jet_http_headers(a,different)); different->length=3;
    watch_headers(a); watch_headers(b); watch_headers(different);
    jet_rt_handle_drop_jet_http_headers(a); text_is(b->data[0].value,"first"); text_is(b->data[1].value,""); text_is(b->data[2].value,"caf\xc3\xa9");
    jet_rt_handle_drop_jet_http_headers(b); jet_rt_handle_drop_jet_http_headers(different); jet_rt_handle_drop_jet_http_headers(NULL); all_freed();
    puts("http headers: ordered deep ownership + equality ok"); return 0;
}
