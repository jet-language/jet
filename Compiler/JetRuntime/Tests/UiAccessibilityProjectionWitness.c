/* Real Jet helper doors and alloc/free ledger; inline metadata owns no temporary carrier. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {H kind,payload;} State;
typedef struct {Text *name,*description;H len,cap;State *data;} Metadata;
typedef struct {Text *node;Metadata metadata;} Projection;
_Static_assert(sizeof(Metadata)==40 && sizeof(Projection)==48,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_projection_witness_clone(H);
extern void ui_projection_witness_drop(H);
extern bool ui_projection_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H h(const void *p){return (H)(uintptr_t)p;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static void different(Text *t,H a,H b){if(t->len){t->data[0]^=1;assert(!ui_projection_witness_eq(a,b));t->data[0]^=1;}else{t->len=1;assert(!ui_projection_witness_eq(a,b));t->len=0;}}
static void optional(Text **field,H a,H b){Text *saved=*field;if(saved){*field=NULL;assert(!ui_projection_witness_eq(a,b));*field=saved;different(saved,a,b);}else{Text zero={0};*field=&zero;assert(!ui_projection_witness_eq(a,b));*field=NULL;}}
static void cases(unsigned name,unsigned description,H n,uint64_t flag){
    Projection *a=(Projection *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->node=text("node\xc3\xa9");Metadata *m=&a->metadata;m->name=name?text(name==1?"":"name"):NULL;m->description=description?text(description==1?"":"description"):NULL;m->len=n;m->cap=n+3;m->data=(State *)(uintptr_t)jet_rt_alloc(m->cap*(H)sizeof(State),8);
    for(H i=0;i<n;++i){m->data[i].kind=i%7;m->data[i].payload=i%7==6?h(text("value")):(H)(UINT64_C(0xdeadbeef00000000)|flag);}
    Projection *b=(Projection *)(uintptr_t)ui_projection_witness_clone(h(a));Metadata *z=&b->metadata;assert(a!=b&&a->node!=b->node&&ui_projection_witness_eq(h(a),h(b)));assert(z->len==n&&z->cap==n);assert(!ui_projection_witness_eq(0,h(b))&&!ui_projection_witness_eq(h(b),0));different(b->node,h(a),h(b));optional(&z->name,h(a),h(b));optional(&z->description,h(a),h(b));if(m->name)assert(m->name!=z->name);if(m->description)assert(m->description!=z->description);
    for(H i=0;i<n;++i){State *x=&m->data[i],*y=&z->data[i];assert(x!=y&&x->kind==y->kind);if(y->kind==6){assert(x->payload!=y->payload);different((Text *)(uintptr_t)y->payload,h(a),h(b));}else if(y->kind==2||y->kind==3||y->kind==4){assert(y->payload==(flag!=0));y->payload^=1;assert(!ui_projection_witness_eq(h(a),h(b)));y->payload^=1;}else{assert(y->payload==0);y->payload=INT64_MIN;assert(ui_projection_witness_eq(h(a),h(b)));y->payload=0;}H saved=y->kind;y->kind=(saved+1)%7;assert(!ui_projection_witness_eq(h(a),h(b)));y->kind=saved;}
    if(n){--z->len;assert(!ui_projection_witness_eq(h(a),h(b)));++z->len;}if(n>1){State tmp=z->data[0];z->data[0]=z->data[1];z->data[1]=tmp;assert(!ui_projection_witness_eq(h(a),h(b)));tmp=z->data[0];z->data[0]=z->data[1];z->data[1]=tmp;}
    ui_projection_witness_drop(h(a));assert(ui_projection_witness_eq(h(b),h(b)));ui_projection_witness_drop(h(b));balanced();
}
int main(void){jet_rt_alloc_init();tracking=true;assert(ui_projection_witness_clone(0)==0&&ui_projection_witness_eq(0,0));for(unsigned name=0;name<3;++name)for(unsigned description=0;description<3;++description){cases(name,description,0,0);for(uint64_t flag=0;flag<256;++flag)cases(name,description,14,flag);}ui_projection_witness_drop(0);balanced();puts("UI AccessibilityProjection: NodeId identity, all metadata options/states, independent owners and exact-once frees ok");return 0;}
