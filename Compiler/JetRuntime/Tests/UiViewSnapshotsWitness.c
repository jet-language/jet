/* Link real providers --wrap=jet_rt_alloc/jet_rt_free. No C runtime mocks. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {H kind,custom,width,height,scale;} Preview;
typedef struct {uint64_t len,cap;H *data;} Pairs;
typedef struct {H fields[12];Pairs *pairs;} Plan;
_Static_assert(sizeof(Preview)==40&&sizeof(Plan)==104,"UiABI snapshot layouts");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H jet_rt_handle_clone_jet_ui_preview_viewport(H),jet_rt_handle_clone_jet_web_virtual_plan(H);
extern void jet_rt_handle_drop_jet_ui_preview_viewport(H),jet_rt_handle_drop_jet_web_virtual_plan(H);
extern bool jet_rt_handle_eq_jet_ui_preview_viewport(H,H),jet_rt_handle_eq_jet_web_virtual_plan(H,H);
static struct {H address,bytes,align;bool live;} ledger[512];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking){assert(address&&bytes>0&&allocations<512&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}
    return address;
}
void __wrap_jet_rt_free(H address,H bytes,H align) {
    if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}
    __real_jet_rt_free(address,bytes,align);
}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static void preview(H kind,const char *name) {
    Preview *a=(Preview *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->kind=kind;a->custom=kind==3?(H)(uintptr_t)jet_rt_string_from_static(name,(H)strlen(name)):INT64_MIN;
    a->width=(H)UINT64_C(0xdeadbeefffffffff);a->height=(H)UINT64_C(0xabababab00000000);a->scale=(H)UINT64_C(0x123456789abcffff);
    H ah=(H)(uintptr_t)a;Preview *b=(Preview *)(uintptr_t)jet_rt_handle_clone_jet_ui_preview_viewport(ah);H bh=(H)(uintptr_t)b;
    assert(a!=b&&jet_rt_handle_eq_jet_ui_preview_viewport(ah,bh));assert(b->width==UINT32_MAX&&b->height==0&&b->scale==UINT16_MAX);
    H *fields[]={&b->width,&b->height,&b->scale};
    for(size_t i=0;i<3;++i){*fields[i]^=1;assert(!jet_rt_handle_eq_jet_ui_preview_viewport(ah,bh));*fields[i]^=1;}
    b->kind=(kind+1)%4;assert(!jet_rt_handle_eq_jet_ui_preview_viewport(ah,bh));b->kind=kind;
    if(kind==3){Text *x=(Text *)(uintptr_t)a->custom,*y=(Text *)(uintptr_t)b->custom;assert(x!=y);if(x->len){assert(x->data!=y->data);x->data[0]^=1;assert(!jet_rt_handle_eq_jet_ui_preview_viewport(ah,bh));x->data[0]^=1;}}
    jet_rt_handle_drop_jet_ui_preview_viewport(ah);assert(jet_rt_handle_eq_jet_ui_preview_viewport(bh,bh));jet_rt_handle_drop_jet_ui_preview_viewport(bh);balanced();
}
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
int main(void){jet_rt_alloc_init();tracking=true;for(H i=0;i<4;++i)preview(i,"custom\xc3\xa9");preview(3,"");plan(0);plan(1);plan(17);jet_rt_handle_drop_jet_ui_preview_viewport(0);jet_rt_handle_drop_jet_web_virtual_plan(0);balanced();puts("UI view snapshots: all devices, scalar masks, twelve raw fields, ordered sparse pairs and exact-once frees ok");return 0;}
