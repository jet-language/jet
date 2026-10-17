/* Real providers --wrap=jet_rt_alloc/jet_rt_free; no substitutes. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {uint64_t len,cap;H *data;} List;
typedef struct {Text *root;H rights;} Grant;
typedef struct {Text *path,*root;H access;} GrantedPath;
typedef struct {Text *label;List *extensions,*mime;} Filter;
typedef struct {H kind;Text *title;Grant grant;GrantedPath *initial;List *filters;H multiple;} Dialog;
_Static_assert(sizeof(Dialog)==56&&sizeof(GrantedPath)==24&&sizeof(Filter)==24,"UiABI dialog graph");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern H jet_rt_handle_clone_jet_ui_file_dialog_request(H);
extern void jet_rt_handle_drop_jet_ui_file_dialog_request(H);
extern bool jet_rt_handle_eq_jet_ui_file_dialog_request(H,H);
extern void jet_rt_handle_field_jet_ui_file_dialog_request_title(H,Text *);
extern H jet_rt_handle_field_jet_ui_file_dialog_request_grant(H),jet_rt_handle_field_jet_ui_file_dialog_request_allow_multiple(H);
extern H jet_rt_record_seq_list_rec_jet_ui_file_filter_new(H);
extern void jet_rt_record_seq_list_rec_jet_ui_file_filter_push(H,H);
static struct {H address,bytes,align;bool live;} ledger[4096];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking){assert(address&&bytes>0&&allocations<4096&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}
    return address;
}
void __wrap_jet_rt_free(H address,H bytes,H align) {
    if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}
    __real_jet_rt_free(address,bytes,align);
}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static List *texts(const char *s) {
    List *l=(List *)(uintptr_t)jet_rt_alloc(sizeof *l,8);l->len=3;l->cap=5;l->data=(H *)(uintptr_t)jet_rt_alloc((H)l->cap*8,8);l->data[0]=(H)(uintptr_t)text(s);l->data[1]=(H)(uintptr_t)text("");l->data[2]=(H)(uintptr_t)text("duplicate");return l;
}
static Filter *filter(size_t i){Filter *f=(Filter *)(uintptr_t)jet_rt_alloc(sizeof *f,8);f->label=text(i%2?"odd":"even");f->extensions=texts(".jet");f->mime=texts("text/jet");return f;}
static void different_text(Text *t,H a,H b){if(t->len){t->data[0]^=1;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(a,b));t->data[0]^=1;}else{++t->len;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(a,b));--t->len;}}
static void cases(H kind,unsigned initial,bool multiple,size_t n) {
    Dialog *a=(Dialog *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->kind=kind;a->title=text("dialog\xc3\xa9");a->grant.root=text("/grant");a->grant.rights=(H)UINT64_C(0xdeadbeef000000ff);a->initial=NULL;
    if(initial){a->initial=(GrantedPath *)(uintptr_t)jet_rt_alloc(sizeof *a->initial,8);a->initial->path=text(initial==1?"":"/grant/dir");a->initial->root=text("/grant");a->initial->access=kind;}
    a->filters=(List *)(uintptr_t)jet_rt_record_seq_list_rec_jet_ui_file_filter_new(0);for(size_t i=0;i<n;++i)jet_rt_record_seq_list_rec_jet_ui_file_filter_push((H)(uintptr_t)a->filters,(H)(uintptr_t)filter(i));a->multiple=(H)UINT64_C(0xdeadbeef00000000)|multiple;
    H ah=(H)(uintptr_t)a;Dialog *b=(Dialog *)(uintptr_t)jet_rt_handle_clone_jet_ui_file_dialog_request(ah);H bh=(H)(uintptr_t)b;
    assert(a!=b&&jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh)&&b->grant.rights==255&&b->multiple==multiple);assert(a->title!=b->title&&a->grant.root!=b->grant.root&&a->filters!=b->filters&&b->filters->len==n);
    size_t before=allocations;Text view={0};jet_rt_handle_field_jet_ui_file_dialog_request_title(ah,&view);assert(view.data==a->title->data&&view.len==a->title->len&&view.cap==a->title->cap&&allocations==before);assert(jet_rt_handle_field_jet_ui_file_dialog_request_grant(ah)==(H)(uintptr_t)&a->grant&&allocations==before);assert(jet_rt_handle_field_jet_ui_file_dialog_request_allow_multiple(ah)==(H)(uintptr_t)&a->multiple&&allocations==before);
    b->kind^=1;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->kind^=1;different_text(b->title,ah,bh);different_text(b->grant.root,ah,bh);b->grant.rights^=1;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->grant.rights^=1;b->multiple^=1;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->multiple^=1;
    if(initial){assert(a->initial!=b->initial&&a->initial->path!=b->initial->path&&a->initial->root!=b->initial->root);GrantedPath *saved=b->initial;b->initial=NULL;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->initial=saved;different_text(b->initial->path,ah,bh);different_text(b->initial->root,ah,bh);b->initial->access^=1;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->initial->access^=1;}
    for(size_t i=0;i<n;++i){Filter *x=(Filter *)(uintptr_t)a->filters->data[i],*y=(Filter *)(uintptr_t)b->filters->data[i];assert(x!=y&&x->label!=y->label&&x->extensions!=y->extensions&&x->mime!=y->mime);different_text(y->label,ah,bh);List *lists[]={y->extensions,y->mime};for(size_t j=0;j<2;++j){for(size_t k=0;k<3;++k)different_text((Text *)(uintptr_t)lists[j]->data[k],ah,bh);--lists[j]->len;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));++lists[j]->len;}}
    if(n){--b->filters->len;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));++b->filters->len;}
    if(n>1){H t=b->filters->data[0];b->filters->data[0]=b->filters->data[1];b->filters->data[1]=t;assert(!jet_rt_handle_eq_jet_ui_file_dialog_request(ah,bh));b->filters->data[1]=b->filters->data[0];b->filters->data[0]=t;}
    jet_rt_handle_drop_jet_ui_file_dialog_request(ah);assert(jet_rt_handle_eq_jet_ui_file_dialog_request(bh,bh));jet_rt_handle_drop_jet_ui_file_dialog_request(bh);balanced();
}
int main(void){jet_rt_alloc_init();tracking=true;for(H kind=0;kind<2;++kind)for(unsigned initial=0;initial<3;++initial)for(unsigned multiple=0;multiple<2;++multiple){cases(kind,initial,multiple,0);cases(kind,initial,multiple,1);cases(kind,initial,multiple,11);}jet_rt_handle_drop_jet_ui_file_dialog_request(0);balanced();puts("UI FileDialog: every field, 36 option/list combinations, borrowed views, transitive clones and exact-once frees ok");return 0;}
