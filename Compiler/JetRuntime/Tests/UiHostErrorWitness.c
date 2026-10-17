/* Real Jet private helpers through witness-only Jet doors.
 * --wrap=jet_rt_alloc/jet_rt_free. Static E4 views are immutable rodata.
 * Bad tag modes clone/drop/eq use shared record-variant stop (2442d5d80). */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {H kind,first,second,third;} Error;
static const unsigned char service_bytes[]="static service";
static const unsigned char service_copy_bytes[]="static service";
static const unsigned char other_bytes[]="other service";
static const Text service={(unsigned char *)service_bytes,sizeof(service_bytes)-1,0};
static const Text service_copy={(unsigned char *)service_copy_bytes,sizeof(service_copy_bytes)-1,0};
static const Text other={(unsigned char *)other_bytes,sizeof(other_bytes)-1,0};
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H ui_host_error_witness_clone(H);
extern void ui_host_error_witness_drop(H);
extern bool ui_host_error_witness_eq(H,H);
static struct {H address,bytes,align;bool live;} ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}__real_jet_rt_free(address,bytes,align);}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H text(const char *s){return (H)(uintptr_t)jet_rt_string_from_static(s,(H)strlen(s));}
static Error *error(H kind,H ordinal){Error *e=(Error *)(uintptr_t)jet_rt_alloc(sizeof *e,8);e->kind=kind;e->first=ordinal;e->second=INT64_MIN;e->third=INT64_MAX;if(kind==1||kind==4||kind==7)e->first=(H)(uintptr_t)&service;if(kind==3||kind==5||kind==6)e->first=text("owned\xc3\xa9");if(kind==4)e->second=text("message");if(kind==6){e->second=(H)UINT64_C(0xdeadbeef000000ff);e->third=text("action");}return e;}
static void changed_text(H slot,H a,H b){Text *t=(Text *)(uintptr_t)slot;t->data[0]^=1;assert(!ui_host_error_witness_eq(a,b));t->data[0]^=1;}
static void cases(H kind,H ordinal){Error *a=error(kind,ordinal);H ah=(H)(uintptr_t)a;size_t before=allocations;Error *b=(Error *)(uintptr_t)ui_host_error_witness_clone(ah);H bh=(H)(uintptr_t)b;assert(a!=b&&ui_host_error_witness_eq(ah,bh));if(kind==0||kind==1||kind==2||kind==7)assert(allocations==before+1);
    if(kind==0||kind==2){b->first^=1;assert(!ui_host_error_witness_eq(ah,bh));b->first^=1;assert(b->second==0&&b->third==0);}
    if(kind==1||kind==4||kind==7){assert(b->first==a->first);b->first=(H)(uintptr_t)&service_copy;assert(ui_host_error_witness_eq(ah,bh));b->first=(H)(uintptr_t)&other;assert(!ui_host_error_witness_eq(ah,bh));b->first=(H)(uintptr_t)&service;}
    if(kind==3||kind==5||kind==6){assert(a->first!=b->first&&((Text *)(uintptr_t)a->first)->data!=((Text *)(uintptr_t)b->first)->data);changed_text(b->first,ah,bh);}
    if(kind==4){assert(a->second!=b->second);changed_text(b->second,ah,bh);}
    if(kind==6){assert(b->second==255&&a->third!=b->third);b->second^=1;assert(!ui_host_error_witness_eq(ah,bh));b->second^=1;changed_text(b->third,ah,bh);}
    b->kind=kind==0?2:0;assert(!ui_host_error_witness_eq(ah,bh));b->kind=kind;ui_host_error_witness_drop(ah);assert(ui_host_error_witness_eq(bh,bh));ui_host_error_witness_drop(bh);balanced();}
int main(int argc,char **argv){jet_rt_alloc_init();if(argc==2){Error *a=error(8,0);H h=(H)(uintptr_t)a;if(!strcmp(argv[1],"clone-tag"))ui_host_error_witness_clone(h);else if(!strcmp(argv[1],"drop-tag"))ui_host_error_witness_drop(h);else if(!strcmp(argv[1],"eq-tag"))ui_host_error_witness_eq(h,h);else return 2;return 99;}if(argc!=1)return 2;tracking=true;for(H i=0;i<5;++i)cases(0,i);for(H i=0;i<7;++i)cases(2,i);for(H kind=1;kind<8;++kind)if(kind!=2)cases(kind,0);ui_host_error_witness_drop(0);balanced();puts("UI HostError: eight seed variants, all nested enums, static borrows, inline shortcut and exact-once frees ok");return 0;}
