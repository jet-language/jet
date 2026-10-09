/* Real Jet providers via test-only doors, wrap allocations/frees. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {Text *path,*root;H access;} Path;
typedef struct {H len,cap;Path *data;} Selection;
_Static_assert(sizeof(Path)==24 && sizeof(Selection)==24,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_dialog_selection_witness_clone(H);
extern void ui_dialog_selection_witness_drop(H);
extern bool ui_dialog_selection_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[512];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<512&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H h(const void *p){return (H)(uintptr_t)p;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static void different(Text *t,H a,H b){if(t->len){t->data[0]^=1;assert(!ui_dialog_selection_witness_eq(a,b));t->data[0]^=1;}else{t->len=1;assert(!ui_dialog_selection_witness_eq(a,b));t->len=0;}}
static void cases(H n,H spare){
    Selection *a=(Selection *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->len=n;a->cap=n+spare;a->data=a->cap?(Path *)(uintptr_t)jet_rt_alloc(a->cap*(H)sizeof(Path),8):(Path *)(uintptr_t)8;
    for(H i=0;i<n;++i){a->data[i].path=text(i%2?"":"/grant/file");a->data[i].root=text(i%3?"/grant":"");a->data[i].access=i%2;}
    Selection *b=(Selection *)(uintptr_t)ui_dialog_selection_witness_clone(h(a));assert(a!=b&&b->len==n&&b->cap==n&&ui_dialog_selection_witness_eq(h(a),h(b)));
    if(n){assert(a->data!=b->data);for(H i=0;i<n;++i){Path *x=&a->data[i],*y=&b->data[i];assert(x->path!=y->path&&x->root!=y->root);if(x->path->len)assert(x->path->data!=y->path->data);if(x->root->len)assert(x->root->data!=y->root->data);different(y->path,h(a),h(b));different(y->root,h(a),h(b));y->access^=1;assert(!ui_dialog_selection_witness_eq(h(a),h(b)));y->access^=1;}--b->len;assert(!ui_dialog_selection_witness_eq(h(a),h(b)));++b->len;}
    if(n>1){Path tmp=b->data[0];b->data[0]=b->data[1];b->data[1]=tmp;assert(!ui_dialog_selection_witness_eq(h(a),h(b)));tmp=b->data[0];b->data[0]=b->data[1];b->data[1]=tmp;}
    ui_dialog_selection_witness_drop(h(a));assert(ui_dialog_selection_witness_eq(h(b),h(b)));ui_dialog_selection_witness_drop(h(b));balanced();
}
int main(void){jet_rt_alloc_init();tracking=true;const H lengths[]={0,1,11};for(size_t i=0;i<sizeof lengths/sizeof lengths[0];++i){cases(lengths[i],0);cases(lengths[i],7);}ui_dialog_selection_witness_drop(0);balanced();puts("UI DialogSelection: inline ordered paths, every field, independent text clones and exact-capacity frees ok");return 0;}
