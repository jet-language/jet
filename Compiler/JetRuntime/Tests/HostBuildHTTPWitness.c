// Real Jet-only runtime witness for HostBuildHTTP.jet: the checked-build DNS
// answer as native [[U8]] carriers with the exact HTTP.rs:5536-5712 error text.
// The pinned GET is Jet source; tests/cases/net/build_http_transport covers it.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
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

static Text text(const char *value) { return (Text){(unsigned char *)value, (int64_t)strlen(value), 0}; }
static bool same(const Bytes *bytes, const void *expected, size_t length) {
    if (bytes->length != (int64_t)length) return false;
    for (size_t at = 0; at < length; at++) if ((bytes->data[at] & 0xff) != ((const unsigned char *)expected)[at]) return false;
    return length != 0 || bytes->data == NULL;
}
static bool said(const Text *error, const char *expected) {
    return error && error->length == (int64_t)strlen(expected) && !memcmp(error->data, expected, strlen(expected));
}

int main(void) {
    char *argv[] = {"build-http-witness", NULL}, *env[] = {NULL};
    jet_rt_host_arguments(1, argv, env);
    Bytes *rows;
    int64_t count;
    Text *error;

    // Raw DNS: literal answers in order, one bracket pair removed.
    Text v4 = text("127.0.0.1"), v6 = text("[::1]");
    static const unsigned char local4[4] = {127, 0, 0, 1};
    assert(jet_http_build_resolve(&v4, 80, &rows, &count, &error) == 1 && count == 1 && same(&rows[0], local4, 4));
    static const unsigned char local6[16] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1};
    assert(jet_http_build_resolve(&v6, 80, &rows, &count, &error) == 1 && count == 1 && same(&rows[0], local6, 16));
    assert(jet_http_build_resolve(&v4, 65536, &rows, &count, &error) == 0);
    assert(said(error, "out of range integral type conversion attempted"));
    puts("build http: ok");
}
