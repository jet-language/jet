/* Real Jet providers --wrap=jet_rt_alloc/jet_rt_free, no C providers.
 * Cursor3 reuses febd2658e canonical inline E4+pos, never a second provider. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>
typedef int64_t H;
typedef struct {unsigned char *data;uint64_t len,cap;} Text;
typedef struct {H width,height;Text *title;H native;} Window;
typedef struct {Text *name;H fields[4];} Region;
typedef struct {uint64_t len,cap;Region *data;} Regions;
typedef struct {Text *path,*name,*texture_path;Regions *regions;H present;int32_t texture[5];uint32_t padding;} Atlas;
typedef struct {Text buf;uint64_t pos;} Cursor;
_Static_assert(sizeof(Window)==32&&sizeof(Region)==40&&sizeof(Atlas)==64&&sizeof(Cursor)==32,"native-private layouts");
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H),__real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern void jet_rt_string_drop(Text *);
extern H jet_int_owned_from_i64(H),jet_int_owned_to_i64(H);
extern void jet_rt_int_drop(H);
extern H jet_rt_record_new_raylib_window(H,H,Text *,uint8_t),jet_rt_handle_clone_raylib_window(H);
extern void jet_rt_handle_drop_raylib_window(H);
extern H jet_rt_record_get_raylib_window_width(H),jet_rt_record_get_raylib_window_height(H);
extern Text *jet_rt_record_get_raylib_window_title(H);
extern bool jet_rt_record_get_raylib_window_native(H);
extern H jet_rt_handle_clone_raylib_texture_atlas(H);
extern void jet_rt_handle_drop_raylib_texture_atlas(H);
extern void jet_rt_handle_field_raylib_texture_atlas_path(H,Text *),jet_rt_handle_field_raylib_texture_atlas_name(H,Text *);
extern Text *jet_rt_handle_show_raylib_texture_atlas(H),*jet_rt_handle_debug_raylib_texture_atlas(H);
extern H jet_rt_handle_clone_jet_cursor(H);
extern void jet_rt_handle_drop_jet_cursor(H),jet_rt_handle_field_jet_cursor_buf(H,Text *);
static struct {H address,bytes,align;bool live;} ledger[1024];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking){assert(address&&bytes>0&&allocations<1024&&(uint64_t)address%(uint64_t)align==0);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live||ledger[i].address!=address);ledger[allocations].address=address;ledger[allocations].bytes=bytes;ledger[allocations].align=align;ledger[allocations++].live=true;}
    return address;
}
void __wrap_jet_rt_free(H address,H bytes,H align) {
    if(tracking){for(size_t i=allocations;i>0;--i)if(ledger[i-1].address==address&&ledger[i-1].live){assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align);ledger[i-1].live=false;++frees;__real_jet_rt_free(address,bytes,align);return;}assert(!"untracked or double free");}
    __real_jet_rt_free(address,bytes,align);
}
static void balanced(void){assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);allocations=frees=0;}
static Text *text(const char *s){return jet_rt_string_from_static(s,(H)strlen(s));}
static void text_is(Text *t,const char *s){assert(t->len==strlen(s)&&!memcmp(t->data,s,t->len));}
static void exact_is(H value,H expected){assert(jet_int_owned_to_i64(value)==expected);jet_rt_int_drop(value);}
static void window(uint8_t native) {
    H min=jet_int_owned_from_i64(INT64_MIN),max=jet_int_owned_from_i64(INT64_MAX);Text *title=text("win\xc3\xa9");
    H a=jet_rt_record_new_raylib_window(min,max,title,native);jet_rt_int_drop(min);jet_rt_int_drop(max);jet_rt_string_drop(title);
    Window *x=(Window *)(uintptr_t)a,*y=(Window *)(uintptr_t)jet_rt_handle_clone_raylib_window(a);H b=(H)(uintptr_t)y;
    assert(x!=y&&x->title!=y->title&&x->title->data!=y->title->data);assert(x->width==INT64_MIN&&x->height==INT64_MAX);
    exact_is(jet_rt_record_get_raylib_window_width(a),INT64_MIN);exact_is(jet_rt_record_get_raylib_window_height(a),INT64_MAX);
    assert(jet_rt_record_get_raylib_window_native(a)==(native!=0));Text *copy=jet_rt_record_get_raylib_window_title(a);assert(copy!=x->title);text_is(copy,"win\xc3\xa9");jet_rt_string_drop(copy);
    x->native=(H)UINT64_C(0xdeadbeef00000000)|(native!=0);assert(jet_rt_record_get_raylib_window_native(a)==(native!=0));
    jet_rt_handle_drop_raylib_window(a);text_is(y->title,"win\xc3\xa9");jet_rt_handle_drop_raylib_window(b);balanced();
}
static void atlas(bool path,bool texture,size_t n) {
    Atlas *a=(Atlas *)(uintptr_t)jet_rt_alloc(sizeof *a,8);a->path=text("atlas.path");a->name=text("A\"\n\xc3\xa9");a->texture_path=path?text(""):NULL;
    Regions *r=(Regions *)(uintptr_t)jet_rt_alloc(sizeof *r,8);r->len=n;r->cap=n+2;r->data=(Region *)(uintptr_t)jet_rt_alloc((H)r->cap*sizeof(Region),8);a->regions=r;
    for(size_t i=0;i<n;++i){r->data[i].name=text(i%2?"duplicate":"first");for(size_t j=0;j<4;++j)r->data[i].fields[j]=i%2?INT64_MIN+(H)j:INT64_MAX-(H)j;}
    a->present=(H)UINT64_C(0xdeadbeef00000000)|texture;for(size_t i=0;i<5;++i)a->texture[i]=(int32_t)(UINT32_C(0x80000000)+(uint32_t)i);a->padding=UINT32_MAX;
    H ah=(H)(uintptr_t)a;Atlas *b=(Atlas *)(uintptr_t)jet_rt_handle_clone_raylib_texture_atlas(ah);H bh=(H)(uintptr_t)b;
    assert(a!=b&&a->path!=b->path&&a->name!=b->name&&r!=b->regions&&b->regions->len==n&&b->regions->cap==n);assert((b->texture_path!=NULL)==path&&b->present==texture&&b->padding==0);
    if(path)assert(a->texture_path!=b->texture_path);
    if(texture)assert(!memcmp(a->texture,b->texture,sizeof a->texture));
    Text view={0};size_t before=allocations;jet_rt_handle_field_raylib_texture_atlas_name(ah,&view);assert(view.data==a->name->data&&view.len==a->name->len&&view.cap==a->name->cap&&allocations==before);jet_rt_handle_field_raylib_texture_atlas_path(ah,&view);assert(view.data==a->path->data&&allocations==before);
    Text *show=jet_rt_handle_show_raylib_texture_atlas(ah),*debug=jet_rt_handle_debug_raylib_texture_atlas(ah);text_is(show,"RaylibTextureAtlas(A\"\n\xc3\xa9)");text_is(debug,"RaylibTextureAtlas(A\"\n\xc3\xa9)");jet_rt_string_drop(show);jet_rt_string_drop(debug);
    for(size_t i=0;i<n;++i){assert(r->data[i].name!=b->regions->data[i].name&&!memcmp(r->data[i].fields,b->regions->data[i].fields,sizeof r->data[i].fields));text_is(b->regions->data[i].name,i%2?"duplicate":"first");}
    jet_rt_handle_drop_raylib_texture_atlas(ah);text_is(b->name,"A\"\n\xc3\xa9");jet_rt_handle_drop_raylib_texture_atlas(bh);balanced();
}
static void cursor(const char *s,uint64_t pos) {
    Cursor *a=(Cursor *)(uintptr_t)jet_rt_alloc(sizeof *a,8);size_t n=strlen(s);a->buf.len=a->buf.cap=n;a->buf.data=n?(unsigned char *)(uintptr_t)jet_rt_alloc((H)n,1):(unsigned char *)(uintptr_t)1;a->pos=pos;if(n)memcpy(a->buf.data,s,n);
    H ah=(H)(uintptr_t)a;Cursor *b=(Cursor *)(uintptr_t)jet_rt_handle_clone_jet_cursor(ah);H bh=(H)(uintptr_t)b;assert(a!=b&&b->pos==pos);text_is(&b->buf,s);if(n)assert(a->buf.data!=b->buf.data);
    size_t before=allocations;Text view={0};jet_rt_handle_field_jet_cursor_buf(ah,&view);assert(view.data==a->buf.data&&view.len==a->buf.len&&view.cap==a->buf.cap&&allocations==before);
    jet_rt_handle_drop_jet_cursor(ah);text_is(&b->buf,s);jet_rt_handle_drop_jet_cursor(bh);balanced();
}
int main(void){jet_rt_alloc_init();tracking=true;window(0);window(1);window(255);for(unsigned i=0;i<4;++i){atlas(i&1,i&2,0);atlas(i&1,i&2,17);}cursor("",UINT64_MAX);cursor("a\xc3\xa9",UINT64_C(0x8000000000000001));jet_rt_handle_drop_raylib_window(0);jet_rt_handle_drop_raylib_texture_atlas(0);jet_rt_handle_drop_jet_cursor(0);balanced();puts("UI raylib metadata and canonical Cursor: raw fields, ordered regions, option textures, projections and exact-once frees ok");return 0;}
