// Real Jet object client: all Unicode 17.0.0 NormalizationTest rows, both NFC
// routes, idempotence, ASCII ownership and hostile descending CCC runs.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
extern void jet_rt_alloc_init(void);
extern Text *jet_rt_string_new(unsigned char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_text_nfc(Text *);
extern Text *jet_text_normalize_nfc(Text *);
static FILE *seed;
static int equal(const Text *a,const Text *b) {
    return a->length==b->length && (!a->length || memcmp(a->data,b->data,(size_t)a->length)==0);
}
static Text *scalars(const char *s) {
    size_t size=strlen(s)*4+1, used=0;
    unsigned char *bytes=malloc(size); assert(bytes);
    while(*s) {
        while(*s==' ' || *s=='\t' || *s=='\r' || *s=='\n') ++s;
        if(!*s) break;
        char *end; unsigned long cp=strtoul(s,&end,16);
        assert(end!=s && cp<=0x10ffff && !(cp>=0xd800 && cp<=0xdfff)); s=end;
        if(cp<0x80) bytes[used++]=(unsigned char)cp;
        else if(cp<0x800) { bytes[used++]=(unsigned char)(0xc0|(cp>>6)); bytes[used++]=(unsigned char)(0x80|(cp&63)); }
        else if(cp<0x10000) { bytes[used++]=(unsigned char)(0xe0|(cp>>12)); bytes[used++]=(unsigned char)(0x80|((cp>>6)&63)); bytes[used++]=(unsigned char)(0x80|(cp&63)); }
        else { bytes[used++]=(unsigned char)(0xf0|(cp>>18)); bytes[used++]=(unsigned char)(0x80|((cp>>12)&63)); bytes[used++]=(unsigned char)(0x80|((cp>>6)&63)); bytes[used++]=(unsigned char)(0x80|(cp&63)); }
    }
    Text *result=jet_rt_string_new(bytes,(int64_t)used); free(bytes); return result;
}
static void check(Text *input,Text *expected,size_t row,unsigned column) {
    Text *a=jet_text_nfc(input),*b=jet_text_normalize_nfc(input);
    if(!equal(a,expected) || !equal(b,expected)) {
        fprintf(stderr,"NFC mismatch row %zu column %u\n",row,column); abort();
    }
    if(seed && row) {
        char line[16384]; assert(fgets(line,sizeof(line),seed));
        Text *oracle=scalars(line); assert(equal(a,oracle) && equal(b,oracle));
        jet_rt_string_drop(oracle);
    }
    assert(a!=input && b!=input && a!=b);
    if(input->length) assert(a->data!=input->data && b->data!=input->data);
    Text *again=jet_text_nfc(a); assert(equal(again,a));
    jet_rt_string_drop(again); jet_rt_string_drop(a); jet_rt_string_drop(b);
}
static void focused(const char *input,const char *expected) {
    Text *a=scalars(input),*b=scalars(expected); check(a,b,0,0);
    jet_rt_string_drop(a); jet_rt_string_drop(b);
}
int main(int argc,char **argv) {
    assert(argc==2 || argc==3); FILE *file=fopen(argv[1],"rb"); assert(file);
    if(argc==3) { seed=fopen(argv[2],"rb"); assert(seed); }
    jet_rt_alloc_init();
    char line[16384]; size_t rows=0; unsigned parts=0;
    while(fgets(line,sizeof(line),file)) {
        assert(strchr(line,'\n') || feof(file));
        if(line[0]=='@') { unsigned part; assert(sscanf(line,"@Part%u",&part)==1 && part<=5); parts|=1u<<part; continue; }
        char *comment=strchr(line,'#'); if(comment) *comment=0;
        char *p=line; while(*p==' ' || *p=='\t') ++p;
        if(!*p || *p=='\r' || *p=='\n') continue;
        Text *columns[5];
        for(unsigned i=0;i<5;++i) { char *end=strchr(p,';'); assert(end); *end=0; columns[i]=scalars(p); p=end+1; }
        ++rows;
        for(unsigned i=0;i<5;++i) check(columns[i],columns[i<3?1:3],rows,i+1);
        for(unsigned i=0;i<5;++i) jet_rt_string_drop(columns[i]);
    }
    if(seed) { assert(fgetc(seed)==EOF && !ferror(seed)); assert(fclose(seed)==0); }
    assert(!ferror(file) && fclose(file)==0 && rows==20034 && parts==63);
    focused("",""); focused("0041 0000 0042","0041 0000 0042");
    focused("1100 1161 11A8","AC01"); focused("AC01","AC01");
    focused("0301 0300 0041 030A","0301 0300 00C5");
    focused("0958","0915 093C"); focused("0344","0308 0301");
    focused("09C7 0300 09BE","09C7 0300 09BE"); // class-0 composition blocked by intervening mark
    focused("0041 0305 030A","0041 0305 030A"); // equal-class blocker
    // Long descending runs, equal-class stability, and scratch reuse across starters.
    const size_t n=8192;
    unsigned char *descending=malloc(n*4+3),*ordered=malloc(n*4+3); assert(descending && ordered);
    size_t offset=0;
    for(unsigned run=0;run<2;++run) {
        descending[offset]=ordered[offset]='B'; ++offset;
        for(size_t i=0;i<n;++i) { descending[offset+2*i]=0xcc; descending[offset+2*i+1]=(unsigned char)(i%2?0x96:0x95); }
        for(size_t i=0;i<n;++i) { ordered[offset+2*i]=0xcc; ordered[offset+2*i+1]=(unsigned char)(i<n/2?0x96:0x95); }
        offset+=n*2;
    }
    Text *a=jet_rt_string_new(descending,(int64_t)offset),*b=jet_rt_string_new(ordered,(int64_t)offset);
    free(descending); free(ordered); check(a,b,0,0); jet_rt_string_drop(a); jet_rt_string_drop(b);
    puts("NFC2 Unicode17 20034 rows / both routes / ownership OK"); return 0;
}
