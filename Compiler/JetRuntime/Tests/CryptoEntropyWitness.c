// Native callback transport for the exact seed provider contract. NO crypto
// implementation: scripted steps enter the same private Rand adapter as live.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <pthread.h>
extern void jet_rt_alloc_init(void);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_crypto_entropy_init(void);
extern void jet_rt_entropy_fixture_set(int64_t,int64_t);
extern void jet_rt_entropy_fixture_clear(void);
extern int64_t jet_rt_entropy_fixture_fill(void *,int64_t);
typedef struct { unsigned calls; int mode; unsigned char *base; } Script;
static int64_t step(int64_t raw,int64_t address,int64_t count) {
    Script *s=(Script *)(uintptr_t)raw;unsigned char *out=(unsigned char *)(uintptr_t)address;
    ++s->calls;
    if(s->mode==0) {
        if(s->calls==1){assert(out==s->base&&count==7);out[0]=1;out[1]=2;return 2;}
        if(s->calls==2){assert(out==s->base+2&&count==5);memset(out,0xa5,5);return -4;}
        assert(s->calls==3&&out==s->base+2&&count==5);for(int i=0;i<5;++i){assert(out[i]==0);out[i]=(unsigned char)(i+3);}return 5;
    }
    if(s->mode==1||s->mode==2||s->mode==3){assert(count==32);memset(out,0x5a,8);return s->mode==1?0:(s->mode==2?-5:33);}
    if(s->mode==4){if(s->calls==1){assert(count==16);out[0]=1;out[1]=2;out[2]=3;out[3]=4;return 4;}assert(count==12);return -5;}
    assert(s->mode==5);memset(out,0xa5,(size_t)count);return -38;
}
static void set(Script *s){jet_rt_entropy_fixture_set((int64_t)(uintptr_t)step,(int64_t)(uintptr_t)s);}
static void zero(const unsigned char *p,size_t n){for(size_t i=0;i<n;++i)assert(p[i]==0);}
static unsigned char live[8][64];
static void *worker(void *arg){uintptr_t i=(uintptr_t)arg;jet_rt_thread_attach();assert(jet_rt_entropy_fixture_fill(live[i],64)==0);jet_rt_thread_detach();return NULL;}
int main(void){
    jet_rt_alloc_init();jet_rt_thread_init();jet_rt_crypto_entropy_init();
    unsigned char guarded[34];memset(guarded,0xa5,sizeof guarded);guarded[0]=0x42;guarded[33]=0x24;
    Script script={0,0,guarded+1};set(&script);assert(jet_rt_entropy_fixture_fill(guarded+1,7)==0);assert(script.calls==3);assert(memcmp(guarded+1,"\x01\x02\x03\x04\x05\x06\x07",7)==0);puts("suffix ok=0 calls=3 bytes=01020304050607");
    for(int mode=1;mode<=3;++mode){memset(guarded+1,0xa5,32);script=(Script){0,mode,guarded+1};set(&script);assert(jet_rt_entropy_fixture_fill(guarded+1,32)==1);zero(guarded+1,32);assert(guarded[0]==0x42&&guarded[33]==0x24);}puts("failure zero oversized ok=1 wipe=32");
    memset(guarded+1,0xa5,16);script=(Script){0,4,guarded+1};set(&script);assert(jet_rt_entropy_fixture_fill(guarded+1,16)==1);assert(script.calls==2);zero(guarded+1,16);puts("partial ok=1 wipe=16");
    memset(guarded+1,0xa5,16);script=(Script){0,5,guarded+1};set(&script);assert(jet_rt_entropy_fixture_fill(guarded+1,16)==1);zero(guarded+1,16);puts("unsupported ok=1 wipe=16");
    unsigned before=script.calls;assert(jet_rt_entropy_fixture_fill(NULL,0)==0);assert(jet_rt_entropy_fixture_fill(NULL,-1)==1);assert(jet_rt_entropy_fixture_fill(NULL,1048577)==1);assert(script.calls==before);
    // Child threads must not inherit the parent's deliberately failing hook.
    pthread_t threads[8];for(uintptr_t i=0;i<8;++i)assert(pthread_create(&threads[i],NULL,worker,(void *)i)==0);for(size_t i=0;i<8;++i)assert(pthread_join(threads[i],NULL)==0);
    for(size_t i=0;i<8;++i){unsigned aggregate=0;for(size_t k=0;k<64;++k)aggregate|=live[i][k];assert(aggregate);for(size_t j=0;j<i;++j)assert(memcmp(live[i],live[j],64)!=0);}
    jet_rt_entropy_fixture_clear();jet_rt_entropy_fixture_clear();assert(jet_rt_entropy_fixture_fill(guarded+1,32)==0);
    jet_rt_thread_detach();return 0;
}
