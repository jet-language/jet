/* Real Jet-object client; link with --wrap=jet_rt_free. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern void jet_rt_handle_drop_jet_db_connection(void *);
static void *watched;
static unsigned freed;
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    if(ptr==watched) { assert(!freed && size==8 && align==8); ++freed; }
    __real_jet_rt_free(ptr,size,align);
}
int main(void) {
    jet_rt_alloc_init(); jet_rt_handle_drop_jet_db_connection(NULL);
    const uint64_t ids[]={0,1,UINT64_MAX};
    for(unsigned i=0;i<3;++i) {
        uint64_t *handle=jet_rt_alloc(8,8); *handle=ids[i]; watched=handle; freed=0;
        jet_rt_handle_drop_jet_db_connection(handle); assert(freed==1);
    }
    puts("db-connection: explicit-close carrier drop frees only shell8, all u64 IDs, null drop OK");
    return 0;
}
