// Official BLAKE3-team vectors, default32byte output, input i%251.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "CryptoBlake3Goldens.h"
extern void jet_rt_alloc_init(void);
extern void jet_rt_blake3_fixture(const void *,int64_t,void *);
static unsigned nibble(char c){assert((c>='0'&&c<='9')||(c>='a'&&c<='f'));return (unsigned)(c<='9'?c-'0':c-'a'+10);}
int main(void){
    jet_rt_alloc_init();unsigned char input[102400];for(size_t i=0;i<sizeof input;++i)input[i]=(unsigned char)(i%251);
    for(size_t i=0;i<sizeof blake3_goldens/sizeof blake3_goldens[0];++i){const struct Blake3Golden *g=blake3_goldens+i;assert(g->length>=0&&(size_t)g->length<=sizeof input);unsigned char out[34];memset(out,0xa5,sizeof out);out[0]=0x42;out[33]=0x24;jet_rt_blake3_fixture(input,g->length,out+1);assert(out[0]==0x42&&out[33]==0x24);assert(strlen(g->hex)==64);const char *alphabet="0123456789abcdef";char hex[65];for(size_t j=0;j<32;++j){assert(out[j+1]==(unsigned char)((nibble(g->hex[2*j])<<4)|nibble(g->hex[2*j+1])));hex[2*j]=alphabet[out[j+1]>>4];hex[2*j+1]=alphabet[out[j+1]&15];}hex[64]=0;printf("%lld %s\n",(long long)g->length,hex);}
    return 0;
}
