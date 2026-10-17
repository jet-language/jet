// Real Jet-only runtime witness for HostBuildHTTP.jet: loopback HTTP/1.1
// peers, native [[U8]] carriers and the exact HTTP.rs:5536-5712 error texts.
// Mirrors HTTP.rs build_http_transport_tests plus the leaf's other failures.
#define _POSIX_C_SOURCE 200809L
#include <arpa/inet.h>
#include <assert.h>
#include <netinet/in.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>
#ifndef JET_U8_STRIDE
#error "runner must select the runtime's native U8 epoch"
#endif
#if JET_U8_STRIDE == 1
typedef uint8_t Byte;
#elif JET_U8_STRIDE == 8
typedef uint64_t Byte;
#else
#error "unknown native U8 epoch"
#endif
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t length, capacity; Byte *data; } Bytes;
_Static_assert(sizeof(Bytes) == 24, "native List<U8> header");
extern void jet_rt_host_arguments(int64_t, char **, char **);
extern int64_t jet_http_build_resolve(Text *, int64_t, Bytes **, int64_t *, Text **);
extern int64_t jet_http_build_get(Text *, Text *, Text *, bool, Bytes *, int64_t, int64_t, int64_t, int64_t, Bytes **, int64_t *, Text **);

static Text text(const char *value) { return (Text){(unsigned char *)value, (int64_t)strlen(value), 0}; }
static bool same(const Bytes *bytes, const void *expected, size_t length) {
    if (bytes->length != (int64_t)length) return false;
    for (size_t at = 0; at < length; at++) if ((bytes->data[at] & 0xff) != ((const unsigned char *)expected)[at]) return false;
    return length != 0 || bytes->data == NULL;
}
static bool said(const Text *error, const char *expected) {
    return error && error->length == (int64_t)strlen(expected) && !memcmp(error->data, expected, strlen(expected));
}

typedef struct { int listener; const unsigned char *response; size_t length; int delay_ms; char request[4096]; size_t received; } Peer;
static int listen_loopback(int *port) {
    int listener = socket(AF_INET, SOCK_STREAM, 0); assert(listener >= 0);
    struct sockaddr_in address = { .sin_family = AF_INET, .sin_port = 0 };
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    assert(bind(listener, (struct sockaddr *)&address, sizeof address) == 0);
    assert(listen(listener, 4) == 0);
    socklen_t size = sizeof address;
    assert(getsockname(listener, (struct sockaddr *)&address, &size) == 0);
    *port = ntohs(address.sin_port);
    return listener;
}
static void *serve(void *raw) {
    Peer *peer = raw;
    int stream = accept(peer->listener, NULL, NULL); assert(stream >= 0);
    for (;;) {
        ssize_t got = read(stream, peer->request + peer->received, sizeof peer->request - 1 - peer->received);
        if (got <= 0) break;
        peer->received += (size_t)got;
        peer->request[peer->received] = 0;
        if (strstr(peer->request, "\r\n\r\n")) break;
    }
    if (peer->delay_ms) {
        struct timespec pause = { .tv_sec = peer->delay_ms / 1000, .tv_nsec = (long)(peer->delay_ms % 1000) * 1000000L };
        nanosleep(&pause, NULL);
    }
    for (size_t at = 0; at < peer->length;) {
        ssize_t sent = write(stream, peer->response + at, peer->length - at);
        if (sent <= 0) break;
        at += (size_t)sent;
    }
    close(stream);
    return NULL;
}

static unsigned char loopback_octets[] = {127, 0, 0, 1};
static Bytes loopback = {4, 4, NULL};
static int64_t exchange(const void *response, size_t length, int64_t limit, int64_t timeout, bool secure, int delay, Bytes **rows, int64_t *count, Text **error, Peer *peer) {
    int port = 0;
    memset(peer, 0, sizeof *peer);
    peer->listener = listen_loopback(&port);
    peer->response = response; peer->length = length; peer->delay_ms = delay;
    pthread_t thread; assert(pthread_create(&thread, NULL, serve, peer) == 0);
    Text host = text("fixture.invalid"), authority = text("fixture.invalid"), target = text("/locked?x=1");
    *rows = NULL; *count = 0; *error = NULL;
    int64_t tag = jet_http_build_get(&host, &authority, &target, secure, &loopback, 1, port, timeout, limit, rows, count, error);
    assert(pthread_join(thread, NULL) == 0);
    close(peer->listener);
    return tag;
}
static Bytes *wire(const char *response, int64_t limit, int64_t *count) {
    Bytes *rows; Text *error; Peer peer;
    assert(exchange(response, strlen(response), limit, 2000, false, 0, &rows, count, &error, &peer) == 1 && !error);
    assert(!strncmp(peer.request, "GET /locked?x=1 HTTP/1.1\r\n", 26));
    assert(strstr(peer.request, "\r\nHost: fixture.invalid\r\n"));
    assert(strstr(peer.request, "\r\nAccept-Encoding: identity\r\n"));
    assert(strstr(peer.request, "\r\nConnection: close\r\n\r\n"));
    return rows;
}
static void failure(const char *response, int64_t limit, const char *expected) {
    Bytes *rows; int64_t count; Text *error; Peer peer;
    assert(exchange(response, strlen(response), limit, 2000, false, 0, &rows, &count, &error, &peer) == 0);
    assert(said(error, expected));
}

