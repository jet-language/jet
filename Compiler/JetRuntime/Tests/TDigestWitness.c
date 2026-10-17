// Real providers, -pthread -Wl,--wrap=jet_rt_free. Pass TDigestVectors.txt as argv[1].
#include <assert.h>
#include <inttypes.h>
#include <math.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len,capacity; } Text;
typedef struct { double mean,weight; } Centroid;
typedef struct { int64_t refs,mutex,len,capacity; Centroid *data; } TD;
_Static_assert(sizeof(TD)==40 && sizeof(Centroid)==16,"TD40 and centroid16");
extern void jet_rt_alloc_init(void);
extern TD *jet_t_digest_new(void);
extern TD *jet_rt_handle_clone_jet_t_digest(TD *);
extern void jet_rt_handle_drop_jet_t_digest(TD *);
extern Text *jet_rt_handle_show_jet_t_digest(TD *);
extern void jet_rt_native_tdigest_add(TD *,double);
extern double jet_rt_native_tdigest_quantile(TD *,double);
extern void jet_rt_string_drop(Text *);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
static TD *watched;
static Centroid *watched_buffer;
static int64_t watched_capacity;
static unsigned shell_frees,buffer_frees;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    if(p==watched){assert(bytes==40 && align==8);assert(++shell_frees==1);}
    if(p==watched_buffer){assert(bytes==watched_capacity*16 && align==8);assert(++buffer_frees==1);}
    __real_jet_rt_free(p,bytes,align);
}
static uint64_t bits(double v){uint64_t b;memcpy(&b,&v,8);return b;}
static double from(uint64_t b){double v;memcpy(&v,&b,8);return v;}
static void q(TD *td,double quantile,uint64_t expected){assert(bits(jet_rt_native_tdigest_quantile(td,quantile))==expected);}
static void *adds(void *p){for(unsigned i=0;i<1000;++i)jet_rt_native_tdigest_add(p,42.0);return NULL;}
static void nan_cases(void){
    const uint64_t nan=UINT64_C(0x7ff8000000000000),one=UINT64_C(0x3ff0000000000000),half=UINT64_C(0x3ff8000000000000),two=UINT64_C(0x4000000000000000);
    const uint64_t initial[3][3]={{nan,one,two},{one,nan,two},{one,two,nan}};
    const uint64_t expected[3][4]={{nan,one,half,two},{one,half,nan,two},{one,half,two,nan}};
    for(unsigned n=0;n<3;++n){
        TD *td=jet_t_digest_new();
        for(unsigned i=0;i<3;++i)jet_rt_native_tdigest_add(td,(double)i);
        assert(td->len==3);
        for(unsigned i=0;i<3;++i)td->data[i].mean=from(initial[n][i]);
        jet_rt_native_tdigest_add(td,1.5);assert(td->len==4);
        for(unsigned i=0;i<4;++i){assert(bits(td->data[i].mean)==expected[n][i]);assert(td->data[i].weight==1.0);}
        q(td,0.0,expected[n][0]);q(td,0.5,expected[n][1]);q(td,1.0,expected[n][3]);q(td,from(nan),expected[n][3]);
        jet_rt_handle_drop_jet_t_digest(td);
    }
}
int main(int argc,char **argv){
    assert(argc==2);jet_rt_alloc_init();
    TD *td=jet_t_digest_new();watched=td;
    assert(td->refs==1 && td->mutex==0 && td->len==0 && td->capacity==0);
    q(td,0.5,0);q(td,NAN,0);
    TD *a=jet_rt_handle_clone_jet_t_digest(td),*b=jet_rt_handle_clone_jet_t_digest(a);
    assert(td==a && a==b && td->refs==3);
    for(unsigned i=0;i<512;++i)jet_rt_native_tdigest_add(a,(double)((i*37)%97)-48.0);
    FILE *vectors=fopen(argv[1],"r");assert(vectors);
    int64_t index=0;uint64_t mean,weight;
    while(fscanf(vectors,"%" SCNx64 " %" SCNx64,&mean,&weight)==2){
        if(index>=b->len || bits(b->data[index].mean)!=mean || bits(b->data[index].weight)!=weight){
            fprintf(stderr,"TDigest centroid mismatch at index %" PRId64 "\n",index);assert(0);
        }
        ++index;
    }
    assert(feof(vectors) && index==250 && index==b->len);assert(fclose(vectors)==0);
    const double quantiles[]={-1,0,.01,.1,.5,.9,.99,1,2,NAN};
    const uint64_t expected[]={UINT64_C(0xc048000000000000),UINT64_C(0xc048000000000000),UINT64_C(0xc048000000000000),UINT64_C(0xc043800000000000),UINT64_C(0x3fc999999999999a),UINT64_C(0x4043800000000000),UINT64_C(0x4047800000000000),UINT64_C(0x4048000000000000),UINT64_C(0x4048000000000000),UINT64_C(0x4048000000000000)};
    for(unsigned i=0;i<10;++i)q(b,quantiles[i],expected[i]);
    Text *text=jet_rt_handle_show_jet_t_digest(b);assert(text->len==7 && memcmp(text->data,"TDigest",7)==0);jet_rt_string_drop(text);
    watched_buffer=b->data;watched_capacity=b->capacity;
    jet_rt_handle_drop_jet_t_digest(td);assert(shell_frees==0 && buffer_frees==0 && a->refs==2);
    jet_rt_handle_drop_jet_t_digest(a);assert(shell_frees==0 && buffer_frees==0 && b->refs==1);
    jet_rt_handle_drop_jet_t_digest(NULL);jet_rt_handle_drop_jet_t_digest(b);assert(shell_frees==1 && buffer_frees==1);
    watched=NULL;watched_buffer=NULL;
    nan_cases();
    TD *shared=jet_t_digest_new();pthread_t threads[3];
    for(unsigned i=0;i<3;++i)assert(pthread_create(&threads[i],NULL,adds,shared)==0);
    for(unsigned i=0;i<3;++i)assert(pthread_join(threads[i],NULL)==0);
    double total=0;for(int64_t i=0;i<shared->len;++i){assert(shared->data[i].mean==42);total+=shared->data[i].weight;}
    assert(total==3000);q(shared,.5,UINT64_C(0x4045000000000000));jet_rt_handle_drop_jet_t_digest(shared);
    TD *edge=jet_t_digest_new();jet_rt_native_tdigest_add(edge,-0.0);q(edge,0,UINT64_C(0x8000000000000000));jet_rt_handle_drop_jet_t_digest(edge);
    edge=jet_t_digest_new();jet_rt_native_tdigest_add(edge,INFINITY);q(edge,1,UINT64_C(0x7ff0000000000000));jet_rt_handle_drop_jet_t_digest(edge);
    edge=jet_t_digest_new();jet_rt_native_tdigest_add(edge,-INFINITY);q(edge,0,UINT64_C(0xfff0000000000000));jet_rt_handle_drop_jet_t_digest(edge);
    puts("TDigest frozen centroids/NaN/shared/quantile/drop OK");
}
