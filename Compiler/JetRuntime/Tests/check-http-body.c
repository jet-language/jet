// L9 ABI witness, linked with the real Jet runtime object and admitted pack.
// Only the session Propagate rail is scripted;
// reader closures use real Linux pipes. This does not implement a TLS adapter.
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef int64_t W;
extern W jet_rt_alloc(W, W);
extern void jet_rt_free(W, W, W);
extern void jet_rt_alloc_init(void);
extern W jet_rt_http_body_from_payload(W, W, W);
extern W jet_rt_http_body_from_reader(W, W, W, W, W, W);
extern W jet_rt_http_body_take(W, W, W);
extern W jet_rt_http_body_take_checked(W, W);
extern W jet_rt_http_body_next(W, W);
extern W jet_rt_http_body_drop(W);
extern W jet_rt_http_body_close(W);
extern W jet_rt_http_body_chunks_drop(W);
extern void jet_rt_http_body_list_drop(W);
extern void jet_rt_http_body_error_drop(W);
extern W jet_rt_http_body_length_known(W);
extern W jet_rt_http_body_length(W);
extern W jet_rt_http_body_source_kind(W);
extern W jet_rt_http_body_source_payload(W);
extern W jet_rt_http_body_is_drained(W);
extern W jet_rt_http_body_is_taken(W);
extern W jet_rt_http_body_content_type(W);
extern void jet_rt_http_tls_refuse(void);

static W delivered, pending;
W __wrap_jet_rt_session_propagate(void) { return delivered; }
static W ptr(void *p) { return (W)(uintptr_t)p; }
static W *words(W p) { return (W *)(uintptr_t)p; }
static W allocate(W bytes) {
    W p = jet_rt_alloc(bytes, 8);
    memset(words(p), 0, (size_t)bytes);
    return p;
}
static W text(const char *s) {
    W h = allocate(24);
    W n = (W)strlen(s);
    W b = n ? jet_rt_alloc(n, 1) : 1;
    if (n) memcpy((void *)(uintptr_t)b, s, (size_t)n);
    words(h)[0] = b; words(h)[1] = n; words(h)[2] = n;
    return h;
}
static W list(const char *s) {
    W n = (W)strlen(s), h = allocate(24);
    W b = n ? allocate(n) : 0;
    if (n) memcpy((void *)(uintptr_t)b, s, (size_t)n);
    words(h)[0] = n; words(h)[1] = n; words(h)[2] = b;
    return h;
}
static W take(W body, W max) {
    W result = -1;
    assert(jet_rt_http_body_take(body, max, ptr(&result)) == 0);
    assert(result != 0);
    return result;
}
static void chunk(W chunks, const char *s) {
    W result = -1;
    assert(jet_rt_http_body_next(chunks, ptr(&result)) == 1);
    assert(words(result)[0] == (W)strlen(s));
    for (W i = 0; i < words(result)[0]; ++i)
        assert(((unsigned char *)(uintptr_t)words(result)[2])[i] == (unsigned char)s[i]);
    jet_rt_http_body_list_drop(result);
}
static void eof(W chunks) {
    W result = -1;
    assert(jet_rt_http_body_next(chunks, ptr(&result)) == 0);
    assert(result == 0);
}
static void payloads(void) {
    for (W kind = 0; kind < 3; ++kind) {
        const char *s = kind == 0 ? "" : "abcde";
        W payload = kind == 1 ? text(s) : list(s);
        W mime = text("text/plain"), body = jet_rt_http_body_from_payload(payload, kind, mime);
        assert(jet_rt_http_body_source_payload(body) == payload); // move, no buffer copy
        assert(jet_rt_http_body_content_type(body) == mime);
        assert(jet_rt_http_body_source_kind(body) == kind);
        assert(jet_rt_http_body_length_known(body) == 1);
        assert(jet_rt_http_body_length(body) == (W)strlen(s));
        assert(jet_rt_http_body_is_drained(body) == (kind == 0));
        assert(jet_rt_http_body_is_taken(body) == 0);
        W chunks = take(body, 2), second = -1;
        assert(jet_rt_http_body_take(body, 2, ptr(&second)) == -1 && second == 0);
        assert(jet_rt_http_body_is_taken(body) == 1);
        if (kind != 0) { chunk(chunks, "ab"); chunk(chunks, "cd"); chunk(chunks, "e"); }
        eof(chunks); eof(chunks);
        assert(jet_rt_http_body_is_drained(body) == 1);
        assert(jet_rt_http_body_source_payload(body) == payload);
        assert(jet_rt_http_body_chunks_drop(chunks) == 0);
        assert(jet_rt_http_body_drop(body) == 0);
        // Partial consumption and dropped original owner retain the payload
        // until chunks drops; source was taken and is never taken a second time.
        body = jet_rt_http_body_from_payload(kind == 1 ? text(s) : list(s), kind, 0);
        chunks = take(body, 2);
        if (kind != 0) chunk(chunks, "ab");
        assert(jet_rt_http_body_drop(body) == 0);
        assert(jet_rt_http_body_chunks_drop(chunks) == 0);
        // No take: dropping still owns and frees the moved payload.
        body = jet_rt_http_body_from_payload(kind == 1 ? text(s) : list(s), kind, 0);
        assert(jet_rt_http_body_drop(body) == 0);
    }
}

