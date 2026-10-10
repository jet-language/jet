// Real Jet-object client: link with -Wl,--wrap=jet_rt_free; no provider mocks.
// HTTPMessage.rs195-270 supplies semantic values, not native layout.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t word; } Word;
_Static_assert(sizeof(Text)==24 && offsetof(Text,capacity)==16,"E4 text24");
_Static_assert(sizeof(Word)==8,"EncodingABI word8");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
#define DECLARE_TEXT(T) \
extern Text *jet_rt_handle_clone_##T(Text *); \
extern void jet_rt_handle_drop_##T(Text *); \
extern bool jet_rt_handle_eq_##T(Text *,Text *); \
extern Text *jet_rt_handle_show_##T(Text *);
DECLARE_TEXT(JetHTTPMethod)
DECLARE_TEXT(JetHTTPHeaderName)
DECLARE_TEXT(JetHTTPHeaderValue)
#define DECLARE_WORD(T) \
extern Word *jet_rt_handle_clone_##T(Word *); \
extern void jet_rt_handle_drop_##T(Word *); \
extern bool jet_rt_handle_eq_##T(Word *,Word *); \
extern Text *jet_rt_handle_show_##T(Word *);
DECLARE_WORD(JetHTTPStatus)
DECLARE_WORD(JetHTTPVersion)
static struct { void *p; int64_t n,a; unsigned freed; } watched[16];
static size_t count;
void __wrap_jet_rt_free(void *p,int64_t n,int64_t a) {
    for(size_t i=0;i<count;i++) if(watched[i].p==p) {
        assert(n==watched[i].n && a==watched[i].a);
        assert(++watched[i].freed==1);
    }
    __real_jet_rt_free(p,n,a);
}
static void watch(void *p,int64_t n,int64_t a) {
    assert(p && count<16);
    watched[count].p=p; watched[count].n=n; watched[count].a=a;
    watched[count++].freed=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void all_freed(void) { for(size_t i=0;i<count;i++) assert(watched[i].freed==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(!memcmp(t->data,s,(size_t)t->length)); }
static void text_case(const char *s,Text *(*clone)(Text *),void (*drop)(Text *),bool (*eq)(Text *,Text *),Text *(*show)(Text *)) {
    Text *a=text(s),*b=clone(a),*c=show(a),*d=text("different");
    assert(a!=b && a!=c && b!=c); text_is(b,s); text_is(c,s);
    if(a->length) assert(a->data!=b->data && a->data!=c->data);
    assert(eq(a,b) && eq(b,a) && !eq(a,d) && !eq(d,a));
    watch_text(a); watch_text(b); watch_text(c); watch_text(d);
    drop(a); text_is(b,s); text_is(c,s); drop(b); jet_rt_string_drop(c); drop(d); drop(NULL); all_freed();
}
static void word_case(int64_t n,const char *s,Word *(*clone)(Word *),void (*drop)(Word *),bool (*eq)(Word *,Word *),Text *(*show)(Word *)) {
    Word *a=jet_rt_alloc(8,8); a->word=n;
    Word *b=clone(a),different={n+1};
    assert(a!=b && b->word==n && eq(a,b) && eq(b,a) && !eq(a,&different));
    Text *render=show(a); text_is(render,s);
    watch(a,8,8); watch(b,8,8); watch_text(render);
    drop(a); assert(b->word==n); drop(b); jet_rt_string_drop(render); drop(NULL); all_freed();
}
static void invalid_version(void) {
    int fd[2]; assert(pipe(fd)==0); pid_t pid=fork(); assert(pid>=0);
    if(!pid) {
        close(fd[0]); assert(dup2(fd[1],STDERR_FILENO)>=0); close(fd[1]);
        Word invalid={3}; (void)jet_rt_handle_show_jet_http_version(&invalid); _exit(99);
    }
    close(fd[1]); char output[256]; size_t used=0; ssize_t n;
    while((n=read(fd[0],output+used,sizeof(output)-1-used))>0) { used+=(size_t)n; assert(used<sizeof(output)-1); }
    close(fd[0]); output[used]=0; int status; assert(waitpid(pid,&status,0)==pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status)==101);
    assert(strstr(output,"unreachable")!=NULL);
}
#define TEXT_CASE(T,S) text_case(S,jet_rt_handle_clone_##T,jet_rt_handle_drop_##T,jet_rt_handle_eq_##T,jet_rt_handle_show_##T)
#define WORD_CASE(T,N,S) word_case(N,S,jet_rt_handle_clone_##T,jet_rt_handle_drop_##T,jet_rt_handle_eq_##T,jet_rt_handle_show_##T)
int main(void) {
    jet_rt_alloc_init();
    TEXT_CASE(JetHTTPMethod,"GET"); TEXT_CASE(JetHTTPMethod,"PURGE");
    TEXT_CASE(JetHTTPHeaderName,"X-Mixed-Case");
    TEXT_CASE(JetHTTPHeaderValue,""); TEXT_CASE(JetHTTPHeaderValue,"caf\xc3\xa9\tvalue");
    WORD_CASE(JetHTTPStatus,100,"100"); WORD_CASE(JetHTTPStatus,599,"599");
    WORD_CASE(JetHTTPVersion,0,"HTTP/1.0"); WORD_CASE(JetHTTPVersion,1,"HTTP/1.1"); WORD_CASE(JetHTTPVersion,2,"HTTP/2");
    invalid_version();
    puts("http primitives: clone/drop/eq/show + invalid tag ok");
    return 0;
}
