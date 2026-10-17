// Real Jet-object client; requires canonical MailAttachment7641c13a6 and
// DataSequences6d4fa9ba6, linked with -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; unsigned char *data; } Bytes;
typedef struct { Text *filename,*mime; Bytes bytes; } Attachment;
typedef struct { int64_t length,capacity; Attachment *data; } List;
_Static_assert(sizeof(Attachment)==40 && sizeof(List)==24,"Attachment40/native List24");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern void jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_new(unsigned char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Bytes *jet_rt_record_seq_list_byte_new(int64_t);
extern void jet_rt_record_seq_list_byte_push(Bytes *,uint64_t);
extern Attachment *jet_rt_record_new_attachment(Text *,Text *,Bytes *);
extern Attachment *jet_rt_handle_clone_attachment(Attachment *);
extern void jet_rt_handle_drop_attachment(Attachment *);
extern List *jet_rt_record_seq_list_rec_attachment_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_attachment_len(List *);
extern Attachment *jet_rt_record_seq_list_rec_attachment_get(List *,int64_t);
extern void jet_rt_record_seq_list_rec_attachment_push(List *,Attachment *);
static void *donor;
static unsigned donor_frees,intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    if(p==donor) { assert(bytes==40 && align==8 && ++donor_frees==1); }
    __real_jet_rt_free(p,bytes,align);
}
static Attachment *make(unsigned value) {
    unsigned char name[]={'n',0,(unsigned char)value};
    Text *filename=jet_rt_string_new(name,3),*mime=jet_rt_string_new((unsigned char *)"mime/type",9);
    Bytes *bytes=jet_rt_record_seq_list_byte_new(5);
    jet_rt_record_seq_list_byte_push(bytes,value); jet_rt_record_seq_list_byte_push(bytes,255); jet_rt_record_seq_list_byte_push(bytes,0);
    Attachment *a=jet_rt_record_new_attachment(filename,mime,bytes);
    jet_rt_string_drop(filename); jet_rt_string_drop(mime); return a;
}
static void move(List *list,Attachment *a) {
    Text *filename=a->filename,*mime=a->mime; unsigned char *data=a->bytes.data;
    int64_t length=list->length; donor=a; donor_frees=0;
    jet_rt_record_seq_list_rec_attachment_push(list,a);
    assert(donor_frees==1); donor=NULL;
    assert(list->length==length+1);
    Attachment *stored=jet_rt_record_seq_list_rec_attachment_get(list,length);
    assert(stored==list->data+length && stored->filename==filename && stored->mime==mime && stored->bytes.data==data);
}
static void drop(List *list) {
    // The test owns the returned list shell. Inline fields are independently
    // dropped once; do not call heap-element drop on a borrowed inline slot.
    for(int64_t i=0;i<list->length;++i) {
        Attachment *a=list->data+i;
        jet_rt_string_drop(a->filename); jet_rt_string_drop(a->mime);
        if(a->bytes.capacity) jet_rt_free(a->bytes.data,a->bytes.capacity,8);
    }
    if(list->capacity) jet_rt_free(list->data,list->capacity*40,8);
    jet_rt_free(list,24,8);
}
static void check(int64_t capacity) {
    List *list=jet_rt_record_seq_list_rec_attachment_new(capacity);
    assert(list->length==0 && list->capacity==capacity);
    if(!capacity) assert((uintptr_t)list->data==8);
    for(unsigned i=0;i<19;++i) move(list,make(i));
    assert(jet_rt_record_seq_list_rec_attachment_len(list)==19 && list->capacity>=19);
    for(int64_t i=0;i<19;++i) {
        Attachment *a=jet_rt_record_seq_list_rec_attachment_get(list,i);
        assert(a==list->data+i && a->filename->length==3 && a->filename->data[1]==0 && a->filename->data[2]==i);
        assert(a->bytes.length==3 && a->bytes.capacity==5 && a->bytes.data[0]==i && a->bytes.data[1]==255 && a->bytes.data[2]==0);
    }
    Attachment *borrow=jet_rt_record_seq_list_rec_attachment_get(list,7),*clone=jet_rt_handle_clone_attachment(borrow);
    assert(clone->filename!=borrow->filename && clone->bytes.data!=borrow->bytes.data);
    borrow->bytes.data[0]=99; assert(clone->bytes.data[0]==7); borrow->bytes.data[0]=7;
    drop(list); assert(clone->filename->data[2]==7 && clone->bytes.data[0]==7);
    jet_rt_handle_drop_attachment(clone);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"negative-capacity")) { (void)jet_rt_record_seq_list_rec_attachment_new(-1); return 9; }
        if(!strcmp(argv[1],"capacity-overflow")) { (void)jet_rt_record_seq_list_rec_attachment_new(INT64_MAX/40+1); return 9; }
        List *list=jet_rt_record_seq_list_rec_attachment_new(0); move(list,make(0));
        if(!strcmp(argv[1],"negative-index")) { (void)jet_rt_record_seq_list_rec_attachment_get(list,-1); return 9; }
        if(!strcmp(argv[1],"index-equals-length")) { (void)jet_rt_record_seq_list_rec_attachment_get(list,1); return 9; }
        if(!strcmp(argv[1],"index-over-length")) { (void)jet_rt_record_seq_list_rec_attachment_get(list,INT64_MAX); return 9; }
        return 8;
    }
    assert(argc==1); check(0); check(3); check(19);
    List *empty=jet_rt_record_seq_list_rec_attachment_new(0); drop(empty);
    assert(intercepted); puts("AttachmentSeq inline40/move/borrow/growth/ownership OK"); return 0;
}
