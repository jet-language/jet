/* Actual Jet helpers and --wrap alloc/free ownership ledger. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {Text *path,*root;H access;} Path;
typedef struct {H kind;Path payload;} Item;
typedef struct {H len,cap;Item *data;} List;
typedef struct {Text *target;H phase,operation;List *items;} Event;
_Static_assert(sizeof(Item)==32 && sizeof(Event)==32 && sizeof(List)==24,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_drag_witness_clone(H);
extern void ui_drag_witness_drop(H);
extern bool ui_drag_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[512];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<512&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H h(const void *p){return (H)(uintptr_t)p;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static Event *event(H phase,H operation,H n,H first,bool empty){
    Event *e=(Event *)(uintptr_t)jet_rt_alloc(sizeof *e,8);e->target=text("node\xc3\xa9");e->phase=phase;e->operation=operation;e->items=(List *)(uintptr_t)jet_rt_alloc(sizeof *e->items,8);List *list=e->items;list->len=n;list->cap=n+7;list->data=(Item *)(uintptr_t)jet_rt_alloc(list->cap*(H)sizeof(Item),8);
    for(H i=0;i<n;++i){Item *item=&list->data[i];item->kind=(first+i)%3;item->payload.path=text(empty?"":item->kind==0?"text":item->kind==1?"uri":"/grant/file");item->payload.root=item->kind==2?text(empty?"":"/grant"):(Text *)(uintptr_t)UINT64_C(0xa5a5a5a5a5a5a5a5);item->payload.access=item->kind==2?i%2:INT64_MIN;}
    return e;
}
static void different(Text *t,H a,H b){if(t->len){t->data[0]^=1;assert(!ui_drag_witness_eq(a,b));t->data[0]^=1;}else{t->len=1;assert(!ui_drag_witness_eq(a,b));t->len=0;}}
static void cases(H phase,H operation,H n,H first,bool empty){
    Event *a=event(phase,operation,n,first,empty),*b=(Event *)(uintptr_t)ui_drag_witness_clone(h(a));assert(a!=b&&a->target!=b->target&&a->target->data!=b->target->data&&a->items!=b->items&&b->items->len==n&&b->items->cap==n&&ui_drag_witness_eq(h(a),h(b)));assert(!ui_drag_witness_eq(0,h(b))&&!ui_drag_witness_eq(h(b),0));different(b->target,h(a),h(b));b->phase=(phase+1)%5;assert(!ui_drag_witness_eq(h(a),h(b)));b->phase=phase;b->operation=(operation+1)%3;assert(!ui_drag_witness_eq(h(a),h(b)));b->operation=operation;
    for(H i=0;i<n;++i){Item *x=&a->items->data[i],*y=&b->items->data[i];assert(x!=y&&x->kind==y->kind&&x->payload.path!=y->payload.path);if(x->payload.path->len)assert(x->payload.path->data!=y->payload.path->data);different(y->payload.path,h(a),h(b));H kind=y->kind;y->kind=(kind+1)%3;assert(!ui_drag_witness_eq(h(a),h(b)));y->kind=kind;if(kind==2){assert(x->payload.root!=y->payload.root);different(y->payload.root,h(a),h(b));y->payload.access^=1;assert(!ui_drag_witness_eq(h(a),h(b)));y->payload.access^=1;}else{assert(y->payload.root==NULL&&y->payload.access==0);y->payload.root=(Text *)(uintptr_t)UINT64_MAX;y->payload.access=INT64_MAX;assert(ui_drag_witness_eq(h(a),h(b)));y->payload.root=NULL;y->payload.access=0;}}
    if(n){--b->items->len;assert(!ui_drag_witness_eq(h(a),h(b)));++b->items->len;}if(n>1){Item tmp=b->items->data[0];b->items->data[0]=b->items->data[1];b->items->data[1]=tmp;assert(!ui_drag_witness_eq(h(a),h(b)));tmp=b->items->data[0];b->items->data[0]=b->items->data[1];b->items->data[1]=tmp;}
    ui_drag_witness_drop(h(a));assert(ui_drag_witness_eq(h(b),h(b)));ui_drag_witness_drop(h(b));balanced();
}
int main(int argc,char **argv){jet_rt_alloc_init();if(argc==2){Event *e=event(0,0,1,0,false);if(!strncmp(argv[1],"phase-",6))e->phase=5;else if(!strncmp(argv[1],"operation-",10))e->operation=3;else if(!strncmp(argv[1],"item-",5))e->items->data[0].kind=3;else return 2;if(strstr(argv[1],"clone-tag"))ui_drag_witness_clone(h(e));else if(strstr(argv[1],"drop-tag"))ui_drag_witness_drop(h(e));else if(strstr(argv[1],"eq-tag"))ui_drag_witness_eq(h(e),h(e));else return 2;return 99;}if(argc!=1)return 2;tracking=true;assert(ui_drag_witness_clone(0)==0&&ui_drag_witness_eq(0,0));const H lengths[]={0,1,11};for(H phase=0;phase<5;++phase)for(H operation=0;operation<3;++operation)for(size_t n=0;n<sizeof lengths/sizeof lengths[0];++n)for(H first=0;first<3;++first)for(unsigned empty=0;empty<2;++empty)cases(phase,operation,lengths[n],first,empty);ui_drag_witness_drop(0);balanced();puts("UI Drag: all phases/operations/items, ordered inline payloads, independent owners and exact-once frees ok");return 0;}
