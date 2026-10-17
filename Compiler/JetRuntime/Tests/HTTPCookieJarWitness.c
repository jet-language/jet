// Link real Jet object with -Wl,--wrap=jet_rt_free; no providers.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
typedef struct { int64_t ordinal; } CookieJar;
_Static_assert(sizeof(CookieJar)==8,"CookieJar enum8");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern CookieJar *jet_rt_handle_clone_jet_http_cookie_jar(CookieJar *);
extern void jet_rt_handle_drop_jet_http_cookie_jar(CookieJar *);
static CookieJar *original,*copy;
static unsigned original_frees,copy_frees;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    assert(n==8 && a==8);
    if(p==original) assert(++original_frees==1);
    else { assert(p==copy && ++copy_frees==1); }
    __real_jet_rt_free(p,n,a);
}
int main(void) {
    jet_rt_alloc_init(); original=jet_rt_alloc(8,8); original->ordinal=0;
    copy=jet_rt_handle_clone_jet_http_cookie_jar(original); assert(copy!=original && copy->ordinal==0);
    jet_rt_handle_drop_jet_http_cookie_jar(original); assert(copy->ordinal==0);
    jet_rt_handle_drop_jet_http_cookie_jar(copy); jet_rt_handle_drop_jet_http_cookie_jar(NULL);
    assert(original_frees==1 && copy_frees==1);
    puts("http cookie jar: Memory enum independent ownership ok"); return 0;
}
