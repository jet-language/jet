/* Real composed Jet object client, --wrap=jet_rt_free; no provider mocks. */
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { Text *table,*expression; int64_t compiled; } Policy;
typedef struct { uint64_t handle; Policy policy; Text *user,*request; } Scope;
_Static_assert(sizeof(Policy)==24 && sizeof(Scope)==48 && offsetof(Scope,policy)==8 && offsetof(Scope,user)==32 && offsetof(Scope,request)==40,"Scope48 inline Policy24");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern Policy *jet_rt_handle_clone_jet_row_policy(Policy *);
extern void jet_rt_handle_drop_jet_row_policy(Policy *);
extern Scope *jet_rt_handle_clone_jet_db_scope(Scope *);
extern void jet_rt_handle_drop_jet_db_scope(Scope *);
extern Policy *jet_rt_handle_field_jet_db_scope_policy(Scope *);
extern void jet_rt_handle_field_jet_db_scope_user(Scope *,Text *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[512];
static size_t count;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==watched[i-1].align); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *t) { watch(t,24,8); if(t->cap) watch(t->data,t->cap,1); }
static Text *text(unsigned empty) {
    unsigned char bytes[]={'a',0,0xc3,0xa9}; Text input={bytes,empty ? 0 : 4,empty ? 0 : 4};
    Text *out=jet_rt_string_clone(&input); text_watch(out); return out;
}
static void policy_watch(Policy *p) { text_watch(p->table); text_watch(p->expression); }
static void scope_watch(Scope *s) {
    watch(s,48,8); policy_watch(&s->policy); text_watch(s->user); if(s->request) text_watch(s->request);
}
static void equal_text(Text *a,Text *b) { assert(a->len==b->len && !memcmp(a->data,b->data,(size_t)a->len)); }
static void independent(Text *a,Text *b) {
    assert(a!=b); equal_text(a,b);
    if(a->len) { assert(a->data!=b->data); b->data[0]^=1; assert(a->data[0]!=b->data[0]); b->data[0]^=1; }
}
int main(void) {
    jet_rt_alloc_init(); jet_rt_handle_drop_jet_db_scope(NULL);
    for(unsigned kind=0;kind<2;++kind) for(unsigned request=0;request<3;++request) {
        Scope *a=jet_rt_alloc(48,8); watch(a,48,8); a->handle=UINT64_MAX;
        a->policy=(Policy){text(0),text(0),(int64_t)kind}; a->user=text(0); a->request=request ? text(request==2) : NULL;
        Scope *b=jet_rt_handle_clone_jet_db_scope(a); scope_watch(b);
        assert(a!=b && b->handle==UINT64_MAX && b->policy.compiled==kind);
        independent(a->policy.table,b->policy.table); independent(a->policy.expression,b->policy.expression); independent(a->user,b->user);
        assert((a->request==NULL)==(b->request==NULL)); if(a->request) independent(a->request,b->request);
        Policy *projection=jet_rt_handle_field_jet_db_scope_policy(a); assert(projection==&a->policy);
        Policy *owned=jet_rt_handle_clone_jet_row_policy(projection); watch(owned,24,8); policy_watch(owned);
        independent(projection->table,owned->table); independent(projection->expression,owned->expression); assert(owned->compiled==kind);
        Text borrowed={NULL,-1,-1}; jet_rt_handle_field_jet_db_scope_user(a,&borrowed);
        assert(borrowed.data==a->user->data && borrowed.len==a->user->len && borrowed.cap==a->user->cap);
        Text *user=jet_rt_string_clone(&borrowed); text_watch(user); independent(a->user,user);
        jet_rt_handle_drop_jet_db_scope(a); equal_text(user,b->user); equal_text(owned->table,b->policy.table); equal_text(owned->expression,b->policy.expression);
        jet_rt_string_drop(user); jet_rt_handle_drop_jet_row_policy(owned); jet_rt_handle_drop_jet_db_scope(b);
    }
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("db-scope: raw ID, canonical inline Policy24 helpers, deep user/request clone, borrowed policy/user projections, nullable/empty request, exact frees, null drop OK");
    return 0;
}
