/* Real composed Jet object; --wrap=jet_rt_free, real FD72 and Stream56. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
typedef struct { int64_t mutex,number,closed; void *path,*buffer; int64_t mode,filled,position,refs; } FD;
typedef struct { FD *fd; int64_t reserved,read_shutdown,write_shutdown,read_ms,write_ms,mutex; } Stream;
_Static_assert(sizeof(FD)==72 && offsetof(FD,closed)==16 && sizeof(Stream)==56 && offsetof(Stream,read_shutdown)==16 && offsetof(Stream,write_shutdown)==24,"canonical Host geometry");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern FD *jet_fd_adopt_unbuffered_raw(int64_t,void *);
extern Stream *jet_socket_stream_from_fd_raw(FD *);
extern void jet_socket_stream_mark_shutdown_raw(Stream *,int64_t);
extern void jet_socket_stream_mark_closed_raw(Stream *);
extern void jet_rt_handle_drop_jet_tcp_stream(Stream *);
extern const bool *jet_rt_handle_field_jet_tcp_stream_closed(Stream *);
extern const bool *jet_rt_handle_field_jet_tcp_stream_read_shutdown(Stream *);
extern const bool *jet_rt_handle_field_jet_tcp_stream_write_shutdown(Stream *);
static FD *watched_fd;
static Stream *watched_stream;
static unsigned fd_freed,stream_freed;
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    if(ptr==watched_fd) { assert(!fd_freed && !stream_freed && size==72 && align==8); ++fd_freed; }
    if(ptr==watched_stream) { assert(fd_freed==1 && !stream_freed && size==56 && align==8); ++stream_freed; }
    __real_jet_rt_free(ptr,size,align);
}
int main(void) {
    jet_rt_alloc_init();
    for(int64_t how=0;how<3;++how) {
        int pipefd[2]; assert(pipe(pipefd)==0);
        watched_fd=jet_fd_adopt_unbuffered_raw(pipefd[0],NULL); watched_stream=jet_socket_stream_from_fd_raw(watched_fd);
        fd_freed=stream_freed=0;
        // Inspect the native Bool slot's byte through character aliases, which
        // are legal even after this C client writes an I64 padding word.
        const unsigned char *closed=(const unsigned char *)jet_rt_handle_field_jet_tcp_stream_closed(watched_stream);
        const unsigned char *read=(const unsigned char *)jet_rt_handle_field_jet_tcp_stream_read_shutdown(watched_stream);
        const unsigned char *write=(const unsigned char *)jet_rt_handle_field_jet_tcp_stream_write_shutdown(watched_stream);
        assert((const void *)closed==(const void *)&watched_fd->closed);
        assert((const void *)read==(const void *)&watched_stream->read_shutdown && (const void *)write==(const void *)&watched_stream->write_shutdown);
        assert(!*closed && !*read && !*write);
        jet_socket_stream_mark_shutdown_raw(watched_stream,how);
        assert(!*closed && *read==(how==0 || how==2) && *write==(how==1 || how==2));
        jet_socket_stream_mark_closed_raw(watched_stream); assert(*closed && *read && *write);
        // Projections remain live aliases: no allocated snapshots or owners.
        watched_stream->read_shutdown=0x100; watched_stream->write_shutdown=0x101;
        assert(!*read && *write);
        jet_rt_handle_drop_jet_tcp_stream(watched_stream); assert(fd_freed==1 && stream_freed==1); assert(close(pipefd[1])==0);
    }
    puts("tcp-stream-fields: borrowed canonical FD.closed/read/write Bool slots, live low-byte aliases, shutdown/close source of truth, no projection owners OK");
    return 0;
}
