/* The real jet_rt_char_debug (`{c:Debug}`) against the frozen char Debug
 * goldens, with checked-heap alloc/free observers.
 * argv[1]: char-debug-goldens.bin for all 1,112,064 scalars, rebuilt by
 * UIWitness.jet (char-debug) from the tracked CharDebugPlain.ranges.
 * An invalid scalar renders as U+FFFD does. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
// A heap text is this 32-byte body behind its 8-byte owner word.
typedef struct { unsigned char *data; int64_t len,cap,cache; } Text;
_Static_assert(sizeof(Text)==32,"refcounted text body");
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_char_debug(H);
static struct { H address,bytes,align; bool live; } ledger[256];
static size_t allocations,frees;
static bool tracking;
static void observe_alloc(H address, H bytes, H align) {
    if(tracking) {
        assert(address&&bytes>0&&allocations<256&&(uint64_t)address%(uint64_t)align==0);
        for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);
        ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;
    }
}
static void observe_free(H address,H bytes,H align) {
    if(tracking) {
        for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;return;}
        assert(!"untracked or double free");
    }
}
static void balanced(void) {
    assert(allocations==frees);
    for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);
    allocations=frees=0;
}
static void renders(H scalar,const unsigned char *bytes,size_t length) {
    Text *out=jet_rt_char_debug(scalar);
    assert(out->len>=0&&(size_t)out->len==length&&!memcmp(out->data,bytes,length));
    jet_rt_string_drop(out);balanced();
}
static uint32_t u32le(const unsigned char *p) { return (uint32_t)p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24; }
int main(int argc,char **argv) {
    assert(jet_rt_alloc_check_observe((int64_t)(uintptr_t)observe_alloc, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if(argc!=2)return 2;
    FILE *oracle=fopen(argv[1],"rb");assert(oracle);tracking=true;
    unsigned char replacement[16];size_t replacement_len=0;
    uint32_t next=0,total=0;
    for(;;) {
        unsigned char head[5],quoted[16];size_t n=fread(head,1,5,oracle);if(!n)break;assert(n==5);
        uint32_t cp=u32le(head);while(next>=0xd800&&next<=0xdfff)++next;assert(cp==next++&&head[4]<=sizeof quoted);
        size_t len=head[4];assert(fread(quoted,1,len,oracle)==len);
        renders(cp,quoted,len);
        if(cp==0xfffd){memcpy(replacement,quoted,len);replacement_len=len;}
        ++total;
    }
    assert(!ferror(oracle)&&total==1112064&&next==0x110000&&replacement_len>0);assert(!fclose(oracle));
    static const H invalid[]={-1,0xd800,0xdfff,0x110000,INT64_MIN,INT64_MAX};
    for(size_t i=0;i<sizeof(invalid)/sizeof(invalid[0]);++i)renders(invalid[i],replacement,replacement_len);
    puts("Char Debug: all 1112064 scalars and 6 invalid scalars, exact bytes and exact-once frees ok");return 0;
}
