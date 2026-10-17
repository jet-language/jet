/* Real composed Jet object client. Link --wrap=jet_rt_free and compose
 * TLSRootsWitnessABI.jet; no provider bodies or Rust layouts. */
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; unsigned char *data; } Bytes;
typedef struct { int64_t len,cap; Text **data; } Texts;
typedef struct { Bytes der,sha,spki; Texts dns; int64_t from,until; Text *subject,*issuer; } Cert;
typedef struct { int64_t len,cap; Cert *data; } Certs;
typedef struct { Text *verified; Cert leaf; Certs chain; Text *cipher; int64_t version; } Peer;
_Static_assert(sizeof(Cert)==128 && _Alignof(Cert)==8 && offsetof(Cert,dns)==72 && offsetof(Cert,from)==96 && offsetof(Cert,subject)==112,"Cert ABI");
_Static_assert(sizeof(Peer)==176 && _Alignof(Peer)==8 && offsetof(Peer,leaf)==8 && offsetof(Peer,chain)==136 && offsetof(Peer,cipher)==160,"Peer ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t runtime_tls_u8_stride(void);
extern Text *jet_rt_string_from_static(const char *,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern Cert *jet_rt_handle_clone_jet_tls_certificate(Cert *);
extern void jet_rt_handle_drop_jet_tls_certificate(Cert *);
extern void jet_rt_handle_field_jet_tls_certificate_subject(Cert *,Text *);
extern void jet_rt_handle_field_jet_tls_certificate_issuer(Cert *,Text *);
extern Peer *jet_rt_handle_clone_jet_tls_peer_identity(Peer *);
extern void jet_rt_handle_drop_jet_tls_peer_identity(Peer *);
extern void jet_rt_handle_field_jet_tls_peer_identity_verified_server_name(Peer *,Text *);
extern void jet_rt_handle_field_jet_tls_peer_identity_cipher_suite(Peer *,Text *);
extern Cert *jet_rt_handle_field_jet_tls_peer_identity_leaf(Peer *);
extern Certs *jet_rt_record_seq_list_rec_jet_tls_certificate_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_jet_tls_certificate_len(Certs *);
extern Cert *jet_rt_record_seq_list_rec_jet_tls_certificate_get(Certs *,int64_t);
extern void jet_rt_record_seq_list_rec_jet_tls_certificate_push(Certs *,Cert *);
struct Watch { void *ptr; int64_t size,align; unsigned freed; };
static struct Watch watched[4096];
static size_t count,intercepted;
static int64_t stride;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[count++]=(struct Watch){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(watched[i-1].size==size && watched[i-1].align==align); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *t) { watch(t,24,8); if(t->cap) watch(t->data,t->cap,1); }
static Text *text(const char *s) { Text *t=jet_rt_string_from_static(s,(int64_t)strlen(s)); text_watch(t); return t; }
static void bytes_watch(Bytes *b) { if(b->cap) watch(b->data,b->cap*stride,8); }
static void cert_fields_watch(Cert *c) {
    bytes_watch(&c->der); bytes_watch(&c->sha); bytes_watch(&c->spki);
    if(c->dns.cap) watch(c->dns.data,c->dns.cap*8,8);
    for(int64_t i=0;i<c->dns.len;++i) text_watch(c->dns.data[i]);
    text_watch(c->subject); text_watch(c->issuer);
}
static void peer_watch(Peer *p) {
    watch(p,176,8); text_watch(p->verified); cert_fields_watch(&p->leaf);
    if(p->chain.cap) watch(p->chain.data,p->chain.cap*128,8);
    for(int64_t i=0;i<p->chain.len;++i) cert_fields_watch(&p->chain.data[i]);
    text_watch(p->cipher);
}
static Bytes bytes(int64_t len,int64_t cap) {
    Bytes b={len,cap,cap ? jet_rt_alloc(cap*stride,8) : NULL};
    if(cap) memset(b.data,0x5a,(size_t)(cap*stride));
    for(int64_t i=0;i<len;++i) b.data[i*stride]=(unsigned char)(i ? 255 : 0);
    bytes_watch(&b); return b;
}
static Cert *certificate(int64_t len) {
    Cert *c=jet_rt_alloc(128,8); watch(c,128,8);
    c->der=bytes(len,len+1); c->sha=bytes(len,len); c->spki=bytes(0,0);
    c->dns.len=len ? 2 : 0; c->dns.cap=c->dns.len+1;
    c->dns.data=jet_rt_alloc(c->dns.cap*8,8); watch(c->dns.data,c->dns.cap*8,8);
    if(c->dns.len) { c->dns.data[0]=text("example.com"); c->dns.data[1]=text(""); }
    c->from=INT64_MIN; c->until=INT64_MAX; c->subject=text("subject"); c->issuer=text("");
    return c;
}
static void text_equal(Text *a,Text *b) { assert(a->len==b->len && !memcmp(a->data,b->data,(size_t)a->len)); }
static void text_independent(Text *a,Text *b) { assert(a!=b); text_equal(a,b); if(a->len) assert(a->data!=b->data); }
static void bytes_independent(Bytes *a,Bytes *b) {
    assert(a->len==b->len && b->cap==a->len);
    if(a->len) { assert(a->data!=b->data); for(int64_t i=0;i<a->len;++i) assert(a->data[i*stride]==b->data[i*stride]); }
}
static void cert_independent(Cert *a,Cert *b) {
    assert(a!=b); bytes_independent(&a->der,&b->der); bytes_independent(&a->sha,&b->sha); bytes_independent(&a->spki,&b->spki);
    assert(a->from==b->from && a->until==b->until && a->dns.len==b->dns.len && b->dns.cap==a->dns.len);
    if(a->dns.len) assert(a->dns.data!=b->dns.data);
    for(int64_t i=0;i<a->dns.len;++i) text_independent(a->dns.data[i],b->dns.data[i]);
    text_independent(a->subject,b->subject); text_independent(a->issuer,b->issuer);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init(); stride=runtime_tls_u8_stride(); assert(stride==1 || stride==8);
    if(argc==2) {
        if(!strcmp(argv[1],"bad-capacity")) { (void)jet_rt_record_seq_list_rec_jet_tls_certificate_new(-1); return 99; }
        if(!strcmp(argv[1],"capacity-overflow")) { (void)jet_rt_record_seq_list_rec_jet_tls_certificate_new(INT64_MAX); return 99; }
        Certs *s=jet_rt_record_seq_list_rec_jet_tls_certificate_new(0);
        if(!strcmp(argv[1],"index-low")) { (void)jet_rt_record_seq_list_rec_jet_tls_certificate_get(s,-1); return 99; }
        if(!strcmp(argv[1],"index-high")) { (void)jet_rt_record_seq_list_rec_jet_tls_certificate_get(s,0); return 99; }
        assert(!"unknown mode");
    }
    jet_rt_handle_drop_jet_tls_certificate(NULL); jet_rt_handle_drop_jet_tls_peer_identity(NULL);
    for(int64_t len=0;len<3;++len) {
        Cert *c=certificate(len),*copy=jet_rt_handle_clone_jet_tls_certificate(c); watch(copy,128,8); cert_fields_watch(copy); cert_independent(c,copy);
        Text subject,issuer; jet_rt_handle_field_jet_tls_certificate_subject(c,&subject); jet_rt_handle_field_jet_tls_certificate_issuer(c,&issuer);
        assert(subject.data==c->subject->data && subject.len==c->subject->len && subject.cap==c->subject->cap);
        assert(issuer.data==c->issuer->data && issuer.len==0);
        Text *saved=jet_rt_string_clone(&subject); text_watch(saved);
        jet_rt_handle_drop_jet_tls_certificate(c); text_equal(saved,copy->subject); jet_rt_string_drop(saved); jet_rt_handle_drop_jet_tls_certificate(copy);
    }
    Certs *chain=jet_rt_record_seq_list_rec_jet_tls_certificate_new(1); watch(chain,24,8); watch(chain->data,128,8);
    for(int64_t i=0;i<6;++i) {
        Cert *item=certificate(i%3); unsigned char *adopted=item->der.data; Cert *old=chain->data;
        jet_rt_record_seq_list_rec_jet_tls_certificate_push(chain,item);
        if(old!=chain->data) watch(chain->data,chain->cap*128,8);
        assert(jet_rt_record_seq_list_rec_jet_tls_certificate_len(chain)==i+1);
        Cert *got=jet_rt_record_seq_list_rec_jet_tls_certificate_get(chain,i);
        assert(got==chain->data+i && got->der.data==adopted);
        for(int64_t j=0;j<=i;++j) assert(chain->data[j].from==INT64_MIN && chain->data[j].until==INT64_MAX);
    }
    Peer *p=jet_rt_alloc(176,8); watch(p,176,8); p->verified=text("example.com"); p->cipher=text("TLS_AES_128_GCM_SHA256"); p->version=1;
    Cert *leaf=certificate(2); p->leaf=*leaf; jet_rt_free(leaf,128,8); p->chain=*chain; jet_rt_free(chain,24,8);
    Peer *copy=jet_rt_handle_clone_jet_tls_peer_identity(p); peer_watch(copy);
    assert(copy->version==1 && copy->chain.len==6 && copy->chain.cap==6 && copy->chain.data!=p->chain.data);
    cert_independent(&p->leaf,&copy->leaf); text_independent(p->verified,copy->verified); text_independent(p->cipher,copy->cipher);
    for(int64_t i=0;i<6;++i) cert_independent(&p->chain.data[i],&copy->chain.data[i]);
    assert(jet_rt_handle_field_jet_tls_peer_identity_leaf(p)==&p->leaf);
    Text name,cipher; jet_rt_handle_field_jet_tls_peer_identity_verified_server_name(p,&name); jet_rt_handle_field_jet_tls_peer_identity_cipher_suite(p,&cipher);
    assert(name.data==p->verified->data && cipher.data==p->cipher->data);
    Text *saved=jet_rt_string_clone(&cipher); text_watch(saved);
    jet_rt_handle_drop_jet_tls_peer_identity(p); text_equal(saved,copy->cipher); jet_rt_string_drop(saved);
    assert(copy->leaf.der.data[0]==0 && copy->leaf.der.data[stride]==255);
    jet_rt_handle_drop_jet_tls_peer_identity(copy);
    assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("TLSCert13: certificate/peer/list deep ownership, borrowed fields, growth, signed times OK");
    return 0;
}
