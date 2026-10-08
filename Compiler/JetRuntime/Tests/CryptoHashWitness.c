// Official RFC6234 section8.5 KATs and frozen existing Rust boundary outputs.
// C transports bytes/assertions only. No C/Rust hash engine can satisfy exports.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "CryptoHashGoldens.h"
extern void jet_rt_alloc_init(void);
extern void jet_rt_hash_fixture(int64_t,const void *,int64_t,void *);
extern int64_t jet_rt_equal_fixture(const void *,int64_t,const void *,int64_t);
static unsigned nibble(char c){assert((c>='0'&&c<='9')||(c>='a'&&c<='f'));return (unsigned)(c<='9'?c-'0':c-'a'+10);}
static void hash(int64_t kind,const void *input,size_t length,const char *expected,char *rendered){
    size_t size=kind==1?20:(kind==224?28:48);unsigned char out[66];memset(out,0xa5,sizeof out);out[0]=0x42;out[size+1]=0x24;
    jet_rt_hash_fixture(kind,input,(int64_t)length,out+1);assert(out[0]==0x42&&out[size+1]==0x24);
    assert(strlen(expected)==size*2);const char *hex="0123456789abcdef";
    for(size_t i=0;i<size;++i){assert(out[i+1]==(unsigned char)((nibble(expected[2*i])<<4)|nibble(expected[2*i+1])));if(rendered){rendered[2*i]=hex[out[i+1]>>4];rendered[2*i+1]=hex[out[i+1]&15];}}
    if(rendered)rendered[2*size]=0;
}
static void official(void){
    const char *abc="abc",*long32="abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",*long64="abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
    hash(1,abc,3,"a9993e364706816aba3e25717850c26c9cd0d89d",NULL);
    hash(1,long32,strlen(long32),"84983e441c3bd26ebaae4aa1f95129e5e54670f1",NULL);
    hash(224,abc,3,"23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",NULL);
    hash(224,long32,strlen(long32),"75388b16512776cc5dba5da1fd890150b0c6455cb4f58b1952522525",NULL);
    hash(384,abc,3,"cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",NULL);
    hash(384,long64,strlen(long64),"09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039",NULL);
}
int main(void){
    jet_rt_alloc_init();official();unsigned char input[4097];for(size_t i=0;i<sizeof input;++i)input[i]=(unsigned char)((i*197+23)%256);
    for(size_t i=0;i<sizeof hash_goldens/sizeof hash_goldens[0];++i){const struct HashGolden *g=hash_goldens+i;char a[41],b[57],c[97];
        hash(1,input,(size_t)g->length,g->sha1,a);hash(224,input,(size_t)g->length,g->sha224,b);hash(384,input,(size_t)g->length,g->sha384,c);printf("%lld %s %s %s\n",(long long)g->length,a,b,c);
    }
    unsigned char a[32],b[33];for(unsigned i=0;i<32;++i)a[i]=(unsigned char)i;
    for(unsigned mode=0;mode<8;++mode){memcpy(b,a,32);int64_t left=32,right=32;
        if(mode==1)b[0]^=1;
        if(mode==2)b[16]^=1;
        if(mode==3)b[31]^=1;
        if(mode==4)right=31;
        if(mode==5){right=33;b[32]=0;}
        if(mode==6){left=0;right=0;}
        if(mode==7){left=0;right=1;b[0]=0;}
        int64_t equal=jet_rt_equal_fixture(a,left,b,right);assert(equal==equal_goldens[mode]);printf("eq %u %lld\n",mode,(long long)equal);
    }
    // Misalignment/embedded zero are valid borrowed bytes, not machine words.
    assert(jet_rt_equal_fixture(a+1,31,a+1,31)==1);return 0;
}
