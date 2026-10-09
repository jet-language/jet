// RFC7914 section11 official KATs + once-frozen existing Rust boundary corpus.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "CryptoPbkdf2Goldens.h"
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_pbkdf2_fixture(const void *,int64_t,const void *,int64_t,int64_t,void *,int64_t);
static unsigned nibble(char c){assert((c>='0'&&c<='9')||(c>='a'&&c<='f'));return (unsigned)(c<='9'?c-'0':c-'a'+10);}
static void derive(const void *p,int64_t pn,const void *s,int64_t sn,int64_t c,int64_t n,const char *expected,char *hex){
    unsigned char out[99];assert(n>=0&&n<=97);memset(out,0xa5,sizeof out);out[0]=0x42;out[n+1]=0x24;
    assert(jet_rt_pbkdf2_fixture(p,pn,s,sn,c,out+1,n)==1);assert(out[0]==0x42&&out[n+1]==0x24);
    if(n==0){assert(strcmp(expected,"-")==0);strcpy(hex,"-");return;}
    assert(strlen(expected)==2*(size_t)n);const char *alphabet="0123456789abcdef";
    for(int64_t i=0;i<n;++i){assert(out[i+1]==(unsigned char)((nibble(expected[2*i])<<4)|nibble(expected[2*i+1])));hex[2*i]=alphabet[out[i+1]>>4];hex[2*i+1]=alphabet[out[i+1]&15];}hex[2*n]=0;
}
int main(void){
    jet_rt_alloc_init();unsigned char p[257],s[129];for(size_t i=0;i<sizeof p;++i)p[i]=(unsigned char)((i*197+23)%256);for(size_t i=0;i<sizeof s;++i)s[i]=(unsigned char)((i*131+7)%256);
    for(size_t i=0;i<sizeof pbkdf_goldens/sizeof pbkdf_goldens[0];++i){const struct PbkdfGolden *g=pbkdf_goldens+i;char hex[195];derive(p,g->password,s,g->salt,g->iterations,g->length,g->hex,hex);printf("%lld %lld %lld %lld %s\n",(long long)g->password,(long long)g->salt,(long long)g->iterations,(long long)g->length,hex);}
    char hex[195];
    derive("passwd",6,"salt",4,1,64,"55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783",hex);printf("official 1 %s\n",hex);
    derive("Password",8,"NaCl",4,80000,64,"4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d",hex);printf("official 80000 %s\n",hex);
    unsigned char unchanged[32];memset(unchanged,0xa5,sizeof unchanged);
    assert(jet_rt_pbkdf2_fixture(p,32,s,16,0,unchanged,32)==0);
    assert(jet_rt_pbkdf2_fixture(p,32,s,16,-1,unchanged,32)==0);
    assert(jet_rt_pbkdf2_fixture(p,32,s,16,1,NULL,-1)==0);
    assert(jet_rt_pbkdf2_fixture(p,32,s,16,1,NULL,67108865)==0);
    for(size_t i=0;i<sizeof unchanged;++i)assert(unchanged[i]==0xa5);
    return 0;
}
