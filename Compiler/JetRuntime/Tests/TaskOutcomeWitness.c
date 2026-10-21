/* Real native exports; link --wrap=jet_rt_free. Expected bytes, not receipts.
 * Contract: Core/Service/Service.jet:91-125 at 497daa7a0, read-only.
 * Its exact a\"b display/debug vector is asserted below. No workflow proof.
 * wrong mode must use the shared canonical record-variant panic, exit70. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t tag; Text *reason; } TaskOutcome;
_Static_assert(sizeof(TaskOutcome)==16 && offsetof(TaskOutcome,reason)==8,"TaskOutcome16");
extern void jet_rt_alloc_init(void);
extern TaskOutcome *jet_rt_record_new_jet_task_outcome_finished(void);
extern TaskOutcome *jet_rt_record_new_jet_task_outcome_panicked(Text *);
extern TaskOutcome *jet_rt_record_new_jet_task_outcome_cancelled(void);
extern TaskOutcome *jet_rt_record_new_jet_task_outcome_deadline_blown(void);
extern int64_t jet_rt_record_tag_jet_task_outcome(TaskOutcome *);
extern Text *jet_rt_record_get_jet_task_outcome_panicked(TaskOutcome *);
extern TaskOutcome *jet_rt_handle_clone_jet_task_outcome(TaskOutcome *);
extern void jet_rt_handle_drop_jet_task_outcome(TaskOutcome *);
extern bool jet_rt_handle_eq_jet_task_outcome(TaskOutcome *,TaskOutcome *);
extern Text *jet_rt_handle_show_jet_task_outcome(TaskOutcome *);
extern Text *jet_rt_handle_display_jet_task_outcome(TaskOutcome *);
extern Text *jet_rt_handle_debug_jet_task_outcome(TaskOutcome *);
extern TaskOutcome *jet_rt_record_new_jet_task_failure_cancelled(void);
extern TaskOutcome *jet_rt_record_new_jet_task_failure_deadline_blown(void);
extern TaskOutcome *jet_rt_record_new_jet_task_failure_panicked(Text *);
extern Text *jet_rt_handle_show_jet_task_failure(TaskOutcome *);
extern Text *jet_rt_handle_display_jet_task_failure(TaskOutcome *);
extern Text *jet_rt_handle_debug_jet_task_failure(TaskOutcome *);
extern void jet_rt_handle_drop_jet_task_failure(TaskOutcome *);
extern void jet_rt_string_drop(Text *);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
#include "TextDebugGoldens.h"

struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[4096];
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
static Text borrow(const void *bytes,int64_t len) { return (Text){(unsigned char *)bytes,len,0}; }
static void text_watch(Text *value) {
    assert(value && value->len>=0 && value->cap>=value->len);
    watch(value,24,8);
    if(value->cap) watch(value->data,value->cap,1);
}
static void outcome_watch(TaskOutcome *value) {
    assert(value);
    watch(value,16,8);
    if(value->tag==1) text_watch(value->reason);
}
static void same_text(Text *value,const void *bytes,int64_t len) {
    assert(value && value->len==len);
    if(len) assert(memcmp(value->data,bytes,(size_t)len)==0);
}
static void rendered(Text *value,const void *bytes,int64_t len) {
    text_watch(value);
    same_text(value,bytes,len);
    jet_rt_string_drop(value);
}
static void panic_text(Text *value,const void *reason,int64_t len) {
    unsigned char expected[256];
    assert(len>=0 && len<246);
    memcpy(expected,"Panicked(",9);
    memcpy(expected+9,reason,(size_t)len);
    expected[9+len]=')';
    rendered(value,expected,len+10);
}
static void panic_render(TaskOutcome *value,const void *raw,int64_t raw_len,const void *quoted,int64_t quoted_len) {
    panic_text(jet_rt_handle_show_jet_task_outcome(value),raw,raw_len);
    panic_text(jet_rt_handle_display_jet_task_outcome(value),raw,raw_len);
    panic_text(jet_rt_handle_debug_jet_task_outcome(value),quoted,quoted_len);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1 && strcmp(argv[1],"wrong")==0) {
        TaskOutcome *unit=jet_rt_record_new_jet_task_outcome_finished();
        (void)jet_rt_record_get_jet_task_outcome_panicked(unit);
        assert(!"wrong variant getter returned");
    }
    TaskOutcome *units[]={jet_rt_record_new_jet_task_outcome_finished(),jet_rt_record_new_jet_task_outcome_cancelled(),jet_rt_record_new_jet_task_outcome_deadline_blown()};
    const int64_t tags[]={0,2,3};
    const char *names[]={"Finished","Cancelled","DeadlineBlown"};
    for(size_t i=0;i<3;++i) {
        TaskOutcome *unit=units[i];
        outcome_watch(unit);
        assert(jet_rt_record_tag_jet_task_outcome(unit)==tags[i] && unit->reason==NULL);
        /* Inactive storage is not owned: clone/eq/render/drop must ignore it. */
        unit->reason=(Text *)(uintptr_t)1;
        TaskOutcome *clone=jet_rt_handle_clone_jet_task_outcome(unit);
        outcome_watch(clone);
        assert(clone!=unit && clone->reason==NULL && jet_rt_handle_eq_jet_task_outcome(unit,clone));
        for(size_t j=0;j<3;++j) if(i!=j) assert(!jet_rt_handle_eq_jet_task_outcome(unit,units[j]));
        int64_t n=(int64_t)strlen(names[i]);
        rendered(jet_rt_handle_show_jet_task_outcome(unit),names[i],n);
        rendered(jet_rt_handle_display_jet_task_outcome(unit),names[i],n);
        rendered(jet_rt_handle_debug_jet_task_outcome(unit),names[i],n);
        jet_rt_handle_drop_jet_task_outcome(clone);
    }
    /* Explicit 497daa7a0 service #Test vector: raw a\"b, debug quoted/escaped. */
    Text source=borrow("a\"b",3);
    TaskOutcome *vector=jet_rt_record_new_jet_task_outcome_panicked(&source);
    outcome_watch(vector);
    assert(jet_rt_record_tag_jet_task_outcome(vector)==1 && vector->reason!=&source);
    panic_render(vector,"a\"b",3,"\"a\\\"b\"",6);
    for(size_t i=0;i<3;++i) assert(!jet_rt_handle_eq_jet_task_outcome(vector,units[i]));
    jet_rt_handle_drop_jet_task_outcome(vector);
    unsigned char mutable[]={'a',0,'\t','\n','"','\\',0xce,0xbb};
    unsigned char original[sizeof(mutable)];
    memcpy(original,mutable,sizeof(mutable));
    Text input=borrow(mutable,(int64_t)sizeof(mutable));
    TaskOutcome *parent=jet_rt_record_new_jet_task_outcome_panicked(&input);
    outcome_watch(parent);
    TaskOutcome *clone=jet_rt_handle_clone_jet_task_outcome(parent);
    outcome_watch(clone);
    assert(parent!=clone && parent->reason!=clone->reason && parent->reason->data!=clone->reason->data);
    assert(jet_rt_handle_eq_jet_task_outcome(parent,clone));
    Text *getter=jet_rt_record_get_jet_task_outcome_panicked(parent);
    text_watch(getter);
    assert(getter!=parent->reason && getter->data!=parent->reason->data && getter->data!=clone->reason->data);
    mutable[0]='z';
    same_text(parent->reason,original,(int64_t)sizeof(original));
    same_text(clone->reason,original,(int64_t)sizeof(original));
    same_text(getter,original,(int64_t)sizeof(original));
    TaskOutcome *different=jet_rt_record_new_jet_task_outcome_panicked(&input);
    outcome_watch(different);
    assert(!jet_rt_handle_eq_jet_task_outcome(parent,different));
    jet_rt_handle_drop_jet_task_outcome(parent);
    panic_render(clone,original,(int64_t)sizeof(original),"\"a\\0\\t\\n\\\"\\\\\xce\xbb\"",15);
    same_text(getter,original,(int64_t)sizeof(original));
    jet_rt_string_drop(getter);
    jet_rt_handle_drop_jet_task_outcome(clone);
    jet_rt_handle_drop_jet_task_outcome(different);
    for(size_t i=0;i<sizeof(debug_goldens)/sizeof(debug_goldens[0]);++i) {
        Text text=borrow(debug_goldens[i].input,debug_goldens[i].input_length);
        TaskOutcome *value=jet_rt_record_new_jet_task_outcome_panicked(&text);
        outcome_watch(value);
        panic_render(value,debug_goldens[i].input,debug_goldens[i].input_length,debug_goldens[i].expected,debug_goldens[i].expected_length);
        jet_rt_handle_drop_jet_task_outcome(value);
        /* The shared tail must preserve every TaskFailure rendering byte. */
        TaskOutcome *failure=jet_rt_record_new_jet_task_failure_panicked(&text);
        assert(failure->tag==2);
        watch(failure,16,8);
        text_watch(failure->reason);
        panic_text(jet_rt_handle_show_jet_task_failure(failure),debug_goldens[i].input,debug_goldens[i].input_length);
        panic_text(jet_rt_handle_display_jet_task_failure(failure),debug_goldens[i].input,debug_goldens[i].input_length);
        panic_text(jet_rt_handle_debug_jet_task_failure(failure),debug_goldens[i].expected,debug_goldens[i].expected_length);
        jet_rt_handle_drop_jet_task_failure(failure);
    }
    TaskOutcome *failures[]={jet_rt_record_new_jet_task_failure_cancelled(),jet_rt_record_new_jet_task_failure_deadline_blown()};
    for(size_t i=0;i<2;++i) {
        assert(failures[i]->tag==(int64_t)i);
        watch(failures[i],16,8);
        failures[i]->reason=(Text *)(uintptr_t)1;
        const char *name=names[i+1];
        int64_t n=(int64_t)strlen(name);
        rendered(jet_rt_handle_show_jet_task_failure(failures[i]),name,n);
        rendered(jet_rt_handle_display_jet_task_failure(failures[i]),name,n);
        rendered(jet_rt_handle_debug_jet_task_failure(failures[i]),name,n);
        jet_rt_handle_drop_jet_task_failure(failures[i]);
    }
    for(size_t i=0;i<3;++i) jet_rt_handle_drop_jet_task_outcome(units[i]);
    jet_rt_handle_drop_jet_task_outcome(NULL);
    assert(interceptions>0);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("TaskOutcome constructors/ownership/rendering OK");
    return 0;
}
