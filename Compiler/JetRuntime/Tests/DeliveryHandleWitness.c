/* Real composed Jet object; link --wrap=jet_rt_free. native-overflow mode
 * must exit70 through the canonical native-Int range stop, with empty stdout. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { Text *id,*store; int64_t duplicate; Text *authority; int64_t generation; } Delivery;
_Static_assert(sizeof(Delivery)==40 && offsetof(Delivery,duplicate)==16 && offsetof(Delivery,generation)==32,"Delivery40");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Delivery *jet_rt_record_new_jet_delivery(Text *,Text *,uint8_t,Text *,int64_t);
extern Delivery *jet_rt_handle_clone_jet_delivery(Delivery *);
extern bool jet_rt_handle_eq_jet_delivery(Delivery *,Delivery *);
extern void jet_rt_handle_drop_jet_delivery(Delivery *);
extern Text *jet_rt_handle_show_jet_delivery(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_id(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_store(Delivery *);
extern bool jet_rt_record_get_jet_delivery_duplicate(Delivery *);
extern Text *jet_rt_record_get_jet_delivery_authority(Delivery *);
extern int64_t jet_rt_record_get_jet_delivery_generation(Delivery *);
extern Delivery *jet_services_raw_delivery_allocate(Text *,Text *,uint8_t,Text *,int64_t);
extern void jet_services_raw_delivery_identity(Delivery *,Text **,Text **,bool *,Text **,int64_t *);
extern Text *jet_services_raw_delivery_store(Delivery *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[2048];
static size_t count;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==watched[i-1].align); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *t) { watch((char *)(t)-8,32,8); if(t->cap) watch(t->data,t->cap,1); }
static void delivery_watch(Delivery *d) { watch(d,40,8); text_watch(d->id); text_watch(d->store); text_watch(d->authority); }
static void equal_text(Text *a,Text *b) { assert(a->len==b->len && !memcmp(a->data,b->data,(size_t)a->len)); }
static void different(Text *a,Text *b) { assert(a!=b && a->data!=b->data); equal_text(a,b); }
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    unsigned char binary[]={'d',0,0xc3,0xa9}; Text input={binary,4,4},empty={(unsigned char *)"",0,0};
    if(argc==2) {
        assert(!strcmp(argv[1],"native-overflow"));
        int64_t max=jet_int_owned_from_i64(INT64_MAX),one=jet_int_owned_from_i64(1),wide=jet_int_add(max,one);
        (void)jet_rt_record_new_jet_delivery(&input,&input,0,&input,wide); assert(!"native range stop returned");
    }
    jet_rt_handle_drop_jet_delivery(NULL);
    const int64_t generations[]={INT64_MIN,-1,0,1,INT64_MAX}; const uint8_t bools[]={0,1,2,255};
    for(size_t i=0;i<5;++i) for(size_t j=0;j<4;++j) {
        Text *text=jet_rt_string_clone(&input); text_watch(text);
        int64_t exact=jet_int_owned_from_i64(generations[i]);
        Delivery *a=jet_services_raw_delivery_allocate(text,text,bools[j],text,generations[i]); delivery_watch(a);
        assert(a->generation==generations[i] && a->duplicate==(bools[j]!=0)); different(text,a->id); different(text,a->store); different(text,a->authority);
        Delivery *constructed=jet_rt_record_new_jet_delivery(text,text,bools[j],text,exact); delivery_watch(constructed);
        assert(jet_rt_handle_eq_jet_delivery(a,constructed)); jet_rt_handle_drop_jet_delivery(constructed);
        jet_rt_int_drop(exact); jet_rt_string_drop(text);
        Delivery *b=jet_rt_handle_clone_jet_delivery(a); delivery_watch(b); assert(a!=b && jet_rt_handle_eq_jet_delivery(a,b));
        different(a->id,b->id); different(a->store,b->store); different(a->authority,b->authority);
        b->id->data[0]^=1; assert(!jet_rt_handle_eq_jet_delivery(a,b)); b->id->data[0]^=1;
        b->store->data[0]^=1; assert(!jet_rt_handle_eq_jet_delivery(a,b)); b->store->data[0]^=1;
        b->duplicate^=1; assert(!jet_rt_handle_eq_jet_delivery(a,b)); b->duplicate^=1;
        b->authority->data[0]^=1; assert(!jet_rt_handle_eq_jet_delivery(a,b)); b->authority->data[0]^=1;
        b->generation=generations[i]==0 ? 1 : 0; assert(!jet_rt_handle_eq_jet_delivery(a,b)); b->generation=generations[i];
        a->duplicate=bools[j] ? 0x102 : 0x100; assert(jet_rt_handle_eq_jet_delivery(a,b));
        assert(jet_rt_record_get_jet_delivery_duplicate(a)==(bools[j]!=0));
        Text *id=jet_rt_record_get_jet_delivery_id(a),*store=jet_rt_record_get_jet_delivery_store(a),*authority=jet_rt_record_get_jet_delivery_authority(a);
        text_watch(id); text_watch(store); text_watch(authority); different(a->id,id); different(a->store,store); different(a->authority,authority);
        int64_t got=jet_rt_record_get_jet_delivery_generation(a); assert(jet_int_owned_to_i64(got)==generations[i]); jet_rt_int_drop(got);
        Text *raw_id,*raw_store,*raw_authority;
        struct { bool value; unsigned char guard[7]; } raw_bool={false,{11,22,33,44,55,66,77}};
        int64_t raw_generation;
        jet_services_raw_delivery_identity(a,&raw_id,&raw_store,&raw_bool.value,&raw_authority,&raw_generation);
        text_watch(raw_id); text_watch(raw_store); text_watch(raw_authority);
        different(a->id,raw_id); different(a->store,raw_store); different(a->authority,raw_authority);
        assert(raw_bool.value==(bools[j]!=0) && !memcmp(raw_bool.guard,(unsigned char[]){11,22,33,44,55,66,77},7));
        assert(jet_int_owned_to_i64(raw_generation)==generations[i]); jet_rt_int_drop(raw_generation);
        Text *raw_store_copy=jet_services_raw_delivery_store(a); text_watch(raw_store_copy); different(a->store,raw_store_copy);
        jet_rt_string_drop(raw_id); jet_rt_string_drop(raw_store); jet_rt_string_drop(raw_authority); jet_rt_string_drop(raw_store_copy);
        Text *shown=jet_rt_handle_show_jet_delivery(a); text_watch(shown);
        unsigned char expected[14]={'D','e','l','i','v','e','r','y','(',0,0,0,0,')'}; memcpy(expected+9,binary,4);
        assert(shown->len==14 && !memcmp(shown->data,expected,14));
        jet_rt_handle_drop_jet_delivery(a); equal_text(id,&input); equal_text(store,&input); equal_text(authority,&input); equal_text(b->id,&input);
        jet_rt_string_drop(shown); jet_rt_string_drop(id); jet_rt_string_drop(store); jet_rt_string_drop(authority); jet_rt_handle_drop_jet_delivery(b);
    }
    int64_t zero=jet_int_owned_from_i64(0); Delivery *a=jet_rt_record_new_jet_delivery(&empty,&empty,0,&empty,zero); delivery_watch(a); jet_rt_int_drop(zero);
    Text *shown=jet_rt_handle_show_jet_delivery(a); text_watch(shown); assert(shown->len==10 && !memcmp(shown->data,"Delivery()",10));
    jet_rt_string_drop(shown); jet_rt_handle_drop_jet_delivery(a);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("delivery: borrowed ctor/deep clone/owned getters, exact generation i64 edges, Bool low-byte normalization, all equality fields, raw binary Show, exact frees, null drop OK");
    return 0;
}
