// Link the real Jet object with --wrap=jet_rt_free. This is not a provider.
// Native layout mirrors NetErrorABI; Rust cites NetHTTP149-181,530-570 and NetError7-22.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *operation,*address,*name,*message; int64_t present,code; } Detail;
typedef struct { int64_t tag; Text *text; } DNS;
typedef struct { int64_t tag; void *payload; } Error;
_Static_assert(sizeof(Detail)==48 && offsetof(Detail,code)==40, "Net Detail48");
_Static_assert(sizeof(DNS)==16 && offsetof(DNS,text)==8, "Net DNS16");
_Static_assert(sizeof(Error)==16 && offsetof(Error,payload)==8, "Net Error16");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Detail *jet_rt_record_new_jet_net_error_detail(Text *,const Text **,const Text **,Text *,const int64_t *);
extern Detail *jet_rt_handle_clone_jet_net_error_detail(Detail *);
extern void jet_rt_handle_drop_jet_net_error_detail(Detail *);
extern bool jet_rt_handle_eq_jet_net_error_detail(Detail *,Detail *);
extern Text *jet_rt_record_get_jet_net_error_detail_operation(Detail *);
extern Text *jet_rt_record_get_jet_net_error_detail_message(Detail *);
extern int64_t jet_rt_record_get_jet_net_error_detail_address(Detail *,Text **);
extern int64_t jet_rt_record_get_jet_net_error_detail_name(Detail *,Text **);
extern int64_t jet_rt_record_get_jet_net_error_detail_os_code(Detail *,int64_t *);
extern DNS *jet_rt_record_new_jet_net_dns_error_not_found(Text *);
extern DNS *jet_rt_record_new_jet_net_dns_error_failure(Text *);
extern Text *jet_rt_record_get_jet_net_dns_error_not_found(DNS *);
extern Text *jet_rt_record_get_jet_net_dns_error_failure(DNS *);
extern int64_t jet_rt_record_tag_jet_net_dns_error(DNS *);
extern DNS *jet_rt_handle_clone_jet_net_dns_error(DNS *);
extern void jet_rt_handle_drop_jet_net_dns_error(DNS *);
extern bool jet_rt_handle_eq_jet_net_dns_error(DNS *,DNS *);
extern Error *jet_rt_handle_clone_jet_net_error(Error *);
extern void jet_rt_handle_drop_jet_net_error(Error *);
extern bool jet_rt_handle_eq_jet_net_error(Error *,Error *);
extern int64_t jet_rt_record_tag_jet_net_error(Error *);
extern Text *jet_rt_handle_show_jet_net_error(Error *);
extern Text *jet_rt_handle_display_jet_net_error(Error *);
extern Text *jet_rt_handle_debug_jet_net_error(Error *);
#define DETAIL_VARIANTS(X) X(InvalidInput) X(PermissionDenied) X(AddressInUse) X(AddressUnavailable) X(ConnectionRefused) X(ConnectionReset) X(NotConnected) X(Closed) X(Timeout) X(Cancelled) X(Unsupported) X(TLS) X(Protocol) X(Other)
#define DECLARE(NAME) extern Error *jet_rt_record_new_jet_net_error_##NAME(Detail *); extern Detail *jet_rt_record_get_jet_net_error_##NAME(Error *);
DETAIL_VARIANTS(DECLARE)
#undef DECLARE
extern Error *jet_rt_record_new_jet_net_error_dns(DNS *);
extern DNS *jet_rt_record_get_jet_net_error_dns(Error *);
#define NEW(NAME) jet_rt_record_new_jet_net_error_##NAME,
static Error *(*const constructors[])(Detail *)={DETAIL_VARIANTS(NEW)};
#undef NEW
#define GET(NAME) jet_rt_record_get_jet_net_error_##NAME,
static Detail *(*const getters[])(Error *)={DETAIL_VARIANTS(GET)};
#undef GET
static const int64_t tags[]={0,1,2,3,4,5,6,7,8,9,10,12,13,14}; // C ABI DNS11; render DNS14
static struct { void *pointer; int64_t bytes,align; unsigned frees; } watched[128];
static size_t watched_count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<watched_count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align && ++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && watched_count<128);
    for(size_t i=0;i<watched_count;++i) assert(watched[i].pointer!=p);
    watched[watched_count].pointer=p; watched[watched_count].bytes=bytes; watched[watched_count++].align=align;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_detail(Detail *d) { watch(d,48,8); watch_text(d->operation); if(d->address) watch_text(d->address); if(d->name) watch_text(d->name); watch_text(d->message); }
