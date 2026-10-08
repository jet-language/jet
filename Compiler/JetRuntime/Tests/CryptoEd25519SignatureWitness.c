// All pure RFC8032 KATs and ONCE dalek2.2.0 ordinary/strict edge matrix.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "CryptoEd25519SignatureGoldens.h"
#include "Ed25519OfficialGoldens.h"
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_ed25519_signature_fixture(int64_t,const void *,const void *,int64_t,void *,int64_t);
static unsigned nibble(char c){assert((c>='0'&&c<='9')||(c>='a'&&c<='f'));return (unsigned)(c<='9'?c-'0':c-'a'+10);}
static size_t decode(const char *hex,unsigned char *out,size_t capacity){size_t bytes=strlen(hex)/2;assert(strlen(hex)==bytes*2&&bytes<=capacity);for(size_t i=0;i<bytes;++i)out[i]=(unsigned char)((nibble(hex[2*i])<<4)|nibble(hex[2*i+1]));return bytes;}
static void exercise(const struct Ed25519Golden *g,int printing){
    unsigned char key[32],seed[32],message[4096],signature[64];assert(decode(g->public_key,key,sizeof key)==32);assert(decode(g->signature,signature,sizeof signature)==64);size_t n=decode(g->message,message,sizeof message);
    if(g->pattern_length>=0){assert((uint64_t)g->pattern_length<=sizeof message);n=(size_t)g->pattern_length;for(size_t i=0;i<n;++i)message[i]=(unsigned char)((i*197+23)%256);}
    const int decoded=(int)jet_rt_ed25519_signature_fixture(1,key,NULL,0,NULL,0);assert(decoded==g->decoded);
    if(g->seed[0]){assert(decode(g->seed,seed,sizeof seed)==32);unsigned char public_out[34];memset(public_out,0xa5,sizeof public_out);public_out[0]=0x42;public_out[33]=0x24;assert(jet_rt_ed25519_signature_fixture(3,seed,NULL,0,public_out+1,0)==1);assert(public_out[0]==0x42&&public_out[33]==0x24);assert(memcmp(public_out+1,key,32)==0);
        unsigned char signed_out[66];memset(signed_out,0xa5,sizeof signed_out);signed_out[0]=0x42;signed_out[65]=0x24;assert(jet_rt_ed25519_signature_fixture(0,seed,message,(int64_t)n,signed_out+1,0)==1);assert(signed_out[0]==0x42&&signed_out[65]==0x24);assert(memcmp(signed_out+1,signature,64)==0);}
    const int ordinary=(int)jet_rt_ed25519_signature_fixture(2,key,message,(int64_t)n,signature,0),strict=(int)jet_rt_ed25519_signature_fixture(2,key,message,(int64_t)n,signature,1);assert(ordinary==g->ordinary&&strict==g->strict);if(printing)printf("%s %d %d %d\n",g->label,decoded,ordinary,strict);
}
int main(void){jet_rt_alloc_init();for(size_t i=0;i<sizeof ed25519_official/sizeof ed25519_official[0];++i)exercise(ed25519_official+i,0);for(size_t i=0;i<sizeof ed25519_goldens/sizeof ed25519_goldens[0];++i)exercise(ed25519_goldens+i,1);return 0;}
