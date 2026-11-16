// Link real Jet object; no providers.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
typedef struct { int64_t max,same_origin; } Redirect;
_Static_assert(sizeof(Redirect)==16 && offsetof(Redirect,same_origin)==8,"Redirect16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Redirect *jet_rt_handle_clone_jet_http_redirect_policy(Redirect *);
extern void jet_rt_handle_drop_jet_http_redirect_policy(Redirect *);
static Redirect *original,*copy;
static unsigned original_frees,copy_frees;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t n,int64_t a) {
    assert(n==16 && a==8);
    if(p==original) assert(++original_frees==1);
    else { assert(p==copy && ++copy_frees==1); }
}
int main(void) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    const int64_t limits[]={0,7,INT64_MIN,INT64_MAX};
    const int64_t flags[]={0,1,255,256,257,-256};
    for(size_t i=0;i<sizeof(limits)/sizeof(*limits);i++) for(size_t j=0;j<sizeof(flags)/sizeof(*flags);j++) {
        original=jet_rt_alloc(16,8); original->max=limits[i]; original->same_origin=flags[j];
        copy=jet_rt_handle_clone_jet_http_redirect_policy(original); original_frees=copy_frees=0;
        assert(copy!=original && copy->max==limits[i] && copy->same_origin==((unsigned char)flags[j]!=0));
        jet_rt_handle_drop_jet_http_redirect_policy(original); assert(copy->max==limits[i]);
        jet_rt_handle_drop_jet_http_redirect_policy(copy); jet_rt_handle_drop_jet_http_redirect_policy(NULL);
        assert(original_frees==1 && copy_frees==1);
    }
    puts("http redirect: signed max + Bool byte + independent owner ok"); return 0;
}
