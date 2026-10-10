/* Real composed Jet object client, not a provider. Link --wrap=jet_rt_free.
 * ReceiptABI native layouts; Rust source fields in DbPool/ServiceAuthority.
 * Only light syntax checked by writer; normal output is the .stdout golden. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <inttypes.h>
typedef struct { unsigned char *data; int64_t len, capacity; } Text;
typedef struct { int64_t lifecycle,max,available,leased,opening,ready,acquires,releases,timeouts,open_failures,unhealthy,replacements,replacement_failures; } Db;
typedef struct { Text *id; int64_t state,attempts,retention_until,deadline; Text *key; int64_t duplicate; Text *authority; int64_t generation; Text *signature; } Delivery;
_Static_assert(sizeof(Text)==24,"E4 Text");
_Static_assert(sizeof(Db)==104 && offsetof(Db,ready)==40 && offsetof(Db,replacement_failures)==96,"ReceiptABI Db");
_Static_assert(sizeof(Delivery)==80 && offsetof(Delivery,key)==40 && offsetof(Delivery,duplicate)==48 && offsetof(Delivery,signature)==72,"ReceiptABI Delivery");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern int64_t *jet_rt_handle_clone_jet_db_pool_lifecycle(int64_t *);
extern void jet_rt_handle_drop_jet_db_pool_lifecycle(int64_t *);
extern bool jet_rt_handle_eq_jet_db_pool_lifecycle(int64_t *,int64_t *);
extern Db *jet_rt_record_new_jet_db_pool_receipt(int64_t,int64_t,int64_t,int64_t,int64_t,uint8_t,int64_t,int64_t,int64_t,int64_t,int64_t,int64_t,int64_t);
extern Db *jet_rt_handle_clone_jet_db_pool_receipt(Db *);
extern void jet_rt_handle_drop_jet_db_pool_receipt(Db *);
extern bool jet_rt_handle_eq_jet_db_pool_receipt(Db *,Db *);
extern Text *jet_rt_handle_show_jet_db_pool_receipt(Db *);
extern Text *jet_rt_handle_display_jet_db_pool_receipt(Db *);
extern Text *jet_rt_handle_debug_jet_db_pool_receipt(Db *);
extern int64_t jet_rt_record_get_jet_db_pool_receipt_lifecycle(Db *);
extern bool jet_rt_record_get_jet_db_pool_receipt_ready(Db *);
#define DB_GET(name) extern int64_t jet_rt_record_get_jet_db_pool_receipt_##name(Db *)
DB_GET(max); DB_GET(available); DB_GET(leased); DB_GET(opening); DB_GET(acquires); DB_GET(releases); DB_GET(timeouts); DB_GET(open_failures); DB_GET(unhealthy); DB_GET(replacements); DB_GET(replacement_failures);
#undef DB_GET
extern int64_t *jet_rt_handle_clone_jet_delivery_state(int64_t *);
extern void jet_rt_handle_drop_jet_delivery_state(int64_t *);
extern bool jet_rt_handle_eq_jet_delivery_state(int64_t *,int64_t *);
extern Text *jet_rt_handle_show_jet_delivery_state(int64_t *);
extern Delivery *jet_rt_record_new_jet_delivery_receipt(Text *,int64_t,int64_t,int64_t,int64_t,Text *,uint8_t,Text *,int64_t,Text *);
extern Delivery *jet_rt_handle_clone_jet_delivery_receipt(Delivery *);
extern void jet_rt_handle_drop_jet_delivery_receipt(Delivery *);
extern bool jet_rt_handle_eq_jet_delivery_receipt(Delivery *,Delivery *);
extern Text *jet_rt_handle_show_jet_delivery_receipt(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_receipt_id(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_receipt_state(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_receipt_attempts(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_receipt_retention_until(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_receipt_deadline(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_receipt_idempotency_key(Delivery *);
extern bool jet_rt_record_get_jet_delivery_receipt_duplicate(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_receipt_authority(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_receipt_generation(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_receipt_signature(Delivery *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[1024];
static size_t watch_count,intercepted;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && watch_count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<watch_count;++i) assert(watched[i].ptr!=ptr || watched[i].freed==1);
    watched[watch_count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=watch_count;i>0;--i) if(watched[i-1].ptr==ptr && watched[i-1].freed==0) {
        assert(watched[i-1].size==size && watched[i-1].align==align);
        watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void watch_text(Text *t) {
    watch(t,24,8);
    if(t->capacity>0) watch(t->data,t->capacity,1);
}
static void text_is(Text *t,const char *s) {
    assert(t->len==(int64_t)strlen(s));
    assert(!t->len || !memcmp(t->data,s,(size_t)t->len));
}
static void own_text_is(Text *t,const char *s) { watch_text(t); text_is(t,s); jet_rt_string_drop(t); }
static void int_is(int64_t value,int64_t expected) { assert(jet_int_owned_to_i64(value)==expected); jet_rt_int_drop(value); }
static int64_t *enum_owner(int64_t ordinal) { int64_t *p=jet_rt_alloc(8,8); *p=ordinal; watch(p,8,8); return p; }
static Db *db_new(int64_t ordinal,uint8_t ready,int64_t max) {
    int64_t values[]={max,-2,3,4,5,6,7,8,9,10,INT64_MIN};
    int64_t args[11];
    for(size_t i=0;i<11;++i) args[i]=jet_int_owned_from_i64(values[i]);
    Db *p=jet_rt_record_new_jet_db_pool_receipt(ordinal,args[0],args[1],args[2],args[3],ready,args[4],args[5],args[6],args[7],args[8],args[9],args[10]);
    for(size_t i=0;i<11;++i) jet_rt_int_drop(args[i]);
    watch(p,104,8); return p;
}
static void db_cases(void) {
    const char *lifecycle[]={"open","draining","drained"};
    int64_t (*getters[])(Db *)={jet_rt_record_get_jet_db_pool_receipt_max,jet_rt_record_get_jet_db_pool_receipt_available,jet_rt_record_get_jet_db_pool_receipt_leased,jet_rt_record_get_jet_db_pool_receipt_opening,jet_rt_record_get_jet_db_pool_receipt_acquires,jet_rt_record_get_jet_db_pool_receipt_releases,jet_rt_record_get_jet_db_pool_receipt_timeouts,jet_rt_record_get_jet_db_pool_receipt_open_failures,jet_rt_record_get_jet_db_pool_receipt_unhealthy,jet_rt_record_get_jet_db_pool_receipt_replacements,jet_rt_record_get_jet_db_pool_receipt_replacement_failures};
    const int64_t expected[]={INT64_MAX,-2,3,4,5,6,7,8,9,10,INT64_MIN};
    for(int64_t tag=0;tag<3;++tag) {
        int64_t *e=enum_owner(tag),*ec=jet_rt_handle_clone_jet_db_pool_lifecycle(e); watch(ec,8,8);
        assert(ec!=e && jet_rt_handle_eq_jet_db_pool_lifecycle(e,ec)); *ec=(tag+1)%3;
        assert(!jet_rt_handle_eq_jet_db_pool_lifecycle(e,ec)); *ec=tag;
        jet_rt_handle_drop_jet_db_pool_lifecycle(e); assert(*ec==tag); jet_rt_handle_drop_jet_db_pool_lifecycle(ec);
        for(unsigned ready=0;ready<2;++ready) {
            Db *p=db_new(tag,ready ? UINT8_MAX : 0,INT64_MAX),*q=jet_rt_handle_clone_jet_db_pool_receipt(p); watch(q,104,8);
            assert(p!=q && jet_rt_handle_eq_jet_db_pool_receipt(p,q));
            assert(jet_rt_record_get_jet_db_pool_receipt_lifecycle(q)==tag && jet_rt_record_get_jet_db_pool_receipt_ready(q)==(ready!=0));
            for(size_t i=0;i<11;++i) int_is(getters[i](q),expected[i]);
            int64_t *fields[]={&q->lifecycle,&q->max,&q->available,&q->leased,&q->opening,&q->ready,&q->acquires,&q->releases,&q->timeouts,&q->open_failures,&q->unhealthy,&q->replacements,&q->replacement_failures};
            for(size_t i=0;i<13;++i) { int64_t save=*fields[i]; *fields[i]^=1; assert(!jet_rt_handle_eq_jet_db_pool_receipt(p,q)); *fields[i]=save; }
            q->ready=ready ? INT64_C(0x102) : INT64_C(0x100);
            assert(jet_rt_handle_eq_jet_db_pool_receipt(p,q));
            Db *normalized=jet_rt_handle_clone_jet_db_pool_receipt(q); watch(normalized,104,8); assert(normalized->ready==(int64_t)ready); jet_rt_handle_drop_jet_db_pool_receipt(normalized);
            char render[512];
            int n=snprintf(render,sizeof(render),"db.pool lifecycle=%s max=%" PRId64 " available=-2 leased=3 opening=4 ready=%u acquires=5 releases=6 timeouts=7 open_failures=8 unhealthy=9 replacements=10 replacement_failures=%" PRId64,lifecycle[tag],INT64_MAX,ready,INT64_MIN);
            assert(n>0 && (size_t)n<sizeof(render));
            own_text_is(jet_rt_handle_show_jet_db_pool_receipt(p),render);
            own_text_is(jet_rt_handle_debug_jet_db_pool_receipt(p),render);
            own_text_is(jet_rt_handle_display_jet_db_pool_receipt(p),render);
            own_text_is(jet_rt_handle_show_jet_db_pool_receipt(q),render); /* q ready low byte is 2 or 0, upper byte 1. */
            jet_rt_handle_drop_jet_db_pool_receipt(p); int_is(jet_rt_record_get_jet_db_pool_receipt_max(q),INT64_MAX); jet_rt_handle_drop_jet_db_pool_receipt(q);
        }
    }
}
static Delivery *delivery_new(int64_t tag,uint8_t duplicate,bool empty) {
    Text *id=jet_rt_string_from_static(empty ? "" : "id`λ",empty ? 0 : 5);
    Text *key=jet_rt_string_from_static(empty ? "" : "key,=",empty ? 0 : 5);
    Text *authority=jet_rt_string_from_static(empty ? "" : "authority",empty ? 0 : 9);
    Text *signature=jet_rt_string_from_static(empty ? "" : "sig()",empty ? 0 : 5);
    watch_text(id); watch_text(key); watch_text(authority); watch_text(signature);
    int64_t a=jet_int_owned_from_i64(INT64_MIN),r=jet_int_owned_from_i64(-1),d=jet_int_owned_from_i64(0),g=jet_int_owned_from_i64(INT64_MAX);
    Delivery *p=jet_rt_record_new_jet_delivery_receipt(id,tag,a,r,d,key,duplicate,authority,g,signature);
    assert(p->id!=id && p->key!=key && p->authority!=authority && p->signature!=signature);
    jet_rt_int_drop(a); jet_rt_int_drop(r); jet_rt_int_drop(d); jet_rt_int_drop(g);
    jet_rt_string_drop(id); jet_rt_string_drop(key); jet_rt_string_drop(authority); jet_rt_string_drop(signature);
    watch(p,80,8); watch_text(p->id); watch_text(p->key); watch_text(p->authority); watch_text(p->signature); return p;
}
static void delivery_cases(void) {
    const char *names[]={"Pending","Accepted","Delivering","Delivered","DeadLettered","Cancelled"};
    for(int64_t tag=0;tag<6;++tag) {
        int64_t *e=enum_owner(tag),*ec=jet_rt_handle_clone_jet_delivery_state(e); watch(ec,8,8);
        assert(ec!=e && jet_rt_handle_eq_jet_delivery_state(e,ec)); *ec=(tag+1)%6;
        assert(!jet_rt_handle_eq_jet_delivery_state(e,ec)); *ec=tag;
        own_text_is(jet_rt_handle_show_jet_delivery_state(e),names[tag]); jet_rt_handle_drop_jet_delivery_state(e);
        own_text_is(jet_rt_handle_show_jet_delivery_state(ec),names[tag]); jet_rt_handle_drop_jet_delivery_state(ec);
        for(unsigned duplicate=0;duplicate<2;++duplicate) {
            Delivery *p=delivery_new(tag,duplicate ? UINT8_MAX : 0,false),*q=jet_rt_handle_clone_jet_delivery_receipt(p);
            watch(q,80,8); watch_text(q->id); watch_text(q->key); watch_text(q->authority); watch_text(q->signature);
            assert(q!=p && q->id!=p->id && q->key!=p->key && q->authority!=p->authority && q->signature!=p->signature && jet_rt_handle_eq_jet_delivery_receipt(p,q));
            assert(jet_rt_record_get_jet_delivery_receipt_state(q)==tag && jet_rt_record_get_jet_delivery_receipt_duplicate(q)==(duplicate!=0));
            int_is(jet_rt_record_get_jet_delivery_receipt_attempts(q),INT64_MIN);
            int_is(jet_rt_record_get_jet_delivery_receipt_retention_until(q),-1);
            int_is(jet_rt_record_get_jet_delivery_receipt_deadline(q),0);
            int_is(jet_rt_record_get_jet_delivery_receipt_generation(q),INT64_MAX);
            Text *fields[]={q->id,q->key,q->authority,q->signature};
            for(size_t i=0;i<4;++i) { fields[i]->data[0]^=1; assert(!jet_rt_handle_eq_jet_delivery_receipt(p,q)); fields[i]->data[0]^=1; }
            int64_t *scalar[]={&q->state,&q->attempts,&q->retention_until,&q->deadline,&q->duplicate,&q->generation};
            for(size_t i=0;i<6;++i) { *scalar[i]^=1; assert(!jet_rt_handle_eq_jet_delivery_receipt(p,q)); *scalar[i]^=1; }
            q->duplicate=duplicate ? INT64_C(0x102) : INT64_C(0x100); assert(jet_rt_handle_eq_jet_delivery_receipt(p,q));
            Delivery *normalized=jet_rt_handle_clone_jet_delivery_receipt(q);
            watch(normalized,80,8); watch_text(normalized->id); watch_text(normalized->key); watch_text(normalized->authority); watch_text(normalized->signature);
            assert(normalized->duplicate==(int64_t)duplicate); jet_rt_handle_drop_jet_delivery_receipt(normalized);
            Text *id=jet_rt_record_get_jet_delivery_receipt_id(p),*key=jet_rt_record_get_jet_delivery_receipt_idempotency_key(p),*authority=jet_rt_record_get_jet_delivery_receipt_authority(p),*signature=jet_rt_record_get_jet_delivery_receipt_signature(p);
            assert(id!=p->id && key!=p->key && authority!=p->authority && signature!=p->signature);
            watch_text(id); watch_text(key); watch_text(authority); watch_text(signature);
            char render[512];
            int n=snprintf(render,sizeof(render),"DeliveryReceipt(id=id`λ, state=%s, attempts=%" PRId64 ", retention_until=-1, deadline=0, key=key,=, duplicate=%s, authority=authority, generation=%" PRId64 ", signature=sig())",names[tag],INT64_MIN,duplicate ? "true" : "false",INT64_MAX);
            assert(n>0 && (size_t)n<sizeof(render)); own_text_is(jet_rt_handle_show_jet_delivery_receipt(p),render);
            jet_rt_handle_drop_jet_delivery_receipt(p); own_text_is(jet_rt_handle_show_jet_delivery_receipt(q),render);
            text_is(id,"id`λ"); text_is(key,"key,="); text_is(authority,"authority"); text_is(signature,"sig()");
            jet_rt_string_drop(id); jet_rt_string_drop(key); jet_rt_string_drop(authority); jet_rt_string_drop(signature); jet_rt_handle_drop_jet_delivery_receipt(q);
        }
    }
    Delivery *empty=delivery_new(0,0,true);
    own_text_is(jet_rt_record_get_jet_delivery_receipt_id(empty),""); own_text_is(jet_rt_record_get_jet_delivery_receipt_idempotency_key(empty),"");
    own_text_is(jet_rt_record_get_jet_delivery_receipt_authority(empty),""); own_text_is(jet_rt_record_get_jet_delivery_receipt_signature(empty),"");
    jet_rt_handle_drop_jet_delivery_receipt(empty);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"bad-db-lifecycle")) { (void)db_new(3,0,0); }
        else if(!strcmp(argv[1],"bad-delivery-state")) { (void)delivery_new(6,0,false); }
        else if(!strcmp(argv[1],"native-overflow")) {
            int64_t max=jet_int_owned_from_i64(INT64_MAX),one=jet_int_owned_from_i64(1),wide=jet_int_add(max,one),zero=jet_int_owned_from_i64(0);
            (void)jet_rt_record_new_jet_db_pool_receipt(0,wide,zero,zero,zero,0,zero,zero,zero,zero,zero,zero,zero);
        } else { fputs("unknown mode\n",stderr); return 2; }
        fputs("required stop returned\n",stderr); return 1;
    }
    db_cases(); delivery_cases();
    jet_rt_handle_drop_jet_db_pool_receipt(NULL); jet_rt_handle_drop_jet_db_pool_lifecycle(NULL);
    jet_rt_handle_drop_jet_delivery_receipt(NULL); jet_rt_handle_drop_jet_delivery_state(NULL);
    assert(intercepted>0 && watch_count>0);
    for(size_t i=0;i<watch_count;++i) assert(watched[i].freed==1);
    puts("Receipts ownership/equality/render OK"); return 0;
}
