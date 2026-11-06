// Link real Jet object; no providers.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
typedef struct { int64_t ordinal; } Retry;
_Static_assert(sizeof(Retry)==8,"Retry enum8");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Retry *jet_rt_handle_clone_jet_http_retry_policy(Retry *);
extern void jet_rt_handle_drop_jet_http_retry_policy(Retry *);
static Retry *original,*copy;
static unsigned original_frees,copy_frees;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t n,int64_t a) {
    assert(n==8 && a==8);
    if(p==original) assert(++original_frees==1);
    else { assert(p==copy && ++copy_frees==1); }
}
int main(void) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    for(int64_t i=0;i<3;i++) {
        original=jet_rt_alloc(8,8); original->ordinal=i; copy=jet_rt_handle_clone_jet_http_retry_policy(original);
        original_frees=copy_frees=0; assert(copy!=original && copy->ordinal==i);
        jet_rt_handle_drop_jet_http_retry_policy(original); assert(copy->ordinal==i);
        jet_rt_handle_drop_jet_http_retry_policy(copy); jet_rt_handle_drop_jet_http_retry_policy(NULL);
        assert(original_frees==1 && copy_frees==1);
    }
    puts("http retry: three enum owners clone/drop ok"); return 0;
}
