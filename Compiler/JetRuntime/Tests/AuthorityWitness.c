/* Real composed Jet native exports only. Link --wrap=jet_rt_free.
 * denied mode must take the canonical Jet panic exit70 with the exact E0712
 * message (seed exit101 is a registered seed defect, not a parity claim).
 * Workspace is called directly here; MIR reachability is separately regen-owed. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; Text **data; } Authority;
_Static_assert(sizeof(Authority)==24 && offsetof(Authority,data)==16,"Authority24");
extern void jet_rt_alloc_init(void);
extern Authority *jet_authority_from_rights(Text *const *,int64_t);
extern Authority *jet_authority_workspace(void);
extern Authority *jet_authority_with(Authority *,Text *);
extern Authority *jet_authority_without(Authority *,Text *);
extern Authority *jet_rt_handle_clone_jet_authority(Authority *);
extern void jet_rt_handle_drop_jet_authority(Authority *);
extern bool jet_rt_handle_eq_jet_authority(Authority *,Authority *);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[8192];
static size_t count;
static unsigned interceptions;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    ++interceptions;
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==watched[i-1].align);
        watched[i-1].freed=1;
        break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static Text borrow(const char *s) { return (Text){(unsigned char *)s,(int64_t)strlen(s),0}; }
static bool same_text(Text *a,Text *b) {
    return a->len==b->len && (a->len==0 || memcmp(a->data,b->data,(size_t)a->len)==0);
}
static int compare(Text *a,Text *b) {
    int64_t n=a->len<b->len?a->len:b->len;
    int c=n?memcmp(a->data,b->data,(size_t)n):0;
    return c?c:(a->len>b->len)-(a->len<b->len);
}
static void authority_watch(Authority *value) {
    assert(value && value->len>=0 && value->cap>=value->len);
    watch(value,24,8);
    if(value->cap) watch(value->data,value->cap*8,8);
    for(int64_t i=0;i<value->len;++i) {
        Text *text=value->data[i];
        watch(text,24,8);
        if(text->cap) watch(text->data,text->cap,1);
        if(i) assert(compare(value->data[i-1],text)<0);
    }
}
static Authority *from(Text *const *slots,int64_t length) {
    Authority *value=jet_authority_from_rights(slots,length);
    authority_watch(value);
    return value;
}
static void independent(Authority *a,Authority *b) {
    assert(a!=b && a->len==b->len && jet_rt_handle_eq_jet_authority(a,b));
    if(a->len) assert(a->data!=b->data);
    for(int64_t i=0;i<a->len;++i) {
        assert(a->data[i]!=b->data[i] && same_text(a->data[i],b->data[i]));
        if(a->data[i]->len) assert(a->data[i]->data!=b->data[i]->data);
    }
}
static void narrow(Authority *held,const char *requested) {
    Text text=borrow(requested);
    Text *slots[]={&text};
    Authority *value=jet_authority_with(held,&text);
    authority_watch(value);
    Authority *expected=from(slots,1);
    independent(value,expected);
    jet_rt_handle_drop_jet_authority(value);
    jet_rt_handle_drop_jet_authority(expected);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    Text fs=borrow("FS"),net=borrow("Net.Read"),fsread=borrow("FS.Read:repo"),near=borrow("FSX"),empty=borrow("");
    unsigned char nul_bytes[]={'N',0,'x'};
    Text nul={nul_bytes,3,0},unicode=borrow("\xe2\x98\x83");
    Text *slots[]={&net,&fs,&fsread,&fs,&unicode,&nul,&empty,&net};
    Authority *held=from(slots,8);
    assert(held->len==6 && slots[0]==&net && slots[3]==&fs && nul_bytes[1]==0);
    if(argc>1) {
        assert(argc==2 && strcmp(argv[1],"denied")==0);
        (void)jet_authority_with(held,&near);
        assert(!"denied authority returned instead of stopping");
    }
    Text *reverse[]={&net,&empty,&nul,&unicode,&fs,&fsread};
    Authority *permuted=from(reverse,6);
    independent(held,permuted);
    Authority *clone=jet_rt_handle_clone_jet_authority(held);
    authority_watch(clone);
    independent(held,clone);
    nul_bytes[2]='y';
    assert(clone->data[3]->len==3 && clone->data[3]->data[2]=='x');
    nul_bytes[2]='x';
    jet_rt_handle_drop_jet_authority(held);
    assert(jet_rt_handle_eq_jet_authority(clone,permuted));
    Text other_net=borrow("Net.Write");
    Text *changed_slots[]={&other_net,&empty,&nul,&unicode,&fs,&fsread};
    Authority *changed=from(changed_slots,6);
    assert(changed->len==clone->len && !jet_rt_handle_eq_jet_authority(changed,clone));
    narrow(clone,"FS");
    narrow(clone,"FS.Read");
    narrow(clone,"FS:repo");
    narrow(clone,"FS/repo");
    narrow(clone,"Net.Read");
    narrow(clone,"Net.Read:api");
    narrow(clone,"");
    Authority *without=jet_authority_without(clone,&fs);
    authority_watch(without);
    Text *remaining[]={&empty,&nul,&net,&unicode};
    Authority *expected=from(remaining,4);
    independent(without,expected);
    assert(!jet_rt_handle_eq_jet_authority(clone,without));
    Authority *unchanged=jet_authority_without(clone,&near);
    authority_watch(unchanged);
    independent(unchanged,clone);
    Text prefix=borrow("FS.Read:repo");
    Authority *exact=jet_authority_without(clone,&prefix);
    authority_watch(exact);
    assert(exact->len==5);
    Text dot=borrow(".");
    Authority *dot_rights=jet_authority_with(clone,&dot);
    authority_watch(dot_rights);
    assert(dot_rights->len==1 && same_text(dot_rights->data[0],&dot));
    Authority *none=from(NULL,99),*negative=from(slots,-1),*zero=from(slots,0);
    assert(none->len==0 && negative->len==0 && zero->len==0);
    assert(jet_rt_handle_eq_jet_authority(none,negative) && jet_rt_handle_eq_jet_authority(none,zero));
    Authority *empty_clone=jet_rt_handle_clone_jet_authority(none);
    authority_watch(empty_clone);
    independent(empty_clone,none);
    Authority *workspace=jet_authority_workspace();
    authority_watch(workspace);
    Text workspace_read=borrow("FS.Read:repo"),workspace_write=borrow("FS.Write:.jet/build");
    Text *workspace_slots[]={&workspace_write,&workspace_read};
    Authority *workspace_expected=from(workspace_slots,2);
    independent(workspace,workspace_expected);
    jet_rt_handle_drop_jet_authority(NULL);
    Authority *owners[]={clone,permuted,changed,without,expected,unchanged,exact,dot_rights,none,negative,zero,empty_clone,workspace,workspace_expected};
    for(size_t i=0;i<sizeof(owners)/sizeof(owners[0]);++i) jet_rt_handle_drop_jet_authority(owners[i]);
    assert(interceptions>0);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("Authority ownership/set/attenuation/workspace OK");
    return 0;
}
