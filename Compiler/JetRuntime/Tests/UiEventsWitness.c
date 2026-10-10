/* Real Jet runtime providers, --wrap=jet_rt_alloc/jet_rt_free; no C providers.
 * Canonical native List<U8> is packed. Compose shared sequence stop fixes. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
#define DATA_TREE_U8_STRIDE 1
_Static_assert(DATA_TREE_U8_STRIDE==1,"packed native U8 storage");
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len,cap; } Text;
typedef struct { uint64_t len,cap; unsigned char *data; } Bytes;
typedef struct { H kind,first,second; } Event;
_Static_assert(sizeof(Event)==24,"UiABI Event24");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern H jet_rt_handle_clone_jet_input_event(H),jet_rt_handle_clone_jet_tui_event(H);
extern Text *jet_rt_string_from_static(const void *,H);
extern void jet_rt_handle_drop_jet_input_event(H),jet_rt_handle_drop_jet_tui_event(H);
extern bool jet_rt_handle_eq_jet_input_event(H,H),jet_rt_handle_eq_jet_tui_event(H,H);
static struct { H address,bytes,align; bool live; } ledger[512];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking) {
        assert(address&&bytes>0&&allocations<512&&(uint64_t)address%(uint64_t)align==0);
        for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);
        ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;
    }
    return address;
}
void __wrap_jet_rt_free(H address,H bytes,H align) {
    if(tracking) {
        for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}
        assert(!"untracked or double free");
    }
    __real_jet_rt_free(address,bytes,align);
}
static void balanced(void) {assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H text(const char *s) {return (H)(uintptr_t)jet_rt_string_from_static(s,(H)strlen(s));}
static Bytes *bytes(size_t n) {
    Bytes *b=(Bytes *)(uintptr_t)jet_rt_alloc(24,8);b->len=n;b->cap=n+3;
    b->data=(unsigned char *)(uintptr_t)jet_rt_alloc((H)(b->cap*DATA_TREE_U8_STRIDE),8);
    memset(b->data,0xa5,b->cap*DATA_TREE_U8_STRIDE);
    for(size_t i=0;i<n;++i)b->data[i*DATA_TREE_U8_STRIDE]=(unsigned char)(i*73);
    return b;
}
static Event *event(bool tui,H kind,size_t n) {
    Event *e=(Event *)(uintptr_t)jet_rt_alloc(sizeof *e,8);e->kind=kind;e->first=INT64_MIN;e->second=INT64_MAX;
    if(kind==0)e->first=text("k\xc3\xa9y");
    if(kind==1){double x=12.5,y=-30.25;memcpy(&e->first,&x,8);memcpy(&e->second,&y,8);}
    if(tui&&kind==0)e->second=(H)UINT64_C(0xdeadbeef000000ff);
    if(tui&&kind==2){e->first=text("timer");e->second=INT64_MIN;}
    if(tui&&kind==3){e->first=text("io");e->second=(H)(uintptr_t)bytes(n);}
    if(tui&&kind==4)e->first=(H)UINT64_C(0xdeadbeef00000001);
    return e;
}
static void cases(bool tui,H kind,size_t n) {
    H (*clone)(H)=tui?jet_rt_handle_clone_jet_tui_event:jet_rt_handle_clone_jet_input_event;
    void (*drop)(H)=tui?jet_rt_handle_drop_jet_tui_event:jet_rt_handle_drop_jet_input_event;
    bool (*eq)(H,H)=tui?jet_rt_handle_eq_jet_tui_event:jet_rt_handle_eq_jet_input_event;
    Event *a=event(tui,kind,n),*b=(Event *)(uintptr_t)clone((H)(uintptr_t)a);H ah=(H)(uintptr_t)a,bh=(H)(uintptr_t)b;
    assert(a!=b&&eq(ah,bh));
    if(kind==0||(tui&&(kind==2||kind==3))) {
        Text *x=(Text *)(uintptr_t)a->first,*y=(Text *)(uintptr_t)b->first;assert(x!=y&&x->data!=y->data);
        unsigned char old=x->data[0];x->data[0]^=1;assert(!eq(ah,bh));x->data[0]=old;
    }
    if(kind==1) {
        H old=a->first;a->first=(H)UINT64_C(0x7ff8000000000001);assert(!eq(ah,ah)&&!eq(ah,bh));a->first=old;
        a->first=0;b->first=INT64_MIN;assert(eq(ah,bh));
        b->second^=1;assert(!eq(ah,bh));b->second^=1;
    }
    if(tui&&kind==0){assert(b->second==255);a->second^=1;assert(!eq(ah,bh));a->second^=1;}
    if(tui&&kind==2){assert(b->second==INT64_MIN);a->second=INT64_MAX;assert(!eq(ah,bh));a->second=INT64_MIN;}
    if(tui&&kind==3) {
        Bytes *x=(Bytes *)(uintptr_t)a->second,*y=(Bytes *)(uintptr_t)b->second;assert(x!=y&&x->len==y->len&&y->cap==n);
        if(n){assert(x->data!=y->data);for(size_t i=0;i<n;++i)assert(x->data[i*DATA_TREE_U8_STRIDE]==y->data[i*DATA_TREE_U8_STRIDE]);x->data[(n-1)*DATA_TREE_U8_STRIDE]^=1;assert(!eq(ah,bh));x->data[(n-1)*DATA_TREE_U8_STRIDE]^=1;--x->len;assert(!eq(ah,bh));++x->len;}
    }
    if(tui&&kind==4){assert(b->first==1);a->first^=1;assert(!eq(ah,bh));a->first^=1;}
    if(tui&&kind>=5){a->first^=1;a->second^=1;assert(eq(ah,bh));}
    b->kind=kind+1;assert(!eq(ah,bh));b->kind=kind;
    drop(ah);assert(eq(bh,bh));drop(bh);balanced();
}
int main(void) {
    jet_rt_alloc_init();tracking=true;
    for(H kind=0;kind<2;++kind)cases(false,kind,0);
    for(H kind=0;kind<7;++kind)cases(true,kind,0);
    cases(true,3,1);cases(true,3,17);cases(true,3,257);
    jet_rt_handle_drop_jet_input_event(0);jet_rt_handle_drop_jet_tui_event(0);balanced();
    puts("UI events: all variants, IEEE size equality, raw fields, packed IO ownership and exact-once frees ok");return 0;
}