static void watch_error(Error *e) { watch(e,16,8); if(e->tag==11) { DNS *d=e->payload; watch(d,16,8); watch_text(d->text); } else watch_detail(e->payload); }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(!memcmp(t->data,s,(size_t)t->length)); }
static void render(Error *e,const char *s) {
    Text *a=jet_rt_handle_show_jet_net_error(e), *b=jet_rt_handle_display_jet_net_error(e), *c=jet_rt_handle_debug_jet_net_error(e);
    assert(a!=b && b!=c && a!=c); text_is(a,s); text_is(b,s); text_is(c,s);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c);
}
static Detail *detail(const char *address,const char *name,const int64_t *code) {
    Text *op=text("connect"), *msg=text("failure λ"), *addr=address ? text(address) : NULL, *nm=name ? text(name) : NULL;
    Detail *d=jet_rt_record_new_jet_net_error_detail(op,(const Text **)(addr ? &addr : NULL),(const Text **)(nm ? &nm : NULL),msg,code);
    assert(d->operation!=op && d->message!=msg);
    if(addr) assert(d->address!=addr && d->address->data!=addr->data);
    if(nm) assert(d->name!=nm && (nm->length==0 || d->name->data!=nm->data));
    jet_rt_string_drop(op); jet_rt_string_drop(msg); if(addr) jet_rt_string_drop(addr); if(nm) jet_rt_string_drop(nm);
    return d;
}
static void truth(void) {
    int64_t zero=jet_int_owned_from_i64(0);
    Detail *a=detail("address","name",&zero), *b=jet_rt_handle_clone_jet_net_error_detail(a);
    assert(jet_rt_handle_eq_jet_net_error_detail(a,a) && jet_rt_handle_eq_jet_net_error_detail(a,b));
    Text *fields[]={b->operation,b->address,b->name,b->message};
    for(size_t i=0;i<4;++i) { fields[i]->data[0]^=1; assert(!jet_rt_handle_eq_jet_net_error_detail(a,b)); fields[i]->data[0]^=1; }
    b->present=0; assert(!jet_rt_handle_eq_jet_net_error_detail(a,b)); b->present=1;
    b->code=1; assert(!jet_rt_handle_eq_jet_net_error_detail(a,b)); b->code=0;
    Text *saved=b->address; b->address=NULL; assert(!jet_rt_handle_eq_jet_net_error_detail(a,b)); b->address=saved;
    saved=b->name; b->name=NULL; assert(!jet_rt_handle_eq_jet_net_error_detail(a,b)); b->name=saved;
    jet_rt_handle_drop_jet_net_error_detail(a); jet_rt_handle_drop_jet_net_error_detail(b);
    a=detail(NULL,NULL,NULL); b=detail("","",NULL); assert(!jet_rt_handle_eq_jet_net_error_detail(a,b));
    Text *out=(Text *)(uintptr_t)99; int64_t code=99;
    assert(!jet_rt_record_get_jet_net_error_detail_address(a,&out) && out==(Text *)(uintptr_t)99);
    assert(!jet_rt_record_get_jet_net_error_detail_name(a,&out) && out==(Text *)(uintptr_t)99);
    assert(!jet_rt_record_get_jet_net_error_detail_os_code(a,&code) && code==99);
    Detail *c=jet_rt_handle_clone_jet_net_error_detail(a); a->code=12; c->code=13; assert(jet_rt_handle_eq_jet_net_error_detail(a,c));
    Detail *present=detail(NULL,NULL,&zero); assert(!jet_rt_handle_eq_jet_net_error_detail(a,present));
    jet_rt_handle_drop_jet_net_error_detail(a); jet_rt_handle_drop_jet_net_error_detail(b); jet_rt_handle_drop_jet_net_error_detail(c); jet_rt_handle_drop_jet_net_error_detail(present);
    jet_rt_int_drop(zero);
}
static void all_variants(void) {
    int64_t max=jet_int_owned_from_i64(INT64_MAX);
    for(size_t i=0;i<14;++i) {
        Detail *d=detail("address","name",&max); Error *a=constructors[i](d);
        assert(a->payload==d && jet_rt_record_tag_jet_net_error(a)==tags[i]);
        Error *b=jet_rt_handle_clone_jet_net_error(a); Detail *g=getters[i](a);
        assert(b!=a && b->payload!=d && g!=d && jet_rt_handle_eq_jet_net_error(a,b));
        assert(jet_rt_handle_eq_jet_net_error_detail(d,g));
        b->tag=(b->tag+1)%15; assert(!jet_rt_handle_eq_jet_net_error(a,b)); b->tag=tags[i];
        ((Detail *)b->payload)->message->data[0]^=1; assert(!jet_rt_handle_eq_jet_net_error(a,b)); ((Detail *)b->payload)->message->data[0]^=1;
        render(a,"failure λ"); jet_rt_handle_drop_jet_net_error(a); text_is(g->message,"failure λ");
        render(b,"failure λ"); jet_rt_handle_drop_jet_net_error(b); jet_rt_handle_drop_jet_net_error_detail(g);
    }
    for(int64_t tag=0;tag<2;++tag) {
        Text *s=text(tag==0 ? "λ.example`" : "DNS failed λ");
        DNS *d=tag==0 ? jet_rt_record_new_jet_net_dns_error_not_found(s) : jet_rt_record_new_jet_net_dns_error_failure(s);
        assert(d->text!=s && jet_rt_record_tag_jet_net_dns_error(d)==tag); jet_rt_string_drop(s);
        DNS *copy=jet_rt_handle_clone_jet_net_dns_error(d); assert(copy!=d && copy->text!=d->text && jet_rt_handle_eq_jet_net_dns_error(copy,d));
        copy->tag^=1; assert(!jet_rt_handle_eq_jet_net_dns_error(copy,d)); copy->tag^=1;
        copy->text->data[0]^=1; assert(!jet_rt_handle_eq_jet_net_dns_error(copy,d)); copy->text->data[0]^=1;
        Text *g=tag==0 ? jet_rt_record_get_jet_net_dns_error_not_found(d) : jet_rt_record_get_jet_net_dns_error_failure(d); assert(g!=d->text); jet_rt_string_drop(g);
        Error *e=jet_rt_record_new_jet_net_error_dns(d); assert(e->payload==d && jet_rt_record_tag_jet_net_error(e)==11);
        DNS *payload=jet_rt_record_get_jet_net_error_dns(e); Error *clone=jet_rt_handle_clone_jet_net_error(e);
        assert(payload!=d && jet_rt_handle_eq_jet_net_dns_error(payload,d) && jet_rt_handle_eq_jet_net_error(e,clone));
        render(e,tag==0 ? "DNS name not found: `λ.example``" : "DNS failed λ");
        jet_rt_handle_drop_jet_net_error(e); assert(jet_rt_handle_eq_jet_net_dns_error(payload,copy));
        jet_rt_handle_drop_jet_net_error(clone); jet_rt_handle_drop_jet_net_dns_error(payload); jet_rt_handle_drop_jet_net_dns_error(copy);
    }
    jet_rt_int_drop(max);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"wrong-variant")) {
            int64_t zero=jet_int_owned_from_i64(0); Error *e=jet_rt_record_new_jet_net_error_invalid_input(detail(NULL,NULL,&zero));
            (void)jet_rt_record_get_jet_net_error_other(e);
        } else if(!strcmp(argv[1],"wrong-dns-variant")) {
            Text *name=text("example"); DNS *dns=jet_rt_record_new_jet_net_dns_error_not_found(name);
            (void)jet_rt_record_get_jet_net_dns_error_failure(dns);
        } else if(!strcmp(argv[1],"native-overflow")) {
            int64_t max=jet_int_owned_from_i64(INT64_MAX), one=jet_int_owned_from_i64(1), wide=jet_int_add(max,one);
            (void)detail(NULL,NULL,&wide);
        } else { fputs("unknown mode\n",stderr); return 2; }
        fputs("required stop returned\n",stderr); return 1;
    }
    truth(); all_variants();
    int64_t min=jet_int_owned_from_i64(INT64_MIN);
    Error *a=jet_rt_record_new_jet_net_error_other(detail("address","name",&min)); jet_rt_int_drop(min);
    Error *b=jet_rt_handle_clone_jet_net_error(a); Detail *g=jet_rt_record_get_jet_net_error_other(a), *d=a->payload;
    Text *op=jet_rt_record_get_jet_net_error_detail_operation(g), *msg=jet_rt_record_get_jet_net_error_detail_message(g), *addr=NULL,*name=NULL;
    assert(jet_rt_record_get_jet_net_error_detail_address(g,&addr)==1 && jet_rt_record_get_jet_net_error_detail_name(g,&name)==1);
    assert(op!=g->operation && msg!=g->message && addr!=g->address && name!=g->name);
    int64_t code=0,code2=0; assert(jet_rt_record_get_jet_net_error_detail_os_code(g,&code)==1 && jet_rt_record_get_jet_net_error_detail_os_code(g,&code2)==1);
    assert(code!=code2 && jet_int_owned_to_i64(code)==INT64_MIN && jet_int_owned_to_i64(code2)==INT64_MIN);
    jet_rt_int_drop(code); jet_rt_int_drop(code2);
    assert(d!=g && d->operation!=g->operation && d->operation->data!=g->operation->data);
    assert(d->address!=g->address && d->name!=g->name && d->message!=g->message);
    Text *s=text("watched.example"); DNS *dns=jet_rt_record_new_jet_net_dns_error_not_found(s); jet_rt_string_drop(s);
    Error *dns_error=jet_rt_record_new_jet_net_error_dns(dns), *dns_clone=jet_rt_handle_clone_jet_net_error(dns_error);
    // No more allocations while these tracked owners are being destroyed.
    watch_error(a); watch_error(b); watch_detail(g); watch_text(op); watch_text(msg); watch_text(addr); watch_text(name); watch_error(dns_error); watch_error(dns_clone);
    jet_rt_handle_drop_jet_net_error(a); text_is(g->message,"failure λ"); text_is(((Detail *)b->payload)->message,"failure λ");
    text_is(op,"connect"); text_is(addr,"address"); text_is(name,"name"); text_is(msg,"failure λ");
    jet_rt_handle_drop_jet_net_error(b); jet_rt_handle_drop_jet_net_error_detail(g);
    jet_rt_string_drop(op); jet_rt_string_drop(msg); jet_rt_string_drop(addr); jet_rt_string_drop(name);
    jet_rt_handle_drop_jet_net_error(dns_error); text_is(((DNS *)dns_clone->payload)->text,"watched.example"); jet_rt_handle_drop_jet_net_error(dns_clone);
    assert(intercepted>0 && watched_count>0); for(size_t i=0;i<watched_count;++i) assert(watched[i].frees==1);
    jet_rt_handle_drop_jet_net_error(NULL); jet_rt_handle_drop_jet_net_error_detail(NULL); jet_rt_handle_drop_jet_net_dns_error(NULL);
    puts("NetErrors ownership/equality/render OK"); return 0;
}
