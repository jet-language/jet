// Link once against the complete composed L4/L7/L8 runtime object.
// Address byte-list baseline is word stride8; packed epoch flips this fixture.
#include <assert.h>
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
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern IP *jet_net_raw_ip_from_bytes(uint64_t *, int64_t);
extern int64_t jet_net_raw_ip_bytes(IP *, uint64_t **);
extern Address *jet_net_raw_socket_from_parts(IP *, int64_t, int64_t, int64_t);
extern int64_t jet_net_raw_socket_scope(Address *, int64_t *);
extern Address *jet_rt_handle_clone_JetSocketAddr(Address *);
extern bool jet_rt_handle_eq_JetSocketAddr(Address *, Address *);
extern void jet_rt_handle_drop_JetSocketAddr(Address *);
extern void jet_rt_handle_drop_JetIpAddr(IP *);
extern int64_t jet_socket_address_encode_raw(Address *, void *);
extern bool jet_socket_address_decode_raw(void *, int64_t, Address *);
extern int64_t jet_net_raw_resolve_ordered(Text *, int64_t, Address **, int64_t *, ResolveError **);
extern ResolveError *jet_rt_record_new_Jet__ResolveError(int64_t, int64_t, int64_t, Text *);
extern ResolveError *jet_rt_handle_clone_Jet__ResolveError(ResolveError *);
extern int64_t jet_rt_record_get_Jet__ResolveError_kind(ResolveError *);
extern int64_t jet_rt_record_get_Jet__ResolveError_os_code(ResolveError *);
extern void jet_rt_handle_drop_Jet__ResolveError(ResolveError *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);

int main(void) {
    static char *argv[] = {"net-fixture", NULL}, *env[] = {NULL};
    jet_rt_host_arguments(1, argv, env);
    uint64_t *octets = jet_rt_alloc(16 * 8, 8);
    for (int i = 0; i < 16; ++i) octets[i] = i;
    IP *ip = jet_net_raw_ip_from_bytes(octets, 16);
    jet_rt_free(octets, 16 * 8, 8);
    assert(ip->family == 6);
    for (int i = 0; i < 16; ++i) assert(ip->octets[i] == i);
    uint64_t *copy_bytes = NULL;
    assert(jet_net_raw_ip_bytes(ip, &copy_bytes) == 16);
    for (int i = 0; i < 16; ++i) assert(copy_bytes[i] == (uint64_t)i);
    jet_rt_free(copy_bytes, 16 * 8, 8);
    Address *address = jet_net_raw_socket_from_parts(ip, INT64_C(0x12340000ffff), INT64_C(0x123400000001), INT64_C(0x5678ffffffff));
    jet_rt_handle_drop_JetIpAddr(ip);
    assert(address->port == 65535 && address->flow == 1 && address->scope == UINT32_MAX && address->reserved == 0);
    int64_t scope = 0;
    assert(jet_net_raw_socket_scope(address, &scope) == 1 && scope == UINT32_MAX);
    Address *clone = jet_rt_handle_clone_JetSocketAddr(address);
    assert(clone != address && jet_rt_handle_eq_JetSocketAddr(address, clone));
    unsigned char raw[32] = {0};
    assert(jet_socket_address_encode_raw(address, raw) == sizeof(struct sockaddr_in6));
    struct sockaddr_in6 c_address;
    memcpy(&c_address, raw, sizeof(c_address));
    assert(c_address.sin6_family == AF_INET6 && ntohs(c_address.sin6_port) == 65535);
    assert(ntohl(c_address.sin6_flowinfo) == 1 && c_address.sin6_scope_id == UINT32_MAX);
    Address decoded;
    assert(jet_socket_address_decode_raw(raw, sizeof(c_address), &decoded));
    assert(jet_rt_handle_eq_JetSocketAddr(address, &decoded));
    jet_rt_handle_drop_JetSocketAddr(address);
    jet_rt_handle_drop_JetSocketAddr(clone);

    // Exact-Int children must survive releasing the original spilled owners.
    Text message = {(unsigned char *)"exact resolver", 14, 0};
    int64_t kind = jet_int_owned_from_i64(INT64_MAX), code = jet_int_owned_from_i64(INT64_MIN);
    ResolveError *error = jet_rt_record_new_Jet__ResolveError(kind, 1, code, &message);
    jet_rt_int_drop(kind);
    jet_rt_int_drop(code);
    ResolveError *error_copy = jet_rt_handle_clone_Jet__ResolveError(error);
    jet_rt_handle_drop_Jet__ResolveError(error);
    kind = jet_rt_record_get_Jet__ResolveError_kind(error_copy);
    code = jet_rt_record_get_Jet__ResolveError_os_code(error_copy);
    assert(jet_int_owned_to_i64(kind) == INT64_MAX && jet_int_owned_to_i64(code) == INT64_MIN);
    jet_rt_int_drop(kind);
    jet_rt_int_drop(code);
    jet_rt_handle_drop_Jet__ResolveError(error_copy);

    Text numeric = {(unsigned char *)"127.0.0.1", 9, 0};
    Address *rows = NULL;
    int64_t count = 0;
    error = NULL;
    assert(jet_net_raw_resolve_ordered(&numeric, INT64_C(0x123400000050), &rows, &count, &error) == 1);
    assert(count > 0 && rows && !error);
    for (int64_t i = 0; i < count; ++i) {
        assert(rows[i].ip.family == 4 && rows[i].port == 80);
        assert(rows[i].ip.octets[0] == 127 && rows[i].ip.octets[3] == 1 && rows[i].reserved == 0);
    }
    jet_rt_free(rows, count * 56, 8);
    unsigned char invalid[] = {'a', 0, 'b'};
    Text nul = {invalid, 3, 0};
    error = NULL;
    assert(jet_net_raw_resolve_ordered(&nul, 0, &rows, &count, &error) == 0);
    assert(error && error->kind == 1 && !error->has_os_code);
    const char expected[] = "file name contained an unexpected NUL byte";
    assert(error->message->len == sizeof(expected) - 1 && memcmp(error->message->ptr, expected, sizeof(expected) - 1) == 0);
    jet_rt_handle_drop_Jet__ResolveError(error);
    return 0;
}
