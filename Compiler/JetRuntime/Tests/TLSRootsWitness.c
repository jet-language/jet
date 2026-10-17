/* Real composed Jet object client, --wrap=jet_rt_free; compose the test-only
 * TLSRootsWitnessABI.jet export runtime_tls_u8_stride with the unit. */
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int64_t len,cap; unsigned char *data; } Root;
typedef struct { int64_t kind; Root roots; } Trust;
_Static_assert(sizeof(Root)==24 && _Alignof(Root)==8,"Root ABI");
_Static_assert(sizeof(Trust)==32 && offsetof(Trust,roots)==8 && _Alignof(Trust)==8,"Trust ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t runtime_tls_u8_stride(void);
extern Root *jet_rt_record_new_jet_tls_root_certificates(Root *);
extern Root *jet_rt_record_get_jet_tls_root_certificates_pem(Root *);
extern Root *jet_rt_handle_clone_jet_tls_root_certificates(Root *);
extern void jet_rt_handle_drop_jet_tls_root_certificates(Root *);
extern Trust *jet_rt_record_new_jet_tls_trust_system(void);
extern Trust *jet_rt_record_new_jet_tls_trust_system_plus(Root *);
extern Trust *jet_rt_record_new_jet_tls_trust_custom_only(Root *);
extern Root *jet_rt_record_get_jet_tls_trust_system_plus(Trust *);
extern Root *jet_rt_record_get_jet_tls_trust_custom_only(Trust *);
extern int64_t jet_rt_record_tag_jet_tls_trust(Trust *);
extern Trust *jet_rt_handle_clone_jet_tls_trust(Trust *);
extern void jet_rt_handle_drop_jet_tls_trust(Trust *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[512];
static size_t count,intercepted;
static int64_t stride;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(watched[i-1].size==size && watched[i-1].align==align);
        watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void root_watch(Root *r) { watch(r,24,8); if(r->cap) watch(r->data,r->cap*stride,8); }
static void trust_watch(Trust *t) { watch(t,32,8); if(t->kind && t->roots.cap) watch(t->roots.data,t->roots.cap*stride,8); }
static Root *list(int64_t len,int64_t cap) {
    Root *r=jet_rt_alloc(24,8); r->len=len; r->cap=cap;
    r->data=cap ? jet_rt_alloc(cap*stride,8) : NULL;
    if(cap) memset(r->data,0x5a,(size_t)(cap*stride));
    for(int64_t i=0;i<len;++i) r->data[i*stride]=(unsigned char)(i ? 255 : 0);
    root_watch(r); return r;
}
static void equal(Root *a,Root *b) {
    assert(a->len==b->len);
    for(int64_t i=0;i<a->len;++i) assert(a->data[i*stride]==b->data[i*stride]);
}
static void independent(Root *a,Root *b) {
    assert(a!=b && b->cap==a->len); equal(a,b);
    if(a->len) { assert(a->data!=b->data); b->data[0]^=1; assert(a->data[0]!=b->data[0]); b->data[0]^=1; }
}
int main(int argc,char **argv) {
    jet_rt_alloc_init(); stride=runtime_tls_u8_stride(); assert(stride==1 || stride==8);
    if(argc==2) {
        Trust *system=jet_rt_record_new_jet_tls_trust_system();
        if(!strcmp(argv[1],"wrong-plus")) { (void)jet_rt_record_get_jet_tls_trust_system_plus(system); return 99; }
        if(!strcmp(argv[1],"wrong-custom")) { (void)jet_rt_record_get_jet_tls_trust_custom_only(system); return 99; }
        assert(!"unknown mode");
    }
    jet_rt_handle_drop_jet_tls_root_certificates(NULL); jet_rt_handle_drop_jet_tls_trust(NULL);
    for(int64_t len=0;len<3;++len) for(int64_t spare=0;spare<3;++spare) {
        Root *input=list(len,len+spare); unsigned char *data=input->data;
        Root *root=jet_rt_record_new_jet_tls_root_certificates(input); watch(root,24,8);
        assert(root->data==data && root->len==len && root->cap==len+spare);
        assert(jet_rt_record_get_jet_tls_root_certificates_pem(root)==root);
        Root *clone=jet_rt_handle_clone_jet_tls_root_certificates(root); root_watch(clone); independent(root,clone);
        for(int64_t kind=1;kind<3;++kind) {
            Root *donor=jet_rt_handle_clone_jet_tls_root_certificates(root); root_watch(donor); data=donor->data;
            Trust *trust=kind==1 ? jet_rt_record_new_jet_tls_trust_system_plus(donor) : jet_rt_record_new_jet_tls_trust_custom_only(donor);
            watch(trust,32,8); assert(trust->roots.data==data && jet_rt_record_tag_jet_tls_trust(trust)==kind);
            Trust *tc=jet_rt_handle_clone_jet_tls_trust(trust); trust_watch(tc); independent(&trust->roots,&tc->roots);
            Root *got=kind==1 ? jet_rt_record_get_jet_tls_trust_system_plus(trust) : jet_rt_record_get_jet_tls_trust_custom_only(trust);
            root_watch(got); independent(&trust->roots,got);
            jet_rt_handle_drop_jet_tls_trust(trust); equal(root,got); equal(root,&tc->roots);
            jet_rt_handle_drop_jet_tls_root_certificates(got); jet_rt_handle_drop_jet_tls_trust(tc);
        }
        jet_rt_handle_drop_jet_tls_root_certificates(root); assert(clone->len==len); jet_rt_handle_drop_jet_tls_root_certificates(clone);
    }
    Trust *system=jet_rt_record_new_jet_tls_trust_system(); trust_watch(system);
    assert(jet_rt_record_tag_jet_tls_trust(system)==0 && !system->roots.len && !system->roots.cap && !system->roots.data);
    /* Deliberately dirty inactive words. Neither clone nor drop may touch them. */
    system->roots=(Root){INT64_MAX,INT64_MAX,(unsigned char *)(uintptr_t)1};
    Trust *copy=jet_rt_handle_clone_jet_tls_trust(system); trust_watch(copy);
    assert(!copy->kind && !copy->roots.len && !copy->roots.cap && !copy->roots.data);
    jet_rt_handle_drop_jet_tls_trust(system); jet_rt_handle_drop_jet_tls_trust(copy);
    assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("TLSRoots12: roots/trust ownership, binary bytes, canonical stride, borrowed fields OK");
    return 0;
}
