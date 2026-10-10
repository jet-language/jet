// Real dynamic TLS, thread teardown, taken carries and entry process edges.
#include <assert.h>
#include <stdbool.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
typedef intptr_t W;
typedef struct { char *data; W length, capacity; } Text;
typedef struct { W length, capacity; void *data; } List;
typedef struct { Text *function, *file; W line,column; Text *note; W count; } Hop;
typedef struct { Hop *origin; List hops; } Journey;
typedef struct Err { Text *message,*code; struct Err *cause; Text *identity; List context,conversions; void *details; } Err;
extern char **environ;
extern void jet_rt_host_arguments(W,W,W);
extern Text *jet_rt_string_from_static(const char *,W);
extern void jet_rt_string_drop(Text *);
extern Err *jet_err_from_message(Text *);
extern void jet_rt_handle_drop_jet_err(Err *);
extern void jet_journey_origin(Text *,W,W,Text *);
extern void jet_journey_frame_text(Text *,W,W,Text *,Text *);
extern void jet_journey_reset(void);
extern Journey *jet_journey_take_hops(void);
extern void jet_journey_adopt(Journey *);
extern void jet_rt_native_journey_drop(Journey *);
extern Text *jet_journey_report(Text *);
extern Text *jet_error_report(Err *);
extern void jet_report_use_wire(void);
extern void jet_entry_error_exit_jet(Err *);
extern void jet_rt_entry_error_exit_err(Err *);
extern void jet_rt_entry_error_exit(Text *);
extern void jet_rt_thread_bootstrap(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_program_allocator_configure(W,W);
extern void jet_rt_program_allocator_restore(W,W);
extern W jet_rt_program_allocator_fact(W);
extern W jet_rt_logical_env_set(Text *,Text *);
extern W jet_rt_logical_env_unset(Text *);
extern bool fixture_journey_auto_color_raw(bool);
extern W fixture_journey_columns_raw(bool);
static Text *text(const char *s) { return jet_rt_string_from_static(s,(W)strlen(s)); }
static void equal(Text *value,const char *expected) {
    assert(value->length==(W)strlen(expected));
    assert(!memcmp(value->data,expected,(size_t)value->length));
    jet_rt_string_drop(value);
}
static void plant(const char *path,const char *name,W line) {
    Text *file=text(path),*function=text(name);
    jet_journey_origin(file,line,3,function);
    jet_rt_string_drop(file); jet_rt_string_drop(function);
}
static void *worker(void *keep) {
    jet_rt_thread_bootstrap();
    assert(!jet_journey_take_hops()); // parent origin is never inherited
    plant("worker.jet","worker",7);
    if (!keep) { Journey *held=jet_journey_take_hops(); assert(held); jet_rt_native_journey_drop(held); }
    // keep=true leaves the sole live Journey to the registered destructor.
    jet_rt_thread_detach();
    return NULL;
}
static void join_worker(void *keep) {
    pthread_t thread;
    assert(!pthread_create(&thread,NULL,worker,keep));
    assert(!pthread_join(thread,NULL));
}
static void entry(unsigned kind,int wire,const char *expected) {
    int pipes[2]; assert(!pipe(pipes));
    pid_t pid=fork(); assert(pid>=0);
    if (!pid) {
        assert(!dup2(pipes[1],STDERR_FILENO)); close(pipes[0]); close(pipes[1]);
        jet_journey_reset();
        if (wire) jet_report_use_wire();
        Text *message=text("failed\n\n");
        if (kind==2) jet_rt_entry_error_exit(message);
        else {
            Err *error=jet_err_from_message(message); jet_rt_string_drop(message);
            if (kind==0) jet_entry_error_exit_jet(error); else jet_rt_entry_error_exit_err(error);
        }
        _exit(99); // a returned entry exit is an implementation failure
    }
    close(pipes[1]);
    char bytes[4096]; size_t size=0; ssize_t n;
    while ((n=read(pipes[0],bytes+size,sizeof(bytes)-size))>0) size+=(size_t)n;
    close(pipes[0]); int status; assert(waitpid(pid,&status,0)==pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status)==1);
    assert(size==strlen(expected) && !memcmp(bytes,expected,size));
}
static void live_environment(void) {
    Text *no=text("NO_COLOR"),*force=text("FORCE_COLOR"),*columns=text("COLUMNS"),*empty=text("");
    jet_rt_logical_env_unset(no);
    assert(!jet_rt_logical_env_set(force,empty));
    assert(fixture_journey_auto_color_raw(false));
    char invalid[]={ (char)0xff }; Text os_bytes={invalid,1,0};
    assert(!jet_rt_logical_env_set(no,&os_bytes));
    assert(!fixture_journey_auto_color_raw(false));
    assert(jet_rt_logical_env_unset(no)==1);
    assert(fixture_journey_auto_color_raw(false));
    assert(!jet_rt_logical_env_set(no,empty));
    assert(!fixture_journey_auto_color_raw(true));
    assert(jet_rt_logical_env_unset(force)==1);
    Text *width=text("  +120  ");
    assert(!jet_rt_logical_env_set(columns,width));
    assert(fixture_journey_columns_raw(true)==120 && !fixture_journey_columns_raw(false));
    jet_rt_string_drop(width); width=text("18446744073709551615");
    assert(!jet_rt_logical_env_set(columns,width));
    assert(fixture_journey_columns_raw(true)==INTPTR_MAX);
    jet_rt_string_drop(width); width=text("18446744073709551616");
    assert(!jet_rt_logical_env_set(columns,width));
    assert(fixture_journey_columns_raw(true)==80);
    jet_rt_string_drop(width); width=text("184467440737095516150");
    assert(!jet_rt_logical_env_set(columns,width));
    assert(fixture_journey_columns_raw(true)==80);
    jet_rt_string_drop(width);
    jet_rt_string_drop(no); jet_rt_string_drop(force); jet_rt_string_drop(columns); jet_rt_string_drop(empty);
}
int main(int argc,char **argv) {
    jet_rt_host_arguments(argc,(W)(uintptr_t)argv,(W)(uintptr_t)environ);
    // Warm persistent registry/table capacity outside the measured interval.
    plant("warm.jet","warm",1); jet_rt_native_journey_drop(jet_journey_take_hops());
    join_worker((void *)1);
    W cap=jet_rt_program_allocator_fact(4),mode=jet_rt_program_allocator_fact(5);
    jet_rt_program_allocator_configure(1,0);
    W baseline=jet_rt_program_allocator_fact(2);
    plant("parent.jet","parent",4);
    join_worker(NULL); join_worker((void *)1);
    Journey *parent=jet_journey_take_hops();
    assert(parent && parent->origin->line==4 && !jet_journey_take_hops());
    plant("discard.jet","discard",1);
    jet_journey_adopt(parent); // TAKES carry and discards handled origin
    Text *root=text("Error: failed\n\n");
    equal(jet_journey_report(root),"Error: failed\n  failed here: parent (parent.jet:4)\n");
    jet_rt_string_drop(root);
    assert(!jet_journey_take_hops());
    plant("handled.jet","handled",2); jet_journey_reset();
    Journey *empty=jet_journey_take_hops(); assert(empty && !empty->origin && !empty->hops.length);
    jet_rt_native_journey_drop(empty);
    assert(jet_rt_program_allocator_fact(2)==baseline);
    jet_rt_program_allocator_restore(mode,cap);
    entry(0,0,"Error: failed\n"); entry(1,0,"Error: failed\n"); entry(2,0,"failed\n");
    entry(2,1,"{\"schema\":\"jet.err/v1\",\"message\":\"failed\",\"code\":null,\"cause\":null}\n");
    live_environment();
    puts("Journey TLS: isolated threads, destructor, take/adopt/reset, ledger balance, entry status1 bytes");
    return 0;
}