static void checked_take(void) {
    W body = jet_rt_http_body_from_payload(list("abcde"), 2, 0);
    assert(jet_rt_http_body_is_taken(body) == 0);
    W chunks = jet_rt_http_body_take_checked(body, 2);
    assert(chunks != 0 && jet_rt_http_body_is_taken(body) == 1);
    assert(words(body)[2] == 0); // reserved16 is never protocol scratch
    chunk(chunks, "ab"); chunk(chunks, "cd"); chunk(chunks, "e"); eof(chunks);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
}

// Same canonical24B closure header + captures as native Lower/Closures.jet.
typedef struct Closure {
    W refs;
    W code;
    void (*release)(W);
    int fd;
    W mode, tag, value;
} Closure;
static W closed_count, released_count;
static void release(W p) {
    ++released_count;
    jet_rt_free(p, sizeof(Closure), 8);
}
static W close_source(W p) {
    Closure *c = (Closure *)(uintptr_t)p;
    assert(close(c->fd) == 0);
    ++closed_count;
    if (c->mode == 2) delivered = 3;
    return 0;
}
static W read_source(W p, W descriptor) {
    Closure *c = (Closure *)(uintptr_t)p;
    W *d = words(descriptor);
    if (c->mode) {
        W detail = allocate(32);
        words(detail)[0] = c->tag;
        words(detail)[1] = c->value;
        words(detail)[2] = text("bridge-detail");
        d[3] = detail;
        if (c->mode == 2) delivered = c->value;
        if (c->mode == 3) pending = 1; // requested, not delivered: must not stop
        if (c->mode != 3) return -6;
        jet_rt_http_body_error_drop(detail);
        d[3] = 0;
    }
    ssize_t n;
    do { n = read(c->fd, (void *)(uintptr_t)d[0], (size_t)d[1]); } while (n < 0 && errno == EINTR);
    assert(n >= 0);
    d[2] = n;
    return 0;
}
static W closure(int fd, W mode, W tag, W value, int closer) {
    W p = allocate(sizeof(Closure));
    Closure *c = (Closure *)(uintptr_t)p;
    c->refs = 1;
    // Store the raw code word, exactly as the native closure header does.
    c->code = closer ? (W)(uintptr_t)close_source : (W)(uintptr_t)read_source;
    c->release = release; c->fd = fd; c->mode = mode; c->tag = tag; c->value = value;
    return p;
}
static W reader(W mode, W tag, W value, W known, W length, W bounded) {
    int fds[2];
    assert(pipe(fds) == 0);
    assert(write(fds[1], "abcde", 5) == 5);
    assert(close(fds[1]) == 0);
    return jet_rt_http_body_from_reader(closure(fds[0], mode, tag, value, 0),
        closure(fds[0], 0, 0, 0, 1), known, length, bounded, 0);
}
static void streams(void) {
    W before = closed_count, releases = released_count;
    W body = reader(0, 0, 0, 0, 0, 0), chunks = take(body, 2);
    chunk(chunks, "ab");
    assert(jet_rt_http_body_is_drained(body) == 0);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(closed_count == before + 1 && released_count == releases);
    assert(jet_rt_http_body_close(body) == 0); // close after chunks/drop, once
    assert(jet_rt_http_body_drop(body) == 0);
    assert(closed_count == before + 1 && released_count == releases + 2);
    body = reader(0, 0, 0, 1, 3, 1); chunks = take(body, 2);
    chunk(chunks, "ab"); chunk(chunks, "c"); eof(chunks);
    assert(jet_rt_http_body_is_drained(body) == 1);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
    // Reader length is advisory, unlike File Take. All5bytes are consumed.
    body = reader(0, 0, 0, 1, 3, 0); chunks = take(body, 2);
    chunk(chunks, "ab"); chunk(chunks, "cd"); chunk(chunks, "e"); eof(chunks);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
    before = closed_count;
    body = reader(0, 0, 0, 0, 0, 0);
    assert(jet_rt_http_body_close(body) == 0);
    assert(jet_rt_http_body_close(body) == 0);
    W result = -1;
    assert(jet_rt_http_body_take(body, 2, ptr(&result)) == -1 && result == 0);
    assert(jet_rt_http_body_drop(body) == 0 && closed_count == before + 1);
    body = reader(0, 0, 0, 0, 0, 0);
    assert(jet_rt_http_body_drop(body) == 0 && closed_count == before + 2);
}
static void errors_and_stops(void) {
    // Runtime does not inspect tags: every source HTTPError tag/payload passes
    // byte-for-byte. Core owns the variant mapping and text synthesis.
    for (W tag = 0; tag <= 20; ++tag) {
        W body = reader(1, tag, 32768, 0, 0, 0), chunks = take(body, 2), result = -1;
        assert(jet_rt_http_body_next(chunks, ptr(&result)) == -6);
        assert(words(result)[0] == tag && words(result)[1] == 32768);
        W t = words(result)[2];
        assert(words(t)[1] == 13 && memcmp((void *)(uintptr_t)words(t)[0], "bridge-detail", 13) == 0);
        jet_rt_http_body_error_drop(result);
        assert(jet_rt_http_body_is_drained(body) == 0);
        eof(chunks);
        assert(jet_rt_http_body_chunks_drop(chunks) == 0);
        assert(jet_rt_http_body_drop(body) == 0);
    }
    for (W stop = 1; stop <= 5; ++stop) {
        W body = reader(2, 7, stop, 0, 0, 0), chunks = take(body, 2), result = -1;
        assert(jet_rt_http_body_next(chunks, ptr(&result)) == -5 && result == 0);
        assert(delivered == stop && jet_rt_http_body_is_drained(body) == 0);
        assert(jet_rt_http_body_chunks_drop(chunks) == -5);
        assert(jet_rt_http_body_drop(body) == -5);
        delivered = 0;
    }
    W before = released_count;
    W close_body = reader(0, 0, 0, 0, 0, 0);
    Closure *closer = (Closure *)(uintptr_t)words(close_body)[5];
    closer->mode = 2;
    assert(jet_rt_http_body_close(close_body) == -5 && delivered == 3);
    assert(jet_rt_http_body_is_taken(close_body) == 1);
    assert(jet_rt_http_body_drop(close_body) == -5);
    assert(released_count == before + 2); // stop never skips release cleanup
    delivered = 0;
    W body = reader(3, 7, 1, 0, 0, 0), chunks = take(body, 2);
    chunk(chunks, "ab"); assert(pending == 1 && delivered == 0);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
    pending = 0;
}
// Same 1GiB bounded File Take as the Rust reference, with packed native U8
// output. Consume every byte so neither side can elide the body transfer.
static void stream_perf(const char *path) {
    int fd = open(path, O_RDONLY);
    assert(fd >= 0);
    struct stat info;
    assert(fstat(fd, &info) == 0 && info.st_size == INT64_C(1073741824));
    W read_closure = closure(fd, 0, 0, 0, 0), close_closure = closure(fd, 0, 0, 0, 1);
    W body = jet_rt_http_body_from_reader(read_closure, close_closure, 1, info.st_size, 1, 0);
    W chunks = take(body, 65536), output = 0, bytes = 0, checksum = 0, status;
    while ((status = jet_rt_http_body_next(chunks, ptr(&output))) == 1) {
        W count = words(output)[0];
        unsigned char *data = (unsigned char *)(uintptr_t)words(output)[2];
        bytes += count;
        for (W at = 0; at < count; ++at) checksum += data[at];
        jet_rt_http_body_list_drop(output);
        output = 0;
    }
    assert(status == 0 && output == 0 && bytes == info.st_size);
    assert(jet_rt_http_body_is_drained(body) == 1);
    assert(jet_rt_http_body_chunks_drop(chunks) == 0);
    assert(jet_rt_http_body_drop(body) == 0);
    printf("bytes=%lld checksum=%lld\n", (long long)bytes, (long long)checksum);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init();
    if (argc == 2 && strcmp(argv[1], "tls") == 0) { jet_rt_http_tls_refuse(); return 99; }
    if (argc == 3 && strcmp(argv[1], "perf") == 0) { stream_perf(argv[2]); return 0; }
    if (argc == 2 && strcmp(argv[1], "taken") == 0) {
        W body = jet_rt_http_body_from_payload(list("x"), 2, 0);
        assert(jet_rt_http_body_take_checked(body, 1) != 0);
        (void)jet_rt_http_body_take_checked(body, 1);
        return 99; // only a broken invariant adapter can return here
    }
    payloads(); checked_take(); streams(); errors_and_stops();
    puts("http body: ok");
    return 0;
}
