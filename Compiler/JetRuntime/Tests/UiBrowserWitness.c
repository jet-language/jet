/* Real Jet providers, no substitute implementation. Wrap allocation/free. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len,cap; } Text;
typedef struct { Text *kind; } Error;
typedef struct { Text *name,*version; } Profile;
_Static_assert(sizeof(Text)==24&&sizeof(Error)==8&&sizeof(Profile)==16,"native private ABI");
extern void jet_rt_alloc_init(void);
extern H __real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern void jet_rt_string_drop(H);
extern H ui_browser_error_witness_new(H),jet_rt_handle_clone_jet_browser_error(H);
extern void jet_rt_handle_drop_jet_browser_error(H);
extern bool jet_rt_handle_eq_jet_browser_error(H,H);
extern H jet_rt_handle_show_jet_browser_error(H),jet_rt_handle_debug_jet_browser_error(H),jet_rt_handle_display_jet_browser_error(H);
extern H jet_browser_profile(H,H,H),jet_browser_timeout(H,H,H);
extern H jet_rt_handle_clone_jet_browser_profile(H),jet_rt_handle_clone_jet_browser_timeout(H);
extern bool jet_rt_handle_eq_jet_browser_profile(H,H),jet_rt_handle_eq_jet_browser_timeout(H,H);
extern void jet_rt_handle_drop_jet_browser_profile(H),jet_rt_handle_drop_jet_browser_timeout(H);
static struct { H address,bytes,align; bool live; } ledger[128];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H address=__real_jet_rt_alloc(bytes,align);if(tracking){assert(address&&bytes>0&&allocations<128&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return address;}
void __wrap_jet_rt_free(H address,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked/static/double free");}__real_jet_rt_free(address,bytes,align);}
static H handle(const void *p){return (H)(uintptr_t)p;}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static void text_eq(const Text *text,const char *expected){size_t len=strlen(expected);assert(text&&text->len==len&&(!len||memcmp(text->data,expected,len)==0));}
static const struct { const char *kind,*message; } cases[]={
 {"invalid profile","browser profile is not supported"},
 {"invalid timeout","browser timeout must be between 1 and 600000 milliseconds"},
 {"unsupported protocol","browser protocol is not available in this session"},
 {"timeout","browser operation timed out"},
 {"closed","browser session is closed"},
 {"transport","browser transport failed"},
 {"missing lock","no locked browser for this engine in .jet/lock"},
 {"unknown engine","browser engine is not supported"},
 {"missing binary","locked browser binary was not found"},
 {"size mismatch","locked browser binary size drifted"},
 {"invalid lock","browser lock entry is incomplete"},
 {"","browser protocol failed"},
 {"unknown","browser protocol failed"},
 {"é","browser protocol failed"}
};
static Text views[sizeof cases/sizeof cases[0]],peers[sizeof cases/sizeof cases[0]];
static H (*const render[3])(H)={jet_rt_handle_show_jet_browser_error,jet_rt_handle_debug_jet_browser_error,jet_rt_handle_display_jet_browser_error};
static void error_case(size_t n){
    Error *a=(Error *)(uintptr_t)ui_browser_error_witness_new(handle(&views[n]));
    assert(allocations==1&&a->kind==&views[n]);
    Error *b=(Error *)(uintptr_t)jet_rt_handle_clone_jet_browser_error(handle(a));
    assert(allocations==2&&a!=b&&a->kind==b->kind&&jet_rt_handle_eq_jet_browser_error(handle(a),handle(b)));
    Error *peer=(Error *)(uintptr_t)ui_browser_error_witness_new(handle(&peers[n]));
    assert(a->kind!=peer->kind&&jet_rt_handle_eq_jet_browser_error(handle(a),handle(peer)));
    Text *saved=b->kind;b->kind=&views[(n+1)%(sizeof cases/sizeof cases[0])];assert(!jet_rt_handle_eq_jet_browser_error(handle(b),handle(peer)));b->kind=saved;
    for(size_t m=0;m<3;++m){size_t before=allocations;Text *text=(Text *)(uintptr_t)render[m](handle(a));assert(allocations==before+2);text_eq(text,cases[n].message);assert(text!=a->kind&&text->data!=a->kind->data);jet_rt_string_drop(handle(text));}
    jet_rt_handle_drop_jet_browser_error(handle(a));
    assert(jet_rt_handle_eq_jet_browser_error(handle(b),handle(peer)));
    jet_rt_handle_drop_jet_browser_error(handle(b));jet_rt_handle_drop_jet_browser_error(handle(peer));balanced();
}
static const char *const names[]={"playwright-bidi-2025.5","playwright-bidi-2024.11","","unknown","PLAYWRIGHT-bidi-2025.5","playwright-bidi-2025.5 ","playwright-bidi-2025.5extra","playwright-bidi-2024.11/","é"};
static Text name_views[sizeof names/sizeof names[0]];
static void profile_case(size_t n){
    H ok=INT64_C(0x1111),err=INT64_C(0x2222);
    H tag=jet_browser_profile(handle(&name_views[n]),handle(&ok),handle(&err));
    if(n<2){
        assert(tag==1&&err==INT64_C(0x2222)&&ok!=INT64_C(0x1111));
        Profile *a=(Profile *)(uintptr_t)ok;assert(a->name!=&name_views[n]);text_eq(a->name,names[n]);text_eq(a->version,n==0?"2025.5":"2024.11");
        Profile *b=(Profile *)(uintptr_t)jet_rt_handle_clone_jet_browser_profile(ok);
        assert(a!=b&&a->name!=b->name&&a->version!=b->version&&a->name->data!=b->name->data&&jet_rt_handle_eq_jet_browser_profile(ok,handle(b)));
        b->name->data[0]^=1;assert(!jet_rt_handle_eq_jet_browser_profile(ok,handle(b)));b->name->data[0]^=1;
        b->version->data[0]^=1;assert(!jet_rt_handle_eq_jet_browser_profile(ok,handle(b)));b->version->data[0]^=1;
        jet_rt_handle_drop_jet_browser_profile(ok);assert(jet_rt_handle_eq_jet_browser_profile(handle(b),handle(b)));jet_rt_handle_drop_jet_browser_profile(handle(b));
    }else{
        assert(tag==0&&ok==INT64_C(0x1111)&&err!=INT64_C(0x2222)&&allocations==1);
        Error *a=(Error *)(uintptr_t)err;text_eq(a->kind,"invalid profile");assert(a->kind->cap==0);
        Error *b=(Error *)(uintptr_t)jet_rt_handle_clone_jet_browser_error(err);assert(a->kind==b->kind&&allocations==2);
        Text *text=(Text *)(uintptr_t)jet_rt_handle_show_jet_browser_error(err);text_eq(text,cases[0].message);jet_rt_string_drop(handle(text));jet_rt_handle_drop_jet_browser_error(err);jet_rt_handle_drop_jet_browser_error(handle(b));
    }
    balanced();
}
static void timeout_case(H value){
    H ok=INT64_C(0x3333),err=INT64_C(0x4444);H tag=jet_browser_timeout(value,handle(&ok),handle(&err));
    if(value>=1&&value<=600000){
        assert(tag==1&&err==INT64_C(0x4444)&&ok!=INT64_C(0x3333)&&allocations==1);H *a=(H *)(uintptr_t)ok;assert(*a==value);
        H *b=(H *)(uintptr_t)jet_rt_handle_clone_jet_browser_timeout(ok);assert(a!=b&&*b==value&&jet_rt_handle_eq_jet_browser_timeout(ok,handle(b)));*b^=1;assert(!jet_rt_handle_eq_jet_browser_timeout(ok,handle(b)));*b^=1;
        jet_rt_handle_drop_jet_browser_timeout(ok);jet_rt_handle_drop_jet_browser_timeout(handle(b));
    }else{
        assert(tag==0&&ok==INT64_C(0x3333)&&err!=INT64_C(0x4444)&&allocations==1);Error *a=(Error *)(uintptr_t)err;text_eq(a->kind,"invalid timeout");assert(a->kind->cap==0);
        Error *b=(Error *)(uintptr_t)jet_rt_handle_clone_jet_browser_error(err);assert(a->kind==b->kind&&allocations==2);Text *text=(Text *)(uintptr_t)jet_rt_handle_show_jet_browser_error(err);text_eq(text,cases[1].message);jet_rt_string_drop(handle(text));jet_rt_handle_drop_jet_browser_error(err);jet_rt_handle_drop_jet_browser_error(handle(b));
    }
    balanced();
}
int main(void){
    jet_rt_alloc_init();
    for(size_t n=0;n<sizeof cases/sizeof cases[0];++n){views[n]=(Text){(unsigned char *)cases[n].kind,strlen(cases[n].kind),0};peers[n]=views[n];}
    for(size_t n=0;n<sizeof names/sizeof names[0];++n)name_views[n]=(Text){(unsigned char *)names[n],strlen(names[n]),0};
    tracking=true;for(size_t n=0;n<sizeof cases/sizeof cases[0];++n)error_case(n);
    for(size_t n=0;n<sizeof names/sizeof names[0];++n)profile_case(n);
    const H values[]={INT64_MIN,-1,0,1,2,599999,600000,600001,INT64_MAX};for(size_t n=0;n<sizeof values/sizeof values[0];++n)timeout_case(values[n]);
    jet_rt_handle_drop_jet_browser_error(0);balanced();
    puts("UI Browser: real profile/timeout producers, selected-out identity, static kind borrows, all render modes and exact frees ok");return 0;
}
