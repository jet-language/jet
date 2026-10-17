// NIST CAVP SHA3 byte-oriented ShortMsg KATs + once-frozen Rust parity.
// Known rate-1 seed-padding errors have independently canonical expectations.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "CryptoSha3Goldens.h"
extern void jet_rt_alloc_init(void);
extern void jet_rt_sha3_fixture(int64_t,const void *,int64_t,void *);
static unsigned nibble(char c){assert((c>='0'&&c<='9')||(c>='a'&&c<='f'));return (unsigned)(c<='9'?c-'0':c-'a'+10);}
static void digest(int64_t bits,const unsigned char *input,int64_t length,const char *expected,char *hex){
    unsigned char out[66];const int64_t n=bits/8;assert(n>=28&&n<=64);memset(out,0xa5,sizeof out);out[0]=0x42;out[n+1]=0x24;jet_rt_sha3_fixture(bits,input,length,out+1);assert(out[0]==0x42&&out[n+1]==0x24);assert(strlen(expected)==(size_t)n*2);const char *alphabet="0123456789abcdef";
    for(int64_t i=0;i<n;++i){assert(out[i+1]==(unsigned char)((nibble(expected[2*i])<<4)|nibble(expected[2*i+1])));hex[2*i]=alphabet[out[i+1]>>4];hex[2*i+1]=alphabet[out[i+1]&15];}hex[2*n]=0;
}
int main(void){
    jet_rt_alloc_init();unsigned char input[4096];for(size_t i=0;i<sizeof input;++i)input[i]=(unsigned char)((i*197+23)%256);
    for(size_t i=0;i<sizeof sha3_goldens/sizeof sha3_goldens[0];++i){const struct Sha3Golden *g=sha3_goldens+i;char hex[129];digest(g->bits,input,g->length,g->hex,hex);if(!g->divergence)printf("%lld %lld %s\n",(long long)g->bits,(long long)g->length,hex);}
    for(size_t i=0;i<sizeof sha3_official/sizeof sha3_official[0];++i){const struct Sha3Official *g=sha3_official+i;assert((size_t)g->length<=sizeof input);assert(strlen(g->message)==(size_t)g->length*2);for(int64_t j=0;j<g->length;++j)input[j]=(unsigned char)((nibble(g->message[2*j])<<4)|nibble(g->message[2*j+1]));char hex[129];digest(g->bits,input,g->length,g->hex,hex);}
    return 0;
}
