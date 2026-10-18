// Real Jet object client, -Wl,--wrap=jet_rt_free; no providers.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *cert,*key; } Tls;
_Static_assert(sizeof(Tls)==16 && offsetof(Tls,key)==8,"ServerTls16");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Tls *jet_rt_record_new_jet_http_server_tls(Text *,Text *);
extern Tls *jet_rt_handle_clone_jet_http_server_tls(Tls *);
extern void jet_rt_handle_drop_jet_http_server_tls(Tls *);
extern Text *jet_rt_record_get_jet_http_server_tls_cert_pem(Tls *);
extern Text *jet_rt_record_get_jet_http_server_tls_key_pem(Tls *);
static struct { void *p; int64_t n,a; unsigned freed; } watched[28];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) assert(n==watched[i].n && a==watched[i].a && ++watched[i].freed==1);
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<28); watched[count].p=p; watched[count].n=n; watched[count].a=a; watched[count++].freed=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_tls(Tls *t) { watch(t,16,8); watch_text(t->cert); watch_text(t->key); }
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static void run(const char *cert,const char *key) {
    Text *c=text(cert),*k=text(key); Tls *a=jet_rt_record_new_jet_http_server_tls(c,k),*b=jet_rt_handle_clone_jet_http_server_tls(a);
    Text *cg=jet_rt_record_get_jet_http_server_tls_cert_pem(a),*kg=jet_rt_record_get_jet_http_server_tls_key_pem(a),*cg2=jet_rt_record_get_jet_http_server_tls_cert_pem(a);
    assert(a!=b && c!=a->cert && k!=a->key && a->cert!=b->cert && a->key!=b->key && cg!=a->cert && kg!=a->key && cg2!=cg);
    text_is(c,cert); text_is(k,key); text_is(a->cert,cert); text_is(a->key,key); text_is(b->cert,cert); text_is(b->key,key);
    watch_text(c); watch_text(k); watch_tls(a); watch_tls(b); watch_text(cg); watch_text(kg); watch_text(cg2);
    jet_rt_string_drop(c); jet_rt_string_drop(k); jet_rt_handle_drop_jet_http_server_tls(a);
    text_is(b->cert,cert); text_is(b->key,key); text_is(cg,cert); text_is(kg,key); text_is(cg2,cert);
    jet_rt_handle_drop_jet_http_server_tls(b); jet_rt_string_drop(cg); jet_rt_string_drop(kg); jet_rt_string_drop(cg2); all_freed();
}
int main(void) {
    jet_rt_alloc_init(); run("-----BEGIN CERTIFICATE-----\ncert\n-----END CERTIFICATE-----", "-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----");
    run("",""); run("arbitrary certificate text","caf\xc3\xa9 key"); jet_rt_handle_drop_jet_http_server_tls(NULL);
    puts("http server tls: borrowed ctor + deep owners + fresh getters ok"); return 0;
}
