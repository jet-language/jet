/* Real Jet providers, checked-heap alloc/free observers, no C providers.
 * Cursor3 reuses febd2658e canonical inline E4+pos, never a second provider. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {Text buf;uint64_t pos;} Cursor;
_Static_assert(sizeof(Cursor)==32,"native-private layout");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern H jet_rt_handle_clone_jet_cursor(H);
extern void jet_rt_handle_drop_jet_cursor(H),jet_rt_handle_field_jet_cursor_buf(H,Text *);
static struct {H address,bytes,align;bool live;} ledger[1024];
static size_t allocations,frees;
static bool tracking;
static void observe_alloc(H address, H bytes, H align) {
    if(tracking){assert(address&&bytes>0&&allocations<1024&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}
}
static void observe_free(H address,H bytes,H align) {
    if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;return;}assert(!"untracked or double free");}
}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static void text_is(Text *t,const char *s){assert(t->len==strlen(s)&&!memcmp(t->data,s,t->len));}
static void cursor(const char *s,uint64_t pos) {
    Cursor *a=(Cursor *)(uintptr_t)jet_rt_alloc(sizeof *a,8);size_t n=strlen(s);a->buf.len=a->buf.cap=n;a->buf.data=n?(unsigned char *)(uintptr_t)jet_rt_alloc((H)n,1):(unsigned char *)(uintptr_t)1;a->pos=pos;if(n)memcpy(a->buf.data,s,n);
    H ah=(H)(uintptr_t)a;Cursor *b=(Cursor *)(uintptr_t)jet_rt_handle_clone_jet_cursor(ah);H bh=(H)(uintptr_t)b;assert(a!=b&&b->pos==pos);text_is(&b->buf,s);if(n)assert(a->buf.data!=b->buf.data);
    size_t before=allocations;Text view={0};jet_rt_handle_field_jet_cursor_buf(ah,&view);assert(view.data==a->buf.data&&view.len==a->buf.len&&view.cap==a->buf.cap&&allocations==before);
    jet_rt_handle_drop_jet_cursor(ah);text_is(&b->buf,s);jet_rt_handle_drop_jet_cursor(bh);balanced();
}
int main(void){assert(jet_rt_alloc_check_observe((int64_t)(uintptr_t)observe_alloc, (int64_t)(uintptr_t)observe_free) == 1);jet_rt_alloc_init();tracking=true;cursor("",UINT64_MAX);cursor("a\xc3\xa9",UINT64_C(0x8000000000000001));jet_rt_handle_drop_jet_cursor(0);balanced();puts("UI canonical Cursor: raw fields, projections and exact-once frees ok");return 0;}
