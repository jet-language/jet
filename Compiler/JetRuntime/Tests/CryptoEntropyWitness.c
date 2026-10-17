// Native callback transport for the exact seed provider contract. NO crypto
// implementation: scripted steps enter the same private Rand adapter as live.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <pthread.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t tag; Text *operation,*parameter,*expected; int64_t actual; Text *detail; int64_t minimum,maximum; } Error;
extern void jet_rt_alloc_init(void);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_crypto_entropy_init(void);
extern void jet_rt_entropy_fixture_set(int64_t,int64_t);
extern void jet_rt_entropy_fixture_clear(void);
extern int64_t jet_rt_entropy_fixture_fill(void *,int64_t);
extern Text *jet_rt_entropy_fixture_error(const Error *);
extern void jet_rt_string_drop(Text *);
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
static Text text(const char *s){Text t={(unsigned char *)(uintptr_t)s,(int64_t)strlen(s),(int64_t)strlen(s)};return t;}
static void errors(void){
    const char *expected[]={"seal: key must be exactly 32; got 31","PasswordHash.parse: PHC string is not canonical","PasswordHash.parse: version 16 is not supported","PasswordHash.parse: algorithm argon2i is not supported","encrypted data could not be opened","X25519 peer key does not contribute to a shared secret","hkdf_sha256: output length must be 0..8160; got 8161","password hash is outside Jet's accepted policy","the operating system could not provide cryptographic randomness","Argon2 worker pool is unavailable for this cryptographic operation","Jet could not preserve a cryptographic invariant; incident crypto-17"};
    Text seal=text("seal"),key=text("key"),exact=text("exactly 32"),phc=text("PasswordHash.parse"),kind=text("PHC string"),algorithm=text("argon2i"),hkdf=text("hkdf_sha256"),resource=text("Argon2 worker pool"),incident=text("crypto-17"),poison=text("hunter2 plaintext sentinel ciphertext sentinel /home/nate");
    for(int64_t tag=0;tag<11;++tag){
        Error e={tag,&poison,&poison,&poison,0,&poison,0,0};
        if(tag==0){e.operation=&seal;e.parameter=&key;e.expected=&exact;e.actual=31;}
        if(tag==1){e.operation=&phc;e.parameter=&kind;}
        if(tag==2){e.operation=&phc;e.actual=16;}
        if(tag==3){e.operation=&phc;e.detail=&algorithm;}
        if(tag==6){e.operation=&hkdf;e.minimum=0;e.maximum=8160;e.actual=8161;}
        if(tag==9)e.detail=&resource;
        if(tag==10)e.detail=&incident;
        Text *s=jet_rt_entropy_fixture_error(&e);assert(s->length==(int64_t)strlen(expected[tag]));assert(memcmp(s->data,expected[tag],(size_t)s->length)==0);
        // A bounded copy checks redaction; no returned text need have a NUL.
        char rendered[160];assert(s->length<(int64_t)sizeof rendered);memcpy(rendered,s->data,(size_t)s->length);rendered[s->length]=0;
        const char *forbidden[]={"hunter2","plaintext sentinel","ciphertext sentinel","/home/nate"};for(size_t i=0;i<4;++i)assert(strstr(rendered,forbidden[i])==NULL);
        puts(rendered);jet_rt_string_drop(s);
    }
    // UTF-8 metadata passes through byte-identically, not byte-as-codepoint.
    Text unicode=text("alg\xc3\xa9"),op=text("verify");Error e={3,&op,NULL,NULL,0,&unicode,0,0};Text *s=jet_rt_entropy_fixture_error(&e);
    const char *expect="verify: algorithm alg\xc3\xa9 is not supported";assert(s->length==(int64_t)strlen(expect));assert(memcmp(s->data,expect,(size_t)s->length)==0);jet_rt_string_drop(s);
}
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
    errors();jet_rt_thread_detach();return 0;
}
