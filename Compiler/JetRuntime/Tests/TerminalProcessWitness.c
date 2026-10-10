// Real Jet route and Linux PTY witness; link -Wl,--wrap=jet_rt_free.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

typedef struct { unsigned char *data; int64_t len, capacity; } Text;
typedef struct { int64_t cols, rows; } Size;
typedef struct { void *child; } Session;
typedef struct {
    int64_t kind, operation; Text *resource; int64_t has_code, code;
    Text *cause; int64_t limit;
} IOError;
_Static_assert(sizeof(Size)==16 && sizeof(Session)==8 && sizeof(IOError)==56, "native carriers");
extern void jet_rt_alloc_init(void);
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern void *jet_rt_alloc(int64_t, int64_t);
extern int64_t jet_rt_world_new(void);
extern void jet_rt_world_enter(int64_t, int64_t *);
extern void jet_rt_world_leave(int64_t *);
extern void jet_rt_world_drop(int64_t);
#ifdef JET_PROCESS_JET_FIXTURES
// Jet fixture arguments/results are pointer-sized opaque native handles.
extern void *fixture_terminal_process_child(int64_t);
extern int64_t fixture_terminal_process_refs(void *);
#else
// Test-only mechanical construction of the documented native Child72 owner.
// It owns a genuine kernel PTY/pipe; every operation and release uses the pack.
static void *fixture_terminal_process_child(int64_t master) {
    int64_t *child=jet_rt_alloc(72,8);
    memset(child,0,72);
    child[4]=-1; child[5]=-1; child[6]=-1; child[7]=master; child[8]=1;
    return child;
}
static int64_t fixture_terminal_process_refs(void *child) {
    return ((const int64_t *)child)[8];
}
#endif
extern Session *jet_terminal_session_from_child_raw(void *);
extern void *jet_terminal_session_child_raw(const Session *);
extern Session *jet_rt_handle_clone_terminal_session(const Session *);
extern void jet_rt_handle_drop_terminal_session(Session *);
extern bool jet_rt_handle_eq_terminal_session(const Session *, const Session *);
extern int64_t jet_terminal_session_resize(Session *, const Size *, IOError **);
#ifdef JET_PROCESS_JET_FIXTURES
extern int64_t fixture_terminal_process_world_resize(Session *, const Size *, IOError **);
#else
static int64_t fixture_terminal_process_world_resize(Session *session, const Size *size, IOError **error) {
    int64_t world=jet_rt_world_new(), save[4]={0};
    jet_rt_world_enter(world,save);
    int64_t result=jet_terminal_session_resize(session,size,error);
    jet_rt_world_leave(save);
    jet_rt_world_drop(world);
    return result;
}
#endif
extern void jet_process_drop_raw(void *);
extern void jet_process_close_stream_raw(void *, int64_t, int64_t *);
extern Size *jet_rt_record_new_terminal_size(int64_t, int64_t);
extern void jet_rt_handle_drop_terminal_size(Size *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_handle_drop_io_error(IOError *);
extern int64_t jet_rt_root_begin(int64_t *);
extern void jet_rt_root_leave(int64_t, int64_t *);
extern bool jet_rt_root_publish_rendered(const void *, int64_t, int64_t);
extern int64_t jet_rt_root_take_stop(int64_t, int64_t *);
extern Text *jet_rt_stop_take_message(int64_t);
extern void jet_rt_stop_drop(int64_t);
extern void jet_rt_string_drop(Text *);
static void *watched[3];
static unsigned frees[3];
void __wrap_jet_rt_free(void *p, int64_t bytes, int64_t align) {
    for (unsigned i=0; i<3; ++i) if (watched[i] && p==watched[i]) {
        assert(bytes==(i?8:72) && align==8 && ++frees[i]==1);
        watched[i]=NULL; // Later genuine allocator reuse is a different owner.
    }
    __real_jet_rt_free(p,bytes,align);
}
static void text_eq(const Text *t, const char *s) {
    assert(t && t->len==(int64_t)strlen(s) && !memcmp(t->data,s,strlen(s)));
}
static void error_eq(IOError *e, const char *cause) {
    assert(e && e->kind==7 && e->operation==6 && !e->has_code && !e->code && !e->limit);
    text_eq(e->resource,"process terminal"); text_eq(e->cause,cause);
    jet_rt_handle_drop_io_error(e);
}
// Padding canaries prove that the error out is exactly one machine word.
struct ErrorSlot { uint64_t before; IOError *error; uint64_t after; };
static struct ErrorSlot slot(void) {
    return (struct ErrorSlot){UINT64_C(0x1122334455667788),(IOError *)(uintptr_t)0x1234,UINT64_C(0x8877665544332211)};
}
static void canaries(const struct ErrorSlot *s) {
    assert(s->before==UINT64_C(0x1122334455667788) && s->after==UINT64_C(0x8877665544332211));
}
static void invalid_sizes(Session *s) {
    const Size bad[]={{0,24},{80,0},{-1,24},{80,-1},{65536,24},{80,65536},
        {INT64_MIN,INT64_MAX},{INT64_MAX,INT64_MIN}};
    for (unsigned i=0; i<sizeof bad/sizeof bad[0]; ++i) {
        int64_t cols=jet_int_owned_from_i64(bad[i].cols), rows=jet_int_owned_from_i64(bad[i].rows);
        Size *size=jet_rt_record_new_terminal_size(cols,rows);
        jet_rt_int_drop(cols); jet_rt_int_drop(rows);
        assert(size->cols==bad[i].cols && size->rows==bad[i].rows);
        struct ErrorSlot out=slot();
        assert(jet_terminal_session_resize(s,size,&out.error)==0); canaries(&out);
        char message[180];
        snprintf(message,sizeof message,"terminal size must be between 1 and 65535, got %lldx%lld",
            (long long)bad[i].cols,(long long)bad[i].rows);
        error_eq(out.error,message);
        assert(size->cols==bad[i].cols && size->rows==bad[i].rows); // borrowed, never consumed
        jet_rt_handle_drop_terminal_size(size);
    }
}
static void real_pty(void) {
    int master=posix_openpt(O_RDWR|O_NOCTTY|O_CLOEXEC); assert(master>=0);
    assert(grantpt(master)==0 && unlockpt(master)==0);
    int slave=open(ptsname(master),O_RDWR|O_NOCTTY|O_CLOEXEC); assert(slave>=0);
    void *child=fixture_terminal_process_child(master);
    Session *session=jet_terminal_session_from_child_raw(child), *alias=jet_rt_handle_clone_terminal_session(session);
    watched[0]=child; watched[1]=session; watched[2]=alias;
    assert(session!=alias && fixture_terminal_process_refs(child)==3);
    assert(jet_terminal_session_child_raw(session)==child && jet_rt_handle_eq_terminal_session(session,alias));
    jet_process_drop_raw(child); assert(fixture_terminal_process_refs(child)==2 && !frees[0]);
    const Size good[]={{100,40},{1,1},{65535,65535}};
    for (unsigned i=0; i<3; ++i) {
        struct ErrorSlot out=slot();
        assert(jet_terminal_session_resize(alias,&good[i],&out.error)==1); canaries(&out);
        assert(out.error==(IOError *)(uintptr_t)0x1234); // Unit success has no out payload
        struct winsize observed={0}; assert(ioctl(slave,TIOCGWINSZ,&observed)==0);
        assert(observed.ws_col==good[i].cols && observed.ws_row==good[i].rows);
        assert(observed.ws_xpixel==0 && observed.ws_ypixel==0);
    }
    invalid_sizes(session);
    jet_rt_handle_drop_terminal_session(session);
    assert(frees[1]==1 && !frees[0] && fixture_terminal_process_refs(child)==1 && fcntl(master,F_GETFD)>=0);
    Size good_size={92,31}; struct ErrorSlot out=slot();
    assert(jet_terminal_session_resize(alias,&good_size,&out.error)==1 && out.error==(IOError *)(uintptr_t)0x1234);
    int64_t facts[4]={0}; jet_process_close_stream_raw(child,0,facts); assert(!facts[0]);
    assert(fcntl(master,F_GETFD)==-1 && errno==EBADF);
    out=slot(); assert(jet_terminal_session_resize(alias,&good_size,&out.error)==0); canaries(&out);
    error_eq(out.error,"TIOCSWINSZ: Bad file descriptor (os error 9)");
    invalid_sizes(alias); // source dimension error wins even on a closed descriptor
    jet_rt_handle_drop_terminal_session(alias); assert(frees[0]==1 && frees[2]==1);
    memset(watched,0,sizeof watched); assert(close(slave)==0);
}
static void nonterminal(void) {
    int pipes[2]; assert(pipe2(pipes,O_CLOEXEC)==0);
    void *child=fixture_terminal_process_child(pipes[0]);
    Session *s=jet_terminal_session_from_child_raw(child);
    void *other_child=fixture_terminal_process_child(-1);
    Session *other=jet_terminal_session_from_child_raw(other_child);
    assert(!jet_rt_handle_eq_terminal_session(s,other));
    Size size={80,24}; struct ErrorSlot out=slot();
    assert(jet_terminal_session_resize(s,&size,&out.error)==0); canaries(&out);
    error_eq(out.error,"TIOCSWINSZ: Not a tty (os error 25)");
    jet_process_drop_raw(child); jet_rt_handle_drop_terminal_session(s);
    assert(fcntl(pipes[0],F_GETFD)==-1 && errno==EBADF); assert(close(pipes[1])==0);
    out=slot(); assert(jet_terminal_session_resize(other,&size,&out.error)==0);
    error_eq(out.error,"TIOCSWINSZ: Bad file descriptor (os error 9)");
    jet_process_drop_raw(other_child); jet_rt_handle_drop_terminal_session(other);
}
static void stop_gates(void) {
    int master=posix_openpt(O_RDWR|O_NOCTTY|O_CLOEXEC); assert(master>=0);
    assert(grantpt(master)==0 && unlockpt(master)==0);
    void *child=fixture_terminal_process_child(master);
    Session *s=jet_terminal_session_from_child_raw(child); Size size={120,50};
    int64_t save[4], root=jet_rt_root_begin(save); assert(root);
    struct ErrorSlot out=slot();
    assert(fixture_terminal_process_world_resize(s,&size,&out.error)==0);
    canaries(&out); assert(out.error==(IOError *)(uintptr_t)0x1234);
    struct winsize observed={0}; assert(ioctl(master,TIOCGWINSZ,&observed)==0);
    assert(observed.ws_col==0 && observed.ws_row==0);
    int64_t code=0, stop=jet_rt_root_take_stop(root,&code); assert(stop && code==70);
    Text *message=jet_rt_stop_take_message(stop); assert(message && message->len>0);
    // Preserve the actual uncontrolled-world diagnostic; don't turn it into IOError.
    assert(memmem(message->data,(size_t)message->len,"deterministic world",19));
    jet_rt_string_drop(message); jet_rt_stop_drop(stop); jet_rt_root_leave(root,save);
    root=jet_rt_root_begin(save); assert(root);
    assert(jet_rt_root_publish_rendered("pending",7,73)); out=slot();
    assert(jet_terminal_session_resize(s,&size,&out.error)==0);
    canaries(&out); assert(out.error==(IOError *)(uintptr_t)0x1234);
    assert(ioctl(master,TIOCGWINSZ,&observed)==0 && observed.ws_col==0 && observed.ws_row==0);
    stop=jet_rt_root_take_stop(root,&code); assert(stop && code==73);
    jet_rt_stop_drop(stop); jet_rt_root_leave(root,save);
    jet_process_drop_raw(child); jet_rt_handle_drop_terminal_session(s);
}
int main(int argc, char **argv, char **envp) {
    jet_rt_alloc_init(); jet_rt_host_arguments(argc,argv,envp);
    real_pty(); nonterminal(); stop_gates();
    puts("terminal process resize: real PTY, dimensions, errors, ownership, world and stop gates");
    return 0;
}
