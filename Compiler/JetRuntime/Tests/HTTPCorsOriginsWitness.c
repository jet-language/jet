// Real Jet runtime object client: -Wl,--wrap=jet_rt_free; no providers.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; Text **data; } List;
typedef struct { int64_t ordinal; List *payload; } Origins;
_Static_assert(sizeof(Origins)==16 && offsetof(Origins,payload)==8,"CorsOrigins16");
_Static_assert(sizeof(List)==24 && offsetof(List,data)==16,"sequence24");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern Origins *jet_rt_handle_clone_jet_http_cors_origins(Origins *);
extern void jet_rt_handle_drop_jet_http_cors_origins(Origins *);
static struct { void *p; int64_t n,a; unsigned freed; } watched[32];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) assert(n==watched[i].n && a==watched[i].a && ++watched[i].freed==1);
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<32); watched[count].p=p; watched[count].n=n; watched[count].a=a; watched[count++].freed=0;
}
static void watch_origins(Origins *o) {
    watch(o,16,8); if(o->ordinal==0) return;
    List *list=o->payload; watch(list,24,8); if(list->capacity) watch(list->data,list->capacity*8,8);
    for(int64_t i=0;i<list->length;i++) { Text *t=list->data[i]; watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
}
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static Origins *origins(int64_t ordinal,int64_t length,int64_t capacity) {
    Origins *o=jet_rt_alloc(16,8); o->ordinal=ordinal; o->payload=NULL;
    if(ordinal) { List *l=jet_rt_alloc(24,8); l->length=length; l->capacity=capacity; l->data=capacity?jet_rt_alloc(capacity*8,8):(void *)(uintptr_t)8; o->payload=l; }
    return o;
}
int main(void) {
    jet_rt_alloc_init();
    Origins *a=origins(0,0,0),*b=jet_rt_handle_clone_jet_http_cors_origins(a);
    assert(a!=b && b->ordinal==0 && !b->payload); watch_origins(a); watch_origins(b);
    jet_rt_handle_drop_jet_http_cors_origins(a); jet_rt_handle_drop_jet_http_cors_origins(b); all_freed();
    a=origins(1,0,0); b=jet_rt_handle_clone_jet_http_cors_origins(a);
    assert(b!=a && b->payload!=a->payload && b->payload->length==0 && b->payload->capacity==0);
    watch_origins(a); watch_origins(b); jet_rt_handle_drop_jet_http_cors_origins(a); jet_rt_handle_drop_jet_http_cors_origins(b); all_freed();
    a=origins(1,3,7); a->payload->data[0]=text("https://example.com"); a->payload->data[1]=text(""); a->payload->data[2]=text("https://example.com");
    b=jet_rt_handle_clone_jet_http_cors_origins(a);
    assert(b!=a && b->payload!=a->payload && b->payload->data!=a->payload->data && b->payload->capacity==3 && b->payload->length==3);
    for(int i=0;i<3;i++) assert(a->payload->data[i]!=b->payload->data[i]);
    watch_origins(a); watch_origins(b); jet_rt_handle_drop_jet_http_cors_origins(a);
    text_is(b->payload->data[0],"https://example.com"); text_is(b->payload->data[1],""); text_is(b->payload->data[2],"https://example.com");
    jet_rt_handle_drop_jet_http_cors_origins(b); jet_rt_handle_drop_jet_http_cors_origins(NULL); all_freed();
    puts("http cors origins: Any + empty/repeated List ownership ok"); return 0;
}
