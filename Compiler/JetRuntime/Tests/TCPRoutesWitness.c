// Real Jet-only runtime witness: canonical carriers, loopback TCP, no mock providers.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t family; unsigned char octets[16]; int64_t port, flow, scope, reserved; } Address;
typedef struct { int64_t lock, number, closed; void *path, *buffer; int64_t capacity, filled, position, references; } FD;
typedef struct { FD *fd; int64_t reserved, read_shutdown, write_shutdown, read_ms, write_ms, mutex; } Stream;
typedef struct { FD *fd; } Listener;
typedef struct { Text *operation, *address, *name, *message; int64_t present, code; } Detail;
typedef struct { int64_t tag; Detail *payload; } Error;
_Static_assert(sizeof(Address) == 56 && sizeof(Stream) == 56 && sizeof(FD) == 72, "native network carriers");
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_handle_drop_JetNetError(Error *);
extern void jet_rt_handle_drop_JetSocketAddr(Address *);
extern void jet_rt_handle_drop_JetTCPStream(Stream *);
extern void jet_rt_handle_drop_JetTCPListener(Listener *);
extern void jet_socket_stream_mark_closed_raw(Stream *);
extern int64_t jet_net_tcp_listen(Text *, Listener **, Error **);
extern int64_t jet_net_tcp_listen_addr(Address *, Listener **, Error **);
extern int64_t jet_net_tcp_accept(Listener *, Stream **, Error **);
extern int64_t jet_net_tcp_connect(Text *, Stream **, Error **);
extern int64_t jet_net_tcp_connect_addr(Address *, Stream **, Error **);
extern int64_t jet_net_tcp_connect_timeout(Address *, int64_t, Stream **, Error **);
extern int64_t jet_net_tcp_connect_happy(Text *, int64_t, int64_t, Stream **, Error **);
extern int64_t jet_net_listener_local_socket_addr(Listener *, Address **, Error **);
extern int64_t jet_net_tcp_local_socket_addr(Stream *, Address **, Error **);
extern int64_t jet_net_tcp_peer_socket_addr(Stream *, Address **, Error **);
extern int64_t jet_net_set_timeout(Stream *, int64_t, Error **);
extern int64_t jet_net_set_read_timeout(Stream *, int64_t, Error **);
extern int64_t jet_net_set_write_timeout(Stream *, int64_t, Error **);
extern int64_t jet_net_nodelay(Stream *, bool *, Error **);
extern int64_t jet_net_set_nodelay(Stream *, bool, Error **);
extern int64_t jet_net_ttl(Stream *, int64_t *, Error **);
extern int64_t jet_net_set_ttl(Stream *, int64_t, Error **);
static Text *text(const char *s) { return jet_rt_string_from_static(s, (int64_t)strlen(s)); }
static void text_is(Text *t, const char *s) { assert(t && t->length == (int64_t)strlen(s) && !memcmp(t->data, s, (size_t)t->length)); }
static void failure(Error **e, int64_t tag, const char *operation, const char *message) {
    assert(*e && (*e)->tag == tag);
    text_is((*e)->payload->operation, operation);
    if (message) text_is((*e)->payload->message, message);
    jet_rt_handle_drop_JetNetError(*e);
    *e = NULL;
}
static void accept_drop(Listener *listener, Error **error) {
    Stream *accepted = NULL;
    assert(jet_net_tcp_accept(listener, &accepted, error) == 1 && accepted);
    jet_rt_handle_drop_JetTCPStream(accepted);
}
int main(void) {
    char *argv[] = {"tcp-routes", NULL}, *env[] = {NULL};
    jet_rt_host_arguments(1, argv, env);
    Address loopback = {.family = 4, .octets = {127, 0, 0, 1}};
    Listener *listener = NULL;
    Error *error = NULL;
    assert(jet_net_tcp_listen_addr(&loopback, &listener, &error) == 1 && listener && !error);
    Address *address = NULL;
    assert(jet_net_listener_local_socket_addr(listener, &address, &error) == 1 && address->port > 0);
    Stream *stream = NULL;
    assert(jet_net_tcp_connect_timeout(address, 2000, &stream, &error) == 1 && stream);
    accept_drop(listener, &error);
    Address *local = NULL, *peer = NULL;
    assert(jet_net_tcp_local_socket_addr(stream, &local, &error) == 1 && local->port > 0);
    assert(jet_net_tcp_peer_socket_addr(stream, &peer, &error) == 1 && peer->port == address->port);
    jet_rt_handle_drop_JetSocketAddr(local);
    jet_rt_handle_drop_JetSocketAddr(peer);
    struct { bool value; unsigned char guard[7]; } flag = {false, {1, 2, 3, 4, 5, 6, 7}};
    assert(jet_net_set_nodelay(stream, true, &error) == 1);
    assert(jet_net_nodelay(stream, &flag.value, &error) == 1 && flag.value);
    for (unsigned i = 0; i < 7; ++i) assert(flag.guard[i] == i + 1);
    int64_t ttl = -1;
    assert(jet_net_set_ttl(stream, 63, &error) == 1);
    assert(jet_net_ttl(stream, &ttl, &error) == 1 && ttl == 63); // raw Word, not tagged Int
    assert(jet_net_set_timeout(stream, 7, &error) == 1 && stream->read_ms == 7 && stream->write_ms == 7);
    assert(jet_net_set_read_timeout(stream, 9, &error) == 1 && stream->read_ms == 9 && stream->write_ms == 7);
    assert(jet_net_set_write_timeout(stream, 11, &error) == 1 && stream->read_ms == 9 && stream->write_ms == 11);
    assert(jet_net_set_timeout(stream, -1, &error) == 0);
    failure(&error, 0, "set tcp timeout", "network timeout must be non-negative");
    assert(jet_net_set_ttl(stream, INT64_MAX, &error) == 0);
    failure(&error, 0, "set tcp ttl", "ttl must fit u32");
    jet_socket_stream_mark_closed_raw(stream);
    assert(jet_net_nodelay(stream, &flag.value, &error) == 0);
    failure(&error, 7, "tcp nodelay", "tcp nodelay failed: socket is closed");
    // Logical close retains the descriptor: metadata queries and timeout setters still work.
    assert(jet_net_tcp_local_socket_addr(stream, &local, &error) == 1);
    jet_rt_handle_drop_JetSocketAddr(local);
    assert(jet_net_set_timeout(stream, 0, &error) == 1 && stream->read_ms == 0);
    jet_rt_handle_drop_JetTCPStream(stream);
    stream = NULL;
    assert(jet_net_tcp_connect_timeout(address, -1, &stream, &error) == 0 && !stream);
    assert(error->payload->address);
    failure(&error, 0, "tcp connect", "network timeout must be non-negative");
    assert(jet_net_tcp_connect_timeout(address, 0, &stream, &error) == 0 && !stream);
    assert(!error->payload->address && !error->payload->present);
    failure(&error, 8, "tcp connect", "deadline exceeded while waiting in tcp connect");
    assert(jet_net_tcp_connect_addr(address, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_JetTCPStream(stream);
    char endpoint[64];
    snprintf(endpoint, sizeof endpoint, "127.0.0.1:%lld", (long long)address->port);
    Text *endpoint_text = text(endpoint);
    assert(jet_net_tcp_connect(endpoint_text, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_JetTCPStream(stream);
    Text *host = text("localhost");
    assert(jet_net_tcp_connect_happy(host, address->port, 2000, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_JetTCPStream(stream);
    assert(jet_net_tcp_connect_happy(host, -1, 2000, &stream, &error) == 0);
    failure(&error, 0, "tcp connect", "invalid port `-1`: expected 0..65535");
    assert(jet_net_tcp_connect_happy(host, address->port, 0, &stream, &error) == 0);
    failure(&error, 8, "tcp connect", "deadline exceeded while waiting in tcp connect");
    Text *invalid = text("127.0.0.1:not-a-port");
    assert(jet_net_tcp_connect(invalid, &stream, &error) == 0);
    failure(&error, 0, "tcp connect", "tcp connect failed: invalid port value");
    jet_rt_string_drop(invalid);
    jet_rt_handle_drop_JetTCPListener(listener);
    assert(jet_net_tcp_connect(endpoint_text, &stream, &error) == 0);
    assert(error->payload->present && error->payload->code == 111);
    text_is(error->payload->address, endpoint);
    failure(&error, 4, "tcp connect", NULL);
    jet_rt_handle_drop_JetSocketAddr(address);
    jet_rt_string_drop(endpoint_text);
    jet_rt_string_drop(host);
    Text *listen_text = text("127.0.0.1:0");
    assert(jet_net_tcp_listen(listen_text, &listener, &error) == 1);
    jet_rt_handle_drop_JetTCPListener(listener);
    jet_rt_string_drop(listen_text);
    puts("tcp routes: ok");
    return 0;
}
