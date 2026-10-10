/* Real action-error providers; allocation ledger detects transitive leaks. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len,cap; } Text;
typedef struct { uint64_t len,cap; void *data; } List;
typedef struct { Text *key; List messages; } Entry;
typedef struct { List fields,form; } Error;
_Static_assert(sizeof(Text)==24&&sizeof(List)==24&&sizeof(Entry)==32&&sizeof(Error)==48,"shared native layout inventory");
extern void jet_rt_alloc_init(void);
extern H __real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern H jet_web_forms_action_error(void),jet_web_forms_action_field_error(H,H,H),jet_web_forms_action_form_error(H,H);
extern H jet_rt_handle_clone_jet_web_form_action_error(H);
extern void jet_rt_handle_drop_jet_web_form_action_error(H);
extern bool jet_rt_handle_eq_jet_web_form_action_error(H,H);
static struct { H address,bytes,align; bool live; } ledger[8192];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align){H a=__real_jet_rt_alloc(bytes,align);if(tracking){assert(a&&bytes>0&&allocations<8192&&(uint64_t)a%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=a);ledger[allocations].address=a;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}return a;}
void __wrap_jet_rt_free(H a,H bytes,H align){if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==a&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(a,bytes,align);return;}assert(!"untracked/static/double free");}__real_jet_rt_free(a,bytes,align);}
static H h(const void *p){return (H)(uintptr_t)p;}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static H owned(const void *bytes,size_t n){Text *t=(Text *)(uintptr_t)__wrap_jet_rt_alloc(24,8);t->len=t->cap=n;t->data=n?(unsigned char *)(uintptr_t)__wrap_jet_rt_alloc((H)n,1):(unsigned char *)(uintptr_t)1;if(n)memcpy(t->data,bytes,n);return h(t);}
static H string(const char *s){return owned(s,strlen(s));}
static Error *fresh(void){Error *e=(Error *)(uintptr_t)jet_web_forms_action_error();assert(e->fields.len==0&&e->fields.cap==0&&e->form.len==0&&e->form.cap==0);return e;}
static Text *message(List *list,size_t i){assert(i<list->len);return ((Text **)list->data)[i];}
static void text_eq(Text *t,const void *bytes,size_t n){assert(t->len==n&&(!n||memcmp(t->data,bytes,n)==0));}
static void field(Error *e,const char *name,const char *msg){assert(jet_web_forms_action_field_error(h(e),string(name),string(msg))==h(e));}
static void form(Error *e,const char *msg){assert(jet_web_forms_action_form_error(h(e),string(msg))==h(e));}
static void clone_and_drop(Error *a){
    Error *b=(Error *)(uintptr_t)jet_rt_handle_clone_jet_web_form_action_error(h(a));
    assert(a!=b&&jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));
    assert(b->fields.cap==b->fields.len&&b->form.cap==b->form.len);
    if(a->fields.len){assert(a->fields.data!=b->fields.data);Entry *x=a->fields.data,*y=b->fields.data;for(size_t i=0;i<a->fields.len;++i){assert(x[i].key!=y[i].key&&x[i].key->data!=y[i].key->data);assert(y[i].messages.cap==y[i].messages.len);if(x[i].messages.len)assert(x[i].messages.data!=y[i].messages.data);for(size_t j=0;j<x[i].messages.len;++j){Text *u=message(&x[i].messages,j),*v=message(&y[i].messages,j);assert(u!=v);if(u->len)assert(u->data!=v->data);}}}
    if(a->form.len)assert(a->form.data!=b->form.data);
    jet_rt_handle_drop_jet_web_form_action_error(h(a));assert(jet_rt_handle_eq_jet_web_form_action_error(h(b),h(b)));jet_rt_handle_drop_jet_web_form_action_error(h(b));balanced();
}
static void ordered_case(void){
    Error *a=fresh();field(a,"z","z0");field(a,"a","a0");field(a,"m","m0");field(a,"a","a1");field(a,"_","u0");field(a,"b","b0");field(a,"aa","aa0");form(a,"");form(a,"form1");
    const char *keys[]={"_","a","aa","b","m","z"};Entry *entries=a->fields.data;assert(a->fields.len==6&&a->fields.cap>6);
    for(size_t i=0;i<6;++i){text_eq(entries[i].key,keys[i],strlen(keys[i]));}assert(entries[1].messages.len==2);text_eq(message(&entries[1].messages,0),"a0",2);text_eq(message(&entries[1].messages,1),"a1",2);
    Error *b=(Error *)(uintptr_t)jet_rt_handle_clone_jet_web_form_action_error(h(a));assert(jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));
    Entry *peers=b->fields.data;peers[1].key->data[0]^=1;assert(!jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));peers[1].key->data[0]^=1;
    message(&peers[1].messages,1)->data[0]^=1;assert(!jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));message(&peers[1].messages,1)->data[0]^=1;
    message(&b->form,1)->data[0]^=1;assert(!jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));message(&b->form,1)->data[0]^=1;
    Text *saved=message(&peers[1].messages,0);((Text **)peers[1].messages.data)[0]=message(&peers[1].messages,1);((Text **)peers[1].messages.data)[1]=saved;assert(!jet_rt_handle_eq_jet_web_form_action_error(h(a),h(b)));((Text **)peers[1].messages.data)[1]=((Text **)peers[1].messages.data)[0];((Text **)peers[1].messages.data)[0]=saved;
    jet_rt_handle_drop_jet_web_form_action_error(h(b));clone_and_drop(a);
}
static bool letter(unsigned byte){return (byte>='a'&&byte<='z')||(byte>='A'&&byte<='Z')||byte=='_';}
static void name_case(const unsigned char *name,size_t n,bool valid){Error *e=fresh();assert(jet_web_forms_action_field_error(h(e),owned(name,n),string("input"))==h(e));assert(e->fields.len==(valid?1u:0u)&&e->form.len==(valid?0u:1u));if(valid){Entry *entry=e->fields.data;text_eq(entry->key,name,n);text_eq(message(&entry->messages,0),"input",5);}else{text_eq(message(&e->form,0),"action returned an invalid field error",37);}clone_and_drop(e);}
static void names(void){for(unsigned byte=0;byte<256;++byte){unsigned char name[2]={'a',(unsigned char)byte};name_case(name+1,1,letter(byte));name_case(name,2,letter(byte)||(byte>='0'&&byte<='9'));}unsigned char name[257];memset(name,'a',sizeof name);name_case(name,256,true);name_case(name,257,false);name_case(name,0,false);const unsigned char utf8[]={0xc3,0xa9};name_case(utf8,sizeof utf8,false);}
static void bound_case(size_t start,const unsigned char *scalar,size_t width,bool as_field){
    unsigned char bytes[4105];memset(bytes,'a',sizeof bytes);memcpy(bytes+start,scalar,width);size_t n=start+width+2;size_t expected=n<=4096?n:(start<4096&&start+width>4096?start+width:4096);
    Error *e=fresh();H msg=owned(bytes,n);assert((as_field?jet_web_forms_action_field_error(h(e),string("key"),msg):jet_web_forms_action_form_error(h(e),msg))==h(e));List *list=as_field?&((Entry *)e->fields.data)[0].messages:&e->form;Text *out=message(list,0);assert(h(out)==msg&&out->cap==n);text_eq(out,bytes,expected);clone_and_drop(e);
}
int main(void){jet_rt_alloc_init();tracking=true;clone_and_drop(fresh());ordered_case();names();const unsigned char scalars[4][4]={{'x',0,0,0},{0xc3,0xa9,0,0},{0xe2,0x82,0xac,0},{0xf0,0x9f,0x98,0x80}};for(size_t w=1;w<=4;++w)for(size_t start=4092;start<=4096;++start)for(unsigned field_mode=0;field_mode<2;++field_mode)bound_case(start,scalars[w-1],w,field_mode!=0);jet_rt_handle_drop_jet_web_form_action_error(0);balanced();puts("UI ActionError: real consumed producers, ordered maps, name and UTF8 bounds, independent clones and exact frees ok");return 0;}
