// Real Jet-object client; link --wrap=jet_rt_free. No runtime substitutes.
// Native TextGlueABI layout. Goldens: FieldError.rs10-16, Regex.rs294-307,
// CommonTypes.rs6647-6696 (single message copied verbatim by every render).
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { Text *message; } Message;
typedef struct { Text *path,*reason; } Field;
typedef struct { int64_t insensitive,multiline,dotall; } Flags;
_Static_assert(sizeof(Message)==8 && sizeof(Field)==16,"error layouts");
_Static_assert(sizeof(Flags)==24 && offsetof(Flags,dotall)==16,"flags layout");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
#define MESSAGE_API(name,field) \
extern Message *jet_rt_record_new_##name(Text *); \
extern Message *jet_rt_handle_clone_##name(Message *); \
extern void jet_rt_handle_drop_##name(Message *); \
extern bool jet_rt_handle_eq_##name(Message *,Message *); \
extern Text *jet_rt_record_get_##name##_##field(Message *); \
extern Text *jet_rt_handle_show_##name(Message *); \
extern Text *jet_rt_handle_display_##name(Message *); \
extern Text *jet_rt_handle_debug_##name(Message *)
MESSAGE_API(TextError,message); MESSAGE_API(UTF8Error,message); MESSAGE_API(RangeError,reason);
extern Field *jet_rt_record_new_field_error(Text *,Text *);
extern Field *jet_rt_handle_clone_field_error(Field *);
extern void jet_rt_handle_drop_field_error(Field *);
extern bool jet_rt_handle_eq_field_error(Field *,Field *);
extern Text *jet_rt_record_get_field_error_path(Field *);
extern Text *jet_rt_record_get_field_error_reason(Field *);
extern Text *jet_rt_handle_show_field_error(Field *);
extern Text *jet_rt_handle_display_field_error(Field *);
extern Text *jet_rt_handle_debug_field_error(Field *);
extern Flags *jet_rt_record_new_regex_flags(uint8_t,uint8_t,uint8_t);
extern Flags *jet_rt_handle_clone_regex_flags(Flags *);
extern void jet_rt_handle_drop_regex_flags(Flags *);
extern bool jet_rt_handle_eq_regex_flags(Flags *,Flags *);
extern bool jet_rt_record_get_regex_flags_case_insensitive(Flags *);
extern bool jet_rt_record_get_regex_flags_multiline(Flags *);
extern bool jet_rt_record_get_regex_flags_dotall(Flags *);
extern Text *jet_rt_handle_show_regex_flags(Flags *);
static struct { void *pointer; int64_t bytes,align; unsigned frees; } watched[40];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align); assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<40); for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); }
typedef struct {
    Message *(*new_value)(Text *), *(*clone)(Message *);
    void (*drop)(Message *); bool (*eq)(Message *,Message *);
    Text *(*get)(Message *), *(*show)(Message *), *(*display)(Message *), *(*debug)(Message *);
} MessageAPI;
#define API(name,field) {jet_rt_record_new_##name,jet_rt_handle_clone_##name,jet_rt_handle_drop_##name,jet_rt_handle_eq_##name,jet_rt_record_get_##name##_##field,jet_rt_handle_show_##name,jet_rt_handle_display_##name,jet_rt_handle_debug_##name}
static void messages(MessageAPI api) {
    const char *goldens[]={"","plain","\xcf\x80\n`quoted`"};
    for(size_t i=0;i<3;++i) {
        Text *input=text(goldens[i]); Message *a=api.new_value(input), *b=api.clone(a);
        assert(a!=b && a->message!=input && a->message!=b->message && api.eq(a,b) && api.eq(a,a));
        if(input->length) assert(input->data!=a->message->data && a->message->data!=b->message->data);
        jet_rt_string_drop(input);
        Text *g=api.get(a), *s=api.show(a), *d=api.display(a), *dbg=api.debug(a);
        assert(g!=a->message && g!=b->message && s!=d && s!=dbg && d!=dbg);
        text_is(g,goldens[i]); text_is(s,goldens[i]); text_is(d,goldens[i]); text_is(dbg,goldens[i]);
        if(b->message->length) { b->message->data[0]^=1; assert(!api.eq(a,b)); b->message->data[0]^=1; }
        Text *saved=b->message; b->message=text("other"); assert(!api.eq(a,b)); jet_rt_string_drop(b->message); b->message=saved;
        watch(a,8,8); watch_text(a->message); watch(b,8,8); watch_text(b->message);
        api.drop(a); api.drop(b); done(); text_is(g,goldens[i]);
        watch_text(g); watch_text(s); watch_text(d); watch_text(dbg);
        jet_rt_string_drop(g); jet_rt_string_drop(s); jet_rt_string_drop(d); jet_rt_string_drop(dbg); done();
    }
    api.drop(NULL);
}
static void fields(void) {
    const char *paths[]={"","x.y","items[0]","\xcf\x80`\n"};
    const char *goldens[]={"bad","at `x.y`: bad","at `items[0]`: bad","at `\xcf\x80`\n`: bad"};
    for(size_t i=0;i<4;++i) {
        Text *p=text(paths[i]), *r=text("bad"); Field *a=jet_rt_record_new_field_error(p,r), *b=jet_rt_handle_clone_field_error(a);
        assert(a!=b && a->path!=p && a->reason!=r && a->path!=b->path && a->reason!=b->reason);
        jet_rt_string_drop(p); jet_rt_string_drop(r); assert(jet_rt_handle_eq_field_error(a,b));
        Text *gp=jet_rt_record_get_field_error_path(a), *gr=jet_rt_record_get_field_error_reason(a);
        assert(gp!=a->path && gr!=a->reason);
        Text *s=jet_rt_handle_show_field_error(a), *d=jet_rt_handle_display_field_error(a), *dbg=jet_rt_handle_debug_field_error(a);
        assert(s!=d && s!=dbg && d!=dbg); text_is(s,goldens[i]); text_is(d,goldens[i]); text_is(dbg,goldens[i]);
        Text *old=b->path; b->path=text("changed"); assert(!jet_rt_handle_eq_field_error(a,b)); jet_rt_string_drop(b->path); b->path=old;
        b->reason->data[0]^=1; assert(!jet_rt_handle_eq_field_error(a,b)); b->reason->data[0]^=1;
        watch(a,16,8); watch_text(a->path); watch_text(a->reason); watch(b,16,8); watch_text(b->path); watch_text(b->reason);
        jet_rt_handle_drop_field_error(a); jet_rt_handle_drop_field_error(b); done();
        text_is(gp,paths[i]); text_is(gr,"bad");
        watch_text(gp); watch_text(gr); watch_text(s); watch_text(d); watch_text(dbg);
        jet_rt_string_drop(gp); jet_rt_string_drop(gr); jet_rt_string_drop(s); jet_rt_string_drop(d); jet_rt_string_drop(dbg); done();
    }
    jet_rt_handle_drop_field_error(NULL);
}
static void flags(void) {
    const char *goldens[]={"RegexFlags()","RegexFlags(i)","RegexFlags(m)","RegexFlags(im)","RegexFlags(s)","RegexFlags(is)","RegexFlags(ms)","RegexFlags(ims)"};
    for(unsigned mask=0;mask<8;++mask) {
        Flags *a=jet_rt_record_new_regex_flags((mask&1)?255:0,(mask&2)?128:0,(mask&4)?2:0), *b=jet_rt_handle_clone_regex_flags(a);
        assert(a!=b && jet_rt_handle_eq_regex_flags(a,b));
        assert(a->insensitive==((mask&1)!=0) && a->multiline==((mask&2)!=0) && a->dotall==((mask&4)!=0));
        assert(jet_rt_record_get_regex_flags_case_insensitive(a)==((mask&1)!=0));
        assert(jet_rt_record_get_regex_flags_multiline(a)==((mask&2)!=0));
        assert(jet_rt_record_get_regex_flags_dotall(a)==((mask&4)!=0));
        int64_t *slots[]={&b->insensitive,&b->multiline,&b->dotall};
        for(size_t i=0;i<3;++i) {
            *slots[i]^=1; assert(!jet_rt_handle_eq_regex_flags(a,b)); *slots[i]^=1;
            *slots[i]^=INT64_C(0x100); assert(jet_rt_handle_eq_regex_flags(a,b)); *slots[i]^=INT64_C(0x100);
        }
        Text *shown=jet_rt_handle_show_regex_flags(a); text_is(shown,goldens[mask]);
        watch(a,24,8); watch(b,24,8); watch_text(shown);
        jet_rt_handle_drop_regex_flags(a); jet_rt_handle_drop_regex_flags(b); jet_rt_string_drop(shown); done();
    }
    jet_rt_handle_drop_regex_flags(NULL);
}
int main(void) {
    jet_rt_alloc_init();
    MessageAPI apis[]={API(TextError,message),API(UTF8Error,message),API(RangeError,reason)};
    for(size_t i=0;i<3;++i) messages(apis[i]);
    fields(); flags(); puts("TextGlue ownership/equality/render OK"); return 0;
}
