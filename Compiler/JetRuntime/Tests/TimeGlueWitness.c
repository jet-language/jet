// Real Jet-object client, link --wrap=jet_rt_free. No provider substitutes.
// Native TimeGlueABI only; source Core Instant is NOT a pointer carrier.
// Render goldens: Time280-286/472-480/619-620/1760-1765, Duration195-197.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t fields[3]; } Triple;
typedef struct { int64_t hour,minute,second; uint32_t nanos,padding; } Local;
typedef struct { int64_t ns; } Duration;
typedef struct { int64_t tag; } Unit;
_Static_assert(sizeof(Triple)==24 && sizeof(Local)==32 && offsetof(Local,nanos)==24,"civil ABI");
_Static_assert(sizeof(Duration)==8 && sizeof(Unit)==8,"duration ABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
#define TRIPLE_API(name,a,b,c) \
extern Triple *jet_rt_record_new_##name(int64_t,int64_t,int64_t); \
extern Triple *jet_rt_handle_clone_##name(Triple *); \
extern void jet_rt_handle_drop_##name(Triple *); \
extern bool jet_rt_handle_eq_##name(Triple *,Triple *); \
extern int64_t jet_rt_record_get_##name##_##a(Triple *); \
extern int64_t jet_rt_record_get_##name##_##b(Triple *); \
extern int64_t jet_rt_record_get_##name##_##c(Triple *); \
extern Text *jet_rt_handle_show_##name(Triple *); \
extern Text *jet_rt_handle_display_##name(Triple *); \
extern Text *jet_rt_handle_debug_##name(Triple *)
TRIPLE_API(JetDate,year,month,day); TRIPLE_API(JetPeriod,years,months,days);
extern Local *jet_rt_handle_clone_jet_local_time(Local *);
extern void jet_rt_handle_drop_jet_local_time(Local *);
extern bool jet_rt_handle_eq_jet_local_time(Local *,Local *);
extern Text *jet_rt_handle_show_jet_local_time(Local *);
extern Text *jet_rt_handle_display_jet_local_time(Local *);
extern Text *jet_rt_handle_debug_jet_local_time(Local *);
extern Duration *jet_rt_record_new_duration(int64_t);
extern Duration *jet_rt_handle_clone_duration(Duration *);
extern void jet_rt_handle_drop_duration(Duration *);
extern bool jet_rt_handle_eq_duration(Duration *,Duration *);
extern int64_t jet_rt_record_get_duration_ns(Duration *);
extern Text *jet_rt_handle_show_duration(Duration *);
extern Text *jet_rt_handle_display_duration(Duration *);
extern Text *jet_rt_handle_debug_duration(Duration *);
extern Unit *jet_rt_handle_clone_duration_unit(Unit *);
extern void jet_rt_handle_drop_duration_unit(Unit *);
extern bool jet_rt_handle_eq_duration_unit(Unit *,Unit *);
static struct { void *pointer; int64_t bytes,align; unsigned frees; } watched[16];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align); assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<16); for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); }
static void golden(Text *a,Text *b,Text *c,const char *s) {
    assert(a!=b && a!=c && b!=c); text_is(a,s); text_is(b,s); text_is(c,s);
    watch_text(a); watch_text(b); watch_text(c);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c); done();
}
typedef struct {
    Triple *(*new_value)(int64_t,int64_t,int64_t), *(*clone)(Triple *);
    void (*drop)(Triple *); bool (*eq)(Triple *,Triple *);
    int64_t (*get[3])(Triple *);
    Text *(*show)(Triple *), *(*display)(Triple *), *(*debug)(Triple *);
} TripleAPI;
#define API(name,a,b,c) {jet_rt_record_new_##name,jet_rt_handle_clone_##name,jet_rt_handle_drop_##name,jet_rt_handle_eq_##name,{jet_rt_record_get_##name##_##a,jet_rt_record_get_##name##_##b,jet_rt_record_get_##name##_##c},jet_rt_handle_show_##name,jet_rt_handle_display_##name,jet_rt_handle_debug_##name}
static void triples(TripleAPI api,const char *edge_golden,const char *small_golden) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Triple *a=api.new_value(lo,hi,lo), *b=api.clone(a);
    assert(a!=b && api.eq(a,b) && api.eq(a,a));
    assert(a->fields[0]==INT64_MIN && a->fields[1]==INT64_MAX && a->fields[2]==INT64_MIN);
    for(size_t i=0;i<3;++i) {
        int64_t first=api.get[i](a),second=api.get[i](a);
        assert(first!=hi && first!=lo && second!=hi && second!=lo && first!=second);
        assert(jet_int_owned_to_i64(first)==a->fields[i] && jet_int_owned_to_i64(second)==a->fields[i]);
        jet_rt_int_drop(first); jet_rt_int_drop(second);
        b->fields[i]^=1; assert(!api.eq(a,b)); b->fields[i]^=1;
    }
    jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    golden(api.show(a),api.display(a),api.debug(a),edge_golden);
    watch(a,24,8); watch(b,24,8); api.drop(a); api.drop(b); done();
    a=api.new_value(-1,2,3); golden(api.show(a),api.display(a),api.debug(a),small_golden);
    watch(a,24,8); api.drop(a); done(); api.drop(NULL);
}
static void locals(void) {
    const uint32_t nanos[]={0,1,100000000,UINT32_MAX};
    const char *goldens[]={"01:02:03","01:02:03.000000001","01:02:03.100000000","01:02:03.4294967295"};
    for(size_t n=0;n<4;++n) {
        Local *a=jet_rt_alloc(32,8); a->hour=1; a->minute=2; a->second=3; a->nanos=nanos[n]; a->padding=UINT32_MAX;
        Local *b=jet_rt_handle_clone_jet_local_time(a);
        assert(a!=b && b->padding==0 && jet_rt_handle_eq_jet_local_time(a,b));
        int64_t *fields[]={&b->hour,&b->minute,&b->second};
        for(size_t i=0;i<3;++i) { *fields[i]^=1; assert(!jet_rt_handle_eq_jet_local_time(a,b)); *fields[i]^=1; }
        b->nanos^=1; assert(!jet_rt_handle_eq_jet_local_time(a,b)); b->nanos^=1;
        b->padding=123; assert(jet_rt_handle_eq_jet_local_time(a,b));
        golden(jet_rt_handle_show_jet_local_time(a),jet_rt_handle_display_jet_local_time(a),jet_rt_handle_debug_jet_local_time(a),goldens[n]);
        a->hour=-1; a->minute=100; a->second=-9; a->nanos=0;
        golden(jet_rt_handle_show_jet_local_time(a),jet_rt_handle_display_jet_local_time(a),jet_rt_handle_debug_jet_local_time(a),"-1:100:-9");
        watch(a,32,8); watch(b,32,8); jet_rt_handle_drop_jet_local_time(a); jet_rt_handle_drop_jet_local_time(b); done();
    }
    jet_rt_handle_drop_jet_local_time(NULL);
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
    for(int64_t i=0;i<6;++i) {
        Unit *a=jet_rt_alloc(8,8); a->tag=i; Unit *b=jet_rt_handle_clone_duration_unit(a);
        assert(a!=b && jet_rt_handle_eq_duration_unit(a,b)); b->tag=(i+1)%6; assert(!jet_rt_handle_eq_duration_unit(a,b));
        watch(a,8,8); watch(b,8,8); jet_rt_handle_drop_duration_unit(a); jet_rt_handle_drop_duration_unit(b); done();
    }
    jet_rt_handle_drop_duration_unit(NULL);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1) {
        int64_t hi=jet_int_owned_from_i64(INT64_MAX),over=jet_int_add(hi,1);
        if(strcmp(argv[1],"date-overflow")==0) (void)jet_rt_record_new_jet_date(over,1,1);
        else if(strcmp(argv[1],"period-overflow")==0) (void)jet_rt_record_new_jet_period(0,over,0);
        else if(strcmp(argv[1],"duration-overflow")==0) (void)jet_rt_record_new_duration(over);
        else return 2;
        assert(!"overflow mode returned"); return 1;
    }
    TripleAPI date=API(JetDate,year,month,day),period=API(JetPeriod,years,months,days);
    triples(date,"-9223372036854775807-9223372036854775807--9223372036854775808","-0001-02-03");
    triples(period,"P-9223372036854775808Y9223372036854775807M-9223372036854775808D","P-1Y2M3D");
    locals(); durations(); puts("TimeGlue ownership/equality/render OK"); return 0;
}
