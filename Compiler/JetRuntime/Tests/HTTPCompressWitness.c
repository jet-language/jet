// Link real Jet object with -Wl,--wrap=jet_rt_free; no providers.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
typedef struct { int64_t ordinal; } Compress;
_Static_assert(sizeof(Compress)==8,"Compress enum8");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Compress *jet_rt_handle_clone_jet_http_compress_encoding(Compress *);
extern void jet_rt_handle_drop_jet_http_compress_encoding(Compress *);
extern bool jet_rt_handle_eq_jet_http_compress_encoding(Compress *,Compress *);
static Compress *original,*copy;
static unsigned original_frees,copy_frees;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    assert(n==8 && a==8);
    if(p==original) assert(++original_frees==1);
    else { assert(p==copy && ++copy_frees==1); }
    __real_jet_rt_free(p,n,a);
}
int main(void) {
    jet_rt_alloc_init(); original=jet_rt_alloc(8,8); original->ordinal=0;
    copy=jet_rt_handle_clone_jet_http_compress_encoding(original); Compress distinct={1};
    assert(copy!=original && copy->ordinal==0);
    assert(jet_rt_handle_eq_jet_http_compress_encoding(original,copy) && jet_rt_handle_eq_jet_http_compress_encoding(copy,original));
    assert(!jet_rt_handle_eq_jet_http_compress_encoding(original,&distinct));
    jet_rt_handle_drop_jet_http_compress_encoding(original); assert(copy->ordinal==0);
    jet_rt_handle_drop_jet_http_compress_encoding(copy); jet_rt_handle_drop_jet_http_compress_encoding(NULL);
    assert(original_frees==1 && copy_frees==1);
    puts("http compress: Gzip owner clone/drop/eq ok"); return 0;
}
