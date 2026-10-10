/* REAL Jet providers with --wrap=jet_rt_alloc/jet_rt_free.
 * EncodingABI8 only. Compose e1306aee0 + 2442d5d80 before native linking.
 * aria-tag stop: exit70, empty stdout, DataPlotStops-getter-variant.stderr. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len,cap; } Text;
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H);
extern H __real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern void jet_rt_string_drop(Text *);
extern H jet_rt_handle_clone_jet_ui_aria_role(H);
extern void jet_rt_handle_drop_jet_ui_aria_role(H);
extern bool jet_rt_handle_eq_jet_ui_aria_role(H,H);
extern Text *jet_rt_handle_show_jet_ui_aria_role(H);
extern H jet_rt_handle_clone_jet_web_form_validation_timing(H);
extern void jet_rt_handle_drop_jet_web_form_validation_timing(H);
extern bool jet_rt_handle_eq_jet_web_form_validation_timing(H,H);
static struct { H address,bytes,align; bool live; } ledger[256];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking) {
        assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);
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
static H choice(H ordinal) { H value=jet_rt_alloc(8,8);*(H *)(uintptr_t)value=ordinal;return value; }
static void cases(H (*clone)(H),void (*drop)(H),bool (*eq)(H,H),H count,bool aria) {
    static const char *labels[]={"Button","TextInput","Label","Container"};
    for(H i=0;i<count;++i) {
        H a=choice(i),b=clone(a);assert(a!=b&&eq(a,b));
        for(H j=0;j<count;++j){H other=choice(j);assert(eq(a,other)==(i==j));drop(other);}
        if(aria){Text *rendered=jet_rt_handle_show_jet_ui_aria_role(b);assert(rendered->len==strlen(labels[i])&&!memcmp(rendered->data,labels[i],rendered->len));jet_rt_string_drop(rendered);}
        drop(a);assert(*(H *)(uintptr_t)b==i);drop(b);
    }
    drop(0);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2){if(strcmp(argv[1],"aria-tag"))return 2;jet_rt_handle_show_jet_ui_aria_role(choice(4));return 99;}
    tracking=true;
    cases(jet_rt_handle_clone_jet_ui_aria_role,jet_rt_handle_drop_jet_ui_aria_role,jet_rt_handle_eq_jet_ui_aria_role,4,true);
    cases(jet_rt_handle_clone_jet_web_form_validation_timing,jet_rt_handle_drop_jet_web_form_validation_timing,jet_rt_handle_eq_jet_web_form_validation_timing,3,false);
    assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);
    puts("UI choices: all ordinals, role spellings, independent clones and exact-once frees ok");return 0;
}
