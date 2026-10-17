// The UDP family is intentionally excluded from the native pack until its
// packet bridge is ported. Link this fixture only with NetDatagramRoutes +
// NetRoutes explicitly composed; it covers the six completed UDP leaves.
// Baseline native U8 list stride is eight, as in NetTests.c.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t family; unsigned char octets[16]; } IP;
typedef struct { IP ip; int64_t port, flow, scope, reserved; } Address;
typedef struct { int64_t lock, number, closed; Text *path; void *buffer; int64_t capacity, filled, position, references; } FD;
typedef struct { int64_t references, mutex; FD *fd; int64_t timeout_ms; } UDPState;
typedef struct { UDPState *state; } UDP;
typedef struct { int64_t tag; void *detail; } NetError;
typedef struct { uint32_t domain, stage; int64_t code; uint32_t kind, padding; Text *message; } Facts;
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern int64_t jet_net_udp_bind(Text *, UDP **, NetError **);
extern int64_t jet_net_udp_bind_addr(Address *, UDP **, NetError **);
extern int64_t jet_net_udp_local_addr(UDP *, Address **, NetError **);
extern int64_t jet_net_udp_set_timeout(UDP *, int64_t, NetError **);
extern int64_t jet_net_udp_send_to(UDP *, Text *, Address *, int64_t *, NetError **);
extern int64_t jet_net_udp_send_bytes_to(UDP *, const uint64_t *, int64_t, Address *, int64_t *, NetError **);
extern int64_t jet_socket_receive_from_raw(FD *, uint64_t *, int64_t, Address *, Facts *);
extern void jet_udp_socket_mark_closed_raw(UDP *);
extern void jet_rt_handle_drop_jet_udp_socket(UDP *);
extern void jet_rt_handle_drop_jet_socket_addr(Address *);
extern void jet_rt_handle_drop_jet_net_error(NetError *);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
int main(void) {
    char *argv[]={"udp-route-witness",NULL},*env[]={NULL}; jet_rt_host_arguments(1,argv,env);
    Address loopback={.ip={.family=4,.octets={127,0,0,1}}}; UDP *receiver=NULL,*sender=NULL; NetError *error=NULL;
    assert(jet_net_udp_bind_addr(&loopback,&receiver,&error)==1 && receiver && !error);
    Text bind_text={(unsigned char *)"127.0.0.1:0",11,0};
    assert(jet_net_udp_bind(&bind_text,&sender,&error)==1 && sender && !error);
    Address *endpoint=NULL; assert(jet_net_udp_local_addr(receiver,&endpoint,&error)==1 && endpoint && !error);
    assert(endpoint->ip.family==4 && endpoint->port>0 && !memcmp(endpoint->ip.octets,loopback.ip.octets,4));
    assert(jet_net_udp_set_timeout(receiver,1000,&error)==1 && receiver->state->timeout_ms==1000 && !error);
    assert(jet_net_udp_set_timeout(receiver,-1,&error)==0 && error && error->tag==0);
    jet_rt_handle_drop_jet_net_error(error); error=NULL;
    unsigned char binary[]={0,1,127,255}; Text message={binary,4,0}; int64_t sent;
    assert(jet_net_udp_send_to(sender,&message,endpoint,&sent,&error)==1 && !error);
    assert(jet_int_owned_to_i64(sent)==4); jet_rt_int_drop(sent);
    uint64_t bytes[4]={0},input[]={255,127,1,0}; Address from={0}; Facts facts={0};
    assert(jet_socket_receive_from_raw(receiver->state->fd,bytes,4,&from,&facts)==4);
    for(int i=0;i<4;i++) assert(bytes[i]==binary[i]);
    assert(jet_net_udp_send_bytes_to(sender,input,4,endpoint,&sent,&error)==1 && !error);
    assert(jet_int_owned_to_i64(sent)==4); jet_rt_int_drop(sent);
    assert(jet_socket_receive_from_raw(receiver->state->fd,bytes,2,&from,&facts)==4 && bytes[0]==255 && bytes[1]==127);
    jet_udp_socket_mark_closed_raw(sender);
    assert(jet_net_udp_send_to(sender,&message,endpoint,&sent,&error)==0 && error && error->tag==7);
    jet_rt_handle_drop_jet_net_error(error); error=NULL;
    jet_udp_socket_mark_closed_raw(receiver);
    Address *closed=NULL; assert(jet_net_udp_local_addr(receiver,&closed,&error)==0 && error && error->tag==7 && !closed);
    jet_rt_handle_drop_jet_net_error(error); jet_rt_handle_drop_jet_socket_addr(endpoint);
    jet_rt_handle_drop_jet_udp_socket(receiver); jet_rt_handle_drop_jet_udp_socket(sender);
    puts("UDP leaves: bind/address/timeout/raw scalar inputs/exact count outputs/binary text/native bytes/closed errors OK");
}
