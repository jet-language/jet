// A foreign host embedding the Jet runtime that never calls
// jet_rt_host_arguments: its first runtime entry is an allocation. The
// JET_CHECKED_ALLOC decision is the runtime's .init_array's
// (Alloc.jet jet_rt_alloc_check_init, jet_rt_alloc_check_refuse_init), so
// checked mode still holds here (run-alloc-parity.mjs embed-* cases).
#include <stdint.h>
#include <string.h>
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
static void embed_clean(void) {
    void *p=jet_rt_alloc(40,8);
    jet_rt_free(p,40,8);
}
static void embed_double_free(void) {
    void *p=jet_rt_alloc(40,8);
    jet_rt_free(p,40,8);
    jet_rt_free(p,40,8);
}
int main(int argc,char **argv) {
    if(argc!=2) return 2;
    if(!strcmp(argv[1],"embed-clean")) embed_clean();
    else if(!strcmp(argv[1],"embed-double-free")) embed_double_free();
    else return 2;
    return 0;
}
