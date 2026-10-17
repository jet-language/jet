// Link once against the complete composed L4/L7/L8 runtime object.
// Address and socket byte-list carriers are packed C uint8_t elements.
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <netinet/in.h>

typedef struct { unsigned char *ptr; int64_t len, cap; } Text;
typedef struct { int64_t family; uint8_t octets[16]; } IP;
typedef struct { IP ip; int64_t port, flow, scope, reserved; } Address;
typedef struct { int64_t kind, has_os_code, os_code; Text *message; } ResolveError;
_Static_assert(sizeof(IP) == 24 && sizeof(Address) == 56, "network carrier sizes");
_Static_assert(sizeof(ResolveError) == 32, "resolve carrier size");
typedef struct {
    int64_t lock, number, closed;
    Text *path;
    void *buffer;
    int64_t capacity, filled, position, references;
} FD;
typedef struct {
    uint32_t domain, stage;
    int64_t code;
    uint32_t kind, padding;
    Text *message;
} ErrorOut;
_Static_assert(sizeof(FD) == 72 && sizeof(ErrorOut) == 32, "host descriptor/fact sizes");
extern FD *jet_socket_create_raw(int64_t, int64_t, int64_t, ErrorOut *);
extern int64_t jet_socket_bind_raw(FD *, Address *, ErrorOut *);
extern int64_t jet_socket_connect_raw(FD *, Address *, ErrorOut *);
extern int64_t jet_socket_listen_raw(FD *, int64_t, ErrorOut *);
extern FD *jet_socket_accept_raw(FD *, Address *, ErrorOut *);
extern int64_t jet_socket_address_raw(FD *, bool, Address *, ErrorOut *);
extern int64_t jet_socket_option_get_raw(FD *, int64_t, int64_t, int64_t *, ErrorOut *);
extern int64_t jet_socket_option_set_raw(FD *, int64_t, int64_t, int64_t, ErrorOut *);
extern int64_t jet_socket_shutdown_raw(FD *, int64_t, ErrorOut *);
extern void jet_socket_mark_closed_raw(FD *);
extern int64_t jet_socket_ready_raw(FD *, int64_t, int64_t, ErrorOut *);
extern void jet_fd_drop_raw(FD *);
typedef struct { FD *fd; int64_t reserved, read_shutdown, write_shutdown, read_ms, write_ms, mutex; } Stream;
typedef struct { FD *fd; } Listener;
typedef struct { int64_t references, mutex; FD *fd; int64_t timeout_ms; } UDPState;
typedef struct { UDPState *state; } UDP;
_Static_assert(sizeof(Stream) == 56 && sizeof(Listener) == 8 && sizeof(UDPState) == 32 && sizeof(UDP) == 8, "network holder sizes");
extern FD *jet_fd_retain_raw(FD *);
extern Stream *jet_socket_stream_from_fd_raw(FD *);
extern FD *jet_socket_stream_fd_raw(Stream *);
extern int64_t jet_socket_stream_flags_raw(Stream *);
extern void jet_socket_stream_mark_shutdown_raw(Stream *, int64_t);
extern void jet_socket_stream_mark_closed_raw(Stream *);
extern void jet_socket_stream_timeout_set_raw(Stream *, int64_t, int64_t);
extern int64_t jet_socket_stream_timeout_get_raw(Stream *, bool, int64_t *);
extern void jet_rt_handle_drop_jet_tcp_stream(Stream *);
extern Listener *jet_socket_listener_from_fd_raw(FD *);
extern Listener *jet_tcp_listener_clone_raw(Listener *, ErrorOut *);
extern Listener *jet_rt_handle_clone_jet_unix_listener(Listener *);
extern void jet_rt_handle_drop_jet_tcp_listener(Listener *);
extern void jet_rt_handle_drop_jet_unix_listener(Listener *);
extern UDP *jet_udp_socket_from_fd_raw(FD *);
extern UDP *jet_rt_handle_clone_jet_udp_socket(UDP *);
extern void jet_rt_handle_drop_jet_udp_socket(UDP *);
extern FD *jet_udp_socket_fd_raw(UDP *);
extern void jet_udp_socket_timeout_set_raw(UDP *, int64_t);
extern int64_t jet_udp_socket_timeout_get_raw(UDP *, int64_t *);
extern void jet_udp_socket_mark_closed_raw(UDP *);
extern int64_t jet_socket_read_raw(FD *, uint8_t *, int64_t, ErrorOut *);
extern int64_t jet_socket_write_raw(FD *, uint8_t *, int64_t, ErrorOut *);
extern int64_t jet_socket_send_to_raw(FD *, uint8_t *, int64_t, Address *, ErrorOut *);
extern int64_t jet_socket_receive_from_raw(FD *, uint8_t *, int64_t, Address *, ErrorOut *);
extern int64_t jet_socket_unix_bind_raw(FD *, Text *, ErrorOut *);
extern int64_t jet_socket_unix_connect_raw(FD *, Text *, ErrorOut *);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern IP *jet_net_raw_ip_from_bytes(uint8_t *, int64_t);
extern int64_t jet_net_raw_ip_bytes(IP *, uint8_t **);
extern Address *jet_net_raw_socket_from_parts(IP *, int64_t, int64_t, int64_t);
extern int64_t jet_net_raw_socket_scope(Address *, int64_t *);
extern Address *jet_rt_handle_clone_jet_socket_addr(Address *);
extern bool jet_rt_handle_eq_jet_socket_addr(Address *, Address *);
extern void jet_rt_handle_drop_jet_socket_addr(Address *);
extern void jet_rt_handle_drop_jet_ip_addr(IP *);
extern int64_t jet_socket_address_encode_raw(Address *, void *);
extern bool jet_socket_address_decode_raw(void *, int64_t, Address *);
extern int64_t jet_net_raw_resolve_ordered(Text *, int64_t, Address ***, int64_t *, ResolveError **);
extern void jet_rt_handle_drop_jet_socket_addr(Address *);
extern ResolveError *jet_rt_record_new_jet_resolve_error(int64_t, int64_t, int64_t, Text *);
extern ResolveError *jet_rt_handle_clone_jet_resolve_error(ResolveError *);
extern int64_t jet_rt_record_get_jet_resolve_error_kind(ResolveError *);
extern int64_t jet_rt_record_get_jet_resolve_error_os_code(ResolveError *);
extern void jet_rt_handle_drop_jet_resolve_error(ResolveError *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);

