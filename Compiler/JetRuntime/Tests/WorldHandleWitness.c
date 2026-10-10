// Real native World128 lease client; link with -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
extern void jet_rt_alloc_init(void);
extern void *jet_rt_world_new(void);
extern void *jet_rt_handle_clone_jet_deterministic_world(void *);
extern void jet_rt_handle_drop_jet_deterministic_world(void *);
extern int64_t jet_world_now(void *);
extern int64_t jet_world_advance(void *,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
static void *watched;
static unsigned releases,intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    if(p==watched) { assert(bytes==128 && align==8 && ++releases==1); }
    __real_jet_rt_free(p,bytes,align);
}
int main(void) {
    jet_rt_alloc_init(); void *original=jet_rt_world_new(); watched=original;
    assert(((int64_t *)original)[0]==1);
    void *a=jet_rt_handle_clone_jet_deterministic_world(original);
    void *b=jet_rt_handle_clone_jet_deterministic_world(a);
    assert(a==original && b==original && ((int64_t *)original)[0]==3);
    assert(jet_world_now(a)==0 && jet_world_now(b)==0);
    (void)jet_world_advance(a,2000000); assert(jet_world_now(b)==2);
    jet_rt_handle_drop_jet_deterministic_world(original);
    assert(!releases && ((int64_t *)a)[0]==2 && jet_world_now(a)==2);
    (void)jet_world_advance(b,3000000); assert(jet_world_now(a)==5);
    jet_rt_handle_drop_jet_deterministic_world(a);
    assert(!releases && ((int64_t *)b)[0]==1 && jet_world_now(b)==5);
    jet_rt_handle_drop_jet_deterministic_world(b);
    assert(intercepted && releases==1); jet_rt_handle_drop_jet_deterministic_world(NULL);
    puts("WorldHandle shared leases/state/final drop OK"); return 0;
}
