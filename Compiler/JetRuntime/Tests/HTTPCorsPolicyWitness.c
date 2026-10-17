// Real Jet object client: -Wl,--wrap=jet_rt_free; no providers.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; Text **data; } List;
typedef struct { int64_t ordinal; List *payload; } Origins;
typedef struct { Origins origins; Text *methods,*headers; int64_t credentials,max_age; } Policy;
_Static_assert(sizeof(Policy)==48 && offsetof(Policy,methods)==16 && offsetof(Policy,credentials)==32 && offsetof(Policy,max_age)==40,"CorsPolicy48");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern Policy *jet_rt_handle_clone_jet_http_cors_policy(Policy *);
extern void jet_rt_handle_drop_jet_http_cors_policy(Policy *);
extern Origins *jet_rt_handle_clone_jet_http_cors_origins(Origins *);
extern void jet_rt_handle_drop_jet_http_cors_origins(Origins *);
extern const Origins *jet_rt_handle_field_jet_http_cors_policy_origins(Policy *);
extern const unsigned char *jet_rt_handle_field_jet_http_cors_policy_credentials(Policy *);
extern void jet_rt_handle_field_jet_http_cors_policy_allow_methods(Policy *,Text *);
extern void jet_rt_handle_field_jet_http_cors_policy_allow_headers(Policy *,Text *);
static struct { void *p; int64_t n,a; unsigned freed; } watched[40];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) assert(n==watched[i].n && a==watched[i].a && ++watched[i].freed==1);
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<40); watched[count].p=p; watched[count].n=n; watched[count].a=a; watched[count++].freed=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_origins_child(Origins *o) {
    if(o->ordinal==0) return;
    List *l=o->payload; watch(l,24,8); if(l->capacity) watch(l->data,l->capacity*8,8);
    for(int64_t i=0;i<l->length;i++) watch_text(l->data[i]);
}
static void watch_policy(Policy *p) { watch(p,48,8); watch_origins_child(&p->origins); watch_text(p->methods); watch_text(p->headers); }
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static void run(int64_t ordinal,int64_t credential) {
    Policy *p=jet_rt_alloc(48,8); p->origins.ordinal=ordinal; p->origins.payload=NULL;
    if(ordinal) {
        List *l=jet_rt_alloc(24,8); l->length=2; l->capacity=5; l->data=jet_rt_alloc(40,8);
        l->data[0]=text("https://example.com"); l->data[1]=text(""); p->origins.payload=l;
    }
    p->methods=text("GET, POST"); p->headers=text("content-type"); p->credentials=credential; p->max_age=INT64_MAX;
    Text method_view,header_view; jet_rt_handle_field_jet_http_cors_policy_allow_methods(p,&method_view); jet_rt_handle_field_jet_http_cors_policy_allow_headers(p,&header_view);
    assert(method_view.data==p->methods->data && header_view.data==p->headers->data);
    assert(method_view.capacity==p->methods->capacity && header_view.capacity==p->headers->capacity);
    assert(jet_rt_handle_field_jet_http_cors_policy_origins(p)==&p->origins);
    assert(jet_rt_handle_field_jet_http_cors_policy_credentials(p)==(unsigned char *)&p->credentials);
    assert(*jet_rt_handle_field_jet_http_cors_policy_credentials(p)==(unsigned char)credential);
    Text *method=jet_rt_string_clone(&method_view),*header=jet_rt_string_clone(&header_view);
    Origins *origin=jet_rt_handle_clone_jet_http_cors_origins((Origins *)jet_rt_handle_field_jet_http_cors_policy_origins(p));
    Policy *copy=jet_rt_handle_clone_jet_http_cors_policy(p);
    assert(copy!=p && copy->methods!=p->methods && copy->headers!=p->headers);
    assert(copy->credentials==((unsigned char)credential!=0) && copy->max_age==INT64_MAX);
    assert(copy->origins.ordinal==ordinal); if(ordinal) assert(copy->origins.payload!=p->origins.payload && copy->origins.payload->data[0]!=p->origins.payload->data[0]);
    watch_policy(p); watch_policy(copy); watch_text(method); watch_text(header); watch(origin,16,8); watch_origins_child(origin);
    jet_rt_handle_drop_jet_http_cors_policy(p);
    text_is(method,"GET, POST"); text_is(header,"content-type"); text_is(copy->methods,"GET, POST");
    if(ordinal) text_is(origin->payload->data[0],"https://example.com");
    jet_rt_handle_drop_jet_http_cors_policy(copy); jet_rt_string_drop(method); jet_rt_string_drop(header); jet_rt_handle_drop_jet_http_cors_origins(origin); all_freed();
}
int main(void) {
    jet_rt_alloc_init(); run(0,0); run(0,256); run(1,255); run(1,257); jet_rt_handle_drop_jet_http_cors_policy(NULL);
    puts("http cors policy: deep embedded ownership + borrowed fields + Bool byte ok"); return 0;
}
