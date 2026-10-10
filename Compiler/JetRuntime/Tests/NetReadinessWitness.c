/* Real composed Jet-object client; link with --wrap=jet_rt_free. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
typedef struct { int64_t readable,writable; } Ready;
_Static_assert(sizeof(Ready)==16 && _Alignof(Ready)==8,"Ready16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Ready *jet_rt_record_new_jet_net_ready(uint8_t,uint8_t);
extern bool jet_rt_record_get_jet_net_ready_readable(Ready *);
extern bool jet_rt_record_get_jet_net_ready_writable(Ready *);
extern Ready *jet_rt_handle_clone_jet_net_ready(Ready *);
extern bool jet_rt_handle_eq_jet_net_ready(Ready *,Ready *);
extern void jet_rt_handle_drop_jet_net_ready(Ready *);
extern int64_t *jet_rt_handle_clone_jet_net_ready_interest(int64_t *);
extern bool jet_rt_handle_eq_jet_net_ready_interest(int64_t *,int64_t *);
extern void jet_rt_handle_drop_jet_net_ready_interest(int64_t *);
extern int64_t *jet_rt_handle_clone_jet_net_shutdown(int64_t *);
extern bool jet_rt_handle_eq_jet_net_shutdown(int64_t *,int64_t *);
extern void jet_rt_handle_drop_jet_net_shutdown(int64_t *);
struct Watched { void *ptr; int64_t size; unsigned freed; };
static struct Watched watched[256];
static size_t count;
static void watch(void *ptr,int64_t size) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==8); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
int main(void) {
    jet_rt_alloc_init();
    jet_rt_handle_drop_jet_net_ready(NULL); jet_rt_handle_drop_jet_net_ready_interest(NULL); jet_rt_handle_drop_jet_net_shutdown(NULL);
    const uint8_t inputs[]={0,1,2,255};
    for(size_t i=0;i<4;++i) for(size_t j=0;j<4;++j) {
        Ready *a=jet_rt_record_new_jet_net_ready(inputs[i],inputs[j]); watch(a,16);
        assert(a->readable==(inputs[i]!=0) && a->writable==(inputs[j]!=0));
        Ready *b=jet_rt_handle_clone_jet_net_ready(a); watch(b,16);
        assert(a!=b && jet_rt_handle_eq_jet_net_ready(a,b) && jet_rt_handle_eq_jet_net_ready(b,a));
        b->readable^=1; assert(!jet_rt_handle_eq_jet_net_ready(a,b)); b->readable^=1;
        b->writable^=1; assert(!jet_rt_handle_eq_jet_net_ready(a,b)); b->writable^=1;
        jet_rt_handle_drop_jet_net_ready(a);
        assert(jet_rt_record_get_jet_net_ready_readable(b)==(inputs[i]!=0));
        assert(jet_rt_record_get_jet_net_ready_writable(b)==(inputs[j]!=0)); jet_rt_handle_drop_jet_net_ready(b);
    }
    Ready raw={0x100,0x102},canonical={0,1};
    assert(!jet_rt_record_get_jet_net_ready_readable(&raw) && jet_rt_record_get_jet_net_ready_writable(&raw));
    assert(jet_rt_handle_eq_jet_net_ready(&raw,&canonical));
    Ready *copy=jet_rt_handle_clone_jet_net_ready(&raw); watch(copy,16);
    assert(copy->readable==0 && copy->writable==1); jet_rt_handle_drop_jet_net_ready(copy);
    for(int64_t kind=0;kind<3;++kind) {
        int64_t *interest=jet_rt_alloc(8,8),*shutdown=jet_rt_alloc(8,8); *interest=kind; *shutdown=kind;
        watch(interest,8); watch(shutdown,8);
        int64_t *ic=jet_rt_handle_clone_jet_net_ready_interest(interest),*sc=jet_rt_handle_clone_jet_net_shutdown(shutdown);
        watch(ic,8); watch(sc,8); assert(ic!=interest && sc!=shutdown && *ic==kind && *sc==kind);
        assert(jet_rt_handle_eq_jet_net_ready_interest(interest,ic) && jet_rt_handle_eq_jet_net_shutdown(shutdown,sc));
        *ic=(kind+1)%3; *sc=(kind+1)%3;
        assert(!jet_rt_handle_eq_jet_net_ready_interest(interest,ic) && !jet_rt_handle_eq_jet_net_shutdown(shutdown,sc));
        jet_rt_handle_drop_jet_net_ready_interest(interest); jet_rt_handle_drop_jet_net_shutdown(shutdown);
        assert(*ic==(kind+1)%3 && *sc==(kind+1)%3);
        jet_rt_handle_drop_jet_net_ready_interest(ic); jet_rt_handle_drop_jet_net_shutdown(sc);
    }
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("net-readiness: Bool normalization/low-byte slots, clone/equality/getters, all interest/shutdown ordinals, exact shell frees, null drops OK");
    return 0;
}
