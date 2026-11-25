// Real Jet-object client. No provider substitutes.
// Native TimeGlueABI Duration only; source Core Instant is NOT a pointer carrier.
// Render golden: Duration195-197.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity,cache; } Text;
typedef struct { int64_t ns; } Duration;
_Static_assert(sizeof(Duration)==8,"duration ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Duration *jet_rt_record_new_duration(int64_t);
extern Duration *jet_rt_handle_clone_duration(Duration *);
extern void jet_rt_handle_drop_duration(Duration *);
extern bool jet_rt_handle_eq_duration(Duration *,Duration *);
extern int64_t jet_rt_record_get_duration_ns(Duration *);
extern Text *jet_rt_handle_show_duration(Duration *);
extern Text *jet_rt_handle_display_duration(Duration *);
extern Text *jet_rt_handle_debug_duration(Duration *);
static struct { void *pointer; int64_t bytes,align; unsigned frees; } watched[16];
static size_t count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align); assert(++watched[i].frees==1);
    }
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<16); for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
// A text is one 104-byte block: its owner word, the 32-byte body the pointer names and 64 inline bytes, plus a separate buffer past them.
static void watch_text(Text *t) { watch((char *)(t)-8,104,8); if((const char *)t->data!=(const char *)t+32) watch(t->data,t->capacity,1); }
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); }
static void golden(Text *a,Text *b,Text *c,const char *s) {
    assert(a!=b && a!=c && b!=c); text_is(a,s); text_is(b,s); text_is(c,s);
    watch_text(a); watch_text(b); watch_text(c);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c); done();
}
static void durations(void) {
    const int64_t raws[]={INT64_MIN,INT64_MAX,-1,0,1};
    const char *goldens[]={"-9223372036854775808ns","9223372036854775807ns","-1ns","0ns","1ns"};
    for(size_t i=0;i<5;++i) {
        int64_t input=jet_int_owned_from_i64(raws[i]); Duration *a=jet_rt_record_new_duration(input), *b=jet_rt_handle_clone_duration(a);
        int64_t first=jet_rt_record_get_duration_ns(a),second=jet_rt_record_get_duration_ns(a);
        if(i<2) assert(first!=input && second!=input && first!=second);
        jet_rt_int_drop(input); assert(a->ns==raws[i] && a!=b && jet_rt_handle_eq_duration(a,b));
        assert(jet_int_owned_to_i64(first)==raws[i] && jet_int_owned_to_i64(second)==raws[i]);
        jet_rt_int_drop(first); jet_rt_int_drop(second);
        b->ns^=1; assert(!jet_rt_handle_eq_duration(a,b)); b->ns^=1;
        golden(jet_rt_handle_show_duration(a),jet_rt_handle_display_duration(a),jet_rt_handle_debug_duration(a),goldens[i]);
        watch(a,8,8); watch(b,8,8); jet_rt_handle_drop_duration(a); jet_rt_handle_drop_duration(b); done();
    }
    jet_rt_handle_drop_duration(NULL);
}
int main(int argc,char **argv) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if(argc>1) {
        int64_t hi=jet_int_owned_from_i64(INT64_MAX),over=jet_int_add(hi,1);
        if(strcmp(argv[1],"duration-overflow")==0) (void)jet_rt_record_new_duration(over);
        else return 2;
        assert(!"overflow mode returned"); return 1;
    }
    durations(); puts("TimeGlue ownership/equality/render OK"); return 0;
}
