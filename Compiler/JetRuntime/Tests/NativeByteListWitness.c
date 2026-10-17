// Real packed List.Byte client, not a Rust Vec-layout witness.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int64_t length,capacity; unsigned char *data; } List;
_Static_assert(sizeof(List)==24,"native List24");
extern void jet_rt_alloc_init(void);
extern void jet_rt_free(void *,int64_t,int64_t);
extern List *jet_rt_record_seq_list_byte_new(int64_t);
extern int64_t jet_rt_record_seq_list_byte_len(List *);
extern uint64_t jet_rt_record_seq_list_byte_get(List *,int64_t);
extern void jet_rt_record_seq_list_byte_push(List *,uint64_t);
static void drop(List *value) {
    if(value->capacity) jet_rt_free(value->data,value->capacity,8);
    jet_rt_free(value,24,8);
}
static void check(int64_t capacity) {
    List *list=jet_rt_record_seq_list_byte_new(capacity);
    assert(list->length==0 && list->capacity==capacity && jet_rt_record_seq_list_byte_len(list)==0);
    if(!capacity) assert((uintptr_t)list->data==8);
    for(int64_t i=0;i<1025;++i) {
        jet_rt_record_seq_list_byte_push(list,(uint64_t)i%256);
        assert(jet_rt_record_seq_list_byte_len(list)==i+1 && list->capacity>=list->length);
        // Observing every physical byte proves stride1, including growth.
        for(int64_t j=0;j<=i;++j) assert(list->data[j]==(unsigned char)j && jet_rt_record_seq_list_byte_get(list,j)==(uint64_t)j%256);
    }
    drop(list);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"negative-capacity")) { (void)jet_rt_record_seq_list_byte_new(-1); return 9; }
        List *list=jet_rt_record_seq_list_byte_new(2);
        jet_rt_record_seq_list_byte_push(list,0);
        if(!strcmp(argv[1],"negative-index")) { (void)jet_rt_record_seq_list_byte_get(list,-1); return 9; }
        if(!strcmp(argv[1],"index-equals-length")) { (void)jet_rt_record_seq_list_byte_get(list,1); return 9; }
        if(!strcmp(argv[1],"index-over-length")) { (void)jet_rt_record_seq_list_byte_get(list,INT64_MAX); return 9; }
        if(!strcmp(argv[1],"byte-256")) { jet_rt_record_seq_list_byte_push(list,256); return 9; }
        if(!strcmp(argv[1],"byte-u64-max")) { jet_rt_record_seq_list_byte_push(list,UINT64_MAX); return 9; }
        return 8;
    }
    assert(argc==1); check(0); check(3); check(1025);
    List *empty=jet_rt_record_seq_list_byte_new(0); drop(empty);
    puts("List.Byte packed stride1/capacity/growth/values OK"); return 0;
}
