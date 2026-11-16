/* Link real providers; checked-heap alloc/free observers. No C runtime mocks. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
typedef int64_t H;
typedef struct {uint64_t len,cap;H *data;} Pairs;
typedef struct {H fields[12];Pairs *pairs;} Plan;
_Static_assert(sizeof(Plan)==104,"UiABI snapshot layouts");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern H jet_rt_handle_clone_jet_web_virtual_plan(H);
extern void jet_rt_handle_drop_jet_web_virtual_plan(H);
extern bool jet_rt_handle_eq_jet_web_virtual_plan(H,H);
static struct {H address,bytes,align;bool live;} ledger[512];
static size_t allocations,frees;
static bool tracking;
static void observe_alloc(H address, H bytes, H align) {
    if(tracking){assert(address&&bytes>0&&allocations<512&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}
}
static void observe_free(H address,H bytes,H align) {
    if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;return;}assert(!"untracked or double free");}
}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static void plan(size_t n) {
    Plan *a=(Plan *)(uintptr_t)jet_rt_alloc(sizeof *a,8);for(size_t i=0;i<12;++i)a->fields[i]=i%2?INT64_MIN+(H)i:INT64_MAX-(H)i;
    Pairs *p=(Pairs *)(uintptr_t)jet_rt_alloc(sizeof *p,8);p->len=n;p->cap=n+2;p->data=(H *)(uintptr_t)jet_rt_alloc((H)p->cap*16,8);a->pairs=p;
    for(size_t i=0;i<n;++i){p->data[i*2]=(H)(i%3);p->data[i*2+1]=INT64_MIN+(H)i;}
    H ah=(H)(uintptr_t)a;Plan *b=(Plan *)(uintptr_t)jet_rt_handle_clone_jet_web_virtual_plan(ah);H bh=(H)(uintptr_t)b;
    assert(a!=b&&a->pairs!=b->pairs&&b->pairs->len==n&&b->pairs->cap==n&&jet_rt_handle_eq_jet_web_virtual_plan(ah,bh));
    for(size_t i=0;i<12;++i){b->fields[i]^=1;assert(!jet_rt_handle_eq_jet_web_virtual_plan(ah,bh));b->fields[i]^=1;}
    if(n){assert(p->data!=b->pairs->data);for(size_t i=0;i<n*2;++i){assert(p->data[i]==b->pairs->data[i]);b->pairs->data[i]^=1;assert(!jet_rt_handle_eq_jet_web_virtual_plan(ah,bh));b->pairs->data[i]^=1;}--p->len;assert(!jet_rt_handle_eq_jet_web_virtual_plan(ah,bh));++p->len;}
    if(n>1){H old=p->data[1];p->data[1]=p->data[3];p->data[3]=old;assert(!jet_rt_handle_eq_jet_web_virtual_plan(ah,bh));p->data[3]=p->data[1];p->data[1]=old;}
    jet_rt_handle_drop_jet_web_virtual_plan(ah);assert(jet_rt_handle_eq_jet_web_virtual_plan(bh,bh));jet_rt_handle_drop_jet_web_virtual_plan(bh);balanced();
}
int main(void){assert(jet_rt_alloc_check_observe((int64_t)(uintptr_t)observe_alloc, (int64_t)(uintptr_t)observe_free) == 1);jet_rt_alloc_init();tracking=true;plan(0);plan(1);plan(17);jet_rt_handle_drop_jet_web_virtual_plan(0);balanced();puts("UI view snapshots: twelve raw fields, ordered sparse pairs and exact-once frees ok");return 0;}
