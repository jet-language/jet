// Real Jet runtime only; link -Wl,--wrap=jet_rt_free.
// WS_MESSAGE_U8_STRIDE must match composed RT_HOST_U8_STRIDE (canonical 1).
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#ifndef WS_MESSAGE_U8_STRIDE
#define WS_MESSAGE_U8_STRIDE 1
#endif
_Static_assert(WS_MESSAGE_U8_STRIDE==1 || WS_MESSAGE_U8_STRIDE==8,"native U8 stride");
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t length,capacity; unsigned char *data; } Bytes;
typedef struct { int64_t kind,limit; Text *operation; } WsError;
typedef struct { int64_t kind; union { Text *text; Bytes *bytes; int64_t code; } payload; Text *reason; } Message;
typedef struct { int64_t kind; Text *reason; } Failure;
typedef struct { double lanes[4]; } Lane;
_Static_assert(sizeof(WsError)==24 && offsetof(WsError,operation)==16,"WsABI error");
_Static_assert(sizeof(Message)==24 && offsetof(Message,reason)==16,"WsABI message");
_Static_assert(sizeof(Failure)==16 && offsetof(Failure,reason)==8,"TaskFailureABI");
_Static_assert(sizeof(Lane)==32,"SimdLaneABI");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Bytes *jet_rt_bytes_unpack(const void *,int64_t);
#define UNIT_DECL(T) \
extern int64_t *jet_rt_handle_clone_##T(int64_t *); \
extern void jet_rt_handle_drop_##T(int64_t *); \
extern bool jet_rt_handle_eq_##T(int64_t *,int64_t *); \
extern Text *jet_rt_handle_show_##T(int64_t *); \
extern Text *jet_rt_handle_debug_##T(int64_t *); \
extern Text *jet_rt_handle_display_##T(int64_t *);
UNIT_DECL(EnvError)
UNIT_DECL(JetTaskStatus)
extern WsError *jet_rt_handle_clone_jet_ws_error(WsError *);
extern void jet_rt_handle_drop_jet_ws_error(WsError *);
extern bool jet_rt_handle_eq_jet_ws_error(WsError *,WsError *);
extern Text *jet_rt_handle_show_jet_ws_error(WsError *);
extern Text *jet_rt_handle_debug_jet_ws_error(WsError *);
extern Text *jet_rt_handle_display_jet_ws_error(WsError *);
extern Message *jet_rt_handle_clone_jet_ws_message(Message *);
extern void jet_rt_handle_drop_jet_ws_message(Message *);
extern bool jet_rt_handle_eq_jet_ws_message(Message *,Message *);
extern Failure *jet_rt_record_new_jet_task_failure_cancelled(void);
extern Failure *jet_rt_record_new_jet_task_failure_deadline_blown(void);
extern Failure *jet_rt_record_new_jet_task_failure_panicked(Text *);
extern int64_t jet_rt_record_tag_jet_task_failure(Failure *);
extern Text *jet_rt_record_get_jet_task_failure_panicked(Failure *);
extern Failure *jet_rt_handle_clone_jet_task_failure(Failure *);
extern void jet_rt_handle_drop_jet_task_failure(Failure *);
extern bool jet_rt_handle_eq_jet_task_failure(Failure *,Failure *);
extern Text *jet_rt_handle_show_jet_task_failure(Failure *);
extern Text *jet_rt_handle_debug_jet_task_failure(Failure *);
extern Text *jet_rt_handle_display_jet_task_failure(Failure *);
extern Lane *jet_rt_handle_clone_f64x4(Lane *);
extern void jet_rt_handle_drop_f64x4(Lane *);
extern bool jet_rt_handle_eq_f64x4(Lane *,Lane *);
extern Text *jet_rt_handle_show_f64x4(Lane *);
extern Text *jet_rt_handle_debug_f64x4(Lane *);
extern Text *jet_rt_handle_display_f64x4(Lane *);
static struct { void *pointer; int64_t size,align; unsigned frees; } watched[32];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t size,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) { assert(size==watched[i].size && align==watched[i].align);assert(++watched[i].frees==1); }
    __real_jet_rt_free(p,size,align);
}
static void watch(void *p,int64_t size,int64_t align) { assert(count<32);for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);watched[count].pointer=p;watched[count].size=size;watched[count].align=align;watched[count++].frees=0; }
static void freed(unsigned before) { assert(intercepted>before);for(size_t i=0;i<count;++i) assert(watched[i].frees==1);count=0; }
static void watch_text(Text *t) { watch((char *)(t)-8,32,8);if(t->capacity) watch(t->data,t->capacity,1); }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void expect_bytes(Text *t,const char *s,int64_t n) { assert(t->length==n);assert(memcmp(t->data,s,(size_t)n)==0);jet_rt_string_drop(t); }
static void expect(Text *t,const char *s) { expect_bytes(t,s,(int64_t)strlen(s)); }
static void unit_cases(const char *const *goldens,int64_t variants,int64_t *(*clone)(int64_t *),void (*drop)(int64_t *),bool (*eq)(int64_t *,int64_t *),Text *(*show)(int64_t *),Text *(*debug)(int64_t *),Text *(*display)(int64_t *)) {
    for(int64_t kind=0;kind<variants;++kind) {
        int64_t *a=jet_rt_alloc(8,8);*a=kind;int64_t *b=clone(a);assert(a!=b && *b==kind && eq(a,a) && eq(a,b));
        expect(show(a),goldens[kind]);expect(debug(a),goldens[kind]);expect(display(a),goldens[kind]);*b=(kind+1)%variants;assert(!eq(a,b));
        watch(a,8,8);watch(b,8,8);unsigned before=intercepted;drop(a);drop(b);freed(before);
    }
    drop(NULL);
}
static void ws_error_cases(void) {
    // Custom WsClient.rs:22-52, not derive Debug: raw operation, signed limit.
    const char *goldens[]={"websocket URL is invalid","websocket handshake failed","websocket protocol error","websocket message exceeds -9223372036854775808 bytes","websocket I/O failed during read\"\n","websocket timed out","websocket is closed","websocket cancelled","this build target lacks websocket client connect"};
    for(int64_t kind=0;kind<9;++kind) {
        WsError *a=jet_rt_alloc(24,8);a->kind=kind;a->limit=kind==3?INT64_MIN:0;a->operation=kind==4?text("read\"\n"):NULL;
        WsError *b=jet_rt_handle_clone_jet_ws_error(a);assert(a!=b && jet_rt_handle_eq_jet_ws_error(a,a) && jet_rt_handle_eq_jet_ws_error(a,b));
        if(kind==4) assert(a->operation!=b->operation && a->operation->data!=b->operation->data);
        expect(jet_rt_handle_show_jet_ws_error(a),goldens[kind]);expect(jet_rt_handle_debug_jet_ws_error(a),goldens[kind]);expect(jet_rt_handle_display_jet_ws_error(a),goldens[kind]);
        b->kind=(kind+1)%9;assert(!jet_rt_handle_eq_jet_ws_error(a,b));b->kind=kind;
        if(kind==3) { b->limit=INT64_MAX;assert(!jet_rt_handle_eq_jet_ws_error(a,b));expect(jet_rt_handle_show_jet_ws_error(b),"websocket message exceeds 9223372036854775807 bytes"); }
        if(kind==4) { b->operation->data[0]='X';assert(!jet_rt_handle_eq_jet_ws_error(a,b)); }
        watch(a,24,8);watch(b,24,8);if(kind==4) { watch_text(a->operation);watch_text(b->operation); }
        unsigned before=intercepted;jet_rt_handle_drop_jet_ws_error(a);jet_rt_handle_drop_jet_ws_error(b);freed(before);
    }
    jet_rt_handle_drop_jet_ws_error(NULL);
}
static void watch_message(Message *m) {
    watch(m,24,8);
    if(m->kind==0) watch_text(m->payload.text);
    else if(m->kind==1) { Bytes *b=m->payload.bytes;watch(b,24,8);if(b->capacity) watch(b->data,b->capacity*WS_MESSAGE_U8_STRIDE,8); }
    else watch_text(m->reason);
}
static void ws_message_cases(void) {
    for(int64_t kind=0;kind<3;++kind) for(int empty=0;empty<2;++empty) {
        Message *a=jet_rt_alloc(24,8);a->kind=kind;a->reason=NULL;
        if(kind==0) a->payload.text=text(empty?"":"text\"\n");
        else if(kind==1) { const unsigned char data[]={0,128,255};a->payload.bytes=jet_rt_bytes_unpack(data,empty?0:3); }
        else { a->payload.code=INT64_MIN;a->reason=text(empty?"":"close reason"); }
        Message *b=jet_rt_handle_clone_jet_ws_message(a);assert(a!=b && jet_rt_handle_eq_jet_ws_message(a,a) && jet_rt_handle_eq_jet_ws_message(a,b));
        if(kind==0) { assert(a->payload.text!=b->payload.text);if(!empty) { assert(a->payload.text->data!=b->payload.text->data);b->payload.text->data[0]='X';assert(!jet_rt_handle_eq_jet_ws_message(a,b)); } }
        else if(kind==1) {
            assert(a->payload.bytes!=b->payload.bytes);if(!empty) {
                assert(a->payload.bytes->data!=b->payload.bytes->data);
                for(int64_t i=0;i<3;++i) assert(a->payload.bytes->data[i*WS_MESSAGE_U8_STRIDE]==b->payload.bytes->data[i*WS_MESSAGE_U8_STRIDE]);
                b->payload.bytes->data[WS_MESSAGE_U8_STRIDE]=7;assert(!jet_rt_handle_eq_jet_ws_message(a,b));b->payload.bytes->data[WS_MESSAGE_U8_STRIDE]=128;
                b->payload.bytes->length=2;assert(!jet_rt_handle_eq_jet_ws_message(a,b));b->payload.bytes->length=3;
            }
        } else { assert(a->reason!=b->reason);b->payload.code=INT64_MAX;assert(!jet_rt_handle_eq_jet_ws_message(a,b));b->payload.code=INT64_MIN;if(!empty) { assert(a->reason->data!=b->reason->data);b->reason->data[0]='X';assert(!jet_rt_handle_eq_jet_ws_message(a,b)); } }
        b->kind=(kind+1)%3;assert(!jet_rt_handle_eq_jet_ws_message(a,b));b->kind=kind;
        watch_message(a);watch_message(b);unsigned before=intercepted;jet_rt_handle_drop_jet_ws_message(a);jet_rt_handle_drop_jet_ws_message(b);freed(before);
    }
    jet_rt_handle_drop_jet_ws_message(NULL);
}
static void failure_cases(void) {
    const char reason[]="\xcc\x81""panic \"x\"\n\0",raw[]="Panicked(\xcc\x81""panic \"x\"\n\0)";
    for(int64_t kind=0;kind<3;++kind) {
        Text *input=jet_rt_string_from_static(reason,(int64_t)sizeof(reason)-1);
        Failure *a=kind==0?jet_rt_record_new_jet_task_failure_cancelled():kind==1?jet_rt_record_new_jet_task_failure_deadline_blown():jet_rt_record_new_jet_task_failure_panicked(input),*b=jet_rt_handle_clone_jet_task_failure(a);
        assert(a!=b && jet_rt_record_tag_jet_task_failure(a)==kind && jet_rt_handle_eq_jet_task_failure(a,a) && jet_rt_handle_eq_jet_task_failure(a,b));
        Text *getter=NULL;
        if(kind==2) {
            assert(a->reason!=input && b->reason!=a->reason && b->reason->data!=a->reason->data);getter=jet_rt_record_get_jet_task_failure_panicked(a);assert(getter!=a->reason && getter->data!=a->reason->data);
            expect_bytes(jet_rt_handle_show_jet_task_failure(a),raw,(int64_t)sizeof(raw)-1);expect_bytes(jet_rt_handle_display_jet_task_failure(a),raw,(int64_t)sizeof(raw)-1);
            expect(jet_rt_handle_debug_jet_task_failure(a),"Panicked(\"\\u{301}panic \\\"x\\\"\\n\\0\")");b->reason->data[0]='X';assert(!jet_rt_handle_eq_jet_task_failure(a,b));
        } else { const char *s=kind==0?"Cancelled":"DeadlineBlown";expect(jet_rt_handle_show_jet_task_failure(a),s);expect(jet_rt_handle_debug_jet_task_failure(a),s);expect(jet_rt_handle_display_jet_task_failure(a),s); }
        jet_rt_string_drop(input);b->kind=(kind+1)%3;assert(!jet_rt_handle_eq_jet_task_failure(a,b));b->kind=kind;
        watch(a,16,8);watch(b,16,8);if(kind==2) {watch_text(a->reason);watch_text(b->reason);}
        unsigned before=intercepted;jet_rt_handle_drop_jet_task_failure(a);jet_rt_handle_drop_jet_task_failure(b);freed(before);if(getter) expect_bytes(getter,reason,(int64_t)sizeof(reason)-1);
    }
    jet_rt_handle_drop_jet_task_failure(NULL);
}
static Lane *lane(const uint64_t bits[4]) { Lane *result=jet_rt_alloc(32,8);memcpy(result->lanes,bits,32);return result; }
static void lane_cases(void) {
    const uint64_t words[]={0,UINT64_C(0x8000000000000000),UINT64_C(0x7ff8000000000001),UINT64_C(0x7ff0000000000000)};
    Lane *a=lane(words),*b=jet_rt_handle_clone_f64x4(a);assert(a!=b && memcmp(a,b,32)==0 && !jet_rt_handle_eq_f64x4(a,a) && !jet_rt_handle_eq_f64x4(a,b));
    // Exact pinned rustc debug_tuple/array oracle, MathTaskMem.rs:397-407.
    expect(jet_rt_handle_debug_f64x4(a),"F64x4([0.0, -0.0, NaN, inf])");
    watch(a,32,8);watch(b,32,8);unsigned before=intercepted;jet_rt_handle_drop_f64x4(a);jet_rt_handle_drop_f64x4(b);freed(before);
    a=jet_rt_alloc(32,8);a->lanes[0]=1.25;a->lanes[1]=1e-5;a->lanes[2]=1e20;a->lanes[3]=-1e-10;b=jet_rt_handle_clone_f64x4(a);
    assert(jet_rt_handle_eq_f64x4(a,a) && jet_rt_handle_eq_f64x4(a,b));
    expect(jet_rt_handle_show_f64x4(a),"F64x4([1.25, 1e-5, 1e20, -1e-10])");expect(jet_rt_handle_display_f64x4(a),"F64x4([1.25, 1e-5, 1e20, -1e-10])");
    for(size_t i=0;i<4;++i) { double old=b->lanes[i];b->lanes[i]=2.0;assert(!jet_rt_handle_eq_f64x4(a,b));b->lanes[i]=old; }
    a->lanes[0]=0.0;b->lanes[0]=-0.0;assert(jet_rt_handle_eq_f64x4(a,b));
    watch(a,32,8);watch(b,32,8);before=intercepted;jet_rt_handle_drop_f64x4(a);jet_rt_handle_drop_f64x4(b);freed(before);jet_rt_handle_drop_f64x4(NULL);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1) { if(strcmp(argv[1],"wrong-failure-variant")==0) (void)jet_rt_record_get_jet_task_failure_panicked(jet_rt_record_new_jet_task_failure_cancelled());else return 2;assert(!"stop returned"); }
    const char *env[]={"invalid environment variable name","invalid environment variable value","environment contains a name or value that is not valid Unicode"},*status[]={"Running","Paused","CancelRequested"};
    unit_cases(env,3,jet_rt_handle_clone_env_error,jet_rt_handle_drop_env_error,jet_rt_handle_eq_env_error,jet_rt_handle_show_env_error,jet_rt_handle_debug_env_error,jet_rt_handle_display_env_error);
    unit_cases(status,3,jet_rt_handle_clone_jet_task_status,jet_rt_handle_drop_jet_task_status,jet_rt_handle_eq_jet_task_status,jet_rt_handle_show_jet_task_status,jet_rt_handle_debug_jet_task_status,jet_rt_handle_display_jet_task_status);
    ws_error_cases();ws_message_cases();failure_cases();lane_cases();puts("handle-families witness OK");return 0;
}
