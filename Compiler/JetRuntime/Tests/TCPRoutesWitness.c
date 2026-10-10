// Real Jet-only runtime witness: canonical carriers, loopback TCP, no mock providers.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#ifndef JET_TCP_U8_STRIDE
#error "runner must select the runtime's native U8 epoch"
#endif
#if JET_TCP_U8_STRIDE == 1
typedef uint8_t Byte;
#elif JET_TCP_U8_STRIDE == 8
typedef uint64_t Byte;
#else
#error "unknown native U8 epoch"
#endif
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
extern void jet_rt_handle_drop_jet_net_error(Error *);
extern void jet_rt_handle_drop_jet_socket_addr(Address *);
extern void jet_rt_handle_drop_jet_tcp_stream(Stream *);
extern void jet_rt_handle_drop_jet_tcp_listener(Listener *);
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
typedef struct { int64_t readable, writable; } Ready;
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_handle_drop_jet_net_ready(Ready *);
extern Listener *jet_rt_handle_clone_jet_tcp_listener(Listener *);
extern Text *jet_rt_handle_show_jet_tcp_listener(Listener *);
extern Text *jet_rt_handle_show_jet_tcp_stream(Stream *);
extern int64_t jet_net_listener_local_addr(Listener *, Text **, Error **);
extern int64_t jet_net_tcp_local_addr(Stream *, Text **, Error **);
extern int64_t jet_net_tcp_peer_addr(Stream *, Text **, Error **);
extern int64_t jet_net_tcp_accept_deadline(Listener *, int64_t, Stream **, Error **);
extern int64_t jet_net_tcp_read(Stream *, Text **, Error **);
extern int64_t jet_net_tcp_read_bytes(Stream *, int64_t, Byte **, int64_t *, Error **);
extern int64_t jet_net_tcp_read_bytes_deadline(Stream *, int64_t, int64_t, Byte **, int64_t *, Error **);
extern int64_t jet_net_tcp_read_text(Stream *, int64_t, Text **, Error **);
extern int64_t jet_net_tcp_read_text_deadline(Stream *, int64_t, int64_t, Text **, Error **);
extern int64_t jet_net_tcp_write(Stream *, Text *, Error **);
extern int64_t jet_net_tcp_write_text(Stream *, Text *, Error **);
extern int64_t jet_net_tcp_write_text_deadline(Stream *, Text *, int64_t, Error **);
extern int64_t jet_net_tcp_write_bytes(Stream *, const Byte *, int64_t, int64_t *, Error **);
extern int64_t jet_net_tcp_write_bytes_deadline(Stream *, const Byte *, int64_t, int64_t, int64_t *, Error **);
extern int64_t jet_net_tcp_write_all_bytes(Stream *, const Byte *, int64_t, Error **);
extern int64_t jet_net_tcp_write_all_bytes_deadline(Stream *, const Byte *, int64_t, int64_t, Error **);
extern int64_t jet_net_tcp_ready_deadline(Stream *, int64_t, int64_t, Ready **, Error **);
extern int64_t jet_net_tcp_shutdown(Stream *, int64_t, Error **);
extern int64_t jet_net_tcp_close(Stream *, Error **);
extern int64_t jet_net_sendfile(Stream *, Text *, int64_t *, Error **);
static Text *text(const char *s) { return jet_rt_string_from_static(s, (int64_t)strlen(s)); }
static void text_is(Text *t, const char *s) { assert(t && t->length == (int64_t)strlen(s) && !memcmp(t->data, s, (size_t)t->length)); }
static void failure(Error **e, int64_t tag, const char *operation, const char *message) {
    assert(*e && (*e)->tag == tag);
    text_is((*e)->payload->operation, operation);
    if (message) text_is((*e)->payload->message, message);
    jet_rt_handle_drop_jet_net_error(*e);
    *e = NULL;
}
static void accept_drop(Listener *listener, Error **error) {
    Stream *accepted = NULL;
    assert(jet_net_tcp_accept(listener, &accepted, error) == 1 && accepted);
    jet_rt_handle_drop_jet_tcp_stream(accepted);
}
static void handles(Listener *listener, Address *address) {
    // Duration Word arguments borrow exact-Int nanoseconds, not handles or ms.
    const int64_t second = 1000000000;
    Error *error = NULL;
    Stream *client = NULL, *server = NULL;
    Listener *clone = jet_rt_handle_clone_jet_tcp_listener(listener);
    assert(clone && clone->fd->number != listener->fd->number);
    assert(jet_net_tcp_connect_addr(address, &client, &error) == 1);
    assert(jet_net_tcp_accept_deadline(clone, second, &server, &error) == 1);
    jet_rt_handle_drop_jet_tcp_listener(clone);
    Text *value = NULL;
    assert(jet_net_listener_local_addr(listener, &value, &error) == 1 && value->length > 0);
    jet_rt_string_drop(value);
    assert(jet_net_tcp_peer_addr(client, &value, &error) == 1 && value->length > 0);
    jet_rt_string_drop(value);
    assert(jet_net_tcp_local_addr(client, &value, &error) == 1 && value->length > 0);
    jet_rt_string_drop(value);
    value = jet_rt_handle_show_jet_tcp_listener(listener);
    assert(value->length > 12 && !memcmp(value->data, "TcpListener(", 12));
    jet_rt_string_drop(value);
    value = jet_rt_handle_show_jet_tcp_stream(client);
    assert(value->length > 10 && !memcmp(value->data, "TcpStream(", 10));
    jet_rt_string_drop(value);
    Ready *ready = NULL;
    assert(jet_net_tcp_ready_deadline(client, 1, second, &ready, &error) == 1 && ready->writable);
    jet_rt_handle_drop_jet_net_ready(ready);
    Byte byte = 'x', *bytes = NULL;
    int64_t count = -1, length = -1;
    assert(jet_net_tcp_write_bytes(client, &byte, 1, &count, &error) == 1 && count == 1);
    assert(jet_net_tcp_ready_deadline(server, 0, second, &ready, &error) == 1 && ready->readable);
    jet_rt_handle_drop_jet_net_ready(ready);
    assert(jet_net_tcp_read_bytes(server, 1, &bytes, &length, &error) == 1 && length == 1 && bytes[0] == byte);
    jet_rt_free(bytes, length * JET_TCP_U8_STRIDE, 8);
    assert(jet_net_tcp_write_bytes_deadline(server, &byte, 1, second, &count, &error) == 1 && count == 1);
    assert(jet_net_tcp_read_bytes_deadline(client, 1, second, &bytes, &length, &error) == 1 && length == 1 && bytes[0] == byte);
    jet_rt_free(bytes, length * JET_TCP_U8_STRIDE, 8);
    Text *x = text("x");
    assert(jet_net_tcp_write(client, x, &error) == 1);
    assert(jet_net_tcp_read(server, &value, &error) == 1);
    text_is(value, "x"); jet_rt_string_drop(value);
    assert(jet_net_tcp_write_text(server, x, &error) == 1);
    assert(jet_net_tcp_read_text(client, 1, &value, &error) == 1);
    text_is(value, "x"); jet_rt_string_drop(value);
    assert(jet_net_tcp_write_text_deadline(client, x, second, &error) == 1);
    assert(jet_net_tcp_read_text_deadline(server, 1, second, &value, &error) == 1);
    text_is(value, "x"); jet_rt_string_drop(value);
    assert(jet_net_tcp_write_all_bytes(server, &byte, 1, &error) == 1);
    assert(jet_net_tcp_read_text(client, 1, &value, &error) == 1);
    text_is(value, "x"); jet_rt_string_drop(value);
    assert(jet_net_tcp_write_all_bytes_deadline(client, &byte, 1, second, &error) == 1);
    assert(jet_net_tcp_read_text(server, 1, &value, &error) == 1);
    text_is(value, "x"); jet_rt_string_drop(value);
    byte = 255;
    assert(jet_net_tcp_write_all_bytes(client, &byte, 1, &error) == 1);
    assert(jet_net_tcp_read_text(server, 1, &value, &error) == 0);
    failure(&error, 0, "tcp read text", "tcp read text failed: invalid UTF-8: invalid utf-8 sequence of 1 bytes from index 0");
    assert(jet_net_tcp_write_all_bytes(client, &byte, 1, &error) == 1);
    assert(jet_net_tcp_read_text_deadline(server, 1, second, &value, &error) == 0);
    failure(&error, 0, "tcp read text", "tcp read text failed: invalid utf-8 sequence of 1 bytes from index 0");
    char filename[] = "/tmp/jet-tcp-sendfile-XXXXXX";
    int fd = mkstemp(filename);
    assert(fd >= 0 && write(fd, "z", 1) == 1 && close(fd) == 0);
    Text *path = text(filename);
    assert(jet_net_sendfile(client, path, &count, &error) == 1 && count == 1);
    assert(jet_net_tcp_read_text(server, 1, &value, &error) == 1);
    text_is(value, "z"); jet_rt_string_drop(value);
    assert(unlink(filename) == 0);
    assert(jet_net_sendfile(client, path, &count, &error) == 0);
    assert(error->payload->present && error->payload->code == 2);
    text_is(error->payload->address, filename);
    failure(&error, 13, "tcp sendfile open", NULL);
    jet_rt_string_drop(path);
    assert(jet_net_tcp_read_bytes(server, 0, &bytes, &length, &error) == 1 && length == 0);
    assert(jet_net_tcp_read_bytes(server, -1, &bytes, &length, &error) == 0);
    failure(&error, 0, "tcp read", "tcp read limit must be non-negative");
    assert(jet_net_tcp_read_bytes_deadline(server, 1, -1000000, &bytes, &length, &error) == 0);
    failure(&error, 0, "tcp read", "network timeout must be non-negative");
    assert(jet_net_tcp_read_bytes_deadline(server, 1, 0, &bytes, &length, &error) == 0);
    failure(&error, 8, "tcp read", "deadline exceeded while waiting in tcp read");
    assert(jet_net_tcp_read_bytes_deadline(server, 1, 3000000, &bytes, &length, &error) == 0);
    failure(&error, 8, "tcp read", "deadline exceeded while waiting in tcp read");
    assert(jet_net_set_read_timeout(server, 3, &error) == 1);
    assert(jet_net_tcp_read_bytes(server, 1, &bytes, &length, &error) == 0);
    failure(&error, 8, "tcp read", "deadline exceeded while waiting in tcp read");
    assert(jet_net_tcp_ready_deadline(server, 0, 3000000, &ready, &error) == 0);
    failure(&error, 8, "tcp ready", "deadline exceeded while waiting in tcp ready");
    Stream *absent = NULL;
    assert(jet_net_tcp_accept_deadline(listener, 3000000, &absent, &error) == 0 && !absent);
    failure(&error, 8, "tcp accept", "deadline exceeded while waiting in tcp accept");
    assert(jet_net_tcp_shutdown(client, 1, &error) == 1 && client->write_shutdown);
    assert(jet_net_tcp_write_bytes(client, &byte, 1, &count, &error) == 0);
    failure(&error, 7, "tcp write", "tcp write failed: socket is closed");
    assert(jet_net_tcp_read_bytes(server, 1, &bytes, &length, &error) == 1 && length == 0);
    assert(jet_net_tcp_shutdown(server, 0, &error) == 1 && server->read_shutdown);
    assert(jet_net_tcp_read_bytes(server, 0, &bytes, &length, &error) == 0);
    failure(&error, 7, "tcp read", "tcp read failed: socket is closed");
    assert(jet_net_tcp_close(client, &error) == 1 && client->fd->closed);
    assert(jet_net_tcp_close(client, &error) == 1);
    assert(jet_net_tcp_write_all_bytes(client, &byte, 0, &error) == 1);
    assert(jet_net_tcp_shutdown(client, 2, &error) == 0);
    failure(&error, 7, "tcp shutdown", "tcp shutdown failed: socket is closed");
    assert(jet_net_tcp_ready_deadline(client, 1, second, &ready, &error) == 0);
    failure(&error, 7, "tcp ready", "tcp ready failed: socket is closed");
    assert(jet_net_tcp_local_addr(client, &value, &error) == 1);
    jet_rt_string_drop(value);
    jet_rt_string_drop(x);
    jet_rt_handle_drop_jet_tcp_stream(client);
    jet_rt_handle_drop_jet_tcp_stream(server);
    assert(!error);
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
    handles(listener, address);
    Stream *stream = NULL;
    assert(jet_net_tcp_connect_timeout(address, 2000, &stream, &error) == 1 && stream);
    accept_drop(listener, &error);
    Address *local = NULL, *peer = NULL;
    assert(jet_net_tcp_local_socket_addr(stream, &local, &error) == 1 && local->port > 0);
    assert(jet_net_tcp_peer_socket_addr(stream, &peer, &error) == 1 && peer->port == address->port);
    jet_rt_handle_drop_jet_socket_addr(local);
    jet_rt_handle_drop_jet_socket_addr(peer);
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
    jet_rt_handle_drop_jet_socket_addr(local);
    assert(jet_net_set_timeout(stream, 0, &error) == 1 && stream->read_ms == 0);
    jet_rt_handle_drop_jet_tcp_stream(stream);
    stream = NULL;
    assert(jet_net_tcp_connect_timeout(address, -1, &stream, &error) == 0 && !stream);
    assert(error->payload->address);
    failure(&error, 0, "tcp connect", "network timeout must be non-negative");
    assert(jet_net_tcp_connect_timeout(address, 0, &stream, &error) == 0 && !stream);
    assert(!error->payload->address && !error->payload->present);
    failure(&error, 8, "tcp connect", "deadline exceeded while waiting in tcp connect");
    assert(jet_net_tcp_connect_addr(address, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_jet_tcp_stream(stream);
    char endpoint[64];
    snprintf(endpoint, sizeof endpoint, "127.0.0.1:%lld", (long long)address->port);
    Text *endpoint_text = text(endpoint);
    assert(jet_net_tcp_connect(endpoint_text, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_jet_tcp_stream(stream);
    Text *host = text("localhost");
    assert(jet_net_tcp_connect_happy(host, address->port, 2000, &stream, &error) == 1);
    accept_drop(listener, &error);
    jet_rt_handle_drop_jet_tcp_stream(stream);
    assert(jet_net_tcp_connect_happy(host, -1, 2000, &stream, &error) == 0);
    failure(&error, 0, "tcp connect", "invalid port `-1`: expected 0..65535");
    assert(jet_net_tcp_connect_happy(host, address->port, 0, &stream, &error) == 0);
    failure(&error, 8, "tcp connect", "deadline exceeded while waiting in tcp connect");
    Text *invalid = text("127.0.0.1:not-a-port");
    assert(jet_net_tcp_connect(invalid, &stream, &error) == 0);
    failure(&error, 0, "tcp connect", "tcp connect failed: invalid port value");
    jet_rt_string_drop(invalid);
    jet_rt_handle_drop_jet_tcp_listener(listener);
    assert(jet_net_tcp_connect(endpoint_text, &stream, &error) == 0);
    assert(error->payload->present && error->payload->code == 111);
    text_is(error->payload->address, endpoint);
    failure(&error, 4, "tcp connect", NULL);
    jet_rt_handle_drop_jet_socket_addr(address);
    jet_rt_string_drop(endpoint_text);
    jet_rt_string_drop(host);
    Text *listen_text = text("127.0.0.1:0");
    assert(jet_net_tcp_listen(listen_text, &listener, &error) == 1);
    jet_rt_handle_drop_jet_tcp_listener(listener);
    jet_rt_string_drop(listen_text);
    puts("tcp routes: ok");
    return 0;
}
