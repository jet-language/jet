// Link against the composed native runtime; no Rust socket or Box layouts.
// This fixture exercises the deadline route's worker result and FD ownership.
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t lock, number, closed; Text *path; void *buffer; int64_t capacity, filled, position, references; } FD;
typedef struct { FD *fd; int64_t reserved, read_shutdown, write_shutdown, read_ms, write_ms, mutex; } Stream;
typedef struct { int64_t tag; void *detail; } NetError;
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern int64_t jet_net_unix_connect_deadline(Text *, int64_t, Stream **, NetError **);
extern void jet_rt_handle_drop_jet_unix_stream(Stream *);
extern void jet_rt_handle_drop_jet_net_error(NetError *);
static Text text(char *value) { return (Text){(unsigned char *)value,(int64_t)strlen(value),0}; }
int main(void) {
    char *argv[]={"unix-connect-witness",NULL},*env[]={NULL};
    jet_rt_host_arguments(1,argv,env);
    int64_t budget=INT64_C(5000000000);
    Stream *stream=NULL; NetError *error=NULL;
    unsigned char long_name[108]; memset(long_name,'x',sizeof long_name);
    Text path={long_name,108,0};
    assert(jet_net_unix_connect_deadline(&path,budget,&stream,&error)==0);
    assert(stream==NULL && error && error->tag==0); jet_rt_handle_drop_jet_net_error(error); error=NULL;
    long_name[0]=0;
    assert(jet_net_unix_connect_deadline(&path,budget,&stream,&error)==0);
    assert(error && error->tag==0); jet_rt_handle_drop_jet_net_error(error); error=NULL;
    int64_t negative=-1000000;
    assert(jet_net_unix_connect_deadline(&path,negative,&stream,&error)==0);
    assert(error && error->tag==0); jet_rt_handle_drop_jet_net_error(error); error=NULL;
    char filename[]="/tmp/jet-unix-route-XXXXXX";
    int temp=mkstemp(filename); assert(temp>=0); close(temp); assert(unlink(filename)==0);
    int listener=socket(AF_UNIX,SOCK_STREAM|SOCK_CLOEXEC,0); assert(listener>=0);
    struct sockaddr_un address={.sun_family=AF_UNIX}; strcpy(address.sun_path,filename);
    assert(bind(listener,(struct sockaddr *)&address,(socklen_t)(sizeof(sa_family_t)+strlen(filename)+1))==0);
    assert(listen(listener,8)==0);
    path=text(filename);
    assert(jet_net_unix_connect_deadline(&path,budget,&stream,&error)==1 && stream && !error);
    assert(stream->fd->closed==0 && stream->read_shutdown==0 && stream->write_shutdown==0);
    assert(stream->read_ms==-1 && stream->write_ms==-1);
    assert((fcntl((int)stream->fd->number,F_GETFL)&O_NONBLOCK)!=0);
    assert((fcntl((int)stream->fd->number,F_GETFD)&FD_CLOEXEC)!=0);
    int accepted=accept(listener,NULL,NULL); assert(accepted>=0);
    unsigned char sent[]={0,1,127,255},received[4];
    assert(write((int)stream->fd->number,sent,sizeof sent)==sizeof sent);
    assert(read(accepted,received,sizeof received)==sizeof received && !memcmp(sent,received,sizeof sent));
    int owned=(int)stream->fd->number; jet_rt_handle_drop_jet_unix_stream(stream);
    assert(fcntl(owned,F_GETFD)==-1);
    close(accepted); close(listener); assert(unlink(filename)==0);
    puts("unix connect deadline: path errors, raw duration, detached-worker success, nonblocking/CLOEXEC and FD ownership OK");
}