int main(void) {
    static char *argv[] = {"net-fixture", NULL}, *env[] = {NULL};
    jet_rt_host_arguments(1, argv, env);
    uint8_t *octets = jet_rt_alloc(16, 8);
    for (int i = 0; i < 16; ++i) octets[i] = i;
    IP *ip = jet_net_raw_ip_from_bytes(octets, 16);
    jet_rt_free(octets, 16, 8);
    assert(ip->family == 6);
    for (int i = 0; i < 16; ++i) assert(ip->octets[i] == i);
    uint8_t *copy_bytes = NULL;
    assert(jet_net_raw_ip_bytes(ip, &copy_bytes) == 16);
    for (int i = 0; i < 16; ++i) assert(copy_bytes[i] == (uint8_t)i);
    jet_rt_free(copy_bytes, 16, 8);
    Address *address = jet_net_raw_socket_from_parts(ip, INT64_C(0x12340000ffff), INT64_C(0x123412345678), INT64_C(0x5678ffffffff));
    jet_rt_handle_drop_jet_ip_addr(ip);
    assert(address->port == 65535 && address->flow == UINT32_C(0x12345678) && address->scope == UINT32_MAX && address->reserved == 0);
    int64_t scope = 0;
    assert(jet_net_raw_socket_scope(address, &scope) == 1 && scope == UINT32_MAX);
    Address *clone = jet_rt_handle_clone_jet_socket_addr(address);
    assert(clone != address && jet_rt_handle_eq_jet_socket_addr(address, clone));
    unsigned char raw[32] = {0};
    assert(jet_socket_address_encode_raw(address, raw) == sizeof(struct sockaddr_in6));
    struct sockaddr_in6 c_address;
    memcpy(&c_address, raw, sizeof(c_address));
    assert(c_address.sin6_family == AF_INET6 && ntohs(c_address.sin6_port) == 65535);
    assert(c_address.sin6_flowinfo == UINT32_C(0x12345678) && c_address.sin6_scope_id == UINT32_MAX);
    Address decoded;
    assert(jet_socket_address_decode_raw(raw, sizeof(c_address), &decoded));
    assert(jet_rt_handle_eq_jet_socket_addr(address, &decoded));
    jet_rt_handle_drop_jet_socket_addr(address);
    jet_rt_handle_drop_jet_socket_addr(clone);

    // Exact-Int children must survive releasing the original spilled owners.
    Text message = {(unsigned char *)"exact resolver", 14, 0};
    int64_t kind = jet_int_owned_from_i64(INT64_MAX), code = jet_int_owned_from_i64(INT64_MIN);
    ResolveError *error = jet_rt_record_new_jet_resolve_error(kind, 1, code, &message);
    jet_rt_int_drop(kind);
    jet_rt_int_drop(code);
    ResolveError *error_copy = jet_rt_handle_clone_jet_resolve_error(error);
    jet_rt_handle_drop_jet_resolve_error(error);
    kind = jet_rt_record_get_jet_resolve_error_kind(error_copy);
    code = jet_rt_record_get_jet_resolve_error_os_code(error_copy);
    assert(jet_int_owned_to_i64(kind) == INT64_MAX && jet_int_owned_to_i64(code) == INT64_MIN);
    jet_rt_int_drop(kind);
    jet_rt_int_drop(code);
    jet_rt_handle_drop_jet_resolve_error(error_copy);

    Text numeric = {(unsigned char *)"127.0.0.1", 9, 0};
    Address **rows = NULL;
    int64_t count = 0;
    error = NULL;
    assert(jet_net_raw_resolve_ordered(&numeric, INT64_C(0x123400000050), &rows, &count, &error) == 1);
    assert(count > 0 && rows && !error);
    // The route publishes one Socket56 box per answer behind an eight-byte
    // slot: the native [SocketAddr] List of JetSocketAddr handles.
    for (int64_t i = 0; i < count; ++i) {
        assert(rows[i]->ip.family == 4 && rows[i]->port == 80);
        assert(rows[i]->ip.octets[0] == 127 && rows[i]->ip.octets[3] == 1 && rows[i]->reserved == 0);
        jet_rt_handle_drop_jet_socket_addr(rows[i]);
    }
    jet_rt_free(rows, count * 8, 8);
    unsigned char invalid[] = {'a', 0, 'b'};
    Text nul = {invalid, 3, 0};
    error = NULL;
    assert(jet_net_raw_resolve_ordered(&nul, 0, &rows, &count, &error) == 0);
    assert(error && error->kind == 1 && !error->has_os_code);
    const char expected[] = "file name contained an unexpected NUL byte";
    assert(error->message->len == sizeof(expected) - 1 && memcmp(error->message->ptr, expected, sizeof(expected) - 1) == 0);
    jet_rt_handle_drop_jet_resolve_error(error);
    ErrorOut facts = {0};
    Address endpoint = {.ip = {.family = 4, .octets = {127, 0, 0, 1}}};
    FD *listener = jet_socket_create_raw(4, SOCK_STREAM, 0, &facts);
    assert(listener && facts.domain == 0);
    assert(jet_socket_bind_raw(listener, &endpoint, &facts) == 0);
    assert(jet_socket_listen_raw(listener, 128, &facts) == 0);
    assert(jet_socket_address_raw(listener, false, &endpoint, &facts) == 0 && endpoint.port > 0);
    FD *client = jet_socket_create_raw(4, SOCK_STREAM, 0, &facts);
    assert(client && (fcntl(client->number, F_GETFL) & O_NONBLOCK) != 0);
    assert((fcntl(client->number, F_GETFD) & FD_CLOEXEC) != 0);
    int64_t connected = jet_socket_connect_raw(client, &endpoint, &facts);
    if (connected < 0) {
        assert(facts.domain == 1 && facts.code == EINPROGRESS);
        assert((jet_socket_ready_raw(client, 4, 1000000000, &facts) & 4) != 0);
    }
    int64_t option = -1;
    assert(jet_socket_option_get_raw(client, SOL_SOCKET, SO_ERROR, &option, &facts) == 0 && option == 0);
    FD *accepted = jet_socket_accept_raw(listener, &endpoint, &facts);
    if (!accepted) {
        assert(facts.code == EAGAIN);
        assert((jet_socket_ready_raw(listener, 1, 1000000000, &facts) & 1) != 0);
        accepted = jet_socket_accept_raw(listener, &endpoint, &facts);
    }
    assert(accepted && endpoint.ip.family == 4);
    assert((fcntl(accepted->number, F_GETFL) & O_NONBLOCK) != 0);
    assert((fcntl(accepted->number, F_GETFD) & FD_CLOEXEC) != 0);
    assert(jet_socket_option_set_raw(client, IPPROTO_TCP, 1, 1, &facts) == 0);
    assert(jet_socket_option_get_raw(client, IPPROTO_TCP, 1, &option, &facts) == 0 && option == 1);
    uint8_t payload[] = {1, 0, 255, 2}, incoming[4] = {0};
    assert(jet_socket_write_raw(client, payload, 4, &facts) == 4);
    int64_t received = jet_socket_read_raw(accepted, incoming, 4, &facts);
    if (received < 0) {
        assert(facts.code == EAGAIN);
        assert((jet_socket_ready_raw(accepted, 1, 1000000000, &facts) & 1) != 0);
        received = jet_socket_read_raw(accepted, incoming, 4, &facts);
    }
    assert(received == 4 && memcmp(incoming, payload, sizeof(payload)) == 0);

    FD *datagram = jet_socket_create_raw(4, SOCK_DGRAM, 0, &facts);
    FD *sender = jet_socket_create_raw(4, SOCK_DGRAM, 0, &facts);
    endpoint.port = 0;
    assert(datagram && sender && jet_socket_bind_raw(datagram, &endpoint, &facts) == 0);
    assert(jet_socket_address_raw(datagram, false, &endpoint, &facts) == 0);
    assert(jet_socket_send_to_raw(sender, payload, 4, &endpoint, &facts) == 4);
    Address from = {0};
    assert(jet_socket_receive_from_raw(datagram, incoming, 2, &from, &facts) == 4);
    assert(incoming[0] == 1 && incoming[1] == 0 && from.ip.family == 4 && from.port > 0);
    jet_fd_drop_raw(sender);
    jet_fd_drop_raw(datagram);

    FD *unix_listener = jet_socket_create_raw(1, SOCK_STREAM, 0, &facts);
    unsigned char too_long[108];
    memset(too_long, 'x', sizeof(too_long));
    Text unix_path = {too_long, sizeof(too_long), 0};
    assert(jet_socket_unix_bind_raw(unix_listener, &unix_path, &facts) == -4097 && facts.domain == 0);
    too_long[0] = 0; // NUL wins even when the pathname is also overlength.
    assert(jet_socket_unix_bind_raw(unix_listener, &unix_path, &facts) == -4098 && facts.domain == 0);
    char pathname[] = "/tmp/jet-unix-XXXXXX";
    int placeholder = mkstemp(pathname);
    assert(placeholder >= 0);
    close(placeholder);
    assert(unlink(pathname) == 0);
    unix_path = (Text){(unsigned char *)pathname, (int64_t)strlen(pathname), 0};
    assert(jet_socket_unix_bind_raw(unix_listener, &unix_path, &facts) == 0);
    assert(jet_socket_listen_raw(unix_listener, 128, &facts) == 0);
    FD *unix_client = jet_socket_create_raw(1, SOCK_STREAM, 0, &facts);
    assert(unix_client && jet_socket_unix_connect_raw(unix_client, &unix_path, &facts) == 0);
    FD *unix_stream = jet_socket_accept_raw(unix_listener, NULL, &facts);
    assert(unix_stream);
    assert(jet_socket_write_raw(unix_client, payload, 4, &facts) == 4);
    assert(jet_socket_read_raw(unix_stream, incoming, 4, &facts) == 4);
    assert(memcmp(incoming, payload, sizeof(payload)) == 0);
    jet_fd_drop_raw(unix_stream);
    jet_fd_drop_raw(unix_client);
    jet_fd_drop_raw(unix_listener);
    assert(unlink(pathname) == 0);
    assert(jet_socket_shutdown_raw(accepted, 2, &facts) == 0);
    int closed_number = (int)accepted->number;
    jet_socket_mark_closed_raw(accepted);
    assert(accepted->closed == 1 && fcntl(closed_number, F_GETFD) >= 0);
    assert(jet_socket_shutdown_raw(accepted, 2, &facts) == -1 && facts.code == EBADF);
    jet_fd_drop_raw(accepted);
    assert(fcntl(closed_number, F_GETFD) == -1 && errno == EBADF);
    Stream *stream = jet_socket_stream_from_fd_raw(jet_fd_retain_raw(client));
    assert(stream->fd == client && stream->reserved == 0 && stream->read_ms == -1 && stream->write_ms == -1);
    int64_t duration = 17;
    assert(jet_socket_stream_timeout_get_raw(stream, true, &duration) == 0 && duration == 17);
    int64_t maximum = jet_int_owned_from_i64(INT64_MAX);
    jet_socket_stream_timeout_set_raw(stream, 3, maximum);
    jet_rt_int_drop(maximum);
    assert(jet_socket_stream_timeout_get_raw(stream, false, &duration) == 1);
    assert(jet_int_owned_to_i64(duration) == INT64_MAX);
    jet_rt_int_drop(duration);
    jet_socket_stream_mark_shutdown_raw(stream, 0);
    assert(jet_socket_stream_flags_raw(stream) == 2);
    jet_socket_stream_mark_closed_raw(stream);
    assert(jet_socket_stream_flags_raw(stream) == 7 && client->closed == 1);
    jet_rt_handle_drop_jet_tcp_stream(stream);

    Listener *listener_holder = jet_socket_listener_from_fd_raw(jet_fd_retain_raw(listener));
    Listener *listener_copy = jet_tcp_listener_clone_raw(listener_holder, &facts);
    assert(listener_copy && listener_copy->fd != listener && listener_copy->fd->number != listener->number);
    jet_rt_handle_drop_jet_tcp_listener(listener_copy);
    jet_rt_handle_drop_jet_tcp_listener(listener_holder);
    Listener *shared_listener = jet_socket_listener_from_fd_raw(jet_socket_create_raw(1, SOCK_STREAM, 0, &facts));
    Listener *shared_copy = jet_rt_handle_clone_jet_unix_listener(shared_listener);
    assert(shared_copy != shared_listener && shared_copy->fd == shared_listener->fd && shared_copy->fd->references == 2);
    jet_rt_handle_drop_jet_unix_listener(shared_listener);
    assert(shared_copy->fd->references == 1);
    jet_rt_handle_drop_jet_unix_listener(shared_copy);

    UDP *socket_holder = jet_udp_socket_from_fd_raw(jet_socket_create_raw(4, SOCK_DGRAM, 0, &facts));
    UDP *socket_copy = jet_rt_handle_clone_jet_udp_socket(socket_holder);
    assert(socket_copy != socket_holder && socket_copy->state == socket_holder->state && socket_copy->state->references == 2);
    assert(jet_udp_socket_fd_raw(socket_copy) == socket_holder->state->fd);
    jet_udp_socket_timeout_set_raw(socket_copy, 101);
    assert(jet_udp_socket_timeout_get_raw(socket_holder, &duration) == 1 && duration == 101);
    jet_rt_int_drop(duration);
    jet_udp_socket_mark_closed_raw(socket_copy);
    assert(socket_holder->state->fd->closed == 1);
    jet_rt_handle_drop_jet_udp_socket(socket_copy);
    assert(socket_holder->state->references == 1);
    jet_rt_handle_drop_jet_udp_socket(socket_holder);
    jet_fd_drop_raw(client);
    jet_fd_drop_raw(listener);
    return 0;
}
