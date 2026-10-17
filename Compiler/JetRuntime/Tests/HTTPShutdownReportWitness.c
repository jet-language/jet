// Real Jet-object client: -Wl,--wrap=jet_rt_free; no provider mocks.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
typedef struct { int64_t accepted,overloaded,completed,cancelled; } Report;
_Static_assert(sizeof(Report)==32 && offsetof(Report,cancelled)==24,"ShutdownReport32");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Report *jet_rt_record_new_jet_http_shutdown_report(int64_t,int64_t,int64_t,int64_t);
extern Report *jet_rt_handle_clone_jet_http_shutdown_report(Report *);
extern void jet_rt_handle_drop_jet_http_shutdown_report(Report *);
extern int64_t jet_rt_record_get_jet_http_shutdown_report_user_accepted(Report *);
extern int64_t jet_rt_record_get_jet_http_shutdown_report_user_overloaded(Report *);
extern int64_t jet_rt_record_get_jet_http_shutdown_report_user_completed(Report *);
extern int64_t jet_rt_record_get_jet_http_shutdown_report_user_cancelled(Report *);
static Report *original,*copy;
static unsigned original_frees,copy_frees;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    if(p==original) { assert(n==32 && a==8 && ++original_frees==1); }
    if(p==copy) { assert(n==32 && a==8 && ++copy_frees==1); }
    __real_jet_rt_free(p,n,a);
}
static void overflow(int field) {
    int fd[2]; assert(pipe(fd)==0); pid_t pid=fork(); assert(pid>=0);
    if(!pid) {
        close(fd[0]); assert(dup2(fd[1],STDERR_FILENO)>=0); close(fd[1]);
        int64_t max=jet_int_owned_from_i64(INT64_MAX),huge=jet_int_add(max,1),args[]={0,0,0,0}; args[field]=huge;
        (void)jet_rt_record_new_jet_http_shutdown_report(args[0],args[1],args[2],args[3]); _exit(99);
    }
    close(fd[1]); char out[1024]; size_t used=0; ssize_t n;
    while((n=read(fd[0],out+used,sizeof(out)-1-used))>0) { used+=(size_t)n; assert(used<sizeof(out)-1); }
    close(fd[0]); out[used]=0; int status; assert(waitpid(pid,&status,0)==pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status)==70);
    assert(strstr(out,"native index range")!=NULL);
}
int main(void) {
    jet_rt_alloc_init(); int64_t raw[]={INT64_MAX,INT64_MIN,7,0},inputs[4];
    for(int i=0;i<4;i++) inputs[i]=jet_int_owned_from_i64(raw[i]);
    original=jet_rt_record_new_jet_http_shutdown_report(inputs[0],inputs[1],inputs[2],inputs[3]);
    for(int i=0;i<4;i++) { assert(jet_int_owned_to_i64(inputs[i])==raw[i]); jet_rt_int_drop(inputs[i]); }
    copy=jet_rt_handle_clone_jet_http_shutdown_report(original);
    assert(copy!=original && copy->accepted==raw[0] && copy->overloaded==raw[1] && copy->completed==raw[2] && copy->cancelled==raw[3]);
    int64_t a=jet_rt_record_get_jet_http_shutdown_report_user_accepted(original),b=jet_rt_record_get_jet_http_shutdown_report_user_accepted(original);
    assert(a!=b && jet_int_owned_to_i64(a)==INT64_MAX && jet_int_owned_to_i64(b)==INT64_MAX);
    jet_rt_handle_drop_jet_http_shutdown_report(original); original=NULL;
    assert(jet_int_owned_to_i64(a)==INT64_MAX); jet_rt_int_drop(a); jet_rt_int_drop(b);
    a=jet_rt_record_get_jet_http_shutdown_report_user_overloaded(copy); assert(jet_int_owned_to_i64(a)==INT64_MIN); jet_rt_int_drop(a);
    a=jet_rt_record_get_jet_http_shutdown_report_user_completed(copy); assert(jet_int_owned_to_i64(a)==7); jet_rt_int_drop(a);
    a=jet_rt_record_get_jet_http_shutdown_report_user_cancelled(copy); assert(jet_int_owned_to_i64(a)==0); jet_rt_int_drop(a);
    jet_rt_handle_drop_jet_http_shutdown_report(copy); copy=NULL; jet_rt_handle_drop_jet_http_shutdown_report(NULL);
    assert(original_frees==1 && copy_frees==1);
    for(int field=0;field<4;field++) overflow(field);
    puts("http shutdown report: exact Int borrow/fresh getters + clone/drop + range stops ok"); return 0;
}
