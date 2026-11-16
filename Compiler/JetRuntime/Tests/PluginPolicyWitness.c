// Link real Jet runtime, never mock providers.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *function,*component; int64_t offset; } Frame;
typedef struct { int64_t length,capacity; Frame *data; } Frames;
typedef struct { Text *export_name,*message; Frames *frames; } Fault;
typedef struct { int64_t id; Text *name; } Span;
typedef struct { int64_t capacity,overflow; } Policy;
_Static_assert(sizeof(Frame)==24 && offsetof(Frame,offset)==16,"PluginABI Frame");
_Static_assert(sizeof(Fault)==24 && offsetof(Fault,frames)==16,"PluginABI Fault");
_Static_assert(sizeof(Frames)==24 && offsetof(Frames,data)==16,"List24");
_Static_assert(sizeof(Span)==16 && offsetof(Span,name)==8,"LogABI");
_Static_assert(sizeof(Policy)==16,"AsyncPolicyABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Frame *jet_rt_record_new_plugin_frame(Text *,Text *,int64_t);
extern Frame *jet_rt_handle_clone_plugin_frame(Frame *);
extern void jet_rt_handle_drop_plugin_frame(Frame *);
extern bool jet_rt_handle_eq_plugin_frame(Frame *,Frame *);
extern Text *jet_rt_record_get_plugin_frame_function(Frame *);
extern Text *jet_rt_record_get_plugin_frame_component(Frame *);
extern int64_t jet_rt_record_get_plugin_frame_offset(Frame *);
extern Frames *jet_rt_record_seq_list_rec_plugin_frame_new(int64_t);
extern int64_t jet_rt_record_seq_list_rec_plugin_frame_len(Frames *);
extern Frame *jet_rt_record_seq_list_rec_plugin_frame_get(Frames *,int64_t);
extern void jet_rt_record_seq_list_rec_plugin_frame_push(Frames *,Frame *);
extern Fault *jet_rt_record_new_plugin_fault(Text *,Text *,Frames *);
extern Fault *jet_rt_handle_clone_plugin_fault(Fault *);
extern void jet_rt_handle_drop_plugin_fault(Fault *);
extern bool jet_rt_handle_eq_plugin_fault(Fault *,Fault *);
extern Text *jet_rt_record_get_plugin_fault_export(Fault *);
extern Text *jet_rt_record_get_plugin_fault_message(Fault *);
extern Frames *jet_rt_record_get_plugin_fault_frames(Fault *);
extern int64_t *jet_rt_handle_clone_plugin_limit(int64_t *);
extern void jet_rt_handle_drop_plugin_limit(int64_t *);
extern bool jet_rt_handle_eq_plugin_limit(int64_t *,int64_t *);
extern Span *jet_rt_record_new_log_span(int64_t,Text *);
extern Span *jet_rt_handle_clone_log_span(Span *);
extern void jet_rt_handle_drop_log_span(Span *);
extern bool jet_rt_handle_eq_log_span(Span *,Span *);
extern int64_t jet_rt_record_get_log_span_id(Span *);
extern Text *jet_rt_record_get_log_span_name(Span *);
extern Policy *jet_rt_record_new_jet_async_policy(int64_t,int64_t);
extern Policy *jet_rt_handle_clone_jet_async_policy(Policy *);
extern void jet_rt_handle_drop_jet_async_policy(Policy *);
extern int64_t jet_rt_record_get_jet_async_policy_capacity(Policy *);
extern int64_t jet_rt_record_get_jet_async_policy_overflow(Policy *);
extern int64_t *jet_rt_handle_clone_jet_event_overflow(int64_t *);
extern void jet_rt_handle_drop_jet_event_overflow(int64_t *);
extern bool jet_rt_handle_eq_jet_event_overflow(int64_t *,int64_t *);
static struct { void *pointer; int64_t size,align; unsigned frees; } watched[128];
static size_t count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1);
    }
}
// A block watched again before it is freed is another owner of the same
// refcounted text: it stays one entry that must be freed exactly once.
static void watch(void *p,int64_t size,int64_t align) {
    assert(count<128);
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) { assert(!watched[i].frees && size==watched[i].size && align==watched[i].align); return; }
    watched[count].pointer=p;watched[count].size=size;watched[count].align=align;watched[count++].frees=0;
}
static void freed(unsigned before) {
    assert(intercepted>before);for(size_t i=0;i<count;++i) assert(watched[i].frees==1);count=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,104,8); if((const char *)t->data!=(const char *)t+32) watch(t->data,t->capacity,1); }
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(Text *t) { return *(int64_t *)((char *)t-8)&INT64_C(0x3fffffffffffffff); }
static bool text_block(Text *t,void *p) { return p==(char *)t-8 || ((const char *)t->data!=(const char *)t+32 && p==t->data); }
// An owned copy of t whose first byte differs: clones share t's bytes, so a
// differing field is a different text, never a write through a shared one.
static Text *flipped(Text *t) {
    char bytes[64]; assert(t->length>0 && t->length<=64);
    memcpy(bytes,t->data,(size_t)t->length); bytes[0]^=1;
    return jet_rt_string_from_static(bytes,t->length);
}
static void watch_frame_fields(Frame *f) { watch_text(f->function);watch_text(f->component); }
static void watch_fault(Fault *f) {
    watch(f,24,8);watch_text(f->export_name);watch_text(f->message);watch(f->frames,24,8);
    if(f->frames->capacity) watch(f->frames->data,f->frames->capacity*24,8);
    for(int64_t i=0;i<f->frames->length;++i) watch_frame_fields(f->frames->data+i);
}
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void expect(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s));assert(memcmp(t->data,s,(size_t)t->length)==0);jet_rt_string_drop(t); }
static void integer(int64_t x,int64_t value) { assert(jet_int_owned_to_i64(x)==value);jet_rt_int_drop(x); }
static Frame *frame(int64_t offset) {
    Text *f=text("function"),*c=text("component");int64_t i=jet_int_owned_from_i64(offset);
    Frame *result=jet_rt_record_new_plugin_frame(f,c,i);assert(result->function==f && owners(f)==2 && result->component==c && owners(c)==2 && result->offset==offset);
    jet_rt_string_drop(f);jet_rt_string_drop(c);jet_rt_int_drop(i);return result;
}
static void frame_cases(void) {
    Frame *a=frame(INT64_MIN),*b=jet_rt_handle_clone_plugin_frame(a);
    // Records are reallocated; their texts are shared by owner count.
    assert(a!=b && a->function==b->function && owners(a->function)==2 && a->component==b->component && owners(a->component)==2);
    Text *f=jet_rt_record_get_plugin_frame_function(a),*c=jet_rt_record_get_plugin_frame_component(a);
    assert(f==a->function && owners(f)==3 && c==a->component && owners(c)==3);
    expect(f,"function");expect(c,"component");integer(jet_rt_record_get_plugin_frame_offset(a),INT64_MIN);
    assert(owners(a->function)==2 && owners(a->component)==2);
    assert(jet_rt_handle_eq_plugin_frame(a,a) && jet_rt_handle_eq_plugin_frame(a,b));
    Text *kept=b->function,*other=flipped(kept);b->function=other;assert(!jet_rt_handle_eq_plugin_frame(a,b));b->function=kept;jet_rt_string_drop(other);
    kept=b->component;other=flipped(kept);b->component=other;assert(!jet_rt_handle_eq_plugin_frame(a,b));b->component=kept;jet_rt_string_drop(other);
    b->offset=0;assert(!jet_rt_handle_eq_plugin_frame(a,b));
    watch(a,24,8);watch(b,24,8);watch_frame_fields(a);watch_frame_fields(b);
    unsigned before=intercepted;jet_rt_handle_drop_plugin_frame(a);jet_rt_handle_drop_plugin_frame(b);freed(before);jet_rt_handle_drop_plugin_frame(NULL);
}
static void fault_cases(void) {
    for(int64_t n=0;n<=9;n+=9) {
        Frames *seq=jet_rt_record_seq_list_rec_plugin_frame_new(n?1:0);
        for(int64_t i=0;i<n;++i) {
            Frame *f=frame(i==0?INT64_MAX:i);watch(f,24,8);unsigned before=intercepted;
            jet_rt_record_seq_list_rec_plugin_frame_push(seq,f);freed(before); // Shell consumed once, fields moved.
            assert(jet_rt_record_seq_list_rec_plugin_frame_len(seq)==i+1);
        }
        for(int64_t i=0;i<n;++i) assert(jet_rt_record_seq_list_rec_plugin_frame_get(seq,i)==seq->data+i);
        Text *e=text("entry"),*m=text("message");Fault *a=jet_rt_record_new_plugin_fault(e,m,seq),*b=jet_rt_handle_clone_plugin_fault(a);
        assert(a->frames==seq && jet_rt_record_get_plugin_fault_frames(a)==seq && a!=b && a->frames!=b->frames);
        assert(a->export_name==e && a->message==m && b->export_name==a->export_name && b->message==a->message && owners(e)==3 && owners(m)==3);
        jet_rt_string_drop(e);jet_rt_string_drop(m);
        Text *eg=jet_rt_record_get_plugin_fault_export(a),*mg=jet_rt_record_get_plugin_fault_message(a);
        assert(eg==a->export_name && owners(eg)==3 && mg==a->message && owners(mg)==3);
        expect(eg,"entry");expect(mg,"message");assert(jet_rt_handle_eq_plugin_fault(a,a) && jet_rt_handle_eq_plugin_fault(a,b));
        Text *kept=b->export_name,*other=flipped(kept);b->export_name=other;assert(!jet_rt_handle_eq_plugin_fault(a,b));b->export_name=kept;jet_rt_string_drop(other);
        kept=b->message;other=flipped(kept);b->message=other;assert(!jet_rt_handle_eq_plugin_fault(a,b));b->message=kept;jet_rt_string_drop(other);
        if(n) {
            assert(a->frames->data!=b->frames->data);
            for(int64_t i=0;i<n;++i) assert(a->frames->data[i].function==b->frames->data[i].function && owners(a->frames->data[i].function)==2);
            b->frames->data[0].offset=0;assert(!jet_rt_handle_eq_plugin_fault(a,b));b->frames->data[0].offset=INT64_MAX;
            b->frames->length--;assert(!jet_rt_handle_eq_plugin_fault(a,b));b->frames->length++;
            Frame temp=b->frames->data[0];b->frames->data[0]=b->frames->data[1];b->frames->data[1]=temp;assert(!jet_rt_handle_eq_plugin_fault(a,b));
            temp=b->frames->data[0];b->frames->data[0]=b->frames->data[1];b->frames->data[1]=temp;
        }
        watch_fault(a);watch_fault(b);unsigned before=intercepted;jet_rt_handle_drop_plugin_fault(a);jet_rt_handle_drop_plugin_fault(b);freed(before);
    }
    jet_rt_handle_drop_plugin_fault(NULL);
    for(int64_t tag=0;tag<5;++tag) {
        int64_t *a=jet_rt_alloc(8,8);*a=tag;int64_t *b=jet_rt_handle_clone_plugin_limit(a);
        assert(a!=b && *b==tag && jet_rt_handle_eq_plugin_limit(a,b));*b=(tag+1)%5;assert(!jet_rt_handle_eq_plugin_limit(a,b));
        watch(a,8,8);watch(b,8,8);unsigned before=intercepted;jet_rt_handle_drop_plugin_limit(a);jet_rt_handle_drop_plugin_limit(b);freed(before);
    }
    jet_rt_handle_drop_plugin_limit(NULL);
}
static void span_policy_cases(void) {
    const int64_t edges[]={INT64_MIN,0,INT64_MAX};
    for(size_t i=0;i<3;++i) {
        int64_t id=jet_int_owned_from_i64(edges[i]);Text *name=text("span");Span *a=jet_rt_record_new_log_span(id,name),*b=jet_rt_handle_clone_log_span(a);
        jet_rt_int_drop(id);jet_rt_string_drop(name);assert(a!=b && a->id==edges[i] && a->name==b->name && owners(a->name)==2);
        integer(jet_rt_record_get_log_span_id(a),edges[i]);Text *get=jet_rt_record_get_log_span_name(a);assert(get==a->name && owners(get)==3);
        assert(jet_rt_handle_eq_log_span(a,a) && jet_rt_handle_eq_log_span(a,b));b->id^=1;assert(!jet_rt_handle_eq_log_span(a,b));b->id^=1;
        Text *kept=b->name,*other=flipped(kept);b->name=other;assert(!jet_rt_handle_eq_log_span(a,b));b->name=kept;jet_rt_string_drop(other);
        watch(a,16,8);watch(b,16,8);watch_text(a->name);unsigned before=intercepted;jet_rt_handle_drop_log_span(a);jet_rt_handle_drop_log_span(b);
        // The getter's owner keeps the shared name alive past both records.
        assert(owners(get)==1);for(size_t j=0;j<count;++j) assert(watched[j].frees==(text_block(get,watched[j].pointer)?0u:1u));
        expect(get,"span");freed(before);
        for(int64_t tag=0;tag<3;++tag) {
            int64_t capacity=jet_int_owned_from_i64(edges[i]);Policy *p=jet_rt_record_new_jet_async_policy(capacity,tag),*q=jet_rt_handle_clone_jet_async_policy(p);jet_rt_int_drop(capacity);
            assert(p!=q && p->capacity==edges[i] && q->capacity==edges[i] && q->overflow==tag);
            integer(jet_rt_record_get_jet_async_policy_capacity(p),edges[i]);assert(jet_rt_record_get_jet_async_policy_overflow(p)==tag);
            watch(p,16,8);watch(q,16,8);before=intercepted;jet_rt_handle_drop_jet_async_policy(p);jet_rt_handle_drop_jet_async_policy(q);freed(before);
        }
    }
    jet_rt_handle_drop_log_span(NULL);jet_rt_handle_drop_jet_async_policy(NULL);
    for(int64_t tag=0;tag<3;++tag) {
        int64_t *a=jet_rt_alloc(8,8);*a=tag;int64_t *b=jet_rt_handle_clone_jet_event_overflow(a);
        assert(a!=b && *b==tag && jet_rt_handle_eq_jet_event_overflow(a,b));*b=(tag+1)%3;assert(!jet_rt_handle_eq_jet_event_overflow(a,b));
        watch(a,8,8);watch(b,8,8);unsigned before=intercepted;jet_rt_handle_drop_jet_event_overflow(a);jet_rt_handle_drop_jet_event_overflow(b);freed(before);
    }
    jet_rt_handle_drop_jet_event_overflow(NULL);
}
int main(int argc,char **argv) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if(argc>1) {
        if(strcmp(argv[1],"bad-overflow")==0) (void)jet_rt_record_new_jet_async_policy(0,3);
        else if(strcmp(argv[1],"bad-sequence-index")==0) (void)jet_rt_record_seq_list_rec_plugin_frame_get(jet_rt_record_seq_list_rec_plugin_frame_new(0),0);
        // Foreign Vec bounds stop: len 0, unsigned index UINT64_MAX, exit101.
        else if(strcmp(argv[1],"negative-sequence-index")==0) (void)jet_rt_record_seq_list_rec_plugin_frame_get(jet_rt_record_seq_list_rec_plugin_frame_new(0),-1);
        else return 2;
        assert(!"stop returned");
    }
    frame_cases();fault_cases();span_policy_cases();puts("plugin-policy witness OK");return 0;
}
