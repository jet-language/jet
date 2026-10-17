// Real Jet-object client, -Wl,--wrap=jet_rt_free. No providers are mocked.
// Golden renders: HTTPMessage.rs272-325, source ordinals65-123.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t tag,payload; } Error;
typedef struct { int64_t tag; } Operation;
_Static_assert(sizeof(Error)==16 && offsetof(Error,payload)==8,"HTTP Error16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Error *jet_rt_handle_clone_jet_http_error(Error *);
extern void jet_rt_handle_drop_jet_http_error(Error *);
extern bool jet_rt_handle_eq_jet_http_error(Error *,Error *);
extern Text *jet_rt_handle_show_jet_http_error(Error *);
extern Text *jet_rt_handle_display_jet_http_error(Error *);
extern Text *jet_rt_handle_debug_jet_http_error(Error *);
extern Operation *jet_rt_handle_clone_jet_http_operation(Operation *);
extern void jet_rt_handle_drop_jet_http_operation(Operation *);
extern bool jet_rt_handle_eq_jet_http_operation(Operation *,Operation *);
static struct { void *p; int64_t n,a; unsigned freed; } watched[20];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) assert(n==watched[i].n && a==watched[i].a && ++watched[i].freed==1);
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<20); watched[count].p=p; watched[count].n=n; watched[count].a=a; watched[count++].freed=0;
}
static void watch_text(Text *t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void watch_error(Error *e) { watch(e,16,8); if(e->tag>=9 && e->tag<=19) watch_text((Text *)(uintptr_t)e->payload); }
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static Error *error(int64_t tag,int64_t payload,const char *s) {
    Error *e=jet_rt_alloc(16,8); e->tag=tag; e->payload=(tag>=9 && tag<=19)?(int64_t)(uintptr_t)text(s):payload; return e;
}
static void renders(Error *e,const char *golden) {
    Text *a=jet_rt_handle_show_jet_http_error(e),*b=jet_rt_handle_display_jet_http_error(e),*c=jet_rt_handle_debug_jet_http_error(e);
    assert(a!=b && b!=c && a!=c); text_is(a,golden); text_is(b,golden); text_is(c,golden);
    jet_rt_string_drop(a); jet_rt_string_drop(b); jet_rt_string_drop(c);
}
static const char *goldens[]={
    "invalid HTTP method","invalid HTTP URL","invalid HTTP header","invalid HTTP status",
    "HTTP body was already consumed","invalid HTTP framing","unsupported HTTP body encoding","HTTP operation cancelled",
    "HTTP body exceeds 1048576-byte limit","HTTP name resolution failed for payload","HTTP connection failed for payload",
    "HTTP TLS failed during payload","HTTP timeout during payload","HTTP proxy failed during payload",
    "HTTP redirect failed: payload","unsupported HTTP protocol payload","HTTP I/O failed during payload","payload",
    "HTTP resource unavailable: payload","internal HTTP failure; incident payload",
    "HTTP client connect is unavailable on this build target"
};
static void ownership(void) {
    for(int64_t i=0;i<21;i++) {
        Error *a=error(i,i==8?1048576:0,"payload"),*b=jet_rt_handle_clone_jet_http_error(a),*different=error(i,i==8?7:1,"different");
        assert(a!=b && jet_rt_handle_eq_jet_http_error(a,b) && jet_rt_handle_eq_jet_http_error(b,a));
        assert(jet_rt_handle_eq_jet_http_error(a,different)==(i<8));
        if(i>=9 && i<=19) assert(a->payload!=b->payload);
        Error other={i==20?0:i+1,0}; assert(!jet_rt_handle_eq_jet_http_error(a,&other));
        renders(a,goldens[i]); renders(b,goldens[i]); watch_error(a); watch_error(b); watch_error(different);
        jet_rt_handle_drop_jet_http_error(a);
        assert(b->tag==i); if(i>=9 && i<=19) text_is((Text *)(uintptr_t)b->payload,"payload");
        jet_rt_handle_drop_jet_http_error(b); jet_rt_handle_drop_jet_http_error(different); all_freed();
    }
    Error *e=error(8,INT64_MIN,NULL); renders(e,"HTTP body exceeds -9223372036854775808-byte limit"); jet_rt_handle_drop_jet_http_error(e);
    e=error(17,0,""); renders(e,""); jet_rt_handle_drop_jet_http_error(e);
    e=error(17,0,"caf\xc3\xa9\n\"\\"); renders(e,"caf\xc3\xa9\n\"\\"); jet_rt_handle_drop_jet_http_error(e);
    e=error(20,1,NULL); renders(e,"HTTP server bind is unavailable on this build target"); jet_rt_handle_drop_jet_http_error(e);
    e=error(20,2,NULL); renders(e,"HTTP serve listener is unavailable on this build target"); jet_rt_handle_drop_jet_http_error(e);
    jet_rt_handle_drop_jet_http_error(NULL);
}
static void invalid(int mode,int64_t tag,int64_t payload) {
    int fd[2]; assert(pipe(fd)==0); pid_t pid=fork(); assert(pid>=0);
    if(!pid) {
        close(fd[0]); assert(dup2(fd[1],STDERR_FILENO)>=0); close(fd[1]);
        Error bad={tag,payload},good={0,0};
        switch(mode) {
        case 0: (void)jet_rt_handle_clone_jet_http_error(&bad); break;
        case 1: jet_rt_handle_drop_jet_http_error(&bad); break;
        case 2: (void)jet_rt_handle_eq_jet_http_error(&bad,&good); break;
        case 3: (void)jet_rt_handle_show_jet_http_error(&bad); break;
        case 4: (void)jet_rt_handle_display_jet_http_error(&bad); break;
        default: (void)jet_rt_handle_debug_jet_http_error(&bad); break;
        }
        _exit(99);
    }
    close(fd[1]); char out[1024]; size_t used=0; ssize_t n;
    while((n=read(fd[0],out+used,sizeof(out)-1-used))>0) { used+=(size_t)n; assert(used<sizeof(out)-1); }
    close(fd[0]); out[used]=0; int status; assert(waitpid(pid,&status,0)==pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status)==70);
    assert(strstr(out,"record variant index out of range") && strstr(out,"<core.prelude>"));
}
int main(void) {
    jet_rt_alloc_init();
    for(int64_t i=0;i<3;i++) {
        Operation *a=jet_rt_alloc(8,8); a->tag=i; Operation *b=jet_rt_handle_clone_jet_http_operation(a),other={i+1};
        assert(a!=b && b->tag==i && jet_rt_handle_eq_jet_http_operation(a,b) && !jet_rt_handle_eq_jet_http_operation(a,&other));
        watch(a,8,8); watch(b,8,8); jet_rt_handle_drop_jet_http_operation(a); jet_rt_handle_drop_jet_http_operation(b); all_freed();
    }
    jet_rt_handle_drop_jet_http_operation(NULL); ownership();
    for(int mode=0;mode<6;mode++) { invalid(mode,-1,0); invalid(mode,21,0); }
    for(int mode=3;mode<6;mode++) { invalid(mode,20,-1); invalid(mode,20,3); }
    puts("http operation/error: ownership + 21 renders + variant stops ok"); return 0;
}
