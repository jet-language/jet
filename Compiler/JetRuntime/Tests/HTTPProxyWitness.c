// Link real Jet object with -Wl,--wrap=jet_rt_free. No provider mocks.
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t ordinal; Text *payload; } Proxy;
_Static_assert(sizeof(Proxy)==16 && offsetof(Proxy,payload)==8,"HTTP Proxy16");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Proxy *jet_rt_record_new_jet_http_proxy_from_environment(void);
extern Proxy *jet_rt_record_new_jet_http_proxy_none(void);
extern Proxy *jet_rt_record_new_jet_http_proxy_url(Text *);
extern int64_t jet_rt_record_tag_jet_http_proxy(Proxy *);
extern Text *jet_rt_record_get_jet_http_proxy_url(Proxy *);
extern Proxy *jet_rt_handle_clone_jet_http_proxy(Proxy *);
extern void jet_rt_handle_drop_jet_http_proxy(Proxy *);
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
static void watch_proxy(Proxy *p) { watch(p,16,8); if(p->ordinal==2) watch_text(p->payload); }
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s) && !memcmp(t->data,s,(size_t)t->length)); }
static void url(const char *s) {
    Text *source=text(s); Proxy *p=jet_rt_record_new_jet_http_proxy_url(source),*copy=jet_rt_handle_clone_jet_http_proxy(p);
    Text *getter=jet_rt_record_get_jet_http_proxy_url(p);
    assert(jet_rt_record_tag_jet_http_proxy(p)==2 && p!=copy);
    assert(source!=p->payload && p->payload!=copy->payload && getter!=p->payload && getter!=copy->payload);
    text_is(source,s); text_is(p->payload,s); text_is(copy->payload,s); text_is(getter,s);
    watch_text(source); watch_proxy(p); watch_proxy(copy); watch_text(getter);
    jet_rt_string_drop(source); jet_rt_handle_drop_jet_http_proxy(p);
    text_is(copy->payload,s); text_is(getter,s); jet_rt_handle_drop_jet_http_proxy(copy); jet_rt_string_drop(getter); all_freed();
}
static void wrong_getter(int64_t ordinal) {
    int fd[2]; assert(pipe(fd)==0); pid_t pid=fork(); assert(pid>=0);
    if(!pid) {
        close(fd[0]); assert(dup2(fd[1],STDERR_FILENO)>=0); close(fd[1]);
        Proxy wrong={ordinal,NULL}; (void)jet_rt_record_get_jet_http_proxy_url(&wrong); _exit(99);
    }
    close(fd[1]); char out[1024]; size_t used=0; ssize_t n;
    while((n=read(fd[0],out+used,sizeof(out)-1-used))>0) { used+=(size_t)n; assert(used<sizeof(out)-1); }
    close(fd[0]); out[used]=0; int status; assert(waitpid(pid,&status,0)==pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status)==70 && strstr(out,"record variant index out of range") && strstr(out,"<core.prelude>"));
}
int main(void) {
    jet_rt_alloc_init();
    Proxy *environment=jet_rt_record_new_jet_http_proxy_from_environment(),*none=jet_rt_record_new_jet_http_proxy_none();
    Proxy *a=jet_rt_handle_clone_jet_http_proxy(environment),*b=jet_rt_handle_clone_jet_http_proxy(none);
    assert(jet_rt_record_tag_jet_http_proxy(environment)==0 && jet_rt_record_tag_jet_http_proxy(none)==1 && a!=environment && b!=none);
    assert(a->ordinal==0 && b->ordinal==1 && !a->payload && !b->payload);
    watch_proxy(environment); watch_proxy(none); watch_proxy(a); watch_proxy(b);
    jet_rt_handle_drop_jet_http_proxy(environment); jet_rt_handle_drop_jet_http_proxy(none); jet_rt_handle_drop_jet_http_proxy(a); jet_rt_handle_drop_jet_http_proxy(b); all_freed();
    url("http://proxy.example:8080"); url(""); jet_rt_handle_drop_jet_http_proxy(NULL);
    wrong_getter(0); wrong_getter(1); wrong_getter(-1); wrong_getter(3);
    puts("http proxy: three variants + deep ownership + getter stops ok"); return 0;
}