int main(void) {
    char *argv[] = {"build-http-witness", NULL}, *env[] = {NULL};
    jet_rt_host_arguments(1, argv, env);
    Byte octets[4];
    for (int at = 0; at < 4; at++) octets[at] = loopback_octets[at];
    loopback.data = octets;
    int64_t count;

    // Pinned, no-follow: status and raw body bytes are facts, not policy.
    Bytes *rows = wire("HTTP/1.1 302 Found\r\nLocation: http://must-not-resolve.invalid/\r\nContent-Length: 6\r\n\r\nlocked", 1024, &count);
    assert(count == 4 && same(&rows[3], "302", 3) && same(&rows[2], "locked", 6));
    assert(same(&rows[0], "Location: http://must-not-resolve.invalid/", 42) && same(&rows[1], "Content-Length: 6", 17));
    rows = wire("HTTP/1.1 404 Missing\r\nContent-Length: 3\r\n\r\nraw", 1024, &count);
    assert(count == 3 && same(&rows[2], "404", 3) && same(&rows[1], "raw", 3));
    rows = wire("HTTP/1.1 103 Early Hints\r\nLink: </asset>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nraw", 1024, &count);
    assert(count == 3 && same(&rows[2], "200", 3) && same(&rows[1], "raw", 3) && same(&rows[0], "Content-Length: 3", 17));

    // Chunked binary bodies with extensions; the bound returns one excess byte.
    static const unsigned char chunked[] = "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, Chunked\r\n\r\n2\r\n\xff\x00\r\n3;extension=x\r\nabc\r\n0\r\n\r\n";
    Text *error; Peer peer;
    assert(exchange(chunked, sizeof chunked - 1, 512, 2000, false, 0, &rows, &count, &error, &peer) == 1 && !error);
    assert(count == 3 && same(&rows[1], "\xff\x00" "abc", 5) && same(&rows[2], "200", 3));
    static char bounded[600];
    int head = snprintf(bounded, sizeof bounded, "HTTP/1.1 200 OK\r\nContent-Length: 514\r\n\r\n");
    memset(bounded + head, 'x', 514);
    assert(exchange(bounded, (size_t)head + 514, 512, 2000, false, 0, &rows, &count, &error, &peer) == 1 && !error);
    assert(count == 3 && rows[1].length == 513);

    // Malformed Content-Length takes no body; EOF without a length reads all.
    rows = wire("HTTP/1.1 200 OK\r\nContent-Length: +x\r\n\r\nignored", 1024, &count);
    assert(count == 3 && rows[1].length == 0 && rows[1].data == NULL);
    rows = wire("HTTP/1.1 200 OK\r\nServer: eof\r\n\r\nall of it", 1024, &count);
    assert(count == 3 && same(&rows[1], "all of it", 9));

    // Framing errors keep the seed's exact String texts.
    failure("HTTP/1.1 200 OK\r\n", 1024, "unexpected EOF in HTTP response head");
    failure("HTTP/1.1\r\n\r\n", 1024, "HTTP response status is missing");
    failure("HTTP/1.1 200 OK\r\nX: 0123456789\r\n\r\n", 20, "HTTP response head exceeds transport byte bound");
    failure("HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort", 1024, "unexpected EOF in HTTP response body");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n", 1024, "invalid digit found in string");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n", 1024, "cannot parse integer from empty string");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10000000000000000\r\n", 1024, "number too large to fit in target type");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab", 1024, "unexpected EOF in HTTP response chunk");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nabXY", 1024, "invalid HTTP response chunk framing");
    failure("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab", 1024, "failed to fill whole buffer");

    // The transport deadline fires below HTTP framing.
    assert(exchange("", 0, 64, 20, false, 200, &rows, &count, &error, &peer) == 0);
    assert(said(error, "Resource temporarily unavailable (os error 11)") || said(error, "timed out"));

    // TLS is the canonical native refusal after connecting; no plaintext leaves.
    assert(exchange("", 0, 64, 2000, true, 0, &rows, &count, &error, &peer) == 0 && peer.received == 0);
    assert(said(error, "TLS transport is not available in native builds until Jet's own TLS implementation lands (D-NATIVE-TLS1=B); use plain HTTP or TCP meanwhile"));

    // Conversion, carrier and connect failures precede any request.
    Text host = text("fixture.invalid"), target = text("/");
    assert(jet_http_build_get(&host, &host, &target, false, &loopback, 1, 70000, 10, 10, &rows, &count, &error) == 0);
    assert(said(error, "out of range integral type conversion attempted"));
    assert(jet_http_build_get(&host, &host, &target, false, &loopback, 1, 80, -1, 10, &rows, &count, &error) == 0);
    assert(said(error, "out of range integral type conversion attempted"));
    assert(jet_http_build_get(&host, &host, &target, false, &loopback, 0, 80, 10, 10, &rows, &count, &error) == 0);
    assert(said(error, "no resolved addresses"));
    Bytes three = {3, 3, octets};
    assert(jet_http_build_get(&host, &host, &target, false, &three, 1, 80, 10, 10, &rows, &count, &error) == 0);
    assert(said(error, "invalid resolved address carrier"));
    int refused = 0, closed = listen_loopback(&refused);
    close(closed);
    assert(jet_http_build_get(&host, &host, &target, false, &loopback, 1, refused, 2000, 10, &rows, &count, &error) == 0);
    assert(said(error, "Connection refused (os error 111)"));

    // Raw DNS: literal answers in order, one bracket pair removed.
    Text v4 = text("127.0.0.1"), v6 = text("[::1]");
    assert(jet_http_build_resolve(&v4, 80, &rows, &count, &error) == 1 && count == 1 && same(&rows[0], loopback_octets, 4));
    static const unsigned char local6[16] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1};
    assert(jet_http_build_resolve(&v6, 80, &rows, &count, &error) == 1 && count == 1 && same(&rows[0], local6, 16));
    assert(jet_http_build_resolve(&v4, 65536, &rows, &count, &error) == 0);
    assert(said(error, "out of range integral type conversion attempted"));
    puts("build http: ok");
}
