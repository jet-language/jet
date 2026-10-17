/* Real Jet helper doors + --wrap=jet_rt_alloc/jet_rt_free. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {Text *key;uint64_t modifiers;Text *action,*node;} Binding;
typedef struct {H kind;Binding binding;} Dispatch;
_Static_assert(sizeof(Binding)==32 && sizeof(Dispatch)==40,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_binding_witness_clone(H),ui_dispatch_witness_clone(H);
extern void ui_binding_witness_drop(H),ui_dispatch_witness_drop(H);
extern bool ui_binding_witness_eq(H,H),ui_dispatch_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H h(const void *p){return (H)(uintptr_t)p;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static void initialize(Binding *b,unsigned node,uint64_t modifiers){b->key=text("key\xc3\xa9");b->modifiers=UINT64_C(0xdeadbeef00000000)|modifiers;b->action=text("action");b->node=node?text(node==1?"":"node"):NULL;}
static void different_text(Text *t,H a,H b,bool (*eq)(H,H)){if(t->len){t->data[0]^=1;assert(!eq(a,b));t->data[0]^=1;}else{t->len=1;assert(!eq(a,b));t->len=0;}}
static void mutations(Binding *a,Binding *b,H ah,H bh,bool (*eq)(H,H)){
    assert(eq(ah,bh)&&a->key!=b->key&&a->action!=b->action&&b->modifiers==(a->modifiers&255));
    assert(a->key->data!=b->key->data&&a->action->data!=b->action->data);different_text(b->key,ah,bh,eq);different_text(b->action,ah,bh,eq);
    b->modifiers^=1;assert(!eq(ah,bh));b->modifiers^=1;b->modifiers|=UINT64_C(0x8000000000000000);assert(eq(ah,bh));b->modifiers&=255;
    if(a->node){assert(a->node!=b->node);if(a->node->len)assert(a->node->data!=b->node->data);different_text(b->node,ah,bh,eq);Text *saved=b->node;b->node=NULL;assert(!eq(ah,bh));b->node=saved;}
    else{Text zero={0};b->node=&zero;assert(!eq(ah,bh));b->node=NULL;}
}
static void cases(unsigned node,uint64_t modifiers){
    Binding *a=(Binding *)(uintptr_t)jet_rt_alloc(sizeof *a,8);initialize(a,node,modifiers);Binding *b=(Binding *)(uintptr_t)ui_binding_witness_clone(h(a));assert(a!=b);mutations(a,b,h(a),h(b),ui_binding_witness_eq);ui_binding_witness_drop(h(a));assert(ui_binding_witness_eq(h(b),h(b)));ui_binding_witness_drop(h(b));balanced();
    Dispatch *x=(Dispatch *)(uintptr_t)jet_rt_alloc(sizeof *x,8);x->kind=0;initialize(&x->binding,node,modifiers);Dispatch *y=(Dispatch *)(uintptr_t)ui_dispatch_witness_clone(h(x));assert(x!=y);mutations(&x->binding,&y->binding,h(x),h(y),ui_dispatch_witness_eq);y->kind=1;assert(!ui_dispatch_witness_eq(h(x),h(y)));y->kind=0;ui_dispatch_witness_drop(h(x));assert(ui_dispatch_witness_eq(h(y),h(y)));ui_dispatch_witness_drop(h(y));balanced();
}
int main(int argc,char **argv){jet_rt_alloc_init();if(argc==2){Dispatch *a=(Dispatch *)(uintptr_t)jet_rt_alloc(sizeof *a,8);memset(a,0xa5,sizeof *a);a->kind=2;if(!strcmp(argv[1],"clone-tag"))ui_dispatch_witness_clone(h(a));else if(!strcmp(argv[1],"drop-tag"))ui_dispatch_witness_drop(h(a));else if(!strcmp(argv[1],"eq-tag"))ui_dispatch_witness_eq(h(a),h(a));else return 2;return 99;}if(argc!=1)return 2;tracking=true;for(unsigned node=0;node<3;++node)for(uint64_t modifiers=0;modifiers<256;++modifiers)cases(node,modifiers);
    Dispatch *a=(Dispatch *)(uintptr_t)jet_rt_alloc(sizeof *a,8);memset(a,0xa5,sizeof *a);a->kind=1;size_t before=allocations;Dispatch *b=(Dispatch *)(uintptr_t)ui_dispatch_witness_clone(h(a));assert(a!=b&&allocations==before+1&&ui_dispatch_witness_eq(h(a),h(b)));assert(!b->binding.key&&!b->binding.modifiers&&!b->binding.action&&!b->binding.node);ui_dispatch_witness_drop(h(a));ui_dispatch_witness_drop(h(b));ui_binding_witness_drop(0);ui_dispatch_witness_drop(0);balanced();puts("UI ShortcutBinding: all modifiers, optional NodeId, both dispatch variants, independent clones and exact-once frees ok");return 0;}
