// Link real Jet runtime only, -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include "PluginErrorGoldens.h"
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *function,*component; int64_t offset; } Frame;
typedef struct { int64_t length,capacity; Frame *data; } Frames;
typedef struct { Text *export_name,*message; Frames *frames; } Fault;
typedef struct { int64_t tag; Fault *fault; int64_t limit; } Error;
_Static_assert(sizeof(Error)==24 && offsetof(Error,fault)==8 && offsetof(Error,limit)==16,"PluginABI Error");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Frame *jet_rt_record_new_plugin_frame(Text *,Text *,int64_t);
extern Fault *jet_rt_record_new_plugin_fault(Text *,Text *,Frames *);
extern Frames *jet_rt_record_seq_list_rec_plugin_frame_new(int64_t);
extern void jet_rt_record_seq_list_rec_plugin_frame_push(Frames *,Frame *);
extern Error *jet_rt_handle_clone_plugin_error(Error *);
extern void jet_rt_handle_drop_plugin_error(Error *);
extern bool jet_rt_handle_eq_plugin_error(Error *,Error *);
extern Text *jet_rt_handle_show_plugin_error(Error *);
extern Text *jet_rt_handle_display_plugin_error(Error *);
extern Text *jet_rt_handle_debug_plugin_error(Error *);
static struct { void *pointer; int64_t size,align; unsigned frees; } watched[64];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) { assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1); }
    __real_jet_rt_free(p,size,align);
}
static void watch(void *p,int64_t size,int64_t align) {
    assert(count<64);for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p;watched[count].size=size;watched[count].align=align;watched[count++].frees=0;
}
static void watch_text(Text *t) { watch(t,24,8);if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_error(Error *e) {
    watch(e,24,8);Fault *f=e->fault;watch(f,24,8);watch_text(f->export_name);watch_text(f->message);
    Frames *s=f->frames;watch(s,24,8);if(s->capacity) watch(s->data,s->capacity*24,8);
    for(int64_t i=0;i<s->length;++i) { watch_text(s->data[i].function);watch_text(s->data[i].component); }
}
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static Error *error(int64_t tag,int64_t limit,int64_t frames) {
    Frames *s=jet_rt_record_seq_list_rec_plugin_frame_new(frames);
    for(int64_t i=0;i<frames;++i) {
        Text *f=text(i==0?"func\\name":"\xf0\x9f\x98\x80"),*c=text(i==0?"com\xc2\xad""ponent":"\xef\xbb\xbf");
        int64_t n=jet_int_owned_from_i64(i==0?INT64_MIN:INT64_MAX);
        Frame *frame=jet_rt_record_new_plugin_frame(f,c,n);jet_rt_int_drop(n);jet_rt_string_drop(f);jet_rt_string_drop(c);
        jet_rt_record_seq_list_rec_plugin_frame_push(s,frame);
    }
    Text *export_name=text("\xcc\x81""export");const char message[]="guest \"error\"\n\0";
    Text *m=jet_rt_string_from_static(message,(int64_t)sizeof(message)-1);
    Fault *fault=jet_rt_record_new_plugin_fault(export_name,m,s);jet_rt_string_drop(export_name);jet_rt_string_drop(m);
    // Valid native carrier construction; never reinterpret or mock a Rust box.
    Error *e=jet_rt_alloc(24,8);e->tag=tag;e->fault=fault;e->limit=limit;return e;
}
static void expect(Text *t,const char *bytes,int64_t length) { assert(t->length==length);assert(memcmp(t->data,bytes,(size_t)length)==0);jet_rt_string_drop(t); }
int main(void) {
    jet_rt_alloc_init();
    for(size_t i=0;i<sizeof(plugin_goldens)/sizeof(plugin_goldens[0]);++i) {
        Error *a=error(plugin_goldens[i].tag,plugin_goldens[i].limit,plugin_goldens[i].frames);
        if(a->tag!=2) a->limit=99; // Inactive word must never affect equality/debug.
        Error *b=jet_rt_handle_clone_plugin_error(a);
        assert(a!=b && a->fault!=b->fault && a->fault->export_name!=b->fault->export_name && a->fault->message!=b->fault->message);
        assert(a->fault->export_name->data!=b->fault->export_name->data && a->fault->message->data!=b->fault->message->data && a->fault->frames!=b->fault->frames);
        assert(b->limit==(a->tag==2?a->limit:0));
        if(a->fault->frames->length) {
            assert(a->fault->frames->data!=b->fault->frames->data);
            for(int64_t j=0;j<a->fault->frames->length;++j) {
                assert(a->fault->frames->data[j].function!=b->fault->frames->data[j].function && a->fault->frames->data[j].component!=b->fault->frames->data[j].component);
                assert(a->fault->frames->data[j].function->data!=b->fault->frames->data[j].function->data && a->fault->frames->data[j].component->data!=b->fault->frames->data[j].component->data);
            }
        }
        // All 4 variants, all 5 budget limits, empty/two-frame vectors: actual
        // pinned rustc derives (PluginWire.rs:28-50) and Display(:65-68).
        expect(jet_rt_handle_debug_plugin_error(a),plugin_goldens[i].debug,plugin_goldens[i].debug_length);
        Text *show=jet_rt_handle_show_plugin_error(a),*display=jet_rt_handle_display_plugin_error(a);
        assert(show!=a->fault->message && display!=a->fault->message && show!=display);
        assert(show->data!=a->fault->message->data && display->data!=a->fault->message->data);
        expect(show,plugin_goldens[i].display,plugin_goldens[i].display_length);expect(display,plugin_goldens[i].display,plugin_goldens[i].display_length);
        assert(jet_rt_handle_eq_plugin_error(a,a) && jet_rt_handle_eq_plugin_error(a,b));
        b->tag=(a->tag+1)%4;assert(!jet_rt_handle_eq_plugin_error(a,b));b->tag=a->tag;
        b->limit=(a->limit+1)%5;assert(jet_rt_handle_eq_plugin_error(a,b)==(a->tag!=2));b->limit=a->tag==2?a->limit:0;
        b->fault->message->data[0]='X';assert(!jet_rt_handle_eq_plugin_error(a,b));b->fault->message->data[0]='g';
        b->fault->export_name->data[0]='X';assert(!jet_rt_handle_eq_plugin_error(a,b));b->fault->export_name->data[0]=0xcc;
        if(b->fault->frames->length) { b->fault->frames->data[0].offset=0;assert(!jet_rt_handle_eq_plugin_error(a,b));b->fault->frames->data[0].offset=INT64_MIN; }
        Text *survivor=jet_rt_handle_display_plugin_error(a);
        watch_error(a);watch_error(b);unsigned before=intercepted;jet_rt_handle_drop_plugin_error(a);jet_rt_handle_drop_plugin_error(b);
        assert(intercepted>before);for(size_t j=0;j<count;++j) assert(watched[j].frees==1);count=0;
        expect(survivor,plugin_goldens[i].display,plugin_goldens[i].display_length);
    }
    jet_rt_handle_drop_plugin_error(NULL);puts("plugin-error witness OK (16 pinned Rust vectors)");return 0;
}
