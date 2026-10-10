/* Real composed Jet object client. Link --wrap=jet_rt_free and compose
 * TLSRootsWitnessABI.jet. Wipe is observed BEFORE the real allocator free. */
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; unsigned char *data; } Bytes;
typedef struct { int64_t len,cap; Text **data; } Texts;
typedef struct { int64_t kind; Bytes roots; } Trust;
typedef struct { Bytes chain; Bytes *key; } Identity;
typedef struct { Trust trust; Identity *identity; int64_t min,max; Texts alpn; } Config;
_Static_assert(sizeof(Identity)==32 && _Alignof(Identity)==8 && offsetof(Identity,key)==24,"Identity ABI");
_Static_assert(sizeof(Config)==80 && _Alignof(Config)==8 && offsetof(Config,identity)==32 && offsetof(Config,min)==40 && offsetof(Config,max)==48 && offsetof(Config,alpn)==56,"Config ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t runtime_tls_u8_stride(void);
extern Text *jet_rt_string_from_static(const char *,int64_t);
extern Bytes *jet_rt_secret_from_bytes(Bytes *);
extern Identity *jet_rt_handle_clone_jet_tls_client_identity(Identity *);
extern void jet_rt_handle_drop_jet_tls_client_identity(Identity *);
extern Config *jet_rt_handle_clone_jet_tls_client_config(Config *);
extern void jet_rt_handle_drop_jet_tls_client_config(Config *);
extern Trust *jet_rt_handle_field_jet_tls_client_config_trust(Config *);
struct Watch { void *ptr; int64_t size,align; unsigned erase,freed; };
static struct Watch watched[4096];
static size_t count,erased,intercepted;
static int64_t stride;
static void watch(void *ptr,int64_t size,int64_t align,unsigned erase) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[count++]=(struct Watch){ptr,size,align,erase,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        struct Watch *w=&watched[i-1];
        /* Actual free size equals actual List allocation and observed wipe. */
        assert(w->size==size && w->align==align);
        if(w->erase) {
            for(int64_t at=0;at<size;++at) assert(((unsigned char *)ptr)[at]==0);
            ++erased;
        }
        w->freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void bytes_watch(Bytes *b,unsigned erase) { if(b->cap) watch(b->data,b->cap*stride,8,erase); }
static Bytes bytes(int64_t len,int64_t cap,unsigned erase) {
    Bytes b={len,cap,cap ? jet_rt_alloc(cap*stride,8) : NULL};
    if(cap) memset(b.data,0xd5,(size_t)(cap*stride));
    for(int64_t i=0;i<len;++i) b.data[i*stride]=(unsigned char)(i ? 255 : 0);
    bytes_watch(&b,erase); return b;
}
static Bytes *secret(int64_t len,int64_t cap) {
    Bytes *input=jet_rt_alloc(24,8); watch(input,24,8,0); *input=bytes(len,cap,1);
    Bytes *s=jet_rt_secret_from_bytes(input); watch(s,24,8,0);
    /* Secret adopts the buffer and clears, but does not own the donor header. */
    assert(!input->len && !input->cap && !input->data);
    jet_rt_free(input,24,8); return s;
}
static void identity_watch(Identity *i) {
    watch(i,32,8,0); bytes_watch(&i->chain,0); watch(i->key,24,8,0); bytes_watch(i->key,1);
}
static Identity *identity(int64_t len) {
    Identity *i=jet_rt_alloc(32,8); watch(i,32,8,0);
    i->chain=bytes(len,len+2,0); i->key=secret(len,len+3); return i;
}
static void bytes_independent(Bytes *a,Bytes *b) {
    assert(a->len==b->len && b->cap==a->len);
    if(a->len) {
        assert(a->data!=b->data);
        for(int64_t i=0;i<a->len;++i) assert(a->data[i*stride]==b->data[i*stride]);
        b->data[0]^=1; assert(a->data[0]!=b->data[0]); b->data[0]^=1;
    }
}
static void identity_independent(Identity *a,Identity *b) {
    assert(a!=b && a->key!=b->key);
    bytes_independent(&a->chain,&b->chain); bytes_independent(a->key,b->key);
}
static void text_watch(Text *t) { watch(t,24,8,0); if(t->cap) watch(t->data,t->cap,1,0); }
static Text *text(const char *s,int64_t len) { Text *t=jet_rt_string_from_static(s,len); text_watch(t); return t; }
static void config_watch(Config *c) {
    watch(c,80,8,0); if(c->trust.kind) bytes_watch(&c->trust.roots,0);
    if(c->identity) identity_watch(c->identity);
    if(c->alpn.cap) watch(c->alpn.data,c->alpn.cap*8,8,0);
    for(int64_t at=0;at<c->alpn.len;++at) text_watch(c->alpn.data[at]);
}
static Config *config(int64_t kind,int with_identity,int64_t protocols) {
    Config *c=jet_rt_alloc(80,8); watch(c,80,8,0);
    c->trust.kind=kind;
    c->trust.roots=kind ? bytes(2,4,0) : (Bytes){INT64_MAX,INT64_MAX,(unsigned char *)(uintptr_t)1};
    c->identity=with_identity ? identity(3) : NULL;
    c->min=INT64_MIN; c->max=INT64_MAX;
    c->alpn.len=protocols; c->alpn.cap=protocols+1;
    c->alpn.data=jet_rt_alloc(c->alpn.cap*8,8); watch(c->alpn.data,c->alpn.cap*8,8,0);
    if(protocols) c->alpn.data[0]=text("h2",2);
    if(protocols>1) c->alpn.data[1]=text("h\0x\377",4);
    return c;
}
int main(void) {
    jet_rt_alloc_init(); stride=runtime_tls_u8_stride(); assert(stride==1 || stride==8);
    jet_rt_handle_drop_jet_tls_client_identity(NULL); jet_rt_handle_drop_jet_tls_client_config(NULL);
    for(int64_t len=0;len<4;++len) {
        Identity *a=identity(len),*b=jet_rt_handle_clone_jet_tls_client_identity(a); identity_watch(b);
        identity_independent(a,b); jet_rt_handle_drop_jet_tls_client_identity(a);
        assert(b->chain.len==len && b->key->len==len);
        for(int64_t at=0;at<len;++at) assert(b->key->data[at*stride]==(unsigned char)(at ? 255 : 0));
        jet_rt_handle_drop_jet_tls_client_identity(b);
    }
    for(int64_t kind=0;kind<3;++kind) for(int present=0;present<2;++present) for(int64_t protocols=0;protocols<3;++protocols) {
        Config *a=config(kind,present,protocols),*b=jet_rt_handle_clone_jet_tls_client_config(a); config_watch(b);
        assert(a!=b && jet_rt_handle_field_jet_tls_client_config_trust(a)==&a->trust);
        assert(jet_rt_handle_field_jet_tls_client_config_trust(b)==&b->trust && b->trust.kind==kind);
        if(kind) bytes_independent(&a->trust.roots,&b->trust.roots);
        else assert(!b->trust.roots.len && !b->trust.roots.cap && !b->trust.roots.data);
        assert((b->identity!=NULL)==present);
        if(present) identity_independent(a->identity,b->identity);
        assert(b->min==INT64_MIN && b->max==INT64_MAX && b->alpn.len==protocols && b->alpn.cap==protocols);
        for(int64_t at=0;at<protocols;++at) {
            Text *x=a->alpn.data[at],*y=b->alpn.data[at];
            assert(x!=y && x->data!=y->data && x->len==y->len && !memcmp(x->data,y->data,(size_t)x->len));
        }
        jet_rt_handle_drop_jet_tls_client_config(a);
        if(present) assert(b->identity->key->len==3 && b->identity->key->data[2*stride]==255);
        if(protocols>1) assert(b->alpn.data[1]->len==4 && b->alpn.data[1]->data[1]==0 && b->alpn.data[1]->data[3]==255);
        jet_rt_handle_drop_jet_tls_client_config(b);
    }
    assert(intercepted>0 && erased>0);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("TLSClient5: independent identity/config, optional fields, borrowed trust, ALPN, full-capacity Secret wipe OK");
    return 0;
}
