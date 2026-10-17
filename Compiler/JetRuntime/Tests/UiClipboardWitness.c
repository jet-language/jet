/* Link real Jet providers and test-only doors; wrap alloc/free for exact ownership. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {uint64_t start,end;} Range;
typedef struct {Text *text;Range *selection;} Clipboard;
typedef struct {uint64_t characters;} Write;
_Static_assert(sizeof(Range)==16 && sizeof(Clipboard)==16 && sizeof(Write)==8,"private ABI");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_clipboard_text_witness_clone(H),ui_clipboard_write_witness_clone(H);
extern void ui_clipboard_text_witness_drop(H),ui_clipboard_write_witness_drop(H);
extern bool ui_clipboard_text_witness_eq(H,H),ui_clipboard_write_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H handle(const void *p){return (H)(uintptr_t)p;}
static void clipboard(bool present,uint64_t start,uint64_t end,const char *content){
    Clipboard *a=(Clipboard *)(uintptr_t)jet_rt_alloc(sizeof *a,8);
    a->text=jet_rt_string_from_static(content,(H)strlen(content));a->selection=NULL;
    if(present){a->selection=(Range *)(uintptr_t)jet_rt_alloc(sizeof *a->selection,8);a->selection->start=start;a->selection->end=end;}
    Clipboard *b=(Clipboard *)(uintptr_t)ui_clipboard_text_witness_clone(handle(a));
    assert(a!=b&&a->text!=b->text&&ui_clipboard_text_witness_eq(handle(a),handle(b)));
    assert((b->selection!=NULL)==present);
    if(a->text->len){assert(a->text->data!=b->text->data);b->text->data[0]^=1;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->text->data[0]^=1;}
    else {b->text->len=1;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->text->len=0;}
    if(present){assert(a->selection!=b->selection&&b->selection->start==start&&b->selection->end==end);b->selection->start^=1;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->selection->start^=1;b->selection->end^=1;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->selection->end^=1;Range *saved=b->selection;b->selection=NULL;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->selection=saved;}
    else {Range zero={0,0};b->selection=&zero;assert(!ui_clipboard_text_witness_eq(handle(a),handle(b)));b->selection=NULL;}
    ui_clipboard_text_witness_drop(handle(a));assert(ui_clipboard_text_witness_eq(handle(b),handle(b)));ui_clipboard_text_witness_drop(handle(b));balanced();
}
static void receipt(uint64_t characters){Write *a=(Write *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->characters=characters;size_t before=allocations;Write *b=(Write *)(uintptr_t)ui_clipboard_write_witness_clone(handle(a));assert(allocations==before+1&&a!=b&&b->characters==characters&&ui_clipboard_write_witness_eq(handle(a),handle(b)));b->characters^=1;assert(!ui_clipboard_write_witness_eq(handle(a),handle(b)));b->characters^=1;ui_clipboard_write_witness_drop(handle(a));assert(ui_clipboard_write_witness_eq(handle(b),handle(b)));ui_clipboard_write_witness_drop(handle(b));balanced();}
int main(void){jet_rt_alloc_init();tracking=true;const uint64_t words[]={0,1,INT64_MAX,UINT64_C(0x8000000000000000),UINT64_MAX};for(size_t i=0;i<sizeof words/sizeof words[0];++i){receipt(words[i]);for(size_t j=0;j<sizeof words/sizeof words[0];++j){clipboard(true,words[i],words[j],"owned\xc3\xa9");clipboard(true,words[i],words[j],"");}}clipboard(false,0,0,"owned");clipboard(false,0,0,"");ui_clipboard_text_witness_drop(0);ui_clipboard_write_witness_drop(0);balanced();puts("UI Clipboard: nullable ranges, raw usize extremes, independent text clones and exact-once frees ok");return 0;}
