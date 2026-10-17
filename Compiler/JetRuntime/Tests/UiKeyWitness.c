/* Real Jet providers; link --wrap=jet_rt_alloc/jet_rt_free.
 * Compose 301f6b608 (TextDebug), 2442d5d80 (bad-tag stop).
 * argv[1]: frozen RtPlot1b char-debug-goldens.bin, all 1,112,064 scalars.
 * No C renderer/model substitutes for the Jet renderer being exercised. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len,cap; } Text;
enum { KEY_BYTES=16, KEY_KIND=0, KEY_PAYLOAD=8 };
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern void jet_rt_string_drop(Text *);
extern H jet_rt_handle_clone_jet_key(H);
extern void jet_rt_handle_drop_jet_key(H);
extern bool jet_rt_handle_eq_jet_key(H,H);
extern Text *jet_rt_handle_show_jet_key(H),*jet_rt_handle_debug_jet_key(H),*jet_rt_handle_display_jet_key(H);
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
static void balanced(void) {
    assert(allocations==frees);
    for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);
    allocations=frees=0;
}
static H key(H kind,H payload) {
    H value=jet_rt_alloc(KEY_BYTES,8);
    memcpy((void *)(uintptr_t)(value+KEY_KIND),&kind,8);
    memcpy((void *)(uintptr_t)(value+KEY_PAYLOAD),&payload,8);
    return value;
}
static void renders(H value,const unsigned char *bytes,size_t length) {
    Text *(*routes[])(H)={jet_rt_handle_show_jet_key,jet_rt_handle_debug_jet_key,jet_rt_handle_display_jet_key};
    for(size_t i=0;i<3;++i){Text *out=routes[i](value);assert(out->len==length&&!memcmp(out->data,bytes,length));jet_rt_string_drop(out);}
}
static void projection(H kind,H payload,const unsigned char *bytes,size_t length) {
    H a=key(kind,payload),b=jet_rt_handle_clone_jet_key(a);
    assert(a!=b&&jet_rt_handle_eq_jet_key(a,b));renders(b,bytes,length);
    H wrong=key(kind==12?0:kind+1,payload);assert(!jet_rt_handle_eq_jet_key(a,wrong));jet_rt_handle_drop_jet_key(wrong);
    H different=key(kind,payload^1);assert(jet_rt_handle_eq_jet_key(a,different)==(kind!=0&&kind!=10&&kind!=11));jet_rt_handle_drop_jet_key(different);
    jet_rt_handle_drop_jet_key(a);renders(b,bytes,length);jet_rt_handle_drop_jet_key(b);balanced();
}
static uint32_t u32le(const unsigned char *p) { return (uint32_t)p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24; }
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2&&!strcmp(argv[1],"key-tag")){jet_rt_handle_show_jet_key(key(13,0));return 99;}
    if(argc!=2)return 2;
    FILE *oracle=fopen(argv[1],"rb");assert(oracle);tracking=true;
    static const char *labels[]={"Enter","Escape","Backspace","Tab","Delete","Up","Down","Left","Right"};
    for(H i=1;i<=9;++i)projection(i,INT64_MIN,(const unsigned char *)labels[i-1],strlen(labels[i-1]));
    projection(12,INT64_MAX,(const unsigned char *)"Unknown",7);
    static const H numbers[]={INT64_MIN,INT64_MAX,-12,0,12};
    for(size_t i=0;i<sizeof(numbers)/sizeof(numbers[0]);++i){char out[64];int n=snprintf(out,sizeof out,"F(%lld)",(long long)numbers[i]);assert(n>0);projection(10,numbers[i],(const unsigned char *)out,(size_t)n);}
    uint32_t next=0,total=0;
    for(;;) {
        unsigned char head[5],quoted[16],expected[32];size_t n=fread(head,1,5,oracle);if(!n)break;assert(n==5);
        uint32_t cp=u32le(head);while(next>=0xd800&&next<=0xdfff)++next;assert(cp==next++&&head[4]<=sizeof quoted);
        size_t len=head[4];assert(fread(quoted,1,len,oracle)==len);
        for(H kind=0;kind<=11;kind+=11){memcpy(expected,kind==0?"Char(":"Ctrl(",5);memcpy(expected+5,quoted,len);expected[5+len]=')';projection(kind,(H)(UINT64_C(0xdeadbeef00000000)|cp),expected,len+6);}
        ++total;
    }
    assert(!ferror(oracle)&&total==1112064&&next==0x110000);assert(!fclose(oracle));jet_rt_handle_drop_jet_key(0);balanced();
    puts("UI Key: all 1112064 scalars, 13 variants, three projections, raw payloads and exact-once frees ok");return 0;
}
