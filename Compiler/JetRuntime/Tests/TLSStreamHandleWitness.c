/* Real composed Jet object, --wrap=jet_rt_free; real pipe FD, no mocks. */
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { int64_t len,cap; void *data; } List;
typedef struct { List der,pem,serial,dns; int64_t before,after; Text *subject,*issuer; } Certificate;
typedef struct { Text *name; Certificate leaf; List chain; Text *cipher; int64_t version; } Peer;
typedef struct { int64_t id; void *fd; int64_t read_present,read_ms,write_present,write_ms; void (*callbacks[7])(void); Peer *peer; } Stream;
_Static_assert(sizeof(Peer)==176 && sizeof(Certificate)==128,"TLS snapshot geometry");
_Static_assert(sizeof(Stream)==112 && offsetof(Stream,callbacks)==48 && offsetof(Stream,peer)==104 && _Alignof(Stream)==8,"TLSStream112");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern void *jet_fd_adopt_unbuffered_raw(int64_t,Text *);
extern Text *jet_rt_string_clone(Text *);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_handle_drop_jet_tls_stream(Stream *);
extern Text *jet_rt_handle_show_jet_tls_stream(Stream *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[128];
static size_t count;
static void *fd_owner;
static Peer *peer_owner;
static Stream *stream_owner;
static int fd_number;
static unsigned stage;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    if(ptr==fd_owner) { assert(stage==0 && fcntl(fd_number,F_GETFD)==-1 && errno==EBADF); stage=1; }
    if(ptr==peer_owner) { assert(stage==1); stage=2; }
    if(ptr==stream_owner) { assert(stage==(peer_owner ? 2u : 1u)); stage=3; }
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==watched[i-1].align); watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static Text *text(void) {
    unsigned char bytes[]={'x',0,'y'}; Text *out=jet_rt_string_from_static(bytes,3);
    watch((char *)(out)-8,32,8); watch(out->data,out->cap,1); return out;
}
static Peer *peer(void) {
    Peer *p=jet_rt_alloc(176,8); memset(p,0,176); watch(p,176,8);
    p->name=text(); p->leaf.subject=text(); p->leaf.issuer=text(); p->cipher=text(); return p;
}
static void callback_must_not_run(void) { abort(); }
int main(void) {
    jet_rt_alloc_init(); jet_rt_handle_drop_jet_tls_stream(NULL);
    const int64_t ids[]={INT64_MIN,-1,0,1,INT64_MAX};
    for(size_t i=0;i<sizeof(ids)/sizeof(ids[0]);++i) for(unsigned present=0;present<2;++present) {
        int pipefd[2]; assert(pipe(pipefd)==0); fd_number=pipefd[0];
        fd_owner=jet_fd_adopt_unbuffered_raw(fd_number,NULL); watch(fd_owner,72,8);
        Stream *s=jet_rt_alloc(112,8); memset(s,0,112); watch(s,112,8);
        s->id=ids[i]; s->fd=fd_owner; s->read_present=1; s->read_ms=-1; s->write_present=1; s->write_ms=INT64_MAX;
        for(unsigned k=0;k<7;++k) s->callbacks[k]=callback_must_not_run;
        peer_owner=present ? peer() : NULL; s->peer=peer_owner; stream_owner=s; stage=0;
        Text *shown=jet_rt_handle_show_jet_tls_stream(s); watch((char *)(shown)-8,32,8); watch(shown->data,shown->cap,1);
        char expected[64]; int length=snprintf(expected,sizeof(expected),"TLSStream(%lld)",(long long)ids[i]);
        assert(length>=0 && shown->len==length && !memcmp(shown->data,expected,(size_t)length));
        jet_rt_string_drop(shown); assert(fcntl(fd_number,F_GETFD)!=-1);
        jet_rt_handle_drop_jet_tls_stream(s); assert(stage==3); assert(close(pipefd[1])==0);
        fd_owner=NULL; peer_owner=NULL; stream_owner=NULL;
    }
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("tls-stream: exact Show signed IDs, canonical FD close then optional peer then shell112, borrowed callbacks untouched, null drop OK");
    return 0;
}
