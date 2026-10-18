// Link real providers with -pthread -Wl,--wrap=jet_rt_free. overflow mode expects canonical overflow stop.
#include <assert.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { char *data; int64_t len,capacity; } Text;
typedef struct { int64_t len,capacity; Text **data; } List;
typedef struct { int64_t refs,mutex,limit; uint64_t count,rng; List items; } Reservoir;
_Static_assert(sizeof(List)==24 && sizeof(Reservoir)==64,"canonical List24 / Reservoir64");
extern void jet_rt_alloc_init(void);
extern Reservoir *jet_reservoir_sampler_new(int64_t);
extern Reservoir *jet_rt_handle_clone_jet_reservoir_sampler(Reservoir *);
extern void jet_rt_handle_drop_jet_reservoir_sampler(Reservoir *);
extern Text *jet_rt_handle_show_jet_reservoir_sampler(Reservoir *);
extern void jet_rt_native_reservoir_add(Reservoir *,const Text *);
extern List *jet_rt_native_reservoir_sample(Reservoir *);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
static Reservoir *watched;
static void *pointers[34];
static int64_t sizes[34];
static int64_t aligns[34];
static unsigned nwatched,freed[34];
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align){
    for(unsigned i=0;i<nwatched;++i)if(p==pointers[i]){assert(bytes==sizes[i] && align==aligns[i]);assert(++freed[i]==1);}
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align){assert(nwatched<34);pointers[nwatched]=p;sizes[nwatched]=bytes;aligns[nwatched]=align;freed[nwatched++]=0;}
static void watch_owner(Reservoir *r){
    watched=r;nwatched=0;watch(r,64,8);watch(r->items.data,r->items.capacity*8,8);
    for(int64_t i=0;i<r->items.len;++i){Text *t=r->items.data[i];watch((char *)(t)-8,32,8);if(t->capacity)watch(t->data,t->capacity,1);}
}
static void expect(const Text *t,const char *s,int64_t len){assert(t->len==len && memcmp(t->data,s,(size_t)len)==0);}
static void show(Reservoir *r,const char *s){Text *t=jet_rt_handle_show_jet_reservoir_sampler(r);expect(t,s,(int64_t)strlen(s));jet_rt_string_drop(t);}
static void drop_list(List *list){for(int64_t i=0;i<list->len;++i)jet_rt_string_drop(list->data[i]);if(list->capacity)jet_rt_free(list->data,list->capacity*8,8);jet_rt_free(list,24,8);}
static void *adds(void *p){for(unsigned i=0;i<1000;++i){Text *text=jet_rt_string_from_static("shared",6);jet_rt_native_reservoir_add(p,text);jet_rt_string_drop(text);}return NULL;}
int main(int argc,char **argv){
    jet_rt_alloc_init();
    if(argc==2 && strcmp(argv[1],"overflow")==0){Reservoir *r=jet_reservoir_sampler_new(1);r->count=UINT64_MAX;Text t={"x",1,0};jet_rt_native_reservoir_add(r,&t);return 1;}
    assert(argc==1);
    // Actual seed Sketch.rs oracle: capacity clamping, RNG bits and every output slot.
    static const int64_t capacities[]={-1,0,1,4,16};
    static const unsigned lengths[]={1,1,1,4,16};
    static const unsigned indices[][16]={{57},{57},{57},{13,18,105,86},{0,73,77,52,64,54,98,59,69,102,70,11,41,44,95,67}};
    static const uint64_t rng[]={UINT64_C(16206406808196824173),UINT64_C(16206406808196824173),UINT64_C(16206406808196824173),UINT64_C(5777307760054783851),UINT64_C(2515031985831341802)};
    for(unsigned c=0;c<5;++c){
        Reservoir *r=jet_reservoir_sampler_new(capacities[c]);
        assert(r->refs==1 && r->mutex==0 && r->limit==(capacities[c]<1?1:capacities[c]));
        assert(r->count==0 && r->rng==UINT64_C(0xdeadbeefcafebabe) && r->items.len==0 && r->items.capacity==0);
        show(r,"ReservoirSampler(n=0)");List *empty=jet_rt_native_reservoir_sample(r);assert(empty->len==0);drop_list(empty);
        Reservoir *a=jet_rt_handle_clone_jet_reservoir_sampler(r),*b=jet_rt_handle_clone_jet_reservoir_sampler(a);assert(r==a && a==b && r->refs==3);
        for(unsigned i=0;i<128;++i){char word[32];int n=snprintf(word,sizeof(word),"sample-%u",i);Text *text=jet_rt_string_from_static(word,n);jet_rt_native_reservoir_add(a,text);jet_rt_string_drop(text);memset(word,'?',(size_t)n);}
        assert(b->count==128 && b->rng==rng[c] && b->items.len==lengths[c]);show(b,"ReservoirSampler(n=128)");
        List *sample=jet_rt_native_reservoir_sample(b);assert(sample->len==lengths[c]);
        for(unsigned i=0;i<lengths[c];++i){char word[32];int n=snprintf(word,sizeof(word),"sample-%u",indices[c][i]);expect(b->items.data[i],word,n);expect(sample->data[i],word,n);assert(sample->data[i]!=b->items.data[i] && sample->data[i]->data!=b->items.data[i]->data);}
        watch_owner(r);
        jet_rt_handle_drop_jet_reservoir_sampler(r);assert(watched==a && a->refs==2);
        jet_rt_handle_drop_jet_reservoir_sampler(a);assert(b->refs==1);
        jet_rt_handle_drop_jet_reservoir_sampler(NULL);
        for(unsigned i=0;i<nwatched;++i)assert(freed[i]==0);
        jet_rt_handle_drop_jet_reservoir_sampler(b);for(unsigned i=0;i<nwatched;++i)assert(freed[i]==1);
        watched=NULL;nwatched=0;
        for(unsigned i=0;i<lengths[c];++i){char word[32];int n=snprintf(word,sizeof(word),"sample-%u",indices[c][i]);expect(sample->data[i],word,n);}
        drop_list(sample);
    }
    Reservoir *raw=jet_reservoir_sampler_new(INT64_MAX); // Seed allocates lazily, not a capacity-sized eager allocation.
    char unicode[]={ 's',0,(char)0xe9,(char)0x9b,(char)0xaa };Text *text=jet_rt_string_from_static(unicode,5);Text empty={"",0,0};
    jet_rt_native_reservoir_add(raw,text);jet_rt_string_drop(text);jet_rt_native_reservoir_add(raw,&empty);
    List *sample=jet_rt_native_reservoir_sample(raw);memset(unicode,'?',5);assert(sample->len==2);expect(sample->data[0],"s\0\xe9\x9b\xaa",5);expect(sample->data[1],"",0);
    jet_rt_handle_drop_jet_reservoir_sampler(raw);expect(sample->data[0],"s\0\xe9\x9b\xaa",5);drop_list(sample);
    Reservoir *shared=jet_reservoir_sampler_new(4);pthread_t threads[3];
    for(unsigned i=0;i<3;++i)assert(pthread_create(&threads[i],NULL,adds,shared)==0);
    for(unsigned i=0;i<3;++i)assert(pthread_join(threads[i],NULL)==0);
    assert(shared->count==3000 && shared->items.len==4);for(unsigned i=0;i<4;++i)expect(shared->items.data[i],"shared",6);
    show(shared,"ReservoirSampler(n=3000)");
    // Exercise the unsigned half without billions of calls; one real add updates the full U64 count.
    shared->count=INT64_MAX;jet_rt_native_reservoir_add(shared,&empty);assert(shared->count==UINT64_C(9223372036854775808));show(shared,"ReservoirSampler(n=9223372036854775808)");
    shared->count=UINT64_MAX-1;jet_rt_native_reservoir_add(shared,&empty);assert(shared->count==UINT64_MAX);show(shared,"ReservoirSampler(n=18446744073709551615)");
    jet_rt_handle_drop_jet_reservoir_sampler(shared);
    puts("Reservoir frozen RNG/sample/shared/unsigned/drop OK");
}
