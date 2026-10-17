/* Link actual Jet success helpers; --wrap alloc/free for exact ownership. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {uint64_t start,end;} Range;
typedef struct {Text *text;Range selection;Range *marked;} Composition;
typedef struct {Text *target;H phase;Composition *composition;} Event;
_Static_assert(sizeof(Composition)==32 && sizeof(Event)==24,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_ime_witness_clone(H);
extern void ui_ime_witness_drop(H);
extern bool ui_ime_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H h(const void *p){return (H)(uintptr_t)p;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static void different(Text *t,H a,H b){if(t->len){t->data[0]^=1;assert(!ui_ime_witness_eq(a,b));t->data[0]^=1;}else{t->len=1;assert(!ui_ime_witness_eq(a,b));t->len=0;}}
static void range_mutations(Range *r,H a,H b){r->start^=1;assert(!ui_ime_witness_eq(a,b));r->start^=1;r->end^=1;assert(!ui_ime_witness_eq(a,b));r->end^=1;}
static void cases(H phase,unsigned composition,unsigned marked,uint64_t start,uint64_t end){
    Event *a=(Event *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->target=text("node");a->phase=phase;a->composition=NULL;
    if(composition){Composition *c=(Composition *)(uintptr_t)jet_rt_alloc(sizeof *c,8);a->composition=c;c->text=text(composition==1?"":"composing\xc3\xa9");c->selection.start=start;c->selection.end=end;c->marked=NULL;if(marked){c->marked=(Range *)(uintptr_t)jet_rt_alloc(sizeof *c->marked,8);c->marked->start=marked==1?0:start;c->marked->end=marked==1?0:end;}}
    Event *b=(Event *)(uintptr_t)ui_ime_witness_clone(h(a));assert(a!=b&&a->target!=b->target&&a->target->data!=b->target->data&&ui_ime_witness_eq(h(a),h(b)));assert(!ui_ime_witness_eq(0,h(b))&&!ui_ime_witness_eq(h(b),0));different(b->target,h(a),h(b));b->phase=(phase+1)%4;assert(!ui_ime_witness_eq(h(a),h(b)));b->phase=phase;
    if(composition){Composition *x=a->composition,*y=b->composition;assert(x!=y&&x->text!=y->text&&y->selection.start==start&&y->selection.end==end);different(y->text,h(a),h(b));range_mutations(&y->selection,h(a),h(b));Composition *saved=b->composition;b->composition=NULL;assert(!ui_ime_witness_eq(h(a),h(b)));b->composition=saved;
        if(marked){assert(x->marked!=y->marked&&x->marked->start==y->marked->start&&x->marked->end==y->marked->end);range_mutations(y->marked,h(a),h(b));Range *r=y->marked;y->marked=NULL;assert(!ui_ime_witness_eq(h(a),h(b)));y->marked=r;}else{Range zero={0};y->marked=&zero;assert(!ui_ime_witness_eq(h(a),h(b)));y->marked=NULL;}
    }else{Composition empty={0};Text zero={0};empty.text=&zero;b->composition=&empty;assert(!ui_ime_witness_eq(h(a),h(b)));b->composition=NULL;}
    ui_ime_witness_drop(h(a));assert(ui_ime_witness_eq(h(b),h(b)));ui_ime_witness_drop(h(b));balanced();
}
int main(int argc,char **argv){jet_rt_alloc_init();if(argc==2){Event *e=(Event *)(uintptr_t)jet_rt_alloc(sizeof *e,8);memset(e,0xa5,sizeof *e);e->phase=4;if(!strcmp(argv[1],"clone-tag"))ui_ime_witness_clone(h(e));else if(!strcmp(argv[1],"drop-tag"))ui_ime_witness_drop(h(e));else if(!strcmp(argv[1],"eq-tag"))ui_ime_witness_eq(h(e),h(e));else return 2;return 99;}if(argc!=1)return 2;tracking=true;const uint64_t words[]={0,1,INT64_MAX,UINT64_C(0x8000000000000000),UINT64_MAX};assert(ui_ime_witness_clone(0)==0&&ui_ime_witness_eq(0,0));for(H phase=0;phase<4;++phase){cases(phase,0,0,0,0);for(unsigned composition=1;composition<3;++composition)for(unsigned marked=0;marked<3;++marked)for(size_t i=0;i<sizeof words/sizeof words[0];++i)for(size_t j=0;j<sizeof words/sizeof words[0];++j)cases(phase,composition,marked,words[i],words[j]);}ui_ime_witness_drop(0);balanced();puts("UI IME: all phases, nullable compositions/ranges, raw usize extremes, independent owners and exact-once frees ok");return 0;}
