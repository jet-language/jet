/* Link the real composed Jet object with --wrap=jet_rt_free and the test-only
 * AttachmentWitnessABI.jet stride export; no provider mocks. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; unsigned char *data; } Bytes;
typedef struct { Text *filename,*mime; Bytes bytes; } Attachment;
_Static_assert(sizeof(Text)==24 && sizeof(Bytes)==24,"headers");
_Static_assert(sizeof(Attachment)==40 && offsetof(Attachment,bytes)==16 && _Alignof(Attachment)==8,"Attachment40");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t runtime_attachment_u8_stride(void);
extern Text *jet_rt_string_from_static(unsigned char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Attachment *jet_rt_record_new_attachment(Text *,Text *,Bytes *);
extern Attachment *jet_rt_handle_clone_attachment(Attachment *);
extern void jet_rt_handle_drop_attachment(Attachment *);
extern bool jet_rt_handle_eq_attachment(Attachment *,Attachment *);
extern Text *jet_rt_record_get_attachment_filename(Attachment *);
extern Text *jet_rt_record_get_attachment_mime(Attachment *);
extern Bytes *jet_rt_record_get_attachment_bytes(Attachment *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[512];
static size_t count;
static int64_t stride;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(watched[i-1].size==size && watched[i-1].align==align);
        watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
/* Heap Text headers live at t-8 (count word + 24-byte header); immortal
 * cap-0 headers (data != 1) have no heap block and must not be watched. */
static void text_watch(Text *t) {
    if(t->cap || t->data==(unsigned char *)1) watch((char *)t-8,32,8);
    if(t->cap) watch(t->data,t->cap,1);
}
static void record_watch(Attachment *a,bool transferred) {
    watch(a,40,8);
    if(!transferred && a->bytes.cap) watch(a->bytes.data,a->bytes.cap*stride,8);
}
static void attachment_watch(Attachment *a,bool transferred) {
    record_watch(a,transferred); text_watch(a->filename); text_watch(a->mime);
}
static void text_equal(Text *a,Text *b) { assert(a->len==b->len && !memcmp(a->data,b->data,(size_t)a->len)); }
static Bytes *list(int64_t len,int64_t spare) {
    Bytes *b=jet_rt_alloc(24,8); b->len=len; b->cap=len+spare;
    b->data=b->cap ? jet_rt_alloc(b->cap*stride,8) : NULL;
    watch(b,24,8); if(b->cap) { watch(b->data,b->cap*stride,8); memset(b->data,0xa5,(size_t)(b->cap*stride)); }
    for(int64_t i=0;i<len;++i) b->data[i*stride]=(unsigned char)(i ? 255 : 0);
    return b;
}
int main(void) {
    jet_rt_alloc_init(); stride=runtime_attachment_u8_stride(); assert(stride==1);
    jet_rt_handle_drop_attachment(NULL);
    unsigned char name_data[]={'f',0,0xc3,0xa9},mime_data[]={'a','/',0,'b'};
    Text name={name_data,4,4},mime={mime_data,4,4},empty={(unsigned char *)"",0,0};
    for(int64_t len=0;len<3;++len) for(int64_t spare=0;spare<3;++spare) {
        Bytes *b=list(len,spare); unsigned char *data=b->data; size_t shell=count-(b->cap ? 2 : 1);
        Text *n=jet_rt_string_from_static(name_data,4),*m=jet_rt_string_from_static(mime_data,4);
        Attachment *a=jet_rt_record_new_attachment(n,m,b); attachment_watch(a,true);
        assert(watched[shell].freed && a->filename==n && a->mime==m);
        assert(a->bytes.data==data && a->bytes.len==len && a->bytes.cap==len+spare);
        jet_rt_string_drop(n); jet_rt_string_drop(m); text_equal(a->filename,&name); text_equal(a->mime,&mime);
        assert(jet_rt_record_get_attachment_bytes(a)==&a->bytes);
        Attachment *copy=jet_rt_handle_clone_attachment(a); record_watch(copy,false);
        assert(copy!=a && copy->filename==a->filename && copy->mime==a->mime && copy->bytes.cap==len);
        assert(jet_rt_handle_eq_attachment(a,copy) && jet_rt_handle_eq_attachment(copy,a));
        if(len) {
            assert(copy->bytes.data!=a->bytes.data); copy->bytes.data[0]^=1;
            assert(!jet_rt_handle_eq_attachment(a,copy)); copy->bytes.data[0]^=1;
        }
        copy->bytes.len=len+1; assert(!jet_rt_handle_eq_attachment(a,copy)); copy->bytes.len=len;
        Text *got_name=jet_rt_record_get_attachment_filename(a),*got_mime=jet_rt_record_get_attachment_mime(a);
        assert(got_name==a->filename && got_mime==a->mime);
        jet_rt_handle_drop_attachment(a); text_equal(got_name,&name); text_equal(got_mime,&mime);
        text_equal(copy->filename,&name); text_equal(copy->mime,&mime);
        for(int64_t i=0;i<len;++i) assert(copy->bytes.data[i*stride]==(unsigned char)(i ? 255 : 0));
        jet_rt_string_drop(got_name); jet_rt_string_drop(got_mime); jet_rt_handle_drop_attachment(copy);
    }
    Attachment *a=jet_rt_record_new_attachment(&empty,&empty,list(0,0)); attachment_watch(a,true);
    Attachment *b=jet_rt_handle_clone_attachment(a); attachment_watch(b,false); assert(jet_rt_handle_eq_attachment(a,b));
    jet_rt_handle_drop_attachment(a); jet_rt_handle_drop_attachment(b);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("attachment: constructor text borrow/list transfer, owned text and borrowed bytes getters, deep clone/value equality, binary and empty data, capacity frees, packed stride, null drop OK");
    return 0;
}
