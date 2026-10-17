// Real Jet-object client, link -Wl,--wrap=jet_rt_free; no mocks.
// Goldens: pinned rustc1.97.1 (8bab26f4f), Time51-71/1147-55/1680-88.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t seconds; uint64_t nanos,leap; } DateTime;
typedef struct { int64_t first; uint64_t second; } Pair;
typedef struct { int64_t length,capacity; Pair *data; } Rows;
typedef struct { Text *name; Rows *transitions,*infos; } Zone;
typedef struct { DateTime *instant; Zone *zone; } Zoned;
_Static_assert(sizeof(DateTime)==24 && offsetof(DateTime,leap)==16,"DateTime24");
_Static_assert(sizeof(Zone)==24 && offsetof(Zone,infos)==16,"Zone24");
_Static_assert(sizeof(Zoned)==16 && offsetof(Zoned,zone)==8,"Zoned16");
_Static_assert(sizeof(Rows)==24 && sizeof(Pair)==16,"List24 pair16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_new(unsigned char *,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
#define API(type,name) \
extern type *jet_rt_handle_clone_##name(type *); \
extern void jet_rt_handle_drop_##name(type *); \
extern bool jet_rt_handle_eq_##name(type *,type *); \
extern Text *jet_rt_handle_show_##name(type *); \
extern Text *jet_rt_handle_display_##name(type *); \
extern Text *jet_rt_handle_debug_##name(type *)
API(DateTime,JetDateTime); API(Zone,JetZone); API(Zoned,JetZonedDateTime);
extern unsigned char *jet_rt_handle_field_jet_date_time_leap_second(DateTime *);
extern void jet_rt_handle_field_jet_zone_name(Zone *,Text *);
extern DateTime *jet_rt_handle_field_jet_zoned_date_time_instant(Zoned *);
extern Zone *jet_rt_handle_field_jet_zoned_date_time_zone(Zoned *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[48];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align); assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<48); for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void done(void) { assert(intercepted); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_rows(Rows *r) { watch(r,24,8); if(r->capacity) watch(r->data,r->capacity*16,8); }
static void watch_zone(Zone *z) { watch(z,24,8); watch_text(z->name); watch_rows(z->transitions); watch_rows(z->infos); }
static void watch_zoned(Zoned *z) { watch(z,16,8); watch(z->instant,24,8); watch_zone(z->zone); }
static void text_is(Text *t,const char *s) { assert(t && t->length==(int64_t)strlen(s)); assert(!t->length || memcmp(t->data,s,(size_t)t->length)==0); }
static Text *text(const char *s) { return jet_rt_string_new((unsigned char *)s,(int64_t)strlen(s)); }
static DateTime *datetime(int64_t seconds,uint64_t nanos,uint64_t leap) {
    DateTime *d=jet_rt_alloc(24,8); d->seconds=seconds; d->nanos=nanos; d->leap=leap; return d;
}
static Rows *rows(const Pair *data,int64_t length,int64_t spare) {
    Rows *r=jet_rt_alloc(24,8); r->length=length; r->capacity=length+spare;
    r->data=r->capacity ? jet_rt_alloc(r->capacity*16,8) : (void *)(uintptr_t)8;
    if(length) memcpy(r->data,data,(size_t)length*16);
    return r;
}
static Zone *zone(const char *name,int64_t offset) {
    Zone *z=jet_rt_alloc(24,8); z->name=text(name); z->transitions=rows(NULL,0,0);
    Pair info={offset,0}; z->infos=rows(&info,1,3); return z;
}
static Zoned *zoned(int64_t seconds,uint64_t nanos,uint64_t leap,const char *name,int64_t offset) {
    Zoned *z=jet_rt_alloc(16,8); z->instant=datetime(seconds,nanos,leap); z->zone=zone(name,offset); return z;
}
static void check_texts(Text *s,Text *d,Text *g,const char *expected) {
    assert(s!=d && s!=g && d!=g); text_is(s,expected); text_is(d,expected); text_is(g,expected);
    watch_text(s); watch_text(d); watch_text(g); jet_rt_string_drop(s); jet_rt_string_drop(d); jet_rt_string_drop(g); done();
}
static void date_render(int64_t seconds,uint64_t nanos,uint64_t leap,const char *expected) {
    DateTime *a=datetime(seconds,nanos,leap),*b=jet_rt_handle_clone_jet_date_time(a);
    assert(a!=b && b->seconds==seconds && b->nanos==(nanos&UINT64_C(0xffffffff)) && b->leap==((leap&255)!=0));
    unsigned char *field=jet_rt_handle_field_jet_date_time_leap_second(a);
    assert(field==(unsigned char *)&a->leap && *field==(unsigned char)leap);
    assert(jet_rt_handle_eq_jet_date_time(a,b));
    Text *s=jet_rt_handle_show_jet_date_time(a),*d=jet_rt_handle_display_jet_date_time(a),*g=jet_rt_handle_debug_jet_date_time(a);
    watch(a,24,8); watch(b,24,8); jet_rt_handle_drop_jet_date_time(a); jet_rt_handle_drop_jet_date_time(b); done();
    check_texts(s,d,g,expected);
}
static void zoned_render(int64_t seconds,uint64_t nanos,uint64_t leap,const char *name,int64_t offset,const char *expected) {
    Zoned *a=zoned(seconds,nanos,leap,name,offset),*b=jet_rt_handle_clone_jet_zoned_date_time(a);
    assert(a!=b && a->instant!=b->instant && a->zone!=b->zone && jet_rt_handle_eq_jet_zoned_date_time(a,b));
    assert(jet_rt_handle_field_jet_zoned_date_time_instant(a)==a->instant && jet_rt_handle_field_jet_zoned_date_time_zone(a)==a->zone);
    Text *s=jet_rt_handle_show_jet_zoned_date_time(a),*d=jet_rt_handle_display_jet_zoned_date_time(a),*g=jet_rt_handle_debug_jet_zoned_date_time(a);
    watch_zoned(a); jet_rt_handle_drop_jet_zoned_date_time(a); done();
    Text *survivor=jet_rt_handle_show_jet_zoned_date_time(b); text_is(survivor,expected);
    watch_text(survivor); jet_rt_string_drop(survivor); done();
    watch_zoned(b); jet_rt_handle_drop_jet_zoned_date_time(b); done(); check_texts(s,d,g,expected);
}
static void equality(void) {
    DateTime *a=datetime(1,UINT64_C(0xabcdef000000007b),UINT64_C(0xabcdef0000000080)),*b=jet_rt_handle_clone_jet_date_time(a);
    assert(b->nanos==123 && b->leap==1 && jet_rt_handle_eq_jet_date_time(a,b));
    ++b->seconds; assert(!jet_rt_handle_eq_jet_date_time(a,b)); --b->seconds;
    ++b->nanos; assert(!jet_rt_handle_eq_jet_date_time(a,b)); --b->nanos;
    b->leap=0; assert(!jet_rt_handle_eq_jet_date_time(a,b)); b->leap=255;
    assert(jet_rt_handle_eq_jet_date_time(a,b));
    watch(a,24,8); watch(b,24,8); jet_rt_handle_drop_jet_date_time(a); jet_rt_handle_drop_jet_date_time(b); done();
    Zoned *x=zoned(1,2,0,"same",20700),*y=jet_rt_handle_clone_jet_zoned_date_time(x);
    y->zone->infos->data[0].first=-12600;
    assert(!jet_rt_handle_eq_jet_zone(x->zone,y->zone) && jet_rt_handle_eq_jet_zoned_date_time(x,y));
    y->zone->name->data[0]^=1; assert(!jet_rt_handle_eq_jet_zoned_date_time(x,y)); y->zone->name->data[0]^=1;
    ++y->instant->seconds; assert(!jet_rt_handle_eq_jet_zoned_date_time(x,y)); --y->instant->seconds;
    watch_zoned(x); watch_zoned(y); jet_rt_handle_drop_jet_zoned_date_time(x); jet_rt_handle_drop_jet_zoned_date_time(y); done();
}
static void zones(void) {
    Zone *a=zone("\xcf\x80\n",0);
    Pair transitions[]={{0,1},{10,2},{10,0},{20,2}},infos[]={{0,0},{20700,UINT64_C(0xabcdef0000000080)},{-12600,1}};
    watch_rows(a->transitions); __wrap_jet_rt_free(a->transitions,24,8); done(); a->transitions=rows(transitions,4,2);
    watch_rows(a->infos); __wrap_jet_rt_free(a->infos->data,a->infos->capacity*16,8); __wrap_jet_rt_free(a->infos,24,8); done(); a->infos=rows(infos,3,1);
    Zone *b=jet_rt_handle_clone_jet_zone(a); assert(a!=b && a->name!=b->name && a->transitions!=b->transitions && a->infos!=b->infos);
    assert(a->transitions->data!=b->transitions->data && a->infos->data!=b->infos->data && b->infos->data[1].second==1);
    assert(jet_rt_handle_eq_jet_zone(a,b));
    Text field={0}; jet_rt_handle_field_jet_zone_name(a,&field);
    assert(field.data==a->name->data && field.length==a->name->length && field.capacity==a->name->capacity);
    Text *owned=jet_rt_string_clone(&field); assert(owned!=a->name && owned->data!=a->name->data);
    Text *s=jet_rt_handle_show_jet_zone(a),*d=jet_rt_handle_display_jet_zone(a),*g=jet_rt_handle_debug_jet_zone(a);
    for(int64_t i=0;i<4;++i) {
        ++b->transitions->data[i].first; assert(!jet_rt_handle_eq_jet_zone(a,b)); --b->transitions->data[i].first;
        b->transitions->data[i].second^=1; assert(!jet_rt_handle_eq_jet_zone(a,b)); b->transitions->data[i].second^=1;
    }
    for(int64_t i=0;i<3;++i) {
        ++b->infos->data[i].first; assert(!jet_rt_handle_eq_jet_zone(a,b)); --b->infos->data[i].first;
        uint64_t saved=b->infos->data[i].second; b->infos->data[i].second=(saved&255)?0:1;
        assert(!jet_rt_handle_eq_jet_zone(a,b)); b->infos->data[i].second=saved;
    }
    --b->transitions->length; assert(!jet_rt_handle_eq_jet_zone(a,b)); ++b->transitions->length;
    --b->infos->length; assert(!jet_rt_handle_eq_jet_zone(a,b)); ++b->infos->length;
    b->name->data[0]^=1; assert(!jet_rt_handle_eq_jet_zone(a,b)); b->name->data[0]^=1;
    watch_zone(b); jet_rt_handle_drop_jet_zone(b); done();
    const int64_t seconds[]={-1,0,9,10,19,20};
    const char *expected[]={"1969-12-31 23:59:59 \xcf\x80\n (+00:00)","1970-01-01 05:45:00 \xcf\x80\n (+05:45)","1970-01-01 05:45:09 \xcf\x80\n (+05:45)","1970-01-01 00:00:10 \xcf\x80\n (+00:00)","1970-01-01 00:00:19 \xcf\x80\n (+00:00)","1969-12-31 20:30:20 \xcf\x80\n (-03:30)"};
    for(size_t i=0;i<6;++i) {
        Zoned *z=jet_rt_alloc(16,8); z->instant=datetime(seconds[i],123,0); z->zone=jet_rt_handle_clone_jet_zone(a);
        Text *render=jet_rt_handle_show_jet_zoned_date_time(z); text_is(render,expected[i]); watch_text(render); jet_rt_string_drop(render); done();
        watch_zoned(z); jet_rt_handle_drop_jet_zoned_date_time(z); done();
    }
    watch_zone(a); jet_rt_handle_drop_jet_zone(a); done();
    text_is(owned,"\xcf\x80\n"); watch_text(owned); jet_rt_string_drop(owned); done(); check_texts(s,d,g,"\xcf\x80\n");
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        Zoned *z=zoned(0,0,0,"Bad",0);
        if(strcmp(argv[1],"empty-infos")==0) {
            z->zone->infos->length=0;
        } else {
            assert(strcmp(argv[1],"info-index-equals-length")==0 || strcmp(argv[1],"negative-info-index")==0);
            Pair transition={0,strcmp(argv[1],"negative-info-index")==0 ? UINT64_MAX : 1};
            watch_rows(z->zone->transitions); __wrap_jet_rt_free(z->zone->transitions,24,8); done();
            z->zone->transitions=rows(&transition,1,0);
        }
        (void)jet_rt_handle_show_jet_zoned_date_time(z);
        assert(!"invalid zone-info index unexpectedly returned"); return 1;
    }
    assert(argc==1); equality(); zones();
    date_render(INT64_MIN,0,0,"-292277022657-01-27 08:29:52 UTC");
    date_render(INT64_MAX,0,0,"292277026596-12-04 15:30:07 UTC");
    date_render(-62167219200LL,0,0,"0000-01-01 00:00:00 UTC");
    date_render(-62198755200LL,0,0,"-0001-01-01 00:00:00 UTC");
    date_render(0,123456789,0,"1970-01-01 00:00:00 UTC");
    date_render(1483228799,1000000000,255,"2016-12-31 23:59:60 UTC");
    zoned_render(1483228799,1000000000,128,"UTC",20700,"2016-12-31 23:59:60.1000000000 UTC (+00:00)");
    zoned_render(1483228799,1000000000,1,"East",20700,"2017-01-01 05:45:00 East (+05:45)");
    zoned_render(1483228799,1000000000,1,"West",-12600,"2016-12-31 20:30:00 West (-03:30)");
    zoned_render(INT64_MAX,0,1,"East",20700,"292277026596-12-04 15:30:07 East (+05:45)");
    zoned_render(INT64_MIN,0,0,"West",-12600,"-292277022657-01-27 08:29:52 West (-03:30)");
    zoned_render(-62167219200LL,0,0,"West",-12600,"-0001-12-31 20:30:00 West (-03:30)");
    zoned_render(0,123456789,0,"UTC",0,"1970-01-01 00:00:00 UTC (+00:00)");
    jet_rt_handle_drop_jet_date_time(NULL); jet_rt_handle_drop_jet_zone(NULL); jet_rt_handle_drop_jet_zoned_date_time(NULL);
    puts("DateZone ownership/equality/render OK"); return 0;
}
